use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::model::{
    LayerSliceOutput, VisualLayerOutput, VisualMetaOutput, VisualMetaSummaryOutput, VisualOutput,
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
