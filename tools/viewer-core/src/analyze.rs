use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, anyhow};

use crate::business::build_analysis_derived;
use crate::model::{AnalysisOutput, AnalysisSummary};
use crate::nbt::{
    bits_for_palette, decode_palette_frequencies, load_litematic_root, region_bounds, region_volume,
};
use crate::visual::build_metadata_output;

pub fn analyze_litematic(path: &Path, include_entities: bool) -> Result<AnalysisOutput> {
    let root = load_litematic_root(path)?;

    let mut block_counts: HashMap<String, u64> = HashMap::new();
    let mut entity_counts: HashMap<String, u64> = HashMap::new();
    let mut total_non_air_blocks = 0_u64;

    for region in root.regions.values() {
        let volume = region_volume(&region.size)?;
        let nbits = bits_for_palette(region.block_state_palette.len());
        let frequencies = decode_palette_frequencies(
            &region.block_states,
            volume,
            nbits,
            region.block_state_palette.len(),
        )?;

        for (palette_index, frequency) in frequencies.into_iter().enumerate() {
            if frequency == 0 {
                continue;
            }

            let state = crate::visual::format_block_state(
                region
                    .block_state_palette
                    .get(palette_index)
                    .ok_or_else(|| anyhow!("palette index {} out of range", palette_index))?,
            );
            if crate::visual::is_air_state(&state) {
                continue;
            }

            total_non_air_blocks += frequency;
            *block_counts.entry(state).or_insert(0) += frequency;
        }

        if include_entities {
            for entity in &region.entities {
                if let Some(entity_id) = entity.entity_id() {
                    *entity_counts.entry(entity_id.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let bounds = root
        .regions
        .values()
        .map(region_bounds)
        .reduce(|acc, item| crate::nbt::RegionBounds {
            min_x: acc.min_x.min(item.min_x),
            max_x: acc.max_x.max(item.max_x),
            min_y: acc.min_y.min(item.min_y),
            max_y: acc.max_y.max(item.max_y),
            min_z: acc.min_z.min(item.min_z),
            max_z: acc.max_z.max(item.max_z),
        });

    let metadata = build_metadata_output(&root, bounds);
    let derived = build_analysis_derived(
        &block_counts,
        metadata.total_volume,
        (
            metadata.enclosing_size.x,
            metadata.enclosing_size.y,
            metadata.enclosing_size.z,
        ),
        total_non_air_blocks,
    );
    Ok(AnalysisOutput {
        metadata,
        analysis: AnalysisSummary {
            total_non_air_blocks,
            block_counts,
            entity_counts,
        },
        derived,
    })
}
