use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use fastnbt::Value;
use serde::{Deserialize, Serialize};

use crate::model::{BlockStateNbt, EnclosingSize};
use crate::nbt::{
    LitematicRoot, MetadataNbt, RegionNbt, Vec3i, bits_for_palette, load_litematic_root,
    pack_palette_indices, save_litematic_root,
};

const AIR: &str = "minecraft:air";

#[derive(Debug, Deserialize)]
pub struct ProjectionPlan {
    pub version: i32,
    #[serde(default)]
    pub metadata: PlanMetadata,
    #[serde(default)]
    pub minecraft_data_version: Option<i32>,
    #[serde(default)]
    pub regions: Vec<PlanRegion>,
}

#[derive(Debug, Deserialize)]
pub struct PlanMetadata {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_author")]
    pub author: String,
    #[serde(default = "default_description")]
    pub description: String,
}

impl Default for PlanMetadata {
    fn default() -> Self {
        Self {
            name: default_name(),
            author: default_author(),
            description: default_description(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PlanRegion {
    pub name: String,
    pub origin: [i32; 3],
    pub size: [i32; 3],
    #[serde(default)]
    pub operations: Vec<PlanOperation>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanOperation {
    FillBox {
        #[serde(default)]
        name: Option<String>,
        from: [i32; 3],
        to: [i32; 3],
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
    },
    HollowBox {
        #[serde(default)]
        name: Option<String>,
        from: [i32; 3],
        to: [i32; 3],
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
        #[serde(default = "default_thickness")]
        thickness: i32,
    },
    Floor {
        #[serde(default)]
        name: Option<String>,
        from: [i32; 3],
        to: [i32; 3],
        y: Option<i32>,
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
    },
    Wall {
        #[serde(default)]
        name: Option<String>,
        from: [i32; 3],
        to: [i32; 3],
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
        #[serde(default = "default_thickness")]
        thickness: i32,
    },
    Pillar {
        #[serde(default)]
        name: Option<String>,
        base: [i32; 3],
        height: i32,
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
    },
    Cylinder {
        #[serde(default)]
        name: Option<String>,
        center: [i32; 3],
        radius: i32,
        height: i32,
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
        #[serde(default = "default_true")]
        filled: bool,
    },
    Sphere {
        #[serde(default)]
        name: Option<String>,
        center: [i32; 3],
        radius: i32,
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
        #[serde(default = "default_true")]
        filled: bool,
    },
    OutlineBox {
        #[serde(default)]
        name: Option<String>,
        from: [i32; 3],
        to: [i32; 3],
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
    },
    CheckerboardFloor {
        #[serde(default)]
        name: Option<String>,
        from: [i32; 3],
        to: [i32; 3],
        material_a: MaterialInput,
        material_b: MaterialInput,
    },
    RoofGable {
        #[serde(default)]
        name: Option<String>,
        from: [i32; 3],
        to: [i32; 3],
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
        axis: RoofAxis,
        #[serde(default)]
        overhang: i32,
    },
    Ring {
        #[serde(default)]
        name: Option<String>,
        center: [i32; 3],
        radius: i32,
        #[serde(default = "default_thickness")]
        height: i32,
        #[serde(default = "default_thickness")]
        thickness: i32,
        #[serde(default)]
        block: Option<PlanBlock>,
        #[serde(default)]
        material: Option<MaterialInput>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanBlock {
    pub name: String,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum MaterialInput {
    Material(PlanMaterial),
    Block(PlanBlock),
}

impl MaterialInput {
    fn into_material(self) -> PlanMaterial {
        match self {
            MaterialInput::Material(material) => material,
            MaterialInput::Block(block) => PlanMaterial::Single { block },
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanMaterial {
    Single {
        block: PlanBlock,
    },
    WeightedRandom {
        entries: Vec<WeightedMaterialEntry>,
        #[serde(default)]
        seed: Option<u64>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeightedMaterialEntry {
    pub weight: u64,
    pub block: PlanBlock,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoofAxis {
    X,
    Z,
}

#[derive(Debug, Serialize)]
pub struct GenerateSummary {
    pub plan_path: String,
    pub output_path: Option<String>,
    pub dry_run: bool,
    pub regions_count: usize,
    pub operation_count: usize,
    pub total_volume: u64,
    pub estimated_non_air_blocks: u64,
    pub estimated_palette_count: usize,
    pub palette_entries_per_region: Vec<RegionPaletteSummary>,
    pub operations: Vec<OperationSummary>,
    pub material_summary: Vec<MaterialSummaryEntry>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub wrote_file: bool,
    pub output_size: Option<u64>,
    pub verify_load_result: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegionPaletteSummary {
    pub region: String,
    pub palette_entries: usize,
}

#[derive(Debug, Serialize)]
pub struct MaterialSummaryEntry {
    pub block_id: String,
    pub properties: BTreeMap<String, String>,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct OperationSummary {
    pub region: String,
    pub index: usize,
    pub name: Option<String>,
    pub operation_type: String,
    pub affected_block_count: u64,
    pub material_summary: Vec<MaterialSummaryEntry>,
}

#[derive(Debug)]
struct BuiltRegion {
    name: String,
    region: RegionNbt,
    non_air_count: u64,
    material_counts: BTreeMap<String, (BlockStateNbt, u64)>,
    operation_summaries: Vec<OperationSummary>,
}

pub fn generate_projection(
    plan_path: &Path,
    output: Option<&Path>,
    dry_run: bool,
    force: bool,
) -> Result<GenerateSummary> {
    let plan_text = fs::read_to_string(plan_path)
        .with_context(|| format!("failed to read plan {}", plan_path.display()))?;
    let plan: ProjectionPlan = serde_json::from_str(plan_text.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse plan {}", plan_path.display()))?;
    validate_plan(&plan)?;

    if !dry_run {
        let output =
            output.ok_or_else(|| anyhow!("generate requires --output without --dry-run"))?;
        if output.exists() && !force {
            bail!(
                "refusing to overwrite existing output file: {}",
                output.display()
            );
        }
    }

    let built = build_root(&plan)?;
    let mut summary = summarize(plan_path, output, dry_run, &built);

    if !dry_run {
        let output = output.expect("checked above");
        if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create output directory {}", parent.display())
            })?;
        }
        save_litematic_root(output, &built.root)?;
        summary.wrote_file = true;
        summary.output_size = Some(
            fs::metadata(output)
                .with_context(|| format!("failed to stat {}", output.display()))?
                .len(),
        );
        load_litematic_root(output)?;
        summary.verify_load_result = Some("ok".to_string());
    }

    Ok(summary)
}

struct BuiltRoot {
    root: LitematicRoot,
    regions: Vec<BuiltRegion>,
}

fn build_root(plan: &ProjectionPlan) -> Result<BuiltRoot> {
    let mut regions = BTreeMap::new();
    let mut built_regions = Vec::new();
    let mut total_volume = 0_u64;
    let mut total_blocks = 0_u64;
    let mut bounds_min: Option<[i32; 3]> = None;
    let mut bounds_max: Option<[i32; 3]> = None;

    for (region_index, region_plan) in plan.regions.iter().enumerate() {
        let built_region = build_region(region_plan, region_index)?;
        total_volume += region_volume_u64(region_plan.size)?;
        total_blocks += built_region.non_air_count;
        let region_min = region_plan.origin;
        let region_max = [
            region_plan.origin[0].saturating_add(region_plan.size[0] - 1),
            region_plan.origin[1].saturating_add(region_plan.size[1] - 1),
            region_plan.origin[2].saturating_add(region_plan.size[2] - 1),
        ];
        bounds_min = Some(match bounds_min {
            Some(current) => [
                current[0].min(region_min[0]),
                current[1].min(region_min[1]),
                current[2].min(region_min[2]),
            ],
            None => region_min,
        });
        bounds_max = Some(match bounds_max {
            Some(current) => [
                current[0].max(region_max[0]),
                current[1].max(region_max[1]),
                current[2].max(region_max[2]),
            ],
            None => region_max,
        });
        regions.insert(
            built_region.name.clone(),
            clone_region(&built_region.region),
        );
        built_regions.push(built_region);
    }
    let bounds_min = bounds_min.unwrap_or([0, 0, 0]);
    let bounds_max = bounds_max.unwrap_or([0, 0, 0]);

    let metadata = MetadataNbt {
        author: Some(plan.metadata.author.clone()),
        description: Some(plan.metadata.description.clone()),
        name: Some(plan.metadata.name.clone()),
        total_blocks: Some(clamp_i32(total_blocks)),
        total_volume: Some(clamp_i32(total_volume)),
        region_count: Some(clamp_i32(plan.regions.len() as u64)),
        enclosing_size: Some(EnclosingSize {
            x: bounds_max[0]
                .saturating_sub(bounds_min[0])
                .saturating_add(1),
            y: bounds_max[1]
                .saturating_sub(bounds_min[1])
                .saturating_add(1),
            z: bounds_max[2]
                .saturating_sub(bounds_min[2])
                .saturating_add(1),
        }),
    };
    let root = LitematicRoot {
        metadata,
        regions,
        version: 6,
        sub_version: Some(1),
        minecraft_data_version: plan.minecraft_data_version,
    };
    Ok(BuiltRoot {
        root,
        regions: built_regions,
    })
}

fn clone_region(region: &RegionNbt) -> RegionNbt {
    RegionNbt {
        position: region.position,
        size: region.size,
        block_state_palette: region.block_state_palette.clone(),
        block_states: region.block_states.clone(),
        entities: region.entities.clone(),
        tile_entities: region.tile_entities.clone(),
    }
}

fn build_region(region_plan: &PlanRegion, region_index: usize) -> Result<BuiltRegion> {
    let width = region_plan.size[0] as usize;
    let height = region_plan.size[1] as usize;
    let length = region_plan.size[2] as usize;
    let volume = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(length))
        .ok_or_else(|| anyhow!("regions[{region_index}].size volume overflow"))?;
    let air = block_state_from_plan(&PlanBlock {
        name: AIR.to_string(),
        properties: BTreeMap::new(),
    });
    let mut blocks = vec![air; volume];
    let mut operation_summaries = Vec::new();

    for (operation_index, operation) in region_plan.operations.iter().enumerate() {
        let stats = apply_operation(
            operation,
            &mut blocks,
            region_plan.size,
            format!("regions[{region_index}].operations[{operation_index}]"),
            operation_index,
        )?;
        operation_summaries.push(OperationSummary {
            region: region_plan.name.clone(),
            index: operation_index,
            name: operation_name(operation),
            operation_type: operation_type(operation).to_string(),
            affected_block_count: stats.affected_block_count,
            material_summary: material_counts_to_summary(stats.material_counts),
        });
    }

    let mut palette = vec![block_state_from_plan(&PlanBlock {
        name: AIR.to_string(),
        properties: BTreeMap::new(),
    })];
    let mut palette_by_key = BTreeMap::<String, usize>::from([(state_key(&palette[0]), 0)]);
    let mut indices = Vec::with_capacity(blocks.len());
    let mut non_air_count = 0_u64;
    let mut material_counts = BTreeMap::<String, (BlockStateNbt, u64)>::new();

    for state in blocks {
        let key = state_key(&state);
        let palette_index = if let Some(index) = palette_by_key.get(&key) {
            *index
        } else {
            let index = palette.len();
            palette.push(state.clone());
            palette_by_key.insert(key.clone(), index);
            index
        };
        indices.push(palette_index);
        if state.name != AIR {
            non_air_count += 1;
            let entry = material_counts.entry(key).or_insert((state, 0));
            entry.1 += 1;
        }
    }

    let nbits = bits_for_palette(palette.len());
    let region = RegionNbt {
        position: Vec3i {
            x: region_plan.origin[0],
            y: region_plan.origin[1],
            z: region_plan.origin[2],
        },
        size: Vec3i {
            x: region_plan.size[0],
            y: region_plan.size[1],
            z: region_plan.size[2],
        },
        block_state_palette: palette,
        block_states: pack_palette_indices(&indices, nbits),
        entities: Vec::new(),
        tile_entities: Vec::<Value>::new(),
    };

    Ok(BuiltRegion {
        name: region_plan.name.clone(),
        region,
        non_air_count,
        material_counts,
        operation_summaries,
    })
}

#[derive(Default)]
struct OperationWriteStats {
    affected_block_count: u64,
    material_counts: BTreeMap<String, (BlockStateNbt, u64)>,
}

fn apply_operation(
    operation: &PlanOperation,
    blocks: &mut [BlockStateNbt],
    size: [i32; 3],
    path: String,
    operation_index: usize,
) -> Result<OperationWriteStats> {
    match operation {
        PlanOperation::FillBox {
            from,
            to,
            block,
            material,
            ..
        } => {
            let material = resolve_material(block, material, &path)?;
            write_box(
                blocks,
                size,
                *from,
                *to,
                &material,
                operation_index,
                |_| true,
                &path,
            )
        }
        PlanOperation::HollowBox {
            from,
            to,
            block,
            material,
            thickness,
            ..
        } => {
            if *thickness <= 0 {
                bail!("{path}.thickness must be > 0");
            }
            let material = resolve_material(block, material, &path)?;
            let bounds = normalized_bounds(*from, *to, size, &path)?;
            let t = *thickness;
            write_box_with_bounds(
                blocks,
                size,
                bounds,
                &material,
                operation_index,
                |x, y, z| {
                    x - bounds.min[0] < t
                        || bounds.max[0] - x < t
                        || y - bounds.min[1] < t
                        || bounds.max[1] - y < t
                        || z - bounds.min[2] < t
                        || bounds.max[2] - z < t
                },
            )
        }
        PlanOperation::Floor {
            from,
            to,
            y,
            block,
            material,
            ..
        } => {
            let material = resolve_material(block, material, &path)?;
            let mut from = *from;
            let mut to = *to;
            if let Some(layer) = y {
                from[1] = *layer;
                to[1] = *layer;
            }
            write_box(
                blocks,
                size,
                from,
                to,
                &material,
                operation_index,
                |_| true,
                &path,
            )
        }
        PlanOperation::Wall {
            from,
            to,
            block,
            material,
            thickness,
            ..
        } => {
            if *thickness <= 0 {
                bail!("{path}.thickness must be > 0");
            }
            let material = resolve_material(block, material, &path)?;
            let bounds = normalized_bounds(*from, *to, size, &path)?;
            let dx = bounds.max[0] - bounds.min[0] + 1;
            let dz = bounds.max[2] - bounds.min[2] + 1;
            let t = *thickness;
            write_box_with_bounds(
                blocks,
                size,
                bounds,
                &material,
                operation_index,
                |x, _y, z| {
                    if dx <= dz {
                        x - bounds.min[0] < t || bounds.max[0] - x < t
                    } else {
                        z - bounds.min[2] < t || bounds.max[2] - z < t
                    }
                },
            )
        }
        PlanOperation::Pillar {
            base,
            height,
            block,
            material,
            ..
        } => {
            if *height <= 0 {
                bail!("{path}.height must be > 0");
            }
            let material = resolve_material(block, material, &path)?;
            let to = [base[0], base[1] + height - 1, base[2]];
            write_box(
                blocks,
                size,
                *base,
                to,
                &material,
                operation_index,
                |_| true,
                &path,
            )
        }
        PlanOperation::Cylinder {
            center,
            radius,
            height,
            block,
            material,
            filled,
            ..
        } => {
            if *radius < 0 {
                bail!("{path}.radius must be >= 0");
            }
            if *height <= 0 {
                bail!("{path}.height must be > 0");
            }
            let material = resolve_material(block, material, &path)?;
            let from = [center[0] - radius, center[1], center[2] - radius];
            let to = [
                center[0] + radius,
                center[1] + height - 1,
                center[2] + radius,
            ];
            let bounds = normalized_bounds(from, to, size, &path)?;
            let r2 = radius * radius;
            let inner = (radius - 1).max(0);
            let inner2 = inner * inner;
            write_box_with_bounds(
                blocks,
                size,
                bounds,
                &material,
                operation_index,
                |x, _y, z| {
                    let dx = x - center[0];
                    let dz = z - center[2];
                    let d2 = dx * dx + dz * dz;
                    d2 <= r2 && (*filled || d2 > inner2)
                },
            )
        }
        PlanOperation::Sphere {
            center,
            radius,
            block,
            material,
            filled,
            ..
        } => {
            if *radius < 0 {
                bail!("{path}.radius must be >= 0");
            }
            let material = resolve_material(block, material, &path)?;
            let from = [center[0] - radius, center[1] - radius, center[2] - radius];
            let to = [center[0] + radius, center[1] + radius, center[2] + radius];
            let bounds = normalized_bounds(from, to, size, &path)?;
            let r2 = radius * radius;
            let inner = (radius - 1).max(0);
            let inner2 = inner * inner;
            write_box_with_bounds(
                blocks,
                size,
                bounds,
                &material,
                operation_index,
                |x, y, z| {
                    let dx = x - center[0];
                    let dy = y - center[1];
                    let dz = z - center[2];
                    let d2 = dx * dx + dy * dy + dz * dz;
                    d2 <= r2 && (*filled || d2 > inner2)
                },
            )
        }
        PlanOperation::OutlineBox {
            from,
            to,
            block,
            material,
            ..
        } => {
            let material = resolve_material(block, material, &path)?;
            let bounds = normalized_bounds(*from, *to, size, &path)?;
            write_box_with_bounds(
                blocks,
                size,
                bounds,
                &material,
                operation_index,
                |x, y, z| {
                    let on_x = x == bounds.min[0] || x == bounds.max[0];
                    let on_y = y == bounds.min[1] || y == bounds.max[1];
                    let on_z = z == bounds.min[2] || z == bounds.max[2];
                    (on_x as u8 + on_y as u8 + on_z as u8) >= 2
                },
            )
        }
        PlanOperation::CheckerboardFloor {
            from,
            to,
            material_a,
            material_b,
            ..
        } => {
            let bounds = normalized_bounds(*from, *to, size, &path)?;
            let material_a = material_a.clone().into_material();
            let material_b = material_b.clone().into_material();
            let mut stats = OperationWriteStats::default();
            for y in bounds.min[1]..=bounds.max[1] {
                for z in bounds.min[2]..=bounds.max[2] {
                    for x in bounds.min[0]..=bounds.max[0] {
                        let material = if (x + z).rem_euclid(2) == 0 {
                            &material_a
                        } else {
                            &material_b
                        };
                        write_state_at(
                            blocks,
                            size,
                            material,
                            operation_index,
                            x,
                            y,
                            z,
                            &mut stats,
                        );
                    }
                }
            }
            Ok(stats)
        }
        PlanOperation::RoofGable {
            from,
            to,
            block,
            material,
            axis,
            overhang,
            ..
        } => {
            if *overhang < 0 {
                bail!("{path}.overhang must be >= 0");
            }
            let material = resolve_material(block, material, &path)?;
            let raw_bounds = Bounds {
                min: [from[0].min(to[0]), from[1].min(to[1]), from[2].min(to[2])],
                max: [from[0].max(to[0]), from[1].max(to[1]), from[2].max(to[2])],
            };
            let draw_from = [
                (raw_bounds.min[0] - overhang).max(0),
                raw_bounds.min[1],
                (raw_bounds.min[2] - overhang).max(0),
            ];
            let draw_to = [
                (raw_bounds.max[0] + overhang).min(size[0] - 1),
                raw_bounds.max[1],
                (raw_bounds.max[2] + overhang).min(size[2] - 1),
            ];
            let bounds = normalized_bounds(draw_from, draw_to, size, &path)?;
            let span = match axis {
                RoofAxis::X => raw_bounds.max[0] - raw_bounds.min[0],
                RoofAxis::Z => raw_bounds.max[2] - raw_bounds.min[2],
            }
            .max(1);
            let roof_height = (raw_bounds.max[1] - raw_bounds.min[1]).max(0);
            write_box_with_bounds(
                blocks,
                size,
                bounds,
                &material,
                operation_index,
                |x, y, z| {
                    let along = match axis {
                        RoofAxis::X => (x - raw_bounds.min[0]).clamp(0, span),
                        RoofAxis::Z => (z - raw_bounds.min[2]).clamp(0, span),
                    };
                    let edge_distance = along.min(span - along);
                    let target_y =
                        raw_bounds.min[1] + ((edge_distance * roof_height + span / 2) / span);
                    y == target_y
                },
            )
        }
        PlanOperation::Ring {
            center,
            radius,
            height,
            thickness,
            block,
            material,
            ..
        } => {
            if *radius < 0 {
                bail!("{path}.radius must be >= 0");
            }
            if *height <= 0 {
                bail!("{path}.height must be > 0");
            }
            if *thickness <= 0 {
                bail!("{path}.thickness must be > 0");
            }
            let material = resolve_material(block, material, &path)?;
            let from = [center[0] - radius, center[1], center[2] - radius];
            let to = [
                center[0] + radius,
                center[1] + height - 1,
                center[2] + radius,
            ];
            let bounds = normalized_bounds(from, to, size, &path)?;
            let r2 = radius * radius;
            let inner = (radius - thickness).max(0);
            let inner2 = inner * inner;
            write_box_with_bounds(
                blocks,
                size,
                bounds,
                &material,
                operation_index,
                |x, _y, z| {
                    let dx = x - center[0];
                    let dz = z - center[2];
                    let d2 = dx * dx + dz * dz;
                    d2 <= r2 && d2 >= inner2
                },
            )
        }
    }
}

fn write_box<F>(
    blocks: &mut [BlockStateNbt],
    size: [i32; 3],
    from: [i32; 3],
    to: [i32; 3],
    material: &PlanMaterial,
    operation_index: usize,
    predicate: F,
    path: &str,
) -> Result<OperationWriteStats>
where
    F: Fn([i32; 3]) -> bool,
{
    let bounds = normalized_bounds(from, to, size, path)?;
    write_box_with_bounds(
        blocks,
        size,
        bounds,
        material,
        operation_index,
        |x, y, z| predicate([x, y, z]),
    )
}

#[derive(Clone, Copy)]
struct Bounds {
    min: [i32; 3],
    max: [i32; 3],
}

fn write_box_with_bounds<F>(
    blocks: &mut [BlockStateNbt],
    size: [i32; 3],
    bounds: Bounds,
    material: &PlanMaterial,
    operation_index: usize,
    predicate: F,
) -> Result<OperationWriteStats>
where
    F: Fn(i32, i32, i32) -> bool,
{
    let mut stats = OperationWriteStats::default();
    for y in bounds.min[1]..=bounds.max[1] {
        for z in bounds.min[2]..=bounds.max[2] {
            for x in bounds.min[0]..=bounds.max[0] {
                if predicate(x, y, z) {
                    write_state_at(blocks, size, material, operation_index, x, y, z, &mut stats);
                }
            }
        }
    }
    Ok(stats)
}

fn write_state_at(
    blocks: &mut [BlockStateNbt],
    size: [i32; 3],
    material: &PlanMaterial,
    operation_index: usize,
    x: i32,
    y: i32,
    z: i32,
    stats: &mut OperationWriteStats,
) {
    let state = material_state_for(material, operation_index, [x, y, z]);
    let index = storage_index(x, y, z, size);
    blocks[index] = state.clone();
    stats.affected_block_count += 1;
    if state.name != AIR {
        let key = state_key(&state);
        let entry = stats.material_counts.entry(key).or_insert((state, 0));
        entry.1 += 1;
    }
}

fn normalized_bounds(from: [i32; 3], to: [i32; 3], size: [i32; 3], path: &str) -> Result<Bounds> {
    let min = [from[0].min(to[0]), from[1].min(to[1]), from[2].min(to[2])];
    let max = [from[0].max(to[0]), from[1].max(to[1]), from[2].max(to[2])];
    for axis in 0..3 {
        if min[axis] < 0 || max[axis] >= size[axis] {
            bail!(
                "{path}.from/to axis {axis} out of region bounds 0..{}",
                size[axis] - 1
            );
        }
    }
    Ok(Bounds { min, max })
}

fn storage_index(x: i32, y: i32, z: i32, size: [i32; 3]) -> usize {
    let width = size[0] as usize;
    let length = size[2] as usize;
    y as usize * width * length + z as usize * width + x as usize
}

fn validate_plan(plan: &ProjectionPlan) -> Result<()> {
    if plan.version != 1 {
        bail!("version must be 1");
    }
    if plan.regions.is_empty() {
        bail!("regions must contain at least one region");
    }
    let mut names = BTreeMap::<&str, usize>::new();
    for (region_index, region) in plan.regions.iter().enumerate() {
        let region_path = format!("regions[{region_index}]");
        if region.name.trim().is_empty() {
            bail!("{region_path}.name is empty");
        }
        if names.insert(region.name.as_str(), region_index).is_some() {
            bail!("{region_path}.name duplicates an earlier region");
        }
        for axis in 0..3 {
            if region.size[axis] <= 0 {
                bail!("{region_path}.size[{axis}] must be > 0");
            }
        }
        for (operation_index, operation) in region.operations.iter().enumerate() {
            validate_operation(
                operation,
                region.size,
                &format!("{region_path}.operations[{operation_index}]"),
            )?;
        }
    }
    Ok(())
}

fn validate_operation(operation: &PlanOperation, size: [i32; 3], path: &str) -> Result<()> {
    match operation {
        PlanOperation::FillBox {
            from,
            to,
            block,
            material,
            ..
        } => {
            normalized_bounds(*from, *to, size, path)?;
            validate_resolved_material(block, material, path)
        }
        PlanOperation::HollowBox {
            from,
            to,
            block,
            material,
            thickness,
            ..
        } => {
            normalized_bounds(*from, *to, size, path)?;
            if *thickness <= 0 {
                bail!("{path}.thickness must be > 0");
            }
            validate_resolved_material(block, material, path)
        }
        PlanOperation::Floor {
            from,
            to,
            y,
            block,
            material,
            ..
        } => {
            let mut from = *from;
            let mut to = *to;
            if let Some(layer) = y {
                from[1] = *layer;
                to[1] = *layer;
            }
            normalized_bounds(from, to, size, path)?;
            validate_resolved_material(block, material, path)
        }
        PlanOperation::Wall {
            from,
            to,
            block,
            material,
            thickness,
            ..
        } => {
            normalized_bounds(*from, *to, size, path)?;
            if *thickness <= 0 {
                bail!("{path}.thickness must be > 0");
            }
            validate_resolved_material(block, material, path)
        }
        PlanOperation::Pillar {
            base,
            height,
            block,
            material,
            ..
        } => {
            if *height <= 0 {
                bail!("{path}.height must be > 0");
            }
            normalized_bounds(*base, [base[0], base[1] + height - 1, base[2]], size, path)?;
            validate_resolved_material(block, material, path)
        }
        PlanOperation::Cylinder {
            center,
            radius,
            height,
            block,
            material,
            ..
        } => {
            if *radius < 0 {
                bail!("{path}.radius must be >= 0");
            }
            if *height <= 0 {
                bail!("{path}.height must be > 0");
            }
            normalized_bounds(
                [center[0] - radius, center[1], center[2] - radius],
                [
                    center[0] + radius,
                    center[1] + height - 1,
                    center[2] + radius,
                ],
                size,
                path,
            )?;
            validate_resolved_material(block, material, path)
        }
        PlanOperation::Sphere {
            center,
            radius,
            block,
            material,
            ..
        } => {
            if *radius < 0 {
                bail!("{path}.radius must be >= 0");
            }
            normalized_bounds(
                [center[0] - radius, center[1] - radius, center[2] - radius],
                [center[0] + radius, center[1] + radius, center[2] + radius],
                size,
                path,
            )?;
            validate_resolved_material(block, material, path)
        }
        PlanOperation::OutlineBox {
            from,
            to,
            block,
            material,
            ..
        } => {
            normalized_bounds(*from, *to, size, path)?;
            validate_resolved_material(block, material, path)
        }
        PlanOperation::CheckerboardFloor {
            from,
            to,
            material_a,
            material_b,
            ..
        } => {
            normalized_bounds(*from, *to, size, path)?;
            validate_material(
                &material_a.clone().into_material(),
                &format!("{path}.material_a"),
            )?;
            validate_material(
                &material_b.clone().into_material(),
                &format!("{path}.material_b"),
            )
        }
        PlanOperation::RoofGable {
            from,
            to,
            block,
            material,
            overhang,
            ..
        } => {
            if *overhang < 0 {
                bail!("{path}.overhang must be >= 0");
            }
            let min = [from[0].min(to[0]), from[1].min(to[1]), from[2].min(to[2])];
            let max = [from[0].max(to[0]), from[1].max(to[1]), from[2].max(to[2])];
            normalized_bounds(
                [
                    (min[0] - overhang).max(0),
                    min[1],
                    (min[2] - overhang).max(0),
                ],
                [
                    (max[0] + overhang).min(size[0] - 1),
                    max[1],
                    (max[2] + overhang).min(size[2] - 1),
                ],
                size,
                path,
            )?;
            validate_resolved_material(block, material, path)
        }
        PlanOperation::Ring {
            center,
            radius,
            height,
            thickness,
            block,
            material,
            ..
        } => {
            if *radius < 0 {
                bail!("{path}.radius must be >= 0");
            }
            if *height <= 0 {
                bail!("{path}.height must be > 0");
            }
            if *thickness <= 0 {
                bail!("{path}.thickness must be > 0");
            }
            normalized_bounds(
                [center[0] - radius, center[1], center[2] - radius],
                [
                    center[0] + radius,
                    center[1] + height - 1,
                    center[2] + radius,
                ],
                size,
                path,
            )?;
            validate_resolved_material(block, material, path)
        }
    }
}

fn validate_resolved_material(
    block: &Option<PlanBlock>,
    material: &Option<MaterialInput>,
    path: &str,
) -> Result<()> {
    let material = resolve_material(block, material, path)?;
    validate_material(&material, &format!("{path}.material"))
}

fn validate_material(material: &PlanMaterial, path: &str) -> Result<()> {
    match material {
        PlanMaterial::Single { block } => validate_block(block, &format!("{path}.block")),
        PlanMaterial::WeightedRandom { entries, .. } => {
            if entries.is_empty() {
                bail!("{path}.entries must contain at least one entry");
            }
            let mut total = 0_u64;
            for (index, entry) in entries.iter().enumerate() {
                if entry.weight == 0 {
                    bail!("{path}.entries[{index}].weight must be > 0");
                }
                total = total
                    .checked_add(entry.weight)
                    .ok_or_else(|| anyhow!("{path}.entries total weight overflow"))?;
                validate_block(&entry.block, &format!("{path}.entries[{index}].block"))?;
            }
            Ok(())
        }
    }
}

fn validate_block(block: &PlanBlock, path: &str) -> Result<()> {
    normalize_block_name(&block.name).with_context(|| format!("{path}.name is invalid"))?;
    for (key, value) in &block.properties {
        if key.trim().is_empty() {
            bail!("{path}.properties has an empty key");
        }
        if value.is_empty() {
            bail!("{path}.properties.{key} is empty");
        }
    }
    Ok(())
}

fn normalize_block_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("block name is empty");
    }
    if trimmed.contains(':') {
        if !trimmed.starts_with("minecraft:") || trimmed.matches(':').count() != 1 {
            bail!("block name must be minecraft:* or an unqualified vanilla id");
        }
        let local = trimmed.trim_start_matches("minecraft:");
        if local.is_empty() {
            bail!("block local id is empty");
        }
        Ok(trimmed.to_string())
    } else {
        Ok(format!("minecraft:{trimmed}"))
    }
}

fn block_state_from_plan(block: &PlanBlock) -> BlockStateNbt {
    BlockStateNbt {
        name: normalize_block_name(&block.name).unwrap_or_else(|_| block.name.clone()),
        properties: block.properties.clone(),
    }
}

fn resolve_material(
    block: &Option<PlanBlock>,
    material: &Option<MaterialInput>,
    path: &str,
) -> Result<PlanMaterial> {
    match (block, material) {
        (Some(block), None) => Ok(PlanMaterial::Single {
            block: block.clone(),
        }),
        (None, Some(material)) => Ok(material.clone().into_material()),
        (Some(_), Some(_)) => bail!("{path} must use either block or material, not both"),
        (None, None) => bail!("{path} requires block or material"),
    }
}

fn material_state_for(
    material: &PlanMaterial,
    operation_index: usize,
    coord: [i32; 3],
) -> BlockStateNbt {
    match material {
        PlanMaterial::Single { block } => block_state_from_plan(block),
        PlanMaterial::WeightedRandom { entries, seed } => {
            let total = entries.iter().map(|entry| entry.weight).sum::<u64>().max(1);
            let seed = seed.unwrap_or(DEFAULT_RANDOM_SEED);
            let roll = stable_random_u64(seed, operation_index, coord) % total;
            let mut cursor = 0_u64;
            for entry in entries {
                cursor += entry.weight;
                if roll < cursor {
                    return block_state_from_plan(&entry.block);
                }
            }
            block_state_from_plan(&entries[entries.len() - 1].block)
        }
    }
}

const DEFAULT_RANDOM_SEED: u64 = 0x4c42_415f_5052_4f4a;

fn stable_random_u64(seed: u64, operation_index: usize, coord: [i32; 3]) -> u64 {
    let mut value = seed
        ^ ((operation_index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
        ^ ((coord[0] as i64 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9))
        ^ ((coord[1] as i64 as u64).wrapping_mul(0x94d0_49bb_1331_11eb))
        ^ ((coord[2] as i64 as u64).wrapping_mul(0x2545_f491_4f6c_dd1d));
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn operation_name(operation: &PlanOperation) -> Option<String> {
    match operation {
        PlanOperation::FillBox { name, .. }
        | PlanOperation::HollowBox { name, .. }
        | PlanOperation::Floor { name, .. }
        | PlanOperation::Wall { name, .. }
        | PlanOperation::Pillar { name, .. }
        | PlanOperation::Cylinder { name, .. }
        | PlanOperation::Sphere { name, .. }
        | PlanOperation::OutlineBox { name, .. }
        | PlanOperation::CheckerboardFloor { name, .. }
        | PlanOperation::RoofGable { name, .. }
        | PlanOperation::Ring { name, .. } => name.clone(),
    }
}

fn operation_type(operation: &PlanOperation) -> &'static str {
    match operation {
        PlanOperation::FillBox { .. } => "fill_box",
        PlanOperation::HollowBox { .. } => "hollow_box",
        PlanOperation::Floor { .. } => "floor",
        PlanOperation::Wall { .. } => "wall",
        PlanOperation::Pillar { .. } => "pillar",
        PlanOperation::Cylinder { .. } => "cylinder",
        PlanOperation::Sphere { .. } => "sphere",
        PlanOperation::OutlineBox { .. } => "outline_box",
        PlanOperation::CheckerboardFloor { .. } => "checkerboard_floor",
        PlanOperation::RoofGable { .. } => "roof_gable",
        PlanOperation::Ring { .. } => "ring",
    }
}

fn summarize(
    plan_path: &Path,
    output: Option<&Path>,
    dry_run: bool,
    built: &BuiltRoot,
) -> GenerateSummary {
    let mut material_counts = BTreeMap::<String, (BlockStateNbt, u64)>::new();
    let mut total_volume = 0_u64;
    let mut total_non_air = 0_u64;
    let mut operation_count = 0_usize;
    let mut estimated_palette_count = 0_usize;
    let mut palettes = Vec::new();
    let mut operations = Vec::new();

    for built_region in &built.regions {
        total_volume += built_region.region.size.x as u64
            * built_region.region.size.y as u64
            * built_region.region.size.z as u64;
        total_non_air += built_region.non_air_count;
        operation_count += built_region.operation_summaries.len();
        estimated_palette_count += built_region.region.block_state_palette.len();
        palettes.push(RegionPaletteSummary {
            region: built_region.name.clone(),
            palette_entries: built_region.region.block_state_palette.len(),
        });
        operations.extend(clone_operation_summaries(&built_region.operation_summaries));
        for (key, (state, count)) in &built_region.material_counts {
            let entry = material_counts
                .entry(key.clone())
                .or_insert((state.clone(), 0));
            entry.1 += count;
        }
    }

    let material_summary = material_counts
        .into_values()
        .map(|(state, count)| MaterialSummaryEntry {
            block_id: state.name,
            properties: state.properties,
            count,
        })
        .collect();

    GenerateSummary {
        plan_path: plan_path.display().to_string(),
        output_path: output.map(|path| path.display().to_string()),
        dry_run,
        regions_count: built.regions.len(),
        operation_count,
        total_volume,
        estimated_non_air_blocks: total_non_air,
        estimated_palette_count,
        palette_entries_per_region: palettes,
        operations,
        material_summary,
        warnings: Vec::new(),
        errors: Vec::new(),
        wrote_file: false,
        output_size: None,
        verify_load_result: None,
    }
}

fn material_counts_to_summary(
    material_counts: BTreeMap<String, (BlockStateNbt, u64)>,
) -> Vec<MaterialSummaryEntry> {
    material_counts
        .into_values()
        .map(|(state, count)| MaterialSummaryEntry {
            block_id: state.name,
            properties: state.properties,
            count,
        })
        .collect()
}

fn clone_operation_summaries(summaries: &[OperationSummary]) -> Vec<OperationSummary> {
    summaries
        .iter()
        .map(|summary| OperationSummary {
            region: summary.region.clone(),
            index: summary.index,
            name: summary.name.clone(),
            operation_type: summary.operation_type.clone(),
            affected_block_count: summary.affected_block_count,
            material_summary: summary
                .material_summary
                .iter()
                .map(|entry| MaterialSummaryEntry {
                    block_id: entry.block_id.clone(),
                    properties: entry.properties.clone(),
                    count: entry.count,
                })
                .collect(),
        })
        .collect()
}

fn state_key(state: &BlockStateNbt) -> String {
    let mut key = state.name.clone();
    key.push('{');
    for (property, value) in &state.properties {
        key.push_str(property);
        key.push('=');
        key.push_str(value);
        key.push(';');
    }
    key.push('}');
    key
}

fn region_volume_u64(size: [i32; 3]) -> Result<u64> {
    let width = size[0] as u64;
    let height = size[1] as u64;
    let length = size[2] as u64;
    width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(length))
        .ok_or_else(|| anyhow!("region volume overflow"))
}

fn clamp_i32(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn default_name() -> String {
    "Generated Projection".to_string()
}

fn default_author() -> String {
    "Litematica-BA".to_string()
}

fn default_description() -> String {
    "Generated by Litematica-BA".to_string()
}

fn default_thickness() -> i32 {
    1
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::nbt::{palette_index_at, region_volume};

    #[test]
    fn dry_run_empty_projection_does_not_write_file() -> Result<()> {
        let temp = temp_dir("empty")?;
        let plan = temp.join("plan.json");
        let output = temp.join("out.litematic");
        write_plan(
            &plan,
            r#"{
  "version": 1,
  "metadata": { "name": "Empty Projection" },
  "minecraft_data_version": 3953,
  "regions": [
    { "name": "main", "origin": [0, 0, 0], "size": [4, 3, 2], "operations": [] }
  ]
}"#,
        )?;

        let summary = generate_projection(&plan, Some(&output), true, false)?;
        assert_eq!(summary.regions_count, 1);
        assert_eq!(summary.total_volume, 24);
        assert_eq!(summary.estimated_non_air_blocks, 0);
        assert_eq!(summary.palette_entries_per_region[0].palette_entries, 1);
        assert!(!output.exists());
        fs::remove_dir_all(temp)?;
        Ok(())
    }

    #[test]
    fn fill_hollow_floor_and_overwrite_order_generate_valid_litematic() -> Result<()> {
        let temp = temp_dir("ops")?;
        let plan = temp.join("plan.json");
        let output = temp.join("out.litematic");
        write_plan(
            &plan,
            r#"{
  "version": 1,
  "metadata": {
    "name": "Operation Fixture",
    "author": "Litematica-BA Test",
    "description": "generated in unit test"
  },
  "minecraft_data_version": 3953,
  "regions": [
    {
      "name": "main",
      "origin": [0, 0, 0],
      "size": [5, 5, 5],
      "operations": [
        {
          "type": "fill_box",
          "from": [0, 0, 0],
          "to": [2, 2, 2],
          "block": { "name": "stone" }
        },
        {
          "type": "hollow_box",
          "from": [1, 1, 1],
          "to": [4, 4, 4],
          "block": { "name": "minecraft:stone_bricks" },
          "thickness": 1
        },
        {
          "type": "floor",
          "from": [0, 0, 0],
          "to": [4, 0, 4],
          "block": { "name": "minecraft:oak_slab", "properties": { "type": "bottom" } }
        },
        {
          "type": "fill_box",
          "from": [0, 0, 0],
          "to": [0, 0, 0],
          "block": { "name": "minecraft:oak_slab", "properties": { "type": "top" } }
        },
        {
          "type": "pillar",
          "base": [4, 0, 0],
          "height": 3,
          "block": { "name": "minecraft:oak_slab", "properties": { "type": "bottom" } }
        },
        {
          "type": "wall",
          "from": [0, 0, 4],
          "to": [4, 4, 4],
          "block": { "name": "minecraft:glass" },
          "thickness": 1
        }
      ]
    }
  ]
}"#,
        )?;

        let summary = generate_projection(&plan, Some(&output), false, false)?;
        assert!(summary.wrote_file);
        assert_eq!(summary.verify_load_result.as_deref(), Some("ok"));
        assert!(summary.estimated_non_air_blocks > 27);
        assert!(
            summary
                .material_summary
                .iter()
                .any(|entry| entry.block_id == "minecraft:stone")
        );
        assert!(
            summary
                .material_summary
                .iter()
                .any(|entry| entry.block_id == "minecraft:oak_slab"
                    && entry.properties.get("type").map(String::as_str) == Some("bottom"))
        );
        assert!(
            summary
                .material_summary
                .iter()
                .any(|entry| entry.block_id == "minecraft:oak_slab"
                    && entry.properties.get("type").map(String::as_str) == Some("top"))
        );

        let root = load_litematic_root(&output)?;
        assert_eq!(root.metadata.name.as_deref(), Some("Operation Fixture"));
        assert_eq!(root.metadata.region_count, Some(1));
        let region = root
            .regions
            .get("main")
            .ok_or_else(|| anyhow!("missing generated region"))?;
        assert_eq!(region.block_state_palette[0].name, "minecraft:air");
        let names: Vec<_> = region
            .block_state_palette
            .iter()
            .map(|state| state_key(state))
            .collect();
        assert_eq!(
            names
                .iter()
                .filter(|key| key.starts_with("minecraft:oak_slab{type=bottom;"))
                .count(),
            1
        );
        assert!(
            names
                .iter()
                .any(|key| key.starts_with("minecraft:oak_slab{type=top;"))
        );

        assert_eq!(state_name_at(region, 0, 0, 0)?, "minecraft:oak_slab");
        assert_eq!(
            state_properties_at(region, 0, 0, 0)?
                .get("type")
                .map(String::as_str),
            Some("top")
        );
        assert_eq!(state_name_at(region, 3, 3, 3)?, "minecraft:air");
        assert_eq!(state_name_at(region, 1, 1, 1)?, "minecraft:stone_bricks");
        assert_eq!(state_name_at(region, 4, 2, 0)?, "minecraft:oak_slab");
        assert_eq!(state_name_at(region, 2, 3, 4)?, "minecraft:glass");

        fs::remove_dir_all(temp)?;
        Ok(())
    }

    #[test]
    fn supports_multi_region_cylinder_sphere_and_refuses_overwrite() -> Result<()> {
        let temp = temp_dir("multi")?;
        let plan = temp.join("plan.json");
        let output = temp.join("out.litematic");
        write_plan(
            &plan,
            r#"{
  "version": 1,
  "regions": [
    {
      "name": "cylinder_region",
      "origin": [0, 0, 0],
      "size": [7, 5, 7],
      "operations": [
        {
          "type": "cylinder",
          "center": [3, 0, 3],
          "radius": 2,
          "height": 5,
          "filled": false,
          "block": { "name": "minecraft:copper_block" }
        }
      ]
    },
    {
      "name": "sphere_region",
      "origin": [10, 0, 0],
      "size": [7, 7, 7],
      "operations": [
        {
          "type": "sphere",
          "center": [3, 3, 3],
          "radius": 2,
          "filled": true,
          "block": { "name": "minecraft:gold_block" }
        }
      ]
    }
  ]
}"#,
        )?;

        let summary = generate_projection(&plan, Some(&output), false, false)?;
        assert_eq!(summary.regions_count, 2);
        assert!(summary.estimated_non_air_blocks > 0);
        assert!(generate_projection(&plan, Some(&output), false, false).is_err());

        let root = load_litematic_root(&output)?;
        assert_eq!(root.regions.len(), 2);
        assert!(root.regions.contains_key("cylinder_region"));
        assert!(root.regions.contains_key("sphere_region"));

        fs::remove_dir_all(temp)?;
        Ok(())
    }

    #[test]
    fn weighted_random_is_stable_and_matches_dry_run_apply_summary() -> Result<()> {
        let temp = temp_dir("weighted")?;
        let plan = temp.join("plan.json");
        let output_a = temp.join("a.litematic");
        let output_b = temp.join("b.litematic");
        write_plan(
            &plan,
            r#"{
  "version": 1,
  "regions": [
    {
      "name": "main",
      "origin": [0, 0, 0],
      "size": [8, 4, 8],
      "operations": [
        {
          "type": "fill_box",
          "name": "random walls",
          "from": [0, 0, 0],
          "to": [7, 3, 7],
          "material": {
            "type": "weighted_random",
            "seed": 12345,
            "entries": [
              { "weight": 70, "block": { "name": "minecraft:stone_bricks" } },
              { "weight": 20, "block": { "name": "minecraft:cracked_stone_bricks" } },
              { "weight": 10, "block": { "name": "minecraft:mossy_stone_bricks" } }
            ]
          }
        }
      ]
    }
  ]
}"#,
        )?;

        let dry = generate_projection(&plan, Some(&output_a), true, false)?;
        let apply_a = generate_projection(&plan, Some(&output_a), false, false)?;
        let apply_b = generate_projection(&plan, Some(&output_b), false, false)?;

        assert_eq!(dry.operation_count, 1);
        assert_eq!(
            dry.operations[0].affected_block_count,
            apply_a.operations[0].affected_block_count
        );
        assert_eq!(
            material_counts_tuple(&dry.material_summary),
            material_counts_tuple(&apply_a.material_summary)
        );
        assert_eq!(
            material_counts_tuple(&apply_a.material_summary),
            material_counts_tuple(&apply_b.material_summary)
        );
        assert_eq!(fs::read(&output_a)?, fs::read(&output_b)?);

        fs::remove_dir_all(temp)?;
        Ok(())
    }

    #[test]
    fn new_operations_generate_expected_shapes() -> Result<()> {
        let temp = temp_dir("new_ops")?;
        let plan = temp.join("plan.json");
        let output = temp.join("out.litematic");
        write_plan(
            &plan,
            r#"{
  "version": 1,
  "regions": [
    {
      "name": "main",
      "origin": [0, 0, 0],
      "size": [16, 10, 16],
      "operations": [
        {
          "type": "outline_box",
          "name": "frame",
          "from": [0, 0, 0],
          "to": [3, 3, 3],
          "block": { "name": "minecraft:oak_log", "properties": { "axis": "y" } }
        },
        {
          "type": "checkerboard_floor",
          "name": "floor",
          "from": [4, 0, 0],
          "to": [7, 0, 3],
          "material_a": { "name": "minecraft:white_concrete" },
          "material_b": { "type": "single", "block": { "name": "minecraft:black_concrete" } }
        },
        {
          "type": "roof_gable",
          "name": "roof",
          "from": [0, 4, 6],
          "to": [7, 7, 11],
          "axis": "x",
          "overhang": 1,
          "block": { "name": "minecraft:brick_stairs", "properties": { "facing": "north" } }
        },
        {
          "type": "ring",
          "name": "ring",
          "center": [12, 0, 12],
          "radius": 3,
          "height": 2,
          "thickness": 1,
          "block": { "name": "minecraft:stone" }
        }
      ]
    }
  ]
}"#,
        )?;

        let summary = generate_projection(&plan, Some(&output), false, false)?;
        assert_eq!(summary.operation_count, 4);
        assert!(
            summary
                .operations
                .iter()
                .all(|op| op.affected_block_count > 0)
        );
        assert_eq!(summary.operations[0].affected_block_count, 32);
        assert!(summary.estimated_non_air_blocks > 0);

        let root = load_litematic_root(&output)?;
        let region = root
            .regions
            .get("main")
            .ok_or_else(|| anyhow!("missing generated region"))?;
        assert_eq!(region.block_state_palette[0].name, "minecraft:air");
        assert!(region.block_state_palette[0].properties.is_empty());
        assert_eq!(state_name_at(region, 4, 0, 0)?, "minecraft:white_concrete");
        assert_eq!(state_name_at(region, 5, 0, 0)?, "minecraft:black_concrete");
        assert_eq!(state_name_at(region, 12, 0, 12)?, "minecraft:air");
        assert!(
            summary
                .material_summary
                .iter()
                .any(|entry| entry.block_id == "minecraft:brick_stairs")
        );

        fs::remove_dir_all(temp)?;
        Ok(())
    }

    fn state_name_at(region: &RegionNbt, x: i32, y: i32, z: i32) -> Result<String> {
        Ok(state_at(region, x, y, z)?.name.clone())
    }

    fn state_properties_at(
        region: &RegionNbt,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<BTreeMap<String, String>> {
        Ok(state_at(region, x, y, z)?.properties.clone())
    }

    fn state_at(region: &RegionNbt, x: i32, y: i32, z: i32) -> Result<&BlockStateNbt> {
        let index = storage_index(x, y, z, [region.size.x, region.size.y, region.size.z]);
        let palette_index = palette_index_at(
            &region.block_states,
            region_volume(&region.size)?,
            bits_for_palette(region.block_state_palette.len()),
            index,
        )?;
        region
            .block_state_palette
            .get(palette_index)
            .ok_or_else(|| anyhow!("palette index out of range"))
    }

    fn temp_dir(label: &str) -> Result<std::path::PathBuf> {
        let path = std::env::temp_dir().join(format!(
            "lba_generate_projection_{label}_{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn write_plan(path: &Path, text: &str) -> Result<()> {
        fs::write(path, text.trim_start())?;
        Ok(())
    }

    fn material_counts_tuple(summary: &[MaterialSummaryEntry]) -> Vec<(String, String, u64)> {
        let mut values = summary
            .iter()
            .map(|entry| {
                (
                    entry.block_id.clone(),
                    format!("{:?}", entry.properties),
                    entry.count,
                )
            })
            .collect::<Vec<_>>();
        values.sort();
        values
    }
}
