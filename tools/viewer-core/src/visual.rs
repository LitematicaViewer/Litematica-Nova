use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Result, anyhow};

use crate::model::{
    EnclosingSize, LayerBlockOutput, MetadataOutput, PaletteEntryOutput, VisualLayerOutput,
    VisualMetaOutput, VisualMetaSummaryOutput, VisualOutput, VisualSummaryOutput,
};
use crate::nbt::{
    LitematicRoot, RegionBounds, bits_for_palette, for_each_palette_index, load_litematic_root,
    region_bounds, region_volume, storage_to_region_coords,
};

#[derive(Clone)]
pub struct VisualCatalog {
    pub metadata: MetadataOutput,
    pub bounds: Option<RegionBounds>,
    pub palette: Vec<PaletteEntryOutput>,
    pub property_pool: Vec<BTreeMap<String, String>>,
    pub state_lookup: HashMap<String, usize>,
}

pub fn build_metadata_output(root: &LitematicRoot, bounds: Option<RegionBounds>) -> MetadataOutput {
    let enclosing = if let Some(bounds) = bounds {
        EnclosingSize {
            x: bounds.max_x - bounds.min_x + 1,
            y: bounds.max_y - bounds.min_y + 1,
            z: bounds.max_z - bounds.min_z + 1,
        }
    } else {
        root.metadata.enclosing_size.clone().unwrap_or_default()
    };

    MetadataOutput {
        name: root.metadata.name.clone().unwrap_or_default(),
        author: root.metadata.author.clone().unwrap_or_default(),
        description: root.metadata.description.clone().unwrap_or_default(),
        time_created: root.metadata.time_created.unwrap_or_default(),
        time_modified: root.metadata.time_modified.unwrap_or_default(),
        total_blocks: root.metadata.total_blocks.unwrap_or_default(),
        total_volume: root.metadata.total_volume.unwrap_or(
            enclosing
                .x
                .saturating_mul(enclosing.y)
                .saturating_mul(enclosing.z),
        ),
        region_count: root
            .metadata
            .region_count
            .unwrap_or(root.regions.len().min(i32::MAX as usize) as i32),
        enclosing_size: enclosing,
        litematic_version: root.version,
        litematic_subversion: root.sub_version.unwrap_or_default(),
        minecraft_data_version: root.minecraft_data_version.unwrap_or_default(),
    }
}

