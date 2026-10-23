use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const VIEWER_LAUNCH_SCHEMA: &str = "lba.viewer.launch.v1";
pub const VIEWER_LAUNCH_ACK_SCHEMA: &str = "lba.viewer.launch-ack.v1";
pub const VIEWER_LAUNCH_ACK_PREFIX: &str = "[LBA_VIEWER_LAUNCH_ACK] ";
pub const VIEWER_INTEGRITY_SCHEMA: &str = "lba.viewer.integrity.v1";
pub const VIEWER_INTEGRITY_PREFIX: &str = "[LBA_VIEWER_INTEGRITY] ";
pub const MAX_VIEWER_INTEGRITY_WARNINGS: usize = 16;
pub const MAX_VIEWER_INTEGRITY_WARNING_BYTES: usize = 512;
pub const VIEWER_READY_SCHEMA: &str = "lba.viewer.ready.v1";
pub const VIEWER_READY_PREFIX: &str = "[LBA_VIEWER_READY] ";
pub const VIEWER_FRAME_CAPTURE_RECEIPT_SCHEMA: &str = "lba.viewer.frame-capture-receipt.v1";
pub const VIEWER_SESSION_COMMAND_SCHEMA: &str = "litematica-nova.viewer-session-command.v2";
pub const VIEWER_SESSION_STATUS_SCHEMA: &str = "litematica-nova.viewer-session-status.v1";
pub const VIEWER_SESSION_EVENT_SCHEMA: &str = "litematica-nova.viewer-session-event.v1";

/// Display-only state for a resident scene. No build or resource identity changes.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct SceneViewControls {
    pub apply_layer_range: bool,
    #[serde(skip_serializing_if = "scene_control_false")]
    pub layer_view: bool,
    pub layer_min_y: Option<i32>,
    pub layer_max_y: Option<i32>,
    pub camera_mode: Option<String>,
    pub camera_input_mode: Option<String>,
    pub camera_speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_sensitivity: Option<f32>,
    #[serde(skip_serializing_if = "scene_control_false")]
    pub release_camera_input: bool,
    pub reset_camera: bool,
    pub reload_shader_options: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shader_options_result_path: Option<PathBuf>,
    pub shaderpack_root: Option<PathBuf>,
    pub shaderpack_name: Option<String>,
}
fn scene_control_false(value: &bool) -> bool { !*value }

impl SceneViewControls {
    pub fn validate(&self) -> Result<(), String> {
        if self.layer_min_y.zip(self.layer_max_y).is_some_and(|(a,b)| a > b) {
            return Err("Layer minimum exceeds maximum".into());
        }
        if self.shaderpack_root.is_some() != self.shaderpack_name.is_some() { return Err("Shader pack root and name must be paired".into()); }
        if self.camera_mode.as_deref().is_some_and(|mode| mode != "orbit" && mode != "free") {
            return Err("Unknown camera mode".into());
        }
        if self.camera_input_mode.as_deref().is_some_and(|mode| mode != "drag" && mode != "pointer_lock") {
            return Err("Unknown free camera input mode".into());
        }
        if self.shader_options_result_path.as_ref().is_some_and(|path| !self.reload_shader_options || !path.is_absolute()) {
            return Err("Shader options receipt requires a reload and an absolute result path".into());
        }
        if self.camera_sensitivity.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err("Camera sensitivity must be finite and positive".into());
        }
        if self.camera_speed.is_some_and(|speed| !speed.is_finite() || speed <= 0.0) {
            return Err("Camera speed must be finite and positive".into());
        }
        Ok(())
    }
}

/// A single atomic command published by the desktop application to an already
/// running Viewer process. Scene replacement still points at the normal,
/// hash-bound launch contract; presentation commands only mutate the existing
/// Viewer-owned HWND and never carry scene or renderer payloads.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerSessionCommand {
    pub schema: String,
    pub session_id: String,
    pub sequence: u64,
    pub kind: ViewerSessionCommandKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_contract: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_hwnd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_composition: Option<NativeCompositionEndpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi_scale: Option<f64>,
    /// Screen-space origin for an out-of-process overlay presentation. Older
    /// Nova builds may omit it and provide `hostHwnd`; the Viewer then samples
    /// that host's screen rectangle once while applying the async command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_y: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    /// Original user action clock for presentation-only end-to-end timing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_started_at_epoch_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_controls: Option<SceneViewControls>,
}

impl ViewerSessionCommand {
    /// Backwards-compatible constructor for the original persistent-session
    /// operation: replacing the prepared scene from another launch contract.
    pub fn new(
        session_id: String,
        sequence: u64,
        launch_contract: PathBuf,
    ) -> Result<Self, String> {
        Self::replace_scene(session_id, sequence, launch_contract)
    }

    pub fn replace_scene(
        session_id: String,
        sequence: u64,
        launch_contract: PathBuf,
    ) -> Result<Self, String> {
        let command = Self {
            schema: VIEWER_SESSION_COMMAND_SCHEMA.to_owned(),
            session_id,
            sequence,
            kind: ViewerSessionCommandKind::ReplaceScene,
            launch_contract: Some(launch_contract),
            host_hwnd: None,
            native_composition: None,
            presentation_epoch: None, dpi_scale: None,
            screen_x: None,
            screen_y: None,
            width: None,
            height: None,
            focused: None,
            click_started_at_epoch_ms: None,
            view_controls: None,
        };
        command.validate()?;
        Ok(command)
    }

    pub fn attach_embedded(
        session_id: String,
        sequence: u64,
        host_hwnd: String,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        Self::presentation_command(
            session_id,
            sequence,
            ViewerSessionCommandKind::AttachEmbedded,
            Some(host_hwnd),
            None,
            None,
            Some(width),
            Some(height),
            None,
        )
    }

    pub fn attach_native_composition(
        session_id: String, sequence: u64, endpoint: NativeCompositionEndpoint,
        width: u32, height: u32,
    ) -> Result<Self, String> {
        let mut command = Self::hide(session_id, sequence)?;
        command.kind = ViewerSessionCommandKind::AttachEmbedded;
        command.host_hwnd = Some("0".into());
        command.native_composition = Some(endpoint);
        command.width = Some(width);
        command.height = Some(height);
        command.validate()?;
        Ok(command)
    }

    /// Presents the Viewer as a frameless top-level overlay. The render HWND
    /// remains Viewer-owned and is never reparented into the Nova process.
    pub fn show_overlay(
        session_id: String,
        sequence: u64,
        screen_x: i32,
        screen_y: i32,
        width: u32,
        height: u32,
        focused: Option<bool>,
    ) -> Result<Self, String> {
        Self::presentation_command(
            session_id,
            sequence,
            ViewerSessionCommandKind::ShowOverlay,
            None,
            Some(screen_x),
            Some(screen_y),
            Some(width),
            Some(height),
            focused,
        )
    }

