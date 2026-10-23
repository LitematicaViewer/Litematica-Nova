//! Read-only, one-row material lookup. The caller supplies an existing analysis
//! row and its resource identity; this path never opens a projection or task DB.
use std::collections::BTreeMap;
use anyhow::{ensure, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use crate::{recipe_tree, stats_api::MaterialItemOutput, stockpile};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MaterialDetailRequest {
    pub material: MaterialItemOutput,
    pub material_semantics: String,
    pub data_version: i32,
    pub selected_recipe: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialDetail {
    pub material: stockpile::StockpileMaterialItem,
    pub recipe_options: Vec<recipe_tree::MaterialRecipeOption>,
    pub recipe_tree: Option<recipe_tree::RecipeTreeNode>,
    pub icon_images: BTreeMap<String, String>,
    pub item_names: BTreeMap<String, BTreeMap<String, String>>,
}

pub fn from_request(path: &std::path::Path) -> Result<MaterialDetail> {
    let request: MaterialDetailRequest = serde_json::from_reader(std::fs::File::open(path)?)?;
    read(request)
}

pub fn read(request: MaterialDetailRequest) -> Result<MaterialDetail> {
    let context = crate::game_data::active_context()?;
    ensure!(request.material_semantics == format!("minecraft-inventory-v9:{}", context.content_id), "Material snapshot uses a different game-resource identity; reanalyse the projection");
    ensure!(request.material.material_status == "item" && request.material.stack_size > 0, "A verified inventory material row is required");
    let version = crate::block_database::database()?.version_for_data_version(request.data_version)
        .ok_or_else(|| anyhow::anyhow!("Material snapshot has no supported Minecraft version identity"))?.to_owned();
    let item = stockpile::material_detail_item(&request.material, &version)?;
    let (options, tree) = recipe_tree::resolve_material_detail(&version, &item.item_icon_key, item.required_count, request.selected_recipe.as_deref())?;
    let mut trees = BTreeMap::new();
    if let Some(tree) = tree.as_ref() { trees.insert(item.namespace_id.clone(), tree.clone()); }
    let stacks = item.required_count.div_ceil(item.stack_size);
    let mut materials = stockpile::StockpileMaterialsData {
        schema_version: crate::stockpile_schema::STOCKPILE_MATERIALS_SCHEMA_VERSION,
        ui_locale: None, ui_translations: BTreeMap::new(),
        project: stockpile::StockpileProjectInfo { source_file: String::new(), created_at: 0, data_version: request.data_version, regions: item.source_regions.clone() },
        summary: stockpile::StockpileSummary { total_blocks: item.required_count, unique_materials: 1, total_stacks: stacks, estimated_shulker_boxes: stacks.div_ceil(27) },
        materials: vec![item], diagnostics: Vec::new(),
    };
    let names = crate::item_names::resolve_stockpile_item_names_cached(&version, &mut materials, &trees)?;
    let icons = crate::item_icons::resolve_stockpile_icons_cached(&version, &mut materials, &trees)?;
    let files = icons.files.into_iter().map(|file| (file.path, format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(file.bytes)))).collect::<BTreeMap<_, _>>();
    let icon_images = icons.payload.by_key.into_iter().filter_map(|(key, icon)| files.get(&icon.path).cloned().map(|url| (key, url))).collect();
    Ok(MaterialDetail { material: materials.materials.remove(0), recipe_options: options, recipe_tree: tree, icon_images, item_names: names.names })
}
