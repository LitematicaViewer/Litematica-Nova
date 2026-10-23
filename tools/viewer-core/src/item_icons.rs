#![cfg_attr(test, allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use image::{ImageFormat, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};



use crate::recipe_tree::RecipeTreeNode;
use crate::runtime_paths;
use crate::stockpile::StockpileMaterialsData;

const ICON_SCHEMA_VERSION: u32 = 1;
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
    resolve_stockpile_icons_with_network(minecraft_version, materials, recipe_trees, true)
}

pub(crate) fn resolve_stockpile_icons_cached(minecraft_version: &str, materials: &mut StockpileMaterialsData, recipe_trees: &BTreeMap<String, RecipeTreeNode>) -> Result<StockpileIconAssets> {
    resolve_stockpile_icons_with_network(minecraft_version, materials, recipe_trees, false)
}

fn resolve_stockpile_icons_with_network(minecraft_version: &str, materials: &mut StockpileMaterialsData, recipe_trees: &BTreeMap<String, RecipeTreeNode>, allow_network: bool) -> Result<StockpileIconAssets> {
    let keys = collect_icon_keys(materials, recipe_trees);
    let mut resolver = IconResolver::new(minecraft_version)?;
    let _ = allow_network; // Historical API: all resources are now local selected game data.
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
        if !resolved.bytes.is_empty() { files.push(IconZipAsset { path: resolved.zip_path, bytes: resolved.bytes }); }
    }

    for material in &mut materials.materials {
        if let Some(icon) = by_key.get(&material.item_icon_key) {
            material.icon_path = icon.path.clone();
            material.icon_available = icon.available;
            material.icon_diagnostic = (!icon.available).then(|| {
                format!(
                    "no real icon in selected game data for {}",
                    material.item_icon_key
                )
            });
        }
    }
    for material in &materials.materials {
        if !material.icon_available {
            materials
                .diagnostics
                .push(crate::stockpile::StockpileDiagnostic {
                    kind: "icon_missing".to_string(),
                    source: material.namespace_id.clone(),
                    target: material.item_icon_key.clone(),
                    message: format!(
                        "no real item/block icon found in selected game data for {}",
                        material.item_icon_key
                    ),
                });
        }
    }

    let available_count = by_key.values().filter(|icon| icon.available).count();
    let manifest =
        icon_cache_manifest(minecraft_version, &resolver.source, &files, available_count)?;
    write_cache_manifest(&resolver.context.content_id, &manifest)?;

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

fn collect_blockstate_models(value: &Value) -> Vec<String> {
    let mut models = BTreeSet::<String>::new();
    collect_blockstate_models_inner(value, &mut models);
    models.into_iter().collect()
}

fn collect_blockstate_models_inner(value: &Value, models: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(model) = map.get("model").and_then(Value::as_str) {
                models.insert(if model.contains(':') {
                    model.to_string()
                } else {
                    format!("minecraft:{model}")
                });
            }
            for child in map.values() {
                collect_blockstate_models_inner(child, models);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_blockstate_models_inner(child, models);
            }
        }
        _ => {}
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
    context: crate::game_data::GameDataContext,
}

impl IconResolver {
    fn new(_minecraft_version: &str) -> Result<Self> {
        let context = crate::game_data::active_context()?;
        let cache_dir = runtime_paths::item_icons_cache_root()?.join(version_dir_name(&context.content_id)).join("icons");
        fs::create_dir_all(&cache_dir)?;
        Ok(Self { minecraft_version: context.game_version.clone(), cache_dir, source: format!("game-data:{}", context.content_id), context })
    }

