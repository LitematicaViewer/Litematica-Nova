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
}

impl QuickPreviewView {
    fn accepts_presentation(&self, revision: u64) -> bool { revision > self.presentation_revision }

    fn apply_status(&mut self, state: &ViewerSessionState, latest: bool) {
        match state {
            ViewerSessionState::Ready => self.state = "ready",
            ViewerSessionState::Attached { .. } if latest => self.visible = self.presentation.visible,
            ViewerSessionState::Hidden if latest => self.visible = false,
            ViewerSessionState::Released => { self.state = "closed"; self.visible = false; }
            ViewerSessionState::Error { message, .. } if latest => self.error = Some(message.clone()),
            _ => {}
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct StartQuickPreviewInput { pub(crate) source_path: PathBuf }

#[derive(Clone)]
pub(crate) struct QuickPreviewHost {
    supervisor: Arc<ProcessSupervisor>,
    viewer_exe: PathBuf,
    data_root: PathBuf,
    sequence: Arc<AtomicU64>,
    session: Arc<Mutex<Option<QuickSession>>>,
}

struct QuickSession {
    view: QuickPreviewView,
    child: ChildLease,
    contract: ViewerLaunchContractFile,
    directory: PathBuf,
    stdout: PathBuf,
    stderr: PathBuf,
    source_bytes: u64,
    source_modified: Option<SystemTime>,
    next_command: u64,
    next_status: u64,
    next_event: u64,
    pending_intents: BTreeMap<u64, QuickPresentationIntent>,
    host_window: QuickPreviewHostWindow,
    launch_accepted: bool,
}

impl QuickPreviewHost {
    pub(crate) fn new(supervisor: Arc<ProcessSupervisor>, viewer_exe: PathBuf, data_root: PathBuf) -> Self {
        Self { supervisor, viewer_exe, data_root, sequence: Arc::new(AtomicU64::new(1)), session: Arc::new(Mutex::new(None)) }
    }

    pub(crate) fn start(&self, input: StartQuickPreviewInput, host_window: QuickPreviewHostWindow) -> Result<QuickPreviewView, String> {
        let source = fs::canonicalize(&input.source_path).map_err(|error| format!("读取快速预览投影失败：{error}"))?;
        let meta = fs::metadata(&source).map_err(|error| error.to_string())?;
        if !meta.is_file() || !source.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("litematic")) {
            return Err("快速预览需要现有的 .litematic 文件。".to_owned());
        }
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        if let Some(session) = slot.as_mut() {
            if let Err(error) = session.refresh(&self.supervisor) {
                self.stop_session(session)?;
                session.view.state = "failed";
                session.view.error = Some(error);
            }
            if session.host_window == host_window && session.view.source_path == source && session.source_bytes == meta.len()
                && session.source_modified == meta.modified().ok() && matches!(session.view.state, "starting" | "ready") {
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
        let backend = ViewerBackendIdentity::capture(&self.viewer_exe)?;
        let launch = ViewerLaunchContract::new(id.clone(), source.clone(), ViewerContent::default(), None,
            ViewerTask::QuickPreview(QuickPreviewTask { initial_visible: false, host: Some(host_window.clone()) }))?;
        let contract = ViewerLaunchContractFile::create(directory.join("launch.json"), &launch, &backend)?;
        let stdout = directory.join("stdout.log");
        let stderr = directory.join("stderr.log");
        let mut spec = LaunchSpec::new(ExecutableRole::NativeViewer, LaunchIdentity {
            owner_epoch: self.supervisor.owner_epoch(), generation,
            launch_token: LaunchToken::new(id.clone()).map_err(|error| error.to_string())?,
        });
        spec.args.push(contract.command_arg().into());
        spec.environment = vec![
            ("LITEMATICA_NOVA_VIEWER_SESSION_DIRECTORY".into(), session_directory.as_os_str().to_owned()),
            ("LITEMATICA_NOVA_VIEWER_SESSION_ID".into(), id.clone().into()),
        ];
        spec.current_dir = self.viewer_exe.parent().map(Path::to_path_buf);
        spec.stdout = OutputTarget::File { path: stdout.clone(), append: false };
        spec.stderr = OutputTarget::File { path: stderr.clone(), append: false };
        let child = self.supervisor.spawn(spec).map_err(|error| format!("启动快速预览失败：{error}"))?.child;
        let view = QuickPreviewView { preview_id: id, source_path: source, render_mode: "fast_experimental", state: "starting",
            presentation: QuickPresentationIntent::default(), presentation_revision: 0, visible: false, error: None };
        *slot = Some(QuickSession { view: view.clone(), child, contract, directory: session_directory, stdout, stderr,
            source_bytes: meta.len(), source_modified: meta.modified().ok(), next_command: 1, next_status: 0, next_event: 1, pending_intents: BTreeMap::new(),
            launch_accepted: false, host_window });
        Ok(view)
    }

    pub(crate) fn snapshot(&self) -> Result<Option<QuickPreviewView>, String> {
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        if let Some(session) = slot.as_mut() {
            if let Err(error) = session.refresh(&self.supervisor) {
                self.stop_session(session)?;
                session.view.state = "failed";
                session.view.error = Some(error);
            }
        }
        Ok(slot.as_ref().map(|session| session.view.clone()))
    }

    pub(crate) fn present(&self, id: &str, intent: QuickPresentationIntent, revision: u64) -> Result<QuickPreviewView, String> {
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        let session = slot.as_mut().filter(|session| session.view.preview_id == id).ok_or("快速预览已切换，请重新加载。")?;
        session.refresh(&self.supervisor)?;
        if !session.view.accepts_presentation(revision) { return Ok(session.view.clone()); }
        if session.view.state != "ready" { return Err("快速预览尚未就绪。".to_owned()); }
        let seq = session.next_command;
        let command = presentation_command(id, seq, &intent, &session.host_window)?;
        publish_command(&session.directory, &command)?;
        session.next_command += 1;
        session.pending_intents.insert(seq, intent.clone());
        // This is desired intent, not evidence that an OS window is visible.
        // `visible` is updated only from the Viewer's session acknowledgement.
        session.view.presentation = intent;
        session.view.presentation_revision = revision;
        session.view.error = None;
        Ok(session.view.clone())
    }

    pub(crate) fn close(&self, id: &str) -> Result<(), String> {
        let mut slot = self.session.lock().map_err(|_| "快速预览状态锁异常".to_owned())?;
        if let Some(session) = slot.as_mut().filter(|session| session.view.preview_id == id) { self.stop_session(session)?; }
        Ok(())
    }

    fn stop_session(&self, session: &mut QuickSession) -> Result<(), String> {
        if session.view.state == "ready" {
            let command = ViewerSessionCommand::release(session.view.preview_id.clone(), session.next_command)?;
            publish_command(&session.directory, &command)?;
            session.next_command += 1;
        }
        if !matches!(self.supervisor.wait_for_exit(&session.child, Duration::from_millis(300)), WaitOutcome::Exited(_) | WaitOutcome::Stale) {
            self.supervisor.terminate(&session.child, 0).map_err(|error| error.to_string())?;
            if matches!(self.supervisor.wait_for_exit(&session.child, Duration::from_secs(2)), WaitOutcome::TimedOut) {
                return Err("快速预览正在结束，请稍后重试。".to_owned());
            }
        }
        session.view.state = "closed";
        session.view.visible = false;
        session.view.presentation = QuickPresentationIntent::default();
        Ok(())
    }
}

impl QuickSession {
    fn refresh(&mut self, supervisor: &ProcessSupervisor) -> Result<(), String> {
        if matches!(self.view.state, "closed" | "failed") { return Ok(()); }
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
                if status.session_id != self.view.preview_id || status.pid != self.child.pid || status.sequence != self.next_status {
                    return Err("快速预览回执与当前进程不一致。".to_owned());
                }
                if !matches!(&status.state, ViewerSessionState::Error { .. }) && status.sequence + 1 == self.next_command {
                    if let Some(intent) = self.pending_intents.get(&status.sequence) { self.view.presentation = intent.clone(); }
                }
                self.view.apply_status(&status.state, status.sequence + 1 == self.next_command);
                self.pending_intents.remove(&self.next_status);
                self.next_status += 1;
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        loop {
            let path = self.directory.join(format!("event-{:020}.json", self.next_event));
            if !path.is_file() { break; }
            let event = ViewerSessionEvent::from_json_slice(&fs::read(&path).map_err(|error| error.to_string())?)?;
            if event.session_id != self.view.preview_id || event.pid != self.child.pid || event.event_id != self.next_event {
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
            if self.view.state != "closed" {
                self.view.state = if exit.exit_code == Some(0) { "closed" } else { "failed" };
                self.view.visible = false;
                self.view.presentation = QuickPresentationIntent::default();
                if self.view.state == "failed" {
                    self.view.error = Some(format!("快速预览已退出（{:?}）。\n{}\n{}", exit.exit_code, read_log(&self.stderr, true)?, read_log(&self.stdout, true)?));
                }
            }
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
            visible: false, error: None }
    }

    #[test]
    fn quick_presentation_revision_rejects_old_show_and_duplicate_commands() {
        let view = view(QuickPresentationIntent::default());
        assert!(!view.accepts_presentation(6));
        assert!(!view.accepts_presentation(7));
        assert!(view.accepts_presentation(8));
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
