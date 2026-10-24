//! Rust-owned paginated display models. UI only supplies input and renders rows.
use std::collections::BTreeMap;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use crate::stats_api::{MaterialItemOutput, MaterialScopeCount, StableStructureOutput};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogQuery {
    pub search: String, pub category: String, pub sort: String, pub filter: String,
    pub individual_counts: bool, pub offset: usize, pub limit: Option<usize>,
    pub ui_locale: String, pub language: String,
    pub language_map: BTreeMap<String,String>, pub source_revision: String,
    pub source_query: SourceQuery,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SourceQuery { pub region: Option<String>, pub offset: usize, pub limit: Option<usize> }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary { pub region_count: usize, pub min_y: Option<i32>, pub max_y: Option<i32>, pub total_count: u64, pub unlocated_count: u64 }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLayer { pub layer: i32, pub world_y: i32, pub count: u64, pub block_count: u64, pub container_item_count: u64 }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGroup {
    pub region: String, pub display_name: String, pub min_y: Option<i32>, pub max_y: Option<i32>,
    pub total_count: u64, pub layer_count: usize, pub unlocated_count: u64,
    pub layers: Vec<SourceLayer>, pub offset: usize, pub limit: usize,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialItemView {
    pub id: String, pub item_id: String, pub name: String, pub source_display_name: String,
    pub count: u64, pub block_count: u64, pub container_item_count: u64, pub total_count: u64,
    pub icon_hint: String, pub stack_size: u64, pub can_fit_container: bool,
    pub material_status: String, pub has_custom_stack_size: bool,
    pub quantity_text: String, pub source_summary: SourceSummary, pub source_groups: Vec<SourceGroup>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    pub rows: Vec<MaterialItemView>, pub total_rows: usize, pub total_materials: usize,
    pub offset: usize, pub limit: usize, pub source_revision: String, pub include_container_items: bool,
}

pub fn locale<'a>(language: &'a str, ui_locale: &'a str) -> &'a str {
    if ["zh-CN","zh_cn","en-US","en_us"].contains(&language) { language }
    else if ui_locale.is_empty() { "zh-CN" } else { ui_locale }
}
pub fn quantity_text(count: u64, stack_size: u64, individual: bool, locale: &str) -> String {
    if individual { return count.to_string(); }
    let stack = stack_size.max(1); let sizes = [stack.saturating_mul(729), stack.saturating_mul(27), stack, 1];
    let labels = if locale.starts_with("zh") { ["箱", "盒", "组", "个"] } else { ["chests", "boxes", "stacks", "items"] };
    let mut remainder = count; let mut parts = Vec::new();
    for (size,label) in sizes.into_iter().zip(labels) { let n = remainder / size; remainder %= size; if n > 0 { parts.push(format!("{n} {label}")); } }
    if parts.is_empty() { format!("0 {}", labels[3]) } else { parts.join(" ") }
}

pub fn material_quantity_text(item:&MaterialItemOutput,individual:bool,locale:&str)->String {
    if individual||item.can_fit_container&&!item.has_custom_stack_size {return quantity_text(item.total_count,item.stack_size,individual,locale);}
    let stack=item.stack_size.max(1);let full=item.total_count/stack;let rest=item.total_count%stack;
    let (stacks,each)=if locale.starts_with("zh"){("组","个")}else{("stacks","items")};
    if full==0 {format!("{rest} {each}")}else if rest==0 {format!("{full} {stacks}")}else{format!("{full} {stacks} {rest} {each}")}
}

/// Only joint scope counts may assign a layer to a region. Marginals never
/// cross-join: old snapshots without joint data remain explicitly unlocated.
pub fn sources(item: &MaterialItemOutput, min_y: i32, query: &SourceQuery, locale: &str) -> (SourceSummary,Vec<SourceGroup>) {
    let mut groups = BTreeMap::<String,BTreeMap<Option<i32>,(u64,u64)>>::new();
    for MaterialScopeCount {region,layer,block_count,container_item_count,..} in item.scope_counts.as_deref().unwrap_or_default() {
        let count=groups.entry(region.clone()).or_default().entry(*layer).or_default();
        count.0=count.0.saturating_add(*block_count); count.1=count.1.saturating_add(*container_item_count);
    }
    let accounted=groups.values().flat_map(|layers|layers.values()).fold(0_u64,|n,(b,c)|n.saturating_add(*b).saturating_add(*c));
    if accounted < item.total_count { groups.entry(String::new()).or_default().entry(None).or_default().0 += item.total_count-accounted; }
    let limit=query.limit.unwrap_or(100).clamp(1,500);
    let mut summary=SourceSummary{region_count:groups.len(),min_y:None,max_y:None,total_count:item.total_count,unlocated_count:0};
    let groups=groups.into_iter().map(|(region,counts)| {
        let mut layers=Vec::new(); let mut total=0_u64; let mut unlocated=0_u64;
        for (layer,(blocks,containers)) in counts { let count=blocks.saturating_add(containers); total=total.saturating_add(count);
            if count==0 {continue;}
            if let Some(layer)=layer {layers.push(SourceLayer{layer,world_y:min_y.saturating_add(layer),count,block_count:blocks,container_item_count:containers});}
            else {unlocated=unlocated.saturating_add(count);}
        }
        let first=layers.first().map(|x|x.world_y); let last=layers.last().map(|x|x.world_y);
        if let Some(y)=first {summary.min_y=Some(summary.min_y.map_or(y,|old|old.min(y)));}
        if let Some(y)=last {summary.max_y=Some(summary.max_y.map_or(y,|old|old.max(y)));}
        summary.unlocated_count=summary.unlocated_count.saturating_add(unlocated);
        let layer_count=layers.len();
        let expanded=query.region.as_deref()==Some(region.as_str());
        let page=if expanded {layers.into_iter().skip(query.offset).take(limit).collect()}else{Vec::new()};
        SourceGroup{display_name:if region.is_empty(){if locale.starts_with("zh"){"未定位来源".into()}else{"Unlocated source".into()}}else{region.clone()},region,
            min_y:first,max_y:last,total_count:total,layer_count,unlocated_count:unlocated,layers:page,offset:query.offset,limit}
    }).collect();
    (summary,groups)
}

pub fn catalog_query(stats: &StableStructureOutput, query: &CatalogQuery) -> Result<CatalogPage> {
    ensure!(query.search.len()<=4096,"Material search is too long");
    let locale=locale(&query.language,&query.ui_locale);
    let names=crate::item_names::display_names_for_keys(stats.material_items.iter().map(|x|x.block_id.as_str()),locale,&query.language_map)?;
    let needle=query.search.trim().to_lowercase();
    let min_y=stats.layers_summary.first().map_or(0,|x|x.world_y-x.layer);
    let mut rows=stats.material_items.iter().filter(|x|x.material_status=="item").filter_map(|item| {
        let name=names.get(&item.block_id).cloned().unwrap_or_else(||friendly_fallback(&item.display_name,&item.block_id));
        if (query.category=="placed" && item.block_count==0)||(query.category=="containers" && item.container_item_count==0)||(query.category=="variants" && item.item_identity.is_none()) {return None;}
        if !needle.is_empty() && !format!("{name} {} {}",item.block_id,item.item_identity.as_deref().unwrap_or("")).to_lowercase().contains(&needle) {return None;}
        Some((item,name))
    }).collect::<Vec<_>>();
    rows.sort_by(|(a,an),(b,bn)| {let name=||an.cmp(bn).then_with(||a.item_identity.as_ref().unwrap_or(&a.block_id).cmp(b.item_identity.as_ref().unwrap_or(&b.block_id)));
        match query.sort.as_str(){"count_asc"=>a.total_count.cmp(&b.total_count).then_with(name),"count_desc"=>b.total_count.cmp(&a.total_count).then_with(name),_=>name()}});
    let total_rows=rows.len(); let limit=query.limit.unwrap_or(200).clamp(1,500);
    let rows=rows.into_iter().skip(query.offset).take(limit).map(|(item,name)| {
        let (source_summary,source_groups)=sources(item,min_y,&query.source_query,locale);
        MaterialItemView{id:item.item_identity.clone().unwrap_or_else(||item.block_id.clone()),item_id:item.block_id.clone(),name,source_display_name:item.display_name.clone(),count:item.total_count,
            block_count:item.block_count,container_item_count:item.container_item_count,total_count:item.total_count,icon_hint:item.icon_hint.clone(),stack_size:item.stack_size,
            can_fit_container:item.can_fit_container,material_status:item.material_status.clone(),has_custom_stack_size:item.has_custom_stack_size,
            quantity_text:material_quantity_text(item,query.individual_counts,locale),source_summary,source_groups}
    }).collect();
    Ok(CatalogPage{rows,total_rows,total_materials:stats.material_items.iter().filter(|x|x.material_status=="item").count(),offset:query.offset,limit,source_revision:query.source_revision.clone(),include_container_items:stats.container_scan.enabled})
}

pub fn friendly_fallback(name: &str, id: &str) -> String {
    if !name.is_empty() && name!=id && !name.starts_with("minecraft:") {name.to_owned()}
    else {id.trim_start_matches('#').split(':').next_back().unwrap_or(id).split('#').next().unwrap_or(id).replace(['_','/']," ")}
}

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct StatisticsView {
    pub total_non_air_blocks:u64, pub region_count:usize, pub enclosing_size:crate::model::EnclosingSize,
    pub density:f64,pub building_type:String,pub redstone_ratio:Option<f64>,pub fluid_ratio:Option<f64>,
    pub entity_count:usize,pub material_count:usize,pub regions:Vec<crate::stats_api::RegionSummaryOutput>,pub layers:Vec<crate::stats_api::LayerSummaryOutput>,pub container_scan:crate::stats_api::ContainerScanOutput,
}
pub fn statistics_view(stats:&StableStructureOutput)->StatisticsView {statistics_view_localized(stats,"zh-CN")}
pub fn statistics_view_localized(stats:&StableStructureOutput,locale:&str)->StatisticsView {
    let building=stats.derived.as_ref().map(|x|&x.building);
    StatisticsView{total_non_air_blocks:stats.structure_stats.total_non_air_blocks,region_count:stats.regions.len(),enclosing_size:stats.metadata.enclosing_size.clone(),
        density:building.map_or_else(||if stats.metadata.total_volume>0{stats.structure_stats.total_non_air_blocks as f64/stats.metadata.total_volume as f64}else{0.0},|b|b.density),
        building_type:building_type_label(building.map_or("unknown",|b|b.building_type.as_str()),locale),redstone_ratio:building.map(|b|b.redstone_ratio),fluid_ratio:building.map(|b|b.fluid_ratio),
        entity_count:stats.entity_summary.total_entities,material_count:stats.material_items.len(),regions:stats.regions.clone(),layers:stats.layers_summary.clone(),container_scan:crate::stats_api::ContainerScanOutput{
            enabled:stats.container_scan.enabled,containers_scanned:stats.container_scan.containers_scanned,
            item_stacks_scanned:stats.container_scan.item_stacks_scanned,warnings:stats.container_scan.warnings.clone(),scope_entries:None}}
}

fn building_type_label(kind:&str,locale:&str)->String {
    let labels=if locale.starts_with("zh") {["建筑","小型结构","红石机器","红石设施","机械建筑","建筑机关","像素画","混合结构","未知"]}
        else {["Building","Small structure","Redstone machine","Redstone facility","Mechanical building","Structural mechanism","Pixel art","Mixed structure","Unknown"]};
    labels[match kind{"building"=>0,"too_small"=>1,"redstone_machine"|"redstone"=>2,"redstone_power"=>3,"mechanical_build"=>4,"structural_mechanism"=>5,"pixel_art"=>6,"mixed"=>7,_=>8}].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn quantity_units_preserve_exact_counts() {assert_eq!(quantity_text(64*729+64*27+65,64,false,"zh-CN"),"1 箱 1 盒 1 组 1 个");assert_eq!(quantity_text(0,64,false,"en-US"),"0 items");}
    #[test] fn source_groups_never_cross_join_marginals() {
        let mut item=crate::stats_api::inventory_requirement("minecraft:stone".into(),11).unwrap();
        item.scope_counts=Some(vec![MaterialScopeCount{region:"A".into(),layer:Some(1),block_count:3,container_item_count:0,has_custom_stack_size:false},MaterialScopeCount{region:"B".into(),layer:Some(5),block_count:7,container_item_count:0,has_custom_stack_size:false}]);
        let (summary,groups)=sources(&item,-64,&SourceQuery{region:Some("A".into()),offset:0,limit:Some(1)},"zh-CN");
        assert_eq!((summary.total_count,summary.unlocated_count),(11,1));assert_eq!(groups.iter().find(|x|x.region=="A").unwrap().layers[0].world_y,-63);
        assert!(groups.iter().find(|x|x.region=="B").unwrap().layers.is_empty());
    }
}
