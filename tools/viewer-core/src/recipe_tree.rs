use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::recipe_cache;
use crate::stockpile::StockpileMaterialsData;

pub const MAX_RECIPE_DEPTH: u32 = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipeTreeNode {
    pub node_id: String,
    pub item_id: String,
    pub display_name: String,
    pub icon_key: String,
    pub needed_count: u64,
    pub output_count: u64,
    pub batch_count: u64,
    pub extra_output: u64,
    pub recipe_type: String,
    pub process_type: String,
    pub ingredients: Vec<RecipeTreeIngredient>,
    pub children: Vec<RecipeTreeNode>,
    pub unresolved: bool,
    pub unresolved_reason: Option<String>,
    pub visual_kind: String,
    pub depth: u32,
    pub tag: Option<String>,
    pub possible_items: Vec<String>,
    pub requires_fuel: bool,
    pub fuel_estimate: Option<u64>,
    pub decorative_smithing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipeTreeIngredient {
    pub item_id: String,
    pub display_name: String,
    pub icon_key: String,
    pub needed_count: u64,
    pub visual_kind: String,
    pub unresolved: bool,
    pub unresolved_reason: Option<String>,
    pub tag: Option<String>,
    pub possible_items: Vec<String>,
}

#[derive(Debug, Clone)]
struct RecipeCandidate {
    recipe_id: String,
    recipe_type: String,
    process_type: ProcessType,
    output_item: String,
    output_count: u64,
    ingredients: Vec<RecipeInput>,
    requires_fuel: bool,
    decorative_smithing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessType {
    Craft,
    Stonecut,
    Smelt,
    Blast,
    Smoke,
    Campfire,
    Smith,
    SmithTrim,
    Special,
}

#[derive(Debug, Clone)]
struct RecipeInput {
    role: String,
    spec: InputSpec,
    count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum InputSpec {
    Item(String),
    Tag(String),
    Alternatives(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TreeScore {
    unresolved_count: u32,
    special_count: u32,
    input_total: u64,
    ingredient_priority: u32,
    extra_output: u64,
    type_rank: u8,
}

pub fn resolve_recipe_trees_for_materials(
    minecraft_version: &str,
    materials: &StockpileMaterialsData,
) -> Result<BTreeMap<String, RecipeTreeNode>> {
    let Some(recipes) = recipe_cache::load_available_recipes(minecraft_version)? else {
        return Ok(BTreeMap::new());
    };
    Ok(resolve_recipe_trees_from_recipes(&recipes, materials))
}

pub fn resolve_recipe_trees_from_recipes(
    recipes: &BTreeMap<String, Value>,
    materials: &StockpileMaterialsData,
) -> BTreeMap<String, RecipeTreeNode> {
    let resolver = RecipeTreeResolver::new(recipes);
    materials
        .materials
        .iter()
        .filter(|material| material.recipe_status == "available")
        .map(|material| {
            (
                material.namespace_id.clone(),
                resolver.resolve(&material.namespace_id, material.required_count),
            )
        })
        .collect()
}

struct RecipeTreeResolver {
    candidates: BTreeMap<String, Vec<RecipeCandidate>>,
}

impl RecipeTreeResolver {
    fn new(recipes: &BTreeMap<String, Value>) -> Self {
        let mut candidates = BTreeMap::<String, Vec<RecipeCandidate>>::new();
        for (recipe_id, recipe) in recipes {
            if let Some(candidate) = parse_supported_recipe(recipe_id, recipe) {
                candidates
                    .entry(candidate.output_item.clone())
                    .or_default()
                    .push(candidate);
            }
        }
        for values in candidates.values_mut() {
            values.sort_by(compare_static_candidates);
        }
        Self { candidates }
    }

    fn resolve(&self, item_id: &str, needed_count: u64) -> RecipeTreeNode {
        self.resolve_inner(item_id, needed_count, 0, &mut BTreeSet::new())
    }

    fn resolve_inner(
        &self,
        item_id: &str,
        needed_count: u64,
        depth: u32,
        stack: &mut BTreeSet<String>,
    ) -> RecipeTreeNode {
        if item_id.starts_with('#') {
            return tag_node(item_id, needed_count, depth, Vec::new());
        }
        if stack.contains(item_id) {
            return unresolved_node(item_id, needed_count, depth, "cycle");
        }
        if depth >= MAX_RECIPE_DEPTH {
            return unresolved_node(item_id, needed_count, depth, "max_depth");
        }
        let Some(candidates) = self.candidates.get(item_id) else {
            if is_directly_obtainable_material(item_id) {
                return direct_node(item_id, needed_count, depth);
            }
            return unresolved_node(item_id, needed_count, depth, "no_recipe");
        };

        let mut best: Option<(TreeScore, RecipeTreeNode)> = None;
        for candidate in candidates {
            let node = self.resolve_candidate(candidate, needed_count, depth, stack);
            let score = score_tree(&node, candidate, is_stone_family(item_id));
            if best
                .as_ref()
                .is_none_or(|(best_score, _)| score < *best_score)
            {
                best = Some((score, node));
            }
        }
        best.map(|(_, node)| node)
            .unwrap_or_else(|| unresolved_node(item_id, needed_count, depth, "no_recipe"))
    }

    fn resolve_candidate(
        &self,
        candidate: &RecipeCandidate,
        needed_count: u64,
        depth: u32,
        stack: &mut BTreeSet<String>,
    ) -> RecipeTreeNode {
        let output_count = candidate.output_count.max(1);
        let batch_count = ceil_div(needed_count, output_count);
        let extra_output = batch_count
            .saturating_mul(output_count)
            .saturating_sub(needed_count);

        let mut ingredients = Vec::new();
        let mut children = Vec::new();
        if candidate.process_type == ProcessType::Special {
            return special_node(
                &candidate.output_item,
                needed_count,
                depth,
                &candidate.recipe_type,
            );
        }

        stack.insert(candidate.output_item.clone());
        for input in &candidate.ingredients {
            let ingredient_needed = input.count.saturating_mul(batch_count);
            ingredients.push(ingredient_summary(input, ingredient_needed));
            children.push(match &input.spec {
                InputSpec::Item(child_item) => {
                    self.resolve_inner(child_item, ingredient_needed, depth + 1, stack)
                }
                InputSpec::Tag(tag) => {
                    tag_node(tag, ingredient_needed, depth + 1, possible_items(input))
                }
                InputSpec::Alternatives(values) => {
                    alternative_node(values, ingredient_needed, depth + 1)
                }
            });
        }
        stack.remove(&candidate.output_item);

        RecipeTreeNode {
            node_id: node_id(&candidate.output_item, depth),
            item_id: candidate.output_item.clone(),
            display_name: display_name(&candidate.output_item),
            icon_key: candidate.output_item.clone(),
            needed_count,
            output_count,
            batch_count,
            extra_output,
            recipe_type: candidate.recipe_type.clone(),
            process_type: process_type_name(candidate.process_type).to_string(),
            ingredients,
            children,
            unresolved: false,
            unresolved_reason: None,
            visual_kind: "process".to_string(),
            depth,
            tag: None,
            possible_items: Vec::new(),
            requires_fuel: candidate.requires_fuel,
            fuel_estimate: None,
            decorative_smithing: candidate.decorative_smithing,
        }
    }
}

fn parse_supported_recipe(recipe_id: &str, recipe: &Value) -> Option<RecipeCandidate> {
    let recipe_type = recipe.get("type")?.as_str()?.to_string();
    let process_type = process_type_from_recipe_type(&recipe_type)?;
    if process_type == ProcessType::Special {
        return Some(RecipeCandidate {
            recipe_id: recipe_id.to_string(),
            recipe_type,
            process_type,
            output_item: recipe
                .get("result")
                .and_then(parse_result)
                .map(|(item, _)| item)
                .unwrap_or_else(|| recipe_id.to_string()),
            output_count: 1,
            ingredients: Vec::new(),
            requires_fuel: false,
            decorative_smithing: false,
        });
    }

    let (output_item, output_count) = parse_result(recipe.get("result")?)?;
    let ingredients = match process_type {
        ProcessType::Craft => {
            if recipe_type == "minecraft:crafting_shaped" {
                parse_shaped_ingredients(recipe)?
            } else {
                parse_shapeless_ingredients(recipe)?
            }
        }
        ProcessType::Stonecut => vec![RecipeInput {
            role: "ingredient".to_string(),
            spec: parse_input(recipe.get("ingredient")?)?,
            count: 1,
        }],
        ProcessType::Smelt | ProcessType::Blast | ProcessType::Smoke | ProcessType::Campfire => {
            vec![RecipeInput {
                role: "ingredient".to_string(),
                spec: parse_input(recipe.get("ingredient")?)?,
                count: 1,
            }]
        }
        ProcessType::Smith => vec![
            role_input("template", recipe.get("template")?)?,
            role_input("base", recipe.get("base")?)?,
            role_input("addition", recipe.get("addition")?)?,
        ],
        ProcessType::SmithTrim => vec![
            role_input("template", recipe.get("template")?)?,
            role_input("base", recipe.get("base")?)?,
            role_input("addition", recipe.get("addition")?)?,
        ],
        ProcessType::Special => Vec::new(),
    };

    Some(RecipeCandidate {
        recipe_id: recipe_id.to_string(),
        recipe_type,
        process_type,
        output_item,
        output_count,
        ingredients: aggregate_inputs(ingredients),
        requires_fuel: matches!(
            process_type,
            ProcessType::Smelt | ProcessType::Blast | ProcessType::Smoke | ProcessType::Campfire
        ),
        decorative_smithing: process_type == ProcessType::SmithTrim,
    })
}

fn process_type_from_recipe_type(recipe_type: &str) -> Option<ProcessType> {
    match recipe_type {
        "minecraft:crafting_shaped" | "minecraft:crafting_shapeless" => Some(ProcessType::Craft),
        "minecraft:stonecutting" => Some(ProcessType::Stonecut),
        "minecraft:smelting" => Some(ProcessType::Smelt),
        "minecraft:blasting" => Some(ProcessType::Blast),
        "minecraft:smoking" => Some(ProcessType::Smoke),
        "minecraft:campfire_cooking" => Some(ProcessType::Campfire),
        "minecraft:smithing_transform" => Some(ProcessType::Smith),
        "minecraft:smithing_trim" => Some(ProcessType::SmithTrim),
        value if value.starts_with("minecraft:crafting_special_") => Some(ProcessType::Special),
        _ => None,
    }
}

fn parse_result(result: &Value) -> Option<(String, u64)> {
    match result {
        Value::String(item) => Some((item.clone(), 1)),
        Value::Object(object) => {
            let item = object
                .get("id")
                .or_else(|| object.get("item"))?
                .as_str()?
                .to_string();
            let count = object.get("count").and_then(Value::as_u64).unwrap_or(1);
            Some((item, count.max(1)))
        }
        _ => None,
    }
}

fn parse_shaped_ingredients(recipe: &Value) -> Option<Vec<RecipeInput>> {
    let pattern = recipe.get("pattern")?.as_array()?;
    let key = recipe.get("key")?.as_object()?;
    let mut inputs = Vec::new();
    for row in pattern {
        for ch in row.as_str()?.chars().filter(|ch| *ch != ' ') {
            inputs.push(RecipeInput {
                role: "ingredient".to_string(),
                spec: parse_input(key.get(&ch.to_string())?)?,
                count: 1,
            });
        }
    }
    Some(inputs)
}

fn parse_shapeless_ingredients(recipe: &Value) -> Option<Vec<RecipeInput>> {
    recipe
        .get("ingredients")?
        .as_array()?
        .iter()
        .map(|value| {
            Some(RecipeInput {
                role: "ingredient".to_string(),
                spec: parse_input(value)?,
                count: 1,
            })
        })
        .collect()
}

fn role_input(role: &str, value: &Value) -> Option<RecipeInput> {
    Some(RecipeInput {
        role: role.to_string(),
        spec: parse_input(value)?,
        count: 1,
    })
}

fn parse_input(value: &Value) -> Option<InputSpec> {
    match value {
        Value::String(input) => Some(input_spec_from_string(input)),
        Value::Object(object) => {
            if let Some(item) = object.get("item").and_then(Value::as_str) {
                return Some(InputSpec::Item(item.to_string()));
            }
            if let Some(tag) = object.get("tag").and_then(Value::as_str) {
                return Some(InputSpec::Tag(format!("#{tag}")));
            }
            None
        }
        Value::Array(values) => {
            let alternatives = values
                .iter()
                .filter_map(|value| match parse_input(value)? {
                    InputSpec::Item(item) | InputSpec::Tag(item) => Some(item),
                    InputSpec::Alternatives(items) => Some(items.join("|")),
                })
                .collect::<Vec<_>>();
            if alternatives.len() == 1 {
                Some(input_spec_from_string(&alternatives[0]))
            } else if alternatives.is_empty() {
                None
            } else {
                Some(InputSpec::Alternatives(alternatives))
            }
        }
        _ => None,
    }
}

fn input_spec_from_string(input: &str) -> InputSpec {
    if input.starts_with('#') {
        InputSpec::Tag(input.to_string())
    } else {
        InputSpec::Item(input.to_string())
    }
}

fn aggregate_inputs(inputs: Vec<RecipeInput>) -> Vec<RecipeInput> {
    let mut counts = BTreeMap::<(String, InputSpec), u64>::new();
    for input in inputs {
        *counts.entry((input.role, input.spec)).or_default() += input.count;
    }
    counts
        .into_iter()
        .map(|((role, spec), count)| RecipeInput { role, spec, count })
        .collect()
}

fn compare_static_candidates(left: &RecipeCandidate, right: &RecipeCandidate) -> Ordering {
    static_candidate_priority(left)
        .cmp(&static_candidate_priority(right))
        .then_with(|| left.recipe_id.cmp(&right.recipe_id))
}

fn static_candidate_priority(
    candidate: &RecipeCandidate,
) -> (u8, u64, u32, std::cmp::Reverse<u64>) {
    (
        candidate_type_rank(candidate, false),
        candidate.ingredients.iter().map(|input| input.count).sum(),
        candidate_ingredient_priority(candidate),
        std::cmp::Reverse(candidate.output_count),
    )
}

fn score_tree(
    node: &RecipeTreeNode,
    candidate: &RecipeCandidate,
    stone_family_target: bool,
) -> TreeScore {
    TreeScore {
        unresolved_count: unresolved_count(node),
        special_count: special_count(node),
        input_total: candidate.ingredients.iter().map(|input| input.count).sum(),
        ingredient_priority: candidate_ingredient_priority(candidate),
        extra_output: node.extra_output,
        type_rank: candidate_type_rank(candidate, stone_family_target),
    }
}

fn candidate_ingredient_priority(candidate: &RecipeCandidate) -> u32 {
    candidate
        .ingredients
        .iter()
        .map(|input| input_priority(&input.spec))
        .sum()
}

fn input_priority(input: &InputSpec) -> u32 {
    match input {
        InputSpec::Item(item) => material_priority(item),
        InputSpec::Tag(tag) => tag_preferred_items(tag)
            .first()
            .map(|item| material_priority(item))
            .unwrap_or(5_000),
        InputSpec::Alternatives(items) => items
            .iter()
            .map(|item| material_priority(item))
            .min()
            .unwrap_or(5_000),
    }
}

fn material_priority(item_id: &str) -> u32 {
    let local = item_id.split(':').next_back().unwrap_or(item_id);
    match local {
        "iron_ore" => 0,
        "raw_iron" => 10,
        "deepslate_iron_ore" => 20,
        "iron_nugget" => 500,
        "oak_log" | "oak_planks" => 0,
        "oak_wood" => 1,
        "spruce_log" | "spruce_planks" => 10,
        "birch_log" | "birch_planks" => 11,
        "jungle_log" | "jungle_planks" => 12,
        "acacia_log" | "acacia_planks" => 13,
        "dark_oak_log" | "dark_oak_planks" => 14,
        "mangrove_log" | "mangrove_planks" => 15,
        "cherry_log" | "cherry_planks" => 16,
        "bamboo" | "bamboo_planks" | "bamboo_block" => 80,
        "crimson_stem" | "crimson_planks" | "warped_stem" | "warped_planks" => 90,
        _ => 1_000,
    }
}

fn candidate_type_rank(candidate: &RecipeCandidate, stone_family_target: bool) -> u8 {
    let output_local = candidate
        .output_item
        .split(':')
        .next_back()
        .unwrap_or(candidate.output_item.as_str());
    if output_local.ends_with("_ingot")
        && candidate.process_type == ProcessType::Craft
        && candidate.ingredients.iter().any(|input| match &input.spec {
            InputSpec::Item(item) => item.ends_with("_nugget"),
            InputSpec::Alternatives(items) => items.iter().any(|item| item.ends_with("_nugget")),
            InputSpec::Tag(_) => false,
        })
    {
        return 8;
    }
    static_type_rank(candidate.process_type, stone_family_target)
}

fn unresolved_count(node: &RecipeTreeNode) -> u32 {
    u32::from(node.unresolved)
        + node.children.iter().map(unresolved_count).sum::<u32>()
        + node
            .ingredients
            .iter()
            .filter(|ingredient| ingredient.unresolved)
            .count() as u32
}

fn special_count(node: &RecipeTreeNode) -> u32 {
    u32::from(node.visual_kind == "special") + node.children.iter().map(special_count).sum::<u32>()
}

fn static_type_rank(process_type: ProcessType, stone_family_target: bool) -> u8 {
    match process_type {
        ProcessType::Stonecut if stone_family_target => 0,
        ProcessType::Craft => 1,
        ProcessType::Stonecut => 2,
        ProcessType::Smelt => 3,
        ProcessType::Blast => 4,
        ProcessType::Smoke => 5,
        ProcessType::Campfire => 6,
        ProcessType::Smith => 7,
        ProcessType::SmithTrim => 8,
        ProcessType::Special => 9,
    }
}

fn ingredient_summary(input: &RecipeInput, needed_count: u64) -> RecipeTreeIngredient {
    let role_prefix = if input.role == "ingredient" {
        String::new()
    } else {
        format!("{}: ", input.role)
    };
    match &input.spec {
        InputSpec::Item(item) => RecipeTreeIngredient {
            item_id: item.clone(),
            display_name: format!("{role_prefix}{}", display_name(item)),
            icon_key: item.clone(),
            needed_count,
            visual_kind: "item".to_string(),
            unresolved: false,
            unresolved_reason: None,
            tag: None,
            possible_items: Vec::new(),
        },
        InputSpec::Tag(tag) => RecipeTreeIngredient {
            item_id: tag.clone(),
            display_name: format!("{role_prefix}{}", tag_display_name(tag)),
            icon_key: tag_icon_key(tag),
            needed_count,
            visual_kind: "tag".to_string(),
            unresolved: false,
            unresolved_reason: None,
            tag: Some(tag.clone()),
            possible_items: possible_items(input),
        },
        InputSpec::Alternatives(items) => RecipeTreeIngredient {
            item_id: items.join("|"),
            display_name: format!(
                "{role_prefix}{}",
                items
                    .iter()
                    .map(|item| display_name(item))
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
            icon_key: sorted_alternatives(items)
                .first()
                .cloned()
                .unwrap_or_default(),
            needed_count,
            visual_kind: "tag".to_string(),
            unresolved: false,
            unresolved_reason: None,
            tag: None,
            possible_items: sorted_alternatives(items),
        },
    }
}

fn tag_node(
    tag: &str,
    needed_count: u64,
    depth: u32,
    possible_items: Vec<String>,
) -> RecipeTreeNode {
    RecipeTreeNode {
        node_id: node_id(tag, depth),
        item_id: tag.to_string(),
        display_name: tag_display_name(tag),
        icon_key: tag_icon_key(tag),
        needed_count,
        output_count: 0,
        batch_count: 0,
        extra_output: 0,
        recipe_type: "tag_input".to_string(),
        process_type: "tag".to_string(),
        ingredients: Vec::new(),
        children: Vec::new(),
        unresolved: false,
        unresolved_reason: None,
        visual_kind: "tag".to_string(),
        depth,
        tag: Some(tag.to_string()),
        possible_items: if possible_items.is_empty() {
            tag_preferred_items(tag)
        } else {
            sorted_alternatives(&possible_items)
        },
        requires_fuel: false,
        fuel_estimate: None,
        decorative_smithing: false,
    }
}

fn alternative_node(values: &[String], needed_count: u64, depth: u32) -> RecipeTreeNode {
    RecipeTreeNode {
        node_id: node_id(&values.join("|"), depth),
        item_id: values.join("|"),
        display_name: "Alternative material group".to_string(),
        icon_key: sorted_alternatives(values)
            .first()
            .cloned()
            .unwrap_or_default(),
        needed_count,
        output_count: 0,
        batch_count: 0,
        extra_output: 0,
        recipe_type: "tag_input".to_string(),
        process_type: "tag".to_string(),
        ingredients: Vec::new(),
        children: Vec::new(),
        unresolved: false,
        unresolved_reason: None,
        visual_kind: "tag".to_string(),
        depth,
        tag: None,
        possible_items: sorted_alternatives(values),
        requires_fuel: false,
        fuel_estimate: None,
        decorative_smithing: false,
    }
}

fn special_node(item_id: &str, needed_count: u64, depth: u32, recipe_type: &str) -> RecipeTreeNode {
    RecipeTreeNode {
        node_id: node_id(item_id, depth),
        item_id: item_id.to_string(),
        display_name: display_name(item_id),
        icon_key: item_id.to_string(),
        needed_count,
        output_count: 0,
        batch_count: 0,
        extra_output: 0,
        recipe_type: recipe_type.to_string(),
        process_type: "special".to_string(),
        ingredients: Vec::new(),
        children: Vec::new(),
        unresolved: true,
        unresolved_reason: Some("special_recipe".to_string()),
        visual_kind: "special".to_string(),
        depth,
        tag: None,
        possible_items: Vec::new(),
        requires_fuel: false,
        fuel_estimate: None,
        decorative_smithing: false,
    }
}

fn unresolved_node(item_id: &str, needed_count: u64, depth: u32, reason: &str) -> RecipeTreeNode {
    RecipeTreeNode {
        node_id: node_id(item_id, depth),
        item_id: item_id.to_string(),
        display_name: display_name(item_id),
        icon_key: item_id.to_string(),
        needed_count,
        output_count: 0,
        batch_count: 0,
        extra_output: 0,
        recipe_type: "unresolved".to_string(),
        process_type: "unresolved".to_string(),
        ingredients: Vec::new(),
        children: Vec::new(),
        unresolved: true,
        unresolved_reason: Some(reason.to_string()),
        visual_kind: "unresolved".to_string(),
        depth,
        tag: None,
        possible_items: Vec::new(),
        requires_fuel: false,
        fuel_estimate: None,
        decorative_smithing: false,
    }
}

fn direct_node(item_id: &str, needed_count: u64, depth: u32) -> RecipeTreeNode {
    RecipeTreeNode {
        node_id: node_id(item_id, depth),
        item_id: item_id.to_string(),
        display_name: display_name(item_id),
        icon_key: item_id.to_string(),
        needed_count,
        output_count: 0,
        batch_count: 0,
        extra_output: 0,
        recipe_type: "direct".to_string(),
        process_type: "direct".to_string(),
        ingredients: Vec::new(),
        children: Vec::new(),
        unresolved: false,
        unresolved_reason: None,
        visual_kind: "direct".to_string(),
        depth,
        tag: None,
        possible_items: Vec::new(),
        requires_fuel: false,
        fuel_estimate: None,
        decorative_smithing: false,
    }
}

fn is_directly_obtainable_material(item_id: &str) -> bool {
    let local = item_id.split(':').next_back().unwrap_or(item_id);
    [
        "stone",
        "cobblestone",
        "iron_ore",
        "deepslate_iron_ore",
        "dirt",
        "sand",
        "gravel",
        "clay",
        "netherrack",
        "basalt",
        "tuff",
        "deepslate",
        "granite",
        "diorite",
        "andesite",
        "oak_log",
        "spruce_log",
        "birch_log",
        "jungle_log",
        "acacia_log",
        "dark_oak_log",
    ]
    .contains(&local)
        || local.ends_with("_ore")
        || local.ends_with("_log")
        || local.ends_with("_wood")
        || local.ends_with("_leaves")
}

fn possible_items(input: &RecipeInput) -> Vec<String> {
    match &input.spec {
        InputSpec::Tag(tag) => tag_preferred_items(tag),
        InputSpec::Alternatives(items) => sorted_alternatives(items),
        _ => Vec::new(),
    }
}

fn sorted_alternatives(items: &[String]) -> Vec<String> {
    let mut values = items.to_vec();
    values.sort_by_key(|item| (material_priority(item), item.clone()));
    values
}

fn tag_preferred_items(tag: &str) -> Vec<String> {
    match tag {
        "#minecraft:planks" => vec![
            "minecraft:oak_planks",
            "minecraft:spruce_planks",
            "minecraft:birch_planks",
            "minecraft:jungle_planks",
            "minecraft:acacia_planks",
            "minecraft:dark_oak_planks",
            "minecraft:mangrove_planks",
            "minecraft:cherry_planks",
            "minecraft:bamboo_planks",
            "minecraft:crimson_planks",
            "minecraft:warped_planks",
        ],
        "#minecraft:logs" | "#minecraft:logs_that_burn" => vec![
            "minecraft:oak_log",
            "minecraft:spruce_log",
            "minecraft:birch_log",
            "minecraft:jungle_log",
            "minecraft:acacia_log",
            "minecraft:dark_oak_log",
            "minecraft:mangrove_log",
            "minecraft:cherry_log",
            "minecraft:crimson_stem",
            "minecraft:warped_stem",
        ],
        "#minecraft:stone_crafting_materials" => {
            vec!["minecraft:cobblestone", "minecraft:blackstone"]
        }
        "#minecraft:stone_tool_materials" => vec!["minecraft:cobblestone", "minecraft:blackstone"],
        "#minecraft:coals" => vec!["minecraft:coal", "minecraft:charcoal"],
        "#minecraft:iron_ores" => vec!["minecraft:iron_ore", "minecraft:deepslate_iron_ore"],
        _ => Vec::new(),
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn tag_display_name(tag: &str) -> String {
    match tag {
        "#minecraft:planks" => "Any planks".to_string(),
        "#minecraft:logs" | "#minecraft:logs_that_burn" => "Any logs".to_string(),
        "#minecraft:stone_crafting_materials" => "Any stone material".to_string(),
        "#minecraft:stone_tool_materials" => "Any stone tool material".to_string(),
        "#minecraft:coals" => "Any coal".to_string(),
        "#minecraft:iron_ores" => "Any iron ore".to_string(),
        _ => "Alternative material group".to_string(),
    }
}

fn tag_icon_key(tag: &str) -> String {
    match tag {
        "#minecraft:planks" => "minecraft:oak_planks",
        "#minecraft:logs" | "#minecraft:logs_that_burn" => "minecraft:oak_log",
        "#minecraft:stone_crafting_materials" | "#minecraft:stone_tool_materials" => {
            "minecraft:cobblestone"
        }
        "#minecraft:coals" => "minecraft:coal",
        "#minecraft:iron_ores" => "minecraft:iron_ore",
        _ => "__tag",
    }
    .to_string()
}

fn process_type_name(process_type: ProcessType) -> &'static str {
    match process_type {
        ProcessType::Craft => "craft",
        ProcessType::Stonecut => "stonecut",
        ProcessType::Smelt => "smelt",
        ProcessType::Blast => "blast",
        ProcessType::Smoke => "smoke",
        ProcessType::Campfire => "campfire",
        ProcessType::Smith => "smith",
        ProcessType::SmithTrim => "smith_trim",
        ProcessType::Special => "special",
    }
}

fn ceil_div(value: u64, divisor: u64) -> u64 {
    if value == 0 {
        0
    } else {
        ((value - 1) / divisor.max(1)) + 1
    }
}

fn display_name(item_id: &str) -> String {
    if item_id.starts_with('#') {
        return item_id.to_string();
    }
    item_id
        .split(':')
        .next_back()
        .unwrap_or(item_id)
        .replace('_', " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn node_id(item_id: &str, depth: u32) -> String {
    format!(
        "{}_{}",
        item_id
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>(),
        depth
    )
}

fn is_stone_family(item_id: &str) -> bool {
    let local = item_id.split(':').next_back().unwrap_or(item_id);
    [
        "stone",
        "deepslate",
        "andesite",
        "diorite",
        "granite",
        "tuff",
        "basalt",
        "blackstone",
        "brick",
        "quartz",
        "copper",
    ]
    .iter()
    .any(|needle| local.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stockpile::{
        StockpileMaterialItem, StockpileMaterialsData, StockpileProjectInfo, StockpileSummary,
    };
    use serde_json::json;

    #[test]
    fn resolves_shaped_recipe_and_batch_math() {
        let resolver = RecipeTreeResolver::new(&recipe_map([(
            "minecraft:rail",
            json!({
                "type": "minecraft:crafting_shaped",
                "pattern": ["X X", "X#X", "X X"],
                "key": { "X": "minecraft:iron_ingot", "#": "minecraft:stick" },
                "result": { "id": "minecraft:rail", "count": 16 }
            }),
        )]));
        let tree = resolver.resolve("minecraft:rail", 17);
        assert_eq!(tree.process_type, "craft");
        assert_eq!(tree.visual_kind, "process");
        assert_eq!(tree.output_count, 16);
        assert_eq!(tree.batch_count, 2);
        assert_eq!(tree.extra_output, 15);
        assert_eq!(tree.depth, 0);
        assert_eq!(tree.icon_key, "minecraft:rail");
        assert_eq!(tree.ingredients.len(), 2);
        assert_eq!(
            tree.ingredients
                .iter()
                .find(|input| input.item_id == "minecraft:iron_ingot")
                .expect("iron input")
                .needed_count,
            12
        );
    }

    #[test]
    fn resolves_shapeless_recipe_recursively() {
        let resolver = RecipeTreeResolver::new(&recipe_map([
            (
                "minecraft:acacia_button",
                json!({
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["minecraft:acacia_planks"],
                    "result": { "id": "minecraft:acacia_button", "count": 1 }
                }),
            ),
            (
                "minecraft:acacia_planks",
                json!({
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["minecraft:acacia_log"],
                    "result": { "id": "minecraft:acacia_planks", "count": 4 }
                }),
            ),
        ]));
        let tree = resolver.resolve("minecraft:acacia_button", 3);
        assert_eq!(tree.process_type, "craft");
        assert_eq!(tree.children[0].item_id, "minecraft:acacia_planks");
        assert_eq!(tree.children[0].children[0].item_id, "minecraft:acacia_log");
    }

    #[test]
    fn resolves_stonecutting_recipe() {
        let resolver = RecipeTreeResolver::new(&recipe_map([(
            "minecraft:stone_bricks_from_stone_stonecutting",
            json!({
                "type": "minecraft:stonecutting",
                "ingredient": "minecraft:stone",
                "result": { "id": "minecraft:stone_bricks", "count": 1 }
            }),
        )]));
        let tree = resolver.resolve("minecraft:stone_bricks", 2);
        assert_eq!(tree.process_type, "stonecut");
        assert_eq!(tree.children[0].item_id, "minecraft:stone");
    }

    #[test]
    fn resolves_smelting_as_fuel_requiring_process() {
        let resolver = RecipeTreeResolver::new(&recipe_map([(
            "minecraft:glass",
            json!({
                "type": "minecraft:smelting",
                "ingredient": "minecraft:sand",
                "result": { "id": "minecraft:glass" }
            }),
        )]));
        let tree = resolver.resolve("minecraft:glass", 64);
        assert_eq!(tree.process_type, "smelt");
        assert!(tree.requires_fuel);
        assert_eq!(tree.fuel_estimate, None);
        assert_eq!(tree.batch_count, 64);
        assert_eq!(tree.children[0].item_id, "minecraft:sand");
        assert_eq!(tree.children[0].needed_count, 64);
    }

    #[test]
    fn ingot_recipe_prefers_furnace_ore_over_blasting_deepslate_or_nuggets() {
        let resolver = RecipeTreeResolver::new(&recipe_map([
            (
                "minecraft:iron_ingot_from_nuggets",
                json!({
                    "type": "minecraft:crafting_shaped",
                    "pattern": ["###", "###", "###"],
                    "key": { "#": "minecraft:iron_nugget" },
                    "result": { "id": "minecraft:iron_ingot", "count": 1 }
                }),
            ),
            (
                "minecraft:iron_ingot_from_blasting_deepslate_iron_ore",
                json!({
                    "type": "minecraft:blasting",
                    "ingredient": "minecraft:deepslate_iron_ore",
                    "result": { "id": "minecraft:iron_ingot", "count": 1 }
                }),
            ),
            (
                "minecraft:iron_ingot_from_smelting_iron_ore",
                json!({
                    "type": "minecraft:smelting",
                    "ingredient": "minecraft:iron_ore",
                    "result": { "id": "minecraft:iron_ingot", "count": 1 }
                }),
            ),
        ]));
        let tree = resolver.resolve("minecraft:iron_ingot", 1);
        assert_eq!(tree.process_type, "smelt");
        assert_eq!(tree.ingredients[0].item_id, "minecraft:iron_ore");
    }

    #[test]
    fn resolves_blasting_smoking_and_campfire() {
        for (recipe_type, process) in [
            ("minecraft:blasting", "blast"),
            ("minecraft:smoking", "smoke"),
            ("minecraft:campfire_cooking", "campfire"),
        ] {
            let resolver = RecipeTreeResolver::new(&recipe_map([(
                "minecraft:processed",
                json!({
                    "type": recipe_type,
                    "ingredient": "minecraft:raw",
                    "result": { "id": "minecraft:processed" }
                }),
            )]));
            let tree = resolver.resolve("minecraft:processed", 2);
            assert_eq!(tree.process_type, process);
            assert!(tree.requires_fuel);
            assert_eq!(tree.children[0].item_id, "minecraft:raw");
        }
    }

    #[test]
    fn resolves_smithing_transform() {
        let resolver = RecipeTreeResolver::new(&recipe_map([(
            "minecraft:netherite_axe_smithing",
            json!({
                "type": "minecraft:smithing_transform",
                "template": "minecraft:netherite_upgrade_smithing_template",
                "base": "minecraft:diamond_axe",
                "addition": "#minecraft:netherite_tool_materials",
                "result": { "id": "minecraft:netherite_axe" }
            }),
        )]));
        let tree = resolver.resolve("minecraft:netherite_axe", 1);
        assert_eq!(tree.process_type, "smith");
        assert_eq!(tree.ingredients.len(), 3);
        assert!(
            tree.children
                .iter()
                .any(|child| child.item_id == "minecraft:diamond_axe")
        );
        assert!(tree.children.iter().any(|child| child.visual_kind == "tag"));
    }

    #[test]
    fn parses_smithing_trim_as_decorative_when_result_is_present() {
        let resolver = RecipeTreeResolver::new(&recipe_map([(
            "minecraft:test_trim",
            json!({
                "type": "minecraft:smithing_trim",
                "template": "minecraft:bolt_armor_trim_smithing_template",
                "base": "#minecraft:trimmable_armor",
                "addition": "#minecraft:trim_materials",
                "result": { "id": "minecraft:trimmed_armor" }
            }),
        )]));
        let tree = resolver.resolve("minecraft:trimmed_armor", 1);
        assert_eq!(tree.process_type, "smith_trim");
        assert!(tree.decorative_smithing);
    }

    #[test]
    fn special_recipe_is_unresolved_and_not_static_expanded() {
        let resolver = RecipeTreeResolver::new(&recipe_map([(
            "minecraft:armor_dye",
            json!({
                "type": "minecraft:crafting_special_armordye",
                "result": { "id": "minecraft:dyed_armor" }
            }),
        )]));
        let tree = resolver.resolve("minecraft:dyed_armor", 1);
        assert!(tree.unresolved);
        assert_eq!(tree.visual_kind, "special");
        assert_eq!(tree.unresolved_reason.as_deref(), Some("special_recipe"));
        assert_eq!(tree.recipe_type, "minecraft:crafting_special_armordye");
    }

    #[test]
    fn tag_input_becomes_localized_alternative_tag_node() {
        let resolver = RecipeTreeResolver::new(&recipe_map([(
            "minecraft:chest",
            json!({
                "type": "minecraft:crafting_shaped",
                "pattern": ["###", "# #", "###"],
                "key": { "#": "#minecraft:planks" },
                "result": { "id": "minecraft:chest", "count": 1 }
            }),
        )]));
        let tree = resolver.resolve("minecraft:chest", 1);
        assert_eq!(tree.children[0].visual_kind, "tag");
        assert_eq!(tree.children[0].tag.as_deref(), Some("#minecraft:planks"));
        assert!(!tree.children[0].unresolved);
        assert_eq!(tree.children[0].icon_key, "minecraft:oak_planks");
        assert_eq!(tree.children[0].possible_items[0], "minecraft:oak_planks");
    }

    #[test]
    fn no_recipe_node_is_explicit() {
        let resolver = RecipeTreeResolver::new(&BTreeMap::new());
        let tree = resolver.resolve("minecraft:diamond", 1);
        assert!(tree.unresolved);
        assert_eq!(tree.visual_kind, "unresolved");
        assert_eq!(tree.unresolved_reason.as_deref(), Some("no_recipe"));
    }

    #[test]
    fn cycle_guard_stops_recursive_loop() {
        let resolver = RecipeTreeResolver::new(&recipe_map([
            (
                "minecraft:a",
                json!({
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["minecraft:b"],
                    "result": { "id": "minecraft:a", "count": 1 }
                }),
            ),
            (
                "minecraft:b",
                json!({
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["minecraft:a"],
                    "result": { "id": "minecraft:b", "count": 1 }
                }),
            ),
        ]));
        let tree = resolver.resolve("minecraft:a", 1);
        assert_eq!(
            tree.children[0].children[0].unresolved_reason.as_deref(),
            Some("cycle")
        );
    }

    #[test]
    fn max_depth_guard_stops_long_chain() {
        let recipes = (0..16)
            .map(|index| {
                (
                    format!("minecraft:item_{index}"),
                    json!({
                        "type": "minecraft:crafting_shapeless",
                        "ingredients": [format!("minecraft:item_{}", index + 1)],
                        "result": { "id": format!("minecraft:item_{index}"), "count": 1 }
                    }),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let resolver = RecipeTreeResolver::new(&recipes);
        let tree = resolver.resolve("minecraft:item_0", 1);
        assert!(contains_unresolved_reason(&tree, "max_depth"));
    }

    #[test]
    fn candidate_selection_prefers_expandable_recipe_before_low_input_recipe() {
        let resolver = RecipeTreeResolver::new(&recipe_map([
            (
                "minecraft:target_bad",
                json!({
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["minecraft:missing"],
                    "result": { "id": "minecraft:target", "count": 1 }
                }),
            ),
            (
                "minecraft:target_good",
                json!({
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["minecraft:known_a", "minecraft:known_b"],
                    "result": { "id": "minecraft:target", "count": 1 }
                }),
            ),
            leaf_recipe("minecraft:known_a"),
            leaf_recipe("minecraft:known_b"),
        ]));
        let tree = resolver.resolve("minecraft:target", 1);
        assert!(
            tree.children
                .iter()
                .any(|child| child.item_id == "minecraft:known_a")
        );
    }

    #[test]
    fn stone_family_prefers_stonecutting() {
        let resolver = RecipeTreeResolver::new(&recipe_map([
            (
                "minecraft:stone_bricks",
                json!({
                    "type": "minecraft:crafting_shaped",
                    "pattern": ["##", "##"],
                    "key": { "#": "minecraft:stone" },
                    "result": { "id": "minecraft:stone_bricks", "count": 4 }
                }),
            ),
            (
                "minecraft:stone_bricks_from_stone_stonecutting",
                json!({
                    "type": "minecraft:stonecutting",
                    "ingredient": "minecraft:stone",
                    "result": { "id": "minecraft:stone_bricks", "count": 1 }
                }),
            ),
        ]));
        let tree = resolver.resolve("minecraft:stone_bricks", 1);
        assert_eq!(tree.process_type, "stonecut");
    }

    #[test]
    fn resolves_material_map_for_available_materials() {
        let materials = materials_data([("minecraft:rail", 16, "available")]);
        let trees = resolve_recipe_trees_from_recipes(
            &recipe_map([(
                "minecraft:rail",
                json!({
                    "type": "minecraft:crafting_shaped",
                    "pattern": ["X X", "X#X", "X X"],
                    "key": { "X": "minecraft:iron_ingot", "#": "minecraft:stick" },
                    "result": { "id": "minecraft:rail", "count": 16 }
                }),
            )]),
            &materials,
        );
        assert!(trees.contains_key("minecraft:rail"));
    }

    fn recipe_map<const N: usize>(values: [(&str, Value); N]) -> BTreeMap<String, Value> {
        values
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    fn leaf_recipe(item: &str) -> (&str, Value) {
        let leaked: &'static str = Box::leak(format!("{item}_leaf").into_boxed_str());
        (
            leaked,
            json!({
                "type": "minecraft:crafting_shapeless",
                "ingredients": [],
                "result": { "id": item, "count": 1 }
            }),
        )
    }

    fn materials_data<const N: usize>(values: [(&str, u64, &str); N]) -> StockpileMaterialsData {
        StockpileMaterialsData {
            schema_version: crate::stockpile_schema::STOCKPILE_MATERIALS_SCHEMA_VERSION,
            project: StockpileProjectInfo {
                source_file: "fixture.litematic".to_string(),
                created_at: 0,
                data_version: 0,
                regions: Vec::new(),
            },
            summary: StockpileSummary {
                total_blocks: 0,
                unique_materials: values.len(),
                total_stacks: 0,
                estimated_shulker_boxes: 0,
            },
            materials: values
                .into_iter()
                .map(|(id, count, status)| StockpileMaterialItem {
                    id: id.split(':').next_back().unwrap_or(id).to_string(),
                    namespace_id: id.to_string(),
                    display_name: display_name(id),
                    required_count: count,
                    stack_size: 64,
                    stacks: 0,
                    remainder: count,
                    shulker_boxes: 1,
                    category: "test".to_string(),
                    category_icon: "minecraft:stone".to_string(),
                    item_icon_key: id.to_string(),
                    icon_path: String::new(),
                    icon_available: false,
                    display_names: std::collections::BTreeMap::new(),
                    source_regions: Vec::new(),
                    recipe_status: status.to_string(),
                    craft_complexity: 0,
                })
                .collect(),
        }
    }

    fn contains_unresolved_reason(node: &RecipeTreeNode, reason: &str) -> bool {
        node.unresolved_reason.as_deref() == Some(reason)
            || node
                .children
                .iter()
                .any(|child| contains_unresolved_reason(child, reason))
    }
}
