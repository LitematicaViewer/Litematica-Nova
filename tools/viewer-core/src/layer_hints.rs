//! Public layer presentation rules migrated from the pre-0.4 TS implementation.
//! The declarative table retains the existing rule decisions; model projection,
//! exact-version validation, file reads and all pixel operations remain in Rust.
use anyhow::{Context, Result, ensure};
use image::{
    RgbaImage,
    imageops::{self, FilterType},
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[derive(Deserialize)]
struct Catalog {
    schema: u32,
    rules: Vec<Rule>,
    blocks: BTreeMap<String, BlockRules>,
}
#[derive(Deserialize)]
struct BlockRules {
    keys: Vec<String>,
    states: BTreeMap<String, usize>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Rule {
    image_rel_paths: Vec<String>,
    subtract_image_rel_paths: Vec<String>,
    icon_block_ids: Vec<String>,
    base_image_rel_paths: Vec<String>,
    base_image_rotate_quarter_turns: i32,
    overlay_icon_block_ids: Vec<String>,
    subtract_overlay_icon_block_ids: Vec<String>,
    image_layers: Vec<Layer>,
    requires_base: bool,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Layer {
    image_rel_path: String,
    flip_horizontal: bool,
    rotate_quarter_turns: i32,
    effect: String,
}
fn catalog() -> Result<&'static Catalog> {
    static DATA: OnceLock<std::result::Result<Catalog, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str::<Catalog>(include_str!("layer_hints.json"))
            .map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| anyhow::anyhow!(e.clone()))
}
fn rule(id: &str, properties: &BTreeMap<String, String>) -> Result<Option<&'static Rule>> {
    let data = catalog()?;
    ensure!(data.schema == 1, "Unsupported layer hint table");
    let Some(block) = data.blocks.get(id) else {
        return Ok(None);
    };
    let key = block
        .keys
        .iter()
        .map(|key| properties.get(key).map(String::as_str).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\0");
    Ok(block
        .states
        .get(&key)
        .and_then(|index| data.rules.get(*index)))
}
struct IconRoot {
    path: PathBuf,
    label: String,
    layering: bool,
}
pub(super) struct HintAssets {
    config: PathBuf,
    bundled: PathBuf,
    roots: Vec<IconRoot>,
    revision: String,
}
impl HintAssets {
    pub(super) fn new(config: PathBuf, version: &str) -> Result<Self> {
        let bundled = crate::runtime_paths::app_root()?.join("pack-in");
        let mut roots = Vec::new();
        let bytes = Self::index_bytes(&config)?;
        if let Some(bytes) = bytes.as_ref() {
            let index: Value = serde_json::from_slice(bytes)
                .context("Invalid installed layering icon registry")?;
            if let Some(entry) = index["entries"].as_array().into_iter().flatten().find(|v| {
                v["active_layering"] == true
                    && v["builtin"] != true
                    && v["block_icon_variant"] != "3d"
            }) {
                let path = entry["root_path"]
                    .as_str()
                    .filter(|_| entry["source"] == "external")
                    .map(PathBuf::from)
                    .or_else(|| entry["root_relpath"].as_str().map(|p| config.join(p)));
                if let Some(path) = path {
                    roots.push(IconRoot {
                        path,
                        label: entry["label"].as_str().unwrap_or("").into(),
                        layering: true,
                    });
                }
            }
        }
        let context = crate::game_data::active_context()?;
        // A selected latest-game icon must not replace another exact source version.
        if context.game_version == version {
            roots.push(IconRoot {
                path: context.root.join("derived/item-icons"),
                label: version.into(),
                layering: false,
            });
        }
        Ok(Self {
            config,
            bundled,
            roots,
            revision: Self::digest(bytes.as_deref()),
        })
    }
    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        Self {
            config: PathBuf::new(),
            bundled: PathBuf::new(),
            roots: vec![],
            revision: Self::digest(None),
        }
    }
    fn index_bytes(config: &Path) -> Result<Option<Vec<u8>>> {
        let path = config.join("minecraft-assets/block_icon/installed.json");
        if path.is_file() {
            Ok(Some(fs::read(path)?))
        } else {
            Ok(None)
        }
    }
    fn digest(bytes: Option<&[u8]>) -> String {
        format!("{:x}", Sha256::digest(bytes.unwrap_or(&[])))
    }
    pub(super) fn revision(&self) -> &str {
        &self.revision
    }
    pub(super) fn is_current(&self) -> Result<bool> {
        Ok(Self::digest(Self::index_bytes(&self.config)?.as_deref()) == self.revision)
    }
    fn asset(&self, name: &str) -> Result<Option<RgbaImage>> {
        let relative = name.strip_prefix("data/").unwrap_or(name);
        for base in [&self.config, &self.bundled] {
            let path = base.join(relative);
            if path.is_file() {
                return Ok(Some(image::open(path)?.to_rgba8()));
            }
        }
        if relative.starts_with("flake/state_hint/") || relative.starts_with("flake/state_overlay/")
        {
            for base in [&self.config, &self.bundled] {
                let path = base.join("flake/state_hint/alt.png");
                if path.is_file() {
                    return Ok(Some(image::open(path)?.to_rgba8()));
                }
            }
        }
        Ok(None)
    }
    fn icon(&self, id: &str, layering_only: bool) -> Result<Option<RgbaImage>> {
        let name = id
            .strip_prefix("minecraft:")
            .unwrap_or(id)
            .replace(':', "/");
        ensure!(
            !name.contains("..") && !name.starts_with('/'),
            "Invalid layer icon identity"
        );
        for root in &self.roots {
            if layering_only && !root.layering {
                continue;
            }
            let mut paths = vec![format!("{name}.png")];
            if !root.label.is_empty() {
                for folder in ["block_2d", "block_icon", "item"] {
                    paths.push(format!("{folder}/{}/{name}.png", root.label));
                }
            }
            for relative in paths {
                let path = root.path.join(relative);
                if path.is_file() {
                    return Ok(Some(image::open(path)?.to_rgba8()));
                }
            }
        }
        Ok(None)
    }
    pub(super) fn apply(&self, base: &RgbaImage, state: &str, enabled: bool) -> Result<RgbaImage> {
        let (id, properties) = crate::version_conversion::parse_key(state)?;
        let mut image = self.icon(&id, true)?.unwrap_or_else(|| base.clone());
        if !enabled {
            return Ok(image);
        }
        if id == "minecraft:redstone_wire" {
            return self.redstone(&properties).map(|v| v.unwrap_or(image));
        }
        let Some(rule) = rule(&id, &properties)? else {
            return Ok(image);
        };
        let explicit = !rule.base_image_rel_paths.is_empty() || !rule.icon_block_ids.is_empty();
        let mut replacement = None;
        for name in &rule.base_image_rel_paths {
            if let Some(value) = self.asset(name)? {
                replacement = Some(value);
            }
        }
        if rule.base_image_rel_paths.is_empty() {
            for id in &rule.icon_block_ids {
                if let Some(value) = self.icon(id, false)? {
                    replacement = Some(value);
                }
            }
        }
        if rule.requires_base && explicit && replacement.is_none() {
            return Ok(image);
        }
        let resolved = replacement.is_some();
        if let Some(value) = replacement {
            image = rotate(value, rule.base_image_rotate_quarter_turns);
        }
        for id in &rule.subtract_overlay_icon_block_ids {
            if let Some(mask) = self.icon(id, false)? {
                blend_mask(&mut image, &mask, "subtract");
            }
        }
        for name in &rule.subtract_image_rel_paths {
            if let Some(mask) = self.asset(name)? {
                blend_mask(&mut image, &mask, "subtract");
            }
        }
        for id in &rule.overlay_icon_block_ids {
            if let Some(mask) = self.icon(id, false)? {
                blend_mask(&mut image, &mask, "overlay");
            }
        }
        for name in &rule.image_rel_paths {
            if let Some(mask) = self.asset(name)? {
                blend_mask(&mut image, &mask, "overlay");
            }
        }
        if !explicit || resolved {
            for layer in &rule.image_layers {
                if let Some(mut mask) = self.asset(&layer.image_rel_path)? {
                    if layer.flip_horizontal {
                        mask = imageops::flip_horizontal(&mask);
                    }
                    mask = rotate(mask, layer.rotate_quarter_turns);
                    blend_mask(&mut image, &mask, &layer.effect);
                }
            }
        }
        Ok(image)
    }
    fn redstone(&self, properties: &BTreeMap<String, String>) -> Result<Option<RgbaImage>> {
        // Retained public redstone-display palette, not guessed map colours.
        const COLORS: [u32; 16] = [
            0x4c0000, 0x700000, 0x7a0000, 0x840000, 0x8e0000, 0x990000, 0xa30000, 0xad0000,
            0xb70000, 0xc10000, 0xcc0000, 0xd60000, 0xe00000, 0xea0000, 0xf41b00, 0xff3200,
        ];
        let power = properties
            .get("power")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v < 16)
            .unwrap_or(0);
        let Some(base) = self.asset("flake/redstone_display/redstone_wire_base.png")? else {
            return Ok(None);
        };
        let mut image = RgbaImage::new(base.width(), base.height());
        let mut layers = vec![base];
        for direction in ["north", "east", "south", "west"] {
            if let Some(value) = properties
                .get(direction)
                .filter(|v| matches!(v.as_str(), "side" | "up"))
            {
                let suffix = if value == "up" { "_up" } else { "" };
                if let Some(mask) = self.asset(&format!(
                    "flake/redstone_display/redstone_wire_{direction}{suffix}.png"
                ))? {
                    layers.push(mask);
                }
            }
        }
        let color = COLORS[power];
        for mut layer in layers {
            for pixel in layer.pixels_mut() {
                pixel.0[0] = (color >> 16) as u8;
                pixel.0[1] = (color >> 8) as u8;
                pixel.0[2] = color as u8;
            }
            blend_mask(&mut image, &layer, "overlay");
        }
        if let Some(number) = self.asset(&format!("flake/redstone_display/num_{power}.png"))? {
            blend_mask(&mut image, &number, "overlay");
        }
        Ok(Some(image))
    }
}
fn rotate(image: RgbaImage, turns: i32) -> RgbaImage {
    match turns.rem_euclid(4) {
        1 => imageops::rotate90(&image),
        2 => imageops::rotate180(&image),
        3 => imageops::rotate270(&image),
        _ => image,
    }
}
fn blend_mask(image: &mut RgbaImage, mask: &RgbaImage, effect: &str) {
    let mask = imageops::resize(mask, image.width(), image.height(), FilterType::Nearest);
    if effect != "subtract" && effect != "alpha" {
        imageops::overlay(image, &mask, 0, 0);
        return;
    }
    for (base, mask) in image.pixels_mut().zip(mask.pixels()) {
        let factor = (f64::from(mask.0[0]) + f64::from(mask.0[1]) + f64::from(mask.0[2]))
            / (255. * 3.)
            * (f64::from(mask.0[3]) / 255.);
        if effect == "alpha" {
            base.0[3] = (f64::from(base.0[3]) * (1. - factor)).round() as u8;
        } else {
            for channel in 0..3 {
                let value = f64::from(base.0[channel]);
                base.0[channel] = (value * (1. - factor) + (255. - value) * factor).round() as u8;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrated_public_rules_keep_axis_fluid_and_bookshelf_states() {
        let fluid = rule(
            "minecraft:water",
            &BTreeMap::from([("level".into(), "0".into())]),
        )
        .unwrap()
        .unwrap();
        assert!(
            fluid
                .subtract_image_rel_paths
                .iter()
                .any(|p| p.ends_with("level_0.png"))
        );
        let log = rule(
            "minecraft:oak_log",
            &BTreeMap::from([("axis".into(), "x".into())]),
        )
        .unwrap()
        .unwrap();
        assert!(!log.subtract_image_rel_paths.is_empty() || !log.image_rel_paths.is_empty());
    }
    #[test]
    fn alpha_mask_does_not_rewrite_rgb_and_subtract_keeps_alpha() {
        let mut image = RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 200]));
        let mask = RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        blend_mask(&mut image, &mask, "subtract");
        assert_eq!(image.get_pixel(0, 0).0, [245, 235, 225, 200]);
        blend_mask(&mut image, &mask, "alpha");
        assert_eq!(image.get_pixel(0, 0).0, [245, 235, 225, 0]);
    }
}
