use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use fastnbt::{LongArray, Value, from_reader, to_writer};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};

use crate::model::{BlockStateNbt, EnclosingSize, EntityNbt};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LitematicRoot {
    pub metadata: MetadataNbt,
    pub regions: BTreeMap<String, RegionNbt>,
    pub version: i32,
    pub sub_version: Option<i32>,
    #[serde(rename = "MinecraftDataVersion")]
    pub minecraft_data_version: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MetadataNbt {
    pub author: Option<String>,
    pub description: Option<String>,
    pub name: Option<String>,
    pub total_blocks: Option<i32>,
    pub total_volume: Option<i32>,
    pub region_count: Option<i32>,
    pub enclosing_size: Option<EnclosingSize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RegionNbt {
    pub position: Vec3i,
    pub size: Vec3i,
    pub block_state_palette: Vec<BlockStateNbt>,
    pub block_states: LongArray,
    #[serde(default)]
    pub entities: Vec<EntityNbt>,
    #[serde(default, rename = "TileEntities", alias = "BlockEntities")]
    pub tile_entities: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Vec3i {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct RegionBounds {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub min_z: i32,
    pub max_z: i32,
}

pub fn load_litematic_root(path: &Path) -> Result<LitematicRoot> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let decoder = GzDecoder::new(file);
    from_reader(decoder).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn save_litematic_root(path: &Path, root: &LitematicRoot) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let encoder = GzEncoder::new(file, Compression::default());
    to_writer(encoder, root).with_context(|| format!("failed to write {}", path.display()))
}

pub fn region_volume(size: &Vec3i) -> Result<usize> {
    let width = i64::from(size.x).unsigned_abs() as usize;
    let height = i64::from(size.y).unsigned_abs() as usize;
    let length = i64::from(size.z).unsigned_abs() as usize;
    width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(length))
        .ok_or_else(|| anyhow!("region volume overflow"))
}

pub fn bits_for_palette(palette_len: usize) -> usize {
    if palette_len <= 1 {
        return 2;
    }
    let bits = usize::BITS as usize - (palette_len - 1).leading_zeros() as usize;
    bits.max(2)
}

pub fn region_bounds(region: &RegionNbt) -> RegionBounds {
    let min_x = std::cmp::min(region.position.x, region.position.x + region.size.x + 1);
    let max_x = std::cmp::max(region.position.x, region.position.x + region.size.x - 1);
    let min_y = std::cmp::min(region.position.y, region.position.y + region.size.y + 1);
    let max_y = std::cmp::max(region.position.y, region.position.y + region.size.y - 1);
    let min_z = std::cmp::min(region.position.z, region.position.z + region.size.z + 1);
    let max_z = std::cmp::max(region.position.z, region.position.z + region.size.z - 1);
    RegionBounds {
        min_x,
        max_x,
        min_y,
        max_y,
        min_z,
        max_z,
    }
}

pub fn decode_palette_frequencies(
    block_states: &LongArray,
    size: usize,
    nbits: usize,
    palette_len: usize,
) -> Result<Vec<u64>> {
    let expected_len = (size * nbits).div_ceil(64);
    if block_states.len() != expected_len {
        bail!(
            "block state long array length mismatch, expected {}, got {}",
            expected_len,
            block_states.len()
        );
    }

    let mask = if nbits >= 64 {
        u64::MAX
    } else {
        (1_u64 << nbits) - 1
    };
    let mut counts = vec![0_u64; palette_len.max(1)];

    for index in 0..size {
        let start_offset = index * nbits;
        let start_arr_index = start_offset >> 6;
        let end_arr_index = (((index + 1) * nbits) - 1) >> 6;
        let start_bit_offset = start_offset & 0x3f;

        let value = if start_arr_index == end_arr_index {
            ((block_states[start_arr_index] as u64) >> start_bit_offset) & mask
        } else {
            let end_offset = 64 - start_bit_offset;
            (((block_states[start_arr_index] as u64) >> start_bit_offset)
                | ((block_states[end_arr_index] as u64) << end_offset))
                & mask
        } as usize;

        let slot = counts
            .get_mut(value)
            .ok_or_else(|| anyhow!("palette index {} out of range {}", value, palette_len))?;
        *slot += 1;
    }

    Ok(counts)
}

pub fn pack_palette_indices(indices: &[usize], nbits: usize) -> LongArray {
    let expected_len = (indices.len() * nbits).div_ceil(64);
    let mask = if nbits >= 64 {
        u64::MAX
    } else {
        (1_u64 << nbits) - 1
    };
    let mut packed = vec![0_u64; expected_len];

    for (index, palette_index) in indices.iter().copied().enumerate() {
        let value = (palette_index as u64) & mask;
        let start_offset = index * nbits;
        let start_arr_index = start_offset >> 6;
        let end_arr_index = (((index + 1) * nbits) - 1) >> 6;
        let start_bit_offset = start_offset & 0x3f;

        packed[start_arr_index] |= value << start_bit_offset;
        if start_arr_index != end_arr_index {
            let end_offset = 64 - start_bit_offset;
            packed[end_arr_index] |= value >> end_offset;
        }
    }

    LongArray::new(packed.into_iter().map(|value| value as i64).collect())
}

pub fn for_each_palette_index<F>(
    block_states: &LongArray,
    size: usize,
    nbits: usize,
    mut callback: F,
) -> Result<()>
where
    F: FnMut(usize, usize) -> Result<()>,
{
    let expected_len = (size * nbits).div_ceil(64);
    if block_states.len() != expected_len {
        bail!(
            "block state long array length mismatch, expected {}, got {}",
            expected_len,
            block_states.len()
        );
    }

    let mask = if nbits >= 64 {
        u64::MAX
    } else {
        (1_u64 << nbits) - 1
    };

    for index in 0..size {
        let start_offset = index * nbits;
        let start_arr_index = start_offset >> 6;
        let end_arr_index = (((index + 1) * nbits) - 1) >> 6;
        let start_bit_offset = start_offset & 0x3f;

        let value = if start_arr_index == end_arr_index {
            ((block_states[start_arr_index] as u64) >> start_bit_offset) & mask
        } else {
            let end_offset = 64 - start_bit_offset;
            (((block_states[start_arr_index] as u64) >> start_bit_offset)
                | ((block_states[end_arr_index] as u64) << end_offset))
                & mask
        } as usize;

        callback(index, value)?;
    }

    Ok(())
}

pub fn palette_index_at(
    block_states: &LongArray,
    size: usize,
    nbits: usize,
    index: usize,
) -> Result<usize> {
    if index >= size {
        bail!(
            "palette index request out of range: index {} size {}",
            index,
            size
        );
    }

    let expected_len = (size * nbits).div_ceil(64);
    if block_states.len() != expected_len {
        bail!(
            "block state long array length mismatch, expected {}, got {}",
            expected_len,
            block_states.len()
        );
    }

    let mask = if nbits >= 64 {
        u64::MAX
    } else {
        (1_u64 << nbits) - 1
    };
    let start_offset = index * nbits;
    let start_arr_index = start_offset >> 6;
    let end_arr_index = (((index + 1) * nbits) - 1) >> 6;
    let start_bit_offset = start_offset & 0x3f;

    let value = if start_arr_index == end_arr_index {
        ((block_states[start_arr_index] as u64) >> start_bit_offset) & mask
    } else {
        let end_offset = 64 - start_bit_offset;
        (((block_states[start_arr_index] as u64) >> start_bit_offset)
            | ((block_states[end_arr_index] as u64) << end_offset))
            & mask
    } as usize;

    Ok(value)
}

pub fn storage_to_region_coords(
    store_x: usize,
    store_y: usize,
    store_z: usize,
    size: &Vec3i,
) -> (i32, i32, i32) {
    let mut x = store_x as i32;
    let mut y = store_y as i32;
    let mut z = store_z as i32;
    if size.x < 0 {
        x += size.x + 1;
    }
    if size.y < 0 {
        y += size.y + 1;
    }
    if size.z < 0 {
        z += size.z + 1;
    }
    (x, y, z)
}

pub fn region_coord_to_storage_coord(region_coord: i32, axis_size: i32) -> Option<usize> {
    if axis_size >= 0 {
        if (0..axis_size).contains(&region_coord) {
            Some(region_coord as usize)
        } else {
            None
        }
    } else {
        let start = axis_size + 1;
        if (start..=0).contains(&region_coord) {
            Some((region_coord - start) as usize)
        } else {
            None
        }
    }
}
