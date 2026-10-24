//! Shared immutable game-resource packages. Import reads ZIP/data only; it never
//! starts Java and never copies executable game implementation into a package.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File}, io::{BufRead, BufReader, Read, Write}, path::{Component, Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

const SCHEMA: u32 = 1;
const MANIFEST: &str = "manifest.json";
const RESOURCE_JAR: &str = "resources.jar";
const SNAPSHOT: &str = "NOVA_GAME_DATA_SNAPSHOT";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameDataContext {
    pub schema_version: u32,
    pub id: String,
    pub game_version: String,
    pub content_id: String,
    pub root: PathBuf,
    pub resource_jar: PathBuf,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceRecord { pub bytes: u64, pub sha256: String }

#[derive(Clone, Debug, Deserialize)]
pub struct OracleInputIdentity {
    pub minecraft: String,
    pub world_version: i32,
    pub protocol_version: i32,
    pub java_major: u32,
    pub client_jar_sha256: String,
    pub client_jar_size: u64,
    pub manifest_sha256: String,
}

/// Exact oracle provenance is bound to the immutable resource index, not to a
/// hash merely self-reported by oracle-manifest.json. Old default packages can
/// return None so their existing compile-time 26.3 pin remains authoritative.
pub fn oracle_identity(context: &GameDataContext) -> Result<Option<OracleInputIdentity>> {
    let index: BTreeMap<String, ResourceRecord> = read_json(&context.root.join("index.json"))?;
    let mut package_hash = Sha256::new();
    package_hash.update(format!("nova.game-data.v{SCHEMA}\0{}\0", context.game_version));
    package_hash.update(serde_json::to_vec(&index)?);
    ensure!(format!("{:x}", package_hash.finalize()) == context.content_id, "oracle resource index no longer matches its package identity");
    let read_indexed = |relative: &str| -> Result<Option<Vec<u8>>> {
        let Some(record) = index.get(relative) else { return Ok(None); };
        let bytes = read_resource_from(context, relative)?.with_context(|| format!("oracle identity resource is missing: {relative}"))?;
        ensure!(bytes.len() as u64 == record.bytes && format!("{:x}", Sha256::digest(&bytes)) == record.sha256,
            "oracle identity resource content differs from package index: {relative}");
        Ok(Some(bytes))
    };
    let Some(adapter_bytes) = read_indexed("derived/adapter.json")? else { return Ok(None); };
    let adapter: serde_json::Value = serde_json::from_slice(&adapter_bytes)?;
    ensure!(adapter["game_version"].as_str() == Some(context.game_version.as_str()), "oracle adapter version differs from selected game data");
    let Some(value) = adapter.get("oracle_identity") else { return Ok(None); };
    let identity: OracleInputIdentity = serde_json::from_value(value.clone()).context("invalid version oracle input identity")?;
    ensure!(identity.minecraft == context.game_version, "oracle input identity version differs from selected game data");
    let manifest_bytes = read_indexed("derived/oracle/oracle-manifest.json")?.context("oracle input manifest is missing")?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    ensure!(format!("{:x}", Sha256::digest(&manifest_bytes)) == identity.manifest_sha256,
        "oracle manifest differs from indexed adapter identity");
    ensure!(manifest["minecraft"].as_str() == Some(identity.minecraft.as_str())
        && manifest["world_version"].as_i64() == Some(identity.world_version as i64)
        && manifest["protocol_version"].as_i64() == Some(identity.protocol_version as i64)
        && manifest["java_major"].as_u64() == Some(identity.java_major as u64)
        && manifest["client_jar_sha256"].as_str() == Some(identity.client_jar_sha256.as_str())
        && manifest["client_jar_size"].as_u64() == Some(identity.client_jar_size), "oracle provenance differs from indexed adapter identity");
    ensure!(identity.client_jar_sha256.len() == 64 && identity.client_jar_sha256.bytes().all(|c| c.is_ascii_hexdigit()), "invalid oracle client JAR digest");
    Ok(Some(identity))
}

#[derive(Serialize, Deserialize)]
struct PackageManifest {
    #[serde(flatten)]
    context: GameDataContext,
    imported_ms: u64,
    source_jar_sha256: String,
    data_version: Option<i64>,
    resource_counts: BTreeMap<String, usize>,
    missing: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct PackageIndex { schema_version: u32, ids: Vec<String> }

#[derive(Serialize, Deserialize)]
struct Selection { schema_version: u32, id: String }

#[derive(Serialize)]
pub struct GameDataList { pub active: Option<GameDataContext>, pub entries: Vec<GameDataContext> }

fn stamp() -> u128 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() }
fn store_root() -> Result<PathBuf> { Ok(crate::runtime_paths::data_root()?.join("game-data")) }
fn default_root() -> Result<PathBuf> { Ok(crate::runtime_paths::app_root()?.join("pack-in/game-data/default")) }
fn absolute(path: &Path) -> Result<PathBuf> { Ok(if path.is_absolute() { path.to_owned() } else { std::env::current_dir()?.join(path) }) }

fn normalized(relative: &str) -> Result<String> {
    ensure!(!relative.is_empty() && !relative.contains('\\') && !relative.contains('\0'), "invalid game-resource path");
    let path = Path::new(relative);
    ensure!(path.components().all(|part| matches!(part, Component::Normal(_))), "game-resource path leaves its package");
    Ok(path.to_string_lossy().replace('\\', "/"))
}

fn resource_path(relative: &str) -> bool {
    let name = relative.to_ascii_lowercase();
    let permitted = name.starts_with("assets/") || name.starts_with("data/")
        || matches!(name.as_str(), "version.json" | "pack.mcmeta" | "pack.png");
    permitted && ![".class", ".java", ".exe", ".dll", ".so", ".dylib", ".rs", ".py", ".ps1", ".cmd", ".bat", ".js", ".jar"].iter().any(|extension| name.ends_with(extension))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("读取游戏数据：{}", path.display()))?)
        .with_context(|| format!("游戏数据 JSON 无效：{}", path.display()))
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("game-data destination has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".game-data-{}-{}.tmp", std::process::id(), stamp()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path).context("atomic game-data metadata update")?;
        Ok(())
    })();
    if temporary.exists() { let _ = fs::remove_file(&temporary); }
    result
}

