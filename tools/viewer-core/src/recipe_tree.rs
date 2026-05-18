use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::recipe_cache;
use crate::stockpile::StockpileMaterialsData;

const MAX_RECIPE_DEPTH: u32 = 8;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RecipeTreeNode {
    pub item_id: String,
    pub display_name: String,
    pub needed_count: u64,
    pub output_count: u64,
    pub batch_count: u64,
    pub extra_output: u64,
    pub recipe_type: String,
    pub ingredients: Vec<RecipeTreeIngredient>,
    pub children: Vec<RecipeTreeNode>,
    pub unresolved: bool,
    pub unresolved_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RecipeTreeIngredient {
    pub item_id: String,
    pub display_name: String,
    pub needed_count: u64,
    pub unresolved: bool,
    pub unresolved_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct RecipeCandidate {
    recipe_id: String,
    recipe_type: String,
    output_item: String,
    output_count: u64,
    ingredients: Vec<RecipeInput>,
}

#[derive(Debug, Clone)]
struct RecipeInput {
    spec: InputSpec,
    count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum InputSpec {
    Item(String),
    Tag(String),
    Alternatives(Vec<String>),
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
            values.sort_by(compare_candidates);
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
            return unresolved_node(item_id, needed_count, "tag_input");
        }
        if stack.contains(item_id) {
            return unresolved_node(item_id, needed_count, "cycle");
        }
        if depth >= MAX_RECIPE_DEPTH {
            return unresolved_node(item_id, needed_count, "max_depth");
        }
        let Some(candidate) = self
            .candidates
            .get(item_id)
            .and_then(|values| values.first())
        else {
            return unresolved_node(item_id, needed_count, "no_recipe");
        };

        let output_count = candidate.output_count.max(1);
        let batch_count = ceil_div(needed_count, output_count);
        let extra_output = batch_count
            .saturating_mul(output_count)
            .saturating_sub(needed_count);
        stack.insert(item_id.to_string());
        let mut ingredients = Vec::new();
        let mut children = Vec::new();
        for input in &candidate.ingredients {
            let ingredient_needed = input.count.saturating_mul(batch_count);
            ingredients.push(ingredient_summary(input, ingredient_needed));
            children.push(match &input.spec {
                InputSpec::Item(child_item) => {
                    self.resolve_inner(child_item, ingredient_needed, depth + 1, stack)
                }
                InputSpec::Tag(tag) => unresolved_node(tag, ingredient_needed, "tag_input"),
                InputSpec::Alternatives(values) => {
                    unresolved_node(&values.join("|"), ingredient_needed, "alternative_input")
                }
            });
        }
        stack.remove(item_id);

        RecipeTreeNode {
            item_id: item_id.to_string(),
            display_name: display_name(item_id),
            needed_count,
            output_count,
            batch_count,
            extra_output,
            recipe_type: candidate.recipe_type.clone(),
            ingredients,
            children,
            unresolved: false,
            unresolved_reason: None,
        }
    }
}

fn parse_supported_recipe(recipe_id: &str, recipe: &Value) -> Option<RecipeCandidate> {
    let recipe_type = recipe.get("type")?.as_str()?.to_string();
    let (output_item, output_count) = parse_result(recipe)?;
    let ingredients = match recipe_type.as_str() {
        "minecraft:crafting_shaped" => parse_shaped_ingredients(recipe)?,
        "minecraft:crafting_shapeless" => parse_shapeless_ingredients(recipe)?,
        "minecraft:stonecutting" => vec![parse_input(recipe.get("ingredient")?)?],
        _ => return None,
    };
    Some(RecipeCandidate {
        recipe_id: recipe_id.to_string(),
        recipe_type,
        output_item,
        output_count,
        ingredients: aggregate_inputs(ingredients),
    })
}

fn parse_result(recipe: &Value) -> Option<(String, u64)> {
    let result = recipe.get("result")?;
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

fn parse_shaped_ingredients(recipe: &Value) -> Option<Vec<InputSpec>> {
    let pattern = recipe.get("pattern")?.as_array()?;
    let key = recipe.get("key")?.as_object()?;
    let mut inputs = Vec::new();
    for row in pattern {
        for ch in row.as_str()?.chars().filter(|ch| *ch != ' ') {
            inputs.push(parse_input(key.get(&ch.to_string())?)?);
        }
    }
    Some(inputs)
}

fn parse_shapeless_ingredients(recipe: &Value) -> Option<Vec<InputSpec>> {
    recipe
        .get("ingredients")?
        .as_array()?
        .iter()
        .map(parse_input)
        .collect()
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

fn aggregate_inputs(inputs: Vec<InputSpec>) -> Vec<RecipeInput> {
    let mut counts = BTreeMap::<InputSpec, u64>::new();
    for input in inputs {
        *counts.entry(input).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(spec, count)| RecipeInput { spec, count })
        .collect()
}

fn compare_candidates(left: &RecipeCandidate, right: &RecipeCandidate) -> Ordering {
    candidate_priority(left)
        .cmp(&candidate_priority(right))
        .then_with(|| left.recipe_id.cmp(&right.recipe_id))
}

fn candidate_priority(candidate: &RecipeCandidate) -> (u8, usize, std::cmp::Reverse<u64>) {
    let type_rank = match candidate.recipe_type.as_str() {
        "minecraft:crafting_shaped" | "minecraft:crafting_shapeless" => 0,
        "minecraft:stonecutting" => 1,
        _ => 2,
    };
    (
        type_rank,
        candidate
            .ingredients
            .iter()
            .map(|input| input.count as usize)
            .sum(),
        std::cmp::Reverse(candidate.output_count),
    )
}

fn ingredient_summary(input: &RecipeInput, needed_count: u64) -> RecipeTreeIngredient {
    match &input.spec {
        InputSpec::Item(item) => RecipeTreeIngredient {
            item_id: item.clone(),
            display_name: display_name(item),
            needed_count,
            unresolved: false,
            unresolved_reason: None,
        },
        InputSpec::Tag(tag) => RecipeTreeIngredient {
            item_id: tag.clone(),
            display_name: tag.clone(),
            needed_count,
            unresolved: true,
            unresolved_reason: Some("tag_input".to_string()),
        },
        InputSpec::Alternatives(items) => RecipeTreeIngredient {
            item_id: items.join("|"),
            display_name: items.join(" 或 "),
            needed_count,
            unresolved: true,
            unresolved_reason: Some("alternative_input".to_string()),
        },
    }
}

fn unresolved_node(item_id: &str, needed_count: u64, reason: &str) -> RecipeTreeNode {
    RecipeTreeNode {
        item_id: item_id.to_string(),
        display_name: display_name(item_id),
        needed_count,
        output_count: 0,
        batch_count: 0,
        extra_output: 0,
        recipe_type: "unresolved".to_string(),
        ingredients: Vec::new(),
        children: Vec::new(),
        unresolved: true,
        unresolved_reason: Some(reason.to_string()),
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
        assert_eq!(tree.recipe_type, "minecraft:crafting_shaped");
        assert_eq!(tree.output_count, 16);
        assert_eq!(tree.batch_count, 2);
        assert_eq!(tree.extra_output, 15);
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
    fn resolves_shapeless_recipe() {
        let resolver = RecipeTreeResolver::new(&recipe_map([(
            "minecraft:acacia_button",
            json!({
                "type": "minecraft:crafting_shapeless",
                "ingredients": ["minecraft:acacia_planks"],
                "result": { "id": "minecraft:acacia_button", "count": 1 }
            }),
        )]));
        let tree = resolver.resolve("minecraft:acacia_button", 3);
        assert_eq!(tree.recipe_type, "minecraft:crafting_shapeless");
        assert_eq!(tree.children[0].item_id, "minecraft:acacia_planks");
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
        assert_eq!(tree.recipe_type, "minecraft:stonecutting");
        assert_eq!(tree.children[0].item_id, "minecraft:stone");
    }

    #[test]
    fn prefers_crafting_over_stonecutting() {
        let resolver = RecipeTreeResolver::new(&recipe_map([
            (
                "minecraft:stone_bricks_from_stone_stonecutting",
                json!({
                    "type": "minecraft:stonecutting",
                    "ingredient": "minecraft:stone",
                    "result": { "id": "minecraft:stone_bricks", "count": 1 }
                }),
            ),
            (
                "minecraft:stone_bricks",
                json!({
                    "type": "minecraft:crafting_shaped",
                    "pattern": ["##", "##"],
                    "key": { "#": "minecraft:stone" },
                    "result": { "id": "minecraft:stone_bricks", "count": 4 }
                }),
            ),
        ]));
        let tree = resolver.resolve("minecraft:stone_bricks", 4);
        assert_eq!(tree.recipe_type, "minecraft:crafting_shaped");
        assert_eq!(tree.output_count, 4);
    }

    #[test]
    fn unresolved_when_no_recipe() {
        let resolver = RecipeTreeResolver::new(&BTreeMap::new());
        let tree = resolver.resolve("minecraft:diamond", 1);
        assert!(tree.unresolved);
        assert_eq!(tree.unresolved_reason.as_deref(), Some("no_recipe"));
    }

    #[test]
    fn tag_input_becomes_unresolved_tag_node() {
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
        assert_eq!(tree.children[0].item_id, "#minecraft:planks");
        assert!(tree.children[0].unresolved);
        assert_eq!(
            tree.children[0].unresolved_reason.as_deref(),
            Some("tag_input")
        );
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
        let recipes = (0..12)
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

    fn materials_data<const N: usize>(values: [(&str, u64, &str); N]) -> StockpileMaterialsData {
        StockpileMaterialsData {
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
