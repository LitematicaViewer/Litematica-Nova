#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod application;
#[path = "../../../tools/viewer-core/src/game_data.rs"]
mod game_data;
// Compile the same public data resolver, not a second Host-side importer.
mod runtime_paths {
    pub fn app_root() -> anyhow::Result<std::path::PathBuf> { Ok(crate::get_root()) }
    pub fn data_root() -> anyhow::Result<std::path::PathBuf> {
        crate::current_user_config_dir().map_err(anyhow::Error::msg)
    }
}
mod i18n;
mod headless_e2e;
mod platform;
#[cfg(target_os="macos")]
mod native_macos;
mod render_service_client;
mod viewer_backend;
mod viewer_compatibility;
mod viewer_launch_contract;
mod shell_integration;
mod online_sources;
mod assets;
mod wiki_block_icons;

use crate::viewer_backend::ViewerBackendIdentity;
use crate::viewer_launch_contract::ViewerLaunchContext;
use ::viewer_launch_contract::ViewerControls;
use application::model::{
    ApplicationRenderSnapshot, GraphicsBackendPreference, IrisFallbackPreference, LayerSelection,
    OperationId, PresentationIntent, PresentationMode, ProductPreviewImage, ProjectRenderSettings,
    ProjectSource, RenderBuildMode, RenderDisplayMode, ResourcePreflightId, ResourcePreflightView,
    ShaderSelection, UserVisibleRenderRequest, WeatherPreference,
};
use application::operations::{
    ApplicationOperations, EditProjectMetadataInput, EditProjectOriginInput, EmbeddedPreview, GenerateProjectInput,
    GenerateProjectResult, InspectProjectInput, ProjectMaterialsInput, ProjectMutationResult,
    ProjectStatsInput, ReplaceBlocksInput, ReplaceBlocksResult, RuntimeHealth,
    LayerContainerData, LayerSliceData, LayerSliceMeta, ProjectionMarkerEditSummary,
    ProjectionMarkers, ReadLayerContainerInput, ReadLayerInput,
    ExportMaterialListInput, MaterialRecipeCacheInput, MaterialDetailInput, ExportWorldInput, StockpileTaskInput, StockpileTaskCommandInput, StockpileTaskSetInput, StockpileTaskResetInput,
};
use application::quick_preview::{QuickPreviewHost, QuickPreviewView, StartQuickPreviewInput, QuickPresentationIntent};
use application::updater::{UpdateHost, application_update_snapshot, application_update_check,
    application_update_download, application_update_install, application_update_cancel, application_update_skip, application_update_restart};
use application::render_facade::{
    RenderEnvironmentInputs, RenderFacade, RenderStartAuthorization, APPLICATION_RENDER_STATE_EVENT,
};
use base64::Engine;
use render_service_client::RenderServiceClient;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use viewer_compatibility::{
    CompatibilityScope, V5CompatibilityConfig, V5CompatibilityConfigInput,
    ViewerCompatibilityInputs, ViewerCompatibilityPlan,
};

static APP_RESOURCE_ROOT: OnceLock<PathBuf> = OnceLock::new();
static APP_LOCAL_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();
const PRODUCT_DATA_DIRECTORY: &str = "Litematica-Nova";
static SHUTDOWN_STARTED: AtomicBool = AtomicBool::new(false);
static CONFIG_WRITE_LOCK: Mutex<()> = Mutex::new(());
static FLAKE_RESOURCE_SEED_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const CLOSE_DRAFT_ACK_WARNING_AFTER: Duration = Duration::from_secs(5);

#[derive(Default)]
struct ApplicationCloseGate(Mutex<CloseGateState>);
#[derive(Default)]
struct CloseGateState { sequence: u64, pending: Option<CloseAttempt> }
struct CloseAttempt { request_id: u64, started: std::time::Instant, approved: bool }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationCloseRequest { request_id: u64 }
enum CloseDecision {
    AwaitDrafts { request: ApplicationCloseRequest, created: bool },
    Approved { request_id: u64, started: std::time::Instant },
}
impl ApplicationCloseGate {
    fn native_request(&self) -> CloseDecision {
        let mut state = self.0.lock().unwrap();
        if let Some(attempt) = state.pending.as_ref() {
            if attempt.approved {
                let attempt = state.pending.take().unwrap();
                return CloseDecision::Approved { request_id: attempt.request_id, started: attempt.started };
            }
            return CloseDecision::AwaitDrafts { request: ApplicationCloseRequest { request_id: attempt.request_id }, created: false };
        }
        state.sequence += 1;
        let request_id = state.sequence;
        state.pending = Some(CloseAttempt { request_id, started: std::time::Instant::now(), approved: false });
        CloseDecision::AwaitDrafts { request: ApplicationCloseRequest { request_id }, created: true }
    }
    fn snapshot(&self) -> Option<ApplicationCloseRequest> {
        self.0.lock().unwrap().pending.as_ref().filter(|attempt| !attempt.approved)
            .map(|attempt| ApplicationCloseRequest { request_id: attempt.request_id })
    }
    fn acknowledge(&self, request_id: u64, preserved: bool) -> Result<bool, String> {
        let mut state = self.0.lock().unwrap();
        let attempt = state.pending.as_mut().filter(|attempt| attempt.request_id == request_id)
            .ok_or("关闭请求已取消，请再次关闭窗口")?;
        if !preserved { state.pending = None; return Ok(false); }
        if attempt.approved { return Ok(false); }
        attempt.approved = true;
        eprintln!("[NOVA_LIFECYCLE] stage=close_drafts_preserved request_id={request_id} elapsed_ms={}", attempt.started.elapsed().as_millis());
        Ok(true)
    }
    fn cancel_pending(&self, request_id: u64) -> Option<Duration> {
        let mut state = self.0.lock().unwrap();
        if state.pending.as_ref().is_some_and(|attempt| attempt.request_id == request_id) {
            return Some(state.pending.take().unwrap().started.elapsed());
        }
        None
    }
    fn pending_elapsed(&self, request_id: u64) -> Option<Duration> {
        self.0.lock().unwrap().pending.as_ref()
            .filter(|attempt| attempt.request_id == request_id && !attempt.approved)
            .map(|attempt| attempt.started.elapsed())
    }
}

#[tauri::command]
fn application_get_close_request(window: tauri::WebviewWindow, gate: tauri::State<'_, ApplicationCloseGate>) -> Result<Option<ApplicationCloseRequest>, String> {
    if window.label() != "main" { return Err("关闭操作仅由主窗口执行".into()); }
    let pending = gate.snapshot();
    if let Some(request) = pending { eprintln!("[NOVA_LIFECYCLE] stage=close_frontend_request_received request_id={}", request.request_id); }
    Ok(pending)
}
#[tauri::command]
fn application_confirm_close(window: tauri::WebviewWindow, gate: tauri::State<'_, ApplicationCloseGate>, request_id: u64, drafts_preserved: bool) -> Result<(), String> {
    if window.label() != "main" { return Err("关闭操作仅由主窗口执行".into()); }
    eprintln!("[NOVA_LIFECYCLE] stage=close_frontend_ack_received request_id={request_id} drafts_preserved={drafts_preserved}");
    if window.try_state::<UpdateHost>().is_some_and(|host| !host.prepare_for_close(window.app_handle())) {
        let _ = gate.cancel_pending(request_id);
        return Err("正在安装更新，请完成后再关闭".into());
    }
    if gate.acknowledge(request_id, drafts_preserved)? {
        if let Err(error) = window.close() { let _ = gate.cancel_pending(request_id); return Err(error.to_string()); }
    }
    Ok(())
}
#[tauri::command]
fn application_request_close(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" { return Err("关闭操作仅由主窗口执行".into()); }
    window.close().map_err(|error| error.to_string())
}

#[cfg(test)]
mod close_gate_tests {
    use super::*;
    #[test]
    fn native_close_requires_storage_ack_deduplicates_and_keeps_the_first_clock() {
        let gate = ApplicationCloseGate::default();
        let CloseDecision::AwaitDrafts { request, created } = gate.native_request() else { panic!("first X must await drafts"); };
        assert!(created);
        let first = gate.0.lock().unwrap().pending.as_ref().unwrap().started;
        let CloseDecision::AwaitDrafts { request: duplicate, created } = gate.native_request() else { panic!("unapproved X must not exit"); };
        assert!(!created); assert_eq!(duplicate.request_id, request.request_id);
        assert!(gate.acknowledge(request.request_id + 1, true).is_err());
        assert!(gate.acknowledge(request.request_id, true).unwrap());
        let CloseDecision::Approved { started, request_id } = gate.native_request() else { panic!("valid storage ACK must permit bounded teardown"); };
        assert_eq!(started, first); assert_eq!(request_id, request.request_id); assert!(gate.snapshot().is_none());
    }
    #[test]
    fn failed_storage_and_missing_ack_keep_the_window_open_and_allow_retry() {
        let gate = ApplicationCloseGate::default();
        let CloseDecision::AwaitDrafts { request, .. } = gate.native_request() else { unreachable!() };
        assert!(!gate.acknowledge(request.request_id, false).unwrap());
        assert!(gate.snapshot().is_none());
        let CloseDecision::AwaitDrafts { request: retry, created } = gate.native_request() else { unreachable!() };
        assert!(created); assert_ne!(retry.request_id, request.request_id);
        assert!(gate.cancel_pending(retry.request_id).is_some());
        assert!(gate.acknowledge(retry.request_id, true).is_err());
        assert!(matches!(gate.native_request(), CloseDecision::AwaitDrafts { created: true, .. }));
    }
    #[test]
    fn delayed_storage_ack_keeps_the_same_close_transaction_after_warning() {
        let gate = ApplicationCloseGate::default();
        let CloseDecision::AwaitDrafts { request, .. } = gate.native_request() else { unreachable!() };
        gate.0.lock().unwrap().pending.as_mut().unwrap().started -= CLOSE_DRAFT_ACK_WARNING_AFTER + Duration::from_millis(1);
        assert!(gate.pending_elapsed(request.request_id).unwrap() >= CLOSE_DRAFT_ACK_WARNING_AFTER);
        let CloseDecision::AwaitDrafts { request: repeated, created } = gate.native_request() else { panic!("no storage ACK means no exit"); };
        assert!(!created); assert_eq!(repeated.request_id, request.request_id);
        assert_eq!(gate.snapshot().unwrap().request_id, request.request_id);
        assert!(gate.acknowledge(request.request_id, true).unwrap(), "a real delayed storage ACK is not stale merely because five seconds elapsed");
        assert!(gate.pending_elapsed(request.request_id).is_none());
        assert!(!gate.acknowledge(request.request_id, true).unwrap());
        assert!(matches!(gate.native_request(), CloseDecision::Approved { request_id, .. } if request_id == request.request_id));
    }
    #[cfg(windows)]
    #[test]
    fn acknowledged_close_reaps_owned_inflight_render_roles_without_waiting_for_build_completion() {
        use platform::process_supervisor::{ExecutableAllowlist, ExecutableRole, LaunchIdentity, LaunchSpec, LaunchToken, ProcessSupervisor};
        let command = std::env::var_os("COMSPEC").map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        let mut allowlist = ExecutableAllowlist::new();
        for role in [ExecutableRole::RenderService, ExecutableRole::BuildWorker, ExecutableRole::NativeViewer] {
            allowlist.allow(role, &command).unwrap();
        }
        let (supervisor, _events) = ProcessSupervisor::new(allowlist).unwrap();
        let mut owned = Vec::new();
        for (index, role) in [ExecutableRole::RenderService, ExecutableRole::BuildWorker, ExecutableRole::NativeViewer].into_iter().enumerate() {
            let mut spec = LaunchSpec::new(role, LaunchIdentity {
                owner_epoch: supervisor.owner_epoch(), generation: 1,
                launch_token: LaunchToken::new(format!("close-inflight-owned-{index}")).unwrap(),
            });
            // Hidden, supervisor-owned long-running fixtures stand in for busy
            // render roles; no Nova window, projection or user process is touched.
            spec.args = vec!["/d".into(), "/c".into(), "ping -n 30 127.0.0.1 >nul".into()];
            owned.push(supervisor.spawn(spec).unwrap().child);
        }
        let gate = ApplicationCloseGate::default();
        let CloseDecision::AwaitDrafts { request, .. } = gate.native_request() else { unreachable!() };
        gate.0.lock().unwrap().pending.as_mut().unwrap().started -= CLOSE_DRAFT_ACK_WARNING_AFTER + Duration::from_millis(1);
        assert!(gate.pending_elapsed(request.request_id).is_some());
        assert_eq!(supervisor.active_children().len(), 3, "a missing draft ACK must not terminate an active render");
        assert!(gate.acknowledge(request.request_id, true).unwrap());
        let CloseDecision::Approved { request_id, .. } = gate.native_request() else { panic!("preserved drafts must permit closing even while a build is active"); };
        assert_eq!(request_id, request.request_id);
        assert_eq!(supervisor.active_children().len(), 3, "ACK does not wait for synthetic build completion");
        let started = std::time::Instant::now();
        supervisor.stop_admitting();
        let report = supervisor.shutdown(Duration::from_millis(500), Duration::from_millis(1500));
        assert!(report.remaining.is_empty());
        assert_eq!(report.terminate_requests, 3);
        assert!(started.elapsed() < Duration::from_secs(3), "owned teardown must use the existing shared bounded deadline, not the build's 30-second lifetime");
        assert!(supervisor.active_children().is_empty());
        println!("[NOVA_CLOSE_REGRESSION] same_request_after_warning=true storage_ack_required=true active_roles_before_ack={} remaining={} teardown_ms={} GUI=UNMEASURED", owned.len(), report.remaining.len(), started.elapsed().as_millis());
    }
}

#[derive(Serialize)]
struct PathInfo {
    raw: String,
    normalized: String,
    parent_dir: String,
    parent_exists: bool,
    exists: bool,
    is_dir: bool,
    is_file: bool,
    is_absolute: bool,
    has_litematic_ext: bool,
}

#[derive(Serialize)]
struct DirectoryEntryInfo {
    path: String,
    name: String,
    is_dir: bool,
    is_file: bool,
    file_size: u64,
    mtime_ms: u128,
    extension: String,
}

#[derive(Serialize)]
struct CopyFileToDirectoryOutput {
    target_path: String,
    overwritten: bool,
    bytes_copied: u64,
}

#[derive(Deserialize, Serialize, Clone)]
struct UserConfig {
    #[serde(default = "default_ui_language")]
    ui_language: String,
    #[serde(default = "default_ui_locale")]
    ui_locale: String,
    theme: String,
    #[serde(default = "default_color_mode")]
    color_mode: String,
    render_display_mode: String,
    preview_mode: String,
    #[serde(default = "default_material_list_window_behavior")]
    material_list_window_behavior: String,
    #[serde(default = "default_statistics_enumerator_info_rules")]
    statistics_enumerator_info_rules: Vec<String>,
    #[serde(default = "default_show_ui_test_page")]
    show_ui_test_page: bool,
    #[serde(default)]
    debug_mode: bool,
    #[serde(default = "default_local_library_tail_path_count")]
    local_library_tail_path_count: u32,
    #[serde(default)]
    v5_compatibility: V5CompatibilityConfig,
    #[serde(default)]
    viewer_controls: ViewerControlConfig,
    #[serde(default)]
    graphics_backend: ::viewer_launch_contract::GraphicsApiPreference,
}

#[derive(Deserialize)]
struct UserConfigInput {
    ui_language: Option<String>,
    ui_locale: Option<String>,
    theme: Option<String>,
    color_mode: Option<String>,
    render_display_mode: Option<String>,
    preview_mode: Option<String>,
    material_list_window_behavior: Option<String>,
    statistics_enumerator_info_rules: Option<Vec<String>>,
    show_ui_test_page: Option<bool>,
    debug_mode: Option<bool>,
    local_library_tail_path_count: Option<u32>,
    v5_compatibility: Option<V5CompatibilityConfigInput>,
    viewer_controls: Option<ViewerControlConfig>,
    graphics_backend: Option<::viewer_launch_contract::GraphicsApiPreference>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ViewerControlConfig {
    #[serde(default = "default_true")]
    allow_time_modification: bool,
    #[serde(default = "default_true")]
    allow_weather_modification: bool,
}

impl Default for ViewerControlConfig {
    fn default() -> Self {
        Self {
            allow_time_modification: true,
            allow_weather_modification: true,
        }
    }
}

impl From<ViewerControlConfig> for ViewerControls {
    fn from(value: ViewerControlConfig) -> Self {
        Self {
            allow_time_modification: value.allow_time_modification,
            allow_weather_modification: value.allow_weather_modification,
        }
    }
}

#[derive(Serialize)]
struct UserConfigInfo {
    config_dir: String,
    default_config_dir: String,
    config: UserConfig,
}

#[derive(Deserialize, Serialize, Clone)]
struct AiStoredConfig {
    provider: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Deserialize)]
struct AiSaveConfigInput {
    provider: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Serialize)]
struct AiPublicConfig {
    provider: String,
    base_url: String,
    model: String,
    has_key: bool,
    key_status: String,
    storage_note: String,
}

