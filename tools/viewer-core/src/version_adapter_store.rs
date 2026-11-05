//! Canonical offline store. Original adapter identities name whole-file hashes;
//! v2 uses ordered shared pieces; v3 groups small pieces into bounded random-access entries.
use super::{normalized, read_json, valid_id, ResourceRecord};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell, collections::{BTreeMap, BTreeSet, VecDeque}, fs::{self, File},
    io::{BufReader, Read}, path::{Path, PathBuf}, sync::Arc, time::SystemTime,
};

const LIMIT: u64 = 512 * 1024 * 1024;
const FORMAT: &str = "nova.version-adapter-store";
const INDEX: &str = "index.json";
const ARCHIVE: &str = "shared.zip";
const RECIPE_LIMIT: u64 = 16 * 1024 * 1024;
const PIECE_LIMIT: usize = 131_072;
const PACK_BYTES: u64 = 256 * 1024;

#[derive(Deserialize)]
struct Index {
    schema_version: u32,
    format: String,
    versions: BTreeMap<String, String>,
    #[serde(default)]
    recipes: BTreeMap<String, String>,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    locators: BTreeMap<String, String>,
}
impl Index {
    fn check(&self) -> Result<()> {
        ensure!(matches!(self.schema_version, 1..=3) && self.format == FORMAT && !self.versions.is_empty()
            && (self.schema_version >= 2 || self.recipes.is_empty())
            && (self.schema_version == 3 || (self.groups.is_empty() && self.locators.is_empty())),
            "invalid version adapter store schema");
        ensure!(self.versions.iter().all(|(v, sha)| valid_id(v) && valid_hash(sha))
            && self.recipes.iter().all(|(whole, recipe)| valid_hash(whole) && valid_hash(recipe))
            && self.groups.iter().all(|sha| valid_hash(sha))
            && self.locators.iter().all(|(prefix, sha)| prefix.len() == 2
                && prefix.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) && valid_hash(sha)),
            "invalid version adapter store mapping");
        Ok(())
    }
}
#[derive(Deserialize)]
struct Recipe { chunks: Vec<ResourceRecord> }

// Some logical files interleave hundreds of copies of the same 200 KiB array
// with unique bytes. Keep only repeated pieces, prioritizing avoided inflation.
fn hot_pieces(pieces: &[ResourceRecord]) -> BTreeSet<&str> {
    let mut counts = BTreeMap::new();
    for piece in pieces {
        let entry = counts.entry(piece.sha256.as_str()).or_insert((0usize, piece.bytes));
        entry.0 += 1;
    }
    let mut repeated: Vec<_> = counts.into_iter().filter(|(_, (n, _))| *n > 1).collect();
    repeated.sort_unstable_by_key(|(_, (n, bytes))| std::cmp::Reverse((*n - 1) as u64 * bytes));
    let mut budget = 4 * 1024 * 1024u64;
    let mut hot = BTreeSet::new();
    for (sha, (_, bytes)) in repeated {
        if bytes <= budget {
            hot.insert(sha);
            budget -= bytes;
            if hot.len() == 16 { break; }
        }
    }
    hot
}

#[derive(Deserialize)]
struct Identity {
    schema_version: u32,
    game_version: String,
    content_id: String,
    files: BTreeMap<String, ResourceRecord>,
}

pub(super) struct Version {
    id: String,
    pub(super) identity: Vec<u8>,
    pub(super) files: BTreeMap<String, ResourceRecord>,
}

pub(super) struct Store {
    index: Index,
    zip: zip::ZipArchive<BufReader<File>>,
    versions: BTreeMap<String, Arc<Version>>,
    small: VecDeque<(String, Arc<Vec<u8>>)>,
    shards: BTreeMap<String, BTreeMap<String, [u32; 3]>>,
    packs: VecDeque<(u32, Vec<u8>)>,
}
fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn valid_hash(s: &str) -> bool { s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) }

impl Store {
    pub(super) fn present(dir: &Path) -> bool { dir.join(INDEX).exists() || dir.join(ARCHIVE).exists() }

