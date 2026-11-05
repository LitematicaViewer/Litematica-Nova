//! Mod JAR resources and externally captured registry data share the selected,
//! immutable game-data context. No Java code is loaded or executed here.
use super::*;
use serde_json::{Value, json};

const PROFILE: &str = "derived/mods.json";
const MAX_RESOURCE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub loader: String,
    pub minecraft: Vec<String>,
    pub dependencies: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ImportedMod {
    id: String,
    jar_sha256: String,
    mods: Vec<ModMetadata>,
    enabled: bool,
    resources: usize,
    candidates: Vec<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Profile {
    base_id: String,
    base_content_id: String,
    mods: Vec<ImportedMod>,
    #[serde(default)]
    snapshot_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotIdentity {
    schema_version: u32,
    game_version: String,
    /// Every enabled mod and exact source JAR, not just the names found in assets.
    mods: BTreeMap<String, SnapshotMod>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SnapshotMod {
    version: String,
    jar_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModBlockInfo {
    pub block_id: String,
    pub source_name: String,
    pub item_id: Option<String>,
    pub default_state: String,
    pub properties: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModTintRecipe {
    Constant { argb: u32 },
    Grass,
    Foliage,
    DryFoliage,
    Water,
    White,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModTint {
    pub recipe: ModTintRecipe,
    pub plains_argb: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModTints {
    schema_version: u32,
    game_version: String,
    tints: BTreeMap<String, BTreeMap<String, ModTint>>,
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn read_entry(archive: &mut zip::ZipArchive<File>, name: &str) -> Result<Option<String>> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        entry.size() <= 4 * 1024 * 1024,
        "Mod metadata is too large: {name}"
    );
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    Ok(Some(text))
}
fn constraints(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}
fn metadata(archive: &mut zip::ZipArchive<File>) -> Result<Vec<ModMetadata>> {
    if let Some(text) = read_entry(archive, "fabric.mod.json")? {
        let value: Value = serde_json::from_str(&text).context("fabric.mod.json 无效")?;
        let id = value["id"]
            .as_str()
            .context("Fabric Mod 缺少 id")?
            .to_owned();
        ensure!(valid_id(&id) && id != "minecraft", "Mod ID 无效：{id}");
        let dependencies = value["depends"]
            .as_object()
            .map(|entries| {
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), constraints(Some(value)).join(" || ")))
                    .collect()
            })
            .unwrap_or_default();
        return Ok(vec![ModMetadata {
            name: value["name"].as_str().unwrap_or(&id).to_owned(),
            id,
            version: value["version"]
                .as_str()
                .context("Fabric Mod 缺少 version")?
                .to_owned(),
            loader: "fabric".into(),
            minecraft: constraints(value["depends"].get("minecraft")),
            dependencies,
        }]);
    }
    for (path, loader) in [
        ("META-INF/neoforge.mods.toml", "neoforge"),
        ("META-INF/mods.toml", "forge"),
    ] {
        let Some(text) = read_entry(archive, path)? else {
            continue;
        };
        let value: toml::Value = toml::from_str(&text).with_context(|| format!("{path} 无效"))?;
        let jar_version = read_entry(archive, "META-INF/MANIFEST.MF")?.and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("Implementation-Version: ")
                    .map(str::to_owned)
            })
        });
        let mut mods = Vec::new();
        for entry in value
            .get("mods")
            .and_then(toml::Value::as_array)
            .context("Mod TOML 缺少 [[mods]]")?
        {
            let id = entry
                .get("modId")
                .and_then(toml::Value::as_str)
                .context("Mod TOML 缺少 modId")?
                .to_owned();
            ensure!(valid_id(&id) && id != "minecraft", "Mod ID 无效：{id}");
            let raw_version = entry
                .get("version")
                .and_then(toml::Value::as_str)
                .context("Mod TOML 缺少 version")?;
            let version = if raw_version == "${file.jarVersion}" {
                jar_version
                    .clone()
                    .context("Mod JAR 未提供 Implementation-Version")?
            } else {
                raw_version.to_owned()
            };
            let mut dependencies = BTreeMap::new();
            if let Some(entries) = value
                .get("dependencies")
                .and_then(|value| value.get(&id))
                .and_then(toml::Value::as_array)
            {
                for dependency in entries {
                    let dep_id = dependency
                        .get("modId")
                        .and_then(toml::Value::as_str)
                        .context("Mod dependency 缺少 modId")?;
                    let range = dependency
                        .get("versionRange")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("*");
                    let required = dependency
                        .get("mandatory")
                        .and_then(toml::Value::as_bool)
                        .unwrap_or_else(|| {
                            dependency.get("type").and_then(toml::Value::as_str) != Some("optional")
                        });
                    if required {
                        dependencies.insert(dep_id.to_owned(), range.to_owned());
                    }
                }
            }
            mods.push(ModMetadata {
                name: entry
                    .get("displayName")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(&id)
                    .to_owned(),
                id,
                version,
                loader: loader.into(),
                minecraft: dependencies.get("minecraft").cloned().into_iter().collect(),
                dependencies,
            });
        }
        ensure!(!mods.is_empty(), "Mod TOML 未声明任何 Mod");
        return Ok(mods);
    }
    bail!("JAR 缺少 Fabric / Forge / NeoForge 元数据；未执行其中的 Java 代码")
}
// Common Fabric predicates and Forge Maven intervals are handled without
// running a loader. Unrecognised predicates need the exact captured snapshot.
fn version_matches(version: &str, constraint: &str) -> Option<bool> {
    fn parts(value: &str) -> Option<Vec<u64>> {
        value
            .trim()
            .split('.')
            .map(str::parse)
            .collect::<std::result::Result<Vec<_>, _>>()
            .ok()
    }
    fn cmp(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
        for i in 0..a.len().max(b.len()) {
            let order = a
                .get(i)
                .copied()
                .unwrap_or(0)
                .cmp(&b.get(i).copied().unwrap_or(0));
            if !order.is_eq() {
                return order;
            }
        }
        std::cmp::Ordering::Equal
    }
    let constraint = constraint.trim();
    if constraint.is_empty() || constraint == "*" {
        return Some(true);
    }
    if constraint.contains("||") {
        let values: Vec<_> = constraint
            .split("||")
            .map(|part| version_matches(version, part))
            .collect();
        return if values.contains(&Some(true)) {
            Some(true)
        } else if values.contains(&None) {
            None
        } else {
            Some(false)
        };
    }
    let actual = parts(version)?;
    if (constraint.starts_with('[') || constraint.starts_with('('))
        && (constraint.ends_with(']') || constraint.ends_with(')'))
    {
        let inner = &constraint[1..constraint.len() - 1];
        if let Some((low, high)) = inner.split_once(',') {
            let lower = if low.trim().is_empty() {
                true
            } else {
                let order = cmp(&actual, &parts(low)?);
                order.is_gt() || (order.is_eq() && constraint.starts_with('['))
            };
            let upper = if high.trim().is_empty() {
                true
            } else {
                let order = cmp(&actual, &parts(high)?);
                order.is_lt() || (order.is_eq() && constraint.ends_with(']'))
            };
            return Some(lower && upper);
        }
        return Some(cmp(&actual, &parts(inner)?).is_eq());
    }
    let mut result = true;
    for predicate in constraint.split_whitespace() {
        if let Some(prefix) = predicate
            .strip_suffix(".*")
            .or_else(|| predicate.strip_suffix(".x"))
        {
            let wanted = parts(prefix)?;
            result &= actual.starts_with(&wanted);
            continue;
        }
        let (operator, number) = [">=", "<=", ">", "<", "=", "~", "^"]
            .into_iter()
            .find_map(|op| predicate.strip_prefix(op).map(|rest| (op, rest)))
            .unwrap_or(("=", predicate));
        let wanted = parts(number)?;
        let order = cmp(&actual, &wanted);
        result &= match operator {
            ">=" => !order.is_lt(),
            "<=" => !order.is_gt(),
            ">" => order.is_gt(),
            "<" => order.is_lt(),
            "=" => order.is_eq(),
            "~" => {
                !order.is_lt() && actual.first() == wanted.first() && actual.get(1) == wanted.get(1)
            }
            "^" => !order.is_lt() && actual.first() == wanted.first(),
            _ => return None,
        };
    }
    Some(result)
}
fn profile(context: &GameDataContext) -> Result<Profile> {
    match read_resource_from(context, PROFILE)? {
        Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
        None => Ok(Profile {
            base_id: context.id.clone(),
            base_content_id: context.content_id.clone(),
            ..Default::default()
        }),
    }
}
fn base_context(profile: &Profile, store: &Path, default: &Path) -> Result<GameDataContext> {
    let builtin = package_context(default)?;
    let base = if builtin.id == profile.base_id {
        builtin
    } else {
        package_context(&store.join("packages").join(&profile.base_id))?
    };
    ensure!(
        base.content_id == profile.base_content_id,
        "Mod 上下文的原版资源身份已变化，请重新导入"
    );
    Ok(base)
}
fn enabled_mods(profile: &Profile) -> Result<BTreeMap<String, SnapshotMod>> {
    let mut mods = BTreeMap::new();
    for entry in profile.mods.iter().filter(|entry| entry.enabled) {
        for metadata in &entry.mods {
            ensure!(
                mods.insert(
                    metadata.id.clone(),
                    SnapshotMod {
                        version: metadata.version.clone(),
                        jar_sha256: entry.jar_sha256.clone()
                    }
                )
                .is_none(),
                "多个已启用 JAR 注册了相同 Mod ID：{}",
                metadata.id
            );
        }
    }
    Ok(mods)
}
fn snapshot_matches(profile: &Profile, version: &str, store: &Path) -> Result<bool> {
    let Some(id) = &profile.snapshot_id else {
        return Ok(false);
    };
    ensure!(valid_id(id), "invalid Mod snapshot identity");
    let snapshot: SnapshotIdentity = read_json(
        &store
            .join("mod-snapshots")
            .join(id)
            .join("mod-registry.json"),
    )?;
    Ok(snapshot.schema_version == 1
        && snapshot.game_version == version
        && snapshot.mods == enabled_mods(profile)?)
}
fn replace_file(
    root: &Path,
    relative: &str,
    input: impl Read,
    index: &mut BTreeMap<String, ResourceRecord>,
) -> Result<()> {
    let relative = normalized(relative)?;
    let target = root.join(&relative);
    // Unlink before writing: inherited immutable files can be hardlinks.
    if target.exists() {
        fs::remove_file(&target)?;
    }
    record_file(root, &relative, input, index)
}
fn copy_indexed(
    source: &Path,
    root: &Path,
    records: &BTreeMap<String, ResourceRecord>,
    index: &mut BTreeMap<String, ResourceRecord>,
    mut overrides: Option<&mut BTreeMap<String, PathBuf>>,
) -> Result<()> {
    let referenced_oracle = source.join(version_adapter_store::REFERENCE).is_file();
    let bundled_adapters = overrides.is_none() && records.contains_key("derived/version-adapters/index.json")
        && records.contains_key("derived/version-adapters/shared.zip");
    for (relative, record) in records {
        let relative = normalized(relative)?;
        // A Mod profile pins the originating version map, not a second 193 MiB
        // archive (hardlinks would otherwise become full copies across volumes).
        if bundled_adapters && relative == "derived/version-adapters/index.json" {
            replace_file(root, version_adapter_store::INHERITED_INDEX, File::open(source.join(&relative))?, index)?;
            continue;
        }
        if bundled_adapters && relative == "derived/version-adapters/shared.zip" { continue; }
        if referenced_oracle && version_adapter_store::virtual_oracle(&relative) {
            index.insert(relative, record.clone());
            continue;
        }
        let source_file = source.join(&relative);
        if resource_path(&relative) {
            if let Some(paths) = overrides.as_deref_mut() {
                ensure!(!fs::symlink_metadata(&source_file)?.file_type().is_symlink(), "Mod resource symlinks are not admitted");
                paths.insert(relative.clone(), source_file);
            }
            index.insert(relative, record.clone());
            continue;
        }
        ensure!(
            !fs::symlink_metadata(&source_file)?.file_type().is_symlink(),
            "Mod resource symlinks are not admitted"
        );
        let destination = root.join(&relative);
        if !destination.exists() {
            fs::create_dir_all(destination.parent().context("resource parent")?)?;
            if fs::hard_link(&source_file, &destination).is_ok() {
                index.insert(relative, record.clone());
                continue;
            }
        }
        replace_file(root, &relative, File::open(source_file)?, index)?;
    }
    Ok(())
}
fn drop_derived(
    root: &Path,
    index: &mut BTreeMap<String, ResourceRecord>,
    prefixes: &[&str],
) -> Result<()> {
    let paths: Vec<_> = index
        .keys()
        .filter(|path| prefixes.iter().any(|prefix| path.starts_with(prefix)))
        .cloned()
        .collect();
    let referenced_oracle = root.join(version_adapter_store::REFERENCE).is_file();
    for path in paths {
        if !(referenced_oracle && version_adapter_store::virtual_oracle(&path)) {
            fs::remove_file(root.join(&path))?;
        }
        index.remove(&path);
    }
    Ok(())
}
fn publish(profile: &Profile, store: &Path, default: &Path) -> Result<GameDataContext> {
    let base = base_context(profile, store, default)?;
    enabled_mods(profile)?;
    let packages = store.join("packages");
    let temporary = staging(&packages)?;
    let root = &temporary.0;
    let mut index = BTreeMap::new();
    let base_index = read_json(&base.root.join("index.json"))?;
    let mut overrides = BTreeMap::new();
    copy_indexed(&base.root, root, &base_index, &mut index, None)?;
    for entry in profile.mods.iter().filter(|entry| entry.enabled) {
        let directory = store.join("mods").join(&entry.id);
        let records = read_json(&directory.join("index.json"))?;
        copy_indexed(&directory, root, &records, &mut index, Some(&mut overrides))?;
    }
    let snapshot_valid = snapshot_matches(profile, &base.game_version, store)?;
    if snapshot_valid {
        let directory = store
            .join("mod-snapshots")
            .join(profile.snapshot_id.as_ref().context("snapshot identity")?);
        let records: BTreeMap<String, ResourceRecord> = read_json(&directory.join("index.json"))?;
        drop_derived(
            root,
            &mut index,
            &[
                "derived/oracle/",
                "derived/inventory.json",
                "derived/block_tints.json",
                "derived/adapter.json",
                "derived/blockstates.json",
                "derived/enumerator/",
            ],
        )?;
        copy_indexed(&directory, root, &records, &mut index, Some(&mut overrides))?;
    }
    // Keep the base inside its archive. Mod priority is resolved per path before writing;
    // unchanged entries are copied compressed, not inflated to temporary loose files.
    let mut resources = new_resource_jar(root)?;
    let mut base_jar = base.resource_jar.is_file().then(|| open_build_jar(&base.root)).transpose()?;
    for (path, record) in index.iter().filter(|(path, _)| resource_path(path)) {
        if !overrides.contains_key(path) {
            if let Some((jar, i)) = base_jar.as_mut().and_then(|jar| jar.index_for_name(path).map(|i| (jar, i))) {
                let file = jar.by_index(i)?;
                ensure!(file.size() == record.bytes, "base resource size mismatch: {path}");
                resources.raw_copy_file(file)?;
                continue;
            }
        }
        let source = overrides.get(path).cloned().unwrap_or_else(|| base.root.join(path));
        ensure!(!fs::symlink_metadata(&source)?.file_type().is_symlink(), "Mod resource symlinks are not admitted");
        let input = File::open(&source).with_context(|| format!("missing game resource: {path}"))?;
        resources.start_file(path, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))?;
        let mut digest = Sha256::new();
        let mut input = BufReader::new(input);
        loop {
            let bytes = input.fill_buf()?;
            if bytes.is_empty() { break; }
            digest.update(bytes);
            resources.write_all(bytes)?;
            let n = bytes.len();
            input.consume(n);
        }
        ensure!(format!("{:x}", digest.finalize()) == record.sha256, "Mod resource differs from index: {path}");
    }
    resources.finish()?.flush()?;
    let mut resources = open_build_jar(root)?;
    if snapshot_valid {
        // Registry, materials and renderer all consume this same exact export.
        derive_registry_data(root, &base.game_version, Some(default), &mut index, &mut resources)?;
    }
    drop_derived(root, &mut index, &["derived/lang/"])?;
    derive_languages(root, &mut index, &mut resources)?;
    drop(resources);
    replace_file(
        root,
        PROFILE,
        serde_json::to_vec(profile)?.as_slice(),
        &mut index,
    )?;
    replace_file(
        root,
        "derived/mod-snapshot-status.json",
        serde_json::to_vec(&json!({"valid":snapshot_valid}))?.as_slice(),
        &mut index,
    )?;
    let mut hash = Sha256::new();
    hash.update(format!("nova.game-data.v{SCHEMA}\0{}\0", base.game_version));
    hash.update(serde_json::to_vec(&index)?);
    let content_id = format!("{:x}", hash.finalize());
    let context = GameDataContext {
        schema_version: SCHEMA,
        id: format!("mods-{}", &content_id[..24]),
        game_version: base.game_version.clone(),
        content_id,
        root: PathBuf::from("."),
        resource_jar: RESOURCE_JAR.into(),
        source: "imported-mods".into(),
    };
    atomic_json(&root.join("index.json"), &index)?;
    let original: PackageManifest = read_json(&base.root.join(MANIFEST))?;
    let mut missing = original.missing;
    if !snapshot_valid && profile.mods.iter().any(|entry| entry.enabled) {
        missing.push(
            "启用 Mod 的准确版本 registry/oracle 快照尚未导入；资源文件不证明方块已注册".into(),
        );
    }
    atomic_json(
        &root.join(MANIFEST),
        &PackageManifest {
            context: context.clone(),
            imported_ms: (stamp() / 1_000_000) as u64,
            source_jar_sha256: original.source_jar_sha256,
            data_version: original.data_version,
            resource_counts: [("resources".into(), index.len())].into(),
            missing,
        },
    )?;
    let target = packages.join(&context.id);
    if target.exists() {
        ensure!(
            package_context(&target)?.content_id == context.content_id,
            "immutable Mod identity conflict"
        );
    } else {
        fs::rename(root, &target)?;
    }
    let index_path = store.join("index.json");
    let mut packages_index: PackageIndex = if index_path.exists() {
        read_json(&index_path)?
    } else {
        PackageIndex {
            schema_version: SCHEMA,
            ids: Vec::new(),
        }
    };
    if !packages_index.ids.contains(&context.id) {
        packages_index.ids.push(context.id.clone());
        atomic_json(&index_path, &packages_index)?;
    }
    select_context(store, default, &context.id)
}

