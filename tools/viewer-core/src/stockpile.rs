use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::recipe_cache::{self, RecipeCacheStatus};
use crate::runtime_paths;
use crate::stats_api::{
    MaterialItemOutput, MaterialScope, RegionSummaryOutput, StableStructureOutput,
    build_materials_output,
};

const DEFAULT_MINECRAFT_VERSION: &str = "1.21.10";
const DEFAULT_STACK_SIZE: u64 = 64;
const SHULKER_STACKS: u64 = 27;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileMaterialsData {
    pub project: StockpileProjectInfo,
    pub summary: StockpileSummary,
    pub materials: Vec<StockpileMaterialItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileProjectInfo {
    pub source_file: String,
    pub created_at: u64,
    pub data_version: i32,
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileSummary {
    pub total_blocks: u64,
    pub unique_materials: usize,
    pub total_stacks: u64,
    pub estimated_shulker_boxes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileMaterialItem {
    pub id: String,
    pub namespace_id: String,
    pub display_name: String,
    pub required_count: u64,
    pub stack_size: u64,
    pub stacks: u64,
    pub remainder: u64,
    pub shulker_boxes: u64,
    pub category: String,
    pub category_icon: String,
    pub item_icon_key: String,
    pub icon_path: String,
    pub icon_available: bool,
    pub source_regions: Vec<String>,
    pub recipe_status: String,
    pub craft_complexity: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StackBreakdown {
    full_stacks: u64,
    remainder: u64,
    stack_units: u64,
    shulker_boxes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MaterialCategory {
    name: &'static str,
    icon: &'static str,
}

pub fn export_materials_data(
    input: &Path,
    output: Option<&Path>,
    include_container_items: bool,
    minecraft_version: Option<&str>,
) -> Result<StockpileMaterialsData> {
    let data = build_materials_data(input, include_container_items, minecraft_version)?;
    let output_path = resolve_output_path(input, output)?;
    ensure_stockpile_project_output(&output_path)?;
    let file = File::create(&output_path).with_context(|| {
        format!(
            "create stockpile materials output failed: {}",
            output_path.display()
        )
    })?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &data)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(data)
}

pub fn build_materials_data(
    input: &Path,
    include_container_items: bool,
    minecraft_version: Option<&str>,
) -> Result<StockpileMaterialsData> {
    let stats = build_materials_output(input, MaterialScope::All, include_container_items)?;
    let recipe_availability =
        resolve_recipe_availability(minecraft_version.unwrap_or(DEFAULT_MINECRAFT_VERSION))?;
    build_stockpile_materials_data(input, stats, &recipe_availability)
}

fn build_stockpile_materials_data(
    input: &Path,
    stats: StableStructureOutput,
    recipe_availability: &RecipeAvailability,
) -> Result<StockpileMaterialsData> {
    let mut total_stack_units = 0_u64;
    let materials = stats
        .material_items
        .iter()
        .map(|item| {
            let breakdown = stack_breakdown(item.total_count, DEFAULT_STACK_SIZE);
            total_stack_units = total_stack_units.saturating_add(breakdown.stack_units);
            stockpile_material_item(item, breakdown, recipe_availability)
        })
        .collect::<Vec<_>>();
    let total_blocks = materials
        .iter()
        .map(|item| item.required_count)
        .sum::<u64>();
    let unique_materials = materials.len();

    Ok(StockpileMaterialsData {
        project: StockpileProjectInfo {
            source_file: input.display().to_string(),
            created_at: current_unix_timestamp()?,
            data_version: stats.metadata.minecraft_data_version,
            regions: stats.regions.iter().map(region_name).collect::<Vec<_>>(),
        },
        summary: StockpileSummary {
            total_blocks,
            unique_materials,
            total_stacks: total_stack_units,
            estimated_shulker_boxes: ceil_div(total_stack_units, SHULKER_STACKS),
        },
        materials,
    })
}

fn stockpile_material_item(
    item: &MaterialItemOutput,
    breakdown: StackBreakdown,
    recipe_availability: &RecipeAvailability,
) -> StockpileMaterialItem {
    let namespace_id = item.block_id.clone();
    let id = namespace_id
        .split(':')
        .next_back()
        .unwrap_or(namespace_id.as_str())
        .to_string();
    let category = categorize_material(&namespace_id);
    let recipe_status = recipe_status_for_material(&namespace_id, recipe_availability);
    StockpileMaterialItem {
        id,
        namespace_id: namespace_id.clone(),
        display_name: item.display_name.clone(),
        required_count: item.total_count,
        stack_size: DEFAULT_STACK_SIZE,
        stacks: breakdown.full_stacks,
        remainder: breakdown.remainder,
        shulker_boxes: breakdown.shulker_boxes,
        category: category.name.to_string(),
        category_icon: category.icon.to_string(),
        item_icon_key: namespace_id.clone(),
        icon_path: String::new(),
        icon_available: false,
        source_regions: source_regions(item),
        recipe_status,
        craft_complexity: 0,
    }
}

#[derive(Debug, Clone)]
pub(crate) enum RecipeAvailability {
    Missing,
    Unresolved,
    Available(BTreeSet<String>),
}

fn resolve_recipe_availability(minecraft_version: &str) -> Result<RecipeAvailability> {
    let status = recipe_cache::recipe_status(minecraft_version)?;
    match status.status {
        RecipeCacheStatus::Available => {
            let items =
                recipe_cache::load_available_recipe_items(minecraft_version)?.unwrap_or_default();
            Ok(RecipeAvailability::Available(items))
        }
        RecipeCacheStatus::Missing => Ok(RecipeAvailability::Missing),
        RecipeCacheStatus::Stale
        | RecipeCacheStatus::Corrupted
        | RecipeCacheStatus::VersionMismatch => Ok(RecipeAvailability::Unresolved),
    }
}

fn recipe_status_for_material(
    namespace_id: &str,
    recipe_availability: &RecipeAvailability,
) -> String {
    match recipe_availability {
        RecipeAvailability::Missing => "missing",
        RecipeAvailability::Unresolved => "unresolved",
        RecipeAvailability::Available(items) => {
            if items.contains(namespace_id) {
                "available"
            } else {
                "unresolved"
            }
        }
    }
    .to_string()
}

fn source_regions(item: &MaterialItemOutput) -> Vec<String> {
    let mut regions = BTreeSet::<String>::new();
    if let Some(region_counts) = item.region_counts.as_ref() {
        for region in region_counts {
            if region.count > 0 {
                regions.insert(region.region.clone());
            }
        }
    }
    regions.into_iter().collect()
}

fn region_name(region: &RegionSummaryOutput) -> String {
    region.name.clone()
}

fn resolve_output_path(input: &Path, output: Option<&Path>) -> Result<PathBuf> {
    let path = match output {
        Some(path) => {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                runtime_paths::app_root()?.join(path)
            }
        }
        None => runtime_paths::stockpile_projects_root()?
            .join(project_slug(input))
            .join("materials.json"),
    };
    Ok(path)
}

fn ensure_stockpile_project_output(path: &Path) -> Result<()> {
    let projects_root = runtime_paths::stockpile_projects_root()?;
    let full_output = absolutize(path)?;
    let full_projects = absolutize(&projects_root)?;
    if !full_output.starts_with(&full_projects) {
        bail!(
            "stockpile export output must be under {}",
            full_projects.display()
        );
    }
    if let Some(parent) = full_output.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "create stockpile output directory failed: {}",
                parent.display()
            )
        })?;
    }
    Ok(())
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(runtime_paths::app_root()?.join(path))
}

