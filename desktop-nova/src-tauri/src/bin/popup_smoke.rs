//! Real, headless-control Gate B smoke for the production render-service path.
//! This is a test driver only: all build, process, IPC and presentation work is
//! performed by the same production components as the desktop application.

#[path = "../platform/mod.rs"]
mod platform;
#[path = "../render_service_client.rs"]
mod render_service_client;
#[path = "../viewer_backend.rs"]
mod viewer_backend;
#[path = "../viewer_compatibility.rs"]
mod viewer_compatibility;

use nova_render_protocol::{
    AdmittedBuildStart, BuildAdmission, BuildStart, BuildState, PresentationSpec, RenderBackend,
    RenderBuildMode, ResourcePressure, ResponseBody, SceneContent, ServiceEvent, UiCommand,
    ViewerPresentationState, ViewerRuntimeState,
};
use platform::process_supervisor::{
    ChildLease, ExecutableAllowlist, ExecutableRole, ProcessEvent, ProcessExit, ProcessSupervisor,
    WaitOutcome,
};
use render_service_client::RenderServiceClient;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use viewer_backend::ViewerBackendIdentity;
use viewer_compatibility::{CompatibilityScope, V5CompatibilityConfig, ViewerCompatibilityInputs};
use viewer_launch_contract::{IrisFallback, ViewerBackend, WeatherPrecipitation};

const REAL_PROJECTION_ROOT: &str = r"C:\Users\27232\Documents\投影测试";
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const HOLD_DURATION: Duration = Duration::from_secs(5);
const BACKEND_BINARIES: [(ExecutableRole, &str); 4] = [
    (ExecutableRole::RenderService, "nova-render-service.exe"),
    (ExecutableRole::BuildWorker, "nova-build-worker.exe"),
    (ExecutableRole::NativeViewer, "litematica_native_viewer.exe"),
    (ExecutableRole::CoreHelper, "litematica_core.exe"),
];

#[derive(Deserialize)]
struct BackendDeployment {
    schema: String,
    profile: String,
    artifacts: BTreeMap<String, BackendArtifact>,
}

#[derive(Deserialize)]
struct BackendArtifact {
    bytes: u64,
    sha256: String,
}

#[derive(Debug)]
struct Args {
    projection: PathBuf,
    backend_dir: PathBuf,
    config: PathBuf,
    mode: RenderBuildMode,
    confirm_warning: bool,
    prepare_timeout: Duration,
    visible_timeout: Duration,
    data_root: PathBuf,
}

#[derive(Deserialize)]
struct ProductConfig {
    #[serde(default)]
    v5_compatibility: V5CompatibilityConfig,
    #[serde(default)]
    viewer_controls: ViewerControls,
}

#[derive(Deserialize)]
struct ViewerControls {
    #[serde(default = "enabled")]
    allow_time_modification: bool,
    #[serde(default = "enabled")]
    allow_weather_modification: bool,
}

impl Default for ViewerControls {
    fn default() -> Self {
        Self {
            allow_time_modification: true,
            allow_weather_modification: true,
        }
    }
}

fn enabled() -> bool {
    true
}

#[derive(Default)]
struct Report {
    build_id: Option<String>,
    session_id: Option<String>,
    viewer_pid: Option<u32>,
    viewer_exit: Option<ProcessExit>,
    last_fatal_panic: Option<String>,
    last_viewer_log_line: Option<String>,
    disconnect_reason: Option<String>,
    viewer_prepared_ms: Option<u128>,
    first_popup_first_frame_ms: Option<u128>,
    second_popup_first_frame_ms: Option<u128>,
    visible_first_frame_log_count: usize,
    cleanup_orphans: Option<usize>,
    cleanup_forced: Option<usize>,
}

