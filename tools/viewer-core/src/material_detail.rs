//! Read-only, one-row material lookup. The caller supplies an existing analysis
//! row and its resource identity; this path never opens a projection or task DB.
use std::collections::BTreeMap;
use std::sync::Arc;
use anyhow::{ensure, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use crate::{recipe_tree, stats_api::MaterialItemOutput, stockpile};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MaterialDetailRequest {
    pub material: MaterialItemOutput,
    pub material_semantics: String,
    pub data_version: i32,
    pub selected_recipe: Option<String>,
    #[serde(default)] pub ui_locale: String,
    #[serde(default)] pub language_map: BTreeMap<String,String>,
    #[serde(default)] pub source_query: crate::data_view::SourceQuery,
    #[serde(default)] pub source_min_y: i32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialDetail {
    pub material: stockpile::StockpileMaterialItem,
    pub recipe_options: Vec<recipe_tree::MaterialRecipeOption>,
    pub recipe_tree: Option<recipe_tree::RecipeTreeNode>,
    pub icon_images: BTreeMap<String, String>,
    pub item_names: BTreeMap<String, BTreeMap<String, String>>,
    pub recipe_status: String,
    pub source_summary: crate::data_view::SourceSummary,
    pub source_groups: Vec<crate::data_view::SourceGroup>,
}

pub fn from_request(path: &std::path::Path) -> Result<MaterialDetail> {
    let request: MaterialDetailRequest = serde_json::from_reader(std::fs::File::open(path)?)?;
    read_cached(request)
}

pub fn read_cached(request: MaterialDetailRequest) -> Result<MaterialDetail> {
    use std::sync::{Mutex,OnceLock};
    use sha2::{Digest,Sha256};
    static DETAILS:OnceLock<Mutex<BTreeMap<String,MaterialDetail>>>=OnceLock::new();
    let identity=crate::game_data::active_context()?.content_id;
    let key=format!("{identity}:{:x}",Sha256::digest(serde_json::to_vec(&request)?));
    let cache=DETAILS.get_or_init(Default::default);
    if let Some(detail)=cache.lock().map_err(|_|anyhow::anyhow!("material detail cache lock poisoned"))?.get(&key).cloned(){return Ok(detail);}
    let detail=read(request)?;
    let mut cache=cache.lock().map_err(|_|anyhow::anyhow!("material detail cache lock poisoned"))?;
    if cache.len()>=32{cache.clear();}cache.insert(key,detail.clone());Ok(detail)
}

pub fn read(request: MaterialDetailRequest) -> Result<MaterialDetail> {
    let context = crate::game_data::active_context()?;
    ensure!(request.material_semantics == format!("minecraft-inventory-v9:{}", context.content_id), "Material snapshot uses a different game-resource identity; reanalyse the projection");
    ensure!(request.material.material_status == "item" && request.material.stack_size > 0, "A verified inventory material row is required");
    let version = crate::block_database::database()?.version_for_data_version(request.data_version)
        .ok_or_else(|| anyhow::anyhow!("Material snapshot has no supported Minecraft version identity"))?.to_owned();
    let item = stockpile::material_detail_item(&request.material, &version)?;
    let (mut options, mut tree) = recipe_tree::resolve_material_detail(&version, &item.item_icon_key, item.required_count, request.selected_recipe.as_deref())?;
    let mut trees = BTreeMap::new();
    if let Some(tree) = tree.as_ref() { trees.insert(item.namespace_id.clone(), tree.clone()); }
    let stacks = item.required_count.div_ceil(item.stack_size);
    let mut materials = stockpile::StockpileMaterialsData {
        schema_version: crate::stockpile_schema::STOCKPILE_MATERIALS_SCHEMA_VERSION,
        ui_locale: Some(request.ui_locale.clone()), ui_translations: request.language_map.clone(),
        project: stockpile::StockpileProjectInfo { source_file: String::new(), created_at: 0, data_version: request.data_version, regions: item.source_regions.clone() },
        summary: stockpile::StockpileSummary { total_blocks: item.required_count, unique_materials: 1, total_stacks: stacks, estimated_shulker_boxes: stacks.div_ceil(27) },
        materials: vec![item], diagnostics: Vec::new(),
    };
    let names = crate::item_names::resolve_stockpile_item_names_cached(&version, &mut materials, &trees)?;
    let icons = crate::item_icons::resolve_stockpile_icons_cached(&version, &mut materials, &trees)?;
    let files = icons.files.into_iter().map(|file| (file.path, format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(file.bytes)))).collect::<BTreeMap<_, _>>();
    let icon_images = icons.payload.by_key.into_iter().filter_map(|(key, icon)| files.get(&icon.path).cloned().map(|url| (key, url))).collect();
    let locale=if request.ui_locale.is_empty(){"zh-CN"}else{request.ui_locale.as_str()};
    let local_names=names.names.iter().filter_map(|(key,values)|values.get(locale).cloned().map(|value|(key.clone(),value))).collect::<BTreeMap<_,_>>();
    for option in &mut options {option.display_name=local_names.get(&request.material.block_id).cloned().unwrap_or_else(||crate::data_view::friendly_fallback(&item_display_name(&materials),&request.material.block_id));option.recipe_type_name=recipe_tree::recipe_type_name(&option.recipe_type,locale);}
    if let Some(tree)=tree.as_mut(){recipe_tree::localize_tree(tree,locale,&local_names)?;}
    let (source_summary,source_groups)=crate::data_view::sources(&request.material,request.source_min_y,&request.source_query,locale);
    let recipe_status=if options.is_empty(){"no_available_recipe"}else{"ready"}.into();
    Ok(MaterialDetail { material: materials.materials.remove(0), recipe_options: options, recipe_tree: tree, icon_images, item_names: names.names,recipe_status,source_summary,source_groups })
}