fn package_context(root: &Path) -> Result<GameDataContext> {
    let manifest: PackageManifest = read_json(&root.join(MANIFEST))?;
    let mut context = manifest.context;
    ensure!(context.schema_version == SCHEMA, "unsupported game-data package schema");
    ensure!(context.content_id.len() == 64 && context.content_id.bytes().all(|c| c.is_ascii_hexdigit()), "invalid game-data content identity");
    // Stored metadata is relocatable, including a default copied into an app.
    context.root = root.canonicalize().context("game-data package directory is missing")?;
    context.resource_jar = context.root.join(RESOURCE_JAR);
    ensure!(context.resource_jar.is_file() && context.root.join("index.json").is_file(), "游戏数据包不完整：缺少资源或索引");
    Ok(context)
}

pub fn context_from_resource_jar(path: &Path) -> Result<Option<GameDataContext>> {
    if path.file_name().and_then(|value| value.to_str()) != Some(RESOURCE_JAR) { return Ok(None); }
    let Some(root) = path.parent() else { return Ok(None); };
    if !root.join(MANIFEST).is_file() { return Ok(None); }
    let value: serde_json::Value = read_json(&root.join(MANIFEST))?;
    if value.get("schema_version").and_then(|value| value.as_u64()) != Some(SCHEMA as u64) { return Ok(None); }
    Ok(Some(package_context(root)?))
}

/// UI selection resolution deliberately ignores a running task's snapshot.
pub fn resolve_context(store: &Path, default: &Path) -> Result<GameDataContext> {
    let selection_path = store.join("selection.json");
    if !selection_path.is_file() { return package_context(default).context("未找到默认游戏数据，请在选项中导入客户端 JAR"); }
    let selection: Selection = read_json(&selection_path)?;
    ensure!(selection.schema_version == SCHEMA, "unsupported game-data selection schema");
    if selection.id == "default" { return package_context(default); }
    ensure!(valid_id(&selection.id), "invalid selected game-data identity");
    package_context(&store.join("packages").join(selection.id)).context("选定游戏数据包不可用，请重新选择或恢复默认")
}

thread_local! {
    static OPERATION_CONTEXT: std::cell::RefCell<Option<GameDataContext>> = const { std::cell::RefCell::new(None) };
}

pub(crate) struct OperationContext(Option<GameDataContext>);
impl Drop for OperationContext {
    fn drop(&mut self) {
        OPERATION_CONTEXT.with(|slot| { slot.replace(self.0.take()); });
    }
}

/// Validate the immutable worker snapshot once per operation, not once per
/// material/recipe lookup. A later operation validates it again; Host resource
/// selection changes still create a worker with a different fixed snapshot.
pub(crate) fn operation_context() -> Result<OperationContext> {
    let context = if std::env::var_os(SNAPSHOT).is_some_and(|v| !v.is_empty()) {
        Some(resolve_active_context()?)
    } else { None };
    Ok(OperationContext(OPERATION_CONTEXT.with(|slot| slot.replace(context))))
}

pub fn active_context() -> Result<GameDataContext> {
    if let Some(context) = OPERATION_CONTEXT.with(|slot| slot.borrow().clone()) { return Ok(context); }
    resolve_active_context()
}

fn resolve_active_context() -> Result<GameDataContext> {
    if let Some(json) = std::env::var_os(SNAPSHOT).filter(|value| !value.is_empty()) {
        let context: GameDataContext = serde_json::from_str(&json.to_string_lossy()).context("invalid fixed game-data snapshot")?;
        ensure!(context.root.is_absolute() && context.resource_jar.is_absolute(), "fixed game-data snapshot paths must be absolute");
        let actual = package_context(&context.root)?;
        ensure!(actual.id == context.id && actual.content_id == context.content_id && actual.game_version == context.game_version
            && actual.resource_jar == context.resource_jar, "fixed game-data snapshot no longer matches its immutable package");
        return Ok(actual);
    }
    resolve_context(&store_root()?, &default_root()?)
}

#[cfg(test)]
mod operation_context_tests {
    use super::*;
    #[test]
    fn fixed_snapshot_is_reused_within_operation_and_revalidated_between_operations() {
        struct RestoreEnv(Option<std::ffi::OsString>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) { unsafe { if let Some(value)=self.0.take(){std::env::set_var(SNAPSHOT,value)}else{std::env::remove_var(SNAPSHOT)} } }
        }
        let _restore=RestoreEnv(std::env::var_os(SNAPSHOT));
        struct RemoveDirectory(PathBuf);
        impl Drop for RemoveDirectory { fn drop(&mut self){ let _=fs::remove_dir_all(&self.0); } }
        let directory=std::env::temp_dir().join(format!("nova-context-test-{}-{}",std::process::id(),stamp()));
        fs::create_dir(&directory).unwrap();
        let _directory=RemoveDirectory(directory.clone());
        let root=directory.canonicalize().unwrap();
        fs::write(root.join(RESOURCE_JAR),[]).unwrap();fs::write(root.join("index.json"),b"{}").unwrap();
        let context=GameDataContext{schema_version:1,id:"fixture".into(),game_version:"26.3".into(),content_id:"a".repeat(64),root:root.clone(),resource_jar:root.join(RESOURCE_JAR),source:"fixture".into()};
        let manifest=PackageManifest{context:context.clone(),imported_ms:0,source_jar_sha256:"b".repeat(64),data_version:Some(5023),resource_counts:BTreeMap::new(),missing:Vec::new()};
        fs::write(root.join(MANIFEST),serde_json::to_vec(&manifest).unwrap()).unwrap();
        unsafe { std::env::set_var(SNAPSHOT,serde_json::to_string(&context).unwrap()); }
        {
            let _operation=operation_context().unwrap();
            fs::write(root.join(MANIFEST),b"invalid").unwrap();
            assert_eq!(active_context().unwrap().content_id,context.content_id);
        }
        assert!(active_context().is_err());
        assert!(operation_context().is_err());
    }
}

