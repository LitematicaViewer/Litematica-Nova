use std::path::Path;

use anyhow::{Result, anyhow};
use fastnbt::Value;
use serde::Serialize;

use crate::nbt::{
    LitematicRoot, bits_for_palette, load_litematic_root, palette_index_at,
    region_coord_to_storage_coord, region_volume,
};
use crate::visual::compute_bounds;

#[derive(Debug, Clone, Serialize)]
pub struct BlockEntityOutput {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block_id: String,
    pub nbt: Value,
}

/// Compatibility CLI: x/y/z are world coordinates relative to schematic origin.
pub fn read_block_entity_at(
    path: &Path,
    x: i32,
    y: i32,
    z: i32,
) -> Result<Option<BlockEntityOutput>> {
    read_world(&load_litematic_root(path)?, [x, y, z])
}

/// Product layer UI uses enclosing-box normalized coordinates, exactly as
/// visualize-layer. The region name is not an alternative coordinate origin.
pub fn read_layer_container(
    path: &Path,
    x: i32,
    y: i32,
    z: i32,
) -> Result<Option<BlockEntityOutput>> {
    read_layer(&load_litematic_root(path)?, [x, y, z])
}

fn read_layer(root: &LitematicRoot, layer: [i32; 3]) -> Result<Option<BlockEntityOutput>> {
    let Some(bounds) = compute_bounds(root) else {
        return Ok(None);
    };
    let minimum = [bounds.min_x, bounds.min_y, bounds.min_z];
    let maximum = [bounds.max_x, bounds.max_y, bounds.max_z];
    let mut world = [0; 3];
    for axis in 0..3 {
        let coordinate = i64::from(layer[axis]) + i64::from(minimum[axis]);
        if layer[axis] < 0 || coordinate > i64::from(maximum[axis]) {
            return Ok(None);
        }
        world[axis] =
            i32::try_from(coordinate).map_err(|_| anyhow!("layer coordinate overflow"))?;
    }
    Ok(read_world(root, world)?.map(|mut result| {
        [result.x, result.y, result.z] = layer;
        result
    }))
}

fn read_world(root: &LitematicRoot, world: [i32; 3]) -> Result<Option<BlockEntityOutput>> {
    for region in root.regions.values() {
        let position = [region.position.x, region.position.y, region.position.z];
        let size = [region.size.x, region.size.y, region.size.z];
        let mut storage = [0; 3];
        let mut contains = true;
        for axis in 0..3 {
            let relative = i64::from(world[axis]) - i64::from(position[axis]);
            let coordinate = i32::try_from(relative)
                .ok()
                .and_then(|coordinate| region_coord_to_storage_coord(coordinate, size[axis]));
            match coordinate {
                Some(coordinate) => storage[axis] = coordinate,
                None => {
                    contains = false;
                    break;
                }
            }
        }
        if !contains {
            continue;
        }
        // Litematic TileEntities coordinates address the region's stored
        // min-corner array, including each negative-sized axis. The same
        // region_coord_to_storage_coord inversion is used by layer blocks.
        let Some(tile_entity) = region.tile_entities.iter().find(|value| {
            let Value::Compound(compound) = value else {
                return false;
            };
            ["x", "y", "z"].iter().enumerate().all(|(axis, key)| {
                coordinate(compound, key).is_some_and(|v| i64::from(v) == storage[axis] as i64)
            })
        }) else {
            continue;
        };
        let volume = region_volume(&region.size)?;
        let width = i64::from(region.size.x).unsigned_abs() as usize;
        let length = i64::from(region.size.z).unsigned_abs() as usize;
        let index = storage[1] * (width * length) + storage[2] * width + storage[0];
        let palette_index = palette_index_at(
            &region.block_states,
            volume,
            bits_for_palette(region.block_state_palette.len()),
            region.block_state_palette.len(),
            index,
        )?;
        let block = region
            .block_state_palette
            .get(palette_index)
            .ok_or_else(|| anyhow!("container block palette index out of range"))?;
        return Ok(Some(BlockEntityOutput {
            x: world[0],
            y: world[1],
            z: world[2],
            block_id: block.name.clone(),
            nbt: tile_entity.clone(),
        }));
    }
    Ok(None)
}

