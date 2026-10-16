use super::model::{
    ApplicationRenderSnapshot, GraphicsBackendPreference, IrisFallbackPreference, OperationId,
    OperationProgress, OperationState, PresentationIntent, PresentationMode, PresentationPlacement,
    PresentationState, ProductPreviewImage, ProjectAnalysis, ProjectRenderSettings, ProjectSource,
    RenderBuildMode, RenderDisplayMode, RenderOperationView, ResourcePreflightAction, ResourcePreflightComponents,
    ResourcePreflightId, ResourcePreflightSeverity, ResourcePreflightView,
    UserVisibleRenderRequest, WeatherPreference,
};
use crate::render_service_client::RenderServiceClient;
use crate::viewer_launch_contract::{ViewerLaunchContext, ViewerLaunchContractFile};
use nova_render_protocol::{
    AdmittedBuildStart, ArtifactState, BuildAccepted, BuildAdmission, BuildResourcePreflight,
    BuildStart, BuildState, PresentationSpec, RenderBackend,
    RenderBuildMode as ProtocolRenderBuildMode, RenderPreviewRequest,
    ResourcePressure as ProtocolResourcePressure, ResponseBody, SceneContent, ScreenRect,
    ServiceSnapshot, UiCommand, ViewerPresentationState, ViewerRuntimeState,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub(crate) const APPLICATION_RENDER_STATE_EVENT: &str = "nova://application/render-state";

/// Small environment inputs already resolved by the Application Host.
///
/// The facade never discovers, scans, hashes, or reads these paths. They are
/// forwarded once with the semantic request so render-service remains the
/// authority for content identity and artifact validity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderEnvironmentInputs {
    pub(crate) asset_inputs: Vec<PathBuf>,
    pub(crate) viewport: Option<[u32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RenderStartResult {
    pub(crate) operation: RenderOperationView,
    pub(crate) attached: bool,
}

/// Application-facing render boundary. Its public surface contains product
/// semantics only; renderer wire DTOs remain private implementation details.
#[derive(Clone)]
pub(crate) struct RenderFacade {
    inner: Arc<RenderFacadeInner>,
}

struct RenderFacadeInner {
    transport: Arc<dyn RenderTransport>,
    state: Mutex<FacadeState>,
    operation_sequence: AtomicU64,
    preflight_sequence: AtomicU64,
}

#[derive(Default)]
struct FacadeState {
    operations: HashMap<OperationId, OperationRecord>,
    operation_order: Vec<OperationId>,
    current_operation: Option<OperationId>,
    preflights: HashMap<ResourcePreflightId, PendingResourcePreflight>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderIntentKey {
    source: ProjectSource,
    settings: ProjectRenderSettings,
}

struct OperationRecord {
    request_key: RenderIntentKey,
    view: RenderOperationView,
    binding: Option<RenderBinding>,
    cancel_requested: bool,
}

/// Renderer correlation state is deliberately private to the facade. None of
/// these values may cross the product-semantic API exposed to React.
struct RenderBinding {
    build_id: String,
    generation: u64,
    session_id: Option<String>,
}

struct PendingResourcePreflight {
    request: UserVisibleRenderRequest,
    environment: RenderEnvironmentInputs,
    prospective_build: BuildStart,
    pressure: ProtocolResourcePressure,
    admission_token: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderStartAuthorization {
    pub(crate) preflight_id: Option<ResourcePreflightId>,
    pub(crate) resource_warning_confirmed: bool,
    pub(crate) click_started_at_epoch_ms: Option<u64>,
}

trait RenderTransport: Send + Sync {
    fn request<'a>(
        &'a self,
        command: UiCommand,
    ) -> Pin<Box<dyn Future<Output = Result<ResponseBody, String>> + Send + 'a>>;

    fn snapshot(&self) -> ServiceSnapshot;

    fn subscribe_events(&self) -> std::sync::mpsc::Receiver<nova_render_protocol::ServiceEvent>;
}

impl RenderTransport for RenderServiceClient {
    fn request<'a>(
        &'a self,
        command: UiCommand,
    ) -> Pin<Box<dyn Future<Output = Result<ResponseBody, String>> + Send + 'a>> {
        Box::pin(async move { RenderServiceClient::request(self, command).await })
    }

    fn snapshot(&self) -> ServiceSnapshot {
        RenderServiceClient::snapshot(self)
    }

    fn subscribe_events(&self) -> std::sync::mpsc::Receiver<nova_render_protocol::ServiceEvent> {
        RenderServiceClient::subscribe_events(self)
    }
}

impl RenderFacade {
    pub(crate) fn new(client: RenderServiceClient) -> Self {
        Self::with_transport(Arc::new(client))
    }

    /// Bridges render-service notifications into an Application-owned event.
    /// The WebView never receives the renderer protocol event or snapshot.
    pub(crate) fn start_event_bridge(&self, app: AppHandle) -> Result<(), String> {
        self.start_event_bridge_with(move |snapshot| {
            let _ = app.emit(APPLICATION_RENDER_STATE_EVENT, snapshot);
        })
    }

    /// The product E2E client consumes the same Application projections that
    /// the WebView bridge publishes, without subscribing to renderer DTOs.
    pub(crate) fn start_headless_event_bridge(
        &self,
    ) -> Result<mpsc::Receiver<ApplicationRenderSnapshot>, String> {
        let (sender, receiver) = mpsc::channel();
        self.start_event_bridge_with(move |snapshot| {
            let _ = sender.send(snapshot);
        })?;
        Ok(receiver)
    }

    fn start_event_bridge_with(
        &self,
        publish: impl Fn(ApplicationRenderSnapshot) + Send + 'static,
    ) -> Result<(), String> {
        let receiver = self.inner.transport.subscribe_events();
        let facade = self.clone();
        std::thread::Builder::new()
            .name("nova-application-render-events".to_owned())
            .spawn(move || {
                while let Ok(event) = receiver.recv() {
                    let changed = facade.observe_service_event(&event);
                    if !changed.is_empty() {
                        publish(ApplicationRenderSnapshot {
                            operations: facade.render_states(),
                        });
                    }
                }
            })
            .map(|_| ())
            .map_err(|error| format!("start Application render event bridge: {error}"))
    }

    fn with_transport(transport: Arc<dyn RenderTransport>) -> Self {
        Self {
            inner: Arc::new(RenderFacadeInner {
                transport,
                state: Mutex::new(FacadeState::default()),
                operation_sequence: AtomicU64::new(1),
                preflight_sequence: AtomicU64::new(1),
            }),
        }
    }

    /// Executes the existing Viewer preview contract behind the render boundary
    /// and exposes only product preview data to the Tauri/WebView API.
    pub(crate) async fn run_viewer_preview(
        &self,
        launch: &ViewerLaunchContext,
        launch_contract: &ViewerLaunchContractFile,
        output_path: PathBuf,
    ) -> Result<ProductPreviewImage, String> {
        let output = match self
            .inner
            .transport
            .request(UiCommand::RunViewerPreview(RenderPreviewRequest {
                launch_contract_path: launch_contract.path().to_path_buf(),
                output_path,
                render_compatibility_fingerprint: launch.compatibility_fingerprint().to_owned(),
            }))
            .await?
        {
            ResponseBody::ViewerPreview(output) => output,
            other => {
                return Err(format!(
                    "render service returned an unexpected preview response: {other:?}"
                ));
            }
        };
        if !launch_contract.accepted_ack(&output.stdout)? {
            return Err(format!(
                "Viewer exited without accepting launch contract {}\nStdout: {}\nStderr: {}",
                launch_contract.path().display(),
                output.stdout,
                output.stderr
            ));
        }
        if output.exit_code != Some(0) {
            return Err(format!(
                "Exit code: {:?}\nStdout: {}\nStderr: {}",
                output.exit_code, output.stdout, output.stderr
            ));
        }
        Ok(ProductPreviewImage {
            preview_path: output.preview_path,
            data_url: output.data_url,
        })
    }

    /// Runs the renderer-owned, bounded resource estimate for one exact
    /// prospective prebuilt request. The renderer admission token never leaves
    /// this facade; React receives an unrelated one-time Application id.
    pub(crate) async fn preflight_render(
        &self,
        request: UserVisibleRenderRequest,
        environment: RenderEnvironmentInputs,
    ) -> Result<ResourcePreflightView, String> {
        if request.settings.build_mode != RenderBuildMode::Prebuilt {
            return Err("resource preflight is only required for prebuilt mode".to_owned());
        }
        let prospective_build = derive_build_start(&request, environment.clone());
        let preflight_started = std::time::Instant::now();
        let response = match self
            .inner
            .transport
            .request(UiCommand::PreflightBuild(prospective_build.clone()))
            .await?
        {
            ResponseBody::BuildPreflight(response) => response,
            other => {
                return Err(format!(
                    "render service returned an unexpected preflight response: {other:?}"
                ));
            }
        };
        eprintln!("[NOVA_RESOURCE_PHASE] stage=host_preflight_service_response elapsed_ms={:.3}", preflight_started.elapsed().as_secs_f64() * 1000.0);
        validate_preflight_response(&response)?;

        let preflight_id = self.next_preflight_id();
        let view = resource_preflight_view(
            preflight_id.clone(),
            &response,
            basic_vulkan_is_available(&request),
        );
        self.inner.state.lock().unwrap().preflights.insert(
            preflight_id,
            PendingResourcePreflight {
                request,
                environment,
                prospective_build,
                pressure: response.pressure,
                admission_token: response.admission_token,
            },
        );
        Ok(view)
    }

    /// Consumes one assessment and re-runs it with the only currently honest
    /// visual-loss option: Vulkan basic rendering without a shader pack.
    ///
    /// The old id is removed before any await and can never be replayed. The
    /// frozen request/environment are reused so this operation cannot observe
    /// newer global settings or a different source.
    pub(crate) async fn use_basic_vulkan(
        &self,
        preflight_id: &ResourcePreflightId,
        expected_source: &ProjectSource,
    ) -> Result<ResourcePreflightView, String> {
        let mut pending = self
            .inner
            .state
            .lock()
            .unwrap()
            .preflights
            .remove(preflight_id)
            .ok_or_else(|| "resource preflight is unknown or already consumed".to_owned())?;

        if &pending.request.source != expected_source {
            return Err("resource preflight no longer matches the requested source".to_owned());
        }
        if pending.pressure == ProtocolResourcePressure::Invalid {
            return Err(
                "basic Vulkan requires valid renderer inputs".to_owned(),
            );
        }
        if !basic_vulkan_is_available(&pending.request) {
            return Err(
                "basic Vulkan cannot reduce this request because it is already Vulkan or has no shader pack"
                    .to_owned(),
            );
        }

        pending.request.settings.backend = GraphicsBackendPreference::Vulkan;
        pending.request.settings.shader.path = None;
        self.preflight_render(pending.request, pending.environment)
            .await
    }

    pub(crate) fn discard_preflight(&self, preflight_id: &ResourcePreflightId) -> bool {
        self.inner
            .state
            .lock()
            .unwrap()
            .preflights
            .remove(preflight_id)
            .is_some()
    }

    /// Starts or attaches to an equivalent active render intent.
    ///
    /// Presentation is intentionally excluded from the dedupe key: changing
    /// Popup/Overlay placement cannot create a second Build or Viewer runtime.
    pub(crate) async fn start_prebuilt_render(
        &self,
        expected_source: ProjectSource,
        authorization: RenderStartAuthorization,
    ) -> Result<RenderStartResult, String> {
        // Preflight ids are consumed before dedupe or transport. A failed,
        // mismatched or attached start can therefore never replay a grant.
        let (request, start, admission) =
            self.consume_prebuilt_resource_admission(&expected_source, authorization)?;
        self.start_resolved_render(request, start, Some(admission))
            .await
    }

    pub(crate) async fn start_realtime_render(
        &self,
        request: UserVisibleRenderRequest,
        environment: RenderEnvironmentInputs,
    ) -> Result<RenderStartResult, String> {
        self.start_realtime_render_with_click(request, environment, None).await
    }

    pub(crate) async fn start_realtime_render_with_click(
        &self, request: UserVisibleRenderRequest, environment: RenderEnvironmentInputs, click_started_at_epoch_ms: Option<u64>,
    ) -> Result<RenderStartResult, String> {
        if request.settings.build_mode != RenderBuildMode::Realtime {
            return Err("realtime start requires realtime build mode".to_owned());
        }
        let mut start = derive_build_start(&request, environment);
        set_click_diagnostic(&mut start, click_started_at_epoch_ms);
        self.start_resolved_render(request, start, None).await
    }

    /// Consume only the frozen UI decision, never a Prebuilt admission token.
    /// This preserves an explicitly chosen Vulkan/no-Iris setting for Realtime.
    pub(crate) async fn start_realtime_from_preflight(
        &self,
        preflight_id: &ResourcePreflightId,
        expected_source: ProjectSource,
    ) -> Result<RenderStartResult, String> {
        self.start_realtime_from_preflight_with_click(preflight_id, expected_source, None).await
    }

    pub(crate) async fn start_realtime_from_preflight_with_click(
        &self, preflight_id: &ResourcePreflightId, expected_source: ProjectSource, click_started_at_epoch_ms: Option<u64>,
    ) -> Result<RenderStartResult, String> {
        let pending = self.inner.state.lock().unwrap().preflights.remove(preflight_id)
            .ok_or_else(|| "加载决策已结束，请重新检查显存".to_owned())?;
        if pending.request.source != expected_source {
            return Err("加载决策与当前投影不一致".to_owned());
        }
        let mut request = pending.request;
        request.settings.build_mode = RenderBuildMode::Realtime;
        self.start_realtime_render_with_click(request, pending.environment, click_started_at_epoch_ms).await
    }

    async fn start_resolved_render(
        &self,
        request: UserVisibleRenderRequest,
        start: BuildStart,
        admission: Option<BuildAdmission>,
    ) -> Result<RenderStartResult, String> {
        let started = std::time::Instant::now();
        eprintln!("[NOVA_RENDER_PHASE] stage=application_start mode={:?} loading={:?} source={:?} layers={:?} scope=product_request existing_scene_state=resolved_by_intent_key wait=render_service_acceptance",
            request.settings.display_mode, request.settings.build_mode, request.source.path, request.settings.layers);
        let request_key = RenderIntentKey {
            source: request.source.clone(),
            settings: request.settings.clone(),
        };
        let service_snapshot = self.inner.transport.snapshot();
        let existing = {
            let mut state = self.inner.state.lock().unwrap();
            let existing = state.operation_order.iter().rev().find_map(|operation_id| {
                let record = state.operations.get(operation_id)?;
                let binding_live = match record.binding.as_ref().and_then(|binding| binding.session_id.as_ref().map(|session| (binding, session))) {
                    None => true, // Requested/accepted work may not have a Viewer yet.
                    Some((binding, session)) => service_snapshot.viewers.iter().any(|viewer| {
                        viewer.session_id == *session && viewer.build_id == binding.build_id
                            && viewer.generation == binding.generation
                            && !matches!(viewer.runtime_state, ViewerRuntimeState::Released | ViewerRuntimeState::Failed)
                    }),
                };
                (record.request_key == request_key && !record.cancel_requested && is_attachable(record.view.state) && binding_live)
                    .then(|| operation_id.clone())
            });
            if let Some(operation_id) = existing.as_ref() {
                state.current_operation = Some(operation_id.clone());
            }
            existing
        };

        if let Some(operation_id) = existing {
            eprintln!("[NOVA_RENDER_PHASE] stage=application_existing_operation operation_id={:?} attached=true reused_operation_state={:?} elapsed_ms={} new_worker_requested=false new_viewer_requested=false",
                operation_id, self.render_state(&operation_id).map(|view| view.state), started.elapsed().as_millis());
            let presentation_changed = self
                .render_state(&operation_id)
                .is_some_and(|view| view.presentation != request.presentation);
            if presentation_changed {
                self.update_presentation(&operation_id, request.presentation)
                    .await?;
            }
            return self
                .render_state(&operation_id)
                .map(|operation| RenderStartResult {
                    operation,
                    attached: true,
                })
                .ok_or_else(|| "render operation disappeared during dedupe".to_owned());
        }

        let operation_id = self.next_operation_id();
        let view = RenderOperationView {
            operation_id: operation_id.clone(),
            source: request.source.clone(),
            display_mode: request.settings.display_mode,
            build_mode: request.settings.build_mode,
            state: OperationState::Requested,
            progress: OperationProgress {
                phase: "requested".to_owned(),
                ..OperationProgress::default()
            },
            presentation: request.presentation.clone(),
            presentation_state: PresentationState::Hidden,
            analysis: None,
            error: None,
        };
        {
            let mut state = self.inner.state.lock().unwrap();
            state.operations.insert(
                operation_id.clone(),
                OperationRecord {
                    request_key,
                    view,
                    binding: None,
                    cancel_requested: false,
                },
            );
            state.operation_order.push(operation_id.clone());
            state.current_operation = Some(operation_id.clone());
        }

        let accepted = match self
            .inner
            .transport
            .request(UiCommand::StartBuild(AdmittedBuildStart {
                build: start,
                admission,
            }))
            .await
        {
            Ok(ResponseBody::BuildAccepted(accepted)) => accepted,
            Ok(other) => {
                let error =
                    format!("render service returned an unexpected start response: {other:?}");
                self.fail_operation(&operation_id, error.clone());
                return Err(error);
            }
            Err(error) => {
                self.fail_operation(&operation_id, error.clone());
                return Err(error);
            }
        };
        self.bind_accepted_start(&operation_id, &accepted)?;
        let cancelled_before_acceptance = self.inner.state.lock().unwrap().operations
            .get(&operation_id).is_some_and(|record| record.cancel_requested);
        if cancelled_before_acceptance { self.cancel_render(&operation_id).await?; }
        self.refresh_from_service();
        eprintln!("[NOVA_RENDER_PHASE] stage=application_service_accepted operation_id={:?} build_id={} generation={} attached={} viewer_session={:?} elapsed_ms={} GPU_ready=not_inferred",
            operation_id, accepted.build_id, accepted.generation, accepted.attached, accepted.viewer_session_id, started.elapsed().as_millis());

        // The desired presentation can be recorded before a Viewer session
        // exists. Once render-service supplies the session, a later refresh or
        // explicit presentation update applies it without rebuilding.
        if accepted.viewer_session_id.is_some() && request.presentation.visible {
            self.dispatch_current_presentation(&operation_id).await?;
        }

        self.render_state(&operation_id)
            .map(|operation| RenderStartResult {
                operation,
                attached: accepted.attached,
            })
            .ok_or_else(|| "render operation disappeared after acceptance".to_owned())
    }

    pub(crate) async fn cancel_render(&self, operation_id: &OperationId) -> Result<(), String> {
        eprintln!("[NOVA_RENDER_PHASE] stage=application_cancel operation_id={:?} operation_state={:?} scope=build_and_owned_viewer wait=service_ack", operation_id, self.render_state(operation_id).map(|view| view.state));
        let build_id = {
            let mut state = self.inner.state.lock().unwrap();
            let record = state
                .operations
                .get_mut(operation_id)
                .ok_or_else(|| unknown_operation(operation_id))?;
            if matches!(record.view.state, OperationState::Cancelled | OperationState::RuntimeReleased) { return Ok(()); }
            record.cancel_requested = true;
            record.view.presentation = PresentationIntent::default();
            record.view.presentation_state = PresentationState::Hidden;
            record.view.state = OperationState::Running;
            record.view.progress.phase = "cancelling".to_owned();
            record.binding.as_ref().map(|binding| binding.build_id.clone())
        };
        // A StartBuild still waiting for acceptance has no renderer identity
        // yet. bind_accepted_start preserves this barrier and then sends the
        // real CancelBuild as soon as the service returns that identity.
        let Some(build_id) = build_id else { return Ok(()); };
        expect_ack(
            self.inner
                .transport
                .request(UiCommand::CancelBuild { build_id })
                .await?,
        )?;
        if let Some(record) = self.inner.state.lock().unwrap().operations.get_mut(operation_id) {
            // ACK confirms the service's cancellation-intent barrier, not
            // process/GPU reclamation. The supervisor reports actual exit.
            record.view.state = OperationState::Cancelled;
            record.view.progress.phase = "cancelled".to_owned();
            record.view.progress.activity = None;
        }
        self.refresh_from_service();
        Ok(())
    }

    /// Changes only Viewer presentation. Hidden presentation always maps to
    /// HideViewer, never ReleaseViewer.
    pub(crate) async fn update_presentation(
        &self,
        operation_id: &OperationId,
        presentation: PresentationIntent,
    ) -> Result<RenderOperationView, String> {
        eprintln!("[NOVA_RENDER_PHASE] stage=application_presentation operation_id={:?} mode={:?} visible={} placement={:?} scope=presentation_only build_request=false scene_release_request=false",
            operation_id, presentation.mode, presentation.visible, presentation.placement);
        {
            let mut state = self.inner.state.lock().unwrap();
            let record = state
                .operations
                .get_mut(operation_id)
                .ok_or_else(|| unknown_operation(operation_id))?;
            if record.cancel_requested && presentation.visible {
                return Err("预览任务已取消，请重新加载。".to_owned());
            }
            record.view.presentation = presentation;
        }
        self.dispatch_current_presentation(operation_id).await?;
        self.render_state(operation_id)
            .ok_or_else(|| unknown_operation(operation_id))
    }

    pub(crate) async fn show_viewer(
        &self,
        operation_id: &OperationId,
        mode: PresentationMode,
        placement: Option<PresentationPlacement>,
    ) -> Result<RenderOperationView, String> {
        if mode == PresentationMode::Hidden {
            return Err("show_viewer requires popup or overlay presentation mode".to_owned());
        }
        self.update_presentation(
            operation_id,
            PresentationIntent {
                mode,
                visible: true,
                placement,
             presentation_epoch: None },
        )
        .await
    }

    pub(crate) async fn hide_viewer(
        &self,
        operation_id: &OperationId,
    ) -> Result<RenderOperationView, String> {
        let mut presentation = self
            .render_state(operation_id)
            .ok_or_else(|| unknown_operation(operation_id))?
            .presentation;
        presentation.visible = false;
        self.update_presentation(operation_id, presentation).await
    }

    /// Product-semantic E2E operation. The protocol command and Viewer session
    /// identity remain behind the same Application boundary as Show/Hide.
    pub(crate) async fn capture_frame(&self, operation_id: &OperationId, source: &Path, display_mode: RenderDisplayMode) -> Result<PathBuf, String> {
        self.refresh_from_service();
        let snapshot = self.inner.transport.snapshot();
        let session_id = {
            let state = self.inner.state.lock().unwrap();
            let record = state.operations.get(operation_id).ok_or_else(|| unknown_operation(operation_id))?;
            let rejection = if !capture_source_matches(&record.view.source.path, source) {
                Some("source_identity_mismatch")
            } else if record.view.display_mode != display_mode {
                Some("display_mode_mismatch")
            } else if record.cancel_requested {
                Some("cancel_requested")
            } else if !matches!(record.view.state, OperationState::ViewerPrepared | OperationState::RealtimeReady) {
                Some("viewer_not_ready")
            } else { None };
            if let Some(reason) = rejection {
                eprintln!("[NOVA_CAPTURE] stage=guard_rejected operation_id={} state={:?} reason={reason}", operation_id.0, record.view.state);
                return Err("当前投影的渲染尚未就绪，请完成加载后截图".into());
            }
            let Some((binding, session_id)) = record.binding.as_ref()
                .and_then(|binding| binding.session_id.as_ref().map(|session| (binding, session))) else {
                eprintln!("[NOVA_CAPTURE] stage=guard_rejected operation_id={} state={:?} reason=viewer_binding_missing", operation_id.0, record.view.state);
                return Err("当前渲染缺少Viewer会话".into());
            };
            let build_live = snapshot.builds.iter().any(|build| {
                build.build_id == binding.build_id && build.generation == binding.generation
                    && !matches!(build.state, BuildState::Cancelled | BuildState::Failed)
            });
            let viewer_ready = snapshot.viewers.iter().any(|viewer| {
                viewer.session_id == *session_id && viewer.build_id == binding.build_id
                    && viewer.generation == binding.generation
                    && matches!(viewer.runtime_state, ViewerRuntimeState::ViewerPrepared | ViewerRuntimeState::RealtimeReady)
            });
            if !build_live || !viewer_ready {
                eprintln!("[NOVA_CAPTURE] stage=guard_rejected operation_id={} state={:?} reason=viewer_binding_stale", operation_id.0, record.view.state);
                return Err("当前渲染会话已失效，请重新加载后截图".into());
            }
            session_id.clone()
        };
        let output_path = super::thumbnails::temporary_preview_path();
        let error_path = output_path.with_extension("error.txt");
        match self.inner.transport.request(UiCommand::CaptureViewer { session_id, output_path: output_path.clone() }).await? {
            ResponseBody::Ack => {}, other => return Err(format!("unexpected capture response: {other:?}")),
        }
        let started = std::time::Instant::now();
        loop {
            if error_path.is_file() {
                let error = std::fs::read_to_string(&error_path).unwrap_or_else(|_| "截图失败".into());
                let _ = std::fs::remove_file(&error_path); return Err(error);
            }
            if output_path.is_file() { return Ok(output_path); }
            if started.elapsed() > std::time::Duration::from_secs(30) { return Err("当前场景截图超时，请重试".into()); }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    }

    pub(crate) async fn headless_camera_stress(
        &self,
        operation_id: &OperationId,
    ) -> Result<u64, String> {
        self.refresh_from_service();
        let session_id = {
            let state = self.inner.state.lock().unwrap();
            let record = state
                .operations
                .get(operation_id)
                .ok_or_else(|| unknown_operation(operation_id))?;
            if !matches!(
                record.view.state,
                OperationState::ViewerPrepared | OperationState::RealtimeReady
            ) || record.view.presentation_state != PresentationState::Visible
            {
                return Err("camera/zoom stress requires a prepared, visible Viewer".to_owned());
            }
            record
                .binding
                .as_ref()
                .and_then(|binding| binding.session_id.clone())
                .ok_or_else(|| "prepared Viewer has no session".to_owned())?
        };
        match self
            .inner
            .transport
            .request(UiCommand::HeadlessCameraStress { session_id })
            .await?
        {
            ResponseBody::HeadlessCameraStress { frames } if frames > 0 => Ok(frames as u64),
            ResponseBody::HeadlessCameraStress { .. } => {
                Err("camera/zoom stress presented no frames".to_owned())
            }
            other => Err(format!(
                "render service returned an unexpected camera/zoom response: {other:?}"
            )),
        }
    }

    /// Diagnostics only: bind E2E log inspection to this operation's exact
    /// Viewer session rather than to file timestamps or unrelated launches.
    pub(crate) fn headless_viewer_log_paths(
        &self,
        operation_id: &OperationId,
        log_dir: &Path,
    ) -> Result<[PathBuf; 2], String> {
        self.refresh_from_service();
        let state = self.inner.state.lock().unwrap();
        let session_id = state
            .operations
            .get(operation_id)
            .ok_or_else(|| unknown_operation(operation_id))?
            .binding
            .as_ref()
            .and_then(|binding| binding.session_id.as_deref())
            .ok_or_else(|| "render operation has no Viewer session".to_owned())?;
        if session_id.is_empty()
            || !session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("Viewer session id is not a safe log-file component".to_owned());
        }
        Ok([
            log_dir.join(format!("{session_id}-viewer-stdout.log")),
            log_dir.join(format!("{session_id}-viewer-stderr.log")),
        ])
    }

    /// E2E-only compiler proof, independent of Viewer store/GPU watermarks.
    pub(crate) fn headless_compilation_progress(
        &self,
        operation_id: &OperationId,
    ) -> Result<(u64, u64), String> {
        let snapshot = self.inner.transport.snapshot();
        let state = self.inner.state.lock().unwrap();
        let binding = state
            .operations
            .get(operation_id)
            .ok_or_else(|| unknown_operation(operation_id))?
            .binding
            .as_ref()
            .ok_or_else(|| "render operation has no build binding".to_owned())?;
        let build = snapshot
            .builds
            .iter()
            .find(|build| {
                build.build_id == binding.build_id && build.generation == binding.generation
            })
            .ok_or_else(|| "render operation build is missing".to_owned())?;
        match (build.sections_compiled, build.sections_existing) {
            (Some(compiled), Some(existing)) => Ok((compiled, existing)),
            _ => Err("worker exact Cell compilation progress is unavailable".to_owned()),
        }
    }

    /// Reconciles the facade with render-service's authoritative snapshot and
    /// returns only coarse operation views whose presentation-facing value
    /// changed. Renderer counters and runtime identities never leave here.
    pub(crate) fn refresh_from_service(&self) -> Vec<RenderOperationView> {
        let snapshot = self.inner.transport.snapshot();
        let mut state = self.inner.state.lock().unwrap();
        let mut changed = Vec::new();
        for operation_id in state.operation_order.clone() {
            let Some(record) = state.operations.get_mut(&operation_id) else {
                continue;
            };
            let previous = record.view.clone();
            reconcile_operation(record, &snapshot);
            if record.view != previous {
                changed.push(record.view.clone());
            }
        }
        changed
    }

    pub(crate) fn current_render_state(&self) -> Option<RenderOperationView> {
        self.refresh_from_service();
        let state = self.inner.state.lock().unwrap();
        state
            .current_operation
            .as_ref()
            .and_then(|operation_id| state.operations.get(operation_id))
            .map(|record| record.view.clone())
    }

    pub(crate) fn render_state(&self, operation_id: &OperationId) -> Option<RenderOperationView> {
        self.inner
            .state
            .lock()
            .unwrap()
            .operations
            .get(operation_id)
            .map(|record| record.view.clone())
    }

    pub(crate) fn render_states(&self) -> Vec<RenderOperationView> {
        self.refresh_from_service();
        let state = self.inner.state.lock().unwrap();
        state
            .operation_order
            .iter()
            .filter_map(|operation_id| state.operations.get(operation_id))
            .map(|record| record.view.clone())
            .collect()
    }

    fn observe_service_event(
        &self,
        event: &nova_render_protocol::ServiceEvent,
    ) -> Vec<RenderOperationView> {
        match event {
            nova_render_protocol::ServiceEvent::Snapshot(_)
            | nova_render_protocol::ServiceEvent::BuildState(_)
            | nova_render_protocol::ServiceEvent::ArtifactState(_)
            | nova_render_protocol::ServiceEvent::ViewerState(_) => self.refresh_from_service(),
            nova_render_protocol::ServiceEvent::Error(error) if error.fatal => {
                self.fail_active_operations(error.message.clone())
            }
            nova_render_protocol::ServiceEvent::ServiceStopping => self.fail_active_operations(
                "render service stopped before the operation completed".to_owned(),
            ),
            nova_render_protocol::ServiceEvent::Error(_) => Vec::new(),
        }
    }

    fn next_operation_id(&self) -> OperationId {
        let sequence = self
            .inner
            .operation_sequence
            .fetch_add(1, Ordering::Relaxed);
        OperationId(format!("render-{}-{sequence}", std::process::id()))
    }

    fn next_preflight_id(&self) -> ResourcePreflightId {
        let sequence = self
            .inner
            .preflight_sequence
            .fetch_add(1, Ordering::Relaxed);
        ResourcePreflightId(format!("preflight-{}-{sequence}", std::process::id()))
    }

    fn consume_prebuilt_resource_admission(
        &self,
        expected_source: &ProjectSource,
        authorization: RenderStartAuthorization,
    ) -> Result<(UserVisibleRenderRequest, BuildStart, BuildAdmission), String> {
        let preflight_id = authorization
            .preflight_id
            .ok_or_else(|| "prebuilt build requires a fresh resource preflight".to_owned())?;
        let pending = self
            .inner
            .state
            .lock()
            .unwrap()
            .preflights
            .remove(&preflight_id)
            .ok_or_else(|| "resource preflight is unknown or already consumed".to_owned())?;

        if &pending.request.source != expected_source {
            return Err("resource preflight no longer matches the requested source".to_owned());
        }
        if pending.request.settings.build_mode != RenderBuildMode::Prebuilt {
            return Err("resource preflight did not contain a prebuilt request".to_owned());
        }
        if matches!(pending.pressure, ProtocolResourcePressure::Invalid | ProtocolResourcePressure::Unmeasured | ProtocolResourcePressure::HardLimit) {
            return Err("prebuilt resource demand is unknown, invalid or exceeds the 95% limit".to_owned());
        }
        if pending.pressure == ProtocolResourcePressure::Warning && !authorization.resource_warning_confirmed {
            return Err("prebuilt resource pressure above 80% requires explicit confirmation".to_owned());
        }
        let token = pending.admission_token.ok_or_else(|| {
            "admitted resource preflight did not contain a service token".to_owned()
        })?;
        let mut start = pending.prospective_build;
        set_click_diagnostic(&mut start, authorization.click_started_at_epoch_ms);
        Ok((
            pending.request,
            start,
            BuildAdmission {
                token,
                resource_warning_confirmed: authorization.resource_warning_confirmed,
            },
        ))
    }

    fn bind_accepted_start(
        &self,
        operation_id: &OperationId,
        accepted: &BuildAccepted,
    ) -> Result<(), String> {
        let mut state = self.inner.state.lock().unwrap();
        let record = state
            .operations
            .get_mut(operation_id)
            .ok_or_else(|| unknown_operation(operation_id))?;
        record.binding = Some(RenderBinding {
            build_id: accepted.build_id.clone(),
            generation: accepted.generation,
            session_id: accepted.viewer_session_id.clone(),
        });
        record.view.state = OperationState::Running;
        record.view.progress.phase = if record.cancel_requested { "cancelling" } else { "starting" }.to_owned();
        Ok(())
    }

    fn fail_operation(&self, operation_id: &OperationId, error: String) {
        if let Some(record) = self
            .inner
            .state
            .lock()
            .unwrap()
            .operations
            .get_mut(operation_id)
        {
            record.view.state = OperationState::Failed;
            record.view.progress.phase = "failed".to_owned();
            record.view.error = Some(error);
        }
    }

    fn fail_active_operations(&self, error: String) -> Vec<RenderOperationView> {
        let mut state = self.inner.state.lock().unwrap();
        let mut changed = Vec::new();
        for record in state.operations.values_mut() {
            if is_attachable(record.view.state) {
                record.view.state = OperationState::Failed;
                record.view.progress.phase = "failed".to_owned();
                record.view.error = Some(error.clone());
                changed.push(record.view.clone());
            }
        }
        changed
    }

    async fn dispatch_current_presentation(
        &self,
        operation_id: &OperationId,
    ) -> Result<(), String> {
        let command = {
            let state = self.inner.state.lock().unwrap();
            let record = state
                .operations
                .get(operation_id)
                .ok_or_else(|| unknown_operation(operation_id))?;
            let Some(session_id) = record
                .binding
                .as_ref()
                .and_then(|binding| binding.session_id.clone())
            else {
                return Ok(());
            };
            let binding = record.binding.as_ref().unwrap();
            let presentation = if record.cancel_requested { PresentationIntent::default() } else { record.view.presentation.clone() };
            presentation_command(session_id, binding.build_id.clone(), binding.generation, &presentation)?
        };
        expect_ack(self.inner.transport.request(command).await?)?;
        self.refresh_from_service();
        Ok(())
    }
}

fn validate_preflight_response(response: &BuildResourcePreflight) -> Result<(), String> {
    if response.pressure == ProtocolResourcePressure::Invalid {
        if response.admission_token.is_some() || response.pressure_basis_points.is_some() {
            return Err("invalid resource inputs must not grant an admission token or a pressure percentage".to_owned());
        }
        if response.invalid_reason.as_deref().is_none_or(str::is_empty) {
            return Err("invalid resource inputs must explain the failure".to_owned());
        }
        return Ok(());
    }
    if response.pressure == ProtocolResourcePressure::Unmeasured {
        if response.conservative_vram_bytes.is_some() != response.components.is_some()
            || response.pressure_basis_points.is_some() {
            return Err("unmeasured resources must not contain fabricated GPU allocation values".to_owned());
        }
    } else if response.conservative_vram_bytes.is_none() || response.components.is_none()
        || response.invalid_reason.is_some() {
        return Err("measured resources require complete GPU allocation components".to_owned());
    }
    if matches!(response.pressure, ProtocolResourcePressure::Unmeasured | ProtocolResourcePressure::HardLimit) {
        if response.admission_token.is_some() {
            return Err("unknown or hard-limit resources must not grant a prebuilt admission token".to_owned());
        }
    } else if response.admission_token.as_deref().is_none_or(str::is_empty) {
        return Err("admitted resource inputs require their service admission token".to_owned());
    }
    Ok(())
}

fn resource_preflight_view(
    preflight_id: ResourcePreflightId,
    response: &BuildResourcePreflight,
    basic_vulkan_available: bool,
) -> ResourcePreflightView {
    let severity = match response.pressure {
        ProtocolResourcePressure::Unmeasured => ResourcePreflightSeverity::Unmeasured,
        ProtocolResourcePressure::Normal => ResourcePreflightSeverity::Normal,
        ProtocolResourcePressure::Warning => ResourcePreflightSeverity::Warning,
        ProtocolResourcePressure::HardLimit => ResourcePreflightSeverity::Hard,
        ProtocolResourcePressure::Invalid => ResourcePreflightSeverity::Invalid,
    };
    let mut allowed_actions = if matches!(response.pressure, ProtocolResourcePressure::Invalid | ProtocolResourcePressure::Unmeasured | ProtocolResourcePressure::HardLimit) {
        vec![ResourcePreflightAction::SwitchRealtime, ResourcePreflightAction::Cancel]
    } else if response.pressure == ProtocolResourcePressure::Normal {
        vec![ResourcePreflightAction::ContinuePrebuilt]
    } else {
        vec![ResourcePreflightAction::ContinuePrebuilt, ResourcePreflightAction::SwitchRealtime,
            ResourcePreflightAction::Cancel]
    };
    if basic_vulkan_available && response.pressure != ProtocolResourcePressure::Invalid {
        let cancel_index = allowed_actions.iter().position(|action| *action == ResourcePreflightAction::Cancel)
            .unwrap_or(allowed_actions.len());
        allowed_actions.insert(cancel_index, ResourcePreflightAction::UseBasicVulkan);
    }
    ResourcePreflightView {
        preflight_id,
        severity,
        conservative_vram_bytes: response.conservative_vram_bytes,
        safe_available_vram_bytes: response.safe_available_vram_bytes,
        pressure_percent: response
            .pressure_basis_points
            .map(|basis_points| basis_points as f64 / 100.0),
        components: response
            .components
            .map(|components| ResourcePreflightComponents {
                exact_geometry_bytes: components.exact_geometry_bytes,
                draw_metadata_bytes: components.draw_metadata_bytes,
                block_entity_bytes: components.block_entity_bytes,
                texture_and_shader_bytes: components.texture_and_shader_bytes,
                render_target_bytes: components.render_target_bytes,
                runtime_reserve_bytes: components.runtime_reserve_bytes,
            }),
        invalid_reason: response.invalid_reason.clone(),
        allowed_actions,
    }
}

fn basic_vulkan_is_available(request: &UserVisibleRenderRequest) -> bool {
    request.settings.backend != GraphicsBackendPreference::Vulkan
        && request.settings.shader.path.is_some()
}

/// Converts the existing persisted spelling into Application semantics. The
/// old stable value is `iris-gl46`; serde's `iris_gl46` spelling is accepted as
/// input during migration but is never written back as the persisted value.
pub(crate) fn graphics_backend_from_persisted(
    value: &str,
) -> Result<GraphicsBackendPreference, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(GraphicsBackendPreference::Auto),
        "vulkan" => Ok(GraphicsBackendPreference::Vulkan),
        "iris-gl46" | "iris_gl46" => Ok(GraphicsBackendPreference::IrisGl46),
        value => Err(format!(
            "V5 backend {value:?} invalid; expected auto, vulkan, or iris-gl46"
        )),
    }
}