pub fn context_metadata(context: &GameDataContext) -> Result<serde_json::Value> {
    let mut value: serde_json::Value = read_json(&context.root.join(MANIFEST))?;
    for (key, entry) in serde_json::to_value(context)?.as_object().context("game-data context must be an object")? { value[key] = entry.clone(); }
    Ok(value)
}

pub fn read_resource(relative: &str) -> Result<Option<Vec<u8>>> { read_resource_from(&active_context()?, relative) }

pub fn read_resource_from(context: &GameDataContext, relative: &str) -> Result<Option<Vec<u8>>> {
    let relative = normalized(relative)?;
    ensure!(resource_path(&relative) || relative.starts_with("derived/"), "not a resource/data package path");
    let path = context.root.join(relative);
    if !path.is_file() { return Ok(None); }
    ensure!(path.canonicalize()?.starts_with(context.root.canonicalize()?), "game-resource symlink leaves its package");
    Ok(Some(fs::read(path)?))
}

pub fn list_resources(prefix: &str) -> Result<Vec<String>> { list_resources_from(&active_context()?, prefix) }

pub fn list_resources_from(context: &GameDataContext, prefix: &str) -> Result<Vec<String>> {
    let prefix = if prefix.is_empty() { String::new() } else { normalized(prefix.trim_end_matches('/'))? };
    let index: BTreeMap<String, ResourceRecord> = read_json(&context.root.join("index.json"))?;
    Ok(index.keys().filter(|path| prefix.is_empty() || *path == &prefix || path.starts_with(&format!("{prefix}/"))).cloned().collect())
}

pub fn list_contexts(store: &Path, default: &Path) -> Result<GameDataList> {
    let mut entries = Vec::new();
    if default.join(MANIFEST).is_file() { entries.push(package_context(default)?); }
    let index_path = store.join("index.json");
    if index_path.is_file() {
        let index: PackageIndex = read_json(&index_path)?;
        ensure!(index.schema_version == SCHEMA, "unsupported game-data index schema");
        for id in index.ids {
            ensure!(valid_id(&id), "invalid indexed game-data identity");
            let context = package_context(&store.join("packages").join(&id))?;
            if !entries.iter().any(|entry: &GameDataContext| entry.id == context.id) { entries.push(context); }
        }
    }
    let active = if !default.join(MANIFEST).exists() && !store.join("selection.json").exists() { None }
        else { Some(resolve_context(store, default)?) };
    Ok(GameDataList { active, entries })
}

fn valid_id(id: &str) -> bool { !id.is_empty() && id != "." && id != ".." && id.len() <= 160 && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)) }

pub fn select_context(store: &Path, default: &Path, id: &str) -> Result<GameDataContext> {
    ensure!(valid_id(id), "invalid game-data identity");
    let imported = store.join("packages").join(id);
    let (selected, context) = if id == "default" { ("default", package_context(default)?) }
        else if imported.join(MANIFEST).is_file() { (id, package_context(&imported)?) }
        else { let context = package_context(default)?; ensure!(context.id == id, "requested game-data package was not found"); ("default", context) };
    atomic_json(&store.join("selection.json"), &Selection { schema_version: SCHEMA, id: selected.to_owned() })?;
    Ok(context)
}

fn record_file(root: &Path, relative: &str, mut input: impl Read, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
    let relative = normalized(relative)?;
    let destination = root.join(&relative);
    fs::create_dir_all(destination.parent().context("resource destination parent")?)?;
    let mut file = File::create(destination)?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 { break; }
        bytes += count as u64;
        ensure!(bytes <= 1024 * 1024 * 1024, "individual resource exceeds the importer memory/storage boundary");
        digest.update(&buffer[..count]);
        file.write_all(&buffer[..count])?;
    }
    index.insert(relative, ResourceRecord { bytes, sha256: format!("{:x}", digest.finalize()) });
    Ok(())
}

fn language_values(bytes: &[u8], legacy: bool) -> Result<BTreeMap<String, String>> {
    if !legacy {
        let value: serde_json::Value = serde_json::from_slice(bytes).context("invalid game language JSON")?;
        let entries = value.as_object().context("game language JSON must contain an object")?;
        return Ok(entries.iter().filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned()))).collect());
    }
    let mut values = BTreeMap::new();
    for line in std::str::from_utf8(bytes)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((key, value)) = line.split_once('=') { values.insert(key.trim().to_owned(), value.to_owned()); }
    }
    Ok(values)
}

fn derive_languages(root: &Path, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
    let mut languages: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut sources = index.keys().filter(|path| path.starts_with("assets/") && path.contains("/lang/") && (path.ends_with(".json") || path.ends_with(".lang"))).cloned().collect::<Vec<_>>();
    sources.sort_by_key(|path| (path.ends_with(".json"), path.clone()));
    for relative in sources {
        let language = Path::new(&relative).file_stem().and_then(|name| name.to_str()).context("language filename")?.to_ascii_lowercase();
        // lang/ also contains metadata such as deprecated.json. Real locale
        // identifiers use a short language code, with optional region/script.
        let mut parts = language.split('_');
        let primary = parts.next().unwrap_or_default();
        if !(2..=4).contains(&primary.len()) || !primary.bytes().all(|c| c.is_ascii_alphabetic())
            || !parts.all(|part| (2..=8).contains(&part.len()) && part.bytes().all(|c| c.is_ascii_alphanumeric())) { continue; }
        let values = language_values(&fs::read(root.join(&relative))?, relative.ends_with(".lang"))
            .with_context(|| format!("读取游戏语言：{relative}"))?;
        if values.is_empty() { continue; }
        languages.entry(language).or_default().extend(values);
    }
    for (language, values) in languages {
        record_file(root, &format!("derived/lang/{language}.json"), serde_json::to_vec(&values)?.as_slice(), index)?;
    }
    Ok(())
}