#[derive(Serialize)]
struct AiTestResult {
    ok: bool,
    message: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct AiChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AiChatCompletionInput {
    messages: Vec<AiChatMessage>,
}

#[derive(Serialize)]
struct AiChatCompletionOutput {
    provider: String,
    model: String,
    content: String,
}

fn get_root() -> PathBuf {
    if let Some(resource_root) = APP_RESOURCE_ROOT.get() {
        if resource_root.join("bin").join("viewer-backend").is_dir() {
            return resource_root.clone();
        }
    }
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    find_workspace_root(&current_dir)
        .or_else(|| find_workspace_root(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .unwrap_or(current_dir)
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join("bin").join("viewer-backend").is_dir() {
            return Some(candidate.to_path_buf());
        }
        if candidate.ends_with("src-tauri") {
            let maybe_root = candidate.parent()?.parent()?;
            if maybe_root.join("bin").join("viewer-backend").is_dir() {
                return Some(maybe_root.to_path_buf());
            }
        }
        if candidate.ends_with("desktop-nova") {
            let maybe_root = candidate.parent()?;
            if maybe_root.join("bin").join("viewer-backend").is_dir() {
                return Some(maybe_root.to_path_buf());
            }
        }
    }
    None
}

fn backend_exe_path(binary_name: &str) -> PathBuf {
    // The ordinary development entry has already built these real public
    // helpers. Use that exact stage, not a stale installed/private backend.
    #[cfg(debug_assertions)]
    if let Some(directory) = option_env!("NOVA_PUBLIC_BACKEND_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(directory).join(binary_name);
    }
    get_root()
        .join("bin")
        .join("viewer-backend")
        .join(binary_name)
}

fn target_backend_file_name(binary_name: &str) -> Result<String, String> {
    // Existing callers use logical Windows names. Resolve once at the platform
    // boundary; never accept paths, aliases or arbitrary helper executables.
    let stem = match binary_name {
        "litematica_core.exe" => "litematica_core",
        "litematica_native_viewer.exe" => "litematica_native_viewer",
        "nova-build-worker.exe" => "nova-build-worker",
        "nova-render-service.exe" => "nova-render-service",
        _ => return Err(format!("不允许的后端可执行文件名 {binary_name:?}；不得包含路径、别名或大小写变体。")),
    };
    Ok(format!("{stem}{}", std::env::consts::EXE_SUFFIX))
}

fn required_backend_exe_path(binary_name: &str) -> Result<PathBuf, String> {
    let path = backend_exe_path(&target_backend_file_name(binary_name)?);
    if path.is_file() { return Ok(path); }
    Err(format!("缺少当前平台后端可执行文件：{}；请部署与本程序版本及架构匹配的 bin/viewer-backend。",path.display()))
}

#[cfg(test)]
mod backend_executable_path_tests {
    use super::required_backend_exe_path;

    #[test]
    fn backend_file_suffix_matches_the_current_target() {
        assert_eq!(super::target_backend_file_name("nova-build-worker.exe").unwrap(),
            format!("nova-build-worker{}",std::env::consts::EXE_SUFFIX));
        assert!(super::target_backend_file_name("/tmp/nova-build-worker").is_err());
    }

    #[test]
    fn backend_binary_names_are_an_exact_allowlist() {
        for alias in [
            r".\litematica_native_viewer.exe",
            "LITEMATICA_NATIVE_VIEWER.EXE",
            r"sub\..\litematica_native_viewer.exe",
            "../litematica_core.exe",
        ] {
            let error = required_backend_exe_path(alias).unwrap_err();
            assert!(error.contains("不允许的后端可执行文件名"));
        }
    }

    #[test]
    #[ignore = "requires the actual prepared Community helpers; no GUI or child launch"]
    fn community_host_starts_with_public_helpers_and_rejects_optional_core() {
        assert!(nova_protected_loader::is_community_build(), "this fixture requires the Community build, not a signed/private runtime");
        let project = super::get_root();
        let data = project.join(".tmp/beta-ui-corrections/community-startup-data");
        let render = super::RenderServiceClient::default();
        // These are the actual public helpers prepared by the normal entry,
        // not empty executables or a substitute private renderer.
        for name in ["litematica_core.exe", "litematica_native_viewer.exe"] {
            assert!(std::fs::metadata(super::required_backend_exe_path(name).unwrap()).unwrap().len() > 64);
        }
        let host = super::initialize_application_host(project, data.clone(), &render).unwrap();
        assert!(host.service_exe.is_none());
        assert_eq!(host.supervisor.registered_role_count(), 2);
        assert!(host.supervisor.active_children().is_empty());
        let mut protection = super::application::protected_render::ProtectedRender::default();
        for mode in ["standard", "full"] {
            let request = serde_json::to_vec(&serde_json::json!({"displayMode":mode})).unwrap();
            assert_eq!(protection.acquire(&data, &data, &request).unwrap_err(), nova_protected_loader::FEATURE_NOT_INCLUDED);
        }
        render.request_shutdown();
        let report = host.supervisor.shutdown(std::time::Duration::ZERO, std::time::Duration::from_millis(500));
        assert!(report.remaining.is_empty());
    }

    #[test]
    #[ignore = "actual staged Release backend Host initialization only; no GUI or service launch"]
    #[cfg(windows)]
    fn startup_host_initialization_profile() {
        assert!(!cfg!(debug_assertions), "启动阶段测量必须使用 Release Host");
        let project = super::get_root();
        let data = project.join(".tmp/beta-ui-corrections/startup-profile-data");
        let binaries = ["litematica_core.exe", "litematica_native_viewer.exe", "nova-build-worker.exe", "nova-render-service.exe"]
            .into_iter().map(|name| {
                let path = required_backend_exe_path(name).unwrap();
                let bytes = std::fs::metadata(&path).unwrap().len();
                assert!(bytes > 0);
                serde_json::json!({"name":name,"path":path,"bytes":bytes})
            }).collect::<Vec<_>>();
        let seeds_before = super::FLAKE_RESOURCE_SEED_CALLS.load(super::Ordering::Relaxed);
        let render = super::RenderServiceClient::default();
        let started = std::time::Instant::now();
        let host = super::initialize_application_host(project.clone(), data, &render).unwrap();
        let initialize_ms = started.elapsed().as_secs_f64() * 1000.0;
        let activity = host.operations.lifecycle_snapshot();
        let seeds_after = super::FLAKE_RESOURCE_SEED_CALLS.load(super::Ordering::Relaxed);
        assert!(activity["coreCalls"].as_object().unwrap().is_empty(), "Host初始化不得启动读取/资源扫描Core");
        assert_eq!(activity["activeReads"], 0); assert_eq!(activity["pendingWrites"], 0);
        assert_eq!(seeds_before, 0); assert_eq!(seeds_after, 0, "初始化不能复制扫描Flake资源");
        assert!(host.supervisor.active_children().is_empty(), "本测试不启动后台服务或Viewer");
        render.request_shutdown();
        let stopped = host.supervisor.shutdown(std::time::Duration::ZERO, std::time::Duration::from_millis(500));
        assert!(stopped.remaining.is_empty()); assert_eq!(stopped.terminate_requests, 0);
        let report = serde_json::json!({
            "profile":"Release","measurement":"actual synchronous initialize_application_host only; no GUI/service launch",
            "hostInitializeMs":initialize_ms,"operations":activity,"resourceSeedCallsBefore":seeds_before,"resourceSeedCallsAfter":seeds_after,
            "backendFiles":binaries,"ownedChildren":0,"remainingChildren":0,
            "updateQuery":"initializer has no updater/AppHandle invocation; frontend async query not exercised",
            "mainWindowResponse":"UNMEASURED","backgroundServiceConnect":"UNMEASURED","WebViewBootstrap":"UNMEASURED"
        });
        let output = project.join(".tmp/beta-ui-corrections/beta02-startup-host-release.json");
        std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!("[NOVA_LIFECYCLE_PROFILE] startup={report} output={}", output.display());
    }
}

#[cfg(test)]
mod product_runtime_path_tests {
    use super::{product_local_data_root, PRODUCT_DATA_DIRECTORY};
    use std::path::Path;

    #[test]
    fn product_runtime_root_uses_the_formal_product_directory() {
        let local_data = Path::new(r"C:\Users\viewer\AppData\Local");
        assert_eq!(
            product_local_data_root(local_data),
            local_data.join("Litematica-Nova")
        );
        assert_eq!(PRODUCT_DATA_DIRECTORY, "Litematica-Nova");
    }
}

fn data_root() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    let dir = env::var_os("LBA_DATA_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                get_root().join(path)
            }
        })
        .unwrap_or_else(|| get_root().join("data"));
    #[cfg(not(debug_assertions))]
    let dir = APP_LOCAL_DATA_ROOT.get().cloned().ok_or_else(|| {
        "Litematica-Nova local data directory is not initialized; refusing to write beside the installed executable"
            .to_owned()
    })?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn product_local_data_root(local_data_dir: &Path) -> PathBuf {
    local_data_dir.join(PRODUCT_DATA_DIRECTORY)
}

fn legacy_appdata_config_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(2);
    if let Some(base) = env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(base).join("com.litematicaba.nova"));
    }
    if let Some(base) = env::var_os("APPDATA").filter(|value| !value.is_empty()) {
        dirs.push(
            PathBuf::from(base)
                .join("Litematica-BA")
                .join("desktop-nova"),
        );
    }
    dirs
}

fn render_tmp_path(prefix: &str, ext: &str) -> Result<PathBuf, String> {
    let dir = data_root()?.join("cache").join("render");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    Ok(dir.join(format!("{prefix}_{now}_{:x}.{ext}", std::process::id())))
}

fn app_config_dir() -> Result<PathBuf, String> {
    let dir = data_root()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    migrate_legacy_appdata_if_needed(&dir)?;
    Ok(dir)
}

fn copy_file_if_missing(source: &Path, target: &Path) -> Result<(), String> {
    if !source.is_file() || target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(source, target)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn copy_dir_files_if_missing(source: &Path, target: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            copy_dir_files_if_missing(&source_path, &target_path)?;
        } else if metadata.is_file() {
            copy_file_if_missing(&source_path, &target_path)?;
        }
    }
    Ok(())
}

/// These editable defaults belong to the selected user directory, not to the
/// executable directory. Fill missing files without replacing custom content.
fn seed_flake_resources(resource_root: &Path, user_root: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let call = FLAKE_RESOURCE_SEED_CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    for name in ["enumerator", "flake"] {
        let source = resource_root.join("pack-in").join(name);
        if !source.is_dir() {
            return Err(format!("Missing bundled Flake resources: {}", source.display()));
        }
        copy_dir_files_if_missing(&source, &user_root.join(name))?;
    }
    eprintln!("[NOVA_LIFECYCLE] stage=resource_seed calls={call} elapsed_ms={}", started.elapsed().as_millis());
    Ok(())
}

fn initialize_flake_user_resources() -> Result<(), String> {
    // A Release must use its distributed resources, never the build machine's
    // CARGO_MANIFEST_DIR or a coincidental source checkout in the working dir.
    #[cfg(not(debug_assertions))]
    let resource_root = APP_RESOURCE_ROOT.get().cloned()
        .ok_or_else(|| "Application resource directory is not initialized".to_owned())?;
    #[cfg(debug_assertions)]
    let resource_root = get_root();
    seed_flake_resources(&resource_root, &current_user_config_dir()?)
}

#[cfg(test)]
mod flake_resource_tests {
    use super::seed_flake_resources;
    use std::fs;
    use std::path::PathBuf;

    fn fixture_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "nova-flake-seed-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(root.join("release/pack-in/enumerator/system_enum")).unwrap();
        fs::create_dir_all(root.join("release/pack-in/flake/state_hint")).unwrap();
        fs::write(root.join("release/pack-in/enumerator/system_enum/waterlogged.json"), b"[\"minecraft:stairs\"]").unwrap();
        fs::write(root.join("release/pack-in/flake/state_hint/waterlogged.png"), b"bundled image").unwrap();
        root
    }

    fn remove_fixture(root: PathBuf) {
        let absolute = fs::canonicalize(&root).unwrap();
        assert!(absolute.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("nova-flake-seed-"));
        fs::remove_dir_all(absolute).unwrap();
    }

    #[test]
    fn flake_defaults_initialize_selected_directory_and_preserve_custom_files() {
        let root = fixture_root();
        let resources = root.join("release");
        let user = root.join("selected-user-data");
        seed_flake_resources(&resources, &user).unwrap();
        let catalog = user.join("enumerator/system_enum/waterlogged.json");
        let image = user.join("flake/state_hint/waterlogged.png");
        assert_eq!(fs::read(&catalog).unwrap(), b"[\"minecraft:stairs\"]");
        assert_eq!(fs::read(&image).unwrap(), b"bundled image");
        fs::write(&catalog, b"user catalog").unwrap();
        fs::write(&image, b"user image").unwrap();
        fs::write(resources.join("pack-in/flake/state_hint/new.png"), b"new default").unwrap();
        seed_flake_resources(&resources, &user).unwrap();
        assert_eq!(fs::read(&catalog).unwrap(), b"user catalog");
        assert_eq!(fs::read(&image).unwrap(), b"user image");
        assert_eq!(fs::read(user.join("flake/state_hint/new.png")).unwrap(), b"new default");
        let second_user = root.join("new-selected-user-data");
        seed_flake_resources(&resources, &second_user).unwrap();
        assert_eq!(fs::read(second_user.join("flake/state_hint/new.png")).unwrap(), b"new default");
        remove_fixture(root);
    }

    #[test]
    fn flake_missing_distributed_resources_are_not_silently_accepted() {
        let root = fixture_root();
        assert!(seed_flake_resources(&root.join("not-a-release"), &root.join("user")).is_err());
        remove_fixture(root);
    }

    #[test]
    fn flake_bundle_resources_are_explicit_and_do_not_include_private_archives() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let resources = config["bundle"]["resources"].as_object().unwrap();
        assert_eq!(resources.get("../../pack-in/enumerator").unwrap(), "pack-in/enumerator");
        assert_eq!(resources.get("../../pack-in/flake").unwrap(), "pack-in/flake");
        assert_eq!(resources.get("../../pack-in/lang/zh_cn.json").unwrap(), "pack-in/lang/zh_cn.json");
        assert_eq!(resources.get("gen/block-icons").unwrap(), "block");
        assert!(!resources.keys().any(|key| key == "../../pack-in" || key.contains("arr-private")));
    }
}

fn migrate_legacy_appdata_if_needed(target_root: &Path) -> Result<(), String> {
    static MIGRATED_ROOTS: OnceLock<Mutex<std::collections::BTreeSet<PathBuf>>> = OnceLock::new();
    let mut migrated = MIGRATED_ROOTS.get_or_init(Mutex::default).lock().map_err(|_| "配置迁移锁异常")?;
    if migrated.contains(target_root) { return Ok(()); }
    let started = std::time::Instant::now();
    for legacy_root in legacy_appdata_config_dirs() {
        if legacy_root == target_root || !legacy_root.is_dir() {
            continue;
        }
        for file in ["config.json", "config_dir.json", "ai_config.json"] {
            copy_file_if_missing(&legacy_root.join(file), &target_root.join(file))?;
        }
        for dir in ["projection-library", "generation-templates/custom"] {
            copy_dir_files_if_missing(&legacy_root.join(dir), &target_root.join(dir))?;
        }
    }
    migrated.insert(target_root.to_path_buf());
    eprintln!("[NOVA_LIFECYCLE] stage=legacy_config_migration elapsed_ms={}", started.elapsed().as_millis());
    Ok(())
}

fn ai_config_path() -> Result<PathBuf, String> {
    let path = app_config_dir()?.join("ai_config.json");
    if !path.exists() {
        if let Some(base) = env::var_os("APPDATA") {
            let old_path = PathBuf::from(base)
                .join("Litematica-BA")
                .join("ai_config.json");
            if old_path.is_file() {
                let _ = std::fs::copy(old_path, &path);
            }
        }
    }
    Ok(path)
}

fn default_user_config_dir() -> Result<PathBuf, String> {
    app_config_dir()
}

fn config_dir_pointer_path() -> Result<PathBuf, String> {
    Ok(default_user_config_dir()?.join("config_dir.json"))
}

fn read_selected_config_dir() -> Option<PathBuf> {
    let pointer = config_dir_pointer_path().ok()?;
    let text = std::fs::read_to_string(pointer).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("config_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
}

fn current_user_config_dir() -> Result<PathBuf, String> {
    let dir = read_selected_config_dir().unwrap_or(default_user_config_dir()?);
    ensure_user_config_layout(&dir)?;
    Ok(dir)
}

fn ensure_user_config_layout(dir: &Path) -> Result<(), String> {
    for relative in [
        "",
        "projection-library",
        "projection-library/previews",
        "cache/render",
        "tmp/render",
        "reden/downloads",
        "exports",
        "generation-templates",
        "generation-templates/custom",
        "replace/presets",
    ] {
        std::fs::create_dir_all(dir.join(relative)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn default_user_config() -> UserConfig {
    UserConfig {
        ui_language: default_ui_language(),
        ui_locale: default_ui_locale(),
        theme: "shadcn".to_string(),
        color_mode: default_color_mode(),
        render_display_mode: "full".to_string(),
        preview_mode: "full".to_string(),
        material_list_window_behavior: default_material_list_window_behavior(),
        statistics_enumerator_info_rules: default_statistics_enumerator_info_rules(),
        show_ui_test_page: default_show_ui_test_page(),
        debug_mode: false,
        local_library_tail_path_count: default_local_library_tail_path_count(),
        v5_compatibility: V5CompatibilityConfig::default(),
        viewer_controls: ViewerControlConfig::default(),
        graphics_backend: Default::default(),
    }
}

fn default_ui_language() -> String { "zh-CN".into() }
fn default_ui_locale() -> String { "zh-CN".into() }
fn normalize_ui_language(value: &str) -> String {
    match value { "en-US" => "en-US", "system" => "system", _ => "zh-CN" }.into()
}
fn normalize_ui_locale(value: &str) -> String {
    (if value == "en-US" { "en-US" } else { "zh-CN" }).into()
}

fn default_true() -> bool {
    true
}

fn default_statistics_enumerator_info_rules() -> Vec<String> {
    vec!["理论版本 = version(L)".into(), "生存不可达 = any(L & survival_impossible)".into()]
}

#[cfg(test)]
mod viewer_control_config_tests {
    use super::{UserConfig, ViewerControlConfig};

    #[test]
    fn missing_debug_setting_defaults_off_and_explicit_value_survives() {
        let old = serde_json::json!({"theme":"shadcn","render_display_mode":"full","preview_mode":"full"});
        let config: UserConfig = serde_json::from_value(old.clone()).unwrap();
        assert!(!config.debug_mode);
        let mut enabled = old;
        enabled["debug_mode"] = true.into();
        assert!(serde_json::from_value::<UserConfig>(enabled).unwrap().debug_mode);
    }

    #[test]
    fn graphics_backend_defaults_persists_and_does_not_disguise_exact_modes() {
        use ::viewer_launch_contract::GraphicsApiPreference as Api;
        let old = serde_json::json!({"theme":"shadcn","render_display_mode":"full","preview_mode":"full"});
        assert_eq!(serde_json::from_value::<UserConfig>(old.clone()).unwrap().graphics_backend, Api::Auto);
        for backend in [Api::Auto, Api::Dx12, Api::Vulkan] {
            let mut value = old.clone();
            value["graphics_backend"] = serde_json::to_value(backend).unwrap();
            let saved: UserConfig = serde_json::from_value(value).unwrap();
            assert_eq!(serde_json::from_slice::<UserConfig>(&serde_json::to_vec(&saved).unwrap()).unwrap().graphics_backend, backend);
            assert_eq!(super::validate_exact_graphics_preference(backend).is_ok(), backend != Api::Dx12);
        }
        let mut bad = old; bad["graphics_backend"] = "opengl".into();
        assert!(serde_json::from_value::<UserConfig>(bad).is_err());
    }

    #[test]
    fn statistics_rules_survive_native_config_save_and_reload() {
        let dir = std::env::temp_dir().join(format!("nova-rules-config-{}", std::process::id()));
        let mut config = super::default_user_config();
        config.statistics_enumerator_info_rules = vec!["红石比例 = ratio(L & redstone / L)".into()];
        super::write_user_config_data(&dir, &config).unwrap();
        let mut loaded = super::read_user_config_data(&dir);
        assert_eq!(loaded.statistics_enumerator_info_rules, config.statistics_enumerator_info_rules);
        loaded.theme = "minecraft".into();
        super::write_user_config_data(&dir, &loaded).unwrap();
        assert_eq!(super::read_user_config_data(&dir).statistics_enumerator_info_rules, config.statistics_enumerator_info_rules);
        let old = serde_json::json!({"theme":"shadcn","render_display_mode":"full","preview_mode":"full"});
        assert_eq!(serde_json::from_value::<UserConfig>(old).unwrap().statistics_enumerator_info_rules, super::default_statistics_enumerator_info_rules());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn old_user_config_defaults_both_manual_world_controls_to_enabled() {
        let config = serde_json::from_value::<UserConfig>(serde_json::json!({
            "theme": "WebDefault",
            "render_display_mode": "full",
            "preview_mode": "full",
            "material_list_window_behavior": "independent_window",
            "show_ui_test_page": true,
            "local_library_tail_path_count": 3,
            "v5_compatibility": {}
        }))
        .unwrap();
        assert_eq!(config.viewer_controls, ViewerControlConfig::default());
        let serialized = serde_json::to_value(config).unwrap();
        assert_eq!(
            serialized["viewer_controls"]["allow_time_modification"],
            true
        );
        assert_eq!(
            serialized["viewer_controls"]["allow_weather_modification"],
            true
        );
    }

    #[test]
    fn explicit_disabled_controls_are_not_overwritten_by_defaults() {
        let controls = serde_json::from_value::<ViewerControlConfig>(serde_json::json!({
            "allow_time_modification": false,
            "allow_weather_modification": false
        }))
        .unwrap();
        assert!(!controls.allow_time_modification);
        assert!(!controls.allow_weather_modification);
    }
}

fn default_material_list_window_behavior() -> String {
    "independent_window".to_string()
}

fn default_show_ui_test_page() -> bool {
    true
}

fn default_local_library_tail_path_count() -> u32 {
    3
}

fn normalize_material_list_window_behavior(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "main_window_overlay" => "main_window_overlay".to_string(),
        _ => "independent_window".to_string(),
    }
}

fn normalize_local_library_tail_path_count(value: u32) -> u32 {
    if value >= 1 {
        value
    } else {
        default_local_library_tail_path_count()
    }
}

fn normalize_theme(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "8bitcn" => "8bitcn".to_string(),
        "minecraft" => "Minecraft".to_string(),
        "metro10" => "Metro10".to_string(),
        "aero7" => "aero7".to_string(),
        _ => "shadcn".to_string(),
    }
}

fn default_color_mode() -> String {
    "system".to_string()
}

fn normalize_color_mode(value: &str) -> String {
    match value {
        "light" | "dark" => value.to_string(),
        _ => default_color_mode(),
    }
}

#[cfg(test)]
mod theme_config_tests {
    use super::{default_user_config, normalize_color_mode, normalize_theme, UserConfig};

    #[test]
    fn appearance_preference_defaults_to_system_and_round_trips() {
        let mut legacy = serde_json::to_value(default_user_config()).unwrap();
        legacy.as_object_mut().unwrap().remove("color_mode");
        assert_eq!(
            serde_json::from_value::<UserConfig>(legacy).unwrap().color_mode,
            "system"
        );
        for mode in ["light", "dark", "system"] {
            assert_eq!(normalize_color_mode(mode), mode);
            let mut config = default_user_config();
            config.color_mode = mode.to_string();
            let restored: UserConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(restored.color_mode, mode);
        }
        assert_eq!(normalize_color_mode("unknown"), "system");
    }

    #[test]
    fn five_theme_ids_are_canonical_and_legacy_values_migrate() {
        for value in ["", "WebDefault", "Bootstrap5", "QTDefault", "unknown"] {
            assert_eq!(normalize_theme(value), "shadcn");
        }
        assert_eq!(normalize_theme(" SHADCN "), "shadcn");
        assert_eq!(normalize_theme(" 8BITCN "), "8bitcn");
        assert_eq!(normalize_theme(" minecraft "), "Minecraft");
        assert_eq!(normalize_theme(" METRO10 "), "Metro10");
        assert_eq!(normalize_theme(" aero7 "), "aero7");
        assert_eq!(default_user_config().theme, "shadcn");
    }
}

fn normalize_mode_string(value: &str) -> String {
    let _ = value;
    "full".to_string()
}

fn user_config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

fn read_user_config_data(dir: &Path) -> UserConfig {
    let path = user_config_path(dir);
    let mut config = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<UserConfig>(&text).ok())
        .unwrap_or_else(default_user_config);
    config.ui_language = normalize_ui_language(&config.ui_language);
    config.ui_locale = normalize_ui_locale(&config.ui_locale);
    if config.ui_language != "system" { config.ui_locale = config.ui_language.clone(); }
    config.theme = normalize_theme(&config.theme);
    config.color_mode = normalize_color_mode(&config.color_mode);
    config.render_display_mode = normalize_mode_string(&config.render_display_mode);
    config.preview_mode = normalize_mode_string(&config.preview_mode);
    config.material_list_window_behavior =
        normalize_material_list_window_behavior(&config.material_list_window_behavior);
    config.local_library_tail_path_count =
        normalize_local_library_tail_path_count(config.local_library_tail_path_count);
    config
}

fn write_user_config_data(dir: &Path, config: &UserConfig) -> Result<(), String> {
    ensure_user_config_layout(dir)?;
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    if std::fs::read_to_string(user_config_path(dir)).ok().as_deref() == Some(content.as_str()) { return Ok(()); }
    std::fs::write(
        user_config_path(dir),
        content,
    )
    .map_err(|e| e.to_string())
}

fn current_viewer_launch_context(
    scope: CompatibilityScope,
    requested_display_mode: &str,
) -> Result<ViewerLaunchContext, String> {
    current_launch_context_for_backend(
        scope,
        requested_display_mode,
        "litematica_native_viewer.exe",
    )
}

fn current_launch_context_for_backend(
    scope: CompatibilityScope,
    _requested_display_mode: &str,
    binary_name: &str,
) -> Result<ViewerLaunchContext, String> {
    let config_dir = current_user_config_dir()?;
    let config = read_user_config_data(&config_dir);
    let compatibility = config.v5_compatibility;
    let plan = ViewerCompatibilityPlan::build(&compatibility, &get_root(), scope)?;
    let exe_path = required_backend_exe_path(binary_name)?;
    let backend = ViewerBackendIdentity::capture(&exe_path)?;
    Ok(ViewerLaunchContext::new(
        plan,
        config.viewer_controls.into(),
        backend,
    ))
}

async fn prepare_viewer_launch_context(
    scope: CompatibilityScope,
    requested_display_mode: String,
) -> Result<ViewerLaunchContext, String> {
    tauri::async_runtime::spawn_blocking(move || {
        current_viewer_launch_context(scope, &requested_display_mode)
    })
    .await
    .map_err(|error| format!("prepare Viewer launch context worker failed: {error}"))?
}

fn migrate_legacy_library_if_needed(dir: &Path) -> Result<(), String> {
    let target = dir.join("projection-library").join("js_library.json");
    if target.exists() {
        return Ok(());
    }
    let legacy = get_root()
        .join("data")
        .join("projection-library")
        .join("js_library.json");
    if legacy.is_file() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(legacy, target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn safe_user_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Invalid user config relative path.".to_string());
    }
    Ok(path)
}

fn user_runtime_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let path = safe_user_relative_path(relative_path)?;
    let mut components = path.components();
    let Some(first) = components.next() else {
        return Ok(path);
    };
    let first = first.as_os_str().to_string_lossy();
    if first == "render" {
        return Ok(PathBuf::from("tmp").join(path));
    }
    if first == "previews" {
        return Ok(PathBuf::from("projection-library").join(path));
    }
    Ok(path)
}

fn default_ai_config() -> AiStoredConfig {
    AiStoredConfig {
        provider: "mock".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        model: "gpt-4.1-mini".to_string(),
        api_key: None,
    }
}

fn read_ai_config() -> AiStoredConfig {
    let Ok(path) = ai_config_path() else {
        return default_ai_config();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return default_ai_config();
    };
    serde_json::from_str(&text).unwrap_or_else(|_| default_ai_config())
}

fn write_ai_config(config: &AiStoredConfig) -> Result<(), String> {
    let path = ai_config_path()?;
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn public_ai_config(config: AiStoredConfig, key_status: &str) -> AiPublicConfig {
    AiPublicConfig {
        provider: config.provider,
        base_url: config.base_url,
        model: config.model,
        has_key: config.api_key.as_ref().is_some_and(|key| !key.is_empty()),
        key_status: key_status.to_string(),
        storage_note:
            "API Key is stored by the Tauri backend in local app config, not localStorage."
                .to_string(),
    }
}

fn normalized_input_path(file_path: &str) -> PathBuf {
    let input = PathBuf::from(file_path);
    if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    }
}

async fn run_application_operation<T>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("Application operation task failed: {error}"))?
}