pub(crate) const fn graphics_backend_persisted_value(
    value: GraphicsBackendPreference,
) -> &'static str {
    match value {
        GraphicsBackendPreference::Auto => "auto",
        GraphicsBackendPreference::Vulkan => "vulkan",
        GraphicsBackendPreference::IrisGl46 => "iris-gl46",
    }
}

fn derive_build_start(
    request: &UserVisibleRenderRequest,
    environment: RenderEnvironmentInputs,
) -> BuildStart {
    let display_mode = request.settings.display_mode.as_str().to_owned();
    BuildStart {
        source_path: request.source.path.clone(),
        display_mode: display_mode.clone(),
        build_mode: match request.settings.build_mode {
            RenderBuildMode::Prebuilt => ProtocolRenderBuildMode::Prebuilt,
            RenderBuildMode::Realtime => ProtocolRenderBuildMode::Realtime,
        },
        content: SceneContent {
            fabric_mods_dir: request.settings.fabric_mods_dir.clone(),
            resource_packs: request.settings.resource_packs.clone(),
            asset_inputs: environment.asset_inputs,
        },
        // Keep the established identity material byte-for-byte compatible for
        // the default product route. Layer/highlight data is not smuggled into
        // an unowned renderer map while the data-plane contract is frozen.
        build_options: BTreeMap::from([("displayMode".to_owned(), display_mode)]),
        presentation: derive_presentation_spec(&request.settings, environment.viewport),
    }
}

