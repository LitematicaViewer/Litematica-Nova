use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[path = "material_catalog.rs"]
mod material_catalog;

use anyhow::{Result, bail};
use fastnbt::Value as NbtValue;
use serde::{Deserialize, Serialize};

use crate::model::MetadataOutput;
use crate::nbt::{
    RegionBounds, bits_for_palette, for_each_palette_index, load_litematic_root, region_bounds,
    region_volume, storage_to_region_coords,
};
use crate::canonical_ir::{CanonicalRegion, CanonicalWorld};
use crate::visual::{build_metadata_output, compute_bounds};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StableStructureOutput {
    pub material_semantics: String,
    pub metadata: MetadataOutput,
    pub structure_stats: StructureStatsOutput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived: Option<crate::model::AnalysisDerivedOutput>,
    pub material_items: Vec<MaterialItemOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural_block_counts: Option<Vec<StructuralBlockCountOutput>>,
    pub non_item_blocks: Vec<NonItemBlockOutput>,
    pub regions: Vec<RegionSummaryOutput>,
    pub layers_summary: Vec<LayerSummaryOutput>,
    pub entity_summary: EntitySummaryOutput,
    pub container_scan: ContainerScanOutput,
}

pub(crate) fn inventory_requirement(id: String, count: u64) -> Option<MaterialItemOutput> {
    let item = material_catalog::item(&id)?;
    Some(MaterialItemOutput {
        display_name: material_catalog::name(&id), icon_hint: id.clone(), block_id: id,
        count, block_count: count, container_item_count: 0, total_count: count,
        stack_size: item.stack_size, can_fit_container: item.can_fit_container,
        material_status: "item".into(), has_custom_stack_size: false,
        region_counts: None, layer_counts: None, scope_counts: None,
    })
}

/// Original structure identifiers remain distinct from survival inventory IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralBlockCountOutput {
    pub block_id: String,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_counts: Option<Vec<StructuralScopeCount>>,
}

fn build_structural_counts(counts: BTreeMap<String, u64>, mut scopes: Option<BTreeMap<String, BTreeMap<(String, i32), u64>>>) -> Vec<StructuralBlockCountOutput> {
    counts.into_iter().map(|(block_id, count)| {
        let scope_counts = scopes.as_mut().map(|all| all.remove(&block_id).unwrap_or_default().into_iter()
            .map(|((region, layer), count)| StructuralScopeCount { region, layer, count }).collect());
        StructuralBlockCountOutput { block_id, count, scope_counts }
    }).collect()
}

/// Structural states with no registered inventory item are visible diagnostics,
/// not material rows. `count` remains a count of blocks, never required items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonItemBlockOutput {
    pub block_id: String,
    pub display_name: String,
    pub count: u64,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_counts: Option<Vec<StructuralScopeCount>>,
}

