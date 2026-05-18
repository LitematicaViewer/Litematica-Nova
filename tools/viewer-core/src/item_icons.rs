#![cfg_attr(test, allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use image::{ImageFormat, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::recipe_cache::{self, RecipeCacheStatus};
use crate::recipe_tree::RecipeTreeNode;
use crate::runtime_paths;
use crate::stockpile::StockpileMaterialsData;

const ICON_SCHEMA_VERSION: u32 = 1;
const MOJANG_VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconRef {
    pub key: String,
    pub path: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconCacheManifest {
    pub schema_version: u32,
    pub minecraft_version: String,
    pub source: String,
    pub fetched_at: u64,
    pub hash: String,
    pub icon_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileIconPayload {
    pub manifest: Option<IconCacheManifest>,
    pub by_key: BTreeMap<String, IconRef>,
}

#[derive(Debug, Clone)]
pub struct IconZipAsset {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StockpileIconAssets {
    pub payload: StockpileIconPayload,
    pub files: Vec<IconZipAsset>,
}

pub fn resolve_stockpile_icons(
    minecraft_version: &str,
    materials: &mut StockpileMaterialsData,
    recipe_trees: &BTreeMap<String, RecipeTreeNode>,
) -> Result<StockpileIconAssets> {
    let keys = collect_icon_keys(materials, recipe_trees);
    let mut resolver = IconResolver::new(minecraft_version)?;
    let mut by_key = BTreeMap::<String, IconRef>::new();
    let mut files = Vec::<IconZipAsset>::new();

    for key in keys {
        let resolved = resolver.resolve(&key)?;
        by_key.insert(
            key.clone(),
            IconRef {
                key,
                path: resolved.zip_path.clone(),
                available: resolved.available,
            },
        );
        files.push(IconZipAsset {
            path: resolved.zip_path,
            bytes: resolved.bytes,
        });
    }

    for material in &mut materials.materials {
        if let Some(icon) = by_key.get(&material.item_icon_key) {
            material.icon_path = icon.path.clone();
            material.icon_available = icon.available;
        }
    }

    let available_count = by_key.values().filter(|icon| icon.available).count();
    let manifest =
        icon_cache_manifest(minecraft_version, &resolver.source, &files, available_count)?;
    write_cache_manifest(minecraft_version, &manifest)?;

    Ok(StockpileIconAssets {
        payload: StockpileIconPayload {
            manifest: Some(manifest),
            by_key,
        },
        files,
    })
}

fn collect_icon_keys(
    materials: &StockpileMaterialsData,
    recipe_trees: &BTreeMap<String, RecipeTreeNode>,
) -> BTreeSet<String> {
    let mut keys = BTreeSet::<String>::new();
    keys.insert("__fallback".to_string());
    keys.insert("__tag".to_string());
    keys.insert("__special".to_string());
    keys.insert("__unresolved".to_string());
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
    if node.icon_key.starts_with("minecraft:") {
        keys.insert(node.icon_key.clone());
    } else if node.visual_kind == "tag" {
        keys.insert("__tag".to_string());
    } else if node.visual_kind == "special" {
        keys.insert("__special".to_string());
    } else if node.unresolved {
        keys.insert("__unresolved".to_string());
    }
    for child in &node.children {
        collect_tree_keys(child, keys);
    }
}

struct ResolvedIcon {
    zip_path: String,
    bytes: Vec<u8>,
    available: bool,
}

struct IconResolver {
    minecraft_version: String,
    cache_dir: PathBuf,
    source: String,
    jar_bytes: Option<Vec<u8>>,
}

impl IconResolver {
    fn new(minecraft_version: &str) -> Result<Self> {
        let cache_dir = runtime_paths::item_icons_cache_root()?
            .join(version_dir_name(minecraft_version))
            .join("icons");
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("create item icon cache failed: {}", cache_dir.display()))?;
        Ok(Self {
            minecraft_version: minecraft_version.to_string(),
            cache_dir,
            source: recipe_cache::recipe_status(minecraft_version)
                .ok()
                .and_then(|status| status.source)
                .unwrap_or_else(|| "fallback_generated".to_string()),
            jar_bytes: None,
        })
    }

    fn resolve(&mut self, key: &str) -> Result<ResolvedIcon> {
        let zip_path = format!("assets/icons/{}.png", safe_icon_name(key));
        if !key.starts_with("minecraft:") {
            return Ok(ResolvedIcon {
                zip_path,
                bytes: fallback_png(key)?,
                available: false,
            });
        }

        let cache_path = self.cache_dir.join(format!("{}.png", safe_icon_name(key)));
        if cache_path.is_file() {
            return Ok(ResolvedIcon {
                zip_path,
                bytes: fs::read(&cache_path).with_context(|| {
                    format!("read cached icon failed: {}", cache_path.display())
                })?,
                available: true,
            });
        }

        if let Some(bytes) = self.extract_from_client_jar(key)? {
            fs::write(&cache_path, &bytes)
                .with_context(|| format!("write cached icon failed: {}", cache_path.display()))?;
            return Ok(ResolvedIcon {
                zip_path,
                bytes,
                available: true,
            });
        }

        Ok(ResolvedIcon {
            zip_path,
            bytes: fallback_png(key)?,
            available: false,
        })
    }

    fn extract_from_client_jar(&mut self, key: &str) -> Result<Option<Vec<u8>>> {
        let Some(local) = key.strip_prefix("minecraft:") else {
            return Ok(None);
        };
        #[cfg(test)]
        {
            let _ = local;
            return Ok(None);
        }
        #[cfg(not(test))]
        {
            self.ensure_client_jar()?;
            let Some(bytes) = self.jar_bytes.as_ref() else {
                return Ok(None);
            };
            let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice()))
                .context("open Minecraft client jar for icons failed")?;

            for path in [
                format!("assets/minecraft/textures/item/{local}.png"),
                format!("assets/minecraft/textures/block/{local}.png"),
            ] {
                if let Some(bytes) = read_zip_bytes(&mut archive, &path)? {
                    return Ok(Some(bytes));
                }
            }

            let mut visited = BTreeSet::<String>::new();
            self.resolve_model_texture(
                &mut archive,
                &format!("minecraft:item/{local}"),
                &mut visited,
            )
        }
    }

    fn resolve_model_texture(
        &self,
        archive: &mut ZipArchive<Cursor<&[u8]>>,
        model_ref: &str,
        visited: &mut BTreeSet<String>,
    ) -> Result<Option<Vec<u8>>> {
        if !visited.insert(model_ref.to_string()) || visited.len() > 12 {
            return Ok(None);
        }
        let Some((namespace, local)) = split_namespaced_model(model_ref) else {
            return Ok(None);
        };
        if namespace != "minecraft" {
            return Ok(None);
        }
        let model_path = format!("assets/minecraft/models/{local}.json");
        let Some(model_text) = read_zip_string(archive, &model_path)? else {
            return Ok(None);
        };
        let model: Value = serde_json::from_str(&model_text)
            .with_context(|| format!("parse item model failed: {model_path}"))?;

        if let Some(texture_ref) = best_texture_ref(&model)
            && let Some(bytes) = self.resolve_texture_ref(archive, texture_ref)?
        {
            return Ok(Some(bytes));
        }
        if let Some(parent) = model.get("parent").and_then(Value::as_str) {
            return self.resolve_model_texture(archive, parent, visited);
        }
        Ok(None)
    }

    fn resolve_texture_ref(
        &self,
        archive: &mut ZipArchive<Cursor<&[u8]>>,
        texture_ref: &str,
    ) -> Result<Option<Vec<u8>>> {
        let reference = texture_ref.trim_start_matches('#');
        let Some((namespace, local)) = split_namespaced_texture(reference) else {
            return Ok(None);
        };
        if namespace != "minecraft" {
            return Ok(None);
        }
        read_zip_bytes(archive, &format!("assets/minecraft/textures/{local}.png"))
    }

    fn ensure_client_jar(&mut self) -> Result<()> {
        if self.jar_bytes.is_some() {
            return Ok(());
        }
        let client_url = client_url_from_recipe_manifest(&self.minecraft_version)
            .or_else(|_| client_url_from_mojang_manifest(&self.minecraft_version))?;
        let client = reqwest::blocking::Client::builder()
            .user_agent("Litematica-BA item-icons/0.1")
            .build()
            .context("build item icon HTTP client failed")?;
        let bytes = client
            .get(&client_url)
            .send()
            .with_context(|| format!("download Minecraft client jar failed: {client_url}"))?
            .error_for_status()
            .with_context(|| format!("download Minecraft client jar returned error: {client_url}"))?
            .bytes()
            .context("read Minecraft client jar bytes failed")?
            .to_vec();
        self.source = format!("mojang_client_jar; client={client_url}");
        self.jar_bytes = Some(bytes);
        Ok(())
    }
}