fn copy_assets(root: &Path, assets: &Path, asset_index: Option<&str>, index: &mut BTreeMap<String, ResourceRecord>, missing: &mut Vec<String>) -> Result<()> {
    let (assets, file) = if assets.is_file() {
        (assets.parent().and_then(Path::parent).context("asset index location")?.to_owned(), assets.to_owned())
    } else {
        let base = if assets.join("indexes").is_dir() { assets.to_owned() } else { assets.join("assets") };
        let indexes = base.join("indexes");
        let file = if let Some(id) = asset_index { indexes.join(format!("{id}.json")) }
            else { let mut files = fs::read_dir(&indexes).context("缺少配套 assets/indexes 目录")?.collect::<std::io::Result<Vec<_>>>()?
                .into_iter().map(|entry| entry.path()).filter(|path| path.extension().and_then(|v|v.to_str()) == Some("json")).collect::<Vec<_>>();
                ensure!(files.len() == 1, "配套资源存在多个版本索引；请提供此 JAR 对应的索引文件"); files.remove(0) };
        (base, file)
    };
    let value: serde_json::Value = read_json(&file).context("无法读取此游戏版本的配套资源索引")?;
    for (logical, entry) in value["objects"].as_object().context("配套资源索引缺少 objects")? {
        // Nova consumes language/font resources, not game audio or launcher
        // icons. Do not demand or copy unrelated entries from a full index.
        let language = logical.contains("/lang/");
        if !language && !logical.contains("/font/") { continue; }
        if language && !logical.ends_with("/zh_cn.json") && !logical.ends_with("/en_us.json")
            && !logical.ends_with("/zh_CN.lang") && !logical.ends_with("/en_US.lang") { continue; }
        let relative = format!("assets/{}", normalized(logical)?);
        if !resource_path(&relative) || index.contains_key(&relative) { continue; }
        let hash = entry["hash"].as_str().filter(|value| value.len() == 40 && value.bytes().all(|c| c.is_ascii_hexdigit())).context("invalid asset object SHA1")?;
        let object = assets.join("objects").join(&hash[..2]).join(hash);
        if !object.is_file() { missing.push(format!("配套资源：{logical}")); continue; }
        let mut file = File::open(&object)?;
        let mut digest = Sha1::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop { let count = file.read(&mut buffer)?; if count == 0 { break; } digest.update(&buffer[..count]); }
        ensure!(format!("{:x}", digest.finalize()).eq_ignore_ascii_case(hash), "配套资源内容与索引不符：{logical}");
        record_file(root, &relative, File::open(object)?, index)?;
    }
    Ok(())
}

fn copy_adapters(root: &Path, adapter: &Path, version: &str, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
    let identity: serde_json::Value = read_json(&adapter.join("adapter.json"))?;
    ensure!(identity["game_version"].as_str() == Some(version), "派生数据适配器与实际游戏版本不一致");
    fn visit(base: &Path, directory: &Path, root: &Path, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?; let kind = entry.file_type()?;
            ensure!(!kind.is_symlink(), "adapter symlinks are not admitted");
            if kind.is_dir() { visit(base, &entry.path(), root, index)?; }
            else if kind.is_file() {
                let name = entry.path().strip_prefix(base)?.to_string_lossy().replace('\\', "/");
                record_file(root, &format!("derived/{name}"), File::open(entry.path())?, index)?;
            }
        }
        Ok(())
    }
    visit(adapter, adapter, root, index)?;
    validate_adapter(root, "derived", version, index, false)?;
    let versions = root.join("derived/version-adapters");
    if versions.is_dir() {
        for entry in fs::read_dir(versions)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type()?.is_file() && name.ends_with(".zip") {
                let version = name.strip_suffix(".zip").context("invalid adapter archive name")?;
                ensure!(valid_id(version), "invalid version adapter identity");
                let mut archive = zip::ZipArchive::new(File::open(entry.path())?)?;
                let identity = read_archived_adapter_identity(&mut archive)?;
                ensure!(identity["game_version"].as_str() == Some(version) && identity["schema_version"].as_u64() == Some(1)
                    && identity["content_id"].as_str().is_some_and(|id| id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit()))
                    && identity["files"].is_object(), "版本适配归档缺少有效内容身份：{version}");
                continue;
            }
            ensure!(entry.file_type()?.is_dir(), "version adapter entries must be directories or ZIP archives");
            let version = name;
            ensure!(valid_id(&version), "invalid version adapter identity");
            validate_adapter(root, &format!("derived/version-adapters/{version}"), &version, index, true)?;
        }
    }
    Ok(())
}

fn read_archived_adapter_identity(archive: &mut zip::ZipArchive<File>) -> Result<serde_json::Value> {
    let mut entry = archive.by_name("adapter.json").context("version adapter archive has no identity")?;
    ensure!(entry.size() <= 1024 * 1024, "version adapter identity is too large");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn unpack_version_adapter(archive_path: &Path, root: &Path, version: &str, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(archive_path)?)?;
    let identity = read_archived_adapter_identity(&mut archive)?;
    ensure!(identity["game_version"].as_str() == Some(version), "版本适配归档与所选游戏版本不一致：{version}");
    ensure!(archive.len() <= 100_000, "version adapter archive has too many files");
    let mut bytes = 0_u64;
    let mut names = BTreeSet::new();
    for position in 0..archive.len() {
        let mut entry = archive.by_index(position)?;
        ensure!(entry.unix_mode().is_none_or(|mode| mode & 0o170000 != 0o120000), "adapter symlinks are not admitted");
        if entry.is_dir() { continue; }
        let relative = normalized(entry.name())?;
        ensure!(names.insert(relative.clone()), "duplicate version adapter archive entry: {relative}");
        bytes = bytes.checked_add(entry.size()).context("version adapter size overflow")?;
        ensure!(bytes <= 512 * 1024 * 1024, "version adapter archive exceeds its decompressed storage boundary");
        record_file(root, &format!("derived/{relative}"), &mut entry, index)?;
    }
    validate_adapter(root, "derived", version, index, true)
}