fn derive_presentation_spec(settings: &ProjectRenderSettings, viewport: Option<[u32; 2]>) -> PresentationSpec {
    let backend = match settings.backend {
        GraphicsBackendPreference::Auto => RenderBackend::Auto,
        GraphicsBackendPreference::Vulkan => RenderBackend::Vulkan,
        GraphicsBackendPreference::IrisGl46 => RenderBackend::IrisGl46,
    };
    let iris_fallback = match settings.iris_fallback {
        IrisFallbackPreference::Fail => "fail",
        IrisFallbackPreference::Vulkan => "vulkan",
    };
    let precipitation = match settings.precipitation {
        WeatherPreference::Auto => "auto",
        WeatherPreference::Rain => "rain",
        WeatherPreference::Snow => "snow",
        WeatherPreference::None => "none",
    };
    let mut spec = PresentationSpec {
        backend,
        shaderpack: settings.shader.path.clone(),
        render_settings: BTreeMap::from([
            ("effectsProfile".to_owned(), if settings.display_mode == RenderDisplayMode::Standard { "unlit" } else { "full" }.to_owned()),
            ("irisFallback".to_owned(), iris_fallback.to_owned()),
            ("precipitation".to_owned(), precipitation.to_owned()),
            (
                "allowTimeModification".to_owned(),
                settings.allow_time_modification.to_string(),
            ),
            (
                "allowWeatherModification".to_owned(),
                settings.allow_weather_modification.to_string(),
            ),
        ]),
    };
    // The admission meter needs the real physical target, not a camera default
    // or CSS size. Keep it frozen with the prospective request across the modal.
    if let Some([width, height]) = viewport {
        spec.render_settings.insert("width".to_owned(), width.to_string());
        spec.render_settings.insert("height".to_owned(), height.to_string());
    }
    spec
}