#[tauri::command]
async fn application_inspect_project(
    input: InspectProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.inspect_project(input)).await
}

#[tauri::command]
async fn application_read_layer_bounds(input: InspectProjectInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_layer_bounds(input)).await
}

#[tauri::command]
async fn application_edit_project_metadata(
    input: EditProjectMetadataInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProjectMutationResult, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || {
        let preview_changed = input.patch.get("preview_image_data").is_some() || input.patch.get("preview_image_path").is_some();
        let result = operations.edit_project_metadata(input)?;
        if preview_changed { application::thumbnails::notify_shell(Path::new(&result.output_path)); }
        Ok(result)
    }).await
}

#[tauri::command]
async fn application_read_layer_meta(
    input: InspectProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<LayerSliceMeta, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_layer_meta(input)).await
}

#[tauri::command]
async fn application_read_layer(
    input: ReadLayerInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<LayerSliceData, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_layer(input)).await
}

#[tauri::command]
async fn application_read_layer_container(
    input: ReadLayerContainerInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<Option<LayerContainerData>, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_layer_container(input)).await
}

#[tauri::command]
async fn application_read_project_markers(
    input: InspectProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProjectionMarkers, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_project_markers(input)).await
}

#[tauri::command]
async fn application_edit_project_origin(
    input: EditProjectOriginInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProjectionMarkerEditSummary, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.edit_project_origin(input)).await
}

#[tauri::command]
async fn application_export_world(input: ExportWorldInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    let work=operations.begin_world_export(input)?;
    tauri::async_runtime::spawn_blocking(move || operations.export_world(work)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn application_world_export_profiles(source_path: String, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations=operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || operations.world_export_profiles(source_path)).await.map_err(|error|error.to_string())?
}

#[tauri::command]
fn application_world_export_status(task_id: String, cancel: bool, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    operations.world_export_status(&task_id,cancel)
}

#[tauri::command]
async fn application_convert_project_version(
    input: application::operations::ConvertProjectVersionInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.convert_project_version(input)).await
}

#[tauri::command]
async fn application_query_block_database(
    input: application::operations::BlockDatabaseQueryInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<serde_json::Value, String> {
    let operations=operations.inner().clone();
    run_application_operation(move ||operations.query_block_database(input)).await
}

#[tauri::command]
async fn application_replace_blocks(
    input: ReplaceBlocksInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ReplaceBlocksResult, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.replace_blocks(input)).await
}

struct StatisticsResponse(serde_json::Value);
impl Serialize for StatisticsResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let started = std::time::Instant::now();
        let result = self.0.serialize(serializer);
        if env::var_os("NOVA_STATS_TIMING").is_some() {
            eprintln!("[NOVA_STATS_TIMING] host_ipc_json_encode_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
        }
        result
    }
}

#[tauri::command]
async fn application_get_project_stats(
    input: ProjectStatsInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<StatisticsResponse, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.project_stats(input)).await.map(StatisticsResponse)
}

#[tauri::command]
async fn application_get_project_materials(
    input: ProjectMaterialsInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.project_materials(input)).await
}

#[tauri::command]
async fn application_stockpile_task(input: StockpileTaskInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.stockpile_task(input)).await
}
#[tauri::command]
async fn application_data_query(input: application::operations::DataQueryInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value,String> {
    let operations=operations.inner().clone(); run_application_operation(move||operations.data_query(input)).await
}
#[tauri::command]
async fn application_data_operation_status(operation_id:String,cancel:Option<bool>,operations:tauri::State<'_,ApplicationOperations>)->Result<serde_json::Value,String>{
    let operations=operations.inner().clone();run_application_operation(move||operations.data_operation_status(&operation_id,cancel.unwrap_or(false))).await
}
#[tauri::command]
async fn application_read_material_detail(input: MaterialDetailInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_material_detail(input)).await
}
#[tauri::command]
async fn application_stockpile_task_command(app: AppHandle, input: StockpileTaskCommandInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    let result = run_application_operation(move || operations.stockpile_task_command(input)).await?;
    app.emit("stockpile-task-changed", serde_json::json!({"taskId": result["task"]["taskId"]})).map_err(|error|error.to_string())?;
    Ok(result)
}

#[tauri::command]
async fn application_stockpile_task_set(app: AppHandle, input: StockpileTaskSetInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    let result = run_application_operation(move || operations.set_stockpile_quantity(input)).await?;
    app.emit("stockpile-task-changed", serde_json::json!({"taskId": result["task"]["taskId"]})).map_err(|error|error.to_string())?;
    Ok(result)
}
#[tauri::command]
async fn application_stockpile_task_reset(app: AppHandle, input: StockpileTaskResetInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    let result = run_application_operation(move || operations.reset_stockpile(input)).await?;
    app.emit("stockpile-task-changed", serde_json::json!({"taskId": result["task"]["taskId"]})).map_err(|error|error.to_string())?;
    Ok(result)
}
#[tauri::command]
async fn application_prepare_material_task_window(facade: tauri::State<'_, RenderFacade>, quick: tauri::State<'_, QuickPreviewHost>) -> Result<(), String> {
    facade.release_camera_input().await?;
    quick.release_camera_input()
}

#[tauri::command]
async fn application_generate_project(
    input: GenerateProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<GenerateProjectResult, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.generate_project(input)).await
}

#[tauri::command]
async fn application_runtime_health(
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<RuntimeHealth, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || Ok(operations.runtime_health())).await
}

#[tauri::command]
async fn application_read_embedded_preview(
    input: InspectProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<Option<EmbeddedPreview>, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_embedded_preview(input)).await
}

fn persist_rendered_preview(operations: &ApplicationOperations, source: &Path, frame: &Path, expected: &Path, mode: &str) -> Result<ProductPreviewImage, String> {
    if application::thumbnails::cache_path(source)? != expected { return Err("截图期间投影已改变，请重新加载".into()); }
    operations.edit_project_metadata(EditProjectMetadataInput {
        source_path: source.to_string_lossy().into(),
        patch: serde_json::json!({"preview_image_path": frame, "preview_image_mode": mode}), output_path: None,
    })?;
    let preview = operations.read_embedded_preview(InspectProjectInput { source_path: source.to_string_lossy().into() })?
        .ok_or("内嵌预览写入后未能重新读取")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(preview.data_url.split_once(',').ok_or("投影预览编码错误")?.1).map_err(|error| error.to_string())?;
    let temporary = application::thumbnails::temporary_preview_path();
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    let result = application::thumbnails::save(source, &temporary, &application::thumbnails::cache_path(source)?);
    let _ = std::fs::remove_file(&temporary);
    let _ = std::fs::remove_file(frame);
    result
}

#[tauri::command]
async fn render_preview_image(
    file_path: String,
    display_mode: String,
    cache_input: Option<String>,
    operation_id: Option<String>,
    facade: tauri::State<'_, RenderFacade>,
    host: tauri::State<'_, QuickPreviewHost>,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProductPreviewImage, String> {
    let _ = cache_input; // Capture always uses the selected live scene, never a new build.
    let source = std::fs::canonicalize(file_path).map_err(|error| error.to_string())?;
    let expected = application::thumbnails::cache_path(&source)?;
    let operations = operations.inner().clone();
    if display_mode == "fast_experimental" {
        let preview_id = operation_id.ok_or("请先启动该投影的快速渲染")?;
        let host = host.inner().clone();
        return tauri::async_runtime::spawn_blocking(move || {
            let view = host.snapshot()?.filter(|view| view.source_path == source && view.state == "ready"
                && preview_id == view.preview_id).ok_or("请先启动该投影的快速渲染")?;
            let (source, frame) = host.capture_frame(&view.preview_id)?;
            persist_rendered_preview(&operations, &source, &frame, &expected, "fast_experimental")
        }).await.map_err(|error| error.to_string())?;
    }
    let mode = match display_mode.as_str() { "standard" => RenderDisplayMode::Standard, "full" => RenderDisplayMode::Full, _ => return Err("未知截图模式".into()) };
    let id = operation_id.ok_or("请先启动对应渲染场景")?;
    let frame = facade.capture_frame(&OperationId(id), &source, mode).await?;
    tauri::async_runtime::spawn_blocking(move || persist_rendered_preview(&operations, &source, &frame, &expected, &display_mode))
        .await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn generate_preview_image(
    file_path: String,
    display_mode: String,
    facade: tauri::State<'_, RenderFacade>,
    window: tauri::WebviewWindow,
    host: tauri::State<'_, QuickPreviewHost>,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProductPreviewImage, String> {
    if display_mode == "fast_experimental" {

    let host = host.inner().clone();
    let operations = operations.inner().clone();
    let parent = platform::native_preview_host::host_window(&window);
    let graphics_backend = read_user_config_data(&current_user_config_dir()?).graphics_backend;
    return tauri::async_runtime::spawn_blocking(move || {
        let source = std::fs::canonicalize(file_path).map_err(|error| error.to_string())?;
        let cache = application::thumbnails::cache_path(&source)?;
        // Explicit generation takes a current real frame. Ordinary reads still
        // reuse embedded/cache previews, including legacy square images.
        if let Some(view) = host.snapshot()?.filter(|view| view.source_path == source && view.state == "ready") {
            let (source, frame) = host.capture_frame(&view.preview_id)?;
            return persist_rendered_preview(&operations, &source, &frame, &cache, "fast_experimental");
        }
        // Reuse QuickPreviewHost for an independent hidden WS_CHILD session; an
        // existing interactive Viewer and its camera/GPU lifetime stay untouched.
        let temporary = host.thumbnail_session();
        let view = temporary.start(StartQuickPreviewInput { source_path: source.clone(), preload: false, click_started_at_epoch_ms: None, graphics_backend }, parent?)?;
        let output = (|| {
            loop {
                let current = temporary.snapshot()?.ok_or("缩略图预览会话结束")?;
                if let Some(error) = current.error { return Err(error); }
                if current.state == "ready" { break; }
                if matches!(current.state, "failed" | "closed") { return Err("缩略图预览准备未完成".into()); }
                std::thread::sleep(Duration::from_millis(100));
            }
            let (source, frame) = temporary.capture_frame(&view.preview_id)?;
            persist_rendered_preview(&operations, &source, &frame, &cache, "fast_experimental")
        })();
        let _ = temporary.close(&view.preview_id);
        output
    }).await.map_err(|error| error.to_string())?;
    }

    let launch = prepare_viewer_launch_context(CompatibilityScope::Render, display_mode).await?;
    let mode = launch.display_mode();
    let preview_mode = mode.to_owned();
    let resolved_input = normalized_input_path(&file_path);
    let source = resolved_input.clone();
    let expected = application::thumbnails::cache_path(&source)?;
    let preview_dir = data_root()?.join("projection-library").join("previews");
    std::fs::create_dir_all(&preview_dir).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(resolved_input.to_string_lossy().as_bytes());
    hasher.update(mode.as_bytes());
    hasher.update(launch.compatibility_fingerprint().as_bytes());
    if let Ok(meta) = std::fs::metadata(&resolved_input) {
        hasher.update(meta.len().to_le_bytes());
        if let Ok(modified) = meta.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(duration.as_nanos().to_le_bytes());
            }
        }
    }
    let digest = hasher.finalize();
    let output_path = preview_dir.join(format!("{mode}_{:x}.png", digest));
    let launch_contract = launch.prepare_preview(resolved_input, output_path.clone(), None)?;
    let image = facade.run_viewer_preview(&launch, &launch_contract, output_path).await?;
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || persist_rendered_preview(&operations, &source, Path::new(&image.preview_path), &expected, &preview_mode))
        .await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn application_export_material_list(input: ExportMaterialListInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.export_material_list(input)).await
}

#[tauri::command]
async fn application_prepare_material_recipes(input: MaterialRecipeCacheInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.prepare_material_recipes(input)).await
}

#[tauri::command]
async fn get_user_config() -> Result<UserConfigInfo, String> {
    tauri::async_runtime::spawn_blocking(load_user_config_info).await.map_err(|error| error.to_string())?
}

fn load_user_config_info() -> Result<UserConfigInfo, String> {
    let started = std::time::Instant::now();
    let dir = current_user_config_dir()?;
    migrate_legacy_library_if_needed(&dir)?;
    let config = read_user_config_data(&dir);
    // Reading normalized preferences must not overwrite a newer concurrent save.
    // Defaults are returned in memory; only explicit changes persist configuration.
    eprintln!("[NOVA_LIFECYCLE] stage=config_read_migration elapsed_ms={}", started.elapsed().as_millis());
    Ok(UserConfigInfo {
        config_dir: dir.display().to_string(),
        default_config_dir: default_user_config_dir()?.display().to_string(),
        config,
    })
}

