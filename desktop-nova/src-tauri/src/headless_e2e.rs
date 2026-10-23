//! Product E2E entry point of the *same* Release Application executable.
//! No Tauri Builder/WebView or second test-only rendering API is involved.

use super::{
    APP_LOCAL_DATA_ROOT, APP_RESOURCE_ROOT, ApplicationHostComponents,
    RenderServiceStartBuildInput, application_render_request, initialize_application_host,
    run_application_operation,
};
use crate::application::model::{
    ApplicationRenderSnapshot, OperationId, OperationState, PresentationMode, PresentationState,
    ProjectSource, RenderBuildMode, RenderOperationView, ResourcePreflightAction,
    ResourcePreflightSeverity,
};
use crate::application::operations::InspectProjectInput;
use crate::application::render_facade::{RenderFacade, RenderStartAuthorization};
use crate::render_service_client::RenderServiceClient;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEPLOYMENT_SCHEMA: &str = "lba.backend.deployment.v1";
const REPORT_SCHEMA: &str = "nova.headless-e2e.v1";
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(30);
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(180);
const ACTION_TIMEOUT: Duration = Duration::from_secs(240);
const BUILD_TIMEOUT: Duration = Duration::from_secs(40 * 60);
const HOLD_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
struct Options {
    projection: PathBuf,
    backend_dir: PathBuf,
    report: PathBuf,
    mode: RenderBuildMode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackendDeployment {
    schema: String,
    #[serde(rename = "gitSha")]
    git_sha: String,
    profile: String,
    artifacts: BTreeMap<String, DeployedArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeployedArtifact {
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Milestone {
    name: &'static str,
    attempt: Option<u8>,
    elapsed_ms: u128,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessExitRecord {
    role: String,
    pid: u32,
    exit_code: Option<i32>,
    termination_requested: bool,
    wait_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema: &'static str,
    build_git_sha: &'static str,
    backend_manifest_git_sha: Option<String>,
    passed: bool,
    projection: Option<PathBuf>,
    mode: Option<&'static str>,
    backend_dir: Option<PathBuf>,
    data_root: Option<PathBuf>,
    viewer_log_dir: Option<PathBuf>,
    viewer_logs: Vec<PathBuf>,
    render_service_stdout_log: Option<PathBuf>,
    render_service_stderr_log: Option<PathBuf>,
    first_visible_frame_ordinals: Vec<u64>,
    run_started_unix_ms: u128,
    elapsed_ms: u128,
    milestones: Vec<Milestone>,
    timings_ms: BTreeMap<String, u128>,
    preflight_severity: Option<ResourcePreflightSeverity>,
    application_event_count: u64,
    camera_stress_frames: Option<u64>,
    early_compiled_sections: Option<u64>,
    early_existing_sections: Option<u64>,
    early_stored_sections: Option<u64>,
    early_exact_resident_sections: Option<u64>,
    final_operation: Option<RenderOperationView>,
    process_exits: Vec<ProcessExitRecord>,
    shutdown_terminate_requests: Option<usize>,
    shutdown_remaining: Option<usize>,
    diagnostic_failures: Vec<String>,
    error: Option<String>,
}

impl Report {
    fn new(started: SystemTime) -> Self {
        Self {
            schema: REPORT_SCHEMA,
            build_git_sha: env!("LBA_NOVA_GIT_SHA"),
            backend_manifest_git_sha: None,
            passed: false,
            projection: None,
            mode: None,
            backend_dir: None,
            data_root: None,
            viewer_log_dir: None,
            viewer_logs: Vec::new(),
            render_service_stdout_log: None,
            render_service_stderr_log: None,
            first_visible_frame_ordinals: Vec::new(),
            run_started_unix_ms: started
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0),
            elapsed_ms: 0,
            milestones: Vec::new(),
            timings_ms: BTreeMap::new(),
            preflight_severity: None,
            application_event_count: 0,
            camera_stress_frames: None,
            early_compiled_sections: None,
            early_existing_sections: None,
            early_stored_sections: None,
            early_exact_resident_sections: None,
            final_operation: None,
            process_exits: Vec::new(),
            shutdown_terminate_requests: None,
            shutdown_remaining: None,
            diagnostic_failures: Vec::new(),
            error: None,
        }
    }

    fn mark(&mut self, name: &'static str, attempt: Option<u8>, started: Instant) {
        self.milestones.push(Milestone {
            name,
            attempt,
            elapsed_ms: started.elapsed().as_millis(),
        });
    }

    fn measure(&mut self, name: &str, started: Instant) {
        self.timings_ms
            .insert(name.to_owned(), started.elapsed().as_millis());
    }
}

pub(crate) fn run(args: &[OsString]) -> i32 {
    let started = Instant::now();
    let started_at = SystemTime::now();
    let mut report = Report::new(started_at);
    let report_path = raw_report_path(args);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let options = parse_options(args)?;
        report.projection = Some(options.projection.clone());
        report.backend_dir = Some(options.backend_dir.clone());
        report.mode = Some(mode_name(options.mode));
        tauri::async_runtime::block_on(run_async(&options, &mut report, started))
    }));
    let failure = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(panic) => Some(format!(
            "Application E2E panic: {}",
            panic.downcast_ref::<String>().cloned().unwrap_or_else(|| {
                panic
                    .downcast_ref::<&str>()
                    .map(|value| (*value).to_owned())
                    .unwrap_or_else(|| "non-string panic payload".to_owned())
            })
        )),
    };
    report.error = failure;
    report.passed = report.error.is_none();
    report.elapsed_ms = started.elapsed().as_millis();
    let output = report_path.or_else(|| parse_options(args).ok().map(|options| options.report));
    match output {
        Some(path) if path.is_absolute() => {
            if let Err(error) = write_report(&path, &report) {
                eprintln!("headless E2E report {}: {error}", path.display());
                return 3;
            }
        }
        _ => return 2,
    }
    if report.passed { 0 } else { 1 }
}

