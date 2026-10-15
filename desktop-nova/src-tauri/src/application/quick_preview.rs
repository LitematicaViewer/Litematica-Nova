//! Independent historical fast_experimental preview. Never enters the V5 build/admission path.
use super::model::PresentationPlacement;
use crate::platform::process_supervisor::{ChildLease, ExecutableRole, LaunchIdentity, LaunchSpec, LaunchToken, OutputTarget, ProcessSupervisor, WaitOutcome};
use crate::viewer_backend::ViewerBackendIdentity;
use crate::viewer_launch_contract::ViewerLaunchContractFile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use viewer_launch_contract::{QuickPreviewHostWindow, QuickPreviewTask, ViewerContent, ViewerLaunchContract, ViewerSessionCommand, ViewerSessionEvent, ViewerSessionState, ViewerSessionStatus, ViewerTask};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QuickPresentationMode { #[default] Hidden, Embedded }

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct QuickPresentationIntent {
    pub(crate) mode: QuickPresentationMode,
    pub(crate) visible: bool,
    pub(crate) placement: Option<PresentationPlacement>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuickPreviewView {
    pub(crate) preview_id: String,
    pub(crate) source_path: PathBuf,
    pub(crate) render_mode: &'static str,
    pub(crate) state: &'static str,
    pub(crate) presentation: QuickPresentationIntent,
    pub(crate) presentation_revision: u64,
    pub(crate) visible: bool,
    pub(crate) error: Option<String>,
    pub(crate) preload: bool,
    pub(crate) resource: Option<QuickResourceReport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuickResourceReport {
    schema: String,
    request_id: String,
    source_bytes: u64,
    preload: bool,
    pressure: String,
    pressure_basis_points: Option<u64>,
    budget: Option<serde_json::Value>,
    estimate: Option<serde_json::Value>,
    invalid_reason: Option<String>,
    pub(crate) decision_required: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QuickResourceAction { Preload, Realtime, ReducedRealtime, Cancel }

fn validate_resource_decision(report: &QuickResourceReport, action: QuickResourceAction, warning_confirmed: bool) -> Result<(), String> {
    if matches!(action, QuickResourceAction::Cancel) { return Ok(()); }
    if !matches!(report.pressure.as_str(), "unmeasured" | "normal" | "warning" | "hard" | "invalid") {
        return Err("快速预览资源报告无效，请重新加载。".into());
    }
    if matches!(action, QuickResourceAction::Preload) {
        if report.estimate.is_none() || report.budget.is_none() {
            return Err("快速模式尚未获得完整投影显存需求估算，请选择实时加载或取消。".into());
        }
        if report.pressure != "normal" && !(report.pressure == "warning" && warning_confirmed) {
            return Err("当前显存需求未获预加载准入，请选择实时加载或取消。".into());
        }
    }
    if matches!(action, QuickResourceAction::ReducedRealtime) && report.estimate.is_none() {
        return Err("显存需求尚未确认，请选择实时加载或取消。".into());
    }
    Ok(())
}

fn publish_resource_decision(path: &Path, id: &str, action: QuickResourceAction, warning_confirmed: bool) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let value = serde_json::json!({ "schema": "nova.quick.decision.v1", "requestId": id, "action": action, "warningConfirmed": warning_confirmed });
    fs::write(&temporary, serde_json::to_vec(&value).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    if path.exists() { fs::remove_file(path).map_err(|error| error.to_string())?; }
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

impl QuickPreviewView {
    fn accepts_presentation(&self, revision: u64) -> bool { revision > self.presentation_revision }

    fn apply_status(&mut self, state: &ViewerSessionState, latest: bool) {
        if self.state == "closed" { return; }
        match state {
            ViewerSessionState::Ready if latest => self.state = "ready",
            ViewerSessionState::SceneReady { .. } if latest => self.state = "ready",
            ViewerSessionState::Attached { .. } if latest => self.visible = self.presentation.visible,
            ViewerSessionState::Hidden if latest => self.visible = false,
            ViewerSessionState::Released if latest => { self.state = "closed"; self.visible = false; }
            ViewerSessionState::Error { command_kind, message } if latest => {
                self.error = Some(message.clone());
                if matches!(command_kind, viewer_launch_contract::ViewerSessionCommandKind::ReplaceScene) { self.state = "failed"; }
            },
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct StartQuickPreviewInput {
    pub(crate) source_path: PathBuf,
    #[serde(default)] pub(crate) preload: bool,
    #[serde(default)] pub(crate) click_started_at_epoch_ms: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct QuickPreviewHost {
    supervisor: Arc<ProcessSupervisor>,
    viewer_exe: PathBuf,
    data_root: PathBuf,
    sequence: Arc<AtomicU64>,
    session: Arc<Mutex<Option<QuickSession>>>,
    pending_resource: Arc<Mutex<Option<PendingQuickResource>>>,
}

struct PendingQuickResource {
    view: QuickPreviewView,
    input: StartQuickPreviewInput,
    host_window: QuickPreviewHostWindow,
    source_bytes: u64,
    source_modified: Option<SystemTime>,
    decision_path: PathBuf,
}

struct QuickSession {
    view: QuickPreviewView,
    session_id: String,
    child: ChildLease,
    contract: ViewerLaunchContractFile,
    backend: ViewerBackendIdentity,
    directory: PathBuf,
    stdout: PathBuf,
    stderr: PathBuf,
    source_bytes: u64,
    source_modified: Option<SystemTime>,
    requested_preload: bool,
    pending_click_epoch_ms: Option<u64>,
    next_command: u64,
    next_status: u64,
    next_event: u64,
    pending_intents: BTreeMap<u64, QuickPresentationIntent>,
    deferred_presentation: bool,
    pending_replacements: BTreeMap<u64, ViewerLaunchContractFile>,
    capture_errors: BTreeMap<u64, String>,
    host_window: QuickPreviewHostWindow,
    launch_accepted: bool,
    gpu_context_only: bool,
    gpu_ready_observed_epoch_ms: Option<u64>,
    native_hwnd: Option<isize>,
    resource_report_path: PathBuf,
    resource_decision_path: PathBuf,
    resource_decided: bool,
}

impl QuickPreviewHost {
    pub(crate) fn thumbnail_session(&self) -> Self {
        Self { session: Arc::new(Mutex::new(None)), pending_resource: Arc::new(Mutex::new(None)), ..self.clone() }
    }
    pub(crate) fn new(supervisor: Arc<ProcessSupervisor>, viewer_exe: PathBuf, data_root: PathBuf) -> Self {
        Self { supervisor, viewer_exe, data_root, sequence: Arc::new(AtomicU64::new(1)), session: Arc::new(Mutex::new(None)), pending_resource: Arc::new(Mutex::new(None)) }
    }

    /// Normal startup initializes only static GPU resources asynchronously.
    /// No projection is read or prepared until an actual user start request.
    pub(crate) fn prepare_gpu_context(&self, host_window: QuickPreviewHostWindow) -> Result<(), String> {
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        if slot.is_some() { return Ok(()); }
        let started = std::time::Instant::now();
        let epoch_ms = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_err(|error| error.to_string())?.as_millis();
        let generation = self.sequence.fetch_add(1, Ordering::Relaxed);
        let id = format!("quick-{}-{generation}", self.supervisor.owner_epoch().get());
        let directory = self.data_root.join("runtime").join("quick-preview").join(&id);
        let session_directory = directory.join("session");
        fs::create_dir_all(&session_directory).map_err(|error| error.to_string())?;
        let backend = ViewerBackendIdentity::capture(&self.viewer_exe)?;
        let launch = ViewerLaunchContract::new(id.clone(), self.data_root.clone(), ViewerContent::default(), None,
            ViewerTask::QuickGpuContext(viewer_launch_contract::QuickGpuContextTask { host: host_window.clone() }))?;
        let contract = ViewerLaunchContractFile::create(directory.join("launch.json"), &launch, &backend)?;
        let stdout = super::diagnostics::log_path("quick-preview", &format!("{id}-stdout.log"), &directory)?;
        let stderr = super::diagnostics::log_path("quick-preview", &format!("{id}-stderr.log"), &directory)?;
        let mut spec = LaunchSpec::new(ExecutableRole::NativeViewer, LaunchIdentity {
            owner_epoch: self.supervisor.owner_epoch(), generation,
            launch_token: LaunchToken::new(id.clone()).map_err(|error| error.to_string())?,
        });
        spec.args.push(contract.command_arg().into());
        spec.environment = vec![("LITEMATICA_NOVA_VIEWER_SESSION_DIRECTORY".into(), session_directory.as_os_str().to_owned()),
            ("LITEMATICA_NOVA_VIEWER_SESSION_ID".into(), id.clone().into())];
        spec.current_dir = self.viewer_exe.parent().map(Path::to_path_buf);
        spec.stdout = OutputTarget::File { path: stdout.clone(), append: false };
        spec.stderr = OutputTarget::File { path: stderr.clone(), append: false };
        let child = self.supervisor.spawn(spec).map_err(|error| error.to_string())?.child;
        let view = QuickPreviewView { preview_id: id.clone(), source_path: self.data_root.clone(), render_mode: "fast_experimental", state: "starting",
            presentation: QuickPresentationIntent::default(), presentation_revision: 0, visible: false, error: None, preload: false, resource: None };
        *slot = Some(QuickSession { view, session_id: id.clone(), child, contract, backend, directory: session_directory.clone(), stdout, stderr,
            source_bytes: 0, source_modified: None, requested_preload: false, pending_click_epoch_ms: None, next_command: 1, next_status: 0, next_event: 1,
            pending_intents: BTreeMap::new(), deferred_presentation: false, pending_replacements: BTreeMap::new(), capture_errors: BTreeMap::new(), host_window, launch_accepted: false,
            gpu_context_only: true, gpu_ready_observed_epoch_ms: None, native_hwnd: None,
            resource_report_path: session_directory.join("resource-report.json"), resource_decision_path: session_directory.join("resource-decision.json"), resource_decided: false });
        eprintln!("[NOVA_GPU_INITIALIZATION] stage=ordinary_startup_requested epoch_ms={epoch_ms} launch_dispatch_ms={:.3} session_id={id} asynchronous=true projection_reads=0 scene_mesh_builds=0", started.elapsed().as_secs_f64()*1000.0);
        Ok(())
    }

    pub(crate) fn start(&self, input: StartQuickPreviewInput, host_window: QuickPreviewHostWindow) -> Result<QuickPreviewView, String> {
        let source = fs::canonicalize(&input.source_path).map_err(|error| format!("读取快速预览投影失败：{error}"))?;
        let meta = fs::metadata(&source).map_err(|error| error.to_string())?;
        if !meta.is_file() || !source.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("litematic")) {
            return Err("快速预览需要现有的 .litematic 文件。".to_owned());
        }
        if input.preload {
            let mut pending = self.pending_resource.lock().map_err(|_| "快速预览显存决策锁异常")?;
            if let Some(request) = pending.as_ref().filter(|request| request.view.state == "starting"
                && request.view.source_path == source && request.source_bytes == meta.len()
                && request.source_modified == meta.modified().ok() && request.host_window == host_window) {
                return Ok(request.view.clone());
            }
            let generation = self.sequence.fetch_add(1, Ordering::Relaxed);
            let id = format!("quick-{}-resource-{generation}", self.supervisor.owner_epoch().get());
            let directory = self.data_root.join("runtime").join("quick-preview").join(&id);
            fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            let resource = QuickResourceReport { schema: "nova.quick.resource.v1".into(), request_id: id.clone(),
                source_bytes: meta.len(), preload: true, pressure: "unmeasured".into(), pressure_basis_points: None,
                budget: None, estimate: None, invalid_reason: Some("快速模式尚未获得完整投影显存需求估算；静态 GPU 资源预算不代表投影需求。请选择实时加载或取消。".into()), decision_required: true };
            let view = QuickPreviewView { preview_id: id.clone(), source_path: source.clone(), render_mode: "fast_experimental",
                state: "starting", presentation: QuickPresentationIntent::default(), presentation_revision: 0,
                visible: false, error: None, preload: true, resource: Some(resource.clone()) };
            // No ReplaceScene or full projection work has been submitted. Only
            // hide the old presentation; its Viewer and GPU remain owned.
            let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常")?;
            if let Some(session) = slot.as_mut() {
                if !session.gpu_context_only {
                    session.publish_command(&ViewerSessionCommand::hide(session.session_id.clone(), session.next_command)?)?;
                    session.next_command += 1;
                    session.pending_intents.clear(); session.deferred_presentation = false;
                    session.view.visible = false; session.view.presentation = QuickPresentationIntent::default();
                }
            }
            super::diagnostics::record_resource(&id, &resource);
            eprintln!("[QUICK_RESOURCE] stage=await_explicit_choice request_id={id} estimate=unknown requested_preload=true preload_admitted=false scene_replace_commands=0 new_viewers=0");
            *pending = Some(PendingQuickResource { view: view.clone(), input: StartQuickPreviewInput { source_path: source, ..input },
                host_window, source_bytes: meta.len(), source_modified: meta.modified().ok(), decision_path: directory.join("resource-decision.json") });
            return Ok(view);
        }
        self.pending_resource.lock().map_err(|_| "快速预览显存决策锁异常")?.take();
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        if let Some(session) = slot.as_mut() {
            if let Err(error) = session.refresh(&self.supervisor) {
                self.stop_session(session)?;
                session.view.state = "failed";
                session.view.error = Some(error);
            }
            if session.host_window == host_window && session.view.source_path == source && session.source_bytes == meta.len()
                && session.source_modified == meta.modified().ok() && session.view.preload == input.preload && matches!(session.view.state, "starting" | "ready") {
                session.pending_click_epoch_ms = input.click_started_at_epoch_ms;
                return Ok(session.view.clone());
            }
            let reusable_gpu = session.gpu_ready_observed_epoch_ms.is_some()
                && matches!(self.supervisor.wait_for_exit(&session.child, Duration::ZERO), WaitOutcome::TimedOut);
            if session.host_window == host_window && (reusable_gpu && matches!(session.view.state, "starting" | "ready" | "closed")
                || session.gpu_context_only && session.view.state == "starting") {
                if session.view.state == "starting" && !session.gpu_context_only {
                    publish_resource_decision(&session.resource_decision_path, &session.session_id, QuickResourceAction::Cancel, false)?;
                }
                let generation = self.sequence.fetch_add(1, Ordering::Relaxed);
                let preview_id = format!("{}-scene-{generation}", session.session_id);
                let replacement = session.directory.join(format!("replace-{generation:020}.launch.json"));
                let resource_report_path = session.directory.join(format!("resource-report-{generation:020}.json"));
                let resource_decision_path = session.directory.join(format!("resource-decision-{generation:020}.json"));
                let launch = ViewerLaunchContract::new(session.session_id.clone(), source.clone(), ViewerContent::default(), None,
                    ViewerTask::QuickPreview(QuickPreviewTask { initial_visible: false, host: Some(host_window), preload: input.preload,
                        resource_report_path: Some(resource_report_path.clone()), resource_decision_path: Some(resource_decision_path.clone()) }))?;
                let replacement_contract = ViewerLaunchContractFile::create(replacement.clone(), &launch, &session.backend)?;
                let hide = ViewerSessionCommand::hide(session.session_id.clone(), session.next_command)?;
                session.publish_command(&hide)?;
                session.next_command += 1;
                let replace = ViewerSessionCommand::replace_scene(session.session_id.clone(), session.next_command, replacement)?;
                session.publish_command(&replace)?;
                session.pending_replacements.insert(replace.sequence, replacement_contract);
                session.next_command += 1;
                session.view = QuickPreviewView { preview_id, source_path: source, render_mode: "fast_experimental", state: "starting",
                    presentation: QuickPresentationIntent::default(), presentation_revision: 0, visible: false, error: None, preload: input.preload, resource: None };
                session.source_bytes = meta.len();
                session.source_modified = meta.modified().ok();
                session.requested_preload = input.preload;
                session.pending_click_epoch_ms = input.click_started_at_epoch_ms;
                session.resource_report_path = resource_report_path;
                session.resource_decision_path = resource_decision_path;
                session.resource_decided = false;
                session.pending_intents.clear();
                session.deferred_presentation = false;
                session.capture_errors.clear();
                session.gpu_context_only = false;
                eprintln!("[NOVA_RENDER_PHASE] stage=fast_scene_replacement session_id={} preview_id={} existing_gpu_context=true gpu_ready_observed_epoch_ms={:?} source_bytes={} preload={} scene_work_started_before_click=false", session.session_id, session.view.preview_id, session.gpu_ready_observed_epoch_ms, session.source_bytes, input.preload);
                return Ok(session.view.clone());
            }
            self.stop_session(session)?;
        }
        *slot = None;
        let generation = self.sequence.fetch_add(1, Ordering::Relaxed);
        let id = format!("quick-{}-{generation}", self.supervisor.owner_epoch().get());
        let directory = self.data_root.join("runtime").join("quick-preview").join(&id);
        let session_directory = directory.join("session");
        fs::create_dir_all(&session_directory).map_err(|error| error.to_string())?;
        let resource_report_path = session_directory.join("resource-report.json");
        let resource_decision_path = session_directory.join("resource-decision.json");
        let backend = ViewerBackendIdentity::capture(&self.viewer_exe)?;
        let launch = ViewerLaunchContract::new(id.clone(), source.clone(), ViewerContent::default(), None,
            ViewerTask::QuickPreview(QuickPreviewTask { initial_visible: false, host: Some(host_window.clone()), preload: input.preload,
                resource_report_path: Some(resource_report_path.clone()), resource_decision_path: Some(resource_decision_path.clone()) }))?;
        let contract = ViewerLaunchContractFile::create(directory.join("launch.json"), &launch, &backend)?;
        let stdout = super::diagnostics::log_path("quick-preview", &format!("{id}-stdout.log"), &directory)?;
        let stderr = super::diagnostics::log_path("quick-preview", &format!("{id}-stderr.log"), &directory)?;
        let mut spec = LaunchSpec::new(ExecutableRole::NativeViewer, LaunchIdentity {
            owner_epoch: self.supervisor.owner_epoch(), generation,
            launch_token: LaunchToken::new(id.clone()).map_err(|error| error.to_string())?,
        });
        spec.args.push(contract.command_arg().into());
        spec.environment = vec![
            ("LITEMATICA_NOVA_VIEWER_SESSION_DIRECTORY".into(), session_directory.as_os_str().to_owned()),
            ("LITEMATICA_NOVA_VIEWER_SESSION_ID".into(), id.clone().into()),
        ];
        if let Some(click) = input.click_started_at_epoch_ms {
            spec.environment.push(("LITEMATICA_NOVA_CLICK_EPOCH_MS".into(), click.to_string().into()));
        }
        spec.current_dir = self.viewer_exe.parent().map(Path::to_path_buf);
        spec.stdout = OutputTarget::File { path: stdout.clone(), append: false };
        spec.stderr = OutputTarget::File { path: stderr.clone(), append: false };
        let child = self.supervisor.spawn(spec).map_err(|error| format!("启动快速预览失败：{error}"))?.child;
        let view = QuickPreviewView { preview_id: id, source_path: source, render_mode: "fast_experimental", state: "starting",
            presentation: QuickPresentationIntent::default(), presentation_revision: 0, visible: false, error: None, preload: input.preload, resource: None };
        *slot = Some(QuickSession { view: view.clone(), session_id: view.preview_id.clone(), child, contract, backend, directory: session_directory, stdout, stderr,
            source_bytes: meta.len(), source_modified: meta.modified().ok(), requested_preload: input.preload, pending_click_epoch_ms: input.click_started_at_epoch_ms, next_command: 1, next_status: 0, next_event: 1, pending_intents: BTreeMap::new(), deferred_presentation: false, pending_replacements: BTreeMap::new(),
            launch_accepted: false, gpu_context_only: false, gpu_ready_observed_epoch_ms: None, native_hwnd: None, resource_report_path, resource_decision_path, resource_decided: false, capture_errors: BTreeMap::new(), host_window });
        Ok(view)
    }

    pub(crate) fn resource_decision(&self, id: &str, action: QuickResourceAction, warning_confirmed: bool) -> Result<QuickPreviewView, String> {
        let mut pending = self.pending_resource.lock().map_err(|_| "快速预览显存决策锁异常")?;
        if let Some(request) = pending.as_mut().filter(|request| request.view.preview_id == id) {
            let report = request.view.resource.as_ref().filter(|report| report.decision_required).ok_or("当前没有等待中的显存决策")?;
            validate_resource_decision(report, action, warning_confirmed)?;
            publish_resource_decision(&request.decision_path, id, action, warning_confirmed)?;
            if matches!(action, QuickResourceAction::Cancel) {
                request.view.state = "closed"; request.view.resource = None;
                return Ok(request.view.clone());
            }
            let request = pending.take().unwrap();
            drop(pending);
            let metadata = fs::metadata(&request.input.source_path).map_err(|error| error.to_string())?;
            if metadata.len() != request.source_bytes || metadata.modified().ok() != request.source_modified {
                return Err("投影已修改，请重新加载快速预览。".into());
            }
            eprintln!("[QUICK_RESOURCE] stage=explicit_realtime request_id={id} estimate=unknown preload_admitted=false");
            return self.start(StartQuickPreviewInput { preload: false, ..request.input }, request.host_window);
        }
        drop(pending);
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        let session = slot.as_mut().filter(|session| session.view.preview_id == id).ok_or("预算决策不属于当前快速预览")?;
        session.refresh(&self.supervisor)?;
        let report = session.view.resource.as_ref().filter(|report| report.decision_required).ok_or("当前没有等待中的显存决策")?;
        validate_resource_decision(report, action, warning_confirmed)?;
        publish_resource_decision(&session.resource_decision_path, &session.session_id, action, warning_confirmed)?;
        session.resource_decided = true;
        if !matches!(action, QuickResourceAction::Cancel) { session.view.preload = matches!(action, QuickResourceAction::Preload); }
        if let Some(report) = session.view.resource.as_mut() { report.decision_required = false; }
        Ok(session.view.clone())
    }

    pub(crate) fn snapshot(&self) -> Result<Option<QuickPreviewView>, String> {
        if let Some(request) = self.pending_resource.lock().map_err(|_| "快速预览显存决策锁异常")?.as_ref() {
            return Ok(Some(request.view.clone()));
        }
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        if let Some(session) = slot.as_mut() {
            if let Err(error) = session.refresh(&self.supervisor) {
                self.stop_session(session)?;
                session.view.state = "failed";
                session.view.error = Some(error);
            }
        }
        Ok(slot.as_ref().filter(|session| !session.gpu_context_only).map(|session| session.view.clone()))
    }

    pub(crate) fn present(&self, id: &str, intent: QuickPresentationIntent, revision: u64) -> Result<QuickPreviewView, String> {
        if let Some(request) = self.pending_resource.lock().map_err(|_| "快速预览显存决策锁异常")?.as_mut().filter(|request| request.view.preview_id == id) {
            if request.view.accepts_presentation(revision) { request.view.presentation_revision = revision; }
            // A pending choice has no prepared scene and cannot admit a Show.
            return Ok(request.view.clone());
        }
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        let session = slot.as_mut().filter(|session| session.view.preview_id == id).ok_or("快速预览已切换，请重新加载。")?;
        if !session.view.accepts_presentation(revision) { return Ok(session.view.clone()); }
        if !matches!(session.view.state, "starting" | "ready") { return Ok(session.view.clone()); }
        // Validate the same native command now, but keep the child hidden while
        // its current scene is preparing. The existing ready receipt admits it.
        presentation_command(&session.session_id, session.next_command, &intent, &session.host_window)?;
        // Record the newest lease/revision before reading a pending Ready. A
        // page Hide must not cause refresh to dispatch an older deferred Show.
        session.pending_intents.clear();
        session.view.visible &= intent.visible;
        session.view.presentation = intent;
        session.view.presentation_revision = revision;
        session.view.error = None;
        session.deferred_presentation = true;
        session.refresh(&self.supervisor)?;
        Ok(session.view.clone())
    }

    pub(crate) fn close(&self, id: &str) -> Result<(), String> {
        if let Some(request) = self.pending_resource.lock().map_err(|_| "快速预览显存决策锁异常")?.as_mut().filter(|request| request.view.preview_id == id) {
            publish_resource_decision(&request.decision_path, id, QuickResourceAction::Cancel, false)?;
            request.view.state = "closed"; request.view.resource = None;
            return Ok(());
        }
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        if let Some(session) = slot.as_mut().filter(|session| session.view.preview_id == id) {
            session.refresh(&self.supervisor)?;
            if session.gpu_ready_observed_epoch_ms.is_some()
                && matches!(self.supervisor.wait_for_exit(&session.child, Duration::ZERO), WaitOutcome::TimedOut) {
                session.close_presentation()?;
            } else { self.stop_session(session)?; }
        }
        Ok(())
    }

    /// Whole-application exit sends intent only. The common Supervisor owns
    /// the single parallel grace/force deadline; don't spend a separate wait
    /// here before other viewers and the render service can begin exiting.
    pub(crate) fn request_shutdown(&self) -> Result<(), String> {
        self.pending_resource.lock().map_err(|_| "快速预览显存决策锁异常")?.take();
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        if let Some(session) = slot.as_mut() {
            publish_resource_decision(&session.resource_decision_path, &session.session_id, QuickResourceAction::Cancel, false)?;
            if session.gpu_ready_observed_epoch_ms.is_some() {
                session.publish_command(&ViewerSessionCommand::release(session.session_id.clone(), session.next_command)?)?;
                session.next_command += 1;
            }
            session.deferred_presentation = false;
            session.view.state = "closed";
            session.view.visible = false;
            session.view.presentation = QuickPresentationIntent::default();
        }
        Ok(())
    }

    pub(crate) fn capture_frame(&self, id: &str) -> Result<(PathBuf, PathBuf), String> {
        let (source, output, sequence) = {
            let mut slot = self.session.lock().map_err(|error| error.to_string())?;
            let session = slot.as_mut().filter(|session| session.view.preview_id == id).ok_or("预览会话已改变")?;
            session.refresh(&self.supervisor)?;
            if session.view.state != "ready" { return Err("请先等待快速预览准备完成".into()); }
            let metadata = fs::metadata(&session.view.source_path).map_err(|error| error.to_string())?;
            if metadata.len() != session.source_bytes || metadata.modified().ok() != session.source_modified { return Err("投影已修改，请重新加载预览".into()); }
            let sequence = session.next_command;
            session.publish_command(&ViewerSessionCommand::capture_frame(session.session_id.clone(), sequence)?)?;
            session.next_command += 1;
            (session.view.source_path.clone(), session.directory.join(format!("capture-{sequence:020}.png")), sequence)
        };
        let started = std::time::Instant::now();
        loop {
            {
                let mut slot = self.session.lock().map_err(|error| error.to_string())?;
                let session = slot.as_mut().filter(|session| session.view.preview_id == id).ok_or("预览会话已改变，截图未保存")?;
                session.refresh(&self.supervisor)?;
                if let Some(error) = session.capture_errors.remove(&sequence) { return Err(error); }
                if let Some(error) = &session.view.error { return Err(error.clone()); }
                if matches!(session.view.state, "closed" | "failed") { return Err("预览已关闭，截图未保存".into()); }
            }
            if output.is_file() {
                let slot = self.session.lock().map_err(|error| error.to_string())?;
                let session = slot.as_ref().filter(|session| session.view.preview_id == id).ok_or("预览会话已改变")?;
                let metadata = fs::metadata(&source).map_err(|error| error.to_string())?;
                if metadata.len() != session.source_bytes || metadata.modified().ok() != session.source_modified { return Err("截图期间投影发生修改，未保存旧帧".into()); }
                return Ok((source, output));
            }
            if started.elapsed() > Duration::from_secs(75) { return Err("真实帧读取尚未完成，请稍后重试".into()); }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn stop_session(&self, session: &mut QuickSession) -> Result<(), String> {
        publish_resource_decision(&session.resource_decision_path, &session.session_id, QuickResourceAction::Cancel, false)?;
        if session.view.state == "ready" {
            let command = ViewerSessionCommand::release(session.session_id.clone(), session.next_command)?;
            session.publish_command(&command)?;
            session.next_command += 1;
        }
        if !matches!(self.supervisor.wait_for_exit(&session.child, Duration::from_millis(300)), WaitOutcome::Exited(_) | WaitOutcome::Stale) {
            self.supervisor.terminate(&session.child, 0).map_err(|error| error.to_string())?;
            if matches!(self.supervisor.wait_for_exit(&session.child, Duration::from_secs(2)), WaitOutcome::TimedOut) {
                return Err("快速预览正在结束，请稍后重试。".to_owned());
            }
        }
        session.deferred_presentation = false;
        session.view.state = "closed";
        session.pending_replacements.clear();
        session.view.visible = false;
        session.view.presentation = QuickPresentationIntent::default();
        Ok(())
    }
}

impl QuickSession {
    fn close_presentation(&mut self) -> Result<(), String> {
        publish_resource_decision(&self.resource_decision_path, &self.session_id, QuickResourceAction::Cancel, false)?;
        self.publish_command(&ViewerSessionCommand::hide(self.session_id.clone(), self.next_command)?)?;
        self.next_command += 1;
        self.pending_intents.clear();
        self.deferred_presentation = false;
        self.capture_errors.clear();
        self.view.state = "closed";
        self.view.visible = false;
        self.view.presentation = QuickPresentationIntent::default();
        self.view.resource = None;
        self.pending_click_epoch_ms = None;
        Ok(())
    }

    fn publish_command(&self, command: &ViewerSessionCommand) -> Result<(), String> {
        publish_command(&self.directory, command)?;
        // Wake the original native message loop after the atomic command rename;
        // no GUI automation and no extra visibility or process control operation.
        #[cfg(windows)]
        if let Some(hwnd) = self.native_hwnd {
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, PostMessageW, WM_NULL};
            let mut pid = 0;
            unsafe {
                if GetWindowThreadProcessId(hwnd as _, &mut pid) != 0 && pid == self.child.pid {
                    PostMessageW(hwnd as _, WM_NULL, 0, 0);
                }
            }
        }
        Ok(())
    }
    fn refresh(&mut self, supervisor: &ProcessSupervisor) -> Result<(), String> {
        if matches!(self.view.state, "closed" | "failed") { return Ok(()); }
        if self.view.resource.is_none() && self.resource_report_path.is_file() {
            let metadata = fs::metadata(&self.resource_report_path).map_err(|error| error.to_string())?;
            if metadata.len() > 128 * 1024 { return Err("快速显存报告超过合同大小".into()); }
            let mut report: QuickResourceReport = serde_json::from_slice(&fs::read(&self.resource_report_path).map_err(|error| error.to_string())?).map_err(|error| format!("快速显存报告无效：{error}"))?;
            if report.schema != "nova.quick.resource.v1" || report.request_id != self.session_id || report.source_bytes != self.source_bytes || report.preload != self.requested_preload
                || !matches!(report.pressure.as_str(), "unmeasured" | "normal" | "warning" | "hard" | "invalid") { return Err("快速显存报告与当前请求不一致".into()); }
            if self.resource_decided { report.decision_required = false; }
            super::diagnostics::record_resource(&self.view.preview_id, &report);
            self.view.resource = Some(report);
        }
        if !self.launch_accepted {
            let output = read_log(&self.stdout, false)?;
            // An in-flight partial stdout line is not yet a complete ACK.
            let complete = output.rsplit_once('\n').map(|(head, _)| head).unwrap_or("");
            self.launch_accepted = self.contract.accepted_ack(complete)?;
        }
        if self.launch_accepted {
            while self.next_status < self.next_command {
                let path = self.directory.join(format!("status-{:020}.json", self.next_status));
                if !path.is_file() { break; }
                let status = ViewerSessionStatus::from_json_slice(&fs::read(&path).map_err(|error| error.to_string())?)?;
                if status.session_id != self.session_id || status.pid != self.child.pid || status.sequence != self.next_status {
                    return Err("快速预览回执与当前进程不一致。".to_owned());
                }
                if let ViewerSessionState::SceneReady { request_id } = &status.state {
                    if request_id != &self.session_id { return Err("快速预览场景回执与当前请求不一致。".to_owned()); }
                    if !self.pending_replacements.get(&status.sequence).is_some_and(|contract| contract.request_id() == request_id) {
                        return Err("快速预览场景回执没有对应的切换请求。".to_owned());
                    }
                }
                // The status has already passed session / PID / sequence checks.
                self.native_hwnd = status.hwnd.parse::<isize>().ok().filter(|hwnd| *hwnd != 0);
                if status.sequence == 0 && matches!(status.state, ViewerSessionState::Ready) {
                    self.gpu_ready_observed_epoch_ms = Some(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_err(|error| error.to_string())?.as_millis().min(u128::from(u64::MAX)) as u64);
                }
                if !matches!(&status.state, ViewerSessionState::Error { .. }) && status.sequence + 1 == self.next_command {
                    if let Some(intent) = self.pending_intents.get(&status.sequence) { self.view.presentation = intent.clone(); }
                }
                if let ViewerSessionState::Error { command_kind: viewer_launch_contract::ViewerSessionCommandKind::CaptureFrame, message } = &status.state {
                    self.capture_errors.insert(status.sequence, message.clone());
                } else { self.view.apply_status(&status.state, status.sequence + 1 == self.next_command); }
                if matches!(&status.state, ViewerSessionState::SceneReady { .. }
                    | ViewerSessionState::Error { command_kind: viewer_launch_contract::ViewerSessionCommandKind::ReplaceScene, .. }) {
                    self.pending_replacements.remove(&status.sequence);
                }
                self.pending_intents.remove(&self.next_status);
                self.next_status += 1;
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        loop {
            let path = self.directory.join(format!("event-{:020}.json", self.next_event));
            if !path.is_file() { break; }
            let event = ViewerSessionEvent::from_json_slice(&fs::read(&path).map_err(|error| error.to_string())?)?;
            if event.session_id != self.session_id || event.pid != self.child.pid || event.event_id != self.next_event {
                return Err("快速预览事件与当前进程不一致。".to_owned());
            }
            if matches!(event.state, ViewerSessionState::Hidden) {
                self.view.visible = false;
                self.view.presentation = QuickPresentationIntent::default();
            }
            self.next_event += 1;
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        if let WaitOutcome::Exited(exit) = supervisor.wait_for_exit(&self.child, Duration::ZERO) {
            self.pending_replacements.clear();
            if self.view.state != "closed" {
                self.view.state = if exit.exit_code == Some(0) { "closed" } else { "failed" };
                self.view.visible = false;
                self.view.presentation = QuickPresentationIntent::default();
                if self.view.state == "failed" {
                    self.view.error = Some(format!("快速预览已退出（{:?}）。\n{}\n{}", exit.exit_code, read_log(&self.stderr, true)?, read_log(&self.stdout, true)?));
                }
            }
        }
        if self.deferred_presentation && self.view.state == "ready" && !self.gpu_context_only {
            let sequence = self.next_command;
            let intent = self.view.presentation.clone();
            let mut command = presentation_command(&self.session_id, sequence, &intent, &self.host_window)?;
            if intent.visible { command.click_started_at_epoch_ms = self.pending_click_epoch_ms.take(); }
            self.publish_command(&command)?;
            self.next_command += 1;
            self.pending_intents.insert(sequence, intent);
            self.deferred_presentation = false;
            eprintln!("[NOVA_RENDER_PHASE] stage=fast_deferred_presentation_dispatched session_id={} preview_id={} revision={} sequence={} command={:?} scope=verified_current_scene_ready actual_display=not_inferred",
                self.session_id, self.view.preview_id, self.view.presentation_revision, sequence, command.kind);
        }
        Ok(())
    }
}

fn presentation_command(id: &str, sequence: u64, intent: &QuickPresentationIntent, host: &QuickPreviewHostWindow) -> Result<ViewerSessionCommand, String> {
    if !intent.visible || intent.mode == QuickPresentationMode::Hidden { return ViewerSessionCommand::hide(id.to_owned(), sequence); }
    match intent.mode {
        QuickPresentationMode::Embedded => {
            let rect = intent.placement.ok_or("快速预览缺少视口位置。")?;
            if !rect.dpi_scale.is_finite() || rect.dpi_scale <= 0.0 { return Err("快速预览 DPI 无效。".to_owned()); }
            let mut command = ViewerSessionCommand::attach_embedded(id.to_owned(), sequence, host.hwnd.clone(), rect.width, rect.height)?;
            command.screen_x = Some(rect.x);
            command.screen_y = Some(rect.y);
            command.validate()?;
            Ok(command)
        }
        QuickPresentationMode::Hidden => unreachable!(),
    }
}

fn publish_command(directory: &Path, command: &ViewerSessionCommand) -> Result<(), String> {
    let path = directory.join(format!("command-{:020}.json", command.sequence));
    let staging = path.with_extension("json.tmp");
    fs::write(&staging, command.to_json_bytes()?).map_err(|error| error.to_string())?;
    fs::rename(staging, path).map_err(|error| error.to_string())
}

fn read_log(path: &Path, tail: bool) -> Result<String, String> {
    let mut file = match File::open(path) { Ok(file) => file, Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()), Err(error) => return Err(error.to_string()) };
    let limit = 16 * 1024;
    if tail { let len = file.metadata().map_err(|error| error.to_string())?.len(); file.seek(SeekFrom::Start(len.saturating_sub(limit))).map_err(|error| error.to_string())?; }
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::model::PresentationPlacement;
    fn view(presentation: QuickPresentationIntent) -> QuickPreviewView {
        QuickPreviewView { preview_id: "quick-one".into(), source_path: "fixture.litematic".into(),
            render_mode: "fast_experimental", state: "ready", presentation, presentation_revision: 7,
            visible: false, error: None, preload: false, resource: None }
    }

    #[test]
    fn quick_presentation_revision_rejects_old_show_and_duplicate_commands() {
        let view = view(QuickPresentationIntent::default());
        assert!(!view.accepts_presentation(6));
        assert!(!view.accepts_presentation(7));
        assert!(view.accepts_presentation(8));
    }

    #[test]
    fn quick_scene_replacement_old_ready_and_attached_never_admit_new_scene() {
        let mut view = view(QuickPresentationIntent::default());
        view.preview_id = "quick-one-scene-2".into();
        view.state = "starting";
        view.presentation_revision = 0;
        view.apply_status(&ViewerSessionState::Ready, false);
        view.apply_status(&ViewerSessionState::Attached { host_hwnd: "42".into(), width: 640, height: 480 }, false);
        assert_eq!(view.state, "starting");
        assert!(!view.visible);
        view.apply_status(&ViewerSessionState::Ready, true);
        assert_eq!(view.state, "ready");
        assert!(!view.visible, "replacement readiness is not a native Present");
        view.apply_status(&ViewerSessionState::Error { command_kind: viewer_launch_contract::ViewerSessionCommandKind::ReplaceScene, message: "actual scene decode failure".into() }, true);
        assert_eq!(view.state, "failed");
        view.apply_status(&ViewerSessionState::Ready, false);
        assert_eq!(view.state, "failed");
    }

    #[test]
    fn quick_closed_view_rejects_late_ready_error_and_old_release() {
        let mut closed = view(QuickPresentationIntent::default());
        closed.state = "closed";
        closed.apply_status(&ViewerSessionState::Ready, true);
        closed.apply_status(&ViewerSessionState::SceneReady { request_id: "quick-one".into() }, true);
        closed.apply_status(&ViewerSessionState::Error { command_kind: viewer_launch_contract::ViewerSessionCommandKind::ReplaceScene, message: "retired scene error".into() }, true);
        assert_eq!(closed.state, "closed"); assert!(closed.error.is_none());
        let mut current = view(QuickPresentationIntent::default());
        current.state = "starting";
        current.apply_status(&ViewerSessionState::Released, false);
        assert_eq!(current.state, "starting");
    }

    #[cfg(windows)]
    #[test]
    fn quick_preparing_presentation_uses_latest_ready_rect_and_hide_revision_on_live_cli_session() {
        use crate::platform::process_supervisor::ExecutableAllowlist;
        let command = std::env::var_os("COMSPEC").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        let mut allowlist = ExecutableAllowlist::new();
        allowlist.allow(ExecutableRole::NativeViewer, &command).unwrap();
        let (supervisor, _) = ProcessSupervisor::new(allowlist).unwrap();
        let supervisor = Arc::new(supervisor);
        let mut spec = LaunchSpec::new(ExecutableRole::NativeViewer, LaunchIdentity { owner_epoch: supervisor.owner_epoch(), generation: 1, launch_token: LaunchToken::new("quick-host-cli-lease").unwrap() });
        // A supervised CLI sleeper, not a Viewer/window or rendering claim.
        spec.args = vec!["/d".into(), "/c".into(), "ping -n 20 127.0.0.1 > nul".into()];
        let child = supervisor.spawn(spec).unwrap().child;
        struct TestDirectory(PathBuf);
        impl Drop for TestDirectory { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
        let directory = TestDirectory(std::env::temp_dir().join(format!("nova-quick-host-{}-{}", std::process::id(), SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos())));
        fs::create_dir_all(&directory.0).unwrap();
        let sources = fs::read_dir(r"C:\Users\27232\Documents\投影测试").unwrap().filter_map(Result::ok)
            .map(|entry| entry.path()).filter(|path| path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("litematic"))).take(2).collect::<Vec<_>>();
        assert_eq!(sources.len(), 2);
        let source = fs::canonicalize(&sources[0]).unwrap(); let metadata = fs::metadata(&source).unwrap();
        let window = QuickPreviewHostWindow { hwnd: "42".into(), process_id: std::process::id() };
        let backend = ViewerBackendIdentity::capture(&command).unwrap();
        let launch = ViewerLaunchContract::new("quick-one".into(), source.clone(), ViewerContent::default(), None, ViewerTask::QuickPreview(QuickPreviewTask { initial_visible: false, host: Some(window.clone()), ..Default::default() })).unwrap();
        let contract = ViewerLaunchContractFile::create(directory.0.join("launch.json"), &launch, &backend).unwrap();
        let stdout = directory.0.join("stdout.log"); let stderr = directory.0.join("stderr.log");
        fs::write(&stdout, "").unwrap(); fs::write(&stderr, "").unwrap();
        let mut initial = view(QuickPresentationIntent::default()); initial.source_path = source.clone(); initial.state = "starting"; initial.presentation_revision = 0;
        let host = QuickPreviewHost::new(Arc::clone(&supervisor), command, directory.0.clone());
        *host.session.lock().unwrap() = Some(QuickSession { view: initial, session_id: "quick-one".into(), child: child.clone(), contract, backend, directory: directory.0.clone(), stdout, stderr,
            source_bytes: metadata.len(), source_modified: metadata.modified().ok(), requested_preload: false, pending_click_epoch_ms: Some(41),
            next_command: 1, next_status: 0, next_event: 1, pending_intents: BTreeMap::new(), deferred_presentation: false, pending_replacements: BTreeMap::new(), capture_errors: BTreeMap::new(), host_window: window.clone(),
            launch_accepted: true, gpu_context_only: false, gpu_ready_observed_epoch_ms: None, native_hwnd: None,
            resource_report_path: directory.0.join("resource-report.json"), resource_decision_path: directory.0.join("resource-decision.json"), resource_decided: false });
        let status = |sequence, state| {
            let value = ViewerSessionStatus::new("quick-one".into(), sequence, child.pid, "42".into(), state).unwrap();
            fs::write(directory.0.join(format!("status-{sequence:020}.json")), value.to_json_bytes().unwrap()).unwrap();
        };
        let read_command = |sequence| ViewerSessionCommand::from_json_slice(&fs::read(directory.0.join(format!("command-{sequence:020}.json"))).unwrap()).unwrap();

        let embedded = |x, scale| QuickPresentationIntent { mode: QuickPresentationMode::Embedded, visible: true,
            placement: Some(PresentationPlacement { x, y: 120, width: (600.0 * scale) as u32,
                height: (400.0 * scale) as u32, dpi_scale: scale, focused: false }) };
        host.present("quick-one", embedded(10, 1.0), 1).unwrap();
        host.present("quick-one", embedded(900, 1.5), 2).unwrap();
        assert!(!directory.0.join("command-00000000000000000001.json").exists(), "Preparing cannot attach an empty child");
        assert!(!host.snapshot().unwrap().unwrap().visible);
        status(0, ViewerSessionState::Ready);
        let ready = host.snapshot().unwrap().unwrap();
        assert_eq!(ready.state, "ready"); assert!(!ready.visible, "Ready is not a native visibility receipt");
        let attach = read_command(1);
        assert_eq!(attach.kind, viewer_launch_contract::ViewerSessionCommandKind::AttachEmbedded);
        assert_eq!((attach.screen_x, attach.width, attach.height), (Some(900), Some(900), Some(600)));
        assert_eq!(attach.click_started_at_epoch_ms, Some(41));
        status(1, ViewerSessionState::Attached { host_hwnd: "42".into(), width: 900, height: 600 });
        assert!(host.snapshot().unwrap().unwrap().visible);

        let changed = host.start(StartQuickPreviewInput { source_path: sources[1].clone(), preload: true,
            click_started_at_epoch_ms: Some(42) }, window.clone()).unwrap();
        assert_ne!(changed.preview_id, "quick-one");
        assert_eq!(read_command(2).kind, viewer_launch_contract::ViewerSessionCommandKind::Hide);
        let replacement = read_command(3); let replacement_path = replacement.launch_contract.unwrap();
        let launch = ViewerLaunchContract::from_json_slice(&fs::read(&replacement_path).unwrap()).unwrap();
        assert!(matches!(launch.task, ViewerTask::QuickPreview(QuickPreviewTask { preload: true, .. })));
        host.present(&changed.preview_id, embedded(20, 1.0), 1).unwrap();
        host.present(&changed.preview_id, embedded(800, 2.0), 2).unwrap();
        host.present(&changed.preview_id, QuickPresentationIntent::default(), 3).unwrap();
        assert!(!directory.0.join("command-00000000000000000004.json").exists());
        status(2, ViewerSessionState::Hidden);
        status(3, ViewerSessionState::SceneReady { request_id: "quick-one".into() });
        // A late geometry request must not flush its old Show before the newer Hide.
        host.present(&changed.preview_id, embedded(999, 1.0), 2).unwrap();
        let hidden = host.snapshot().unwrap().unwrap();
        assert_eq!(hidden.state, "ready"); assert!(!hidden.visible);
        assert_eq!(read_command(4).kind, viewer_launch_contract::ViewerSessionCommandKind::Hide);
        assert!(!replacement_path.exists(), "verified terminal receipt releases the same owned contract");
        status(4, ViewerSessionState::Hidden); host.snapshot().unwrap();
        host.present(&changed.preview_id, embedded(810, 2.0), 4).unwrap();
        assert_eq!(read_command(5).screen_x, Some(810));
        assert_eq!(read_command(5).click_started_at_epoch_ms, Some(42));
        status(5, ViewerSessionState::Attached { host_hwnd: "42".into(), width: 1200, height: 800 });
        assert!(host.snapshot().unwrap().unwrap().visible);

        let closing = host.start(StartQuickPreviewInput { source_path: source.clone(), preload: false,
            click_started_at_epoch_ms: Some(43) }, window.clone()).unwrap();
        host.present(&closing.preview_id, embedded(30, 1.0), 1).unwrap();
        host.close(&closing.preview_id).unwrap();
        assert_eq!(read_command(8).kind, viewer_launch_contract::ViewerSessionCommandKind::Hide);
        status(6, ViewerSessionState::Hidden);
        status(7, ViewerSessionState::SceneReady { request_id: "quick-one".into() });
        assert_eq!(host.snapshot().unwrap().unwrap().state, "closed");
        assert!(!directory.0.join("command-00000000000000000009.json").exists(), "late SceneReady cannot flush the closed page's desired Show");
        let reopened = host.start(StartQuickPreviewInput { source_path: source, preload: false,
            click_started_at_epoch_ms: Some(44) }, window).unwrap();
        assert_ne!(reopened.preview_id, closing.preview_id);
        assert!(host.present(&closing.preview_id, embedded(999, 1.0), 2).is_err());
        status(8, ViewerSessionState::Hidden); status(9, ViewerSessionState::Hidden);
        status(10, ViewerSessionState::SceneReady { request_id: "quick-one".into() });
        assert_eq!(host.snapshot().unwrap().unwrap().state, "ready");
        assert!(!directory.0.join("command-00000000000000000011.json").exists(), "fresh source never inherits a retired queued Show");
        assert_eq!(supervisor.active_children(), vec![child.clone()]);
        supervisor.terminate(&child, 0).unwrap();
        assert!(matches!(supervisor.wait_for_exit(&child, Duration::from_secs(2)), WaitOutcome::Exited(_)));
    }

    #[cfg(windows)]
    #[test]
    fn quick_close_and_starting_rebind_preserve_live_cli_lease_and_saved_command_identity() {
        use crate::platform::process_supervisor::ExecutableAllowlist;
        let command = std::env::var_os("COMSPEC").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        let mut allowlist = ExecutableAllowlist::new();
        allowlist.allow(ExecutableRole::NativeViewer, &command).unwrap();
        let (supervisor, _) = ProcessSupervisor::new(allowlist).unwrap();
        let supervisor = Arc::new(supervisor);
        let mut spec = LaunchSpec::new(ExecutableRole::NativeViewer, LaunchIdentity { owner_epoch: supervisor.owner_epoch(), generation: 1, launch_token: LaunchToken::new("quick-host-cli-lease").unwrap() });
        // A supervised CLI sleeper, not a Viewer/window or rendering claim.
        spec.args = vec!["/d".into(), "/c".into(), "ping -n 20 127.0.0.1 > nul".into()];
        let child = supervisor.spawn(spec).unwrap().child;
        struct TestDirectory(PathBuf);
        impl Drop for TestDirectory { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
        let directory = TestDirectory(std::env::temp_dir().join(format!("nova-quick-host-{}-{}", std::process::id(), SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos())));
        fs::create_dir_all(&directory.0).unwrap();
        let sources = fs::read_dir(r"C:\Users\27232\Documents\投影测试").unwrap().filter_map(Result::ok)
            .map(|entry| entry.path()).filter(|path| path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("litematic"))).take(2).collect::<Vec<_>>();
        assert_eq!(sources.len(), 2);
        let source = fs::canonicalize(&sources[0]).unwrap(); let metadata = fs::metadata(&source).unwrap();
        let window = QuickPreviewHostWindow { hwnd: "42".into(), process_id: std::process::id() };
        let backend = ViewerBackendIdentity::capture(&command).unwrap();
        let launch = ViewerLaunchContract::new("quick-one".into(), source.clone(), ViewerContent::default(), None, ViewerTask::QuickPreview(QuickPreviewTask { initial_visible: false, host: Some(window.clone()), ..Default::default() })).unwrap();
        let contract = ViewerLaunchContractFile::create(directory.0.join("launch.json"), &launch, &backend).unwrap();
        let stdout = directory.0.join("stdout.log"); let stderr = directory.0.join("stderr.log");
        fs::write(&stdout, "").unwrap(); fs::write(&stderr, "").unwrap();
        let mut initial = view(QuickPresentationIntent::default()); initial.source_path = source.clone();
        let host = QuickPreviewHost::new(Arc::clone(&supervisor), command, directory.0.clone());
        *host.session.lock().unwrap() = Some(QuickSession { view: initial, session_id: "quick-one".into(), child: child.clone(), contract, backend, directory: directory.0.clone(), stdout, stderr,
            source_bytes: metadata.len(), source_modified: metadata.modified().ok(), requested_preload: false, pending_click_epoch_ms: None,
            next_command: 1, next_status: 0, next_event: 1, pending_intents: BTreeMap::new(), deferred_presentation: false, pending_replacements: BTreeMap::new(), capture_errors: BTreeMap::new(), host_window: window.clone(),
            launch_accepted: true, gpu_context_only: false, gpu_ready_observed_epoch_ms: None, native_hwnd: None,
            resource_report_path: directory.0.join("resource-report.json"), resource_decision_path: directory.0.join("resource-decision.json"), resource_decided: false });
        let status = |sequence, state| {
            let value = ViewerSessionStatus::new("quick-one".into(), sequence, child.pid, "42".into(), state).unwrap();
            fs::write(directory.0.join(format!("status-{sequence:020}.json")), value.to_json_bytes().unwrap()).unwrap();
        };
        let read_command = |sequence| ViewerSessionCommand::from_json_slice(&fs::read(directory.0.join(format!("command-{sequence:020}.json"))).unwrap()).unwrap();
        status(0, ViewerSessionState::Ready); host.snapshot().unwrap();
        host.close("quick-one").unwrap();
        assert_eq!(read_command(1).kind, viewer_launch_contract::ViewerSessionCommandKind::Hide);
        assert_eq!(supervisor.active_children(), vec![child.clone()]);
        status(1, ViewerSessionState::Hidden);
        assert_eq!(host.snapshot().unwrap().unwrap().state, "closed", "old Ready cannot reopen a closed preview");
        let reopened = host.start(StartQuickPreviewInput { source_path: source.clone(), preload: false, click_started_at_epoch_ms: Some(11) }, window.clone()).unwrap();
        assert_ne!(reopened.preview_id, "quick-one"); assert_eq!(reopened.state, "starting");
        let replacement = read_command(3); assert_eq!(replacement.kind, viewer_launch_contract::ViewerSessionCommandKind::ReplaceScene);
        let replacement_path = replacement.launch_contract.unwrap();
        assert!(replacement_path.is_file(), "queued source contract remains owned until its verified terminal status");
        let launch = ViewerLaunchContract::from_json_slice(&fs::read(&replacement_path).unwrap()).unwrap();
        assert_eq!(launch.request_id, "quick-one"); assert_eq!(fs::canonicalize(launch.scene).unwrap(), fs::canonicalize(&sources[0]).unwrap());
        host.close(&reopened.preview_id).unwrap();
        assert!(replacement_path.is_file(), "closing presentation does not remove an in-flight source contract");
        assert_eq!(read_command(4).kind, viewer_launch_contract::ViewerSessionCommandKind::Hide);
        status(2, ViewerSessionState::Hidden); status(3, ViewerSessionState::SceneReady { request_id: "quick-one".into() });
        assert_eq!(host.snapshot().unwrap().unwrap().state, "closed", "actual delayed replacement completion cannot reopen a closed preview");
        let reopened_again = host.start(StartQuickPreviewInput { source_path: source, preload: false, click_started_at_epoch_ms: Some(12) }, window.clone()).unwrap();
        assert_ne!(reopened_again.preview_id, reopened.preview_id);
        let replacement_again = read_command(6); assert_eq!(replacement_again.kind, viewer_launch_contract::ViewerSessionCommandKind::ReplaceScene);
        let replacement_again_path = replacement_again.launch_contract.unwrap();
        status(4, ViewerSessionState::Hidden); status(5, ViewerSessionState::Hidden);
        let retired_decision = host.session.lock().unwrap().as_ref().unwrap().resource_decision_path.clone();
        let switched = host.start(StartQuickPreviewInput { source_path: sources[1].clone(), preload: false, click_started_at_epoch_ms: Some(13) }, window).unwrap();
        assert_ne!(switched.preview_id, reopened_again.preview_id); assert_eq!(switched.state, "starting");
        let new_replacement = read_command(8); assert_eq!(new_replacement.kind, viewer_launch_contract::ViewerSessionCommandKind::ReplaceScene);
        let new_replacement_path = new_replacement.launch_contract.unwrap();
        assert!(!replacement_path.exists(), "verified retired completion releases only its own contract");
        assert!(replacement_again_path.is_file() && new_replacement_path.is_file());
        let new_launch = ViewerLaunchContract::from_json_slice(&fs::read(&new_replacement_path).unwrap()).unwrap();
        assert_eq!(fs::canonicalize(new_launch.scene).unwrap(), fs::canonicalize(&sources[1]).unwrap());
        assert!(matches!(new_launch.task, ViewerTask::QuickPreview(QuickPreviewTask { preload: false, .. })));
        let retired_budget: serde_json::Value = serde_json::from_slice(&fs::read(retired_decision).unwrap()).unwrap();
        assert_eq!(retired_budget["action"], "cancel");
        status(6, ViewerSessionState::SceneReady { request_id: "quick-one".into() });
        assert_eq!(host.snapshot().unwrap().unwrap().state, "starting", "retired Ready must not admit the new source");
        assert!(!replacement_again_path.exists()); assert!(new_replacement_path.is_file());
        status(7, ViewerSessionState::Error { command_kind: viewer_launch_contract::ViewerSessionCommandKind::Hide, message: "retired failure".into() });
        status(8, ViewerSessionState::SceneReady { request_id: "other-request".into() });
        {
            let mut slot = host.session.lock().unwrap(); let session = slot.as_mut().unwrap();
            assert_eq!(session.refresh(&supervisor).unwrap_err(), "快速预览场景回执与当前请求不一致。");
            assert_eq!(session.view.state, "starting"); assert!(!session.view.visible);
            assert!(new_replacement_path.is_file(), "wrong request completion cannot release the actual pending contract");
        }
        status(8, ViewerSessionState::SceneReady { request_id: "quick-one".into() });
        let current = host.snapshot().unwrap().unwrap();
        assert_eq!(current.state, "ready"); assert!(current.error.is_none()); assert_eq!(current.preview_id, switched.preview_id);
        assert!(!new_replacement_path.exists());
        assert_eq!(supervisor.active_children(), vec![child.clone()]);
        let host_window = host.session.lock().unwrap().as_ref().unwrap().host_window.clone();
        host.start(StartQuickPreviewInput { source_path: sources[0].clone(), preload: true, click_started_at_epoch_ms: Some(14) }, host_window).unwrap();
        let failed_replacement_path = read_command(10).launch_contract.unwrap();
        let failed_launch = ViewerLaunchContract::from_json_slice(&fs::read(&failed_replacement_path).unwrap()).unwrap();
        assert!(matches!(failed_launch.task, ViewerTask::QuickPreview(QuickPreviewTask { preload: true, .. })));
        status(9, ViewerSessionState::Hidden);
        status(10, ViewerSessionState::Error { command_kind: viewer_launch_contract::ViewerSessionCommandKind::ReplaceScene, message: "actual failed replacement".into() });
        assert_eq!(host.snapshot().unwrap().unwrap().state, "failed");
        assert!(!failed_replacement_path.exists(), "verified replacement failure also releases its contract");
        host.request_shutdown().unwrap();
        assert_eq!(read_command(11).kind, viewer_launch_contract::ViewerSessionCommandKind::Release, "application shutdown really releases the retained context");
        supervisor.terminate(&child, 0).unwrap();
        assert!(matches!(supervisor.wait_for_exit(&child, Duration::from_secs(2)), WaitOutcome::Exited(_)));
    }

    #[test]
    fn quick_presentation_old_ack_and_ready_never_resurrect_hidden_view() {
        let mut view = view(QuickPresentationIntent::default());
        let attached = ViewerSessionState::Attached { host_hwnd: "42".into(), width: 600, height: 400 };
        view.apply_status(&attached, false);
        assert!(!view.visible);
        view.apply_status(&attached, true);
        assert!(!view.visible);
        view.apply_status(&ViewerSessionState::Ready, false);
        assert_eq!(view.state, "ready"); assert!(!view.visible);
        view.presentation = QuickPresentationIntent { mode: QuickPresentationMode::Embedded, visible: true, placement: None };
        view.apply_status(&attached, true);
        assert!(view.visible);
        view.apply_status(&ViewerSessionState::Hidden, false);
        assert!(view.visible);
        view.apply_status(&ViewerSessionState::Hidden, true);
        assert!(!view.visible); assert_eq!(view.preview_id, "quick-one"); assert_eq!(view.state, "ready");
    }
    #[test]
    fn quick_presentation_keeps_physical_rect_and_rejects_missing_host() {
        let host = QuickPreviewHostWindow { hwnd: "42".into(), process_id: 1 };
        let mut intent = QuickPresentationIntent { mode: QuickPresentationMode::Embedded, visible: true, placement: None };
        assert!(presentation_command("quick-one", 1, &intent, &host).is_err());
        intent.placement = Some(PresentationPlacement { x: -120, y: 40, width: 640, height: 480, dpi_scale: 1.5, focused: false });
        let cmd = presentation_command("quick-one", 1, &intent, &host).unwrap();
        assert_eq!(cmd.screen_x, Some(-120)); assert_eq!(cmd.width, Some(640)); assert_eq!(cmd.host_hwnd.as_deref(), Some("42"));
        intent.visible = false; assert_eq!(presentation_command("quick-one", 2, &intent, &host).unwrap().kind, viewer_launch_contract::ViewerSessionCommandKind::Hide);
    }
}

#[cfg(test)]
mod quick_resource_tests {
    use super::*;

    fn report(pressure: &str, basis_points: Option<u64>) -> QuickResourceReport {
        let measured = matches!(pressure, "normal" | "warning" | "hard");
        QuickResourceReport {
            schema: "nova.quick.resource.v1".into(), request_id: "quick-resource-test".into(),
            source_bytes: 840, preload: true, pressure: pressure.into(),
            pressure_basis_points: basis_points,
            budget: measured.then(|| serde_json::json!({"budgetBytes":10_000,"currentUsageBytes":0})),
            estimate: measured.then(|| serde_json::json!({"gpuPeakUpperBytes":8_000})),
            invalid_reason: None, decision_required: true,
        }
    }

    #[cfg(windows)]
    #[test]
    fn quick_resource_unknown_waits_without_viewer_or_scene_and_rejects_stale_choices() {
        use crate::platform::process_supervisor::ExecutableAllowlist;
        struct Fixture(PathBuf);
        impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
        let root = Fixture(std::env::temp_dir().join(format!("nova-quick-resource-host-{}-{}", std::process::id(), SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos())));
        fs::create_dir_all(&root.0).unwrap();
        let source = root.0.join("first.litematic"); let second = root.0.join("second.litematic");
        fs::write(&source, b"metadata-only fixture; never decoded").unwrap(); fs::write(&second, b"another source").unwrap();
        let (supervisor, _) = ProcessSupervisor::new(ExecutableAllowlist::new()).unwrap();
        let supervisor = Arc::new(supervisor);
        let host = QuickPreviewHost::new(supervisor.clone(), root.0.join("deliberately-absent-viewer.exe"), root.0.clone());
        let window = QuickPreviewHostWindow { hwnd: "42".into(), process_id: std::process::id() };
        let input = |source_path| StartQuickPreviewInput { source_path, preload: true, click_started_at_epoch_ms: Some(123) };
        let first = host.start(input(source.clone()), window.clone()).unwrap();
        assert_eq!(first.state, "starting"); assert!(!first.visible);
        assert!(first.resource.as_ref().unwrap().decision_required); assert!(first.resource.as_ref().unwrap().estimate.is_none());
        assert!(host.session.lock().unwrap().is_none()); assert!(supervisor.active_children().is_empty());
        assert_eq!(host.start(input(source.clone()), window.clone()).unwrap().preview_id, first.preview_id);
        let show = QuickPresentationIntent { mode: QuickPresentationMode::Embedded, visible: true,
            placement: Some(PresentationPlacement { x: 0, y: 0, width: 640, height: 480, dpi_scale: 1.0, focused: true }) };
        assert!(!host.present(&first.preview_id, show, 2).unwrap().visible);
        assert!(host.resource_decision(&first.preview_id, QuickResourceAction::Preload, true).is_err());
        let current = host.start(input(second.clone()), window).unwrap();
        assert_ne!(current.preview_id, first.preview_id);
        assert!(host.resource_decision(&first.preview_id, QuickResourceAction::Realtime, false).is_err());
        assert_eq!(host.snapshot().unwrap().unwrap().preview_id, current.preview_id);
        host.close(&current.preview_id).unwrap();
        let closed = host.snapshot().unwrap().unwrap(); assert_eq!(closed.state, "closed"); assert!(closed.resource.is_none());
        assert!(host.resource_decision(&current.preview_id, QuickResourceAction::Realtime, false).is_err());
        assert!(supervisor.active_children().is_empty());
        assert!(host.session.lock().unwrap().is_none(), "unknown request never dispatched scene/Viewer work");
    }

    #[cfg(windows)]
    #[test]
    fn quick_resource_changed_frozen_input_cannot_launch_on_late_realtime_choice() {
        use crate::platform::process_supervisor::ExecutableAllowlist;
        let root = std::env::temp_dir().join(format!("nova-quick-resource-frozen-{}-{}", std::process::id(), SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("fixture.litematic"); fs::write(&source, b"before").unwrap();
        let (supervisor, _) = ProcessSupervisor::new(ExecutableAllowlist::new()).unwrap(); let supervisor = Arc::new(supervisor);
        let host = QuickPreviewHost::new(supervisor.clone(), root.join("absent-viewer.exe"), root.clone());
        let view = host.start(StartQuickPreviewInput { source_path: source.clone(), preload: true, click_started_at_epoch_ms: None },
            QuickPreviewHostWindow { hwnd: "42".into(), process_id: std::process::id() }).unwrap();
        fs::write(&source, b"changed input content").unwrap();
        let error = host.resource_decision(&view.preview_id, QuickResourceAction::Realtime, false).unwrap_err();
        assert!(error.contains("投影已修改")); assert!(supervisor.active_children().is_empty());
        assert!(host.session.lock().unwrap().is_none());
        drop(host); drop(supervisor); fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quick_resource_preload_requires_complete_demand_and_current_admission() {
        assert!(validate_resource_decision(&report("normal", Some(7_999)), QuickResourceAction::Preload, false).is_ok());
        assert!(validate_resource_decision(&report("warning", Some(8_000)), QuickResourceAction::Preload, false).is_err());
        assert!(validate_resource_decision(&report("warning", Some(8_000)), QuickResourceAction::Preload, true).is_ok());
        for pressure in ["hard", "invalid", "unmeasured"] {
            assert!(validate_resource_decision(&report(pressure, None), QuickResourceAction::Preload, true).is_err());
        }
        let mut fixed_only = report("normal", Some(0)); fixed_only.estimate = None;
        assert!(validate_resource_decision(&fixed_only, QuickResourceAction::Preload, false).is_err());
    }

    #[test]
    fn quick_resource_report_keeps_unmeasured_nulls_and_rejects_unknown_schema_values() {
        let resource = report("unmeasured", None);
        let saved = serde_json::to_vec(&resource).unwrap();
        let restored: QuickResourceReport = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.pressure, "unmeasured");
        assert!(restored.estimate.is_none()); assert!(restored.pressure_basis_points.is_none());
        assert!(restored.decision_required);
        assert!(validate_resource_decision(&report("unknown-value", None), QuickResourceAction::Preload, true).is_err());
    }

    #[test]
    fn quick_resource_explicit_realtime_reduction_and_cancel_do_not_need_preload_confirmation() {
        for resource in [report("warning", Some(8_000)), report("hard", Some(9_500)), report("invalid", None)] {
            for action in [QuickResourceAction::Realtime, QuickResourceAction::Cancel] {
                assert!(validate_resource_decision(&resource, action, false).is_ok());
                assert!(resource.decision_required, "validation does not auto-select or consume a decision");
            }
        }
        assert!(validate_resource_decision(&report("warning", Some(8_000)), QuickResourceAction::ReducedRealtime, false).is_ok());
        assert!(validate_resource_decision(&report("unmeasured", None), QuickResourceAction::ReducedRealtime, false).is_err());
    }

    #[test]
    fn quick_resource_decision_file_replaces_confirmed_preload_with_cancel() {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        struct DecisionFile(PathBuf);
        impl Drop for DecisionFile {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
                let _ = fs::remove_file(self.0.with_extension("tmp"));
            }
        }
        let unique = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
        let file = DecisionFile(std::env::temp_dir().join(format!("nova-quick-decision-{}-{unique}-{}.json", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed))));
        let read = || serde_json::from_slice::<serde_json::Value>(&fs::read(&file.0).unwrap()).unwrap();
        publish_resource_decision(&file.0, "quick-resource-test", QuickResourceAction::Preload, true).unwrap();
        assert_eq!(read(), serde_json::json!({ "schema": "nova.quick.decision.v1", "requestId": "quick-resource-test", "action": "preload", "warningConfirmed": true }));
        publish_resource_decision(&file.0, "quick-resource-test", QuickResourceAction::Cancel, false).unwrap();
        assert_eq!(read(), serde_json::json!({ "schema": "nova.quick.decision.v1", "requestId": "quick-resource-test", "action": "cancel", "warningConfirmed": false }));
        assert!(!file.0.with_extension("tmp").exists());
    }
}