fn item_display_name(materials:&stockpile::StockpileMaterialsData)->String {materials.materials.first().map_or_else(String::new,|item|item.display_name.clone())}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct MaterialRecipesSummary {
    pub total_materials:usize, pub excluded_materials:usize, pub total_required_count:u64,
    pub materials_with_recipes:usize, pub materials_without_recipes:usize,
    pub unresolved_materials:usize, pub recipe_option_count:usize, pub recipe_tree_count:usize,
    pub recipe_tree_nodes:usize, pub unique_resource_count:usize,
    pub cache_hit:bool, pub elapsed_ms:f64, pub recipe_ms:f64, pub names_ms:f64, pub resources_ms:f64,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct MaterialRecipesBatch {
    pub source_revision:String,
    pub material_semantics:String,
    #[serde(serialize_with="serialize_shared")] pub materials:Arc<stockpile::StockpileMaterialsData>,
    #[serde(serialize_with="serialize_shared")] pub entries:Arc<Vec<recipe_tree::MaterialRecipeBatchEntry>>,
    #[serde(serialize_with="serialize_shared")] pub recipe_trees:Arc<BTreeMap<String,recipe_tree::RecipeTreeNode>>,
    #[serde(serialize_with="serialize_shared")] pub item_names:Arc<BTreeMap<String,BTreeMap<String,String>>>,
    #[serde(serialize_with="serialize_shared")] pub icon_images:Arc<BTreeMap<String,String>>,
    pub summary:MaterialRecipesSummary,
}
fn serialize_shared<S:serde::Serializer,T:Serialize>(value:&Arc<T>,serializer:S)->std::result::Result<S::Ok,S::Error>{value.as_ref().serialize(serializer)}

/// Computes every real inventory material in one Rust call. All quantities,
/// NBT identities and candidate recipes come from the existing analysis and
/// resolver; localization and resource lookup are performed once per batch.
/// The one-second development target is deliberately not a runtime deadline.
pub fn read_all_cached(analysis:&crate::stats_api::StableStructureOutput,source_revision:&str,ui_locale:&str,
    language_map:&BTreeMap<String,String>,cancel:&dyn Fn()->bool)->Result<Arc<MaterialRecipesBatch>>
{
    use std::sync::{Mutex,OnceLock};
    use sha2::{Digest,Sha256};
    use std::time::Instant;
    static BATCHES:OnceLock<Mutex<BTreeMap<String,Arc<MaterialRecipesBatch>>>>=OnceLock::new();
    let started=Instant::now();ensure!(!cancel(),"Cancelled");
    let context=crate::game_data::active_context()?;
    ensure!(analysis.material_semantics==format!("minecraft-inventory-v9:{}",context.content_id),"Material snapshot uses a different game-resource identity; reanalyse the projection");
    let locale=if ui_locale.is_empty(){"zh-CN"}else{ui_locale};
    let mut hash=Sha256::new();
    hash.update(serde_json::to_vec(&(source_revision,&analysis.material_semantics,analysis.metadata.minecraft_data_version,locale,language_map))?);
    for item in &analysis.material_items {
        hash.update(serde_json::to_vec(&(&item.block_id,&item.item_identity,&item.display_name,item.count,item.block_count,item.container_item_count,item.total_count,item.stack_size,item.can_fit_container,item.has_custom_stack_size,&item.material_status,&item.region_counts))?);
    }
    let key=format!("{:x}",hash.finalize());let cache=BATCHES.get_or_init(Default::default);
    if let Some(mut batch)=cache.lock().map_err(|_|anyhow::anyhow!("material recipe batch cache lock poisoned"))?.get(&key).cloned(){
        ensure!(!cancel(),"Cancelled");
        // Only the small response shell/summary is copied; all complete trees,
        // material rows, names and image bytes stay shared with the cache.
        let summary=&mut Arc::make_mut(&mut batch).summary;
        summary.cache_hit=true;summary.recipe_ms=0.0;summary.names_ms=0.0;summary.resources_ms=0.0;
        summary.elapsed_ms=started.elapsed().as_secs_f64()*1000.0;return Ok(batch);
    }
    let version=crate::block_database::database()?.version_for_data_version(analysis.metadata.minecraft_data_version)
        .ok_or_else(||anyhow::anyhow!("Material snapshot has no supported Minecraft version identity"))?.to_owned();
    let phase=Instant::now();
    let mapped=stockpile::material_detail_items(&analysis.material_items,&version)?;
    let total_required_count=mapped.iter().try_fold(0_u64,|total,item|total.checked_add(item.required_count).ok_or_else(||anyhow::anyhow!("Material requirement total overflow")))?;
    let stacks=mapped.iter().try_fold(0_u64,|total,item|total.checked_add(item.required_count.div_ceil(item.stack_size.max(1))).ok_or_else(||anyhow::anyhow!("Material stack total overflow")))?;
    let mut materials=stockpile::StockpileMaterialsData{schema_version:crate::stockpile_schema::STOCKPILE_MATERIALS_SCHEMA_VERSION,
        ui_locale:Some(locale.into()),ui_translations:language_map.clone(),
        project:stockpile::StockpileProjectInfo{source_file:String::new(),created_at:0,data_version:analysis.metadata.minecraft_data_version,regions:analysis.regions.iter().map(|region|region.name.clone()).collect()},
        summary:stockpile::StockpileSummary{total_blocks:total_required_count,unique_materials:mapped.len(),total_stacks:stacks,estimated_shulker_boxes:stacks.div_ceil(27)},materials:mapped,diagnostics:Vec::new()};
    let (mut entries,mut trees)=recipe_tree::resolve_material_batch(&version,&materials,cancel)?;
    let recipe_ms=phase.elapsed().as_secs_f64()*1000.0;
    ensure!(!cancel(),"Cancelled");let phase=Instant::now();
    let names=crate::item_names::resolve_stockpile_item_names_cached(&version,&mut materials,&trees)?;
    let mut local_names=names.names.iter().filter_map(|(key,values)|values.get(locale).cloned().map(|value|(key.clone(),value))).collect::<BTreeMap<_,_>>();
    recipe_tree::localize_trees(&mut trees,locale,&mut local_names)?;
    for entry in &mut entries{for option in &mut entry.recipe_options{
        option.display_name=local_names.get(&entry.item_id).cloned().unwrap_or_else(||crate::data_view::friendly_fallback("",&entry.item_id));
        option.recipe_type_name=recipe_tree::recipe_type_name(&option.recipe_type,locale);
    }}
    let names_ms=phase.elapsed().as_secs_f64()*1000.0;
    ensure!(!cancel(),"Cancelled");let phase=Instant::now();
    let icons=crate::item_icons::resolve_stockpile_icons_cached(&version,&mut materials,&trees)?;
    let unique_resource_count=icons.payload.by_key.len();
    let files=icons.files.into_iter().map(|file|(file.path,format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(file.bytes)))).collect::<BTreeMap<_,_>>();
    let icon_images=icons.payload.by_key.into_iter().filter_map(|(key,icon)|files.get(&icon.path).cloned().map(|value|(key,value))).collect::<BTreeMap<_,_>>();
    let resources_ms=phase.elapsed().as_secs_f64()*1000.0;
    fn inspect(node:&recipe_tree::RecipeTreeNode)->(usize,bool){node.children.iter().fold((1,node.unresolved),|(count,unresolved),child|{let(next,child_unresolved)=inspect(child);(count+next,unresolved||child_unresolved)})}
    let recipe_tree_nodes=trees.values().map(|tree|inspect(tree).0).sum();
    let unresolved_materials=entries.iter().filter(|entry|entry.recipe_tree_key.as_ref().and_then(|key|trees.get(key)).is_some_and(|tree|inspect(tree).1)).count();
    let materials_with_recipes=entries.iter().filter(|entry|!entry.recipe_options.is_empty()).count();
    let recipe_option_count=entries.iter().map(|entry|entry.recipe_options.len()).sum();
    let summary=MaterialRecipesSummary{total_materials:entries.len(),excluded_materials:analysis.material_items.len()-entries.len(),total_required_count,
        materials_with_recipes,materials_without_recipes:entries.len()-materials_with_recipes,unresolved_materials,recipe_option_count,recipe_tree_count:trees.len(),recipe_tree_nodes,unique_resource_count,
        cache_hit:false,elapsed_ms:started.elapsed().as_secs_f64()*1000.0,recipe_ms,names_ms,resources_ms};
    let mut batch=MaterialRecipesBatch{source_revision:source_revision.into(),material_semantics:analysis.material_semantics.clone(),materials:Arc::new(materials),entries:Arc::new(entries),recipe_trees:Arc::new(trees),item_names:Arc::new(names.names),icon_images:Arc::new(icon_images),summary};
    ensure!(!cancel(),"Cancelled");
    batch.summary.elapsed_ms=started.elapsed().as_secs_f64()*1000.0;
    let batch=Arc::new(batch);
    // Limits govern retention only, never whether a real material is computed.
    if batch.summary.recipe_tree_nodes<=100_000&&batch.icon_images.values().map(String::len).sum::<usize>()<=32*1024*1024{
        let mut cache=cache.lock().map_err(|_|anyhow::anyhow!("material recipe batch cache lock poisoned"))?;
        if cache.len()>=2{cache.clear();}cache.insert(key,batch.clone());
    }
    Ok(batch)
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_recipes_covers_every_identity_and_quantity_and_invalidates_only_changed_inputs() {
        let path=std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/stats_water_fixture.litematic"));
        let mut analysis=crate::stats_api::build_stats_output(path,false).unwrap();
        let first=crate::stats_api::inventory_requirement("minecraft:white_stained_glass".into(),8).unwrap();
        let mut variant=crate::stats_api::inventory_requirement("minecraft:white_stained_glass".into(),16).unwrap();
        variant.item_identity=Some("minecraft:white_stained_glass#nbt-batch-fixture".into());
        analysis.material_items=vec![first,variant];
        let language=BTreeMap::new();
        assert!(read_all_cached(&analysis,"batch-unit-test","zh-CN",&language,&||true).is_err());
        let cold=read_all_cached(&analysis,"batch-unit-test","zh-CN",&language,&||false).unwrap();
        assert!(!cold.summary.cache_hit);assert_eq!(cold.summary.total_materials,2);assert_eq!(cold.summary.total_required_count,24);
        assert_eq!(cold.entries.iter().map(|entry|entry.required_count).collect::<Vec<_>>(),vec![8,16]);
        assert_ne!(cold.entries[0].material_id,cold.entries[1].material_id);
        for entry in cold.entries.iter() {
            assert!(!entry.recipe_options.is_empty());
            let tree=&cold.recipe_trees[entry.recipe_tree_key.as_ref().unwrap()];
            assert_eq!(tree.needed_count,entry.required_count);assert_eq!(tree.item_id,entry.item_id);
            assert!(!tree.recipe_type_name.is_empty());
        }
        assert!(!cold.item_names.is_empty());assert!(!cold.icon_images.is_empty());
        let hot=read_all_cached(&analysis,"batch-unit-test","zh-CN",&language,&||false).unwrap();
        assert!(hot.summary.cache_hit);assert_eq!(serde_json::to_value(cold.entries.as_ref()).unwrap(),serde_json::to_value(hot.entries.as_ref()).unwrap());
        assert_eq!(cold.recipe_trees,hot.recipe_trees);
        assert!(Arc::ptr_eq(&cold.recipe_trees,&hot.recipe_trees));
        assert!(Arc::ptr_eq(&cold.icon_images,&hot.icon_images));
        assert!(Arc::ptr_eq(&cold.entries,&hot.entries));
        analysis.material_items[0].total_count=9;analysis.material_items[0].count=9;analysis.material_items[0].block_count=9;
        let changed=read_all_cached(&analysis,"batch-unit-test","zh-CN",&language,&||false).unwrap();
        assert!(!changed.summary.cache_hit);assert_eq!(changed.summary.total_required_count,25);
        let cancelled=read_all_cached(&analysis,"batch-unit-test","zh-CN",&language,&||true);
        assert!(cancelled.is_err());
    }
}