// A version number selects a table; it is not evidence of that table's contents.
// Bind every shipped per-version adapter to its actual immutable file records.
fn validate_adapter(root: &Path, prefix: &str, version: &str, index: &BTreeMap<String, ResourceRecord>, required: bool) -> Result<()> {
    let identity: serde_json::Value = read_json(&root.join(prefix).join("adapter.json"))?;
    ensure!(identity["game_version"].as_str() == Some(version), "派生数据适配器与实际游戏版本不一致：{version}");
    if required || identity.get("content_id").is_some() {
        ensure!(identity["schema_version"].as_u64() == Some(1), "unsupported version adapter schema: {version}");
        let files = identity["files"].as_object().context("version adapter has no content records")?;
        let mut digest = Sha256::new();
        let mut found = 0;
        let start = format!("{prefix}/");
        for (path, record) in index.range(start.clone()..) {
            let Some(relative) = path.strip_prefix(&start) else { break; };
            if relative == "adapter.json" { continue; }
            let expected = files.get(relative).with_context(|| format!("version adapter has an unrecorded file: {version}/{relative}"))?;
            ensure!(expected["bytes"].as_u64() == Some(record.bytes) && expected["sha256"].as_str() == Some(record.sha256.as_str()), "版本适配数据内容身份不匹配：{version}/{relative}");
            digest.update(format!("{relative}\0{}\0{}\n", record.sha256, record.bytes));
            found += 1;
        }
        ensure!(found == files.len(), "version adapter content records are incomplete: {version}");
        ensure!(identity["content_id"].as_str() == Some(format!("{:x}", digest.finalize()).as_str()), "版本适配数据内容身份不匹配：{version}");
    }
    validate_semantic_versions(root, prefix, version, index)
}

fn validate_semantic_versions(root: &Path, prefix: &str, version: &str, index: &BTreeMap<String, ResourceRecord>) -> Result<()> {
    let inventory_path = root.join(prefix).join("inventory.json");
    if inventory_path.is_file() {
        let inventory: serde_json::Value = read_json(&inventory_path)?;
        ensure!(inventory["minecraft_version"].as_str() == Some(version), "物品语义适配器与实际游戏版本不一致：{version}");
    }
    let oracle_path = root.join(prefix).join("oracle/oracle-manifest.json");
    if oracle_path.is_file() {
        let manifest: serde_json::Value = read_json(&oracle_path)?;
        ensure!(manifest["minecraft"].as_str() == Some(version), "注册表 oracle 与实际游戏版本不一致：{version}");
        if let Some(artifacts) = manifest["artifacts"].as_array() {
            for artifact in artifacts {
                let path = normalized(artifact["path"].as_str().context("oracle artifact has no relative path")?)?;
                let record = index.get(&format!("{prefix}/oracle/{path}")).with_context(|| format!("oracle artifact is missing: {version}/{path}"))?;
                ensure!(artifact["records_sha256"].as_str() == Some(record.sha256.as_str()), "oracle 内容与版本清单不一致：{version}/{path}");
            }
        }
    }
    Ok(())
}

fn copy_derived(source: &Path, destination: &Path, relative: &str, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
    let path = source.join(relative);
    if !path.exists() { return Ok(()); }
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(!metadata.file_type().is_symlink(), "adapter symlinks are not admitted");
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            copy_derived(source, destination, &format!("{relative}/{}", entry.file_name().to_string_lossy()), index)?;
        }
    } else if metadata.is_file() {
        record_file(destination, relative, File::open(path)?, index)?;
    }
    Ok(())
}

fn reuse_version_adapters(root: &Path, default: &Path, version: &str, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
    if !default.join(MANIFEST).is_file() { return Ok(()); }
    let default_context = package_context(default)?;
    // This contains real per-version tables, not default-version resources.
    // Consumers must still select the requested registry exactly.
    ensure!(valid_id(version), "invalid game version adapter selector");
    let exact = default.join("derived/version-adapters").join(version);
    let archive = default.join("derived/version-adapters").join(format!("{version}.zip"));
    if archive.is_file() {
        unpack_version_adapter(&archive, root, version, index)?;
    } else if exact.is_dir() {
        // Flatten only the selected exact adapter into this imported package.
        // Other versions' inventory/oracle facts never enter its active paths.
        copy_adapters(root, &exact, version, index)?;
        validate_adapter(root, "derived", version, index, true)?;
    } else if default_context.game_version == version {
        for relative in ["derived/inventory.json", "derived/oracle", "derived/blockstates.json", "derived/enumerator"] {
            copy_derived(default, root, relative, index)?;
        }
    }
    copy_derived(default, root, "derived/blockdb", index)?;
    // Visual icons and language are source-dependent; never reuse their old
    // generated results just because a modified JAR reports the same version.
    Ok(())
}

fn oracle_rows(root: &Path, file: &str, mut visit: impl FnMut(serde_json::Value) -> Result<()>) -> Result<bool> {
    let path = root.join("derived/oracle").join(file);
    if !path.is_file() { return Ok(false); }
    for (line, value) in BufReader::new(File::open(&path)?).lines().enumerate() {
        let value = value?;
        if value.trim().is_empty() { continue; }
        visit(serde_json::from_str(&value).with_context(|| format!("oracle {}:{}", path.display(), line + 1))?)?;
    }
    Ok(true)
}