    pub(super) fn open(dir: &Path) -> Result<Self> {
        ensure!(fs::metadata(dir.join(INDEX)).context("missing version adapter store index")?.len() <= 4 * 1024 * 1024,
            "version adapter store index too large");
        let index: Index = read_json(&dir.join(INDEX))?;
        index.check()?;
        let zip = zip::ZipArchive::new(BufReader::new(File::open(dir.join(ARCHIVE)).context("missing version adapter store shared.zip")?))?;
        ensure!(zip.len() <= if index.schema_version == 1 { 100_000 } else { 1_000_000 }, "version adapter store has too many entries");
        Ok(Self { index, zip, versions: BTreeMap::new(), small: VecDeque::new(),
            shards: BTreeMap::new(), packs: VecDeque::new() })
    }

    fn read_entry(&mut self, name: &str, max: u64) -> Result<Vec<u8>> {
        let entry = self.zip.by_name(name)?;
        ensure!(entry.size() <= max && !entry.is_dir() && entry.unix_mode().is_none_or(|mode| mode & 0o170000 != 0o120000),
            "invalid version adapter entry {name}");
        let size = usize::try_from(entry.size())?;
        let mut bytes = Vec::with_capacity(size);
        entry.take(size as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() == size, "invalid length of version adapter entry {name}");
        Ok(bytes)
    }

    pub(super) fn select(&mut self, version: &str) -> Result<Option<Arc<Version>>> {
        if let Some(v) = self.versions.get(version) { return Ok(Some(v.clone())); }
        let Some(expected) = self.index.versions.get(version).cloned() else { return Ok(None); };
        let bytes = self.read_entry(&format!("versions/{version}.json"), 1024 * 1024)
            .with_context(|| format!("missing or damaged version identity: {version}/adapter.json"))?;
        ensure!(digest(&bytes) == expected, "version identity digest mismatch: {version}/adapter.json");
        let id: Identity = serde_json::from_slice(&bytes)?;
        ensure!(id.schema_version == 1 && id.game_version == version, "wrong exact version: {version}/adapter.json");
        ensure!(id.files.contains_key("inventory.json") && id.files.contains_key("oracle/oracle-manifest.json"), "incomplete adapter: {version}");
        let mut content = Sha256::new();
        let mut total = 0u64;
        for (name, record) in &id.files {
            ensure!(normalized(name)? == *name && !name.contains(':') && !name.split('/').any(|part| matches!(part, "" | "." | ".."))
                && name != "adapter.json" && valid_hash(&record.sha256), "invalid version adapter mapping: {version}/{name}");
            // Only look up headers here. Reading inventory must not inflate recipes
            // or chunks for unrelated oracle tables in the same Minecraft version.
            if let Some(sha) = self.index.recipes.get(&record.sha256) {
                let recipe = self.zip.by_name(&format!("recipes/{sha}")).with_context(|| format!("missing adapter recipe: {version}/{name}"))?;
                ensure!(recipe.size() <= RECIPE_LIMIT, "oversized adapter recipe: {version}/{name}");
            } else {
                self.check_blob(record).with_context(|| format!("missing or invalid adapter blob: {version}/{name}"))?;
            }
            total = total.checked_add(record.bytes).context("version adapter size overflow")?;
            content.update(format!("{name}\0{}\0{}\n", record.sha256, record.bytes));
        }
        ensure!(total <= LIMIT && format!("{:x}", content.finalize()) == id.content_id, "invalid adapter content identity: {version}");
        let value = Arc::new(Version { id: version.into(), identity: bytes, files: id.files });
        self.versions.insert(version.into(), value.clone());
        Ok(Some(value))
    }

    // Only the selected hash-prefix directory is inflated; checking identity headers
    // does not decompress the other versions or any unrelated group payload.
    fn placement(&mut self, record: &ResourceRecord) -> Result<Option<[u32; 3]>> {
        if self.index.schema_version != 3 || record.bytes == 0 || record.bytes > PACK_BYTES { return Ok(None); }
        let prefix = &record.sha256[..2];
        if !self.shards.contains_key(prefix) {
            let expected = self.index.locators.get(prefix).context("missing adapter locator directory")?.clone();
            let raw = self.read_entry(&format!("locators/{prefix}"), PACK_BYTES)?;
            ensure!(digest(&raw) == expected, "adapter locator digest mismatch");
            let entries: BTreeMap<String, [u32; 3]> = serde_json::from_slice(&raw)?;
            ensure!(entries.iter().all(|(key, &[group, offset, len])| valid_hash(key) && key.starts_with(prefix)
                && (group as usize) < self.index.groups.len() && len != 0
                && u64::from(offset) + u64::from(len) <= PACK_BYTES), "invalid adapter locator");
            self.shards.insert(prefix.into(), entries);
        }
        let place = self.shards[prefix].get(&record.sha256).context("missing adapter chunk location")?;
        ensure!(u64::from(place[2]) == record.bytes, "adapter locator length mismatch");
        Ok(Some(*place))
    }

