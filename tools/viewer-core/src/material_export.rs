//! Material-list export uses the existing inventory, Stockpile and recipe
//! contracts. The Application supplies a source-validated scoped snapshot.
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use crate::{recipe_cache, recipe_tree, stats_api, stockpile, stockpile_zip};
use crate::stats_api::StableStructureOutput;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialExportFormat { Svg, Txt, Html, Csv }

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialExportRequest {
    pub source_path: String,
    pub stats: StableStructureOutput,
    pub multiplier: u64,
    pub minecraft_version: String,
    pub terminal_raw_materials: bool,
    pub format: MaterialExportFormat,
}

#[derive(Debug, Serialize)]
pub struct MaterialExportSummary {
    pub output: String,
    pub format: MaterialExportFormat,
    pub material_count: usize,
    pub multiplier: u64,
    pub terminal_raw_materials: bool,
    pub include_container_items: bool,
}

pub fn run_cli(args: &[String]) -> Result<()> {
    ensure!(args.len() == 3 && args[1] == "--output", "usage: litematica_core materials-export REQUEST_JSON --output FILE");
    let request: MaterialExportRequest = serde_json::from_reader(std::fs::File::open(&args[0])?)?;
    let summary = export(request, Path::new(&args[2]))?;
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

pub fn export(mut request: MaterialExportRequest, output: &Path) -> Result<MaterialExportSummary> {
    ensure!(request.stats.material_semantics == "minecraft-26.3-inventory-v2", "材料导出需要 Minecraft 26.3 inventory-v2 统计快照");
    ensure!((1..=1_000_000).contains(&request.multiplier), "建造份数必须在 1 至 1000000 之间");
    ensure!(!request.minecraft_version.is_empty(), "材料导出必须指定 Minecraft 版本");
    let source = Path::new(&request.source_path);
    ensure!(output.is_absolute() && output != source, "材料列表必须另存为独立文件");
    let parent = output.parent().context("材料列表输出缺少父目录")?;
    ensure!(parent.is_dir(), "材料列表输出目录不存在");
    if output.exists() {
        ensure!(std::fs::canonicalize(output)? != std::fs::canonicalize(source)?, "材料列表输出不能覆盖原投影");
    }
    request.stats.material_items.retain(|row| row.material_status == "item" && row.total_count > 0);
    for row in &mut request.stats.material_items {
        for count in [&mut row.count, &mut row.block_count, &mut row.container_item_count, &mut row.total_count] {
            *count = count.checked_mul(request.multiplier).context("材料数量超出 u64 范围")?;
        }
        for entry in row.region_counts.iter_mut().flatten() { entry.count = entry.count.checked_mul(request.multiplier).context("区域材料数量溢出")?; }
        for entry in row.layer_counts.iter_mut().flatten() { entry.count = entry.count.checked_mul(request.multiplier).context("层材料数量溢出")?; }
    }
    let scope = &request.stats.structure_stats.scope;
    let scope_text = match scope.kind.as_str() {
        "region" => format!("区域：{}", scope.region.as_deref().unwrap_or_default()),
        "layer" => format!("层：{}", scope.layer.unwrap_or_default()),
        _ => "整个投影".to_owned(),
    };
    let included = request.stats.container_scan.enabled;
    let mut materials = stockpile::build_materials_data_from_stats(source, request.stats.clone(), &request.minecraft_version)?;
    if request.terminal_raw_materials {
        ensure!(recipe_cache::load_available_recipes(&request.minecraft_version)?.is_some(), "末端原料统计需要该 Minecraft 版本的完整本地配方缓存；请先准备配方缓存");
        let trees = recipe_tree::resolve_recipe_trees_for_materials(&request.minecraft_version, &materials)?;
        let mut counts = BTreeMap::<String, u64>::new();
        for material in &materials.materials {
            if let Some(tree) = trees.get(&material.namespace_id) { add_terminal(tree, &mut counts)?; }
            else { add_count(&mut counts, &material.namespace_id, material.required_count)?; }
        }
        request.stats.material_items = counts.into_iter().map(|(id, count)| {
            stats_api::inventory_requirement(id.clone(), count).with_context(|| format!("末端配方项 {id} 尚未解析成确定的真实物品"))
        }).collect::<Result<Vec<_>>>()?;
        materials = stockpile::build_materials_data_from_stats(source, request.stats, &request.minecraft_version)?;
    }
    let context = format!("{} · {} · {} 份 · 容器物品：{} · 末端原料：{}", source.file_name().unwrap_or_default().to_string_lossy(), scope_text, request.multiplier, if included { "包含" } else { "不包含" }, if request.terminal_raw_materials { "是（使用本地配方；烧制燃料另计）" } else { "否" });
    materials.diagnostics.push(stockpile::StockpileDiagnostic { kind: "export_context".into(), source: request.source_path.clone(), target: String::new(), message: context.clone() });
    let text = match request.format {
        MaterialExportFormat::Html => stockpile_zip::render_standalone_materials_html(source, &request.minecraft_version, materials.clone())?,
        MaterialExportFormat::Txt => format!("材料列表\n{context}\n\n名称\t物品 ID\t数量\n{}", materials.materials.iter().map(|row| format!("{}\t{}\t{}\n", row.display_name, row.namespace_id, row.required_count)).collect::<String>()),
        MaterialExportFormat::Svg => svg(&materials, &context),
        MaterialExportFormat::Csv => format!("\u{feff}名称,物品ID,数量\r\n{}", materials.materials.iter().map(|row| format!("{},{},{}\r\n", csv(&row.display_name), csv(&row.namespace_id), row.required_count)).collect::<String>()),
    };
    std::fs::write(output, text.as_bytes()).with_context(|| format!("写入材料列表 {}", output.display()))?;
    Ok(MaterialExportSummary { output: output.display().to_string(), format: request.format, material_count: materials.materials.len(), multiplier: request.multiplier, terminal_raw_materials: request.terminal_raw_materials, include_container_items: included })
}

fn add_count(counts: &mut BTreeMap<String, u64>, id: &str, count: u64) -> Result<()> {
    let value = counts.entry(id.to_owned()).or_default();
    *value = value.checked_add(count).context("末端原料数量溢出")?;
    Ok(())
}

fn add_terminal(node: &recipe_tree::RecipeTreeNode, counts: &mut BTreeMap<String, u64>) -> Result<()> {
    if node.children.is_empty() {
        if node.tag.is_some() || node.item_id.contains('|') || (node.unresolved && node.unresolved_reason.as_deref() != Some("no_recipe")) {
            bail!("配方末端 {} 存在未确定标签、特殊配方、循环或深度限制；请先确定配方再导出末端原料", node.item_id);
        }
        add_count(counts, &node.item_id, node.needed_count)
    } else {
        for child in &node.children { add_terminal(child, counts)?; }
        Ok(())
    }
}

fn xml(value: &str) -> String { value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;") }
fn csv(value: &str) -> String { format!("\"{}\"", value.replace('"', "\"\"")) }
fn svg(materials: &stockpile::StockpileMaterialsData, context: &str) -> String {
    let height = 108 + materials.materials.len() * 32;
    let mut result = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"{height}\" viewBox=\"0 0 1000 {height}\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/><g font-family=\"Microsoft YaHei,sans-serif\" fill=\"#111\"><text x=\"24\" y=\"32\" font-size=\"22\">材料列表</text><text x=\"24\" y=\"58\" font-size=\"12\">{}</text><text x=\"24\" y=\"90\" font-size=\"15\">名称</text><text x=\"400\" y=\"90\" font-size=\"15\">物品 ID</text><text x=\"960\" y=\"90\" font-size=\"15\" text-anchor=\"end\">数量</text>", xml(context));
    for (index, row) in materials.materials.iter().enumerate() {
        let y = 122 + index * 32;
        result.push_str(&format!("<text x=\"24\" y=\"{y}\" font-size=\"14\">{}</text><text x=\"400\" y=\"{y}\" font-size=\"12\">{}</text><text x=\"960\" y=\"{y}\" font-size=\"14\" text-anchor=\"end\">{}</text>", xml(&row.display_name), xml(&row.namespace_id), row.required_count));
    }
    result.push_str("</g></svg>"); result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_counts_reuse_existing_recipe_tree_rounding() {
        let recipes = BTreeMap::from([("planks".into(), serde_json::json!({"type":"minecraft:crafting_shapeless","ingredients":[{"item":"minecraft:oak_log"}],"result":{"id":"minecraft:oak_planks","count":4}}))]);
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stats_water_fixture.litematic");
        let mut stats = stats_api::build_stats_output(&fixture, false).unwrap();
        stats.material_items = vec![stats_api::inventory_requirement("minecraft:oak_planks".into(), 5).unwrap()];
        let mut materials = stockpile::build_materials_data_from_stats(&fixture, stats, "26.3").unwrap();
        materials.materials[0].recipe_status = "available".into();
        let trees = recipe_tree::resolve_recipe_trees_from_recipes(&recipes, &materials);
        let mut counts = BTreeMap::new(); add_terminal(&trees["minecraft:oak_planks"], &mut counts).unwrap();
        assert_eq!(counts["minecraft:oak_log"], 2);
        materials.materials[0].required_count = 10;
        let trees = recipe_tree::resolve_recipe_trees_from_recipes(&recipes, &materials);
        let mut counts = BTreeMap::new(); add_terminal(&trees["minecraft:oak_planks"], &mut counts).unwrap();
        assert_eq!(counts["minecraft:oak_log"], 3, "scale quantities before recipe batch rounding");
    }
    #[test]
    fn snapshot_preserves_inventory_semantics_and_markup_escaping() {
        assert_eq!(xml("<&\"'>"), "&lt;&amp;&quot;&apos;&gt;");
        assert_eq!(csv("a\"b"), "\"a\"\"b\"");
        assert!(stats_api::inventory_requirement("minecraft:piston_head".into(), 3).is_none());
    }

    #[test]
    #[ignore = "CLI export check of the single measured real-input snapshot, no reanalysis"]
    fn real_snapshot_exports() {
        let snapshot = std::env::var("NOVA_STATS_SNAPSHOT").unwrap();
        let source = std::env::var("NOVA_STATS_FIXTURE").unwrap();
        let stats: StableStructureOutput = serde_json::from_reader(std::fs::File::open(&snapshot).unwrap()).unwrap();
        assert_eq!(stats.structure_stats.total_non_air_blocks, 88534);
        assert_eq!(stats.material_items.len(), 59);
        let directory = Path::new(&snapshot).parent().unwrap();
        for (extension, format) in [("svg", MaterialExportFormat::Svg), ("txt", MaterialExportFormat::Txt), ("html", MaterialExportFormat::Html)] {
            let path = directory.join(format!("840FTL-materials-2x.{extension}"));
            let summary = export(MaterialExportRequest { source_path: source.clone(), stats: stats.clone(), multiplier: 2, minecraft_version: "26.3".into(), terminal_raw_materials: false, format }, &path).unwrap();
            assert_eq!(summary.material_count, 59);
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("材料列表") || extension == "html");
            if extension == "html" {
                assert!(!content.contains("src=\"assets/app.js\""));
                assert!(!content.contains("href=\"assets/app.css\""));
                assert!(content.contains("data:image/png;base64,"));
                let embedded = content.split("window.__STOCKPILE_DATA__ = ").nth(1).unwrap().split(";</script>").next().unwrap();
                let payload: serde_json::Value = serde_json::from_str(embedded).unwrap();
                for row in &stats.material_items {
                    let actual = payload["materials"]["materials"].as_array().unwrap().iter().find(|actual| actual["namespace_id"] == row.block_id).unwrap();
                    assert_eq!(actual["required_count"], row.total_count * 2);
                }
            }
            println!("MATERIAL_EXPORT format={extension} rows={} multiplier=2 bytes={} path={}", summary.material_count, content.len(), path.display());
        }
    }
}
