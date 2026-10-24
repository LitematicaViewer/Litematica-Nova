#![cfg_attr(test, allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};



use crate::recipe_tree::RecipeTreeNode;
use crate::runtime_paths;
use crate::stockpile::StockpileMaterialsData;
use crate::stockpile_schema::STOCKPILE_ITEM_NAMES_SCHEMA_VERSION;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemNamesManifest {
    pub schema_version: u32,
    pub minecraft_version: String,
    pub source: String,
    pub fetched_at: u64,
    pub hash: String,
    pub en_us_count: usize,
    pub zh_cn_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileItemNamesPayload {
    pub schema_version: u32,
    pub status: String,
    pub manifest: Option<ItemNamesManifest>,
    pub names: BTreeMap<String, BTreeMap<String, String>>,
    pub warning: Option<String>,
}

pub fn resolve_stockpile_item_names(
    minecraft_version: &str,
    materials: &mut StockpileMaterialsData,
    recipe_trees: &BTreeMap<String, RecipeTreeNode>,
) -> Result<StockpileItemNamesPayload> {
    let cache = language_cache(minecraft_version)?;
    Ok(build_payload_from_cache(materials, recipe_trees, cache))
}

pub(crate) fn resolve_stockpile_item_names_cached(minecraft_version: &str, materials: &mut StockpileMaterialsData, recipe_trees: &BTreeMap<String, RecipeTreeNode>) -> Result<StockpileItemNamesPayload> {
    let cache = language_cache(minecraft_version)?;
    Ok(build_payload_from_cache(materials, recipe_trees, cache))
}

fn build_payload_from_cache(
    materials: &mut StockpileMaterialsData,
    recipe_trees: &BTreeMap<String, RecipeTreeNode>,
    cache: impl std::borrow::Borrow<LangCache>,
) -> StockpileItemNamesPayload {
    let cache = cache.borrow();
    let keys = collect_name_keys(materials, recipe_trees);
    let minecraft_keys = keys
        .iter().cloned()
        .filter(|key| key.split_once(':').is_some())
        .collect::<BTreeSet<_>>();
    let mut names = BTreeMap::<String, BTreeMap<String, String>>::new();
    for key in &minecraft_keys {
        let mut localized = BTreeMap::<String, String>::new();
        if let Some(value) = cache.lookup("en-US", key) {
            localized.insert("en-US".to_string(), value);
        }
        if let Some(value) = cache.lookup("zh-CN", key) {
            localized.insert("zh-CN".to_string(), value);
        }
        if !localized.is_empty() {
            names.insert(key.clone(), localized);
        }
    }
    // UI-export names include the selected language resource and configured
    // resource-pack priority. Apply only display values; IDs/counts stay intact.
    let locale = materials.ui_locale.as_deref().unwrap_or("zh-CN");
    for key in &keys {
        let Some((namespace, path)) = key.split_once(':') else { continue; };
        let path = path.replace('/', ".");
        let value = materials.ui_translations.get(&format!("block.{namespace}.{path}"))
            .or_else(|| materials.ui_translations.get(&format!("item.{namespace}.{path}")));
        if let Some(value) = value {
            names.entry(key.clone()).or_default().insert(locale.to_owned(), value.clone());
        }
    }
    for material in &mut materials.materials {
        if let Some(localized) = names.get(&material.item_icon_key) {
            material.display_names = localized.clone();
        }
    }
    let has_missing_locale = names
        .values()
        .any(|localized| !localized.contains_key("en-US") || !localized.contains_key("zh-CN"));
    let (status, warning) = if cache.manifest.is_none() {
        (
            "missing".to_string(),
            Some("item name cache is unavailable".to_string()),
        )
    } else if names.len() < minecraft_keys.len() || has_missing_locale {
        (
            "partial".to_string(),
            Some("item name cache is missing some project translations".to_string()),
        )
    } else {
        ("available".to_string(), None)
    };
    StockpileItemNamesPayload {
        schema_version: STOCKPILE_ITEM_NAMES_SCHEMA_VERSION,
        status,
        manifest: cache.manifest.clone(),
        names,
        warning,
    }
}

fn collect_name_keys(
    materials: &StockpileMaterialsData,
    recipe_trees: &BTreeMap<String, RecipeTreeNode>,
) -> BTreeSet<String> {
    let mut keys = BTreeSet::<String>::new();
    for material in &materials.materials {
        keys.insert(material.item_icon_key.clone());
        keys.insert(material.category_icon.clone());
    }
    for tree in recipe_trees.values() {
        collect_tree_keys(tree, &mut keys);
    }
    keys
}

fn collect_tree_keys(node: &RecipeTreeNode, keys: &mut BTreeSet<String>) {
    if node.item_id.contains(':') {
        keys.insert(node.item_id.clone());
    }
    if node.icon_key.contains(':') {
        keys.insert(node.icon_key.clone());
    }
    for item in &node.possible_items {if item.contains(':'){keys.insert(item.clone());}}
    for ingredient in &node.ingredients {
        if ingredient.item_id.contains(':') {
            keys.insert(ingredient.item_id.clone());
        }
        for item in &ingredient.possible_items {
            if item.contains(':') {
                keys.insert(item.clone());
            }
        }
    }
    for child in &node.children {
        collect_tree_keys(child, keys);
    }
}

#[derive(Clone)]
struct LangCache {
    manifest: Option<ItemNamesManifest>,
    en_us: BTreeMap<String, String>,
    zh_cn: BTreeMap<String, String>,
}

impl LangCache {
    fn empty() -> Self {
        Self {
            manifest: None,
            en_us: BTreeMap::new(),
            zh_cn: BTreeMap::new(),
        }
    }

    fn load_or_fetch(_minecraft_version: &str) -> Result<Self> {
        let context = crate::game_data::active_context()?;
        let root = cache_dir(&context.content_id)?;
        let manifest_path = root.join("manifest.json");
        if manifest_path.is_file() {
            let manifest: ItemNamesManifest = read_json(&manifest_path)?;
            if manifest.source == format!("game-data:{}", context.content_id) {
                return Ok(Self { manifest: Some(manifest), en_us: read_json(&root.join("en_us.json"))?, zh_cn: read_json(&root.join("zh_cn.json"))? });
            }
        }
        let mut en_us = BTreeMap::new();
        let mut zh_cn = BTreeMap::new();
        for path in crate::game_data::list_resources_from(&context, "assets/")? {
            let destination = if path.ends_with("/lang/en_us.json") { &mut en_us }
                else if path.ends_with("/lang/zh_cn.json") { &mut zh_cn } else { continue; };
            if let Some(bytes) = crate::game_data::read_resource_from(&context, &path)? {
                let values: BTreeMap<String, String> = serde_json::from_slice(&bytes)
                    .with_context(|| format!("parse selected game language failed: {path}"))?;
                destination.extend(values);
            }
        }
        for (locale, destination) in [("en_us", &mut en_us), ("zh_cn", &mut zh_cn)] {
            if let Some(bytes) = crate::game_data::read_resource_from(&context, &format!("derived/lang/{locale}.json"))? {
                let values: BTreeMap<String, String> = serde_json::from_slice(&bytes).with_context(|| format!("invalid normalized selected language: {locale}"))?;
                destination.extend(values);
            }
        }
        let manifest = manifest_for(&context.game_version, &format!("game-data:{}", context.content_id), &en_us, &zh_cn)?;
        write_cache(&context.content_id, &manifest, &en_us, &zh_cn)?;
        Ok(Self { manifest: Some(manifest), en_us, zh_cn })
    }

    fn lookup(&self, locale: &str, item_id: &str) -> Option<String> {
        let (namespace, local) = item_id.split_once(':')?;
        let map = if locale == "zh-CN" {
            &self.zh_cn
        } else {
            &self.en_us
        };
        for key in [
            format!("block.{namespace}.{}", local.replace('/', ".")),
            format!("item.{namespace}.{}", local.replace('/', ".")),
        ] {
            if let Some(value) = map.get(&key) {
                return Some(value.clone());
            }
        }
        None
    }
}

fn language_cache(version: &str) -> Result<std::sync::Arc<LangCache>> {
    use std::sync::{Arc, Mutex, OnceLock};
    static LANGUAGES: OnceLock<Mutex<BTreeMap<String, Arc<LangCache>>>> = OnceLock::new();
    let context=crate::game_data::active_context()?;
    let cache=LANGUAGES.get_or_init(Default::default);
    if let Some(value)=cache.lock().map_err(|_|anyhow!("game language cache lock poisoned"))?.get(&context.content_id).cloned() {return Ok(value);}
    let value=Arc::new(LangCache::load_or_fetch(version)?);
    let mut cache=cache.lock().map_err(|_|anyhow!("game language cache lock poisoned"))?;
    if cache.len()>=4 {cache.clear();}
    cache.insert(context.content_id,value.clone()); Ok(value)
}

pub(crate) fn display_names_for_keys<'a>(keys: impl IntoIterator<Item=&'a str>, locale: &str, overrides: &BTreeMap<String,String>) -> Result<BTreeMap<String,String>> {
    let cache=language_cache("")?; let mut names=BTreeMap::new();
    for key in keys { let Some((namespace,path))=key.split_once(':') else{continue;}; let path=path.replace('/',".");
        let value=overrides.get(&format!("block.{namespace}.{path}")).or_else(||overrides.get(&format!("item.{namespace}.{path}"))).cloned()
            .or_else(||cache.lookup(if locale.starts_with("zh"){"zh-CN"}else{"en-US"},key));
        if let Some(value)=value {names.insert(key.into(),value);}
    } Ok(names)
}

