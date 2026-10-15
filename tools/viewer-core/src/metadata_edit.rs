use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use fastnbt::IntArray;
use image::{GenericImageView, imageops::FilterType};
use serde::{Deserialize, Serialize};

use crate::nbt::{load_litematic_root, save_litematic_root};

#[derive(Debug, Deserialize)]
pub struct MetadataEditPatch {
    pub name: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub time_created: Option<i64>,
    pub time_modified: Option<i64>,
    pub litematic_version: Option<i32>,
    pub litematic_subversion: Option<i32>,
    pub minecraft_data_version: Option<i32>,
    pub preview_image_data: Option<Vec<i32>>,
    pub preview_image_path: Option<String>,
    #[serde(default)]
    pub regions: Vec<RegionRenamePatch>,
}

#[derive(Debug, Deserialize)]
pub struct RegionRenamePatch {
    pub old_name: String,
    pub new_name: String,
}

#[derive(Debug, Serialize)]
pub struct MetadataEditSummary {
    pub input: String,
    pub output: String,
    pub backup_path: Option<String>,
    pub changed_regions: Vec<RegionRenamePatchOutput>,
}

#[derive(Debug, Serialize)]
pub struct RegionRenamePatchOutput {
    pub old_name: String,
    pub new_name: String,
}

