//! A supervised, process-local data execution boundary. No rendering or remote service.
use crate::{
    data_worker_protocol::{self as wire, DataKind, DataRequest, DataResponse, DataState},
    stats_api::StableStructureOutput,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Read,
    net::TcpStream,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Instant, SystemTime},
};

// Retain one large parsed projection without decoding it again on each query.
// This bounds cache retention, not the amount of data a task may process.
const ANALYSIS_CACHE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceKey {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}
impl SourceKey {
    fn read(path: &Path) -> Result<Self> {
        let path = fs::canonicalize(path)?;
        let m = fs::metadata(&path)?;
        ensure!(m.is_file(), "Projection input is not a file");
        Ok(Self {
            path,
            bytes: m.len(),
            modified: m.modified()?,
        })
    }
    fn revision(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                format!("{:?}\0{}\0{:?}", self.path, self.bytes, self.modified).as_bytes()
            )
        )
    }
    fn legacy_revision(&self, containers: bool) -> String {
        let mut args = vec![
            "stats".to_owned(),
            "--input".into(),
            self.path.to_string_lossy().into_owned(),
            "--json".into(),
        ];
        if containers {
            args.push("--include-container-items".into());
        }
        format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{:?}\0{}\0{:?}\0{:?}",
                    self.path, self.bytes, self.modified, args
                )
                .as_bytes()
            )
        )
    }
}
struct CachedAnalysis {
    key: SourceKey,
    resource_id: String,
    root: Arc<crate::nbt::LitematicRoot>,
    analysis: Option<Arc<StableStructureOutput>>,
    blocks: Arc<StableStructureOutput>,
    content_id: String,
    weight: usize,
}
#[derive(Default)]
struct AnalysisCache {
    entries: VecDeque<CachedAnalysis>,
}
impl AnalysisCache {
    fn finish(
        entry: &mut CachedAnalysis,
        containers: bool,
        cancel: &AtomicBool,
    ) -> Result<Arc<StableStructureOutput>> {
        if !containers {
            return Ok(entry.blocks.clone());
        }
        if let Some(analysis) = entry.analysis.as_ref() {
            return Ok(analysis.clone());
        }
        let started = Instant::now();
        let analysis = Arc::new(crate::stats_api::add_container_items_to_stats(
            &entry.root,
            &entry.blocks,
            &|| cancel.load(Ordering::Relaxed),
        )?);
        eprintln!(
            "[NOVA_DATA_TIMING] stage=container_delta elapsed_ms={:.3} containers={} stacks={}",
            started.elapsed().as_secs_f64() * 1000.,
            analysis.container_scan.containers_scanned,
            analysis.container_scan.item_stacks_scanned
        );
        entry.analysis = Some(analysis.clone());
        Ok(analysis)
    }
    fn get(
        &mut self,
        path: &Path,
        containers: bool,
        cancel: &AtomicBool,
    ) -> Result<(Arc<StableStructureOutput>, String, String, bool)> {
        let key = SourceKey::read(path)?;
        let resource_id = crate::game_data::active_context()?.content_id;
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.key == key && e.resource_id == resource_id)
        {
            let hit = !containers || entry.analysis.is_some();
            return Ok((
                Self::finish(entry, containers, cancel)?,
                key.revision(),
                entry.content_id.clone(),
                hit,
            ));
        }
        ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
        let start = Instant::now();
        let root = Arc::new(crate::nbt::load_litematic_root(path)?);
        let decode_ms = start.elapsed().as_secs_f64() * 1000.;
        let blocks = Arc::new(crate::stats_api::build_stats_output_from_root_cancellable(
            &root,
            crate::stats_api::MaterialScope::All,
            false,
            true,
            &|| cancel.load(Ordering::Relaxed),
        )?);
        ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
        let mut file = fs::File::open(path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        ensure!(
            SourceKey::read(path)? == key,
            "Projection changed while statistics were being prepared"
        );
        let content_id = format!("{:x}", hash.finalize());
        let weight = root
            .regions
            .values()
            .map(|region| region.block_states.len().saturating_mul(8))
            .sum::<usize>()
            .saturating_add((key.bytes as usize).saturating_mul(8));
        let mut entry = CachedAnalysis {
            key: key.clone(),
            resource_id,
            root,
            analysis: None,
            blocks,
            content_id: content_id.clone(),
            weight,
        };
        let analysis = Self::finish(&mut entry, containers, cancel)?;
        eprintln!(
            "[NOVA_DATA_TIMING] stage=statistics cold=true decode_ms={decode_ms:.3} total_ms={:.3} blocks={} containers={} item_stacks={}",
            start.elapsed().as_secs_f64() * 1000.,
            analysis.structure_stats.total_non_air_blocks,
            analysis.container_scan.containers_scanned,
            analysis.container_scan.item_stacks_scanned
        );
        self.entries.retain(|e| e.key.path != key.path);
        // Retention limits are not task admission or performance deadlines.
        if weight <= ANALYSIS_CACHE_BYTES {
            while self.entries.len() >= 2
                || self
                    .entries
                    .iter()
                    .map(|e| e.weight)
                    .sum::<usize>()
                    .saturating_add(weight)
                    > ANALYSIS_CACHE_BYTES
            {
                self.entries.pop_front();
            }
            self.entries.push_back(entry);
        }
        Ok((analysis, key.revision(), content_id, false))
    }
}

