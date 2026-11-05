//! Native Viewer feedback, not transport acceptance, owns displayed scene state.
use super::{
    model::RenderDisplayMode, quick_preview::QuickPreviewHost, render_facade::RenderFacade,
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use viewer_launch_contract::{camera_input::SceneViewFeedback, SceneViewControls};

const MAX_RECEIPTS: usize = 32;
// An ACK deadline reports an unconfirmed command; it never kills/releases a scene.
const ACK_DEADLINE: Duration = Duration::from_secs(10);
static SEQUENCE: AtomicU64 = AtomicU64::new(1);
static ROOT: OnceLock<PathBuf> = OnceLock::new();
static RECEIPTS: OnceLock<Mutex<VecDeque<Arc<SceneReceipt>>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SceneViewAck {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) interaction: Option<viewer_launch_contract::SceneInteractionSnapshot>,
    pub(crate) source_path: PathBuf,
    pub(crate) display_mode: RenderDisplayMode,
    pub(crate) scene_id: String,
    pub(crate) request_id: String,
    pub(crate) camera_locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) world_time: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) world_time_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) weather: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SceneKey {
    operation_id: String,
    source: PathBuf,
    mode: RenderDisplayMode,
    owner: String,
}
struct SceneReceipt {
    key: SceneKey,
    scene_id: String,
    path: PathBuf,
    submission: tokio::sync::Mutex<()>,
    overlay_revision: AtomicU64,
}
impl Drop for SceneReceipt {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}
impl SceneReceipt {
    // Called under submission, so a late IPC request cannot undo the latest
    // mounted owner's clipping/input state for this exact Viewer generation.
    fn accept_overlay(&self, expected: Option<&str>, revision: Option<u64>) -> Result<(), String> {
        if expected.is_some_and(|expected| self.key.operation_id != expected) {
            return Err("Preview scene changed before overlay submission".into());
        }
        if let Some(revision) = revision {
            if revision <= self.overlay_revision.load(Ordering::Relaxed) {
                return Err("Native overlay update was superseded".into());
            }
            self.overlay_revision.store(revision, Ordering::Relaxed);
        }
        Ok(())
    }
    fn ack(&self, feedback: SceneViewFeedback) -> SceneViewAck {
        SceneViewAck {
            interaction: feedback.interaction,
            source_path: self.key.source.clone(),
            display_mode: self.key.mode,
            scene_id: self.scene_id.clone(),
            request_id: feedback.request_id,
            camera_locked: feedback.camera_locked,
            world_time: feedback.world_time,
            world_time_label: feedback.world_time.map(|ticks| { let minutes = (ticks % 24_000) * 60 / 1_000 + 360; format!("{:02}:{:02}", (minutes / 60) % 24, minutes % 60) }),
            weather: feedback.weather,
            error: feedback.error,
        }
    }
}

