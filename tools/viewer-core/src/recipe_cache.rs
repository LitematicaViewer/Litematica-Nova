use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::runtime_paths;

const MANIFEST_FILE: &str = "manifest.json";
const RECIPES_FILE: &str = "recipes.json";
const ITEMS_FILE: &str = "items.json";
const BLOCKS_FILE: &str = "blocks.json";
const CURRENT_SCHEMA_VERSION: u32 = 1;
const MOJANG_VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const SOURCE_NAME: &str = "mojang_piston_meta_client_jar";
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

pub struct MojangClientJarRecipeProvider {
    client: reqwest::blocking::Client,
}

impl MojangClientJarRecipeProvider {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("Litematica-BA recipe-cache/0.1")
            .build()
            .context("build recipe cache HTTP client failed")?;
        Ok(Self { client })
    }
}

impl RecipeProvider for MojangClientJarRecipeProvider {
    fn fetch(&self, minecraft_version: &str) -> Result<RecipeCachePayload> {
        let manifest: Value = self.get_json(MOJANG_VERSION_MANIFEST_URL)?;
        let version_url = manifest
            .get("versions")
            .and_then(Value::as_array)
            .and_then(|versions| {
                versions.iter().find_map(|version| {
                    let id = version.get("id")?.as_str()?;
                    if id == minecraft_version {
                        version.get("url")?.as_str()
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| {
                anyhow!("minecraft version not found in Mojang manifest: {minecraft_version}")
            })?;

        let version_manifest: Value = self.get_json(version_url)?;
        let client_url = version_manifest
            .pointer("/downloads/client/url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Mojang version manifest has no downloads.client.url"))?;

        let jar_bytes = self
            .client
            .get(client_url)
            .send()
            .with_context(|| format!("download Minecraft client jar failed: {client_url}"))?
            .error_for_status()
            .with_context(|| format!("download Minecraft client jar returned error: {client_url}"))?
            .bytes()
            .context("read Minecraft client jar bytes failed")?;

        let mut archive = ZipArchive::new(Cursor::new(jar_bytes))
            .context("open Minecraft client jar as zip failed")?;
        let mut recipes = BTreeMap::<String, Value>::new();
        let mut item_set = BTreeSet::<String>::new();

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().replace('\\', "/");
            if !name.starts_with("data/minecraft/recipe/") || !name.ends_with(".json") {
                continue;
            }
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .with_context(|| format!("read recipe entry failed: {name}"))?;
            let recipe: Value = serde_json::from_str(&content)
                .with_context(|| format!("parse recipe json failed: {name}"))?;
            let recipe_id = recipe_id_from_entry_name(&name)?;
            collect_recipe_result_items(&recipe, &mut item_set);
            recipes.insert(recipe_id, recipe);
        }

        if recipes.is_empty() {
            bail!("no vanilla recipe JSON files found in Minecraft client jar");
        }

        let block_ids = load_known_block_ids().unwrap_or_default();
        let items = item_set.iter().cloned().collect::<Vec<_>>();
        let blocks = items
            .iter()
            .filter(|item| block_ids.contains(*item))
            .cloned()
            .collect::<Vec<_>>();

        Ok(RecipeCachePayload {
            source: format!(
                "{SOURCE_NAME}; manifest={MOJANG_VERSION_MANIFEST_URL}; client={client_url}"
            ),
            license: SOURCE_LICENSE.to_string(),
            recipes,
            items,
            blocks,
        })
    }
}

impl MojangClientJarRecipeProvider {
    fn get_json(&self, url: &str) -> Result<Value> {
        self.client
            .get(url)
            .send()
            .with_context(|| format!("download JSON failed: {url}"))?
            .error_for_status()
            .with_context(|| format!("download JSON returned error: {url}"))?
            .text()
            .with_context(|| format!("read JSON response failed: {url}"))
            .and_then(|text| {
                serde_json::from_str(&text).with_context(|| format!("parse JSON failed: {url}"))
            })
    }
}

pub fn recipe_status(minecraft_version: &str) -> Result<RecipeCacheStatusOutput> {
    recipe_status_in(&runtime_paths::recipe_cache_root()?, minecraft_version)
}

pub fn fetch_recipe_cache(minecraft_version: &str) -> Result<RecipeCacheStatusOutput> {
    let provider = MojangClientJarRecipeProvider::new()?;
    fetch_recipe_cache_with_provider(&provider, minecraft_version)
}

pub fn fetch_recipe_cache_with_provider(
    provider: &dyn RecipeProvider,
    minecraft_version: &str,
) -> Result<RecipeCacheStatusOutput> {
    let cache_root = runtime_paths::recipe_cache_root()?;
    let payload = provider.fetch(minecraft_version)?;
    write_recipe_cache_in(&cache_root, minecraft_version, &payload)?;
    recipe_status_in(&cache_root, minecraft_version)
}

pub fn load_available_recipe_items(minecraft_version: &str) -> Result<Option<BTreeSet<String>>> {
    let cache_root = runtime_paths::recipe_cache_root()?;
    if recipe_status_in(&cache_root, minecraft_version)?.status != RecipeCacheStatus::Available {
        return Ok(None);
    }
    let items_path = version_cache_dir(&cache_root, minecraft_version).join(ITEMS_FILE);
    let items = read_string_vec(&items_path)?;
    Ok(Some(items.into_iter().collect()))
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

fn load_known_block_ids() -> Result<BTreeSet<String>> {
    let path = runtime_paths::app_root()?
        .join("data")
        .join("minecraft_blockstates")
        .join("26.1.json");
    let file = File::open(&path)
        .with_context(|| format!("open blockstate DB failed: {}", path.display()))?;
    let value: Value = serde_json::from_reader(file)
        .with_context(|| format!("parse blockstate DB failed: {}", path.display()))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("blockstate DB root is not an object"))?;
    Ok(object.keys().cloned().collect())
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