struct Engine {
    analysis: Mutex<AnalysisCache>,
    layers: crate::layer_view::LayerService,
    conversion: Mutex<crate::version_conversion::ConversionCache>,
    recipe_resources_ready: AtomicBool,
}
impl Engine {
    fn new() -> Self {
        Self {
            analysis: Mutex::new(AnalysisCache::default()),
            layers: crate::layer_view::LayerService::default(),
            conversion: Mutex::new(crate::version_conversion::ConversionCache::new(
                ANALYSIS_CACHE_BYTES,
            )),
            recipe_resources_ready: AtomicBool::new(false),
        }
    }
    fn analysis(
        &self,
        input: &Value,
        cancel: &AtomicBool,
    ) -> Result<(Arc<StableStructureOutput>, String, String, bool)> {
        let path = Path::new(
            input["sourcePath"]
                .as_str()
                .context("A source path is required")?,
        );
        self.analysis
            .lock()
            .map_err(|_| anyhow::anyhow!("Statistics cache lock failed"))?
            .get(
                path,
                input["includeContainerItems"].as_bool() == Some(true),
                cancel,
            )
    }
    fn snapshot(
        &self,
        input: &Value,
        cancel: &AtomicBool,
    ) -> Result<(Value, String, String, bool)> {
        let (analysis, revision, content, hit) = self.analysis(input, cancel)?;
        let mut value = serde_json::to_value(&*analysis)?;
        if let Some(scope) = input.get("scope").filter(|v| !v.is_null()) {
            value = crate::data_scope::material_snapshot_for_scope(
                value,
                &serde_json::from_value(scope.clone())?,
            )
            .map_err(anyhow::Error::msg)?;
        }
        Ok((value, revision, content, hit))
    }
    fn execute(
        &self,
        req: &DataRequest,
        cancel: &AtomicBool,
        progress: impl Fn(Value),
    ) -> Result<Value> {
        ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
        // Embedded preview bytes do not depend on selected game/model resources.
        let _context = if req.kind == DataKind::Thumbnail {
            None
        } else {
            Some(crate::game_data::operation_context()?)
        };
        progress(json!({"stage":"prepare","current":0,"total":1}));
        match req.kind {
            DataKind::Composition => crate::composition::execute(&req.input, cancel),
            DataKind::LayerView => self.layers.execute(&req.input, cancel),
            DataKind::Thumbnail => crate::projection_preview::read_cached(&req.input, cancel),
            DataKind::ReplaceRules => crate::replace_blocks::editor_view(&req.input),
            DataKind::Format => crate::schematic_formats::execute(&req.input, cancel),
            DataKind::Workspace => crate::workspace::recent_projects(std::path::Path::new(
                req.input["storageDir"]
                    .as_str()
                    .context("Workspace storage directory is missing")?,
            )),
            DataKind::Analysis => {
                let (mut value, revision, _, _) = self.snapshot(&req.input, cancel)?;
                value["source_revision"] = revision.into();
                Ok(value)
            }
            DataKind::Stats => {
                let (analysis, revision, _, hit) = self.analysis(&req.input, cancel)?;
                let view = crate::data_view::statistics_view_localized(
                    &analysis,
                    req.input["uiLocale"].as_str().unwrap_or("zh-CN"),
                );
                let mut result = json!({"source_revision":revision,"material_semantics":analysis.material_semantics,"statisticsView":view,"timing":{"cacheReused":hit}});
                if let Some(scan) = result
                    .pointer_mut("/statisticsView/containerScan")
                    .and_then(Value::as_object_mut)
                {
                    scan.remove("scope_entries");
                }
                if req.input["includeStructuralBlocks"].as_bool() == Some(true) {
                    result["structural_block_counts"] =
                        serde_json::to_value(&analysis.structural_block_counts)?;
                }
                Ok(result)
            }
            DataKind::Materials => {
                let (analysis, revision, _, _) = self.analysis(&req.input, cancel)?;
                let mut result = serde_json::to_value(crate::data_view::catalog_query(
                    &analysis,
                    &serde_json::from_value(req.input.clone())?,
                )?)?;
                result["sourceRevision"] = revision.into();
                Ok(result)
            }
            DataKind::MaterialDetail => {
                let mut input = req.input.clone();
                if input.get("sourcePath").is_some() {
                    let (analysis, _, _, _) = self.analysis(&input, cancel)?;
                    let id = input["materialId"].as_str().context("Select a material")?;
                    let row = analysis
                        .material_items
                        .iter()
                        .find(|row| row.item_identity.as_deref().unwrap_or(&row.block_id) == id)
                        .context("Material identity no longer exists")?;
                    input["material"] = serde_json::to_value(row)?;
                    input["materialSemantics"] = analysis.material_semantics.clone().into();
                    input["dataVersion"] = analysis.metadata.minecraft_data_version.into();
                    input["sourceMinY"] = analysis
                        .layers_summary
                        .first()
                        .map_or(0, |row| row.world_y - row.layer)
                        .into();
                }
                for key in ["sourcePath", "materialId", "includeContainerItems", "scope"] {
                    input.as_object_mut().unwrap().remove(key);
                }
                input
                    .as_object_mut()
                    .unwrap()
                    .retain(|_, value| !value.is_null());
                Ok(serde_json::to_value(crate::material_detail::read_cached(
                    serde_json::from_value(input)?,
                )?)?)
            }
            DataKind::MaterialRecipes => {
                // Resource registry/index construction is input-independent.
                // Overlap it with this request's real source read/count, never
                // with work hidden before the click or outside its timing.
                let (analysis, revision, _, _) =
                    if self.recipe_resources_ready.load(Ordering::Relaxed) {
                        self.analysis(&req.input, cancel)?
                    } else {
                        std::thread::scope(|scope| -> Result<_> {
                            let resources = std::thread::Builder::new()
                                .name("nova-recipe-resources".into())
                                .spawn_scoped(scope, || -> Result<()> {
                                    let _context = crate::game_data::operation_context()?;
                                    crate::block_database::database()?;
                                    ensure!(
                                        !cancel.load(Ordering::Relaxed),
                                        "Data operation cancelled"
                                    );
                                    crate::recipe_tree::prepare_selected_resolver()
                                })
                                .context("Start recipe resource preparation")?;
                            let analysis = self.analysis(&req.input, cancel);
                            let resources = resources
                                .join()
                                .map_err(|_| anyhow::anyhow!("Recipe resource preparation failed"));
                            let analysis = analysis?;
                            resources??;
                            self.recipe_resources_ready.store(true, Ordering::Relaxed);
                            Ok(analysis)
                        })?
                    };
                let language_map: BTreeMap<String, String> = serde_json::from_value(
                    req.input
                        .get("languageMap")
                        .filter(|v| !v.is_null())
                        .cloned()
                        .unwrap_or(json!({})),
                )?;
                let batch = crate::material_detail::read_all_cached(
                    &analysis,
                    &revision,
                    req.input["uiLocale"].as_str().unwrap_or("zh-CN"),
                    &language_map,
                    &|| cancel.load(Ordering::Relaxed),
                )?;
                Ok(json!({"summary":batch.summary,"sourceRevision":revision}))
            }
            DataKind::MaterialExport => self.material_export(req.input.clone(), cancel),
            DataKind::Stockpile => self.stockpile(req.input.clone(), cancel),
            DataKind::VersionConvert => {
                let input = &req.input;
                let source = Path::new(
                    input["sourcePath"]
                        .as_str()
                        .context("Source path is required")?,
                );
                let output = input["outputPath"].as_str().map(Path::new);
                let request = serde_json::from_value(
                    json!({"targetVersion":input["targetVersion"],"replacements":input.get("replacements").cloned().unwrap_or(json!({})),"expectedSourceSha256":input["expectedSourceSha256"],"overwrite":input["overwrite"].as_bool().unwrap_or(false),"selection":input["selection"],"propertyEdit":input["propertyEdit"]}),
                )?;
                let result = self
                    .conversion
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Version conversion cache lock failed"))?
                    .convert(source, output, &request, cancel, |stage| {
                        progress(serde_json::to_value(stage).unwrap_or(Value::Null))
                    })?;
                Ok(serde_json::to_value(result)?)
            }
            DataKind::StatisticsRules => {
                let (value, _, _, _) = self.snapshot(&req.input, cancel)?;
                crate::statistics_rules::query(&value, &req.input)
            }
            DataKind::Cancel => bail!("Cancellation is a control message"),
        }
    }
    fn material_export(&self, mut input: Value, cancel: &AtomicBool) -> Result<Value> {
        let action = input["action"].as_str().unwrap_or("configure").to_owned();
        ensure!(
            ["configure", "export", "reset"].contains(&action.as_str()),
            "Unknown export action"
        );
        if action == "reset" {
            input
                .as_object_mut()
                .context("Invalid export input")?
                .remove("settings");
        }
        if crate::material_export::is_task_export(&input) {
            let context = crate::stockpile_serve::material_export_context(&input)?;
            let mut view = crate::material_export::configuration(&input, None, Some(&context))?;
            view["sourceRevision"] = context["sourceRevision"].clone();
            if action == "export" {
                crate::stockpile_serve::material_export_task(&input, &mut view, cancel)?;
            }
            return Ok(view);
        }
        if let Some(include) = input
            .pointer("/settings/includeContainerItems")
            .and_then(Value::as_bool)
        {
            input["includeContainerItems"] = include.into();
        }
        let (analysis, revision, _, hit) = self.analysis(&input, cancel)?;
        // A static projection export may also offer a saved task as another
        // source. Read its preferences without applying the projection's file
        // revision to that independent task identity.
        let task_context = if input["taskId"].is_string() {
            Some(crate::stockpile_serve::material_export_context(
                &json!({"taskId":input["taskId"]}),
            )?)
        } else {
            None
        };
        let mut view =
            crate::material_export::configuration(&input, Some(&analysis), task_context.as_ref())?;
        view["sourceRevision"] = revision.clone().into();
        view["cacheReused"] = hit.into();
        if action == "export" {
            if let Some(expected) = input["sourceRevision"].as_str().filter(|s| !s.is_empty()) {
                ensure!(
                    expected == revision,
                    "Projection changed; reopen export settings"
                );
            }
            ensure!(view["valid"].as_bool() == Some(true), "{}", view["errors"]);
            let settings = &view["settings"];
            let snapshot = if settings["scope"].as_str() == Some("all") {
                (*analysis).clone()
            } else {
                let kind = settings["scope"].as_str().unwrap_or("all");
                let value = if kind == "region" {
                    settings["region"].as_str().unwrap_or_default().to_owned()
                } else {
                    settings["layer"].as_i64().unwrap_or(0).to_string()
                };
                serde_json::from_value(
                    crate::data_scope::material_snapshot_for_scope(
                        serde_json::to_value(&*analysis)?,
                        &serde_json::from_value(json!({"kind":kind,"value":value}))?,
                    )
                    .map_err(anyhow::Error::msg)?,
                )?
            };
            crate::material_export::export_snapshot(&input, snapshot, &mut view, cancel)?;
        }
        Ok(view)
    }

