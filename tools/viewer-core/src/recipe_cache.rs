use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};


use crate::runtime_paths;

const MANIFEST_FILE: &str = "manifest.json";
const RECIPES_FILE: &str = "recipes.json";
const ITEMS_FILE: &str = "items.json";
const BLOCKS_FILE: &str = "blocks.json";
const CURRENT_SCHEMA_VERSION: u32 = 1;
const SOURCE_LICENSE: &str = "Minecraft game data from Mojang client jar";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeCacheStatus {
    Available,
    Missing,
    Stale,
    Corrupted,
    VersionMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeCacheStatusOutput {
    pub status: RecipeCacheStatus,
    pub minecraft_version: String,
    pub cache_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub has_recipes: bool,
    pub has_items: bool,
    pub has_blocks: bool,
    pub source: Option<String>,
    pub fetched_at: Option<u64>,
    pub schema_version: Option<u32>,
    pub hash: Option<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeCacheManifest {
    pub schema_version: u32,
    pub minecraft_version: String,
    pub source: String,
    pub license: String,
    pub fetched_at: u64,
    pub hash: String,
    pub recipes_file: String,
    pub items_file: String,
    pub blocks_file: String,
    pub recipe_count: usize,
    pub item_count: usize,
    pub block_count: usize,
}

#[derive(Debug, Clone)]
pub struct RecipeCachePayload {
    pub source: String,
    pub license: String,
    pub recipes: BTreeMap<String, Value>,
    pub items: Vec<String>,
    pub blocks: Vec<String>,
}

pub trait RecipeProvider {
    fn fetch(&self, minecraft_version: &str) -> Result<RecipeCachePayload>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SelectedRecipeIndex {
    schema: u32,
    content_id: String,
    game_version: String,
    recipes: BTreeMap<String, Value>,
    tags: BTreeMap<String, Vec<String>>,
    items: BTreeSet<String>,
}

fn selected_index() -> Result<std::sync::Arc<SelectedRecipeIndex>> {
    use std::sync::{Arc, Mutex, OnceLock};
    static INDEX: OnceLock<Mutex<BTreeMap<String, Arc<SelectedRecipeIndex>>>> = OnceLock::new();
    let context = crate::game_data::active_context()?;
    let cache = INDEX.get_or_init(Default::default);
    if let Some(index) = cache.lock().map_err(|_| anyhow!("game recipe index lock poisoned"))?.get(&context.content_id).cloned() { return Ok(index); }
    let directory = runtime_paths::recipe_cache_root()?.join(&context.content_id);
    let file = directory.join("selected-index-v2.json");
    if file.is_file() {
        let index: SelectedRecipeIndex = read_json(&file)?;
        if index.schema == 2 && index.content_id == context.content_id && index.game_version == context.game_version {
            let index = Arc::new(index);
            cache.lock().map_err(|_| anyhow!("game recipe index lock poisoned"))?.insert(context.content_id, index.clone());
            return Ok(index);
        }
    }
    let mut recipes = BTreeMap::new();
    let mut raw_tags = BTreeMap::<String, Value>::new();
    let mut items = BTreeSet::new();
    for path in crate::game_data::list_resources_from(&context, "data/")? {
        let parts = path.split('/').collect::<Vec<_>>();
        if parts.len() < 4 || !path.ends_with(".json") { continue; }
        let recipe = matches!(parts[2], "recipe" | "recipes");
        let tag = parts.len() >= 5 && parts[2] == "tags" && matches!(parts[3], "item" | "items");
        if !recipe && !tag { continue; }
        let Some(bytes) = crate::game_data::read_resource_from(&context, &path)? else { bail!("selected game data entry disappeared: {path}"); };
        let value: Value = serde_json::from_slice(&bytes).with_context(|| format!("invalid selected game JSON: {path}"))?;
        let local = parts[if recipe { 3 } else { 4 }..].join("/");
        let id = format!("{}:{}", parts[1], local.trim_end_matches(".json"));
        if recipe {
            if value.get("type").and_then(Value::as_str).is_none() { bail!("recipe type missing: {id}"); }
            collect_recipe_result_items(&value, &mut items);
            if recipes.insert(id.clone(), value.clone()).is_some_and(|previous| previous != value) { bail!("conflicting recipe directories for {id}"); }
        } else if raw_tags.insert(id.clone(), value.clone()).is_some_and(|previous| previous != value) { bail!("conflicting item tag directories for {id}"); }
    }
    let mut tags = BTreeMap::new();
    for id in raw_tags.keys() { expand_tag(id, &raw_tags, &mut tags, &mut BTreeSet::new())?; }
    let index = Arc::new(SelectedRecipeIndex { schema: 2, content_id: context.content_id.clone(), game_version: context.game_version, recipes, tags, items });
    fs::create_dir_all(&directory)?;
    let temporary = directory.join(format!("selected-index-v2.{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_vec(&*index)?)?;
    if file.exists() { fs::remove_file(&file)?; }
    fs::rename(temporary, &file)?;
    cache.lock().map_err(|_| anyhow!("game recipe index lock poisoned"))?.insert(context.content_id, index.clone());
    Ok(index)
}

fn expand_tag(id: &str, raw: &BTreeMap<String, Value>, complete: &mut BTreeMap<String, Vec<String>>, visiting: &mut BTreeSet<String>) -> Result<Vec<String>> {
    if let Some(values) = complete.get(id) { return Ok(values.clone()); }
    if !visiting.insert(id.to_owned()) { bail!("item tag cycle: {id}"); }
    let value = raw.get(id).with_context(|| format!("required item tag missing: {id}"))?;
    let entries = value.get("values").and_then(Value::as_array).with_context(|| format!("item tag values missing: {id}"))?;
    let mut values = BTreeSet::new();
    for entry in entries {
        let name = entry.as_str().or_else(|| entry.get("id").and_then(Value::as_str)).with_context(|| format!("invalid item tag entry: {id}"))?;
        let required = entry.get("required").and_then(Value::as_bool).unwrap_or(true);
        if let Some(nested) = name.strip_prefix('#') {
            let nested = if nested.contains(':') { nested.to_owned() } else { format!("minecraft:{nested}") };
            if !required && !raw.contains_key(&nested) { continue; }
            values.extend(expand_tag(&nested, raw, complete, visiting)?);
        } else {
            values.insert(if name.contains(':') { name.to_owned() } else { format!("minecraft:{name}") });
        }
    }
    visiting.remove(id);
    let values = values.into_iter().collect::<Vec<_>>();
    complete.insert(id.to_owned(), values.clone());
    Ok(values)
}

pub fn load_item_tags() -> Result<BTreeMap<String, Vec<String>>> { Ok(selected_index()?.tags.clone()) }

pub fn recipe_status(_minecraft_version: &str) -> Result<RecipeCacheStatusOutput> {
    let context = crate::game_data::active_context()?;
    let index = selected_index()?;
    Ok(RecipeCacheStatusOutput {
        status: if index.recipes.is_empty() { RecipeCacheStatus::Missing } else { RecipeCacheStatus::Available },
        minecraft_version: context.game_version,
        cache_dir: runtime_paths::recipe_cache_root()?.join(&context.content_id),
        manifest_path: context.root.join("manifest.json"),
        has_recipes: !index.recipes.is_empty(), has_items: !index.items.is_empty(), has_blocks: false,
        source: Some(format!("game-data:{}", context.content_id)), fetched_at: None,
        schema_version: Some(2), hash: Some(context.content_id),
        warning: index.recipes.is_empty().then(|| "selected game data contains no recipe files".to_owned()),
    })
}

/// Refresh the derived index from the selected installed/imported data; never downloads a client JAR.
pub fn fetch_recipe_cache(minecraft_version: &str) -> Result<RecipeCacheStatusOutput> { recipe_status(minecraft_version) }

pub fn fetch_recipe_cache_with_provider(
    provider: &dyn RecipeProvider,
    minecraft_version: &str,
) -> Result<RecipeCacheStatusOutput> {
    let cache_root = runtime_paths::recipe_cache_root()?;
    let payload = provider.fetch(minecraft_version)?;
    write_recipe_cache_in(&cache_root, minecraft_version, &payload)?;
    recipe_status_in(&cache_root, minecraft_version)
}

pub fn load_available_recipe_items(_minecraft_version: &str) -> Result<Option<BTreeSet<String>>> {
    let index = selected_index()?;
    Ok((!index.recipes.is_empty()).then(|| index.items.clone()))
}

pub fn load_available_recipes(_minecraft_version: &str) -> Result<Option<BTreeMap<String, Value>>> {
    let index = selected_index()?;
    Ok((!index.recipes.is_empty()).then(|| index.recipes.clone()))
}

pub fn recipe_status_in(
    recipe_cache_root: &Path,
    minecraft_version: &str,
) -> Result<RecipeCacheStatusOutput> {
    let cache_dir = version_cache_dir(recipe_cache_root, minecraft_version);
    let manifest_path = cache_dir.join(MANIFEST_FILE);
    let recipes_path = cache_dir.join(RECIPES_FILE);
    let items_path = cache_dir.join(ITEMS_FILE);
    let blocks_path = cache_dir.join(BLOCKS_FILE);
    let has_recipes = recipes_path.is_file();
    let has_items = items_path.is_file();
    let has_blocks = blocks_path.is_file();

    if !cache_dir.is_dir() || !manifest_path.is_file() {
        return Ok(status_output(
            RecipeCacheStatus::Missing,
            minecraft_version,
            cache_dir,
            manifest_path,
            has_recipes,
            has_items,
            has_blocks,
            None,
            None,
            None,
            None,
            Some("recipe cache manifest is missing".to_string()),
        ));
    }

    let manifest = match read_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(error) => {
            return Ok(status_output(
                RecipeCacheStatus::Corrupted,
                minecraft_version,
                cache_dir,
                manifest_path,
                has_recipes,
                has_items,
                has_blocks,
                None,
                None,
                None,
                None,
                Some(format!("manifest is corrupted: {error}")),
            ));
        }
    };

    let base_output = |status, warning: Option<String>| {
        status_output(
            status,
            minecraft_version,
            cache_dir.clone(),
            manifest_path.clone(),
            has_recipes,
            has_items,
            has_blocks,
            Some(manifest.source.clone()),
            Some(manifest.fetched_at),
            Some(manifest.schema_version),
            Some(manifest.hash.clone()),
            warning,
        )
    };

    if manifest.minecraft_version != minecraft_version {
        return Ok(base_output(
            RecipeCacheStatus::VersionMismatch,
            Some(format!(
                "manifest version {} does not match requested version {}",
                manifest.minecraft_version, minecraft_version
            )),
        ));
    }
    if manifest.schema_version < CURRENT_SCHEMA_VERSION {
        return Ok(base_output(
            RecipeCacheStatus::Stale,
            Some(format!(
                "manifest schema {} is older than {}",
                manifest.schema_version, CURRENT_SCHEMA_VERSION
            )),
        ));
    }
    if manifest.schema_version > CURRENT_SCHEMA_VERSION {
        return Ok(base_output(
            RecipeCacheStatus::VersionMismatch,
            Some(format!(
                "manifest schema {} is newer than supported {}",
                manifest.schema_version, CURRENT_SCHEMA_VERSION
            )),
        ));
    }
    if !(has_recipes && has_items && has_blocks) {
        return Ok(base_output(
            RecipeCacheStatus::Missing,
            Some("one or more recipe cache payload files are missing".to_string()),
        ));
    }

    let validation = validate_payload_files(&recipes_path, &items_path, &blocks_path);
    if let Err(error) = validation {
        return Ok(base_output(
            RecipeCacheStatus::Corrupted,
            Some(format!("recipe cache payload is corrupted: {error}")),
        ));
    }

    let hash = compute_cache_hash_from_paths(&recipes_path, &items_path, &blocks_path)?;
    if hash != manifest.hash {
        return Ok(base_output(
            RecipeCacheStatus::Corrupted,
            Some("recipe cache hash does not match manifest".to_string()),
        ));
    }

    Ok(base_output(RecipeCacheStatus::Available, None))
}

pub fn write_recipe_cache_in(
    recipe_cache_root: &Path,
    minecraft_version: &str,
    payload: &RecipeCachePayload,
) -> Result<RecipeCacheManifest> {
    let cache_dir = version_cache_dir(recipe_cache_root, minecraft_version);
    fs::create_dir_all(&cache_dir).with_context(|| {
        format!(
            "create recipe cache directory failed: {}",
            cache_dir.display()
        )
    })?;

    let recipes_bytes = serde_json::to_vec_pretty(&payload.recipes)?;
    let items_bytes = serde_json::to_vec_pretty(&payload.items)?;
    let blocks_bytes = serde_json::to_vec_pretty(&payload.blocks)?;
    let hash = compute_cache_hash(&recipes_bytes, &items_bytes, &blocks_bytes);

    write_bytes(&cache_dir.join(RECIPES_FILE), &recipes_bytes)?;
    write_bytes(&cache_dir.join(ITEMS_FILE), &items_bytes)?;
    write_bytes(&cache_dir.join(BLOCKS_FILE), &blocks_bytes)?;

    let manifest = RecipeCacheManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        minecraft_version: minecraft_version.to_string(),
        source: payload.source.clone(),
        license: payload.license.clone(),
        fetched_at: current_unix_timestamp()?,
        hash,
        recipes_file: RECIPES_FILE.to_string(),
        items_file: ITEMS_FILE.to_string(),
        blocks_file: BLOCKS_FILE.to_string(),
        recipe_count: payload.recipes.len(),
        item_count: payload.items.len(),
        block_count: payload.blocks.len(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    write_bytes(&cache_dir.join(MANIFEST_FILE), &manifest_bytes)?;
    Ok(manifest)
}

fn status_output(
    status: RecipeCacheStatus,
    minecraft_version: &str,
    cache_dir: PathBuf,
    manifest_path: PathBuf,
    has_recipes: bool,
    has_items: bool,
    has_blocks: bool,
    source: Option<String>,
    fetched_at: Option<u64>,
    schema_version: Option<u32>,
    hash: Option<String>,
    warning: Option<String>,
) -> RecipeCacheStatusOutput {
    RecipeCacheStatusOutput {
        status,
        minecraft_version: minecraft_version.to_string(),
        cache_dir,
        manifest_path,
        has_recipes,
        has_items,
        has_blocks,
        source,
        fetched_at,
        schema_version,
        hash,
        warning,
    }
}

fn validate_payload_files(
    recipes_path: &Path,
    items_path: &Path,
    blocks_path: &Path,
) -> Result<()> {
    let recipes: BTreeMap<String, Value> = read_json(recipes_path)?;
    let items = read_string_vec(items_path)?;
    let blocks = read_string_vec(blocks_path)?;
    if recipes.is_empty() {
        bail!("recipes.json is empty");
    }
    if items.iter().any(|item| !is_namespaced_id(item)) {
        bail!("items.json contains non-namespaced item id");
    }
    if blocks.iter().any(|block| !is_namespaced_id(block)) {
        bail!("blocks.json contains non-namespaced block id");
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<RecipeCacheManifest> {
    read_json(path)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("open JSON failed: {}", path.display()))?;
    serde_json::from_reader(file).with_context(|| format!("parse JSON failed: {}", path.display()))
}

fn read_string_vec(path: &Path) -> Result<Vec<String>> {
    read_json(path)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file =
        File::create(path).with_context(|| format!("create file failed: {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write file failed: {}", path.display()))?;
    Ok(())
}

fn compute_cache_hash_from_paths(
    recipes_path: &Path,
    items_path: &Path,
    blocks_path: &Path,
) -> Result<String> {
    let recipes = fs::read(recipes_path)?;
    let items = fs::read(items_path)?;
    let blocks = fs::read(blocks_path)?;
    Ok(compute_cache_hash(&recipes, &items, &blocks))
}

fn compute_cache_hash(recipes: &[u8], items: &[u8], blocks: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hash_part(&mut hasher, RECIPES_FILE, recipes);
    hash_part(&mut hasher, ITEMS_FILE, items);
    hash_part(&mut hasher, BLOCKS_FILE, blocks);
    to_hex(&hasher.finalize())
}

fn hash_part(hasher: &mut Sha256, name: &str, bytes: &[u8]) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    hasher.update([0xff]);
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn recipe_id_from_entry_name(name: &str) -> Result<String> {
    let relative = name
        .strip_prefix("data/minecraft/recipe/")
        .and_then(|value| value.strip_suffix(".json"))
        .ok_or_else(|| anyhow!("invalid recipe entry path: {name}"))?;
    Ok(format!("minecraft:{}", relative.replace('/', "/")))
}

fn collect_recipe_result_items(recipe: &Value, items: &mut BTreeSet<String>) {
    if let Some(result) = recipe.get("result") {
        collect_result_value(result, items);
    }
    if let Some(results) = recipe.get("results") {
        collect_result_value(results, items);
    }
}

fn collect_result_value(value: &Value, items: &mut BTreeSet<String>) {
    match value {
        Value::String(item) => insert_namespaced_item(item, items),
        Value::Array(values) => {
            for value in values {
                collect_result_value(value, items);
            }
        }
        Value::Object(map) => {
            for key in ["id", "item"] {
                if let Some(value) = map.get(key).and_then(Value::as_str) {
                    insert_namespaced_item(value, items);
                }
            }
        }
        _ => {}
    }
}

fn insert_namespaced_item(value: &str, items: &mut BTreeSet<String>) {
    if is_namespaced_id(value) {
        items.insert(value.to_string());
    }
}

fn is_namespaced_id(value: &str) -> bool {
    let Some((namespace, local)) = value.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !local.is_empty()
        && namespace
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
        && local.chars().all(|ch| {
            ch.is_ascii_lowercase()
                || ch.is_ascii_digit()
                || ch == '_'
                || ch == '-'
                || ch == '/'
                || ch == '.'
        })
}

fn version_cache_dir(recipe_cache_root: &Path, minecraft_version: &str) -> PathBuf {
    recipe_cache_root.join(version_dir_name(minecraft_version))
}

fn version_dir_name(minecraft_version: &str) -> String {
    let safe = minecraft_version
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("minecraft_{safe}")
}

fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn recipe_cache_missing_reports_missing() {
        let root = temp_root("missing");
        let status = recipe_status_in(&root, "1.21.10").expect("status");
        assert_eq!(status.status, RecipeCacheStatus::Missing);
        assert!(!status.has_recipes);
        assert!(status.warning.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recipe_cache_manifest_corrupted_reports_corrupted() {
        let root = temp_root("corrupted");
        let dir = version_cache_dir(&root, "1.21.10");
        fs::create_dir_all(&dir).expect("dir");
        fs::write(dir.join(MANIFEST_FILE), b"{not json").expect("manifest");
        let status = recipe_status_in(&root, "1.21.10").expect("status");
        assert_eq!(status.status, RecipeCacheStatus::Corrupted);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recipe_cache_version_mismatch_reports_version_mismatch() {
        let root = temp_root("version_mismatch");
        write_fixture_cache(&root, "1.21.10").expect("write cache");
        let dir = version_cache_dir(&root, "1.21.10");
        let mut manifest: RecipeCacheManifest =
            read_json(&dir.join(MANIFEST_FILE)).expect("manifest");
        manifest.minecraft_version = "1.20.6".to_string();
        fs::write(
            dir.join(MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest).expect("manifest bytes"),
        )
        .expect("rewrite manifest");
        let status = recipe_status_in(&root, "1.21.10").expect("status");
        assert_eq!(status.status, RecipeCacheStatus::VersionMismatch);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recipe_cache_available_reports_available() {
        let root = temp_root("available");
        write_fixture_cache(&root, "1.21.10").expect("write cache");
        let status = recipe_status_in(&root, "1.21.10").expect("status");
        assert_eq!(status.status, RecipeCacheStatus::Available);
        assert!(status.has_recipes);
        assert!(status.has_items);
        assert!(status.has_blocks);
        let _ = fs::remove_dir_all(root);
    }

    fn write_fixture_cache(root: &Path, version: &str) -> Result<()> {
        let mut recipe = serde_json::Map::new();
        recipe.insert(
            "result".to_string(),
            Value::Object(serde_json::Map::from_iter([(
                "id".to_string(),
                Value::String("minecraft:stone".to_string()),
            )])),
        );
        let mut recipes = BTreeMap::new();
        recipes.insert("minecraft:stone".to_string(), Value::Object(recipe));
        let payload = RecipeCachePayload {
            source: "test".to_string(),
            license: "test".to_string(),
            recipes,
            items: vec!["minecraft:stone".to_string()],
            blocks: vec!["minecraft:stone".to_string()],
        };
        write_recipe_cache_in(root, version, &payload)?;
        Ok(())
    }

    fn temp_root(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "lba_recipe_cache_{name}_{}_{}",
            std::process::id(),
            id
        ))
    }
}