fn icon_cache_manifest(
    minecraft_version: &str,
    source: &str,
    files: &[IconZipAsset],
    available_count: usize,
) -> Result<IconCacheManifest> {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        hasher.update(&file.bytes);
        hasher.update([0xff]);
    }
    Ok(IconCacheManifest {
        schema_version: ICON_SCHEMA_VERSION,
        minecraft_version: minecraft_version.to_string(),
        source: source.to_string(),
        fetched_at: current_unix_timestamp()?,
        hash: to_hex(&hasher.finalize()),
        icon_count: available_count,
    })
}

fn write_cache_manifest(minecraft_version: &str, manifest: &IconCacheManifest) -> Result<()> {
    let version_root =
        runtime_paths::item_icons_cache_root()?.join(version_dir_name(minecraft_version));
    fs::create_dir_all(&version_root).with_context(|| {
        format!(
            "create item icon cache version dir failed: {}",
            version_root.display()
        )
    })?;
    let path = version_root.join("manifest.json");
    let mut file = fs::File::create(&path)
        .with_context(|| format!("create icon manifest failed: {}", path.display()))?;
    file.write_all(&serde_json::to_vec_pretty(manifest)?)
        .with_context(|| format!("write icon manifest failed: {}", path.display()))?;
    Ok(())
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

fn client_url_from_mojang_manifest(minecraft_version: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Litematica-BA item-icons/0.1")
        .build()
        .context("build item icon HTTP client failed")?;
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
    version_manifest
        .pointer("/downloads/client/url")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("Mojang version details have no client URL"))
}