fn derive_version_registry(root: &Path, version: &str, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
    if index.contains_key("derived/blockstates.json") && index.contains_key("derived/enumerator/DV_Blocks.json")
        && index.contains_key("derived/enumerator/DV_Items.json") { return Ok(()); }
    let path = root.join("derived/blockdb/blocks.dat.z");
    if !path.is_file() { return Ok(()); }
    // Reuse the existing NOVAST04 exact-version supplement. The embedded
    // visual tables are not used as another version's resource fallback.
    let mut bytes = Vec::new();
    flate2::read::ZlibDecoder::new(File::open(path)?).take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 64 * 1024 * 1024, "version registry adapter is too large");
    let position = bytes.windows(8).rposition(|value| value == b"NOVAST04").context("version registry adapter has no exact supplement")?;
    let length_bytes: [u8; 4] = bytes.get(position + 8..position + 12).context("truncated registry supplement")?.try_into()?;
    let length = u32::from_le_bytes(length_bytes) as usize;
    let compressed = bytes.get(position + 12..position + 12 + length).context("truncated registry supplement data")?;
    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(compressed).take(64 * 1024 * 1024 + 1).read_to_end(&mut decoded)?;
    ensure!(decoded.len() <= 64 * 1024 * 1024, "registry supplement is too large");
    let supplement: serde_json::Value = serde_json::from_slice(&decoded)?;
    ensure!(supplement["schema"].as_u64() == Some(4), "unsupported exact registry supplement");
    let Some(registry_id) = supplement["versions"][version]["registry"].as_str() else { return Ok(()); };
    let registry = &supplement["registries"][registry_id];
    let registered_blocks = registry["blocks"].as_object().context("exact version registry is missing blocks")?;
    if !index.contains_key("derived/blockstates.json") {
        let mut blocks = BTreeMap::new();
        for (id, block) in registered_blocks {
            let default = block["default"].as_str().context("registered block is missing its actual default state")?;
            let mut defaults = BTreeMap::new();
            if let Some((_, properties)) = default.split_once('[') {
                for property in properties.strip_suffix(']').context("invalid registered default block state")?.split(',') {
                    if property.is_empty() { continue; }
                    let (key, value) = property.split_once('=').context("invalid registered default block property")?;
                    defaults.insert(key.to_owned(), value.to_owned());
                }
            }
            blocks.insert(id.clone(), serde_json::json!({"properties":block["properties"],"default_properties":defaults}));
        }
        record_file(root, "derived/blockstates.json", serde_json::to_vec(&serde_json::json!({"version_id":version,"blocks":blocks}))?.as_slice(), index)?;
    }
    if !index.contains_key("derived/enumerator/DV_Blocks.json") {
        let blocks: BTreeSet<_> = registered_blocks.keys().collect();
        record_file(root, "derived/enumerator/DV_Blocks.json", serde_json::to_vec(&blocks)?.as_slice(), index)?;
    }
    if !index.contains_key("derived/enumerator/DV_Items.json") {
        let items: BTreeSet<String> = serde_json::from_value(registry["items"].clone()).context("exact version registry is missing items")?;
        record_file(root, "derived/enumerator/DV_Items.json", serde_json::to_vec(&items)?.as_slice(), index)?;
    }
    Ok(())
}

#[derive(Default, Serialize)]
struct BlockProperties {
    properties: BTreeMap<String, BTreeSet<String>>,
    default_properties: BTreeMap<String, String>,
}

fn derive_registry_data(root: &Path, version: &str, index: &mut BTreeMap<String, ResourceRecord>) -> Result<()> {
    validate_semantic_versions(root, "derived", version, index)?;
    let mut blocks: BTreeMap<String, BlockProperties> = BTreeMap::new();
    if oracle_rows(root, "block_states.jsonl", |row| {
        let id = row["block_id"].as_str().context("oracle block state has no block_id")?;
        let block = blocks.entry(id.to_owned()).or_default();
        if let Some(properties) = row["properties"].as_object() {
            for (name, value) in properties {
                let value = value.as_str().context("oracle block property is not a string")?;
                block.properties.entry(name.clone()).or_default().insert(value.to_owned());
                if row["default_state"].as_bool() == Some(true) { block.default_properties.insert(name.clone(), value.to_owned()); }
            }
        }
        Ok(())
    })? {
        record_file(root, "derived/enumerator/DV_Blocks.json", serde_json::to_vec(&blocks.keys().collect::<Vec<_>>())?.as_slice(), index)?;
        record_file(root, "derived/blockstates.json", serde_json::to_vec(&serde_json::json!({"version_id":version,"blocks":blocks}))?.as_slice(), index)?;
    }
    let mut items = BTreeSet::new();
    if oracle_rows(root, "builtin_registries.jsonl", |row| {
        if row["registry_id"].as_str() == Some("minecraft:item") { items.insert(row["entry_id"].as_str().context("oracle item has no entry_id")?.to_owned()); }
        Ok(())
    })? { record_file(root, "derived/enumerator/DV_Items.json", serde_json::to_vec(&items)?.as_slice(), index)?; }
    let mut entities = BTreeSet::new();
    if oracle_rows(root, "entity_types.jsonl", |row| {
        entities.insert(row["type_id"].as_str().context("oracle entity has no type_id")?.to_owned()); Ok(())
    })? { record_file(root, "derived/enumerator/DV_Entities.json", serde_json::to_vec(&entities)?.as_slice(), index)?; }
    let mut enchantments = BTreeSet::new();
    if oracle_rows(root, "data_registries.jsonl", |row| {
        if row["registry_id"].as_str() == Some("minecraft:enchantment") { enchantments.insert(row["entry_id"].as_str().context("oracle enchantment has no entry_id")?.to_owned()); }
        Ok(())
    })? { record_file(root, "derived/enumerator/DV_Enchantments.json", serde_json::to_vec(&enchantments)?.as_slice(), index)?; }
    if !index.contains_key("derived/enumerator/DV_Enchantments.json") {
        // Data-driven enchantments are real registry definitions in versions
        // that ship them. Older code-defined registries remain a missing fact.
        for path in index.keys().filter(|path| path.starts_with("data/") && path.ends_with(".json")) {
            let Some((namespace, entry)) = path.strip_prefix("data/").and_then(|path| path.split_once('/')) else { continue; };
            let Some(entry) = entry.strip_prefix("enchantment/").and_then(|path| path.strip_suffix(".json")) else { continue; };
            let definition: serde_json::Value = read_json(&root.join(path))?;
            ensure!(definition.is_object(), "enchantment registry definition is invalid: {path}");
            enchantments.insert(format!("{namespace}:{entry}"));
        }
        if !enchantments.is_empty() { record_file(root, "derived/enumerator/DV_Enchantments.json", serde_json::to_vec(&enchantments)?.as_slice(), index)?; }
    }
    derive_version_registry(root, version, index)?;
    Ok(())
}