impl Report {
    fn print(&self) {
        println!("build_id={}", self.build_id.as_deref().unwrap_or("none"));
        println!(
            "viewer_session_id={}",
            self.session_id.as_deref().unwrap_or("none")
        );
        println!("viewer_pid={}", option_number(self.viewer_pid));
        println!(
            "viewer_exit_code={}",
            option_number(self.viewer_exit.as_ref().and_then(|exit| exit.exit_code))
        );
        println!(
            "viewer_termination_requested={}",
            self.viewer_exit
                .as_ref()
                .map(|exit| exit.termination_requested.to_string())
                .unwrap_or_else(|| "none".to_owned())
        );
        println!(
            "viewer_wait_error={}",
            self.viewer_exit
                .as_ref()
                .and_then(|exit| exit.wait_error.as_deref())
                .unwrap_or("none")
        );
        println!(
            "last_fatal_panic={}",
            self.last_fatal_panic.as_deref().unwrap_or("none")
        );
        println!(
            "last_viewer_log_line={}",
            self.last_viewer_log_line.as_deref().unwrap_or("none")
        );
        println!(
            "render_service_disconnect_reason={}",
            self.disconnect_reason.as_deref().unwrap_or("none")
        );
        println!(
            "viewer_prepared_ms={}",
            option_number(self.viewer_prepared_ms)
        );
        println!(
            "first_popup_first_frame_ms={}",
            option_number(self.first_popup_first_frame_ms)
        );
        println!(
            "second_popup_first_frame_ms={}",
            option_number(self.second_popup_first_frame_ms)
        );
        println!(
            "visible_first_frame_log_count={}",
            self.visible_first_frame_log_count
        );
        println!("cleanup_orphans={}", option_number(self.cleanup_orphans));
        println!("cleanup_forced={}", option_number(self.cleanup_forced));
    }
}

fn option_number<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "none".to_owned())
}

struct LogTail {
    path: PathBuf,
    offset: u64,
    partial: String,
}

impl LogTail {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: String::new(),
        }
    }

    fn scan(&mut self, report: &mut Report) -> Result<(), String> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("open log {}: {error}", self.path.display())),
        };
        let length = file
            .metadata()
            .map_err(|error| format!("stat log {}: {error}", self.path.display()))?
            .len();
        if length < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|error| format!("seek log {}: {error}", self.path.display()))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("read log {}: {error}", self.path.display()))?;
        self.offset += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(newline) = self.partial.find('\n') {
            let line = self.partial[..newline].trim_end_matches('\r').to_owned();
            self.partial.drain(..=newline);
            if !line.is_empty() {
                report.last_viewer_log_line = Some(line.clone());
            }
            let lower = line.to_ascii_lowercase();
            if lower.contains("panicked at")
                || lower.contains("fatal")
                || lower.contains("assertion failed")
                || lower.contains("[assert]")
            {
                report.last_fatal_panic = Some(line.clone());
            }
            if line.contains("stage=first_visible_frame") {
                report.visible_first_frame_log_count += 1;
            }
        }
        if self.partial.len() > 65_536 {
            self.partial.drain(..self.partial.len() - 65_536);
        }
        Ok(())
    }
}

struct Harness {
    client: RenderServiceClient,
    supervisor: Arc<ProcessSupervisor>,
    events: mpsc::Receiver<ServiceEvent>,
    exits: mpsc::Receiver<ProcessEvent>,
    stdout: Option<LogTail>,
    stderr: Option<LogTail>,
    viewer_lease: Option<ChildLease>,
    runtime_root: PathBuf,
    report: Report,
    shutdown_started: bool,
}