    fn group_slice(&mut self, [group, offset, len]: [u32; 3]) -> Result<&[u8]> {
        let slot = if let Some(i) = self.packs.iter().position(|p| p.0 == group) { i } else {
            let bytes = self.read_entry(&format!("groups/{group}"), PACK_BYTES)?;
            ensure!(digest(&bytes) == self.index.groups[group as usize], "adapter group digest mismatch");
            if self.packs.len() == 4 { self.packs.pop_front(); }
            self.packs.push_back((group, bytes));
            self.packs.len() - 1
        };
        self.packs[slot].1.get(offset as usize..(offset + len) as usize).context("adapter chunk exceeds group")
    }

    fn check_blob(&mut self, record: &ResourceRecord) -> Result<String> {
        let place = self.placement(record)?;
        let name = place.map_or_else(|| format!("blobs/{}", record.sha256), |p| format!("groups/{}", p[0]));
        let blob = self.zip.by_name(&name)?;
        let valid_size = place.map_or(blob.size() == record.bytes, |[_, off, len]|
            u64::from(off) + u64::from(len) <= blob.size() && blob.size() <= PACK_BYTES);
        ensure!(valid_size && !blob.is_dir()
            && blob.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000), "invalid adapter blob: {}", record.sha256);
        Ok(name)
    }

    fn recipe(&mut self, record: &ResourceRecord) -> Result<Option<Recipe>> {
        let Some(sha) = self.index.recipes.get(&record.sha256).cloned() else { return Ok(None); };
        let raw = self.read_entry(&format!("recipes/{sha}"), RECIPE_LIMIT)?;
        ensure!(digest(&raw) == sha, "adapter recipe digest mismatch: {sha}");
        let recipe: Recipe = serde_json::from_slice(&raw)?;
        ensure!(!recipe.chunks.is_empty() && recipe.chunks.len() <= PIECE_LIMIT, "invalid adapter recipe count: {sha}");
        let mut total = 0u64;
        for chunk in &recipe.chunks {
            ensure!(valid_hash(&chunk.sha256) && chunk.bytes > 0, "invalid adapter chunk: {sha}");
            total = total.checked_add(chunk.bytes).context("adapter recipe length overflow")?;
        }
        ensure!(total == record.bytes, "wrong adapter recipe length: {sha}");
        Ok(Some(recipe))
    }

    // Use one open archive cursor and one fixed-size buffer, even for the largest
    // table. SHA-256 validates both the physical payloads and their exact ordering.
    fn stream(&mut self, record: &ResourceRecord, mut visit: impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
        let recipe = self.recipe(record)?;
        let pieces = recipe.as_ref().map_or(std::slice::from_ref(record), |r| r.chunks.as_slice());
        let hot = if pieces.len() > 1 { hot_pieces(pieces) } else { BTreeSet::new() };
        let mut cache: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
        let mut whole = Sha256::new();
        let mut buf = [0; 64 * 1024];
        for piece in pieces {
            if let Some(data) = cache.get(piece.sha256.as_str()) {
                ensure!(data.len() as u64 == piece.bytes, "conflicting adapter chunk lengths: {}", piece.sha256);
                whole.update(data);
                visit(data)?;
                continue;
            }
            if let Some(place) = self.placement(piece)? {
                let data = self.group_slice(place)?;
                ensure!(digest(data) == piece.sha256, "adapter blob content mismatch: {}", piece.sha256);
                if recipe.is_some() { whole.update(data); }
                visit(data)?;
                if hot.contains(piece.sha256.as_str()) { cache.insert(piece.sha256.as_str(), data.to_vec()); }
                continue;
            }
            let mut blob = self.zip.by_name(&format!("blobs/{}", piece.sha256))?;
            ensure!(blob.size() == piece.bytes && !blob.is_dir()
                && blob.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000), "invalid adapter blob: {}", piece.sha256);
            let mut physical = Sha256::new();
            let mut remaining = piece.bytes;
            let mut saved = hot.contains(piece.sha256.as_str()).then(|| Vec::with_capacity(piece.bytes as usize));
            while remaining != 0 {
                let n = remaining.min(buf.len() as u64) as usize;
                let n = blob.read(&mut buf[..n])?;
                ensure!(n != 0, "truncated adapter blob: {}", piece.sha256);
                physical.update(&buf[..n]);
                if recipe.is_some() { whole.update(&buf[..n]); }
                if let Some(data) = &mut saved { data.extend_from_slice(&buf[..n]); }
                visit(&buf[..n])?;
                remaining -= n as u64;
            }
            ensure!(blob.read(&mut buf[..1])? == 0 && format!("{:x}", physical.finalize()) == piece.sha256,
                "adapter blob content mismatch: {}", piece.sha256);
            if let Some(data) = saved { cache.insert(piece.sha256.as_str(), data); }
        }
        ensure!(recipe.is_none() || format!("{:x}", whole.finalize()) == record.sha256,
            "reconstructed adapter file digest mismatch: {}", record.sha256);
        Ok(())
    }

    pub(super) fn read(&mut self, version: &Version, name: &str) -> Result<Arc<Vec<u8>>> {
        let record = version.files.get(name).with_context(|| format!("missing adapter mapping: {}/{name}", version.id))?;
        if let Some((_, cached)) = self.small.iter().find(|(sha, _)| sha == &record.sha256) {
            ensure!(cached.len() as u64 == record.bytes, "invalid blob length: {}/{name}", version.id);
            return Ok(cached.clone());
        }
        let mut bytes = Vec::with_capacity(usize::try_from(record.bytes)?);
        self.stream(record, |part| { bytes.extend_from_slice(part); Ok(()) })
            .with_context(|| format!("missing or damaged adapter payload: {}/{name} ({})", version.id, record.sha256))?;
        let bytes = Arc::new(bytes);
        // A worker can retain at most 8 MiB of hot small entries. Large oracle
        // tables live only as long as their consumer; the store never expands to disk.
        if bytes.len() <= 512 * 1024 {
            if self.small.len() == 16 { self.small.pop_front(); }
            self.small.push_back((record.sha256.clone(), bytes.clone()));
        }
        Ok(bytes)
    }

    // Packaging checks directory references without inflating all oracle versions.
    pub(super) fn check_layout(&mut self) -> Result<()> {
        let mut expected = BTreeSet::new();
        let mut files = BTreeMap::new();
        let mut grouped = BTreeSet::new();
        for v in self.index.versions.keys().cloned().collect::<Vec<_>>() {
            let id = self.select(&v)?.context("missing adapter identity")?;
            expected.insert(format!("versions/{v}.json"));
            for r in id.files.values() {
                if let Some(old) = files.insert(r.sha256.clone(), r.bytes) {
                    ensure!(old == r.bytes, "conflicting adapter file lengths: {}", r.sha256);
                }
            }
        }
        ensure!(self.index.recipes.keys().all(|sha| files.contains_key(sha)), "unmapped recipe in adapter index");
        for (sha, bytes) in files {
            let record = ResourceRecord { sha256: sha, bytes };
            let recipe = self.recipe(&record)?;
            if recipe.is_some() {
                expected.insert(format!("recipes/{}", self.index.recipes[&record.sha256]));
            }
            for piece in recipe.as_ref().map_or(std::slice::from_ref(&record), |r| r.chunks.as_slice()) {
                let name = self.check_blob(piece)?;
                if name.starts_with("groups/") {
                    expected.insert(format!("locators/{}", &piece.sha256[..2]));
                    grouped.insert(piece.sha256.clone());
                }
                expected.insert(name);
            }
        }
        ensure!(self.index.locators.len() == self.shards.len()
            && grouped.len() == self.shards.values().map(BTreeMap::len).sum::<usize>()
            && (0..self.index.groups.len()).all(|g| expected.contains(&format!("groups/{g}"))),
            "unmapped adapter group or locator");
        ensure!(self.zip.file_names().all(|name| expected.contains(name)), "unmapped entry in shared adapter store");
        Ok(())
    }
}

