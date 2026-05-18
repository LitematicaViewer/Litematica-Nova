#![cfg_attr(test, allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::recipe_cache::{self, RecipeCacheStatus};
use crate::recipe_tree::RecipeTreeNode;
use crate::runtime_paths;
use crate::stockpile::StockpileMaterialsData;
use crate::stockpile_schema::STOCKPILE_ITEM_NAMES_SCHEMA_VERSION;

const MOJANG_VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

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
    let cache = LangCache::load_or_fetch(minecraft_version).unwrap_or_else(|_| LangCache::empty());
    Ok(build_payload_from_cache(materials, recipe_trees, cache))
}

fn build_payload_from_cache(
    materials: &mut StockpileMaterialsData,
    recipe_trees: &BTreeMap<String, RecipeTreeNode>,
    cache: LangCache,
) -> StockpileItemNamesPayload {
    let keys = collect_name_keys(materials, recipe_trees);
    let minecraft_keys = keys
        .into_iter()
        .filter(|key| key.starts_with("minecraft:"))
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
    for material in &mut materials.materials {
        if let Some(localized) = names.get(&material.namespace_id) {
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
        manifest: cache.manifest,
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
        keys.insert(material.namespace_id.clone());
        keys.insert(material.category_icon.clone());
    }
    for tree in recipe_trees.values() {
        collect_tree_keys(tree, &mut keys);
    }
    keys
}

fn collect_tree_keys(node: &RecipeTreeNode, keys: &mut BTreeSet<String>) {
    if node.item_id.starts_with("minecraft:") {
        keys.insert(node.item_id.clone());
    }
    if node.icon_key.starts_with("minecraft:") {
        keys.insert(node.icon_key.clone());
    }
    for ingredient in &node.ingredients {
        if ingredient.item_id.starts_with("minecraft:") {
            keys.insert(ingredient.item_id.clone());
        }
        for item in &ingredient.possible_items {
            if item.starts_with("minecraft:") {
                keys.insert(item.clone());
            }
        }
    }
    for child in &node.children {
        collect_tree_keys(child, keys);
    }
}

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

    fn load_or_fetch(minecraft_version: &str) -> Result<Self> {
        let root = cache_dir(minecraft_version)?;
        let manifest_path = root.join("manifest.json");
        let en_path = root.join("en_us.json");
        let zh_path = root.join("zh_cn.json");
        if manifest_path.is_file() && en_path.is_file() && zh_path.is_file() {
            let manifest = read_json(&manifest_path)?;
            let en_us = read_json(&en_path)?;
            let zh_cn = read_json(&zh_path)?;
            return Ok(Self {
                manifest: Some(manifest),
                en_us,
                zh_cn,
            });
        }
        Self::fetch(minecraft_version)
    }

    fn fetch(minecraft_version: &str) -> Result<Self> {
        #[cfg(test)]
        {
            let _ = minecraft_version;
            return Err(anyhow!("network disabled in tests"));
        }
        #[cfg(not(test))]
        {
            let assets = client_assets_from_mojang_manifest(minecraft_version).ok();
            let client_url = client_url_from_recipe_manifest(minecraft_version).or_else(|_| {
                assets
                    .as_ref()
                    .map(|assets| assets.client_url.clone())
                    .ok_or_else(|| anyhow!("minecraft client URL not found: {minecraft_version}"))
            })?;
            let client = reqwest::blocking::Client::builder()
                .user_agent("Litematica-BA item-names/0.1")
                .build()
                .context("build item name HTTP client failed")?;
            let jar = client
                .get(&client_url)
                .send()
                .with_context(|| format!("download Minecraft client jar failed: {client_url}"))?
                .error_for_status()
                .with_context(|| {
                    format!("download Minecraft client jar returned error: {client_url}")
                })?
                .bytes()
                .context("read Minecraft client jar bytes failed")?
                .to_vec();
            let mut archive = ZipArchive::new(Cursor::new(jar.as_slice()))
                .context("open Minecraft client jar for lang failed")?;
            let en_us =
                read_lang(&mut archive, "assets/minecraft/lang/en_us.json").or_else(|_| {
                    fetch_lang_from_asset_index(
                        &client,
                        assets
                            .as_ref()
                            .and_then(|assets| assets.asset_index_url.as_deref()),
                        "minecraft/lang/en_us.json",
                    )
                })?;
            let zh_cn = read_lang(&mut archive, "assets/minecraft/lang/zh_cn.json")
                .or_else(|_| {
                    fetch_lang_from_asset_index(
                        &client,
                        assets
                            .as_ref()
                            .and_then(|assets| assets.asset_index_url.as_deref()),
                        "minecraft/lang/zh_cn.json",
                    )
                })
                .unwrap_or_default();
            let source = assets
                .as_ref()
                .and_then(|assets| assets.asset_index_url.as_ref())
                .map(|asset_index| {
                    format!("mojang_client_jar; client={client_url}; asset_index={asset_index}")
                })
                .unwrap_or_else(|| format!("mojang_client_jar; client={client_url}"));
            let manifest = manifest_for(minecraft_version, &source, &en_us, &zh_cn)?;
            write_cache(minecraft_version, &manifest, &en_us, &zh_cn)?;
            Ok(Self {
                manifest: Some(manifest),
                en_us,
                zh_cn,
            })
        }
    }

    fn lookup(&self, locale: &str, item_id: &str) -> Option<String> {
        let local = item_id.strip_prefix("minecraft:")?;
        let map = if locale == "zh-CN" {
            &self.zh_cn
        } else {
            &self.en_us
        };
        for key in [
            format!("block.minecraft.{local}"),
            format!("item.minecraft.{local}"),
        ] {
            if let Some(value) = map.get(&key) {
                return Some(value.clone());
            }
        }
        None
    }
}