    fn resolve(&mut self, key: &str) -> Result<ResolvedIcon> {
        let zip_path = format!("assets/icons/{}.png", safe_icon_name(key));
        if !key.contains(':') {
            // Virtual recipe/tag UI markers are not Minecraft item geometry.
            return Ok(ResolvedIcon { zip_path, bytes: fallback_png(key)?, available: false });
        }
        if let Some(local) = key.strip_prefix("minecraft:").filter(|local| !local.is_empty() && local.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')) {
            let bundled = self.context.root.join("derived/item-icons").join(format!("{local}.png"));
            if bundled.is_file() {
                let bytes = fs::read(&bundled)?;
                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") { return Err(anyhow!("invalid selected pre-generated item icon: {}", bundled.display())); }
                return Ok(ResolvedIcon { zip_path, bytes, available: true });
            }
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

        Ok(ResolvedIcon { zip_path: String::new(), bytes: Vec::new(), available: false })
    }

    fn extract_from_client_jar(&mut self, key: &str) -> Result<Option<Vec<u8>>> {
        let Some((namespace, local)) = key.split_once(':') else { return Ok(None); };
        let context = &self.context;
        for path in [format!("assets/{namespace}/textures/item/{local}.png"), format!("assets/{namespace}/textures/block/{local}.png")] {
            if read_selected_bytes(context, &path)?.is_some() {
                let reference = path.strip_prefix(&format!("assets/{namespace}/textures/")).and_then(|path|path.strip_suffix(".png")).context("invalid selected texture path")?;
                return self.resolve_texture_ref(context, &format!("{namespace}:{reference}"));
            }
        }
        if let Some(bytes) = read_selected_bytes(context, &format!("assets/{namespace}/items/{local}.json"))? {
            let definition: Value = serde_json::from_slice(&bytes).context("parse selected item appearance")?;
            for model in collect_blockstate_models(&definition) {
                if let Some(bytes) = self.resolve_model_texture(context, &model, &mut BTreeSet::new())? { return Ok(Some(bytes)); }
            }
        }
        for model in [format!("{namespace}:item/{local}"), format!("{namespace}:block/{local}")] {
            if let Some(bytes) = self.resolve_model_texture(context, &model, &mut BTreeSet::new())? { return Ok(Some(bytes)); }
        }
        self.resolve_blockstate_texture(context, namespace, local)
    }

    fn resolve_model_texture(
        &self,
        archive: &crate::game_data::GameDataContext,
        model_ref: &str,
        visited: &mut BTreeSet<String>,
    ) -> Result<Option<Vec<u8>>> {
        let reference = if model_ref.contains(':') { model_ref.to_owned() } else { format!("minecraft:{model_ref}") };
        let Some((namespace, local)) = reference.split_once(':') else { return Ok(None); };
        if read_selected_bytes(archive, &format!("assets/{namespace}/models/{local}.json"))?.is_none() { return Ok(None); }
        let model = crate::block_database::resource_model(archive, &reference, visited)?;
        let Some(value) = best_texture_ref(&model) else { return Ok(None); };
        let mut texture = value.to_owned();
        let mut visited = BTreeSet::new();
        while let Some(key) = texture.strip_prefix('#') {
            if !visited.insert(key.to_owned()) { return Err(anyhow!("item model texture cycle: {reference}")); }
            texture = model.get("textures").and_then(|textures| textures.get(key)).and_then(Value::as_str).with_context(|| format!("item model texture variable missing: {reference} #{key}"))?.to_owned();
        }
        self.resolve_texture_ref(archive, &texture)
    }

    fn resolve_texture_ref(
        &self,
        archive: &crate::game_data::GameDataContext,
        texture_ref: &str,
    ) -> Result<Option<Vec<u8>>> {
        let reference = texture_ref.trim_start_matches('#');
        let Some((namespace, local)) = split_namespaced_texture(reference) else {
            return Ok(None);
        };
        let path = format!("assets/{namespace}/textures/{local}.png");
        if read_selected_bytes(archive, &path)?.is_none() { return Ok(None); }
        let texture = crate::block_database::selected_texture(archive, &format!("{namespace}:{local}"))?;
        let mut output = Cursor::new(Vec::new());
        texture.write_to(&mut output, ImageFormat::Png).context("encode selected item icon")?;
        Ok(Some(output.into_inner()))
    }

    fn resolve_blockstate_texture(
        &self,
        archive: &crate::game_data::GameDataContext,
        namespace: &str,
        local: &str,
    ) -> Result<Option<Vec<u8>>> {
        let path = format!("assets/{namespace}/blockstates/{local}.json");
        let Some(text) = read_selected_string(archive, &path)? else {
            return Ok(None);
        };
        let value: Value = serde_json::from_str(&text)
            .with_context(|| format!("parse blockstate failed: {path}"))?;
        for model_ref in collect_blockstate_models(&value) {
            let mut visited = BTreeSet::<String>::new();
            if let Some(bytes) = self.resolve_model_texture(archive, &model_ref, &mut visited)? {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
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

fn best_texture_ref(model: &Value) -> Option<&str> {
    let textures = model.get("textures")?.as_object()?;
    for key in ["layer0", "all", "top", "side", "front", "particle"] {
        if let Some(value) = textures.get(key).and_then(Value::as_str) {
            return Some(value);
        }
    }
    textures.values().find_map(Value::as_str)
}

fn read_selected_bytes(context: &crate::game_data::GameDataContext, name: &str) -> Result<Option<Vec<u8>>> {
    crate::game_data::read_resource_from(context, name)
}
fn read_selected_string(context: &crate::game_data::GameDataContext, name: &str) -> Result<Option<String>> {
    read_selected_bytes(context, name)?.map(|bytes| String::from_utf8(bytes).with_context(|| format!("resource is not UTF-8: {name}"))).transpose()
}

pub fn read_game_data_icon(id: &str) -> Result<PathBuf> {
    if !id.split_once(':').is_some_and(|(namespace, local)| !namespace.is_empty() && !local.is_empty() && namespace.bytes().chain(local.bytes()).all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-/.".contains(&byte)) && !local.contains("..")) { return Err(anyhow!("invalid namespaced item icon id: {id}")); }
    let mut resolver = IconResolver::new("")?;
    let output = resolver.cache_dir.join(format!("{}.png", safe_icon_name(id)));
    if output.is_file() { return Ok(output); }
    let value = resolver.resolve(id)?;
    if !value.available { return Err(anyhow!("selected Minecraft {} data has no real model/texture icon for {id}", resolver.minecraft_version)); }
    fs::write(&output, value.bytes)?;
    Ok(output)
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