fn make_resource_jar(root: &Path, index: &BTreeMap<String, ResourceRecord>) -> Result<()> {
    let mut archive = zip::ZipWriter::new(File::create(root.join(RESOURCE_JAR))?);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for relative in index.keys().filter(|path| resource_path(path)) {
        archive.start_file(relative, options)?;
        std::io::copy(&mut File::open(root.join(relative))?, &mut archive)?;
    }
    archive.finish()?;
    Ok(())
}

fn create_package(jar: &Path, assets: Option<&Path>, version: Option<&str>, adapter: Option<&Path>, default: Option<&Path>, root: &Path, source: &str) -> Result<GameDataContext> {
    let mut archive = zip::ZipArchive::new(File::open(jar).context("无法打开客户端 JAR")?).context("客户端 JAR 不是有效 ZIP")?;
    let version_json: serde_json::Value = match archive.by_name("version.json") {
        Ok(mut entry) => { let mut bytes = Vec::new(); entry.read_to_end(&mut bytes)?; serde_json::from_slice(&bytes).context("JAR 的 version.json 无效")? },
        Err(zip::result::ZipError::FileNotFound) => serde_json::Value::Null,
        Err(error) => return Err(error.into()),
    };
    let launcher_path = jar.with_extension("json");
    let launcher: serde_json::Value = if launcher_path.is_file() { read_json(&launcher_path)? } else { serde_json::Value::Null };
    let detected = version_json["id"].as_str().or_else(|| version_json["name"].as_str()).or_else(|| launcher["id"].as_str());
    if let (Some(explicit), Some(detected)) = (version, detected) { ensure!(explicit == detected, "指定游戏版本与 JAR 实际版本不一致：{explicit} / {detected}"); }
    let game_version = detected.or(version).context("JAR 未提供可识别版本，请提供同目录版本 JSON 或明确 --game-version")?.to_owned();
    ensure!(!game_version.is_empty() && game_version.len() <= 100, "invalid game version");
    let mut index = BTreeMap::new();
    for position in 0..archive.len() {
        let mut entry = archive.by_index(position)?;
        if entry.is_dir() { continue; }
        let relative = normalized(entry.name())?;
        if !resource_path(&relative) { continue; }
        ensure!(!index.contains_key(&relative), "JAR 包含重复资源路径：{relative}");
        record_file(root, &relative, &mut entry, &mut index)?;
    }
    ensure!(index.keys().any(|path| path.starts_with("assets/") || path.starts_with("data/")), "JAR 未包含可导入的 assets 或 data；不能把程序类当作游戏数据");
    let mut missing = Vec::new();
    let asset_id = launcher.pointer("/assetIndex/id").and_then(|value| value.as_str()).or_else(|| launcher["assets"].as_str());
    // Resolve only this launcher's exact index within its existing install
    // ancestors. No disk-wide search and no arbitrary other-version index.
    let nearby_assets = if assets.is_none() {
        asset_id.and_then(|id| jar.ancestors().skip(1).take(3).map(|parent| parent.join("assets"))
            .find(|directory| directory.join("indexes").join(format!("{id}.json")).is_file()))
    } else { None };
    if let Some(assets) = assets.or(nearby_assets.as_deref()) { copy_assets(root, assets, asset_id, &mut index, &mut missing)?; }
    if let Some(adapter) = adapter { copy_adapters(root, adapter, &game_version, &mut index)?; }
    else if let Some(default) = default { reuse_version_adapters(root, default, &game_version, &mut index)?; }
    derive_registry_data(root, &game_version, &mut index)?;
    derive_languages(root, &mut index)?;
    if !index.keys().any(|path| path.starts_with("assets/") && path.contains("/textures/")) { missing.push("贴图资源".into()); }
    if !index.keys().any(|path| path.starts_with("data/") && (path.contains("/recipe/") || path.contains("/recipes/"))) { missing.push("配方数据".into()); }
    for language in ["zh_cn", "en_us"] { if !index.contains_key(&format!("derived/lang/{language}.json")) { missing.push(format!("游戏语言：{language}")); } }
    if !index.contains_key("derived/inventory.json") { missing.push(format!("{} 的物品/放置语义适配数据", game_version)); }
    if !index.contains_key("derived/oracle/block_state_semantics.jsonl") { missing.push(format!("{} 的完整方块行为/遮挡/光照 oracle 语义", game_version)); }
    if !index.contains_key("derived/blockstates.json") { missing.push(format!("{} 的归一化方块状态数据（可用版本仍由精确注册表适配）", game_version)); }
    for kind in ["Blocks", "Items", "Entities", "Enchantments"] {
        if !index.contains_key(&format!("derived/enumerator/DV_{kind}.json")) { missing.push(format!("{} 的 {kind} 枚举数据", game_version)); }
    }
    let supported_versions = root.join("derived/blockdb/versions.json");
    if supported_versions.is_file() {
        let supported: Vec<String> = read_json(&supported_versions)?;
        if !supported.iter().any(|candidate| candidate == &game_version) { missing.push(format!("{} 的精确注册表适配", game_version)); }
    } else if !index.contains_key("derived/blockdb/blocks.dat.z") { missing.push(format!("{} 的精确注册表适配", game_version)); }
    let mut identity = Sha256::new();
    identity.update(format!("nova.game-data.v{SCHEMA}\0{game_version}\0"));
    identity.update(serde_json::to_vec(&index)?);
    let content_id = format!("{:x}", identity.finalize());
    let slug = game_version.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect::<String>();
    let context = GameDataContext { schema_version: SCHEMA, id: format!("mc-{slug}-{}", &content_id[..20]), game_version,
        content_id, root: PathBuf::from("."), resource_jar: PathBuf::from(RESOURCE_JAR), source: source.into() };
    let mut source_hash = Sha256::new();
    let mut source_file = File::open(jar)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop { let count = source_file.read(&mut buffer)?; if count == 0 { break; } source_hash.update(&buffer[..count]); }
    let counts = ["assets", "data", "derived"].into_iter().map(|prefix| (prefix.to_owned(), index.keys().filter(|path| path.starts_with(&format!("{prefix}/"))).count())).collect();
    make_resource_jar(root, &index)?;
    atomic_json(&root.join("index.json"), &index)?;
    atomic_json(&root.join(MANIFEST), &PackageManifest { context, imported_ms: (stamp() / 1_000_000).min(u64::MAX as u128) as u64,
        source_jar_sha256: format!("{:x}", source_hash.finalize()), data_version: version_json["world_version"].as_i64(), resource_counts: counts, missing })?;
    package_context(root)
}

