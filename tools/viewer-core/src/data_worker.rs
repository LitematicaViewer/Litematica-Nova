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
        if let Some(entry) = self.entries.iter_mut().find(|e| e.key == key) {
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
    conversion: Mutex<crate::version_conversion::ConversionCache>,
    recipe_resources_ready: AtomicBool,
}
impl Engine {
    fn new() -> Self {
        Self {
            analysis: Mutex::new(AnalysisCache::default()),
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
        let _context = crate::game_data::operation_context()?;
        progress(json!({"stage":"prepare","current":0,"total":1}));
        match req.kind {
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
                let (analysis, revision, _, _) = if self.recipe_resources_ready.load(Ordering::Relaxed) {
                    self.analysis(&req.input, cancel)?
                } else {
                    std::thread::scope(|scope| -> Result<_> {
                        let resources = std::thread::Builder::new().name("nova-recipe-resources".into()).spawn_scoped(scope, || -> Result<()> {
                            let _context = crate::game_data::operation_context()?;
                            crate::block_database::database()?;
                            ensure!(!cancel.load(Ordering::Relaxed), "Data operation cancelled");
                            crate::recipe_tree::prepare_selected_resolver()
                        }).context("Start recipe resource preparation")?;
                        let analysis = self.analysis(&req.input, cancel);
                        let resources = resources.join().map_err(|_| anyhow::anyhow!("Recipe resource preparation failed"));
                        let analysis = analysis?;
                        resources??;
                        self.recipe_resources_ready.store(true, Ordering::Relaxed);
                        Ok(analysis)
                    })?
                };
                let language_map: BTreeMap<String,String> = serde_json::from_value(req.input.get("languageMap").filter(|v|!v.is_null()).cloned().unwrap_or(json!({})))?;
                let batch = crate::material_detail::read_all_cached(&analysis,&revision,req.input["uiLocale"].as_str().unwrap_or("zh-CN"),&language_map,&||cancel.load(Ordering::Relaxed))?;
                Ok(json!({"summary":batch.summary,"sourceRevision":revision}))
            },
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
    fn stockpile(&self, mut input: Value, cancel: &AtomicBool) -> Result<Value> {
        let action = input["action"].as_str().unwrap_or("open").to_owned();
        if action == "open" && input.get("taskId").is_none_or(Value::is_null) {
            let existing = crate::stockpile_serve::desktop_task_from_value(
                json!({"action":"open","sourcePath":input["sourcePath"],"uiLocale":input["uiLocale"],"languageMap":input["languageMap"],"query":input["query"]}),
            )?;
            let value = serde_json::to_value(existing)?;
            if value.get("task").is_some_and(|task| !task.is_null())
                || input["sourcePath"]
                    .as_str()
                    .is_none_or(|path| !Path::new(path).is_file())
            {
                return Ok(value);
            }
        }
        if action == "sync"
            || action == "bindLegacy"
            || action == "open"
            || action == "create" && input["taskId"].is_null()
        {
            let (snapshot, revision, content, _) = self.snapshot(&input, cancel)?;
            let scope = input
                .get("scope")
                .cloned()
                .unwrap_or(json!({"kind":"all","value":null}));
            input["context"] = json!({"scope":scope,"includeContainerItems":input["includeContainerItems"].as_bool().unwrap_or(false),"sourceContentId":content,"sourceFileRevision":revision});
            input["projectId"] = content.into();
            input["sourceRevision"] = revision.into();
            input["snapshot"] = snapshot;
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
        if let Some(task) = result.get_mut("task").filter(|task| !task.is_null()) {
            if let Some(path) = task["sourcePath"].as_str() {
                let source = Path::new(path);
                task["sourceStatus"] = if !source.is_file() {
                    "missing"
                } else if task
                    .pointer("/context/sourceFileRevision")
                    .and_then(Value::as_str)
                    .is_some_and(|revision| {
                        SourceKey::read(source).is_ok_and(|key| {
                            key.revision() != revision
                                && key.legacy_revision(
                                    task.pointer("/context/includeContainerItems")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false),
                                ) != revision
                        })
                    })
                {
                    "changed"
                } else {
                    "current"
                }
                .into();
            }
        }
        Ok(result)
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
                        && !(result.is_ok()
                            && (request.kind == DataKind::Stockpile
                                || request.kind == DataKind::VersionConvert
                                    && request.input["outputPath"].is_string()))
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