pub fn import_jar(jar: &Path, store: &Path, default: &Path) -> Result<Value> {
    ensure!(
        jar.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jar")),
        "请选择实际 Mod JAR"
    );
    let context = resolve_context(store, default)?;
    let mut profile = profile(&context)?;
    let mut archive = zip::ZipArchive::new(File::open(jar)?)?;
    ensure!(archive.len() <= 100_000, "Mod JAR 文件数过多");
    let mods = metadata(&mut archive)?;
    for metadata in &mods {
        if !metadata.minecraft.is_empty() {
            let checks: Vec<_> = metadata
                .minecraft
                .iter()
                .map(|range| version_matches(&context.game_version, range))
                .collect();
            ensure!(
                checks.iter().any(|value| *value != Some(false)),
                "Mod {} 要求 Minecraft {}，当前为 {}",
                metadata.id,
                metadata.minecraft.join(" || "),
                context.game_version
            );
        }
    }
    let jar_sha256 = hash_file(jar)?;
    let id = format!("mod-{}", &jar_sha256[..32]);
    let parent = store.join("mods");
    let temporary = staging(&parent)?;
    let mut index = BTreeMap::new();
    let mut candidates = BTreeSet::new();
    let mut total = 0u64;
    for position in 0..archive.len() {
        let mut entry = archive.by_index(position)?;
        if entry.is_dir() {
            continue;
        }
        ensure!(
            entry
                .unix_mode()
                .is_none_or(|mode| mode & 0o170000 != 0o120000),
            "Mod JAR symlinks are not admitted"
        );
        let relative = normalized(entry.name())?;
        if !resource_path(&relative) {
            continue;
        }
        total = total
            .checked_add(entry.size())
            .context("Mod JAR size overflow")?;
        ensure!(total <= MAX_RESOURCE_BYTES, "Mod JAR 解压资源超过 1 GiB");
        ensure!(
            !index.contains_key(&relative),
            "Mod JAR 包含重复资源：{relative}"
        );
        if let Some((namespace, rest)) = relative
            .strip_prefix("assets/")
            .and_then(|value| value.split_once('/'))
        {
            if let Some(name) = rest
                .strip_prefix("blockstates/")
                .and_then(|value| value.strip_suffix(".json"))
            {
                candidates.insert(format!("{namespace}:{name}"));
            }
        }
        record_file(&temporary.0, &relative, &mut entry, &mut index)?;
    }
    let entry = ImportedMod {
        id: id.clone(),
        jar_sha256,
        mods,
        enabled: true,
        resources: index.len(),
        candidates: candidates.into_iter().collect(),
    };
    atomic_json(&temporary.0.join("index.json"), &index)?;
    atomic_json(&temporary.0.join("metadata.json"), &entry)?;
    let destination = parent.join(&id);
    if !destination.exists() {
        fs::rename(&temporary.0, &destination)?;
    }
    // Reimport/update replaces the same Mod IDs in place, retaining priority.
    let ids: BTreeSet<_> = entry
        .mods
        .iter()
        .map(|metadata| metadata.id.as_str())
        .collect();
    if let Some(position) = profile.mods.iter().position(|old| {
        old.mods
            .iter()
            .any(|metadata| ids.contains(metadata.id.as_str()))
    }) {
        let old_ids: BTreeSet<_> = profile.mods[position]
            .mods
            .iter()
            .map(|metadata| metadata.id.as_str())
            .collect();
        ensure!(
            old_ids == ids,
            "此多 Mod JAR 的更新改变了注册 Mod 集合，请先移除原导入包"
        );
        profile.mods[position] = entry;
    } else {
        profile.mods.push(entry);
    }
    publish(&profile, store, default)?;
    list(store, default)
}