pub(crate) fn selected_name(translation_key: &str, locale: &str) -> Result<Option<String>> {
    let cache=language_cache("")?;
    Ok(if locale.starts_with("zh") {cache.zh_cn.get(translation_key)}else{cache.en_us.get(translation_key)}.cloned().or_else(||cache.en_us.get(translation_key).cloned()))
}

fn manifest_for(
    minecraft_version: &str,
    source: &str,
    en_us: &BTreeMap<String, String>,
    zh_cn: &BTreeMap<String, String>,
) -> Result<ItemNamesManifest> {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(en_us)?);
    hasher.update([0xff]);
    hasher.update(serde_json::to_vec(zh_cn)?);
    Ok(ItemNamesManifest {
        schema_version: STOCKPILE_ITEM_NAMES_SCHEMA_VERSION,
        minecraft_version: minecraft_version.to_string(),
        source: source.to_string(),
        fetched_at: current_unix_timestamp()?,
        hash: to_hex(&hasher.finalize()),
        en_us_count: en_us.len(),
        zh_cn_count: zh_cn.len(),
    })
}

fn write_cache(
    minecraft_version: &str,
    manifest: &ItemNamesManifest,
    en_us: &BTreeMap<String, String>,
    zh_cn: &BTreeMap<String, String>,
) -> Result<()> {
    let root = cache_dir(minecraft_version)?;
    fs::create_dir_all(&root)
        .with_context(|| format!("create item name cache dir failed: {}", root.display()))?;
    write_json(&root.join("manifest.json"), manifest)?;
    write_json(&root.join("en_us.json"), en_us)?;
    write_json(&root.join("zh_cn.json"), zh_cn)?;
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut file = fs::File::create(path)
        .with_context(|| format!("create JSON failed: {}", path.display()))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("write JSON failed: {}", path.display()))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let file =
        fs::File::open(path).with_context(|| format!("open JSON failed: {}", path.display()))?;
    serde_json::from_reader(std::io::BufReader::new(file)).with_context(|| format!("parse JSON failed: {}", path.display()))
}