fn registry() -> &'static Mutex<VecDeque<Arc<SceneReceipt>>> {
    RECEIPTS.get_or_init(Default::default)
}
fn root() -> &'static PathBuf {
    ROOT.get_or_init(|| {
        std::env::temp_dir().join(format!(
            "nova-scene-controls-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    })
}
fn receipt_for(key: SceneKey) -> Result<Arc<SceneReceipt>, String> {
    let mut entries = registry()
        .lock()
        .map_err(|_| "Scene control receipt lock failed")?;
    // A replacement scene never reuses the previous scene's feedback file.
    entries.retain(|entry| {
        entry.key.source != key.source || entry.key.mode != key.mode || entry.key.owner == key.owner
    });
    if let Some(entry) = entries.iter().find(|entry| entry.key == key) {
        return Ok(entry.clone());
    }
    if entries.len() >= MAX_RECEIPTS {
        let index = entries
            .iter()
            .position(|entry| Arc::strong_count(entry) == 1)
            .ok_or("Too many scene control requests are awaiting native feedback")?;
        entries.remove(index);
    }
    let scene_id = format!(
        "scene-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let directory = root().join(&scene_id);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Create scene control receipt directory: {error}"))?;
    let entry = Arc::new(SceneReceipt {
        key,
        scene_id,
        path: directory.join("feedback.json"),
        submission: tokio::sync::Mutex::new(()),
        overlay_revision: AtomicU64::new(0),
    });
    entries.push_back(entry.clone());
    Ok(entry)
}
fn retire(source: &Path, mode: Option<RenderDisplayMode>) {
    if let Ok(mut entries) = registry().lock() {
        entries.retain(|entry| {
            entry.key.source != source || mode.is_some_and(|mode| entry.key.mode != mode)
        });
    }
}

fn active_scene(
    source: &Path,
    mode: Option<RenderDisplayMode>,
    controls: Option<&SceneViewControls>,
    quick: &QuickPreviewHost,
    facade: &RenderFacade,
) -> Result<SceneKey, String> {
    if mode.is_some_and(|mode| {
        !matches!(
            mode,
            RenderDisplayMode::FastExperimental
                | RenderDisplayMode::Standard
                | RenderDisplayMode::Full
        )
    }) {
        return Err("Scene controls require Fast, Standard or Full mode".into());
    }
    if mode.is_none_or(|mode| mode == RenderDisplayMode::FastExperimental) {
        if let Some(view) = quick
            .snapshot()?
            .filter(|view| view.source_path == source && view.state == "ready")
        {
            return Ok(SceneKey {
                operation_id: view.preview_id.clone(),
                source: source.to_owned(),
                mode: RenderDisplayMode::FastExperimental,
                owner: format!("quick:{}", view.preview_id),
            });
        }
        if mode == Some(RenderDisplayMode::FastExperimental) {
            return Err("The corresponding Fast preview is not running".into());
        }
    }
    if let Some(identity) = facade.active_scene_identity(source, mode, controls)? {
        return Ok(SceneKey {
            operation_id: identity.operation_id.0.clone(),
            source: source.to_owned(),
            mode: identity.display_mode,
            owner: format!(
                "render:{}:{}:{}:{}",
                identity.operation_id.0,
                identity.build_id,
                identity.generation,
                identity.session_id
            ),
        });
    }
    Err("The corresponding preview scene has ended or is not ready".into())
}

fn read_feedback(path: &Path) -> Result<Option<SceneViewFeedback>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| format!("Invalid native scene control feedback: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Read native scene control feedback: {error}")),
    }
}
async fn wait_for_feedback(
    path: &Path,
    request_id: &str,
    mut active: impl FnMut() -> Result<bool, String>,
) -> Result<SceneViewFeedback, String> {
    let started = Instant::now();
    loop {
        // Check lifetime before consuming a late receipt, including a successful one.
        if !active()? {
            return Err(
                "Preview scene changed or ended before native control acknowledgment".into(),
            );
        }
        if let Some(value) = read_feedback(path)?.filter(|value| value.request_id == request_id) {
            return Ok(value);
        }
        if started.elapsed() >= ACK_DEADLINE {
            return Err(
                "Native scene control acknowledgment timed out; the preview remains running".into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

pub(crate) async fn set_scene_view(
    source: &Path,
    mode: Option<RenderDisplayMode>,
    mut controls: SceneViewControls,
    expected_operation: Option<&str>,
    overlay_revision: Option<u64>,
    quick: &QuickPreviewHost,
    facade: &RenderFacade,
) -> Result<SceneViewAck, String> {
    // Receipt identities and paths are always Host-owned, never accepted from UI.
    controls.control_request_id = None;
    controls.control_receipt_path = None;
    controls.validate()?;
    let source = std::fs::canonicalize(source).map_err(|error| error.to_string())?;
    let key = active_scene(&source, mode, Some(&controls), quick, facade)
        .inspect_err(|_| retire(&source, mode))?;
    let entry = receipt_for(key)?;
    let _submission = entry.submission.lock().await;
    entry.accept_overlay(expected_operation, overlay_revision)?;
    let still_active = || {
        active_scene(
            &source,
            Some(entry.key.mode),
            Some(&controls),
            quick,
            facade,
        )
        .map(|key| key == entry.key)
    };
    if !still_active()? {
        return Err("Preview scene changed before controls were submitted".into());
    }
    let request_id = format!(
        "control-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    controls.control_request_id = Some(request_id.clone());
    controls.control_receipt_path = Some(entry.path.clone());
    if entry.key.mode == RenderDisplayMode::FastExperimental {
        let worker = quick.clone();
        let path = source.clone();
        let request = controls.clone();
        let accepted =
            tauri::async_runtime::spawn_blocking(move || worker.set_scene_view(&path, request))
                .await
                .map_err(|error| error.to_string())??;
        if !accepted {
            return Err("The corresponding Fast preview ended before control submission".into());
        }
    } else {
        facade
            .set_scene_view(&source, Some(entry.key.mode), controls.clone())
            .await?;
    }
    let feedback = wait_for_feedback(&entry.path, &request_id, || {
        active_scene(
            &source,
            Some(entry.key.mode),
            Some(&controls),
            quick,
            facade,
        )
        .map(|key| key == entry.key)
    })
    .await?;
    if let Some(error) = &feedback.error {
        return Err(error.clone());
    }
    Ok(entry.ack(feedback))
}

pub(crate) async fn get_scene_view_state(
    source: &Path,
    mode: Option<RenderDisplayMode>,
    quick: &QuickPreviewHost,
    facade: &RenderFacade,
) -> Result<SceneViewAck, String> {
    let source = std::fs::canonicalize(source).map_err(|error| error.to_string())?;
    let key =
        active_scene(&source, mode, None, quick, facade).inspect_err(|_| retire(&source, mode))?;
    let entry = receipt_for(key)?;
    if let Some(feedback) = read_feedback(&entry.path)? {
        if active_scene(&source, Some(entry.key.mode), None, quick, facade)? != entry.key {
            return Err("Preview scene changed while reading native control state".into());
        }
        return Ok(entry.ack(feedback));
    }
    // A no-op request asks the actual Viewer for its first readback. No guessed
    // locked/default state is returned while a receipt has never existed.
    set_scene_view(
        &source,
        Some(entry.key.mode),
        SceneViewControls::default(),
        None, None,
        quick,
        facade,
    )
    .await
}

/// Call during normal application shutdown, after stopping the owned Viewers.
pub(crate) fn cleanup() {
    if let Ok(mut entries) = registry().lock() {
        entries.clear();
    }
    if let Some(root) = ROOT.get() {
        let _ = std::fs::remove_dir(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Arc<SceneReceipt> {
        receipt_for(SceneKey {
            operation_id: "test-operation".into(),
            source: root().join(format!(
                "fixture-{}",
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            )),
            mode: RenderDisplayMode::Full,
            owner: "test-viewer".into(),
        })
        .unwrap()
    }
    #[tokio::test(flavor = "current_thread")]
    async fn receipt_requires_matching_request_and_live_scene() {
        let entry = fixture();
        let feedback = SceneViewFeedback {
            interaction: None,
            request_id: "requested".into(),
            camera_locked: true,
            world_time: Some(6000),
            weather: Some("clear".into()),
            error: None,
        };
        viewer_launch_contract::camera_input::publish_feedback(&entry.path, &feedback).unwrap();
        assert_eq!(
            wait_for_feedback(&entry.path, "requested", || Ok(true))
                .await
                .unwrap(),
            feedback
        );
        assert!(wait_for_feedback(&entry.path, "requested", || Ok(false))
            .await
            .unwrap_err()
            .contains("changed or ended"));
        let mut observations = 0;
        assert!(wait_for_feedback(&entry.path, "newer", || {
            observations += 1;
            Ok(observations < 2)
        })
        .await
        .is_err());
        assert_eq!(observations, 2);
    }
    #[test]
    fn replacement_scene_never_reuses_receipt_or_ack_identity() {
        let first = fixture();
        let second = receipt_for(SceneKey {
            owner: "replacement-viewer".into(),
            ..first.key.clone()
        })
        .unwrap();
        assert_ne!(first.path, second.path);
        assert_ne!(first.scene_id, second.scene_id);
        assert!(read_feedback(&second.path).unwrap().is_none());
    }
    #[test]
    fn overlay_revision_and_operation_reject_late_cleanup_without_changing_scene() {
        let entry = fixture();
        entry.accept_overlay(Some("test-operation"), Some(7)).unwrap();
        assert!(entry.accept_overlay(Some("test-operation"), Some(6)).is_err());
        assert!(entry.accept_overlay(Some("departed-operation"), Some(9)).is_err());
        assert_eq!(entry.overlay_revision.load(Ordering::Relaxed), 7);
        entry.accept_overlay(Some("test-operation"), Some(8)).unwrap();
        entry.accept_overlay(None, None).unwrap(); // Existing non-overlay controls.
        assert_eq!(entry.overlay_revision.load(Ordering::Relaxed), 8);
    }

}