    pub fn show_popup(session_id: String, sequence: u64) -> Result<Self, String> {
        Self::presentation_command(
            session_id,
            sequence,
            ViewerSessionCommandKind::ShowPopup,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    pub fn hide(session_id: String, sequence: u64) -> Result<Self, String> {
        Self::presentation_command(
            session_id,
            sequence,
            ViewerSessionCommandKind::Hide,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    pub fn resize(
        session_id: String,
        sequence: u64,
        width: u32,
        height: u32,
        focused: Option<bool>,
    ) -> Result<Self, String> {
        Self::presentation_command(
            session_id,
            sequence,
            ViewerSessionCommandKind::Resize,
            None,
            None,
            None,
            Some(width),
            Some(height),
            focused,
        )
    }

    pub fn move_overlay(
        session_id: String,
        sequence: u64,
        screen_x: i32,
        screen_y: i32,
        width: u32,
        height: u32,
        focused: Option<bool>,
    ) -> Result<Self, String> {
        Self::presentation_command(
            session_id,
            sequence,
            ViewerSessionCommandKind::Resize,
            None,
            Some(screen_x),
            Some(screen_y),
            Some(width),
            Some(height),
            focused,
        )
    }

    pub fn release(session_id: String, sequence: u64) -> Result<Self, String> {
        Self::presentation_command(
            session_id,
            sequence,
            ViewerSessionCommandKind::Release,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    /// Capture the next actual fast-rendered frame into the session-owned PNG.
    /// No source, GPU lifetime or presentation parameters change.
    pub fn capture_frame(session_id: String, sequence: u64) -> Result<Self, String> {
        Self::presentation_command(session_id, sequence, ViewerSessionCommandKind::CaptureFrame,
            None, None, None, None, None, None)
    }

    pub fn set_view(session_id: String, sequence: u64, controls: SceneViewControls) -> Result<Self, String> {
        controls.validate()?;
        let mut command = Self::presentation_command(session_id, sequence,
            ViewerSessionCommandKind::CaptureFrame, None, None, None, None, None, None)?;
        command.kind = ViewerSessionCommandKind::SetView;
        command.view_controls = Some(controls);
        command.validate()?;
        Ok(command)
    }

    fn presentation_command(
        session_id: String,
        sequence: u64,
        kind: ViewerSessionCommandKind,
        host_hwnd: Option<String>,
        screen_x: Option<i32>,
        screen_y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
        focused: Option<bool>,
    ) -> Result<Self, String> {
        let command = Self {
            schema: VIEWER_SESSION_COMMAND_SCHEMA.to_owned(),
            session_id,
            sequence,
            kind,
            launch_contract: None,
            host_hwnd,
            native_composition: None,
            presentation_epoch: None, dpi_scale: None,
            screen_x,
            screen_y,
            width,
            height,
            focused,
            click_started_at_epoch_ms: None,
            view_controls: None,
        };
        command.validate()?;
        Ok(command)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.dpi_scale.is_some_and(|scale| !scale.is_finite() || scale <= 0.0 || scale > 16.0) {
            return Err("viewer presentation DPI scale must be finite and positive".into());
        }

        if self.presentation_epoch == Some(0) { return Err("native presentationEpoch must be non-zero".into()); }
        if self.presentation_epoch.is_some() && !matches!(self.kind, ViewerSessionCommandKind::AttachEmbedded | ViewerSessionCommandKind::Resize | ViewerSessionCommandKind::ShowPopup | ViewerSessionCommandKind::Hide | ViewerSessionCommandKind::Release) {
            return Err("presentationEpoch requires a native presentation command".into());
        }
        if self.native_composition.is_some() && self.kind != ViewerSessionCommandKind::AttachEmbedded {
            return Err("nativeComposition is only valid for attach_embedded".into());
        }
        if let Some(click) = self.click_started_at_epoch_ms {
            if click == 0 || !matches!(self.kind, ViewerSessionCommandKind::AttachEmbedded | ViewerSessionCommandKind::ShowOverlay | ViewerSessionCommandKind::ShowPopup) {
                return Err("clickStartedAtEpochMs requires a visible presentation action and non-zero epoch".into());
            }
        }
        if self.schema != VIEWER_SESSION_COMMAND_SCHEMA {
            return Err(format!(
                "unsupported Viewer session command schema {:?}; expected {:?}",
                self.schema, VIEWER_SESSION_COMMAND_SCHEMA
            ));
        }
        validate_session_id(&self.session_id)?;
        if self.sequence == 0 {
            return Err("Viewer session command sequence must be non-zero".to_owned());
        }
        if self.view_controls.is_some() && self.kind != ViewerSessionCommandKind::SetView {
            return Err("viewControls only belongs to set_view".into());
        }
        match self.kind {
            ViewerSessionCommandKind::SetView => {
                self.require_no_payload_fields()?;
                self.view_controls.as_ref().ok_or("set_view requires viewControls")?.validate()
            }
            ViewerSessionCommandKind::ReplaceScene => {
                let launch_contract = self
                    .launch_contract
                    .as_deref()
                    .ok_or_else(|| "replace_scene requires launchContract".to_owned())?;
                require_absolute("launchContract", launch_contract)?;
                self.require_no_presentation_fields()
            }
            ViewerSessionCommandKind::AttachEmbedded => {
                if self.launch_contract.is_some() || self.focused.is_some() {
                    return Err(
                        "attach_embedded only accepts hostHwnd, optional screen origin, width, and height"
                            .to_owned()
                    );
                }
                let host = self.host_hwnd.as_deref().ok_or_else(|| "attach_embedded requires hostHwnd".to_owned())?;
                if let Some(endpoint) = &self.native_composition {
                    endpoint.validate()?;
                    if host != "0" { return Err("native composition must not carry a process-local window pointer".into()); }
                } else { validate_hwnd("hostHwnd", host)?; }
                validate_optional_screen_origin(self.screen_x, self.screen_y, "attach_embedded")?;
                validate_extent(self.width, self.height, "attach_embedded")
            }
            ViewerSessionCommandKind::ShowOverlay => {
                if self.launch_contract.is_some() || self.host_hwnd.is_some() {
                    return Err(
                        "show_overlay only accepts screen origin, width, height, and focused"
                            .to_owned(),
                    );
                }
                validate_required_screen_origin(self.screen_x, self.screen_y, "show_overlay")?;
                validate_extent(self.width, self.height, "show_overlay")
            }
            ViewerSessionCommandKind::ShowPopup
            | ViewerSessionCommandKind::Hide
            | ViewerSessionCommandKind::CaptureFrame
            | ViewerSessionCommandKind::Release => self.require_no_payload_fields(),
            ViewerSessionCommandKind::Resize => {
                if self.launch_contract.is_some() || self.host_hwnd.is_some() {
                    return Err(
                        "resize only accepts optional screen origin, width, height, and focused"
                            .to_owned(),
                    );
                }
                validate_optional_screen_origin(self.screen_x, self.screen_y, "resize")?;
                validate_extent(self.width, self.height, "resize")
            }
        }
    }

    fn require_no_presentation_fields(&self) -> Result<(), String> {
        if self.host_hwnd.is_some()
            || self.screen_x.is_some()
            || self.screen_y.is_some()
            || self.width.is_some()
            || self.height.is_some()
            || self.focused.is_some()
        {
            return Err("replace_scene only accepts launchContract".to_owned());
        }
        Ok(())
    }

    fn require_no_payload_fields(&self) -> Result<(), String> {
        if self.launch_contract.is_some()
            || self.host_hwnd.is_some()
            || self.screen_x.is_some()
            || self.screen_y.is_some()
            || self.width.is_some()
            || self.height.is_some()
            || self.focused.is_some()
        {
            return Err(format!(
                "{} does not accept payload fields",
                self.kind.as_str()
            ));
        }
        Ok(())
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| format!("serialize Viewer session command: {error}"))
    }

    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, String> {
        let command = serde_json::from_slice::<Self>(bytes)
            .map_err(|error| format!("parse Viewer session command JSON: {error}"))?;
        command.validate()?;
        Ok(command)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerSessionCommandKind {
    ReplaceScene,
    AttachEmbedded,
    ShowOverlay,
    ShowPopup,
    Hide,
    Resize,
    Release,
    CaptureFrame,
    SetView,
}

impl ViewerSessionCommandKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReplaceScene => "replace_scene",
            Self::AttachEmbedded => "attach_embedded",
            Self::ShowOverlay => "show_overlay",
            Self::ShowPopup => "show_popup",
            Self::Hide => "hide",
            Self::Resize => "resize",
            Self::Release => "release",
            Self::CaptureFrame => "capture_frame",
            Self::SetView => "set_view",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerSessionStatus {
    pub schema: String,
    pub session_id: String,
    pub sequence: u64,
    pub pid: u32,
    pub hwnd: String,
    pub state: ViewerSessionState,
}

impl ViewerSessionStatus {
    pub fn new(
        session_id: String,
        sequence: u64,
        pid: u32,
        hwnd: String,
        state: ViewerSessionState,
    ) -> Result<Self, String> {
        let status = Self {
            schema: VIEWER_SESSION_STATUS_SCHEMA.to_owned(),
            session_id,
            sequence,
            pid,
            hwnd,
            state,
        };
        status.validate()?;
        Ok(status)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != VIEWER_SESSION_STATUS_SCHEMA {
            return Err(format!(
                "unsupported Viewer session status schema {:?}; expected {:?}",
                self.schema, VIEWER_SESSION_STATUS_SCHEMA
            ));
        }
        validate_session_id(&self.session_id)?;
        if self.pid == 0 {
            return Err("Viewer session status pid must be non-zero".to_owned());
        }
        validate_hwnd("hwnd", &self.hwnd)?;
        if matches!(self.state, ViewerSessionState::Ready) {
            if self.sequence != 0 {
                return Err("initial ready status sequence must be zero".to_owned());
            }
        } else if self.sequence == 0 {
            return Err("Viewer session command status sequence must be non-zero".to_owned());
        }
        self.state.validate()
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| format!("serialize Viewer session status: {error}"))
    }

    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, String> {
        let status = serde_json::from_slice::<Self>(bytes)
            .map_err(|error| format!("parse Viewer session status JSON: {error}"))?;
        status.validate()?;
        Ok(status)
    }
}

/// An unsolicited state transition produced by the Viewer itself (for
/// example, a user closing a persistent Popup). Command acknowledgements use
/// `ViewerSessionStatus`; events have their own monotonic namespace so they
/// never overwrite a command's immutable status file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerSessionEvent {
    pub schema: String,
    pub session_id: String,
    pub event_id: u64,
    pub pid: u32,
    pub hwnd: String,
    pub state: ViewerSessionState,
}

impl ViewerSessionEvent {
    pub fn new(
        session_id: String,
        event_id: u64,
        pid: u32,
        hwnd: String,
        state: ViewerSessionState,
    ) -> Result<Self, String> {
        let event = Self {
            schema: VIEWER_SESSION_EVENT_SCHEMA.to_owned(),
            session_id,
            event_id,
            pid,
            hwnd,
            state,
        };
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != VIEWER_SESSION_EVENT_SCHEMA {
            return Err(format!(
                "unsupported Viewer session event schema {:?}; expected {:?}",
                self.schema, VIEWER_SESSION_EVENT_SCHEMA
            ));
        }
        validate_session_id(&self.session_id)?;
        if self.event_id == 0 {
            return Err("Viewer session eventId must be non-zero".to_owned());
        }
        if self.pid == 0 {
            return Err("Viewer session event pid must be non-zero".to_owned());
        }
        validate_hwnd("hwnd", &self.hwnd)?;
        self.state.validate()
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| format!("serialize Viewer session event: {error}"))
    }

    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, String> {
        let event = serde_json::from_slice::<Self>(bytes)
            .map_err(|error| format!("parse Viewer session event JSON: {error}"))?;
        event.validate()?;
        Ok(event)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ViewerSessionState {
    Ready,
    Captured,
    SceneReady {
        request_id: String,
    },
    Attached {
        host_hwnd: String,
        width: u32,
        height: u32,
    },
    OverlayVisible {
        screen_x: i32,
        screen_y: i32,
        width: u32,
        height: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focused: Option<bool>,
    },
    PopupVisible,
    Hidden,
    Resized {
        width: u32,
        height: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focused: Option<bool>,
    },
    Released,
    Error {
        command_kind: ViewerSessionCommandKind,
        message: String,
    },
}

impl ViewerSessionState {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Ready | Self::Captured | Self::PopupVisible | Self::Hidden | Self::Released => Ok(()),
            Self::SceneReady { request_id } => validate_request_id(request_id),
            Self::Attached {
                host_hwnd,
                width,
                height,
            } => {
                validate_hwnd("state.hostHwnd", host_hwnd)?;
                validate_extent(Some(*width), Some(*height), "attached state")
            }
            Self::OverlayVisible { width, height, .. } => {
                validate_extent(Some(*width), Some(*height), "overlay visible state")
            }
            Self::Resized { width, height, .. } => {
                validate_extent(Some(*width), Some(*height), "resized state")
            }
            Self::Error { message, .. } if message.trim().is_empty() => {
                Err("Viewer session error status message must not be empty".to_owned())
            }
            Self::Error { .. } => Ok(()),
        }
    }
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.trim().is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("sessionId must be 1..=128 ASCII letters, digits, '-' or '_'".to_owned());
    }
    Ok(())
}

fn validate_request_id(request_id: &str) -> Result<(), String> {
    if request_id.trim().is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("requestId must be 1..=128 ASCII letters, digits, '-' or '_'".to_owned());
    }
    Ok(())
}

fn validate_hwnd(field: &str, hwnd: &str) -> Result<(), String> {
    let parsed = hwnd
        .parse::<isize>()
        .map_err(|_| format!("{field} must be a decimal integer"))?;
    if parsed == 0 {
        return Err(format!("{field} must not be zero"));
    }
    Ok(())
}

fn validate_optional_screen_origin(
    screen_x: Option<i32>,
    screen_y: Option<i32>,
    role: &str,
) -> Result<(), String> {
    if screen_x.is_some() != screen_y.is_some() {
        return Err(format!(
            "{role} screenX and screenY must either both be present or both be absent"
        ));
    }
    Ok(())
}

fn validate_required_screen_origin(
    screen_x: Option<i32>,
    screen_y: Option<i32>,
    role: &str,
) -> Result<(), String> {
    validate_optional_screen_origin(screen_x, screen_y, role)?;
    if screen_x.is_none() {
        return Err(format!("{role} requires screenX and screenY"));
    }
    Ok(())
}

fn validate_extent(width: Option<u32>, height: Option<u32>, operation: &str) -> Result<(), String> {
    let width = width.ok_or_else(|| format!("{operation} requires width"))?;
    let height = height.ok_or_else(|| format!("{operation} requires height"))?;
    if width == 0 || height == 0 {
        return Err(format!("{operation} width and height must be non-zero"));
    }
    Ok(())
}

/// Current-user runtime references. These are not part of scene/build identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProtectedTask {
    pub ticket_path: PathBuf,
    pub module_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerLaunchContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protected_task: Option<ProtectedTask>,
    pub schema: String,
    pub request_id: String,
    pub scene: PathBuf,
    pub content: ViewerContent,
    pub renderer: Option<ViewerRenderer>,
    pub task: ViewerTask,
}

impl ViewerLaunchContract {
    pub fn new(
        request_id: String,
        scene: PathBuf,
        content: ViewerContent,
        renderer: Option<ViewerRenderer>,
        task: ViewerTask,
    ) -> Result<Self, String> {
        let contract = Self {
            protected_task: None,
            schema: VIEWER_LAUNCH_SCHEMA.to_owned(),
            request_id,
            scene,
            content,
            renderer,
            task,
        };
        contract.validate()?;
        Ok(contract)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != VIEWER_LAUNCH_SCHEMA {
            return Err(format!(
                "unsupported launch contract schema {:?}; expected {:?}",
                self.schema, VIEWER_LAUNCH_SCHEMA
            ));
        }
        if self.request_id.trim().is_empty()
            || self.request_id.len() > 128
            || !self
                .request_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("requestId must be 1..=128 ASCII letters, digits, '-' or '_'".to_owned());
        }
        require_absolute("scene", &self.scene)?;
        self.content.validate()?;
        match &self.task {
            ViewerTask::BuildCache(task) => {
                if self.renderer.is_some() {
                    return Err(
                        "build_cache launch must not contain renderer-only settings".to_owned()
                    );
                }
                task.validate()
            }
            ViewerTask::BuildCacheAndWarmViewer(task) => {
                let renderer = self.renderer.as_ref().ok_or_else(|| {
                    "build_cache_and_warm_viewer launch requires renderer settings".to_owned()
                })?;
                renderer.validate()?;
                task.validate()
            }
            ViewerTask::PrepareViewer(task) => {
                let renderer = self
                    .renderer
                    .as_ref()
                    .ok_or_else(|| "prepare_viewer launch requires renderer settings".to_owned())?;
                renderer.validate()?;
                task.validate()
            }
            ViewerTask::Interactive(task) => {
                let renderer = self
                    .renderer
                    .as_ref()
                    .ok_or_else(|| "interactive launch requires renderer settings".to_owned())?;
                renderer.validate()?;
                if task.capture.is_some()
                    && !(renderer.shaderpack.is_some()
                        && matches!(renderer.backend, ViewerBackend::IrisGl46 | ViewerBackend::Vulkan))
                {
                    return Err(
                        "interactive frame capture requires an explicit Iris OpenGL or Vulkan shaderpack backend".to_owned()
                    );
                }
                task.validate()
            }
            ViewerTask::Preview(task) => {
                self.renderer
                    .as_ref()
                    .ok_or_else(|| "preview launch requires renderer settings".to_owned())?
                    .validate()?;
                task.validate()
            }
            ViewerTask::QuickPreview(task) => {
                if self.renderer.is_some() || self.content != ViewerContent::default() {
                    return Err("quick_preview launch must not contain renderer settings or content resources".to_owned());
                }
                if task.initial_visible {
                    return Err("quick_preview task.initialVisible must be false".to_owned());
                }
                match (&task.resource_report_path, &task.resource_decision_path) {
                    (Some(report), Some(decision)) => {
                        require_absolute("task.resourceReportPath", report)?;
                        require_absolute("task.resourceDecisionPath", decision)?;
                        if report == decision || report == &self.scene || decision == &self.scene {
                            return Err("quick resource report/decision/source paths must differ".to_owned());
                        }
                    }
                    (None, None) => {}
                    _ => return Err("quick resource report/decision paths must be paired".to_owned()),
                }
                if let Some(host) = &task.host {
                    host.validate()?;
                    if host.process_id == 0 {
                        return Err("quick_preview host processId must be non-zero".to_owned());
                    }
                }
                Ok(())
            }
            ViewerTask::QuickGpuContext(task) => {
                if self.renderer.is_some() || self.content != ViewerContent::default() {
                    return Err("quick_gpu_context cannot contain renderer/content or projection preparation".into());
                }
                task.host.validate()?;
                if task.host.process_id == 0 { return Err("quick_gpu_context host processId must be non-zero".into()); }
                Ok(())
            }
        }
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| format!("serialize launch contract: {error}"))
    }

    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, String> {
        let contract = serde_json::from_slice::<Self>(bytes)
            .map_err(|error| format!("parse launch contract JSON: {error}"))?;
        contract.validate()?;
        Ok(contract)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerContent {
    pub fabric_mods_dir: Option<PathBuf>,
    pub resource_packs: Vec<PathBuf>,
}

impl ViewerContent {
    fn validate(&self) -> Result<(), String> {
        if let Some(path) = self.fabric_mods_dir.as_deref() {
            require_absolute("content.fabricModsDir", path)?;
        }
        for (index, path) in self.resource_packs.iter().enumerate() {
            require_absolute(&format!("content.resourcePacks[{index}]"), path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerRenderer {
    pub backend: ViewerBackend,
    pub iris_fallback: IrisFallback,
    pub shaderpack: Option<ViewerShaderpack>,
    pub vulkan_adapter: Option<VulkanAdapterSelection>,
    pub precipitation: WeatherPrecipitation,
}

impl ViewerRenderer {
    fn validate(&self) -> Result<(), String> {
        if let Some(shaderpack) = self.shaderpack.as_ref() {
            shaderpack.validate()?;
        }
        match (self.backend, self.shaderpack.is_some()) {
            (ViewerBackend::IrisGl46, false) => {
                Err("iris-gl46 backend requires a shaderpack".to_owned())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ViewerBackend {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "vulkan")]
    Vulkan,
    #[serde(rename = "iris-gl46")]
    IrisGl46,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IrisFallback {
    Fail,
    Vulkan,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerShaderpack {
    pub path: PathBuf,
}

impl ViewerShaderpack {
    fn validate(&self) -> Result<(), String> {
        require_absolute("renderer.shaderpack.path", &self.path)?;
        let name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if name.is_empty() || name == "." || name == ".." {
            return Err("renderer.shaderpack.path must select a shaderpack".to_owned());
        }
        self.path
            .parent()
            .ok_or_else(|| "renderer.shaderpack.path must have a parent directory".to_owned())
            .map(|_| ())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VulkanAdapterSelection {
    pub uuid: Option<String>,
    pub luid: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherPrecipitation {
    Auto,
    Rain,
    Snow,
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewerTask {
    BuildCache(BuildCacheTask),
    BuildCacheAndWarmViewer(BuildCacheAndWarmViewerTask),
    PrepareViewer(PrepareViewerTask),
    Interactive(InteractiveTask),
    Preview(PreviewTask),
    QuickPreview(QuickPreviewTask),
    QuickGpuContext(QuickGpuContextTask),
}

/// OS graphics API for a new GPU task, independent from the V5/Iris route.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsApiPreference {
    #[default]
    Auto,
    Dx12,
    Vulkan,
}

/// Ordinary asynchronous static GPU initialization. The legacy top-level
/// `scene` identifies the runtime context directory, never projection geometry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct QuickGpuContextTask {
    pub host: QuickPreviewHostWindow,
    #[serde(default)]
    pub graphics_backend: GraphicsApiPreference,
}

/// The historical independent quick renderer, never a Full/V5 build task.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct QuickPreviewTask {
    pub initial_visible: bool,
    #[serde(default)]
    pub graphics_backend: GraphicsApiPreference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<QuickPreviewHostWindow>,
    #[serde(default)]
    pub preload: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_report_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_decision_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickResourceAction { Preload, Realtime, ReducedRealtime, Cancel }

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct QuickResourceDecision {
    pub schema: String,
    pub request_id: String,
    pub action: QuickResourceAction,
    #[serde(default)]
    pub warning_confirmed: bool,
}

impl QuickResourceDecision {
    pub fn validate(&self, expected_request: &str) -> Result<(), String> {
        if self.schema != "nova.quick.decision.v1" || self.request_id != expected_request {
            return Err("quick resource decision belongs to another launch/schema".to_owned());
        }
        Ok(())
    }
}

/// Native child creation target, supplied by the desktop host, never by page JavaScript.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct QuickPreviewHostWindow {
    pub hwnd: String,
    pub process_id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_composition: Option<NativeCompositionEndpoint>,
}

impl QuickPreviewHostWindow {
    pub fn validate(&self) -> Result<(), String> {
        if self.process_id == 0 { return Err("native Host processId must be non-zero".into()); }
        if let Some(endpoint) = &self.native_composition {
            endpoint.validate()?;
            if self.hwnd != "0" { return Err("native composition must not carry a process-local window pointer".into()); }
            Ok(())
        } else { validate_hwnd("task.host.hwnd", &self.hwnd) }
    }
}

/// A capability-bound OS composition service, never an NSView/wl_surface pointer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NativeCompositionEndpoint {
    pub kind: String,
    pub service_name: String,
    pub nonce_hex: String,
    pub generation: u64,
}

impl NativeCompositionEndpoint {
    pub fn validate(&self) -> Result<(), String> {
        if self.kind != "macos_iosurface_mach_v1"
            || !self.service_name.starts_with("com.litematica.nova.viewport.")
            || self.service_name.len() > 127
            || !self.service_name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c,b'.'|b'-'|b'_'))
            || self.nonce_hex.len() != 32
            || !self.nonce_hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || self.generation == 0 {
            return Err("native composition endpoint identity is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod native_composition_tests {
    use super::*;
    #[test]
    fn native_presentation_dpi_and_epoch_round_trip_without_changing_legacy_packets() {
        let mut command = ViewerSessionCommand::resize("session-native".into(), 1, 1280, 720, Some(true)).unwrap();
        let old_json = serde_json::to_value(&command).unwrap();
        assert!(old_json.get("dpiScale").is_none());
        assert!(old_json.get("presentationEpoch").is_none());
        command.dpi_scale = Some(1.5);
        command.presentation_epoch = Some(8);
        assert_eq!(ViewerSessionCommand::from_json_slice(&command.to_json_bytes().unwrap()).unwrap(), command);
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, 16.01] {
            command.dpi_scale = Some(scale);
            assert!(command.validate().is_err());
        }
        command.dpi_scale = Some(2.0);
        command.presentation_epoch = Some(0);
        assert!(command.validate().is_err());
    }
    #[test]
    fn explicit_native_popup_keeps_the_host_epoch_without_carrying_a_view_pointer() {
        let mut command = ViewerSessionCommand::show_popup("session-native".into(), 2).unwrap();
        command.presentation_epoch = Some(9);
        let json = command.to_json_bytes().unwrap();
        assert_eq!(ViewerSessionCommand::from_json_slice(&json).unwrap(), command);
        assert!(command.host_hwnd.is_none());
        assert!(command.native_composition.is_none());
    }
    fn endpoint() -> NativeCompositionEndpoint {
        NativeCompositionEndpoint { kind: "macos_iosurface_mach_v1".into(),
            service_name: "com.litematica.nova.viewport.test".into(),
            nonce_hex: "0123456789abcdef0123456789abcdef".into(), generation: 1 }
    }
    #[test]
    fn composed_host_never_accepts_a_process_local_view_pointer() {
        let mut host = QuickPreviewHostWindow { hwnd: "0".into(), process_id: 7, native_composition: Some(endpoint()) };
        assert!(host.validate().is_ok());
        host.hwnd = "1234".into();
        assert!(host.validate().is_err());
        host.hwnd = "0".into(); host.native_composition = None;
        assert!(host.validate().is_err());
    }
    #[test]
    fn composition_service_rejects_unbound_or_malformed_identity() {
        let valid = endpoint();
        assert!(valid.validate().is_ok());
        for invalid in [
            NativeCompositionEndpoint { generation: 0, ..valid.clone() },
            NativeCompositionEndpoint { nonce_hex: "short".into(), ..valid.clone() },
            NativeCompositionEndpoint { kind: "foreign_pointer".into(), ..valid.clone() },
            NativeCompositionEndpoint { service_name: "/tmp/foreign".into(), ..valid.clone() },
        ] { assert!(invalid.validate().is_err()); }
    }
    #[test]
    fn native_attach_is_explicit_and_old_host_packets_do_not_change() {
        let legacy = QuickPreviewHostWindow { hwnd: "42".into(), process_id: 7, native_composition: None };
        let json = serde_json::to_value(&legacy).unwrap();
        assert!(json.get("nativeComposition").is_none());
        assert_eq!(serde_json::from_value::<QuickPreviewHostWindow>(json).unwrap(), legacy);
        let mut command = ViewerSessionCommand::attach_native_composition("session-test".into(), 1, endpoint(), 640, 480).unwrap();
        assert_eq!(command.host_hwnd.as_deref(), Some("0"));
        assert!(command.validate().is_ok());
        command.kind = ViewerSessionCommandKind::Hide;
        assert!(command.validate().is_err());
    }
}

/// Product-selected geometry lifetime for one immutable render build.
///
/// This is deliberately separate from the Viewer's legacy quality/build
/// switches: `Prebuilt` and `Realtime` define when exact geometry must become
/// resident, not how the Viewer implementation generates a mesh.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBuildMode {
    Prebuilt,
    Realtime,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InitialLayerView { pub range: Option<[i32; 2]> }

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildCacheTask {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_layer_view: Option<InitialLayerView>,
    pub ready_file: PathBuf,
    pub cache_file: PathBuf,
    pub build_mode: RenderBuildMode,
    #[serde(default, skip_serializing_if = "ViewerEffectsProfile::is_full")]
    pub effects_profile: ViewerEffectsProfile,
}

/// Presentation effects do not select a different geometry renderer. Unlit
/// retains Full's exact model/fluid meshes, atlas and material identities.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerEffectsProfile {
    #[default]
    Full,
    Unlit,
}

impl ViewerEffectsProfile {
    pub const fn is_full(&self) -> bool { matches!(self, Self::Full) }
}

impl BuildCacheTask {
    fn validate(&self) -> Result<(), String> {
        if let Some(view) = &self.initial_layer_view {
            if self.build_mode != RenderBuildMode::Realtime || self.effects_profile != ViewerEffectsProfile::Unlit {
                return Err("3D layers require Standard Realtime".into());
            }
            if view.range.is_some_and(|[low, high]| low > high) { return Err("Layer minimum exceeds maximum".into()); }
        }
        require_absolute("task.readyFile", &self.ready_file)?;
        require_absolute("task.cacheFile", &self.cache_file)?;
        if self.ready_file == self.cache_file {
            return Err("task.readyFile and task.cacheFile must differ".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildCacheAndWarmViewerTask {
    pub ready_file: PathBuf,
    pub cache_file: PathBuf,
    #[serde(default)]
    pub cache_input: Option<PathBuf>,
    pub presentation: ViewerPresentation,
    pub purpose: ViewerInteractivePurpose,
    pub controls: ViewerControls,
    pub capture: Option<ViewerFrameCapture>,
    pub initial_visible: bool,
}

impl BuildCacheAndWarmViewerTask {
    fn validate(&self) -> Result<(), String> {
        require_absolute("task.readyFile", &self.ready_file)?;
        require_absolute("task.cacheFile", &self.cache_file)?;
        if let Some(path) = self.cache_input.as_deref() {
            require_absolute("task.cacheInput", path)?;
        }
        if self.ready_file == self.cache_file {
            return Err("task.readyFile and task.cacheFile must differ".to_owned());
        }
        if self.presentation != ViewerPresentation::Popup {
            return Err("build_cache_and_warm_viewer task.presentation must be popup".to_owned());
        }
        if self.purpose != ViewerInteractivePurpose::Render {
            return Err("build_cache_and_warm_viewer task.purpose must be render".to_owned());
        }
        if self.capture.is_some() {
            return Err("build_cache_and_warm_viewer task.capture must be null".to_owned());
        }
        if self.initial_visible {
            return Err("build_cache_and_warm_viewer task.initialVisible must be false".to_owned());
        }
        Ok(())
    }
}

/// Prepares a persistent renderer process from an already completed cache.
///
/// Unlike `BuildCacheAndWarmViewer`, this task never owns cache construction:
/// `cache_input` is mandatory and any missing, stale, or invalid artifact is a
/// fatal launch error that the application-level build job must resolve.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PrepareViewerTask {
    pub ready_file: PathBuf,
    pub cache_input: PathBuf,
    pub presentation: ViewerPresentation,
    pub purpose: ViewerInteractivePurpose,
    pub controls: ViewerControls,
    pub capture: Option<ViewerFrameCapture>,
    pub initial_visible: bool,
}

impl PrepareViewerTask {
    fn validate(&self) -> Result<(), String> {
        require_absolute("task.readyFile", &self.ready_file)?;
        require_absolute("task.cacheInput", &self.cache_input)?;
        if self.ready_file == self.cache_input {
            return Err("task.readyFile and task.cacheInput must differ".to_owned());
        }
        if self.presentation != ViewerPresentation::Popup {
            return Err("prepare_viewer task.presentation must be popup".to_owned());
        }
        if self.purpose != ViewerInteractivePurpose::Render {
            return Err("prepare_viewer task.purpose must be render".to_owned());
        }
        if self.capture.is_some() {
            return Err("prepare_viewer task.capture must be null".to_owned());
        }
        if self.initial_visible {
            return Err("prepare_viewer task.initialVisible must be false".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InteractiveTask {
    pub presentation: ViewerPresentation,
    pub purpose: ViewerInteractivePurpose,
    pub cache_input: Option<PathBuf>,
    pub controls: ViewerControls,
    pub capture: Option<ViewerFrameCapture>,
}

impl InteractiveTask {
    fn validate(&self) -> Result<(), String> {
        self.presentation.validate()?;
        if let Some(path) = self.cache_input.as_deref() {
            require_absolute("task.cacheInput", path)?;
        }
        if let Some(capture) = self.capture.as_ref() {
            capture.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerInteractivePurpose {
    Render,
    PropertiesPreview,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewerPresentation {
    Popup,
    Embedded { parent_hwnd: String },
}

impl ViewerPresentation {
    fn validate(&self) -> Result<(), String> {
        if let Self::Embedded { parent_hwnd } = self {
            let parsed = parent_hwnd
                .parse::<isize>()
                .map_err(|_| "task.presentation.parentHwnd must be a decimal integer".to_owned())?;
            if parsed == 0 {
                return Err("task.presentation.parentHwnd must not be zero".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerControls {
    pub allow_time_modification: bool,
    pub allow_weather_modification: bool,
}

impl ViewerControls {
    pub const fn unrestricted() -> Self {
        Self {
            allow_time_modification: true,
            allow_weather_modification: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerFrameCapture {
    pub output: PathBuf,
    pub receipt: PathBuf,
    pub frame: u64,
}

impl ViewerFrameCapture {
    pub fn new(output: PathBuf, frame: u64) -> Result<Self, String> {
        let receipt = Self::receipt_path_for(&output)?;
        let capture = Self {
            output,
            receipt,
            frame,
        };
        capture.validate()?;
        Ok(capture)
    }

    pub fn receipt_path_for(output: &Path) -> Result<PathBuf, String> {
        require_absolute("task.capture.output", output)?;
        require_png_path("task.capture.output", output)?;
        let file_name = output
            .file_name()
            .ok_or_else(|| "task.capture.output must have a file name".to_owned())?;
        let mut receipt_name = file_name.to_os_string();
        receipt_name.push(".receipt.json");
        Ok(output.with_file_name(receipt_name))
    }

    pub fn validate(&self) -> Result<(), String> {
        require_absolute("task.capture.output", &self.output)?;
        require_png_path("task.capture.output", &self.output)?;
        require_absolute("task.capture.receipt", &self.receipt)?;
        let expected_receipt = Self::receipt_path_for(&self.output)?;
        if self.receipt != expected_receipt {
            return Err(format!(
                "task.capture.receipt must be derived from task.capture.output: expected {}, found {}",
                expected_receipt.display(),
                self.receipt.display()
            ));
        }
        if self.frame == 0 {
            return Err("task.capture.frame is one-based".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerFrameCaptureReceipt {
    pub schema: String,
    pub frame: u64,
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub sha256: String,
}

impl ViewerFrameCaptureReceipt {
    pub fn new(
        capture: &ViewerFrameCapture,
        width: u32,
        height: u32,
        bytes: u64,
        sha256: String,
    ) -> Result<Self, String> {
        capture.validate()?;
        let receipt = Self {
            schema: VIEWER_FRAME_CAPTURE_RECEIPT_SCHEMA.to_owned(),
            frame: capture.frame,
            output: capture.output.clone(),
            width,
            height,
            bytes,
            sha256,
        };
        receipt.validate_for_capture(capture)?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != VIEWER_FRAME_CAPTURE_RECEIPT_SCHEMA {
            return Err(format!(
                "unsupported frame capture receipt schema {:?}; expected {:?}",
                self.schema, VIEWER_FRAME_CAPTURE_RECEIPT_SCHEMA
            ));
        }
        if self.frame == 0 {
            return Err("frame capture receipt frame is one-based".to_owned());
        }
        require_absolute("frame capture receipt output", &self.output)?;
        require_png_path("frame capture receipt output", &self.output)?;
        if self.width == 0 || self.height == 0 {
            return Err("frame capture receipt dimensions must be non-zero".to_owned());
        }
        if self.bytes == 0 {
            return Err("frame capture receipt byte length must be non-zero".to_owned());
        }
        if !is_lower_hex(&self.sha256, &[64]) {
            return Err("frame capture receipt SHA-256 is invalid".to_owned());
        }
        Ok(())
    }

    pub fn validate_for_capture(&self, capture: &ViewerFrameCapture) -> Result<(), String> {
        self.validate()?;
        capture.validate()?;
        if self.frame != capture.frame {
            return Err(format!(
                "frame capture receipt frame mismatch: expected {}, found {}",
                capture.frame, self.frame
            ));
        }
        if self.output != capture.output {
            return Err(format!(
                "frame capture receipt output mismatch: expected {}, found {}",
                capture.output.display(),
                self.output.display()
            ));
        }
        Ok(())
    }

    pub fn validate_artifact(
        &self,
        width: u32,
        height: u32,
        bytes: u64,
        sha256: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if width == 0 || height == 0 {
            return Err("observed frame capture dimensions must be non-zero".to_owned());
        }
        if bytes == 0 {
            return Err("observed frame capture byte length must be non-zero".to_owned());
        }
        if !is_lower_hex(sha256, &[64]) {
            return Err("observed frame capture SHA-256 is invalid".to_owned());
        }
        if self.bytes != bytes {
            return Err(format!(
                "frame capture PNG byte length mismatch: expected {}, found {bytes}",
                self.bytes,
            ));
        }
        if self.sha256 != sha256 {
            return Err(format!(
                "frame capture PNG SHA-256 mismatch: expected {}, found {sha256}",
                self.sha256,
            ));
        }
        if self.width != width || self.height != height {
            return Err(format!(
                "frame capture PNG dimensions mismatch: expected {}x{}, found {width}x{height}",
                self.width, self.height,
            ));
        }
        Ok(())
    }

    pub fn validate_completion(
        &self,
        capture: &ViewerFrameCapture,
        width: u32,
        height: u32,
        bytes: u64,
        sha256: &str,
    ) -> Result<(), String> {
        self.validate_for_capture(capture)?;
        self.validate_artifact(width, height, bytes, sha256)
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| format!("serialize frame capture receipt: {error}"))
    }

    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, String> {
        let receipt = serde_json::from_slice::<Self>(bytes)
            .map_err(|error| format!("parse frame capture receipt JSON: {error}"))?;
        receipt.validate()?;
        Ok(receipt)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PreviewTask {
    pub output: PathBuf,
    pub cache_input: Option<PathBuf>,
}

impl PreviewTask {
    fn validate(&self) -> Result<(), String> {
        require_absolute("task.output", &self.output)?;
        if let Some(path) = self.cache_input.as_deref() {
            require_absolute("task.cacheInput", path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerLaunchAck {
    pub schema: String,
    pub contract_schema: String,
    pub request_id: String,
    pub contract_sha256: String,
    pub backend_git_sha: String,
    pub backend_profile: String,
    pub backend_exe_path: PathBuf,
    pub backend_sha256: String,
}

impl ViewerLaunchAck {
    pub fn validate_for_contract(
        &self,
        expected_request_id: &str,
        expected_sha256: &str,
    ) -> Result<(), String> {
        if self.schema != VIEWER_LAUNCH_ACK_SCHEMA {
            return Err(format!("unsupported launch ACK schema {:?}", self.schema));
        }
        if self.contract_schema != VIEWER_LAUNCH_SCHEMA {
            return Err(format!(
                "launch ACK contract schema mismatch: {:?}",
                self.contract_schema
            ));
        }
        if self.request_id != expected_request_id {
            return Err(format!(
                "launch ACK request ID mismatch: expected {expected_request_id:?}, found {:?}",
                self.request_id
            ));
        }
        if self.contract_sha256 != expected_sha256 {
            return Err(format!(
                "launch ACK contract SHA-256 mismatch: expected {expected_sha256}, found {}",
                self.contract_sha256
            ));
        }
        if !is_lower_hex(&self.backend_git_sha, &[40, 64]) {
            return Err("launch ACK backend git SHA is invalid".to_owned());
        }
        if !matches!(self.backend_profile.as_str(), "debug" | "release") {
            return Err("launch ACK backend profile is invalid".to_owned());
        }
        if !self.backend_exe_path.is_absolute() {
            return Err("launch ACK backend executable path must be absolute".to_owned());
        }
        if !is_lower_hex(&self.backend_sha256, &[64]) {
            return Err("launch ACK backend SHA-256 is invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerIntegrity {
    pub schema: String,
    pub request_id: String,
    pub contract_sha256: String,
    pub error_count: usize,
    pub unsupported_count: usize,
    pub missing_count: usize,
    pub skipped_count: usize,
    pub warning_count: usize,
    pub warnings: Vec<String>,
}

impl ViewerIntegrity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: String,
        contract_sha256: String,
        error_count: usize,
        unsupported_count: usize,
        missing_count: usize,
        skipped_count: usize,
        warnings: Vec<String>,
    ) -> Result<Self, String> {
        let marker = Self {
            schema: VIEWER_INTEGRITY_SCHEMA.to_owned(),
            request_id,
            contract_sha256,
            error_count,
            unsupported_count,
            missing_count,
            skipped_count,
            warning_count: warnings.len(),
            warnings,
        };
        marker.validate()?;
        Ok(marker)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != VIEWER_INTEGRITY_SCHEMA {
            return Err(format!(
                "unsupported Viewer integrity schema {:?}",
                self.schema
            ));
        }
        if self.request_id.trim().is_empty()
            || self.request_id.len() > 128
            || !self
                .request_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(
                "Viewer integrity requestId must be 1..=128 ASCII letters, digits, '-' or '_'"
                    .to_owned(),
            );
        }
        if !is_lower_hex(&self.contract_sha256, &[64]) {
            return Err("Viewer integrity contract SHA-256 is invalid".to_owned());
        }
        if self.error_count != 0
            || self.unsupported_count != 0
            || self.missing_count != 0
            || self.skipped_count != 0
        {
            return Err(format!(
                "Viewer integrity fatal counts must all be zero: error={} unsupported={} missing={} skipped={}",
                self.error_count, self.unsupported_count, self.missing_count, self.skipped_count
            ));
        }
        if self.warning_count != self.warnings.len() {
            return Err(format!(
                "Viewer integrity warningCount {} does not match {} warnings",
                self.warning_count,
                self.warnings.len()
            ));
        }
        if self.warnings.len() > MAX_VIEWER_INTEGRITY_WARNINGS {
            return Err(format!(
                "Viewer integrity warnings exceed the {} entry limit",
                MAX_VIEWER_INTEGRITY_WARNINGS
            ));
        }
        for (index, warning) in self.warnings.iter().enumerate() {
            if warning.trim().is_empty()
                || warning.len() > MAX_VIEWER_INTEGRITY_WARNING_BYTES
                || warning.contains(['\r', '\n'])
            {
                return Err(format!(
                    "Viewer integrity warning {index} must be non-empty, single-line, and at most {} UTF-8 bytes",
                    MAX_VIEWER_INTEGRITY_WARNING_BYTES
                ));
            }
        }
        Ok(())
    }

    pub fn validate_for_contract(
        &self,
        expected_request_id: &str,
        expected_contract_sha256: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.request_id != expected_request_id {
            return Err(format!(
                "Viewer integrity request ID mismatch: expected {expected_request_id:?}, found {:?}",
                self.request_id
            ));
        }
        if self.contract_sha256 != expected_contract_sha256 {
            return Err(format!(
                "Viewer integrity contract SHA-256 mismatch: expected {expected_contract_sha256}, found {}",
                self.contract_sha256
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewerReady {
    pub schema: String,
    pub request_id: String,
    pub contract_sha256: String,
    pub pid: u32,
    pub hwnd: String,
    pub visible: bool,
    pub runtime_ready: bool,
    pub first_frame_presented: bool,
    pub cache_file_bytes: u64,
}

impl ViewerReady {
    pub fn validate_hidden_warm_viewer(
        &self,
        expected_request_id: &str,
        expected_contract_sha256: &str,
        expected_pid: u32,
    ) -> Result<isize, String> {
        if self.schema != VIEWER_READY_SCHEMA {
            return Err(format!("unsupported Viewer ready schema {:?}", self.schema));
        }
        if self.request_id != expected_request_id {
            return Err(format!(
                "Viewer ready request ID mismatch: expected {expected_request_id:?}, found {:?}",
                self.request_id
            ));
        }
        if self.contract_sha256 != expected_contract_sha256
            || !is_lower_hex(&self.contract_sha256, &[64])
        {
            return Err("Viewer ready contract SHA-256 mismatch or invalid encoding".to_owned());
        }
        if self.pid != expected_pid {
            return Err(format!(
                "Viewer ready PID mismatch: expected {expected_pid}, found {}",
                self.pid
            ));
        }
        let hwnd = self
            .hwnd
            .parse::<isize>()
            .map_err(|_| format!("Viewer ready HWND is invalid: {:?}", self.hwnd))?;
        if hwnd == 0 {
            return Err("Viewer ready HWND must be non-zero".to_owned());
        }
        if self.visible {
            return Err("hidden warm Viewer reported itself visible".to_owned());
        }
        if !self.runtime_ready || !self.first_frame_presented {
            return Err("Viewer ready marker precedes runtime readiness or first frame".to_owned());
        }
        if self.cache_file_bytes == 0 {
            return Err("Viewer ready marker reports an empty cache".to_owned());
        }
        Ok(hwnd)
    }
}

fn require_absolute(label: &str, path: &Path) -> Result<(), String> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(format!(
            "{label} must be an absolute path: {}",
            path.display()
        ))
    }
}

fn require_png_path(label: &str, path: &Path) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("png") {
        Ok(())
    } else {
        Err(format!("{label} must end in .png: {}", path.display()))
    }
}

fn is_lower_hex(value: &str, lengths: &[usize]) -> bool {
    lengths.contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute(name: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!(r"C:\lba-contract-tests\{name}"))
        } else {
            PathBuf::from(format!("/tmp/lba-contract-tests/{name}"))
        }
    }

    fn interactive_contract() -> ViewerLaunchContract {
        ViewerLaunchContract::new(
            "contract-test".to_owned(),
            absolute("scene.litematic"),
            ViewerContent {
                fabric_mods_dir: None,
                resource_packs: vec![absolute("pack.zip")],
            },
            Some(ViewerRenderer {
                backend: ViewerBackend::IrisGl46,
                iris_fallback: IrisFallback::Fail,
                shaderpack: Some(ViewerShaderpack {
                    path: absolute("shaderpacks/pack.zip"),
                }),
                vulkan_adapter: None,
                precipitation: WeatherPrecipitation::Auto,
            }),
            ViewerTask::Interactive(InteractiveTask {
                presentation: ViewerPresentation::Popup,
                purpose: ViewerInteractivePurpose::Render,
                cache_input: Some(absolute("scene.cache")),
                controls: ViewerControls::unrestricted(),
                capture: None,
            }),
        )
        .unwrap()
    }

    #[test]
    fn quick_native_host_is_strict_and_round_trips() {
        let mut contract = ViewerLaunchContract::new(
            "native-child".into(), absolute("scene.litematic"), ViewerContent::default(), None,
            ViewerTask::QuickPreview(QuickPreviewTask { initial_visible: false,
                host: Some(QuickPreviewHostWindow { hwnd: "42".into(), process_id: 100 , native_composition: None }), ..Default::default() }),
        ).unwrap();
        let bytes = contract.to_json_bytes().unwrap();
        assert_eq!(ViewerLaunchContract::from_json_slice(&bytes).unwrap(), contract);
        if let ViewerTask::QuickPreview(task) = &mut contract.task {
            task.host.as_mut().unwrap().process_id = 0;
        }
        assert!(contract.validate().unwrap_err().contains("processId"));
        if let ViewerTask::QuickPreview(task) = &mut contract.task {
            task.host.as_mut().unwrap().process_id = 100;
            task.host.as_mut().unwrap().hwnd = "0".into();
        }
        assert!(contract.validate().is_err());
    }

    #[test]
    fn quick_gpu_context_is_host_only_and_rejects_projection_preparation_options() {
        let contract = ViewerLaunchContract::new("gpu-context".into(), absolute("runtime-context"),
            ViewerContent::default(), None, ViewerTask::QuickGpuContext(QuickGpuContextTask {
                host: QuickPreviewHostWindow { hwnd: "42".into(), process_id: 100 , native_composition: None },
                graphics_backend: GraphicsApiPreference::Auto,
            })).unwrap();
        let bytes = contract.to_json_bytes().unwrap();
        assert_eq!(ViewerLaunchContract::from_json_slice(&bytes).unwrap(), contract);
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["task"]["preload"] = true.into();
        assert!(ViewerLaunchContract::from_json_slice(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut resource_contract = contract;
        resource_contract.content.resource_packs.push(absolute("pack.zip"));
        assert!(resource_contract.validate().is_err());
    }
    #[test]
    fn quick_graphics_api_is_task_bound_defaults_to_auto_and_keeps_v5_route_separate() {
        for (preference, encoded) in [(GraphicsApiPreference::Auto, "auto"), (GraphicsApiPreference::Dx12, "dx12"), (GraphicsApiPreference::Vulkan, "vulkan")] {
            let contract = ViewerLaunchContract::new("quick-api".into(), absolute("scene.litematic"),
                ViewerContent::default(), None, ViewerTask::QuickPreview(QuickPreviewTask {
                    graphics_backend: preference, ..Default::default()
                })).unwrap();
            let mut value = serde_json::to_value(&contract).unwrap();
            assert_eq!(value["task"]["graphicsBackend"], encoded);
            assert!(value["renderer"].is_null());
            assert_eq!(ViewerLaunchContract::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap(), contract);
            value["task"].as_object_mut().unwrap().remove("graphicsBackend");
            let decoded = ViewerLaunchContract::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(matches!(decoded.task, ViewerTask::QuickPreview(QuickPreviewTask { graphics_backend: GraphicsApiPreference::Auto, .. })));
        }
        assert!(serde_json::from_str::<GraphicsApiPreference>("\"iris-gl46\"").is_err());
        assert!(serde_json::from_str::<ViewerBackend>("\"dx12\"").is_err());
    }

    #[test]
    fn quick_gpu_context_round_trips_explicit_graphics_api_and_accepts_old_auto_context() {
        for preference in [GraphicsApiPreference::Auto, GraphicsApiPreference::Dx12, GraphicsApiPreference::Vulkan] {
            let contract = ViewerLaunchContract::new("quick-context-api".into(), absolute("runtime-context"),
                ViewerContent::default(), None, ViewerTask::QuickGpuContext(QuickGpuContextTask {
                    host: QuickPreviewHostWindow { hwnd: "42".into(), process_id: 100 , native_composition: None }, graphics_backend: preference,
                })).unwrap();
            let mut value = serde_json::to_value(&contract).unwrap();
            assert_eq!(ViewerLaunchContract::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap(), contract);
            value["task"].as_object_mut().unwrap().remove("graphicsBackend");
            let legacy = ViewerLaunchContract::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(matches!(legacy.task, ViewerTask::QuickGpuContext(QuickGpuContextTask { graphics_backend: GraphicsApiPreference::Auto, .. })));
            value["task"]["graphicsBackend"] = "iris-gl46".into();
            assert!(ViewerLaunchContract::from_json_slice(&serde_json::to_vec(&value).unwrap()).is_err());
        }
    }

    #[test]
    fn quick_preview_realtime_and_preload_without_resource_paths_round_trip() {
        for preload in [false, true] {
            let contract = ViewerLaunchContract::new(
                "quick-no-estimate".into(), absolute("scene.litematic"), ViewerContent::default(), None,
                ViewerTask::QuickPreview(QuickPreviewTask { preload,
                    host: Some(QuickPreviewHostWindow { hwnd: "42".into(), process_id: 100, native_composition: None }),
                    ..Default::default() }),
            ).unwrap();
            let bytes = contract.to_json_bytes().unwrap();
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["task"]["preload"], preload);
            assert!(!value["task"].as_object().unwrap().contains_key("resourceReportPath"));
            assert!(!value["task"].as_object().unwrap().contains_key("resourceDecisionPath"));
            assert_eq!(ViewerLaunchContract::from_json_slice(&bytes).unwrap(), contract);
            value["task"]["resourceReportPath"] = serde_json::Value::Null;
            value["task"]["resourceDecisionPath"] = serde_json::Value::Null;
            assert_eq!(ViewerLaunchContract::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap(), contract);
        }
    }

    #[test]
    fn quick_preview_resource_paths_remain_paired_absolute_and_distinct() {
        let mut contract = ViewerLaunchContract::new(
            "quick-path-validation".into(), absolute("scene.litematic"), ViewerContent::default(), None,
            ViewerTask::QuickPreview(QuickPreviewTask::default()),
        ).unwrap();
        for preload in [false, true] {
            for (report, decision, expected) in [
                (Some(absolute("report.json")), None, "must be paired"),
                (None, Some(absolute("decision.json")), "must be paired"),
                (Some(PathBuf::from("relative-report.json")), Some(absolute("decision.json")), "resourceReportPath"),
                (Some(absolute("report.json")), Some(PathBuf::from("relative-decision.json")), "resourceDecisionPath"),
                (Some(absolute("same.json")), Some(absolute("same.json")), "paths must differ"),
                (Some(absolute("scene.litematic")), Some(absolute("decision.json")), "paths must differ"),
                (Some(absolute("report.json")), Some(absolute("scene.litematic")), "paths must differ"),
            ] {
                contract.task = ViewerTask::QuickPreview(QuickPreviewTask {
                    preload, resource_report_path: report, resource_decision_path: decision, ..Default::default()
                });
                assert!(contract.validate().unwrap_err().contains(expected));
                assert!(contract.to_json_bytes().is_err());
                let bytes = serde_json::to_vec(&contract).unwrap();
                assert!(ViewerLaunchContract::from_json_slice(&bytes).unwrap_err().contains(expected));
            }
            contract.task = ViewerTask::QuickPreview(QuickPreviewTask {
                preload, resource_report_path: Some(absolute("report.json")),
                resource_decision_path: Some(absolute("decision.json")), ..Default::default()
            });
            assert_eq!(ViewerLaunchContract::from_json_slice(&contract.to_json_bytes().unwrap()).unwrap(), contract);
        }
    }

    #[test]
    fn quick_preview_contract_is_independent_hidden_and_strict() {
        let mut contract = ViewerLaunchContract::new(
            "quick-preview-test".to_owned(),
            absolute("scene.litematic"),
            ViewerContent::default(),
            None,
            ViewerTask::QuickPreview(QuickPreviewTask { initial_visible: false, host: None, ..Default::default() }),
        ).unwrap();
        let json = contract.to_json_bytes().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(value["task"]["kind"], "quick_preview");
        assert_eq!(value["task"]["initialVisible"], false);
        assert!(value["renderer"].is_null());
        assert_eq!(ViewerLaunchContract::from_json_slice(&json).unwrap(), contract);

        contract.renderer = interactive_contract().renderer;
        assert!(contract.validate().unwrap_err().contains("renderer settings"));
        contract.renderer = None;
        contract.content.resource_packs.push(absolute("pack.zip"));
        assert!(contract.validate().unwrap_err().contains("content resources"));
        contract.content.resource_packs.clear();
        contract.content.fabric_mods_dir = Some(absolute("mods"));
        assert!(contract.validate().unwrap_err().contains("content resources"));
        contract.content = ViewerContent::default();
        contract.task = ViewerTask::QuickPreview(QuickPreviewTask { initial_visible: true, host: None, ..Default::default() });
        assert!(contract.validate().unwrap_err().contains("initialVisible must be false"));

        let mut unknown = value;
        unknown["task"]["cacheInput"] = serde_json::Value::String(absolute("scene.cache").to_string_lossy().into_owned());
        assert!(ViewerLaunchContract::from_json_slice(&serde_json::to_vec(&unknown).unwrap()).is_err());
    }

    fn build_cache_and_warm_contract() -> ViewerLaunchContract {
        ViewerLaunchContract::new(
            "build-and-warm-contract-test".to_owned(),
            absolute("scene.litematic"),
            ViewerContent::default(),
            interactive_contract().renderer,
            ViewerTask::BuildCacheAndWarmViewer(BuildCacheAndWarmViewerTask {
                ready_file: absolute("ready.json"),
                cache_file: absolute("scene.cache"),
                cache_input: Some(absolute("scene.cache")),
                presentation: ViewerPresentation::Popup,
                purpose: ViewerInteractivePurpose::Render,
                controls: ViewerControls::unrestricted(),
                capture: None,
                initial_visible: false,
            }),
        )
        .unwrap()
    }

    fn prepare_viewer_contract() -> ViewerLaunchContract {
        ViewerLaunchContract::new(
            "prepare-viewer-contract-test".to_owned(),
            absolute("scene.litematic"),
            ViewerContent::default(),
            interactive_contract().renderer,
            ViewerTask::PrepareViewer(PrepareViewerTask {
                ready_file: absolute("viewer-ready.json"),
                cache_input: absolute("scene.cache"),
                presentation: ViewerPresentation::Popup,
                purpose: ViewerInteractivePurpose::Render,
                controls: ViewerControls::unrestricted(),
                capture: None,
                initial_visible: false,
            }),
        )
        .unwrap()
    }

    #[test]
    fn v1_round_trip_preserves_required_controls() {
        let contract = interactive_contract();
        let bytes = contract.to_json_bytes().unwrap();
        let decoded = ViewerLaunchContract::from_json_slice(&bytes).unwrap();
        assert_eq!(decoded, contract);
        let json = String::from_utf8(bytes).unwrap();
        assert!(json.contains("\"allowTimeModification\": true"));
        assert!(json.contains("\"allowWeatherModification\": true"));
    }

    #[test]
    fn future_schema_and_unknown_fields_are_rejected() {
        let mut value = serde_json::to_value(interactive_contract()).unwrap();
        value["schema"] = serde_json::Value::String("lba.viewer.launch.v2".to_owned());
        let error = ViewerLaunchContract::from_json_slice(
            serde_json::to_string(&value).unwrap().as_bytes(),
        )
        .unwrap_err();
        assert!(error.contains("unsupported launch contract schema"));

        value["schema"] = serde_json::Value::String(VIEWER_LAUNCH_SCHEMA.to_owned());
        value["legacyArgs"] = serde_json::json!(["--display-mode=full"]);
        let error = ViewerLaunchContract::from_json_slice(
            serde_json::to_string(&value).unwrap().as_bytes(),
        )
        .unwrap_err();
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn missing_controls_are_rejected_instead_of_defaulted() {
        let mut value = serde_json::to_value(interactive_contract()).unwrap();
        value["task"].as_object_mut().unwrap().remove("controls");
        let error = ViewerLaunchContract::from_json_slice(
            serde_json::to_string(&value).unwrap().as_bytes(),
        )
        .unwrap_err();
        assert!(error.contains("missing field"));
        assert!(error.contains("controls"));
    }

    #[test]
    fn legacy_render_core_selection_is_not_part_of_the_v5_contract() {
        let mut value = serde_json::to_value(interactive_contract()).unwrap();
        value["displayMode"] = serde_json::Value::String("normal".to_owned());
        let error = ViewerLaunchContract::from_json_slice(
            serde_json::to_string(&value).unwrap().as_bytes(),
        )
        .unwrap_err();
        assert!(error.contains("unknown field"));
        assert!(error.contains("displayMode"));
    }

    #[test]
    fn build_cache_rejects_renderer_only_settings() {
        let contract = ViewerLaunchContract {
            protected_task: None,
            schema: VIEWER_LAUNCH_SCHEMA.to_owned(),
            request_id: "build-contract-test".to_owned(),
            scene: absolute("scene.litematic"),
            content: ViewerContent::default(),
            renderer: interactive_contract().renderer,
            task: ViewerTask::BuildCache(BuildCacheTask {
                initial_layer_view: None,
            ready_file: absolute("ready.json"),
                cache_file: absolute("scene.cache"),
                build_mode: RenderBuildMode::Prebuilt,
                effects_profile: ViewerEffectsProfile::Full,
            }),
        };
        assert!(
            contract
                .validate()
                .unwrap_err()
                .contains("must not contain renderer-only settings")
        );
    }

    #[test]
    fn build_cache_round_trip_preserves_the_frozen_product_mode() {
        for build_mode in [RenderBuildMode::Prebuilt, RenderBuildMode::Realtime] {
            let contract = ViewerLaunchContract::new(
                "build-mode-contract-test".to_owned(),
                absolute("scene.litematic"),
                ViewerContent::default(),
                None,
                ViewerTask::BuildCache(BuildCacheTask {
                    initial_layer_view: None,
            ready_file: absolute("ready.json"),
                    cache_file: absolute("scene.cache"),
                    build_mode,
                    effects_profile: ViewerEffectsProfile::Full,
                }),
            )
            .unwrap();
            let bytes = contract.to_json_bytes().unwrap();
            let decoded = ViewerLaunchContract::from_json_slice(&bytes).unwrap();
            assert_eq!(decoded, contract);
            assert_eq!(
                serde_json::to_value(&decoded).unwrap()["task"]["buildMode"],
                serde_json::to_value(build_mode).unwrap()
            );
        }
    }

    #[test]
    fn build_cache_rejects_a_missing_product_mode_instead_of_defaulting() {
        let contract = ViewerLaunchContract::new(
            "build-mode-required-test".to_owned(),
            absolute("scene.litematic"),
            ViewerContent::default(),
            None,
            ViewerTask::BuildCache(BuildCacheTask {
                initial_layer_view: None,
            ready_file: absolute("ready.json"),
                cache_file: absolute("scene.cache"),
                build_mode: RenderBuildMode::Prebuilt,
                effects_profile: ViewerEffectsProfile::Full,
            }),
        )
        .unwrap();
        let mut value = serde_json::to_value(contract).unwrap();
        value["task"].as_object_mut().unwrap().remove("buildMode");
        let error = ViewerLaunchContract::from_json_slice(
            serde_json::to_string(&value).unwrap().as_bytes(),
        )
        .unwrap_err();
        assert!(error.contains("missing field"));
        assert!(error.contains("buildMode"));
    }

    #[test]
    fn build_cache_and_warm_contract_round_trips_with_strict_hidden_render_shape() {
        let contract = build_cache_and_warm_contract();
        let bytes = contract.to_json_bytes().unwrap();
        let decoded = ViewerLaunchContract::from_json_slice(&bytes).unwrap();
        assert_eq!(decoded, contract);

        let value = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
        assert_eq!(value["task"]["kind"], "build_cache_and_warm_viewer");
        assert_eq!(
            value["task"]["cacheInput"],
            serde_json::to_value(absolute("scene.cache")).unwrap()
        );
        assert_eq!(value["task"]["presentation"]["kind"], "popup");
        assert_eq!(value["task"]["purpose"], "render");
        assert_eq!(value["task"]["capture"], serde_json::Value::Null);
        assert_eq!(value["task"]["initialVisible"], false);
        assert!(value["renderer"].is_object());

        let mut without_cache_input = value;
        without_cache_input["task"]
            .as_object_mut()
            .unwrap()
            .remove("cacheInput");
        let decoded = ViewerLaunchContract::from_json_slice(
            serde_json::to_string(&without_cache_input)
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        let ViewerTask::BuildCacheAndWarmViewer(task) = decoded.task else {
            unreachable!()
        };
        assert_eq!(task.cache_input, None);
    }

    #[test]
    fn build_cache_and_warm_contract_rejects_relaxed_shape() {
        let mut contract = build_cache_and_warm_contract();
        contract.renderer = None;
        assert!(
            contract
                .validate()
                .unwrap_err()
                .contains("requires renderer settings")
        );

        let mut contract = build_cache_and_warm_contract();
        contract.renderer = Some(ViewerRenderer {
            backend: ViewerBackend::Auto,
            iris_fallback: IrisFallback::Vulkan,
            shaderpack: None,
            vulkan_adapter: None,
            precipitation: WeatherPrecipitation::Auto,
        });
        contract.validate().unwrap();

        let mut contract = build_cache_and_warm_contract();
        let ViewerTask::BuildCacheAndWarmViewer(task) = &mut contract.task else {
            unreachable!()
        };
        task.cache_input = Some(PathBuf::from("scene.cache"));
        assert!(contract.validate().unwrap_err().contains("task.cacheInput"));

        let mut contract = build_cache_and_warm_contract();
        let ViewerTask::BuildCacheAndWarmViewer(task) = &mut contract.task else {
            unreachable!()
        };
        task.presentation = ViewerPresentation::Embedded {
            parent_hwnd: "1".to_owned(),
        };
        assert!(contract.validate().unwrap_err().contains("must be popup"));

        let mut contract = build_cache_and_warm_contract();
        let ViewerTask::BuildCacheAndWarmViewer(task) = &mut contract.task else {
            unreachable!()
        };
        task.purpose = ViewerInteractivePurpose::PropertiesPreview;
        assert!(contract.validate().unwrap_err().contains("must be render"));

        let mut contract = build_cache_and_warm_contract();
        let ViewerTask::BuildCacheAndWarmViewer(task) = &mut contract.task else {
            unreachable!()
        };
        task.capture =
            Some(ViewerFrameCapture::new(absolute("captures/warm-frame.png"), 1).unwrap());
        assert!(contract.validate().unwrap_err().contains("must be null"));

        let mut contract = build_cache_and_warm_contract();
        let ViewerTask::BuildCacheAndWarmViewer(task) = &mut contract.task else {
            unreachable!()
        };
        task.initial_visible = true;
        assert!(contract.validate().unwrap_err().contains("must be false"));
    }

    #[test]
    fn prepare_viewer_contract_round_trips_with_required_cache_only_shape() {
        let contract = prepare_viewer_contract();
        let bytes = contract.to_json_bytes().unwrap();
        let decoded = ViewerLaunchContract::from_json_slice(&bytes).unwrap();
        assert_eq!(decoded, contract);

        let value = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
        assert_eq!(value["task"]["kind"], "prepare_viewer");
        assert_eq!(
            value["task"]["cacheInput"],
            serde_json::to_value(absolute("scene.cache")).unwrap()
        );
        assert_eq!(value["task"]["presentation"]["kind"], "popup");
        assert_eq!(value["task"]["purpose"], "render");
        assert_eq!(value["task"]["capture"], serde_json::Value::Null);
        assert_eq!(value["task"]["initialVisible"], false);

        let mut missing_cache_input = value;
        missing_cache_input["task"]
            .as_object_mut()
            .unwrap()
            .remove("cacheInput");
        let error = ViewerLaunchContract::from_json_slice(
            serde_json::to_string(&missing_cache_input)
                .unwrap()
                .as_bytes(),
        )
        .unwrap_err();
        assert!(error.contains("missing field"));
        assert!(error.contains("cacheInput"));
    }

    #[test]
    fn prepare_viewer_contract_rejects_non_hidden_non_popup_or_output_aliases() {
        let mut contract = prepare_viewer_contract();
        contract.renderer = None;
        assert!(
            contract
                .validate()
                .unwrap_err()
                .contains("requires renderer settings")
        );

        let mut contract = prepare_viewer_contract();
        let ViewerTask::PrepareViewer(task) = &mut contract.task else {
            unreachable!()
        };
        task.initial_visible = true;
        assert!(contract.validate().unwrap_err().contains("must be false"));

        let mut contract = prepare_viewer_contract();
        let ViewerTask::PrepareViewer(task) = &mut contract.task else {
            unreachable!()
        };
        task.presentation = ViewerPresentation::Embedded {
            parent_hwnd: "1".to_owned(),
        };
        assert!(contract.validate().unwrap_err().contains("must be popup"));

        let mut contract = prepare_viewer_contract();
        let ViewerTask::PrepareViewer(task) = &mut contract.task else {
            unreachable!()
        };
        task.ready_file = task.cache_input.clone();
        assert!(contract.validate().unwrap_err().contains("must differ"));
    }

    #[test]
    fn capture_contract_requires_explicit_derived_receipt_path() {
        let output = absolute("captures/frame-000007.png");
        let capture = ViewerFrameCapture::new(output.clone(), 7).unwrap();
        assert_eq!(
            capture.receipt,
            absolute("captures/frame-000007.png.receipt.json")
        );

        let value = serde_json::to_value(&capture).unwrap();
        assert_eq!(value["output"], output.to_string_lossy().as_ref());
        assert_eq!(value["receipt"], capture.receipt.to_string_lossy().as_ref());
        assert_eq!(value["frame"], 7);

        let mut missing_receipt = value.clone();
        missing_receipt.as_object_mut().unwrap().remove("receipt");
        let error = serde_json::from_value::<ViewerFrameCapture>(missing_receipt).unwrap_err();
        assert!(error.to_string().contains("missing field"));
        assert!(error.to_string().contains("receipt"));

        let mut mismatched = capture.clone();
        mismatched.receipt = absolute("captures/other.receipt.json");
        assert!(
            mismatched
                .validate()
                .unwrap_err()
                .contains("must be derived")
        );
    }

    #[test]
    fn explicit_vulkan_iris_keeps_shaderpack_and_real_capture_contract() {
        for backend in [ViewerBackend::IrisGl46, ViewerBackend::Vulkan] {
            let mut contract = interactive_contract();
            contract.renderer.as_mut().unwrap().backend = backend;
            if let ViewerTask::Interactive(task) = &mut contract.task {
                task.capture = Some(ViewerFrameCapture::new(absolute("captures/iris-frame.png"), 1).unwrap());
            }
            contract.validate().unwrap();
            let bytes = contract.to_json_bytes().unwrap();
            let decoded = ViewerLaunchContract::from_json_slice(&bytes).unwrap();
            assert_eq!(decoded.renderer.as_ref().unwrap().backend, backend);
            assert!(decoded.renderer.as_ref().unwrap().shaderpack.is_some());
        }
    }

    #[test]
    fn iris_capture_does_not_relabel_plain_vulkan_or_auto_as_a_shaderpack_path() {
        let mut contract = interactive_contract();
        if let ViewerTask::Interactive(task) = &mut contract.task {
            task.capture = Some(ViewerFrameCapture::new(absolute("captures/frame.png"), 1).unwrap());
        }
        let renderer = contract.renderer.as_mut().unwrap();
        renderer.backend = ViewerBackend::Vulkan;
        renderer.shaderpack = None;
        assert!(contract.validate().unwrap_err().contains("explicit Iris"));

        let renderer = contract.renderer.as_mut().unwrap();
        renderer.backend = ViewerBackend::Auto;
        renderer.shaderpack = Some(ViewerShaderpack { path: absolute("shaderpacks/pack.zip") });
        assert!(contract.validate().unwrap_err().contains("explicit Iris"));
    }

    #[test]
    fn capture_contract_rejects_relative_non_png_and_zero_frame_paths() {
        let relative = ViewerFrameCapture {
            output: PathBuf::from("frame.png"),
            receipt: PathBuf::from("frame.png.receipt.json"),
            frame: 1,
        };
        assert!(relative.validate().unwrap_err().contains("absolute"));

        let output = absolute("captures/frame.jpg");
        let non_png = ViewerFrameCapture {
            receipt: absolute("captures/frame.jpg.receipt.json"),
            output,
            frame: 1,
        };
        assert!(non_png.validate().unwrap_err().contains(".png"));

        let mut zero_frame = ViewerFrameCapture::new(absolute("captures/frame.png"), 1).unwrap();
        zero_frame.frame = 0;
        assert!(zero_frame.validate().unwrap_err().contains("one-based"));
    }

    #[test]
    fn capture_receipt_round_trip_is_strict_and_bound_to_contract() {
        let capture = ViewerFrameCapture::new(absolute("captures/frame-000011.png"), 11).unwrap();
        let sha256 = "a".repeat(64);
        let receipt =
            ViewerFrameCaptureReceipt::new(&capture, 1280, 720, 4_194_304, sha256.clone()).unwrap();
        assert_eq!(receipt.schema, VIEWER_FRAME_CAPTURE_RECEIPT_SCHEMA);
        assert_eq!(receipt.frame, 11);
        assert_eq!(receipt.output, capture.output);
        assert_eq!((receipt.width, receipt.height), (1280, 720));
        assert_eq!(receipt.bytes, 4_194_304);
        assert!(is_lower_hex(&receipt.sha256, &[64]));
        receipt
            .validate_completion(&capture, 1280, 720, 4_194_304, &sha256)
            .unwrap();

        let json = receipt.to_json_bytes().unwrap();
        assert_eq!(
            ViewerFrameCaptureReceipt::from_json_slice(&json).unwrap(),
            receipt
        );

        let mut value = serde_json::to_value(&receipt).unwrap();
        value["schema"] = serde_json::json!("lba.viewer.frame-capture-receipt.v2");
        assert!(
            ViewerFrameCaptureReceipt::from_json_slice(
                serde_json::to_string(&value).unwrap().as_bytes()
            )
            .unwrap_err()
            .contains("unsupported frame capture receipt schema")
        );

        value = serde_json::to_value(&receipt).unwrap();
        value["legacyComplete"] = serde_json::json!(true);
        assert!(
            ViewerFrameCaptureReceipt::from_json_slice(
                serde_json::to_string(&value).unwrap().as_bytes()
            )
            .unwrap_err()
            .contains("unknown field")
        );
    }

    #[test]
    fn capture_receipt_fails_closed_for_tampered_png_and_receipt() {
        let capture = ViewerFrameCapture::new(absolute("captures/frame-000013.png"), 13).unwrap();
        let sha256 = "b".repeat(64);
        let receipt =
            ViewerFrameCaptureReceipt::new(&capture, 640, 480, 1_048_576, sha256.clone()).unwrap();

        let tampered_sha256 = "c".repeat(64);
        assert!(
            receipt
                .validate_completion(&capture, 640, 480, 1_048_576, &tampered_sha256)
                .unwrap_err()
                .contains("SHA-256 mismatch")
        );

        let mut tampered_receipt = receipt.clone();
        tampered_receipt.frame += 1;
        assert!(
            tampered_receipt
                .validate_completion(&capture, 640, 480, 1_048_576, &sha256)
                .unwrap_err()
                .contains("frame mismatch")
        );

        let mut tampered_receipt = receipt.clone();
        tampered_receipt.sha256.make_ascii_uppercase();
        assert!(
            tampered_receipt
                .validate_completion(&capture, 640, 480, 1_048_576, &sha256)
                .unwrap_err()
                .contains("SHA-256 is invalid")
        );
    }

    #[test]
    fn preview_cache_input_is_optional_absolute_and_round_trips() {
        let renderer = interactive_contract().renderer;
        let with_cache = ViewerLaunchContract::new(
            "preview-cache-contract".to_owned(),
            absolute("scene.litematic"),
            ViewerContent::default(),
            renderer.clone(),
            ViewerTask::Preview(PreviewTask {
                output: absolute("preview.png"),
                cache_input: Some(absolute("scene.cache")),
            }),
        )
        .unwrap();
        let json = with_cache.to_json_bytes().unwrap();
        assert_eq!(
            ViewerLaunchContract::from_json_slice(&json).unwrap(),
            with_cache
        );
        assert!(String::from_utf8(json).unwrap().contains("\"cacheInput\""));

        let without_cache = ViewerLaunchContract::new(
            "preview-no-cache-contract".to_owned(),
            absolute("scene.litematic"),
            ViewerContent::default(),
            renderer.clone(),
            ViewerTask::Preview(PreviewTask {
                output: absolute("preview.png"),
                cache_input: None,
            }),
        )
        .unwrap();
        let mut missing_optional_field = serde_json::to_value(&without_cache).unwrap();
        missing_optional_field["task"]
            .as_object_mut()
            .unwrap()
            .remove("cacheInput");
        assert_eq!(
            ViewerLaunchContract::from_json_slice(
                serde_json::to_string(&missing_optional_field)
                    .unwrap()
                    .as_bytes()
            )
            .unwrap(),
            without_cache
        );

        let error = ViewerLaunchContract::new(
            "preview-relative-cache-contract".to_owned(),
            absolute("scene.litematic"),
            ViewerContent::default(),
            renderer,
            ViewerTask::Preview(PreviewTask {
                output: absolute("preview.png"),
                cache_input: Some(PathBuf::from("scene.cache")),
            }),
        )
        .unwrap_err();
        assert!(error.contains("task.cacheInput"));
        assert!(error.contains("absolute"));
    }

    #[test]
    fn viewer_integrity_is_strict_bounded_and_contract_bound() {
        let marker = ViewerIntegrity::new(
            "warm-request".to_owned(),
            "a".repeat(64),
            0,
            0,
            0,
            0,
            vec!["terrain/atlas_load count=2 sample=missing texture".to_owned()],
        )
        .unwrap();
        marker
            .validate_for_contract("warm-request", &"a".repeat(64))
            .unwrap();
        assert_eq!(marker.schema, VIEWER_INTEGRITY_SCHEMA);
        assert_eq!(marker.warning_count, 1);

        let value = serde_json::to_value(&marker).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 9);
        assert_eq!(value["schema"], VIEWER_INTEGRITY_SCHEMA);
        assert_eq!(value["requestId"], "warm-request");
        assert_eq!(value["contractSha256"], "a".repeat(64));
        assert_eq!(value["errorCount"], 0);
        assert_eq!(value["unsupportedCount"], 0);
        assert_eq!(value["missingCount"], 0);
        assert_eq!(value["skippedCount"], 0);
        assert_eq!(value["warningCount"], 1);
        assert_eq!(value["warnings"].as_array().unwrap().len(), 1);

        let mut unknown = value;
        unknown["legacyOk"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<ViewerIntegrity>(unknown)
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );
    }

    #[test]
    fn viewer_integrity_rejects_fatal_counts_and_unbounded_warnings() {
        for fatal in 0..4 {
            let mut counts = [0_usize; 4];
            counts[fatal] = 1;
            assert!(
                ViewerIntegrity::new(
                    "warm-request".to_owned(),
                    "b".repeat(64),
                    counts[0],
                    counts[1],
                    counts[2],
                    counts[3],
                    Vec::new(),
                )
                .unwrap_err()
                .contains("fatal counts")
            );
        }
        assert!(
            ViewerIntegrity::new(
                "warm-request".to_owned(),
                "b".repeat(64),
                0,
                0,
                0,
                0,
                vec!["x".repeat(MAX_VIEWER_INTEGRITY_WARNING_BYTES + 1)],
            )
            .unwrap_err()
            .contains("at most")
        );
        assert!(
            ViewerIntegrity::new(
                "warm-request".to_owned(),
                "b".repeat(64),
                0,
                0,
                0,
                0,
                vec!["warning".to_owned(); MAX_VIEWER_INTEGRITY_WARNINGS + 1],
            )
            .unwrap_err()
            .contains("entry limit")
        );
    }

    #[test]
    fn hidden_warm_ready_is_strictly_bound_to_contract_process_and_first_frame() {
        let ready = ViewerReady {
            schema: VIEWER_READY_SCHEMA.to_owned(),
            request_id: "warm-request".to_owned(),
            contract_sha256: "a".repeat(64),
            pid: 42,
            hwnd: "1234".to_owned(),
            visible: false,
            runtime_ready: true,
            first_frame_presented: true,
            cache_file_bytes: 4096,
        };
        let encoded = serde_json::to_vec(&ready).unwrap();
        let decoded: ViewerReady = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            decoded
                .validate_hidden_warm_viewer("warm-request", &"a".repeat(64), 42)
                .unwrap(),
            1234
        );

        let mut invalid = ready.clone();
        invalid.first_frame_presented = false;
        assert!(
            invalid
                .validate_hidden_warm_viewer("warm-request", &"a".repeat(64), 42)
                .unwrap_err()
                .contains("first frame")
        );
        let mut invalid = ready.clone();
        invalid.pid = 41;
        assert!(
            invalid
                .validate_hidden_warm_viewer("warm-request", &"a".repeat(64), 42)
                .unwrap_err()
                .contains("PID mismatch")
        );
        let mut invalid = ready;
        invalid.visible = true;
        assert!(
            invalid
                .validate_hidden_warm_viewer("warm-request", &"a".repeat(64), 42)
                .unwrap_err()
                .contains("reported itself visible")
        );
    }

    #[test]
    fn session_command_is_strict_sequence_bound_and_absolute() {
        let command = ViewerSessionCommand::new(
            "nova-session-42".to_owned(),
            7,
            absolute("next-launch.json"),
        )
        .unwrap();
        assert_eq!(
            ViewerSessionCommand::from_json_slice(&command.to_json_bytes().unwrap()).unwrap(),
            command
        );

        assert!(
            ViewerSessionCommand::new(
                "nova-session-42".to_owned(),
                0,
                absolute("next-launch.json")
            )
            .unwrap_err()
            .contains("non-zero")
        );
        assert!(
            ViewerSessionCommand::new(
                "nova-session-42".to_owned(),
                1,
                PathBuf::from("next-launch.json")
            )
            .unwrap_err()
            .contains("absolute")
        );

        let mut unknown = serde_json::to_value(&command).unwrap();
        unknown["detach"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<ViewerSessionCommand>(unknown)
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );
    }

    #[test]
    fn session_presentation_commands_are_strict_and_round_trip() {
        let commands = [
            ViewerSessionCommand::attach_embedded(
                "nova-session-42".to_owned(),
                1,
                "123456".to_owned(),
                1280,
                720,
            )
            .unwrap(),
            ViewerSessionCommand::show_overlay(
                "nova-session-42".to_owned(),
                2,
                -120,
                48,
                1280,
                720,
                Some(false),
            )
            .unwrap(),
            ViewerSessionCommand::show_popup("nova-session-42".to_owned(), 3).unwrap(),
            ViewerSessionCommand::hide("nova-session-42".to_owned(), 4).unwrap(),
            ViewerSessionCommand::move_overlay(
                "nova-session-42".to_owned(),
                5,
                20,
                40,
                1920,
                1080,
                Some(true),
            )
            .unwrap(),
            ViewerSessionCommand::resize("nova-session-42".to_owned(), 6, 1920, 1080, Some(true))
                .unwrap(),
            ViewerSessionCommand::release("nova-session-42".to_owned(), 7).unwrap(),
        ];
        for command in commands {
            let bytes = command.to_json_bytes().unwrap();
            assert_eq!(
                ViewerSessionCommand::from_json_slice(&bytes).unwrap(),
                command
            );
        }

        assert!(
            ViewerSessionCommand::attach_embedded(
                "nova-session-42".to_owned(),
                1,
                "0".to_owned(),
                1280,
                720,
            )
            .unwrap_err()
            .contains("must not be zero")
        );
        assert!(
            ViewerSessionCommand::resize("nova-session-42".to_owned(), 1, 0, 720, None,)
                .unwrap_err()
                .contains("non-zero")
        );

        let mut invalid =
            ViewerSessionCommand::show_popup("nova-session-42".to_owned(), 2).unwrap();
        invalid.width = Some(1280);
        assert!(invalid.validate().unwrap_err().contains("payload"));
    }

    #[test]
    fn session_status_and_unsolicited_events_use_distinct_sequences() {
        let ready = ViewerSessionStatus::new(
            "nova-session-42".to_owned(),
            0,
            123,
            "456".to_owned(),
            ViewerSessionState::Ready,
        )
        .unwrap();
        assert_eq!(
            ViewerSessionStatus::from_json_slice(&ready.to_json_bytes().unwrap()).unwrap(),
            ready
        );

        let attached = ViewerSessionStatus::new(
            "nova-session-42".to_owned(),
            7,
            123,
            "456".to_owned(),
            ViewerSessionState::Attached {
                host_hwnd: "789".to_owned(),
                width: 1280,
                height: 720,
            },
        )
        .unwrap();
        let attached_json = serde_json::to_value(&attached).unwrap();
        assert_eq!(attached_json["state"]["kind"], "attached");
        assert_eq!(attached_json["state"]["hostHwnd"], "789");

        let overlay = ViewerSessionStatus::new(
            "nova-session-42".to_owned(),
            8,
            123,
            "456".to_owned(),
            ViewerSessionState::OverlayVisible {
                screen_x: -120,
                screen_y: 48,
                width: 1280,
                height: 720,
                focused: Some(true),
            },
        )
        .unwrap();
        let overlay_json = serde_json::to_value(&overlay).unwrap();
        assert_eq!(overlay_json["state"]["kind"], "overlay_visible");
        assert_eq!(overlay_json["state"]["screenX"], -120);

        let hidden = ViewerSessionEvent::new(
            "nova-session-42".to_owned(),
            1,
            123,
            "456".to_owned(),
            ViewerSessionState::Hidden,
        )
        .unwrap();
        assert_eq!(
            ViewerSessionEvent::from_json_slice(&hidden.to_json_bytes().unwrap()).unwrap(),
            hidden
        );
        assert!(
            ViewerSessionEvent::new(
                "nova-session-42".to_owned(),
                0,
                123,
                "456".to_owned(),
                ViewerSessionState::Hidden,
            )
            .unwrap_err()
            .contains("eventId")
        );
    }
}

/// Native GPU presentation messages shared by Host and Viewer.
pub mod native_macos_wire;

#[cfg(test)]
mod scene_view_optional_control_tests {
    use super::*;
    #[test]
    fn unused_new_controls_keep_existing_wire_shape() {
        let value = serde_json::to_value(SceneViewControls::default()).unwrap();
        assert!(value.get("cameraSensitivity").is_none());
        assert!(value.get("releaseCameraInput").is_none());
        assert!(value.get("shaderOptionsResultPath").is_none());
        let input: SceneViewControls = serde_json::from_value(serde_json::json!({"releaseCameraInput": true, "cameraSensitivity": 0.5})).unwrap();
        assert!(input.release_camera_input);
        assert!(input.validate().is_ok());
    }
    #[test]
    fn sensitivity_and_shader_receipts_validate_their_real_requirements() {
        let input = SceneViewControls { camera_sensitivity: Some(0.0), ..Default::default() };
        assert!(input.validate().is_err());
        let input = SceneViewControls { shader_options_result_path: Some(PathBuf::from("relative.json")), reload_shader_options: true, ..Default::default() };
        assert!(input.validate().is_err());
        let input = SceneViewControls { shader_options_result_path: Some(std::env::temp_dir().join("receipt.json")), ..Default::default() };
        assert!(input.validate().is_err());
    }
}