/// A controlled import uses the existing version adapter and headless oracle
/// schema, plus mod-registry.json binding the export to exact JAR inputs.
pub fn import_snapshot(path: &Path, store: &Path, default: &Path) -> Result<Value> {
    ensure!(
        path.file_name()
            .is_some_and(|name| name == "mod-registry.json"),
        "请选择受控导出的 mod-registry.json（同目录需有 adapter.json、inventory.json 和 oracle/）"
    );
    let context = resolve_context(store, default)?;
    let mut profile = profile(&context)?;
    let identity: SnapshotIdentity = read_json(path)?;
    ensure!(
        identity.schema_version == 1 && identity.game_version == context.game_version,
        "Mod 快照版本与当前游戏数据不一致"
    );
    ensure!(
        !identity.mods.is_empty() && identity.mods == enabled_mods(&profile)?,
        "Mod 快照的 ID / 版本 / JAR SHA256 必须与全部已启用 JAR 一致"
    );
    let source = path.parent().context("snapshot parent")?;
    let adapter: Value = read_json(&source.join("adapter.json"))?;
    ensure!(
        adapter["game_version"].as_str() == Some(context.game_version.as_str()),
        "Mod adapter 游戏版本不一致"
    );
    let files = adapter["files"]
        .as_object()
        .context("Mod 快照 adapter 缺少内容索引")?;
    let parent = store.join("mod-snapshots");
    let temporary = staging(&parent)?;
    let mut index = BTreeMap::new();
    let mut total = 0u64;
    for (relative, expected) in files {
        let relative = normalized(relative)?;
        ensure!(
            relative == "inventory.json"
                || relative == "block_tints.json"
                || relative.starts_with("oracle/"),
            "Mod 快照只接收 inventory.json、block_tints.json 和 oracle 数据：{relative}"
        );
        ensure!(
            relative.ends_with(".json") || relative.ends_with(".jsonl"),
            "Mod 快照包含非数据文件：{relative}"
        );
        let file = source.join(&relative).canonicalize()?;
        ensure!(
            file.starts_with(source.canonicalize()?),
            "Mod snapshot escapes its directory"
        );
        total = total
            .checked_add(fs::metadata(&file)?.len())
            .context("Mod snapshot size overflow")?;
        ensure!(total <= MAX_RESOURCE_BYTES, "Mod 快照超过 1 GiB");
        record_file(
            &temporary.0,
            &format!("derived/{relative}"),
            File::open(file)?,
            &mut index,
        )?;
        let actual = &index[&format!("derived/{relative}")];
        ensure!(
            expected["bytes"].as_u64() == Some(actual.bytes)
                && expected["sha256"].as_str() == Some(actual.sha256.as_str()),
            "Mod 快照内容校验失败：{relative}"
        );
    }
    record_file(
        &temporary.0,
        "derived/adapter.json",
        serde_json::to_vec(&adapter)?.as_slice(),
        &mut index,
    )?;
    validate_adapter(&temporary.0, "derived", &context.game_version, &index, true)?;
    for required in [
        "inventory.json",
        "oracle/oracle-manifest.json",
        "oracle/block_states.jsonl",
        "oracle/block_state_semantics.jsonl",
        "oracle/face_occlusion_shapes.jsonl",
        "oracle/skip_render_rows.jsonl",
        "oracle/fluid_states.jsonl",
        "oracle/block_entity_types.jsonl",
        "oracle/builtin_registries.jsonl",
    ] {
        ensure!(
            index.contains_key(&format!("derived/{required}")),
            "Mod 快照缺少真实渲染数据：{required}"
        );
    }
    let inventory: Value = read_json(&temporary.0.join("derived/inventory.json"))?;
    let mut registered = BTreeSet::new();
    let mut states = BTreeSet::new();
    oracle_rows(&temporary.0, "block_states.jsonl", None, |row| {
        let id = row["block_id"]
            .as_str()
            .context("Mod oracle block_id 缺失")?;
        ensure!(
            inventory["blocks"].get(id).is_some(),
            "Mod inventory 未提供方块的实际放置身份：{id}"
        );
        registered.insert(id.to_owned());
        let state = row["state_id"]
            .as_str()
            .context("Mod oracle state_id 缺失")?;
        ensure!(
            states.insert(state.to_owned()),
            "Mod oracle 状态重复：{state}"
        );
        let item = inventory["blocks"][id]["item_id"]
            .as_str()
            .context("Mod 方块缺少 Block.asItem 结果")?;
        ensure!(
            item == "minecraft:air" || inventory["items"].get(item).is_some(),
            "{id} 的物品 {item} 不在 inventory 注册表中"
        );
        Ok(())
    })?;
    let mut semantic_states = BTreeSet::new();
    oracle_rows(&temporary.0, "block_state_semantics.jsonl", None, |row| {
        let state = row["state_id"]
            .as_str()
            .context("Mod semantic state_id 缺失")?;
        ensure!(
            semantic_states.insert(state.to_owned()),
            "Mod 语义状态重复：{state}"
        );
        Ok(())
    })?;
    ensure!(
        states == semantic_states,
        "Mod block_states 与 block_state_semantics 未覆盖同一状态全集"
    );
    if index.contains_key("derived/block_tints.json") {
        let tints: ModTints = read_json(&temporary.0.join("derived/block_tints.json"))?;
        ensure!(
            tints.schema_version == 1 && tints.game_version == context.game_version,
            "Mod 着色数据版本不一致"
        );
        for (state, records) in tints.tints {
            ensure!(states.contains(&state), "Mod 着色状态未注册：{state}");
            for (index, tint) in records {
                ensure!(index.parse::<u32>().is_ok(), "Mod 着色索引无效：{index}");
                if let ModTintRecipe::Constant { argb } = tint.recipe {
                    ensure!(
                        argb == tint.plains_argb,
                        "常量着色与记录结果不一致：{state}"
                    );
                }
            }
        }
    }
    // A full runtime export includes vanilla records as well as added IDs; a
    // partial Mod-only oracle must never replace the baseline world semantics.
    let base = base_context(&profile, store, default)?;
    let base_blocks: Value = read_json(&base.root.join("derived/blockstates.json"))?;
    for id in base_blocks["blocks"]
        .as_object()
        .context("base registry missing")?
        .keys()
    {
        ensure!(registered.contains(id), "Mod oracle 缺少原版方块：{id}");
    }
    let provenance = adapter["oracle_identity"]
        .as_object()
        .context("Mod 快照缺少 oracle_identity")?;
    let manifest_hash = &index["derived/oracle/oracle-manifest.json"].sha256;
    ensure!(
        provenance.get("manifest_sha256").and_then(Value::as_str) == Some(manifest_hash.as_str()),
        "Mod oracle manifest identity 不匹配"
    );
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&identity)?);
    hash.update(serde_json::to_vec(&index)?);
    let id = format!("snapshot-{:x}", hash.finalize());
    atomic_json(&temporary.0.join("mod-registry.json"), &identity)?;
    atomic_json(&temporary.0.join("index.json"), &index)?;
    let target = parent.join(&id);
    if !target.exists() {
        fs::rename(&temporary.0, &target)?;
    }
    profile.snapshot_id = Some(id);
    publish(&profile, store, default)?;
    list(store, default)
}
pub fn set_enabled(id: &str, enabled: bool, store: &Path, default: &Path) -> Result<Value> {
    let mut profile = profile(&resolve_context(store, default)?)?;
    profile
        .mods
        .iter_mut()
        .find(|entry| entry.id == id)
        .context("Mod 导入包不存在")?
        .enabled = enabled;
    publish(&profile, store, default)?;
    list(store, default)
}
pub fn remove(id: &str, store: &Path, default: &Path) -> Result<Value> {
    let mut profile = profile(&resolve_context(store, default)?)?;
    ensure!(
        profile.mods.iter().any(|entry| entry.id == id),
        "Mod 导入包不存在"
    );
    profile.mods.retain(|entry| entry.id != id);
    // Keep immutable payloads referenced by running render snapshots. Removing
    // a selection must not unlink resources an existing task is still using.
    publish(&profile, store, default)?;
    list(store, default)
}
pub fn list(store: &Path, default: &Path) -> Result<Value> {
    let context = resolve_context(store, default)?;
    let profile = profile(&context)?;
    let valid = snapshot_matches(&profile, &context.game_version, store)?;
    let registry: Value = read_resource_from(&context, "derived/blockstates.json")?
        .map(|bytes| serde_json::from_slice(&bytes))
        .transpose()?
        .unwrap_or(Value::Null);
    let mods:Vec<_> = profile.mods.iter().enumerate().map(|(priority,entry)| {
        let namespaces:BTreeSet<_> = entry.mods.iter().map(|metadata|metadata.id.as_str()).collect();
        let registered = if valid && entry.enabled { registry["blocks"].as_object().map(|blocks|blocks.keys().filter(|id|id.split_once(':').is_some_and(|(namespace,_)|namespaces.contains(namespace))).cloned().collect::<Vec<_>>()).unwrap_or_default() } else {Vec::new()};
        let unresolved:Vec<_> = entry.candidates.iter().filter(|id| !registered.contains(id)).map(|id|json!({"id":id,"reason":if !entry.enabled {"disabled"} else if !valid {"registry_oracle_snapshot_required"} else {"asset_not_registered_in_snapshot"}})).collect();
        json!({"id":entry.id,"mods":entry.mods,"jarSha256":entry.jar_sha256,"enabled":entry.enabled,"priority":priority,"resourceCount":entry.resources,"registeredBlocks":registered,"unresolved":unresolved,"status":if !entry.enabled {"disabled"} else if valid {"registry_verified"} else {"resources_only"}})
    }).collect();
    Ok(
        json!({"context":context,"mods":mods,"snapshotValid":valid,"snapshotFormat":"mod-registry.json + existing version adapter.json / inventory.json / oracle/"}),
    )
}