struct Staging(PathBuf);
impl Drop for Staging { fn drop(&mut self) { if self.0.exists() { let _ = fs::remove_dir_all(&self.0); } } }
fn staging(parent: &Path) -> Result<Staging> { fs::create_dir_all(parent)?; let path = parent.join(format!(".game-data-import-{}-{}", std::process::id(), stamp())); fs::create_dir(&path)?; Ok(Staging(path)) }

pub fn import_jar(jar: &Path, assets: Option<&Path>, version: Option<&str>, adapter: Option<&Path>, store: &Path, default: &Path) -> Result<GameDataContext> {
    let store = absolute(store)?;
    let packages = store.join("packages");
    let temporary = staging(&packages)?;
    let generated = create_package(jar, assets, version, adapter, Some(default), &temporary.0, "imported-jar")?;
    let destination = packages.join(&generated.id);
    if destination.exists() { ensure!(package_context(&destination)?.content_id == generated.content_id, "immutable package identity conflict"); }
    else { fs::rename(&temporary.0, &destination)?; }
    let path = store.join("index.json");
    let mut index: PackageIndex = if path.exists() { read_json(&path)? } else { PackageIndex { schema_version: SCHEMA, ids: Vec::new() } };
    ensure!(index.schema_version == SCHEMA, "unsupported game-data index schema");
    if !index.ids.contains(&generated.id) { index.ids.push(generated.id.clone()); index.ids.sort(); atomic_json(&path, &index)?; }
    select_context(&store, default, &generated.id)
}

pub fn build_default(jar: &Path, assets: Option<&Path>, game_version: &str, adapter: Option<&Path>, output: &Path) -> Result<GameDataContext> {
    let output = absolute(output)?;
    if output.exists() {
        ensure!(output.is_dir(), "default game-data output must be a package directory");
        let empty = fs::read_dir(&output)?.next().is_none();
        let owned_package = read_json::<PackageManifest>(&output.join(MANIFEST))
            .is_ok_and(|manifest| manifest.context.schema_version == SCHEMA);
        ensure!(empty || owned_package, "拒绝替换非游戏数据包目录：{}", output.display());
    }
    let parent = output.parent().context("default package output has no parent")?;
    let temporary = staging(parent)?;
    let generated = create_package(jar, assets, Some(game_version), adapter, None, &temporary.0, "bundled-default")?;
    if output.exists() && package_context(&output).is_ok_and(|context| context.content_id == generated.content_id) { return package_context(&output); }
    let retired = parent.join(format!(".game-data-replace-{}-{}", std::process::id(), stamp()));
    let existed = output.exists();
    if existed { fs::rename(&output, &retired)?; }
    if let Err(error) = fs::rename(&temporary.0, &output) {
        if existed { fs::rename(&retired, &output).context("restore the existing default game-data package")?; }
        return Err(error.into());
    }
    if existed { fs::remove_dir_all(&retired)?; }
    package_context(&output)
}

pub fn run_cli(args: &[String]) -> Result<()> {
    let command = args.first().map(String::as_str).unwrap_or("context");
    let mut values = BTreeMap::new();
    let mut position = 1;
    while position < args.len() {
        let flag = &args[position];
        ensure!(flag.starts_with("--"), "unexpected game-data argument: {flag}");
        let value = args.get(position + 1).with_context(|| format!("missing {flag} value"))?;
        ensure!(values.insert(flag.as_str(), value.as_str()).is_none(), "duplicate game-data option: {flag}");
        position += 2;
    }
    let get = |flag: &str| values.get(flag).copied().with_context(|| format!("missing {flag}"));
    let store = values.get("--store").map(PathBuf::from).map_or_else(store_root, Ok)?;
    let default = values.get("--default-root").map(PathBuf::from).map_or_else(default_root, Ok)?;
    let value = match command {
        "list" => serde_json::to_value(list_contexts(&store, &default)?)?,
        "context" => serde_json::to_value(resolve_context(&store, &default)?)?,
        "inspect" => context_metadata(&resolve_context(&store, &default)?)?,
        "select" => serde_json::to_value(select_context(&store, &default, get("--id")?)?)?,
        "default" => serde_json::to_value(select_context(&store, &default, "default")?)?,
        "import" => serde_json::to_value(import_jar(Path::new(get("--jar")?), values.get("--assets-dir").map(Path::new), values.get("--game-version").copied(), values.get("--adapter-dir").map(Path::new), &store, &default)?)?,
        "build-default" => serde_json::to_value(build_default(Path::new(get("--jar")?), values.get("--assets-dir").map(Path::new), values.get("--game-version").copied().unwrap_or("26.3"), values.get("--adapter-dir").map(Path::new), Path::new(get("--output")?))?)?,
        "language" => {
            let language = get("--language")?.to_ascii_lowercase().replace('-', "_");
            ensure!(language.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'), "invalid game-language identity");
            let context = resolve_context(&store, &default)?;
            let values: BTreeMap<String, String> = match read_resource_from(&context, &format!("derived/lang/{language}.json"))? { Some(bytes) => serde_json::from_slice(&bytes)?, None => BTreeMap::new() };
            serde_json::json!({"values":values})
        }
        _ => bail!("unknown game-data command: {command}"),
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
