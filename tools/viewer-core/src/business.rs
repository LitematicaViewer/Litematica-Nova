use std::collections::{HashMap, HashSet};

use crate::model::{
    AnalysisDerivedOutput, BuildingStatsOutput, CategorizedCountGroupOutput, CountEntryOutput,
};

const CATEGORY_RULES: &[(&str, &[&str])] = &[
    ("wool", &["wool", "carpet"]),
    ("terracotta", &["terracotta"]),
    ("concrete", &["concrete"]),
    ("glass", &["glass"]),
    (
        "wood",
        &[
            "birch", "oak", "bamboo", "cherry", "crimson", "acacia", "jungle", "mangrove",
            "spruce", "warped", "azalea",
        ],
    ),
    ("quartz", &["quartz"]),
    ("ore", &["copper", "ore", "raw"]),
    (
        "redstone",
        &[
            "redstone",
            "lever",
            "observer",
            "note",
            "piston",
            "dispenser",
            "dropper",
            "hopper",
            "trapped",
            "slime",
            "honey",
            "detector",
            "sensor",
            "target",
            "tnt",
            "repeater",
            "comparator",
            "crafter",
            "hook",
        ],
    ),
    ("container", &["chest", "box", "barrel", "composter", "pot"]),
    ("iron", &["iron", "anvil", "cauldron", "heavy", "rail"]),
    ("end", &["obsidian", "end", "purpur"]),
    (
        "nether",
        &["glowstone", "nether", "netherrack", "magma", "soul"],
    ),
    (
        "stone",
        &["cobblestone", "stone", "blackstone", "deepslate"],
    ),
    (
        "rock",
        &["calcite", "granite", "andesite", "diorite", "tuff"],
    ),
    (
        "natural",
        &[
            "sand",
            "sandstone",
            "gravel",
            "dirt",
            "mud",
            "podzol",
            "mycelium",
            "farmland",
            "grass",
            "ice",
            "snow",
        ],
    ),
    ("marine", &["prismarine", "sea", "coral"]),
    ("clay", &["clay", "brick"]),
    ("fluid", &["water", "lava", "bubble"]),
];

pub fn build_analysis_derived(
    block_counts: &HashMap<String, u64>,
    metadata_total_volume: i32,
    enclosing_size: (i32, i32, i32),
    total_non_air_blocks: u64,
) -> AnalysisDerivedOutput {
    let material_counts = aggregate_material_counts(block_counts);
    let categorized_groups = categorize_material_counts(&material_counts);
    let category_totals = categorized_groups
        .iter()
        .map(|group| CountEntryOutput {
            key: group.category.clone(),
            count: group.total,
        })
        .collect();
    let building = compute_building_stats(
        &material_counts,
        &categorized_groups,
        metadata_total_volume,
        enclosing_size,
        total_non_air_blocks,
    );

    AnalysisDerivedOutput {
        material_counts,
        category_totals,
        categorized_groups,
        building,
    }
}

fn aggregate_material_counts(block_counts: &HashMap<String, u64>) -> Vec<CountEntryOutput> {
    let mut aggregated = HashMap::<String, u64>::new();
    for (block_id, count) in block_counts {
        let base_id = namespaced_to_base_id(block_id);
        *aggregated.entry(base_id).or_insert(0) += count;
    }

    to_sorted_entries(aggregated)
}

fn categorize_material_counts(
    material_counts: &[CountEntryOutput],
) -> Vec<CategorizedCountGroupOutput> {
    let mut grouped = HashMap::<String, Vec<CountEntryOutput>>::new();
    for row in material_counts {
        grouped
            .entry(categorize_block(&row.key))
            .or_default()
            .push(row.clone());
    }

    let mut groups = grouped
        .into_iter()
        .map(|(category, mut rows)| {
            rows.sort_by(|left, right| {
                right
                    .count
                    .cmp(&left.count)
                    .then_with(|| left.key.cmp(&right.key))
            });
            let total = rows.iter().map(|row| row.count).sum();
            CategorizedCountGroupOutput {
                category,
                total,
                rows,
            }
        })
        .collect::<Vec<_>>();

    groups.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| left.category.cmp(&right.category))
    });
    groups
}

fn compute_building_stats(
    material_counts: &[CountEntryOutput],
    categorized_groups: &[CategorizedCountGroupOutput],
    metadata_total_volume: i32,
    enclosing_size: (i32, i32, i32),
    total_non_air_blocks: u64,
) -> BuildingStatsOutput {
    let enclosing_volume = i64::from(enclosing_size.0.max(1))
        * i64::from(enclosing_size.1.max(1))
        * i64::from(enclosing_size.2.max(1));
    let total_volume = metadata_total_volume.max(1) as i64;
    let effective_volume = total_volume.max(enclosing_volume).max(1) as f64;
    let density = total_non_air_blocks as f64 / effective_volume;

    let redstone_count = category_total(categorized_groups, "redstone")
        + category_total(categorized_groups, "container");
    let fluid_count = category_total(categorized_groups, "fluid");
    let top_block_count = material_counts.first().map(|row| row.count).unwrap_or(0);
    let redstone_base = if material_counts.len() > 5 {
        total_non_air_blocks.saturating_sub(top_block_count)
    } else {
        total_non_air_blocks
    };
    let redstone_ratio = redstone_count as f64 / redstone_base.max(1) as f64;

    let building_type = if total_non_air_blocks <= 10 {
        "too_small"
    } else if redstone_ratio > 0.5 {
        "redstone_machine"
    } else if redstone_ratio >= 0.3 {
        "redstone_power"
    } else if redstone_ratio >= 0.1 {
        "mechanical_build"
    } else if redstone_ratio >= 0.01 {
        "structural_mechanism"
    } else {
        "building"
    };

    BuildingStatsOutput {
        density,
        fluid_count,
        fluid_ratio: fluid_count as f64 / total_non_air_blocks.max(1) as f64,
        redstone_count,
        redstone_ratio,
        building_type: building_type.to_string(),
        unique_block_types: material_counts.len() as u64,
    }
}

fn category_total(groups: &[CategorizedCountGroupOutput], category: &str) -> u64 {
    groups
        .iter()
        .find(|group| group.category == category)
        .map(|group| group.total)
        .unwrap_or(0)
}

fn categorize_block(base_id: &str) -> String {
    let parts = base_id.split('_').collect::<HashSet<_>>();
    for (category, values) in CATEGORY_RULES {
        if values.iter().any(|value| parts.contains(value)) {
            return (*category).to_string();
        }
    }
    "other".to_string()
}

fn namespaced_to_base_id(block_id: &str) -> String {
    let state_free = block_id.split('[').next().unwrap_or(block_id);
    let namespaced = if state_free.contains(':') {
        state_free
    } else {
        return state_free.to_string();
    };
    namespaced
        .split(':')
        .next_back()
        .unwrap_or(namespaced)
        .to_string()
}

fn to_sorted_entries(entries: HashMap<String, u64>) -> Vec<CountEntryOutput> {
    let mut rows = entries
        .into_iter()
        .map(|(key, count)| CountEntryOutput { key, count })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.key.cmp(&right.key))
    });
    rows
}