fn read_lang(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
) -> Result<BTreeMap<String, String>> {
    let mut entry = archive
        .by_name(path)
        .with_context(|| format!("client jar missing lang file: {path}"))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .with_context(|| format!("read lang file failed: {path}"))?;
    serde_json::from_str(&text).with_context(|| format!("parse lang json failed: {path}"))
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
    serde_json::from_reader(file).with_context(|| format!("parse JSON failed: {}", path.display()))
}

fn client_url_from_recipe_manifest(minecraft_version: &str) -> Result<String> {
    let status = recipe_cache::recipe_status(minecraft_version)?;
    if status.status != RecipeCacheStatus::Available {
        return Err(anyhow!("recipe cache is not available"));
    }
    let source = status.source.unwrap_or_default();
    source
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("client="))
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("recipe cache manifest source has no client jar URL"))
}

struct MinecraftClientAssets {
    client_url: String,
    asset_index_url: Option<String>,
}

fn client_assets_from_mojang_manifest(minecraft_version: &str) -> Result<MinecraftClientAssets> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Litematica-BA item-names/0.1")
        .build()
        .context("build item name HTTP client failed")?;
    let manifest: Value = client
        .get(MOJANG_VERSION_MANIFEST_URL)
        .send()
        .context("download Mojang version manifest failed")?
        .error_for_status()
        .context("download Mojang version manifest returned error")?
        .text()
        .context("read Mojang version manifest failed")
        .and_then(|text| {
            serde_json::from_str(&text).context("parse Mojang version manifest failed")
        })?;
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
        .ok_or_else(|| anyhow!("minecraft version not found: {minecraft_version}"))?;
    let version_manifest: Value = client
        .get(version_url)
        .send()
        .context("download Mojang version details failed")?
        .error_for_status()
        .context("download Mojang version details returned error")?
        .text()
        .context("read Mojang version details failed")
        .and_then(|text| {
            serde_json::from_str(&text).context("parse Mojang version details failed")
        })?;
    let client_url = version_manifest
        .pointer("/downloads/client/url")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("Mojang version details have no client URL"))?;
    let asset_index_url = version_manifest
        .pointer("/assetIndex/url")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    Ok(MinecraftClientAssets {
        client_url,
        asset_index_url,
    })
}

fn fetch_lang_from_asset_index(
    client: &reqwest::blocking::Client,
    asset_index_url: Option<&str>,
    lang_path: &str,
) -> Result<BTreeMap<String, String>> {
    let asset_index_url =
        asset_index_url.ok_or_else(|| anyhow!("Mojang version details have no asset index URL"))?;
    let index: Value = client
        .get(asset_index_url)
        .send()
        .with_context(|| format!("download Mojang asset index failed: {asset_index_url}"))?
        .error_for_status()
        .with_context(|| format!("download Mojang asset index returned error: {asset_index_url}"))?
        .text()
        .context("read Mojang asset index failed")
        .and_then(|text| serde_json::from_str(&text).context("parse Mojang asset index failed"))?;
    let hash = index
        .get("objects")
        .and_then(Value::as_object)
        .and_then(|objects| objects.get(lang_path))
        .and_then(|entry| entry.get("hash"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("asset index missing language file: {lang_path}"))?;
    let prefix = hash
        .get(0..2)
        .ok_or_else(|| anyhow!("invalid asset hash for {lang_path}: {hash}"))?;
    let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
    client
        .get(&url)
        .send()
        .with_context(|| format!("download Minecraft language asset failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("download Minecraft language asset returned error: {url}"))?
        .text()
        .with_context(|| format!("read Minecraft language asset failed: {url}"))
        .and_then(|text| {
            serde_json::from_str(&text)
                .with_context(|| format!("parse Minecraft language asset failed: {lang_path}"))
        })
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

    fn fixture_materials() -> StockpileMaterialsData {
        StockpileMaterialsData {
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
                display_names: BTreeMap::new(),
                source_regions: vec!["main".to_string()],
                recipe_status: "missing".to_string(),
                craft_complexity: 0,
            }],
        }
    }
}