fn set_click_diagnostic(start: &mut BuildStart, click_started_at_epoch_ms: Option<u64>) {
    if let Some(epoch) = click_started_at_epoch_ms.filter(|value| *value != 0) {
        start.presentation.render_settings.insert("clickStartedAtEpochMs".to_owned(), epoch.to_string());
    }
}

fn presentation_command(
    session_id: String, build_id: String, generation: u64, presentation: &PresentationIntent,
) -> Result<UiCommand, String> {
    let state = if !presentation.visible || presentation.mode == PresentationMode::Hidden {
        ViewerPresentationState::Hidden
    } else {
        match presentation.mode {
            PresentationMode::Hidden => ViewerPresentationState::Hidden,
            PresentationMode::Popup => ViewerPresentationState::PopupVisible,
            PresentationMode::Overlay | PresentationMode::Embedded => {
                let placement = presentation.placement.ok_or_else(|| "embedded preview needs a viewport".to_owned())?;
                ViewerPresentationState::EmbeddedOverlayVisible { rect: ScreenRect {
                    x: placement.x, y: placement.y, width: placement.width, height: placement.height, focused: placement.focused,
                } }
            }
        }
    };
    Ok(UiCommand::SetBoundPresentation { session_id, build_id, generation, state,
        presentation_epoch: presentation.presentation_epoch,
        dpi_scale: presentation.placement.map(|placement| placement.dpi_scale) })
}

fn reconcile_operation(record: &mut OperationRecord, snapshot: &ServiceSnapshot) {
    let Some(binding) = record.binding.as_mut() else {
        return;
    };
    let Some(build) = snapshot
        .builds
        .iter()
        .find(|build| build.build_id == binding.build_id && build.generation == binding.generation)
    else {
        return;
    };

    if record.cancel_requested && !matches!(build.state, BuildState::Cancelled | BuildState::Failed) {
        return;
    }

    record.view.progress = OperationProgress {
        phase: build.phase.clone(),
        activity: build.activity.clone(),
        completed: build.processed_units,
        total: build.total_units,
        elapsed_ms: build.elapsed_ms,
    };
    record.view.error = build.error.as_ref().map(|error| error.message.clone());
    let build_completed = matches!(build.state, BuildState::BuildCompleted);
    record.view.state = match build.state {
        BuildState::Starting
        | BuildState::Scanning
        | BuildState::Normalizing
        | BuildState::Building
        | BuildState::Finalizing
        | BuildState::BuildCompleted
        | BuildState::ArtifactReady => OperationState::Running,
        BuildState::Cancelled => OperationState::Cancelled,
        BuildState::Failed => OperationState::Failed,
    };

    if matches!(
        record.view.state,
        OperationState::Cancelled | OperationState::Failed
    ) {
        return;
    }

    let ready_artifact = build.scene_identity.as_ref().and_then(|scene_identity| {
        snapshot.artifacts.iter().find(|artifact| {
            artifact.scene_identity == *scene_identity
                && artifact.generation == binding.generation
                && artifact.state == ArtifactState::Ready
        })
    });
    if let Some(artifact) = ready_artifact {
        record.view.analysis = artifact
            .analysis_summary
            .as_ref()
            .and_then(|analysis| serde_json::from_value::<ProjectAnalysis>(analysis.clone()).ok());
    }

    let viewer = binding
        .session_id
        .as_ref()
        .and_then(|session_id| {
            snapshot.viewers.iter().find(|viewer| {
                viewer.session_id == *session_id && viewer.generation == binding.generation
            })
        })
        .or_else(|| {
            snapshot.viewers.iter().rev().find(|viewer| {
                viewer.build_id == binding.build_id
                    && viewer.generation == binding.generation
                    && !matches!(&viewer.runtime_state, ViewerRuntimeState::Released)
            })
        });
    let Some(viewer) = viewer else {
        return;
    };
    binding.session_id = Some(viewer.session_id.clone());
    record.view.presentation_state = match &viewer.presentation_state {
        ViewerPresentationState::Hidden => PresentationState::Hidden,
        ViewerPresentationState::PopupVisible
        | ViewerPresentationState::EmbeddedOverlayVisible { .. } => PresentationState::Visible,
    };

    record.view.state = match &viewer.runtime_state {
        ViewerRuntimeState::Released => {
            binding.session_id = None;
            record.view.presentation_state = PresentationState::Hidden;
            OperationState::RuntimeReleased
        }
        ViewerRuntimeState::Failed => OperationState::Failed,
        ViewerRuntimeState::RealtimeReady => OperationState::RealtimeReady,
        // Runtime milestones are authoritative only after the matching
        // artifact is ready. ViewerPrepared additionally requires the service's
        // atomic BuildCompleted barrier; artifact publication alone is not a
        // product completion state.
        ViewerRuntimeState::Preparing
        | ViewerRuntimeState::GpuAccepted
        | ViewerRuntimeState::GpuResident
            if ready_artifact.is_none() =>
        {
            return;
        }
        ViewerRuntimeState::ViewerPrepared if ready_artifact.is_none() || !build_completed => {
            return;
        }
        ViewerRuntimeState::Preparing => OperationState::ViewerPreparing,
        ViewerRuntimeState::GpuAccepted => OperationState::GpuAccepted,
        ViewerRuntimeState::GpuResident => OperationState::GpuResident,
        ViewerRuntimeState::ViewerPrepared => OperationState::ViewerPrepared,
    };
    record.view.progress.phase = match record.view.state {
        OperationState::ViewerPreparing => "viewer_preparing",
        OperationState::RealtimeReady => "realtime_ready",
        OperationState::GpuAccepted => "gpu_accepted",
        OperationState::GpuResident => "gpu_resident",
        OperationState::ViewerPrepared => "viewer_prepared",
        OperationState::RuntimeReleased => "runtime_released",
        OperationState::Failed => "failed",
        _ => unreachable!("Viewer runtime must map to a Viewer-stage operation state"),
    }
    .to_owned();
    record.view.progress.activity = None;
    record.view.progress.completed = None;
    record.view.progress.total = None;
    record.view.progress.elapsed_ms = viewer.elapsed_ms;
    if viewer.error.is_some() {
        record.view.error = viewer.error.as_ref().map(|error| error.message.clone());
    }
}

const fn is_attachable(state: OperationState) -> bool {
    !matches!(
        state,
        OperationState::RuntimeReleased | OperationState::Cancelled | OperationState::Failed
    )
}

fn expect_ack(response: ResponseBody) -> Result<(), String> {
    match response {
        ResponseBody::Ack => Ok(()),
        other => Err(format!(
            "render service returned an unexpected acknowledgement: {other:?}"
        )),
    }
}

// Keep the existing equal-path fast path. Nonidentical representations must
// resolve successfully on BOTH sides; two missing/inaccessible paths never match.
fn capture_source_matches(rendered: &Path, requested: &Path) -> bool {
    if rendered == requested { return true; }
    match (std::fs::canonicalize(rendered), std::fs::canonicalize(requested)) {
        (Ok(rendered), Ok(requested)) => rendered == requested,
        _ => false,
    }
}