#[derive(PartialEq)]
struct Stamp { directory: PathBuf, index: (u64, SystemTime), archive: (u64, SystemTime) }
fn stat(path: PathBuf) -> Result<(u64, SystemTime)> { let m = fs::metadata(path)?; Ok((m.len(), m.modified()?)) }

// Independent workers own independent archive cursors. No seek/decode while
// holding a process-wide lock, no repeated central-directory scans on hot reads.
thread_local! { static CURRENT: RefCell<Option<(Stamp, Store)>> = const { RefCell::new(None) }; }
fn with_store<T>(dir: &Path, run: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    let stamp = Stamp { directory: dir.to_owned(), index: stat(dir.join(INDEX))?, archive: stat(dir.join(ARCHIVE))? };
    CURRENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if !slot.as_ref().is_some_and(|(key, _)| *key == stamp) { *slot = Some((stamp, Store::open(dir)?)); }
        run(&mut slot.as_mut().unwrap().1)
    })
}
pub(super) const INHERITED_INDEX: &str = "derived/version-adapters-index.json";
pub(super) fn read_selected(dir: &Path, version: &str, name: &str, pinned: Option<&Path>) -> Result<Option<Vec<u8>>> {
    with_store(dir, |store| {
        if let Some(path) = pinned {
            ensure!(fs::metadata(path)?.len() <= 4 * 1024 * 1024, "inherited version index too large");
            let inherited: Index = read_json(path)?;
            inherited.check().context("invalid inherited adapter index")?;
            let Some(expected) = inherited.versions.get(version) else { return Ok(None); };
            ensure!(store.index.versions.get(version) == Some(expected), "inherited adapter identity mismatch: {version}");
        }
        let Some(id) = store.select(version)? else { return Ok(None); };
        Ok(Some(Arc::unwrap_or_clone(store.read(&id, name)?)))
    }).with_context(|| format!("reading version adapter {version}/{name}"))
}