#[derive(Default)]
struct BlockIndex {
    sources: BTreeMap<String, String>,
    blocks: BTreeMap<String, ModBlockInfo>,
    tints: BTreeMap<String, BTreeMap<String, ModTint>>,
    valid: bool,
}
fn block_index(context: &GameDataContext) -> Result<std::sync::Arc<BlockIndex>> {
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<BTreeMap<String, Arc<BlockIndex>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("Mod registry cache poisoned"))?;
    if let Some(index) = cache.get(&context.content_id) {
        return Ok(index.clone());
    }
    let profile = profile(context)?;
    let sources: BTreeMap<_, _> = profile
        .mods
        .iter()
        .filter(|entry| entry.enabled)
        .flat_map(|entry| &entry.mods)
        .map(|metadata| (metadata.id.clone(), metadata.name.clone()))
        .collect();
    let status: Value = read_resource_from(context, "derived/mod-snapshot-status.json")?
        .map(|bytes| serde_json::from_slice(&bytes))
        .transpose()?
        .unwrap_or(Value::Null);
    let mut index = BlockIndex {
        sources,
        valid: status["valid"] == true,
        ..Default::default()
    };
    if index.valid {
        if let Some(bytes) = read_resource_from(context, "derived/block_tints.json")? {
            let tints: ModTints = serde_json::from_slice(&bytes)?;
            ensure!(
                tints.game_version == context.game_version && tints.schema_version == 1,
                "Mod tint version mismatch"
            );
            index.tints = tints.tints;
        }
        let registry: Value = serde_json::from_slice(
            &read_resource_from(context, "derived/blockstates.json")?
                .context("Mod registry missing")?,
        )?;
        let inventory: Value = serde_json::from_slice(
            &read_resource_from(context, "derived/inventory.json")?
                .context("Mod inventory missing")?,
        )?;
        for (id, block) in registry["blocks"]
            .as_object()
            .context("Mod registry blocks missing")?
        {
            let Some(source_name) = id
                .split_once(':')
                .and_then(|(namespace, _)| index.sources.get(namespace))
            else {
                continue;
            };
            let properties: BTreeMap<String, Vec<String>> =
                serde_json::from_value(block["properties"].clone())?;
            let defaults: BTreeMap<String, String> =
                serde_json::from_value(block["default_properties"].clone())?;
            let item = inventory["blocks"][id]["item_id"]
                .as_str()
                .with_context(|| format!("{id}：缺少实际物品映射"))?;
            if item != "minecraft:air" {
                ensure!(
                    inventory["items"].get(item).is_some(),
                    "{id}：放置物品 {item} 未注册"
                );
            }
            index.blocks.insert(
                id.clone(),
                ModBlockInfo {
                    block_id: id.clone(),
                    source_name: source_name.clone(),
                    item_id: (item != "minecraft:air").then(|| item.to_owned()),
                    default_state: state_key(id, &defaults),
                    properties,
                },
            );
        }
    }
    let index = Arc::new(index);
    if cache.len() >= 8 {
        cache.clear();
    }
    cache.insert(context.content_id.clone(), index.clone());
    Ok(index)
}
pub fn block_info(context: &GameDataContext, id: &str) -> Result<Option<ModBlockInfo>> {
    let index = block_index(context)?;
    let Some((namespace, _)) = id.split_once(':') else {
        return Ok(None);
    };
    if !index.sources.contains_key(namespace) {
        return Ok(None);
    }
    ensure!(
        index.valid,
        "{id}：缺少与当前 Mod JAR / 版本匹配的 registry/oracle 快照，资源文件不是注册证据"
    );
    Ok(Some(
        index
            .blocks
            .get(id)
            .with_context(|| format!("{id}：未在导入的真实注册快照中注册"))?
            .clone(),
    ))
}
/// Unknown Mod color providers must not silently become white.
pub fn tint(context: &GameDataContext, state: &str, index: i32) -> Result<Option<ModTint>> {
    if index < 0 {
        return Ok(None);
    }
    let data = block_index(context)?;
    let id = state.split('[').next().unwrap_or(state);
    let Some((namespace, _)) = id.split_once(':') else {
        return Ok(None);
    };
    if !data.sources.contains_key(namespace) {
        return Ok(None);
    }
    ensure!(data.valid, "{id} 缺少有效注册 / Oracle 快照");
    Ok(Some(data.tints.get(state).and_then(|values|values.get(&index.to_string())).with_context(||format!("{state} tint={index}：Mod 着色提供器未解析，请补入 block_tints.json；未退化为白色"))?.clone()))
}
pub fn validate_state(
    context: &GameDataContext,
    id: &str,
    properties: &BTreeMap<String, String>,
) -> Result<Option<ModBlockInfo>> {
    let Some(info) = block_info(context, id)? else {
        return Ok(None);
    };
    ensure!(
        info.properties.len() == properties.len()
            && info.properties.iter().all(|(key, values)| properties
                .get(key)
                .is_some_and(|value| values.contains(value))),
        "{id}：状态不在该版本 Mod 注册表中"
    );
    Ok(Some(info))
}
pub fn state_key(id: &str, properties: &BTreeMap<String, String>) -> String {
    if properties.is_empty() {
        id.to_owned()
    } else {
        format!(
            "{id}[{}]",
            properties
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}
/// Only verified Mod rows are returned; vanilla database identities remain owned
/// by the selected version's existing registry adapter.
pub fn registry_overlay(context: &GameDataContext) -> Result<Vec<ModBlockInfo>> {
    Ok(block_index(context)?.blocks.values().cloned().collect())
}

/// Read exact-version placement semantics; never substitute another release.
pub fn version_inventory(context: &GameDataContext, version: &str) -> Result<Vec<u8>> {
    if version == context.game_version {
        return read_resource_from(context, "derived/inventory.json")?
            .context("所选版本缺少物品映射");
    }
    ensure!(valid_id(version), "invalid inventory version");
    let default = default_root()?;
    for root in [&context.root, &default] {
        let versions = root.join("derived/version-adapters");
        let inherited = context.root.join(version_adapter_store::INHERITED_INDEX);
        let pin = (root == &default && inherited.is_file()).then_some(inherited.as_path());
        if pin.is_some() || version_adapter_store::Store::present(&versions) {
            if let Some(bytes) = version_adapter_store::read_selected(&versions, version, "inventory.json", pin)? {
                return Ok(bytes);
            }
            continue;
        }
        let directory = versions.join(version);
        let archive = versions.join(format!("{version}.zip"));
        let (identity, bytes): (Value, Vec<u8>) = if directory.join("adapter.json").is_file() {
            (
                read_json(&directory.join("adapter.json"))?,
                fs::read(directory.join("inventory.json"))?,
            )
        } else if archive.is_file() {
            let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
            let identity = read_archived_adapter_identity(&mut zip)?;
            let mut bytes = Vec::new();
            let mut inventory = zip.by_name("inventory.json")?;
            ensure!(inventory.size() <= 16 * 1024 * 1024, "source inventory is too large: {version}");
            inventory.read_to_end(&mut bytes)?;
            (identity, bytes)
        } else {
            continue;
        };
        let record = &identity["files"]["inventory.json"];
        ensure!(
            identity["game_version"].as_str() == Some(version)
                && record["bytes"].as_u64() == Some(bytes.len() as u64)
                && record["sha256"].as_str()
                    == Some(format!("{:x}", Sha256::digest(&bytes)).as_str()),
            "物品映射版本/内容身份不匹配：{version}"
        );
        return Ok(bytes);
    }
    bail!("{version} 缺少准确物品映射；未使用其他版本代替")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn jar(entries: &[(&str, &str)]) -> (Staging, PathBuf) {
        let directory = staging(&std::env::temp_dir()).unwrap();
        let path = directory.0.join("mod.jar");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, value) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(value.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        (directory, path)
    }
    #[test]
    fn fabric_identity_does_not_promote_asset_names_to_registry() {
        let (_directory, path) = jar(&[
            (
                "fabric.mod.json",
                r#"{"id":"sample","name":"Sample","version":"1.0","depends":{"minecraft":">=1.20 <1.21","fabricloader":">=0.15"}}"#,
            ),
            ("assets/sample/blockstates/not_registered.json", "{}"),
        ]);
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mods = metadata(&mut zip).unwrap();
        assert_eq!(mods[0].id, "sample");
        assert_eq!(mods[0].minecraft, [">=1.20 <1.21"]);
        assert_eq!(mods[0].dependencies["fabricloader"], ">=0.15");
        assert_eq!(mods.len(), 1);
    }
    #[test]
    fn forge_and_neoforge_read_real_mod_identity_and_dependencies() {
        for (name, loader) in [
            ("META-INF/mods.toml", "forge"),
            ("META-INF/neoforge.mods.toml", "neoforge"),
        ] {
            let (_directory, path) = jar(&[
                (
                    name,
                    "modLoader='javafml'\n[[mods]]\nmodId='sample'\nversion='${file.jarVersion}'\ndisplayName='Sample'\n[[dependencies.sample]]\nmodId='minecraft'\nmandatory=true\nversionRange='[1.20,1.21)'",
                ),
                ("META-INF/MANIFEST.MF", "Implementation-Version: 2.0\n"),
            ]);
            let mods =
                metadata(&mut zip::ZipArchive::new(File::open(path).unwrap()).unwrap()).unwrap();
            assert_eq!(mods[0].loader, loader);
            assert_eq!(mods[0].version, "2.0");
            assert_eq!(mods[0].minecraft, ["[1.20,1.21)"]);
        }
    }
    #[test]
    fn minecraft_ranges_are_not_a_path_or_latest_version_guess() {
        assert_eq!(version_matches("1.20.4", ">=1.20 <1.21"), Some(true));
        assert_eq!(version_matches("26.3", "[1.20,1.21)"), Some(false));
        assert_eq!(version_matches("1.20.4", "1.20.*"), Some(true));
        assert_eq!(version_matches("1.20.4", "loader_custom_predicate"), None);
    }
    #[test]
    fn import_disable_remove_preserves_base_and_unproven_registry_boundary() {
        let directory = staging(&std::env::temp_dir()).unwrap();
        let base = directory.0.join("default");
        let store = directory.0.join("store");
        fs::create_dir_all(&base).unwrap();
        let mut index = BTreeMap::new();
        let mut jar_out = new_resource_jar(&base).unwrap();
        add_resource(&mut jar_out, "assets/minecraft/lang/en_us.json",
            br#"{"block.minecraft.stone":"Stone"}"#.as_slice(), &mut index).unwrap();
        jar_out.finish().unwrap().flush().unwrap();
        record_file(&base,"derived/blockstates.json",br#"{"version_id":"1.20.4","blocks":{"minecraft:stone":{"properties":{},"default_properties":{}}}}"#.as_slice(),&mut index).unwrap();
        let mut hash = Sha256::new();
        hash.update("nova.game-data.v1\01.20.4\0");
        hash.update(serde_json::to_vec(&index).unwrap());
        let content_id = format!("{:x}", hash.finalize());
        let context = GameDataContext {
            schema_version: 1,
            id: "test-base".into(),
            game_version: "1.20.4".into(),
            content_id,
            root: ".".into(),
            resource_jar: RESOURCE_JAR.into(),
            source: "test".into(),
        };
        atomic_json(&base.join("index.json"), &index).unwrap();
        atomic_json(
            &base.join(MANIFEST),
            &PackageManifest {
                context,
                imported_ms: 0,
                source_jar_sha256: "0".repeat(64),
                data_version: Some(3700),
                resource_counts: BTreeMap::new(),
                missing: Vec::new(),
            },
        )
        .unwrap();
        let (_jar_dir, path) = jar(&[
            (
                "fabric.mod.json",
                r#"{"id":"sample","version":"1.0","depends":{"minecraft":"1.20.*"}}"#,
            ),
            ("assets/sample/blockstates/lamp.json", "{}"),
            (
                "assets/sample/lang/en_us.json",
                r#"{"block.sample.lamp":"Lamp"}"#,
            ),
            ("sample/Registration.class", "must not be copied"),
        ]);
        let imported = import_jar(&path, &store, &base).unwrap();
        assert_eq!(imported["mods"][0]["status"], "resources_only");
        assert_eq!(imported["mods"][0]["unresolved"][0]["id"], "sample:lamp");
        let selected = resolve_context(&store, &base).unwrap();
        assert!(
            read_resource_from(&selected, "assets/sample/blockstates/lamp.json")
                .unwrap()
                .is_some()
        );
        assert!(!selected.root.join("sample/Registration.class").exists());
        assert!(!selected.root.join("assets").exists());
        let names: Value = serde_json::from_slice(&read_resource_from(&selected, "derived/lang/en_us.json").unwrap().unwrap()).unwrap();
        assert_eq!(names["block.minecraft.stone"], "Stone");
        assert_eq!(names["block.sample.lamp"], "Lamp");
        assert!(
            block_info(&selected, "sample:lamp")
                .unwrap_err()
                .to_string()
                .contains("快照")
        );
        assert!(!base.join("assets/sample").exists());
        let id = imported["mods"][0]["id"].as_str().unwrap();
        set_enabled(id, false, &store, &base).unwrap();
        let disabled = resolve_context(&store, &base).unwrap();
        assert!(
            read_resource_from(&disabled, "assets/sample/blockstates/lamp.json")
                .unwrap()
                .is_none()
        );
        assert!(
            read_resource_from(&selected, "assets/sample/blockstates/lamp.json")
                .unwrap()
                .is_some()
        );
        let removed = remove(id, &store, &base).unwrap();
        assert_eq!(removed["mods"].as_array().unwrap().len(), 0);
    }
    #[test]
    fn state_identity_keeps_every_property() {
        assert_eq!(
            state_key(
                "sample:lamp",
                &BTreeMap::from([
                    ("facing".into(), "west".into()),
                    ("lit".into(), "true".into())
                ])
            ),
            "sample:lamp[facing=west,lit=true]"
        );
    }
}
