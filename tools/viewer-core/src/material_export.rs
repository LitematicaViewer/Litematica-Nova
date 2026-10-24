//! Material-list export uses the existing inventory, Stockpile and recipe
//! contracts. The persistent data worker supplies its identity-validated cache.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::stats_api::StableStructureOutput;
use crate::{recipe_cache, recipe_tree, stats_api, stockpile, stockpile_zip};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialExportFormat {
    Svg,
    Txt,
    Html,
    Csv,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct MaterialWebOptions {
    pub mode: MaterialWebMode,
    pub access_password: Option<String>,
    pub admin_password: Option<String>,
    pub whitelist: Vec<String>,
    pub whitelist_enabled: Option<bool>,
    pub allow_guest_readonly: Option<bool>,
    pub admin_page_enabled: Option<bool>,
    pub targets: Vec<String>,
}

#[derive(Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialWebMode {
    #[default]
    Single,
    Multi,
}

impl MaterialWebOptions {
    fn deploy_options(self) -> Result<stockpile_zip::StockpileDeployOptions> {
        Ok(stockpile_zip::StockpileDeployOptions {
            access_password: self.access_password,
            admin_password: self.admin_password,
            whitelist: self.whitelist,
            whitelist_enabled: self.whitelist_enabled,
            allow_guest_readonly: self.allow_guest_readonly,
            admin_page_enabled: self.admin_page_enabled,
            targets: stockpile_zip::parse_deploy_targets(&self.targets)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialExportRequest {
    #[serde(default)]
    pub ui_locale: Option<String>,
    #[serde(default)]
    pub language_map: BTreeMap<String, String>,
    pub source_path: String,
    pub stats: StableStructureOutput,
    pub multiplier: u64,
    pub minecraft_version: String,
    pub terminal_raw_materials: bool,
    pub format: MaterialExportFormat,
    #[serde(default)]
    pub web_options: Option<MaterialWebOptions>,
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
    ensure!(
        args.len() == 3 && args[1] == "--output",
        "usage: litematica_core materials-export REQUEST_JSON --output FILE"
    );
    let request: MaterialExportRequest = serde_json::from_reader(std::fs::File::open(&args[0])?)?;
    let summary = export(request, Path::new(&args[2]))?;
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

pub fn export(request: MaterialExportRequest, output: &Path) -> Result<MaterialExportSummary> {
    export_cancellable(request, output, &AtomicBool::new(false))
}

fn export_cancellable(
    request: MaterialExportRequest,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<MaterialExportSummary> {
    Ok(build_export(request, Some(output), cancel)?.0)
}

fn build_export(
    mut request: MaterialExportRequest,
    output: Option<&Path>,
    cancel: &AtomicBool,
) -> Result<(MaterialExportSummary, Option<String>)> {
    let english = request.ui_locale.as_deref() == Some("en-US");
    let locale = if english { "en-US" } else { "zh-CN" };
    ensure!(
        request.stats.material_semantics
            == format!(
                "minecraft-inventory-v9:{}",
                crate::game_data::active_context()?.content_id
            ),
        "材料快照资源身份已过期，请刷新材料列表后导出"
    );
    ensure!(
        (1..=1_000_000).contains(&request.multiplier),
        "建造份数必须在 1 至 1000000 之间"
    );
    ensure!(
        !request.minecraft_version.is_empty(),
        "材料导出必须指定 Minecraft 版本"
    );
    let source = Path::new(&request.source_path);
    if let Some(output) = output {
        ensure!(
            output.is_absolute() && output != source,
            "材料列表必须另存为独立文件"
        );
        let parent = output.parent().context("材料列表输出缺少父目录")?;
        ensure!(parent.is_dir(), "材料列表输出目录不存在");
        if output.exists() {
            ensure!(
                std::fs::canonicalize(output)? != std::fs::canonicalize(source)?,
                "材料列表输出不能覆盖原投影"
            );
        }
    }
    request
        .stats
        .material_items
        .retain(|row| row.material_status == "item" && row.total_count > 0);
    for row in &mut request.stats.material_items {
        for count in [
            &mut row.count,
            &mut row.block_count,
            &mut row.container_item_count,
            &mut row.total_count,
        ] {
            *count = count
                .checked_mul(request.multiplier)
                .context("材料数量超出 u64 范围")?;
        }
        for entry in row.region_counts.iter_mut().flatten() {
            entry.count = entry
                .count
                .checked_mul(request.multiplier)
                .context("区域材料数量溢出")?;
        }
        for entry in row.layer_counts.iter_mut().flatten() {
            entry.count = entry
                .count
                .checked_mul(request.multiplier)
                .context("层材料数量溢出")?;
        }
    }
    let scope = &request.stats.structure_stats.scope;
    let scope_text = match scope.kind.as_str() {
        "region" => format!(
            "{}{}",
            if english { "Region: " } else { "区域：" },
            scope.region.as_deref().unwrap_or_default()
        ),
        "layer" => format!(
            "{}{}",
            if english { "Layer: " } else { "层：" },
            scope.layer.unwrap_or_default()
        ),
        _ => if english {
            "Entire schematic"
        } else {
            "整个投影"
        }
        .to_owned(),
    };
    let included = request.stats.container_scan.enabled;
    let mut materials = stockpile::build_materials_data_from_stats(
        source,
        request.stats.clone(),
        &request.minecraft_version,
    )?;
    if request.terminal_raw_materials {
        ensure!(
            recipe_cache::load_available_recipes(&request.minecraft_version)?.is_some(),
            "末端原料统计需要该 Minecraft 版本的完整本地配方缓存；请先准备配方缓存"
        );
        let trees = recipe_tree::resolve_recipe_trees_for_materials(
            &request.minecraft_version,
            &materials,
        )?;
        let mut counts = BTreeMap::<String, u64>::new();
        for material in &materials.materials {
            if let Some(tree) = trees.get(&material.namespace_id) {
                add_terminal(tree, &mut counts)?;
            } else {
                add_count(&mut counts, &material.namespace_id, material.required_count)?;
            }
        }
        request.stats.material_items = counts
            .into_iter()
            .map(|(id, count)| {
                stats_api::inventory_requirement(id.clone(), count)
                    .with_context(|| format!("末端配方项 {id} 尚未解析成确定的真实物品"))
            })
            .collect::<Result<Vec<_>>>()?;
        materials = stockpile::build_materials_data_from_stats(
            source,
            request.stats,
            &request.minecraft_version,
        )?;
    }
    // Resolve names through the existing offline language cache. No resource download
    // or schematic rescan is introduced by exporting in another UI language.
    materials.ui_locale = Some(locale.into());
    materials.ui_translations = request.language_map;
    crate::item_names::resolve_stockpile_item_names_cached(
        &request.minecraft_version,
        &mut materials,
        &BTreeMap::new(),
    )?;
    for row in &mut materials.materials {
        row.display_name = row
            .display_names
            .get(locale)
            .or_else(|| row.display_names.get("en-US"))
            .cloned()
            .unwrap_or_else(|| {
                if english {
                    row.namespace_id.clone()
                } else {
                    row.display_name.clone()
                }
            });
    }
    let filename = source.file_name().unwrap_or_default().to_string_lossy();
    let context = if english {
        format!(
            "{filename} · {scope_text} · {copies} {copy_unit} · Container items: {containers} · Terminal raw materials: {terminal}",
            copies = request.multiplier,
            copy_unit = if request.multiplier == 1 {
                "copy"
            } else {
                "copies"
            },
            containers = if included { "included" } else { "excluded" },
            terminal = if request.terminal_raw_materials {
                "yes (local recipes; smelting fuel not included)"
            } else {
                "no"
            }
        )
    } else {
        format!(
            "{filename} · {scope_text} · {copies} 份 · 容器物品：{containers} · 末端原料：{terminal}",
            copies = request.multiplier,
            containers = if included { "包含" } else { "不包含" },
            terminal = if request.terminal_raw_materials {
                "是（使用本地配方；烧制燃料另计）"
            } else {
                "否"
            }
        )
    };
    materials.diagnostics.push(stockpile::StockpileDiagnostic {
        kind: "export_context".into(),
        source: request.source_path.clone(),
        target: String::new(),
        message: context.clone(),
    });
    let web = request.web_options.unwrap_or_default();
    let mut export_text = None;
    if matches!(request.format, MaterialExportFormat::Html) && web.mode == MaterialWebMode::Multi {
        let output = output.context("Choose an output path for the web package")?;
        atomic_output(output, source, cancel, |temporary| {
            stockpile_zip::export_stockpile_zip_from_materials(
                source,
                Some(temporary),
                &request.minecraft_version,
                materials.clone(),
                stockpile_zip::StockpileExportMode::Multi,
                web.deploy_options()?,
            )?;
            Ok(())
        })?;
    } else {
        let text = match request.format {
            MaterialExportFormat::Html => stockpile_zip::render_standalone_materials_html(
                source,
                &request.minecraft_version,
                materials.clone(),
            )?,
            MaterialExportFormat::Txt => format!(
                "{}\n{context}\n\n{}\n{}",
                if english {
                    "Material List"
                } else {
                    "材料列表"
                },
                if english {
                    "Name\tItem ID\tQuantity"
                } else {
                    "名称\t物品 ID\t数量"
                },
                materials
                    .materials
                    .iter()
                    .map(|row| format!(
                        "{}\t{}\t{}\n",
                        row.display_name, row.namespace_id, row.required_count
                    ))
                    .collect::<String>()
            ),
            MaterialExportFormat::Svg => svg(&materials, &context, english),
            MaterialExportFormat::Csv => format!(
                "\u{feff}{}\r\n{}",
                if english {
                    "Name,Item ID,Quantity"
                } else {
                    "名称,物品ID,数量"
                },
                materials
                    .materials
                    .iter()
                    .map(|row| format!(
                        "{},{},{}\r\n",
                        csv(&row.display_name),
                        csv(&row.namespace_id),
                        row.required_count
                    ))
                    .collect::<String>()
            ),
        };
        if let Some(output) = output {
            atomic_output(output, source, cancel, |temporary| {
                Ok(std::fs::write(temporary, text.as_bytes())?)
            })?;
        } else {
            ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
            export_text = Some(text);
        }
    }
    Ok((
        MaterialExportSummary {
            output: output
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            format: request.format,
            material_count: materials.materials.len(),
            multiplier: request.multiplier,
            terminal_raw_materials: request.terminal_raw_materials,
            include_container_items: included,
        },
        export_text,
    ))
}

/// A dialog draft, deliberately not a second persisted preset system. Historical
/// exports reset on unmount; existing task preferences remain in their SQLite DB.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ExportSettings {
    pub source: String,
    pub format: String,
    pub scope: String,
    pub region: String,
    pub layer: i32,
    pub multiplier: u64,
    pub include_container_items: bool,
    pub terminal_raw_materials: bool,
    pub web_mode: String,
    pub access_password: String,
    pub admin_password: String,
    pub whitelist: String,
    pub whitelist_enabled: bool,
    pub allow_guest_readonly: bool,
    pub admin_page_enabled: bool,
    pub targets: Vec<String>,
    pub individual_counts: bool,
}
impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            source: "materials".into(),
            format: "svg".into(),
            scope: "all".into(),
            region: String::new(),
            layer: 0,
            multiplier: 1,
            include_container_items: false,
            terminal_raw_materials: false,
            web_mode: "single".into(),
            access_password: String::new(),
            admin_password: String::new(),
            whitelist: String::new(),
            whitelist_enabled: false,
            allow_guest_readonly: false,
            admin_page_enabled: true,
            targets: vec![
                if cfg!(windows) {
                    "windows-x64"
                } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
                    "macos-arm64"
                } else if cfg!(target_os = "macos") {
                    "macos-x64"
                } else {
                    "linux-x64"
                }
                .into(),
            ],
            individual_counts: false,
        }
    }
}

pub(crate) fn is_task_export(input: &Value) -> bool {
    matches!(
        input.pointer("/settings/source").and_then(Value::as_str),
        Some("task" | "taskMaterials")
    ) || (input["sourcePath"].as_str().is_none() && input["taskId"].is_string())
}

/// Rust owns schema, defaults, applicability, version/resource identity and all
/// file semantics. The UI renders this response and only sends edited values.
pub(crate) fn configuration(
    input: &Value,
    stats: Option<&StableStructureOutput>,
    task: Option<&Value>,
) -> Result<Value> {
    let reset = input["action"].as_str() == Some("reset");
    let mut defaults = ExportSettings::default();
    defaults.include_container_items = input["includeContainerItems"]
        .as_bool()
        .unwrap_or_else(|| stats.is_some_and(|s| s.container_scan.enabled));
    if input["sourcePath"].as_str().is_none() && task.is_some() {
        defaults.source = "task".into();
        defaults.format = "copy".into();
    }
    defaults.individual_counts = task
        .and_then(|t| t["individualCounts"].as_bool())
        .unwrap_or(false);
    let mut value = serde_json::to_value(&defaults)?;
    if !reset {
        if let Some(patch) = input["settings"].as_object() {
            value.as_object_mut().unwrap().extend(patch.clone());
        }
    }
    let mut s: ExportSettings = serde_json::from_value(value)?;
    let task_source = ["task", "taskMaterials"].contains(&s.source.as_str());
    let formats: Vec<&str> = if s.source == "task" {
        vec!["copy", "txt", "csv", "json"]
    } else if task_source {
        vec!["copy", "txt", "csv"]
    } else {
        vec!["svg", "txt", "html", "csv", "copy"]
    };
    if !formats.contains(&s.format.as_str()) {
        s.format = if task_source { "copy" } else { "svg" }.into();
    }
    let scopes: Vec<&str> = if task_source {
        vec!["all", "visible", "remaining", "overfilled", "prepared"]
    } else {
        vec!["all", "region", "layer"]
    };
    if !scopes.contains(&s.scope.as_str()) {
        s.scope = "all".into();
    }
    let snapshot = s.source == "task" && s.format == "json";
    if snapshot {
        s.scope = "all".into();
    }
    let web = !task_source && s.format == "html";
    let multi = web && s.web_mode == "multi";
    let english = input["uiLocale"].as_str() == Some("en-US");
    let message = |zh: &str, en: &str| {
        if english {
            en.to_owned()
        } else {
            zh.to_owned()
        }
    };
    let version = if task_source {
        task.and_then(|t| t["minecraftVersion"].as_str())
            .unwrap_or("")
            .to_owned()
    } else {
        crate::game_data::active_context()?.game_version
    };
    let recipe_ready = crate::recipe_cache::recipe_status(&version)
        .is_ok_and(|s| s.status == crate::recipe_cache::RecipeCacheStatus::Available);
    let mut errors = Vec::<String>::new();
    if !["materials", "taskMaterials", "task"].contains(&s.source.as_str()) {
        errors.push(message(
            "请选择有效导出对象",
            "Select a valid export source",
        ));
    }
    if task_source && task.is_none() {
        errors.push(message(
            "请选择有效备货任务",
            "Select an existing material task",
        ));
    }
    if !task_source && stats.is_none() {
        errors.push(message(
            "请先打开投影材料清单",
            "Open a schematic material list first",
        ));
    }
    if !task_source && !(1..=1_000_000).contains(&s.multiplier) {
        errors.push(message(
            "建造份数必须在 1 至 1000000 之间",
            "Copies must be between 1 and 1000000",
        ));
    }
    if web && !["single", "multi"].contains(&s.web_mode.as_str()) {
        errors.push(message(
            "请选择单人或多人网页",
            "Select single-player or multiplayer web mode",
        ));
    }
    if !task_source && s.terminal_raw_materials && !recipe_ready {
        errors.push(message(
            "末端原料需要先准备当前版本的本地配方缓存",
            "Prepare local recipes for this version before exporting terminal raw materials",
        ));
    }
    if !task_source
        && s.scope == "region"
        && !stats.is_some_and(|stats| stats.regions.iter().any(|r| r.name == s.region))
    {
        errors.push(message("请选择有效区域", "Select an existing region"));
    }
    if !task_source
        && s.scope == "layer"
        && !stats.is_some_and(|stats| stats.layers_summary.iter().any(|r| r.layer == s.layer))
    {
        errors.push(message("请选择有效层", "Select an existing layer"));
    }
    let whitelist = split_whitelist(&s.whitelist);
    if multi {
        if s.access_password.trim().is_empty() || s.admin_password.trim().is_empty() {
            errors.push(message(
                "多人网页需要访问密码和管理员密码",
                "Multiplayer export requires access and administrator passwords",
            ));
        }
        if whitelist
            .iter()
            .any(|name| name.is_empty() || name.len() > 128)
        {
            errors.push(message(
                "白名单名称必须在 1 至 128 字节之间",
                "Whitelist names must contain 1 to 128 bytes",
            ));
        }
        if s.targets.is_empty() {
            errors.push(message(
                "请至少选择一个部署平台",
                "Select at least one deployment platform",
            ));
        }
        if stockpile_zip::parse_deploy_targets(&s.targets).is_err() {
            errors.push(message("部署平台无效", "Invalid deployment platform"));
        }
        let root = std::env::var_os("LBA_STOCKPILE_SERVER_BIN_ROOT")
            .map(PathBuf::from)
            .unwrap_or(crate::runtime_paths::app_root()?.join("bin/stockpile-server"));
        for target in &s.targets {
            if !["windows-x64", "linux-x64", "macos-x64", "macos-arm64"].contains(&target.as_str())
            {
                continue;
            }
            let binary = root.join(target).join(if target == "windows-x64" {
                "stockpile_server.exe"
            } else {
                "stockpile_server"
            });
            if !binary.is_file() {
                errors.push(format!(
                    "{}: {target}",
                    message(
                        "缺少该平台的真实服务器程序",
                        "A real server binary is missing for this platform"
                    )
                ));
            }
        }
    }
    let settings = serde_json::to_value(&s)?;
    let mut fields = Vec::<Value>::new();
    let mut field = |key: &str,
                     kind: &str,
                     group: &str,
                     visible: bool,
                     enabled: bool,
                     options: Vec<Value>,
                     reason: Option<&str>| {
        let mut f = json!({"key":key,"type":kind,"group":group,"visible":visible,"enabled":enabled,"value":settings[key],"labelKey":format!("stats.export.field.{key}")});
        if !options.is_empty() {
            f["options"] = options.into();
        }
        if let Some(reason) = reason {
            f["reasonKey"] = format!("stats.export.reason.{reason}").into();
        }
        fields.push(f);
    };
    let options = |values: &[&str]| {
        values
            .iter()
            .map(|v| json!({"value":v,"labelKey":format!("stats.export.option.{v}")}))
            .collect::<Vec<_>>()
    };
    field(
        "source",
        "select",
        "primary",
        true,
        true,
        options(if input["taskId"].is_string() {
            &["materials", "taskMaterials", "task"]
        } else {
            &["materials"]
        }),
        None,
    );
    field(
        "format",
        "select",
        "primary",
        true,
        true,
        options(&formats),
        None,
    );
    field(
        "scope",
        "select",
        "common",
        true,
        !snapshot,
        options(&scopes),
        if snapshot { Some("snapshot") } else { None },
    );
    field(
        "region",
        "select",
        "common",
        !task_source && s.scope == "region",
        true,
        stats
            .map(|stats| {
                stats
                    .regions
                    .iter()
                    .map(|r| json!({"value":r.name,"label":r.name}))
                    .collect()
            })
            .unwrap_or_default(),
        None,
    );
    field(
        "layer",
        "number",
        "common",
        !task_source && s.scope == "layer",
        true,
        Vec::new(),
        None,
    );
    field(
        "multiplier",
        "number",
        "common",
        !task_source,
        true,
        Vec::new(),
        None,
    );
    field(
        "includeContainerItems",
        "boolean",
        "common",
        !task_source,
        true,
        Vec::new(),
        None,
    );
    field(
        "individualCounts",
        "boolean",
        "common",
        task_source,
        !snapshot,
        Vec::new(),
        if snapshot { Some("snapshot") } else { None },
    );
    field(
        "terminalRawMaterials",
        "boolean",
        "advanced",
        !task_source,
        recipe_ready,
        Vec::new(),
        if recipe_ready { None } else { Some("recipes") },
    );
    field(
        "webMode",
        "select",
        "web",
        web,
        true,
        options(&["single", "multi"]),
        None,
    );
    for (key, kind) in [
        ("accessPassword", "password"),
        ("adminPassword", "password"),
        ("whitelist", "multiline"),
        ("whitelistEnabled", "boolean"),
        ("allowGuestReadonly", "boolean"),
        ("adminPageEnabled", "boolean"),
    ] {
        field(key, kind, "web", multi, multi, Vec::new(), None);
    }
    field(
        "targets",
        "multiselect",
        "web",
        multi,
        multi,
        options(&["windows-x64", "linux-x64", "macos-x64", "macos-arm64"]),
        None,
    );
    for f in &mut fields {
        match f["key"].as_str() {
            Some("multiplier") => {
                f["min"] = 1.into();
                f["max"] = 1_000_000.into();
                f["step"] = 1.into();
            }
            Some("layer") => {
                f["step"] = 1.into();
                if let Some(stats) = stats {
                    if let Some(min) = stats.layers_summary.iter().map(|l| l.layer).min() {
                        f["min"] = min.into();
                    }
                    if let Some(max) = stats.layers_summary.iter().map(|l| l.layer).max() {
                        f["max"] = max.into();
                    }
                }
            }
            _ => {}
        }
    }
    let extension = if multi {
        "zip"
    } else if s.format == "copy" {
        "txt"
    } else {
        &s.format
    };
    let source = if task_source {
        task.and_then(|t| t["sourcePath"].as_str()).unwrap_or("")
    } else {
        input["sourcePath"].as_str().unwrap_or("")
    };
    let source = Path::new(source);
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    let default_path = source.with_file_name(format!(
        "{stem}-{}.{}",
        if snapshot {
            "task-backup"
        } else if task_source {
            "task"
        } else {
            "materials"
        },
        extension
    ));
    Ok(
        json!({"settings":settings,"fields":fields,"valid":errors.is_empty(),"errors":errors,"defaultPath":default_path,"extension":extension,"copy":s.format=="copy","destination":if s.format=="copy"{"clipboard"}else{"file"},"recipeCacheReady":recipe_ready}),
    )
}

fn split_whitelist(value: &str) -> Vec<String> {
    value
        .split(|c: char| c.is_whitespace() || [',', '，', ';', '；'].contains(&c))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn export_snapshot(
    input: &Value,
    stats: StableStructureOutput,
    view: &mut Value,
    cancel: &AtomicBool,
) -> Result<()> {
    ensure!(view["valid"].as_bool() == Some(true), "{}", view["errors"]);
    let s: ExportSettings = serde_json::from_value(view["settings"].clone())?;
    let source = input["sourcePath"]
        .as_str()
        .context("A source path is required")?;
    let output = if s.format == "copy" {
        None
    } else {
        Some(Path::new(
            input["outputPath"]
                .as_str()
                .context("Choose an output path")?,
        ))
    };
    let format = match s.format.as_str() {
        "svg" => MaterialExportFormat::Svg,
        "txt" | "copy" => MaterialExportFormat::Txt,
        "html" => MaterialExportFormat::Html,
        "csv" => MaterialExportFormat::Csv,
        _ => bail!("Invalid material export format"),
    };
    let version = crate::game_data::active_context()?.game_version;
    let language_map = serde_json::from_value(
        input
            .get("languageMap")
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or(json!({})),
    )?;
    let (summary, text) = build_export(
        MaterialExportRequest {
            ui_locale: input["uiLocale"].as_str().map(str::to_owned),
            language_map,
            source_path: source.into(),
            stats,
            multiplier: s.multiplier,
            minecraft_version: version,
            terminal_raw_materials: s.terminal_raw_materials,
            format,
            web_options: Some(MaterialWebOptions {
                mode: if s.web_mode == "multi" {
                    MaterialWebMode::Multi
                } else {
                    MaterialWebMode::Single
                },
                access_password: Some(s.access_password.trim().into()),
                admin_password: Some(s.admin_password.trim().into()),
                whitelist: split_whitelist(&s.whitelist),
                whitelist_enabled: Some(s.whitelist_enabled),
                allow_guest_readonly: Some(s.allow_guest_readonly),
                admin_page_enabled: Some(s.admin_page_enabled),
                targets: s.targets,
            }),
        },
        output,
        cancel,
    )?;
    view["summary"] = serde_json::to_value(summary)?;
    if let Some(output) = output {
        view["outputPath"] = output.display().to_string().into();
    }
    if let Some(text) = text {
        view["exportText"] = text.into();
    }
    view["settings"]["accessPassword"] = "".into();
    view["settings"]["adminPassword"] = "".into();
    Ok(())
}

/// Produce beside the destination, then publish one complete file. Cancellation
/// before publication preserves the previous file and never overwrites a source.
pub(crate) fn atomic_output(
    output: &Path,
    source: &Path,
    cancel: &AtomicBool,
    produce: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    ensure!(
        output.is_absolute() && output.parent().is_some_and(Path::is_dir),
        "Choose a file in an existing output directory"
    );
    ensure!(
        output != source,
        "Export cannot replace the source schematic"
    );
    if output.exists() && source.exists() {
        ensure!(
            std::fs::canonicalize(output)? != std::fs::canonicalize(source)?,
            "Export cannot replace the source schematic"
        );
    }
    ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
    let temporary = output.with_extension(format!("{:032x}.export.tmp", rand::random::<u128>()));
    let result = (|| -> Result<()> {
        produce(&temporary)?;
        ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let from = temporary
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let to = output
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            ensure!(
                unsafe {
                    MoveFileExW(
                        from.as_ptr(),
                        to.as_ptr(),
                        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                    )
                } != 0,
                "{}",
                std::io::Error::last_os_error()
            );
        }
        #[cfg(not(windows))]
        std::fs::rename(&temporary, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn add_count(counts: &mut BTreeMap<String, u64>, id: &str, count: u64) -> Result<()> {
    let value = counts.entry(id.to_owned()).or_default();
    *value = value.checked_add(count).context("末端原料数量溢出")?;
    Ok(())
}

fn add_terminal(
    node: &recipe_tree::RecipeTreeNode,
    counts: &mut BTreeMap<String, u64>,
) -> Result<()> {
    if node.children.is_empty() {
        if node.tag.is_some()
            || node.item_id.contains('|')
            || (node.unresolved && node.unresolved_reason.as_deref() != Some("no_recipe"))
        {
            bail!(
                "配方末端 {} 存在未确定标签、特殊配方、循环或深度限制；请先确定配方再导出末端原料",
                node.item_id
            );
        }
        add_count(counts, &node.item_id, node.needed_count)
    } else {
        for child in &node.children {
            add_terminal(child, counts)?;
        }
        Ok(())
    }
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn svg(materials: &stockpile::StockpileMaterialsData, context: &str, english: bool) -> String {
    let (title, name, item_id, quantity) = if english {
        ("Material List", "Name", "Item ID", "Quantity")
    } else {
        ("材料列表", "名称", "物品 ID", "数量")
    };
    let height = 108 + materials.materials.len() * 32;
    let mut result = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"{height}\" viewBox=\"0 0 1000 {height}\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/><g font-family=\"Microsoft YaHei,sans-serif\" fill=\"#111\"><text x=\"24\" y=\"32\" font-size=\"22\">{title}</text><text x=\"24\" y=\"58\" font-size=\"12\">{}</text><text x=\"24\" y=\"90\" font-size=\"15\">{name}</text><text x=\"400\" y=\"90\" font-size=\"15\">{item_id}</text><text x=\"960\" y=\"90\" font-size=\"15\" text-anchor=\"end\">{quantity}</text>",
        xml(context)
    );
    for (index, row) in materials.materials.iter().enumerate() {
        let y = 122 + index * 32;
        result.push_str(&format!("<text x=\"24\" y=\"{y}\" font-size=\"14\">{}</text><text x=\"400\" y=\"{y}\" font-size=\"12\">{}</text><text x=\"960\" y=\"{y}\" font-size=\"14\" text-anchor=\"end\">{}</text>", xml(&row.display_name), xml(&row.namespace_id), row.required_count));
    }
    result.push_str("</g></svg>");
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_export_static_copy_uses_txt_renderer_without_file() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/stats_water_fixture.litematic");
        let stats = stats_api::build_stats_output(&source, false).unwrap();
        let mut input = json!({"action":"export","sourcePath":source,"uiLocale":"en-US","settings":{"source":"materials","format":"copy","multiplier":2}});
        let mut copy = configuration(&input, Some(&stats), None).unwrap();
        assert_eq!(copy["settings"]["format"], "copy");
        assert_eq!(copy["copy"], true);
        export_snapshot(&input, stats.clone(), &mut copy, &AtomicBool::new(false)).unwrap();
        assert!(
            copy["exportText"]
                .as_str()
                .is_some_and(|text| text.contains("Material List"))
        );
        assert!(copy.get("outputPath").is_none());
        assert_eq!(copy["summary"]["output"], "");
        let output = std::env::temp_dir().join(format!(
            "nova-copy-equality-{:032x}.txt",
            rand::random::<u128>()
        ));
        input["settings"]["format"] = "txt".into();
        input["outputPath"] = output.display().to_string().into();
        let mut file = configuration(&input, Some(&stats), None).unwrap();
        export_snapshot(&input, stats, &mut file, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&output).unwrap(),
            copy["exportText"].as_str().unwrap()
        );
        std::fs::remove_file(output).unwrap();
        let task_context =
            json!({"minecraftVersion":"26.3","sourcePath":source,"individualCounts":true});
        let task = configuration(
            &json!({"settings":{"source":"task"}}),
            None,
            Some(&task_context),
        )
        .unwrap();
        assert_eq!(
            task["settings"]["individualCounts"], true,
            "task context exposes saved preferences at the top level"
        );
    }

    #[test]
    fn material_export_configuration_restores_history_and_backend_dependencies() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/stats_water_fixture.litematic");
        let stats = stats_api::build_stats_output(&source, false).unwrap();
        let input = json!({"action":"configure","sourcePath":source,"includeContainerItems":true,"settings":{"format":"html","webMode":"multi","multiplier":0}});
        let view = configuration(&input, Some(&stats), None).unwrap();
        let keys = view["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f["key"].as_str())
            .collect::<Vec<_>>();
        for key in [
            "scope",
            "region",
            "layer",
            "multiplier",
            "includeContainerItems",
            "format",
            "terminalRawMaterials",
            "webMode",
            "accessPassword",
            "adminPassword",
            "whitelist",
            "whitelistEnabled",
            "allowGuestReadonly",
            "adminPageEnabled",
            "targets",
        ] {
            assert!(keys.contains(&key), "missing historical option {key}");
        }
        assert!(!view["valid"].as_bool().unwrap());
        assert!(view["errors"].as_array().unwrap().len() >= 2);
        assert_eq!(view["extension"], "zip");
        assert!(
            view["fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["key"] == "accessPassword")
                .unwrap()["visible"]
                .as_bool()
                .unwrap()
        );
        let defaults=configuration(&json!({"action":"reset","sourcePath":source,"settings":{"multiplier":99,"accessPassword":"not-persisted"}}),Some(&stats),None).unwrap();
        assert_eq!(defaults["settings"]["multiplier"], 1);
        assert_eq!(defaults["settings"]["format"], "svg");
        assert_eq!(defaults["settings"]["adminPageEnabled"], true);
        assert_eq!(defaults["settings"]["accessPassword"], "");
        assert_eq!(
            split_whitelist("Alice, Bob；Carol\nDan，Eve;Fred"),
            ["Alice", "Bob", "Carol", "Dan", "Eve", "Fred"]
        );
    }

    #[test]
    fn material_export_real_snapshot_formats_and_atomic_cancel_preserve_source() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/stats_water_fixture.litematic");
        let stats = stats_api::build_stats_output(&source, false).unwrap();
        let root = std::env::temp_dir().join(format!(
            "nova-material-export-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&root).unwrap();
        for (format, suffix) in [
            (MaterialExportFormat::Svg, "svg"),
            (MaterialExportFormat::Txt, "txt"),
            (MaterialExportFormat::Csv, "csv"),
            (MaterialExportFormat::Html, "html"),
        ] {
            let output = root.join(format!("materials.{suffix}"));
            let summary = export(
                MaterialExportRequest {
                    ui_locale: Some("en-US".into()),
                    language_map: BTreeMap::new(),
                    source_path: source.display().to_string(),
                    stats: stats.clone(),
                    multiplier: 2,
                    minecraft_version: crate::game_data::active_context().unwrap().game_version,
                    terminal_raw_materials: false,
                    format,
                    web_options: None,
                },
                &output,
            )
            .unwrap();
            assert_eq!(summary.material_count, stats.material_items.len());
            let text = std::fs::read_to_string(&output).unwrap();
            assert!(!text.is_empty());
            if suffix == "html" {
                assert!(text.contains("window.__STOCKPILE_DATA__"));
                assert!(!text.contains("src=\"assets/app.js\""));
            }
            for row in &stats.material_items {
                assert!(text.contains(&row.block_id));
                assert!(text.contains(&(row.total_count * 2).to_string()));
            }
        }
        let output = root.join("cancelled.txt");
        std::fs::write(&output, "preserved").unwrap();
        let cancel = AtomicBool::new(false);
        assert!(
            atomic_output(&output, &source, &cancel, |temporary| {
                std::fs::write(temporary, "partial")?;
                cancel.store(true, Ordering::Relaxed);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "preserved");
        assert!(
            !std::fs::read_dir(&root).unwrap().any(|e| e
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "tmp"))
        );
        assert!(atomic_output(&source, &source, &AtomicBool::new(false), |_| Ok(())).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn multiplayer_web_options_reuse_stockpile_deployment_contract() {
        let options: MaterialWebOptions = serde_json::from_value(serde_json::json!({
            "mode":"multi", "accessPassword":"fixture-access", "adminPassword":"fixture-admin",
            "whitelist":["FixturePlayer"], "whitelistEnabled":true, "allowGuestReadonly":true,
            "adminPageEnabled":false, "targets":["windows-x64","linux-x64"]
        }))
        .unwrap();
        assert!(options.mode == MaterialWebMode::Multi);
        let deployed = options.deploy_options().unwrap();
        assert!(deployed.access_password.is_some() && deployed.admin_password.is_some());
        assert_eq!(deployed.whitelist, ["FixturePlayer"]);
        assert_eq!(deployed.whitelist_enabled, Some(true));
        assert_eq!(deployed.allow_guest_readonly, Some(true));
        assert_eq!(deployed.admin_page_enabled, Some(false));
        assert_eq!(
            deployed.targets,
            [
                stockpile_zip::StockpileDeployTarget::WindowsX64,
                stockpile_zip::StockpileDeployTarget::LinuxX64
            ]
        );
        assert!(
            MaterialWebOptions {
                targets: vec!["not-a-platform".into()],
                ..Default::default()
            }
            .deploy_options()
            .is_err()
        );
    }

    #[test]
    #[ignore = "CLI real snapshot export; requires an existing actual Windows Stockpile server, no reanalysis"]
    fn real_snapshot_multiplayer_web_package() {
        use std::io::Read;
        let snapshot = std::env::var("NOVA_STATS_SNAPSHOT").unwrap();
        let source = std::env::var("NOVA_STATS_FIXTURE").unwrap();
        let stats: StableStructureOutput =
            serde_json::from_reader(std::fs::File::open(&snapshot).unwrap()).unwrap();
        let directory = Path::new(&snapshot).parent().unwrap();
        let output = directory.join("840FTL-materials-2x-multi.zip");
        let original_rows = stats.material_items.clone();
        export(
            MaterialExportRequest {
                ui_locale: None,
                language_map: BTreeMap::new(),
                source_path: source,
                stats,
                multiplier: 2,
                minecraft_version: "26.3".into(),
                terminal_raw_materials: false,
                format: MaterialExportFormat::Html,
                web_options: Some(MaterialWebOptions {
                    mode: MaterialWebMode::Multi,
                    access_password: Some("fixture-access".into()),
                    admin_password: Some("fixture-admin".into()),
                    whitelist: vec!["FixturePlayer".into()],
                    whitelist_enabled: Some(true),
                    allow_guest_readonly: Some(true),
                    admin_page_enabled: Some(false),
                    targets: vec!["windows-x64".into()],
                }),
            },
            &output,
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&output).unwrap()).unwrap();
        let mut text = String::new();
        archive
            .by_name("data/materials.json")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(payload["materials"].as_array().unwrap().len(), 59);
        for row in original_rows {
            let item = payload["materials"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["namespace_id"] == row.block_id)
                .unwrap();
            assert_eq!(item["required_count"], row.total_count * 2);
        }
        assert!(
            archive
                .by_name("server/windows-x64/stockpile_server.exe")
                .unwrap()
                .size()
                > 1_000_000
        );
        let database = directory.join("840FTL-materials-2x-multi-auth.sqlite");
        {
            let mut file = std::fs::File::create(&database).unwrap();
            std::io::copy(
                &mut archive.by_name("db/stockpile.sqlite").unwrap(),
                &mut file,
            )
            .unwrap();
        }
        let db = rusqlite::Connection::open(&database).unwrap();
        let values:(i64,i64,i64,i64,i64) = db.query_row("SELECT access_password_enabled, admin_password_enabled, whitelist_enabled, allow_guest_readonly, admin_page_enabled FROM stockpile_config WHERE id=1",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap();
        assert_eq!(values, (1, 1, 1, 1, 0));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM password_hashes", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row("SELECT user_id FROM stockpile_whitelist", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "FixturePlayer"
        );
    }
    #[test]
    fn terminal_counts_reuse_existing_recipe_tree_rounding() {
        let recipes = BTreeMap::from([(
            "planks".into(),
            serde_json::json!({"type":"minecraft:crafting_shapeless","ingredients":[{"item":"minecraft:oak_log"}],"result":{"id":"minecraft:oak_planks","count":4}}),
        )]);
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/stats_water_fixture.litematic");
        let mut stats = stats_api::build_stats_output(&fixture, false).unwrap();
        stats.material_items =
            vec![stats_api::inventory_requirement("minecraft:oak_planks".into(), 5).unwrap()];
        let mut materials =
            stockpile::build_materials_data_from_stats(&fixture, stats, "26.3").unwrap();
        materials.materials[0].recipe_status = "available".into();
        let trees = recipe_tree::resolve_recipe_trees_from_recipes(&recipes, &materials);
        let mut counts = BTreeMap::new();
        add_terminal(&trees["minecraft:oak_planks"], &mut counts).unwrap();
        assert_eq!(counts["minecraft:oak_log"], 2);
        materials.materials[0].required_count = 10;
        let trees = recipe_tree::resolve_recipe_trees_from_recipes(&recipes, &materials);
        let mut counts = BTreeMap::new();
        add_terminal(&trees["minecraft:oak_planks"], &mut counts).unwrap();
        assert_eq!(
            counts["minecraft:oak_log"], 3,
            "scale quantities before recipe batch rounding"
        );
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
        let stats: StableStructureOutput =
            serde_json::from_reader(std::fs::File::open(&snapshot).unwrap()).unwrap();
        assert_eq!(stats.structure_stats.total_non_air_blocks, 88534);
        assert_eq!(stats.material_items.len(), 59);
        let directory = Path::new(&snapshot).parent().unwrap();
        for (extension, format) in [
            ("svg", MaterialExportFormat::Svg),
            ("txt", MaterialExportFormat::Txt),
            ("html", MaterialExportFormat::Html),
        ] {
            let path = directory.join(format!("840FTL-materials-2x.{extension}"));
            let summary = export(
                MaterialExportRequest {
                    ui_locale: None,
                    language_map: BTreeMap::new(),
                    source_path: source.clone(),
                    stats: stats.clone(),
                    multiplier: 2,
                    minecraft_version: "26.3".into(),
                    terminal_raw_materials: false,
                    format,
                    web_options: None,
                },
                &path,
            )
            .unwrap();
            assert_eq!(summary.material_count, 59);
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("材料列表") || extension == "html");
            if extension == "html" {
                assert!(!content.contains("src=\"assets/app.js\""));
                assert!(!content.contains("href=\"assets/app.css\""));
                assert!(content.contains("data:image/png;base64,"));
                let embedded = content
                    .split("window.__STOCKPILE_DATA__ = ")
                    .nth(1)
                    .unwrap()
                    .split(";</script>")
                    .next()
                    .unwrap();
                let payload: serde_json::Value = serde_json::from_str(embedded).unwrap();
                for row in &stats.material_items {
                    let actual = payload["materials"]["materials"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|actual| actual["namespace_id"] == row.block_id)
                        .unwrap();
                    assert_eq!(actual["required_count"], row.total_count * 2);
                }
            }
            println!(
                "MATERIAL_EXPORT format={extension} rows={} multiplier=2 bytes={} path={}",
                summary.material_count,
                content.len(),
                path.display()
            );
        }
    }
}
