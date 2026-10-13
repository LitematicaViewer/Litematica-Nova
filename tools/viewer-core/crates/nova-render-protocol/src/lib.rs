//! Small, renderer-independent control protocol shared by Nova, the render
//! service, build workers and Viewer processes.
//!
//! The transport is a duplex Windows named pipe. Every packet is a four-byte
//! little-endian length followed by one UTF-8 JSON [`Envelope`]. The first
//! packet on every connection must be [`Payload::Hello`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;

pub mod host_supervision;
pub mod identity;

pub const PROTOCOL_SCHEMA: &str = "litematica-nova.render-service.v1";
pub const PROTOCOL_VERSION: u32 = 17;
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Two concrete upload/decode payload slots plus one metadata-only queued
/// commit. The worker and Viewer use the same credits so cache publication can
/// run ahead without creating an unbounded cross-process backlog.
pub const STREAM_WINDOW_MAX_INFLIGHT_SECTIONS: u64 = 3;
/// Two 64 MiB renderer staging units. A single valid oversize section is still
/// admitted into an otherwise empty window, so this is a working-set budget,
/// never a correctness limit.
pub const STREAM_WINDOW_MAX_INFLIGHT_BYTES: u64 = 128 * 1024 * 1024;

pub const PIPE_ENV: &str = "NOVA_RENDER_SERVICE_PIPE";
pub const AUTH_TOKEN_ENV: &str = "NOVA_RENDER_SERVICE_AUTH_TOKEN";
pub const CLIENT_ROLE_ENV: &str = "NOVA_RENDER_SERVICE_ROLE";
pub const BUILD_ID_ENV: &str = "NOVA_BUILD_ID";
pub const BUILD_GENERATION_ENV: &str = "NOVA_BUILD_GENERATION";
pub const VIEWER_SESSION_ENV: &str = "NOVA_VIEWER_SESSION";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Envelope {
    pub schema: String,
    pub sequence: u64,
    pub payload: Payload,
}