async fn run_async(options: &Options, report: &mut Report, started: Instant) -> Result<(), String> {
    let backend_dir = fs::canonicalize(&options.backend_dir)
        .map_err(|error| format!("resolve backend directory: {error}"))?;
    let deployment = verify_backend_deployment(&backend_dir)?;
    report.backend_manifest_git_sha = Some(deployment.git_sha);
    let project_root = backend_dir
        .parent()
        .filter(|parent| parent.file_name() == Some(OsStr::new("bin")))
        .and_then(Path::parent)
        .ok_or_else(|| "backend directory must be <project-root>/bin/viewer-backend".to_owned())?
        .to_path_buf();
    if backend_dir.file_name() != Some(OsStr::new("viewer-backend")) {
        return Err("backend directory must be <project-root>/bin/viewer-backend".to_owned());
    }
    let projection = validate_projection(&options.projection)?;
    let local_app_data = env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            "LOCALAPPDATA is required for the production Application data root".to_owned()
        })?;
    // This matches Tauri setup's app.path().local_data_dir() followed by the
    // production product_local_data_root(), not app_local_data_dir().
    let data_root = super::product_local_data_root(&local_app_data);
    fs::create_dir_all(&data_root).map_err(|error| {
        format!(
            "create Application data root {}: {error}",
            data_root.display()
        )
    })?;
    report.data_root = Some(data_root.clone());
    let viewer_log_dir = data_root.join("render-service").join("logs");
    report.viewer_log_dir = Some(viewer_log_dir.clone());
    report.render_service_stdout_log = Some(viewer_log_dir.join("render-service-stdout.log"));
    report.render_service_stderr_log = Some(viewer_log_dir.join("render-service-stderr.log"));
    APP_RESOURCE_ROOT
        .set(project_root.clone())
        .map_err(|_| "Application resource root was already initialized".to_owned())?;
    APP_LOCAL_DATA_ROOT
        .set(data_root.clone())
        .map_err(|_| "Application data root was already initialized".to_owned())?;

    // ProcessSupervisor inherits this environment for the Render Service and
    // all hosted children. No top-level Viewer or WebView window is created.
    env::set_var("NOVA_HEADLESS_E2E", "1");
    let service = RenderServiceClient::default();
    let facade = RenderFacade::new(service.clone());
    let host = initialize_application_host(project_root, data_root.clone(), &service)?;
    let application_events = facade.start_headless_event_bridge()?;
    report.mark("ApplicationReady", None, started);
    service.launch_in_background(
        Arc::clone(&host.supervisor),
        host.service_exe.clone().ok_or_else(|| nova_protected_loader::FEATURE_NOT_INCLUDED.to_owned())?,
        data_root,
    );

    let pipeline = run_pipeline(
        options,
        &projection,
        &host,
        &service,
        &facade,
        &application_events,
        report,
        started,
    )
    .await;
    let active_before_shutdown = host.supervisor.active_children().len();
    let exits_before_shutdown = service.observed_process_exits().len();
    host.supervisor.stop_admitting();
    service.request_shutdown();
    // Exercise the same graceful/forced OS deadlines as the product exit path.
    let shutdown = host
        .supervisor
        .shutdown(Duration::from_secs(3), Duration::from_secs(2));
    let _ = service.wait_for_observed_process_exits(
        exits_before_shutdown + active_before_shutdown,
        Duration::from_secs(2),
    );
    report.shutdown_terminate_requests = Some(shutdown.terminate_requests);
    report.shutdown_remaining = Some(shutdown.remaining.len());
    report.process_exits = service
        .observed_process_exits()
        .into_iter()
        .map(|exit| ProcessExitRecord {
            role: format!("{:?}", exit.child.role),
            pid: exit.child.pid,
            exit_code: exit.exit_code,
            termination_requested: exit.termination_requested,
            wait_error: exit.wait_error,
        })
        .collect();
    if report.viewer_logs.is_empty() {
        if let Some(view) = report.final_operation.as_ref() {
            if let Ok(paths) = facade.headless_viewer_log_paths(&view.operation_id, &viewer_log_dir)
            {
                report.viewer_logs = paths.into_iter().collect();
            }
        }
    }
    let mut diagnostic_logs = report
        .viewer_logs
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect::<Vec<_>>();
    diagnostic_logs.extend(
        [
            report.render_service_stdout_log.as_ref(),
            report.render_service_stderr_log.as_ref(),
        ]
        .into_iter()
        .flatten()
        .filter(|path| path.is_file())
        .cloned(),
    );
    let mut failures = Vec::new();
    if let Err(error) = pipeline {
        failures.push(error);
    } else {
        if report.viewer_logs.len() != 2 || report.viewer_logs.iter().any(|path| !path.is_file()) {
            failures.push("exact Viewer stdout/stderr logs are missing".to_owned());
        } else {
            report.first_visible_frame_ordinals =
                read_first_visible_frame_ordinals(&report.viewer_logs)?;
            let expected_ordinals = if options.mode == RenderBuildMode::Realtime {
                vec![1, 2, 3]
            } else {
                vec![1, 2]
            };
            if report.first_visible_frame_ordinals != expected_ordinals {
                failures.push(format!(
                    "Viewer first-visible popup ordinals differ from {expected_ordinals:?}: {:?}",
                    report.first_visible_frame_ordinals
                ));
            }
            if let Err(error) = validate_runtime_contracts(&report.viewer_logs, options.mode) {
                failures.push(error);
            }
        }
    }
    match scan_diagnostics(&diagnostic_logs) {
        Ok(diagnostics) => report.diagnostic_failures = diagnostics,
        Err(error) => failures.push(format!("scan diagnostics: {error}")),
    }
    if shutdown.terminate_requests != 0 || !shutdown.remaining.is_empty() {
        failures.push(format!(
            "graceful shutdown failed: {} forced termination request(s), {} orphan handle(s)",
            shutdown.terminate_requests,
            shutdown.remaining.len()
        ));
    }
    if let Some(exit) = report
        .process_exits
        .iter()
        .find(|exit| exit.exit_code != Some(0) || exit.wait_error.is_some())
    {
        failures.push(format!(
            "child {} pid {} exited abnormally: {:?}, {:?}",
            exit.role, exit.pid, exit.exit_code, exit.wait_error
        ));
    }
    if let Some(diagnostic) = report.diagnostic_failures.first() {
        failures.push(format!("fatal diagnostic: {diagnostic}"));
    }
    if !failures.is_empty() {
        return Err(failures.join(" | "));
    }
    report.mark("Shutdown", None, started);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_pipeline(
    options: &Options,
    projection: &Path,
    host: &ApplicationHostComponents,
    service: &RenderServiceClient,
    facade: &RenderFacade,
    application_events: &mpsc::Receiver<ApplicationRenderSnapshot>,
    report: &mut Report,
    started: Instant,
) -> Result<(), String> {
    timed(
        "RenderServiceReady",
        SERVICE_START_TIMEOUT,
        report,
        service.wait_connected(),
    )
    .await?;
    report.mark("RenderServiceReady", None, started);

    let inspect_input = InspectProjectInput {
        source_path: projection.to_string_lossy().into_owned(),
    };
    let operations = host.operations.clone();
    let inspected = timed(
        "OpenProjection",
        ACTION_TIMEOUT,
        report,
        run_application_operation(move || operations.inspect_project(inspect_input)),
    )
    .await?;
    if !inspected.is_object() {
        return Err("Application inspect_project returned a non-object result".to_owned());
    }
    report.mark("OpenProjection", None, started);

    let preflight_input = RenderServiceStartBuildInput {
                layer_view: false, initial_layer_range: None,
        source_path: projection.to_path_buf(),
        build_mode: RenderBuildMode::Prebuilt,
        display_mode: crate::RenderDisplayMode::Full,
        preflight_id: None,
        resource_warning_confirmed: false,
        realtime_decision_id: None,
        click_started_at_epoch_ms: None,
        viewport: Some([1280, 720]),
    };
    let (preflight_request, preflight_environment) =
        timed("PrepareRenderRequest", ACTION_TIMEOUT, report, async move {
            run_application_operation(move || application_render_request(preflight_input)).await
        })
        .await?;
    let preflight = timed(
        "ResourcePreflight",
        PREFLIGHT_TIMEOUT,
        report,
        facade.preflight_render(preflight_request, preflight_environment),
    )
    .await?;
    report.preflight_severity = Some(preflight.severity);
    report.mark("ResourcePreflight", None, started);

    let operation = match options.mode {
        RenderBuildMode::Prebuilt => {
            if !preflight
                .allowed_actions
                .contains(&ResourcePreflightAction::ContinuePrebuilt)
            {
                return Err(format!(
                    "prebuilt resource preflight cannot continue: {:?}",
                    preflight.severity
                ));
            }
            timed(
                "BuildStart",
                ACTION_TIMEOUT,
                report,
                facade.start_prebuilt_render(
                    ProjectSource {
                        path: projection.to_path_buf(),
                    },
                    RenderStartAuthorization {
                        preflight_id: Some(preflight.preflight_id),
                        click_started_at_epoch_ms: None,
                        resource_warning_confirmed: preflight.severity
                            == ResourcePreflightSeverity::Warning,
                    },
                ),
            )
            .await?
            .operation
        }
        RenderBuildMode::Realtime => {
            facade.discard_preflight(&preflight.preflight_id);
            let realtime_input = RenderServiceStartBuildInput {
                layer_view: false, initial_layer_range: None,
                source_path: projection.to_path_buf(),
                build_mode: RenderBuildMode::Realtime,
                display_mode: crate::RenderDisplayMode::Full,
                preflight_id: None,
                resource_warning_confirmed: false,
                realtime_decision_id: None,
                click_started_at_epoch_ms: None,
                viewport: Some([1280, 720]),
            };
            let (request, environment) = timed(
                "PrepareRealtimeRequest",
                ACTION_TIMEOUT,
                report,
                run_application_operation(move || application_render_request(realtime_input)),
            )
            .await?;
            timed(
                "BuildStart",
                ACTION_TIMEOUT,
                report,
                facade.start_realtime_render(request, environment),
            )
            .await?
            .operation
        }
    };
    let operation_id = operation.operation_id.clone();
    report.final_operation = Some(operation);
    report.mark("Build", None, started);
    if options.mode == RenderBuildMode::Realtime {
        let early = wait_for_operation(
            "RealtimeReady",
            &operation_id,
            facade,
            service,
            application_events,
            report,
            BUILD_TIMEOUT,
            |view| {
                matches!(
                    view.state,
                    OperationState::RealtimeReady | OperationState::ViewerPrepared
                )
            },
        )
        .await?;
        report.final_operation = Some(early);
        report.mark("RealtimeReady", None, started);
        let log_dir = report
            .viewer_log_dir
            .as_ref()
            .ok_or_else(|| "Viewer log directory was not initialized".to_owned())?;
        report.viewer_logs = facade
            .headless_viewer_log_paths(&operation_id, log_dir)?
            .into_iter()
            .collect();
        show_and_hold(
            0,
            &operation_id,
            facade,
            service,
            application_events,
            report,
            started,
        )
        .await?;
        timed(
            "EarlyHide",
            ACTION_TIMEOUT,
            report,
            facade.hide_viewer(&operation_id),
        )
        .await?;
        let hidden = wait_for_operation(
            "EarlyHideState",
            &operation_id,
            facade,
            service,
            application_events,
            report,
            ACTION_TIMEOUT,
            |view| {
                matches!(
                    view.state,
                    OperationState::RealtimeReady | OperationState::ViewerPrepared
                ) && view.presentation_state == PresentationState::Hidden
            },
        )
        .await?;
        report.final_operation = Some(hidden);
        report.mark("EarlyHide", None, started);
    }
    let prepared = wait_for_operation(
        "ViewerPrepared",
        &operation_id,
        facade,
        service,
        application_events,
        report,
        BUILD_TIMEOUT,
        |view| view.state == OperationState::ViewerPrepared,
    )
    .await?;
    report.final_operation = Some(prepared);
    if report.application_event_count == 0 {
        return Err("Application event bridge produced no render-state events".to_owned());
    }
    let log_dir = report
        .viewer_log_dir
        .as_ref()
        .ok_or_else(|| "Viewer log directory was not initialized".to_owned())?;
    report.viewer_logs = facade
        .headless_viewer_log_paths(&operation_id, log_dir)?
        .into_iter()
        .collect();
    report.mark("ViewerPrepared", None, started);

    show_and_hold(
        1,
        &operation_id,
        facade,
        service,
        application_events,
        report,
        started,
    )
    .await?;
    timed(
        "Hide",
        ACTION_TIMEOUT,
        report,
        facade.hide_viewer(&operation_id),
    )
    .await?;
    let hidden = wait_for_operation(
        "HideState",
        &operation_id,
        facade,
        service,
        application_events,
        report,
        ACTION_TIMEOUT,
        |view| {
            view.state == OperationState::ViewerPrepared
                && view.presentation_state == PresentationState::Hidden
        },
    )
    .await?;
    report.final_operation = Some(hidden);
    report.mark("Hide", None, started);
    show_and_hold(
        2,
        &operation_id,
        facade,
        service,
        application_events,
        report,
        started,
    )
    .await?;
    let frames = timed(
        "CameraZoomStress",
        ACTION_TIMEOUT,
        report,
        facade.headless_camera_stress(&operation_id),
    )
    .await?;
    report.camera_stress_frames = Some(frames);
    report.mark("CameraZoomStress", None, started);
    let final_view = current_operation(&operation_id, facade, service, application_events, report)?;
    if final_view.state != OperationState::ViewerPrepared
        || final_view.presentation_state != PresentationState::Visible
    {
        return Err("Viewer lost prepared/visible state during camera/zoom stress".to_owned());
    }
    report.final_operation = Some(final_view);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn show_and_hold(
    attempt: u8,
    operation_id: &OperationId,
    facade: &RenderFacade,
    service: &RenderServiceClient,
    application_events: &mpsc::Receiver<ApplicationRenderSnapshot>,
    report: &mut Report,
    started: Instant,
) -> Result<(), String> {
    let label = match attempt {
        0 => "EarlyShow",
        1 => "Show",
        _ => "SecondShow",
    };
    timed(
        label,
        ACTION_TIMEOUT,
        report,
        facade.show_viewer(operation_id, PresentationMode::Popup, None),
    )
    .await?;
    report.mark("Show", Some(attempt), started);
    let visible = wait_for_operation(
        "FirstFrame",
        operation_id,
        facade,
        service,
        application_events,
        report,
        ACTION_TIMEOUT,
        |view| {
            matches!(
                view.state,
                OperationState::ViewerPrepared | OperationState::RealtimeReady
            ) && view.presentation_state == PresentationState::Visible
        },
    )
    .await?;
    report.final_operation = Some(visible);
    report.mark("FirstFrame", Some(attempt), started);
    if attempt == 0 {
        let (compiled, existing) = facade.headless_compilation_progress(operation_id)?;
        report.early_compiled_sections = Some(compiled);
        report.early_existing_sections = Some(existing);
        let (stored, exact) =
            verify_early_realtime_first_frame(&report.viewer_logs, compiled, existing)?;
        report.early_stored_sections = Some(stored);
        report.early_exact_resident_sections = Some(exact);
        let frames = timed(
            "EarlyCameraZoomStress",
            ACTION_TIMEOUT,
            report,
            facade.headless_camera_stress(operation_id),
        )
        .await?;
        if frames != 28 {
            return Err(format!(
                "early camera/zoom stress presented {frames}/28 frames"
            ));
        }
        // Keep the final, post-Prepared stress count independent.
        report.mark("EarlyCameraZoomStress", None, started);
    }
    let hold_start = Instant::now();
    while hold_start.elapsed() < HOLD_DURATION {
        let view = current_operation(operation_id, facade, service, application_events, report)?;
        if !matches!(
            view.state,
            OperationState::ViewerPrepared | OperationState::RealtimeReady
        ) || view.presentation_state != PresentationState::Visible
        {
            return Err(format!(
                "Viewer did not remain prepared and visible throughout hold {attempt}: {:?}/{:?}",
                view.state, view.presentation_state
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    report.measure(
        match attempt {
            0 => "EarlyHold",
            1 => "Hold1",
            _ => "Hold2",
        },
        hold_start,
    );
    report.mark("Hold5s", Some(attempt), started);
    Ok(())
}

fn verify_early_realtime_first_frame(
    logs: &[PathBuf],
    compiled: u64,
    expected_existing: u64,
) -> Result<(u64, u64), String> {
    for path in logs {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("read early Realtime frame evidence: {e}"))?;
        if let Some(line) = text
            .lines()
            .find(|line| line.starts_with("[NOVA_REALTIME_FIRST_FRAME]"))
        {
            let field = |key: &str| {
                line.split_whitespace()
                    .find_map(|token| token.strip_prefix(key))
                    .and_then(|v| v.parse::<u64>().ok())
            };
            if let (Some(stored), Some(existing), Some(exact)) = (
                field("stored="),
                field("existing="),
                field("exact_resident="),
            ) {
                if compiled >= stored
                    && compiled < existing
                    && existing == expected_existing
                    && stored > 0
                    && exact > 0
                {
                    return Ok((stored, exact));
                }
            }
            return Err(format!(
                "Realtime first frame was not early exact geometry: worker_compiled={compiled} worker_existing={expected_existing}; {line}"
            ));
        }
    }
    Err("Realtime first-frame geometry evidence is missing".to_owned())
}

#[allow(clippy::too_many_arguments)]
async fn wait_for_operation(
    stage: &str,
    operation_id: &OperationId,
    facade: &RenderFacade,
    service: &RenderServiceClient,
    application_events: &mpsc::Receiver<ApplicationRenderSnapshot>,
    report: &mut Report,
    timeout: Duration,
    predicate: impl Fn(&RenderOperationView) -> bool,
) -> Result<RenderOperationView, String> {
    let stage_start = Instant::now();
    loop {
        let view = current_operation(operation_id, facade, service, application_events, report)?;
        if predicate(&view) {
            report.measure(stage, stage_start);
            return Ok(view);
        }
        if stage_start.elapsed() >= timeout {
            return Err(format!(
                "{stage} timed out after {} ms; state={:?}, phase={}",
                stage_start.elapsed().as_millis(),
                view.state,
                view.progress.phase
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn current_operation(
    operation_id: &OperationId,
    facade: &RenderFacade,
    service: &RenderServiceClient,
    application_events: &mpsc::Receiver<ApplicationRenderSnapshot>,
    report: &mut Report,
) -> Result<RenderOperationView, String> {
    while application_events.try_recv().is_ok() {
        report.application_event_count += 1;
    }
    if let Some(error) = service.connection_error() {
        return Err(format!("Viewer/render service disconnected: {error}"));
    }
    if let Some(exit) = service
        .observed_process_exits()
        .iter()
        .find(|exit| exit.exit_code != Some(0) || exit.wait_error.is_some())
    {
        return Err(format!(
            "child {:?} pid {} exited abnormally: {:?}, {:?}",
            exit.child.role, exit.child.pid, exit.exit_code, exit.wait_error
        ));
    }
    let view = facade
        .render_states()
        .into_iter()
        .find(|view| &view.operation_id == operation_id)
        .ok_or_else(|| format!("Application operation {} disappeared", operation_id.0))?;
    if matches!(
        view.state,
        OperationState::Cancelled | OperationState::Failed | OperationState::RuntimeReleased
    ) {
        return Err(format!(
            "Application render operation ended in {:?}: {}",
            view.state,
            view.error.as_deref().unwrap_or("no detail")
        ));
    }
    Ok(view)
}

async fn timed<T>(
    stage: &str,
    timeout: Duration,
    report: &mut Report,
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let started = Instant::now();
    let result = tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| format!("{stage} timed out after {} s", timeout.as_secs()))?
        .map_err(|error| format!("{stage}: {error}"))?;
    report.measure(stage, started);
    Ok(result)
}

fn parse_options(args: &[OsString]) -> Result<Options, String> {
    let mut fields = BTreeMap::<String, OsString>::new();
    let mut iterator = args.iter();
    if !args
        .iter()
        .any(|arg| arg.as_os_str() == OsStr::new("--headless-e2e"))
    {
        return Err("--headless-e2e is required".to_owned());
    }
    while let Some(arg) = iterator.next() {
        if arg.as_os_str() == OsStr::new("--headless-e2e") {
            continue;
        }
        let key = arg
            .to_str()
            .ok_or_else(|| "headless E2E option name is not UTF-8".to_owned())?;
        if !matches!(
            key,
            "--projection" | "--backend-dir" | "--report" | "--mode"
        ) {
            return Err(format!("unknown headless E2E option {key}"));
        }
        let value = iterator
            .next()
            .ok_or_else(|| format!("missing value for {key}"))?;
        if fields.insert(key.to_owned(), value.clone()).is_some() {
            return Err(format!("duplicate headless E2E option {key}"));
        }
    }
    let required = |key: &str| -> Result<OsString, String> {
        fields
            .get(key)
            .cloned()
            .ok_or_else(|| format!("missing headless E2E option {key}"))
    };
    let projection = PathBuf::from(required("--projection")?);
    let backend_dir = PathBuf::from(required("--backend-dir")?);
    let report = PathBuf::from(required("--report")?);
    if !projection.is_absolute() || !backend_dir.is_absolute() || !report.is_absolute() {
        return Err("projection, backend-dir and report must all be absolute paths".to_owned());
    }
    let mode_value = required("--mode")?;
    let mode = match mode_value.to_str() {
        Some("prebuilt") => RenderBuildMode::Prebuilt,
        Some("realtime") => RenderBuildMode::Realtime,
        _ => return Err("mode must be prebuilt or realtime".to_owned()),
    };
    Ok(Options {
        projection,
        backend_dir,
        report,
        mode,
    })
}

fn raw_report_path(args: &[OsString]) -> Option<PathBuf> {
    args.windows(2)
        .find(|pair| pair[0].as_os_str() == OsStr::new("--report"))
        .map(|pair| PathBuf::from(&pair[1]))
}

fn mode_name(mode: RenderBuildMode) -> &'static str {
    match mode {
        RenderBuildMode::Prebuilt => "prebuilt",
        RenderBuildMode::Realtime => "realtime",
    }
}

fn validate_projection(path: &Path) -> Result<PathBuf, String> {
    let projection_root = std::env::var_os("USERPROFILE")
        .map(PathBuf::from).ok_or("current user profile is unavailable")?
        .join("Documents").join("投影测试");
    let allowed = fs::canonicalize(projection_root)
        .map_err(|error| format!("resolve projection allowlist directory: {error}"))?;
    let projection = fs::canonicalize(path)
        .map_err(|error| format!("resolve projection {}: {error}", path.display()))?;
    if !projection.starts_with(&allowed)
        || projection.extension().and_then(OsStr::to_str) != Some("litematic")
        || !projection.is_file()
    {
        return Err(format!(
            "projection must be a .litematic file under {}",
            allowed.display()
        ));
    }
    Ok(projection)
}

fn verify_backend_deployment(backend_dir: &Path) -> Result<BackendDeployment, String> {
    let manifest_path = backend_dir.join("backend-deployment.json");
    let deployment: BackendDeployment = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("read backend deployment manifest: {error}"))?,
    )
    .map_err(|error| format!("parse backend deployment manifest: {error}"))?;
    if deployment.schema != DEPLOYMENT_SCHEMA || deployment.profile != "release" {
        return Err("backend deployment manifest is not Release schema v1".to_owned());
    }
    let expected = BTreeSet::from([
        "litematica_core.exe".to_owned(),
        "nova-render-service.exe".to_owned(),
        "nova-build-worker.exe".to_owned(),
        "litematica_native_viewer.exe".to_owned(),
    ]);
    if deployment
        .artifacts
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(
            "backend deployment manifest has missing or extra executable entries".to_owned(),
        );
    }
    for (name, artifact) in &deployment.artifacts {
        if artifact.sha256.len() != 64
            || !artifact
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!("backend deployment has invalid SHA-256 for {name}"));
        }
        let path = backend_dir.join(name);
        let mut file = File::open(&path)
            .map_err(|error| format!("open backend executable {}: {error}", path.display()))?;
        let before = file
            .metadata()
            .map_err(|error| format!("stat backend executable {}: {error}", path.display()))?;
        if !before.is_file() || before.len() != artifact.bytes {
            return Err(format!(
                "backend executable byte count differs from manifest: {name}"
            ));
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 256 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| format!("hash backend executable {name}: {error}"))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        let after = fs::metadata(&path)
            .map_err(|error| format!("re-stat backend executable {name}: {error}"))?;
        if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
            return Err(format!(
                "backend executable changed during verification: {name}"
            ));
        }
        if format!("{:x}", hasher.finalize()) != artifact.sha256 {
            return Err(format!(
                "backend executable SHA-256 differs from manifest: {name}"
            ));
        }
    }
    Ok(deployment)
}

fn scan_diagnostics(logs: &[PathBuf]) -> Result<Vec<String>, String> {
    let mut failures = Vec::new();
    for log in logs {
        let reader = BufReader::new(
            File::open(log)
                .map_err(|error| format!("open Viewer log {}: {error}", log.display()))?,
        );
        for (line_number, line) in reader.lines().enumerate() {
            let line =
                line.map_err(|error| format!("read Viewer log {}: {error}", log.display()))?;
            let lower = line.to_ascii_lowercase();
            if lower.contains("panicked at")
                || lower.contains("fatal=true")
                || lower.contains("status=fatal")
                || lower.contains("iris_fatal_error")
                || lower.contains("assertion failed")
                || lower.contains("assert failed")
            {
                failures.push(format!("{}:{}: {}", log.display(), line_number + 1, line));
                if failures.len() >= 16 {
                    return Ok(failures);
                }
            }
        }
    }
    Ok(failures)
}

fn read_first_visible_frame_ordinals(logs: &[PathBuf]) -> Result<Vec<u64>, String> {
    let stdout = logs
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.ends_with("-viewer-stdout.log"))
        })
        .ok_or_else(|| "exact Viewer stdout log path is unavailable".to_owned())?;
    let reader = BufReader::new(
        File::open(stdout)
            .map_err(|error| format!("open exact Viewer stdout {}: {error}", stdout.display()))?,
    );
    let mut ordinals = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| format!("read exact Viewer stdout: {error}"))?;
        if !line.starts_with("[NOVA_RENDER_DATA] ")
            || !line
                .split_ascii_whitespace()
                .any(|token| token == "stage=first_visible_frame")
        {
            continue;
        }
        let ordinal = line
            .split_ascii_whitespace()
            .find_map(|token| token.strip_prefix("popup_ordinal="))
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| "Viewer first_visible_frame marker lacks popup_ordinal".to_owned())?;
        ordinals.push(ordinal);
    }
    Ok(ordinals)
}