impl Harness {
    fn launch(args: &Args) -> Result<Self, String> {
        let mut allowlist = ExecutableAllowlist::new();
        for (role, file_name) in BACKEND_BINARIES {
            allowlist
                .allow(role, args.backend_dir.join(file_name))
                .map_err(|error| format!("allow {file_name}: {error}"))?;
        }
        let (supervisor, process_events) = ProcessSupervisor::new(allowlist)
            .map_err(|error| format!("start production process supervisor: {error}"))?;
        let supervisor = Arc::new(supervisor);
        // The production client remains the owner of HostChildExited forwarding.
        // The second read-only fanout gives the smoke exact OS exit evidence.
        let (client_events_tx, client_events_rx) = mpsc::channel();
        let (smoke_events_tx, smoke_events_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("popup-smoke-exit-fanout".to_owned())
            .spawn(move || {
                while let Ok(event) = process_events.recv() {
                    let _ = client_events_tx.send(event.clone());
                    let _ = smoke_events_tx.send(event);
                }
            })
            .map_err(|error| format!("start process exit observer: {error}"))?;
        let client = RenderServiceClient::default();
        let events = client.subscribe_events();
        client.observe_process_events(client_events_rx);
        client.launch_in_background(
            Arc::clone(&supervisor),
            args.backend_dir.join("nova-render-service.exe"),
            args.data_root.clone(),
        );
        Ok(Self {
            client,
            supervisor,
            events,
            exits: smoke_events_rx,
            stdout: None,
            stderr: None,
            viewer_lease: None,
            runtime_root: args.data_root.join("render-service"),
            report: Report::default(),
            shutdown_started: false,
        })
    }

    fn request(&self, command: UiCommand, timeout: Duration) -> Result<ResponseBody, String> {
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(timeout, self.client.request(command))
                .await
                .map_err(|_| format!("render-service request timed out after {timeout:?}"))?
        })
    }

    fn exercise(&mut self, args: &Args, build: BuildStart) -> Result<(), String> {
        let admission = if args.mode == RenderBuildMode::Prebuilt {
            let preflight = match self.request(
                UiCommand::PreflightBuild(build.clone()),
                args.prepare_timeout,
            )? {
                ResponseBody::BuildPreflight(value) => value,
                other => return Err(format!("unexpected preflight response: {other:?}")),
            };
            println!(
                "preflight_pressure={:?} conservative_vram_bytes={} safe_available_vram_bytes={}",
                preflight.pressure,
                option_number(preflight.conservative_vram_bytes),
                option_number(preflight.safe_available_vram_bytes)
            );
            if matches!(
                preflight.pressure,
                ResourcePressure::HardLimit | ResourcePressure::Invalid
            ) {
                return Err(format!(
                    "prebuilt resource preflight rejected this projection: {:?} {}",
                    preflight.pressure,
                    preflight.invalid_reason.as_deref().unwrap_or("")
                ));
            }
            if preflight.pressure == ResourcePressure::Warning && !args.confirm_warning {
                return Err("prebuilt resource warning requires --confirm-warning".to_owned());
            }
            Some(BuildAdmission {
                token: preflight
                    .admission_token
                    .ok_or_else(|| "preflight returned no admission token".to_owned())?,
                resource_warning_confirmed: preflight.pressure == ResourcePressure::Warning,
            })
        } else {
            None
        };

        // Prebuilt resource preflight launches a separate short-lived Viewer
        // probe. Its successful exit is not the interactive Viewer under test.
        // The preflight response is sent only after that probe has exited.
        while let Ok(ProcessEvent::Exited(exit)) = self.exits.try_recv() {
            if exit.child.role == ExecutableRole::NativeViewer {
                if exit.exit_code != Some(0)
                    || exit.termination_requested
                    || exit.wait_error.is_some()
                {
                    return Err(format!(
                        "resource probe did not exit cleanly: code={:?} forced={} wait_error={:?}",
                        exit.exit_code, exit.termination_requested, exit.wait_error
                    ));
                }
                println!("resource_probe_exit_code=0");
            } else if exit.child.role == ExecutableRole::RenderService {
                return Err(format!(
                    "render service exited before Build: code={:?}",
                    exit.exit_code
                ));
            }
        }

        let started = Instant::now();
        let accepted = match self.request(
            UiCommand::StartBuild(AdmittedBuildStart { build, admission }),
            args.prepare_timeout,
        )? {
            ResponseBody::BuildAccepted(value) => value,
            other => return Err(format!("unexpected build response: {other:?}")),
        };
        self.report.build_id = Some(accepted.build_id.clone());
        self.report.session_id = accepted.viewer_session_id;
        println!(
            "build_accepted generation={} attached={}",
            accepted.generation, accepted.attached
        );

        self.wait_for(args.prepare_timeout, "ViewerPrepared", |this| {
            let snapshot = this.client.snapshot();
            let viewer = snapshot.viewers.iter().find(|viewer| {
                viewer.build_id == accepted.build_id && viewer.generation == accepted.generation
            });
            let Some(viewer) = viewer else {
                return Ok(false);
            };
            if let Some(expected) = this.report.session_id.as_deref() {
                if viewer.session_id != expected {
                    return Err(format!(
                        "Viewer session changed {expected} -> {}",
                        viewer.session_id
                    ));
                }
            } else {
                this.report.session_id = Some(viewer.session_id.clone());
            }
            if viewer.runtime_state == ViewerRuntimeState::ViewerPrepared {
                let pid = viewer
                    .pid
                    .ok_or_else(|| "ViewerPrepared has no Viewer pid".to_owned())?;
                this.report.viewer_pid = Some(pid);
                this.report.viewer_prepared_ms = Some(started.elapsed().as_millis());
                this.viewer_lease = Some(
                    this.supervisor
                        .active_children()
                        .into_iter()
                        .find(|child| {
                            child.role == ExecutableRole::NativeViewer && child.pid == pid
                        })
                        .ok_or_else(|| {
                            format!("ViewerPrepared pid {pid} is not supervised/alive")
                        })?,
                );
                return Ok(true);
            }
            Ok(false)
        })?;

        self.show_popup(args.visible_timeout, false)?;
        self.hold_alive(HOLD_DURATION)?;
        let session_id = self.session_id()?.to_owned();
        self.expect_ack(UiCommand::HideViewer { session_id }, args.visible_timeout)?;
        self.wait_for(args.visible_timeout, "Hidden", |this| {
            Ok(this.viewer()?.presentation_state == ViewerPresentationState::Hidden)
        })?;
        self.show_popup(args.visible_timeout, true)?;
        self.hold_alive(HOLD_DURATION)?;
        Ok(())
    }

    fn show_popup(&mut self, timeout: Duration, second: bool) -> Result<(), String> {
        let session_id = self.session_id()?.to_owned();
        let started = Instant::now();
        self.expect_ack(UiCommand::ShowPopup { session_id }, timeout)?;
        self.wait_for(
            timeout,
            if second {
                "second PresentationVisible/FirstFrame"
            } else {
                "first PresentationVisible/FirstFrame"
            },
            |this| Ok(this.viewer()?.presentation_state == ViewerPresentationState::PopupVisible),
        )?;
        let elapsed = started.elapsed().as_millis();
        if second {
            self.report.second_popup_first_frame_ms = Some(elapsed);
        } else {
            self.report.first_popup_first_frame_ms = Some(elapsed);
        }
        println!(
            "popup_{}_presentation_visible_first_frame_ms={elapsed}",
            if second { "second" } else { "first" }
        );
        Ok(())
    }

    fn expect_ack(&self, command: UiCommand, timeout: Duration) -> Result<(), String> {
        match self.request(command, timeout)? {
            ResponseBody::Ack => Ok(()),
            other => Err(format!("expected Ack, got {other:?}")),
        }
    }

    fn session_id(&self) -> Result<&str, String> {
        self.report
            .session_id
            .as_deref()
            .ok_or_else(|| "no Viewer session".to_owned())
    }

    fn viewer(&self) -> Result<nova_render_protocol::ViewerSnapshot, String> {
        let session_id = self.session_id()?;
        self.client
            .snapshot()
            .viewers
            .into_iter()
            .find(|viewer| viewer.session_id == session_id)
            .ok_or_else(|| format!("render service lost Viewer {session_id}"))
    }

    fn wait_for(
        &mut self,
        timeout: Duration,
        milestone: &str,
        mut condition: impl FnMut(&mut Self) -> Result<bool, String>,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            self.watch()?;
            if condition(self)? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!("{milestone} timeout after {timeout:?}"));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn hold_alive(&mut self, duration: Duration) -> Result<(), String> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.watch()?;
            if self.viewer()?.presentation_state != ViewerPresentationState::PopupVisible {
                return Err("Popup stopped being visible during 5-second hold".to_owned());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        self.watch()
    }

    fn watch(&mut self) -> Result<(), String> {
        while let Ok(ProcessEvent::Exited(exit)) = self.exits.try_recv() {
            if exit.child.role == ExecutableRole::NativeViewer {
                if self.report.viewer_pid.is_none()
                    || self.report.viewer_pid == Some(exit.child.pid)
                {
                    self.report.viewer_exit = Some(exit.clone());
                }
                if !self.shutdown_started {
                    return Err(format!(
                        "Viewer exited before graceful shutdown: pid={} code={:?} wait_error={:?}",
                        exit.child.pid, exit.exit_code, exit.wait_error
                    ));
                }
            } else if exit.child.role == ExecutableRole::RenderService && !self.shutdown_started {
                self.report.disconnect_reason = Some(format!(
                    "render service exited: code={:?} wait_error={:?}",
                    exit.exit_code, exit.wait_error
                ));
                return Err(self.report.disconnect_reason.clone().unwrap());
            }
        }
        while let Ok(event) = self.events.try_recv() {
            match event {
                ServiceEvent::Error(error) if !self.shutdown_started => {
                    if error.code.contains("disconnect") || error.code.contains("viewer_exited") {
                        self.report.disconnect_reason =
                            Some(format!("{}: {}", error.code, error.message));
                        return Err(format!(
                            "render service lost Viewer: {}",
                            self.report.disconnect_reason.as_deref().unwrap()
                        ));
                    }
                    if error.fatal {
                        return Err(format!(
                            "render service fatal {}: {}",
                            error.code, error.message
                        ));
                    }
                }
                ServiceEvent::ViewerState(viewer) => {
                    if Some(viewer.session_id.as_str()) == self.report.session_id.as_deref()
                        && viewer.runtime_state == ViewerRuntimeState::Failed
                    {
                        if let Some(error) = viewer.error.as_ref() {
                            self.report.disconnect_reason =
                                Some(format!("{}: {}", error.code, error.message));
                        }
                        if !self.shutdown_started {
                            return Err(format!(
                                "render service lost Viewer {}: {:?}",
                                viewer.session_id, viewer.error
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        self.scan_logs()?;
        if let Some(line) = &self.report.last_fatal_panic {
            if !self.shutdown_started {
                return Err(format!("Viewer fatal/panic/assert: {line}"));
            }
        }
        if let Some(build_id) = self.report.build_id.as_deref() {
            let snapshot = self.client.snapshot();
            if let Some(build) = snapshot
                .builds
                .iter()
                .find(|build| build.build_id == build_id)
            {
                if matches!(build.state, BuildState::Failed | BuildState::Cancelled) {
                    return Err(format!("Build failed: {:?} {:?}", build.state, build.error));
                }
            }
            if let Some(expected_pid) = self.report.viewer_pid {
                let viewer = self.viewer()?;
                if viewer.pid != Some(expected_pid) {
                    return Err(format!(
                        "Viewer pid changed {:?} -> {:?}",
                        expected_pid, viewer.pid
                    ));
                }
                if !self.supervisor.active_children().iter().any(|child| {
                    child.role == ExecutableRole::NativeViewer && child.pid == expected_pid
                }) {
                    return Err(format!("Viewer pid {expected_pid} is no longer alive"));
                }
                if snapshot.viewers.iter().any(|other| {
                    other.build_id == build_id
                        && Some(other.session_id.as_str()) != self.report.session_id.as_deref()
                        && other.runtime_state != ViewerRuntimeState::Released
                }) {
                    return Err("Hide/Show rebuilt the Viewer session".to_owned());
                }
            }
        }
        Ok(())
    }

    fn scan_logs(&mut self) -> Result<(), String> {
        if self.stdout.is_none() {
            if let Some(session_id) = self.report.session_id.as_deref() {
                let logs = self.runtime_root.join("logs");
                self.stdout = Some(LogTail::new(
                    logs.join(format!("{session_id}-viewer-stdout.log")),
                ));
                self.stderr = Some(LogTail::new(
                    logs.join(format!("{session_id}-viewer-stderr.log")),
                ));
            }
        }
        if let Some(stdout) = self.stdout.as_mut() {
            stdout.scan(&mut self.report)?;
        }
        if let Some(stderr) = self.stderr.as_mut() {
            stderr.scan(&mut self.report)?;
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.shutdown_started = true;
        self.supervisor.stop_admitting();
        self.client.request_shutdown();
        if let Some(lease) = self.viewer_lease.as_ref() {
            if let WaitOutcome::Exited(exit) = self
                .supervisor
                .wait_for_exit(lease, Duration::from_secs(15))
            {
                self.report.viewer_exit = Some(exit);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.supervisor.active_children().is_empty() && Instant::now() < deadline {
            let _ = self.scan_logs();
            self.capture_disconnect_events();
            while let Ok(ProcessEvent::Exited(exit)) = self.exits.try_recv() {
                if exit.child.role == ExecutableRole::NativeViewer
                    && (self.report.viewer_pid.is_none()
                        || self.report.viewer_pid == Some(exit.child.pid))
                {
                    self.report.viewer_exit = Some(exit);
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        let shutdown = self
            .supervisor
            .shutdown(Duration::from_secs(3), Duration::from_secs(2));
        self.report.cleanup_orphans = Some(shutdown.remaining.len());
        self.report.cleanup_forced = Some(shutdown.terminate_requests);
        while let Ok(ProcessEvent::Exited(exit)) = self.exits.try_recv() {
            if exit.child.role == ExecutableRole::NativeViewer
                && (self.report.viewer_pid.is_none()
                    || self.report.viewer_pid == Some(exit.child.pid))
            {
                self.report.viewer_exit = Some(exit);
            }
        }
        self.capture_disconnect_events();
        self.scan_logs()?;
        if !shutdown.remaining.is_empty() || shutdown.terminate_requests != 0 {
            return Err(format!(
                "cleanup orphan/forced: remaining={} terminate_requests={}",
                shutdown.remaining.len(),
                shutdown.terminate_requests
            ));
        }
        if self.report.viewer_pid.is_some() {
            let exit = self
                .report
                .viewer_exit
                .as_ref()
                .ok_or_else(|| "Viewer exit code missing".to_owned())?;
            if exit.exit_code != Some(0) || exit.termination_requested || exit.wait_error.is_some()
            {
                return Err(format!(
                    "Viewer did not exit gracefully: code={:?} forced={} wait_error={:?}",
                    exit.exit_code, exit.termination_requested, exit.wait_error
                ));
            }
        }
        Ok(())
    }

    fn capture_disconnect_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                ServiceEvent::Error(error)
                    if error.code == "viewer_disconnected" || error.code == "viewer_exited" =>
                {
                    self.report.disconnect_reason =
                        Some(format!("{}: {}", error.code, error.message));
                }
                ServiceEvent::ViewerState(viewer)
                    if Some(viewer.session_id.as_str()) == self.report.session_id.as_deref() =>
                {
                    if let Some(error) = viewer.error {
                        self.report.disconnect_reason =
                            Some(format!("{}: {}", error.code, error.message));
                    }
                }
                _ => {}
            }
        }
        if self.report.disconnect_reason.is_none() {
            if let Ok(viewer) = self.viewer() {
                if let Some(error) = viewer.error {
                    self.report.disconnect_reason =
                        Some(format!("{}: {}", error.code, error.message));
                }
            }
        }
    }
}

fn build_start(args: &Args) -> Result<BuildStart, String> {
    let text = fs::read_to_string(&args.config)
        .map_err(|error| format!("read product config {}: {error}", args.config.display()))?;
    let config: ProductConfig = serde_json::from_str(&text)
        .map_err(|error| format!("parse product config {}: {error}", args.config.display()))?;
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "cannot resolve workspace root".to_owned())?;
    let compatibility = ViewerCompatibilityInputs::resolve(
        &config.v5_compatibility,
        workspace_root,
        CompatibilityScope::Render,
    )?;
    let renderer = compatibility
        .renderer()
        .ok_or_else(|| "no production renderer".to_owned())?;
    let backend = match renderer.backend {
        ViewerBackend::Auto => RenderBackend::Auto,
        ViewerBackend::Vulkan => RenderBackend::Vulkan,
        ViewerBackend::IrisGl46 => RenderBackend::IrisGl46,
    };
    let iris_fallback = match renderer.iris_fallback {
        IrisFallback::Fail => "fail",
        IrisFallback::Vulkan => "vulkan",
    };
    let precipitation = match renderer.precipitation {
        WeatherPrecipitation::Auto => "auto",
        WeatherPrecipitation::Rain => "rain",
        WeatherPrecipitation::Snow => "snow",
        WeatherPrecipitation::None => "none",
    };
    let content = compatibility.content();
    let asset_inputs = [
        workspace_root
            .join("mc26-oracle")
            .join("verified-schema3-26.3"),
        workspace_root
            .join("tools")
            .join("mc26-oracle")
            .join("build")
            .join("verified-schema3-26.3"),
        args.backend_dir.join("backend-deployment.json"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect();
    let display_mode = "full".to_owned();
    Ok(BuildStart {
        source_path: args.projection.clone(),
        display_mode: display_mode.clone(),
        build_mode: args.mode,
        content: SceneContent {
            fabric_mods_dir: content.fabric_mods_dir.clone(),
            resource_packs: content.resource_packs.clone(),
            asset_inputs,
        },
        build_options: BTreeMap::from([("displayMode".to_owned(), display_mode)]),
        presentation: PresentationSpec {
            backend,
            shaderpack: renderer
                .shaderpack
                .as_ref()
                .map(|shaderpack| shaderpack.path.clone()),
            render_settings: BTreeMap::from([
                ("irisFallback".to_owned(), iris_fallback.to_owned()),
                ("precipitation".to_owned(), precipitation.to_owned()),
                (
                    "allowTimeModification".to_owned(),
                    config.viewer_controls.allow_time_modification.to_string(),
                ),
                (
                    "allowWeatherModification".to_owned(),
                    config
                        .viewer_controls
                        .allow_weather_modification
                        .to_string(),
                ),
            ]),
        },
    })
}

fn parse_args() -> Result<Args, String> {
    let mut projection = None;
    let mut backend_dir = None;
    let mut config = None;
    let mut mode = None;
    let mut confirm_warning = false;
    let mut prepare_timeout = Duration::from_secs(900);
    let mut visible_timeout = Duration::from_secs(120);
    let mut data_root = None;
    for arg in std::env::args().skip(1) {
        if arg == "--confirm-warning" {
            confirm_warning = true;
            continue;
        }
        let (key, value) = arg
            .split_once('=')
            .ok_or_else(|| format!("expected --key=value, got {arg}"))?;
        match key {
            "--projection" => projection = Some(PathBuf::from(value)),
            "--backend-dir" => backend_dir = Some(PathBuf::from(value)),
            "--config" => config = Some(PathBuf::from(value)),
            "--mode" => {
                mode = Some(match value {
                    "prebuilt" => RenderBuildMode::Prebuilt,
                    "realtime" => RenderBuildMode::Realtime,
                    _ => return Err(format!("--mode must be prebuilt or realtime, got {value}")),
                })
            }
            "--prepare-timeout-secs" => {
                prepare_timeout =
                    Duration::from_secs(value.parse().map_err(|_| "invalid prepare timeout")?)
            }
            "--visible-timeout-secs" => {
                visible_timeout =
                    Duration::from_secs(value.parse().map_err(|_| "invalid visible timeout")?)
            }
            "--data-root" => data_root = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown option {key}")),
        }
    }
    let root = fs::canonicalize(REAL_PROJECTION_ROOT)
        .map_err(|error| format!("open real projection directory: {error}"))?;
    let projection = fs::canonicalize(projection.ok_or("missing --projection")?)
        .map_err(|error| format!("open real projection: {error}"))?;
    if !projection.starts_with(&root)
        || !projection.is_file()
        || !projection
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("litematic"))
    {
        return Err(format!(
            "projection must be a real .litematic under {}",
            root.display()
        ));
    }
    let backend_dir = fs::canonicalize(backend_dir.ok_or("missing --backend-dir")?)
        .map_err(|error| format!("open Release backend directory: {error}"))?;
    verify_release_backends(&backend_dir)?;
    let config = fs::canonicalize(config.ok_or("missing --config")?)
        .map_err(|error| format!("open product config: {error}"))?;
    let mode = mode.ok_or("missing --mode=prebuilt|realtime")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    let data_root = data_root.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("nova-popup-smoke-{}-{now}", std::process::id()))
    });
    if !data_root.is_absolute() || data_root.exists() {
        return Err(format!(
            "--data-root must be an unused absolute directory: {}",
            data_root.display()
        ));
    }
    fs::create_dir_all(&data_root).map_err(|error| format!("create smoke data root: {error}"))?;
    Ok(Args {
        projection,
        backend_dir,
        config,
        mode,
        confirm_warning,
        prepare_timeout,
        visible_timeout,
        data_root,
    })
}

fn verify_release_backends(backend_dir: &Path) -> Result<(), String> {
    let path = backend_dir.join("backend-deployment.json");
    let deployment: BackendDeployment =
        serde_json::from_slice(&fs::read(&path).map_err(|error| {
            format!("read Release backend manifest {}: {error}", path.display())
        })?)
        .map_err(|error| format!("parse Release backend manifest {}: {error}", path.display()))?;
    if deployment.schema != "lba.backend.deployment.v1" || deployment.profile != "release" {
        return Err(format!(
            "backend manifest must declare release profile: {}",
            path.display()
        ));
    }
    for (_, name) in BACKEND_BINARIES {
        let expected = deployment
            .artifacts
            .get(name)
            .ok_or_else(|| format!("backend manifest is missing {name}"))?;
        let actual = ViewerBackendIdentity::capture(&backend_dir.join(name))?;
        if actual.bytes() != expected.bytes
            || !actual.sha256().eq_ignore_ascii_case(&expected.sha256)
        {
            return Err(format!(
                "Release backend hash/size mismatch for {name}: actual bytes={} sha256={}",
                actual.bytes(),
                actual.sha256()
            ));
        }
        println!(
            "release_backend={} bytes={} sha256={}",
            name,
            actual.bytes(),
            actual.sha256()
        );
    }
    Ok(())
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("popup_smoke=FAIL reason={error}");
            std::process::exit(2);
        }
    };
    println!("projection={}", args.projection.display());
    println!("backend_dir={}", args.backend_dir.display());
    println!("data_root={}", args.data_root.display());
    println!("mode={:?}", args.mode);
    let build = match build_start(&args) {
        Ok(build) => build,
        Err(error) => {
            eprintln!("popup_smoke=FAIL reason={error}");
            std::process::exit(2);
        }
    };
    let mut harness = match Harness::launch(&args) {
        Ok(harness) => harness,
        Err(error) => {
            eprintln!("popup_smoke=FAIL reason={error}");
            std::process::exit(2);
        }
    };
    let run = harness.exercise(&args, build);
    let cleanup = harness.shutdown();
    harness.report.print();
    match (run, cleanup) {
        (Ok(()), Ok(())) => println!("popup_smoke=PASS"),
        (run, cleanup) => {
            eprintln!("popup_smoke=FAIL run={run:?} cleanup={cleanup:?}");
            std::process::exit(1);
        }
    }
}