pub(super) const REFERENCE: &str = "derived/oracle/.nova-store.json";
#[derive(serde::Serialize, Deserialize)]
pub(super) struct Reference { schema_version: u32, game_version: String, identity_sha256: String }
impl Reference {
    pub(super) fn for_version(v: &Version) -> Self {
        Self { schema_version: 1, game_version: v.id.clone(), identity_sha256: digest(&v.identity) }
    }
}
pub(super) fn virtual_oracle(relative: &str) -> bool {
    relative.strip_prefix("derived/oracle/").is_some_and(|s| s.ends_with(".jsonl") && valid_id(s))
}
// Metadata and byte reads must resolve the same exact adapter, never a default
// version substitute. References contain no absolute path or user-supplied target.
fn referenced<T>(path: &Path, default: Option<&Path>, run: impl FnOnce(&mut Store, &Version, &str) -> Result<T>) -> Result<Option<T>> {
    let Some(file) = path.file_name().and_then(|v| v.to_str()) else { return Ok(None); };
    let Some(oracle) = path.parent().filter(|p| p.ends_with("derived/oracle")) else { return Ok(None); };
    if !file.ends_with(".jsonl") || !valid_id(file) { return Ok(None); }
    let marker = oracle.join(".nova-store.json");
    if !marker.try_exists()? { return Ok(None); }
    let reference: Reference = read_json(&marker)?;
    ensure!(reference.schema_version == 1 && valid_id(&reference.game_version) && valid_hash(&reference.identity_sha256), "invalid shared oracle reference");
    // Pin both ends: replacing the marker with a *valid other version's* pin
    // must not change the oracle interpreted by this particular package.
    let adapter_path = oracle.parent().context("missing derived directory")?.join("adapter.json");
    ensure!(fs::metadata(&adapter_path)?.len() <= 1024 * 1024
        && digest(&fs::read(adapter_path)?) == reference.identity_sha256,
        "oracle reference differs from this package's adapter: {}/{file}", reference.game_version);
    let default = match default { Some(p) => p.to_owned(), None => super::default_root()? };
    let name = format!("oracle/{file}");
    with_store(&default.join("derived/version-adapters"), |store| {
        ensure!(store.index.versions.get(&reference.game_version) == Some(&reference.identity_sha256),
            "installed adapter no longer matches imported Minecraft {} identity", reference.game_version);
        let id = store.select(&reference.game_version)?.context("missing exact adapter")?;
        if !id.files.contains_key(&name) { return Ok(None); } // Optional artifact not exported in this release.
        run(store, &id, &name).map(Some)
    }).with_context(|| format!("reading referenced oracle {}/{name}", reference.game_version))
}

