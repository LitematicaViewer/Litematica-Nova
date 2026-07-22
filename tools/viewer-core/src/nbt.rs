use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::num::NonZeroUsize;

use anyhow::{Context, Result, anyhow, bail};
use fastnbt::{IntArray, LongArray, Value, from_reader, to_writer};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use lru::LruCache;
use serde::{Deserialize, Serialize};

use crate::model::{BlockStateNbt, EnclosingSize, EntityNbt};

// 全局 LRU 缓存,存储最近访问的 litematic 文件
// 默认缓存 10 个文件
lazy_static::lazy_static! {
    static ref LITEMATIC_CACHE: Mutex<LruCache<PathBuf, Arc<LitematicRoot>>> = {
        Mutex::new(LruCache::new(NonZeroUsize::new(10).unwrap()))
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LitematicRoot {
    pub metadata: MetadataNbt,
    pub regions: BTreeMap<String, RegionNbt>,
    pub version: i32,
    pub sub_version: Option<i32>,
    #[serde(rename = "MinecraftDataVersion")]
    pub minecraft_data_version: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MetadataNbt {
    pub author: Option<String>,
    pub description: Option<String>,
    pub name: Option<String>,
    pub time_created: Option<i64>,
    pub time_modified: Option<i64>,
    pub total_blocks: Option<i32>,
    pub total_volume: Option<i32>,
    pub region_count: Option<i32>,
    pub enclosing_size: Option<EnclosingSize>,
    pub preview_image_data: Option<IntArray>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub fn load_litematic_root(path: &Path) -> Result<Arc<LitematicRoot>> {
    // 规范化路径用于缓存键
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    
    // 检查缓存
    if let Ok(mut cache) = LITEMATIC_CACHE.lock() {
        if let Some(cached) = cache.get(&canonical_path) {
            return Ok(Arc::clone(cached));
        }
    }
    
    // 缓存未命中,从磁盘加载
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let decoder = GzDecoder::new(file);
    let root: LitematicRoot = from_reader(decoder).with_context(|| format!("failed to parse {}", path.display()))?;
    let arc_root = Arc::new(root);
    
    // 存入缓存
    if let Ok(mut cache) = LITEMATIC_CACHE.lock() {
        cache.put(canonical_path, Arc::clone(&arc_root));
    }
    
    Ok(arc_root)
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

#[derive(Debug, Clone, Copy)]
enum PalettePacking {
    CompactBitstream,
    PaddedLongs,
}

#[derive(Debug, Clone, Copy)]
struct PaletteLayout {
    packing: PalettePacking,
    nbits: usize,
}

fn compact_block_states_len(size: usize, nbits: usize) -> usize {
    (size * nbits).div_ceil(64)
}

fn padded_block_states_len(size: usize, nbits: usize) -> usize {
    let values_per_long = (64 / nbits).max(1);
    size.div_ceil(values_per_long)
}

fn palette_mask(nbits: usize) -> u64 {
    if nbits >= 64 {
        u64::MAX
    } else {
        (1_u64 << nbits) - 1
    }
}

fn palette_index_at_with_packing(
    block_states: &LongArray,
    nbits: usize,
    index: usize,
    packing: PalettePacking,
) -> usize {
    let mask = palette_mask(nbits);
    match packing {
        PalettePacking::CompactBitstream => {
            let start_offset = index * nbits;
            let start_arr_index = start_offset >> 6;
            let end_arr_index = (((index + 1) * nbits) - 1) >> 6;
            let start_bit_offset = start_offset & 0x3f;

            if start_arr_index == end_arr_index {
                (((block_states[start_arr_index] as u64) >> start_bit_offset) & mask) as usize
            } else {
                let end_offset = 64 - start_bit_offset;
                ((((block_states[start_arr_index] as u64) >> start_bit_offset)
                    | ((block_states[end_arr_index] as u64) << end_offset))
                    & mask) as usize
            }
        }
        PalettePacking::PaddedLongs => {
            let values_per_long = (64 / nbits).max(1);
            let arr_index = index / values_per_long;
            let bit_offset = (index % values_per_long) * nbits;
            (((block_states[arr_index] as u64) >> bit_offset) & mask) as usize
        }
    }
}

fn choose_palette_packing(
    block_states: &LongArray,
    size: usize,
    nbits: usize,
    palette_len: usize,
) -> Result<PaletteLayout> {
    let compact_len = compact_block_states_len(size, nbits);
    let padded_len = padded_block_states_len(size, nbits);
    let actual_len = block_states.len();

    let compact_possible = actual_len >= compact_len;
    let padded_possible = actual_len >= padded_len;
    if !compact_possible && !padded_possible {
        if let Some(layout) = choose_legacy_underpacked_palette_layout(
            block_states,
            size,
            nbits,
            palette_len,
            actual_len,
        ) {
            return Ok(layout);
        }

        bail!(
            "block state long array length mismatch, expected at least compact {} or padded {}, got {}",
            compact_len,
            padded_len,
            actual_len
        );
    }

    let layout_is_valid = |packing: PalettePacking, layout_bits: usize| {
        (0..size).all(|index| {
            palette_index_at_with_packing(block_states, layout_bits, index, packing)
                < palette_len.max(1)
        })
    };

    match (compact_possible, padded_possible) {
        (true, false) => Ok(PaletteLayout {
            packing: PalettePacking::CompactBitstream,
            nbits,
        }),
        (false, true) => Ok(PaletteLayout {
            packing: PalettePacking::PaddedLongs,
            nbits,
        }),
        (true, true) => {
            let compact_valid = layout_is_valid(PalettePacking::CompactBitstream, nbits);
            let padded_valid = layout_is_valid(PalettePacking::PaddedLongs, nbits);
            match (compact_valid, padded_valid) {
                (true, _) => Ok(PaletteLayout {
                    packing: PalettePacking::CompactBitstream,
                    nbits,
                }),
                (false, true) => Ok(PaletteLayout {
                    packing: PalettePacking::PaddedLongs,
                    nbits,
                }),
                (false, false) => Ok(PaletteLayout {
                    packing: PalettePacking::CompactBitstream,
                    nbits,
                }),
            }
        }
        (false, false) => unreachable!("invalid palette packing length was rejected above"),
    }
}

fn choose_legacy_underpacked_palette_layout(
    block_states: &LongArray,
    size: usize,
    expected_nbits: usize,
    palette_len: usize,
    actual_len: usize,
) -> Option<PaletteLayout> {
    if expected_nbits <= 2 {
        return None;
    }

    for candidate_bits in (2..expected_nbits).rev() {
        let compact_exact = actual_len == compact_block_states_len(size, candidate_bits);
        let padded_exact = actual_len == padded_block_states_len(size, candidate_bits);

        for packing in [
            PalettePacking::CompactBitstream,
            PalettePacking::PaddedLongs,
        ] {
            let matches_len = match packing {
                PalettePacking::CompactBitstream => compact_exact,
                PalettePacking::PaddedLongs => padded_exact,
            };
            if !matches_len {
                continue;
            }

            let layout_is_valid = (0..size).all(|index| {
                palette_index_at_with_packing(block_states, candidate_bits, index, packing)
                    < palette_len.max(1)
            });
            if layout_is_valid {
                return Some(PaletteLayout {
                    packing,
                    nbits: candidate_bits,
                });
            }
        }
    }

    None
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
    let layout = choose_palette_packing(block_states, size, nbits, palette_len)?;
    let mut counts = vec![0_u64; palette_len.max(1)];

    for index in 0..size {
        let value =
            palette_index_at_with_packing(block_states, layout.nbits, index, layout.packing);
        if let Some(slot) = counts.get_mut(value) {
            *slot += 1;
        }
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
    palette_len: usize,
    mut callback: F,
) -> Result<()>
where
    F: FnMut(usize, usize) -> Result<()>,
{
    let layout = choose_palette_packing(block_states, size, nbits, palette_len)?;

    for index in 0..size {
        let value =
            palette_index_at_with_packing(block_states, layout.nbits, index, layout.packing);
        callback(index, value)?;
    }

    Ok(())
}

pub fn palette_index_at(
    block_states: &LongArray,
    size: usize,
    nbits: usize,
    palette_len: usize,
    index: usize,
) -> Result<usize> {
    if index >= size {
        bail!(
            "palette index request out of range: index {} size {}",
            index,
            size
        );
    }

    let layout = choose_palette_packing(block_states, size, nbits, palette_len)?;
    Ok(palette_index_at_with_packing(
        block_states,
        layout.nbits,
        index,
        layout.packing,
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_legacy_underpacked_palette_when_extra_palette_entry_is_unused() {
        let indices: Vec<usize> = (0..1000).map(|index| index % 32).collect();
        let block_states = pack_palette_indices(&indices, 5);

        let mut decoded = Vec::new();
        for_each_palette_index(
            &block_states,
            indices.len(),
            bits_for_palette(33),
            33,
            |_, value| {
                decoded.push(value);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(decoded, indices);
    }

    #[test]
    fn rejects_underpacked_palette_when_actual_length_does_not_match_known_layout() {
        let block_states = LongArray::new(vec![0]);
        let error =
            for_each_palette_index(&block_states, 1000, bits_for_palette(33), 33, |_, _| Ok(()))
                .unwrap_err()
                .to_string();

        assert!(error.contains("block state long array length mismatch"));
    }
}
