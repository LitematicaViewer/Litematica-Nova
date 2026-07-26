use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::nbt::{
    LitematicRoot, RegionBounds, Vec3i, load_litematic_root, region_bounds, save_litematic_root,
};

/// A marker point in normalized layer-view coordinates.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ProjectionMarkerPoint {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Projection markers currently supported by the viewer.
#[derive(Debug, Serialize)]
pub struct ProjectionMarkersOutput {
    pub origin: Option<ProjectionMarkerPoint>,
}

/// Summary emitted after editing projection markers.
#[derive(Debug, Serialize)]
pub struct ProjectionMarkerEditSummary {
    pub input: String,
    pub output: String,
    pub backup_path: Option<String>,
    pub origin: Option<ProjectionMarkerPoint>,
}

/// Reads supported projection markers from a `.litematic` file.
pub fn read_projection_markers(input: &Path) -> Result<ProjectionMarkersOutput> {
    let root = load_litematic_root(input)?;
    Ok(markers_for_root(&root))
}

/// Moves the projection origin marker to the given layer-view coordinate.
pub fn set_projection_origin(
    input: &Path,
    output: Option<&Path>,
    x: i32,
    y: i32,
    z: i32,
) -> Result<ProjectionMarkerEditSummary> {
    let arc_root = load_litematic_root(input)?;
    let mut root = (*arc_root).clone();
    let bounds = root_bounds(&root).ok_or_else(|| anyhow::anyhow!("projection has no regions"))?;
    let target = ProjectionMarkerPoint { x, y, z };
    validate_marker_target(target, bounds)?;

    let shift = Vec3i {
        x: -(bounds.min_x + x),
        y: -(bounds.min_y + y),
        z: -(bounds.min_z + z),
    };
    shift_regions(&mut root, shift);
    touch_modified_time(&mut root);

    let (output_path, backup_path) = save_marker_root(input, output, &root)?;
    Ok(ProjectionMarkerEditSummary {
        input: input.display().to_string(),
        output: output_path.display().to_string(),
        backup_path: backup_path.map(|path| path.display().to_string()),
        origin: markers_for_root(&root).origin,
    })
}

/// Removes the projection origin marker from a `.litematic` file.
pub fn remove_projection_origin(
    input: &Path,
    output: Option<&Path>,
) -> Result<ProjectionMarkerEditSummary> {
    let arc_root = load_litematic_root(input)?;
    let mut root = (*arc_root).clone();
    let bounds = root_bounds(&root).ok_or_else(|| anyhow::anyhow!("projection has no regions"))?;
    let shift = Vec3i {
        x: -bounds.min_x,
        y: -bounds.min_y,
        z: -bounds.min_z,
    };
    shift_regions(&mut root, shift);
    touch_modified_time(&mut root);

    let (output_path, backup_path) = save_marker_root(input, output, &root)?;
    Ok(ProjectionMarkerEditSummary {
        input: input.display().to_string(),
        output: output_path.display().to_string(),
        backup_path: backup_path.map(|path| path.display().to_string()),
        origin: markers_for_root(&root).origin,
    })
}

fn markers_for_root(root: &LitematicRoot) -> ProjectionMarkersOutput {
    let Some(bounds) = root_bounds(root) else {
        return ProjectionMarkersOutput { origin: None };
    };
    if !has_non_default_origin(root) || !bounds_contains_origin(bounds) {
        return ProjectionMarkersOutput { origin: None };
    }

    ProjectionMarkersOutput {
        origin: Some(ProjectionMarkerPoint {
            x: -bounds.min_x,
            y: -bounds.min_y,
            z: -bounds.min_z,
        }),
    }
}

fn root_bounds(root: &LitematicRoot) -> Option<RegionBounds> {
    root.regions
        .values()
        .map(region_bounds)
        .reduce(|acc, item| RegionBounds {
            min_x: acc.min_x.min(item.min_x),
            max_x: acc.max_x.max(item.max_x),
            min_y: acc.min_y.min(item.min_y),
            max_y: acc.max_y.max(item.max_y),
            min_z: acc.min_z.min(item.min_z),
            max_z: acc.max_z.max(item.max_z),
        })
}

fn has_non_default_origin(root: &LitematicRoot) -> bool {
    root.regions
        .values()
        .any(|region| region.position.x != 0 || region.position.y != 0 || region.position.z != 0)
}