fn cache_dir(minecraft_version: &str) -> Result<PathBuf> {
    Ok(runtime_paths::item_names_cache_root()?.join(version_dir_name(minecraft_version)))
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

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stockpile::{
        StockpileMaterialItem, StockpileMaterialsData, StockpileProjectInfo, StockpileSummary,
    };

    #[test]
    fn lookup_falls_back_when_cache_missing() {
        let cache = LangCache::empty();
        assert_eq!(cache.lookup("zh-CN", "minecraft:stone"), None);
    }

    #[test]
    fn lookup_supports_block_and_item_keys() {
        let mut cache = LangCache::empty();
        cache
            .en_us
            .insert("block.minecraft.stone".to_string(), "Stone".to_string());
        cache
            .zh_cn
            .insert("block.minecraft.stone".to_string(), "石头".to_string());
        cache.en_us.insert(
            "item.minecraft.iron_ingot".to_string(),
            "Iron Ingot".to_string(),
        );
        assert_eq!(
            cache.lookup("zh-CN", "minecraft:stone").as_deref(),
            Some("石头")
        );
        assert_eq!(
            cache.lookup("en-US", "minecraft:iron_ingot").as_deref(),
            Some("Iron Ingot")
        );
    }

    #[test]
    fn payload_marks_missing_cache_and_exports_no_names() {
        let mut materials = fixture_materials();
        let payload =
            build_payload_from_cache(&mut materials, &BTreeMap::new(), LangCache::empty());
        assert_eq!(payload.status, "missing");
        assert!(payload.warning.is_some());
        assert!(payload.names.is_empty());
        assert!(materials.materials[0].display_names.is_empty());
    }

    #[test]
    fn payload_only_exports_project_keys_and_localizes_materials() {
        let mut cache = LangCache::empty();
        cache.manifest = Some(ItemNamesManifest {
            schema_version: STOCKPILE_ITEM_NAMES_SCHEMA_VERSION,
            minecraft_version: "1.21.10".to_string(),
            source: "test".to_string(),
            fetched_at: 1,
            hash: "hash".to_string(),
            en_us_count: 3,
            zh_cn_count: 3,
        });
        cache
            .en_us
            .insert("block.minecraft.stone".to_string(), "Stone".to_string());
        cache
            .zh_cn
            .insert("block.minecraft.stone".to_string(), "石头".to_string());
        cache
            .en_us
            .insert("item.minecraft.barrier".to_string(), "Barrier".to_string());
        cache
            .zh_cn
            .insert("item.minecraft.barrier".to_string(), "屏障".to_string());
        cache
            .en_us
            .insert("item.minecraft.unused".to_string(), "Unused".to_string());
        cache
            .zh_cn
            .insert("item.minecraft.unused".to_string(), "未使用".to_string());

        let mut materials = fixture_materials();
        let payload = build_payload_from_cache(&mut materials, &BTreeMap::new(), cache);

        assert_eq!(payload.status, "available");
        assert_eq!(payload.names.len(), 2);
        assert!(payload.names.contains_key("minecraft:stone"));
        assert!(payload.names.contains_key("minecraft:barrier"));
        assert!(!payload.names.contains_key("minecraft:unused"));
        assert_eq!(
            materials.materials[0]
                .display_names
                .get("zh-CN")
                .map(String::as_str),
            Some("石头")
        );
    }

    #[test]
    fn payload_marks_partial_when_project_translation_is_incomplete() {
        let mut cache = LangCache::empty();
        cache.manifest = Some(ItemNamesManifest {
            schema_version: STOCKPILE_ITEM_NAMES_SCHEMA_VERSION,
            minecraft_version: "1.21.10".to_string(),
            source: "test".to_string(),
            fetched_at: 1,
            hash: "hash".to_string(),
            en_us_count: 1,
            zh_cn_count: 0,
        });
        cache
            .en_us
            .insert("block.minecraft.stone".to_string(), "Stone".to_string());

        let mut materials = fixture_materials();
        let payload = build_payload_from_cache(&mut materials, &BTreeMap::new(), cache);

        assert_eq!(payload.status, "partial");
        assert!(payload.warning.is_some());
        assert_eq!(
            payload.names["minecraft:stone"]
                .get("en-US")
                .map(String::as_str),
            Some("Stone")
        );
    }

    #[test]
    fn variant_task_names_use_the_base_item_icon_identity() {
        let mut materials = fixture_materials();
        materials.materials[0].namespace_id = "minecraft:stone#nbt-variant".into();
        materials.ui_translations.insert("block.minecraft.stone".into(), "石头".into());
        let payload = build_payload_from_cache(&mut materials, &BTreeMap::new(), LangCache::empty());
        assert!(payload.names.contains_key("minecraft:stone"));
        assert!(!payload.names.contains_key("minecraft:stone#nbt-variant"));
        assert_eq!(materials.materials[0].display_names.get("zh-CN").map(String::as_str), Some("石头"));
        assert_eq!(materials.materials[0].namespace_id, "minecraft:stone#nbt-variant");
    }

    fn fixture_materials() -> StockpileMaterialsData {
        StockpileMaterialsData {
            ui_locale: None,
            ui_translations: Default::default(),
            schema_version: crate::stockpile_schema::STOCKPILE_MATERIALS_SCHEMA_VERSION,
            project: StockpileProjectInfo {
                source_file: "fixture.litematic".to_string(),
                created_at: 1,
                data_version: 3953,
                regions: vec!["main".to_string()],
            },
            summary: StockpileSummary {
                total_blocks: 1,
                unique_materials: 1,
                total_stacks: 1,
                estimated_shulker_boxes: 1,
            },
            materials: vec![StockpileMaterialItem {
                id: "stone".to_string(),
                namespace_id: "minecraft:stone".to_string(),
                display_name: "Stone".to_string(),
                required_count: 1,
                stack_size: 64,
                stacks: 0,
                remainder: 1,
                shulker_boxes: 1,
                category: "Other".to_string(),
                category_icon: "minecraft:barrier".to_string(),
                item_icon_key: "minecraft:stone".to_string(),
                icon_path: String::new(),
                icon_available: false,
                icon_diagnostic: None,
                normalized_from: Vec::new(),
                display_names: BTreeMap::new(),
                source_regions: vec!["main".to_string()],
                recipe_status: "missing".to_string(),
                craft_complexity: 0,
            }],
            diagnostics: Vec::new(),
        }
    }
}