fn unknown_operation(operation_id: &OperationId) -> String {
    format!("unknown render operation {}", operation_id.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_render_protocol::{
        ArtifactSnapshot, BuildSnapshot, ServiceError, ViewerPresentationState, ViewerRuntimeState,
        ViewerSnapshot,
    };
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeTransport {
        commands: Mutex<Vec<UiCommand>>,
        responses: Mutex<VecDeque<Result<ResponseBody, String>>>,
        deferred: Mutex<VecDeque<tokio::sync::oneshot::Receiver<Result<ResponseBody, String>>>>,
        snapshot: Mutex<ServiceSnapshot>,
    }

    impl FakeTransport {
        fn push_response(&self, response: ResponseBody) {
            self.responses.lock().unwrap().push_back(Ok(response));
        }

        fn set_snapshot(&self, snapshot: ServiceSnapshot) {
            *self.snapshot.lock().unwrap() = snapshot;
        }

        fn defer_response(&self) -> tokio::sync::oneshot::Sender<Result<ResponseBody, String>> {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.deferred.lock().unwrap().push_back(receiver);
            sender
        }
    }

    impl RenderTransport for FakeTransport {
        fn request<'a>(
            &'a self,
            command: UiCommand,
        ) -> Pin<Box<dyn Future<Output = Result<ResponseBody, String>> + Send + 'a>> {
            self.commands.lock().unwrap().push(command);
            let deferred = self.deferred.lock().unwrap().pop_front();
            let response = deferred.is_none().then(|| self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("missing fake response".to_owned())));
            Box::pin(async move {
                match deferred {
                    Some(receiver) => receiver.await.map_err(|error| error.to_string())?,
                    None => response.expect("immediate fake response"),
                }
            })
        }

        fn snapshot(&self) -> ServiceSnapshot {
            self.snapshot.lock().unwrap().clone()
        }

        fn subscribe_events(
            &self,
        ) -> std::sync::mpsc::Receiver<nova_render_protocol::ServiceEvent> {
            let (_sender, receiver) = std::sync::mpsc::channel();
            receiver
        }
    }

    fn request() -> UserVisibleRenderRequest {
        UserVisibleRenderRequest {
            source: ProjectSource {
                path: PathBuf::from(r"C:\projections\castle.litematic"),
            },
            settings: ProjectRenderSettings::default(),
            presentation: PresentationIntent {
                mode: PresentationMode::Popup,
                visible: false,
                placement: None,
             presentation_epoch: None },
        }
    }

    fn accepted(session_id: Option<&str>) -> ResponseBody {
        ResponseBody::BuildAccepted(BuildAccepted {
            build_id: "build-1".to_owned(),
            generation: 7,
            provisional_scene_key: "scene-provisional".to_owned(),
            viewer_session_id: session_id.map(ToOwned::to_owned),
            attached: false,
        })
    }

    fn preflight_response(
        pressure: ProtocolResourcePressure,
        admission_token: Option<&str>,
    ) -> ResponseBody {
        let conservative_vram_bytes = match pressure {
            ProtocolResourcePressure::Normal => 7_900,
            ProtocolResourcePressure::Warning => 8_500,
            ProtocolResourcePressure::HardLimit => 9_500,
            ProtocolResourcePressure::Invalid | ProtocolResourcePressure::Unmeasured => 0,
        };
        ResponseBody::BuildPreflight(BuildResourcePreflight {
            pressure,
            conservative_vram_bytes: (!matches!(pressure, ProtocolResourcePressure::Invalid | ProtocolResourcePressure::Unmeasured))
                .then_some(conservative_vram_bytes),
            safe_available_vram_bytes: (pressure != ProtocolResourcePressure::Invalid)
                .then_some(10_000),
            pressure_basis_points: (!matches!(pressure, ProtocolResourcePressure::Invalid | ProtocolResourcePressure::Unmeasured))
                .then_some(conservative_vram_bytes as u64),
            components: (!matches!(pressure, ProtocolResourcePressure::Invalid | ProtocolResourcePressure::Unmeasured)).then(|| {
                nova_render_protocol::ConservativeVramComponents {
                    exact_geometry_bytes: conservative_vram_bytes - 3_500,
                    draw_metadata_bytes: 500,
                    block_entity_bytes: 250,
                    texture_and_shader_bytes: 1_000,
                    render_target_bytes: 750,
                    runtime_reserve_bytes: 1_000,
                }
            }),
            invalid_reason: (pressure == ProtocolResourcePressure::Invalid)
                .then_some("metadata estimate unavailable".to_owned()),
            admission_token: admission_token.map(ToOwned::to_owned),
        })
    }

    fn facade(fake: Arc<FakeTransport>) -> RenderFacade {
        RenderFacade::with_transport(fake)
    }

    async fn start_for_test(
        facade: &RenderFacade,
        request: UserVisibleRenderRequest,
        environment: RenderEnvironmentInputs,
    ) -> Result<RenderStartResult, String> {
        if request.settings.build_mode == RenderBuildMode::Prebuilt {
            let preflight_id = facade.next_preflight_id();
            facade.inner.state.lock().unwrap().preflights.insert(
                preflight_id.clone(),
                PendingResourcePreflight {
                    request: request.clone(),
                    environment: environment.clone(),
                    prospective_build: derive_build_start(&request, environment),
                    pressure: ProtocolResourcePressure::Normal,
                    admission_token: Some(format!("admission-{}", preflight_id.0)),
                },
            );
            facade
                .start_prebuilt_render(
                    request.source,
                    RenderStartAuthorization {
                click_started_at_epoch_ms: None,
                        preflight_id: Some(preflight_id),
                        resource_warning_confirmed: false,
                    },
                )
                .await
        } else {
            facade.start_realtime_render(request, environment).await
        }
    }

    #[test]
    fn persisted_backend_adapter_preserves_hyphenated_iris_value() {
        assert_eq!(
            graphics_backend_from_persisted("iris-gl46").unwrap(),
            GraphicsBackendPreference::IrisGl46
        );
        assert_eq!(
            graphics_backend_from_persisted("iris_gl46").unwrap(),
            GraphicsBackendPreference::IrisGl46
        );
        assert_eq!(
            graphics_backend_persisted_value(GraphicsBackendPreference::IrisGl46),
            "iris-gl46"
        );
    }

    #[test]
    fn semantic_request_derives_the_established_wire_config_without_io() {
        let mut request = request();
        request.settings.backend = GraphicsBackendPreference::IrisGl46;
        request.settings.shader.path = Some(PathBuf::from(r"C:\packs\Complementary.zip"));
        request.settings.iris_fallback = IrisFallbackPreference::Vulkan;
        request.settings.precipitation = WeatherPreference::Snow;
        request.settings.fabric_mods_dir = Some(PathBuf::from(r"C:\mods"));
        request.settings.resource_packs = vec![PathBuf::from(r"C:\packs\Faithful.zip")];
        let start = derive_build_start(
            &request,
            RenderEnvironmentInputs {
                asset_inputs: vec![PathBuf::from(r"C:\runtime\backend-deployment.json")],
                viewport: Some([2316, 1178]),
            },
        );

        assert_eq!(start.source_path, request.source.path);
        assert_eq!(start.display_mode, "full");
        assert_eq!(start.build_mode, ProtocolRenderBuildMode::Prebuilt);
        assert_eq!(start.build_options.len(), 1);
        assert_eq!(start.build_options.get("displayMode").unwrap(), "full");
        assert_eq!(start.presentation.backend, RenderBackend::IrisGl46);
        assert_eq!(
            start
                .presentation
                .render_settings
                .get("irisFallback")
                .unwrap(),
            "vulkan"
        );
        assert_eq!(
            start
                .presentation
                .render_settings
                .get("precipitation")
                .unwrap(),
            "snow"
        );
        assert_eq!(
            start.content.resource_packs,
            request.settings.resource_packs
        );
        assert_eq!(start.content.asset_inputs.len(), 1);
        assert_eq!(start.presentation.render_settings.get("width").map(String::as_str), Some("2316"));
        assert_eq!(start.presentation.render_settings.get("height").map(String::as_str), Some("1178"));
    }

    #[test]
    fn standard_and_full_preflight_freeze_physical_viewport_for_admitted_build() {
        for display_mode in [RenderDisplayMode::Standard, RenderDisplayMode::Full] {
            let fake = Arc::new(FakeTransport::default());
            fake.push_response(preflight_response(ProtocolResourcePressure::Normal, Some("physical-size-admission")));
            fake.push_response(accepted(None));
            let facade = facade(fake.clone());
            let mut request = request();
            request.settings.display_mode = display_mode;
            let environment = RenderEnvironmentInputs { asset_inputs: Vec::new(), viewport: Some([2316, 1178]) };
            let assessment = tauri::async_runtime::block_on(facade.preflight_render(request.clone(), environment)).unwrap();
            tauri::async_runtime::block_on(facade.start_prebuilt_render(request.source.clone(), RenderStartAuthorization {
                preflight_id: Some(assessment.preflight_id), resource_warning_confirmed: false, click_started_at_epoch_ms: None,
            })).unwrap();
            let commands = fake.commands.lock().unwrap();
            let UiCommand::PreflightBuild(preflight) = &commands[0] else { panic!("preflight first") };
            let UiCommand::StartBuild(start) = &commands[1] else { panic!("admitted build second") };
            assert_eq!(preflight.presentation.render_settings.get("width").map(String::as_str), Some("2316"));
            assert_eq!(preflight.presentation.render_settings.get("height").map(String::as_str), Some("1178"));
            assert_eq!(&start.build, preflight);
            assert_eq!(preflight.display_mode, display_mode.as_str());
        }
    }

    #[test]
    fn normal_preflight_hides_wire_token_and_starts_the_exact_prospective_build() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(preflight_response(
            ProtocolResourcePressure::Normal,
            Some("wire-admission-normal"),
        ));
        fake.push_response(accepted(None));
        let facade = facade(fake.clone());
        let request = request();
        let environment = RenderEnvironmentInputs::default();

        let preflight = tauri::async_runtime::block_on(
            facade.preflight_render(request.clone(), environment.clone()),
        )
        .unwrap();
        assert_eq!(preflight.severity, ResourcePreflightSeverity::Normal);
        assert_eq!(
            preflight.allowed_actions,
            vec![ResourcePreflightAction::ContinuePrebuilt]
        );
        assert_eq!(preflight.pressure_percent, Some(79.0));

        tauri::async_runtime::block_on(facade.start_prebuilt_render(
            request.source,
            RenderStartAuthorization {
                click_started_at_epoch_ms: None,
                preflight_id: Some(preflight.preflight_id),
                resource_warning_confirmed: false,
            },
        ))
        .unwrap();

        let commands = fake.commands.lock().unwrap();
        let UiCommand::PreflightBuild(preflight_build) = &commands[0] else {
            panic!("first command must be PreflightBuild")
        };
        let UiCommand::StartBuild(start) = &commands[1] else {
            panic!("second command must be StartBuild")
        };
        assert_eq!(&start.build, preflight_build);
        assert_eq!(
            start.admission,
            Some(BuildAdmission {
                token: "wire-admission-normal".to_owned(),
                resource_warning_confirmed: false,
            })
        );
    }

    #[test]
    fn basic_vulkan_action_requires_actual_shader_saving_not_memory_pressure() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(preflight_response(
            ProtocolResourcePressure::Normal,
            Some("normal"),
        ));
        fake.push_response(preflight_response(
            ProtocolResourcePressure::Warning,
            Some("warning"),
        ));
        fake.push_response(preflight_response(
            ProtocolResourcePressure::HardLimit,
            None,
        ));
        fake.push_response(preflight_response(
            ProtocolResourcePressure::Warning,
            Some("no-shader"),
        ));
        fake.push_response(preflight_response(
            ProtocolResourcePressure::Warning,
            Some("already-vulkan"),
        ));
        let facade = facade(fake);
        let mut shader_request = request();
        shader_request.settings.backend = GraphicsBackendPreference::IrisGl46;
        shader_request.settings.shader.path = Some(PathBuf::from(r"C:\packs\shader.zip"));

        let normal = tauri::async_runtime::block_on(
            facade.preflight_render(shader_request.clone(), RenderEnvironmentInputs::default()),
        )
        .unwrap();
        assert!(
            normal
                .allowed_actions
                .contains(&ResourcePreflightAction::UseBasicVulkan)
        );

        let warning = tauri::async_runtime::block_on(
            facade.preflight_render(shader_request.clone(), RenderEnvironmentInputs::default()),
        )
        .unwrap();
        assert!(
            warning
                .allowed_actions
                .contains(&ResourcePreflightAction::UseBasicVulkan)
        );

        let hard = tauri::async_runtime::block_on(
            facade.preflight_render(shader_request, RenderEnvironmentInputs::default()),
        )
        .unwrap();
        assert!(
            hard.allowed_actions
                .contains(&ResourcePreflightAction::UseBasicVulkan)
        );

        let no_shader = tauri::async_runtime::block_on(
            facade.preflight_render(request(), RenderEnvironmentInputs::default()),
        )
        .unwrap();
        assert!(
            !no_shader
                .allowed_actions
                .contains(&ResourcePreflightAction::UseBasicVulkan)
        );

        let mut vulkan_shader = request();
        vulkan_shader.settings.backend = GraphicsBackendPreference::Vulkan;
        vulkan_shader.settings.shader.path = Some(PathBuf::from(r"C:\packs\shader.zip"));
        let already_vulkan = tauri::async_runtime::block_on(
            facade.preflight_render(vulkan_shader, RenderEnvironmentInputs::default()),
        )
        .unwrap();
        assert!(
            !already_vulkan
                .allowed_actions
                .contains(&ResourcePreflightAction::UseBasicVulkan)
        );
    }

    #[test]
    fn basic_vulkan_repreflight_freezes_the_optimized_build_and_invalidates_the_old_id() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(preflight_response(
            ProtocolResourcePressure::HardLimit,
            None,
        ));
        fake.push_response(preflight_response(
            ProtocolResourcePressure::Normal,
            Some("wire-admission-basic-vulkan"),
        ));
        fake.push_response(accepted(None));
        let facade = facade(fake.clone());
        let mut request = request();
        request.settings.backend = GraphicsBackendPreference::IrisGl46;
        request.settings.shader.path = Some(PathBuf::from(r"C:\packs\shader.zip"));
        let environment = RenderEnvironmentInputs {
            asset_inputs: vec![PathBuf::from(r"C:\runtime\deployment.json")],
            viewport: Some([1280, 720]),
        };

        let original = tauri::async_runtime::block_on(
            facade.preflight_render(request.clone(), environment.clone()),
        )
        .unwrap();
        let optimized = tauri::async_runtime::block_on(
            facade.use_basic_vulkan(&original.preflight_id, &request.source),
        )
        .unwrap();
        assert_ne!(optimized.preflight_id, original.preflight_id);
        assert_eq!(optimized.severity, ResourcePreflightSeverity::Normal);
        assert_eq!(
            optimized.allowed_actions,
            vec![ResourcePreflightAction::ContinuePrebuilt]
        );

        let replay_error = tauri::async_runtime::block_on(
            facade.use_basic_vulkan(&original.preflight_id, &request.source),
        )
        .unwrap_err();
        assert!(replay_error.contains("already consumed"));

        tauri::async_runtime::block_on(facade.start_prebuilt_render(
            request.source,
            RenderStartAuthorization {
                click_started_at_epoch_ms: None,
                preflight_id: Some(optimized.preflight_id),
                resource_warning_confirmed: false,
            },
        ))
        .unwrap();

        let commands = fake.commands.lock().unwrap();
        let UiCommand::PreflightBuild(original_build) = &commands[0] else {
            panic!("first command must be the original PreflightBuild")
        };
        assert_eq!(original_build.presentation.backend, RenderBackend::IrisGl46);
        assert!(original_build.presentation.shaderpack.is_some());
        let UiCommand::PreflightBuild(optimized_build) = &commands[1] else {
            panic!("second command must be the optimized PreflightBuild")
        };
        assert_eq!(optimized_build.presentation.backend, RenderBackend::Vulkan);
        assert!(optimized_build.presentation.shaderpack.is_none());
        let UiCommand::StartBuild(start) = &commands[2] else {
            panic!("third command must be StartBuild")
        };
        assert_eq!(&start.build, optimized_build);
    }

    fn assert_memory_preflight_starts_without_capacity_gate(pressure: ProtocolResourcePressure, basic: bool) {
        let fake = Arc::new(FakeTransport::default());
        let admitted = matches!(pressure, ProtocolResourcePressure::Normal | ProtocolResourcePressure::Warning);
        fake.push_response(preflight_response(pressure, admitted.then_some("source-token")));
        if basic { fake.push_response(preflight_response(pressure, admitted.then_some("optimized-token"))); }
        fake.push_response(accepted(None));
        let facade = facade(fake.clone());
        let mut request = request();
        if basic {
            request.settings.backend = GraphicsBackendPreference::IrisGl46;
            request.settings.shader.path = Some(PathBuf::from(r"C:\packs\shader.zip"));
        }
        let mut assessment = tauri::async_runtime::block_on(facade.preflight_render(request.clone(), RenderEnvironmentInputs::default())).unwrap();
        if basic {
            assessment = tauri::async_runtime::block_on(facade.use_basic_vulkan(&assessment.preflight_id, &request.source)).unwrap();
        }
        assert_eq!(assessment.allowed_actions.contains(&ResourcePreflightAction::ContinuePrebuilt), admitted);
        let id = assessment.preflight_id;
        if !admitted {
            let error = tauri::async_runtime::block_on(facade.start_prebuilt_render(request.source,
                RenderStartAuthorization { preflight_id: Some(id), resource_warning_confirmed: true,
                    click_started_at_epoch_ms: None })).unwrap_err();
            assert!(error.contains("unknown, invalid or exceeds"));
            assert_eq!(fake.commands.lock().unwrap().len(), if basic { 2 } else { 1 });
            return;
        }
        tauri::async_runtime::block_on(facade.start_prebuilt_render(request.source.clone(),
            RenderStartAuthorization { preflight_id: Some(id.clone()), resource_warning_confirmed: pressure == ProtocolResourcePressure::Warning,
                click_started_at_epoch_ms: None })).unwrap();
        let replay = tauri::async_runtime::block_on(facade.start_prebuilt_render(request.source.clone(),
            RenderStartAuthorization { preflight_id: Some(id), resource_warning_confirmed: true,
                click_started_at_epoch_ms: None })).unwrap_err();
        assert!(replay.contains("already consumed"));
        let commands = fake.commands.lock().unwrap();
        assert_eq!(commands.len(), if basic { 3 } else { 2 });
        let UiCommand::StartBuild(start) = commands.last().unwrap() else { panic!("real StartBuild required"); };
        assert_eq!(start.build.source_path, request.source.path);
        assert_eq!(start.build.build_mode, ProtocolRenderBuildMode::Prebuilt);
        let admission = start.admission.as_ref().unwrap();
        assert_eq!(admission.resource_warning_confirmed, pressure == ProtocolResourcePressure::Warning);
        assert_eq!(admission.token, if basic { "optimized-token" } else { "source-token" });
        if basic { assert_eq!(start.build.presentation.backend, RenderBackend::Vulkan); assert!(start.build.presentation.shaderpack.is_none()); }
    }

    #[test]
    fn basic_vulkan_preflight_hard_measurement_blocks_preloaded_start() {
        assert_memory_preflight_starts_without_capacity_gate(ProtocolResourcePressure::HardLimit, true);
    }

    #[test]
    fn basic_vulkan_preflight_warning_carries_explicit_memory_confirmation() {
        assert_memory_preflight_starts_without_capacity_gate(ProtocolResourcePressure::Warning, true);
    }

    #[test]
    fn basic_vulkan_rejects_requests_without_an_actual_shader_saving() {
        let no_shader = request();
        let mut already_vulkan = request();
        already_vulkan.settings.backend = GraphicsBackendPreference::Vulkan;
        already_vulkan.settings.shader.path = Some(PathBuf::from(r"C:\packs\shader.zip"));

        for request in [no_shader, already_vulkan] {
            let fake = Arc::new(FakeTransport::default());
            fake.push_response(preflight_response(
                ProtocolResourcePressure::Warning,
                Some("wire-admission"),
            ));
            let facade = facade(fake.clone());
            let preflight = tauri::async_runtime::block_on(
                facade.preflight_render(request.clone(), RenderEnvironmentInputs::default()),
            )
            .unwrap();
            let error = tauri::async_runtime::block_on(
                facade.use_basic_vulkan(&preflight.preflight_id, &request.source),
            )
            .unwrap_err();
            assert!(error.contains("has no shader pack"));
            assert!(
                facade
                    .inner
                    .state
                    .lock()
                    .unwrap()
                    .preflights
                    .get(&preflight.preflight_id)
                    .is_none()
            );
        }
    }

    #[test]
    fn warning_preflight_confirmation_consumes_its_source_bound_grant() {
        assert_memory_preflight_starts_without_capacity_gate(ProtocolResourcePressure::Warning, false);
    }

    #[test]
    fn hard_preflight_blocks_95_percent_capacity_pressure() {
        assert_memory_preflight_starts_without_capacity_gate(ProtocolResourcePressure::HardLimit, false);
    }

    #[test]
    fn unmeasured_preflight_preserves_null_values_and_blocks_preloaded_build() {
        let ResponseBody::BuildPreflight(response) = preflight_response(ProtocolResourcePressure::Unmeasured, None) else { unreachable!() };
        validate_preflight_response(&response).unwrap();
        let view = resource_preflight_view(ResourcePreflightId("cold".into()), &response, false);
        let json = serde_json::to_value(view).unwrap();
        assert_eq!(json["severity"], "unmeasured");
        for field in ["conservativeVramBytes", "components", "pressurePercent"] { assert!(json[field].is_null()); }
        assert_memory_preflight_starts_without_capacity_gate(ProtocolResourcePressure::Unmeasured, false);
    }

    #[test]
    fn unmeasured_preflight_rejects_fabricated_zero_and_admission_token() {
        let ResponseBody::BuildPreflight(mut response) = preflight_response(ProtocolResourcePressure::Unmeasured, None) else { unreachable!() };
        response.conservative_vram_bytes = Some(0);
        assert!(validate_preflight_response(&response).is_err());
        response.conservative_vram_bytes = None; response.admission_token = Some("unexpected-token".into());
        assert!(validate_preflight_response(&response).is_err());
    }

    #[test]
    fn unknown_live_budget_preserves_valid_demand_but_grants_no_preloaded_admission() {
        let ResponseBody::BuildPreflight(mut response) = preflight_response(ProtocolResourcePressure::Normal, Some("normal-token")) else { unreachable!() };
        response.pressure = ProtocolResourcePressure::Unmeasured;
        response.admission_token = None;
        response.safe_available_vram_bytes = None;
        response.pressure_basis_points = None;
        validate_preflight_response(&response).unwrap();
        let view = resource_preflight_view(ResourcePreflightId("budget-unknown".into()), &response, false);
        assert!(view.conservative_vram_bytes.is_some());
        assert!(!view.allowed_actions.contains(&ResourcePreflightAction::ContinuePrebuilt));
        assert!(view.allowed_actions.contains(&ResourcePreflightAction::SwitchRealtime));
    }

    #[test]
    fn warning_preloaded_requires_explicit_confirmation_at_host_boundary() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(preflight_response(ProtocolResourcePressure::Warning, Some("warning-token")));
        let facade = facade(fake.clone());
        let request = request();
        let preflight = tauri::async_runtime::block_on(facade.preflight_render(request.clone(), RenderEnvironmentInputs::default())).unwrap();
        let error = tauri::async_runtime::block_on(facade.start_prebuilt_render(request.source,
            RenderStartAuthorization { preflight_id: Some(preflight.preflight_id), resource_warning_confirmed: false,
                click_started_at_epoch_ms: None })).unwrap_err();
        assert!(error.contains("80% requires explicit confirmation"));
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
    }

    #[test]
    fn invalid_preflight_exposes_no_fake_numbers_or_prebuilt_admission() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(preflight_response(ProtocolResourcePressure::Invalid, None));
        let facade = facade(fake.clone());
        let request = request();
        let preflight = tauri::async_runtime::block_on(
            facade.preflight_render(request.clone(), RenderEnvironmentInputs::default()),
        )
        .unwrap();
        assert_eq!(preflight.severity, ResourcePreflightSeverity::Invalid);
        assert_eq!(preflight.conservative_vram_bytes, None);
        assert_eq!(preflight.safe_available_vram_bytes, None);
        assert_eq!(preflight.pressure_percent, None);
        assert_eq!(preflight.components, None);
        assert_eq!(
            preflight.invalid_reason.as_deref(),
            Some("metadata estimate unavailable")
        );
        assert_eq!(
            preflight.allowed_actions,
            vec![
                ResourcePreflightAction::SwitchRealtime,
                ResourcePreflightAction::Cancel
            ]
        );
        let error = tauri::async_runtime::block_on(facade.start_prebuilt_render(
            request.source,
            RenderStartAuthorization {
                click_started_at_epoch_ms: None,
                preflight_id: Some(preflight.preflight_id),
                resource_warning_confirmed: false,
            },
        ))
        .unwrap_err();
        assert!(error.contains("invalid"));
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
    }

    #[test]
    fn invalid_preflight_rejects_a_wire_admission_token() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(preflight_response(
            ProtocolResourcePressure::Invalid,
            Some("unexpected-token"),
        ));
        let facade = facade(fake);
        let error = tauri::async_runtime::block_on(
            facade.preflight_render(request(), RenderEnvironmentInputs::default()),
        )
        .unwrap_err();
        assert!(error.contains("must not grant an admission token"));
    }

    #[test]
    fn preflight_id_is_bound_to_the_expected_source() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(preflight_response(
            ProtocolResourcePressure::Normal,
            Some("wire-admission-bound"),
        ));
        let facade = facade(fake.clone());
        let request = request();
        let environment = RenderEnvironmentInputs::default();
        let preflight = tauri::async_runtime::block_on(
            facade.preflight_render(request.clone(), environment.clone()),
        )
        .unwrap();
        let changed_source = ProjectSource {
            path: PathBuf::from(r"C:\projections\other.litematic"),
        };
        let preflight_id = preflight.preflight_id;

        let error = tauri::async_runtime::block_on(facade.start_prebuilt_render(
            changed_source,
            RenderStartAuthorization {
                click_started_at_epoch_ms: None,
                preflight_id: Some(preflight_id.clone()),
                resource_warning_confirmed: false,
            },
        ))
        .unwrap_err();
        assert!(error.contains("requested source"));
        let replay = tauri::async_runtime::block_on(facade.start_prebuilt_render(
            request.source,
            RenderStartAuthorization {
                click_started_at_epoch_ms: None,
                preflight_id: Some(preflight_id),
                resource_warning_confirmed: false,
            },
        ))
        .unwrap_err();
        assert!(replay.contains("already consumed"));
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
    }

    #[test]
    fn realtime_start_never_preflights_or_carries_admission() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(None));
        let facade = facade(fake.clone());
        let mut request = request();
        request.settings.build_mode = RenderBuildMode::Realtime;

        tauri::async_runtime::block_on(
            facade.start_realtime_render(request, RenderEnvironmentInputs::default()),
        )
        .unwrap();
        let commands = fake.commands.lock().unwrap();
        assert_eq!(commands.len(), 1);
        let UiCommand::StartBuild(start) = &commands[0] else {
            panic!("realtime must issue StartBuild directly")
        };
        assert_eq!(start.build.build_mode, ProtocolRenderBuildMode::Realtime);
        assert!(start.admission.is_none());
    }

    #[test]
    fn frozen_realtime_decision_preserves_vulkan_and_never_carries_prebuilt_admission() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(None));
        let facade = facade(fake.clone());
        let mut request = request();
        request.settings.backend = GraphicsBackendPreference::Vulkan;
        request.settings.shader.path = None;
        let id = facade.next_preflight_id();
        facade.inner.state.lock().unwrap().preflights.insert(id.clone(), PendingResourcePreflight {
            request: request.clone(), environment: RenderEnvironmentInputs::default(),
            prospective_build: derive_build_start(&request, RenderEnvironmentInputs::default()),
            pressure: ProtocolResourcePressure::Warning, admission_token: Some("prebuilt-only".to_owned()),
        });
        tauri::async_runtime::block_on(facade.start_realtime_from_preflight(&id, request.source.clone())).unwrap();
        let commands = fake.commands.lock().unwrap();
        let UiCommand::StartBuild(start) = &commands[0] else { panic!("expected the real StartBuild path"); };
        assert_eq!(start.build.build_mode, ProtocolRenderBuildMode::Realtime);
        assert_eq!(start.build.presentation.backend, RenderBackend::Vulkan);
        assert!(start.build.presentation.shaderpack.is_none());
        assert!(start.admission.is_none());
        drop(commands);
        assert!(tauri::async_runtime::block_on(facade.start_realtime_from_preflight(&id, request.source)).is_err());
    }

    #[test]
    fn frozen_realtime_decision_rejects_changed_source_before_transport() {
        let fake = Arc::new(FakeTransport::default());
        let facade = facade(fake.clone());
        let request = request();
        let id = facade.next_preflight_id();
        facade.inner.state.lock().unwrap().preflights.insert(id.clone(), PendingResourcePreflight {
            request: request.clone(), environment: RenderEnvironmentInputs::default(),
            prospective_build: derive_build_start(&request, RenderEnvironmentInputs::default()),
            pressure: ProtocolResourcePressure::HardLimit, admission_token: None,
        });
        let other = ProjectSource { path: PathBuf::from("C:/different.litematic") };
        assert!(tauri::async_runtime::block_on(facade.start_realtime_from_preflight(&id, other)).is_err());
        assert!(fake.commands.lock().unwrap().is_empty());
    }


    #[test]
    fn hidden_same_scene_attaches_but_source_a_b_a_rebinds_only_the_retired_scene() {
        let fake = Arc::new(FakeTransport::default());
        let facade = facade(fake.clone());
        let mut a = request(); a.settings.build_mode = RenderBuildMode::Realtime;
        a.presentation.visible = false; a.presentation.mode = PresentationMode::Hidden;
        let mut b = a.clone(); b.source.path = PathBuf::from(r"C:\projections\other.litematic");
        let push_accepted = |id: &str, generation, attached| fake.push_response(ResponseBody::BuildAccepted(BuildAccepted {
            build_id: id.to_owned(), generation, provisional_scene_key: id.to_owned(),
            viewer_session_id: Some("same-native-session".to_owned()), attached,
        }));
        push_accepted("build-a", 7, false);
        let first = tauri::async_runtime::block_on(facade.start_realtime_render(a.clone(), RenderEnvironmentInputs::default())).unwrap();
        let mut a_build = build_snapshot(7, BuildState::BuildCompleted); a_build.build_id = "build-a".to_owned();
        let mut a_viewer = viewer_snapshot(7, ViewerRuntimeState::ViewerPrepared, ViewerPresentationState::Hidden);
        a_viewer.session_id = "same-native-session".to_owned(); a_viewer.build_id = "build-a".to_owned();
        fake.set_snapshot(ServiceSnapshot { builds: vec![a_build.clone()], artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)], viewers: vec![a_viewer] });
        facade.refresh_from_service();
        // An ordinary hidden page/return keeps the still-valid runtime, no StartBuild.
        let same = tauri::async_runtime::block_on(facade.start_realtime_render(a.clone(), RenderEnvironmentInputs::default())).unwrap();
        assert!(same.attached); assert_eq!(first.operation.operation_id, same.operation.operation_id);
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
        push_accepted("build-b", 12, false);
        let second = tauri::async_runtime::block_on(facade.start_realtime_render(b, RenderEnvironmentInputs::default())).unwrap();
        assert_ne!(first.operation.operation_id, second.operation.operation_id);
        let mut b_build = build_snapshot(12, BuildState::BuildCompleted); b_build.build_id = "build-b".to_owned();
        let mut b_viewer = viewer_snapshot(12, ViewerRuntimeState::ViewerPrepared, ViewerPresentationState::Hidden);
        b_viewer.session_id = "same-native-session".to_owned(); b_viewer.build_id = "build-b".to_owned();
        fake.set_snapshot(ServiceSnapshot { builds: vec![a_build, b_build], artifacts: vec![artifact_snapshot(12, ArtifactState::Ready)], viewers: vec![b_viewer] });
        push_accepted("restore-a", 13, true);
        let restored = tauri::async_runtime::block_on(facade.start_realtime_render(a, RenderEnvironmentInputs::default())).unwrap();
        assert!(restored.attached); assert_ne!(first.operation.operation_id, restored.operation.operation_id);
        let state = facade.inner.state.lock().unwrap();
        let binding = state.operations[&restored.operation.operation_id].binding.as_ref().unwrap();
        assert_eq!(binding.build_id, "restore-a"); assert_eq!(binding.generation, 13);
        let commands = fake.commands.lock().unwrap();
        assert_eq!(commands.iter().filter(|command| matches!(command, UiCommand::StartBuild(_))).count(), 3);
    }

    #[test]
    fn duplicate_start_attaches_without_a_second_build() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(None));
        let facade = facade(fake.clone());

        let first = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();
        let second = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();

        assert!(!first.attached);
        assert!(second.attached);
        assert_eq!(first.operation.operation_id, second.operation.operation_id);
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
    }

    #[test]
    fn different_build_modes_never_attach_to_the_same_operation() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(None));
        fake.push_response(accepted(None));
        let facade = facade(fake.clone());

        let prebuilt = request();
        let mut realtime = request();
        realtime.settings.build_mode = RenderBuildMode::Realtime;
        let first = tauri::async_runtime::block_on(start_for_test(
            &facade,
            prebuilt,
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();
        let second = tauri::async_runtime::block_on(start_for_test(
            &facade,
            realtime,
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();

        assert!(!first.attached);
        assert!(!second.attached);
        assert_ne!(first.operation.operation_id, second.operation.operation_id);
        assert_eq!(fake.commands.lock().unwrap().len(), 2);
    }

    #[test]
    fn cancellation_pending_ack_blocks_late_ready_show_and_reuse_without_locking() {
        tauri::async_runtime::block_on(async {
            let fake = Arc::new(FakeTransport::default());
            fake.push_response(accepted(Some("viewer-1")));
            let facade = facade(fake.clone());
            let started = start_for_test(&facade, request(), RenderEnvironmentInputs::default()).await.unwrap();
            let ack = fake.defer_response();
            let mut cancelling = Box::pin(facade.cancel_render(&started.operation.operation_id));
            std::future::poll_fn(|context| {
                assert!(cancelling.as_mut().poll(context).is_pending());
                std::task::Poll::Ready(())
            }).await;
            fake.set_snapshot(ServiceSnapshot {
                builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
                artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
                viewers: vec![viewer_snapshot(7, ViewerRuntimeState::ViewerPrepared, ViewerPresentationState::PopupVisible)],
            });
            facade.refresh_from_service();
            let view = facade.render_state(&started.operation.operation_id).unwrap();
            assert_eq!(view.state, OperationState::Running);
            assert_eq!(view.progress.phase, "cancelling");
            assert!(!view.presentation.visible);
            assert_eq!(view.presentation_state, PresentationState::Hidden);
            assert!(facade.show_viewer(&view.operation_id, PresentationMode::Popup, None).await.is_err());
            assert_eq!(fake.commands.lock().unwrap().len(), 2);
            fake.push_response(accepted(None));
            let next = start_for_test(&facade, request(), RenderEnvironmentInputs::default()).await.unwrap();
            assert_ne!(next.operation.operation_id, started.operation.operation_id);
            assert!(!next.attached);
            ack.send(Ok(ResponseBody::Ack)).unwrap();
            cancelling.await.unwrap();
            assert_eq!(facade.render_state(&started.operation.operation_id).unwrap().state, OperationState::Cancelled);
            // A stale matching generation cannot revive acknowledged cancellation.
            facade.refresh_from_service();
            assert_eq!(facade.render_state(&started.operation.operation_id).unwrap().state, OperationState::Cancelled);
        });
    }

    #[test]
    fn cancellation_before_acceptance_sends_the_actual_build_cancel_after_binding() {
        tauri::async_runtime::block_on(async {
            let fake = Arc::new(FakeTransport::default());
            let acceptance = fake.defer_response();
            let facade = facade(fake.clone());
            let mut starting = Box::pin(start_for_test(&facade, request(), RenderEnvironmentInputs::default()));
            std::future::poll_fn(|context| {
                assert!(starting.as_mut().poll(context).is_pending());
                std::task::Poll::Ready(())
            }).await;
            let operation = facade.current_render_state().unwrap();
            facade.cancel_render(&operation.operation_id).await.unwrap();
            assert_eq!(fake.commands.lock().unwrap().len(), 1);
            fake.push_response(ResponseBody::Ack);
            acceptance.send(Ok(accepted(Some("viewer-1")))).unwrap();
            let result = starting.await.unwrap();
            assert_eq!(result.operation.state, OperationState::Cancelled);
            assert!(!result.operation.presentation.visible);
            let commands = fake.commands.lock().unwrap();
            assert_eq!(commands.len(), 2);
            assert!(matches!(&commands[1], UiCommand::CancelBuild { build_id } if build_id == "build-1"));
        });
    }

    #[test]
    fn cancellation_failed_ack_does_not_claim_a_terminal_cancel_or_process_exit() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(None));
        let facade = facade(fake.clone());
        let started = tauri::async_runtime::block_on(start_for_test(&facade, request(), RenderEnvironmentInputs::default())).unwrap();
        fake.responses.lock().unwrap().push_back(Err("service connection failed".to_owned()));
        let error = tauri::async_runtime::block_on(facade.cancel_render(&started.operation.operation_id)).unwrap_err();
        assert_eq!(error, "service connection failed");
        let view = facade.render_state(&started.operation.operation_id).unwrap();
        assert_eq!(view.state, OperationState::Running);
        assert_eq!(view.progress.phase, "cancelling");
        assert!(!view.presentation.visible);
    }

    #[test]
    fn stale_generation_is_ignored_and_runtime_state_is_authoritative() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        let facade = facade(fake.clone());
        let started = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(8, BuildState::ArtifactReady)],
            artifacts: vec![],
            viewers: vec![viewer_snapshot(
                8,
                ViewerRuntimeState::ViewerPrepared,
                ViewerPresentationState::Hidden,
            )],
        });
        facade.refresh_from_service();
        assert_eq!(
            facade
                .render_state(&started.operation.operation_id)
                .unwrap()
                .state,
            OperationState::Running
        );

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::Finalizing)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::Preparing,
                ViewerPresentationState::Hidden,
            )],
        });
        facade.refresh_from_service();
        let view = facade
            .render_state(&started.operation.operation_id)
            .unwrap();
        assert_eq!(view.state, OperationState::ViewerPreparing);
        assert_eq!(view.progress.completed, None);
        assert_eq!(view.progress.total, None);

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::ViewerPrepared,
                ViewerPresentationState::Hidden,
            )],
        });
        facade.refresh_from_service();
        assert_eq!(
            facade
                .render_state(&started.operation.operation_id)
                .unwrap()
                .state,
            OperationState::ViewerPrepared
        );
    }

    #[test]
    fn accepted_count_equality_does_not_imply_resident_or_prepared() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        let facade = facade(fake.clone());
        let started = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();

        let mut viewer = viewer_snapshot(
            7,
            ViewerRuntimeState::GpuAccepted,
            ViewerPresentationState::Hidden,
        );
        viewer.sections_gpu_accepted = viewer.sections_available;
        viewer.sections_gpu_completed = viewer.sections_available;
        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::Finalizing)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer],
        });
        facade.refresh_from_service();

        let view = facade
            .render_state(&started.operation.operation_id)
            .unwrap();
        assert_eq!(view.state, OperationState::GpuAccepted);
        assert_eq!(view.progress.completed, None);
        assert_eq!(view.progress.total, None);
    }

    #[test]
    fn product_completion_requires_the_matching_ready_artifact_and_viewer() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        let facade = facade(fake.clone());
        let started = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
            artifacts: vec![],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::Preparing,
                ViewerPresentationState::Hidden,
            )],
        });
        facade.refresh_from_service();

        assert_eq!(
            facade
                .render_state(&started.operation.operation_id)
                .unwrap()
                .state,
            OperationState::Running
        );
    }

    #[test]
    fn desired_visibility_does_not_imply_observed_visibility() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        fake.push_response(ResponseBody::Ack);
        let facade = facade(fake.clone());
        let mut visible_request = request();
        visible_request.presentation.visible = true;
        let started = tauri::async_runtime::block_on(start_for_test(
            &facade,
            visible_request,
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::ViewerPrepared,
                ViewerPresentationState::Hidden,
            )],
        });
        facade.refresh_from_service();

        let view = facade
            .render_state(&started.operation.operation_id)
            .unwrap();
        assert!(view.presentation.visible);
        assert_eq!(view.presentation_state, PresentationState::Hidden);
    }

    #[test]
    fn hide_and_show_never_release_the_viewer_runtime() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        fake.push_response(ResponseBody::Ack);
        fake.push_response(ResponseBody::Ack);
        let facade = facade(fake.clone());
        let started = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();
        let operation_id = &started.operation.operation_id;

        tauri::async_runtime::block_on(facade.show_viewer(
            operation_id,
            PresentationMode::Overlay,
            Some(PresentationPlacement {
                x: 10,
                y: 20,
                width: 640,
                height: 360,
                dpi_scale: 1.5,
                focused: true,
            }),
        ))
        .unwrap();

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::ViewerPrepared,
                ViewerPresentationState::EmbeddedOverlayVisible {
                    rect: ScreenRect {
                        x: 10,
                        y: 20,
                        width: 640,
                        height: 360,
                        focused: true,
                    },
                },
            )],
        });
        facade.refresh_from_service();
        tauri::async_runtime::block_on(facade.hide_viewer(operation_id)).unwrap();

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::ViewerPrepared,
                ViewerPresentationState::Hidden,
            )],
        });
        facade.refresh_from_service();

        let view = facade.render_state(operation_id).unwrap();
        assert_eq!(view.state, OperationState::ViewerPrepared);
        assert_eq!(view.presentation_state, PresentationState::Hidden);

        let commands = fake.commands.lock().unwrap();
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, UiCommand::SetBoundPresentation { state: ViewerPresentationState::EmbeddedOverlayVisible { .. }, .. }))
        );
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, UiCommand::SetBoundPresentation { state: ViewerPresentationState::Hidden, .. }))
        );
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, UiCommand::ReleaseViewer { .. }))
        );
    }

    #[test]
    fn operation_view_keeps_actual_display_and_loading_modes_isolated() {
        use super::super::model::RenderDisplayMode;
        let fake = Arc::new(FakeTransport::default());
        for _ in 0..3 { fake.push_response(accepted(None)); }
        let facade = facade(fake.clone());
        let mut request = request();
        request.settings.display_mode = RenderDisplayMode::Standard;
        request.settings.build_mode = RenderBuildMode::Realtime;
        let standard = tauri::async_runtime::block_on(start_for_test(&facade, request.clone(), RenderEnvironmentInputs::default())).unwrap();
        assert_eq!(standard.operation.display_mode, RenderDisplayMode::Standard);
        assert_eq!(standard.operation.build_mode, RenderBuildMode::Realtime);
        request.settings.display_mode = RenderDisplayMode::Full;
        let full = tauri::async_runtime::block_on(start_for_test(&facade, request.clone(), RenderEnvironmentInputs::default())).unwrap();
        request.settings.build_mode = RenderBuildMode::Prebuilt;
        let preloaded = tauri::async_runtime::block_on(start_for_test(&facade, request, RenderEnvironmentInputs::default())).unwrap();
        assert_eq!(full.operation.display_mode, RenderDisplayMode::Full);
        assert_eq!(full.operation.build_mode, RenderBuildMode::Realtime);
        assert_eq!(preloaded.operation.build_mode, RenderBuildMode::Prebuilt);
        assert_ne!(standard.operation.operation_id, full.operation.operation_id);
        assert_ne!(full.operation.operation_id, preloaded.operation.operation_id);
        assert!(!standard.attached && !full.attached && !preloaded.attached);
        assert_eq!(fake.commands.lock().unwrap().iter().filter(|command| matches!(command, UiCommand::StartBuild(_))).count(), 3);
    }

    #[test]
    fn released_viewer_does_not_dedupe_a_later_render_request() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        fake.push_response(accepted(Some("viewer-2")));
        let facade = facade(fake.clone());
        let first = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::Released,
                ViewerPresentationState::Hidden,
            )],
        });
        facade.refresh_from_service();
        assert_eq!(
            facade
                .render_state(&first.operation.operation_id)
                .unwrap()
                .state,
            OperationState::RuntimeReleased
        );

        let second = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();
        assert!(!second.attached);
        assert_ne!(first.operation.operation_id, second.operation.operation_id);
        assert_eq!(fake.commands.lock().unwrap().len(), 2);
    }

    #[test]
    fn capture_source_identity_requires_real_canonical_match_for_different_paths() {
        let root = std::env::temp_dir().join(format!("nova-capture-source-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(root.join("nested")).unwrap();
        let plain = root.join("投影 空格.litematic");
        let different = root.join("另一投影.litematic");
        std::fs::write(&plain, b"source fixture").unwrap();
        std::fs::write(&different, b"different source fixture").unwrap();
        let canonical = std::fs::canonicalize(&plain).unwrap();
        assert!(capture_source_matches(&plain, &canonical));
        assert!(capture_source_matches(&root.join("nested/../投影 空格.litematic"), &canonical));
        assert!(!capture_source_matches(&different, &canonical));
        assert!(!capture_source_matches(&root.join("missing-one"), &root.join("missing-two")));
        assert!(!capture_source_matches(&root.join("missing-one"), &canonical));
        // Preserve the original zero-I/O equal-representation fast path.
        assert!(capture_source_matches(&root.join("missing-one"), &root.join("missing-one")));
        #[cfg(windows)]
        assert_ne!(plain, canonical, "exercise plain versus verbatim Windows representation");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capture_realtime_and_completed_viewers_use_the_exact_bound_session() {
        let root = std::env::temp_dir().join(format!("nova-capture-ready-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let plain = root.join("投影 空格.litematic");
        std::fs::write(&plain, b"source fixture").unwrap();
        let canonical = std::fs::canonicalize(&plain).unwrap();
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        let facade = facade(fake.clone());
        let mut input = request();
        input.source.path = plain;
        input.settings.display_mode = RenderDisplayMode::Standard;
        input.settings.build_mode = RenderBuildMode::Realtime;
        let started = tauri::async_runtime::block_on(facade.start_realtime_render(input, RenderEnvironmentInputs::default())).unwrap();
        for (build_state, viewer_state, artifacts) in [
            (BuildState::Building, ViewerRuntimeState::RealtimeReady, vec![]),
            (BuildState::BuildCompleted, ViewerRuntimeState::ViewerPrepared, vec![artifact_snapshot(7, ArtifactState::Ready)]),
        ] {
            fake.set_snapshot(ServiceSnapshot { builds: vec![build_snapshot(7, build_state)], artifacts,
                viewers: vec![viewer_snapshot(7, viewer_state, ViewerPresentationState::PopupVisible)] });
            // Stop after dispatch: this unit test asserts admission/identity, not a GPU capture.
            fake.responses.lock().unwrap().push_back(Err("capture_fixture_dispatched".into()));
            let error = tauri::async_runtime::block_on(facade.capture_frame(&started.operation.operation_id, &canonical, RenderDisplayMode::Standard)).unwrap_err();
            assert_eq!(error, "capture_fixture_dispatched");
            assert!(matches!(fake.commands.lock().unwrap().last(), Some(UiCommand::CaptureViewer { session_id, .. }) if session_id == "viewer-1"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capture_rejects_wrong_source_mode_cancel_and_stale_viewer_generation() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        let facade = facade(fake.clone());
        let mut input = request();
        input.settings.display_mode = RenderDisplayMode::Standard;
        input.settings.build_mode = RenderBuildMode::Realtime;
        let source = input.source.path.clone();
        let started = tauri::async_runtime::block_on(facade.start_realtime_render(input, RenderEnvironmentInputs::default())).unwrap();
        let ready = ServiceSnapshot { builds: vec![build_snapshot(7, BuildState::Building)], artifacts: vec![],
            viewers: vec![viewer_snapshot(7, ViewerRuntimeState::RealtimeReady, ViewerPresentationState::PopupVisible)] };
        fake.set_snapshot(ready.clone());
        let command_count = fake.commands.lock().unwrap().len();
        assert!(tauri::async_runtime::block_on(facade.capture_frame(&started.operation.operation_id, Path::new("different-missing-source"), RenderDisplayMode::Standard)).is_err());
        assert!(tauri::async_runtime::block_on(facade.capture_frame(&started.operation.operation_id, &source, RenderDisplayMode::Full)).is_err());
        facade.inner.state.lock().unwrap().operations.get_mut(&started.operation.operation_id).unwrap().cancel_requested = true;
        assert!(tauri::async_runtime::block_on(facade.capture_frame(&started.operation.operation_id, &source, RenderDisplayMode::Standard)).is_err());
        facade.inner.state.lock().unwrap().operations.get_mut(&started.operation.operation_id).unwrap().cancel_requested = false;
        let mut wrong_build = viewer_snapshot(7, ViewerRuntimeState::RealtimeReady, ViewerPresentationState::PopupVisible);
        wrong_build.build_id = "different-build".into();
        let mut wrong_session = wrong_build.clone();
        wrong_session.session_id = "different-viewer-session".into();
        for snapshot in [
            ServiceSnapshot { viewers: vec![wrong_build], ..ready.clone() },
            ServiceSnapshot { viewers: vec![wrong_session], ..ready.clone() },
            ServiceSnapshot { viewers: vec![viewer_snapshot(8, ViewerRuntimeState::RealtimeReady, ViewerPresentationState::PopupVisible)], ..ready.clone() },
            ServiceSnapshot { viewers: vec![viewer_snapshot(7, ViewerRuntimeState::Released, ViewerPresentationState::Hidden)], ..ready.clone() },
            ServiceSnapshot { viewers: vec![viewer_snapshot(7, ViewerRuntimeState::Preparing, ViewerPresentationState::Hidden)], ..ready.clone() },
            ServiceSnapshot { builds: vec![build_snapshot(8, BuildState::Building)], ..ready.clone() },
            ServiceSnapshot { builds: vec![build_snapshot(7, BuildState::Failed)], ..ready.clone() },
        ] {
            fake.set_snapshot(ready.clone());
            facade.refresh_from_service();
            fake.set_snapshot(snapshot);
            assert!(tauri::async_runtime::block_on(facade.capture_frame(&started.operation.operation_id, &source, RenderDisplayMode::Standard)).is_err());
        }
        assert_eq!(fake.commands.lock().unwrap().len(), command_count, "rejected capture must never reach the Viewer");
    }

    #[test]
    fn realtime_ready_exposes_viewer_while_build_remains_running() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        let facade = facade(fake.clone());
        let mut request = request();
        request.settings.build_mode = RenderBuildMode::Realtime;
        let started = tauri::async_runtime::block_on(
            facade.start_realtime_render(request, RenderEnvironmentInputs::default()),
        )
        .unwrap();
        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::Building)],
            artifacts: vec![],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::RealtimeReady,
                ViewerPresentationState::PopupVisible,
            )],
        });
        facade.refresh_from_service();
        let view = facade
            .render_state(&started.operation.operation_id)
            .unwrap();
        assert_eq!(view.state, OperationState::RealtimeReady);
        assert!(view.analysis.is_none());
        assert_eq!(fake.snapshot().builds[0].state, BuildState::Building);
        fake.push_response(ResponseBody::HeadlessCameraStress { frames: 28 });
        assert_eq!(
            tauri::async_runtime::block_on(
                facade.headless_camera_stress(&started.operation.operation_id)
            )
            .unwrap(),
            28
        );
    }

    #[test]
    fn headless_camera_stress_requires_visible_prepared_viewer_and_uses_bound_session() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        let facade = facade(fake.clone());
        let started = tauri::async_runtime::block_on(start_for_test(
            &facade,
            request(),
            RenderEnvironmentInputs::default(),
        ))
        .unwrap();
        let operation_id = &started.operation.operation_id;
        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::ViewerPrepared,
                ViewerPresentationState::Hidden,
            )],
        });
        let error = tauri::async_runtime::block_on(facade.headless_camera_stress(operation_id))
            .unwrap_err();
        assert!(error.contains("prepared, visible Viewer"));
        assert_eq!(fake.commands.lock().unwrap().len(), 1);

        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build_snapshot(7, BuildState::BuildCompleted)],
            artifacts: vec![artifact_snapshot(7, ArtifactState::Ready)],
            viewers: vec![viewer_snapshot(
                7,
                ViewerRuntimeState::ViewerPrepared,
                ViewerPresentationState::PopupVisible,
            )],
        });
        fake.push_response(ResponseBody::HeadlessCameraStress { frames: 28 });
        let frames =
            tauri::async_runtime::block_on(facade.headless_camera_stress(operation_id)).unwrap();
        assert_eq!(frames, 28);
        assert!(matches!(
            fake.commands.lock().unwrap().last(),
            Some(UiCommand::HeadlessCameraStress { session_id }) if session_id == "viewer-1"
        ));
    }

    fn build_snapshot(generation: u64, state: BuildState) -> BuildSnapshot {
        BuildSnapshot {
            build_id: "build-1".to_owned(),
            generation,
            state,
            source_path: PathBuf::from(r"C:\projections\castle.litematic"),
            artifact_path: PathBuf::from(r"C:\cache\scene.gpu-scene"),
            scene_identity: Some("scene-1".to_owned()),
            sections_committed: 45_276,
            sections_compiled: Some(45_276),
            sections_existing: Some(45_276),
            bytes_committed: 1024,
            phase: "artifact_ready".to_owned(),
            activity: None,
            processed_units: Some(45_276),
            total_units: Some(45_276),
            elapsed_ms: 100,
            error: None,
            worker_pid: Some(99),
        }
    }

    #[test]
    fn completed_cpu_counts_keep_hidden_until_present_and_build_failure_is_terminal() {
        let fake = Arc::new(FakeTransport::default());
        fake.push_response(accepted(Some("viewer-1")));
        let facade = facade(fake.clone());
        let mut input = request();
        input.settings.build_mode = RenderBuildMode::Realtime;
        input.settings.display_mode = RenderDisplayMode::Standard;
        let started = tauri::async_runtime::block_on(start_for_test(
            &facade, input, RenderEnvironmentInputs::default(),
        )).unwrap();
        let id = &started.operation.operation_id;
        let mut build = build_snapshot(7, BuildState::Building);
        build.processed_units = Some(47);
        build.total_units = Some(47);
        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build.clone()], artifacts: vec![],
            viewers: vec![viewer_snapshot(7, ViewerRuntimeState::Preparing, ViewerPresentationState::Hidden)],
        });
        facade.refresh_from_service();
        let view = facade.render_state(id).unwrap();
        assert_eq!(view.state, OperationState::Running);
        assert_eq!(view.presentation_state, PresentationState::Hidden);
        assert_eq!(view.progress.completed, Some(47));
        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build.clone()], artifacts: vec![],
            viewers: vec![viewer_snapshot(7, ViewerRuntimeState::RealtimeReady, ViewerPresentationState::Hidden)],
        });
        facade.refresh_from_service();
        assert_eq!(facade.render_state(id).unwrap().state, OperationState::RealtimeReady);
        assert_eq!(facade.render_state(id).unwrap().presentation_state, PresentationState::Hidden);
        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build.clone()], artifacts: vec![],
            viewers: vec![viewer_snapshot(7, ViewerRuntimeState::RealtimeReady, ViewerPresentationState::EmbeddedOverlayVisible {
                rect: ScreenRect { x: 10, y: 20, width: 640, height: 360, focused: true },
            })],
        });
        facade.refresh_from_service();
        assert_eq!(facade.render_state(id).unwrap().presentation_state, PresentationState::Visible);
        build.state = BuildState::Failed;
        build.phase = "failed".to_owned();
        build.error = Some(ServiceError::new("build_failed", "invalid entity geometry", true));
        // A late Viewer receipt cannot revive the failed generation or erase its error.
        fake.set_snapshot(ServiceSnapshot {
            builds: vec![build], artifacts: vec![],
            viewers: vec![viewer_snapshot(7, ViewerRuntimeState::RealtimeReady, ViewerPresentationState::Hidden)],
        });
        facade.refresh_from_service();
        let view = facade.render_state(id).unwrap();
        assert_eq!(view.state, OperationState::Failed);
        assert_eq!(view.error.as_deref(), Some("invalid entity geometry"));
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
    }

    fn artifact_snapshot(generation: u64, state: ArtifactState) -> ArtifactSnapshot {
        ArtifactSnapshot {
            scene_identity: "scene-1".to_owned(),
            generation,
            path: PathBuf::from(r"C:\cache\scene.gpu-scene"),
            state,
            sections: 45_276,
            bytes: 1024,
            payload_digest: Some("digest".to_owned()),
            analysis_summary: None,
        }
    }

    fn viewer_snapshot(
        generation: u64,
        runtime_state: ViewerRuntimeState,
        presentation_state: ViewerPresentationState,
    ) -> ViewerSnapshot {
        ViewerSnapshot {
            session_id: "viewer-1".to_owned(),
            build_id: "build-1".to_owned(),
            generation,
            scene_identity: Some("scene-1".to_owned()),
            presentation_identity: Some("presentation-1".to_owned()),
            session_identity: Some("session-1".to_owned()),
            runtime_state,
            presentation_state,
            pid: Some(100),
            sections_available: 45_276,
            sections_gpu_accepted: 45_276,
            sections_gpu_completed: 45_276,
            elapsed_ms: 200,
            error: None,
        }
    }

    #[allow(dead_code)]
    fn service_error(message: &str) -> ServiceError {
        ServiceError::new("test", message, false)
    }
}