fn coordinate(compound: &std::collections::HashMap<String, Value>, key: &str) -> Option<i32> {
    let value = compound.get(key).or_else(|| {
        compound
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    })?;
    match value {
        Value::Int(value) => Some(*value),
        Value::Long(value) => i32::try_from(*value).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BlockStateNbt;
    use crate::nbt::{MetadataNbt, RegionNbt, Vec3i, pack_palette_indices, save_litematic_root};
    use std::collections::{BTreeMap, HashMap};

    fn region(position: Vec3i, size: Vec3i, at: [i32; 3], block_id: &str) -> RegionNbt {
        RegionNbt {
            position,
            size,
            block_state_palette: vec![BlockStateNbt {
                name: block_id.into(),
                properties: BTreeMap::new(),
            }],
            block_states: pack_palette_indices(&vec![0; region_volume(&size).unwrap()], 2),
            entities: Vec::new(),
            tile_entities: vec![Value::Compound(HashMap::from([
                ("x".into(), Value::Int(at[0])),
                ("Y".into(), Value::Long(i64::from(at[1]))),
                ("z".into(), Value::Int(at[2])),
                ("Items".into(), Value::List(Vec::new())),
            ]))],
        }
    }

    fn fixture() -> LitematicRoot {
        LitematicRoot {
            version: 6,
            sub_version: Some(1),
            minecraft_data_version: Some(3700),
            metadata: MetadataNbt {
                author: None,
                description: None,
                name: None,
                time_created: None,
                time_modified: None,
                total_blocks: None,
                total_volume: None,
                region_count: None,
                enclosing_size: None,
                preview_image_data: None,
            },
            regions: BTreeMap::from([
                (
                    "negative".into(),
                    region(
                        Vec3i {
                            x: -10,
                            y: -4,
                            z: -6,
                        },
                        Vec3i { x: -3, y: -2, z: 4 },
                        [1, 0, 2],
                        "minecraft:chest",
                    ),
                ),
                (
                    "positive".into(),
                    region(
                        Vec3i { x: 5, y: 2, z: 3 },
                        Vec3i { x: 2, y: 2, z: 2 },
                        [0, 1, 1],
                        "minecraft:barrel",
                    ),
                ),
            ]),
            preview_image_data: None,
        }
    }

    #[test]
    fn layer_container_handles_negative_origin_and_signed_multi_region_sizes() {
        let root = fixture();
        // Bounds min = (-12, -5, -6); the stored chest is world (-11, -5, -4).
        let chest = read_layer(&root, [1, 0, 2]).unwrap().unwrap();
        assert_eq!((chest.x, chest.y, chest.z), (1, 0, 2));
        assert_eq!(chest.block_id, "minecraft:chest");
        let barrel = read_layer(&root, [17, 8, 10]).unwrap().unwrap();
        assert_eq!(barrel.block_id, "minecraft:barrel");
        assert!(read_layer(&root, [0, 0, 0]).unwrap().is_none());
        assert!(read_layer(&root, [-1, 0, 0]).unwrap().is_none());
        assert!(read_layer(&root, [i32::MAX, 0, 0]).unwrap().is_none());
        assert_eq!(
            read_world(&root, [-11, -5, -4]).unwrap().unwrap().block_id,
            "minecraft:chest"
        );
    }

    #[test]
    fn layer_container_matches_file_layer_block_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("containers.litematic");
        save_litematic_root(&input, &fixture()).unwrap();
        let layer = crate::visual::build_visual_layer_output(&input, 32, 0).unwrap();
        assert!(
            layer
                .blocks
                .iter()
                .any(|block| block.x == 1 && block.z == 2)
        );
        let entity = read_layer_container(&input, 1, 0, 2).unwrap().unwrap();
        assert_eq!(entity.block_id, "minecraft:chest");
        let json = serde_json::to_value(&entity).unwrap();
        assert_eq!(json["nbt"]["Items"], serde_json::json!([]));
    }

    #[test]
    fn container_coordinate_rejects_wrapping_long() {
        let values = HashMap::from([("x".into(), Value::Long(i64::from(i32::MAX) + 1))]);
        assert_eq!(coordinate(&values, "x"), None);
    }
}