    fn stockpile_snapshot(&self, input: &mut Value, cancel: &AtomicBool) -> Result<()> {
        let (snapshot, revision, content, _) = self.snapshot(input, cancel)?;
        let scope = input
            .get("scope")
            .filter(|value| !value.is_null())
            .cloned()
            .unwrap_or(json!({"kind":"all","value":null}));
        let resource = crate::game_data::active_context()?.content_id;
        input["context"] = json!({"scope":scope,"includeContainerItems":input["includeContainerItems"].as_bool().unwrap_or(false),
            "sourceContentId":content,"sourceFileRevision":revision,"resourceIdentity":resource});
        input["projectId"] = content.into();
        input["sourceRevision"] = revision.into();
        input["snapshot"] = snapshot;
        Ok(())
    }

    fn stockpile(&self, mut input: Value, cancel: &AtomicBool) -> Result<Value> {
        ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
        let action = input["action"].as_str().unwrap_or("open").to_owned();
        if matches!(action.as_str(), "open" | "select") && input["taskId"].is_string() {
            // Switching tasks does not relink any source to the projection
            // currently open in the editor. Source changes require confirmation.
            let command = input.as_object_mut().context("Invalid task command")?;
            command.remove("sourcePath");
            command.remove("projectId");
        }
        // Opening the workbench never creates a task or re-analyses a file.
        // The selected task is independent of the currently open projection.
        if action == "open" && input["taskId"].is_null() {
            let mut result = serde_json::to_value(
                crate::stockpile_serve::desktop_task_from_value(
                    json!({"action":"open","uiLocale":input["uiLocale"],"languageMap":input["languageMap"],"query":input["query"]}),
                )?,
            )?;
            stockpile_source_states(&mut result);
            return Ok(result);
        }
        let source_edit = matches!(action.as_str(), "setSources" | "addSources" | "sync");
        let saved = if source_edit && input["taskId"].is_string() {
            serde_json::to_value(crate::stockpile_serve::desktop_task_from_value(json!({
                "action":"query","taskId":input["taskId"],"query":{"offset":0,"limit":1}
            }))?)?["task"]
                .clone()
        } else {
            Value::Null
        };
        let saved_sources = saved["sources"].as_array().cloned().unwrap_or_default();
        let current_resource = if action == "sync" {
            Some(crate::game_data::active_context()?.content_id)
        } else {
            None
        };
        if action == "sync" && !saved_sources.is_empty() {
            let selected = input["sourceIds"].as_array();
            if let Some(ids) = selected {
                ensure!(
                    !ids.is_empty(),
                    "Select at least one projection to synchronize"
                );
                for id in ids {
                    ensure!(
                        saved_sources.iter().any(|source| source["sourceId"] == *id),
                        "The selected task source no longer exists"
                    );
                }
            }
            input["sources"] = Value::Array(
                saved_sources
                    .iter()
                    .filter(|source| selected.is_none_or(|ids| ids.contains(&source["sourceId"])))
                    .map(stockpile_source_input)
                    .collect(),
            );
        }
        if let Some(sources) = input["sources"].as_array_mut() {
            // Group each file's scopes together, retaining one parsed source;
            // copies never multiply parsing or container scans.
            let mut order: Vec<_> = (0..sources.len()).collect();
            order.sort_by_key(|index| {
                sources[*index]["sourcePath"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            });
            for index in order {
                ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
                let requested = stockpile_source_input(&sources[index]);
                let path = requested["sourcePath"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .context("A task source path is required")?;
                let previous = saved_sources.iter().find(|source| {
                    let same_id = requested["sourceId"].is_string()
                        && source["sourceId"] == requested["sourceId"];
                    let same_path = stockpile_same_path(
                        source["sourcePath"].as_str().unwrap_or_default(),
                        path,
                    );
                    (same_id || same_path)
                        && same_path
                        && stockpile_scope(source) == stockpile_scope(&requested)
                        && source["includeContainerItems"].as_bool().unwrap_or(false)
                            == requested["includeContainerItems"]
                                .as_bool()
                                .unwrap_or(false)
                });
                let reuse = previous.is_some_and(|source| {
                    action != "sync"
                        || stockpile_source_status(source) == "current"
                            && source["resourceIdentity"].as_str() == current_resource.as_deref()
                });
                sources[index] = requested;
                if reuse {
                    sources[index]["sourceId"] = previous.unwrap()["sourceId"].clone();
                } else {
                    self.stockpile_snapshot(&mut sources[index], cancel)?;
                }
            }
        } else if action == "bindLegacy"
            || action == "sync"
            || action == "create" && input["taskId"].is_null()
        {
            self.stockpile_snapshot(&mut input, cancel)?;
        }
        if input["projectId"].is_null() {
            input.as_object_mut().unwrap().remove("projectId");
        }
        for field in ["scope", "includeContainerItems"] {
            input
                .as_object_mut()
                .context("Invalid task command")?
                .remove(field);
        }
        ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
        let mut result =
            serde_json::to_value(crate::stockpile_serve::desktop_task_from_value(input)?)?;
        stockpile_source_states(&mut result);
        Ok(result)
    }
}

fn stockpile_scope(source: &Value) -> Value {
    let scope = source
        .get("scope")
        .filter(|value| !value.is_null())
        .cloned()
        .or_else(|| source.pointer("/context/scope").cloned())
        .unwrap_or(json!({"kind":"all","value":null}));
    if scope["kind"] == "all" {
        json!({"kind":"all","value":null})
    } else if scope["kind"] == "layer" {
        match scope["value"]
            .as_str()
            .and_then(|value| value.parse::<i32>().ok())
        {
            Some(layer) => json!({"kind":"layer","value":layer.to_string()}),
            None => scope,
        }
    } else {
        scope
    }
}
fn stockpile_source_input(source: &Value) -> Value {
    json!({"sourceId":source["sourceId"],"sourcePath":source["sourcePath"],
        "scope":stockpile_scope(source),"includeContainerItems":source.get("includeContainerItems").cloned().unwrap_or(json!(false)),
        "copies":source.get("copies").cloned().unwrap_or(json!(1))})
}
fn stockpile_same_path(left: &str, right: &str) -> bool {
    if let (Ok(left), Ok(right)) = (fs::canonicalize(left), fs::canonicalize(right)) {
        return left == right;
    }
    if cfg!(windows) {
        left.trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .eq_ignore_ascii_case(&right.trim_start_matches(r"\\?\").replace('/', "\\"))
    } else {
        left == right
    }
}
fn stockpile_source_status(source: &Value) -> &'static str {
    let Some(path) = source["sourcePath"].as_str() else {
        return "missing";
    };
    let Ok(key) = SourceKey::read(Path::new(path)) else {
        return "missing";
    };
    let saved = source
        .pointer("/context/sourceFileRevision")
        .and_then(Value::as_str)
        .or_else(|| source["sourceRevision"].as_str());
    let containers = source["includeContainerItems"].as_bool().unwrap_or(false)
        || source
            .pointer("/context/includeContainerItems")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if saved.is_some_and(|revision| {
        key.revision() != revision && key.legacy_revision(containers) != revision
    }) {
        "changed"
    } else {
        "current"
    }
}
fn stockpile_source_states(result: &mut Value) {
    if let Some(task) = result.get_mut("task").filter(|task| !task.is_null()) {
        let resource = crate::game_data::active_context()
            .ok()
            .map(|context| context.content_id);
        if let Some(sources) = task["sources"].as_array_mut() {
            for source in sources.iter_mut() {
                let mut status = stockpile_source_status(source);
                if status == "current"
                    && resource
                        .as_deref()
                        .zip(source["resourceIdentity"].as_str())
                        .is_some_and(|(current, saved)| current != saved)
                {
                    status = "incompatible";
                }
                source["status"] = status.into();
            }
            task["sourceStatus"] = ["missing", "changed", "incompatible"]
                .into_iter()
                .find(|status| sources.iter().any(|source| source["status"] == *status))
                .unwrap_or("current")
                .into();
        } else {
            task["sourceStatus"] = stockpile_source_status(task).into();
        }
    }
}

pub fn run() -> Result<()> {
    let config: Value = serde_json::from_str(&std::env::var("NOVA_DATA_WORKER_CONNECT")?)?;
    let mut stream = TcpStream::connect(
        config["address"]
            .as_str()
            .context("Missing worker address")?,
    )?;
    stream.set_nodelay(true)?;
    wire::write_frame(&mut stream, &config["nonce"])?;
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let engine = Arc::new(Engine::new());
    let cancellations = Arc::new(Mutex::new(BTreeMap::<String, Arc<AtomicBool>>::new()));
    let mut lanes = Vec::new();
    for _ in 0..4 {
        let (tx, rx) = mpsc::sync_channel::<(DataRequest, Arc<AtomicBool>)>(16);
        lanes.push(tx);
        let engine = engine.clone();
        let writer = writer.clone();
        let cancellations = cancellations.clone();
        std::thread::spawn(move || {
            while let Ok((request, cancel)) = rx.recv() {
                let send = |response: &DataResponse| {
                    if let Ok(mut stream) = writer.lock() {
                        let _ = wire::write_frame(&mut *stream, response);
                    }
                };
                send(&DataResponse::state(
                    &request.operation_id,
                    DataState::Running,
                ));
                let started = Instant::now();
                let result = engine.execute(&request, &cancel, |progress| {
                    let mut status = DataResponse::state(&request.operation_id, DataState::Running);
                    status.progress = Some(progress);
                    send(&status)
                });
                let mut response = DataResponse::state(
                    &request.operation_id,
                    if cancel.load(Ordering::Relaxed)
                        && !(result.is_ok() && request.kind.writes(&request.input))
                    {
                        DataState::Cancelled
                    } else if result.is_ok() {
                        DataState::Ready
                    } else {
                        DataState::Failed
                    },
                );
                match result {
                    Ok(result) if response.state == DataState::Ready => {
                        response.result = Some(result)
                    }
                    Err(error) => response.error = Some(format!("{error:#}")),
                    _ => {}
                };
                eprintln!(
                    "[NOVA_DATA_TIMING] operation={} kind={:?} total_ms={:.3} state={:?}",
                    request.operation_id,
                    request.kind,
                    started.elapsed().as_secs_f64() * 1000.,
                    response.state
                );
                send(&response);
                cancellations.lock().unwrap().remove(&request.operation_id);
            }
        });
    }
    loop {
        let request: DataRequest = match wire::read_frame(&mut stream) {
            Ok(request) => request,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if request.kind == DataKind::Cancel {
            if let Some(cancel) = cancellations.lock().unwrap().get(&request.operation_id) {
                cancel.store(true, Ordering::Relaxed);
            }
            continue;
        }
        // Supersede stale 2D views before they wait behind an older render in
        // the data lane. This only updates in-memory cancellation identities.
        if request.kind == DataKind::LayerView {
            engine.layers.prepare(&request.input);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let mut active = cancellations.lock().unwrap();
        ensure!(
            !active.contains_key(&request.operation_id),
            "Duplicate active data operation"
        );
        active.insert(request.operation_id.clone(), cancel.clone());
        drop(active);
        let index = match request.kind {
            DataKind::VersionConvert => 2,
            DataKind::Stockpile => 1,
            DataKind::Stats | DataKind::Analysis => 0,
            _ => 3,
        };
        let id = request.operation_id.clone();
        let pending = DataResponse::state(&id, DataState::Pending);
        wire::write_frame(&mut *writer.lock().unwrap(), &pending)?;
        if lanes[index].try_send((request, cancel)).is_err() {
            cancellations.lock().unwrap().remove(&id);
            let mut failed = DataResponse::state(&id, DataState::Failed);
            failed.error = Some("Data queue is full; retry after current work completes".into());
            wire::write_frame(&mut *writer.lock().unwrap(), &failed)?;
        }
    }
    for cancel in cancellations.lock().unwrap().values() {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[cfg(test)]
mod stockpile_worker_tests {
    use super::*;

    #[test]
    fn stockpile_source_binding_and_file_status_use_saved_identity() {
        let path = std::env::temp_dir().join(format!(
            "nova-stockpile-status-{}-{}.bin",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, b"first").unwrap();
        let key = SourceKey::read(&path).unwrap();
        let source = json!({"sourcePath":path,"sourceId":"stable","copies":4,"scope":{"kind":"all"},"context":{"sourceFileRevision":key.revision()}});
        assert_eq!(stockpile_source_status(&source), "current");
        assert_eq!(stockpile_scope(&source), json!({"kind":"all","value":null}));
        assert_eq!(stockpile_source_input(&source)["copies"], 4);
        fs::write(&path, b"changed input content").unwrap();
        assert_eq!(stockpile_source_status(&source), "changed");
        fs::remove_file(&path).unwrap();
        assert_eq!(stockpile_source_status(&source), "missing");
        assert!(stockpile_same_path(
            path.to_str().unwrap(),
            path.to_str().unwrap()
        ));
        let invalid = stockpile_source_input(
            &json!({"sourcePath":"fixture","copies":-1,"scope":{"kind":"all"}}),
        );
        assert_eq!(
            invalid["copies"], -1,
            "invalid quantities must reach validation, not silently become one"
        );
    }

    #[test]
    #[ignore = "set LBA_DATA_ROOT to an isolated test directory; writes real task SQLite and generated small projections"]
    fn stockpile_worker_multisource_production_flow() -> Result<()> {
        let data = PathBuf::from(
            std::env::var_os(crate::runtime_paths::LBA_DATA_ROOT_ENV)
                .context("isolated LBA_DATA_ROOT is required")?,
        );
        ensure!(
            data.to_string_lossy().contains("multi-stockpile-cli"),
            "test data must use the isolated multi-stockpile-cli directory"
        );
        let root = data.join(format!(
            "inputs-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let make = |name: &str, count: i32| -> Result<PathBuf> {
            let output = root.join(format!("{name}.litematic"));
            let plan = root.join(format!("{name}.json"));
            fs::write(
                &plan,
                serde_json::to_vec(&json!({"version":1,"minecraft_data_version":3953,
                "regions":[{"name":name,"origin":[0,0,0],"size":[count,1,1],"operations":[{"type":"fill_box","from":[0,0,0],"to":[count-1,0,0],"block":{"name":"minecraft:stone"}}]}]}))?,
            )?;
            crate::generate_projection::generate_projection(&plan, Some(&output), false, true)?;
            Ok(output)
        };
        let a = make("A", 64)?;
        let b = make("B", 20)?;
        let engine = Engine::new();
        let cancel = AtomicBool::new(false);
        let source = |path: &Path, copies: u64| json!({"sourcePath":path,"copies":copies,"scope":{"kind":"all","value":null},"includeContainerItems":false});
        let create = json!({"action":"create","name":"Production multi-source fixture","requestId":format!("worker-create-{}-{}",std::process::id(),SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()),"sources":[source(&a,3),source(&b,2)]});
        let first = engine.stockpile(create.clone(), &cancel)?;
        let mut task = first["task"].clone();
        ensure!(
            task["requiredCount"] == 232 && task["sourceCount"] == 2 && task["totalCopies"] == 5,
            "64x3+20x2 must be 232: {task}"
        );
        ensure!(
            task["materials"]["materials"].as_array().unwrap().len() == 1,
            "stone must be one merged semantic row"
        );
        let replay = engine.stockpile(create, &cancel)?;
        ensure!(
            replay["task"]["taskId"] == task["taskId"],
            "create retry must reuse the same task"
        );
        let id = task["taskId"].clone();
        let material = task["items"][0]["materialId"].clone();
        let selected = engine.stockpile(json!({"action":"select","taskId":id,"projectId":task["projectId"],"sourcePath":root.join("unrelated-current-file.litematic")}), &cancel)?;
        ensure!(
            selected["task"]["sourcePath"] == task["sourcePath"],
            "task switching must not relink a saved projection to the editor's current file"
        );
        let a_id = task["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| {
                stockpile_same_path(source["sourcePath"].as_str().unwrap(), a.to_str().unwrap())
            })
            .context("A source missing")?["sourceId"]
            .clone();
        task=engine.stockpile(json!({"action":"mutate","taskId":id,"sourceRevision":task["sourceRevision"],"mutation":{"materialId":material,"quantity":100,"status":"done","publicNote":"north","storageLocation":"box 2"}}),&cancel)?["task"].clone();
        let mut sources = task["sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(stockpile_source_input)
            .collect::<Vec<_>>();
        let a_index = sources
            .iter()
            .position(|source| source["sourceId"] == a_id)
            .unwrap();
        sources[a_index]["copies"] = 4.into();
        task=engine.stockpile(json!({"action":"setSources","taskId":id,"expectedRevision":task["sourceRevision"],"sources":sources}),&cancel)?["task"].clone();
        ensure!(
            task["requiredCount"] == 296
                && task["preparedCount"] == 100
                && task["remainingCount"] == 196,
            "source editing must preserve prepared 100: {task}"
        );
        ensure!(
            engine.analysis.lock().unwrap().entries.len() == 2,
            "copies must not create extra parsed scenes"
        );
        make("A", 80)?;
        let changed =
            engine.stockpile(json!({"action":"select","taskId":id}), &cancel)?["task"].clone();
        ensure!(
            changed["requiredCount"] == 296
                && changed["sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|source| source["sourceId"] == a_id && source["status"] == "changed"),
            "source changes must not replace confirmed requirements"
        );
        task=engine.stockpile(json!({"action":"sync","taskId":id,"expectedRevision":changed["sourceRevision"],"sourceIds":[a_id]}),&cancel)?["task"].clone();
        ensure!(
            task["requiredCount"] == 360 && task["preparedCount"] == 100,
            "explicit source sync must update only A"
        );
        fs::rename(&a, root.join("A-moved.litematic"))?;
        fs::rename(&b, root.join("B-moved.litematic"))?;
        let fresh = Engine::new();
        let offline =
            fresh.stockpile(json!({"action":"select","taskId":id}), &cancel)?["task"].clone();
        ensure!(
            offline["requiredCount"] == 360
                && offline["sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|source| source["status"] == "missing"),
            "saved task must open without its source files"
        );
        let mut offline_sources = offline["sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(stockpile_source_input)
            .collect::<Vec<_>>();
        let a_index = offline_sources
            .iter()
            .position(|source| source["sourceId"] == a_id)
            .unwrap();
        offline_sources[a_index]["copies"] = 3.into();
        task=fresh.stockpile(json!({"action":"setSources","taskId":id,"expectedRevision":offline["sourceRevision"],"sources":offline_sources}),&cancel)?["task"].clone();
        ensure!(
            task["requiredCount"] == 280 && task["preparedCount"] == 100,
            "offline multiplier changes must reuse confirmed snapshots"
        );
        let before = task.clone();
        let failure=fresh.stockpile(json!({"action":"addSources","taskId":id,"expectedRevision":task["sourceRevision"],"sources":[source(&root.join("missing.litematic"),1)]}),&cancel);
        ensure!(failure.is_err(), "missing new source must fail explicitly");
        task = fresh.stockpile(json!({"action":"select","taskId":id}), &cancel)?["task"].clone();
        ensure!(
            task["sourceRevision"] == before["sourceRevision"] && task["preparedCount"] == 100,
            "failed edit must preserve the complete previous task"
        );
        let exported = fresh.stockpile(json!({"action":"export","taskId":id}), &cancel)?;
        let backup = root.join("multi-task-backup.json");
        fs::write(&backup, serde_json::to_vec(&exported["backup"])?)?;
        let restored =
            fresh.stockpile(json!({"action":"import","inputPath":backup}), &cancel)?["task"]
                .clone();
        ensure!(
            restored["requiredCount"] == 280
                && restored["preparedCount"] == 100
                && restored["sourceCount"] == 2,
            "backup restore must retain sources and progress while files are missing"
        );
        ensure!(
            restored["items"][0]["publicNote"] == "north"
                && restored["items"][0]["storageLocation"] == "box 2",
            "notes and location must survive source edits and backup restore"
        );
        let stopped = AtomicBool::new(true);
        ensure!(fresh.stockpile(json!({"action":"setSources","taskId":id,"expectedRevision":task["sourceRevision"],"sources":[]}),&stopped).is_err(),"cancelled source changes must not commit");
        println!(
            "MULTI_SOURCE_PRODUCTION_FLOW initial=232 edited=296 prepared=100 remaining=196 synced=360 offline=280 restored=280 sources=2"
        );
        Ok(())
    }
}
