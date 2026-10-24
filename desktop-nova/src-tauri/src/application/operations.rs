//! Product-semantic one-shot operations owned by the Application Host.
//!
//! The WebView never supplies executable names, command-line arguments, or
//! temporary file paths. This module translates product requests into the
//! existing core helper contract and delegates only OS process mechanics to
//! [`ProcessSupervisor`].

use crate::platform::process_supervisor::{
    ChildLease, ExecutableRole, LaunchIdentity, LaunchSpec, LaunchToken, OutputTarget, ProcessSupervisor,
    WaitOutcome,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const MAX_DIAGNOSTIC_CHARS: usize = 16 * 1024;

#[derive(Clone)]
pub(crate) struct ApplicationOperations {
    supervisor: Arc<ProcessSupervisor>,
    project_root: PathBuf,
    data_root: PathBuf,
    backend_dir: PathBuf,
    next_generation: Arc<AtomicU64>,
    analysis_cache: Arc<Mutex<BTreeMap<AnalysisKey, Arc<OnceLock<Result<Value, String>>>>>>,
    layer_cache: Arc<Mutex<BTreeMap<AnalysisKey, Arc<OnceLock<Result<Arc<LayerCatalog>, String>>>>>>,
    lifecycle: Arc<(Mutex<OperationLifecycle>, Condvar)>,
    world_exports: Arc<Mutex<BTreeMap<String, WorldExportPaths>>>,
    stockpile_tasks: Arc<Mutex<()>>,
    data_worker: Arc<super::data_worker::DataWorkerHost>,
    stockpile_project_ids: Arc<Mutex<BTreeMap<AnalysisKey, String>>>,
    iris_inspector: Arc<Mutex<(super::protected_render::ProtectedRender, Option<nova_render_protocol::ProtectedTask>)>>,
}

#[derive(Default)]
struct OperationLifecycle {
    closing: bool,
    writes: usize,
    reads: BTreeMap<u64, ChildLease>,
    core_launches: BTreeMap<String, u64>,
}

pub(crate) struct ApplicationWriteGuard(Arc<(Mutex<OperationLifecycle>, Condvar)>);
impl Drop for ApplicationWriteGuard {
    fn drop(&mut self) {
        let (state, changed) = &*self.0;
        let mut state = state.lock().unwrap();
        state.writes = state.writes.saturating_sub(1);
        changed.notify_all();
    }
}

struct CoreReadGuard {
    lifecycle: Arc<(Mutex<OperationLifecycle>, Condvar)>,
    generation: u64,
}
impl Drop for CoreReadGuard {
    fn drop(&mut self) { self.lifecycle.0.lock().unwrap().reads.remove(&self.generation); }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct AnalysisKey {
    source: PathBuf,
    bytes: u64,
    modified: SystemTime,
    args: Vec<String>,
}

impl AnalysisKey {
    fn new(source: &Path, mut args: Vec<String>) -> Result<Self, String> {
        let source = fs::canonicalize(source).map_err(|error| format!("Resolve statistics source: {error}"))?;
        let metadata = fs::metadata(&source).map_err(|error| format!("Inspect statistics source: {error}"))?;
        if !metadata.is_file() { return Err("Statistics source must be a file".into()); }
        let modified = metadata.modified().map_err(|error| format!("Read statistics source revision: {error}"))?;
        // The source in the argument list must share the same identity too;
        // otherwise drive case/long-path aliases defeat the single-flight key.
        if let Some(index) = args.iter().position(|arg| arg == "--input") {
            if let Some(arg) = args.get_mut(index + 1) { *arg = path_text(&source); }
        }
        Ok(Self { source, bytes: metadata.len(), modified, args })
    }

    fn revision(&self) -> String {
        use sha2::{Digest, Sha256};
        let stamp = format!("{:?}\0{}\0{:?}\0{:?}", self.source, self.bytes, self.modified, self.args);
        format!("{:x}", Sha256::digest(stamp.as_bytes()))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
pub(crate) struct DataQueryInput { pub(crate) operation_id:String, pub(crate) kind:super::data_worker::protocol::DataKind, pub(crate) input:Value }

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct InspectProjectInput {
    pub(crate) source_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ReadLayerInput {
    pub(crate) source_path: String,
    pub(crate) y: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ReadLayerContainerInput {
    pub(crate) source_path: String,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectionMarkerPoint {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct EditProjectOriginInput {
    pub(crate) source_path: String,
    pub(crate) origin: Option<ProjectionMarkerPoint>,
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ExportWorldInput {
    pub(crate) task_id: String,
    pub(crate) target_version: String,
    pub(crate) source_path: String,
    pub(crate) output_directory: String,
    pub(crate) world_name: String,
    pub(crate) origin: super::model::IntegerVector,
}

#[derive(Clone)]
struct WorldExportPaths { cancel: PathBuf, progress: PathBuf }
pub(crate) struct WorldExportWork { input: ExportWorldInput, generation: u64, prefix: String, paths: WorldExportPaths }

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ConvertProjectVersionInput {
    pub(crate) source_path: String,
    pub(crate) target_version: String,
    #[serde(default)]
    pub(crate) replacements: BTreeMap<String, String>,
    pub(crate) expected_source_sha256: Option<String>,
    pub(crate) output_path: Option<String>,
    #[serde(default)]
    pub(crate) overwrite: bool,
    pub(crate) selection: Option<Value>,pub(crate) property_edit: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct BlockDatabaseQueryInput {
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) query: String,
    pub(crate) state: Option<String>,
    pub(crate) states: Option<Vec<String>>,
    /// Layer palettes only need names + map colours up front; model planes are
    /// fetched lazily for the few blocks that have no 2D icon.
    #[serde(default)]
    pub(crate) no_textures: bool,
    /// Rasterise the +Y (top-down) plane instead of the legacy -Z side plane.
    #[serde(default)]
    pub(crate) top_plane: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LayerPaletteEntry {
    pub(crate) block_id: String,
    pub(crate) property_id: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LayerSliceMeta {
    #[serde(default)]
    pub(crate) minecraft_data_version: Option<i32>,
    pub(crate) chunk_size: u32,
    pub(crate) size_x: i32,
    pub(crate) size_y: i32,
    pub(crate) size_z: i32,
    pub(crate) palette: Vec<LayerPaletteEntry>,
    pub(crate) property_pool: Vec<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LayerSliceBlock {
    pub(crate) x: i32,
    pub(crate) z: i32,
    pub(crate) palette_id: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LayerSliceData {
    pub(crate) y: i32,
    pub(crate) blocks: Vec<LayerSliceBlock>,
}

struct LayerCatalog {
    meta: LayerSliceMeta,
    layers: BTreeMap<i32, LayerSliceData>,
}

impl LayerCatalog {
    fn decode(mut result: Value) -> Result<Self, String> {
        let minecraft_data_version = result.pointer("/metadata/minecraft_data_version")
            .and_then(Value::as_i64).filter(|value| *value > 0).and_then(|value| i32::try_from(value).ok());
        let mut visual = result.get_mut("visual").map(Value::take).ok_or("分层数据缺少目录")?;
        let layers = visual.as_object_mut().ok_or("分层目录格式错误")?.remove("layers").ok_or("分层数据缺少层列表")?;
        let mut meta: LayerSliceMeta = decode_product(visual)?;
        meta.minecraft_data_version = minecraft_data_version;
        let layers: Vec<LayerSliceData> = decode_product(layers)?;
        Ok(Self { meta, layers: layers.into_iter().map(|layer| (layer.y, layer)).collect() })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LayerContainerData {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
    pub(crate) block_id: String,
    pub(crate) nbt: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ProjectionMarkers {
    pub(crate) origin: Option<ProjectionMarkerPoint>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ProjectionMarkerEditSummary {
    pub(crate) input: String,
    pub(crate) output: String,
    pub(crate) backup_path: Option<String>,
    pub(crate) origin: Option<ProjectionMarkerPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct EditProjectMetadataInput {
    pub(crate) source_path: String,
    pub(crate) patch: Value,
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectMutationResult {
    pub(crate) message: String,
    pub(crate) output_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ReplaceBlocksInput {
    pub(crate) source_path: String,
    pub(crate) output_path: Option<String>,
    #[serde(default)]
    pub(crate) rules: Vec<Value>,
    #[serde(default)]
    pub(crate) units: Option<Vec<Value>>,
    #[serde(default)]
    pub(crate) stages: Option<Vec<Vec<Value>>>,
    pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReplaceBlocksResult {
    pub(crate) message: String,
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ProjectStatsInput {
    pub(crate) source_path: String,
    pub(crate) include_container_items: bool,
    #[serde(default)]
    pub(crate) known_revision: Option<String>,
    #[serde(default)] pub(crate) include_structural_blocks: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MaterialScopeKind {
    All,
    Region,
    Layer,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct MaterialScope {
    pub(crate) kind: MaterialScopeKind,
    pub(crate) value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ProjectMaterialsInput {
    pub(crate) source_path: String,
    pub(crate) scope: MaterialScope,
    pub(crate) include_container_items: bool,
}

#[derive(Debug, Deserialize, Default, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase", default)]
pub(crate) struct StockpileTaskInput {
    source_path: Option<String>, task_id: Option<String>, scope: Option<MaterialScope>,
    include_container_items: Option<bool>, multiplier: Option<u64>,
    ui_locale:Option<String>, language_map:Option<BTreeMap<String,String>>,
}
#[derive(Debug, Deserialize, Default, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase", default)]
pub(crate) struct StockpileTaskCommandInput {
    pub(crate) action: String, pub(crate) task_id: Option<String>, project_id: Option<String>, legacy_task_id: Option<String>,
    source_revision: Option<String>, expected_revision: Option<String>, source_path: Option<String>, scope: Option<MaterialScope>,
    include_container_items: Option<bool>, multiplier: Option<u64>, name: Option<String>,
    preferences: Option<Value>, mutation: Option<Value>, material_id: Option<String>,
    output_path: Option<String>, input_path: Option<String>,
    query: Option<Value>, source_query: Option<Value>, export: Option<Value>, ui_locale: Option<String>, language_map: Option<BTreeMap<String,String>>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct StockpileTaskSetInput {
    pub(crate) task_id: String, source_revision: String, material_id: String, quantity: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct StockpileTaskResetInput { pub(crate) task_id: String, source_revision: String }

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct MaterialRecipeCacheInput { pub(crate) minecraft_version: String }

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct MaterialDetailInput {
    pub(crate) source_path:String,pub(crate) material_id:String,
    #[serde(default)] pub(crate) include_container_items:bool,
    pub(crate) selected_recipe:Option<String>,pub(crate) ui_locale:Option<String>,pub(crate) language_map:Option<BTreeMap<String,String>>,pub(crate) source_query:Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct GenerateProjectInput {
    pub(crate) plan: Value,
    pub(crate) output_path: Option<String>,
    pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateProjectResult {
    pub(crate) summary: Value,
    pub(crate) analysis: Option<Value>,
    pub(crate) output_path: Option<String>,
    pub(crate) diagnostics: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeComponentHealth {
    pub(crate) available: bool,
    pub(crate) message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeHealth {
    pub(crate) ok: bool,
    pub(crate) core: RuntimeComponentHealth,
    pub(crate) viewer: RuntimeComponentHealth,
}

#[derive(Debug, Serialize)]
pub(crate) struct EmbeddedPreview {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data_url: String,
}

#[derive(Debug, Deserialize)]
struct CoreProjectionPreviewResult {
    found: bool,
    width: Option<u32>,
    height: Option<u32>,
    output_path: Option<PathBuf>,
}

struct CoreOutput {
    stdout: String,
}

struct OperationFiles {
    paths: Vec<PathBuf>,
}

impl OperationFiles {
    fn new() -> Self {
        Self { paths: Vec::new() }
    }

    fn track(&mut self, path: PathBuf) -> PathBuf {
        self.paths.push(path.clone());
        path
    }
}

impl Drop for OperationFiles {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

impl ApplicationOperations {
    pub(crate) fn new(
        supervisor: Arc<ProcessSupervisor>,
        project_root: PathBuf,
        data_root: PathBuf,
        backend_dir: PathBuf,
    ) -> Result<Self, String> {
        if !project_root.is_absolute() || !project_root.is_dir() {
            return Err(
                "Application project root must be an existing absolute directory".to_owned(),
            );
        }
        if !backend_dir.is_absolute() || !backend_dir.is_dir() {
            return Err(
                "Application backend directory must be an existing absolute directory".to_owned(),
            );
        }
        let operations_root = data_root.join("application").join("operations");
        fs::create_dir_all(&operations_root).map_err(|error| {
            format!(
                "create Application operation directory {}: {error}",
                operations_root.display()
            )
        })?;
        let data_worker = Arc::new(super::data_worker::DataWorkerHost::new(supervisor.clone(), data_root.clone(), backend_dir.clone()));
        Ok(Self {
            supervisor,
            project_root,
            data_root,
            backend_dir,
            next_generation: Arc::new(AtomicU64::new(1)),
            analysis_cache: Arc::new(Mutex::new(BTreeMap::new())),
            layer_cache: Arc::new(Mutex::new(BTreeMap::new())),
            lifecycle: Arc::new((Mutex::new(OperationLifecycle::default()), Condvar::new())),
            world_exports: Arc::new(Mutex::new(BTreeMap::new())),
            stockpile_tasks: Arc::new(Mutex::new(())),
            data_worker,
            stockpile_project_ids: Arc::new(Mutex::new(BTreeMap::new())),
            iris_inspector: Arc::new(Mutex::new((super::protected_render::ProtectedRender::default(), None))),
        })
    }

    /// Covers actual persistent writes (including small Host-side config
    /// saves), not ordinary reads or discarded temporary request files.
    pub(crate) fn begin_write(&self) -> Result<ApplicationWriteGuard, String> {
        let mut state = self.lifecycle.0.lock().map_err(|_| "任务状态锁异常")?;
        if state.closing { return Err("应用正在关闭，新的保存尚未开始".into()); }
        state.writes += 1;
        Ok(ApplicationWriteGuard(Arc::clone(&self.lifecycle)))
    }

    pub(crate) fn begin_shutdown(&self) -> Vec<ChildLease> {
        let mut state = self.lifecycle.0.lock().unwrap();
        state.closing = true;
        self.data_worker.cancel_reads();
        eprintln!("[NOVA_LIFECYCLE] stage=core_activity_at_close core_calls={} active_reads={} pending_writes={}", json!(state.core_launches), state.reads.len(), state.writes);
        state.reads.values().cloned().collect()
    }

    pub(crate) fn lifecycle_snapshot(&self) -> Value {
        let state = self.lifecycle.0.lock().unwrap();
        json!({"coreCalls":state.core_launches,"activeReads":state.reads.len(),"pendingWrites":state.writes,"closing":state.closing})
    }

    pub(crate) fn wait_for_writes(&self, timeout: Duration) -> bool {
        let (state, changed) = &*self.lifecycle;
        let deadline = Instant::now() + timeout;
        let mut state = state.lock().unwrap();
        while state.writes > 0 {
            let now = Instant::now();
            if now >= deadline { return false; }
            state = changed.wait_timeout(state, deadline.saturating_duration_since(now)).unwrap().0;
        }
        true
    }

    pub(crate) fn cancel_shutdown(&self) { self.lifecycle.0.lock().unwrap().closing = false; }

    pub(crate) fn inspect_project(&self, input: InspectProjectInput) -> Result<Value, String> {
        let source = self.resolve_path(&input.source_path);
        self.run_core_json(vec!["analyze".to_owned(), path_text(&source)])
    }

    pub(crate) fn read_layer_bounds(&self, input: InspectProjectInput) -> Result<Value, String> {
        let source = self.resolve_path(&input.source_path);
        self.cached_analysis(&source, vec!["layer-bounds".into(), path_text(&source)])
    }

    pub(crate) fn read_layer_meta(&self, input: InspectProjectInput) -> Result<LayerSliceMeta, String> {
        let source = self.resolve_path(&input.source_path);
        Ok(self.cached_layers(&source)?.meta.clone())
    }

    pub(crate) fn read_layer(&self, input: ReadLayerInput) -> Result<LayerSliceData, String> {
        let source = self.resolve_path(&input.source_path);
        if input.y < 0 { return Err("分层高度不能为负数".into()); }
        Ok(self.cached_layers(&source)?.layers.get(&input.y).cloned()
            .unwrap_or(LayerSliceData { y: input.y, blocks: Vec::new() }))
    }

    fn cached_layers(&self, source: &Path) -> Result<Arc<LayerCatalog>, String> {
        let mut key = AnalysisKey::new(source, vec!["visual-layers-all".into()])?;
        key.args.push(path_text(&key.source));
        let slot = {
            let mut cache = self.layer_cache.lock().map_err(|_| "分层缓存锁异常")?;
            // Keep one completed source catalog, and never evict an in-flight
            // leader. Only the requested layer is copied back through IPC.
            cache.retain(|old, slot| old == &key || slot.get().is_none());
            Arc::clone(cache.entry(key.clone()).or_insert_with(|| Arc::new(OnceLock::new())))
        };
        let result = slot.get_or_init(|| {
            let started = Instant::now();
            let value = self.run_core_json(key.args.clone())?;
            let current = AnalysisKey::new(source, key.args.clone())?;
            if current != key { return Err("投影已改变，请重新读取分层".into()); }
            let catalog = LayerCatalog::decode(value)?;
            eprintln!("[NOVA_LAYER_TIMING] stage=source_catalog elapsed_ms={} layers={} blocks={} source_reads=1", started.elapsed().as_millis(), catalog.layers.len(), catalog.layers.values().map(|layer| layer.blocks.len()).sum::<usize>());
            Ok(Arc::new(catalog))
        }).clone();
        if result.is_err() {
            if let Ok(mut cache) = self.layer_cache.lock() {
                if cache.get(&key).is_some_and(|current| Arc::ptr_eq(current, &slot)) { cache.remove(&key); }
            }
        }
        result
    }

    pub(crate) fn read_layer_container(
        &self,
        input: ReadLayerContainerInput,
    ) -> Result<Option<LayerContainerData>, String> {
        let source = self.resolve_path(&input.source_path);
        decode_product(self.run_core_json(layer_container_args(&source, input.x, input.y, input.z)?)?)
    }

    pub(crate) fn read_project_markers(&self, input: InspectProjectInput) -> Result<ProjectionMarkers, String> {
        let source = self.resolve_path(&input.source_path);
        decode_product(self.run_core_json(vec!["projection-markers".to_owned(), path_text(&source)])?)
    }

    pub(crate) fn edit_project_origin(
        &self,
        input: EditProjectOriginInput,
    ) -> Result<ProjectionMarkerEditSummary, String> {
        let source = self.resolve_path(&input.source_path);
        if input.output_path.as_ref().is_some_and(|path| path.trim().is_empty()) {
            return Err("The projection output path must not be empty".to_owned());
        }
        let output = input.output_path.as_deref().map(|path| self.resolve_path(path));
        let (source, output) = protected_origin_paths(&source, output.as_deref())?;
        decode_product(self.run_core_json(origin_args(&source, input.origin, output.as_deref()))?)
    }

    pub(crate) fn edit_project_metadata(
        &self,
        input: EditProjectMetadataInput,
    ) -> Result<ProjectMutationResult, String> {
        let source = self.resolve_path(&input.source_path);
        let output = input
            .output_path
            .as_deref()
            .map(|path| self.resolve_path(path))
            .unwrap_or_else(|| source.clone());
        let (generation, prefix) = self.next_operation("metadata");
        let mut files = OperationFiles::new();
        let patch_path = files.track(self.operation_path(&prefix, "patch.json"));
        write_json(&patch_path, &input.patch)?;
        let mut args = vec![
            "edit-metadata".to_owned(),
            "--input".to_owned(),
            path_text(&source),
            "--patch".to_owned(),
            path_text(&patch_path),
        ];
        if output != source {
            args.extend(["--output".to_owned(), path_text(&output)]);
        }
        let result = self.run_core(generation, &prefix, args, &mut files)?;
        Ok(ProjectMutationResult {
            message: semantic_message(&result.stdout),
            output_path: path_text(&output),
        })
    }

    pub(crate) fn world_export_profiles(&self, source_path: String) -> Result<Value, String> {
        self.run_core_json(vec!["world-export".into(), "--profiles".into(), "--input".into(), path_text(&self.resolve_path(&source_path))])
    }

    pub(crate) fn begin_world_export(&self, input: ExportWorldInput) -> Result<WorldExportWork, String> {
        if input.task_id.is_empty() || input.task_id.len() > 64 || !input.task_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return Err("无效的世界导出任务标识".into()); }
        if input.output_directory.trim().is_empty() { return Err("请选择世界输出目录".into()); }
        if input.target_version.trim().is_empty() { return Err("请选择明确的目标游戏版本".into()); }
        let (generation,prefix)=self.next_operation("world-export");
        let paths=WorldExportPaths {cancel:self.operation_path(&prefix,"cancel"),progress:self.operation_path(&prefix,"progress.json")};
        let mut active=self.world_exports.lock().map_err(|_|"世界导出任务状态锁异常")?;
        if active.contains_key(&input.task_id) { return Err("世界导出任务已经存在".into()); }
        active.insert(input.task_id.clone(),paths.clone());
        Ok(WorldExportWork {input,generation,prefix,paths})
    }

    pub(crate) fn world_export_status(&self, task_id: &str, cancel: bool) -> Result<Value, String> {
        let paths=self.world_exports.lock().map_err(|_|"世界导出任务状态锁异常")?.get(task_id).cloned();
        let Some(paths)=paths else {return Ok(json!({"active":false}));};
        if cancel {fs::write(&paths.cancel,b"cancel").map_err(|e|format!("取消世界导出：{e}"))?;}
        // A progress file can be read between truncation and write. Keep the
        // last UI stage instead of inventing success or resetting percentage.
        let progress=fs::read(&paths.progress).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok());
        Ok(json!({"active":true,"progress":progress}))
    }

    pub(crate) fn export_world(&self, work: WorldExportWork) -> Result<Value, String> {
        let input=work.input;
        let source=self.resolve_path(&input.source_path);let output=self.resolve_path(&input.output_directory);
        let mut files=OperationFiles::new();files.track(work.paths.cancel.clone());files.track(work.paths.progress.clone());
        let result=self.run_core(work.generation,&work.prefix,vec!["world-export".into(),"--input".into(),path_text(&source),
            "--output".into(),path_text(&output),"--name".into(),input.world_name,"--target".into(),input.target_version,
            "--cancel".into(),path_text(&work.paths.cancel),"--progress".into(),path_text(&work.paths.progress),
            "--x".into(),input.origin.x.to_string(),"--y".into(),input.origin.y.to_string(),"--z".into(),input.origin.z.to_string()],&mut files)
            .and_then(|result|parse_core_json(&result.stdout));
        if let Ok(mut active)=self.world_exports.lock(){active.remove(&input.task_id);}
        result
    }

    pub(crate) fn convert_project_version(&self, mut input: ConvertProjectVersionInput) -> Result<Value, String> {
        input.source_path = path_text(&self.resolve_path(&input.source_path));
        if let Some(path) = input.output_path.as_mut() { *path = path_text(&self.resolve_path(path)); }
        if input.output_path.is_some() && input.expected_source_sha256.is_none() { return Err("另存前须确认转换差异".into()); }
        let writing = input.output_path.is_some();
        self.data_request(super::data_worker::protocol::DataKind::VersionConvert, serde_json::to_value(input).map_err(|e|e.to_string())?, None, writing)
    }

    pub(crate) fn query_block_database(&self, input: BlockDatabaseQueryInput) -> Result<Value, String> {
        let (generation, prefix) = self.next_operation("block-database");
        let mut files=OperationFiles::new();
        let mut args=vec!["block-db".into()];
        if let Some(version)=input.version { args.extend(["--version".into(),version]); } else { args.push("--versions".into()); }
        if input.no_textures { args.push("--no-textures".into()); }
        if input.top_plane { args.push("--top-plane".into()); }
        if !input.query.is_empty() { args.extend(["--query".into(),input.query]); }
        if let Some(state)=input.state { args.extend(["--state".into(),state]); }
        if let Some(states)=input.states {
            let path=files.track(self.operation_path(&prefix,"states.json"));
            write_json(&path,&serde_json::json!(states))?;
            args.extend(["--states-json".into(),path_text(&path)]);
        }
        let result=self.run_core(generation,&prefix,args,&mut files)?;
        serde_json::from_str(result.stdout.trim()).map_err(|error|format!("读取方块数据库结果：{error}"))
    }

    pub(crate) fn replace_blocks(
        &self,
        input: ReplaceBlocksInput,
    ) -> Result<ReplaceBlocksResult, String> {
        if input.rules.is_empty() && input.units.as_ref().is_none_or(Vec::is_empty) && input.stages.as_ref().is_none_or(Vec::is_empty) {
            return Err("At least one replacement rule is required".to_owned());
        }
        if [!input.rules.is_empty(), input.units.is_some(), input.stages.is_some()].into_iter().filter(|value| *value).count() != 1 {
            return Err("请选择单一替换规则格式".into());
        }
        if input.stages.as_ref().is_some_and(|stages| stages.iter().any(Vec::is_empty)) {
            return Err("串行分组不能包含空步骤".into());
        }
        if input.dry_run && input.output_path.is_some() {
            return Err("A dry-run replacement may not specify an output path".to_owned());
        }
        if !input.dry_run && input.output_path.is_none() {
            return Err("A writing replacement requires an output path".to_owned());
        }
        let source = self.resolve_path(&input.source_path);
        let output = input
            .output_path
            .as_deref()
            .map(|path| self.resolve_path(path));
        let (generation, prefix) = self.next_operation("replace");
        let mut files = OperationFiles::new();
        let rules_path = files.track(self.operation_path(&prefix, "rules.json"));
        let document = if let Some(stages) = input.stages { json!({ "schema_version": 2, "stages": stages }) }
            else if let Some(units) = input.units { json!({ "schema_version": 2, "units": units }) }
            else { json!({ "rules": input.rules }) };
        write_json(&rules_path, &document)?;
        let args = replacement_args(&source, &rules_path, output.as_deref(), input.dry_run);
        let result = self.run_core(generation, &prefix, args, &mut files)?;
        Ok(ReplaceBlocksResult {
            message: semantic_message(&result.stdout),
            output_path: output.as_deref().map(path_text),
        })
    }

    pub(crate) fn project_stats(&self, mut input: ProjectStatsInput) -> Result<Value, String> {
        input.source_path = path_text(&self.resolve_path(&input.source_path));
        self.data_request(super::data_worker::protocol::DataKind::Stats,serde_json::to_value(input).map_err(|e|e.to_string())?,None,false)
    }

    pub(crate) fn project_materials(&self, input: ProjectMaterialsInput) -> Result<Value, String> {
        let value=self.material_snapshot(&self.resolve_path(&input.source_path),&input.scope,input.include_container_items)?;
        Ok(json!({"material_items":value["material_items"]}))
    }

    pub(crate) fn stockpile_task(&self, input: StockpileTaskInput) -> Result<Value, String> {
        let mut request=serde_json::to_value(input).map_err(|e|e.to_string())?;
        request["action"] = if request["taskId"].is_string() {"select"} else {"open"}.into();
        self.data_request(super::data_worker::protocol::DataKind::Stockpile,request,None,true)
    }

    pub(crate) fn stockpile_task_command(&self, input: StockpileTaskCommandInput) -> Result<Value, String> {
        let writing = !matches!(input.action.as_str(),"details"|"query"|"exportView"|"list");
        self.data_request(super::data_worker::protocol::DataKind::Stockpile,serde_json::to_value(input).map_err(|e|e.to_string())?,None,writing)
    }

    pub(crate) fn set_stockpile_quantity(&self, input: StockpileTaskSetInput) -> Result<Value, String> {
        self.run_stockpile_request(&json!({"action":"mutate","taskId":input.task_id,"sourceRevision":input.source_revision,
            "mutation":{"materialId":input.material_id,"quantity":input.quantity,"status":"done"}}))
    }
    pub(crate) fn reset_stockpile(&self, input: StockpileTaskResetInput) -> Result<Value, String> {
        self.run_stockpile_request(&json!({"action":"mutate","taskId":input.task_id,"sourceRevision":input.source_revision,"mutation":{"reset":true}}))
    }
    fn stockpile_project_id(&self, source: &Path) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let key = AnalysisKey::new(source,Vec::new())?;
        if let Some(id) = self.stockpile_project_ids.lock().map_err(|_|"备货来源身份缓存锁异常")?.get(&key).cloned() { return Ok(id); }
        let mut file = fs::File::open(source).map_err(|error|format!("读取备货来源身份：{error}"))?;
        let mut digest = Sha256::new(); let mut bytes = [0_u8;64*1024];
        loop { let count = file.read(&mut bytes).map_err(|error|format!("读取备货来源身份：{error}"))?; if count == 0 { break; } digest.update(&bytes[..count]); }
        if AnalysisKey::new(source,Vec::new())? != key { return Err("投影在读取身份时发生变化，请重试".into()); }
        let id = format!("{:x}",digest.finalize());
        let mut cache = self.stockpile_project_ids.lock().map_err(|_|"备货来源身份缓存锁异常")?;
        cache.retain(|old,_| old.source != key.source);
        if cache.len() >= 16 { if let Some(old) = cache.keys().next().cloned() { cache.remove(&old); } }
        cache.insert(key,id.clone()); Ok(id)
    }

    fn run_stockpile_request(&self, request: &Value) -> Result<Value, String> {
        self.data_request(super::data_worker::protocol::DataKind::Stockpile,request.clone(),None,true)
    }

    pub(crate) fn export_material_list(&self, input: Value) -> Result<Value, String> {
        let writing = input["action"] == "export";
        self.data_request(super::data_worker::protocol::DataKind::MaterialExport,input,None,writing)
    }

    pub(crate) fn read_material_detail(&self, input: MaterialDetailInput) -> Result<Value, String> {
        self.data_request(super::data_worker::protocol::DataKind::MaterialDetail,serde_json::to_value(input).map_err(|e|e.to_string())?,None,false)
    }

    pub(crate) fn prepare_material_recipes(&self, input: MaterialRecipeCacheInput) -> Result<Value, String> {
        if input.minecraft_version != "26.3" { return Err("当前材料语义需要 Minecraft 26.3 配方".into()); }
        self.run_core_json(material_recipe_fetch_args(input.minecraft_version))
    }

    fn material_snapshot(&self, source: &Path, scope: &MaterialScope, include_container_items: bool) -> Result<Value, String> {
        self.data_request(super::data_worker::protocol::DataKind::Analysis,json!({"sourcePath":path_text(source),"scope":scope,"includeContainerItems":include_container_items}),None,false)
    }

    fn cached_analysis(&self, source: &Path, args: Vec<String>) -> Result<Value, String> {
        let key = AnalysisKey::new(source, args)?;
        let slot = {
            let mut cache = self.analysis_cache.lock().map_err(|_| "Statistics cache lock failed")?;
            // A small completed-result cache, shared across all WebViews. Never
            // evict an in-flight slot or allow two leaders for the same request.
            cache.retain(|old, slot| old.source != key.source || old.modified == key.modified && old.bytes == key.bytes || slot.get().is_none());
            if !cache.contains_key(&key) && cache.len() >= 8 {
                if let Some(old) = cache.iter().find(|(_, slot)| slot.get().is_some()).map(|(old, _)| old.clone()) { cache.remove(&old); }
            }
            Arc::clone(cache.entry(key.clone()).or_insert_with(|| Arc::new(OnceLock::new())))
        };
        let result = slot.get_or_init(|| {
            let value = self.run_core_json(key.args.clone())?;
            let current = AnalysisKey::new(source, key.args.clone())?;
            if current != key { return Err("Projection changed during statistics analysis; reload its latest version".into()); }
            Ok(value)
        }).clone();
        if result.is_err() {
            if let Ok(mut cache) = self.analysis_cache.lock() {
                if cache.get(&key).is_some_and(|current| Arc::ptr_eq(current, &slot)) { cache.remove(&key); }
            }
        }
        result
    }

    pub(crate) fn generate_project(
        &self,
        input: GenerateProjectInput,
    ) -> Result<GenerateProjectResult, String> {
        if input.dry_run && input.output_path.is_some() {
            return Err("A dry-run generation may not specify an output path".to_owned());
        }
        if !input.dry_run && input.output_path.is_none() {
            return Err("A writing generation requires an output path".to_owned());
        }
        let output = input
            .output_path
            .as_deref()
            .map(|path| self.resolve_path(path));
        let (generation, prefix) = self.next_operation("generate");
        let mut files = OperationFiles::new();
        let plan_path = files.track(self.operation_path(&prefix, "plan.json"));
        write_json(&plan_path, &input.plan)?;
        let core = self.run_core(
            generation,
            &prefix,
            generate_args(&plan_path, output.as_deref(), input.dry_run),
            &mut files,
        )?;
        let mut summary = parse_core_json(&core.stdout)?;
        if let Some(object) = summary.as_object_mut() {
            object.remove("plan_path");
        }
        let (analysis, analysis_warning) = match output.as_ref() {
            Some(path) => match self.run_core_json(vec!["analyze".to_owned(), path_text(path)]) {
                Ok(analysis) => (Some(analysis), None),
                Err(error) => (
                    None,
                    Some(format!(
                        "Generated project was written successfully, but follow-up analysis failed: {}",
                        bounded_text(&error)
                    )),
                ),
            },
            None => (None, None),
        };
        let mut diagnostics = serde_json::to_string_pretty(&summary)
            .unwrap_or_else(|_| "Generation completed".to_owned());
        if let Some(warning) = analysis_warning {
            diagnostics.push_str("\n\n");
            diagnostics.push_str(&warning);
        }
        Ok(GenerateProjectResult {
            summary,
            analysis,
            output_path: output.as_deref().map(path_text),
            diagnostics,
        })
    }

    pub(crate) fn runtime_health(&self) -> RuntimeHealth {
        let viewer_available = self
            .backend_dir
            .join(format!("litematica_native_viewer{}", std::env::consts::EXE_SUFFIX))
            .is_file();
        let core = match self.run_core_text(vec!["--help".to_owned()]) {
            Ok(help) if !help.trim().is_empty() => RuntimeComponentHealth {
                available: true,
                message: "Core operations are available".to_owned(),
            },
            Ok(_) => RuntimeComponentHealth {
                available: false,
                message: "Core helper returned an empty capability response".to_owned(),
            },
            Err(error) => RuntimeComponentHealth {
                available: false,
                message: bounded_text(&error),
            },
        };
        let viewer = RuntimeComponentHealth {
            available: viewer_available,
            message: if viewer_available {
                "Native preview is available".to_owned()
            } else {
                "Native preview component is unavailable".to_owned()
            },
        };
        RuntimeHealth {
            ok: core.available && viewer.available,
            core,
            viewer,
        }
    }

    pub(crate) fn read_embedded_preview(
        &self,
        input: InspectProjectInput,
    ) -> Result<Option<EmbeddedPreview>, String> {
        let source = self.resolve_path(&input.source_path);
        if !source.is_file() {
            return Err(format!(
                "Projection preview source is missing: {}",
                source.display()
            ));
        }
        let (generation, prefix) = self.next_operation("embedded-preview");
        let mut files = OperationFiles::new();
        let output_path = files.track(self.operation_path(&prefix, "preview.png"));
        let core = self.run_core(
            generation,
            &prefix,
            vec![
                "projection-preview".to_owned(),
                "--input".to_owned(),
                path_text(&source),
                "--output".to_owned(),
                path_text(&output_path),
                "--json".to_owned(),
            ],
            &mut files,
        )?;
        let result: CoreProjectionPreviewResult = serde_json::from_str(core.stdout.trim())
            .map_err(|error| format!("decode embedded preview result: {error}"))?;
        if !result.found {
            if result.width.is_some() || result.height.is_some() || result.output_path.is_some() {
                return Err("Embedded preview returned metadata for found=false".to_owned());
            }
            if output_path.exists() {
                return Err("Embedded preview published an image for found=false".to_owned());
            }
            return Ok(None);
        }
        let width = result
            .width
            .filter(|value| *value > 0)
            .ok_or_else(|| "Embedded preview omitted a positive width".to_owned())?;
        let height = result
            .height
            .filter(|value| *value > 0)
            .ok_or_else(|| "Embedded preview omitted a positive height".to_owned())?;
        let reported = result
            .output_path
            .ok_or_else(|| "Embedded preview omitted its output path".to_owned())?;
        let expected = fs::canonicalize(&output_path).map_err(|error| {
            format!(
                "resolve embedded preview {}: {error}",
                output_path.display()
            )
        })?;
        let reported = fs::canonicalize(&reported).map_err(|error| {
            format!(
                "resolve reported embedded preview {}: {error}",
                reported.display()
            )
        })?;
        if expected != reported {
            return Err("Embedded preview reported an unexpected output path".to_owned());
        }
        let bytes = fs::read(&expected)
            .map_err(|error| format!("read embedded preview {}: {error}", expected.display()))?;
        validate_png_dimensions(&bytes, width, height)?;
        Ok(Some(EmbeddedPreview {
            width,
            height,
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ),
        }))
    }

    /// Game-data selection/import uses the same supervised Core business bridge.
    /// It deliberately resolves the current store instead of a render snapshot.
    pub(crate) fn game_data_command(&self, args: Vec<String>) -> Result<Value, String> {
        let writing = matches!(args.first().map(String::as_str), Some("import" | "select" | "default"));
        let _write = if writing { Some(self.begin_write()?) } else { None };
        let mut command = vec!["game-data".to_owned()];
        command.extend(args);
        command.extend([
            "--store".to_owned(),
            path_text(&crate::current_user_config_dir()?.join("game-data")),
            "--default-root".to_owned(),
            path_text(&crate::get_root().join("pack-in/game-data/default")),
        ]);
        self.run_core_json(command)
    }

    pub(crate) fn game_data_icon(&self, id: String) -> Result<Value, String> {
        self.run_core_json(vec!["game-data-icon".to_owned(), "--id".to_owned(), id])
    }

    pub(crate) fn iris_settings(&self, args: Vec<String>) -> Result<Value, String> {
        if args.first().map(String::as_str) != Some("iris-settings") || args.len() % 2 != 1 {
            return Err("Invalid shader settings request".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for pair in args[1..].chunks_exact(2) {
            if !matches!(pair[0].as_str(), "--root" | "--pack" | "--language" | "--save-json" | "--rollback-json")
                || !seen.insert(pair[0].as_str()) || pair[1].is_empty() {
                return Err("Invalid shader settings argument".into());
            }
        }
        if !seen.contains("--root") || !seen.contains("--pack") {
            return Err("Shaderpack root and name are required".into());
        }
        self.run_core_json(args)
    }

    fn run_core_json(&self, args: Vec<String>) -> Result<Value, String> {
        let started = Instant::now();
        let output = self.run_core_text(args)?;
        let helper_ms = started.elapsed().as_secs_f64() * 1000.0;
        let decode = Instant::now();
        let result = parse_core_json(&output);
        if std::env::var_os("NOVA_STATS_TIMING").is_some() {
            eprintln!("[NOVA_STATS_TIMING] supervised_helper_and_transport_ms={helper_ms:.3} host_json_decode_ms={:.3} payload_bytes={}", decode.elapsed().as_secs_f64() * 1000.0, output.len());
        }
        result
    }

    fn run_core_text(&self, args: Vec<String>) -> Result<String, String> {
        let (generation, prefix) = self.next_operation("core");
        let mut files = OperationFiles::new();
        self.run_core(generation, &prefix, args, &mut files)
            .map(|output| output.stdout)
    }

    fn run_core(
        &self,
        generation: u64,
        prefix: &str,
        args: Vec<String>,
        files: &mut OperationFiles,
    ) -> Result<CoreOutput, String> {
        let command = args.first().cloned().unwrap_or_default();
        let writing = core_command_writes(&args);
        let _write = if writing { Some(self.begin_write()?) } else { None };
        let stdout_path = files.track(self.operation_path(prefix, "stdout.log"));
        // stdout is the Core business response, not a diagnostic archive. Its
        // existing temporary lifetime stays unchanged; stderr survives crashes.
        let stderr_path = if super::diagnostics::has_session() {
            super::diagnostics::log_path("core", &format!("{prefix}-stderr.log"), self.operation_path(prefix, "stderr.log").parent().unwrap())?
        } else {
            files.track(self.operation_path(prefix, "stderr.log"))
        };
        let identity = LaunchIdentity {
            owner_epoch: self.supervisor.owner_epoch(),
            generation,
            launch_token: LaunchToken::new(format!("application-core-{prefix}"))
                .map_err(|error| format!("create core operation launch token: {error}"))?,
        };
        let mut launch = LaunchSpec::new(ExecutableRole::CoreHelper, identity);
        launch.args = args.into_iter().map(Into::into).collect();
        launch.environment = vec![(
            "LBA_DATA_ROOT".into(),
            self.data_root.as_os_str().to_owned(),
        )];
        // Each helper keeps the selected immutable data identity for its lifetime.
        // Selection/import commands resolve the live store themselves.
        let saved_stockpile_snapshot = command == "stockpile-task" && args_request_snapshot_only(&launch.args);
        if command != "game-data" && command != "iris-settings" && !saved_stockpile_snapshot {
            match crate::game_data::resolve_context(
                &crate::current_user_config_dir()?.join("game-data"),
                &crate::get_root().join("pack-in/game-data/default"),
            ) {
                Ok(context) => launch.environment.push(("NOVA_GAME_DATA_SNAPSHOT".into(),
                    serde_json::to_string(&context).map_err(|error| error.to_string())?.into())),
                Err(error) if matches!(command.as_str(), "runtime-paths" | "version" | "help") => { let _ = error; },
                Err(error) => return Err(format!("{error:#}")),
            }
        }
        if command == "iris-settings" {
            // One Host-owned metadata inspector context, reused across reads/saves.
            // No scene, worker, shared-light request or render task is started.
            let mut inspector = self.iris_inspector.lock().map_err(|_| "Shader inspector lock failed")?;
            if inspector.1.is_none() {
                let task = inspector.0.acquire(&self.data_root, &self.backend_dir,
                    br#"{"displayMode":"standard","purpose":"iris-settings"}"#)?;
                inspector.1 = Some(task);
            }
            let task = inspector.1.as_ref().unwrap();
            launch.environment.push(("NOVA_PROTECTED_MODULE_PATH".into(), task.module_path.as_os_str().to_owned()));
            launch.environment.push(("NOVA_PROTECTED_TICKET_PATH".into(), task.ticket_path.as_os_str().to_owned()));
        }
        launch.current_dir = Some(self.backend_dir.clone());
        launch.stdout = OutputTarget::File {
            path: stdout_path.clone(),
            append: false,
        };
        launch.stderr = OutputTarget::File {
            path: stderr_path.clone(),
            append: false,
        };
        let spawn_started = Instant::now();
        let child = {
            let mut state = self.lifecycle.0.lock().map_err(|_| "任务状态锁异常")?;
            if state.closing && !writing { return Err("读取已取消，应用正在关闭".into()); }
            let child = self.supervisor.spawn(launch)
                .map_err(|error| format!("start core operation: {error}"))?.child;
            if !writing { state.reads.insert(generation, child.clone()); }
              *state.core_launches.entry(command.clone()).or_default() += 1;
            child
        };
        let spawn_ms = spawn_started.elapsed().as_secs_f64() * 1000.0;
        let _read = CoreReadGuard { lifecycle: Arc::clone(&self.lifecycle), generation };
        let wait_started = Instant::now();
        let exit = loop {
            match self.supervisor.wait_for_exit(&child, Duration::from_millis(100)) {
                WaitOutcome::Exited(exit) => break exit,
                // Polling is not an execution deadline. Explicit shutdown still
                // terminates owned read tasks through the existing supervisor.
                WaitOutcome::TimedOut => continue,
                WaitOutcome::Stale => {
                    return Err("Core operation lost its supervised process lease".to_owned())
                }
            }
        };
        let wait_ms = wait_started.elapsed().as_secs_f64() * 1000.0;
        let transport_started = Instant::now();
        let stdout = read_output(&stdout_path)?;
        let stderr = read_output(&stderr_path)?;
        if std::env::var_os("NOVA_STATS_TIMING").is_some() {
            eprint!("{stderr}");
            eprintln!("[NOVA_STATS_TIMING] host_process_spawn_ms={spawn_ms:.3} host_child_wait_ms={wait_ms:.3} host_output_transport_ms={:.3}", transport_started.elapsed().as_secs_f64() * 1000.0);
        }
        if exit.wait_error.is_some() {
            return Err(format!(
                "Core operation could not be reaped: {}",
                exit.wait_error.unwrap_or_default()
            ));
        }
        if exit.exit_code != Some(0) {
            return Err(format!(
                "Core operation failed: {}",
                bounded_text(if stderr.trim().is_empty() {
                    &stdout
                } else {
                    &stderr
                })
            ));
        }
        Ok(CoreOutput { stdout })
    }

    pub(crate) fn canonical_render_source(&self, source: &Path) -> Result<PathBuf, String> {
        let result = self.data_request(super::data_worker::protocol::DataKind::Format,
            json!({"action": "runtimeInput", "input": path_text(source)}), None, false)?;
        result["snapshotPath"].as_str().map(PathBuf::from).ok_or_else(|| "格式准备未返回有效场景快照".into())
    }

    pub(crate) fn inspect_library_source(&self, source: &Path) -> Result<(), String> {
        self.data_request(super::data_worker::protocol::DataKind::Format,
            json!({"action": "libraryInspect", "input": path_text(source)}), None, false).map(|_| ())
    }

    pub(crate) fn data_query(&self, query: DataQueryInput) -> Result<Value, String> {
        use super::data_worker::protocol::DataKind;
        if matches!(query.kind,DataKind::Analysis|DataKind::Cancel) { return Err("Unsupported public data query".into()); }
        let writing = query.kind.writes(&query.input);
        self.data_request(query.kind,query.input,Some(query.operation_id),writing)
    }
    pub(crate) fn data_operation_status(&self, id:&str, cancel:bool) -> Result<Value,String> { self.data_worker.status(id,cancel) }
    fn data_request(&self,kind:super::data_worker::protocol::DataKind,mut input:Value,id:Option<String>,writing:bool)->Result<Value,String>{
        if self.lifecycle.0.lock().map_err(|_|"任务状态锁异常")?.closing {return Err("应用正在关闭，新的数据任务已取消".into());}
        let _write=if writing{Some(self.begin_write()?)}else{None};
        if matches!(kind, super::data_worker::protocol::DataKind::Composition | super::data_worker::protocol::DataKind::Workspace | super::data_worker::protocol::DataKind::Format) {
            input["storageDir"] = path_text(&crate::current_user_config_dir()?).into();
        }

        if let Some(path)=input["sourcePath"].as_str().filter(|path|!path.is_empty()){input["sourcePath"]=path_text(&self.resolve_path(path)).into();}
        for key in ["inputPath","outputPath"]{if let Some(path)=input[key].as_str().filter(|path|!path.is_empty()){input[key]=path_text(&self.resolve_path(path)).into();}}
        let operation_id=id.unwrap_or_else(||self.next_operation("data").1);
        self.data_worker.execute(super::data_worker::protocol::DataRequest{operation_id,kind,input})
    }

    fn resolve_path(&self, value: &str) -> PathBuf {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            self.project_root.join(path)
        }
    }

    fn next_operation(&self, kind: &str) -> (u64, String) {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let prefix = format!("{kind}-{}-{generation}", std::process::id());
        (generation, prefix)
    }

    fn operation_path(&self, prefix: &str, suffix: &str) -> PathBuf {
        self.data_root
            .join("application")
            .join("operations")
            .join(format!("{prefix}-{suffix}"))
    }
}

fn layer_args(source: &Path, y: i32) -> Result<Vec<String>, String> {
    if y < 0 {
        return Err("Layer coordinates must be non-negative".to_owned());
    }
    Ok(vec!["visualize-layer".to_owned(), path_text(source), format!("--y={y}")])
}

fn layer_container_args(source: &Path, x: i32, y: i32, z: i32) -> Result<Vec<String>, String> {
    if x < 0 || y < 0 || z < 0 {
        return Err("Layer coordinates must be non-negative".to_owned());
    }
    Ok(vec![
        "read-layer-container".to_owned(), path_text(source),
        format!("--x={x}"), format!("--y={y}"), format!("--z={z}"),
    ])
}

fn origin_args(source: &Path, origin: Option<ProjectionMarkerPoint>, output: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        if origin.is_some() { "set-origin" } else { "remove-origin" }.to_owned(),
        path_text(source),
    ];
    if let Some(point) = origin {
        args.extend([format!("--x={}", point.x), format!("--y={}", point.y), format!("--z={}", point.z)]);
    }
    if let Some(output) = output {
        args.push(format!("--output={}", path_text(output)));
    }
    args
}

/// Core owns in-place backup/restore; a separate output must be a new file.
/// Core also uses exclusive creation so this preflight is not the only overwrite guard.
fn protected_origin_paths(source: &Path, output: Option<&Path>) -> Result<(PathBuf, Option<PathBuf>), String> {
    let source = fs::canonicalize(source).map_err(|error| format!("Resolve projection source: {error}"))?;
    if !source.is_file() {
        return Err("Projection source must be a file".to_owned());
    }
    let Some(output) = output else { return Ok((source, None)); };
    if output.exists() {
        return Err("The Save As destination already exists; choose a new file".to_owned());
    }
    let parent = output.parent().filter(|parent| parent.is_dir())
        .ok_or_else(|| "Projection output directory does not exist".to_owned())?;
    let name = output.file_name().ok_or_else(|| "Projection output requires a file name".to_owned())?;
    let output = fs::canonicalize(parent).map_err(|error| format!("Resolve projection output directory: {error}"))?.join(name);
    Ok((source, Some(output)))
}

fn decode_product<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("Core operation returned invalid product data: {error}"))
}

#[cfg(test)]
#[path="../../../../tools/viewer-core/src/data_scope.rs"]
mod core_data_scope_tests;
#[cfg(test)]
fn material_snapshot_for_scope(value:Value,scope:&MaterialScope)->Result<Value,String>{
    core_data_scope_tests::material_snapshot_for_scope(value,&serde_json::from_value(serde_json::to_value(scope).unwrap()).unwrap())
}

/// This inspects only a Host-owned temporary request, never a projection.
/// Saved Stockpile state remains usable without a live game-resource selection.
fn args_request_snapshot_only(args: &[std::ffi::OsString]) -> bool {
    args.get(1).and_then(|path| fs::read(Path::new(path)).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .is_some_and(|request| request.get("snapshot").is_none() && match request["action"].as_str() {
            Some("open" | "list" | "select" | "mutate" | "rename" | "export" | "import") => true,
            Some("settings") => request.get("multiplier").is_none(),
            _ => false,
        })
}

#[cfg(test)]
fn desktop_task_identity(source: &Path, scope: &MaterialScope, containers: bool, multiplier: u64, snapshot: &Value) -> String {
    use sha2::{Digest, Sha256};
    let source_id = path_text(source);
    #[cfg(windows)]
    let source_id = source_id.to_lowercase();
    let identity = json!([source_id, scope, containers, multiplier, snapshot.pointer("/metadata/minecraft_data_version"), snapshot.get("material_semantics")]);
    format!("{:x}", Sha256::digest(identity.to_string().as_bytes()))
}

fn stats_args(source: &Path, include_container_items: bool) -> Vec<String> {
    let mut args = vec![
        "stats".to_owned(),
        "--input".to_owned(),
        path_text(source),
        "--json".to_owned(),
    ];
    if include_container_items {
        args.push("--include-container-items".to_owned());
    }
    args
}

fn core_command_writes(args: &[String]) -> bool {
    let command = args.first().map(String::as_str).unwrap_or("");
    match command {
        "edit-metadata" | "set-origin" | "remove-origin" | "materials-export" | "stockpile-task" => true,
        "world-export" => !args.iter().any(|arg|arg=="--profiles"),
        "iris-settings" => args.iter().any(|arg| arg == "--save-json" || arg == "--rollback-json"),
        "version-convert" => args.iter().any(|arg| arg == "--output" || arg == "--overwrite"),
        "replace-blocks" | "generate" | "generate-projection" => !args.iter().any(|arg| arg == "--dry-run"),
        // Recipe downloads/extraction only write validated replaceable caches;
        // they can be cancelled without interrupting a user projection save.
        _ => false,
    }
}

fn materials_args(
    source: &Path,
    scope: &MaterialScope,
    include_container_items: bool,
) -> Result<Vec<String>, String> {
    let mut args = vec![
        "materials".to_owned(),
        "--input".to_owned(),
        path_text(source),
    ];
    match scope.kind {
        MaterialScopeKind::All => args.extend(["--scope".to_owned(), "all".to_owned()]),
        MaterialScopeKind::Region | MaterialScopeKind::Layer => {
            let value = scope
                .value
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "The selected material scope requires a value".to_owned())?;
            let flag = match scope.kind {
                MaterialScopeKind::Region => "--region",
                MaterialScopeKind::Layer => "--layer",
                MaterialScopeKind::All => unreachable!(),
            };
            args.extend([flag.to_owned(), value.to_owned()]);
        }
    }
    args.push("--json".to_owned());
    if include_container_items {
        args.push("--include-container-items".to_owned());
    }
    Ok(args)
}

fn replacement_args(
    source: &Path,
    rules: &Path,
    output: Option<&Path>,
    dry_run: bool,
) -> Vec<String> {
    let mut args = vec![
        "replace-blocks".to_owned(),
        "--input".to_owned(),
        path_text(source),
        "--rules".to_owned(),
        path_text(rules),
    ];
    if let Some(output) = output {
        args.extend(["--output".to_owned(), path_text(output)]);
    }
    if dry_run {
        args.push("--dry-run".to_owned());
    }
    args
}

fn generate_args(plan: &Path, output: Option<&Path>, dry_run: bool) -> Vec<String> {
    let mut args = vec!["generate".to_owned(), "--plan".to_owned(), path_text(plan)];
    if let Some(output) = output {
        args.extend(["--output".to_owned(), path_text(output)]);
    }
    if dry_run {
        args.push("--dry-run".to_owned());
    }
    args
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("encode Application operation input: {error}"))?;
    fs::write(path, bytes).map_err(|error| {
        format!(
            "write Application operation input {}: {error}",
            path.display()
        )
    })
}

fn read_output(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| {
            format!(
                "read Application operation output {}: {error}",
                path.display()
            )
        })
}

fn parse_core_json(output: &str) -> Result<Value, String> {
    serde_json::from_str(output.trim())
        .map_err(|error| format!("Core operation returned invalid product data: {error}"))
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn semantic_message(stdout: &str) -> String {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        "Operation completed".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn bounded_text(value: &str) -> String {
    let mut chars = value.chars();
    let bounded = chars
        .by_ref()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}\n[diagnostic truncated]")
    } else {
        bounded
    }
}

fn validate_png_dimensions(bytes: &[u8], width: u32, height: u32) -> Result<(), String> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err("Embedded preview did not produce a valid PNG header".to_owned());
    }
    let actual_width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let actual_height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if actual_width != width || actual_height != height {
        return Err(format!(
            "Embedded preview dimensions differ from its product metadata: {actual_width}x{actual_height} != {width}x{height}"
        ));
    }
    Ok(())
}

fn material_recipe_fetch_args(version: String) -> Vec<String> {
    // Stockpile recipe commands always emit JSON; their parser does not accept --json.
    vec!["stockpile".into(), "recipe-fetch".into(), "--minecraft-version".into(), version]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stockpile_saved_tasks_do_not_require_live_game_resources() {
        let file = std::env::temp_dir().join(format!("nova-stockpile-request-{}-{}.json",std::process::id(),SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()));
        let args = vec!["stockpile-task".into(),file.as_os_str().to_owned()];
        for action in ["open","list","select","mutate","rename","export","import"] {
            write_json(&file,&json!({"action":action})).unwrap(); assert!(args_request_snapshot_only(&args));
        }
        write_json(&file,&json!({"action":"settings","preferences":{"search":"stone"}})).unwrap(); assert!(args_request_snapshot_only(&args));
        for request in [json!({"action":"create"}),json!({"action":"sync"}),json!({"action":"details"}),json!({"action":"settings","multiplier":2}),json!({"action":"open","snapshot":{}})] {
            write_json(&file,&request).unwrap(); assert!(!args_request_snapshot_only(&args));
        }
        fs::remove_file(file).unwrap();
    }

    #[test]
    fn stockpile_source_content_identity_is_path_independent_and_revalidates_changes() {
        use crate::platform::process_supervisor::ExecutableAllowlist;
        let root = std::env::temp_dir().join(format!("nova-stockpile-identity-{}-{}",std::process::id(),SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(root.join("backend")).unwrap();
        let input = root.join("before.litematic"); fs::write(&input,b"compressed schematic sample").unwrap();
        let (supervisor,_) = ProcessSupervisor::new(ExecutableAllowlist::new()).unwrap();
        let operations = ApplicationOperations::new(Arc::new(supervisor),root.clone(),root.join("data"),root.join("backend")).unwrap();
        let first = operations.stockpile_project_id(&input).unwrap();
        assert_eq!(first,operations.stockpile_project_id(&input).unwrap());
        assert_eq!(operations.stockpile_project_ids.lock().unwrap().len(),1);
        let moved = root.join("renamed.litematic"); fs::rename(&input,&moved).unwrap();
        assert_eq!(first,operations.stockpile_project_id(&moved).unwrap());
        fs::write(&moved,b"edited compressed schematic sample with changed length").unwrap();
        assert_ne!(first,operations.stockpile_project_id(&moved).unwrap());
        assert_eq!(operations.lifecycle_snapshot()["coreCalls"],json!({}));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn stockpile_commands_accept_only_desktop_fields_and_keep_old_revision_distinct() {
        let command: StockpileTaskCommandInput = serde_json::from_value(json!({"action":"sync","taskId":"a".repeat(64),
            "expectedRevision":"saved-snapshot","sourcePath":"fixture.litematic","scope":{"kind":"all","value":null}})).unwrap();
        assert_eq!(command.expected_revision.as_deref(),Some("saved-snapshot"));
        assert!(command.source_revision.is_none());
        for forbidden in ["snapshot","databasePath","privateKey","commandLine"] {
            assert!(serde_json::from_value::<StockpileTaskCommandInput>(json!({"action":"mutate",(forbidden):{}})).is_err());
        }
        let historical: StockpileTaskInput = serde_json::from_value(json!({"taskId":"b".repeat(64)})).unwrap();
        assert!(historical.source_path.is_none());
    }

    #[test]
    fn desktop_task_identity_tracks_context_not_source_revision_or_row_order() {
        let all = MaterialScope { kind: MaterialScopeKind::All, value: None };
        let snapshot = json!({"metadata":{"minecraft_data_version":3953},"material_semantics":"minecraft-inventory-v9:resources-a","material_items":[{"count":10}]});
        let id = desktop_task_identity(Path::new("C:/one/source.litematic"), &all, false, 1, &snapshot);
        let mut edited = snapshot.clone(); edited["material_items"] = json!([{"count":20}]);
        assert_eq!(id, desktop_task_identity(Path::new("C:/one/source.litematic"), &all, false, 1, &edited));
        assert_ne!(id, desktop_task_identity(Path::new("C:/two/source.litematic"), &all, false, 1, &snapshot));
        assert_ne!(id, desktop_task_identity(Path::new("C:/one/source.litematic"), &all, true, 1, &snapshot));
        assert_ne!(id, desktop_task_identity(Path::new("C:/one/source.litematic"), &all, false, 2, &snapshot));
        let layer = MaterialScope { kind: MaterialScopeKind::Layer, value: Some("1".into()) };
        assert_ne!(id, desktop_task_identity(Path::new("C:/one/source.litematic"), &layer, false, 1, &snapshot));
        edited["material_semantics"] = json!("minecraft-inventory-v9:resources-b");
        assert_ne!(id, desktop_task_identity(Path::new("C:/one/source.litematic"), &all, false, 1, &edited));
        assert!(core_command_writes(&["stockpile-task".into()]));
    }

    #[test]
    fn layer_catalog_keeps_source_version_palette_properties_and_only_requested_slice() {
        let catalog = LayerCatalog::decode(json!({
            "metadata":{"minecraft_data_version":3953},
            "visual":{"chunk_size":16,"size_x":3,"size_y":2,"size_z":4,
                "palette":[{"block_id":"minecraft:oak_log","property_id":0}],"property_pool":[{"axis":"x"}],
                "layers":[{"y":0,"blocks":[{"x":2,"z":3,"palette_id":0}]},{"y":1,"blocks":[]}],"chunks":[]}
        })).unwrap();
        assert_eq!(catalog.meta.minecraft_data_version, Some(3953));
        assert_eq!(catalog.meta.property_pool[0]["axis"], "x");
        assert_eq!(catalog.layers[&0].blocks[0].x, 2);
        assert_eq!(catalog.layers[&0].blocks[0].z, 3);
        assert!(catalog.layers[&1].blocks.is_empty());
    }

    #[test]
    #[ignore = "CLI real-input integration; NOVA_INTEGRATION_CORE required"]
    #[cfg(windows)]
    fn restored_host_layer_preview_and_v2_replace_save_reload() {
        use crate::platform::process_supervisor::ExecutableAllowlist;
        let core = PathBuf::from(std::env::var_os("NOVA_INTEGRATION_CORE").unwrap());
        let source = PathBuf::from(r"C:\Users\27232\Documents\投影测试\16核刷铁机.litematic");
        let original = fs::read(&source).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
        let data = root.join(".tmp/beta-ui-corrections/restored-host-behavior");
        fs::create_dir_all(&data).unwrap();
        let input = data.join("source.litematic"); fs::copy(&source, &input).unwrap();
        let mut allow = ExecutableAllowlist::new(); allow.allow(ExecutableRole::CoreHelper, &core).unwrap();
        let (supervisor, _) = ProcessSupervisor::new(allow).unwrap();
        let operations = ApplicationOperations::new(Arc::new(supervisor), root, data.clone(), core.parent().unwrap().to_path_buf()).unwrap();
        let started = Instant::now();
        let meta = operations.read_layer_meta(InspectProjectInput { source_path: path_text(&input) }).unwrap();
        let first_ms = started.elapsed().as_secs_f64()*1000.0;
        assert_eq!(meta.minecraft_data_version, Some(3953));
        let hot = Instant::now();
        for y in [0, 1, meta.size_y-1] {
            let slice = operations.read_layer(ReadLayerInput { source_path: path_text(&input), y }).unwrap();
            let direct = std::process::Command::new(&core).args(layer_args(&input,y).unwrap()).output().unwrap();
            assert!(direct.status.success());
            let direct: LayerSliceData = decode_product(serde_json::from_slice(&direct.stdout).unwrap()).unwrap();
            assert_eq!(serde_json::to_value(slice).unwrap(), serde_json::to_value(direct).unwrap());
        }
        assert_eq!(operations.lifecycle_snapshot()["coreCalls"]["visual-layers-all"], 1);
        let layer_checks_ms = hot.elapsed().as_secs_f64()*1000.0;
        let preview_path = data.join("with-preview.litematic"); if preview_path.exists() { fs::remove_file(&preview_path).unwrap(); }
        operations.edit_project_metadata(EditProjectMetadataInput { source_path:path_text(&input),output_path:Some(path_text(&preview_path)),patch:json!({"preview_image_data":[0x80445566_u32 as i32]}) }).unwrap();
        let preview = operations.read_embedded_preview(InspectProjectInput { source_path:path_text(&preview_path) }).unwrap().unwrap();
        assert_eq!((preview.width,preview.height),(1,1)); assert!(preview.data_url.starts_with("data:image/png;base64,"));
        let source_id = meta.palette[0].block_id.clone();
        let output = data.join("serial-replaced.litematic"); if output.exists() { fs::remove_file(&output).unwrap(); }
        let unit = |from:&str,to:&str|json!({"input":[{"name":from,"properties":{}}],"output":[{"name":to,"properties":{},"weight":1}]});
        let result = operations.replace_blocks(ReplaceBlocksInput { source_path:path_text(&input),output_path:Some(path_text(&output)),rules:vec![],units:None,
            stages:Some(vec![vec![unit(&source_id,"minecraft:stone")],vec![unit("minecraft:stone","minecraft:gold_block")]]),dry_run:false }).unwrap();
        assert_eq!(result.output_path,Some(path_text(&output)));
        let before = operations.project_stats(ProjectStatsInput { source_path:path_text(&input),include_container_items:true,known_revision:None, include_structural_blocks:false }).unwrap();
        let after = operations.project_stats(ProjectStatsInput { source_path:path_text(&output),include_container_items:true,known_revision:None, include_structural_blocks:false }).unwrap();
        for field in ["total_non_air_blocks","total_regions","total_layers","total_entities","total_tile_entities"] {
            assert_eq!(before["structure_stats"][field],after["structure_stats"][field]);
        }
        assert_eq!(before["container_scan"],after["container_scan"]);
        assert!(after["structural_block_counts"].as_array().unwrap().iter().any(|row|row["block_id"]=="minecraft:gold_block"&&row["count"].as_u64().unwrap_or(0)>0));
        assert_eq!(operations.lifecycle_snapshot()["coreCalls"]["replace-blocks"],1);
        assert_eq!(fs::read(&input).unwrap(),original); assert_eq!(fs::read(&source).unwrap(),original);
        let report=json!({"source":path_text(&source),"version":3953,"sourceCatalogMs":first_ms,"independentLayerComparisonMs":layer_checks_ms,
            "layerSourceLoads":1,"selectedSlices":3,"layerExactOldCli":true,"previewImportRead":true,"serialStagesOneCore":true,"saveReload":true,"sourceUnchanged":true,"coreCalls":operations.lifecycle_snapshot()["coreCalls"]});
        write_json(&data.join("result.json"),&report).unwrap(); println!("[NOVA_RESTORED_HOST] {report}");
    }

    #[test]
    fn cached_scope_preserves_joint_channels_custom_limits_and_container_diagnostics() {
        let all = json!({
            "material_items":[{"block_id":"minecraft:stone","count":19,"total_count":19,"block_count":14,"container_item_count":5,"has_custom_stack_size":true,
                "region_counts":[],"layer_counts":[],"scope_counts":[
                    {"region":"A","layer":0,"block_count":2,"container_item_count":3,"has_custom_stack_size":true},
                    {"region":"A","layer":1,"block_count":7,"container_item_count":0,"has_custom_stack_size":false},
                    {"region":"B","layer":0,"block_count":5,"container_item_count":0,"has_custom_stack_size":false},
                    {"region":"B","layer":null,"block_count":0,"container_item_count":2,"has_custom_stack_size":true}]}],
            "non_item_blocks":[{"block_id":"minecraft:water","count":10,"scope_counts":[{"region":"A","layer":0,"count":4},{"region":"B","layer":1,"count":6}]}],
            "structural_block_counts":[
                {"block_id":"minecraft:stone","count":14,"scope_counts":[{"region":"A","layer":0,"count":2},{"region":"A","layer":1,"count":7},{"region":"B","layer":0,"count":5}]},
                {"block_id":"minecraft:water","count":10,"scope_counts":[{"region":"A","layer":0,"count":4},{"region":"B","layer":1,"count":6}]}],
            "regions":[{"name":"A","total_non_air_blocks":13,"unique_block_types":2},{"name":"B","total_non_air_blocks":11,"unique_block_types":2}],
            "layers_summary":[{"layer":0,"world_y":-4,"total_non_air_blocks":11,"unique_block_types":2},{"layer":1,"world_y":-3,"total_non_air_blocks":13,"unique_block_types":2}],
            "structure_stats":{"scope":{"kind":"all"},"total_non_air_blocks":24,"unique_block_types":2,"total_regions":2},
            "container_scan":{"enabled":true,"containers_scanned":2,"item_stacks_scanned":3,"warnings":["A0","missing-Y-content","B1"],"scope_entries":[
                {"region":"A","layer":0,"containers_scanned":1,"item_stacks_scanned":2,"warnings":["A0"],"missing_layer_warning":null},
                {"region":"B","layer":null,"containers_scanned":1,"item_stacks_scanned":1,"warnings":["missing-Y-content"],"missing_layer_warning":"missing numeric Y"},
                {"region":"B","layer":1,"containers_scanned":0,"item_stacks_scanned":0,"warnings":["B1"],"missing_layer_warning":null}]}
        });
        let region = material_snapshot_for_scope(all.clone(), &MaterialScope { kind: MaterialScopeKind::Region, value: Some("A".into()) }).unwrap();
        assert_eq!(region["material_items"][0]["block_count"], 9);
        assert_eq!(region["material_items"][0]["container_item_count"], 3);
        assert_eq!(region["material_items"][0]["has_custom_stack_size"], true);
        assert_eq!(region["material_items"][0]["layer_counts"], json!([{"layer":0,"world_y":-4,"count":5},{"layer":1,"world_y":-3,"count":7}]));
        assert_eq!(region["non_item_blocks"][0]["count"], 4);
        assert_eq!(region["structural_block_counts"], json!([{"block_id":"minecraft:stone","count":9},{"block_id":"minecraft:water","count":4}]));
        assert_eq!(region["container_scan"]["warnings"], json!(["A0"]));
        let layer = material_snapshot_for_scope(all.clone(), &MaterialScope { kind: MaterialScopeKind::Layer, value: Some("1".into()) }).unwrap();
        assert_eq!(layer["material_items"][0]["block_count"], 7);
        assert_eq!(layer["material_items"][0]["container_item_count"], 0);
        assert_eq!(layer["material_items"][0]["has_custom_stack_size"], false);
        assert_eq!(layer["material_items"][0]["region_counts"], json!([{"region":"A","count":7}]));
        assert_eq!(layer["non_item_blocks"][0]["count"], 6);
        assert_eq!(layer["structural_block_counts"], json!([{"block_id":"minecraft:stone","count":7},{"block_id":"minecraft:water","count":6}]));
        assert_eq!(layer["container_scan"]["warnings"], json!(["missing numeric Y","B1"]));
        assert_eq!(layer["container_scan"]["containers_scanned"], 0);
        assert_eq!(layer["structure_stats"]["scope"], json!({"kind":"layer","layer":1,"world_y":-3}));
        for selected in [region, layer, material_snapshot_for_scope(all.clone(), &MaterialScope { kind: MaterialScopeKind::All, value: None }).unwrap()] {
            let text = serde_json::to_string(&selected).unwrap();
            assert!(!text.contains("scope_counts")); assert!(!text.contains("scope_entries"));
        }
        assert!(material_snapshot_for_scope(all.clone(), &MaterialScope { kind: MaterialScopeKind::Layer, value: Some("2".into()) }).is_err());
        assert!(material_snapshot_for_scope(all, &MaterialScope { kind: MaterialScopeKind::Region, value: Some("missing".into()) }).is_err());
    }

    #[test]
    fn material_recipe_fetch_uses_actual_stockpile_cli_contract() {
        assert_eq!(material_recipe_fetch_args("26.3".into()), ["stockpile", "recipe-fetch", "--minecraft-version", "26.3"]);
    }

    #[test]
    #[ignore = "manual CLI real-input measurement; NOVA_STATS_FIXTURE and NOVA_STATS_CORE required"]
    #[cfg(windows)]
    fn real_stats_request_profile() {
        assert!(!cfg!(debug_assertions), "统计性能对账必须使用 --release，禁止用 Debug Host 对比 Release CLI");
        use crate::platform::process_supervisor::ExecutableAllowlist;
        let source = std::env::var("NOVA_STATS_FIXTURE").unwrap();
        let core = PathBuf::from(std::env::var_os("NOVA_STATS_CORE").unwrap());
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
        let data = root.join(".tmp/beta-ui-corrections/stats-profile-data");
        // Explicitly warm the same filesystem input for both transports; don't
        // label an uncontrolled Windows file cache as an OS-cold benchmark.
        let input_before = fs::read(&source).unwrap();
        let direct_started = Instant::now();
        let direct = std::process::Command::new(&core).args(stats_args(Path::new(&source), false))
              .current_dir(core.parent().unwrap()).env("LBA_DATA_ROOT", &data).env("NOVA_STATS_TIMING", "1").output().unwrap();
        let direct_ms = direct_started.elapsed().as_secs_f64() * 1000.0;
        assert!(direct.status.success(), "{}", String::from_utf8_lossy(&direct.stderr));
        eprint!("{}", String::from_utf8_lossy(&direct.stderr));
          let mut direct_snapshot: Value = serde_json::from_slice(&direct.stdout).unwrap();
            let core_snapshot_bytes = serde_json::to_vec(&direct_snapshot).unwrap().len();
          // Historical manual profiling compares the public DTO size, not the
          // Core's retained source index. Production routing no longer strips
          // that index in Host, so keep this transformation in the fixture.
          for name in ["material_items", "non_item_blocks", "structural_block_counts"] {
              if let Some(rows) = direct_snapshot[name].as_array_mut() {
                  for row in rows {
                      if let Some(row) = row.as_object_mut() { row.remove("scope_counts"); }
                  }
              }
          }
          if let Some(scan) = direct_snapshot["container_scan"].as_object_mut() { scan.remove("scope_entries"); }
            let dto_snapshot_bytes = serde_json::to_vec(&direct_snapshot).unwrap().len();
            let production_stage_log = String::from_utf8_lossy(&direct.stderr).lines().filter(|line|line.contains("NOVA_STATS_TIMING")).map(str::to_owned).collect::<Vec<_>>();
        let mut allowed = ExecutableAllowlist::new();
        allowed.allow(ExecutableRole::CoreHelper, &core).unwrap();
        let (supervisor, _events) = ProcessSupervisor::new(allowed).unwrap();
        let operations = ApplicationOperations::new(Arc::new(supervisor), root, data, core.parent().unwrap().to_path_buf()).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let started = Instant::now();
        let workers = (0..2).map(|_| {
            let operations = operations.clone();
            let source = source.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                operations.project_stats(ProjectStatsInput { source_path: source, include_container_items: false, known_revision: None, include_structural_blocks:false }).unwrap()
            })
        }).collect::<Vec<_>>();
        let results = workers.into_iter().map(|worker| worker.join().unwrap()).collect::<Vec<_>>();
        assert_eq!(results[0], results[1]);
        let cold_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut host_snapshot = results[0].clone();
        host_snapshot.as_object_mut().unwrap().remove("source_revision");
        assert_eq!(host_snapshot, direct_snapshot, "CLI 与真实 Application operation 的范围、容器和所有明细必须完全一致");
        assert_eq!(results[0]["structure_stats"]["total_non_air_blocks"], 88534);
        write_json(&operations.data_root.join("snapshot.json"), &results[0]).unwrap();
        let revision = results[0]["source_revision"].as_str().unwrap().to_owned();
        let hot_started = Instant::now();
        let hot = operations.project_stats(ProjectStatsInput { source_path: source.clone(), include_container_items: false, known_revision: Some(revision), include_structural_blocks:false }).unwrap();
        let hot_ms = hot_started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(hot["cache_unchanged"], true);
        assert_eq!(operations.next_generation.load(Ordering::Relaxed) - 1, 1);
          let mut scope_checks = Vec::new();
          for include_containers in [false, true] {
              let before = operations.next_generation.load(Ordering::Relaxed);
              let all = operations.project_stats(ProjectStatsInput { source_path: source.clone(), include_container_items: include_containers, known_revision: None, include_structural_blocks:false }).unwrap();
              let region = all["regions"].as_array().unwrap().iter().max_by_key(|row| row["total_non_air_blocks"].as_u64()).unwrap()["name"].as_str().unwrap().to_owned();
              let layer = all["layers_summary"].as_array().unwrap().iter().max_by_key(|row| row["total_non_air_blocks"].as_u64()).unwrap()["layer"].as_i64().unwrap().to_string();
              for scope in [MaterialScope { kind: MaterialScopeKind::Region, value: Some(region) }, MaterialScope { kind: MaterialScopeKind::Layer, value: Some(layer) }] {
                  let selected_at = Instant::now();
                  let selected = operations.material_snapshot(Path::new(&source), &scope, include_containers).unwrap();
                  let cached_scope_ms = selected_at.elapsed().as_secs_f64() * 1000.0;
                  let cli = std::process::Command::new(&core).args(materials_args(Path::new(&source), &scope, include_containers).unwrap())
                      .current_dir(core.parent().unwrap()).env("LBA_DATA_ROOT", &operations.data_root).output().unwrap();
                  assert!(cli.status.success(), "{}", String::from_utf8_lossy(&cli.stderr));
                  let independent: Value = serde_json::from_slice(&cli.stdout).unwrap();
                  assert_eq!(selected, independent, "缓存范围必须与独立 CLI 的结构、材料两通道、跨选区/层明细、容器计数及诊断完全一致");
                  assert!(!serde_json::to_string(&selected).unwrap().contains("scope_counts"));
                  scope_checks.push(json!({"scope":selected["structure_stats"]["scope"],"includeContainers":include_containers,"cachedScopeMs":cached_scope_ms,"exactCliMatch":true}));
              }
              assert_eq!(operations.next_generation.load(Ordering::Relaxed) - before, u64::from(include_containers), "同参数选范围不能重复启动 Core");
          }
          assert_eq!(operations.next_generation.load(Ordering::Relaxed) - 1, 2, "只允许容器开关的两个不同分析快照");
        assert_eq!(fs::read(&source).unwrap(), input_before);
        let baseline_path = operations.project_root.join(".tmp/beta-ui-corrections/beta02-stats-release-baseline-snapshot.json");
        if baseline_path.is_file() {
            let mut baseline: Value = serde_json::from_slice(&fs::read(baseline_path).unwrap()).unwrap();
            let mut current = direct_snapshot.clone();
            baseline.as_object_mut().unwrap().remove("material_semantics");
            current.as_object_mut().unwrap().remove("material_semantics");
            let structural = current.as_object_mut().unwrap().remove("structural_block_counts").unwrap();
            assert_eq!(structural.as_array().unwrap().iter().map(|row| row["count"].as_u64().unwrap()).sum::<u64>(), 88534);
            assert!(structural.as_array().unwrap().iter().any(|row| row["block_id"] == "minecraft:piston_head" && row["count"] == 2851));
            assert_eq!(current, baseline, "真实840基线的全部结构、材料、堆叠、区域/层与容器明细不能减少");
        }
        let report = json!({ "profile": "release", "scope": "all", "includeContainers": false,
            "cacheCondition": "OS explicitly warmed; Application cold then validated hot",
            "directCliMs": direct_ms, "applicationColdMs": cold_ms, "applicationHotMs": hot_ms,
            "directPayloadBytes": direct.stdout.len(), "hotPayloadBytes": serde_json::to_vec(&hot).unwrap().len(),
              "coreSnapshotCompactBytes":core_snapshot_bytes,"dtoCompactBytes":dto_snapshot_bytes,"internalScopeCacheBytes":core_snapshot_bytes-dto_snapshot_bytes,
              "productionCoreStageLog":production_stage_log,"applicationCoreCalls":operations.lifecycle_snapshot()["coreCalls"],
            "coreLaunches": operations.next_generation.load(Ordering::Relaxed) - 1,
              "scopeChecks": scope_checks,
            "nonAir": results[0]["structure_stats"]["total_non_air_blocks"], "materials": results[0]["material_items"].as_array().unwrap().len(),
            "WebViewIPC": "UNMEASURED", "ReactCommit": "UNMEASURED" });
        write_json(&operations.project_root.join(".tmp/beta-ui-corrections/beta02-stats-release-modified.json"), &report).unwrap();
        println!("[NOVA_STATS_PROFILE] {report}");
    }

    #[test]
    #[cfg(windows)]
    fn shutdown_drains_writes_and_can_cancel_a_deferred_close_without_data_loss() {
        use crate::platform::process_supervisor::ExecutableAllowlist;
        let command = PathBuf::from(std::env::var_os("COMSPEC").unwrap());
        let mut allowed = ExecutableAllowlist::new();
        allowed.allow(ExecutableRole::CoreHelper, &command).unwrap();
        let (supervisor, _) = ProcessSupervisor::new(allowed).unwrap();
        let root = std::env::temp_dir().join(format!("nova-close-write-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let operations = ApplicationOperations::new(Arc::new(supervisor), root.clone(), root.clone(), command.parent().unwrap().to_path_buf()).unwrap();
        let write = operations.begin_write().unwrap();
        assert!(operations.begin_shutdown().is_empty());
        assert!(operations.begin_write().is_err());
        assert!(!operations.wait_for_writes(Duration::ZERO));
        let destination = root.join("retained-user-write.txt");
        let saved_path = destination.clone();
        let worker = std::thread::spawn(move || {
            use std::io::Write;
            let mut file = fs::File::create(saved_path).unwrap();
            file.write_all(b"existing save completed before process teardown").unwrap();
            std::thread::sleep(Duration::from_millis(25));
            file.sync_all().unwrap();
            drop(file); drop(write);
        });
        let started = Instant::now();
        assert!(operations.wait_for_writes(Duration::from_secs(1)));
        worker.join().unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"existing save completed before process teardown");
        println!("[NOVA_LIFECYCLE_PROFILE] case=save write_drained_ms={} file_synced=true pending_writes=0 source_projection=UNCHANGED window_response=UNMEASURED", started.elapsed().as_millis());
        operations.cancel_shutdown();
        assert!(operations.begin_write().is_ok());
        let absolute = fs::canonicalize(&root).unwrap();
        assert!(absolute.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("nova-close-write-"));
        fs::remove_dir_all(absolute).unwrap();
    }

    #[test]
    #[ignore = "actual Release Core read cancellation; NOVA_STATS_FIXTURE and NOVA_STATS_CORE required"]
    #[cfg(windows)]
    fn real_core_read_is_cancelled_and_owned_process_is_reaped_on_close() {
        use crate::platform::process_supervisor::ExecutableAllowlist;
        let source = std::env::var("NOVA_STATS_FIXTURE").unwrap();
        let core = PathBuf::from(std::env::var_os("NOVA_STATS_CORE").unwrap());
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
        let input_before = fs::read(&source).unwrap();
        let mut allowed = ExecutableAllowlist::new(); allowed.allow(ExecutableRole::CoreHelper, &core).unwrap();
        let (supervisor, _) = ProcessSupervisor::new(allowed).unwrap();
        let supervisor = Arc::new(supervisor);
        let operations = ApplicationOperations::new(Arc::clone(&supervisor), root.clone(), root.join(".tmp/beta-ui-corrections/close-read-data"), core.parent().unwrap().to_path_buf()).unwrap();
        let running = operations.clone(); let running_source = source.clone();
        let worker = std::thread::spawn(move || running.project_stats(ProjectStatsInput { source_path: running_source, include_container_items: true, known_revision: None, include_structural_blocks:false }));
        let deadline = Instant::now() + Duration::from_secs(2);
        while operations.lifecycle.0.lock().unwrap().reads.is_empty() && !worker.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let started = Instant::now();
        let reads = operations.begin_shutdown();
        assert!(operations.wait_for_writes(Duration::ZERO));
        let requested = supervisor.terminate_many(&reads, 0);
        let report = supervisor.shutdown(Duration::ZERO, Duration::from_millis(1500));
        let result = worker.join().unwrap();
        assert!(report.remaining.is_empty()); assert!(supervisor.active_children().is_empty());
        assert!(operations.lifecycle.0.lock().unwrap().reads.is_empty());
        assert!(operations.inspect_project(InspectProjectInput { source_path: source.clone() }).is_err(), "closing cannot start another read");
        assert_eq!(fs::read(&source).unwrap(), input_before);
        println!("[NOVA_LIFECYCLE_PROFILE] case=statistics_read actual_release_core=true tracked_reads={} terminate_requests={} read_result={} all_owned_exit_ms={} remaining=0 window_response=UNMEASURED", reads.len(), requested, if result.is_err() {"cancelled"} else {"completed_at_checkpoint"}, started.elapsed().as_millis());
    }

    #[test]
    fn shutdown_only_protects_real_user_writes_not_preview_reads_or_replace_dry_runs() {
        for command in ["stats", "analyze", "visual-meta", "projection-markers"] {
            assert!(!core_command_writes(&[command.into()]));
        }
        assert!(!core_command_writes(&["replace-blocks".into(), "--dry-run".into()]));
        assert!(!core_command_writes(&["version-convert".into(), "--request".into(), "preview.json".into()]));
        for command in ["edit-metadata", "set-origin", "remove-origin", "materials-export", "generate", "replace-blocks"] {
            assert!(core_command_writes(&[command.into()]));
        }
        assert!(core_command_writes(&["version-convert".into(), "--output".into(), "new.litematic".into()]));
    }

    #[test]
    fn statistics_cache_revalidates_file_revisions_and_separates_parameters() {
        let temp = std::env::temp_dir().join(format!("nova-stats-key-{}", std::process::id()));
        fs::write(&temp, b"first").unwrap();
        let all = AnalysisKey::new(&temp, vec!["stats".into()]).unwrap();
        let containers = AnalysisKey::new(&temp, vec!["stats".into(), "--include-container-items".into()]).unwrap();
        assert_ne!(all, containers);
        fs::write(&temp, b"changed size").unwrap();
        assert_ne!(all, AnalysisKey::new(&temp, vec!["stats".into()]).unwrap());
        fs::remove_file(temp).unwrap();
    }

    #[test]
    fn statistics_singleflight_has_one_leader_and_same_success_for_waiters() {
        let slot = Arc::new(OnceLock::<Result<Value, String>>::new());
        let calls = Arc::new(AtomicU64::new(0));
        let handles = (0..4).map(|_| {
            let slot = Arc::clone(&slot); let calls = Arc::clone(&calls);
            std::thread::spawn(move || slot.get_or_init(|| {
                calls.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(10)); Ok(json!({"material_semantics":"minecraft-inventory-v3"}))
            }).clone())
        }).collect::<Vec<_>>();
        for handle in handles { assert!(handle.join().unwrap().is_ok()); }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn flake_requests_reject_process_and_cache_fields() {
        assert!(serde_json::from_value::<InspectProjectInput>(json!({
            "sourcePath": "scene.litematic", "cachePath": "cache.bin"
        })).is_err());
        assert!(serde_json::from_value::<ReadLayerInput>(json!({
            "sourcePath": "scene.litematic", "y": 1, "args": ["--help"]
        })).is_err());
        assert!(serde_json::from_value::<ReadLayerContainerInput>(json!({
            "sourcePath": "scene.litematic", "x": 0, "y": 1, "z": 2, "executable": "other.exe"
        })).is_err());
        assert!(serde_json::from_value::<EditProjectOriginInput>(json!({
            "sourcePath": "scene.litematic", "origin": { "x": 0, "y": 1, "z": 2, "args": [] }, "outputPath": null
        })).is_err());
        assert!(serde_json::from_value::<EditProjectOriginInput>(json!({
            "sourcePath": "scene.litematic", "origin": null, "outputPath": null, "args": []
        })).is_err());
    }

    #[test]
    fn flake_coordinates_are_bounded_integers() {
        for y in [json!(0.5), json!(2147483648_i64), json!("--help")] {
            assert!(serde_json::from_value::<ReadLayerInput>(json!({
                "sourcePath": "scene.litematic", "y": y
            })).is_err());
        }
        assert!(layer_args(Path::new("scene.litematic"), -1).is_err());
        assert!(layer_container_args(Path::new("scene.litematic"), 0, 0, -1).is_err());
    }

    #[test]
    fn flake_arguments_use_source_not_render_cache_and_layer_coordinates() {
        let source = Path::new(r"C:\projection files\scene.litematic");
        assert_eq!(layer_args(source, 42).unwrap(), vec!["visualize-layer", r"C:\projection files\scene.litematic", "--y=42"]);
        assert_eq!(layer_container_args(source, 2, 42, 3).unwrap(), vec!["read-layer-container", r"C:\projection files\scene.litematic", "--x=2", "--y=42", "--z=3"]);
        assert_eq!(origin_args(source, None, None), vec!["remove-origin", r"C:\projection files\scene.litematic"]);
        assert_eq!(origin_args(source, Some(ProjectionMarkerPoint { x: -4, y: 5, z: 6 }), Some(Path::new(r"C:\copy.litematic"))), vec!["set-origin", r"C:\projection files\scene.litematic", "--x=-4", "--y=5", "--z=6", r"--output=C:\copy.litematic"]);
    }

    #[test]
    fn flake_output_protection_allows_in_place_and_new_files_only() {
        let root = std::env::temp_dir().join(format!("nova-origin-paths-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source.litematic");
        let occupied = root.join("occupied.litematic");
        let new_file = root.join("new.litematic");
        fs::write(&source, b"source").unwrap();
        fs::write(&occupied, b"existing").unwrap();
        assert!(protected_origin_paths(&source, None).unwrap().1.is_none());
        assert!(protected_origin_paths(&source, Some(&source)).is_err());
        assert!(protected_origin_paths(&source, Some(&root.join(".").join("source.litematic"))).is_err());
        #[cfg(windows)]
        assert!(protected_origin_paths(&source, Some(&root.join("SOURCE.LITEMATIC"))).is_err());
        assert!(protected_origin_paths(&source, Some(&occupied)).is_err());
        assert!(protected_origin_paths(&source, Some(&root)).is_err());
        assert!(protected_origin_paths(&source, Some(&root.join("missing/new.litematic"))).is_err());
        assert!(protected_origin_paths(&source, Some(&new_file)).unwrap().1.is_some());
        assert!(!new_file.exists());
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert_eq!(fs::read(&occupied).unwrap(), b"existing");
        fs::remove_file(source).unwrap();
        fs::remove_file(occupied).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn flake_typed_responses_preserve_null_and_reject_malformed_payloads() {
        assert!(decode_product::<Option<LayerContainerData>>(Value::Null).unwrap().is_none());
        assert!(decode_product::<LayerSliceData>(json!({ "y": 3, "blocks": [] })).is_ok());
        assert!(decode_product::<LayerSliceData>(json!({ "y": 3, "blocks": null })).is_err());
        assert!(decode_product::<ProjectionMarkerEditSummary>(json!({
            "input": "a", "output": "b", "backup_path": null, "origin": { "x": -1, "y": 2, "z": 3 }
        })).is_ok());
    }

    #[test]
    fn replacement_always_uses_the_request_owned_rules_file() {
        let args = replacement_args(
            Path::new(r"C:\scene.litematic"),
            Path::new(r"C:\runtime\replace-rules.json"),
            Some(Path::new(r"C:\scene-replaced.litematic")),
            false,
        );
        assert_eq!(args[0], "replace-blocks");
        assert_eq!(args[3], "--rules");
        assert_eq!(args[4], r"C:\runtime\replace-rules.json");
        assert_eq!(args[5], "--output");
        assert!(!args.contains(&"--dry-run".to_owned()));
    }

    #[test]
    fn material_scope_is_translated_from_product_semantics() {
        let args = materials_args(
            Path::new(r"C:\scene.litematic"),
            &MaterialScope {
                kind: MaterialScopeKind::Layer,
                value: Some("42".to_owned()),
            },
            true,
        )
        .unwrap();
        assert!(args.windows(2).any(|pair| pair == ["--layer", "42"]));
        assert!(args.contains(&"--include-container-items".to_owned()));
    }

    #[test]
    fn embedded_preview_rejects_mismatched_dimensions() {
        let mut png = vec![0_u8; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&320_u32.to_be_bytes());
        png[20..24].copy_from_slice(&180_u32.to_be_bytes());
        assert!(validate_png_dimensions(&png, 320, 180).is_ok());
        assert!(validate_png_dimensions(&png, 640, 360).is_err());
    }

    #[test]
    fn generation_does_not_expose_its_temporary_plan_as_product_output() {
        let mut value = json!({ "plan_path": "C:/temp/internal.json", "dry_run": true });
        value.as_object_mut().unwrap().remove("plan_path");
        assert!(value.get("plan_path").is_none());
    }
}