#[tauri::command]
async fn save_user_config(app: AppHandle, input: UserConfigInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<UserConfigInfo, String> {
    let write = operations.begin_write()?;
    run_application_operation(move || {
        let _write = write;
        save_user_config_inner(input)
    }).await.map(|info| {
        if !info.config.debug_mode {
            for label in ["enumerator", "ui-demo-window"] {
                if let Some(window) = app.get_webview_window(label) { let _ = window.close(); }
            }
        }
        i18n::set_locale(&info.config.ui_locale);
        i18n::update_window_titles(&app);
        let _ = app.emit("user-config-changed", &info);
        let _ = app.emit("nova-debug-mode-changed", serde_json::json!({"enabled": info.config.debug_mode}));
        info
    })
}

fn save_user_config_inner(input: UserConfigInput) -> Result<UserConfigInfo, String> {
    let _serial = CONFIG_WRITE_LOCK.lock().map_err(|_| "配置保存锁异常")?;
    let dir = current_user_config_dir()?;
    let mut config = read_user_config_data(&dir);
    if let Some(language) = input.ui_language { config.ui_language = normalize_ui_language(&language); }
    if let Some(locale) = input.ui_locale { config.ui_locale = normalize_ui_locale(&locale); }
    if config.ui_language != "system" { config.ui_locale = config.ui_language.clone(); }
    if let Some(backend) = input.graphics_backend { config.graphics_backend = backend; }
    if let Some(theme) = input.theme {
        config.theme = normalize_theme(&theme);
    }
    if let Some(mode) = input.color_mode {
        config.color_mode = normalize_color_mode(&mode);
    }
    if let Some(mode) = input.render_display_mode {
        config.render_display_mode = normalize_mode_string(&mode);
    }
    if let Some(mode) = input.preview_mode {
        config.preview_mode = normalize_mode_string(&mode);
    }
    if let Some(behavior) = input.material_list_window_behavior {
        config.material_list_window_behavior = normalize_material_list_window_behavior(&behavior);
    }
    if let Some(rules) = input.statistics_enumerator_info_rules {
        config.statistics_enumerator_info_rules = rules;
    }
    if let Some(enabled) = input.debug_mode { config.debug_mode = enabled; }
    if let Some(show) = input.show_ui_test_page {
        config.show_ui_test_page = show;
    }
    if let Some(count) = input.local_library_tail_path_count {
        config.local_library_tail_path_count = normalize_local_library_tail_path_count(count);
    }
    if let Some(compatibility) = input.v5_compatibility {
        config.v5_compatibility.apply_input(compatibility)?;
        let workspace_root = get_root();
        if config.v5_compatibility.requires_full_mode() {
            config.render_display_mode = "full".to_owned();
        }
        // Saving a small UI preference must not recursively fingerprint every
        // configured pack in the Nova process. The render service computes
        // the authoritative content identities when a Build starts.
        ViewerCompatibilityInputs::resolve(
            &config.v5_compatibility,
            &workspace_root,
            CompatibilityScope::Render,
        )?;
    }
    if let Some(viewer_controls) = input.viewer_controls {
        config.viewer_controls = viewer_controls;
    }
    write_user_config_data(&dir, &config)?;
    Ok(UserConfigInfo {
        config_dir: dir.display().to_string(),
        default_config_dir: default_user_config_dir()?.display().to_string(),
        config,
    })
}

#[tauri::command]
fn choose_user_config_dir(app: AppHandle) -> Result<Option<String>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title(i18n::text("dialog.configDirectory"))
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
fn open_user_config_dir() -> Result<(), String> {
    let dir = current_user_config_dir()?;
    local_path_open_command(&dir)?.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if !dst_path.exists() {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn set_user_config_dir(path: String, migrate: bool) -> Result<UserConfigInfo, String> {
    let target = PathBuf::from(path);
    if !target.is_absolute() {
        return Err("User config directory must be an absolute path.".to_string());
    }
    let current = current_user_config_dir()?;
    ensure_user_config_layout(&target)?;
    if migrate {
        copy_dir_recursive(&current, &target)?;
    }
    let pointer = config_dir_pointer_path()?;
    std::fs::write(
        pointer,
        serde_json::json!({ "config_dir": target.display().to_string() }).to_string(),
    )
    .map_err(|e| e.to_string())?;
    initialize_flake_user_resources()?;
    load_user_config_info()
}

#[tauri::command]
fn reset_user_config_dir(migrate: bool) -> Result<UserConfigInfo, String> {
    let current = current_user_config_dir()?;
    let default_dir = default_user_config_dir()?;
    ensure_user_config_layout(&default_dir)?;
    if migrate && current != default_dir {
        copy_dir_recursive(&current, &default_dir)?;
    }
    let pointer = config_dir_pointer_path()?;
    if pointer.exists() {
        std::fs::remove_file(pointer).map_err(|e| e.to_string())?;
    }
    initialize_flake_user_resources()?;
    load_user_config_info()
}

#[tauri::command]
fn read_user_config_file(relative_path: String) -> Result<String, String> {
    let dir = current_user_config_dir()?;
    migrate_legacy_library_if_needed(&dir)?;
    let path = dir.join(user_runtime_relative_path(&relative_path)?);
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn write_user_config_file(relative_path: String, content: String, operations: tauri::State<'_, ApplicationOperations>) -> Result<(), String> {
    let write = operations.begin_write()?;
    run_application_operation(move || {
        let _write = write;
        let _serial = CONFIG_WRITE_LOCK.lock().map_err(|_| "配置保存锁异常")?;
        let dir = current_user_config_dir()?;
        let path = dir.join(user_runtime_relative_path(&relative_path)?);
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        std::fs::write(path, content).map_err(|e| e.to_string())
    }).await
}

#[tauri::command]
fn get_user_config_file_path(relative_path: String) -> Result<String, String> {
    let dir = current_user_config_dir()?;
    let path = dir.join(user_runtime_relative_path(&relative_path)?);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(path.display().to_string())
}

#[tauri::command]
fn read_file_string(path: String) -> Result<String, String> {
    std::fs::read_to_string(get_root().join(path)).map_err(|e| e.to_string())
}

#[tauri::command]
async fn write_file_string(path: String, content: String, operations: tauri::State<'_, ApplicationOperations>) -> Result<(), String> {
    let write = operations.begin_write()?;
    run_application_operation(move || {
        let _write = write;
        let full_path = get_root().join(path);
        if let Some(p) = full_path.parent() { std::fs::create_dir_all(p).map_err(|e| e.to_string())?; }
        std::fs::write(&full_path, content).map_err(|e| e.to_string())
    }).await
}

#[tauri::command]
async fn write_text_file_absolute(path: String, content: String, operations: tauri::State<'_, ApplicationOperations>) -> Result<(), String> {
    let write = operations.begin_write()?;
    run_application_operation(move || {
        let _write = write;
        let full_path = PathBuf::from(path);
        if !full_path.is_absolute() { return Err("output path must be absolute".to_string()); }
        if let Some(parent) = full_path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        std::fs::write(&full_path, content).map_err(|e| e.to_string())
    }).await
}

#[tauri::command]
fn check_file_exists(path: String) -> bool {
    get_root().join(path).is_file()
}

fn collect_litematic_files_in_directory(
    current_dir: &Path,
    recursive: bool,
    files: &mut Vec<String>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(current_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_litematic_files_in_directory(&path, true, files)?;
            }
            continue;
        }
        let has_litematic_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("litematic"))
            .unwrap_or(false);
        if has_litematic_ext {
            files.push(path.display().to_string());
        }
    }
    Ok(())
}

fn metadata_modified_ms(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn directory_entry_info(path: PathBuf) -> Result<DirectoryEntryInfo, String> {
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    Ok(DirectoryEntryInfo {
        path: path.display().to_string(),
        name,
        is_dir: metadata.is_dir(),
        is_file: metadata.is_file(),
        file_size: if metadata.is_file() {
            metadata.len()
        } else {
            0
        },
        mtime_ms: metadata_modified_ms(&metadata),
        extension,
    })
}

fn collect_litematic_file_entries_in_directory(
    current_dir: &Path,
    recursive: bool,
    files: &mut Vec<DirectoryEntryInfo>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(current_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_litematic_file_entries_in_directory(&path, true, files)?;
            }
            continue;
        }
        let has_litematic_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("litematic"))
            .unwrap_or(false);
        if has_litematic_ext {
            files.push(directory_entry_info(path)?);
        }
    }
    Ok(())
}

fn resolve_input_path(path: &str) -> PathBuf {
    let input = PathBuf::from(path);
    if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    }
}

#[tauri::command]
fn list_litematic_files_in_directory(path: String, recursive: bool) -> Result<Vec<String>, String> {
    let full_path = resolve_input_path(&path);
    if !full_path.exists() {
        return Err(format!("directory not found: {}", full_path.display()));
    }
    if !full_path.is_dir() {
        return Err(format!("path is not a directory: {}", full_path.display()));
    }
    let mut files = Vec::new();
    collect_litematic_files_in_directory(&full_path, recursive, &mut files)?;
    files.sort_unstable();
    Ok(files)
}

#[tauri::command]
fn list_directory_entries(path: String) -> Result<Vec<DirectoryEntryInfo>, String> {
    let full_path = resolve_input_path(&path);
    if !full_path.exists() {
        return Err(format!("directory not found: {}", full_path.display()));
    }
    if !full_path.is_dir() {
        return Err(format!("path is not a directory: {}", full_path.display()));
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&full_path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        entries.push(directory_entry_info(entry.path())?);
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
fn list_litematic_file_entries_in_directory(
    path: String,
    recursive: bool,
) -> Result<Vec<DirectoryEntryInfo>, String> {
    let full_path = resolve_input_path(&path);
    if !full_path.exists() {
        return Err(format!("directory not found: {}", full_path.display()));
    }
    if !full_path.is_dir() {
        return Err(format!("path is not a directory: {}", full_path.display()));
    }
    let mut files = Vec::new();
    collect_litematic_file_entries_in_directory(&full_path, recursive, &mut files)?;
    files.sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
    Ok(files)
}

#[tauri::command]
fn get_workspace_root() -> String {
    get_root().display().to_string()
}

#[tauri::command]
fn get_path_info(path: String) -> PathInfo {
    let input = PathBuf::from(&path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };
    let parent = full_path.parent().map(PathBuf::from).unwrap_or_default();
    let metadata = std::fs::metadata(&full_path).ok();
    let parent_exists = parent.is_dir();
    let has_litematic_ext = full_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("litematic"))
        .unwrap_or(false);

    PathInfo {
        raw: path,
        normalized: full_path.display().to_string(),
        parent_dir: parent.display().to_string(),
        parent_exists,
        exists: metadata.is_some(),
        is_dir: metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
        is_file: metadata.as_ref().map(|m| m.is_file()).unwrap_or(false),
        is_absolute: full_path.is_absolute(),
        has_litematic_ext,
    }
}

#[tauri::command]
fn read_image_base64(path: String) -> Result<String, String> {
    let input = PathBuf::from(path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };
    if let Ok(bytes) = std::fs::read(full_path) {
        Ok(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        ))
    } else {
        Err("Not found".into())
    }
}

#[tauri::command]
fn open_file_parent_dir(file_path: String) -> Result<(), String> {
    let input = PathBuf::from(&file_path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };
    let parent = full_path
        .parent()
        .ok_or_else(|| "File path has no parent directory.".to_string())?
        .to_path_buf();
    let exists = full_path.is_file();
    if !exists {
        let message = format!(
            "[LBA_OPEN_FOLDER] file_path={} parent_dir={} exists=false result=error error=file_not_found",
            full_path.display(),
            parent.display()
        );
        eprintln!("{message}");
        return Err(message);
    }
    let result = local_path_open_command(&parent)?.spawn();
    match result {
        Ok(_) => {
            eprintln!(
                "[LBA_OPEN_FOLDER] file_path={} parent_dir={} exists=true result=ok error=",
                full_path.display(),
                parent.display()
            );
            Ok(())
        }
        Err(error) => {
            let message = format!(
                "[LBA_OPEN_FOLDER] file_path={} parent_dir={} exists=true result=error error={}",
                full_path.display(),
                parent.display(),
                error
            );
            eprintln!("{message}");
            Err(message)
        }
    }
}

#[tauri::command]
fn copy_file_to_directory(
    source_path: String,
    target_directory: String,
    target_file_name: String,
    overwrite: bool,
) -> Result<CopyFileToDirectoryOutput, String> {
    let source_input = PathBuf::from(&source_path);
    let source_full_path = if source_input.is_absolute() {
        source_input
    } else {
        get_root().join(source_input)
    };
    if !source_full_path.is_file() {
        return Err(format!(
            "source file not found: {}",
            source_full_path.display()
        ));
    }

    let trimmed_file_name = target_file_name.trim();
    if trimmed_file_name.is_empty() {
        return Err("target file name must not be empty".to_string());
    }
    if trimmed_file_name.contains('/') || trimmed_file_name.contains('\\') {
        return Err("target file name must not contain path separators".to_string());
    }

    let target_input = PathBuf::from(&target_directory);
    let target_dir_path = if target_input.is_absolute() {
        target_input
    } else {
        get_root().join(target_input)
    };
    if target_dir_path.exists() && !target_dir_path.is_dir() {
        return Err(format!(
            "target path is not a directory: {}",
            target_dir_path.display()
        ));
    }
    std::fs::create_dir_all(&target_dir_path).map_err(|e| e.to_string())?;

    let target_full_path = target_dir_path.join(trimmed_file_name);
    if target_full_path == source_full_path {
        return Err("source and target path are identical".to_string());
    }

    let overwritten = target_full_path.exists();
    if overwritten && !overwrite {
        return Err(format!(
            "target file already exists: {}",
            target_full_path.display()
        ));
    }
    if overwritten && !target_full_path.is_file() {
        return Err(format!(
            "target path is not a file: {}",
            target_full_path.display()
        ));
    }

    let bytes_copied =
        std::fs::copy(&source_full_path, &target_full_path).map_err(|e| e.to_string())?;
    Ok(CopyFileToDirectoryOutput {
        target_path: target_full_path.display().to_string(),
        overwritten,
        bytes_copied,
    })
}

// Local paths go to the existing native Explorer route, never the shell URL regex.
fn local_path_open_command(path: &Path) -> Result<Command, String> {
    if !path.exists() { return Err("文件或文件夹不存在，请刷新后重试".into()); }
    #[cfg(windows)]
    let mut command = Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    let raw = path.to_string_lossy();
    let display = if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") { format!(r"\\{unc}") }
        else { raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_owned() };
    command.arg(display);
    Ok(command)
}

#[cfg(test)]
mod local_path_open_tests {
    use super::*;
    #[test]
    fn config_layout_creates_presets_and_native_command_preserves_unicode_spaces_and_long_paths() {
        let root = std::env::temp_dir().join(format!("nova-local-path-{}",std::process::id())).join("中文 配置").join("x".repeat(180));
        ensure_user_config_layout(&root).unwrap();
        let presets=root.join("replace/presets");
        assert!(presets.is_dir());
        let command=local_path_open_command(&presets).unwrap();
        assert_eq!(command.get_args().count(),1);
        assert_eq!(command.get_args().next().unwrap().to_string_lossy(),presets.to_string_lossy());
        #[cfg(windows)] {
            let canonical=std::fs::canonicalize(&presets).unwrap();
            let command=local_path_open_command(&canonical).unwrap();
            assert_eq!(command.get_program(),"explorer");
            assert!(!command.get_args().next().unwrap().to_string_lossy().starts_with(r"\\?\"));
        }
        assert!(local_path_open_command(&root.join("missing")).is_err());
        let cleanup=root.ancestors().nth(2).unwrap();
        assert_eq!(cleanup,std::env::temp_dir().join(format!("nova-local-path-{}",std::process::id())));
        std::fs::remove_dir_all(cleanup).unwrap();
    }
}

#[tauri::command]
fn open_workspace_path(path: String) -> Result<(), String> {
    let input = PathBuf::from(&path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };
    local_path_open_command(&full_path)?.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn cleanup_local_temp_files() -> Result<String, String> {
    let mut removed = 0usize;
    for dir in [
        data_root()?.join("cache").join("render"),
        data_root()?.join("tmp").join("render"),
    ] {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
            } else {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
            removed += 1;
        }
    }
    Ok(format!(
        "Removed {removed} desktop-nova render temp entries."
    ))
}

#[derive(Deserialize)]
struct RedenSizesInput {
    #[serde(rename = "xSize")]
    x_size: Option<u32>,
    #[serde(rename = "ySize")]
    y_size: Option<u32>,
    #[serde(rename = "zSize")]
    z_size: Option<u32>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RedenDownloadOutput {
    Downloaded { path: String, file_name: String, bytes: usize },
    ExternalLink { url: String, provider: &'static str },
}

fn reden_external_drive(url: &reqwest::Url) -> Option<&'static str> {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() { return None; }
    match url.host_str()? {
        "pan.quark.cn" => Some("quark"),
        "pan.baidu.com" | "yun.baidu.com" => Some("baidu"),
        _ => None,
    }
}

fn reden_looks_html(content_type: &str, bytes: &[u8]) -> bool {
    let trimmed = bytes.iter().position(|byte| !byte.is_ascii_whitespace()).map(|at| &bytes[at..]).unwrap_or(bytes);
    content_type.to_ascii_lowercase().contains("text/html")
        || trimmed.get(..9).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"<!doctype"))
        || trimmed.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"<html"))
}

fn reden_save_projection(
    directory: &Path, file_name: &str, bytes: &[u8],
    validate: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<RedenDownloadOutput, String> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = directory.join(format!(".nova-reden-{}-{sequence}.litematic", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        drop(file);
        // Reuse the existing core NBT/projection parser, not extension, MIME, or magic-only checks.
        validate(&temporary).map_err(|error| format!("下载内容不是有效的 NBT 投影，未保存或入库：{error}"))?;
        let original = Path::new(file_name);
        let stem = original.file_stem().and_then(|name| name.to_str()).unwrap_or("reden-download");
        let mut destination = directory.join(file_name);
        let mut suffix = 1;
        while destination.exists() {
            destination = directory.join(format!("{stem}-{suffix}.litematic"));
            suffix += 1;
        }
        std::fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
        Ok(RedenDownloadOutput::Downloaded {
            path: destination.display().to_string(),
            file_name: destination.file_name().and_then(|name| name.to_str()).unwrap_or(file_name).to_string(),
            bytes: bytes.len(),
        })
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result
}

fn reden_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

fn reden_get_json(url: reqwest::Url) -> Result<serde_json::Value, String> {
    crate::online_sources::get_json(url, None)
}

fn reden_download_dir() -> Result<PathBuf, String> {
    let dir = data_root()?.join("reden").join("downloads");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn sanitize_reden_file_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = sanitized.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "reden-download.litematic".to_string()
    } else if trimmed.to_lowercase().ends_with(".litematic") {
        trimmed
    } else {
        format!("{trimmed}.litematic")
    }
}

fn file_name_from_headers_or_url(
    headers: &reqwest::header::HeaderMap,
    url: &reqwest::Url,
    fallback: &str,
) -> String {
    if let Some(value) = headers.get(reqwest::header::CONTENT_DISPOSITION) {
        if let Ok(text) = value.to_str() {
            for part in text.split(';') {
                let part = part.trim();
                if let Some(name) = part.strip_prefix("filename=") {
                    return sanitize_reden_file_name(name.trim_matches('"'));
                }
            }
        }
    }
    // MCimg's official resource endpoint carries the real attachment name in the query.
    if url.host_str() == Some("www.mcimg.com") && url.path().ends_with("/resource") {
        if let Some((_, name)) = url.query_pairs().find(|(key, _)| key == "name") {
            return sanitize_reden_file_name(&name);
        }
    }
    let from_url = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .unwrap_or(fallback);
    sanitize_reden_file_name(from_url)
}

fn reden_fetch_download(
    start_url: reqwest::Url,
    fallback_name: &str,
    operations: &ApplicationOperations,
    request_id: Option<&str>,
) -> Result<RedenDownloadOutput, String> {
    let client = reden_client()?;
    let mut url = start_url;
    for _ in 0..6 {
        if let Some(provider) = reden_external_drive(&url) {
            return Ok(RedenDownloadOutput::ExternalLink { url: url.to_string(), provider });
        }
        let response = crate::online_sources::send_request(client
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, "Litematica-Nova desktop/0.1")
            .header(reqwest::header::REFERER, "https://redenmc.com/")
            .header(reqwest::header::ORIGIN, "https://redenmc.com"), request_id)?;
        let status = response.status();
        if status.is_redirection() {
            let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
                return Err(format!(
                    "RedenMC download redirected without Location: status={status}"
                ));
            };
            let location = location.to_str().map_err(|e| e.to_string())?;
            url = url.join(location).map_err(|e| e.to_string())?;
            continue;
        }
        let final_url = url.clone();
        let headers = response.headers().clone();
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = response.bytes().map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "RedenMC download failed: status={} content-type={} bytes={}",
                status,
                content_type,
                bytes.len()
            ));
        }
        if reden_looks_html(&content_type, &bytes) {
            return Err(format!(
                "RedenMC returned an HTML/external-drive page instead of .litematic: content-type={} bytes={}",
                content_type,
                bytes.len()
            ));
        }
        if bytes.is_empty() {
            return Err("RedenMC download returned an empty file.".to_string());
        }
        let file_name = file_name_from_headers_or_url(&headers, &final_url, fallback_name);
        return reden_save_projection(&reden_download_dir()?, &file_name, &bytes, |temporary| {
            operations.inspect_project(InspectProjectInput { source_path: temporary.display().to_string() }).map(|_| ())
        });
    }
    Err("RedenMC download redirected too many times.".to_string())
}