fn runtime_zero_counters(mode: RenderBuildMode) -> Vec<&'static str> {
    let mut counters = vec![
        "post_prepared_artifact_scale_restore_count",
        "post_prepared_scene_scale_upload_count",
        "post_prepared_scene_scale_replay_count",
        "post_prepared_runtime_attach_restore_count",
        "post_prepared_geometry_conversion_count",
        "post_prepared_repack_count",
    ];
    if mode == RenderBuildMode::Prebuilt {
        counters.extend([
            "post_prepared_artifact_bytes",
            "post_prepared_decodes",
            "post_prepared_gpu_upload_bytes",
            "post_prepared_replays",
        ]);
    }
    counters
}

fn validate_runtime_counter_line(line: &str, counters: &[&str]) -> Result<(), String> {
    for counter in counters {
        let value = line.split_ascii_whitespace().find_map(|token| {
            let (name, value) = token.split_once('=')?;
            (name == *counter).then_some(value)
        });
        if value.and_then(|value| value.parse::<u64>().ok()) != Some(0) {
            return Err(format!(
                "Viewer runtime contract {counter} must be zero; observed {value:?}"
            ));
        }
    }
    Ok(())
}

fn validate_runtime_contracts(logs: &[PathBuf], mode: RenderBuildMode) -> Result<(), String> {
    let stdout = logs
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.ends_with("-viewer-stdout.log"))
        })
        .ok_or_else(|| {
            "exact Viewer stdout log is unavailable for runtime validation".to_owned()
        })?;
    let reader = BufReader::new(
        File::open(stdout).map_err(|error| format!("open Viewer runtime contract log: {error}"))?,
    );
    let counters = runtime_zero_counters(mode);
    let final_popup_ordinal = if mode == RenderBuildMode::Realtime {
        "popup_ordinal=3"
    } else {
        "popup_ordinal=2"
    };
    let mut prepared = false;
    let mut second_frame_presented = false;
    let mut stress_snapshot = false;
    for line in reader.lines() {
        let line = line.map_err(|error| format!("read Viewer runtime contract log: {error}"))?;
        if !line.starts_with("[NOVA_RENDER_DATA] ") {
            continue;
        }
        if line
            .split_ascii_whitespace()
            .any(|token| token == "stage=viewer_prepared")
        {
            prepared = true;
        }
        if prepared {
            validate_runtime_counter_line(&line, &counters)?;
            if line
                .split_ascii_whitespace()
                .any(|token| token == "stage=first_visible_frame")
                && line
                    .split_ascii_whitespace()
                    .any(|token| token == final_popup_ordinal)
            {
                second_frame_presented = true;
            }
            if line
                .split_ascii_whitespace()
                .any(|token| token == "stage=camera_stress_complete")
            {
                if !second_frame_presented {
                    // Compilation may naturally finish during the early stress.
                    // Its popup-1 trace is separate from the final popup-3 proof.
                    if mode == RenderBuildMode::Realtime
                        && line
                            .split_ascii_whitespace()
                            .any(|token| token == "popup_ordinal=1")
                    {
                        continue;
                    }
                    return Err(
                        "Viewer camera stress snapshot precedes the second presented frame"
                            .to_owned(),
                    );
                }
                stress_snapshot = true;
            }
        }
    }
    if !prepared {
        return Err("ViewerPrepared runtime contract marker is missing".to_owned());
    }
    if !stress_snapshot {
        return Err("Viewer final camera stress runtime snapshot is missing".to_owned());
    }
    Ok(())
}

