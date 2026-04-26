use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::model::MetadataOutput;
use crate::nbt::{
    RegionBounds, bits_for_palette, for_each_palette_index, load_litematic_root, region_bounds,
    region_volume, storage_to_region_coords,
};
use crate::visual::{build_metadata_output, compute_bounds};

#[derive(Debug, Clone, Serialize)]
pub struct StableStructureOutput {
    pub metadata: MetadataOutput,
    pub structure_stats: StructureStatsOutput,
    pub material_items: Vec<MaterialItemOutput>,
    pub regions: Vec<RegionSummaryOutput>,
    pub layers_summary: Vec<LayerSummaryOutput>,
    pub entity_summary: EntitySummaryOutput,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructureStatsOutput {
    pub scope: ScopeOutput,
    pub total_non_air_blocks: u64,
    pub unique_block_types: usize,
    pub total_regions: usize,
    pub total_layers: usize,
    pub total_entities: usize,
    pub total_tile_entities: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeOutput {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world_y: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterialItemOutput {
    pub block_id: String,
    pub display_name: String,
    pub count: u64,
    pub icon_hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_counts: Option<Vec<RegionCountOutput>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_counts: Option<Vec<LayerCountOutput>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionCountOutput {
    pub region: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerCountOutput {
    pub layer: i32,
    pub world_y: i32,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionSummaryOutput {
    pub name: String,
    pub position: RegionPositionOutput,
    pub size: RegionSizeOutput,
    pub bounds: RegionBoundsOutput,
    pub total_non_air_blocks: u64,
    pub unique_block_types: usize,
    pub total_entities: usize,
    pub total_tile_entities: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionPositionOutput {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionSizeOutput {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionBoundsOutput {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub min_z: i32,
    pub max_z: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerSummaryOutput {
    pub layer: i32,
    pub world_y: i32,
    pub total_non_air_blocks: u64,
    pub unique_block_types: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntitySummaryOutput {
    pub total_entities: usize,
    pub items: Vec<EntityCountOutput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityCountOutput {
    pub entity_id: String,
    pub count: u64,
}

#[derive(Debug, Clone)]
pub enum MaterialScope {
    All,
    Region(String),
    Layer(i32),
}

#[derive(Default)]
struct MaterialAccumulator {
    count: u64,
    region_counts: BTreeMap<String, u64>,
    layer_counts: BTreeMap<i32, u64>,
}

#[derive(Default)]
struct RegionAccumulator {
    total_non_air_blocks: u64,
    block_types: BTreeSet<String>,
}

#[derive(Default)]
struct LayerAccumulator {
    total_non_air_blocks: u64,
    block_types: BTreeSet<String>,
}

pub fn build_stats_output(path: &Path) -> Result<StableStructureOutput> {
    build_structure_output(path, MaterialScope::All)
}

pub fn build_materials_output(path: &Path, scope: MaterialScope) -> Result<StableStructureOutput> {
    build_structure_output(path, scope)
}

fn build_structure_output(path: &Path, scope: MaterialScope) -> Result<StableStructureOutput> {
    let root = load_litematic_root(path)?;
    let bounds = compute_bounds(&root);
    let metadata = build_metadata_output(&root, bounds);
    let resolved_bounds = bounds.unwrap_or_else(|| fallback_bounds(&metadata));
    let total_layers = metadata.enclosing_size.y.max(0) as usize;
    let resolved_scope = resolve_scope(&scope, &root.regions, &metadata)?;

    let mut material_counts = BTreeMap::<String, MaterialAccumulator>::new();
    let mut regions = Vec::<RegionSummaryOutput>::new();
    let mut layers = (0..total_layers)
        .map(|_| LayerAccumulator::default())
        .collect::<Vec<_>>();
    let mut entity_counts = BTreeMap::<String, u64>::new();
    let mut total_entities = 0_usize;
    let mut total_tile_entities = 0_usize;

    for (region_name, region) in &root.regions {
        let mut region_acc = RegionAccumulator::default();
        for entity in &region.entities {
            if let Some(entity_id) = entity.entity_id() {
                *entity_counts.entry(entity_id.to_string()).or_insert(0) += 1;
            }
            total_entities += 1;
        }
        total_tile_entities += region.tile_entities.len();

        let volume = region_volume(&region.size)?;
        let width = i64::from(region.size.x).unsigned_abs() as usize;
        let length = i64::from(region.size.z).unsigned_abs() as usize;
        let nbits = bits_for_palette(region.block_state_palette.len());

        for_each_palette_index(
            &region.block_states,
            volume,
            nbits,
            |index, palette_index| {
                let block = region
                    .block_state_palette
                    .get(palette_index)
                    .ok_or_else(|| anyhow!("palette index {} out of range", palette_index))?;
                if is_air_block(&block.name) {
                    return Ok(());
                }

                let y = index / (width * length);
                let i_in_layer = index % (width * length);
                let z = i_in_layer / width;
                let x = i_in_layer % width;
                let (_rx, ry, _rz) = storage_to_region_coords(x, y, z, &region.size);
                let layer_index = region.position.y + ry - resolved_bounds.min_y;

                let block_id = block.name.clone();
                region_acc.total_non_air_blocks += 1;
                region_acc.block_types.insert(block_id.clone());

                if let Some(layer_acc) = layers.get_mut(layer_index as usize) {
                    layer_acc.total_non_air_blocks += 1;
                    layer_acc.block_types.insert(block_id.clone());
                }

                if resolved_scope.matches(region_name, layer_index) {
                    let material = material_counts.entry(block_id).or_default();
                    material.count += 1;
                    *material
                        .region_counts
                        .entry(region_name.clone())
                        .or_insert(0) += 1;
                    *material.layer_counts.entry(layer_index).or_insert(0) += 1;
                }

                Ok(())
            },
        )?;

        let region_bounds = region_bounds(region);
        regions.push(RegionSummaryOutput {
            name: region_name.clone(),
            position: RegionPositionOutput {
                x: region.position.x,
                y: region.position.y,
                z: region.position.z,
            },
            size: RegionSizeOutput {
                x: region.size.x,
                y: region.size.y,
                z: region.size.z,
            },
            bounds: RegionBoundsOutput::from(region_bounds),
            total_non_air_blocks: region_acc.total_non_air_blocks,
            unique_block_types: region_acc.block_types.len(),
            total_entities: region.entities.len(),
            total_tile_entities: region.tile_entities.len(),
        });
    }

    let material_items = build_material_items(material_counts, resolved_bounds.min_y);
    let layers_summary = layers
        .into_iter()
        .enumerate()
        .map(|(index, layer)| LayerSummaryOutput {
            layer: index as i32,
            world_y: resolved_bounds.min_y + index as i32,
            total_non_air_blocks: layer.total_non_air_blocks,
            unique_block_types: layer.block_types.len(),
        })
        .collect::<Vec<_>>();

    let total_selected_blocks = material_items.iter().map(|item| item.count).sum();
    let entity_summary = EntitySummaryOutput {
        total_entities,
        items: entity_counts
            .into_iter()
            .map(|(entity_id, count)| EntityCountOutput { entity_id, count })
            .collect(),
    };

    Ok(StableStructureOutput {
        metadata,
        structure_stats: StructureStatsOutput {
            scope: resolved_scope.to_output(resolved_bounds.min_y),
            total_non_air_blocks: total_selected_blocks,
            unique_block_types: material_items.len(),
            total_regions: regions.len(),
            total_layers,
            total_entities,
            total_tile_entities,
        },
        material_items,
        regions,
        layers_summary,
        entity_summary,
    })
}

fn build_material_items(
    material_counts: BTreeMap<String, MaterialAccumulator>,
    min_world_y: i32,
) -> Vec<MaterialItemOutput> {
    let mut items = material_counts
        .into_iter()
        .map(|(block_id, acc)| MaterialItemOutput {
            display_name: display_name_for_block(&block_id),
            icon_hint: block_id.clone(),
            block_id,
            count: acc.count,
            region_counts: Some(
                acc.region_counts
                    .into_iter()
                    .map(|(region, count)| RegionCountOutput { region, count })
                    .collect(),
            ),
            layer_counts: Some(
                acc.layer_counts
                    .into_iter()
                    .map(|(layer, count)| LayerCountOutput {
                        layer,
                        world_y: min_world_y + layer,
                        count,
                    })
                    .collect(),
            ),
        })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.block_id.cmp(&right.block_id))
    });
    items
}

fn display_name_for_block(block_id: &str) -> String {
    let name = block_id.split(':').next_back().unwrap_or(block_id);
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(title_case_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn title_case_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::new();
    out.extend(first.to_uppercase());
    out.push_str(chars.as_str());
    out
}

fn is_air_block(block_id: &str) -> bool {
    matches!(
        block_id,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

fn fallback_bounds(metadata: &MetadataOutput) -> RegionBounds {
    RegionBounds {
        min_x: 0,
        max_x: metadata.enclosing_size.x.saturating_sub(1),
        min_y: 0,
        max_y: metadata.enclosing_size.y.saturating_sub(1),
        min_z: 0,
        max_z: metadata.enclosing_size.z.saturating_sub(1),
    }
}

fn resolve_scope(
    scope: &MaterialScope,
    regions: &BTreeMap<String, crate::nbt::RegionNbt>,
    metadata: &MetadataOutput,
) -> Result<ResolvedScope> {
    match scope {
        MaterialScope::All => Ok(ResolvedScope::All),
        MaterialScope::Region(name) => {
            if regions.contains_key(name) {
                Ok(ResolvedScope::Region(name.clone()))
            } else {
                bail!("unknown region: {name}");
            }
        }
        MaterialScope::Layer(layer) => {
            let total_layers = metadata.enclosing_size.y.max(0);
            if *layer < 0 || *layer >= total_layers {
                bail!(
                    "layer out of range: {} (expected 0..{})",
                    layer,
                    total_layers
                );
            }
            Ok(ResolvedScope::Layer(*layer))
        }
    }
}

enum ResolvedScope {
    All,
    Region(String),
    Layer(i32),
}

impl ResolvedScope {
    fn matches(&self, region_name: &str, layer_index: i32) -> bool {
        match self {
            Self::All => true,
            Self::Region(expected) => region_name == expected,
            Self::Layer(expected) => layer_index == *expected,
        }
    }

    fn to_output(&self, min_world_y: i32) -> ScopeOutput {
        match self {
            Self::All => ScopeOutput {
                kind: "all".to_string(),
                region: None,
                layer: None,
                world_y: None,
            },
            Self::Region(region) => ScopeOutput {
                kind: "region".to_string(),
                region: Some(region.clone()),
                layer: None,
                world_y: None,
            },
            Self::Layer(layer) => ScopeOutput {
                kind: "layer".to_string(),
                region: None,
                layer: Some(*layer),
                world_y: Some(min_world_y + *layer),
            },
        }
    }
}

impl From<RegionBounds> for RegionBoundsOutput {
    fn from(value: RegionBounds) -> Self {
        Self {
            min_x: value.min_x,
            max_x: value.max_x,
            min_y: value.min_y,
            max_y: value.max_y,
            min_z: value.min_z,
            max_z: value.max_z,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MaterialScope, build_materials_output, build_stats_output};

    fn water_fixture_path() -> &'static str {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../_full_mode_water_fixture.litematic"
        )
    }

    #[test]
    fn stats_output_contains_required_sections() {
        let output = build_stats_output(std::path::Path::new(water_fixture_path())).unwrap();

        assert_eq!(output.metadata.name, "Water Fixture");
        assert_eq!(output.structure_stats.scope.kind, "all");
        assert!(!output.material_items.is_empty());
        assert_eq!(output.regions.len(), 1);
        assert_eq!(output.layers_summary.len(), 2);
        assert_eq!(output.entity_summary.total_entities, 0);
    }

    #[test]
    fn materials_layer_scope_filters_counts() {
        let output = build_materials_output(
            std::path::Path::new(water_fixture_path()),
            MaterialScope::Layer(0),
        )
        .unwrap();

        assert_eq!(output.structure_stats.scope.kind, "layer");
        assert_eq!(output.structure_stats.total_non_air_blocks, 2);
        assert_eq!(output.material_items.len(), 1);
        assert_eq!(output.material_items[0].block_id, "minecraft:stone");
        assert_eq!(output.material_items[0].count, 2);
    }
}