fn bounds_contains_origin(bounds: RegionBounds) -> bool {
    bounds.min_x <= 0
        && bounds.max_x >= 0
        && bounds.min_y <= 0
        && bounds.max_y >= 0
        && bounds.min_z <= 0
        && bounds.max_z >= 0
}

fn validate_marker_target(target: ProjectionMarkerPoint, bounds: RegionBounds) -> Result<()> {
    let size_x = bounds.max_x - bounds.min_x + 1;
    let size_y = bounds.max_y - bounds.min_y + 1;
    let size_z = bounds.max_z - bounds.min_z + 1;
    if target.x < 0
        || target.x >= size_x
        || target.y < 0
        || target.y >= size_y
        || target.z < 0
        || target.z >= size_z
    {
        bail!(
            "origin target out of bounds: ({}, {}, {}) for size ({}, {}, {})",
            target.x,
            target.y,
            target.z,
            size_x,
            size_y,
            size_z
        );
    }
    Ok(())
}

fn shift_regions(root: &mut LitematicRoot, shift: Vec3i) {
    for region in root.regions.values_mut() {
        region.position.x = region.position.x.saturating_add(shift.x);
        region.position.y = region.position.y.saturating_add(shift.y);
        region.position.z = region.position.z.saturating_add(shift.z);
    }
}

fn touch_modified_time(root: &mut LitematicRoot) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default();
    root.metadata.time_modified = Some(now);
}

fn save_marker_root(
    input: &Path,
    output: Option<&Path>,
    root: &LitematicRoot,
) -> Result<(PathBuf, Option<PathBuf>)> {
    if let Some(target) = output {
        if !paths_refer_to_same_file(input, target) {
            if let Some(parent) = target.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create output directory {}", parent.display())
                    })?;
                }
            }
            save_litematic_root(target, root)
                .with_context(|| format!("failed to save projection to {}", target.display()))?;
            return Ok((target.to_path_buf(), None));
        }
    }

    let backup = backup_path_for(input);
    fs::copy(input, &backup)
        .with_context(|| format!("failed to create backup {}", backup.display()))?;
    if let Err(error) = save_litematic_root(input, root) {
        let _ = fs::copy(&backup, input);
        return Err(error);
    }
    Ok((input.to_path_buf(), Some(backup)))
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    let Ok(left_canonical) = left.canonicalize() else {
        return false;
    };
    let Ok(right_canonical) = right.canonicalize() else {
        return false;
    };
    left_canonical == right_canonical
}

fn backup_path_for(input: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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
mod tests {
    use super::*;
    use crate::model::{BlockStateNbt, EnclosingSize};
    use crate::nbt::{MetadataNbt, RegionNbt};
    use fastnbt::LongArray;
    use std::collections::BTreeMap;

    #[test]
    fn origin_marker_uses_world_origin_relative_to_bounds() {
        let root = root_with_position(Vec3i {
            x: -1,
            y: -2,
            z: -3,
        });
        assert_eq!(
            markers_for_root(&root).origin,
            Some(ProjectionMarkerPoint { x: 1, y: 2, z: 3 })
        );
    }

    #[test]
    fn default_region_position_has_no_marker() {
        let root = root_with_position(Vec3i { x: 0, y: 0, z: 0 });
        assert_eq!(markers_for_root(&root).origin, None);
    }

    fn root_with_position(position: Vec3i) -> LitematicRoot {
        let region = RegionNbt {
            position,
            size: Vec3i { x: 5, y: 3, z: 4 },
            block_state_palette: vec![BlockStateNbt {
                name: "minecraft:stone".to_string(),
                properties: BTreeMap::new(),
            }],
            block_states: LongArray::new(vec![0]),
            entities: Vec::new(),
            pending_block_ticks: Vec::new(),
            pending_fluid_ticks: Vec::new(),
            tile_entities: Vec::new(),
        };
        LitematicRoot {
            metadata: MetadataNbt {
                author: None,
                description: None,
                name: None,
                time_created: None,
                time_modified: None,
                total_blocks: Some(1),
                total_volume: Some(60),
                region_count: Some(1),
                enclosing_size: Some(EnclosingSize { x: 5, y: 3, z: 4 }),
                preview_image_data: None,
            },
            regions: BTreeMap::from([("main".to_string(), region)]),
            version: 6,
            sub_version: Some(1),
            minecraft_data_version: Some(3700),
        }
    }
}
