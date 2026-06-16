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
    let arc_root = load_litematic_root(input)?;
    let mut root = (*arc_root).clone();

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
        root.minecraft_data_version = Some(value);
    }
    if let Some(image_path) = patch.preview_image_path.as_deref() {
        root.metadata.preview_image_data = Some(load_preview_image_data(Path::new(image_path))?);
    } else if let Some(value) = patch.preview_image_data {
        root.metadata.preview_image_data = Some(IntArray::new(value));
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