fn write_report(path: &Path, report: &Report) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "report path has no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("create report directory {}: {error}", parent.display()))?;
    let mut json = serde_json::to_vec_pretty(report)
        .map_err(|error| format!("serialize headless E2E report: {error}"))?;
    json.push(b'\n');
    fs::write(path, json).map_err(|error| format!("write headless E2E report: {error}"))?;
    serde_json::from_slice::<serde_json::Value>(
        &fs::read(path).map_err(|error| format!("reopen headless E2E report: {error}"))?,
    )
    .map_err(|error| format!("verify headless E2E report: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_runtime_gate_allows_bounded_io_but_never_geometry_repack() {
        let realtime = runtime_zero_counters(RenderBuildMode::Realtime);
        let mut line = realtime
            .iter()
            .map(|name| format!("{name}=0"))
            .collect::<Vec<_>>()
            .join(" ");
        line.push_str(" post_prepared_artifact_bytes=1024 post_prepared_decodes=2 post_prepared_gpu_upload_bytes=2048 post_prepared_replays=0");
        assert!(validate_runtime_counter_line(&line, &realtime).is_ok());
        assert!(
            validate_runtime_counter_line(&line, &runtime_zero_counters(RenderBuildMode::Prebuilt))
                .is_err()
        );
        let repacked = line.replace(
            "post_prepared_repack_count=0",
            "post_prepared_repack_count=1",
        );
        assert!(validate_runtime_counter_line(&repacked, &realtime).is_err());
        let missing = line.replace("post_prepared_repack_count=0", "");
        assert!(validate_runtime_counter_line(&missing, &realtime).is_err());
        let converted = line.replace(
            "post_prepared_geometry_conversion_count=0",
            "post_prepared_geometry_conversion_count=1",
        );
        assert!(validate_runtime_counter_line(&converted, &realtime).is_err());
    }

    #[test]
    fn early_frame_requires_worker_compilation_proof_not_only_a_lagging_store() {
        let path = env::temp_dir().join(format!("nova-early-exact-{}.log", std::process::id()));
        let logs = [path.clone()];
        fs::write(
            &path,
            "[NOVA_REALTIME_FIRST_FRAME] stored=1 existing=2 exact_resident=1\n",
        )
        .unwrap();
        assert_eq!(
            verify_early_realtime_first_frame(&logs, 1, 2).unwrap(),
            (1, 1)
        );
        assert!(
            verify_early_realtime_first_frame(&logs, 2, 2).is_err(),
            "already fully compiled is not early, even with a lagging store"
        );
        assert!(
            verify_early_realtime_first_frame(&logs, 0, 2).is_err(),
            "unknown CPU progress is not proof"
        );
        assert!(verify_early_realtime_first_frame(&logs, 1, 3).is_err());
        fs::write(
            &path,
            "[NOVA_REALTIME_FIRST_FRAME] stored=1 existing=2 exact_resident=0\n",
        )
        .unwrap();
        assert!(verify_early_realtime_first_frame(&logs, 1, 2).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn runtime_gate_requires_final_camera_stress_snapshot_and_checks_late_repack() {
        let path = env::temp_dir().join(format!(
            "nova-headless-stress-contract-{}-viewer-stdout.log",
            std::process::id()
        ));
        let logs = [path.clone()];
        for mode in [RenderBuildMode::Realtime, RenderBuildMode::Prebuilt] {
            let counters = runtime_zero_counters(mode)
                .iter()
                .map(|name| format!("{name}=0"))
                .collect::<Vec<_>>()
                .join(" ");
            let prepared = format!("[NOVA_RENDER_DATA] stage=viewer_prepared {counters}\n");
            let first_frame = format!(
                "[NOVA_RENDER_DATA] stage=first_visible_frame popup_ordinal=1 {counters}\n"
            );
            let final_ordinal = if mode == RenderBuildMode::Realtime {
                3
            } else {
                2
            };
            let second_frame = format!(
                "[NOVA_RENDER_DATA] stage=first_visible_frame popup_ordinal={final_ordinal} {counters}\n"
            );
            let stress = format!(
                "[NOVA_RENDER_DATA] stage=camera_stress_complete popup_ordinal={final_ordinal} {counters}\n"
            );
            let before_stress = format!("{prepared}{first_frame}{second_frame}");

            fs::write(&path, &before_stress).unwrap();
            assert!(
                validate_runtime_contracts(&logs, mode)
                    .unwrap_err()
                    .contains("final camera stress runtime snapshot is missing")
            );
            fs::write(&path, format!("{before_stress}{stress}")).unwrap();
            assert!(validate_runtime_contracts(&logs, mode).is_ok());
            if mode == RenderBuildMode::Realtime {
                let early_stress = stress.replace("popup_ordinal=3", "popup_ordinal=1");
                fs::write(
                    &path,
                    format!("{prepared}{early_stress}{first_frame}{second_frame}{stress}"),
                )
                .unwrap();
                assert!(validate_runtime_contracts(&logs, mode).is_ok());
            }

            let repacked = stress.replace(
                "post_prepared_repack_count=0",
                "post_prepared_repack_count=1",
            );
            fs::write(&path, format!("{before_stress}{repacked}")).unwrap();
            assert!(
                validate_runtime_contracts(&logs, mode)
                    .unwrap_err()
                    .contains("post_prepared_repack_count must be zero")
            );
            fs::write(
                &path,
                format!("{prepared}{first_frame}{stress}{second_frame}"),
            )
            .unwrap();
            assert!(
                validate_runtime_contracts(&logs, mode)
                    .unwrap_err()
                    .contains("precedes the second presented frame")
            );
            fs::write(&path, format!("{stress}{before_stress}")).unwrap();
            assert!(validate_runtime_contracts(&logs, mode).is_err());
        }
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn cli_requires_all_absolute_paths_and_exact_mode() {
        let args = [
            OsString::from("--headless-e2e"),
            OsString::from("--projection"),
            OsString::from(r"C:\Users\27232\Documents\投影测试\sample.litematic"),
            OsString::from("--backend-dir"),
            OsString::from(r"C:\repo\bin\viewer-backend"),
            OsString::from("--report"),
            OsString::from(r"C:\report.json"),
            OsString::from("--mode"),
            OsString::from("prebuilt"),
        ];
        assert_eq!(
            parse_options(&args).unwrap().mode,
            RenderBuildMode::Prebuilt
        );
        let mut invalid = args.to_vec();
        *invalid.last_mut().unwrap() = OsString::from("preview");
        assert!(parse_options(&invalid).is_err());
    }

    #[test]
    fn report_path_is_recovered_even_when_another_option_is_invalid() {
        let args = [
            OsString::from("--headless-e2e"),
            OsString::from("--unknown"),
            OsString::from("value"),
            OsString::from("--report"),
            OsString::from(r"C:\logs\e2e.json"),
        ];
        assert_eq!(
            raw_report_path(&args),
            Some(PathBuf::from(r"C:\logs\e2e.json"))
        );
    }

    #[test]
    fn exact_viewer_markers_and_fatal_boolean_are_distinguished() {
        let path = env::temp_dir().join(format!(
            "nova-headless-e2e-{}-viewer-stdout.log",
            std::process::id()
        ));
        fs::write(
            &path,
            concat!(
                "[NOVA_RENDER_DATA] schema=2 role=viewer stage=first_visible_frame popup_ordinal=1\n",
                "command_error fatal=false\n",
                "[NOVA_RENDER_DATA] schema=2 role=viewer stage=first_visible_frame popup_ordinal=2\n",
            ),
        )
        .unwrap();
        assert_eq!(
            read_first_visible_frame_ordinals(&[path.clone()]).unwrap(),
            vec![1, 2]
        );
        assert!(scan_diagnostics(&[path.clone()]).unwrap().is_empty());
        fs::write(&path, "command_error fatal=true\n").unwrap();
        assert_eq!(scan_diagnostics(&[path.clone()]).unwrap().len(), 1);
        fs::remove_file(path).unwrap();
    }
}