pub fn compute_bounds(root: &LitematicRoot) -> Option<RegionBounds> {
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

pub fn load_visual_catalog(path: &Path) -> Result<(LitematicRoot, VisualCatalog)> {
    let root = load_litematic_root(path)?;
    let bounds = compute_bounds(&root);
    let metadata = build_metadata_output(&root, bounds);

    let mut property_pool = Vec::<BTreeMap<String, String>>::new();
    let mut property_lookup = HashMap::<BTreeMap<String, String>, usize>::new();
    let mut palette = Vec::<PaletteEntryOutput>::new();
    let mut palette_lookup = HashMap::<(String, usize), usize>::new();
    let mut state_lookup = HashMap::<String, usize>::new();

    for region in root.regions.values() {
        for block in &region.block_state_palette {
            let state = format_block_state(block);
            if is_air_state(&state) {
                continue;
            }
            let property_id = *property_lookup
                .entry(block.properties.clone())
                .or_insert_with(|| {
                    let idx = property_pool.len();
                    property_pool.push(block.properties.clone());
                    idx
                });
            let palette_id = *palette_lookup
                .entry((block.name.clone(), property_id))
                .or_insert_with(|| {
                    let idx = palette.len();
                    palette.push(PaletteEntryOutput {
                        block_id: block.name.clone(),
                        property_id,
                    });
                    idx
                });
            state_lookup.insert(state, palette_id);
        }
    }

    Ok((
        (*root).clone(),
        VisualCatalog {
            metadata,
            bounds,
            palette,
            property_pool,
            state_lookup,
        },
    ))
}

pub fn build_visual_meta_output(path: &Path, chunk_size: u32) -> Result<VisualMetaOutput> {
    let (_root, catalog) = load_visual_catalog(path)?;
    let size_x = catalog.metadata.enclosing_size.x;
    let size_y = catalog.metadata.enclosing_size.y;
    let size_z = catalog.metadata.enclosing_size.z;
    Ok(VisualMetaOutput {
        metadata: catalog.metadata,
        visual: VisualMetaSummaryOutput {
            chunk_size,
            size_x,
            size_y,
            size_z,
            palette: catalog.palette,
            property_pool: catalog.property_pool,
        },
    })
}

pub fn build_visual_layer_output(
    path: &Path,
    chunk_size: u32,
    target_y: i32,
) -> Result<VisualLayerOutput> {
    let (root, catalog) = load_visual_catalog(path)?;
    let Some(bounds) = catalog.bounds else {
        return Ok(VisualLayerOutput {
            metadata: catalog.metadata,
            chunk_size,
            y: target_y,
            blocks: Vec::new(),
        });
    };

    let mut blocks = Vec::<LayerBlockOutput>::new();
    for region in root.regions.values() {
        let volume = region_volume(&region.size)?;
        let width = i64::from(region.size.x).unsigned_abs() as usize;
        let length = i64::from(region.size.z).unsigned_abs() as usize;
        let nbits = bits_for_palette(region.block_state_palette.len());

        for_each_palette_index(
            &region.block_states,
            volume,
            nbits,
            region.block_state_palette.len(),
            |index, palette_index| {
                let Some(block) = region.block_state_palette.get(palette_index) else {
                    return Ok(());
                };
                let state = format_block_state(block);
                if is_air_state(&state) {
                    return Ok(());
                }

                let y = index / (width * length);
                let i_in_layer = index % (width * length);
                let z = i_in_layer / width;
                let x = i_in_layer % width;
                let (rx, ry, rz) = storage_to_region_coords(x, y, z, &region.size);
                let global_y = region.position.y + ry - bounds.min_y;
                if global_y != target_y {
                    return Ok(());
                }
                let global_x = region.position.x + rx - bounds.min_x;
                let global_z = region.position.z + rz - bounds.min_z;
                let palette_id = *catalog
                    .state_lookup
                    .get(&state)
                    .ok_or_else(|| anyhow!("missing palette entry for state {}", state))?;

                blocks.push(LayerBlockOutput {
                    x: global_x,
                    z: global_z,
                    palette_id,
                });
                Ok(())
            },
        )?;
    }

    Ok(VisualLayerOutput {
        metadata: catalog.metadata,
        chunk_size,
        y: target_y,
        blocks,
    })
}

pub fn build_visual_output(path: &Path, chunk_size: u32) -> Result<VisualOutput> {
    let meta = build_visual_meta_output(path, chunk_size)?;
    Ok(VisualOutput {
        metadata: meta.metadata,
        visual: VisualSummaryOutput {
            chunk_size: meta.visual.chunk_size,
            size_x: meta.visual.size_x,
            size_y: meta.visual.size_y,
            size_z: meta.visual.size_z,
            palette: meta.visual.palette,
            property_pool: meta.visual.property_pool,
            layers: Vec::new(),
            chunks: Vec::new(),
        },
    })
}

pub fn format_block_state(block: &crate::model::BlockStateNbt) -> String {
    if block.properties.is_empty() {
        return block.name.clone();
    }

    let mut state = String::with_capacity(block.name.len() + 16);
    state.push_str(&block.name);
    state.push('[');
    for (index, (key, value)) in block.properties.iter().enumerate() {
        if index > 0 {
            state.push(',');
        }
        state.push_str(key);
        state.push('=');
        state.push_str(value);
    }
    state.push(']');
    state
}

pub fn is_air_state(state: &str) -> bool {
    matches!(
        state,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}
