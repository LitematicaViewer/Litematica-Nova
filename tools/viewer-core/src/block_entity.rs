use std::path::Path;
use anyhow::Result;
use fastnbt::Value as NbtValue;
use serde::Serialize;

use crate::nbt::{load_litematic_root, region_coord_to_storage_coord, region_volume, palette_index_at};

#[derive(Debug, Clone, Serialize)]
pub struct BlockEntityOutput {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block_id: String,
    pub nbt: NbtValue,
}

/// 读取指定世界坐标位置的 block entity
pub fn read_block_entity_at(
    path: &Path,
    world_x: i32,
    world_y: i32,
    world_z: i32,
) -> Result<Option<BlockEntityOutput>> {
    let root = load_litematic_root(path)?;
    
    eprintln!("[DEBUG] 查找坐标: world({}, {}, {})", world_x, world_y, world_z);
    
    // 遍历所有区域查找匹配的 block entity
    for (region_name, region) in &root.regions {
        eprintln!("[DEBUG] 检查区域: {}, position({}, {}, {}), size({}, {}, {})", 
            region_name, region.position.x, region.position.y, region.position.z,
            region.size.x, region.size.y, region.size.z);
        
        // 检查坐标是否在此区域范围内
        let rel_x = world_x - region.position.x;
        let rel_y = world_y - region.position.y;
        let rel_z = world_z - region.position.z;
        
        eprintln!("[DEBUG] 相对坐标: ({}, {}, {})", rel_x, rel_y, rel_z);
        
        // 转换为存储坐标
        let Some(store_x) = region_coord_to_storage_coord(rel_x, region.size.x) else {
            eprintln!("[DEBUG] store_x 超出范围");
            continue;
        };
        let Some(store_y) = region_coord_to_storage_coord(rel_y, region.size.y) else {
            eprintln!("[DEBUG] store_y 超出范围");
            continue;
        };
        let Some(store_z) = region_coord_to_storage_coord(rel_z, region.size.z) else {
            eprintln!("[DEBUG] store_z 超出范围");
            continue;
        };
        
        eprintln!("[DEBUG] 存储坐标: ({}, {}, {})", store_x, store_y, store_z);
        eprintln!("[DEBUG] tile_entities 数量: {}", region.tile_entities.len());
        
        // 查找此位置的方块
        let volume = region_volume(&region.size)?;
        let width = i64::from(region.size.x).unsigned_abs() as usize;
        let length = i64::from(region.size.z).unsigned_abs() as usize;
        let index = store_y * (width * length) + store_z * width + store_x;
        
        let nbits = crate::nbt::bits_for_palette(region.block_state_palette.len());
        let palette_index = palette_index_at(
            &region.block_states,
            volume,
            nbits,
            region.block_state_palette.len(),
            index,
        )?;
        
        let Some(block_state) = region.block_state_palette.get(palette_index) else {
            continue;
        };
        
        let block_id = &block_state.name;
        eprintln!("[DEBUG] 找到方块: {}", block_id);
        
        // 查找匹配的 tile entity
        for (idx, tile_entity) in region.tile_entities.iter().enumerate() {
            // fastnbt::Value 可能是 Compound
            let compound = match tile_entity {
                NbtValue::Compound(c) => c,
                _ => continue,
            };
            
            // 尝试读取 tile entity 的坐标 (支持 x/X, y/Y, z/Z)
            let te_x = get_i32_case_insensitive(compound, "x");
            let te_y = get_i32_case_insensitive(compound, "y");
            let te_z = get_i32_case_insensitive(compound, "z");
            
            eprintln!("[DEBUG] tile_entity[{}]: x={:?}, y={:?}, z={:?}", idx, te_x, te_y, te_z);
            
            if let (Some(te_x), Some(te_y), Some(te_z)) = (te_x, te_y, te_z) {
                // 当 size 为负数时，tile_entity 使用存储坐标；否则使用相对坐标
                let matches = if region.size.x < 0 || region.size.y < 0 || region.size.z < 0 {
                    eprintln!("[DEBUG] 使用存储坐标比较: te({}, {}, {}) vs store({}, {}, {})", te_x, te_y, te_z, store_x, store_y, store_z);
                    te_x == store_x as i32 && te_y == store_y as i32 && te_z == store_z as i32
                } else {
                    eprintln!("[DEBUG] 使用相对坐标比较: te({}, {}, {}) vs rel({}, {}, {})", te_x, te_y, te_z, rel_x, rel_y, rel_z);
                    te_x == rel_x && te_y == rel_y && te_z == rel_z
                };
                
                if matches {
                    eprintln!("[DEBUG] 找到匹配！");
                    return Ok(Some(BlockEntityOutput {
                        x: world_x,
                        y: world_y,
                        z: world_z,
                        block_id: block_id.clone(),
                        nbt: tile_entity.clone(),
                    }));
                }
            }
        }
    }
    
    eprintln!("[DEBUG] 未找到匹配的 tile_entity");
    Ok(None)
}

fn get_i32_case_insensitive(compound: &std::collections::HashMap<String, NbtValue>, key: &str) -> Option<i32> {
    // 先尝试直接匹配
    if let Some(value) = compound.get(key) {
        if let NbtValue::Int(i) = value {
            return Some(*i);
        }
        if let NbtValue::Long(l) = value {
            return Some(*l as i32);
        }
    }
    
    // 大小写不敏感匹配
    let lower_key = key.to_lowercase();
    for (k, v) in compound {
        if k.to_lowercase() == lower_key {
            if let NbtValue::Int(i) = v {
                return Some(*i);
            }
            if let NbtValue::Long(l) = v {
                return Some(*l as i32);
            }
        }
    }
    
    None
}