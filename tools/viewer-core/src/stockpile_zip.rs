use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::recipe_cache::{self, RecipeCacheStatus, RecipeCacheStatusOutput};
use crate::recipe_tree::{self, RecipeTreeNode};
use crate::runtime_paths;
use crate::stockpile::{self, StockpileMaterialsData};
use crate::stockpile_schema::{
    STOCKPILE_I18N_SCHEMA_VERSION, STOCKPILE_ICONS_SCHEMA_VERSION,
    STOCKPILE_ITEM_NAMES_SCHEMA_VERSION, STOCKPILE_MATERIALS_SCHEMA_VERSION,
    STOCKPILE_RECIPE_TREES_SCHEMA_VERSION, STOCKPILE_ZIP_SCHEMA_VERSION,
};
use crate::{
    item_icons,
    item_icons::{IconZipAsset, StockpileIconPayload},
    item_names::{self, StockpileItemNamesPayload},
};

const DEFAULT_MINECRAFT_VERSION: &str = "1.21.10";
const GENERATOR: &str = "litematica_core stockpile export-zip";
const STOCKPILE_APP_CSS: &str = include_str!("stockpile_app.css");
const STOCKPILE_APP_JS: &str = include_str!("stockpile_app.js");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileZipSummary {
    pub output: PathBuf,
    pub source_file: String,
    pub minecraft_version: String,
    pub material_count: usize,
    pub recipe_status: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileZipPayload {
    pub manifest: StockpileZipManifest,
    pub materials: StockpileMaterialsData,
    pub recipe_status: RecipeCacheStatusOutput,
    pub recipe_trees: BTreeMap<String, RecipeTreeNode>,
    pub icons: StockpileIconPayload,
    pub item_names: StockpileItemNamesPayload,
    pub i18n: Value,
    #[serde(skip)]
    pub(crate) icon_files: Vec<IconZipAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockpileZipManifest {
    pub schema_version: u32,
    pub materials_schema_version: u32,
    pub recipe_trees_schema_version: u32,
    pub i18n_schema_version: u32,
    pub icons_schema_version: u32,
    pub item_names_schema_version: u32,
    pub created_at: u64,
    pub source_file: String,
    pub minecraft_version: String,
    pub recipe_status: String,
    pub generator: String,
    pub material_count: usize,
}

pub fn export_stockpile_zip(
    input: &Path,
    output: Option<&Path>,
    include_container_items: bool,
    minecraft_version: Option<&str>,
) -> Result<StockpileZipSummary> {
    let minecraft_version = minecraft_version.unwrap_or(DEFAULT_MINECRAFT_VERSION);
    let materials =
        stockpile::build_materials_data(input, include_container_items, Some(minecraft_version))?;
    let recipe_status = recipe_cache::recipe_status(minecraft_version)?;
    let output_path = resolve_output_path(input, output)?;
    let payload = build_payload(input, minecraft_version, materials, recipe_status)?;
    write_zip(&output_path, &payload)?;
    Ok(summary(&output_path, &payload))
}

fn build_payload(
    input: &Path,
    minecraft_version: &str,
    mut materials: StockpileMaterialsData,
    recipe_status: RecipeCacheStatusOutput,
) -> Result<StockpileZipPayload> {
    let recipe_trees = if recipe_status.status == RecipeCacheStatus::Available {
        recipe_tree::resolve_recipe_trees_for_materials(minecraft_version, &materials)
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    build_payload_with_trees(
        input,
        minecraft_version,
        &mut materials,
        recipe_status,
        recipe_trees,
    )
}

fn build_payload_with_trees(
    input: &Path,
    minecraft_version: &str,
    materials: &mut StockpileMaterialsData,
    recipe_status: RecipeCacheStatusOutput,
    recipe_trees: BTreeMap<String, RecipeTreeNode>,
) -> Result<StockpileZipPayload> {
    let icon_assets =
        item_icons::resolve_stockpile_icons(minecraft_version, materials, &recipe_trees)?;
    let item_names =
        item_names::resolve_stockpile_item_names(minecraft_version, materials, &recipe_trees)?;
    let manifest = StockpileZipManifest {
        schema_version: STOCKPILE_ZIP_SCHEMA_VERSION,
        materials_schema_version: STOCKPILE_MATERIALS_SCHEMA_VERSION,
        recipe_trees_schema_version: STOCKPILE_RECIPE_TREES_SCHEMA_VERSION,
        i18n_schema_version: STOCKPILE_I18N_SCHEMA_VERSION,
        icons_schema_version: STOCKPILE_ICONS_SCHEMA_VERSION,
        item_names_schema_version: STOCKPILE_ITEM_NAMES_SCHEMA_VERSION,
        created_at: current_unix_timestamp()?,
        source_file: input.display().to_string(),
        minecraft_version: minecraft_version.to_string(),
        recipe_status: recipe_status_string(&recipe_status),
        generator: GENERATOR.to_string(),
        material_count: materials.materials.len(),
    };
    Ok(StockpileZipPayload {
        manifest,
        materials: materials.clone(),
        recipe_status,
        recipe_trees,
        icons: icon_assets.payload,
        item_names,
        i18n: i18n_payload(),
        icon_files: icon_assets.files,
    })
}

fn write_zip(output_path: &Path, payload: &StockpileZipPayload) -> Result<()> {
    ensure_stockpile_export_output(output_path)?;
    let file = File::create(output_path)
        .with_context(|| format!("create stockpile zip failed: {}", output_path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    add_text_file(
        &mut zip,
        "index.html",
        &render_index_html(payload)?,
        options,
    )?;
    add_text_file(&mut zip, "assets/app.css", STOCKPILE_APP_CSS, options)?;
    add_text_file(&mut zip, "assets/app.js", STOCKPILE_APP_JS, options)?;
    add_json_file(&mut zip, "data/manifest.json", &payload.manifest, options)?;
    add_json_file(&mut zip, "data/materials.json", &payload.materials, options)?;
    add_json_file(
        &mut zip,
        "data/recipe_status.json",
        &payload.recipe_status,
        options,
    )?;
    add_json_file(
        &mut zip,
        "data/recipe_trees.json",
        &payload.recipe_trees,
        options,
    )?;
    add_json_file(&mut zip, "data/icons.json", &payload.icons, options)?;
    add_json_file(
        &mut zip,
        "data/item_names.json",
        &payload.item_names,
        options,
    )?;
    add_json_file(&mut zip, "data/i18n.json", &payload.i18n, options)?;
    for icon in &payload.icon_files {
        add_bytes_file(&mut zip, &icon.path, &icon.bytes, options)?;
    }
    zip.finish().context("finish stockpile zip failed")?;
    Ok(())
}

fn add_text_file(
    zip: &mut ZipWriter<File>,
    name: &str,
    content: &str,
    options: SimpleFileOptions,
) -> Result<()> {
    zip.start_file(name, options)?;
    zip.write_all(content.as_bytes())?;
    Ok(())
}

fn add_bytes_file(
    zip: &mut ZipWriter<File>,
    name: &str,
    content: &[u8],
    options: SimpleFileOptions,
) -> Result<()> {
    zip.start_file(name, options)?;
    zip.write_all(content)?;
    Ok(())
}

fn add_json_file<T: Serialize>(
    zip: &mut ZipWriter<File>,
    name: &str,
    value: &T,
    options: SimpleFileOptions,
) -> Result<()> {
    let content = serde_json::to_string_pretty(value)?;
    add_text_file(zip, name, &(content + "\n"), options)
}

fn render_index_html(payload: &StockpileZipPayload) -> Result<String> {
    let payload_json = serde_json::to_string(payload)?;
    Ok(INDEX_HTML.replace("__STOCKPILE_PAYLOAD__", &escape_script_json(&payload_json)))
}

fn escape_script_json(value: &str) -> String {
    value
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
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
        None => runtime_paths::stockpile_exports_root()?
            .join(project_slug(input))
            .with_extension("stockpile.zip"),
    };
    Ok(path)
}

fn ensure_stockpile_export_output(path: &Path) -> Result<()> {
    let exports_root = runtime_paths::stockpile_exports_root()?;
    let full_output = absolutize(path)?;
    let full_exports = absolutize(&exports_root)?;
    if !full_output.starts_with(&full_exports) {
        bail!(
            "stockpile zip output must be under {}",
            full_exports.display()
        );
    }
    if let Some(parent) = full_output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create stockpile export directory failed: {}",
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

fn recipe_status_string(status: &RecipeCacheStatusOutput) -> String {
    serde_json::to_value(status.status)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

fn i18n_payload() -> Value {
    json!({
        "zh-CN": {
            "schema_version": STOCKPILE_I18N_SCHEMA_VERSION,
            "appTitle": "Litematica 备货单",
            "offlineMode": "本机预览状态，非多人同步",
            "syncMode": "多人同步模式",
            "lastSync": "最近同步",
            "participants": "参与人数",
            "currentId": "当前 ID",
            "switchId": "切换 ID",
            "language": "语言",
            "enterIdTitle": "输入你的备货 ID",
            "enterIdBody": "ID 必填。离线打开时状态只保存到本机；serve 模式会同步到本项目会话数据库。",
            "enterIdPlaceholder": "例如 Steve / Builder01",
            "enter": "进入备货单",
            "search": "搜索材料、ID、分类",
            "all": "全部",
            "notStarted": "未开始",
            "preparing": "备货中",
            "done": "已完成",
            "partialDone": "部分完成",
            "overfilled": "超量",
            "mine": "我参与的",
            "unclaimed": "无人认领",
            "craftable": "可合成",
            "unresolved": "未解析",
            "recipeMissing": "合成表缺失",
            "countDesc": "数量大到小",
            "countAsc": "数量小到大",
            "grouped": "按类型分组",
            "remainingDesc": "剩余缺口大到小",
            "status": "完成状态",
            "mineFirst": "我参与的优先",
            "name": "名称",
            "totalMaterials": "材料种类",
            "totalBlocks": "总方块数",
            "totalStacks": "总组数",
            "shulkerEstimate": "潜影盒估算",
            "required": "需求",
            "remaining": "剩余缺口",
            "claimed": "已认领",
            "claimedBy": "参与者",
            "available": "可合成",
            "missing": "缺失",
            "quantity": "数量",
            "cancel": "取消",
            "details": "详情",
            "collapse": "收起",
            "sourceRegions": "来源区域",
            "recipeStatus": "合成状态",
            "recipeUnavailable": "当前备货单未包含合成表",
            "recipeCachedNoTree": "合成表已缓存，合成树解析器未生成该材料路径",
            "recipeUnresolved": "当前材料未解析到固定合成路径",
            "inputs": "输入",
            "need": "需求",
            "recipe": "配方",
            "process": "工艺",
            "recipeCraftingShaped": "有序合成",
            "recipeCraftingShapeless": "无序合成",
            "recipeStonecutting": "切石",
            "recipeSmelting": "熔炼",
            "recipeBlasting": "高炉熔炼",
            "recipeSmoking": "烟熏",
            "recipeCampfireCooking": "营火烹饪",
            "recipeSmithingTransform": "锻造升级",
            "recipeSmithingTrim": "锻造纹饰",
            "recipeSpecial": "特殊配方",
            "processCraft": "合成",
            "processStonecut": "切石",
            "processSmelt": "熔炼",
            "processBlast": "高炉",
            "processSmoke": "烟熏",
            "processCampfire": "营火",
            "processSmith": "锻造",
            "processSpecial": "特殊",
            "processTag": "材料组",
            "processUnresolved": "未解析",
            "outputEach": "每次产出",
            "batches": "需要批次",
            "extra": "多余数量",
            "depth": "深度",
            "fuelRequired": "需要燃料",
            "decorativeSmithing": "装饰锻造",
            "tagGroup": "可替代材料组",
            "specialRecipe": "特殊配方，无法静态展开",
            "noRecipe": "无固定配方，需要手动准备",
            "tagInput": "tag 输入不会自动猜具体材料",
            "iconFallback": "备用图标",
            "syncError": "同步失败",
            "empty": "没有匹配的材料"
        },
        "en-US": {
            "schema_version": STOCKPILE_I18N_SCHEMA_VERSION,
            "appTitle": "Litematica Stockpile",
            "offlineMode": "Local preview state, not multiplayer sync",
            "syncMode": "Multiplayer sync mode",
            "lastSync": "Last sync",
            "participants": "Participants",
            "currentId": "Current ID",
            "switchId": "Switch ID",
            "language": "Language",
            "enterIdTitle": "Enter your stockpile ID",
            "enterIdBody": "ID is required. Offline state stays in this browser; serve mode syncs to this project session database.",
            "enterIdPlaceholder": "e.g. Steve / Builder01",
            "enter": "Open stockpile",
            "search": "Search material, ID, category",
            "all": "All",
            "notStarted": "Not started",
            "preparing": "Preparing",
            "done": "Done",
            "partialDone": "Part done",
            "overfilled": "Overfilled",
            "mine": "Mine",
            "unclaimed": "Unclaimed",
            "craftable": "Craftable",
            "unresolved": "Unresolved",
            "recipeMissing": "Recipe missing",
            "countDesc": "Count high to low",
            "countAsc": "Count low to high",
            "grouped": "Grouped by type",
            "remainingDesc": "Remaining high to low",
            "status": "Status",
            "mineFirst": "Mine first",
            "name": "Name",
            "totalMaterials": "Unique materials",
            "totalBlocks": "Total blocks",
            "totalStacks": "Total stacks",
            "shulkerEstimate": "Shulker estimate",
            "required": "Required",
            "remaining": "Remaining",
            "claimed": "Claimed",
            "claimedBy": "Participants",
            "available": "Craftable",
            "missing": "Missing",
            "quantity": "Qty",
            "cancel": "Cancel",
            "details": "Details",
            "collapse": "Collapse",
            "sourceRegions": "Source regions",
            "recipeStatus": "Recipe status",
            "recipeUnavailable": "This stockpile does not include recipes",
            "recipeCachedNoTree": "Recipe cache is available, but no tree was generated for this material",
            "recipeUnresolved": "This material did not resolve to a fixed recipe path",
            "inputs": "Inputs",
            "need": "Need",
            "recipe": "Recipe",
            "process": "Process",
            "recipeCraftingShaped": "Shaped crafting",
            "recipeCraftingShapeless": "Shapeless crafting",
            "recipeStonecutting": "Stonecutting",
            "recipeSmelting": "Smelting",
            "recipeBlasting": "Blasting",
            "recipeSmoking": "Smoking",
            "recipeCampfireCooking": "Campfire cooking",
            "recipeSmithingTransform": "Smithing transform",
            "recipeSmithingTrim": "Smithing trim",
            "recipeSpecial": "Special recipe",
            "processCraft": "Craft",
            "processStonecut": "Stonecut",
            "processSmelt": "Smelt",
            "processBlast": "Blast",
            "processSmoke": "Smoke",
            "processCampfire": "Campfire",
            "processSmith": "Smith",
            "processSpecial": "Special",
            "processTag": "Tag",
            "processUnresolved": "Unresolved",
            "outputEach": "Output each",
            "batches": "Batches",
            "extra": "Extra",
            "depth": "Depth",
            "fuelRequired": "Fuel required",
            "decorativeSmithing": "Decorative smithing",
            "tagGroup": "Alternative material group",
            "specialRecipe": "Special recipe, cannot be expanded statically",
            "noRecipe": "No fixed recipe; prepare manually",
            "tagInput": "Tag input is not guessed as a concrete item",
            "iconFallback": "Fallback icon",
            "syncError": "Sync failed",
            "empty": "No matching materials"
        }
    })
}

fn summary(output_path: &Path, payload: &StockpileZipPayload) -> StockpileZipSummary {
    StockpileZipSummary {
        output: output_path.to_path_buf(),
        source_file: payload.manifest.source_file.clone(),
        minecraft_version: payload.manifest.minecraft_version.clone(),
        material_count: payload.manifest.material_count,
        recipe_status: payload.manifest.recipe_status.clone(),
        files: vec![
            "index.html".to_string(),
            "assets/app.css".to_string(),
            "assets/app.js".to_string(),
            "data/manifest.json".to_string(),
            "data/materials.json".to_string(),
            "data/recipe_status.json".to_string(),
            "data/recipe_trees.json".to_string(),
            "data/icons.json".to_string(),
            "data/item_names.json".to_string(),
            "data/i18n.json".to_string(),
            format!("assets/icons/*.png ({})", payload.icon_files.len()),
        ],
    }
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Litematica 备货单</title>
  <link rel="stylesheet" href="assets/app.css" />
</head>
<body>
  <div id="app"></div>
  <script>window.__STOCKPILE_DATA__ = __STOCKPILE_PAYLOAD__;</script>
  <script src="assets/app.js"></script>
</body>
</html>
"#;

#[allow(dead_code)]
const APP_CSS: &str = r#":root {
  color-scheme: dark;
  --bg: #101311;
  --panel: #171c19;
  --panel-2: #202620;
  --line: #344136;
  --text: #eef6ee;
  --muted: #9fb09f;
  --accent: #69d391;
  --accent-2: #76b8ff;
  --warn: #f6c56c;
  --done: #7ee0c0;
  --bad: #ff8989;
}

* { box-sizing: border-box; }
html, body { margin: 0; min-height: 100%; background: var(--bg); color: var(--text); font-family: Inter, "Segoe UI", system-ui, sans-serif; }
body { line-height: 1.45; }
button, input, select { font: inherit; }
button { cursor: pointer; }

.topbar {
  position: sticky;
  top: 0;
  z-index: 10;
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 14px;
  align-items: center;
  padding: 14px clamp(14px, 3vw, 34px);
  background: rgba(16, 19, 17, .92);
  border-bottom: 1px solid var(--line);
  backdrop-filter: blur(12px);
}
.brand { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.brand strong { font-size: clamp(22px, 4vw, 40px); letter-spacing: 0; }
.brand span { color: var(--muted); font-size: 13px; }
.identity { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; justify-content: flex-end; }
.identity .note { color: var(--warn); font-size: 12px; }

.shell { max-width: 1360px; margin: 0 auto; padding: 22px clamp(14px, 3vw, 34px) 44px; }
.summary { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin-bottom: 18px; }
.metric { background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 16px; }
.metric b { display: block; font-size: 25px; }
.metric span { color: var(--muted); font-size: 12px; }
.progress-track { height: 10px; background: #0b0e0c; border-radius: 999px; overflow: hidden; margin-top: 8px; border: 1px solid var(--line); }
.progress-bar { height: 100%; background: linear-gradient(90deg, var(--accent), var(--accent-2)); width: 0%; }

.toolbar { display: grid; grid-template-columns: minmax(220px, 1fr) 180px 180px; gap: 10px; margin: 16px 0 20px; }
.field { width: 100%; padding: 11px 12px; border-radius: 8px; border: 1px solid var(--line); color: var(--text); background: #0d110f; outline: none; }
.field:focus { border-color: var(--accent); box-shadow: 0 0 0 2px rgba(105, 211, 145, .16); }
.button { border: 1px solid var(--line); color: var(--text); background: var(--panel-2); border-radius: 8px; padding: 9px 12px; }
.button.primary { background: #1f5f3b; border-color: #3a8b5a; }
.button.danger { background: #3b1d1d; border-color: #714040; }

.section { margin: 22px 0; }
.section-head { display: grid; grid-template-columns: auto 1fr auto; gap: 12px; align-items: center; padding: 14px 2px; border-bottom: 1px solid var(--line); }
.section-icon { width: 42px; height: 42px; display: grid; place-items: center; border: 1px solid var(--line); border-radius: 8px; background: var(--panel); color: var(--accent); font-size: 12px; text-align: center; }
.section-head h2 { margin: 0; font-size: 22px; }
.section-meta { color: var(--muted); font-size: 13px; }
.section-rate { color: var(--accent); font-weight: 700; }

.list { display: grid; gap: 10px; margin-top: 12px; }
.card { background: var(--panel); border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
.card-main { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 14px; padding: 14px; align-items: start; }
.material-title { display: flex; gap: 10px; align-items: center; min-width: 0; flex-wrap: wrap; }
.item-icon { color: var(--accent-2); background: #0d1520; border: 1px solid #244260; border-radius: 6px; padding: 4px 7px; font-size: 12px; }
.name { font-weight: 800; font-size: 17px; }
.sub { color: var(--muted); font-size: 12px; word-break: break-word; }
.count { font-size: 24px; font-weight: 800; text-align: right; }
.badges { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 8px; }
.badge { border-radius: 999px; border: 1px solid var(--line); padding: 3px 8px; font-size: 12px; color: var(--muted); }
.badge.available { color: var(--done); border-color: #347d6f; }
.badge.unresolved { color: var(--warn); border-color: #7b6332; }
.badge.missing { color: var(--bad); border-color: #804949; }
.badge.done { color: var(--done); }
.badge.active { color: var(--accent-2); }

.actions { display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end; margin-top: 10px; }
.qty { width: 92px; }
.details { display: none; padding: 0 14px 14px; border-top: 1px solid var(--line); background: #111612; }
.card.open .details { display: block; }
.detail-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin-top: 12px; }
.detail { background: #0c100e; border: 1px solid var(--line); border-radius: 8px; padding: 10px; }
.detail span { display: block; color: var(--muted); font-size: 12px; }
.craft-chain { margin-top: 14px; display: grid; gap: 10px; }
.tree-node { position: relative; margin: 10px 0 10px 22px; }
.tree-node::before { content: ""; position: absolute; left: -13px; top: -10px; bottom: 20px; width: 1px; background: #3f5b46; }
.tree-node::after { content: ""; position: absolute; left: -13px; top: 24px; width: 13px; height: 1px; background: #3f5b46; }
.tree-node.root { margin-left: 0; }
.tree-node.root::before, .tree-node.root::after { display: none; }
.recipe-card { border: 1px solid var(--line); border-radius: 8px; background: #0b100d; padding: 12px; box-shadow: inset 0 1px 0 rgba(255,255,255,.03); }
.recipe-card.tag { border-color: #7b6332; background: #151309; }
.recipe-card.special { border-color: #805a43; background: #17100d; }
.recipe-card.unresolved { border-color: #804949; background: #180f0f; }
.recipe-card-head { display: grid; grid-template-columns: 46px minmax(0, 1fr) auto; gap: 10px; align-items: start; }
.recipe-icon { width: 46px; height: 46px; border-radius: 8px; display: grid; place-items: center; background: #0d1520; border: 1px solid #244260; color: var(--accent-2); font-size: 10px; text-align: center; overflow-wrap: anywhere; padding: 4px; }
.recipe-name { font-weight: 800; font-size: 15px; }
.recipe-id { color: var(--muted); font-size: 12px; overflow-wrap: anywhere; }
.recipe-badges { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 7px; }
.recipe-badge { border: 1px solid var(--line); border-radius: 999px; padding: 2px 7px; font-size: 11px; color: var(--muted); }
.recipe-badge.process { color: var(--accent); border-color: #3a8b5a; }
.recipe-badge.warn { color: var(--warn); border-color: #7b6332; }
.recipe-badge.bad { color: var(--bad); border-color: #804949; }
.recipe-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; margin-top: 10px; }
.recipe-stat { background: #08100b; border: 1px solid #26372b; border-radius: 8px; padding: 8px; }
.recipe-stat span { display: block; color: var(--muted); font-size: 11px; }
.recipe-ingredients { color: var(--muted); font-size: 12px; margin-top: 9px; }
.recipe-children { margin-top: 8px; }
.tree-toggle { min-width: 34px; height: 30px; padding: 0 8px; }
.possible-items { margin-top: 8px; display: flex; gap: 6px; flex-wrap: wrap; }
.possible-items span { border: 1px solid #5f6234; border-radius: 999px; padding: 2px 7px; font-size: 11px; color: var(--warn); }
.empty { color: var(--muted); border: 1px dashed var(--line); border-radius: 8px; padding: 20px; text-align: center; }

.modal { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; background: rgba(0,0,0,.76); padding: 20px; }
.modal-card { width: min(440px, 100%); background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 20px; }
.modal-card h2 { margin: 0 0 8px; }
.modal-card p { color: var(--muted); margin: 0 0 14px; }
.hidden { display: none; }

@media (max-width: 760px) {
  .topbar { grid-template-columns: 1fr; }
  .identity { justify-content: flex-start; }
  .summary { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .toolbar { grid-template-columns: 1fr; }
  .card-main { grid-template-columns: 1fr; }
  .count { text-align: left; }
  .actions { justify-content: flex-start; }
  .detail-grid { grid-template-columns: 1fr; }
}
"#;

#[allow(dead_code)]
const APP_JS: &str = r#"(function () {
  const data = window.__STOCKPILE_DATA__;
  const app = document.getElementById('app');
  const storageKey = `lba-stockpile:${data.manifest.source_file}:${data.manifest.created_at}`;
  const userKey = 'lba-stockpile-user-id';
  const recipeTrees = data.recipe_trees || {};
  let userId = localStorage.getItem(userKey) || '';
  let state = loadState();
  let controls = { search: '', sort: 'grouped', filter: 'all' };
  let open = new Set();
  let collapsedTree = new Set();

  function loadState() {
    try { return JSON.parse(localStorage.getItem(storageKey) || '{}'); }
    catch (_) { return {}; }
  }
  function saveState() { localStorage.setItem(storageKey, JSON.stringify(state)); }
  function materialState(item) {
    return state[item.namespace_id] || { status: 'not_started', quantity: 0, assignee: '' };
  }
  function setMaterialState(item, patch) {
    state[item.namespace_id] = { ...materialState(item), ...patch };
    saveState();
    render();
  }
  function ensureUser() {
    if (userId.trim()) return;
    renderModal();
  }
  function renderModal() {
    const modal = document.createElement('div');
    modal.className = 'modal';
    modal.innerHTML = `<div class="modal-card">
      <h2>输入你的备货 ID</h2>
      <p>ID 必填。状态只保存在本机 localStorage，非多人同步。</p>
      <input class="field" id="userInput" autocomplete="off" placeholder="例如 Steve / Builder01" />
      <div class="actions"><button class="button primary" id="saveUser">进入备货单</button></div>
    </div>`;
    document.body.appendChild(modal);
    const input = modal.querySelector('#userInput');
    input.focus();
    modal.querySelector('#saveUser').addEventListener('click', () => {
      const value = input.value.trim();
      if (!value) { input.focus(); return; }
      userId = value;
      localStorage.setItem(userKey, userId);
      modal.remove();
      render();
    });
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') modal.querySelector('#saveUser').click();
    });
  }
  function statusLabel(status) {
    return { not_started: '未开始', in_progress: '备货中', done: '已完成' }[status] || '未开始';
  }
  function recipeLabel(status) {
    return { available: '可合成', unresolved: '未解析', missing: '合成表缺失' }[status] || status;
  }
  function filteredMaterials() {
    const query = controls.search.trim().toLowerCase();
    return data.materials.materials.filter((item) => {
      const st = materialState(item);
      const mine = st.assignee === userId;
      if (query && !`${item.display_name} ${item.namespace_id} ${item.category} ${item.item_icon_key}`.toLowerCase().includes(query)) return false;
      switch (controls.filter) {
        case 'not_started': return st.status === 'not_started';
        case 'in_progress': return st.status === 'in_progress';
        case 'done': return st.status === 'done';
        case 'mine': return mine;
        case 'unassigned': return !st.assignee;
        case 'available': return item.recipe_status === 'available';
        case 'unresolved': return item.recipe_status === 'unresolved';
        case 'missing': return item.recipe_status === 'missing';
        default: return true;
      }
    });
  }
  function compareMaterials(a, b) {
    const sa = materialState(a), sb = materialState(b);
    const remainingA = Math.max(0, a.required_count - Number(sa.quantity || 0));
    const remainingB = Math.max(0, b.required_count - Number(sb.quantity || 0));
    switch (controls.sort) {
      case 'count_asc': return a.required_count - b.required_count || a.display_name.localeCompare(b.display_name);
      case 'remaining_desc': return remainingB - remainingA || b.required_count - a.required_count;
      case 'status': return statusRank(sa.status) - statusRank(sb.status) || b.required_count - a.required_count;
      case 'mine': return Number(sb.assignee === userId) - Number(sa.assignee === userId) || b.required_count - a.required_count;
      case 'name': return a.display_name.localeCompare(b.display_name);
      case 'grouped': return a.category.localeCompare(b.category) || doneRank(a) - doneRank(b) || b.required_count - a.required_count;
      default: return b.required_count - a.required_count || a.display_name.localeCompare(b.display_name);
    }
  }
  function statusRank(status) { return { in_progress: 0, not_started: 1, done: 2 }[status] ?? 3; }
  function doneRank(item) { return materialState(item).status === 'done' ? 1 : 0; }
  function grouped(items) {
    const groups = new Map();
    items.forEach((item) => {
      if (!groups.has(item.category)) groups.set(item.category, []);
      groups.get(item.category).push(item);
    });
    return [...groups.entries()].map(([category, group]) => ({ category, items: group.sort(compareMaterials) }));
  }
  function totals(items) {
    const required = items.reduce((sum, item) => sum + item.required_count, 0);
    const done = items.filter((item) => materialState(item).status === 'done').length;
    const progress = items.length ? Math.round((done / items.length) * 100) : 0;
    const assigned = items.filter((item) => materialState(item).assignee).length;
    return { required, done, progress, assigned };
  }
  function render() {
    const items = filteredMaterials().sort(compareMaterials);
    const allTotals = totals(data.materials.materials);
    app.innerHTML = `<header class="topbar">
      <div class="brand"><strong>Litematica 备货单</strong><span>${escapeHtml(data.manifest.source_file)} · Minecraft ${escapeHtml(data.manifest.minecraft_version)}</span></div>
      <div class="identity"><span class="note">本机预览状态，非多人同步</span><span>当前 ID：<b>${escapeHtml(userId || '未设置')}</b></span><button class="button" id="switchUser">切换 ID</button></div>
    </header>
    <main class="shell">
      <section class="summary">
        <div class="metric"><b>${data.materials.summary.total_blocks}</b><span>总材料数量</span></div>
        <div class="metric"><b>${data.materials.summary.unique_materials}</b><span>材料种类</span></div>
        <div class="metric"><b>${data.materials.summary.total_stacks}</b><span>估算组数</span></div>
        <div class="metric"><b>${allTotals.progress}%</b><span>本机完成率</span><div class="progress-track"><div class="progress-bar" style="width:${allTotals.progress}%"></div></div></div>
      </section>
      <section class="toolbar">
        <input class="field" id="search" placeholder="搜索材料、分类、ID、item_icon_key" value="${escapeAttr(controls.search)}" />
        <select class="field" id="sort">${sortOptions()}</select>
        <select class="field" id="filter">${filterOptions()}</select>
      </section>
      <div id="content">${renderContent(items)}</div>
    </main>`;
    bindControls();
  }
  function sortOptions() {
    return options([
      ['count_desc', '数量大到小'], ['count_asc', '数量小到大'], ['grouped', '按类型分组'],
      ['remaining_desc', '剩余缺口大到小'], ['status', '完成状态'], ['mine', '我参与的优先'], ['name', '名称']
    ], controls.sort);
  }
  function filterOptions() {
    return options([
      ['all', '全部'], ['not_started', '未开始'], ['in_progress', '备货中'], ['done', '已完成'],
      ['mine', '我参与的'], ['unassigned', '无人认领'], ['available', '可合成'],
      ['unresolved', '未解析'], ['missing', '合成表缺失']
    ], controls.filter);
  }
  function options(values, current) {
    return values.map(([value, label]) => `<option value="${value}" ${value === current ? 'selected' : ''}>${label}</option>`).join('');
  }
  function renderContent(items) {
    if (!items.length) return '<div class="empty">没有匹配的材料</div>';
    if (controls.sort !== 'grouped') return `<section class="section"><div class="list">${items.map(renderCard).join('')}</div></section>`;
    return grouped(items).map(renderGroup).join('');
  }
  function renderGroup(group) {
    const info = totals(group.items);
    const icon = group.items[0]?.category_icon || 'minecraft:grass_block';
    return `<section class="section">
      <div class="section-head"><div class="section-icon">${escapeHtml(icon)}</div><div><h2>${escapeHtml(group.category)}</h2><div class="section-meta">${group.items.length} 种 · 总数量 ${info.required} · 已认领 ${info.assigned}</div></div><div class="section-rate">${info.progress}%</div></div>
      <div class="list">${group.items.map(renderCard).join('')}</div>
    </section>`;
  }
  function renderCard(item) {
    const st = materialState(item);
    const isOpen = open.has(item.namespace_id);
    const remaining = Math.max(0, item.required_count - Number(st.quantity || 0));
    return `<article class="card ${isOpen ? 'open' : ''}" data-id="${escapeAttr(item.namespace_id)}">
      <div class="card-main">
        <div>
          <div class="material-title"><span class="item-icon">${escapeHtml(item.item_icon_key)}</span><span class="name">${escapeHtml(item.display_name)}</span></div>
          <div class="sub">${escapeHtml(item.namespace_id)} · ${escapeHtml(item.category)} · 剩余缺口 ${remaining}</div>
          <div class="badges"><span class="badge ${item.recipe_status}">${recipeLabel(item.recipe_status)}</span><span class="badge ${st.status === 'done' ? 'done' : st.status === 'in_progress' ? 'active' : ''}">${statusLabel(st.status)}</span><span class="badge">${st.assignee ? `负责人 ${escapeHtml(st.assignee)}` : '无人认领'}</span></div>
        </div>
        <div><div class="count">${item.required_count}</div><div class="actions">
          <input class="field qty" type="number" min="0" max="${item.required_count}" value="${Number(st.quantity || 0)}" data-action="qty" />
          <button class="button" data-action="progress">备货中</button>
          <button class="button primary" data-action="done">已完成</button>
          <button class="button danger" data-action="cancel">取消</button>
          <button class="button" data-action="toggle">${isOpen ? '收起' : '详情'}</button>
        </div></div>
      </div>
      <div class="details">${renderDetails(item)}</div>
    </article>`;
  }
  function renderDetails(item) {
    const tree = recipeTrees[item.namespace_id];
    let recipeNote = '';
    if (data.recipe_status.status === 'missing') recipeNote = '当前备货单未包含合成表';
    else if (item.recipe_status === 'available' && !tree) recipeNote = '合成表已缓存，但当前材料未生成合成树';
    else if (item.recipe_status === 'unresolved') recipeNote = '当前材料未解析到合成树';
    return `<div class="detail-grid">
      ${detail('namespace_id', item.namespace_id)}
      ${detail('数量', item.required_count)}
      ${detail('stacks / remainder', `${item.stacks} / ${item.remainder}`)}
      ${detail('shulker_boxes', item.shulker_boxes)}
      ${detail('source_regions', (item.source_regions || []).join(', ') || '-')}
      ${detail('recipe_status', item.recipe_status)}
    </div>${tree ? `<div class="craft-chain">${renderRecipeTree(tree, true)}</div>` : ''}<p class="sub">${escapeHtml(recipeNote)}</p>`;
  }
  function renderRecipeTree(node, isRoot = true) {
    const nodeId = node.node_id || `${node.item_id}-${node.depth || 0}`;
    const collapsed = collapsedTree.has(nodeId);
    const hasChildren = (node.children || []).length > 0;
    const children = hasChildren && !collapsed ? (node.children || []).map((child) => renderRecipeTree(child, false)).join('') : '';
    const ingredients = (node.ingredients || []).map((ingredient) => `${ingredient.display_name || ingredient.item_id} x${ingredient.needed_count}${ingredient.unresolved ? ` (${ingredient.unresolved_reason})` : ''}`).join(' · ');
    const possible = renderPossibleItems(node);
    const message = nodeMessageSafe(node);
    return `<div class="tree-node ${isRoot ? 'root' : ''}" data-tree-id="${escapeAttr(nodeId)}">
      <div class="recipe-card ${escapeAttr(node.visual_kind || '')}">
        <div class="recipe-card-head">
          <div class="recipe-icon">${escapeHtml(node.icon_key || node.item_id)}</div>
          <div>
            <div class="recipe-name">${escapeHtml(node.display_name || node.item_id)}</div>
            <div class="recipe-id">${escapeHtml(node.tag || node.item_id)}</div>
            <div class="recipe-badges">
              <span class="recipe-badge process">${processLabel(node)}</span>
              <span class="recipe-badge">Need ${node.needed_count}</span>
              ${node.unresolved ? `<span class="recipe-badge bad">${escapeHtml(node.unresolved_reason || 'unresolved')}</span>` : ''}
              ${node.requires_fuel ? '<span class="recipe-badge warn">Fuel required</span>' : ''}
              ${node.decorative_smithing ? '<span class="recipe-badge warn">Decorative smithing</span>' : ''}
            </div>
          </div>
          ${hasChildren ? `<button class="button tree-toggle" data-action="tree-toggle" data-tree-id="${escapeAttr(nodeId)}">${collapsed ? '+' : '-'}</button>` : ''}
        </div>
        <div class="recipe-grid">
          ${recipeStat('Recipe', node.recipe_type || 'n/a')}
          ${recipeStat('Process', node.process_type || 'n/a')}
          ${recipeStat('Output each', node.output_count)}
          ${recipeStat('Batches', node.batch_count)}
          ${recipeStat('Extra', node.extra_output)}
          ${recipeStat('Depth', node.depth || 0)}
        </div>
        ${ingredients ? `<div class="recipe-ingredients">Inputs: ${escapeHtml(ingredients)}</div>` : ''}
        ${message ? `<div class="recipe-ingredients">${escapeHtml(message)}</div>` : ''}
        ${possible}
      </div>
      ${children ? `<div class="recipe-children">${children}</div>` : ''}
    </div>`;
  }
  function recipeStat(label, value) {
    return `<div class="recipe-stat"><span>${escapeHtml(label)}</span>${escapeHtml(String(value ?? 0))}</div>`;
  }
  function processLabel(node) {
    const map = { craft: 'Craft', stonecut: 'Stonecut', smelt: 'Smelt', blast: 'Blast', smoke: 'Smoke', campfire: 'Campfire', smith: 'Smith', smith_trim: 'Smith', special: 'Special', tag: 'Tag', unresolved: 'Unresolved' };
    return escapeHtml(map[node.process_type] || node.process_type || node.recipe_type || 'Recipe');
  }
  function renderPossibleItems(node) {
    const values = (node.possible_items || []).slice(0, 12);
    if (!values.length) return '';
    const more = (node.possible_items || []).length - values.length;
    return `<div class="possible-items">${values.map((item) => `<span>${escapeHtml(item)}</span>`).join('')}${more > 0 ? `<span>+${more}</span>` : ''}</div>`;
  }
  function nodeMessageSafe(node) {
    if (node.visual_kind === 'tag') return `可替代材料组 ${node.tag || node.item_id}`;
    if (node.visual_kind === 'special') return `特殊配方，无法静态展开：${node.recipe_type}`;
    if (node.unresolved_reason === 'no_recipe') return '无固定配方，需要手动准备';
    if (node.unresolved_reason === 'tag_input') return 'tag 输入不会自动猜具体材料';
    if (node.unresolved) return `未解析：${node.unresolved_reason || 'unresolved'}`;
    return '';
  }
  /* function nodeMessage(node) {
    if (node.visual_kind === 'tag') return `可替代材料组 ${node.tag || node.item_id}`;
    if (node.visual_kind === 'special') return `特殊配方，无法静态展开：${node.recipe_type}`;
    if (node.unresolved_reason === 'no_recipe') return '无固定配方，需要手动准备';
    if (node.unresolved_reason === 'tag_input') return 'tag 输入不会自动猜具体材料';
    if (node.unresolved) return `未解析：${node.unresolved_reason || 'unresolved'}`;
    return '';
  }
  */
  function detail(label, value) { return `<div class="detail"><span>${escapeHtml(label)}</span>${escapeHtml(String(value))}</div>`; }
  function bindControls() {
    document.getElementById('switchUser').addEventListener('click', () => {
      localStorage.removeItem(userKey);
      userId = '';
      ensureUser();
      render();
    });
    document.getElementById('search').addEventListener('input', (e) => { controls.search = e.target.value; render(); });
    document.getElementById('sort').addEventListener('change', (e) => { controls.sort = e.target.value; render(); });
    document.getElementById('filter').addEventListener('change', (e) => { controls.filter = e.target.value; render(); });
    document.querySelectorAll('.card').forEach((card) => {
      const item = data.materials.materials.find((m) => m.namespace_id === card.dataset.id);
      card.querySelector('[data-action="qty"]').addEventListener('change', (e) => setMaterialState(item, { quantity: Math.max(0, Number(e.target.value || 0)), assignee: userId || materialState(item).assignee }));
      card.querySelector('[data-action="progress"]').addEventListener('click', () => setMaterialState(item, { status: 'in_progress', assignee: userId }));
      card.querySelector('[data-action="done"]').addEventListener('click', () => setMaterialState(item, { status: 'done', quantity: item.required_count, assignee: userId }));
      card.querySelector('[data-action="cancel"]').addEventListener('click', () => setMaterialState(item, { status: 'not_started', quantity: 0, assignee: '' }));
      card.querySelector('[data-action="toggle"]').addEventListener('click', () => { open.has(item.namespace_id) ? open.delete(item.namespace_id) : open.add(item.namespace_id); render(); });
    });
    document.querySelectorAll('[data-action="tree-toggle"]').forEach((button) => {
      button.addEventListener('click', (event) => {
        event.stopPropagation();
        const id = button.dataset.treeId;
        collapsedTree.has(id) ? collapsedTree.delete(id) : collapsedTree.add(id);
        render();
      });
    });
  }
  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch]));
  }
  function escapeAttr(value) { return escapeHtml(value); }
  render();
  ensureUser();
}());
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe_cache::RecipeCacheStatus;
    use serde_json::Value;
    use serde_json::json;
    use zip::ZipArchive;

    #[test]
    fn stockpile_zip_contains_required_files_and_embedded_payload() {
        let input = fixture_path();
        let materials = stockpile::build_materials_data(&input, false, Some("1.21.10"))
            .expect("materials data");
        let recipe_status = missing_recipe_status("1.21.10");
        let payload = build_payload(&input, "1.21.10", materials, recipe_status).expect("payload");
        let output = temp_zip_path("contains");
        write_zip(&output, &payload).expect("write zip");

        let file = File::open(&output).expect("open zip");
        let mut zip = ZipArchive::new(file).expect("zip archive");
        for name in [
            "index.html",
            "assets/app.css",
            "assets/app.js",
            "data/manifest.json",
            "data/materials.json",
            "data/recipe_status.json",
            "data/recipe_trees.json",
            "data/icons.json",
            "data/item_names.json",
            "data/i18n.json",
        ] {
            zip.by_name(name)
                .unwrap_or_else(|_| panic!("missing {name}"));
        }
        assert!(
            zip.file_names()
                .any(|name| name.starts_with("assets/icons/") && name.ends_with(".png"))
        );

        let index = read_zip_entry(&mut zip, "index.html");
        assert!(index.contains("window.__STOCKPILE_DATA__"));
        let materials_json: Value =
            serde_json::from_str(&read_zip_entry(&mut zip, "data/materials.json"))
                .expect("materials json");
        assert!(
            materials_json["materials"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        let first_material = &materials_json["materials"][0];
        assert!(first_material["icon_path"].as_str().is_some_and(|value| {
            value.starts_with("assets/icons/") && value.ends_with(".png")
        }));
        assert!(first_material["icon_available"].as_bool().is_some());
        let icons_json: Value =
            serde_json::from_str(&read_zip_entry(&mut zip, "data/icons.json")).expect("icons json");
        assert!(
            icons_json["by_key"]
                .as_object()
                .is_some_and(|value| !value.is_empty())
        );
        let i18n_json: Value =
            serde_json::from_str(&read_zip_entry(&mut zip, "data/i18n.json")).expect("i18n json");
        assert!(i18n_json.get("zh-CN").is_some());
        assert!(i18n_json.get("en-US").is_some());
        let item_names_json: Value =
            serde_json::from_str(&read_zip_entry(&mut zip, "data/item_names.json"))
                .expect("item names json");
        assert_eq!(item_names_json["schema_version"], json!(1));
        assert!(matches!(
            item_names_json["status"].as_str(),
            Some("available" | "partial" | "missing")
        ));
        let manifest_json: Value =
            serde_json::from_str(&read_zip_entry(&mut zip, "data/manifest.json"))
                .expect("manifest json");
        assert_eq!(manifest_json["schema_version"], json!(2));
        assert_eq!(manifest_json["materials_schema_version"], json!(2));
        assert_eq!(manifest_json["item_names_schema_version"], json!(1));
        let _ = fs::remove_file(output);
    }

    #[test]
    fn stockpile_zip_exports_when_recipe_cache_is_missing() {
        let input = fixture_path();
        let materials = stockpile::build_materials_data(&input, false, Some("1.21.10"))
            .expect("materials data");
        let payload = build_payload(
            &input,
            "1.21.10",
            materials,
            missing_recipe_status("1.21.10"),
        )
        .expect("payload");
        let output = temp_zip_path("missing_recipe");
        write_zip(&output, &payload).expect("write zip without recipe cache");
        let file = File::open(&output).expect("open zip");
        let mut zip = ZipArchive::new(file).expect("zip archive");
        let status: Value =
            serde_json::from_str(&read_zip_entry(&mut zip, "data/recipe_status.json"))
                .expect("status json");
        assert_eq!(status["status"], json!("missing"));
        let _ = fs::remove_file(output);
    }

    #[test]
    fn stockpile_zip_serializes_nonempty_recipe_trees() {
        let input = fixture_path();
        let mut materials = stockpile::build_materials_data(&input, false, Some("1.21.10"))
            .expect("materials data");
        let mut trees = BTreeMap::new();
        let mut root = test_recipe_node(
            "minecraft:stone",
            "Stone",
            "minecraft:smelting",
            "smelt",
            "process",
            0,
        );
        root.requires_fuel = true;
        root.children.push(test_recipe_node(
            "#minecraft:logs",
            "#minecraft:logs",
            "",
            "tag",
            "tag",
            1,
        ));
        root.children.last_mut().expect("tag node").unresolved = true;
        root.children
            .last_mut()
            .expect("tag node")
            .unresolved_reason = Some("tag_input".to_string());
        root.children.last_mut().expect("tag node").tag = Some("#minecraft:logs".to_string());
        root.children.push(test_recipe_node(
            "minecraft:netherite_pickaxe",
            "Netherite Pickaxe",
            "minecraft:smithing_transform",
            "smith",
            "process",
            1,
        ));
        root.children.push(test_recipe_node(
            "minecraft:firework_rocket",
            "Firework Rocket",
            "minecraft:crafting_special_firework_rocket",
            "special",
            "special",
            1,
        ));
        root.children.last_mut().expect("special node").unresolved = true;
        root.children
            .last_mut()
            .expect("special node")
            .unresolved_reason = Some("special_recipe".to_string());
        trees.insert("minecraft:stone".to_string(), root);
        let payload = build_payload_with_trees(
            &input,
            "1.21.10",
            &mut materials,
            available_recipe_status("1.21.10"),
            trees,
        )
        .expect("payload");
        let output = temp_zip_path("recipe_tree");
        write_zip(&output, &payload).expect("write zip");
        let file = File::open(&output).expect("open zip");
        let mut zip = ZipArchive::new(file).expect("zip archive");
        let trees_json: Value =
            serde_json::from_str(&read_zip_entry(&mut zip, "data/recipe_trees.json"))
                .expect("recipe trees json");
        assert!(
            trees_json
                .as_object()
                .is_some_and(|value| !value.is_empty())
        );
        let index = read_zip_entry(&mut zip, "index.html");
        assert!(index.contains("\"recipe_trees\""));
        assert!(index.contains("minecraft:stone"));
        assert!(index.contains("\"process_type\":\"smelt\""));
        assert!(index.contains("\"process_type\":\"smith\""));
        assert!(index.contains("\"visual_kind\":\"tag\""));
        assert!(index.contains("\"visual_kind\":\"special\""));
        let _ = fs::remove_file(output);
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stats_water_fixture.litematic")
    }

    fn temp_zip_path(name: &str) -> PathBuf {
        runtime_paths::stockpile_exports_root()
            .expect("exports root")
            .join(format!(
                "stockpile_zip_test_{name}_{}.zip",
                std::process::id()
            ))
    }

    fn missing_recipe_status(version: &str) -> RecipeCacheStatusOutput {
        let cache_dir = runtime_paths::recipe_cache_root()
            .expect("recipe root")
            .join(format!("minecraft_{version}"));
        RecipeCacheStatusOutput {
            status: RecipeCacheStatus::Missing,
            minecraft_version: version.to_string(),
            cache_dir: cache_dir.clone(),
            manifest_path: cache_dir.join("manifest.json"),
            has_recipes: false,
            has_items: false,
            has_blocks: false,
            source: None,
            fetched_at: None,
            schema_version: None,
            hash: None,
            warning: Some("recipe cache manifest is missing".to_string()),
        }
    }

    fn available_recipe_status(version: &str) -> RecipeCacheStatusOutput {
        let cache_dir = runtime_paths::recipe_cache_root()
            .expect("recipe root")
            .join(format!("minecraft_{version}"));
        RecipeCacheStatusOutput {
            status: RecipeCacheStatus::Available,
            minecraft_version: version.to_string(),
            cache_dir: cache_dir.clone(),
            manifest_path: cache_dir.join("manifest.json"),
            has_recipes: true,
            has_items: true,
            has_blocks: true,
            source: Some("test".to_string()),
            fetched_at: Some(0),
            schema_version: Some(1),
            hash: Some("test".to_string()),
            warning: None,
        }
    }

    fn test_recipe_node(
        item_id: &str,
        display_name: &str,
        recipe_type: &str,
        process_type: &str,
        visual_kind: &str,
        depth: u32,
    ) -> RecipeTreeNode {
        RecipeTreeNode {
            node_id: format!("{}_{}", item_id.replace([':', '#'], "_"), depth),
            item_id: item_id.to_string(),
            display_name: display_name.to_string(),
            icon_key: item_id.to_string(),
            needed_count: 3,
            output_count: 1,
            batch_count: 3,
            extra_output: 0,
            recipe_type: recipe_type.to_string(),
            process_type: process_type.to_string(),
            ingredients: Vec::new(),
            children: Vec::new(),
            unresolved: false,
            unresolved_reason: None,
            visual_kind: visual_kind.to_string(),
            depth,
            tag: None,
            possible_items: Vec::new(),
            requires_fuel: false,
            fuel_estimate: None,
            decorative_smithing: false,
        }
    }

    fn read_zip_entry(zip: &mut ZipArchive<File>, name: &str) -> String {
        let mut entry = zip.by_name(name).expect("zip entry");
        let mut content = String::new();
        use std::io::Read;
        entry.read_to_string(&mut content).expect("read entry");
        content
    }
}