fn project_slug(input: &Path) -> String {
    let raw = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("projection");
    let slug = raw
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect::<String>();
    if slug.is_empty() {
        "projection".to_string()
    } else {
        slug
    }
}

fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

fn stack_breakdown(count: u64, stack_size: u64) -> StackBreakdown {
    let stack_size = stack_size.max(1);
    let full_stacks = count / stack_size;
    let remainder = count % stack_size;
    let stack_units = full_stacks + u64::from(remainder > 0);
    StackBreakdown {
        full_stacks,
        remainder,
        stack_units,
        shulker_boxes: ceil_div(stack_units, SHULKER_STACKS),
    }
}

fn ceil_div(value: u64, divisor: u64) -> u64 {
    if value == 0 {
        0
    } else {
        ((value - 1) / divisor.max(1)) + 1
    }
}

fn categorize_material(namespace_id: &str) -> MaterialCategory {
    let local = namespace_id.split(':').next_back().unwrap_or(namespace_id);
    if contains_any(
        local,
        &[
            "redstone",
            "repeater",
            "comparator",
            "observer",
            "piston",
            "dispenser",
            "dropper",
            "hopper",
            "target",
            "daylight_detector",
            "sculk_sensor",
        ],
    ) {
        return category("红石制品", "minecraft:redstone");
    }
    if contains_any(
        local,
        &[
            "stone",
            "cobble",
            "brick",
            "deepslate",
            "andesite",
            "diorite",
            "granite",
            "tuff",
            "basalt",
            "blackstone",
            "quartz",
        ],
    ) {
        return category("石制品", "minecraft:stone");
    }
    if contains_any(
        local,
        &[
            "log", "wood", "planks", "slab", "stairs", "fence", "door", "trapdoor", "sign",
            "bamboo", "mangrove", "cherry",
        ],
    ) {
        return category("木制品", "minecraft:oak_planks");
    }
    if contains_any(local, &["glass", "pane"]) {
        return category("玻璃制品", "minecraft:glass");
    }
    if contains_any(local, &["rail", "minecart"]) {
        return category("铁轨/交通", "minecraft:rail");
    }
    if contains_any(
        local,
        &[
            "chest",
            "barrel",
            "shulker_box",
            "crafter",
            "furnace",
            "smoker",
            "blast_furnace",
        ],
    ) {
        return category("容器/存储", "minecraft:chest");
    }
    if contains_any(
        local,
        &[
            "torch",
            "lantern",
            "lamp",
            "light",
            "glowstone",
            "sea_lantern",
            "froglight",
            "candle",
            "end_rod",
        ],
    ) {
        return category("照明", "minecraft:torch");
    }
    if contains_any(
        local,
        &[
            "wool",
            "carpet",
            "terracotta",
            "concrete",
            "glazed",
            "banner",
            "bed",
            "flower_pot",
            "painting",
        ],
    ) {
        return category("染色/装饰", "minecraft:white_wool");
    }
    if contains_any(
        local,
        &[
            "dirt",
            "grass",
            "sand",
            "gravel",
            "clay",
            "mud",
            "snow",
            "ice",
            "leaves",
            "sapling",
            "flower",
            "moss",
            "nylium",
            "netherrack",
            "end_stone",
            "water",
            "lava",
            "dripleaf",
            "vine",
            "kelp",
            "coral",
        ],
    ) {
        return category("自然方块", "minecraft:grass_block");
    }
    if contains_any(
        local,
        &[
            "iron",
            "gold",
            "copper",
            "diamond",
            "emerald",
            "netherite",
            "lapis",
            "coal_block",
        ],
    ) {
        return category("金属制品", "minecraft:iron_ingot");
    }
    if contains_any(
        local,
        &[
            "crafting_table",
            "anvil",
            "enchanting_table",
            "brewing_stand",
            "beacon",
            "lectern",
            "loom",
            "cartography_table",
            "smithing_table",
            "stonecutter",
            "grindstone",
            "bell",
            "note_block",
            "jukebox",
        ],
    ) {
        return category("功能方块", "minecraft:crafting_table");
    }
    category("其他", "minecraft:barrier")
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn category(name: &'static str, icon: &'static str) -> MaterialCategory {
    MaterialCategory { name, icon }
}

#[cfg(test)]
mod tests {
    use super::{
        RecipeAvailability, build_stockpile_materials_data, categorize_material,
        recipe_status_for_material, stack_breakdown,
    };
    use crate::stats_api::{MaterialScope, build_materials_output};

    #[test]
    fn stack_breakdown_counts_full_stacks_remainder_and_shulkers() {
        let empty = stack_breakdown(0, 64);
        assert_eq!(empty.full_stacks, 0);
        assert_eq!(empty.remainder, 0);
        assert_eq!(empty.stack_units, 0);
        assert_eq!(empty.shulker_boxes, 0);

        let partial = stack_breakdown(65, 64);
        assert_eq!(partial.full_stacks, 1);
        assert_eq!(partial.remainder, 1);
        assert_eq!(partial.stack_units, 2);
        assert_eq!(partial.shulker_boxes, 1);

        let large = stack_breakdown(64 * 27 + 1, 64);
        assert_eq!(large.full_stacks, 27);
        assert_eq!(large.remainder, 1);
        assert_eq!(large.stack_units, 28);
        assert_eq!(large.shulker_boxes, 2);
    }

    #[test]
    fn material_categories_cover_core_stockpile_groups() {
        assert_eq!(categorize_material("minecraft:stone_bricks").name, "石制品");
        assert_eq!(categorize_material("minecraft:oak_planks").name, "木制品");
        assert_eq!(categorize_material("minecraft:glass_pane").name, "玻璃制品");
        assert_eq!(
            categorize_material("minecraft:redstone_torch").name,
            "红石制品"
        );
        assert_eq!(
            categorize_material("minecraft:redstone_wire").name,
            "红石制品"
        );
        assert_eq!(
            categorize_material("minecraft:powered_rail").name,
            "铁轨/交通"
        );
        assert_eq!(categorize_material("minecraft:barrel").name, "容器/存储");
        assert_eq!(categorize_material("minecraft:lantern").name, "照明");
        assert_eq!(
            categorize_material("minecraft:white_wool").name,
            "染色/装饰"
        );
        assert_eq!(
            categorize_material("minecraft:grass_block").name,
            "自然方块"
        );
        assert_eq!(categorize_material("minecraft:water").name, "自然方块");
        assert_eq!(categorize_material("minecraft:iron_block").name, "金属制品");
        assert_eq!(
            categorize_material("minecraft:crafting_table").name,
            "功能方块"
        );
        assert_eq!(categorize_material("minecraft:unknown_custom").name, "其他");
    }

    #[test]
    fn recipe_status_marks_missing_without_recipe_cache() {
        assert_eq!(
            recipe_status_for_material("minecraft:stone", &RecipeAvailability::Missing),
            "missing"
        );
    }

    #[test]
    fn export_data_builds_when_recipe_cache_is_missing() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stats_water_fixture.litematic");
        let stats =
            build_materials_output(&fixture, MaterialScope::All, false).expect("fixture stats");
        let output = build_stockpile_materials_data(&fixture, stats, &RecipeAvailability::Missing)
            .expect("stockpile data");
        assert!(!output.materials.is_empty());
        assert!(
            output
                .materials
                .iter()
                .all(|material| material.recipe_status == "missing")
        );
    }
}
