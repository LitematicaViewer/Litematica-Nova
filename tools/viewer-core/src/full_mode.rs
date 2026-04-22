use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::model::PaletteEntryOutput;

pub const FULL_MODE_MATERIAL_CACHE_FORMAT: &str = "lba_full_mode_material_cache_v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FullModeAlphaMode {
    #[default]
    Opaque,
    Cutout,
    Translucent,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FullModeMaterialSlot {
    pub key: String,
    pub uv_rect: [f32; 4],
    #[serde(default)]
    pub alpha_mode: FullModeAlphaMode,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FullModePaletteMaterial {
    #[serde(default)]
    pub geometry_hint: Option<String>,
    #[serde(default)]
    pub down: Option<u32>,
    #[serde(default)]
    pub up: Option<u32>,
    #[serde(default)]
    pub north: Option<u32>,
    #[serde(default)]
    pub south: Option<u32>,
    #[serde(default)]
    pub west: Option<u32>,
    #[serde(default)]
    pub east: Option<u32>,
    #[serde(default)]
    pub cross: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FullModeModelQuad {
    pub vertices: [[f32; 3]; 4],
    pub material: u32,
    #[serde(default)]
    pub uv: Option<[[f32; 2]; 4]>,
    #[serde(default)]
    pub double_sided: bool,
    #[serde(default)]
    pub cullface: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FullModeBlockModelQuads {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub quads: Vec<FullModeModelQuad>,
}

impl FullModePaletteMaterial {
    pub fn slot_for_face_index(&self, face_index: usize) -> Option<u32> {
        match face_index {
            0 => self.west,
            1 => self.east,
            2 => self.down,
            3 => self.up,
            4 => self.north,
            5 => self.south,
            _ => None,
        }
        .or(self.cross)
    }

    pub fn cross_slot(&self) -> Option<u32> {
        self.cross
            .or(self.north)
            .or(self.south)
            .or(self.west)
            .or(self.east)
            .or(self.up)
            .or(self.down)
    }

    pub fn is_flat_top_hint(&self) -> bool {
        matches!(self.geometry_hint.as_deref(), Some("flat_top"))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FullModeStats {
    #[serde(default)]
    pub palette_entries: usize,
    #[serde(default)]
    pub material_slots: usize,
    #[serde(default)]
    pub baked_palette_entries: usize,
    #[serde(default)]
    pub fallback_palette_entries: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FullModeMaterialCache {
    pub format: String,
    pub source: String,
    pub atlas_file: String,
    #[serde(default)]
    pub palette_keys: Vec<String>,
    #[serde(default)]
    pub materials: Vec<FullModeMaterialSlot>,
    #[serde(default)]
    pub palette_materials: Vec<FullModePaletteMaterial>,
    #[serde(default)]
    pub palette_model_quads: Vec<Vec<FullModeModelQuad>>,
    #[serde(default)]
    pub block_model_quads: Vec<FullModeBlockModelQuads>,
    #[serde(default)]
    pub stats: Option<FullModeStats>,
    #[serde(default)]
    pub cache_manifest: Option<String>,
}

impl FullModeMaterialCache {
    pub fn atlas_path(&self, cache_path: &Path) -> PathBuf {
        cache_path
            .parent()
            .map(|parent| parent.join(&self.atlas_file))
            .unwrap_or_else(|| PathBuf::from(&self.atlas_file))
    }

    pub fn validate_palette(
        &self,
        palette: &[PaletteEntryOutput],
        property_pool: &[BTreeMap<String, String>],
    ) -> Result<()> {
        if self.format != FULL_MODE_MATERIAL_CACHE_FORMAT {
            bail!(
                "full mode material cache format mismatch: expected={} actual={}",
                FULL_MODE_MATERIAL_CACHE_FORMAT,
                self.format
            );
        }
        if self.materials.is_empty() || self.palette_materials.is_empty() {
            bail!("full mode material cache is missing material slots");
        }
        if self.palette_model_quads.len() != palette.len() {
            bail!(
                "full mode model geometry missing or stale: model_quads={} scene_palette={}",
                self.palette_model_quads.len(),
                palette.len()
            );
        }
        if self.palette_keys.len() != palette.len() {
            bail!(
                "full mode palette mismatch: cache_keys={} scene_palette={}",
                self.palette_keys.len(),
                palette.len()
            );
        }
        for (index, entry) in palette.iter().enumerate() {
            let expected = self
                .palette_keys
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("missing full mode palette key at index {index}"))?;
            let properties = property_pool.get(entry.property_id);
            let actual = palette_state_key(&entry.block_id, properties);
            if actual != *expected {
                bail!(
                    "full mode palette mismatch at index {}: expected={} actual={}",
                    index,
                    expected,
                    actual
                );
            }
        }
        Ok(())
    }
}

pub fn load_full_mode_material_cache(path: &Path) -> Result<FullModeMaterialCache> {
    let payload = std::fs::read_to_string(path)
        .with_context(|| format!("read full mode material cache failed: {}", path.display()))?;
    serde_json::from_str(&payload)
        .with_context(|| format!("parse full mode material cache failed: {}", path.display()))
}

pub fn palette_state_key(block_id: &str, properties: Option<&BTreeMap<String, String>>) -> String {
    let Some(properties) = properties else {
        return block_id.to_string();
    };
    if properties.is_empty() {
        return block_id.to_string();
    }
    let pairs = properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{block_id}[{pairs}]")
}