pub fn edit_metadata(
    input: &Path,
    output: Option<&Path>,
    patch_path: &Path,
) -> Result<MetadataEditSummary> {
    let patch_text = fs::read_to_string(patch_path)
        .with_context(|| format!("failed to read patch {}", patch_path.display()))?;
    let patch: MetadataEditPatch = serde_json::from_str(patch_text.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse patch {}", patch_path.display()))?;
    let mut root = load_litematic_root(input)?;

    if let Some(value) = patch.name {
        root.metadata.name = Some(value);
    }
    if let Some(value) = patch.author {
        root.metadata.author = Some(value);
    }
    if let Some(value) = patch.description {
        root.metadata.description = Some(value);
    }
    if let Some(value) = patch.time_created {
        root.metadata.time_created = Some(value);
    }
    if let Some(value) = patch.time_modified {
        root.metadata.time_modified = Some(value);
    }
    if let Some(value) = patch.litematic_version {
        root.version = value;
    }
    if let Some(value) = patch.litematic_subversion {
        root.sub_version = Some(value);
    }
    if let Some(value) = patch.minecraft_data_version {
        if Some(value) != root.minecraft_data_version {
            bail!("Minecraft 数据版本变更须使用版本转换：先转换方块状态及相关 NBT，再另存新文件");
        }
    }

    if let Some(image_path) = patch.preview_image_path.as_deref() {
        root.metadata.preview_image_data = Some(load_preview_image_data(Path::new(image_path))?);
        root.preview_image_data = None;
    } else if let Some(value) = patch.preview_image_data {
        let edge = (value.len() as f64).sqrt() as usize;
        if edge * edge != value.len() || edge > 4096 {
            bail!("preview_image_data must be a square ARGB pixel array with edge at most 4096");
        }
        root.metadata.preview_image_data = Some(IntArray::new(value));
        root.preview_image_data = None;
    }

    let mut changed_regions = Vec::new();
    if !patch.regions.is_empty() {
        let mut targets = BTreeSet::new();
        for rename in &patch.regions {
            let new_name = rename.new_name.trim();
            if new_name.is_empty() {
                bail!("region name cannot be empty");
            }
            if !root.regions.contains_key(&rename.old_name) {
                bail!("region not found: {}", rename.old_name);
            }
            if !targets.insert(new_name.to_string()) {
                bail!("duplicate target region name: {new_name}");
            }
        }
        let mut next = BTreeMap::new();
        let rename_lookup: BTreeMap<_, _> = patch
            .regions
            .iter()
            .map(|item| (item.old_name.as_str(), item.new_name.trim().to_string()))
            .collect();
        for (name, region) in root.regions {
            let next_name = rename_lookup.get(name.as_str()).cloned().unwrap_or(name);
            if next.contains_key(&next_name) {
                bail!("duplicate region name after rename: {next_name}");
            }
            next.insert(next_name, region);
        }
        for rename in &patch.regions {
            if !next.contains_key(rename.new_name.trim()) {
                bail!("region not found: {}", rename.old_name);
            }
            changed_regions.push(RegionRenamePatchOutput {
                old_name: rename.old_name.clone(),
                new_name: rename.new_name.trim().to_string(),
            });
        }
        root.regions = next;
    }

    let target = output.unwrap_or(input);
    let mut backup_path = None;
    if output.is_none() {
        let backup = backup_path_for(input);
        fs::copy(input, &backup)
            .with_context(|| format!("failed to create backup {}", backup.display()))?;
        backup_path = Some(backup.display().to_string());
    } else if target.exists() {
        bail!(
            "refusing to overwrite existing output file: {}",
            target.display()
        );
    }

    if let Err(error) = save_litematic_root(target, &root) {
        if let Some(backup) = backup_path.as_ref() {
            let _ = fs::copy(backup, input);
        }
        return Err(error);
    }

    Ok(MetadataEditSummary {
        input: input.display().to_string(),
        output: target.display().to_string(),
        backup_path,
        changed_regions,
    })
}

fn load_preview_image_data(path: &Path) -> Result<IntArray> {
    let image = image::open(path).with_context(|| format!("failed to open preview image {}", path.display()))?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        bail!("preview image must not be empty");
    }

    let size = width.min(height);
    let x = (width - size) / 2;
    let y = (height - size) / 2;
    let cropped = image.crop_imm(x, y, size, size);
    let resized = cropped.resize_exact(140, 140, FilterType::Lanczos3).to_rgba8();

    let pixels = resized
        .pixels()
        .map(|pixel| {
            let [r, g, b, a] = pixel.0;
            (((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32) as i32
        })
        .collect();
    Ok(IntArray::new(pixels))
}

fn backup_path_for(input: &Path) -> PathBuf {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let mut backup = input.to_path_buf();
    let file_name = input
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("projection.litematic");
    backup.set_file_name(format!("{file_name}.bak.{now}"));
    backup
}

#[cfg(test)]
mod version_guard_tests {
    use super::*;
    use fastnbt::Value;
    use flate2::{write::GzEncoder, Compression};
    use std::collections::HashMap;
    use std::io::Write;

    #[test]
    fn version_change_fails_before_output_and_preserves_input() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("converted.litematic");
        let patch = dir.path().join("patch.json");
        let root = Value::Compound(HashMap::from([
            ("Metadata".into(), Value::Compound(HashMap::new())),
            ("Regions".into(), Value::Compound(HashMap::new())),
            ("Version".into(), Value::Int(6)),
            ("MinecraftDataVersion".into(), Value::Int(5023)),
        ]));
        let nbt = fastnbt::to_bytes(&root).unwrap();
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&nbt).unwrap();
        let original = gzip.finish().unwrap();
        fs::write(&input, &original).unwrap();
        fs::write(&patch, r#"{"name":"changed","minecraft_data_version":4903}"#).unwrap();
        let error = edit_metadata(&input, Some(&output), &patch).err().expect("version change must fail");
        assert!(error.to_string().contains("版本转换"));
        assert!(!output.exists());
        assert_eq!(fs::read(&input).unwrap(), original);
    }
}

#[cfg(test)]
mod preview_image_tests {
    use super::*;
    use crate::nbt::load_litematic_preview_image;

    #[test]
    fn png_preview_import_save_as_readback_and_clear_preserve_projection() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = Path::new(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/stats_water_fixture.litematic"));
        let input = dir.path().join("source.litematic");
        fs::copy(fixture,&input).unwrap();
        let original = fs::read(&input).unwrap();
        let root = load_litematic_root(&input).unwrap();
        let regions_before = fastnbt::to_bytes(&root.regions).unwrap();
        let data_version = root.minecraft_data_version;
        let image_path = dir.path().join("cropped.png");
        let image = image::RgbaImage::from_fn(12,8,|x,_|if (2..10).contains(&x) { image::Rgba([79,123,211,143]) } else { image::Rgba([255,0,0,255]) });
        image.save(&image_path).unwrap();
        let patch_path = dir.path().join("import.json");
        fs::write(&patch_path,serde_json::to_vec(&serde_json::json!({"preview_image_path":image_path.to_string_lossy()})).unwrap()).unwrap();
        let output = dir.path().join("with-preview.litematic");
        let summary = edit_metadata(&input,Some(&output),&patch_path).unwrap();
        assert!(summary.backup_path.is_none());
        assert_eq!(fs::read(&input).unwrap(),original);
        let reloaded = load_litematic_root(&output).unwrap();
        assert_eq!(fastnbt::to_bytes(&reloaded.regions).unwrap(),regions_before);
        assert_eq!(reloaded.minecraft_data_version,data_version);
        assert_eq!(reloaded.metadata.preview_image_data.as_ref().unwrap().len(),140*140);
        let preview = load_litematic_preview_image(&output).unwrap().unwrap();
        assert_eq!((preview.width,preview.height),(140,140));
        assert!(preview.rgba.chunks_exact(4).all(|pixel|pixel == [79,123,211,143]));
        let extracted = dir.path().join("extracted.png");
        let extracted_result = crate::projection_preview::extract_projection_preview(&output,&extracted).unwrap();
        assert!(extracted_result.found);
        let decoded = image::open(&extracted).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(70,70).0,[79,123,211,143]);
        let clear_path = dir.path().join("clear.json");
        fs::write(&clear_path,r#"{"preview_image_data":[]}"#).unwrap();
        let cleared = dir.path().join("cleared.litematic");
        edit_metadata(&output,Some(&cleared),&clear_path).unwrap();
        assert!(load_litematic_preview_image(&cleared).unwrap().is_none());
        let reread = load_litematic_root(&cleared).unwrap();
        assert_eq!(fastnbt::to_bytes(&reread.regions).unwrap(),regions_before);
        assert_eq!(reread.minecraft_data_version,data_version);
    }

    #[test]
    fn raw_argb_import_preserves_channels_and_rejects_invalid_pixels_before_write() {
        let dir = tempfile::tempdir().unwrap();
        let input = Path::new(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/stats_water_fixture.litematic"));
        let output = dir.path().join("raw.litematic");
        let patch = dir.path().join("raw.json");
        let pixel = 0x807fdf02_u32 as i32;
        fs::write(&patch,serde_json::to_vec(&serde_json::json!({"preview_image_data":[pixel]})).unwrap()).unwrap();
        edit_metadata(input,Some(&output),&patch).unwrap();
        let image = load_litematic_preview_image(&output).unwrap().unwrap();
        assert_eq!((image.width,image.height),(1,1));
        assert_eq!(image.rgba,vec![127,223,2,128]);
        let invalid = dir.path().join("invalid.litematic");
        fs::write(&patch,r#"{"preview_image_data":[1,2,3]}"#).unwrap();
        assert!(edit_metadata(input,Some(&invalid),&patch).is_err());
        assert!(!invalid.exists());
    }
}