fn best_texture_ref(model: &Value) -> Option<&str> {
    let textures = model.get("textures")?.as_object()?;
    for key in ["layer0", "all", "top", "side", "front", "particle"] {
        if let Some(value) = textures.get(key).and_then(Value::as_str) {
            return Some(value);
        }
    }
    textures.values().find_map(Value::as_str)
}

fn read_zip_bytes(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<Vec<u8>>> {
    let Ok(mut entry) = archive.by_name(name) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .with_context(|| format!("read zip entry failed: {name}"))?;
    Ok(Some(bytes))
}

fn read_zip_string(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<String>> {
    let Some(bytes) = read_zip_bytes(archive, name)? else {
        return Ok(None);
    };
    String::from_utf8(bytes)
        .map(Some)
        .with_context(|| format!("zip entry is not UTF-8: {name}"))
}

fn split_namespaced_model(value: &str) -> Option<(&str, &str)> {
    let (namespace, local) = value.split_once(':').unwrap_or(("minecraft", value));
    Some((namespace, local))
}

fn split_namespaced_texture(value: &str) -> Option<(&str, &str)> {
    let (namespace, local) = value.split_once(':').unwrap_or(("minecraft", value));
    Some((namespace, local))
}

pub fn safe_icon_name(key: &str) -> String {
    let safe = key
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if safe.is_empty() {
        "_fallback".to_string()
    } else {
        safe
    }
}

fn fallback_png(key: &str) -> Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let base = Rgba([digest[0], digest[1], digest[2], 255]);
    let accent = Rgba([
        digest[3].saturating_add(80),
        digest[4].saturating_add(80),
        230,
        255,
    ]);
    let mut image = RgbaImage::from_pixel(32, 32, base);
    for y in 6..26 {
        for x in 6..26 {
            if x == 6 || x == 25 || y == 6 || y == 25 || ((x + y) % 11 == 0) {
                image.put_pixel(x, y, accent);
            }
        }
    }
    let mut cursor = Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, ImageFormat::Png)
        .context("encode fallback icon PNG failed")?;
    Ok(cursor.into_inner())
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

    #[test]
    fn safe_icon_name_replaces_path_and_namespace_chars() {
        assert_eq!(
            safe_icon_name("minecraft:oak/planks"),
            "minecraft_oak_planks"
        );
        assert_eq!(safe_icon_name("#minecraft:logs"), "_minecraft_logs");
    }

    #[test]
    fn fallback_icon_is_png() {
        let icon = fallback_png("minecraft:missing").expect("fallback icon");
        assert!(icon.starts_with(b"\x89PNG"));
    }
}