#[tauri::command]
async fn reden_search_litematica(query: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = reqwest::Url::parse_with_params(
            "https://redenmc.com/api/mc-services/litematica/search",
            &[("q", query.as_str())],
        )
        .map_err(|e| e.to_string())?;
        reden_get_json(url)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reden_machine_detail(machine_id: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = reqwest::Url::parse(&format!(
            "https://redenmc.com/api/mc-services/yisibite/{}/info/en",
            machine_id
        ))
        .map_err(|e| e.to_string())?;
        reden_get_json(url)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reden_download_attachment(
    machine_id: String,
    attachment_index: u32,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<RedenDownloadOutput, String> {
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if attachment_index == 0 {
            return Err("RedenMC attachment index is 1-based; index 0 is invalid.".to_string());
        }
        let url = reqwest::Url::parse(&format!(
            "https://redenmc.com/api/mc-services/yisibite/{}/download/{}",
            machine_id, attachment_index
        ))
        .map_err(|e| e.to_string())?;
        reden_fetch_download(url, &format!("{machine_id}-{attachment_index}.litematic"), &operations, None)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reden_download_parametric(
    machine_id: String,
    sizes: RedenSizesInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<RedenDownloadOutput, String> {
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut url = reqwest::Url::parse(&format!(
            "https://redenmc.com/api/mc-services/yisibite/{}",
            machine_id
        ))
        .map_err(|e| e.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(value) = sizes.x_size {
                pairs.append_pair("xSize", &value.to_string());
            }
            if let Some(value) = sizes.y_size {
                pairs.append_pair("ySize", &value.to_string());
            }
            if let Some(value) = sizes.z_size {
                pairs.append_pair("zSize", &value.to_string());
            }
        }
        reden_fetch_download(url, &format!("{machine_id}.litematic"), &operations, None)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod reden_download_tests {
    use super::*;

    #[test]
    fn drive_links_are_explicit_and_never_match_lookalikes() {
        for (value, expected) in [("https://pan.quark.cn/s/abc", Some("quark")), ("https://pan.baidu.com/s/abc", Some("baidu")), ("https://yun.baidu.com/s/abc", Some("baidu")), ("https://pan.baidu.com.evil.test/a", None), ("http://pan.quark.cn/a", None), ("https://user:pass@pan.baidu.com/a", None)] {
            assert_eq!(reden_external_drive(&reqwest::Url::parse(value).unwrap()), expected);
        }
    }

    #[test]
    fn html_cannot_be_hidden_by_whitespace_case_or_mime() {
        assert!(reden_looks_html("application/octet-stream", b" \r\n<HTML><body>login</body>"));
        assert!(reden_looks_html("application/octet-stream", b"<!DoCtYpE html>"));
        assert!(reden_looks_html("TEXT/HTML; charset=utf8", b"page"));
        assert!(!reden_looks_html("application/octet-stream", &[0x1f, 0x8b, 8, 0]));
    }

    #[test]
    fn invalid_projection_is_removed_before_final_file_or_import() {
        let directory = std::env::temp_dir().join(format!("nova-reden-validation-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let result = reden_save_projection(&directory, "not-a-projection.litematic", b"not NBT", |temporary| {
            assert_eq!(std::fs::read(temporary).unwrap(), b"not NBT");
            Err("NBT decode rejected".into())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
        std::fs::remove_dir(&directory).unwrap();
    }
}

#[tauri::command]
fn ai_get_config() -> Result<AiPublicConfig, String> {
    Ok(public_ai_config(read_ai_config(), "loaded"))
}

#[tauri::command]
fn ai_save_config(input: AiSaveConfigInput) -> Result<AiPublicConfig, String> {
    let mut current = read_ai_config();
    current.provider = match input.provider.as_str() {
        "openai_compatible" => "openai_compatible".to_string(),
        "gemini_compatible" => "gemini_compatible".to_string(),
        _ => "mock".to_string(),
    };
    current.base_url = input.base_url.trim().trim_end_matches('/').to_string();
    current.model = input.model.trim().to_string();
    if let Some(api_key) = input.api_key {
        if !api_key.trim().is_empty() {
            current.api_key = Some(api_key.trim().to_string());
        }
    }
    write_ai_config(&current)?;
    Ok(public_ai_config(current, "saved"))
}

#[tauri::command]
fn ai_clear_key() -> Result<AiPublicConfig, String> {
    let mut current = read_ai_config();
    current.api_key = None;
    write_ai_config(&current)?;
    Ok(public_ai_config(current, "cleared"))
}

#[tauri::command]
fn ai_test_connection() -> Result<AiTestResult, String> {
    let config = read_ai_config();
    match config.provider.as_str() {
        "mock" => {
            if config.base_url.trim().starts_with("http")
                && config.api_key.as_ref().is_some_and(|key| !key.is_empty())
            {
                test_openai_models_endpoint(&config, "Mock provider /models probe")
            } else {
                Ok(AiTestResult {
                    ok: true,
                    message: "Mock provider OK.".to_string(),
                })
            }
        }
        "gemini_compatible" => Ok(AiTestResult {
            ok: false,
            message: "Gemini Compatible is reserved but not implemented yet.".to_string(),
        }),
        "openai_compatible" => test_openai_models_endpoint(&config, "OpenAI Compatible"),
        _ => Ok(AiTestResult {
            ok: false,
            message: "Unknown provider.".to_string(),
        }),
    }
}

fn test_openai_models_endpoint(
    config: &AiStoredConfig,
    label: &str,
) -> Result<AiTestResult, String> {
    if config.base_url.trim().is_empty() {
        return Ok(AiTestResult {
            ok: false,
            message: "Base URL is empty.".to_string(),
        });
    }
    let Some(api_key) = config.api_key.as_ref().filter(|key| !key.is_empty()) else {
        return Ok(AiTestResult {
            ok: false,
            message: "API Key is not saved.".to_string(),
        });
    };
    let url = format!("{}/models", config.base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Ok(AiTestResult {
            ok: false,
            message: format!(
                "{label} connection failed. GET /models status={status}. body={}",
                body.chars().take(600).collect::<String>()
            ),
        });
    }

    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(err) => {
            return Ok(AiTestResult {
                ok: false,
                message: format!(
                    "{label} returned non-JSON from GET /models. status={status}. error={err}"
                ),
            });
        }
    };
    let Some(models) = parsed.get("data").and_then(|data| data.as_array()) else {
        return Ok(AiTestResult {
            ok: false,
            message: format!("{label} returned JSON but no data[] model list. status={status}"),
        });
    };
    let configured_model_found = models
        .iter()
        .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
        .any(|id| id == config.model);

    Ok(AiTestResult {
        ok: true,
        message: format!(
            "{label} connection OK. GET /models status={status}. models={}. configured_model_found={configured_model_found}",
            models.len()
        ),
    })
}

#[tauri::command]
fn ai_chat_completion(input: AiChatCompletionInput) -> Result<AiChatCompletionOutput, String> {
    let config = read_ai_config();
    match config.provider.as_str() {
        "mock" => Ok(AiChatCompletionOutput {
            provider: "mock".to_string(),
            model: config.model,
            content: mock_ai_plan_response(),
        }),
        "gemini_compatible" => {
            Err("Gemini Compatible is reserved but not implemented yet.".to_string())
        }
        "openai_compatible" => openai_chat_completion(&config, input.messages),
        _ => Err("Unknown AI provider.".to_string()),
    }
}

fn openai_chat_completion(
    config: &AiStoredConfig,
    messages: Vec<AiChatMessage>,
) -> Result<AiChatCompletionOutput, String> {
    if config.base_url.trim().is_empty() {
        return Err("Base URL is empty.".to_string());
    }
    if config.model.trim().is_empty() {
        return Err("Model is empty.".to_string());
    }
    let Some(api_key) = config.api_key.as_ref().filter(|key| !key.is_empty()) else {
        return Err("API Key is not saved.".to_string());
    };
    if messages.is_empty() {
        return Err("messages is empty.".to_string());
    }
    let sanitized_messages = messages
        .into_iter()
        .map(|message| {
            let role = match message.role.as_str() {
                "system" | "assistant" | "user" => message.role,
                _ => "user".to_string(),
            };
            serde_json::json!({
                "role": role,
                "content": message.content,
            })
        })
        .collect::<Vec<_>>();
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": config.model,
        "messages": sanitized_messages,
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "POST /chat/completions failed. status={status}. body={}",
            text.chars().take(800).collect::<String>()
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("POST /chat/completions returned non-JSON. error={e}"))?;
    let content = value
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .ok_or_else(|| {
            "POST /chat/completions JSON missing choices[0].message.content".to_string()
        })?;
    Ok(AiChatCompletionOutput {
        provider: config.provider.clone(),
        model: config.model.clone(),
        content: content.to_string(),
    })
}

fn mock_ai_plan_response() -> String {
    r#"{
  "version": 1,
  "metadata": {
    "name": "Mock API Plan",
    "author": "Litematica-Nova",
    "description": "Mock provider returned a local projection plan."
  },
  "minecraft_data_version": 3953,
  "regions": [
    {
      "name": "main",
      "origin": [0, 0, 0],
      "size": [24, 8, 24],
      "operations": [
        {
          "type": "floor",
          "from": [0, 0, 0],
          "to": [23, 0, 23],
          "block": {
            "name": "minecraft:stone_bricks",
            "properties": {}
          }
        },
        {
          "type": "outline_box",
          "from": [2, 1, 2],
          "to": [21, 5, 21],
          "block": {
            "name": "minecraft:oak_planks",
            "properties": {}
          }
        }
      ]
    }
  ]
}"#
    .to_string()
}

fn query_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[tauri::command]
async fn open_material_list_window(
    app: AppHandle,
    active_file: Option<String>,
    view_mode: Option<String>,
) -> Result<(), String> {
    application_prepare_material_task_window(app.state::<RenderFacade>(), app.state::<QuickPreviewHost>()).await?;
    const LABEL: &str = "material-list";
    let mode = if view_mode.as_deref() == Some("export") { "export" } else { "details" };
    i18n::set_material_export(mode == "export");

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        let _ = window.set_title(i18n::text(if mode == "export" { "window.materialExport" } else { "window.materialList" }));
        app.emit_to(LABEL, "material-list-open-file", active_file)
            .map_err(|err| err.to_string())?;
        app.emit_to(LABEL, "material-list-open-mode", mode).map_err(|err| err.to_string())?;
        return Ok(());
    }

    let url = match active_file
        .as_deref()
        .filter(|file| !file.trim().is_empty())
    {
        Some(file) => format!("material_list.html?file={}&viewMode={mode}", query_encode(file)),
        None => format!("material_list.html?viewMode={mode}"),
    };

    tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::App(url.into()))
        .title(i18n::text(if mode == "export" { "window.materialExport" } else { "window.materialList" }))
        .inner_size(1080.0, 760.0)
        .min_inner_size(560.0, 420.0)
        .build()
        .map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
async fn open_enumerator_window(app: AppHandle, active_file: Option<String>) -> Result<(), String> {
    if !read_user_config_data(&current_user_config_dir()?).debug_mode { return Err("请先启用调试模式".into()); }
    const LABEL: &str = "enumerator";
    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        app.emit_to(LABEL, "enumerator-open-file", active_file).map_err(|error| error.to_string())?;
        return Ok(());
    }
    let url = active_file.filter(|file| !file.trim().is_empty())
        .map(|file| format!("enumerator.html?file={}", query_encode(&file))).unwrap_or_else(|| "enumerator.html".into());
    tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::App(url.into()))
        .title(i18n::text("window.enumerator")).inner_size(840.0, 580.0).min_inner_size(600.0, 400.0).build().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn open_asset_manager_window(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "asset-manager";
    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::App("asset_manager.html".into()))
        .title(i18n::text("window.assets")).inner_size(980.0, 680.0).min_inner_size(720.0, 480.0).build().map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct IrisWindowPlacement { x: i32, y: i32, width: f64, height: f64 }
fn iris_window_placement_path() -> Result<PathBuf, String> { Ok(current_user_config_dir()?.join("iris-window.json")) }
fn save_iris_window_placement(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.is_minimized().map_err(|error| error.to_string())? { return Ok(()); }
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let size = window.inner_size().map_err(|error| error.to_string())?.to_logical::<f64>(scale);
    let placement = IrisWindowPlacement { x: position.x, y: position.y, width: size.width, height: size.height };
    std::fs::write(iris_window_placement_path()?, serde_json::to_vec(&placement).map_err(|error| error.to_string())?).map_err(|error| error.to_string())
}
fn restore_iris_window_placement(window: &tauri::WebviewWindow, saved: Option<IrisWindowPlacement>) -> Result<(), String> {
    let monitors = window.available_monitors().map_err(|error| error.to_string())?;
    let current = window.outer_position().map_err(|error| error.to_string())?;
    let position = saved.map(|p| tauri::PhysicalPosition::new(p.x, p.y)).unwrap_or(current);
    let monitor = monitors.iter().find(|monitor| {
        let area = monitor.work_area(); let x = i64::from(position.x); let y = i64::from(position.y);
        x >= i64::from(area.position.x) && y >= i64::from(area.position.y)
            && x < i64::from(area.position.x) + i64::from(area.size.width)
            && y < i64::from(area.position.y) + i64::from(area.size.height)
    }).or_else(|| monitors.first());
    let Some(monitor) = monitor else { return Ok(()); };
    let area = monitor.work_area(); let scale = monitor.scale_factor();
    let current_size = window.inner_size().map_err(|error| error.to_string())?.to_logical::<f64>(window.scale_factor().map_err(|error| error.to_string())?);
    let width = saved.map(|p| p.width).unwrap_or(current_size.width).clamp(360.0, (area.size.width as f64 / scale).max(360.0));
    let height = saved.map(|p| p.height).unwrap_or(current_size.height).clamp(300.0, (area.size.height as f64 / scale - 48.0).max(300.0));
    window.set_size(tauri::LogicalSize::new(width, height)).map_err(|error| error.to_string())?;
    let outer = window.outer_size().map_err(|error| error.to_string())?;
    let max_x = (i64::from(area.position.x) + i64::from(area.size.width.saturating_sub(outer.width))) as i32;
    let max_y = (i64::from(area.position.y) + i64::from(area.size.height.saturating_sub(outer.height))) as i32;
    window.set_position(tauri::PhysicalPosition::new(position.x.clamp(area.position.x, max_x), position.y.clamp(area.position.y, max_y))).map_err(|error| error.to_string())
}
#[tauri::command]
async fn open_iris_options_window(app: AppHandle, root: Option<String>, pack: Option<String>, facade: tauri::State<'_, RenderFacade>) -> Result<(), String> {
    const LABEL: &str = "iris-options";
    let config = read_user_config_data(&current_user_config_dir()?);
    let root = root.unwrap_or(config.v5_compatibility.shaderpack_root);
    let pack = pack.unwrap_or(config.v5_compatibility.shaderpack_name);
    if root.trim().is_empty() || pack.trim().is_empty() { return Err("请先选择光影包 / Select a shader pack first".into()); }
    facade.release_camera_input().await?;
    if let Some(window) = app.get_webview_window(LABEL) {
        restore_iris_window_placement(&window, None)?;
        app.emit_to(LABEL, "iris-options-select-pack", serde_json::json!({ "root": root, "pack": pack })).map_err(|error| error.to_string())?;
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let parent = app.get_webview_window("main").ok_or("Main window is unavailable")?;
    let window = tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::App(format!("iris_options.html?root={}&pack={}", query_encode(&root), query_encode(&pack)).into()))
        .title("Iris").inner_size(480.0, 640.0).min_inner_size(360.0, 300.0).resizable(true).visible(false)
        .parent(&parent).map_err(|error| error.to_string())?.build().map_err(|error| error.to_string())?;
    let saved = iris_window_placement_path().ok().and_then(|path| std::fs::read(path).ok()).and_then(|bytes| serde_json::from_slice(&bytes).ok());
    restore_iris_window_placement(&window, saved)?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}
#[tauri::command]
async fn close_iris_options_window(app: AppHandle, facade: tauri::State<'_, RenderFacade>) -> Result<(), String> {
    facade.release_camera_input().await?;
    if let Some(window) = app.get_webview_window("iris-options") {
        save_iris_window_placement(&window)?;
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn open_reden_library_window(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "reden-library";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("reden_library.html".into()),
    )
    .title(i18n::text("window.onlineLibrary"))
    .inner_size(1080.0, 760.0)
    .min_inner_size(760.0, 560.0)
    .build()
    .map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
async fn open_local_library_folders_window(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "local-library-folders";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("local_library_folders.html".into()),
    )
    .title(i18n::text("window.localFolders"))
    .inner_size(920.0, 680.0)
    .min_inner_size(700.0, 520.0)
    .build()
    .map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
async fn open_ui_demo_window(app: AppHandle) -> Result<(), String> {
    if !read_user_config_data(&current_user_config_dir()?).debug_mode { return Err("请先启用调试模式".into()); }
    const LABEL: &str = "ui-demo-window";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("demo_window.html".into()),
    )
    .title(i18n::text("window.uiDemo"))
    .inner_size(720.0, 520.0)
    .min_inner_size(560.0, 420.0)
    .build()
    .map_err(|err| err.to_string())?;

    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderServiceStartBuildInput {
    #[serde(default)]
    layer_view: bool,
    #[serde(default)]
    initial_layer_range: Option<[i32; 2]>,
    source_path: PathBuf,
    build_mode: RenderBuildMode,
    #[serde(default)]
    display_mode: RenderDisplayMode,
    #[serde(default)]
    preflight_id: Option<ResourcePreflightId>,
    #[serde(default)]
    resource_warning_confirmed: bool,
    #[serde(default)]
    realtime_decision_id: Option<ResourcePreflightId>,
    #[serde(default)]
    click_started_at_epoch_ms: Option<u64>,
    #[serde(default)]
    viewport: Option<[u32; 2]>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderLaunchPlan {
    source_bytes: u64,
    direct_realtime: bool,
    preflight: Option<ResourcePreflightView>,
}

const DIRECT_REALTIME_MAX_BYTES: u64 = 500 * 1024;

fn validate_render_viewport(viewport: Option<[u32; 2]>, build_mode: RenderBuildMode) -> Result<(), String> {
    if viewport.is_some_and(|[width, height]| width == 0 || height == 0 || width > 16384 || height > 16384) {
        return Err("预览视口尺寸无效，请在页面可见后重新加载。".to_owned());
    }
    if build_mode == RenderBuildMode::Prebuilt && viewport.is_none() {
        return Err("尚未取得预览容器的实际像素尺寸，请在页面可见后重新加载。".to_owned());
    }
    Ok(())
}

fn launch_requires_preflight(source_bytes: u64, preload: bool, mode: RenderDisplayMode) -> bool {
    mode != RenderDisplayMode::FastExperimental && (preload || source_bytes > DIRECT_REALTIME_MAX_BYTES)
}

#[tauri::command]
async fn application_render_launch_plan(
    source_path: PathBuf,
    preload: bool,
    display_mode: Option<RenderDisplayMode>,
    viewport: Option<[u32; 2]>,
    facade: tauri::State<'_, RenderFacade>,
) -> Result<RenderLaunchPlan, String> {
    let selected_mode = display_mode.unwrap_or_default();
    if matches!(selected_mode, RenderDisplayMode::Standard | RenderDisplayMode::Full) {
        nova_protected_loader::require_private_runtime().map_err(|error| error.to_string())?;
    }
    let plan_started = std::time::Instant::now();
    let input = RenderServiceStartBuildInput {
        layer_view: false, initial_layer_range: None,
        source_path,
        build_mode: RenderBuildMode::Prebuilt,
        display_mode: selected_mode,
        preflight_id: None,
        resource_warning_confirmed: false,
        realtime_decision_id: None,
        click_started_at_epoch_ms: None,
        viewport,
    };
    let (source_bytes, prepared) = tauri::async_runtime::spawn_blocking(move || {
        if !input.source_path.is_absolute() { return Err("投影路径必须是绝对路径".to_owned()); }
        let metadata = std::fs::metadata(&input.source_path).map_err(|error| error.to_string())?;
        if !metadata.is_file() { return Err("投影不是文件".to_owned()); }
        let bytes = metadata.len();
        // Small direct-Realtime starts do not run the heavy Prebuilt admission probe.
        let compatibility_started = std::time::Instant::now();
        let prepared = if launch_requires_preflight(bytes, preload, input.display_mode) { Some(application_render_request(input)?) } else { None };
        eprintln!("[NOVA_RESOURCE_PHASE] stage=launch_compatibility_prepare elapsed_ms={:.3} preflight_required={}", compatibility_started.elapsed().as_secs_f64() * 1000.0, prepared.is_some());
        Ok::<_, String>((bytes, prepared))
    }).await.map_err(|error| error.to_string())??;
    let preflight = match prepared {
        Some((request, environment)) => Some(facade.preflight_render(request, environment).await?),
        None => None,
    };
    eprintln!("[NOVA_RESOURCE_PHASE] stage=launch_plan_complete elapsed_ms={:.3} preload={preload} preflight_performed={}", plan_started.elapsed().as_secs_f64() * 1000.0, preflight.is_some());
    Ok(RenderLaunchPlan { source_bytes, direct_realtime: !preload && preflight.is_none(), preflight })
}

#[cfg(test)]
mod full_launch_plan_tests {
    use super::*;
    /// Exact production CPU worker role after mode-specific resource admission.
    /// No Viewer launch or presentation is requested. Run alone in a process.
    #[test]
    #[ignore = "requires NOVA_FULL_CPU_TEST_APP_ROOT/SOURCE/OUTPUT_ROOT/SHADERPACK/RESOURCE_PACKS_JSON and the registered signed runtime"]
    fn actual_full_prebuilt_cpu_artifact_with_iris_preflight_cli() {
        use platform::process_supervisor::{ExecutableRole, LaunchIdentity, LaunchSpec, LaunchToken, OutputTarget, WaitOutcome};
        let mode = env::var_os("NOVA_FULL_CPU_TEST_MODE");
        let (display_mode, mode_name, backend_admission, effects_profile) = match mode.as_deref().map(|value| value.to_str().expect("NOVA_FULL_CPU_TEST_MODE must be UTF-8")) {
            None | Some("full") => (RenderDisplayMode::Full, "full", "iris-gl46", "full"),
            Some("standard") => (RenderDisplayMode::Standard, "standard", "vulkan", "unlit"),
            Some(value) => panic!("NOVA_FULL_CPU_TEST_MODE must be standard or full, got {value:?}"),
        };
        let app_root = PathBuf::from(env::var_os("NOVA_FULL_CPU_TEST_APP_ROOT").expect("NOVA_FULL_CPU_TEST_APP_ROOT"));
        let source = PathBuf::from(env::var_os("NOVA_FULL_CPU_TEST_SOURCE").expect("NOVA_FULL_CPU_TEST_SOURCE"));
        let output = PathBuf::from(env::var_os("NOVA_FULL_CPU_TEST_OUTPUT_ROOT").expect("NOVA_FULL_CPU_TEST_OUTPUT_ROOT"));
        let shaderpack = if display_mode == RenderDisplayMode::Full {
            Some(PathBuf::from(env::var_os("NOVA_FULL_CPU_TEST_SHADERPACK").expect("actual user shaderpack is required for Full")))
        } else {
            None
        };
        let resource_packs: Vec<PathBuf> = serde_json::from_str(&env::var("NOVA_FULL_CPU_TEST_RESOURCE_PACKS_JSON").expect("explicit actual resource pack list required, including []")).unwrap();
        assert!(app_root.is_absolute() && source.is_absolute() && output.is_absolute());
        assert!(source.is_file());
        if let Some(path) = &shaderpack { assert!(path.is_absolute() && path.is_file()); }
        assert!(!output.exists(), "preserve prior measurements; use a new isolated output directory");
        std::fs::create_dir_all(&output).unwrap();
        let data_root = output.join("userdata");
        env::set_var("NOVA_USER_DATA_ROOT", &data_root);
        application::diagnostics::init(&data_root, env!("CARGO_PKG_VERSION"), &format!("actual-{mode_name}-cpu-cli")).unwrap();
        APP_RESOURCE_ROOT.set(app_root.clone()).expect("run this ignored test alone");
        APP_LOCAL_DATA_ROOT.set(data_root.clone()).expect("run this ignored test alone");
        let service = RenderServiceClient::default();
        let facade = RenderFacade::new(service.clone());
        let host = initialize_application_host(app_root.clone(), data_root.clone(), &service).unwrap();
        service.launch_in_background(Arc::clone(&host.supervisor), host.service_exe.clone().expect("the exact-render fixture requires the optional private runtime"), data_root.clone());
        let mut protection = application::protected_render::ProtectedRender::default();
        let mut measured = serde_json::json!({"mode":mode_name,"loading":"prebuilt",
            "GPU":"not_executed","firstFrame":"not_measured","workerLaunched":false});
        let result = tauri::async_runtime::block_on(async {
            let viewport = env::var("NOVA_FULL_CPU_TEST_VIEWPORT").ok().map(|value| {
                let (width, height) = value.split_once('x').expect("WIDTHxHEIGHT");
                [width.parse::<u32>().unwrap(), height.parse::<u32>().unwrap()]
            }).unwrap_or([2316, 1148]);
            let (mut request, environment) = application_render_request(RenderServiceStartBuildInput {
                layer_view: false, initial_layer_range: None,
                source_path: source.clone(), build_mode: RenderBuildMode::Prebuilt, display_mode,
                preflight_id: None, resource_warning_confirmed: false, realtime_decision_id: None,
                click_started_at_epoch_ms: None, viewport: Some(viewport),
            })?;
            request.settings.backend = if display_mode == RenderDisplayMode::Full { GraphicsBackendPreference::IrisGl46 } else { GraphicsBackendPreference::Vulkan };
            request.settings.iris_fallback = IrisFallbackPreference::Fail;
            request.settings.shader.path = shaderpack.clone();
            if display_mode == RenderDisplayMode::Standard {
                request.settings.precipitation = WeatherPreference::None;
                request.settings.allow_time_modification = false;
                request.settings.allow_weather_modification = false;
            }
            request.settings.resource_packs = resource_packs.clone();
            if let Some(path) = env::var_os("NOVA_FULL_CPU_TEST_FABRIC_MODS_DIR") { request.settings.fabric_mods_dir = Some(PathBuf::from(path)); }
            let preflight_started = std::time::Instant::now();
            let preflight_result = facade.preflight_render(request.clone(), environment).await;
            let preflight_ms = preflight_started.elapsed().as_millis();
            measured["preflightMs"]=serde_json::json!(preflight_ms);
            let preflight = preflight_result?;
            measured["preflight"]=serde_json::to_value(&preflight).map_err(|e|e.to_string())?;
            if !preflight.allowed_actions.contains(&application::model::ResourcePreflightAction::ContinuePrebuilt) {
                return Err(format!("actual {backend_admission} admission did not allow original {mode_name} Prebuilt: {preflight:?}"));
            }
            if env::var("NOVA_FULL_CPU_TEST_PREFLIGHT_ONLY").as_deref() == Ok("1") {
                if !facade.render_states().is_empty() || !service.snapshot().viewers.is_empty() {
                    return Err("admission-only fixture unexpectedly started a render operation or Viewer".into());
                }
                let deployment: serde_json::Value = serde_json::from_slice(&std::fs::read(
                    app_root.join("bin/viewer-backend/backend-deployment.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
                let build_id = deployment["runtimeBuildId"].as_str().filter(|id|!id.is_empty())
                    .ok_or("validated runtime deployment has no build identity")?;
                // Reuse the completed admission's persisted content identities;
                // do not hash or decode the projection/resources for this report.
                let mut identities = Vec::new();
                for entry in std::fs::read_dir(data_root.join("resource-estimates")).map_err(|e|e.to_string())? {
                    let path = entry.map_err(|e|e.to_string())?.path();
                    if path.extension().and_then(|value|value.to_str()) != Some("json") { continue; }
                    let certificate: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)
                        .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
                    if certificate["inputs"]["source_sha256"].as_str().is_some() {
                        identities.push(serde_json::json!({"certificate":path,"key":certificate["key"],
                            "inputs":certificate["inputs"],"estimator":certificate["estimator"],
                            "layoutVersion":certificate["layout_version"],"deviceIdentity":certificate["device_identity"]}));
                    }
                }
                if identities.is_empty() { return Err(format!("admitted {mode_name} preflight produced no persisted content identity")); }
                let result = serde_json::json!({"mode":mode_name,"loading":"prebuilt","backendAdmission":backend_admission,
                    "preflightOnly":true,"source":source,"shaderpack":shaderpack,"resourcePacks":resource_packs,
                    "buildId":build_id,"runtimeGitSha":deployment["gitSha"],"inputResourceIdentities":identities,
                    "preflightMs":preflight_ms,"admissionMs":preflight_ms,"admissionUnder10s":preflight_ms<=10000,
                    "preflight":preflight,"workerLaunched":false,"GPU":"not_executed","firstFrame":"not_measured","viewerCount":0});
                std::fs::write(output.join("full-preflight-only-result.json"), serde_json::to_vec_pretty(&result)
                    .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
                return Ok::<_,String>(());
            }
            let backend = app_root.join("bin/viewer-backend");
            let material_started = std::time::Instant::now();
            // A separate explicit CPU audit task receives its own real grant;
            // it never invents or reuses a generic process authorization.
            let task_result = protection.acquire(&data_root, &backend, &serde_json::to_vec(&request).map_err(|e| e.to_string())?);
            let material_ms = material_started.elapsed().as_millis();
            measured["workerTaskGrantMaterializeMs"]=serde_json::json!(material_ms);
            let task = task_result?;
            let ready = output.join("full-prebuilt.ready.json");
            let artifact = output.join("full-prebuilt.gpu-scene.cache");
            let contract_path = output.join("full-prebuilt.contract.json");
            let contract = serde_json::json!({
                "schema":"lba.viewer.launch.v1", "requestId":format!("{mode_name}-cpu-prebuilt-{}",std::process::id()),
                "scene":source, "content":{"resourcePacks":resource_packs,"fabricModsDir":request.settings.fabric_mods_dir},
                "renderer":null, "protectedTask":{"ticketPath":task.ticket_path,"modulePath":task.module_path},
                "task":{"kind":"build_cache","buildMode":"prebuilt","effectsProfile":effects_profile,"readyFile":ready,"cacheFile":artifact}
            });
            // BuildCache rejects renderer-only settings by design. Admission
            // validates the mode-specific backend above; the CPU producer uses
            // the same Full or Unlit effects profile as that application route.
            let parsed: ::viewer_launch_contract::ViewerLaunchContract = serde_json::from_value(contract.clone()).map_err(|e|e.to_string())?;
            parsed.validate()?;
            std::fs::write(&contract_path, serde_json::to_vec_pretty(&contract).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            let mut spec = LaunchSpec::new(ExecutableRole::BuildWorker, LaunchIdentity {
                owner_epoch: host.supervisor.owner_epoch(), generation: 1,
                launch_token: LaunchToken::new(format!("{mode_name}-cpu-prebuilt-{}", std::process::id())).map_err(|e|e.to_string())?,
            });
            spec.args.push(format!("--launch-contract={}", contract_path.display()).into());
            spec.current_dir = Some(app_root.clone());
            spec.stdout = OutputTarget::File { path: output.join("full-prebuilt.stdout.log"), append: false };
            spec.stderr = OutputTarget::File { path: output.join("full-prebuilt.stderr.log"), append: false };
            spec.environment = vec![
                ("NOVA_USER_DATA_ROOT".into(), data_root.clone().into_os_string()),
                ("NOVA_PROTECTED_TICKET_PATH".into(), task.ticket_path.into_os_string()),
                ("NOVA_PROTECTED_MODULE_PATH".into(), task.module_path.into_os_string()),
                ("NOVA_RENDER_PHASE_AUDIT".into(), "1".into()),
            ];
            let worker_started = std::time::Instant::now();
            let child = host.supervisor.spawn(spec).map_err(|e|e.to_string())?.child;
            // Hold this exact process object through exit; never query a PID
            // after it might have been reused. Observation is test-only.
            #[cfg(windows)]
            use std::os::windows::io::{AsRawHandle, FromRawHandle};
            #[cfg(windows)]
            let worker_metrics_handle = {
                use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};
                let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, 0, child.pid) };
                if handle.is_null() { None } else {
                    Some(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle.cast()) })
                }
            };
            measured["workerLaunched"]=serde_json::json!(true);
            let timeout = env::var("NOVA_FULL_CPU_TEST_TIMEOUT_SECS").ok().map(|v|v.parse::<u64>().unwrap()).unwrap_or(1200);
            let wait = host.supervisor.wait_for_exit(&child, Duration::from_secs(timeout));
            measured["workerElapsedMs"]=serde_json::json!(worker_started.elapsed().as_millis());
            measured["workerProcessMetrics"]=serde_json::json!({"status":"not_collected"});
            #[cfg(windows)]
            if let Some(handle) = worker_metrics_handle.as_ref() {
                use windows_sys::Win32::{Foundation::FILETIME, System::Threading::{GetProcessTimes, GetProcessIoCounters, IO_COUNTERS}};
                let raw = handle.as_raw_handle().cast();
                let mut creation: FILETIME = unsafe { std::mem::zeroed() };
                let mut exit: FILETIME = unsafe { std::mem::zeroed() };
                let mut kernel: FILETIME = unsafe { std::mem::zeroed() };
                let mut user: FILETIME = unsafe { std::mem::zeroed() };
                if unsafe { GetProcessTimes(raw, &mut creation, &mut exit, &mut kernel, &mut user) } != 0 {
                    let ms = |v: FILETIME| (((v.dwHighDateTime as u64) << 32) | v.dwLowDateTime as u64) as f64 / 10000.0;
                    measured["workerProcessMetrics"]=serde_json::json!({"status":"collected","userCpuMs":ms(user),
                        "kernelCpuMs":ms(kernel),"totalProcessCpuMs":ms(user)+ms(kernel),
                        "scope":"all threads of the actual CPU worker, not wall time or server CPU"});
                    let mut io: IO_COUNTERS = unsafe { std::mem::zeroed() };
                    if unsafe { GetProcessIoCounters(raw, &mut io) } != 0 {
                        measured["workerProcessMetrics"]["readTransferBytes"]=serde_json::json!(io.ReadTransferCount);
                        measured["workerProcessMetrics"]["writeTransferBytes"]=serde_json::json!(io.WriteTransferCount);
                        measured["workerProcessMetrics"]["ioMeaning"]=serde_json::json!("OS transfer counters, not physical disk wait");
                    }
                }
            }
            let exit = match wait {
                WaitOutcome::Exited(exit) => exit,
                other => return Err(format!("owned {mode_name} CPU worker did not exit normally: {other:?}")),
            };
            measured["workerExit"]=serde_json::json!(exit.exit_code);
            if exit.exit_code != Some(0) { return Err(format!("real protected {mode_name} CPU worker exited {:?}; inspect its private stdout/stderr", exit.exit_code)); }
            let worker_ms = worker_started.elapsed().as_millis();
            let ready_value: serde_json::Value = serde_json::from_slice(&std::fs::read(&ready).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            if ready_value["ready"] != true || !artifact.is_file() { return Err("worker exited without a ready exact artifact".into()); }
            if !facade.render_states().is_empty() || !service.snapshot().viewers.is_empty() { return Err("CPU-only fixture unexpectedly started a render operation or Viewer".into()); }
            let mut phase_evidence = Vec::new();
            for log_name in ["full-prebuilt.stdout.log", "full-prebuilt.stderr.log"] {
                use std::io::BufRead;
                let log = std::io::BufReader::new(std::fs::File::open(output.join(log_name)).map_err(|e|e.to_string())?);
                for line in log.lines() {
                    let line = line.map_err(|e|e.to_string())?;
                    if line.starts_with("[V5_BUILD_TRACE]") || line.starts_with("[V5_TERRAIN_TRACE]")
                        || line.starts_with("[NOVA_REMOTE_LIGHT]") || line.starts_with("[NOVA_REMOTE_AO]")
                        || line.starts_with("[NOVA_RENDER_PHASE]") || line.starts_with("[V5_CACHE]") {
                        phase_evidence.push(line);
                    }
                }
            }
            let result = serde_json::json!({"mode":mode_name,"loading":"prebuilt","backendAdmission":backend_admission,"shaderpack":shaderpack,
                "preflightMs":preflight_ms,"admissionMs":preflight_ms,"admissionUnder10s":preflight_ms<=10000,
                "preflight":preflight,"workerTaskGrantMaterializeMs":material_ms,"workerMs":worker_ms,
                "artifact":artifact,"artifactBytes":std::fs::metadata(&artifact).map_err(|e|e.to_string())?.len(),"ready":ready_value,
                "readySummary":{"directorySections":ready_value["total_chunks"],"builtSections":ready_value["built_chunks"],
                    "renderableSections":ready_value["renderable_chunks"],"emptyMeshSections":ready_value["empty_mesh_chunks"]},
                "actualPhaseEvidence":phase_evidence,
                "workerExit":exit.exit_code,"workerProcessMetrics":measured["workerProcessMetrics"],
                "GPU":"not_executed","firstFrame":"not_measured","viewerCount":0,
                "shaderpackBoundary":if display_mode == RenderDisplayMode::Full {
                    "actual Iris preflight; BuildCache has no renderer-only fields, matching production Full CPU role"
                } else {
                    "actual Vulkan preflight without shaderpack or weather; BuildCache uses the production Standard Unlit CPU role"
                }});
            std::fs::write(output.join("full-prebuilt-result.json"), serde_json::to_vec_pretty(&result).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            Ok::<_,String>(())
        });
        if let Err(error)=&result {
            measured["status"]=serde_json::json!("failed");
            measured["error"]=serde_json::json!(error);
            std::fs::write(output.join("full-prebuilt-error-result.json"),serde_json::to_vec_pretty(&measured).unwrap()).unwrap();
        }
        service.request_shutdown(); host.supervisor.stop_admitting();
        let shutdown = host.supervisor.shutdown(Duration::from_millis(500), Duration::from_millis(1500));
        assert!(shutdown.remaining.is_empty());
        result.unwrap();
    }
    /// Opt-in production Host/Service regression. This calls only resource
    /// preflight: it never starts a Build, GPU viewport, WebView or window.
    #[test]
    #[ignore = "requires NOVA_PREFLIGHT_TEST_APP_ROOT/SOURCE/OUTPUT_ROOT with existing signed runtime and live service access"]
    fn actual_application_preflight_standard_and_full_cli() {
        use std::time::Instant;
        let root = PathBuf::from(env::var_os("NOVA_PREFLIGHT_TEST_APP_ROOT").expect("NOVA_PREFLIGHT_TEST_APP_ROOT"));
        let source = PathBuf::from(env::var_os("NOVA_PREFLIGHT_TEST_SOURCE").expect("NOVA_PREFLIGHT_TEST_SOURCE"));
        let output = PathBuf::from(env::var_os("NOVA_PREFLIGHT_TEST_OUTPUT_ROOT").expect("NOVA_PREFLIGHT_TEST_OUTPUT_ROOT"));
        let viewport = env::var("NOVA_PREFLIGHT_TEST_VIEWPORT").ok().map(|value| {
            let (width, height) = value.split_once('x').expect("viewport must be WIDTHxHEIGHT");
            [width.parse::<u32>().unwrap(), height.parse::<u32>().unwrap()]
        }).unwrap_or([1280, 720]);
        assert!(root.is_absolute() && source.is_absolute() && output.is_absolute() && source.is_file());
        std::fs::create_dir_all(&output).unwrap();
        let data_root = output.join("userdata");
        env::set_var("NOVA_USER_DATA_ROOT", &data_root);
        let metadata = application::diagnostics::init(&data_root, env!("CARGO_PKG_VERSION"), "cli-preflight-regression").unwrap();
        let session = data_root.join("logs").join("sessions").join(metadata.session_id);
        APP_RESOURCE_ROOT.set(root.clone()).expect("run this ignored test alone in its own process");
        APP_LOCAL_DATA_ROOT.set(data_root.clone()).expect("run this ignored test alone in its own process");
        let service = RenderServiceClient::default();
        let facade = RenderFacade::new(service.clone());
        let host = initialize_application_host(root, data_root.clone(), &service).unwrap();
        service.launch_in_background(Arc::clone(&host.supervisor), host.service_exe.clone().expect("the exact-render fixture requires the optional private runtime"), data_root.clone());
        let result = tauri::async_runtime::block_on(async {
            let mut results = Vec::new();
            // The large-input cache pair repeats Standard with identical
            // settings; it does not accidentally run a Full lighting audit.
            let display_modes = if env::var_os("NOVA_PREFLIGHT_TEST_STANDARD_CACHE_PAIR").is_some() {
                vec![RenderDisplayMode::Standard, RenderDisplayMode::Standard]
            } else {
                vec![RenderDisplayMode::Standard, RenderDisplayMode::Full]
            };
            for display_mode in display_modes {
                let input = RenderServiceStartBuildInput { layer_view: false, initial_layer_range: None,
                    source_path: source.clone(), build_mode: RenderBuildMode::Prebuilt,
                    display_mode, preflight_id: None, resource_warning_confirmed: false, realtime_decision_id: None,
                    click_started_at_epoch_ms: None, viewport: Some(viewport) };
                let started = Instant::now();
                let (mut request, environment) = application_render_request(input)?;
                // This fixture tests the unmodified Vulkan product route with
                // installed resources, not whichever shader pack a user chose.
                request.settings.backend = application::model::GraphicsBackendPreference::Vulkan;
                request.settings.shader.path = None;
                let view = facade.preflight_render(request, environment).await?;
                let elapsed_ms = started.elapsed().as_millis();
                let diagnostic = std::fs::read_dir(session.join("diagnostics")).map_err(|e| e.to_string())?
                    .filter_map(Result::ok).filter_map(|entry| std::fs::read(entry.path()).ok())
                    .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .max_by_key(|value| value["recordedAtUnixMs"].as_u64().unwrap_or(0))
                    .ok_or_else(|| "real service preflight did not emit resource diagnostic".to_owned())?;
                // The performance target measures the demand body, not device
                // registration, task materialization or fresh budget probes.
                // It is a test assertion, never a production kill deadline.
                if diagnostic["expensiveDemandCalculationPerformed"] == true
                    && diagnostic["calculationElapsedMicros"].as_u64().is_none_or(|micros| micros > 10_000_000) {
                    return Err(format!("actual {display_mode:?} demand calculation exceeded the 10s test target: {diagnostic}"));
                }
                let measured = diagnostic["expensiveDemandCalculationPerformed"] == true || diagnostic["cacheHit"] == true;
                if !measured || view.conservative_vram_bytes.is_none_or(|bytes| bytes == 0)
                    || !view.allowed_actions.contains(&application::model::ResourcePreflightAction::ContinuePrebuilt) {
                    return Err(format!("actual {display_mode:?} preflight not admitted: {view:?}; diagnostic={diagnostic}"));
                }
                if env::var_os("NOVA_PREFLIGHT_TEST_STANDARD_CACHE_PAIR").is_some() && !results.is_empty() && diagnostic["cacheHit"] != true {
                    return Err(format!("identical Standard request repeated expensive demand instead of persistent cache: {diagnostic}"));
                }
                results.push(serde_json::json!({"mode": display_mode, "elapsedMs": elapsed_ms, "preflight": view, "diagnostic": diagnostic}));
            }
            if !service.snapshot().builds.is_empty() || !service.snapshot().viewers.is_empty() {
                return Err("resource-only regression unexpectedly created a Build or Viewer session".to_owned());
            }
            std::fs::write(output.join("application-preflight-result.json"), serde_json::to_vec_pretty(&results).unwrap()).map_err(|e| e.to_string())?;
            Ok::<_, String>(())
        });
        service.request_shutdown(); host.supervisor.stop_admitting();
        let shutdown = host.supervisor.shutdown(Duration::from_secs(3), Duration::from_secs(2));
        assert!(shutdown.remaining.is_empty(), "resource-only regression left owned children");
        result.unwrap();
    }

    #[test]
    fn preflight_requires_actual_physical_viewport_without_defaulting() {
        assert!(validate_render_viewport(None, RenderBuildMode::Prebuilt).is_err());
        for viewport in [[0, 720], [1280, 0], [16385, 720], [1280, 16385]] {
            assert!(validate_render_viewport(Some(viewport), RenderBuildMode::Prebuilt).is_err());
        }
        assert!(validate_render_viewport(Some([2316, 1178]), RenderBuildMode::Prebuilt).is_ok());
        assert!(validate_render_viewport(None, RenderBuildMode::Realtime).is_ok());
    }
    #[test]
    fn fast_skips_demand_preflight_in_both_loading_modes() {
        assert!(!launch_requires_preflight(u64::MAX, true, RenderDisplayMode::FastExperimental));
        assert!(!launch_requires_preflight(u64::MAX, false, RenderDisplayMode::FastExperimental));
        assert!(launch_requires_preflight(u64::MAX, true, RenderDisplayMode::Standard));
    }
    #[test]
    fn small_direct_realtime_skips_prebuilt_probe() {
        assert!(!launch_requires_preflight(DIRECT_REALTIME_MAX_BYTES - 1, false, RenderDisplayMode::Full));
    }
    #[test]
    fn threshold_and_large_sources_require_live_budget() {
        assert!(!launch_requires_preflight(DIRECT_REALTIME_MAX_BYTES, false, RenderDisplayMode::Full));
        assert!(launch_requires_preflight(DIRECT_REALTIME_MAX_BYTES + 1, false, RenderDisplayMode::Full));
    }
    #[test]
    fn requested_preload_always_uses_existing_prebuilt_admission() {
        assert!(launch_requires_preflight(1, true, RenderDisplayMode::Full));
    }
}

fn validate_exact_graphics_preference(backend: ::viewer_launch_contract::GraphicsApiPreference) -> Result<(), String> {
    if backend == ::viewer_launch_contract::GraphicsApiPreference::Dx12 {
        return Err("标准和完整模式当前使用 Vulkan；DX12 仅适用于快速模式。请选择自动或 Vulkan 后重试。".into());
    }
    Ok(())
}

fn application_render_request(
    input: RenderServiceStartBuildInput,
) -> Result<(UserVisibleRenderRequest, RenderEnvironmentInputs), String> {
    nova_protected_loader::require_private_runtime().map_err(|error| error.to_string())?;
    if !input.source_path.is_absolute() || !input.source_path.is_file() {
        return Err(format!(
            "投影文件不存在或不是绝对路径：{}",
            input.source_path.display()
        ));
    }
    validate_render_viewport(input.viewport, input.build_mode)?;
    if input.layer_view && (input.display_mode != RenderDisplayMode::Standard || input.build_mode != RenderBuildMode::Realtime
        || input.preflight_id.is_some() || input.realtime_decision_id.is_some() || input.resource_warning_confirmed) {
        return Err("3D layers require Standard Realtime without resource preflight".into());
    }
    if input.initial_layer_range.is_some_and(|[low, high]| low > high) {
        return Err("Layer minimum exceeds maximum".into());
    }
    if !input.layer_view && input.initial_layer_range.is_some() { return Err("Initial layer range requires 3D layers".into()); }
    let config = read_user_config_data(&current_user_config_dir()?);
    if !matches!(input.display_mode, RenderDisplayMode::Full | RenderDisplayMode::Standard) {
        return Err("精确渲染入口只接受 full 或 standard；快速模式保持独立启动链".to_owned());
    }
    validate_exact_graphics_preference(config.graphics_backend)?;
    let mut compatibility = config.v5_compatibility.clone();
    if config.graphics_backend == ::viewer_launch_contract::GraphicsApiPreference::Vulkan {
        compatibility.backend = "vulkan".into();
    }
    let unlit = input.display_mode == RenderDisplayMode::Standard;
    if unlit {
        compatibility.backend = "vulkan".to_owned();
        compatibility.iris_fallback = "fail".to_owned();
        compatibility.shaderpack_root.clear();
        compatibility.shaderpack_name.clear();
    }
    let plan = ViewerCompatibilityInputs::resolve(
        &compatibility,
        &get_root(),
        CompatibilityScope::Render,
    )?;
    let renderer = plan
        .renderer()
        .ok_or_else(|| "render compatibility plan 缺少 renderer".to_owned())?;
    let backend = match renderer.backend {
        ::viewer_launch_contract::ViewerBackend::Auto => GraphicsBackendPreference::Auto,
        ::viewer_launch_contract::ViewerBackend::Vulkan => GraphicsBackendPreference::Vulkan,
        ::viewer_launch_contract::ViewerBackend::IrisGl46 => GraphicsBackendPreference::IrisGl46,
    };
    let iris_fallback = match renderer.iris_fallback {
        ::viewer_launch_contract::IrisFallback::Fail => IrisFallbackPreference::Fail,
        ::viewer_launch_contract::IrisFallback::Vulkan => IrisFallbackPreference::Vulkan,
    };
    let precipitation = match renderer.precipitation {
        ::viewer_launch_contract::WeatherPrecipitation::Auto => WeatherPreference::Auto,
        ::viewer_launch_contract::WeatherPrecipitation::Rain => WeatherPreference::Rain,
        ::viewer_launch_contract::WeatherPrecipitation::Snow => WeatherPreference::Snow,
        ::viewer_launch_contract::WeatherPrecipitation::None => WeatherPreference::None,
    };
    let content = plan.content();
    let game_data = game_data::resolve_context(&current_user_config_dir()?.join("game-data"),
        &get_root().join("pack-in/game-data/default")).map_err(|error| format!("{error:#}"))?;
    let mut resource_packs = vec![game_data.resource_jar.clone()];
    resource_packs.extend(content.resource_packs.iter().cloned());
    let settings = ProjectRenderSettings {
        display_mode: input.display_mode,
        build_mode: input.build_mode,
        layers: LayerSelection::All,
        layer_view: input.layer_view,
        initial_layer_range: input.initial_layer_range,
        highlights: Default::default(),
        backend,
        shader: ShaderSelection {
            path: renderer.shaderpack.as_ref().map(|value| value.path.clone()),
        },
        iris_fallback,
        precipitation: if unlit { WeatherPreference::None } else { precipitation },
        allow_time_modification: !unlit && config.viewer_controls.allow_time_modification,
        allow_weather_modification: !unlit && config.viewer_controls.allow_weather_modification,
        fabric_mods_dir: content.fabric_mods_dir.clone(),
        resource_packs,
    };
    let root = get_root();
    let asset_inputs = [
        game_data.root.join("manifest.json"),
        game_data.root.join("derived/oracle"),
        root.join("mc26-oracle").join("verified-schema3-26.3"),
        root.join("tools")
            .join("mc26-oracle")
            .join("build")
            .join("verified-schema3-26.3"),
        root.join("bin")
            .join("viewer-backend")
            .join("backend-deployment.json"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect();
    Ok((
        UserVisibleRenderRequest {
            source: ProjectSource {
                path: input.source_path,
            },
            settings,
            presentation: PresentationIntent {
                mode: PresentationMode::Hidden,
                visible: false,
                placement: None,
             presentation_epoch: None },
        },
        RenderEnvironmentInputs { asset_inputs, viewport: input.viewport },
    ))
}

#[tauri::command]
async fn application_set_scene_view(source_path: PathBuf, display_mode: Option<String>, controls: ::viewer_launch_contract::SceneViewControls,
    quick: tauri::State<'_, QuickPreviewHost>, facade: tauri::State<'_, RenderFacade>) -> Result<(), String> {
    controls.validate()?;
    let source = std::fs::canonicalize(source_path).map_err(|error| error.to_string())?;
    if display_mode.as_deref().is_none_or(|mode| mode == "fast_experimental") && quick.set_scene_view(&source, controls.clone())? { return Ok(()); }
    let mode = match display_mode.as_deref() { Some("full") => Some(RenderDisplayMode::Full), Some("standard") => Some(RenderDisplayMode::Standard), None => None, _ => return Err("请先启动快速预览".into()) };
    facade.set_scene_view(&source, mode, controls).await
}

static IRIS_OPTIONS_TRANSACTION: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
#[tauri::command]
async fn iris_shaderpack_settings(app: AppHandle, args: Vec<String>, operations: tauri::State<'_, ApplicationOperations>, facade: tauri::State<'_, RenderFacade>) -> Result<serde_json::Value, String> {
    let saving = args.iter().any(|arg| arg == "--save-json");
    // Reads must not observe a staged disk candidate before Viewer commit/rollback.
    let _transaction = IRIS_OPTIONS_TRANSACTION.get_or_init(|| tokio::sync::Mutex::new(())).lock().await;
    let worker = operations.inner().clone(); let request = args.clone();
    let mut value = tauri::async_runtime::spawn_blocking(move || worker.iris_settings(request)).await.map_err(|error| error.to_string())??;
    if !saving {
        let config = read_user_config_data(&current_user_config_dir()?);
        value["selected_for_rendering"] = serde_json::json!(value["pack"].as_str() == Some(config.v5_compatibility.shaderpack_name.as_str())
            && value["root"].as_str() == Some(config.v5_compatibility.shaderpack_root.as_str()));
        return Ok(value);
    }
    let previous_saved = value.as_object_mut().and_then(|value| value.remove("previous_saved")).ok_or("Shader save transaction has no previous state")?;
    let root = value["root"].as_str().ok_or("Missing shader pack root")?.to_owned();
    let pack = value["pack"].as_str().ok_or("Missing shader pack name")?.to_owned();
    let mut applied = false;
    let mut previous_config = None;
    let apply = async {
        // Finish fallible disk work before GPU commit. A failed config write
        // must never require guessing/reloading the previously running pack.
        let dir = current_user_config_dir()?;
        let default_dir = default_user_config_dir()?;
        let mut config = read_user_config_data(&dir);
        previous_config = Some((dir.clone(), config.clone()));
        config.v5_compatibility.shaderpack_root = root.clone();
        config.v5_compatibility.shaderpack_name = pack.clone();
        write_user_config_data(&dir, &config)?;
        if value["requires_scene_reload"].as_bool() != Some(true) {
            applied = facade.reload_shader_options(PathBuf::from(&root), pack.clone()).await?;
        }
        let info = UserConfigInfo { config_dir: dir.to_string_lossy().into(), default_config_dir: default_dir.to_string_lossy().into(), config };
        let _ = app.emit("user-config-changed", &info);
        Ok::<(), String>(())
    }.await;
    if let Err(error) = apply {
        let mut rollback_args = Vec::new(); let mut index = 0;
        while index < args.len() {
            if args[index] == "--save-json" { index += 2; } else { rollback_args.push(args[index].clone()); index += 1; }
        }
        rollback_args.extend(["--rollback-json".into(), previous_saved.to_string()]);
        let worker = operations.inner().clone();
        let rollback = tauri::async_runtime::spawn_blocking(move || worker.iris_settings(rollback_args)).await
            .map_err(|error| error.to_string()).and_then(|result| result);
        let config_rollback = previous_config.map(|(dir, config)| write_user_config_data(&dir, &config)).unwrap_or(Ok(()));
        let mut failure = error;
        if let Err(restore) = rollback { failure.push_str(&format!("; restore previous shader settings: {restore}")); }
        if let Err(restore) = config_rollback { failure.push_str(&format!("; restore selected shader pack: {restore}")); }
        return Err(failure);
    }
    value["applied_to_active_viewer"] = serde_json::json!(applied);
    value["selected_for_rendering"] = serde_json::json!(true);
    Ok(value)
}

#[tauri::command]
async fn application_quick_preview_resource_decision(
    window: tauri::WebviewWindow,
    preview_id: String,
    action: application::quick_preview::QuickResourceAction,
    warning_confirmed: bool,
    host: tauri::State<'_, QuickPreviewHost>,
) -> Result<QuickPreviewView, String> {
    #[cfg(target_os="macos")]
    let native_owner = platform::native_preview_host::begin_current_owner(&window, &preview_id)?;
    let host = host.inner().clone();
    let deciding = host.clone();
    let view = tauri::async_runtime::spawn_blocking(move || deciding.resource_decision(&preview_id, action, warning_confirmed)).await.map_err(|error| error.to_string())??;
    #[cfg(target_os="macos")]
    if platform::native_preview_host::complete_owner(native_owner, view.preview_id.clone()) {
        let id = view.preview_id.clone();
        let view = tauri::async_runtime::spawn_blocking(move || host.renew_native_presentation(&id)).await.map_err(|error| error.to_string())??;
        sync_native_quick_presentation(&window, &view)?;
        return Ok(view);
    }
    #[cfg(not(target_os="macos"))] let _ = window;
    Ok(view)
}

#[tauri::command]
async fn application_quick_preview_start(window: tauri::WebviewWindow, mut input: StartQuickPreviewInput, host: tauri::State<'_, QuickPreviewHost>) -> Result<QuickPreviewView, String> {
    if window.label() != "main" { return Err("原生预览只允许主窗口启动".into()); }
    let host_window = platform::native_preview_host::host_window(&window)?;
    input.graphics_backend = read_user_config_data(&current_user_config_dir()?).graphics_backend;
    #[cfg(target_os="macos")]
    let native_owner = platform::native_preview_host::begin_owner(&window)?;
    let host = host.inner().clone();
    let starting_host = host.clone();
    let view = tauri::async_runtime::spawn_blocking(move || starting_host.start(input, host_window)).await.map_err(|error| error.to_string())??;
    #[cfg(target_os="macos")]
    if platform::native_preview_host::complete_owner(native_owner, view.preview_id.clone()) {
        let id = view.preview_id.clone();
        let view = tauri::async_runtime::spawn_blocking(move || host.renew_native_presentation(&id)).await.map_err(|error| error.to_string())??;
        sync_native_quick_presentation(&window, &view)?;
        return Ok(view);
    }
    Ok(view)
}

#[tauri::command]
async fn application_quick_preview_snapshot(window: tauri::WebviewWindow, host: tauri::State<'_, QuickPreviewHost>) -> Result<Option<QuickPreviewView>, String> {
    let host = host.inner().clone();
    let view = tauri::async_runtime::spawn_blocking(move || host.snapshot()).await.map_err(|error| error.to_string())??;
    #[cfg(target_os="macos")]
    if let Some(view) = &view { sync_native_quick_presentation(&window, view)?; }
    #[cfg(not(target_os="macos"))] let _ = window;
    Ok(view)
}

#[tauri::command]
async fn application_quick_preview_present(window: tauri::WebviewWindow, preview_id: String, intent: QuickPresentationIntent, revision: u64, host: tauri::State<'_, QuickPreviewHost>) -> Result<QuickPreviewView, String> {
    let host = host.inner().clone();
    let view = tauri::async_runtime::spawn_blocking(move || host.present(&preview_id, intent, revision)).await.map_err(|error| error.to_string())??;
    #[cfg(target_os="macos")] sync_native_quick_presentation(&window, &view)?;
    #[cfg(not(target_os="macos"))] let _ = window;
    Ok(view)
}

#[tauri::command]
fn native_preview_set_background(window: tauri::WebviewWindow, color: [u8; 4]) -> Result<(), String> {
    platform::native_preview_host::set_background(&window, color)
}

#[cfg(target_os="macos")]
fn sync_native_quick_presentation(window: &tauri::WebviewWindow, view: &QuickPreviewView) -> Result<(),String> {
    platform::native_preview_host::apply(window, view.preview_id.clone(), view.presentation.placement,
        view.presentation.presentation_epoch, view.visible && view.state == "ready",
        platform::native_preview_host::NativePresentationMode::Embedded)
}

#[tauri::command]
async fn application_quick_preview_close(window: tauri::WebviewWindow, preview_id: String, host: tauri::State<'_, QuickPreviewHost>) -> Result<(), String> {
    #[cfg(target_os="macos")] { let _ = platform::native_preview_host::hide_owner(&window, &preview_id); }
    #[cfg(not(target_os="macos"))] let _ = window;
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host.close(&preview_id)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn application_render_preflight(
    input: RenderServiceStartBuildInput,
    facade: tauri::State<'_, RenderFacade>,
) -> Result<ResourcePreflightView, String> {
    if input.preflight_id.is_some() || input.resource_warning_confirmed {
        return Err("resource preflight request cannot carry a prior admission".to_owned());
    }
    let (request, environment) =
        tauri::async_runtime::spawn_blocking(move || application_render_request(input))
            .await
            .map_err(|error| format!("准备 Application resource preflight 失败：{error}"))??;
    facade.preflight_render(request, environment).await
}

#[tauri::command]
async fn application_render_use_basic_vulkan(
    preflight_id: ResourcePreflightId,
    source_path: PathBuf,
    facade: tauri::State<'_, RenderFacade>,
) -> Result<ResourcePreflightView, String> {
    facade
        .use_basic_vulkan(&preflight_id, &ProjectSource { path: source_path })
        .await
}

#[tauri::command]
async fn application_render_start(
    input: RenderServiceStartBuildInput,
    app: AppHandle,
    facade: tauri::State<'_, RenderFacade>,
) -> Result<application::model::RenderOperationView, String> {
    #[cfg(target_os="macos")]
    let native_owner = platform::native_preview_host::begin_owner(&app.get_webview_window("main").ok_or("主窗口尚未创建")?)?;
    let click_started_at_epoch_ms = input.click_started_at_epoch_ms;
    let authorization = RenderStartAuthorization {
        click_started_at_epoch_ms,
        preflight_id: input.preflight_id.clone(),
        resource_warning_confirmed: input.resource_warning_confirmed,
    };
    let operation = match input.build_mode {
        RenderBuildMode::Prebuilt => {
            if input.realtime_decision_id.is_some() { return Err("预构建不能携带实时加载决策".into()); }
            facade
                .start_prebuilt_render(
                    ProjectSource {
                        path: input.source_path,
                    },
                    authorization,
                )
                .await
        }
        RenderBuildMode::Realtime => {
            if authorization.preflight_id.is_some() || authorization.resource_warning_confirmed {
                return Err(
                    "realtime build must not carry a prebuilt resource admission".to_owned(),
                );
            }
            if let Some(id) = input.realtime_decision_id {
                let operation = facade.start_realtime_from_preflight_with_click(&id, ProjectSource { path: input.source_path }, click_started_at_epoch_ms).await?.operation;
                #[cfg(target_os="macos")]
                platform::native_preview_host::complete_owner(native_owner, operation.operation_id.0.clone());
                emit_application_render_snapshot(&app, &facade)?;
                return Ok(operation);
            }
            let (request, environment) =
                tauri::async_runtime::spawn_blocking(move || application_render_request(input))
                    .await
                    .map_err(|error| format!("准备 Application render request 失败：{error}"))??;
            facade.start_realtime_render_with_click(request, environment, click_started_at_epoch_ms).await
        }
    }
    .map(|result| result.operation)?;
    #[cfg(target_os="macos")]
    platform::native_preview_host::complete_owner(native_owner, operation.operation_id.0.clone());
    emit_application_render_snapshot(&app, &facade)?;
    Ok(operation)
}

#[tauri::command]
fn application_render_discard_preflight(
    preflight_id: ResourcePreflightId,
    facade: tauri::State<'_, RenderFacade>,
) {
    facade.discard_preflight(&preflight_id);
}

#[tauri::command]
async fn application_render_cancel(
    operation_id: OperationId,
    app: AppHandle,
    facade: tauri::State<'_, RenderFacade>,
) -> Result<(), String> {
    #[cfg(target_os="macos")]
    if let Some(window) = app.get_webview_window("main") { let _ = platform::native_preview_host::hide_owner(&window, &operation_id.0); }
    facade.cancel_render(&operation_id).await?;
    emit_application_render_snapshot(&app, &facade)
}

#[tauri::command]
async fn application_render_update_presentation(
    operation_id: OperationId,
    mut intent: PresentationIntent,
    app: AppHandle,
    facade: tauri::State<'_, RenderFacade>,
) -> Result<application::model::RenderOperationView, String> {
    #[cfg(target_os="macos")]
    {
        match platform::native_preview_host::advance_owner(&operation_id.0) {
            Ok(epoch) => {
                intent.presentation_epoch = Some(epoch);
                let window = app.get_webview_window("main").ok_or("主窗口尚未创建")?;
                platform::native_preview_host::apply(&window, operation_id.0.clone(), intent.placement, Some(epoch), intent.visible,
                    if intent.mode == application::model::PresentationMode::Popup {
                        platform::native_preview_host::NativePresentationMode::Popup
                    } else { platform::native_preview_host::NativePresentationMode::Embedded })?;
            }
            Err(error) if intent.visible => return Err(error),
            Err(_) => { intent.presentation_epoch = None; }
        }
    }
    let operation = facade.update_presentation(&operation_id, intent).await?;
    emit_application_render_snapshot(&app, &facade)?;
    Ok(operation)
}

fn emit_application_render_snapshot(app: &AppHandle, facade: &RenderFacade) -> Result<(), String> {
    app.emit(
        APPLICATION_RENDER_STATE_EVENT,
        ApplicationRenderSnapshot {
            operations: facade.render_states(),
        },
    )
    .map_err(|error| format!("publish Application render state: {error}"))
}

#[tauri::command]
fn application_render_get_snapshot(
    facade: tauri::State<'_, RenderFacade>,
) -> ApplicationRenderSnapshot {
    ApplicationRenderSnapshot {
        operations: facade.render_states(),
    }
}

struct ApplicationHostComponents {
    supervisor: Arc<platform::process_supervisor::ProcessSupervisor>,
    operations: ApplicationOperations,
    service_exe: Option<PathBuf>,
}

/// Both the WebView host and the product E2E client enter through this same
/// process ownership and Application operation setup.
fn initialize_application_host(
    project_root: PathBuf,
    app_local_data_root: PathBuf,
    render_service: &RenderServiceClient,
) -> Result<ApplicationHostComponents, String> {
    let started = std::time::Instant::now();
    let service_exe = if nova_protected_loader::is_community_build() { None }
        else { Some(required_backend_exe_path("nova-render-service.exe")?) };
    let worker_exe = if nova_protected_loader::is_community_build() { None }
        else { Some(required_backend_exe_path("nova-build-worker.exe")?) };
    let viewer_exe = required_backend_exe_path("litematica_native_viewer.exe")?;
    let core_exe = required_backend_exe_path("litematica_core.exe")?;
    let mut allowlist = platform::process_supervisor::ExecutableAllowlist::new();
    for (role, executable) in [
        (
            platform::process_supervisor::ExecutableRole::NativeViewer,
            &viewer_exe,
        ),
        (
            platform::process_supervisor::ExecutableRole::CoreHelper,
            &core_exe,
        ),
    ] {
        allowlist.allow(role, executable).map_err(|error| error.to_string())?;
    }
    for (role, executable) in [
        (platform::process_supervisor::ExecutableRole::RenderService, service_exe.as_ref()),
        (platform::process_supervisor::ExecutableRole::BuildWorker, worker_exe.as_ref()),
    ] {
        if let Some(executable) = executable {
            allowlist.allow(role, executable).map_err(|error| error.to_string())?;
        }
    }
    let (supervisor, process_events) =
        platform::process_supervisor::ProcessSupervisor::new(allowlist)
            .map_err(|error| format!("initialize Application process supervisor: {error}"))?;
    let supervisor = Arc::new(supervisor);
    let operations = ApplicationOperations::new(
        Arc::clone(&supervisor),
        project_root,
        app_local_data_root,
        core_exe
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "backend executable has no directory".to_owned())?,
    )?;
    render_service.observe_process_events(process_events);
    eprintln!("[NOVA_LIFECYCLE] stage=application_host elapsed_ms={} resource_seed_calls={} operations={}", started.elapsed().as_millis(), FLAKE_RESOURCE_SEED_CALLS.load(Ordering::Relaxed), operations.lifecycle_snapshot());
    Ok(ApplicationHostComponents {
        supervisor,
        operations,
        service_exe,
    })
}

#[tauri::command]
async fn application_export_logs_zip(path: String) -> Result<application::diagnostics::LogExportResult, String> {
    tauri::async_runtime::spawn_blocking(move || application::diagnostics::export_zip(Path::new(&path)))
        .await.map_err(|error| error.to_string())?
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args
        .iter()
        .any(|arg| arg.as_os_str() == std::ffi::OsStr::new("--headless-e2e"))
    {
        std::process::exit(headless_e2e::run(&args));
    }
    let application_context = tauri::generate_context!();
    if let Some(status) = application::updater::run_cli(
        &args, application_context.config(),
        &application_context.package_info().version.to_string(),
    ) { std::process::exit(status); }
    #[cfg(target_os = "linux")]
    if let Some(status) = shell_integration::dispatch_before_gui(&args, application_context.package_info()) {
        std::process::exit(status);
    }
    if let Err(error) = platform::native_preview_host::configure_process_graphics() {
        eprintln!("[NOVA_NATIVE_HOST] initialization_failed error={error}");
        std::process::exit(1);
    }
    let render_service = RenderServiceClient::default();
    let render_facade = RenderFacade::new(render_service.clone());
    let builder = tauri::Builder::default()
        .manage(ApplicationCloseGate::default())
        .manage(render_service)
        .manage(render_facade)
        .manage(shell_integration::OpenFiles::default());
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            shell_integration::queue_open_files(app, args.iter().skip(1).map(String::as_str), Path::new(&cwd));
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize(); let _ = window.show(); let _ = window.set_focus();
            }
        }));
    builder.setup(|app| {
            shell_integration::queue_open_files(app.handle(), env::args().skip(1).collect::<Vec<_>>().iter().map(String::as_str), &env::current_dir()?);
            let resource_root = app.path().resource_dir()?;
            let app_local_data_root = product_local_data_root(&app.path().local_data_dir()?);
            #[cfg(target_os = "linux")]
            shell_integration::initialize_linux(app.package_info().clone(), app_local_data_root.clone());
            let _ = APP_RESOURCE_ROOT.set(resource_root);
            let _ = APP_LOCAL_DATA_ROOT.set(app_local_data_root.clone());
            std::env::set_var("NOVA_USER_DATA_ROOT", &app_local_data_root);
            let ui_config = read_user_config_data(&current_user_config_dir()?);
            i18n::set_locale(&ui_config.ui_locale);
            application::diagnostics::init(&app_local_data_root, env!("CARGO_PKG_VERSION"),
                option_env!("LBA_BUILD_GIT_SHA").unwrap_or("unknown"))?;
            app.manage(UpdateHost::new(app.package_info().version.to_string(), app_local_data_root.clone()));
            let host = initialize_application_host(
                get_root(),
                app_local_data_root.clone(),
                app.state::<RenderServiceClient>().inner(),
            )
            .map_err(std::io::Error::other)?;
            app.manage(QuickPreviewHost::new(Arc::clone(&host.supervisor),
                required_backend_exe_path("litematica_native_viewer.exe").map_err(std::io::Error::other)?, app_local_data_root.clone()));
            assets::bind_operations(host.operations.clone());
            app.manage(Arc::clone(&host.supervisor));
            app.manage(host.operations);
            app.state::<RenderFacade>()
                .start_event_bridge(app.handle().clone())
                .map_err(std::io::Error::other)?;
            let main_window = app.get_webview_window("main")
                .ok_or_else(|| std::io::Error::other("主窗口尚未创建"))?;
            let quick = app.state::<QuickPreviewHost>().inner().clone();
            let quick_window = platform::native_preview_host::host_window(&main_window)
                .map_err(std::io::Error::other)?;
            app.state::<RenderServiceClient>().set_native_host(&quick_window).map_err(std::io::Error::other)?;
            let graphics_backend = read_user_config_data(&current_user_config_dir().map_err(std::io::Error::other)?).graphics_backend;
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = quick.prepare_gpu_context(quick_window, graphics_backend) {
                    eprintln!("[NOVA_GPU_INITIALIZATION] stage=ordinary_startup_not_ready error={error} projection_reads=0 scene_mesh_builds=0");
                }
            });
            if let Some(service_exe) = host.service_exe {
                app.state::<RenderServiceClient>()
                    .launch_in_background(host.supervisor, service_exe, app_local_data_root);
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if window.label() == "material-list" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            if window.label() == "iris-options" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Some(webview) = window.app_handle().get_webview_window("iris-options") {
                        if let Err(error) = save_iris_window_placement(&webview) { eprintln!("[NOVA_IRIS_OPTIONS] save_window_placement_error={error}"); }
                        let _ = webview.hide();
                    }
                }
                if matches!(event, tauri::WindowEvent::Focused(true) | tauri::WindowEvent::CloseRequested { .. }) {
                    let facade = window.state::<RenderFacade>().inner().clone();
                    tauri::async_runtime::spawn(async move { if let Err(error) = facade.release_camera_input().await { eprintln!("[NOVA_IRIS_OPTIONS] release_input_error={error}"); } });
                }
                if matches!(event, tauri::WindowEvent::ScaleFactorChanged { .. }) {
                    if let Some(webview) = window.app_handle().get_webview_window("iris-options") {
                        let _ = restore_iris_window_placement(&webview, None);
                    }
                }
            }
            #[cfg(windows)]
            if window.label() == "main"
                && platform::native_preview_host::windows_clipping_needs_refresh(event)
            {
                // Tao rebuilds native styles on maximize/restore. Preserve the
                // child/sibling clipping contract without touching the Viewer.
                let result = window.hwnd().map_err(|error| error.to_string())
                    .and_then(|hwnd| platform::native_preview_host::ensure_windows_host_clipping(hwnd.0 as isize));
                if let Err(error) = result {
                    eprintln!("[NOVA_NATIVE_HOST] stage=maintain_clipping error={error}");
                }
            }
            #[cfg(target_os="macos")]
            if window.label() == "main" {
                // Occlusion/minimization uses native AppKit notifications in
                // MacNativeView; the locked Tauri WindowEvent has no Occluded.
                if matches!(event, tauri::WindowEvent::CloseRequested { .. })
                    || matches!(event, tauri::WindowEvent::Resized(size) if size.width == 0 || size.height == 0) {
                    if let Some(host) = window.app_handle().get_webview_window("main") {
                        let _ = platform::native_preview_host::suspend_current(&host);
                    }
                }
                if matches!(event, tauri::WindowEvent::Destroyed) { platform::native_preview_host::dispose_current(); }
            }
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if window.try_state::<UpdateHost>().is_some_and(|host| !host.prepare_for_close(window.app_handle())) {
                        api.prevent_close();
                        return;
                    }
                }
            }
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event { api.prevent_close(); }
                if SHUTDOWN_STARTED.load(Ordering::Acquire) { return; }
                let (request_id, started) = match window.state::<ApplicationCloseGate>().native_request() {
                    CloseDecision::AwaitDrafts { request, created } => {
                        let app = window.app_handle().clone();
                        // Repeated X retries delivery, not the transaction. An
                        // already-persisted ACK queued behind render IPC must
                        // still authorize this same outstanding close intent.
                        let _ = app.emit_to("main", "nova-close-requested", request);
                        if created {
                            eprintln!("[NOVA_LIFECYCLE] stage=close_native_request request_id={} elapsed_ms=0", request.request_id);
                            tauri::async_runtime::spawn(async move {
                                tokio::time::sleep(CLOSE_DRAFT_ACK_WARNING_AFTER).await;
                                if let Some(elapsed) = app.state::<ApplicationCloseGate>().pending_elapsed(request.request_id) {
                                    let message = i18n::text("close.savingDrafts");
                                    let _ = app.emit_to("main", "nova-close-delayed", serde_json::json!({ "requestId": request.request_id, "message": message }));
                                    if let Some(window) = app.get_webview_window("main") { let _ = window.set_title(i18n::text("window.closing")); }
                                    eprintln!("[NOVA_LIFECYCLE] stage=close_draft_ack_delayed request_id={} elapsed_ms={} request_preserved=true", request.request_id, elapsed.as_millis());
                                }
                            });
                        }
                        return;
                    }
                    CloseDecision::Approved { request_id, started } => (request_id, started),
                };
                let supervisor = window
                    .try_state::<Arc<platform::process_supervisor::ProcessSupervisor>>()
                    .map(|state| Arc::clone(state.inner()));
                let render_service = window.try_state::<RenderServiceClient>().map(|state| state.inner().clone());
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                }
                if SHUTDOWN_STARTED.swap(true, Ordering::AcqRel) { return; }
                let app = window.app_handle().clone();
                let quick = window.try_state::<QuickPreviewHost>().map(|host| host.inner().clone());
                let operations = window.try_state::<ApplicationOperations>().map(|host| host.inner().clone());
                // Parent-native child visibility follows this immediately.
                // Do not run disk/network/process waits on the UI callback.
                for (_, window) in app.webview_windows() { let _ = window.hide(); }
                eprintln!("[NOVA_LIFECYCLE] stage=close_window_response request_id={request_id} elapsed_ms={}", started.elapsed().as_millis());
                tauri::async_runtime::spawn_blocking(move || {
                    if let Some(update) = app.try_state::<UpdateHost>() { update.cancel_for_shutdown(); }
                    online_sources::cancel_for_shutdown();
                    assets::cancel_for_shutdown();
                    let reads = operations.as_ref().map(ApplicationOperations::begin_shutdown).unwrap_or_default();
                    if let Some(supervisor) = supervisor.as_ref() { supervisor.terminate_many(&reads, 0); }
                    let writes = std::time::Instant::now();
                    if operations.as_ref().is_some_and(|operations| !operations.wait_for_writes(Duration::from_secs(8))) {
                        // A stalled persistent write is not safe to force-kill.
                        // Cancel closing, restore a usable window and let the
                        // user retry after the existing save finishes.
                        operations.as_ref().unwrap().cancel_shutdown();
                        if let Some(update) = app.try_state::<UpdateHost>() { update.resume_after_cancelled_shutdown(); }
                        online_sources::resume_after_cancelled_shutdown();
                        assets::resume_after_cancelled_shutdown();
                        SHUTDOWN_STARTED.store(false, Ordering::Release);
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.set_title(i18n::text("window.saving"));
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                        let _ = app.emit_to("main", "nova-close-cancelled", serde_json::json!({ "message": i18n::text("close.stillSaving") }));
                        eprintln!("[NOVA_LIFECYCLE] stage=close_deferred_save request_id={request_id} elapsed_ms={}", started.elapsed().as_millis());
                        return;
                    }
                    eprintln!("[NOVA_LIFECYCLE] stage=close_writes_drained request_id={request_id} elapsed_ms={} total_ms={}", writes.elapsed().as_millis(), started.elapsed().as_millis());
                    if let Some(supervisor) = supervisor.as_ref() { supervisor.stop_admitting(); }
                    // Both graceful intents are non-waiting; all children
                    // share one deadline instead of close(wait)+shutdown(wait).
                    if let Some(service) = render_service { service.request_shutdown(); }
                    if let Some(quick) = quick { let _ = quick.request_shutdown(); }
                    if let Some(supervisor) = supervisor {
                        let report = supervisor.shutdown(Duration::from_millis(500), Duration::from_millis(1500));
                        eprintln!("[NOVA_LIFECYCLE] stage=close_children_reaped request_id={request_id} total_ms={} child_ms={} graceful_ms={} forced_ms={} pending={}", started.elapsed().as_millis(), report.elapsed_ms, report.graceful_wait_ms, report.forced_wait_ms, report.remaining.len());
                    }
                    eprintln!("[NOVA_LIFECYCLE] stage=application_exit_requested request_id={request_id} elapsed_ms={}", started.elapsed().as_millis());
                    app.exit(0);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            native_preview_set_background,
            application_export_logs_zip,
            assets::ensure_builtin_block_icons_extracted,
            assets::ensure_builtin_item_icons_extracted,
            assets::delete_user_config_file,
            assets::pick_litematic_file,
            assets::download_vault_block_icons,
            assets::download_vault_item_icons,
            assets::download_minecraft_wiki_enum_catalogs,
            assets::read_game_resource_remote_text,
            assets::read_resource_pack_language,
            assets::game_data_list,
            assets::game_data_context,
            assets::game_data_language,
            assets::read_game_data_icon,
            assets::game_data_import,
            assets::game_data_select,
            assets::game_data_default,
            wiki_block_icons::download_minecraft_wiki_block_icons,
            open_enumerator_window,
            open_asset_manager_window,
            open_iris_options_window,
            close_iris_options_window,
            application_get_close_request,
            application_confirm_close,
            application_request_close,
            shell_integration::application_take_open_files,
            online_sources::application_online_search,
            online_sources::application_online_detail,
            online_sources::application_online_download,
            online_sources::application_online_cancel,
            application_update_snapshot,
            application_update_check,
            application_update_download,
            application_update_install,
            application_update_cancel,
            application_update_skip,
            application_update_restart,
            application_inspect_project,
            application_read_layer_bounds,
            application_edit_project_metadata,
            application_read_layer_meta,
            application_read_layer,
            application_read_layer_container,
            application_read_project_markers,
            application_edit_project_origin,
            application_replace_blocks,
            application_export_world,
            application_world_export_profiles,
            application_world_export_status,
            application_convert_project_version,
            application_query_block_database,
            application_get_project_stats,
            application_export_material_list,
            application_prepare_material_recipes,
            application_get_project_materials,
            application_stockpile_task,
            application_read_material_detail,
            application_data_query,
            application_data_operation_status,
            application_stockpile_task_set,
            application_stockpile_task_command,
            application_stockpile_task_reset,
            application_prepare_material_task_window,
            application_generate_project,
            application_runtime_health,
            application_read_embedded_preview,
            render_preview_image,
            generate_preview_image,
            application_quick_preview_start,
            application_set_scene_view,
            iris_shaderpack_settings,
            application_quick_preview_resource_decision,
            application_quick_preview_snapshot,
            application_quick_preview_present,
            application_quick_preview_close,
            application_render_preflight,
            application_render_launch_plan,
            application_render_use_basic_vulkan,
            application_render_start,
            application_render_discard_preflight,
            application_render_cancel,
            application_render_update_presentation,
            application_render_get_snapshot,
            get_user_config,
            save_user_config,
            choose_user_config_dir,
            open_user_config_dir,
            set_user_config_dir,
            reset_user_config_dir,
            read_user_config_file,
            write_user_config_file,
            get_user_config_file_path,
            read_file_string,
            write_file_string,
            write_text_file_absolute,
            check_file_exists,
            list_litematic_files_in_directory,
            list_directory_entries,
            list_litematic_file_entries_in_directory,
            get_workspace_root,
            get_path_info,
            read_image_base64,
            open_file_parent_dir,
            copy_file_to_directory,
            open_workspace_path,
            cleanup_local_temp_files,
            reden_search_litematica,
            reden_machine_detail,
            reden_download_attachment,
            reden_download_parametric,
            ai_get_config,
            ai_save_config,
            ai_clear_key,
            ai_test_connection,
            ai_chat_completion,
            open_material_list_window,
            open_reden_library_window,
            open_local_library_folders_window,
            open_ui_demo_window
        ])
        .build(application_context)
        .expect("error while building tauri application")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = event {
                // Finder open requests use the same queued workflow as startup
                // arguments; the UI still owns draft/unsaved-content decisions.
                let files = urls.iter().filter(|url| url.scheme() == "file"
                    && url.host_str().is_none_or(|host| host == "localhost"))
                    .filter_map(|url| url.to_file_path().ok())
                    .map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>();
                shell_integration::queue_open_files(app, files.iter().map(String::as_str), Path::new("/"));
                if !files.is_empty() {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.unminimize(); let _ = window.show(); let _ = window.set_focus();
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