/// Joint counts retain both dimensions and channels. Region/layer marginals
/// alone cannot reproduce a selected scope's breakdown or container semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralScopeCount {
    pub region: String,
    pub layer: i32,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialScopeCount {
    pub region: String,
    pub layer: Option<i32>,
    pub block_count: u64,
    pub container_item_count: u64,
    pub has_custom_stack_size: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureStatsOutput {
    pub scope: ScopeOutput,
    pub total_non_air_blocks: u64,
    pub unique_block_types: usize,
    pub total_regions: usize,
    pub total_layers: usize,
    pub total_entities: usize,
    pub total_tile_entities: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeOutput {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world_y: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialItemOutput {
    pub block_id: String,
    pub display_name: String,
    pub count: u64,
    pub block_count: u64,
    pub container_item_count: u64,
    pub total_count: u64,
    pub icon_hint: String,
    pub stack_size: u64,
    pub can_fit_container: bool,
    pub material_status: String,
    pub has_custom_stack_size: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_counts: Option<Vec<RegionCountOutput>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_counts: Option<Vec<LayerCountOutput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_counts: Option<Vec<MaterialScopeCount>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionCountOutput {
    pub region: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerCountOutput {
    pub layer: i32,
    pub world_y: i32,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionPositionOutput {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionSizeOutput {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionBoundsOutput {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub min_z: i32,
    pub max_z: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerSummaryOutput {
    pub layer: i32,
    pub world_y: i32,
    pub total_non_air_blocks: u64,
    pub unique_block_types: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySummaryOutput {
    pub total_entities: usize,
    pub items: Vec<EntityCountOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCountOutput {
    pub entity_id: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerScanOutput {
    pub enabled: bool,
    pub containers_scanned: usize,
    pub item_stacks_scanned: usize,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_entries: Option<Vec<ContainerScopeScan>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerScopeScan {
    pub region: String,
    pub layer: Option<i32>,
    pub containers_scanned: usize,
    pub item_stacks_scanned: usize,
    pub warnings: Vec<String>,
    pub missing_layer_warning: Option<String>,
}

#[derive(Debug, Clone)]
pub enum MaterialScope {
    All,
    Region(String),
    Layer(i32),
}

#[derive(Default)]
struct MaterialAccumulator {
    block_count: u64,
    container_item_count: u64,
    has_custom_stack_size: bool,
    region_counts: BTreeMap<String, u64>,
    layer_counts: BTreeMap<i32, u64>,
    scope_counts: BTreeMap<(String, Option<i32>), MaterialScopeCount>,
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

pub fn build_stats_output(
    path: &Path,
    include_container_items: bool,
) -> Result<StableStructureOutput> {
    let root = load_litematic_root(path)?;
    let started = std::time::Instant::now();
    let result = build_structure_output_with_scope_cache(&root, MaterialScope::All, include_container_items, true);
    if std::env::var_os("NOVA_STATS_TIMING").is_some() {
        eprintln!("[NOVA_STATS_TIMING] aggregate_ms={:.3} aggregate_runs=1", started.elapsed().as_secs_f64() * 1000.0);
    }
    result
}

/// Builds the legacy source-NBT analysis summary without reopening its input.
/// The public stats/materials commands retain this path for scoped and
/// optional container inspection; render artifacts use canonical semantics
/// below.
pub(crate) fn build_stats_output_from_root(
    root: &crate::nbt::LitematicRoot,
    include_container_items: bool,
) -> Result<StableStructureOutput> {
    build_structure_output_from_root(root, MaterialScope::All, include_container_items)
}

/// Builds artifact analysis from the same parsed input using source-version
/// palette semantics retained alongside normalized renderer semantics. This is the render-build entry point: it deliberately does not
/// retain or revisit the source NBT root.
///
/// Artifact analysis currently excludes container contents, matching the
/// existing render-build contract. The public materials command retains its
/// source-NBT implementation because it additionally supports region/layer
/// scopes and optional recursive container inspection.
pub(crate) fn build_stats_output_from_canonical(
    world: &CanonicalWorld,
) -> Result<StableStructureOutput> {
    let bounds = compute_canonical_bounds(world);
    let metadata = build_canonical_metadata_output(world, bounds);
    let resolved_bounds = bounds.unwrap_or_else(|| fallback_bounds(&metadata));
    let total_layers = metadata.enclosing_size.y.max(0) as usize;

    let mut material_counts = BTreeMap::<String, MaterialAccumulator>::new();
    let mut non_item_counts = BTreeMap::<String, u64>::new();
    let mut structural_counts = BTreeMap::<String, u64>::new();

    let mut regions = Vec::<RegionSummaryOutput>::new();
    let mut layers = (0..total_layers)
        .map(|_| LayerAccumulator::default())
        .collect::<Vec<_>>();
    let mut entity_counts = BTreeMap::<String, u64>::new();
    let mut total_entities = 0_usize;
    let mut total_tile_entities = 0_usize;
    let mut selected_block_count = 0_u64;
    let mut selected_block_types = BTreeSet::new();

    for region in world.regions() {
        let region_name = region.name();
        let position = region.position();
        let size = region.size();
        let mut region_acc = RegionAccumulator::default();

        for entity in region.entities() {
            if let Some(entity_id) = entity.identifier() {
                *entity_counts
                    .entry(entity_id.as_str().to_owned())
                    .or_insert(0) += 1;
            }
            total_entities += 1;
        }
        total_tile_entities += region.block_entities().len();

        let volume = region_volume(&size)?;
        let width = i64::from(size.x).unsigned_abs() as usize;
        let length = i64::from(size.z).unsigned_abs() as usize;
        let nbits = bits_for_palette(region.palette().len());

        let source_palette = region.source_palette();
        let palette_materials = source_palette.iter().map(|state| {
            material_catalog::requirements_for_version(&state.name, &state.properties, Some(world.provenance().source_minecraft_data_version()))
        }).collect::<Vec<_>>();
        let palette_non_items = source_palette.iter().map(|state| {
            material_catalog::non_item_status_for_version(&state.name, Some(world.provenance().source_minecraft_data_version()))
        }).collect::<Vec<_>>();
        let mut layer_palette_counts = vec![0_u64; region.palette().len()];
        let mut storage_y = 0_usize;
        let mut accumulate_layer = |y: usize, counts: &mut [u64]| {
            let (_, ry, _) = storage_to_region_coords(0, y, 0, &size);
            let layer_index = position.y + ry - resolved_bounds.min_y;
            for (palette_index, count) in counts.iter_mut().enumerate() {
                let count = std::mem::take(count);
                if count == 0 { continue; }
                let Some(block) = source_palette.get(palette_index) else { continue; };
                let block_id = block.name.as_str();
                if is_air_block(block_id) { continue; }
                region_acc.total_non_air_blocks += count;
                insert_string_once(&mut region_acc.block_types, block_id);
                if let Some(layer_acc) = layers.get_mut(layer_index as usize) {
                    layer_acc.total_non_air_blocks += count;
                    insert_string_once(&mut layer_acc.block_types, block_id);
                }
                selected_block_count += count;
                increment_string_count(&mut structural_counts, block_id, count);
                insert_string_once(&mut selected_block_types, block_id);
                if palette_non_items[palette_index].is_some() {
                    increment_string_count(&mut non_item_counts, block_id, count);
                }
                for (item_id, amount) in &palette_materials[palette_index] {
                    let amount = amount * count;
                    let material = get_or_insert_material(&mut material_counts, item_id);
                    material.block_count += amount;
                    increment_string_count(&mut material.region_counts, region_name, amount);
                    *material.layer_counts.entry(layer_index).or_insert(0) += amount;
                }
            }
        };
        for_each_palette_index(
            region.block_states(),
            volume,
            nbits,
            region.palette().len(),
            |index, palette_index| {
                let y = index / (width * length);
                if y != storage_y {
                    accumulate_layer(storage_y, &mut layer_palette_counts);
                    storage_y = y;
                }
                if let Some(count) = layer_palette_counts.get_mut(palette_index) { *count += 1; }
                Ok(())
            },
        )?;
        accumulate_layer(storage_y, &mut layer_palette_counts);

        regions.push(RegionSummaryOutput {
            name: region_name.to_owned(),
            position: RegionPositionOutput {
                x: position.x,
                y: position.y,
                z: position.z,
            },
            size: RegionSizeOutput {
                x: size.x,
                y: size.y,
                z: size.z,
            },
            bounds: RegionBoundsOutput::from(canonical_region_bounds(region)),
            total_non_air_blocks: region_acc.total_non_air_blocks,
            unique_block_types: region_acc.block_types.len(),
            total_entities: region.entities().len(),
            total_tile_entities: region.block_entities().len(),
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
    let total_non_air_blocks = selected_block_count;
    let unique_block_types = selected_block_types.len();

    let derived = crate::business::build_analysis_derived(
        &structural_counts.iter().map(|(id, count)| (id.clone(), *count)).collect(),
        metadata.total_volume, (metadata.enclosing_size.x, metadata.enclosing_size.y, metadata.enclosing_size.z), total_non_air_blocks,
    );
    Ok(StableStructureOutput {
        derived: Some(derived),
        material_semantics: "minecraft-inventory-v5".into(),
        metadata,
        structure_stats: StructureStatsOutput {
            scope: ResolvedScope::All.to_output(resolved_bounds.min_y),
            total_non_air_blocks,
            unique_block_types,
            total_regions: regions.len(),
            total_layers,
            total_entities,
            total_tile_entities,
        },
        material_items,
        structural_block_counts: Some(build_structural_counts(structural_counts, None)),
        non_item_blocks: build_non_item_blocks(non_item_counts),
        regions,
        layers_summary,
        entity_summary: EntitySummaryOutput {
            total_entities,
            items: entity_counts
                .into_iter()
                .map(|(entity_id, count)| EntityCountOutput { entity_id, count })
                .collect(),
        },
        container_scan: ContainerScanOutput::default(),
    })
}

fn compute_canonical_bounds(world: &CanonicalWorld) -> Option<RegionBounds> {
    world
        .regions()
        .iter()
        .map(canonical_region_bounds)
        .reduce(|acc, item| RegionBounds {
            min_x: acc.min_x.min(item.min_x),
            max_x: acc.max_x.max(item.max_x),
            min_y: acc.min_y.min(item.min_y),
            max_y: acc.max_y.max(item.max_y),
            min_z: acc.min_z.min(item.min_z),
            max_z: acc.max_z.max(item.max_z),
        })
}

fn canonical_region_bounds(region: &CanonicalRegion) -> RegionBounds {
    let position = region.position();
    let size = region.size();
    RegionBounds {
        min_x: position.x.min(position.x + size.x + 1),
        max_x: position.x.max(position.x + size.x - 1),
        min_y: position.y.min(position.y + size.y + 1),
        max_y: position.y.max(position.y + size.y - 1),
        min_z: position.z.min(position.z + size.z + 1),
        max_z: position.z.max(position.z + size.z - 1),
    }
}

fn build_canonical_metadata_output(
    world: &CanonicalWorld,
    bounds: Option<RegionBounds>,
) -> MetadataOutput {
    let source = world.metadata();
    let enclosing = if let Some(bounds) = bounds {
        crate::model::EnclosingSize {
            x: bounds.max_x - bounds.min_x + 1,
            y: bounds.max_y - bounds.min_y + 1,
            z: bounds.max_z - bounds.min_z + 1,
        }
    } else {
        source.enclosing_size.clone().unwrap_or_default()
    };
    let provenance = world.provenance();

    MetadataOutput {
        name: source.name.clone().unwrap_or_default(),
        author: source.author.clone().unwrap_or_default(),
        description: source.description.clone().unwrap_or_default(),
        time_created: source.time_created.unwrap_or_default(),
        time_modified: source.time_modified.unwrap_or_default(),
        total_blocks: source.total_blocks.unwrap_or_default(),
        total_volume: source.total_volume.unwrap_or(
            enclosing
                .x
                .saturating_mul(enclosing.y)
                .saturating_mul(enclosing.z),
        ),
        region_count: source
            .region_count
            .unwrap_or(world.regions().len().min(i32::MAX as usize) as i32),
        enclosing_size: enclosing,
        litematic_version: provenance.source_litematic_version(),
        litematic_subversion: provenance
            .source_litematic_sub_version()
            .unwrap_or_default(),
        minecraft_data_version: provenance.source_minecraft_data_version(),
    }
}

fn insert_string_once(set: &mut BTreeSet<String>, value: &str) {
    if !set.contains(value) {
        set.insert(value.to_owned());
    }
}

fn get_or_insert_material<'map>(
    materials: &'map mut BTreeMap<String, MaterialAccumulator>,
    block_id: &str,
) -> &'map mut MaterialAccumulator {
    if !materials.contains_key(block_id) {
        materials.insert(block_id.to_owned(), MaterialAccumulator::default());
    }
    materials
        .get_mut(block_id)
        .expect("canonical material was just inserted")
}

fn increment_string_count(counts: &mut BTreeMap<String, u64>, key: &str, amount: u64) {
    if let Some(count) = counts.get_mut(key) {
        *count = count.saturating_add(amount);
    } else {
        counts.insert(key.to_owned(), amount);
    }
}

pub fn build_materials_output(
    path: &Path,
    scope: MaterialScope,
    include_container_items: bool,
) -> Result<StableStructureOutput> {
    let root = load_litematic_root(path)?;
    build_structure_output_from_root(&root, scope, include_container_items)
}

fn build_structure_output_from_root(
    root: &crate::nbt::LitematicRoot,
    scope: MaterialScope,
    include_container_items: bool,
) -> Result<StableStructureOutput> {
    build_structure_output_with_scope_cache(root, scope, include_container_items, false)
}

fn build_structure_output_with_scope_cache(
    root: &crate::nbt::LitematicRoot,
    scope: MaterialScope,
    include_container_items: bool,
    include_scope_cache: bool,
) -> Result<StableStructureOutput> {
    let bounds = compute_bounds(root);
    let metadata = build_metadata_output(root, bounds);
    let resolved_bounds = bounds.unwrap_or_else(|| fallback_bounds(&metadata));
    let total_layers = metadata.enclosing_size.y.max(0) as usize;
    let resolved_scope = resolve_scope(&scope, &root.regions, &metadata)?;

    let mut material_counts = BTreeMap::<String, MaterialAccumulator>::new();
    let mut non_item_counts = BTreeMap::<String, u64>::new();
    let mut structural_counts = BTreeMap::<String, u64>::new();
    let mut structural_scopes = BTreeMap::<String, BTreeMap<(String, i32), u64>>::new();
    let mut non_item_scopes = BTreeMap::<String, BTreeMap<(String, i32), u64>>::new();
    let mut regions = Vec::<RegionSummaryOutput>::new();
    let mut layers = (0..total_layers)
        .map(|_| LayerAccumulator::default())
        .collect::<Vec<_>>();
    let mut entity_counts = BTreeMap::<String, u64>::new();
    let mut container_scan = ContainerScanOutput {
        enabled: include_container_items,
        scope_entries: (include_scope_cache && include_container_items).then(Vec::new),
        ..ContainerScanOutput::default()
    };
    let mut total_entities = 0_usize;
    let mut total_tile_entities = 0_usize;
    let mut selected_block_count = 0_u64;
    let mut selected_block_types = BTreeSet::new();

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

        let palette_materials = region.block_state_palette.iter().map(|block| material_catalog::requirements_for_version(&block.name, &block.properties, root.minecraft_data_version)).collect::<Vec<_>>();
        let palette_non_items = region.block_state_palette.iter().map(|block| material_catalog::non_item_status_for_version(&block.name, root.minecraft_data_version)).collect::<Vec<_>>();
        // Packed indices are ordered by storage Y. Count a whole palette layer
        // before updating string-keyed summaries: repeated blocks must not
        // allocate identifiers or perform five tree lookups for every voxel.
        // Only one palette-sized counter is resident, independent of volume.
        let mut layer_palette_counts = vec![0_u64; region.block_state_palette.len()];
        let mut storage_y = 0_usize;
        let mut accumulate_layer = |y: usize, counts: &mut [u64]| {
            let (_, ry, _) = storage_to_region_coords(0, y, 0, &region.size);
            let layer_index = region.position.y + ry - resolved_bounds.min_y;
            for (palette_index, count) in counts.iter_mut().enumerate() {
                let count = std::mem::take(count);
                if count == 0 { continue; }
                let block_id = &region.block_state_palette[palette_index].name;
                if is_air_block(block_id) { continue; }
                region_acc.total_non_air_blocks += count;
                insert_string_once(&mut region_acc.block_types, block_id);
                if let Some(layer_acc) = layers.get_mut(layer_index as usize) {
                    layer_acc.total_non_air_blocks += count;
                    insert_string_once(&mut layer_acc.block_types, block_id);
                }
                if resolved_scope.matches(region_name, layer_index) {
                    selected_block_count += count;
                    increment_string_count(&mut structural_counts, block_id, count);
                    if include_scope_cache {
                        *structural_scopes.entry(block_id.clone()).or_default().entry((region_name.clone(), layer_index)).or_default() += count;
                    }
                    if palette_non_items[palette_index].is_some() {
                        increment_string_count(&mut non_item_counts, block_id, count);
                        if include_scope_cache {
                            *non_item_scopes.entry(block_id.clone()).or_default()
                                .entry((region_name.clone(), layer_index)).or_default() += count;
                        }
                    }
                    insert_string_once(&mut selected_block_types, block_id);
                    for (item_id, amount) in &palette_materials[palette_index] {
                        let amount = amount * count;
                        let material = get_or_insert_material(&mut material_counts, item_id);
                        material.block_count += amount;
                        increment_string_count(&mut material.region_counts, region_name, amount);
                        *material.layer_counts.entry(layer_index).or_insert(0) += amount;
                        if include_scope_cache {
                            material_scope_count(material, region_name, Some(layer_index)).block_count += amount;
                        }
                    }
                }
            }
        };
        for_each_palette_index(
            &region.block_states,
            volume,
            nbits,
            region.block_state_palette.len(),
            |index, palette_index| {
                let y = index / (width * length);
                if y != storage_y {
                    accumulate_layer(storage_y, &mut layer_palette_counts);
                    storage_y = y;
                }
                if let Some(count) = layer_palette_counts.get_mut(palette_index) { *count += 1; }
                Ok(())
            },
        )?;
        accumulate_layer(storage_y, &mut layer_palette_counts);

        if include_container_items {
            scan_region_container_items(
                region_name,
                region,
                &resolved_scope,
                resolved_bounds.min_y,
                &mut material_counts,
                &mut container_scan,
            );
        }

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

    let total_selected_blocks = selected_block_count;
    let entity_summary = EntitySummaryOutput {
        total_entities,
        items: entity_counts
            .into_iter()
            .map(|(entity_id, count)| EntityCountOutput { entity_id, count })
            .collect(),
    };

    let mut non_item_blocks = build_non_item_blocks(non_item_counts);
    if include_scope_cache {
        for row in &mut non_item_blocks {
            row.scope_counts = Some(non_item_scopes.remove(&row.block_id).unwrap_or_default().into_iter()
                .map(|((region, layer), count)| StructuralScopeCount { region, layer, count }).collect());
        }
    }
    let derived = matches!(scope, MaterialScope::All).then(|| crate::business::build_analysis_derived(
        &structural_counts.iter().map(|(id, count)| (id.clone(), *count)).collect(),
        metadata.total_volume, (metadata.enclosing_size.x, metadata.enclosing_size.y, metadata.enclosing_size.z), total_selected_blocks,
    ));
    Ok(StableStructureOutput {
        derived,
        material_semantics: "minecraft-inventory-v5".into(),
        metadata,
        structure_stats: StructureStatsOutput {
            scope: resolved_scope.to_output(resolved_bounds.min_y),
            total_non_air_blocks: total_selected_blocks,
            unique_block_types: selected_block_types.len(),
            total_regions: regions.len(),
            total_layers,
            total_entities,
            total_tile_entities,
        },
        material_items,
        structural_block_counts: Some(build_structural_counts(structural_counts, include_scope_cache.then_some(structural_scopes))),
        non_item_blocks,
        regions,
        layers_summary,
        entity_summary,
        container_scan,
    })
}

fn build_non_item_blocks(counts: BTreeMap<String, u64>) -> Vec<NonItemBlockOutput> {
    counts.into_iter().map(|(block_id, count)| NonItemBlockOutput {
        display_name: material_catalog::name(&block_id),
        reason: material_catalog::non_item_status(&block_id).unwrap_or("unknown").into(),
        block_id,
        count,
        scope_counts: None,
    }).collect()
}

fn build_material_items(
    material_counts: BTreeMap<String, MaterialAccumulator>,
    min_world_y: i32,
) -> Vec<MaterialItemOutput> {
    let mut items = material_counts
        .into_iter()
        .map(|(block_id, acc)| MaterialItemOutput {
            display_name: material_catalog::name(&block_id),
            stack_size: material_catalog::item(&block_id).map_or(1, |item| item.stack_size),
            can_fit_container: material_catalog::item(&block_id).is_some_and(|item| item.can_fit_container),
            material_status: material_catalog::status(&block_id).to_string(),
            has_custom_stack_size: acc.has_custom_stack_size,
            icon_hint: block_id.clone(),
            block_id,
            count: acc.block_count + acc.container_item_count,
            block_count: acc.block_count,
            container_item_count: acc.container_item_count,
            total_count: acc.block_count + acc.container_item_count,
            scope_counts: (!acc.scope_counts.is_empty()).then(|| acc.scope_counts.into_values().collect()),
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
            .total_count
            .cmp(&left.total_count)
            .then_with(|| left.block_id.cmp(&right.block_id))
    });
    items
}

fn material_scope_count<'a>(
    material: &'a mut MaterialAccumulator, region: &str, layer: Option<i32>,
) -> &'a mut MaterialScopeCount {
    material.scope_counts.entry((region.to_owned(), layer)).or_insert_with(|| MaterialScopeCount {
        region: region.to_owned(), layer, block_count: 0, container_item_count: 0, has_custom_stack_size: false,
    })
}

fn scan_region_container_items(
    region_name: &str,
    region: &crate::nbt::RegionNbt,
    scope: &ResolvedScope,
    min_world_y: i32,
    material_counts: &mut BTreeMap<String, MaterialAccumulator>,
    scan: &mut ContainerScanOutput,
) {
    for (index, tile_entity) in region.tile_entities.iter().enumerate() {
        let layer_index = tile_entity_layer_index(tile_entity, region.position.y, min_world_y);
        let in_scope = match scope {
            ResolvedScope::All => true,
            ResolvedScope::Region(expected) => expected == region_name,
            ResolvedScope::Layer(expected) => match layer_index {
                Some(layer) => layer == *expected,
                None => {
                    scan.warnings.push(format!(
                        "tile entity {region_name}#{index} has no numeric y field; skipped for layer scope"
                    ));
                    false
                }
            },
        };
        if !in_scope {
            continue;
        }

        let mut stacks = Vec::<ContainerStack>::new();
        let mut entry_scan = ContainerScanOutput::default();
        collect_container_stacks(
            tile_entity,
            &mut stacks,
            &mut entry_scan,
            &format!("{region_name}#{index}"),
        );
        if !stacks.is_empty() {
            entry_scan.containers_scanned += 1;
        }
        for stack in stacks {
            entry_scan.item_stacks_scanned += 1;
            if stack.id == "minecraft:air" || material_catalog::item(&stack.id).is_none() {
                entry_scan.warnings.push(format!("{} × {} is not a registered Minecraft 26.3 inventory item; excluded from materials", stack.id, stack.count));
                continue;
            }
            let material = material_counts.entry(stack.id.clone()).or_default();
            material.has_custom_stack_size |= stack.has_custom_stack_size;
            material.container_item_count =
                material.container_item_count.saturating_add(stack.count);
            *material
                .region_counts
                .entry(region_name.to_string())
                .or_insert(0) += stack.count;
            if let Some(layer) = layer_index {
                *material.layer_counts.entry(layer).or_insert(0) += stack.count;
            }
            if scan.scope_entries.is_some() {
                let joint = material_scope_count(material, region_name, layer_index);
                joint.container_item_count = joint.container_item_count.saturating_add(stack.count);
                joint.has_custom_stack_size |= stack.has_custom_stack_size;
            }
        }
        scan.containers_scanned += entry_scan.containers_scanned;
        scan.item_stacks_scanned += entry_scan.item_stacks_scanned;
        scan.warnings.extend(entry_scan.warnings.iter().cloned());
        if let Some(entries) = &mut scan.scope_entries {
            // Even a non-container block entity without numeric Y contributes
            // the existing layer-scope diagnostic. Do not silently drop it.
            if layer_index.is_none() || entry_scan.containers_scanned > 0 || !entry_scan.warnings.is_empty() {
                entries.push(ContainerScopeScan {
                    region: region_name.to_owned(), layer: layer_index,
                    containers_scanned: entry_scan.containers_scanned,
                    item_stacks_scanned: entry_scan.item_stacks_scanned,
                    warnings: entry_scan.warnings,
                    missing_layer_warning: layer_index.is_none().then(|| format!(
                        "tile entity {region_name}#{index} has no numeric y field; skipped for layer scope"
                    )),
                });
            }
        }
    }
}

#[derive(Debug)]
struct ContainerStack {
    id: String,
    count: u64,
    has_custom_stack_size: bool,
}

fn collect_container_stacks(
    value: &NbtValue,
    out: &mut Vec<ContainerStack>,
    scan: &mut ContainerScanOutput,
    path: &str,
) {
    let NbtValue::Compound(map) = value else {
        return;
    };

    if let Some(items) = get_case_insensitive(map, "Items") {
        match items {
            NbtValue::List(list) => {
                for (index, item) in list.iter().enumerate() {
                    match parse_item_stack(item) {
                        Some(stack) => out.push(stack),
                        None => scan
                            .warnings
                            .push(format!("{path}.Items[{index}] is missing id or Count")),
                    }
                }
            }
            _ => scan
                .warnings
                .push(format!("{path}.Items is not a list; skipped")),
        }
    }

    if let Some(item) = get_case_insensitive(map, "Item") {
        if let Some(stack) = parse_pot_item(item) {
            out.push(stack);
        }
    }

    if let Some(sherds) = get_case_insensitive(map, "Sherds") {
        if let NbtValue::List(list) = sherds {
            for sherd in list {
                if let Some(id) = nbt_string(sherd).filter(|id| !id.is_empty()) {
                    out.push(ContainerStack {
                        id: id.to_string(),
                        count: 1,
                        has_custom_stack_size: false,
                    });
                }
            }
        }
    }

    for (key, child) in map {
        if key.eq_ignore_ascii_case("Items")
            || key.eq_ignore_ascii_case("Item")
            || key.eq_ignore_ascii_case("Sherds")
        {
            continue;
        }
        match child {
            NbtValue::Compound(_) => {
                collect_container_stacks(child, out, scan, &format!("{path}.{key}"))
            }
            NbtValue::List(list) => {
                for (index, item) in list.iter().enumerate() {
                    if matches!(item, NbtValue::Compound(_)) {
                        collect_container_stacks(
                            item,
                            out,
                            scan,
                            &format!("{path}.{key}[{index}]"),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn parse_item_stack(value: &NbtValue) -> Option<ContainerStack> {
    let NbtValue::Compound(map) = value else {
        return None;
    };
    let id = get_case_insensitive(map, "id").and_then(nbt_string)?;
    if id.is_empty() || id == "minecraft:air" {
        return None;
    }
    let count = get_case_insensitive(map, "Count")
        .and_then(nbt_u64)
        .unwrap_or(1);
    if count == 0 {
        return None;
    }
    Some(ContainerStack {
        id: id.to_string(),
        count,
        has_custom_stack_size: map.get("components").and_then(|value| match value { NbtValue::Compound(c) => c.get("minecraft:max_stack_size"), _ => None }).is_some(),
    })
}

fn parse_pot_item(value: &NbtValue) -> Option<ContainerStack> {
    match value {
        NbtValue::String(id) if !id.is_empty() => Some(ContainerStack {
            id: id.clone(),
            count: 1,
            has_custom_stack_size: false,
        }),
        NbtValue::Compound(_) => parse_item_stack(value),
        _ => None,
    }
}

fn tile_entity_layer_index(value: &NbtValue, region_y: i32, min_world_y: i32) -> Option<i32> {
    let NbtValue::Compound(map) = value else {
        return None;
    };
    let y = get_case_insensitive(map, "y").and_then(nbt_i64)? as i32;
    Some(region_y + y - min_world_y)
}

fn get_case_insensitive<'a>(
    map: &'a std::collections::HashMap<String, NbtValue>,
    key: &str,
) -> Option<&'a NbtValue> {
    map.get(key).or_else(|| {
        map.iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    })
}

fn nbt_string(value: &NbtValue) -> Option<&str> {
    match value {
        NbtValue::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn nbt_u64(value: &NbtValue) -> Option<u64> {
    nbt_i64(value).and_then(|value| u64::try_from(value).ok())
}

fn nbt_i64(value: &NbtValue) -> Option<i64> {
    match value {
        NbtValue::Byte(value) => Some(i64::from(*value)),
        NbtValue::Short(value) => Some(i64::from(*value)),
        NbtValue::Int(value) => Some(i64::from(*value)),
        NbtValue::Long(value) => Some(*value),
        _ => None,
    }
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
    use super::{is_air_block, parse_item_stack, BTreeMap, NbtValue};
    use super::{
        MaterialScope, build_materials_output, build_stats_output,
        build_stats_output_from_canonical, build_stats_output_from_root,
    };
    use crate::nbt::load_litematic_root;
    use crate::canonical_ir::CanonicalWorld;

    #[test]
    fn structure_derivation_uses_existing_business_rules_and_ignores_container_channel() {
        let path=std::path::Path::new(water_fixture_path());
        let legacy=crate::analyze::analyze_litematic(path,true).unwrap();
        let without=build_stats_output(path,false).unwrap();
        let with=build_stats_output(path,true).unwrap();
        assert_eq!(serde_json::to_value(without.derived.as_ref().unwrap()).unwrap(),serde_json::to_value(legacy.derived).unwrap());
        assert_eq!(serde_json::to_value(&with.derived).unwrap(),serde_json::to_value(&without.derived).unwrap());
        assert_eq!(without.structural_block_counts.as_ref().unwrap().iter().map(|row|row.count).sum::<u64>(),without.structure_stats.total_non_air_blocks);
    }
    #[test]
    fn fluid_sources_and_vines_share_normal_inventory_rows_without_technical_fake_items() {
        let mut root=load_litematic_root(std::path::Path::new(water_fixture_path())).unwrap();
        for region in root.regions.values_mut() {
            for block in &mut region.block_state_palette {
                if !is_air_block(&block.name) { block.name="minecraft:water".into(); block.properties=BTreeMap::from([("level".into(),"0".into())]); }
            }
        }
        let water=build_stats_output_from_root(&root,false).unwrap();
        assert_eq!(water.material_items.len(),1);
        let bucket=&water.material_items[0];
        assert_eq!(bucket.block_id,"minecraft:water_bucket");
        assert_eq!(bucket.total_count,water.structure_stats.total_non_air_blocks);
        assert_eq!(bucket.stack_size,1);
        assert_eq!(water.non_item_blocks[0].count,water.structure_stats.total_non_air_blocks);
        for region in root.regions.values_mut() { for block in &mut region.block_state_palette { if block.name=="minecraft:water" { block.properties.insert("level".into(),"7".into()); } } }
        let flowing=build_stats_output_from_root(&root,false).unwrap();
        assert!(flowing.material_items.is_empty());
        assert_eq!(flowing.structure_stats.total_non_air_blocks,water.structure_stats.total_non_air_blocks);
        for region in root.regions.values_mut() { for block in &mut region.block_state_palette { if !is_air_block(&block.name) { block.name="minecraft:cave_vines_plant".into(); block.properties=BTreeMap::from([("berries".into(),"true".into())]); } } }
        let vines=build_stats_output_from_root(&root,false).unwrap();
        assert_eq!(vines.material_items[0].block_id,"minecraft:glow_berries");
        assert!(vines.non_item_blocks.is_empty());
        assert_eq!(serde_json::to_value(&vines).unwrap(),serde_json::to_value(build_stats_output_from_canonical(&CanonicalWorld::normalize(root).unwrap()).unwrap()).unwrap());
    }

    fn water_fixture_path() -> &'static str {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/stats_water_fixture.litematic"
        )
    }

    #[test]
    fn stats_output_contains_required_sections() {
        let output = build_stats_output(std::path::Path::new(water_fixture_path()), false).unwrap();

        assert_eq!(output.metadata.name, "Water Fixture");
        assert_eq!(output.structure_stats.scope.kind, "all");
        assert_eq!(output.structural_block_counts.as_ref().unwrap().iter().map(|row|row.count).sum::<u64>(), output.structure_stats.total_non_air_blocks);
        assert!(!output.material_items.is_empty());
        assert_eq!(output.regions.len(), 1);
        assert_eq!(output.layers_summary.len(), 2);
        assert_eq!(output.entity_summary.total_entities, 0);
    }

    #[test]
    fn source_scope_cache_keeps_material_channels_without_changing_render_analysis_contract() {
        let mut root = load_litematic_root(std::path::Path::new(water_fixture_path())).unwrap();
        let name = root.regions.keys().next().unwrap().clone();
        root.regions.get_mut(&name).unwrap().tile_entities.push(NbtValue::Compound(std::collections::HashMap::from([
            ("y".into(), NbtValue::Int(0)),
            ("Items".into(), NbtValue::List(vec![NbtValue::Compound(std::collections::HashMap::from([
                ("id".into(), NbtValue::String("minecraft:stone".into())), ("Count".into(), NbtValue::Byte(3)),
                ("components".into(), NbtValue::Compound(std::collections::HashMap::from([("minecraft:max_stack_size".into(),NbtValue::Int(16))]))),
            ]))])),
        ])));
        let cached = super::build_structure_output_with_scope_cache(&root, MaterialScope::All, true, true).unwrap();
        let regular = super::build_structure_output_from_root(&root, MaterialScope::All, true).unwrap();
        let stone = cached.material_items.iter().find(|row| row.block_id == "minecraft:stone").unwrap();
        assert_eq!(stone.scope_counts.as_ref().unwrap().iter().map(|row|row.block_count).sum::<u64>(), stone.block_count);
        assert_eq!(stone.scope_counts.as_ref().unwrap().iter().map(|row|row.container_item_count).sum::<u64>(), 3);
        assert!(stone.scope_counts.as_ref().unwrap().iter().any(|row|row.container_item_count == 3 && row.has_custom_stack_size));
        assert_eq!(cached.container_scan.scope_entries.as_ref().unwrap().iter().map(|row|row.item_stacks_scanned).sum::<usize>(), 1);
        for row in cached.structural_block_counts.as_ref().unwrap() {
            assert_eq!(row.scope_counts.as_ref().unwrap().iter().map(|scope|scope.count).sum::<u64>(), row.count);
        }
        let mut serialized = serde_json::to_value(cached).unwrap();
        for field in ["material_items", "non_item_blocks", "structural_block_counts"] { for row in serialized[field].as_array_mut().unwrap() { row.as_object_mut().unwrap().remove("scope_counts"); } }
        serialized["container_scan"].as_object_mut().unwrap().remove("scope_entries");
        assert_eq!(serialized, serde_json::to_value(regular).unwrap());
        assert!(super::build_stats_output_from_root(&root, false).unwrap().material_items.iter().all(|row|row.scope_counts.is_none()));
    }

    #[test]
    fn materials_layer_scope_filters_counts() {
        let output = build_materials_output(
            std::path::Path::new(water_fixture_path()),
            MaterialScope::Layer(0),
            false,
        )
        .unwrap();

        assert_eq!(output.structure_stats.scope.kind, "layer");
        assert_eq!(output.structure_stats.total_non_air_blocks, 2);
        assert_eq!(output.material_items.len(), 1);
        assert_eq!(output.material_items[0].block_id, "minecraft:stone");
        assert_eq!(output.material_items[0].block_count, 2);
        assert_eq!(output.material_items[0].container_item_count, 0);
        assert_eq!(output.material_items[0].total_count, 2);
    }

    #[test]
    fn canonical_stats_match_existing_root_stats_for_render_contract() {
        let path = std::path::Path::new(water_fixture_path());
        let root_output =
            build_stats_output_from_root(&load_litematic_root(path).unwrap(), false).unwrap();
        let canonical = CanonicalWorld::normalize(load_litematic_root(path).unwrap()).unwrap();
        let canonical_output = build_stats_output_from_canonical(&canonical).unwrap();

        assert_eq!(
            serde_json::to_value(canonical_output).unwrap(),
            serde_json::to_value(root_output).unwrap()
        );
    }


    #[test]
    fn canonical_analysis_preserves_source_chain_ids_properties_and_palette_aliases() {
        let mut root = load_litematic_root(std::path::Path::new(water_fixture_path())).unwrap();
        root.minecraft_data_version = Some(3953); // 1.21, before the canonical iron_chain rename.
        let mut original_states = Vec::new();
        for region in root.regions.values_mut() {
            let mut alias = false;
            for block in &mut region.block_state_palette {
                if !is_air_block(&block.name) {
                    block.name = if alias { "chain" } else { "minecraft:chain" }.to_owned();
                    block.properties = BTreeMap::from([("axis".to_owned(), "x".to_owned()), ("waterlogged".to_owned(), "false".to_owned())]);
                    alias = !alias;
                }
            }
            original_states.push(region.block_state_palette.clone());
        }
        let expected = build_stats_output_from_root(&root, false).unwrap();
        let canonical = CanonicalWorld::normalize(root).unwrap();
        for (region, original) in canonical.regions().iter().zip(original_states) {
            assert_eq!(region.source_palette().len(), original.len());
            for (preserved, source) in region.source_palette().iter().zip(original) {
                assert_eq!(preserved.name, source.name); assert_eq!(preserved.properties, source.properties);
            }
        }
        assert!(canonical.states().iter().any(|state| state.identifier().as_str() == "minecraft:iron_chain"));
        let actual = build_stats_output_from_canonical(&canonical).unwrap();
        assert_eq!(serde_json::to_value(&actual).unwrap(), serde_json::to_value(&expected).unwrap());
        assert!(actual.structural_block_counts.as_ref().unwrap().iter().all(|row| row.block_id != "minecraft:iron_chain"));
    }

    #[test]
    fn material_requirements_do_not_change_structure_block_count() {
        let mut root = load_litematic_root(std::path::Path::new(water_fixture_path())).unwrap();
        let baseline = build_stats_output_from_root(&root, false).unwrap();
        for region in root.regions.values_mut() {
            for block in &mut region.block_state_palette {
                if !is_air_block(&block.name) {
                    block.name = "minecraft:stone_slab".into();
                    block.properties = BTreeMap::from([("type".into(), "double".into())]);
                }
            }
        }
        let output = build_stats_output_from_root(&root, false).unwrap();
        assert_eq!(output.structure_stats.total_non_air_blocks, baseline.structure_stats.total_non_air_blocks);
        assert_eq!(output.structure_stats.unique_block_types, 1);
        assert_eq!(output.material_items.len(), 1);
        assert_eq!(output.material_items[0].total_count, baseline.structure_stats.total_non_air_blocks * 2);
        let canonical = CanonicalWorld::normalize(root).unwrap();
        assert_eq!(serde_json::to_value(output).unwrap(), serde_json::to_value(build_stats_output_from_canonical(&canonical).unwrap()).unwrap());
    }

    #[test]
    fn technical_blocks_remain_structural_diagnostics_in_source_and_canonical_analysis() {
        for id in ["minecraft:piston_head", "minecraft:moving_piston", "minecraft:nether_portal", "other:unregistered"] {
            let mut root = load_litematic_root(std::path::Path::new(water_fixture_path())).unwrap();
            let baseline = build_stats_output_from_root(&root, false).unwrap();
            for region in root.regions.values_mut() {
                for block in &mut region.block_state_palette {
                    if !is_air_block(&block.name) {
                        block.name = id.into();
                        block.properties.clear();
                    }
                }
            }
            let output = build_stats_output_from_root(&root, false).unwrap();
            assert_eq!(output.material_semantics, "minecraft-inventory-v5");
            assert!(output.material_items.is_empty(), "{id}");
            let structural = output.structural_block_counts.as_ref().unwrap();
            assert_eq!(structural.len(), 1);
            assert_eq!(structural[0].block_id, id);
            assert_eq!(structural[0].count, baseline.structure_stats.total_non_air_blocks);
            assert_eq!(output.structure_stats.total_non_air_blocks, baseline.structure_stats.total_non_air_blocks);
            assert_eq!(output.non_item_blocks.len(), 1);
            assert_eq!(output.non_item_blocks[0].block_id, id);
            assert_eq!(output.non_item_blocks[0].count, baseline.structure_stats.total_non_air_blocks);
            assert_eq!(output.non_item_blocks[0].reason, if id.starts_with("other:") { "unknown" } else { "no_item" });
            let scoped = super::build_structure_output_from_root(&root, MaterialScope::Layer(0), false).unwrap();
            assert_eq!(scoped.non_item_blocks[0].count, scoped.structure_stats.total_non_air_blocks);
            let canonical = CanonicalWorld::normalize(root).unwrap();
            assert_eq!(serde_json::to_value(output).unwrap(), serde_json::to_value(build_stats_output_from_canonical(&canonical).unwrap()).unwrap());
        }
    }

    #[test]
    fn invalid_technical_block_ids_in_container_nbt_are_not_promoted_to_items() {
        let mut root = load_litematic_root(std::path::Path::new(water_fixture_path())).unwrap();
        root.regions.values_mut().next().unwrap().tile_entities.push(NbtValue::Compound(std::collections::HashMap::from([
            ("Items".into(), NbtValue::List(vec![NbtValue::Compound(std::collections::HashMap::from([
                ("id".into(), NbtValue::String("minecraft:piston_head".into())),
                ("Count".into(), NbtValue::Byte(3)),
            ]))])),
        ])));
        let output = build_stats_output_from_root(&root, true).unwrap();
        assert!(output.material_items.iter().all(|item| item.material_status == "item"));
        assert!(output.material_items.iter().all(|item| item.block_id != "minecraft:piston_head"));
        assert!(output.container_scan.warnings.iter().any(|warning| warning.contains("minecraft:piston_head × 3")));
    }

    #[test]
    fn custom_container_stack_limits_are_not_silently_formatted_as_defaults() {
        let stack = NbtValue::Compound(std::collections::HashMap::from([
            ("id".into(), NbtValue::String("minecraft:stone".into())),
            ("count".into(), NbtValue::Int(20)),
            ("components".into(), NbtValue::Compound(std::collections::HashMap::from([
                ("minecraft:max_stack_size".into(), NbtValue::Int(16)),
            ]))),
        ]));
        let result = parse_item_stack(&stack).unwrap();
        assert_eq!(result.count, 20);
        assert!(result.has_custom_stack_size);
    }
}