pub(super) fn referenced_bytes(path: &Path, default: Option<&Path>) -> Result<Option<Vec<u8>>> {
    referenced(path, default, |store, id, name| Ok(Arc::unwrap_or_clone(store.read(id, name)?)))
}
pub(super) fn referenced_len(path: &Path) -> Result<Option<u64>> {
    referenced(path, None, |store, id, name| {
        let r = &id.files[name];
        if let Some(recipe) = store.recipe(r)? {
            for chunk in &recipe.chunks { store.check_blob(chunk)?; }
        } else { store.check_blob(r)?; }
        Ok(r.bytes)
    })
}

/// Compare the real reader against each byte of independent original adapters;
/// no generated snapshots or all-version disk extraction.
pub(super) fn verify(default: &Path, originals: &Path) -> Result<serde_json::Value> {
    let mut store = Store::open(&default.join("derived/version-adapters"))?;
    store.check_layout()?;
    let mut versions = BTreeSet::new();
    for item in fs::read_dir(originals)? {
        let item = item?;
        let path = item.path();
        if item.file_type()?.is_dir() {
            versions.insert(item.file_name().into_string().map_err(|_| anyhow::anyhow!("invalid version"))?);
        } else if path.extension().and_then(|e| e.to_str()) == Some("zip") {
            versions.insert(path.file_stem().and_then(|v|v.to_str()).context("invalid input name")?.to_owned());
        }
    }
    ensure!(versions.iter().eq(store.index.versions.keys()), "supported versions differ between store and original inputs");
    let mut entries = 0usize;
    let mut bytes = 0u64;
    let mut blobs = BTreeSet::new();
    for v in &versions {
        let id = store.select(v)?.context("missing exact version")?;
        let file = originals.join(format!("{v}.zip"));
        let dir = originals.join(v);
        let mut zip = file.is_file().then(|| -> Result<_> { Ok(zip::ZipArchive::new(BufReader::new(File::open(file)?))?) }).transpose()?;
        if let Some(z) = &zip {
            ensure!(z.file_names().filter(|n| !n.ends_with('/')).count() == id.files.len() + 1, "original entry set differs: {v}");
        }
        for name in std::iter::once("adapter.json").chain(id.files.keys().map(String::as_str)) {
            let mut source: Box<dyn Read + '_> = if let Some(z) = &mut zip { Box::new(z.by_name(name)?) } else { Box::new(File::open(dir.join(name))?) };
            let mut chunk = [0u8; 64 * 1024];
            let mut compare = |data: &[u8]| -> Result<()> {
                for expected in data.chunks(chunk.len()) {
                    source.read_exact(&mut chunk[..expected.len()])?;
                    ensure!(expected == &chunk[..expected.len()], "original payload differs: {v}/{name}");
                }
                Ok(())
            };
            if name == "adapter.json" {
                compare(&id.identity)?;
                bytes += id.identity.len() as u64;
            } else {
                let record = &id.files[name];
                store.stream(record, compare).with_context(|| format!("verifying adapter payload: {v}/{name}"))?;
                blobs.insert(record.sha256.clone());
                bytes += record.bytes;
            }
            ensure!(source.read(&mut chunk)? == 0, "original payload has trailing bytes: {v}/{name}");
            entries += 1;
        }
    }
    Ok(serde_json::json!({"versions":versions.len(),"logical_entries":entries,"verified_bytes":bytes,
        "unique_files":blobs.len(),"unique_blobs":store.zip.file_names().filter(|s| s.starts_with("blobs/")).count()
            + store.shards.values().map(BTreeMap::len).sum::<usize>(),
        "recipes":store.index.recipes.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Same pieces, deliberately repeated and not line- or UTF-8-aligned.
    const A: &[u8] = b"\xf0\x9f";
    const B: &[u8] = b"\x9a\x80\r\n";

    fn fixture(dir: &Path, schema: u32, fault: &str) -> Result<Store> {
        let payload = [A, B, A].concat();
        let record = |data: &[u8]| ResourceRecord { sha256: digest(data), bytes: data.len() as u64 };
        let full = record(&payload);
        let files = BTreeMap::from([("inventory.json", full.clone()),
            ("oracle/oracle-manifest.json", if schema == 3 { record(B) } else { full.clone() })]);
        let content: String = files.iter().map(|(path, r)| format!("{path}\0{}\0{}\n", r.sha256, r.bytes)).collect();
        let identity = serde_json::to_vec(&serde_json::json!({"schema_version":1, "game_version":"v",
            "files":files, "content_id":digest(content.as_bytes())}))?;
        let mut index = serde_json::json!({"schema_version":schema,"format":FORMAT,"versions":{"v":digest(&identity)}});
        let mut zip = zip::ZipWriter::new(File::create(dir.join(ARCHIVE))?);
        let mut put = |name: String, bytes: &[u8]| -> Result<()> {
            zip.start_file(name, zip::write::SimpleFileOptions::default())?;
            Ok(zip.write_all(bytes)?)
        };
        put("versions/v.json".into(), &identity)?;
        if schema == 1 {
            put(format!("blobs/{}", full.sha256), &payload)?;
        } else {
            let pieces = if fault == "order" { [B, A, A] } else { [A, B, A] };
            let recipe = serde_json::to_vec(&serde_json::json!({"chunks":pieces.map(record)}))?;
            let receipt = if fault == "recipe" { digest(b"other recipe") } else { digest(&recipe) };
            index["recipes"] = serde_json::json!({ (full.sha256): receipt });
            put(format!("recipes/{receipt}"), &recipe)?;
            if schema == 2 {
                put(format!("blobs/{}", digest(A)), if fault == "blob" { b"xx" } else { A })?;
                put(format!("blobs/{}", digest(B)), B)?;
            } else {
                let packed = [if fault == "piece" { b"xx" } else { A }, B].concat();
                put("groups/0".into(), &packed)?;
                index["groups"] = serde_json::json!([if fault == "group" { digest(b"not this group") } else { digest(&packed) }]);
                for (data, offset) in [(A, 0), (B, A.len())] {
                    let sha = digest(data);
                    let offset = if fault == "offset" { PACK_BYTES as usize } else { offset };
                    let locator = serde_json::to_vec(&serde_json::json!({ (sha.clone()): [0, offset, data.len()] }))?;
                    put(format!("locators/{}", &sha[..2]), &locator)?;
                    index["locators"][&sha[..2]] = serde_json::json!(if fault == "locator" { digest(b"other index") } else { digest(&locator) });
                }
            }
        }
        zip.finish()?;
        fs::write(dir.join(INDEX), serde_json::to_vec(&index)?)?;
        Store::open(dir)
    }

    #[test]
    fn v1_and_v2_keep_original_bytes_and_detect_tampering() -> Result<()> {
        for (schema, fault, error) in [(1, "", ""), (2, "", ""), (2, "order", "reconstructed adapter file digest mismatch"),
            (2, "recipe", "adapter recipe digest mismatch"), (2, "blob", "adapter blob content mismatch")] {
            let dir = tempfile::tempdir()?;
            let mut store = fixture(dir.path(), schema, fault)?;
            let id = store.select("v")?.unwrap();
            if error.is_empty() {
                store.check_layout()?;
                assert_eq!(store.read(&id, "inventory.json")?.as_slice(), [A, B, A].concat());
            } else {
                let err = store.read(&id, "inventory.json").unwrap_err();
                assert!(format!("{err:#}").contains(error), "{err:#}");
            }
        }
        Ok(())
    }

    #[test]
    fn v3_groups_preserve_selected_files_and_reject_tampering() -> Result<()> {
        for (fault, expected) in [("", ""), ("locator", "adapter locator digest mismatch"),
            ("offset", "invalid adapter locator"), ("group", "adapter group digest mismatch"),
            ("piece", "adapter blob content mismatch"), ("order", "reconstructed adapter file digest mismatch")] {
            let dir = tempfile::tempdir()?;
            let mut store = fixture(dir.path(), 3, fault)?;
            let result = (|| -> Result<()> {
                let v = store.select("v")?.unwrap();
                assert!(store.packs.is_empty(), "selecting identity must not inflate group payloads");
                assert_eq!(store.read(&v, "inventory.json")?.as_slice(), [A, B, A].concat());
                assert_eq!(store.read(&v, "oracle/oracle-manifest.json")?.as_slice(), B);
                store.check_layout()
            })();
            if expected.is_empty() { result?; }
            else { assert!(format!("{:#}", result.unwrap_err()).contains(expected), "{fault}"); }
        }
        Ok(())
    }
}