impl Envelope {
    pub fn new(sequence: u64, payload: Payload) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            sequence,
            payload,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROTOCOL_SCHEMA {
            return Err(format!(
                "unsupported render service protocol schema {:?}",
                self.schema
            ));
        }
        self.payload.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Payload {
    Hello(ClientHello),
    HelloAccepted(ServerHello),
    Request(Request),
    Response(Response),
    Event(ServiceEvent),
    WorkerUpdate(WorkerUpdate),
    WorkerControl(WorkerControl),
    ViewerUpdate(ViewerUpdate),
    ViewerControl(ViewerControl),
    HostProcessRequest(host_supervision::HostProcessRequest),
    HostProcessResponse(host_supervision::HostProcessResponse),
    HostChildExited(host_supervision::HostChildExited),
}

impl Payload {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Hello(value) => value.validate(),
            Self::Request(value) => value.validate(),
            Self::WorkerUpdate(value) => value.validate(),
            Self::WorkerControl(value) => value.validate(),
            Self::ViewerUpdate(value) => value.validate(),
            Self::ViewerControl(value) => value.validate(),
            Self::HostProcessRequest(value) => value.validate(),
            Self::HostProcessResponse(value) => value.validate(),
            Self::HostChildExited(value) => value.validate(),
            Self::HelloAccepted(_) | Self::Response(_) | Self::Event(_) => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientRole {
    ApplicationHost,
    BuildWorker,
    Viewer,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientHello {
    pub protocol_version: u32,
    pub auth_token: String,
    pub role: ClientRole,
    pub instance_id: String,
    pub pid: u32,
    pub application_owner_epoch: Option<u64>,
}

impl ClientHello {
    fn validate(&self) -> Result<(), String> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(format!(
                "unsupported protocol version {}; expected {}",
                self.protocol_version, PROTOCOL_VERSION
            ));
        }
        require_id("instanceId", &self.instance_id)?;
        if self.auth_token.is_empty() || self.auth_token.len() > 256 {
            return Err("authToken must contain 1..=256 bytes".to_owned());
        }
        if self.pid == 0 {
            return Err("pid must be non-zero".to_owned());
        }
        match (self.role, self.application_owner_epoch) {
            (ClientRole::ApplicationHost, Some(epoch)) if epoch != 0 => {}
            (ClientRole::ApplicationHost, _) => {
                return Err("Application Host must provide a non-zero owner epoch".to_owned());
            }
            (_, None) => {}
            (_, Some(_)) => {
                return Err("only Application Host may provide an owner epoch".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServerHello {
    pub protocol_version: u32,
    pub service_instance_id: String,
    pub service_pid: u32,
    pub snapshot: Option<ServiceSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Request {
    pub request_id: String,
    pub command: UiCommand,
}

impl Request {
    fn validate(&self) -> Result<(), String> {
        require_id("requestId", &self.request_id)?;
        self.command.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Response {
    pub request_id: String,
    pub result: Result<ResponseBody, ServiceError>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", content = "args", rename_all = "snake_case")]
pub enum UiCommand {
    Subscribe,
    GetSnapshot,
    PreflightBuild(BuildStart),
    StartBuild(AdmittedBuildStart),
    CancelBuild {
        build_id: String,
    },
    ShowPopup {
        session_id: String,
    },
    ShowEmbeddedOverlay {
        session_id: String,
        rect: ScreenRect,
    },
    HideViewer {
        session_id: String,
    },
    HeadlessCameraStress {
        session_id: String,
    },
    ReleaseViewer {
        session_id: String,
    },
    RunViewerPreview(RenderPreviewRequest),
    Shutdown,
}

pub const HEADLESS_CAMERA_STRESS_FRAMES: u32 = 28;

impl UiCommand {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::PreflightBuild(request) => {
                if request.build_mode != RenderBuildMode::Prebuilt {
                    return Err("resource preflight is only valid for prebuilt mode".to_owned());
                }
            }
            Self::StartBuild(request) => match (request.build.build_mode, &request.admission) {
                (RenderBuildMode::Prebuilt, Some(admission)) => admission.validate()?,
                (RenderBuildMode::Prebuilt, None) => {
                    return Err("prebuilt StartBuild requires a resource admission".to_owned());
                }
                (RenderBuildMode::Realtime, None) => {}
                (RenderBuildMode::Realtime, Some(_)) => {
                    return Err(
                        "realtime StartBuild must not carry a resource admission".to_owned()
                    );
                }
            },
            Self::HeadlessCameraStress { session_id } => require_id("sessionId", session_id)?,
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ResponseBody {
    Ack,
    Snapshot(ServiceSnapshot),
    BuildPreflight(BuildResourcePreflight),
    BuildAccepted(BuildAccepted),
    ViewerPreview(RenderPreviewOutput),
    HeadlessCameraStress { frames: u32 },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RenderPreviewRequest {
    pub launch_contract_path: PathBuf,
    pub output_path: PathBuf,
    pub render_compatibility_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RenderPreviewOutput {
    pub preview_path: String,
    pub data_url: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub render_compatibility_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildStart {
    pub source_path: PathBuf,
    pub display_mode: String,
    pub build_mode: RenderBuildMode,
    pub content: SceneContent,
    pub build_options: BTreeMap<String, String>,
    pub presentation: PresentationSpec,
}

/// A start request paired with the Render Service's one-time resource grant.
///
/// Keeping admission outside [`BuildStart`] is deliberate: the token and the
/// user's warning acknowledgement are request-local authorization, never
/// scene/build identity material.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AdmittedBuildStart {
    pub build: BuildStart,
    pub admission: Option<BuildAdmission>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildAdmission {
    pub token: String,
    pub resource_warning_confirmed: bool,
}

impl BuildAdmission {
    fn validate(&self) -> Result<(), String> {
        require_id("admissionToken", &self.token)
    }
}

/// Renderer-owned result of the sub-second prebuilt resource estimate.
///
/// `admission_token` is absent at the hard limit. The Application Host keeps
/// any returned token private and exposes a separate product-level preflight
/// id to presentation code.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildResourcePreflight {
    pub pressure: ResourcePressure,
    /// Unknown or unrepresentable values are absent, never sentinels.
    pub conservative_vram_bytes: Option<u64>,
    pub safe_available_vram_bytes: Option<u64>,
    /// 10,000 basis points represent 100%; absent when the ratio is invalid.
    pub pressure_basis_points: Option<u64>,
    pub components: Option<ConservativeVramComponents>,
    pub invalid_reason: Option<String>,
    pub admission_token: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePressure {
    Normal,
    Warning,
    HardLimit,
    Invalid,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConservativeVramComponents {
    pub exact_geometry_bytes: u64,
    pub draw_metadata_bytes: u64,
    pub block_entity_bytes: u64,
    pub texture_and_shader_bytes: u64,
    pub render_target_bytes: u64,
    pub runtime_reserve_bytes: u64,
}

/// Product-selected geometry lifetime contract for one immutable build.
///
/// `Prebuilt` requires the complete exact scene to be resident before
/// `ViewerPrepared`; only `Realtime` may stream exact geometry afterwards.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBuildMode {
    #[default]
    Prebuilt,
    Realtime,
}

impl RenderBuildMode {
    /// Freezes the artifact payload contract selected by this product mode.
    /// The conversion is intentionally total so worker, service and Viewer do
    /// not maintain independent mode/profile lookup tables.
    pub const fn artifact_payload_profile(self) -> ArtifactPayloadProfile {
        match self {
            Self::Prebuilt => ArtifactPayloadProfile::PrebuiltExact,
            Self::Realtime => ArtifactPayloadProfile::RealtimeStreamed,
        }
    }
}

/// Geometry payload carried by one immutable build artifact.
///
/// `PrebuiltExact` contains only final exact geometry plus the lighting needed
/// to render it. `RealtimeStreamed` carries only exact cells and optional
/// shader-semantic light-block data. An unavailable exact cell is not drawn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactPayloadProfile {
    PrebuiltExact,
    RealtimeStreamed,
}

impl ArtifactPayloadProfile {
    pub const fn carries_fallback(self) -> bool {
        false
    }

    /// Iris's `voxelizeLightBlocks` data is exact shader semantics, not a LOD.
    pub const fn carries_light_block_proxies(self) -> bool {
        matches!(self, Self::RealtimeStreamed)
    }
}

/// Main-camera compile demand. Positions are stable 16-cubed section keys,
/// never transient directory or resident-slot indexes. Shadow passes do not
/// publish this state. A new revision replaces (rather than queues) the old one.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RealtimeCellDemand {
    pub revision: u64,
    pub camera_position: [f64; 3],
    pub predicted_camera_position: [f64; 3],
    pub visible_cells: Vec<[i32; 3]>,
    pub predicted_cells: Vec<[i32; 3]>,
}

impl RealtimeCellDemand {
    pub const MAX_CELLS: usize = 65_536;

    pub fn validate(&self) -> Result<(), String> {
        if self.revision == 0
            || self
                .camera_position
                .iter()
                .chain(&self.predicted_camera_position)
                .any(|value| !value.is_finite())
        {
            return Err(
                "realtime demand requires a non-zero revision and finite cameras".to_owned(),
            );
        }
        if self.visible_cells.len() > Self::MAX_CELLS
            || self.predicted_cells.len() > Self::MAX_CELLS
        {
            return Err("realtime demand exceeds the bounded cell directory".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneContent {
    pub fabric_mods_dir: Option<PathBuf>,
    pub resource_packs: Vec<PathBuf>,
    pub asset_inputs: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PresentationSpec {
    pub backend: RenderBackend,
    pub shaderpack: Option<PathBuf>,
    pub render_settings: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBackend {
    Auto,
    Vulkan,
    IrisGl46,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildAccepted {
    pub build_id: String,
    pub generation: u64,
    pub provisional_scene_key: String,
    pub viewer_session_id: Option<String>,
    pub attached: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum ServiceEvent {
    Snapshot(ServiceSnapshot),
    BuildState(BuildSnapshot),
    ArtifactState(ArtifactSnapshot),
    ViewerState(ViewerSnapshot),
    Error(ServiceError),
    ServiceStopping,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServiceSnapshot {
    pub builds: Vec<BuildSnapshot>,
    pub artifacts: Vec<ArtifactSnapshot>,
    pub viewers: Vec<ViewerSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildSnapshot {
    pub build_id: String,
    pub generation: u64,
    pub state: BuildState,
    pub source_path: PathBuf,
    pub artifact_path: PathBuf,
    pub scene_identity: Option<String>,
    pub sections_committed: u64,
    /// CPU exact Cells completed and handed to the asynchronous artifact writer.
    /// This is independent of artifact-store admission and GPU residency.
    pub sections_compiled: Option<u64>,
    pub sections_existing: Option<u64>,
    pub bytes_committed: u64,
    pub phase: String,
    pub activity: Option<String>,
    pub processed_units: Option<u64>,
    pub total_units: Option<u64>,
    pub elapsed_ms: u64,
    pub error: Option<ServiceError>,
    pub worker_pid: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildState {
    Starting,
    Scanning,
    Normalizing,
    Building,
    Finalizing,
    /// All source/mesh/pack work has completed. Artifact publication is a
    /// separate milestone and may still be waiting for its publish handshake.
    BuildCompleted,
    /// The persistent artifact was atomically published and validated. The
    /// matching ArtifactSnapshot remains the artifact authority.
    ArtifactReady,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ArtifactSnapshot {
    pub scene_identity: String,
    pub generation: u64,
    pub path: PathBuf,
    pub state: ArtifactState,
    pub sections: u64,
    pub bytes: u64,
    pub payload_digest: Option<String>,
    pub analysis_summary: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    Streaming,
    Ready,
    Cancelled,
    Invalid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerSnapshot {
    pub session_id: String,
    pub build_id: String,
    pub generation: u64,
    pub scene_identity: Option<String>,
    pub presentation_identity: Option<String>,
    pub session_identity: Option<String>,
    pub runtime_state: ViewerRuntimeState,
    pub presentation_state: ViewerPresentationState,
    pub pid: Option<u32>,
    pub sections_available: u64,
    /// Dense workloads accepted by the Viewer decode/upload pipeline. This is
    /// not a residency or readiness signal.
    pub sections_gpu_accepted: u64,
    /// Dense workloads whose GPU completion primitive has finished. Bounded
    /// residency may retain fewer sections than this cumulative watermark.
    pub sections_gpu_completed: u64,
    pub elapsed_ms: u64,
    pub error: Option<ServiceError>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerRuntimeState {
    Preparing,
    /// Realtime fixed resources and at least one exact GPU Cell are ready;
    /// the artifact build may still be running.
    RealtimeReady,
    /// The Viewer has accepted the finalized workload. It may still be doing
    /// scene-size runtime work and is neither resident nor prepared yet.
    GpuAccepted,
    /// The residency required for a correct runtime has been validated.
    GpuResident,
    /// Future presentation no longer requires scene-size work.
    ViewerPrepared,
    Released,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerPresentationState {
    Hidden,
    /// The Viewer-owned top-level Popup has completed a successful present.
    PopupVisible,
    /// The Viewer-owned overlay has completed a successful present.
    EmbeddedOverlayVisible {
        rect: ScreenRect,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub focused: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "update", content = "data", rename_all = "snake_case")]
pub enum WorkerUpdate {
    Connected {
        build_id: String,
        generation: u64,
    },
    IdentityResolved(BuildIdentityMaterial),
    Progress(BuildProgress),
    StreamStarted(ArtifactStreamStart),
    RealtimeBootstrap {
        build_id: String,
        generation: u64,
        descriptor: RuntimeBootstrapDescriptor,
        section_positions: Vec<[i32; 3]>,
    },
    SectionCommitted(SectionCommit),
    /// The append-only payload is complete but still resides at its growing
    /// temporary path. Render-service must confirm that an active growing
    /// Viewer retained its file handle (or that presentation is cache-only)
    /// before the worker atomically publishes the final path.
    ArtifactPublishing {
        build_id: String,
        generation: u64,
        descriptor: RuntimeBootstrapDescriptor,
    },
    ArtifactFinalized(ArtifactFinalized),
    Cancelled {
        build_id: String,
        generation: u64,
    },
    Failed {
        build_id: String,
        generation: u64,
        error: ServiceError,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", content = "data", rename_all = "snake_case")]
pub enum WorkerControl {
    RealtimeDemand {
        build_id: String,
        generation: u64,
        demand: RealtimeCellDemand,
    },
    Cancel {
        build_id: String,
        generation: u64,
    },
    /// Releases one dense worker commit after render-service has admitted it
    /// into the active Viewer's negotiated bounded stream window. Actual GPU
    /// completion is reported independently by `ViewerUpdate::Progress`; the
    /// terminal artifact drain still waits for that GPU-completion watermark.
    SectionUploadAck {
        build_id: String,
        generation: u64,
        next_section_index: u64,
    },
    /// Releases exactly one dense section commit when presentation streaming
    /// has been explicitly detached. The persistent artifact build continues.
    SectionUploadBypassed {
        build_id: String,
        generation: u64,
        next_section_index: u64,
        reason: String,
    },
    /// One-time pre-publication gate. Unlike `ArtifactDrainComplete`, this does
    /// not wait for GPU completion; it only makes the temp-path rename safe for
    /// a growing Viewer (or records that no Viewer is consuming the stream).
    ArtifactPublishReady {
        build_id: String,
        generation: u64,
        section_count: u64,
        cache_only: bool,
        reason: Option<String>,
    },
    /// Confirms that render-service consumed the terminal artifact update and,
    /// while the Viewer runtime stayed attached, observed both the true GPU-upload
    /// watermark and `ViewerUpdate::ViewerPrepared`. No active Viewer still depends
    /// on the growing section stream after this barrier.
    ArtifactDrainComplete {
        build_id: String,
        generation: u64,
        section_count: u64,
    },
}

impl WorkerControl {
    fn validate(&self) -> Result<(), String> {
        if let Self::RealtimeDemand {
            build_id,
            generation,
            demand,
        } = self
        {
            require_id("buildId", build_id)?;
            if *generation == 0 {
                return Err("realtime demand requires a non-zero build generation".to_owned());
            }
            demand.validate()?;
        }
        Ok(())
    }
}

impl WorkerUpdate {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::RealtimeBootstrap {
                build_id,
                generation,
                descriptor,
                section_positions,
            } => {
                require_id("buildId", build_id)?;
                if *generation == 0 {
                    return Err("Realtime bootstrap requires a non-zero generation".to_owned());
                }
                validate_realtime_bootstrap(descriptor, section_positions)
            }
            Self::ArtifactPublishing { descriptor, .. } => descriptor.validate(),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildIdentityMaterial {
    pub build_id: String,
    pub generation: u64,
    pub source_sha256: String,
    pub data_version: i32,
    pub canonical_schema_version: u32,
    pub adapter_revision: u32,
    pub resource_inputs: Vec<ContentIdentity>,
    pub assets_digest: String,
    pub build_options: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ContentIdentity {
    pub kind: String,
    pub name: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildProgress {
    pub build_id: String,
    pub generation: u64,
    pub state: BuildState,
    pub phase: String,
    pub activity: Option<String>,
    pub processed_units: Option<u64>,
    pub total_units: Option<u64>,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ArtifactStreamStart {
    pub build_id: String,
    /// Render-resource generation persisted in the artifact. This is distinct
    /// from the render-service Build Job generation below.
    pub resource_generation: u64,
    pub generation: u64,
    pub temporary_path: PathBuf,
    pub format: String,
    pub layout: String,
    pub payload_profile: ArtifactPayloadProfile,
    /// Total number of immutable directory slots frozen before the first
    /// streamed section is committed.
    pub section_count: u64,
    pub bootstrap: Vec<ArtifactBootstrapRange>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ArtifactBootstrapRange {
    pub kind: ArtifactBootstrapKind,
    pub range: CommittedRange,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactBootstrapKind {
    Header,
    MaterialTable,
    AtlasUpload,
    EntityGeometry,
    BlockEntityGeometry,
    VisibilityMetadata,
    SceneMetadata,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SectionCommit {
    pub build_id: String,
    /// Render-resource generation carried by the packed section record.
    pub resource_generation: u64,
    pub generation: u64,
    pub index: u64,
    /// Position-sorted immutable directory slot. This is intentionally
    /// independent from `index`, which remains the dense stream order.
    pub directory_index: u64,
    pub position: [i32; 3],
    pub record: CommittedRange,
    pub exact: CommittedRange,
    /// Vulkan ABI section record. The default preserves decoding of old
    /// diagnostic captures; protocol-v3 workers fail before sending a commit
    /// unless this range is present.
    #[serde(default)]
    pub vulkan_packed: Option<CommittedRange>,
    /// Expanded largest-backend upload bytes, including semantic light data.
    /// Disk compression/content references never reduce GPU byte credit.
    #[serde(default)]
    pub packed_gpu_bytes: u64,
    pub panorama: Option<CommittedRange>,
    pub camera_light: CommittedRange,
    pub light_proxies: Option<CommittedRange>,
}

impl SectionCommit {
    pub fn validate_payload_profile(&self, profile: ArtifactPayloadProfile) -> Result<(), String> {
        match profile {
            ArtifactPayloadProfile::PrebuiltExact => {
                if self.panorama.is_some() || self.light_proxies.is_some() {
                    return Err(
                        "prebuilt_exact section carries realtime fallback/proxy payload".to_owned(),
                    );
                }
            }
            ArtifactPayloadProfile::RealtimeStreamed => {
                if self.panorama.is_some() {
                    return Err(
                        "realtime_streamed section carries forbidden panorama payload".to_owned(),
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CommittedRange {
    pub offset: u64,
    pub length: u64,
    pub crc32: u32,
    pub sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeBootstrapStats {
    pub section_count: u64,
    pub vertex_count: u64,
    pub index_count: u64,
    pub primitive_count: u64,
    pub material_count: u64,
    pub block_entity_count: u64,
    pub entity_count: u64,
    pub diagnostic_count: u64,
}

/// Fixed-size authenticated description of the runtime records appended after
/// streamed section geometry. It never carries scene data through IPC: the
/// Viewer reads these three ranges from its retained growing-file handle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeBootstrapDescriptor {
    pub resource_generation: u64,
    pub payload_profile: ArtifactPayloadProfile,
    pub stats: RuntimeBootstrapStats,
    pub payload_offset: u64,
    pub payload_bytes: u64,
    pub payload_sha256: [u8; 32],
    pub runtime_header: CommittedRange,
    pub weather: CommittedRange,
    pub dynamic: CommittedRange,
}

impl RuntimeBootstrapDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        if self.resource_generation == 0 {
            return Err("runtime bootstrap resource generation must be non-zero".to_owned());
        }
        if self.payload_bytes == 0 {
            return Err("runtime bootstrap payload must be non-empty".to_owned());
        }
        let payload_end = self
            .payload_offset
            .checked_add(self.payload_bytes)
            .ok_or_else(|| "runtime bootstrap payload range overflows u64".to_owned())?;
        let range_end = |name: &str, range: &CommittedRange| {
            if range.length == 0 {
                return Err(format!("runtime bootstrap {name} range must be non-empty"));
            }
            range
                .offset
                .checked_add(range.length)
                .ok_or_else(|| format!("runtime bootstrap {name} range overflows u64"))
        };
        let header_end = range_end("header", &self.runtime_header)?;
        let weather_end = range_end("weather", &self.weather)?;
        let dynamic_end = range_end("dynamic", &self.dynamic)?;
        if self.runtime_header.offset < self.payload_offset
            || header_end != self.weather.offset
            || weather_end != self.dynamic.offset
            || dynamic_end > payload_end
        {
            return Err(
                "runtime bootstrap ranges are not contiguous within the payload".to_owned(),
            );
        }
        Ok(())
    }
}

fn validate_realtime_bootstrap(
    descriptor: &RuntimeBootstrapDescriptor,
    positions: &[[i32; 3]],
) -> Result<(), String> {
    descriptor.validate()?;
    if descriptor.payload_profile != ArtifactPayloadProfile::RealtimeStreamed
        || descriptor.stats.section_count != positions.len() as u64
        || positions
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != positions.len()
    {
        return Err(
            "Realtime bootstrap requires a unique complete ExistingCells directory".to_owned(),
        );
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ArtifactFinalized {
    pub build_id: String,
    pub generation: u64,
    pub final_path: PathBuf,
    pub section_count: u64,
    pub payload_bytes: u64,
    pub payload_digest: String,
    /// `stats_api::StableStructureOutput` serialized once by the worker. The
    /// UI consumes this field directly on artifact hits and never reopens the
    /// source projection for statistics.
    pub analysis_summary: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "update", content = "data", rename_all = "snake_case")]
pub enum ViewerUpdate {
    RealtimeReady {
        session_id: String,
        build_id: String,
        generation: u64,
        sections_compiled: u64,
        sections_existing: u64,
        exact_resident_sections: u64,
    },
    /// Dense durable CPU/artifact-store admission; this is never GPU completion.
    RealtimeCellsStored {
        session_id: String,
        build_id: String,
        generation: u64,
        sections_stored: u64,
    },
    RealtimeDemand {
        session_id: String,
        build_id: String,
        generation: u64,
        demand: RealtimeCellDemand,
    },
    /// Advertises the amount of committed artifact work that the Viewer pipe,
    /// decoder and staging path can retain before applying producer
    /// backpressure. This is sent immediately after transport setup, before
    /// renderer/device initialization, so those fixed costs overlap cold build.
    StreamCapacity {
        session_id: String,
        /// Maximum dense commit records retained between dispatch and the
        /// GPU-completion watermark.
        max_inflight_sections: u64,
        /// Maximum encoded bytes for the selected backend payload retained in
        /// that window. Iris counts exact/panorama/camera/proxy ranges; Vulkan
        /// counts only its packed upload range.
        max_inflight_bytes: u64,
    },
    /// Confirms that the Viewer retained an open, share-delete handle to the
    /// growing append-only artifact. Section dispatch must not start before
    /// this update because final publication may rename the `.tmp` path while
    /// a bounded GPU tail is still being decoded.
    StreamOpened {
        session_id: String,
        build_id: String,
        generation: u64,
        resource_generation: u64,
    },
    Connected {
        session_id: String,
        pid: u32,
        device_generation: u64,
    },
    Progress {
        session_id: String,
        sections_gpu_accepted: u64,
        sections_gpu_completed: u64,
        elapsed_ms: u64,
    },
    /// Confirms that the Viewer authenticated and consumed the fixed runtime
    /// bootstrap records from its retained growing-file handle after the full
    /// section stream drained. The descriptor is echoed verbatim so
    /// render-service can reject a stale or mismatched acknowledgement.
    ///
    /// This barrier makes artifact publication safe. It explicitly does not
    /// imply `GpuResident` or `ViewerPrepared`.
    RuntimeBootstrapPrepared {
        session_id: String,
        build_id: String,
        /// Immutable build generation selected by render-service.
        generation: u64,
        /// Viewer device generation which consumed the descriptor.
        device_generation: u64,
        descriptor: RuntimeBootstrapDescriptor,
        sections_gpu_accepted: u64,
        sections_gpu_completed: u64,
    },
    /// All finalized work has been accepted by the Viewer pipeline. Equality
    /// with the total does not imply residency or preparation.
    GpuAccepted {
        session_id: String,
        build_id: String,
        generation: u64,
        sections_gpu_accepted: u64,
        sections_gpu_completed: u64,
        elapsed_ms: u64,
    },
    /// The correctness-required residency has passed hidden validation.
    GpuResident {
        session_id: String,
        build_id: String,
        generation: u64,
        sections_gpu_accepted: u64,
        sections_gpu_completed: u64,
        elapsed_ms: u64,
    },
    ViewerPrepared {
        session_id: String,
        build_id: String,
        generation: u64,
        sections_gpu_accepted: u64,
        sections_gpu_completed: u64,
        elapsed_ms: u64,
    },
    Hidden {
        session_id: String,
    },
    PresentationVisible {
        session_id: String,
        build_id: String,
        generation: u64,
        state: ViewerPresentationState,
    },
    HeadlessCameraStressPresented {
        session_id: String,
        generation: u64,
        frames: u32,
    },
    Released {
        session_id: String,
    },
    Failed {
        session_id: String,
        error: ServiceError,
    },
}

impl ViewerUpdate {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::RealtimeReady {
                session_id,
                build_id,
                generation,
                sections_compiled,
                sections_existing,
                exact_resident_sections,
            } => {
                require_id("sessionId", session_id)?;
                require_id("buildId", build_id)?;
                if *generation == 0
                    || *exact_resident_sections == 0
                    || *sections_compiled > *sections_existing
                    || *exact_resident_sections > *sections_compiled
                {
                    return Err("RealtimeReady requires a real ready exact Cell within compiled ExistingCells".to_owned());
                }
            }
            Self::RealtimeCellsStored {
                session_id,
                build_id,
                generation,
                ..
            } => {
                require_id("sessionId", session_id)?;
                require_id("buildId", build_id)?;
                if *generation == 0 {
                    return Err("Realtime store receipt requires generation".to_owned());
                }
            }
            Self::RealtimeDemand {
                session_id,
                build_id,
                generation,
                demand,
            } => {
                require_id("sessionId", session_id)?;
                require_id("buildId", build_id)?;
                if *generation == 0 {
                    return Err("realtime demand requires a non-zero build generation".to_owned());
                }
                demand.validate()?;
            }
            Self::StreamCapacity {
                session_id,
                max_inflight_sections,
                max_inflight_bytes,
            } => {
                require_id("sessionId", session_id)?;
                if *max_inflight_sections == 0 {
                    return Err("maxInflightSections must be non-zero".to_owned());
                }
                if *max_inflight_bytes == 0 {
                    return Err("maxInflightBytes must be non-zero".to_owned());
                }
            }
            Self::StreamOpened {
                session_id,
                build_id,
                ..
            } => {
                require_id("sessionId", session_id)?;
                require_id("buildId", build_id)?;
            }
            Self::RuntimeBootstrapPrepared {
                session_id,
                build_id,
                generation,
                device_generation,
                descriptor,
                sections_gpu_accepted,
                sections_gpu_completed,
            } => {
                validate_runtime_bootstrap_tuple(
                    session_id,
                    build_id,
                    *generation,
                    *device_generation,
                    descriptor,
                )?;
                let expected = descriptor.stats.section_count;
                if *sections_gpu_accepted != expected || *sections_gpu_completed != expected {
                    return Err(format!(
                        "RuntimeBootstrapPrepared requires drained GPU watermarks {expected}/{expected}; got {sections_gpu_accepted}/{sections_gpu_completed}"
                    ));
                }
            }
            Self::GpuAccepted {
                session_id,
                build_id,
                ..
            }
            | Self::GpuResident {
                session_id,
                build_id,
                ..
            }
            | Self::ViewerPrepared {
                session_id,
                build_id,
                ..
            } => {
                require_id("sessionId", session_id)?;
                require_id("buildId", build_id)?;
            }
            Self::Connected { session_id, .. }
            | Self::Progress { session_id, .. }
            | Self::Hidden { session_id }
            | Self::Released { session_id }
            | Self::Failed { session_id, .. } => require_id("sessionId", session_id)?,
            Self::PresentationVisible {
                session_id,
                build_id,
                state,
                ..
            } => {
                require_id("sessionId", session_id)?;
                require_id("buildId", build_id)?;
                if matches!(state, ViewerPresentationState::Hidden) {
                    return Err("PresentationVisible cannot carry Hidden state".to_owned());
                }
            }
            Self::HeadlessCameraStressPresented {
                session_id,
                generation,
                frames,
            } => {
                require_id("sessionId", session_id)?;
                if *generation == 0 || *frames == 0 {
                    return Err(
                        "headless camera stress requires non-zero generation and frames".to_owned(),
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", content = "data", rename_all = "snake_case")]
pub enum ViewerControl {
    RealtimeBootstrap {
        session_id: String,
        build_id: String,
        generation: u64,
        device_generation: u64,
        descriptor: RuntimeBootstrapDescriptor,
        section_positions: Vec<[i32; 3]>,
    },
    AwaitArtifact {
        session_id: String,
        build_id: String,
        generation: u64,
        source_path: PathBuf,
        artifact_path: PathBuf,
        build_mode: RenderBuildMode,
        /// Hard upper bound for the complete exact geometry runtime admitted
        /// by the matching Prebuilt resource preflight. This includes exact
        /// draw/command metadata owned by the renderer, not other product
        /// resources accounted separately by the preflight policy.
        prebuilt_exact_gpu_budget_bytes: Option<u64>,
        content: SceneContent,
        presentation: PresentationSpec,
    },
    StreamStarted(ArtifactStreamStart),
    SectionCommitted(SectionCommit),
    /// Authenticated fixed-size runtime records which the Viewer reads from
    /// the retained growing artifact before render-service permits the final
    /// path publication. No scene-scale bytes are carried through this IPC.
    RuntimeBootstrap {
        session_id: String,
        build_id: String,
        /// Immutable build generation selected by render-service.
        generation: u64,
        /// Viewer device generation expected to consume the descriptor.
        device_generation: u64,
        descriptor: RuntimeBootstrapDescriptor,
    },
    ArtifactFinalized(ArtifactFinalized),
    ShowPopup {
        session_id: String,
    },
    ShowEmbeddedOverlay {
        session_id: String,
        rect: ScreenRect,
    },
    Hide {
        session_id: String,
    },
    HeadlessCameraStress {
        session_id: String,
        generation: u64,
    },
    Release {
        session_id: String,
    },
}

impl ViewerControl {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::RealtimeBootstrap {
                session_id,
                build_id,
                generation,
                device_generation,
                descriptor,
                section_positions,
            } => {
                require_id("sessionId", session_id)?;
                require_id("buildId", build_id)?;
                if *generation == 0 || *device_generation == 0 {
                    return Err(
                        "Realtime bootstrap requires build and device generations".to_owned()
                    );
                }
                validate_realtime_bootstrap(descriptor, section_positions)?;
            }
            Self::AwaitArtifact {
                build_mode,
                prebuilt_exact_gpu_budget_bytes,
                ..
            } => match (build_mode, prebuilt_exact_gpu_budget_bytes) {
                (RenderBuildMode::Prebuilt, Some(bytes)) if *bytes != 0 => {}
                (RenderBuildMode::Prebuilt, _) => {
                    return Err(
                        "Prebuilt Viewer requires a non-zero exact GPU admission budget".to_owned(),
                    );
                }
                (RenderBuildMode::Realtime, None) => {}
                (RenderBuildMode::Realtime, Some(_)) => {
                    return Err(
                        "Realtime Viewer must not carry a Prebuilt exact GPU admission budget"
                            .to_owned(),
                    );
                }
            },
            Self::RuntimeBootstrap {
                session_id,
                build_id,
                generation,
                device_generation,
                descriptor,
            } => {
                validate_runtime_bootstrap_tuple(
                    session_id,
                    build_id,
                    *generation,
                    *device_generation,
                    descriptor,
                )?;
            }
            Self::HeadlessCameraStress {
                session_id,
                generation,
            } => {
                require_id("sessionId", session_id)?;
                if *generation == 0 {
                    return Err("headless camera stress requires a non-zero generation".to_owned());
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServiceError {
    pub code: String,
    pub message: String,
    pub fatal: bool,
}

impl ServiceError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, fatal: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            fatal,
        }
    }
}

pub fn read_envelope(reader: &mut impl Read) -> io::Result<Option<Envelope>> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid render service frame length {length}"),
        ));
    }
    let mut bytes = vec![0_u8; length];
    reader.read_exact(&mut bytes)?;
    let envelope: Envelope = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    envelope
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Some(envelope))
}

pub fn write_envelope(writer: &mut impl Write, envelope: &Envelope) -> io::Result<()> {
    envelope
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let bytes = serde_json::to_vec(envelope)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "render service frame exceeds maximum size",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

fn require_id(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!(
            "{field} must contain 1..=160 ASCII letters, digits, '-', '_' or '.'"
        ));
    }
    Ok(())
}

fn validate_runtime_bootstrap_tuple(
    session_id: &str,
    build_id: &str,
    generation: u64,
    device_generation: u64,
    descriptor: &RuntimeBootstrapDescriptor,
) -> Result<(), String> {
    require_id("sessionId", session_id)?;
    require_id("buildId", build_id)?;
    if generation == 0 {
        return Err("runtime bootstrap build generation must be non-zero".to_owned());
    }
    if device_generation == 0 {
        return Err("runtime bootstrap device generation must be non-zero".to_owned());
    }
    descriptor.validate()
}

#[cfg(windows)]
pub mod windows_pipe;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed_round_trip(payload: Payload) -> Payload {
        let mut bytes = Vec::new();
        write_envelope(&mut bytes, &Envelope::new(7, payload)).unwrap();
        read_envelope(&mut Cursor::new(bytes))
            .unwrap()
            .expect("one framed envelope")
            .payload
    }

    #[test]
    fn hide_viewer_remains_distinct_from_releasing_viewer_on_the_wire() {
        let payload = framed_round_trip(Payload::ViewerControl(ViewerControl::Hide {
            session_id: "viewer-7".to_owned(),
        }));
        match payload {
            Payload::ViewerControl(ViewerControl::Hide { session_id }) => {
                assert_eq!(session_id, "viewer-7")
            }
            _ => panic!("Hide must not deserialize as Release"),
        }
    }

    #[test]
    fn headless_camera_stress_keeps_session_generation_and_presented_frame_count() {
        let session_id = "viewer-7".to_owned();
        let request = framed_round_trip(Payload::Request(Request {
            request_id: "stress-7".to_owned(),
            command: UiCommand::HeadlessCameraStress {
                session_id: session_id.clone(),
            },
        }));
        assert!(matches!(
            request,
            Payload::Request(Request {
                command: UiCommand::HeadlessCameraStress { session_id },
                ..
            }) if session_id == "viewer-7"
        ));
        let control = framed_round_trip(Payload::ViewerControl(
            ViewerControl::HeadlessCameraStress {
                session_id: session_id.clone(),
                generation: 9,
            },
        ));
        assert!(matches!(
            control,
            Payload::ViewerControl(ViewerControl::HeadlessCameraStress {
                session_id,
                generation: 9,
            }) if session_id == "viewer-7"
        ));
        let update = framed_round_trip(Payload::ViewerUpdate(
            ViewerUpdate::HeadlessCameraStressPresented {
                session_id,
                generation: 9,
                frames: HEADLESS_CAMERA_STRESS_FRAMES,
            },
        ));
        assert!(matches!(
            update,
            Payload::ViewerUpdate(ViewerUpdate::HeadlessCameraStressPresented {
                generation: 9,
                frames: HEADLESS_CAMERA_STRESS_FRAMES,
                ..
            })
        ));
    }

    #[test]
    fn headless_camera_stress_rejects_zero_generation_and_frame_count() {
        let mut bytes = Vec::new();
        assert!(
            write_envelope(
                &mut bytes,
                &Envelope::new(
                    1,
                    Payload::ViewerControl(ViewerControl::HeadlessCameraStress {
                        session_id: "viewer-7".to_owned(),
                        generation: 0,
                    }),
                ),
            )
            .is_err()
        );
        assert!(
            write_envelope(
                &mut bytes,
                &Envelope::new(
                    2,
                    Payload::ViewerUpdate(ViewerUpdate::HeadlessCameraStressPresented {
                        session_id: "viewer-7".to_owned(),
                        generation: 9,
                        frames: 0,
                    }),
                ),
            )
            .is_err()
        );
    }

    #[test]
    fn render_build_mode_has_stable_product_wire_values() {
        assert_eq!(
            serde_json::to_string(&RenderBuildMode::Prebuilt).unwrap(),
            r#""prebuilt""#
        );
        assert_eq!(
            serde_json::to_string(&RenderBuildMode::Realtime).unwrap(),
            r#""realtime""#
        );
        assert_eq!(
            RenderBuildMode::Prebuilt.artifact_payload_profile(),
            ArtifactPayloadProfile::PrebuiltExact
        );
        assert_eq!(
            RenderBuildMode::Realtime.artifact_payload_profile(),
            ArtifactPayloadProfile::RealtimeStreamed
        );
    }

    fn resource_preflight_fixture_build(mode: RenderBuildMode) -> BuildStart {
        BuildStart {
            source_path: PathBuf::from(r"C:\projections\preflight.litematic"),
            display_mode: "full".to_owned(),
            build_mode: mode,
            content: SceneContent::default(),
            build_options: BTreeMap::from([("displayMode".to_owned(), "full".to_owned())]),
            presentation: PresentationSpec {
                backend: RenderBackend::Vulkan,
                shaderpack: None,
                render_settings: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn prebuilt_start_requires_admission_while_realtime_never_carries_one() {
        let prebuilt = resource_preflight_fixture_build(RenderBuildMode::Prebuilt);
        assert!(
            UiCommand::StartBuild(AdmittedBuildStart {
                build: prebuilt.clone(),
                admission: None,
            })
            .validate()
            .is_err()
        );
        UiCommand::StartBuild(AdmittedBuildStart {
            build: prebuilt,
            admission: Some(BuildAdmission {
                token: "admission-prebuilt-1".to_owned(),
                resource_warning_confirmed: true,
            }),
        })
        .validate()
        .unwrap();

        let realtime = resource_preflight_fixture_build(RenderBuildMode::Realtime);
        UiCommand::StartBuild(AdmittedBuildStart {
            build: realtime.clone(),
            admission: None,
        })
        .validate()
        .unwrap();
        assert!(
            UiCommand::StartBuild(AdmittedBuildStart {
                build: realtime,
                admission: Some(BuildAdmission {
                    token: "admission-invalid-realtime".to_owned(),
                    resource_warning_confirmed: false,
                }),
            })
            .validate()
            .is_err()
        );
    }

    #[test]
    fn admission_is_request_local_and_cannot_change_build_identity() {
        let build = resource_preflight_fixture_build(RenderBuildMode::Prebuilt);
        let identity_before = identity::provisional_scene_key(&build).unwrap();
        let first = AdmittedBuildStart {
            build: build.clone(),
            admission: Some(BuildAdmission {
                token: "admission-a".to_owned(),
                resource_warning_confirmed: false,
            }),
        };
        let second = AdmittedBuildStart {
            build,
            admission: Some(BuildAdmission {
                token: "admission-b".to_owned(),
                resource_warning_confirmed: true,
            }),
        };

        assert_eq!(first.build, second.build);
        assert_ne!(first.admission, second.admission);
        assert_eq!(
            identity_before,
            identity::provisional_scene_key(&second.build).unwrap()
        );
    }

    #[test]
    fn resource_preflight_round_trip_preserves_budget_components() {
        let response = BuildResourcePreflight {
            pressure: ResourcePressure::Warning,
            conservative_vram_bytes: Some(8_500),
            safe_available_vram_bytes: Some(10_000),
            pressure_basis_points: Some(8_500),
            components: Some(ConservativeVramComponents {
                exact_geometry_bytes: 5_000,
                draw_metadata_bytes: 500,
                block_entity_bytes: 250,
                texture_and_shader_bytes: 1_000,
                render_target_bytes: 750,
                runtime_reserve_bytes: 1_000,
            }),
            invalid_reason: None,
            admission_token: Some("admission-warning-1".to_owned()),
        };
        let payload = framed_round_trip(Payload::Response(Response {
            request_id: "preflight-1".to_owned(),
            result: Ok(ResponseBody::BuildPreflight(response.clone())),
        }));
        let Payload::Response(Response {
            result: Ok(ResponseBody::BuildPreflight(actual)),
            ..
        }) = payload
        else {
            panic!("expected BuildPreflight response")
        };
        assert_eq!(actual, response);
    }

    #[test]
    fn invalid_resource_preflight_round_trip_has_no_numeric_sentinels() {
        let response = BuildResourcePreflight {
            pressure: ResourcePressure::Invalid,
            conservative_vram_bytes: None,
            safe_available_vram_bytes: None,
            pressure_basis_points: None,
            components: None,
            invalid_reason: Some("metadata unavailable".to_owned()),
            admission_token: None,
        };
        let payload = framed_round_trip(Payload::Response(Response {
            request_id: "preflight-invalid".to_owned(),
            result: Ok(ResponseBody::BuildPreflight(response.clone())),
        }));
        let Payload::Response(Response {
            result: Ok(ResponseBody::BuildPreflight(actual)),
            ..
        }) = payload
        else {
            panic!("expected BuildPreflight response")
        };
        assert_eq!(actual, response);
    }

    #[test]
    fn section_payload_profile_rejects_cross_mode_artifacts() {
        let range = CommittedRange {
            offset: 1,
            length: 1,
            crc32: 1,
            sha256: [1; 32],
        };
        let mut section = SectionCommit {
            build_id: "build-profile".to_owned(),
            resource_generation: 3,
            generation: 5,
            index: 0,
            directory_index: 0,
            position: [0, 0, 0],
            record: range.clone(),
            exact: range.clone(),
            vulkan_packed: Some(range.clone()),
            packed_gpu_bytes: 0,
            panorama: None,
            camera_light: range.clone(),
            light_proxies: None,
        };
        section
            .validate_payload_profile(ArtifactPayloadProfile::PrebuiltExact)
            .unwrap();
        section
            .validate_payload_profile(ArtifactPayloadProfile::RealtimeStreamed)
            .unwrap();

        section.panorama = Some(range.clone());
        assert!(
            section
                .validate_payload_profile(ArtifactPayloadProfile::RealtimeStreamed)
                .is_err()
        );
        assert!(
            section
                .validate_payload_profile(ArtifactPayloadProfile::PrebuiltExact)
                .is_err()
        );

        section.panorama = None;
        section.light_proxies = Some(range);
        section
            .validate_payload_profile(ArtifactPayloadProfile::RealtimeStreamed)
            .unwrap();
        assert!(
            section
                .validate_payload_profile(ArtifactPayloadProfile::PrebuiltExact)
                .is_err()
        );
    }

    #[test]
    fn realtime_demand_round_trip_and_validation() {
        let demand = RealtimeCellDemand {
            revision: 1,
            camera_position: [0.0; 3],
            predicted_camera_position: [1.0, 0.0, 0.0],
            visible_cells: vec![[-2, 1, 8]],
            predicted_cells: vec![[1, 1, 8]],
        };
        let payload = framed_round_trip(Payload::ViewerUpdate(ViewerUpdate::RealtimeDemand {
            session_id: "viewer-realtime".into(),
            build_id: "build-realtime".into(),
            generation: 1,
            demand: demand.clone(),
        }));
        let Payload::ViewerUpdate(ViewerUpdate::RealtimeDemand {
            demand: restored, ..
        }) = payload
        else {
            panic!("wrong demand payload")
        };
        assert_eq!(restored, demand);
        let mut invalid = demand.clone();
        invalid.camera_position[1] = f64::NAN;
        assert!(invalid.validate().is_err());
        invalid = demand;
        invalid.revision = 0;
        assert!(invalid.validate().is_err());
        assert!(!ArtifactPayloadProfile::RealtimeStreamed.carries_fallback());
        assert!(ArtifactPayloadProfile::RealtimeStreamed.carries_light_block_proxies());
        assert!(!ArtifactPayloadProfile::PrebuiltExact.carries_light_block_proxies());
    }

    #[test]
    fn runtime_bootstrap_descriptor_round_trips_without_scene_payload() {
        let descriptor = RuntimeBootstrapDescriptor {
            resource_generation: 17,
            payload_profile: ArtifactPayloadProfile::PrebuiltExact,
            stats: RuntimeBootstrapStats {
                section_count: 45_276,
                vertex_count: 90_000,
                index_count: 120_000,
                primitive_count: 40_000,
                material_count: 64,
                block_entity_count: 7,
                entity_count: 3,
                diagnostic_count: 2,
            },
            payload_offset: 64,
            payload_bytes: 1_024,
            payload_sha256: [11; 32],
            runtime_header: CommittedRange {
                offset: 640,
                length: 128,
                crc32: 1,
                sha256: [1; 32],
            },
            weather: CommittedRange {
                offset: 768,
                length: 64,
                crc32: 2,
                sha256: [2; 32],
            },
            dynamic: CommittedRange {
                offset: 832,
                length: 128,
                crc32: 3,
                sha256: [3; 32],
            },
        };
        descriptor.validate().unwrap();
        let payload = framed_round_trip(Payload::WorkerUpdate(WorkerUpdate::ArtifactPublishing {
            build_id: "build-bootstrap".to_owned(),
            generation: 23,
            descriptor: descriptor.clone(),
        }));
        match payload {
            Payload::WorkerUpdate(WorkerUpdate::ArtifactPublishing {
                descriptor: actual, ..
            }) => assert_eq!(actual, descriptor),
            _ => panic!("expected ArtifactPublishing"),
        }
    }

    #[test]
    fn runtime_bootstrap_descriptor_rejects_noncontiguous_or_external_ranges() {
        let range = |offset, length| CommittedRange {
            offset,
            length,
            crc32: 1,
            sha256: [1; 32],
        };
        let mut descriptor = RuntimeBootstrapDescriptor {
            resource_generation: 17,
            payload_profile: ArtifactPayloadProfile::PrebuiltExact,
            stats: RuntimeBootstrapStats {
                section_count: 0,
                vertex_count: 0,
                index_count: 0,
                primitive_count: 0,
                material_count: 0,
                block_entity_count: 0,
                entity_count: 0,
                diagnostic_count: 0,
            },
            payload_offset: 64,
            payload_bytes: 512,
            payload_sha256: [9; 32],
            runtime_header: range(128, 64),
            weather: range(192, 32),
            dynamic: range(224, 64),
        };
        descriptor.validate().unwrap();
        descriptor.weather.offset += 1;
        assert!(descriptor.validate().is_err());
        descriptor.weather.offset -= 1;
        descriptor.dynamic.length = 512;
        assert!(descriptor.validate().is_err());
    }

    fn runtime_bootstrap_wire_fixture(section_count: u64) -> RuntimeBootstrapDescriptor {
        RuntimeBootstrapDescriptor {
            resource_generation: 17,
            payload_profile: ArtifactPayloadProfile::PrebuiltExact,
            stats: RuntimeBootstrapStats {
                section_count,
                vertex_count: section_count.saturating_mul(8),
                index_count: section_count.saturating_mul(12),
                primitive_count: section_count.saturating_mul(4),
                material_count: if section_count == 0 { 0 } else { 1 },
                block_entity_count: 0,
                entity_count: 0,
                diagnostic_count: 0,
            },
            payload_offset: 64,
            payload_bytes: 1_024,
            payload_sha256: [11; 32],
            runtime_header: CommittedRange {
                offset: 640,
                length: 128,
                crc32: 1,
                sha256: [1; 32],
            },
            weather: CommittedRange {
                offset: 768,
                length: 64,
                crc32: 2,
                sha256: [2; 32],
            },
            dynamic: CommittedRange {
                offset: 832,
                length: 128,
                crc32: 3,
                sha256: [3; 32],
            },
        }
    }

    #[test]
    fn realtime_bootstrap_ready_and_store_receipts_are_distinct_wire_events() {
        let mut descriptor = runtime_bootstrap_wire_fixture(2);
        descriptor.payload_profile = ArtifactPayloadProfile::RealtimeStreamed;
        let positions = vec![[0, 0, 0], [1, 0, 0]];
        let bootstrap = ViewerControl::RealtimeBootstrap {
            session_id: "viewer-1".to_owned(),
            build_id: "build-1".to_owned(),
            generation: 7,
            device_generation: 3,
            descriptor: descriptor.clone(),
            section_positions: positions.clone(),
        };
        bootstrap.validate().unwrap();
        assert!(
            matches!(framed_round_trip(Payload::ViewerControl(bootstrap)),
            Payload::ViewerControl(ViewerControl::RealtimeBootstrap { section_positions, .. }) if section_positions == positions)
        );
        let worker = WorkerUpdate::RealtimeBootstrap {
            build_id: "build-1".to_owned(),
            generation: 7,
            descriptor: descriptor.clone(),
            section_positions: positions.clone(),
        };
        worker.validate().unwrap();
        assert!(matches!(
            framed_round_trip(Payload::WorkerUpdate(worker)),
            Payload::WorkerUpdate(WorkerUpdate::RealtimeBootstrap { .. })
        ));
        let ready = ViewerUpdate::RealtimeReady {
            session_id: "viewer-1".to_owned(),
            build_id: "build-1".to_owned(),
            generation: 7,
            sections_compiled: 1,
            sections_existing: 2,
            exact_resident_sections: 1,
        };
        ready.validate().unwrap();
        assert!(matches!(
            framed_round_trip(Payload::ViewerUpdate(ready)),
            Payload::ViewerUpdate(ViewerUpdate::RealtimeReady {
                sections_compiled: 1,
                sections_existing: 2,
                ..
            })
        ));
        let stored = ViewerUpdate::RealtimeCellsStored {
            session_id: "viewer-1".to_owned(),
            build_id: "build-1".to_owned(),
            generation: 7,
            sections_stored: 2,
        };
        stored.validate().unwrap();
        assert!(matches!(
            framed_round_trip(Payload::ViewerUpdate(stored)),
            Payload::ViewerUpdate(ViewerUpdate::RealtimeCellsStored {
                sections_stored: 2,
                ..
            })
        ));
        descriptor.payload_profile = ArtifactPayloadProfile::PrebuiltExact;
        assert!(validate_realtime_bootstrap(&descriptor, &positions).is_err());
        descriptor.payload_profile = ArtifactPayloadProfile::RealtimeStreamed;
        assert!(validate_realtime_bootstrap(&descriptor, &[[0; 3], [0; 3]]).is_err());
        assert!(validate_realtime_bootstrap(&descriptor, &positions[..1]).is_err());
        let no_gpu_proof = ViewerUpdate::RealtimeReady {
            session_id: "viewer-1".to_owned(),
            build_id: "build-1".to_owned(),
            generation: 7,
            sections_compiled: 1,
            sections_existing: 2,
            exact_resident_sections: 0,
        };
        assert!(no_gpu_proof.validate().is_err());
    }

    #[test]
    fn runtime_bootstrap_handshake_round_trips_bound_generations_and_drained_watermarks() {
        let descriptor = runtime_bootstrap_wire_fixture(3);
        let control = framed_round_trip(Payload::ViewerControl(ViewerControl::RuntimeBootstrap {
            session_id: "viewer-bootstrap".to_owned(),
            build_id: "build-bootstrap".to_owned(),
            generation: 23,
            device_generation: 29,
            descriptor: descriptor.clone(),
        }));
        match control {
            Payload::ViewerControl(ViewerControl::RuntimeBootstrap {
                session_id,
                build_id,
                generation,
                device_generation,
                descriptor: actual,
            }) => {
                assert_eq!(session_id, "viewer-bootstrap");
                assert_eq!(build_id, "build-bootstrap");
                assert_eq!(generation, 23);
                assert_eq!(device_generation, 29);
                assert_eq!(actual, descriptor);
            }
            _ => panic!("expected RuntimeBootstrap control"),
        }

        let acknowledgement = ViewerUpdate::RuntimeBootstrapPrepared {
            session_id: "viewer-bootstrap".to_owned(),
            build_id: "build-bootstrap".to_owned(),
            generation: 23,
            device_generation: 29,
            descriptor: descriptor.clone(),
            sections_gpu_accepted: 3,
            sections_gpu_completed: 3,
        };
        let wire = serde_json::to_value(Payload::ViewerUpdate(acknowledgement.clone())).unwrap();
        assert_eq!(
            wire.pointer("/data/update")
                .and_then(serde_json::Value::as_str),
            Some("runtime_bootstrap_prepared")
        );
        assert_ne!(
            wire.pointer("/data/update")
                .and_then(serde_json::Value::as_str),
            Some("viewer_prepared"),
            "runtime bootstrap publication ACK must remain distinct from ViewerPrepared"
        );

        let acknowledgement = framed_round_trip(Payload::ViewerUpdate(acknowledgement));
        match acknowledgement {
            Payload::ViewerUpdate(ViewerUpdate::RuntimeBootstrapPrepared {
                session_id,
                build_id,
                generation,
                device_generation,
                descriptor: actual,
                sections_gpu_accepted,
                sections_gpu_completed,
            }) => {
                assert_eq!(session_id, "viewer-bootstrap");
                assert_eq!(build_id, "build-bootstrap");
                assert_eq!(generation, 23);
                assert_eq!(device_generation, 29);
                assert_eq!(actual, descriptor);
                assert_eq!(sections_gpu_accepted, 3);
                assert_eq!(sections_gpu_completed, 3);
            }
            _ => panic!("expected RuntimeBootstrapPrepared update"),
        }
    }

    #[test]
    fn runtime_bootstrap_validation_rejects_empty_or_zero_identity_tuple() {
        let descriptor = runtime_bootstrap_wire_fixture(1);
        let control = |session_id: &str, build_id: &str, generation, device_generation| {
            ViewerControl::RuntimeBootstrap {
                session_id: session_id.to_owned(),
                build_id: build_id.to_owned(),
                generation,
                device_generation,
                descriptor: descriptor.clone(),
            }
        };

        control("viewer-bootstrap", "build-bootstrap", 23, 29)
            .validate()
            .unwrap();
        assert!(control("", "build-bootstrap", 23, 29).validate().is_err());
        assert!(control("viewer-bootstrap", "", 23, 29).validate().is_err());
        assert!(
            control("viewer-bootstrap", "build-bootstrap", 0, 29)
                .validate()
                .is_err()
        );
        assert!(
            control("viewer-bootstrap", "build-bootstrap", 23, 0)
                .validate()
                .is_err()
        );

        // The protocol validates tuple shape. Only render-service has the
        // current session state needed to decide whether a non-zero generation
        // is stale.
        control("viewer-bootstrap", "build-bootstrap", 24, 30)
            .validate()
            .unwrap();

        let mut invalid_descriptor = descriptor;
        invalid_descriptor.resource_generation = 0;
        assert!(
            ViewerControl::RuntimeBootstrap {
                session_id: "viewer-bootstrap".to_owned(),
                build_id: "build-bootstrap".to_owned(),
                generation: 23,
                device_generation: 29,
                descriptor: invalid_descriptor,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn runtime_bootstrap_prepared_requires_the_full_gpu_drain_watermark() {
        let acknowledgement = |descriptor: RuntimeBootstrapDescriptor, accepted, completed| {
            ViewerUpdate::RuntimeBootstrapPrepared {
                session_id: "viewer-bootstrap".to_owned(),
                build_id: "build-bootstrap".to_owned(),
                generation: 23,
                device_generation: 29,
                descriptor,
                sections_gpu_accepted: accepted,
                sections_gpu_completed: completed,
            }
        };
        let descriptor = runtime_bootstrap_wire_fixture(2);

        acknowledgement(descriptor.clone(), 2, 2)
            .validate()
            .unwrap();
        assert!(
            acknowledgement(descriptor.clone(), 1, 1)
                .validate()
                .is_err()
        );
        assert!(
            acknowledgement(descriptor.clone(), 2, 1)
                .validate()
                .is_err()
        );
        assert!(acknowledgement(descriptor, 3, 3).validate().is_err());

        acknowledgement(runtime_bootstrap_wire_fixture(0), 0, 0)
            .validate()
            .unwrap();
    }

    #[test]
    fn frozen_directory_identity_round_trips_for_zero_and_reordered_streams() {
        let zero = framed_round_trip(Payload::ViewerControl(ViewerControl::StreamStarted(
            ArtifactStreamStart {
                build_id: "build-empty".to_owned(),
                resource_generation: 3,
                generation: 5,
                temporary_path: PathBuf::from("empty.tmp"),
                format: "format".to_owned(),
                layout: "layout".to_owned(),
                payload_profile: ArtifactPayloadProfile::PrebuiltExact,
                section_count: 0,
                bootstrap: Vec::new(),
            },
        )));
        match zero {
            Payload::ViewerControl(ViewerControl::StreamStarted(start)) => {
                assert_eq!(start.section_count, 0);
            }
            _ => panic!("expected empty StreamStarted"),
        }

        let range = CommittedRange {
            offset: 8,
            length: 16,
            crc32: 23,
            sha256: [42; 32],
        };
        let commit = SectionCommit {
            build_id: "build-reordered".to_owned(),
            resource_generation: 3,
            generation: 5,
            index: 0,
            directory_index: 2,
            position: [9, 1, -4],
            record: range.clone(),
            exact: range.clone(),
            vulkan_packed: Some(range.clone()),
            packed_gpu_bytes: 0,
            panorama: None,
            camera_light: range,
            light_proxies: None,
        };
        let payload = framed_round_trip(Payload::ViewerControl(ViewerControl::SectionCommitted(
            commit.clone(),
        )));
        match payload {
            Payload::ViewerControl(ViewerControl::SectionCommitted(actual)) => {
                assert_eq!(actual, commit);
                assert_ne!(actual.index, actual.directory_index);
            }
            _ => panic!("expected SectionCommitted"),
        }
    }

    #[test]
    fn await_artifact_round_trip_preserves_the_frozen_build_mode() {
        for build_mode in [RenderBuildMode::Prebuilt, RenderBuildMode::Realtime] {
            let prebuilt_exact_gpu_budget_bytes = match build_mode {
                RenderBuildMode::Prebuilt => Some(9 * 1024 * 1024 * 1024),
                RenderBuildMode::Realtime => None,
            };
            let payload = framed_round_trip(Payload::ViewerControl(ViewerControl::AwaitArtifact {
                session_id: "viewer-mode".to_owned(),
                build_id: "build-mode".to_owned(),
                generation: 9,
                source_path: PathBuf::from(r"C:\scene\mode.litematic"),
                artifact_path: PathBuf::from(r"C:\runtime\mode.gpu"),
                build_mode,
                prebuilt_exact_gpu_budget_bytes,
                content: SceneContent::default(),
                presentation: PresentationSpec {
                    backend: RenderBackend::Vulkan,
                    shaderpack: None,
                    render_settings: BTreeMap::new(),
                },
            }));
            match payload {
                Payload::ViewerControl(ViewerControl::AwaitArtifact {
                    build_mode: actual,
                    prebuilt_exact_gpu_budget_bytes: actual_budget,
                    ..
                }) => {
                    assert_eq!(actual, build_mode);
                    assert_eq!(actual_budget, prebuilt_exact_gpu_budget_bytes);
                }
                _ => panic!("AwaitArtifact must retain the frozen build mode"),
            }
        }
    }

    #[test]
    fn await_artifact_rejects_missing_zero_or_cross_mode_prebuilt_budget() {
        let control = |build_mode, prebuilt_exact_gpu_budget_bytes| ViewerControl::AwaitArtifact {
            session_id: "viewer-budget".to_owned(),
            build_id: "build-budget".to_owned(),
            generation: 3,
            source_path: PathBuf::from(r"C:\scene\budget.litematic"),
            artifact_path: PathBuf::from(r"C:\runtime\budget.gpu"),
            build_mode,
            prebuilt_exact_gpu_budget_bytes,
            content: SceneContent::default(),
            presentation: PresentationSpec {
                backend: RenderBackend::Vulkan,
                shaderpack: None,
                render_settings: BTreeMap::new(),
            },
        };

        assert!(control(RenderBuildMode::Prebuilt, None).validate().is_err());
        assert!(
            control(RenderBuildMode::Prebuilt, Some(0))
                .validate()
                .is_err()
        );
        control(RenderBuildMode::Prebuilt, Some(1))
            .validate()
            .unwrap();
        control(RenderBuildMode::Realtime, None).validate().unwrap();
        assert!(
            control(RenderBuildMode::Realtime, Some(1))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn viewer_stream_capacity_round_trips_before_renderer_connection() {
        let payload = framed_round_trip(Payload::ViewerUpdate(ViewerUpdate::StreamCapacity {
            session_id: "viewer-9".to_owned(),
            max_inflight_sections: 12,
            max_inflight_bytes: 48 * 1024 * 1024,
        }));
        match payload {
            Payload::ViewerUpdate(ViewerUpdate::StreamCapacity {
                session_id,
                max_inflight_sections,
                max_inflight_bytes,
            }) => {
                assert_eq!(session_id, "viewer-9");
                assert_eq!(max_inflight_sections, 12);
                assert_eq!(max_inflight_bytes, 48 * 1024 * 1024);
            }
            _ => panic!("stream capacity must retain both negotiated limits"),
        }
    }

    #[test]
    fn viewer_stream_opened_round_trips_before_section_dispatch() {
        let payload = framed_round_trip(Payload::ViewerUpdate(ViewerUpdate::StreamOpened {
            session_id: "viewer-9".to_owned(),
            build_id: "build-3".to_owned(),
            generation: 11,
            resource_generation: 17,
        }));
        match payload {
            Payload::ViewerUpdate(ViewerUpdate::StreamOpened {
                session_id,
                build_id,
                generation,
                resource_generation,
            }) => {
                assert_eq!(session_id, "viewer-9");
                assert_eq!(build_id, "build-3");
                assert_eq!(generation, 11);
                assert_eq!(resource_generation, 17);
            }
            _ => panic!("stream-open acknowledgement must retain every generation identity"),
        }
    }

    #[test]
    fn artifact_publish_gate_round_trips_zero_section_state() {
        let payload = framed_round_trip(Payload::WorkerControl(
            WorkerControl::ArtifactPublishReady {
                build_id: "build-empty".to_owned(),
                generation: 4,
                section_count: 0,
                cache_only: false,
                reason: None,
            },
        ));
        match payload {
            Payload::WorkerControl(WorkerControl::ArtifactPublishReady {
                build_id,
                generation,
                section_count,
                cache_only,
                reason,
            }) => {
                assert_eq!(build_id, "build-empty");
                assert_eq!(generation, 4);
                assert_eq!(section_count, 0);
                assert!(!cache_only);
                assert!(reason.is_none());
            }
            _ => panic!("artifact publish gate must preserve an explicit zero-section ACK"),
        }
    }

    #[test]
    fn viewer_runtime_milestones_remain_distinct_on_the_wire() {
        let updates = [
            ViewerUpdate::GpuAccepted {
                session_id: "viewer-9".to_owned(),
                build_id: "build-9".to_owned(),
                generation: 9,
                sections_gpu_accepted: 45_276,
                sections_gpu_completed: 4_096,
                elapsed_ms: 10,
            },
            ViewerUpdate::GpuResident {
                session_id: "viewer-9".to_owned(),
                build_id: "build-9".to_owned(),
                generation: 9,
                sections_gpu_accepted: 45_276,
                sections_gpu_completed: 4_096,
                elapsed_ms: 20,
            },
            ViewerUpdate::ViewerPrepared {
                session_id: "viewer-9".to_owned(),
                build_id: "build-9".to_owned(),
                generation: 9,
                sections_gpu_accepted: 45_276,
                sections_gpu_completed: 4_096,
                elapsed_ms: 30,
            },
        ];

        for update in updates {
            let payload = Payload::ViewerUpdate(update);
            let expected = serde_json::to_value(&payload).unwrap();
            let actual = serde_json::to_value(framed_round_trip(payload)).unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn presentation_visible_rejects_hidden_state() {
        let mut bytes = Vec::new();
        let error = write_envelope(
            &mut bytes,
            &Envelope::new(
                11,
                Payload::ViewerUpdate(ViewerUpdate::PresentationVisible {
                    session_id: "viewer-9".to_owned(),
                    build_id: "build-9".to_owned(),
                    generation: 9,
                    state: ViewerPresentationState::Hidden,
                }),
            ),
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("cannot carry Hidden"));
    }
}
