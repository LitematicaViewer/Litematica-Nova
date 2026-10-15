use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::model::{
    LayerSliceOutput, VisualAllLayersOutput, VisualLayerOutput, VisualMetaOutput,
    VisualMetaSummaryOutput, VisualOutput,
};

#[derive(Debug, Deserialize)]
struct CacheLayerManifest {
    #[serde(default)]
    layer_index_file: Option<String>,
}

fn layer_index_path(cache_manifest_path: &Path) -> Result<PathBuf> {
    let file = File::open(cache_manifest_path).with_context(|| {
        format!(
            "open cache manifest failed: {}",
            cache_manifest_path.display()
        )
    })?;
    let manifest: CacheLayerManifest =
        serde_json::from_reader(BufReader::new(file)).with_context(|| {
            format!(
                "parse cache manifest failed: {}",
                cache_manifest_path.display()
            )
        })?;
    let Some(layer_index_file) = manifest.layer_index_file else {
        bail!("cache does not contain layer_index_file; rebuild 3D cache before using layer view");
    };
    let parent = cache_manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.join(layer_index_file))
}

fn read_layer_index(cache_manifest_path: &Path) -> Result<VisualOutput> {
    let sidecar = layer_index_path(cache_manifest_path)?;
    let file = File::open(&sidecar)
        .with_context(|| format!("open cache layer index failed: {}", sidecar.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("parse cache layer index failed: {}", sidecar.display()))
}

pub fn build_cache_layer_meta_output(cache_manifest_path: &Path) -> Result<VisualMetaOutput> {
    let sidecar = read_layer_index(cache_manifest_path)?;
    Ok(VisualMetaOutput {
        metadata: sidecar.metadata,
        visual: VisualMetaSummaryOutput {
            chunk_size: sidecar.visual.chunk_size,
            size_x: sidecar.visual.size_x,
            size_y: sidecar.visual.size_y,
            size_z: sidecar.visual.size_z,
            palette: sidecar.visual.palette,
            property_pool: sidecar.visual.property_pool,
        },
    })
}

pub fn build_cache_layer_output(
    cache_manifest_path: &Path,
    target_y: i32,
) -> Result<VisualLayerOutput> {
    let sidecar = read_layer_index(cache_manifest_path)?;
    let blocks = sidecar
        .visual
        .layers
        .into_iter()
        .find(|layer| layer.y == target_y)
        .unwrap_or_else(|| LayerSliceOutput {
            y: target_y,
            blocks: Vec::new(),
        })
        .blocks;
    Ok(VisualLayerOutput {
        metadata: sidecar.metadata,
        chunk_size: sidecar.visual.chunk_size,
        y: target_y,
        blocks,
    })
}

/// Original complete bulk reader: decode the layer sidecar once, not per Y.
pub fn build_cache_all_layers_output(
    cache_manifest_path: &Path,
) -> Result<VisualAllLayersOutput> {
    let sidecar = read_layer_index(cache_manifest_path)?;
    Ok(VisualAllLayersOutput {
        metadata: sidecar.metadata,
        chunk_size: sidecar.visual.chunk_size,
        layers: sidecar.visual.layers,
    })
}

pub fn assert_cache_has_layer_index(cache_manifest_path: &Path) -> Result<()> {
    let path = layer_index_path(cache_manifest_path)?;
    if !path.is_file() {
        return Err(anyhow!(
            "cache layer index file is missing: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod bulk_layer_cache_tests {
    use super::*;
    use crate::model::{EnclosingSize, LayerBlockOutput, MetadataOutput, PaletteEntryOutput, VisualSummaryOutput};
    use std::collections::BTreeMap;

    #[test]
    fn bulk_sidecar_save_read_matches_meta_single_and_reflects_saved_changes() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("render_manifest.json");
        let index = dir.path().join("layer_index.json");
        std::fs::write(&manifest, br#"{"layer_index_file":"layer_index.json"}"#).unwrap();
        let mut source = VisualOutput {
            metadata: MetadataOutput {
                name: "layer cache".into(), author: String::new(), description: String::new(),
                time_created: 1, time_modified: 2, total_blocks: 2, total_volume: 8, region_count: 1,
                enclosing_size: EnclosingSize { x: 2, y: 2, z: 2 },
                litematic_version: 6, litematic_subversion: 1, minecraft_data_version: 3953,
            },
            visual: VisualSummaryOutput {
                chunk_size: 16, size_x: 2, size_y: 2, size_z: 2,
                palette: vec![PaletteEntryOutput { block_id: "minecraft:chest".into(), property_id: 0 }],
                property_pool: vec![BTreeMap::from([("facing".into(), "north".into())])],
                layers: vec![
                    LayerSliceOutput { y: 0, blocks: Vec::new() },
                    LayerSliceOutput { y: 1, blocks: vec![LayerBlockOutput { x: 1, z: 0, palette_id: 0 }] },
                ],
                chunks: Vec::new(),
            },
        };
        std::fs::write(&index, serde_json::to_vec(&source).unwrap()).unwrap();
        let all = build_cache_all_layers_output(&manifest).unwrap();
        let meta = build_cache_layer_meta_output(&manifest).unwrap();
        let single = build_cache_layer_output(&manifest, 1).unwrap();
        assert_eq!(all.chunk_size, 16);
        assert_eq!(all.metadata.minecraft_data_version, meta.metadata.minecraft_data_version);
        assert_eq!(serde_json::to_value(&all.layers).unwrap(), serde_json::to_value(&source.visual.layers).unwrap());
        assert_eq!(serde_json::to_value(&single.blocks).unwrap(), serde_json::to_value(&all.layers[1].blocks).unwrap());
        assert_eq!(meta.visual.palette[0].block_id, "minecraft:chest");
        assert!(build_cache_layer_output(&manifest, -1).unwrap().blocks.is_empty());
        source.metadata.time_modified = 3;
        source.visual.layers[1].blocks[0].z = 1;
        std::fs::write(&index, serde_json::to_vec(&source).unwrap()).unwrap();
        let reread = build_cache_all_layers_output(&manifest).unwrap();
        assert_eq!(reread.metadata.time_modified, 3);
        assert_eq!(reread.layers[1].blocks[0].z, 1);
    }
}
