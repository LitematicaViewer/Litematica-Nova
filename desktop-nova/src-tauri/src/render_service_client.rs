use crate::platform::process_supervisor::{
    ChildId, ChildLease, ExecutableRole, LaunchIdentity, LaunchSpec, LaunchToken, OutputTarget,
    ProcessEvent, ProcessExit, ProcessSupervisor, SpawnDisposition, TerminateOutcome,
};
use nova_render_protocol::host_supervision::{
    ExecutableRole as HostExecutableRole, HostChildExited, HostOutputTarget, HostProcessCommand,
    HostProcessOutcome, HostProcessRequest, HostProcessResponse, HostSpawnDisposition,
    LaunchSpec as HostLaunchSpec,
};
use nova_render_protocol::windows_pipe::NamedPipeStream;
use nova_render_protocol::{
    read_envelope, write_envelope, ArtifactSnapshot, BuildSnapshot, ClientHello, ClientRole,
    Envelope, Payload, Request, ResponseBody, ServiceError, ServiceEvent, ServiceSnapshot,
    UiCommand, ViewerSnapshot, PROTOCOL_VERSION,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type PendingResponse = tokio::sync::oneshot::Sender<Result<ResponseBody, ServiceError>>;

#[derive(Clone)]
pub(crate) struct RenderServiceClient {
    inner: Arc<RenderServiceClientInner>,
}

struct RenderServiceClientInner {
    writer: Mutex<Option<mpsc::Sender<Envelope>>>,
    pending: Mutex<HashMap<String, PendingResponse>>,
    event_subscribers: Mutex<Vec<mpsc::Sender<ServiceEvent>>>,
    snapshot: RwLock<ServiceSnapshot>,
    connection_error: RwLock<Option<String>>,
    connected: tokio::sync::Notify,
    envelope_sequence: AtomicU64,
    request_sequence: AtomicU64,
    shutdown_requested: AtomicBool,
    service_lease: Mutex<Option<ChildLease>>,
    supervisor: Mutex<Option<Arc<ProcessSupervisor>>>,
    backend_dir: Mutex<Option<PathBuf>>,
    native_parent: Mutex<Option<(isize, u32)>>,
    native_composition: Mutex<Option<viewer_launch_contract::NativeCompositionEndpoint>>,
    render_click_epoch_ms: Mutex<Option<u64>>,
    render_runtime_root: Mutex<Option<PathBuf>>,
    configured_service: Mutex<Option<(PathBuf, PathBuf)>>,
    launch_gate: Mutex<()>,
    protection: Mutex<crate::application::protected_render::ProtectedRender>,
    service_protection: Mutex<Option<nova_render_protocol::ProtectedTask>>,
    hosted_children: Mutex<HashMap<ChildId, HostedChild>>,
    pending_process_exits: Mutex<HashMap<ChildId, ProcessExit>>,
    observed_process_exits: Mutex<Vec<ProcessExit>>,
    process_exit_changed: Condvar,
}

#[derive(Clone)]
struct HostedChild {
    role: HostExecutableRole,
    identity: nova_render_protocol::host_supervision::HostLaunchIdentity,
    lease: ChildLease,
    announced: bool,
    pending_exit: Option<ProcessExit>,
}

struct HostProcessExecution {
    outcome: HostProcessOutcome,
    spawned_child: Option<ChildId>,
}

impl Default for RenderServiceClient {
    fn default() -> Self {
        Self {
            inner: Arc::new(RenderServiceClientInner {
                writer: Mutex::new(None),
                pending: Mutex::new(HashMap::new()),
                event_subscribers: Mutex::new(Vec::new()),
                snapshot: RwLock::new(ServiceSnapshot::default()),
                connection_error: RwLock::new(None),
                connected: tokio::sync::Notify::new(),
                envelope_sequence: AtomicU64::new(1),
                request_sequence: AtomicU64::new(1),
                shutdown_requested: AtomicBool::new(false),
                service_lease: Mutex::new(None),
                supervisor: Mutex::new(None),
                backend_dir: Mutex::new(None),
                native_parent: Mutex::new(None),
                native_composition: Mutex::new(None),
                render_click_epoch_ms: Mutex::new(None),
                render_runtime_root: Mutex::new(None),
                configured_service: Mutex::new(None),
                launch_gate: Mutex::new(()),
                protection: Mutex::new(Default::default()),
                service_protection: Mutex::new(None),
                hosted_children: Mutex::new(HashMap::new()),
                pending_process_exits: Mutex::new(HashMap::new()),
                observed_process_exits: Mutex::new(Vec::new()),
                process_exit_changed: Condvar::new(),
            }),
        }
    }
}

impl RenderServiceClient {
    pub(crate) fn record_render_click(&self, epoch_ms: Option<u64>) {
        *self.inner.render_click_epoch_ms.lock().unwrap() = epoch_ms.filter(|value| *value != 0);
    }

    pub(crate) fn set_native_host(&self, host: &viewer_launch_contract::QuickPreviewHostWindow) -> Result<(), String> {
        host.validate()?;
        let native_id = host.hwnd.parse::<isize>().map_err(|error| error.to_string())?;
        self.set_native_parent(native_id);
        *self.inner.native_composition.lock().unwrap() = host.native_composition.clone();
        Ok(())
    }

    pub(crate) fn set_native_parent(&self, hwnd: isize) {
        *self.inner.native_parent.lock().unwrap() = (hwnd != 0).then_some((hwnd, std::process::id()));
    }

    pub(crate) fn launch_in_background(
        &self,
        supervisor: Arc<ProcessSupervisor>,
        service_exe: std::path::PathBuf,
        data_root: std::path::PathBuf,
    ) {
        *self.inner.supervisor.lock().unwrap() = Some(Arc::clone(&supervisor));
        *self.inner.backend_dir.lock().unwrap() = service_exe.parent().map(Path::to_path_buf);
        *self.inner.render_runtime_root.lock().unwrap() = Some(data_root.join("render-service"));
        // Protected code is never unlocked at ordinary application startup.
        // Fast preview, diagnostics and updates remain independent of this service.
        *self.inner.configured_service.lock().unwrap() = Some((service_exe, data_root));
    }

    fn prepare_task_and_connect(&self, command: &UiCommand) -> Result<Option<nova_render_protocol::ProtectedTask>, String> {
        // Presentation, cancellation and safe shutdown must not wait behind a
        // new task's network grant or materialization.
        if !matches!(command, UiCommand::PreflightBuild(_) | UiCommand::StartBuild(_)) {
            return Ok(None);
        }
        let _gate = self.inner.launch_gate.lock().unwrap();
        let configured = self.inner.configured_service.lock().unwrap().clone();
        // In-process transport tests install an authenticated writer directly.
        let Some((service_exe, data_root)) = configured else { return Ok(None); };
        let task = match command {
            UiCommand::PreflightBuild(build) => {
                let input = serde_json::to_vec(build).map_err(|e|e.to_string())?;
                Some(self.inner.protection.lock().unwrap().acquire(&data_root, service_exe.parent().unwrap(), &input)?)
            }
            UiCommand::StartBuild(start) => {
                let mut protection = self.inner.protection.lock().unwrap();
                Some(if let Some(admission) = &start.admission {
                    protection.take_admission(&admission.token)?
                } else {
                    let input = serde_json::to_vec(&start.build).map_err(|e|e.to_string())?;
                    protection.acquire(&data_root, service_exe.parent().unwrap(), &input)?
                })
            }
            _ => None,
        };
        if self.inner.writer.lock().unwrap().is_none() {
            let context = task.as_ref().ok_or_else(|| "受保护渲染服务尚未启动".to_owned())?;
            *self.inner.connection_error.write().unwrap() = None;
            *self.inner.service_protection.lock().unwrap() = Some(context.clone());
            let supervisor = self.inner.supervisor.lock().unwrap().clone().ok_or("process supervisor unavailable")?;
            self.launch_and_connect(&supervisor, &service_exe, &data_root)?;
        }
        Ok(task)
    }

    pub(crate) fn observe_process_events(&self, events: mpsc::Receiver<ProcessEvent>) {
        let client = self.clone();
        std::thread::Builder::new()
            .name("nova-process-events".to_owned())
            .spawn(move || {
                while let Ok(ProcessEvent::Exited(exit)) = events.recv() {
                    client.handle_process_exit(exit.clone());
                    client.inner.observed_process_exits.lock().unwrap().push(exit);
                    client.inner.process_exit_changed.notify_all();
                }
            })
            .expect("spawn process supervisor event bridge");
    }

    fn launch_and_connect(
        &self,
        supervisor: &ProcessSupervisor,
        service_exe: &std::path::Path,
        data_root: &std::path::Path,
    ) -> Result<(), String> {
        if self.inner.shutdown_requested.load(Ordering::Acquire) {
            return Err("render service connection cancelled by application shutdown".into());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("create render service endpoint timestamp: {error}"))?
            .as_nanos();
        let pid = std::process::id();
        let pipe_name = format!(r"\\.\pipe\litematica-nova-render-{pid}-{now:x}");
        let auth_token = format!(
            "{:x}",
            Sha256::digest(format!("{pipe_name}\0{}\0{now}", service_exe.display()).as_bytes())
        );

        let identity = LaunchIdentity {
            owner_epoch: supervisor.owner_epoch(),
            generation: 1,
            launch_token: LaunchToken::new(format!("render-service-{pid}-{now:x}"))
                .map_err(|error| format!("create render service launch token: {error}"))?,
        };
        let logs = crate::application::diagnostics::role_directory("render-service", data_root.join("render-service").join("logs"));
        std::fs::create_dir_all(&logs)
            .map_err(|error| format!("create render service log directory: {error}"))?;
        let mut launch = LaunchSpec::new(ExecutableRole::RenderService, identity);
        launch.args = vec![
            format!("--pipe={pipe_name}").into(),
            format!("--auth-token={auth_token}").into(),
            format!("--parent-pid={pid}").into(),
        ];
        launch.environment = vec![("LBA_DATA_ROOT".into(), data_root.as_os_str().to_owned())];
        if let Some(task) = self.inner.service_protection.lock().unwrap().as_ref() {
            launch.environment.push(("NOVA_PROTECTED_TICKET_PATH".into(), task.ticket_path.as_os_str().to_owned()));
            launch.environment.push(("NOVA_PROTECTED_MODULE_PATH".into(), task.module_path.as_os_str().to_owned()));
        }
        launch.current_dir = service_exe.parent().map(std::path::Path::to_path_buf);
        (launch.stdout, launch.stderr) = render_service_output_targets(data_root);
        let service = supervisor.spawn(launch).map_err(|error| {
            format!(
                "start supervised nova-render-service {}: {error}",
                service_exe.display()
            )
        })?;
        let early_exit = {
            let mut service_lease = self.inner.service_lease.lock().unwrap();
            let mut pending_exits = self.inner.pending_process_exits.lock().unwrap();
            *service_lease = Some(service.child.clone());
            pending_exits.remove(&service.child.id)
        };
        if let Some(exit) = early_exit {
            self.handle_process_exit(exit);
        }

        let mut stream = match connect_with_service_startup_retry(OsStr::new(&pipe_name)) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = supervisor.terminate(&service.child, 1);
                return Err(error);
            }
        };
        let hello = Envelope::new(
            0,
            Payload::Hello(ClientHello {
                protocol_version: PROTOCOL_VERSION,
                auth_token,
                role: ClientRole::ApplicationHost,
                instance_id: format!("nova-ui-{pid}"),
                pid,
                application_owner_epoch: Some(supervisor.owner_epoch().get()),
            }),
        );
        write_envelope(&mut stream, &hello)
            .map_err(|error| format!("send render service hello: {error}"))?;
        let accepted = read_envelope(&mut stream)
            .map_err(|error| format!("read render service hello: {error}"))?
            .ok_or_else(|| "render service closed before hello ACK".to_owned())?;
        let Payload::HelloAccepted(hello) = accepted.payload else {
            return Err("render service returned a non-hello response".to_owned());
        };
        if let Some(snapshot) = hello.snapshot {
            *self.inner.snapshot.write().unwrap() = snapshot.clone();
            self.publish_event(ServiceEvent::Snapshot(snapshot.clone()));
        }

        let (writer_tx, writer_rx) = mpsc::channel::<Envelope>();
        *self.inner.writer.lock().unwrap() = Some(writer_tx);
        *self.inner.connection_error.write().unwrap() = None;
        self.inner.connected.notify_waiters();
        // A close between child spawn and IPC setup must still reach this new
        // writer. Never let the late connection undo a shutdown request.
        if self.inner.shutdown_requested.load(Ordering::Acquire) { self.request_shutdown(); }

        let mut writer_stream = stream
            .try_clone()
            .map_err(|error| format!("duplicate render service writer pipe handle: {error}"))?;
        let writer_client = self.clone();
        std::thread::Builder::new()
            .name("nova-render-service-writer".to_owned())
            .spawn(move || {
                while let Ok(envelope) = writer_rx.recv() {
                    if let Err(error) = write_envelope(&mut writer_stream, &envelope) {
                        let service_error =
                            ServiceError::new("service_write_failed", error.to_string(), true);
                        writer_client.disconnect(service_error);
                        break;
                    }
                }
            })
            .map_err(|error| format!("spawn render service writer: {error}"))?;

        let reader_client = self.clone();
        std::thread::Builder::new()
            .name("nova-render-service-reader".to_owned())
            .spawn(move || loop {
                match read_envelope(&mut stream) {
                    Ok(Some(envelope)) => reader_client.handle_envelope(envelope),
                    Ok(None) => {
                        reader_client.disconnect(ServiceError::new(
                            "service_disconnected",
                            "nova-render-service closed its IPC connection",
                            true,
                        ));
                        break;
                    }
                    Err(error) => {
                        reader_client.disconnect(ServiceError::new(
                            "service_read_failed",
                            error.to_string(),
                            true,
                        ));
                        break;
                    }
                }
            })
            .map_err(|error| format!("spawn render service reader: {error}"))?;
        Ok(())
    }

    pub(crate) async fn request(&self, command: UiCommand) -> Result<ResponseBody, String> {
        if self.inner.writer.lock().unwrap().is_none() {
            match &command {
                UiCommand::GetSnapshot | UiCommand::Subscribe => return Ok(ResponseBody::Snapshot(self.snapshot())),
                UiCommand::CancelBuild { .. } | UiCommand::HideViewer { .. }
                    | UiCommand::ReleaseViewer { .. } | UiCommand::Shutdown => return Ok(ResponseBody::Ack),
                UiCommand::PreflightBuild(_) | UiCommand::StartBuild(_) => {},
                _ => return Err("受保护渲染服务尚未启动".to_owned()),
            }
        }
        let client = self.clone();
        let preparing = command.clone();
        let protected_task = tauri::async_runtime::spawn_blocking(move || client.prepare_task_and_connect(&preparing))
            .await.map_err(|e|format!("prepare protected render task: {e}"))??;
        let admission_context = protected_task.clone();
        let request_number = self.inner.request_sequence.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("ui-{}-{request_number}", std::process::id());
        let sequence = self.inner.envelope_sequence.fetch_add(1, Ordering::Relaxed);
        let envelope = Envelope::new(
            sequence,
            Payload::Request(Request {
                protected_task,
                request_id: request_id.clone(),
                command,
            }),
        );
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        self.inner
            .pending
            .lock()
            .unwrap()
            .insert(request_id.clone(), response_tx);
        let writer = loop {
            let notified = self.inner.connected.notified();
            if let Some(writer) = self.inner.writer.lock().unwrap().as_ref().cloned() {
                break writer;
            }
            if let Some(error) = self.inner.connection_error.read().unwrap().clone() {
                self.inner.pending.lock().unwrap().remove(&request_id);
                return Err(error);
            }
            notified.await;
        };
        let send_result = writer
            .send(envelope)
            .map_err(|_| "nova-render-service writer is unavailable".to_owned());
        if let Err(error) = send_result {
            self.inner.pending.lock().unwrap().remove(&request_id);
            return Err(error);
        }
        let result = response_rx.await
            .map_err(|_| "nova-render-service response channel closed".to_owned())?
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        if let (ResponseBody::BuildPreflight(preflight), Some(task)) = (&result, admission_context) {
            if let Some(token) = &preflight.admission_token {
                self.inner.protection.lock().unwrap().remember_admission(token.clone(), task);
            }
        }
        Ok(result)
    }

    /// Completes only after the production ApplicationHost HelloAccepted path
    /// installed the service writer, or reports the startup failure.
    pub(crate) async fn wait_connected(&self) -> Result<(), String> {
        loop {
            let notified = self.inner.connected.notified();
            if self.inner.writer.lock().unwrap().is_some() {
                return Ok(());
            }
            if let Some(error) = self.inner.connection_error.read().unwrap().clone() {
                return Err(error);
            }
            notified.await;
        }
    }

    pub(crate) fn connection_error(&self) -> Option<String> {
        self.inner.connection_error.read().unwrap().clone()
    }

    pub(crate) fn observed_process_exits(&self) -> Vec<ProcessExit> {
        self.inner.observed_process_exits.lock().unwrap().clone()
    }

    pub(crate) fn wait_for_observed_process_exits(&self, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut exits = self.inner.observed_process_exits.lock().unwrap();
        while exits.len() < count {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (next, wait) = self
                .inner
                .process_exit_changed
                .wait_timeout(exits, deadline.saturating_duration_since(now))
                .unwrap();
            exits = next;
            if wait.timed_out() && exits.len() < count {
                return false;
            }
        }
        true
    }

    pub(crate) fn snapshot(&self) -> ServiceSnapshot {
        self.inner.snapshot.read().unwrap().clone()
    }

    /// Adds an in-process Application listener. Render wire events stay inside
    /// the Rust host; presentation subscribers receive only facade projections.
    pub(crate) fn subscribe_events(&self) -> mpsc::Receiver<ServiceEvent> {
        let (sender, receiver) = mpsc::channel();
        self.inner.event_subscribers.lock().unwrap().push(sender);
        receiver
    }

    pub(crate) fn request_shutdown(&self) {
        self.inner.shutdown_requested.store(true, Ordering::Release);
        let sequence = self.inner.envelope_sequence.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("shutdown-{}-{sequence}", std::process::id());
        if let Some(writer) = self.inner.writer.lock().unwrap().as_ref() {
            let _ = writer.send(Envelope::new(
                sequence,
                Payload::Request(Request {
                    request_id,
                    protected_task: None,
                    command: UiCommand::Shutdown,
                }),
            ));
        }
    }

    fn handle_envelope(&self, envelope: Envelope) {
        match envelope.payload {
            Payload::Response(response) => {
                if let Some(sender) = self
                    .inner
                    .pending
                    .lock()
                    .unwrap()
                    .remove(&response.request_id)
                {
                    let _ = sender.send(response.result);
                }
            }
            Payload::Event(event) => {
                self.apply_event(&event);
                self.publish_event(event.clone());
            }
            Payload::HostProcessRequest(request) => {
                let client = self.clone();
                std::thread::Builder::new()
                    .name("nova-host-process-request".to_owned())
                    .spawn(move || client.handle_host_process_request(request))
                    .expect("spawn host process request handler");
            }
            _ => {}
        }
    }

    fn handle_host_process_request(&self, request: HostProcessRequest) {
        let (response, spawned_child) = match self.execute_host_process_request(&request) {
            Ok(execution) => (
                HostProcessResponse {
                    request_id: request.request_id.clone(),
                    role: request.role(),
                    identity: request.identity().clone(),
                    outcome: execution.outcome,
                },
                execution.spawned_child,
            ),
            Err(error) => (
                HostProcessResponse {
                    request_id: request.request_id.clone(),
                    role: request.role(),
                    identity: request.identity().clone(),
                    outcome: HostProcessOutcome::Failed {
                        code: "host_process_request_rejected".to_owned(),
                        message: bounded_error_message(&error),
                        retryable: false,
                    },
                },
                None,
            ),
        };
        self.publish_host_process_execution(response, spawned_child);
    }

    fn publish_host_process_execution(
        &self,
        response: HostProcessResponse,
        spawned_child: Option<ChildId>,
    ) {
        // Publish the launch response before making a newly created child eligible
        // for exit forwarding. A process that dies during CreateProcess therefore
        // cannot race ahead of the lease that Render Service must record first.
        let response_sent = self
            .send_payload(Payload::HostProcessResponse(response))
            .is_ok();
        if let Some(child_id) = spawned_child {
            self.announce_hosted_child(child_id);
        }
        if !response_sent {
            self.terminate_hosted_children();
        }
    }

    fn execute_host_process_request(
        &self,
        request: &HostProcessRequest,
    ) -> Result<HostProcessExecution, String> {
        request.validate()?;
        let supervisor = self
            .inner
            .supervisor
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "Application process supervisor is unavailable".to_owned())?;
        request.identity().validate_current(
            supervisor.owner_epoch().get(),
            request.identity().generation,
        )?;
        match &request.command {
            HostProcessCommand::Spawn(spec) => {
                let mut checked_spec = spec.clone();
                let ticket = checked_spec.environment.remove("NOVA_PROTECTED_TICKET_PATH");
                let module = checked_spec.environment.remove("NOVA_PROTECTED_MODULE_PATH");
                match (ticket, module) {
                    (Some(ticket), Some(module)) => {
                        let runtime = self.inner.render_runtime_root.lock().unwrap().clone().ok_or("render runtime root unavailable")?;
                        let protected_root = runtime.parent().ok_or("application data root unavailable")?.join("protection/runtime");
                        let ticket_path = PathBuf::from(ticket).canonicalize().map_err(|e|e.to_string())?;
                        let module_path = PathBuf::from(module).canonicalize().map_err(|e|e.to_string())?;
                        let root = protected_root.canonicalize().map_err(|e|e.to_string())?;
                        if !ticket_path.starts_with(&root) || !module_path.starts_with(&root) { return Err("protected child references leave private runtime".into()); }
                        let ticket = nova_protected_loader::read_ticket(&ticket_path).map_err(|e|e.to_string())?;
                        nova_protected_loader::config_for_ticket(&ticket).map_err(|e|e.to_string())?;
                    }
                    (None, None) => {
                        if self.inner.service_protection.lock().unwrap().is_some() { return Err("protected child omitted its signed task".into()); }
                    }
                    _ => return Err("incomplete protected child runtime references".into()),
                }
                self.validate_host_launch_contract(&checked_spec)?;
                let mut launch = application_launch_spec(&supervisor, spec)?;
                if spec.role == HostExecutableRole::NativeViewer && spec.args.iter().any(|arg| arg.starts_with("--nova-render-service-pipe=")) {
                    if let Some(endpoint) = self.inner.native_composition.lock().unwrap().as_ref() {
                        launch.environment.push(("NOVA_NATIVE_PRESENTATION_ENDPOINT".into(),
                            serde_json::to_string(endpoint).map_err(|error| error.to_string())?.into()));
                        // A composition endpoint deliberately has no cross-process NSView
                        // pointer. Keep its process identity separate from that capability.
                        launch.environment.push(("LITEMATICA_NOVA_NATIVE_PARENT_PID".into(),
                            std::process::id().to_string().into()));
        }
        if let Some((hwnd, pid)) = *self.inner.native_parent.lock().unwrap() {
                        launch.environment.push(("LITEMATICA_NOVA_NATIVE_PARENT_HWND".into(), hwnd.to_string().into()));
                        launch.environment.push(("LITEMATICA_NOVA_NATIVE_PARENT_PID".into(), pid.to_string().into()));
                    }
                    if !spec.args.iter().any(|arg| arg == "--nova-gpu-context") {
                    if let Some(epoch_ms) = self.inner.render_click_epoch_ms.lock().unwrap().take() {
                        launch.environment.push(("LITEMATICA_NOVA_CLICK_EPOCH_MS".into(), epoch_ms.to_string().into()));
                    }
                    }
                }
                let result = supervisor
                    .spawn(launch)
                    .map_err(|error| format!("spawn {:?}: {error}", spec.role))?;
                let hosted = HostedChild {
                    role: spec.role,
                    identity: spec.identity.clone(),
                    lease: result.child.clone(),
                    announced: false,
                    pending_exit: None,
                };
                let disposition = match result.disposition {
                    SpawnDisposition::Created => HostSpawnDisposition::Created,
                    SpawnDisposition::Reused => HostSpawnDisposition::Reused,
                };
                let spawned_child = matches!(result.disposition, SpawnDisposition::Created)
                    .then_some(result.child.id);
                if spawned_child.is_some() {
                    self.register_pending_hosted_child(hosted);
                }
                Ok(HostProcessExecution {
                    outcome: HostProcessOutcome::Spawned {
                        pid: result.child.pid,
                        disposition,
                    },
                    spawned_child,
                })
            }
            HostProcessCommand::Terminate { role, identity } => {
                let child = self
                    .inner
                    .hosted_children
                    .lock()
                    .unwrap()
                    .values()
                    .find(|child| child.role == *role && child.identity == *identity)
                    .cloned();
                let Some(child) = child else {
                    return Ok(HostProcessExecution {
                        outcome: HostProcessOutcome::Terminated {
                            already_exited: true,
                        },
                        spawned_child: None,
                    });
                };
                let outcome = match supervisor
                    .terminate(&child.lease, 1)
                    .map_err(|error| format!("terminate {:?}: {error}", child.role))?
                {
                    TerminateOutcome::Requested | TerminateOutcome::AlreadyRequested => {
                        Ok(HostProcessOutcome::Terminated {
                            already_exited: false,
                        })
                    }
                    TerminateOutcome::AlreadyExited => Ok(HostProcessOutcome::Terminated {
                        already_exited: true,
                    }),
                    TerminateOutcome::Stale => Err("stale supervised process lease".to_owned()),
                }?;
                Ok(HostProcessExecution {
                    outcome,
                    spawned_child: None,
                })
            }
        }
    }

    fn register_pending_hosted_child(&self, mut child: HostedChild) {
        let child_id = child.lease.id;
        {
            // Keep this ordering paired with handle_process_exit: an exit is
            // either visible in hosted_children or retained in pending exits.
            let mut hosted_children = self.inner.hosted_children.lock().unwrap();
            let mut pending_exits = self.inner.pending_process_exits.lock().unwrap();
            child.pending_exit = pending_exits.remove(&child_id);
            hosted_children.insert(child_id, child);
        }
    }

    fn announce_hosted_child(&self, child_id: ChildId) {
        let exited = {
            let mut hosted_children = self.inner.hosted_children.lock().unwrap();
            let pending_exit = {
                let Some(child) = hosted_children.get_mut(&child_id) else {
                    return;
                };
                child.announced = true;
                child.pending_exit.take()
            };
            pending_exit.map(|exit| {
                let child = hosted_children.remove(&child_id).unwrap();
                (child, exit)
            })
        };
        if let Some((child, exit)) = exited {
            self.send_hosted_exit(child, exit);
        }
    }

    fn validate_host_launch_contract(&self, spec: &HostLaunchSpec) -> Result<(), String> {
        if matches!(
            spec.role,
            HostExecutableRole::RenderService | HostExecutableRole::CoreHelper
        ) {
            return Err(format!(
                "render-service may not remotely launch {:?}",
                spec.role
            ));
        }
        if !spec.hidden || !spec.new_process_group {
            return Err("render child must use hidden, isolated process-group launch".to_owned());
        }
        let backend_dir = self
            .inner
            .backend_dir
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "backend executable directory is unavailable".to_owned())?;
        if spec
            .current_directory
            .as_deref()
            .is_some_and(|directory| directory != backend_dir)
        {
            return Err(
                "render child working directory must resolve to the backend directory".to_owned(),
            );
        }
        let runtime_root = self
            .inner
            .render_runtime_root
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "render runtime root is unavailable".to_owned())?;
        validate_output_target(&spec.stdout, &runtime_root)?;
        validate_output_target(&spec.stderr, &runtime_root)?;
        validate_render_child_shape(spec, &runtime_root)
    }

    fn handle_process_exit(&self, exit: ProcessExit) {
        let mut service_lease = self.inner.service_lease.lock().unwrap();
        if service_lease
            .as_ref()
            .is_some_and(|lease| lease == &exit.child)
        {
            *service_lease = None;
            drop(service_lease);
            self.contain_hosted_children_after_unexpected_service_loss();
            return;
        }
        let mut hosted_children = self.inner.hosted_children.lock().unwrap();
        if let Some(child) = hosted_children.get_mut(&exit.child.id) {
            if child.lease != exit.child {
                return;
            }
            if !child.announced {
                child.pending_exit = Some(exit);
                return;
            }
            let child = hosted_children.remove(&exit.child.id).unwrap();
            drop(hosted_children);
            drop(service_lease);
            self.send_hosted_exit(child, exit);
        } else if exit.child.role != ExecutableRole::CoreHelper {
            // service_lease and hosted_children stay locked until the exit is
            // retained, closing both registration races.
            self.inner
                .pending_process_exits
                .lock()
                .unwrap()
                .insert(exit.child.id, exit);
        }
    }

    fn send_hosted_exit(&self, child: HostedChild, exit: ProcessExit) {
        let _ = self.send_payload(Payload::HostChildExited(HostChildExited {
            role: child.role,
            identity: child.identity,
            pid: exit.child.pid,
            exit_code: exit.exit_code,
            termination_requested: exit.termination_requested,
        }));
    }

    fn send_payload(&self, payload: Payload) -> Result<(), String> {
        let sequence = self.inner.envelope_sequence.fetch_add(1, Ordering::Relaxed);
        self.inner
            .writer
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| "nova-render-service writer is unavailable".to_owned())?
            .send(Envelope::new(sequence, payload))
            .map_err(|_| "nova-render-service writer is unavailable".to_owned())
    }

    fn apply_event(&self, event: &ServiceEvent) {
        let mut snapshot = self.inner.snapshot.write().unwrap();
        match event {
            ServiceEvent::Snapshot(value) => *snapshot = value.clone(),
            ServiceEvent::BuildState(value) => replace_build(&mut snapshot.builds, value.clone()),
            ServiceEvent::ArtifactState(value) => {
                replace_artifact(&mut snapshot.artifacts, value.clone())
            }
            ServiceEvent::ViewerState(value) => {
                replace_viewer(&mut snapshot.viewers, value.clone())
            }
            ServiceEvent::Error(_) | ServiceEvent::ServiceStopping => {}
        }
    }

    fn disconnect(&self, error: ServiceError) {
        *self.inner.writer.lock().unwrap() = None;
        *self.inner.connection_error.write().unwrap() = Some(error.message.clone());
        self.inner.connected.notify_waiters();
        self.fail_pending(error.clone());
        self.publish_event(ServiceEvent::Error(error));
        self.contain_hosted_children_after_unexpected_service_loss();
    }

    fn contain_hosted_children_after_unexpected_service_loss(&self) {
        if !self.inner.shutdown_requested.load(Ordering::Acquire) {
            self.terminate_hosted_children();
        }
    }

    /// A render-service connection is the logical owner of every hosted render
    /// child. If that authority disappears, the Application Host uses its OS
    /// leases to contain the children without attempting to reinterpret their
    /// render state.
    fn terminate_hosted_children(&self) {
        let children = self
            .inner
            .hosted_children
            .lock()
            .unwrap()
            .values()
            .map(|child| child.lease.clone())
            .collect::<Vec<_>>();
        let supervisor = self.inner.supervisor.lock().unwrap().clone();
        let Some(supervisor) = supervisor else {
            return;
        };
        for child in children {
            let _ = supervisor.terminate(&child, 1);
        }
    }

    fn fail_pending(&self, error: ServiceError) {
        let pending = std::mem::take(&mut *self.inner.pending.lock().unwrap());
        for (_, sender) in pending {
            let _ = sender.send(Err(error.clone()));
        }
    }

    fn publish_event(&self, event: ServiceEvent) {
        self.inner
            .event_subscribers
            .lock()
            .unwrap()
            .retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }
}

fn application_launch_spec(
    supervisor: &ProcessSupervisor,
    spec: &HostLaunchSpec,
) -> Result<LaunchSpec, String> {
    let role = match spec.role {
        HostExecutableRole::BuildWorker => ExecutableRole::BuildWorker,
        HostExecutableRole::NativeViewer => ExecutableRole::NativeViewer,
        HostExecutableRole::RenderService | HostExecutableRole::CoreHelper => {
            return Err(format!("remote launch role {:?} is not allowed", spec.role));
        }
    };
    let identity = LaunchIdentity {
        owner_epoch: crate::platform::process_supervisor::OwnerEpoch::from_raw(
            spec.identity.owner_epoch,
        ),
        generation: spec.identity.generation,
        launch_token: LaunchToken::new(spec.identity.launch_token.clone())
            .map_err(|error| error.to_string())?,
    };
    if identity.owner_epoch != supervisor.owner_epoch() {
        return Err("host launch request uses a stale Application owner epoch".to_owned());
    }
    let mut launch = LaunchSpec::new(role, identity);
    launch.args = spec.args.iter().map(Into::into).collect();
    launch.environment = spec
        .environment
        .iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect();
    launch.current_dir = spec.current_directory.clone();
    launch.stdout = application_output_target(&spec.stdout);
    launch.stderr = application_output_target(&spec.stderr);
    launch.hidden = spec.hidden;
    launch.new_process_group = spec.new_process_group;
    Ok(launch)
}

fn render_service_output_targets(data_root: &Path) -> (OutputTarget, OutputTarget) {
    let logs = crate::application::diagnostics::role_directory("render-service", data_root.join("render-service").join("logs"));
    let append = crate::application::diagnostics::has_session();
    (
        OutputTarget::File {
            path: logs.join("render-service-stdout.log"),
            append,
        },
        OutputTarget::File {
            path: logs.join("render-service-stderr.log"),
            append,
        },
    )
}

fn application_output_target(target: &HostOutputTarget) -> OutputTarget {
    match target {
        HostOutputTarget::Null => OutputTarget::Null,
        HostOutputTarget::File { path, append } => OutputTarget::File {
            path: path.clone(),
            append: *append,
        },
    }
}

fn validate_output_target(target: &HostOutputTarget, runtime_root: &Path) -> Result<(), String> {
    let HostOutputTarget::File { path, .. } = target else {
        return Err("render children must write diagnostics to owned log files".to_owned());
    };
    let logs = crate::application::diagnostics::role_directory("render-service", runtime_root.join("logs"));
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("render child log path contains a traversal component".to_owned());
    }
    if !path.starts_with(&logs) || path.parent() != Some(logs.as_path()) {
        return Err(format!(
            "render child log path must be a direct child of {}",
            logs.display()
        ));
    }
    Ok(())
}

fn validate_render_child_shape(spec: &HostLaunchSpec, runtime_root: &Path) -> Result<(), String> {
    if spec.role == HostExecutableRole::NativeViewer
        && spec.args.len() == 1
        && spec.args[0].starts_with(RESOURCE_DEMAND_ARGUMENT_PREFIX)
    {
        if spec.environment.len() != 1
            || spec.environment.get("LBA_DATA_ROOT").map(PathBuf::from).as_deref()
                != runtime_root.parent()
        {
            return Err("resource demand must use only the Application data root".to_owned());
        }
        let request_path = PathBuf::from(
            spec.args[0].strip_prefix(RESOURCE_DEMAND_ARGUMENT_PREFIX).unwrap(),
        );
        return validate_resource_demand_request(&request_path, runtime_root);
    }
    if spec.role == HostExecutableRole::NativeViewer
        && spec.args.len() == 1
        && spec.args[0].starts_with(RESOURCE_PREFLIGHT_ARGUMENT_PREFIX)
    {
        if !spec.environment.is_empty() {
            return Err("resource preflight Viewer may not inject environment state".to_owned());
        }
        let request_path = spec.args[0]
            .strip_prefix(RESOURCE_PREFLIGHT_ARGUMENT_PREFIX)
            .map(PathBuf::from)
            .ok_or_else(|| "resource preflight request argument is missing".to_owned())?;
        return validate_resource_preflight_request(&request_path, runtime_root);
    }

    if spec.role == HostExecutableRole::NativeViewer
        && spec.args.len() == 1
        && spec.args[0].starts_with("--launch-contract=")
    {
        if !spec.environment.is_empty() {
            return Err("one-shot Viewer preview may not inject environment state".to_owned());
        }
        let contract = spec.args[0]
            .strip_prefix("--launch-contract=")
            .map(PathBuf::from)
            .ok_or_else(|| "Viewer preview launch contract argument is missing".to_owned())?;
        let application_data_root = runtime_root
            .parent()
            .ok_or_else(|| "render runtime root has no Application data parent".to_owned())?;
        let expected_directory = application_data_root.join("cache").join("render");
        if !contract.is_absolute()
            || contract.parent() != Some(expected_directory.as_path())
            || contract.extension().and_then(|value| value.to_str()) != Some("json")
            || contract
                .file_name()
                .and_then(|value| value.to_str())
                .is_none_or(|name| !name.starts_with("litematica_nova_viewer_launch_contract_"))
        {
            return Err(
                "Viewer preview contract is outside the Application render cache".to_owned(),
            );
        }
        return Ok(());
    }

    let allowed_environment: &[&str] = match spec.role {
        HostExecutableRole::BuildWorker => &[
            nova_render_protocol::PIPE_ENV,
            nova_render_protocol::AUTH_TOKEN_ENV,
            nova_render_protocol::CLIENT_ROLE_ENV,
            nova_render_protocol::BUILD_ID_ENV,
            nova_render_protocol::BUILD_GENERATION_ENV,
            "LBA_DATA_ROOT",
        ],
        HostExecutableRole::NativeViewer => &[
            nova_render_protocol::PIPE_ENV,
            nova_render_protocol::AUTH_TOKEN_ENV,
            nova_render_protocol::CLIENT_ROLE_ENV,
            nova_render_protocol::BUILD_ID_ENV,
            nova_render_protocol::BUILD_GENERATION_ENV,
            nova_render_protocol::VIEWER_SESSION_ENV,
            "LBA_DATA_ROOT",
        ],
        HostExecutableRole::RenderService | HostExecutableRole::CoreHelper => unreachable!(),
    };
    if spec.environment.len() != allowed_environment.len()
        || spec
            .environment
            .keys()
            .any(|name| !allowed_environment.contains(&name.as_str()))
    {
        return Err(
            "render child environment does not match its exact launch allowlist".to_owned(),
        );
    }
    if spec
        .environment
        .get("LBA_DATA_ROOT")
        .map(|value| PathBuf::from(value))
        .as_deref()
        != Some(runtime_root)
    {
        return Err("render child LBA_DATA_ROOT does not match its runtime root".to_owned());
    }
    for required in [
        nova_render_protocol::PIPE_ENV,
        nova_render_protocol::AUTH_TOKEN_ENV,
        nova_render_protocol::BUILD_ID_ENV,
    ] {
        if spec
            .environment
            .get(required)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(format!("render child requires a non-empty {required}"));
        }
    }
    let generation = spec
        .environment
        .get(nova_render_protocol::BUILD_GENERATION_ENV)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "render child requires a numeric build generation".to_owned())?;
    if generation != spec.identity.generation {
        return Err("render child generation differs from its supervised lease".to_owned());
    }
    match spec.role {
        HostExecutableRole::BuildWorker => {
            if spec
                .environment
                .get(nova_render_protocol::CLIENT_ROLE_ENV)
                .map(String::as_str)
                != Some("build_worker")
            {
                return Err("build worker launch uses the wrong client role".to_owned());
            }
            let [argument] = spec.args.as_slice() else {
                return Err("build worker launch requires exactly one contract argument".to_owned());
            };
            if let Some(pipe) = argument.strip_prefix("--resident-build-worker=") {
                let data_pipe = spec.environment.get(nova_render_protocol::PIPE_ENV).unwrap();
                if !data_pipe.starts_with(r"\\.\pipe\")
                    || pipe != format!("{data_pipe}-resources-{}", spec.identity.generation)
                    || spec.environment.get(nova_render_protocol::BUILD_ID_ENV) != Some(&spec.identity.child_id)
                {
                    return Err("resident worker control pipe or instance does not match its lease".to_owned());
                }
                return Ok(());
            }
            let path = argument
                .strip_prefix("--launch-contract=")
                .map(PathBuf::from)
                .ok_or_else(|| "build worker launch contract argument is missing".to_owned())?;
            if path.parent() != Some(runtime_root.join("contracts").as_path()) {
                return Err(
                    "build worker contract is outside the render contract directory".to_owned(),
                );
            }
        }
        HostExecutableRole::NativeViewer => {
            if spec
                .environment
                .get(nova_render_protocol::CLIENT_ROLE_ENV)
                .map(String::as_str)
                != Some("viewer")
                || spec
                    .environment
                    .get(nova_render_protocol::VIEWER_SESSION_ENV)
                    .is_none_or(|value| value.trim().is_empty())
            {
                return Err("native Viewer launch uses an invalid Viewer identity".to_owned());
            }
            if spec.args.len() == 3 && spec.args[2] == "--nova-gpu-context" {
                if spec.args[0] != format!("--nova-render-service-pipe={}", spec.environment.get(nova_render_protocol::PIPE_ENV).unwrap())
                    || spec.args[1] != format!("--nova-viewer-session={}", spec.identity.child_id)
                    || spec.environment.get(nova_render_protocol::VIEWER_SESSION_ENV) != Some(&spec.identity.child_id)
                    || spec.environment.get(nova_render_protocol::BUILD_ID_ENV) != Some(&spec.identity.child_id)
                    || !spec.environment.get(nova_render_protocol::PIPE_ENV).unwrap().starts_with(r"\\.\pipe") {
                    return Err("resource-only Viewer arguments differ from its supervised lease".to_owned());
                }
                return Ok(());
            }
            if spec.args.len() != 5
                || !spec.args[0].starts_with("--nova-render-service-pipe=")
                || !spec.args[1].starts_with("--nova-viewer-session=")
                || !spec.args[2].starts_with("--nova-build-id=")
                || !spec.args[3].starts_with("--nova-generation=")
                || spec.args[4] != "--nova-stream-await"
            {
                return Err("native Viewer launch does not match the streaming contract".to_owned());
            }
            let expected = [
                (
                    "--nova-render-service-pipe=",
                    nova_render_protocol::PIPE_ENV,
                ),
                (
                    "--nova-viewer-session=",
                    nova_render_protocol::VIEWER_SESSION_ENV,
                ),
                ("--nova-build-id=", nova_render_protocol::BUILD_ID_ENV),
                (
                    "--nova-generation=",
                    nova_render_protocol::BUILD_GENERATION_ENV,
                ),
            ];
            for (argument, (prefix, environment)) in spec.args[..4].iter().zip(expected) {
                if argument.strip_prefix(prefix)
                    != spec.environment.get(environment).map(String::as_str)
                {
                    return Err(
                        "native Viewer arguments differ from its authenticated environment"
                            .to_owned(),
                    );
                }
            }
        }
        HostExecutableRole::RenderService | HostExecutableRole::CoreHelper => unreachable!(),
    }
    Ok(())
}

const RESOURCE_PREFLIGHT_ARGUMENT_PREFIX: &str = "--nova-resource-preflight=";
const RESOURCE_PREFLIGHT_SCHEMA: u32 = 1;
const RESOURCE_PREFLIGHT_REQUEST_PREFIX: &str = "nova-resource-preflight-request-";
const RESOURCE_PREFLIGHT_RESULT_PREFIX: &str = "nova-resource-preflight-result-";
const RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES: u64 = 64 * 1024;
const RESOURCE_PREFLIGHT_MAX_PATH_UNITS: usize = 4 * 1024;
const RESOURCE_DEMAND_ARGUMENT_PREFIX: &str = "--nova-resource-demand=";
const RESOURCE_DEMAND_REQUEST_PREFIX: &str = "nova-resource-demand-request-";
const RESOURCE_DEMAND_RESULT_PREFIX: &str = "nova-resource-demand-result-";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct HostResourceDemandRequest {
    schema: u32,
    request_id: String,
    launch_contract_path: PathBuf,
    presentation: nova_render_protocol::PresentationSpec,
    selected_backend: HostResourceProbeBackend,
    viewport: [u32; 2],
    result_path: PathBuf,
    device_identity: String,
    #[serde(default)]
    iris_capabilities: Option<serde_json::Value>,
}

fn validate_resource_demand_request(request_path: &Path, runtime_root: &Path) -> Result<(), String> {
    let root = runtime_root.join("status").join("preflight");
    validate_owned_preflight_path(request_path, &root, RESOURCE_DEMAND_REQUEST_PREFIX, "resource demand request")?;
    validate_preflight_file_does_not_escape(request_path, &root)?;
    let metadata = request_path.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES {
        return Err("resource demand request exceeds its bounded file size".to_owned());
    }
    let mut bytes = Vec::new();
    File::open(request_path).map_err(|error| error.to_string())?
        .take(RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES + 1).read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES {
        return Err("resource demand request grew beyond its bound".to_owned());
    }
    let request: HostResourceDemandRequest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse exact resource demand request: {error}"))?;
    validate_preflight_request_id(&request.request_id)?;
    if request.schema != 1 || request.viewport.contains(&0)
        || request.viewport.iter().any(|&value| value > 16_384)
        || request.device_identity.is_empty() || request.device_identity.len() > 1024
        || request_path.file_name().and_then(|value| value.to_str())
            != Some(format!("{RESOURCE_DEMAND_REQUEST_PREFIX}{}.json", request.request_id).as_str())
    {
        return Err("resource demand identity, schema or viewport is invalid".to_owned());
    }
    validate_owned_preflight_path(&request.result_path, &root, RESOURCE_DEMAND_RESULT_PREFIX, "resource demand result")?;
    if request.result_path.exists()
        || request.result_path.file_name().and_then(|value| value.to_str())
            != Some(format!("{RESOURCE_DEMAND_RESULT_PREFIX}{}.json", request.request_id).as_str())
    {
        return Err("resource demand result is stale or belongs to another request".to_owned());
    }
    let contracts = runtime_root.join("contracts");
    if !request.launch_contract_path.is_absolute()
        || request.launch_contract_path != contracts.join(format!("{}.worker.json", request.request_id))
    {
        return Err("resource demand contract is not owned by this request".to_owned());
    }
    let canonical_contract = std::fs::canonicalize(&request.launch_contract_path).map_err(|error| error.to_string())?;
    let canonical_contracts = std::fs::canonicalize(&contracts).map_err(|error| error.to_string())?;
    let canonical_runtime = std::fs::canonicalize(runtime_root).map_err(|error| error.to_string())?;
    if canonical_contracts.parent() != Some(canonical_runtime.as_path())
        || canonical_contract.parent() != Some(canonical_contracts.as_path())
        || !canonical_contract.is_file()
    {
        return Err("resource demand contract resolves outside its owned directory".to_owned());
    }
    match (&request.selected_backend, request.iris_capabilities.as_ref()) {
        (HostResourceProbeBackend::Vulkan, None) => {},
        (HostResourceProbeBackend::IrisGl46, Some(value)) if value.is_object() => {},
        _ => return Err("resource demand must bind an explicitly selected backend and its capabilities".to_owned()),
    }
    let _ = request.presentation; // Typed deserialization preserves the existing Presentation contract.
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HostResourceProbeBackend {
    Auto,
    Vulkan,
    IrisGl46,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HostResourceProbeIrisFallback {
    Fail,
    Vulkan,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostResourceProbeIrisRoute {
    fallback: HostResourceProbeIrisFallback,
    shaderpack_requested: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostResourceProbeVulkanAdapter {
    uuid: Option<String>,
    luid: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostResourceProbeRequest {
    schema: u32,
    request_id: String,
    backend: HostResourceProbeBackend,
    iris_route: HostResourceProbeIrisRoute,
    vulkan_adapter: Option<HostResourceProbeVulkanAdapter>,
    result_path: PathBuf,
}

fn validate_resource_preflight_request(
    request_path: &Path,
    runtime_root: &Path,
) -> Result<(), String> {
    let preflight_root = runtime_root.join("status").join("preflight");
    validate_owned_preflight_path(
        request_path,
        &preflight_root,
        RESOURCE_PREFLIGHT_REQUEST_PREFIX,
        "resource preflight request",
    )?;
    let metadata = request_path
        .metadata()
        .map_err(|error| format!("read resource preflight request metadata: {error}"))?;
    if !metadata.is_file() || metadata.len() > RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES {
        return Err(format!(
            "resource preflight request must be a regular file no larger than {RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES} bytes"
        ));
    }
    validate_preflight_file_does_not_escape(request_path, &preflight_root)?;

    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len().min(RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES)).unwrap_or(0),
    );
    File::open(request_path)
        .map_err(|error| format!("open resource preflight request: {error}"))?
        .take(RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read resource preflight request: {error}"))?;
    if bytes.len() as u64 > RESOURCE_PREFLIGHT_MAX_REQUEST_BYTES {
        return Err("resource preflight request grew beyond its size limit".to_owned());
    }
    let request: HostResourceProbeRequest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse exact resource preflight request: {error}"))?;
    validate_resource_preflight_request_shape(request_path, &preflight_root, &request)
}

fn validate_resource_preflight_request_shape(
    request_path: &Path,
    preflight_root: &Path,
    request: &HostResourceProbeRequest,
) -> Result<(), String> {
    if request.schema != RESOURCE_PREFLIGHT_SCHEMA {
        return Err(format!(
            "unsupported resource preflight request schema {}; expected {RESOURCE_PREFLIGHT_SCHEMA}",
            request.schema
        ));
    }
    validate_preflight_request_id(&request.request_id)?;
    let expected_request_name = format!(
        "{RESOURCE_PREFLIGHT_REQUEST_PREFIX}{}.json",
        request.request_id
    );
    if request_path.file_name().and_then(|value| value.to_str())
        != Some(expected_request_name.as_str())
    {
        return Err("resource preflight request name differs from its request id".to_owned());
    }
    validate_owned_preflight_path(
        &request.result_path,
        preflight_root,
        RESOURCE_PREFLIGHT_RESULT_PREFIX,
        "resource preflight result",
    )?;
    let expected_result_name = format!(
        "{RESOURCE_PREFLIGHT_RESULT_PREFIX}{}.json",
        request.request_id
    );
    if request
        .result_path
        .file_name()
        .and_then(|value| value.to_str())
        != Some(expected_result_name.as_str())
    {
        return Err("resource preflight result name differs from its request id".to_owned());
    }
    if request.result_path.exists() {
        return Err("resource preflight result already exists".to_owned());
    }
    validate_resource_probe_route(request)?;
    validate_resource_probe_adapter(request.vulkan_adapter.as_ref())
}

fn validate_owned_preflight_path(
    path: &Path,
    preflight_root: &Path,
    file_prefix: &str,
    label: &str,
) -> Result<(), String> {
    if !path.is_absolute()
        || path.as_os_str().len() > RESOURCE_PREFLIGHT_MAX_PATH_UNITS
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        || path.parent() != Some(preflight_root)
        || path.extension().and_then(|value| value.to_str()) != Some("json")
        || path
            .file_name()
            .and_then(|value| value.to_str())
            .is_none_or(|name| !name.starts_with(file_prefix))
    {
        return Err(format!(
            "{label} must be a direct JSON child of {}",
            preflight_root.display()
        ));
    }
    Ok(())
}

fn validate_preflight_file_does_not_escape(
    request_path: &Path,
    preflight_root: &Path,
) -> Result<(), String> {
    let runtime_root = preflight_root
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "resource preflight directory has no runtime root".to_owned())?;
    let canonical_runtime_root = std::fs::canonicalize(runtime_root)
        .map_err(|error| format!("resolve render runtime directory: {error}"))?;
    let canonical_preflight_root = std::fs::canonicalize(preflight_root)
        .map_err(|error| format!("resolve resource preflight directory: {error}"))?;
    if canonical_preflight_root.parent().and_then(Path::parent)
        != Some(canonical_runtime_root.as_path())
    {
        return Err("resource preflight directory resolves outside the render runtime".to_owned());
    }
    let canonical_request = std::fs::canonicalize(request_path)
        .map_err(|error| format!("resolve resource preflight request: {error}"))?;
    if canonical_request.parent() != Some(canonical_preflight_root.as_path()) {
        return Err("resource preflight request resolves outside its owned directory".to_owned());
    }
    Ok(())
}

fn validate_preflight_request_id(request_id: &str) -> Result<(), String> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'))
    {
        return Err("resource preflight request id must be a 1..=128 byte ASCII token".to_owned());
    }
    Ok(())
}

fn validate_resource_probe_route(request: &HostResourceProbeRequest) -> Result<(), String> {
    match (
        &request.backend,
        request.iris_route.shaderpack_requested,
        &request.iris_route.fallback,
    ) {
        (HostResourceProbeBackend::Vulkan, true, _) => {
            Err("Vulkan resource preflight may not claim an Iris shaderpack route".to_owned())
        }
        (HostResourceProbeBackend::IrisGl46, false, _) => {
            Err("Iris resource preflight requires a shaderpack route marker".to_owned())
        }
        (
            HostResourceProbeBackend::Auto
            | HostResourceProbeBackend::Vulkan
            | HostResourceProbeBackend::IrisGl46,
            _,
            HostResourceProbeIrisFallback::Fail | HostResourceProbeIrisFallback::Vulkan,
        ) => Ok(()),
    }
}

fn validate_resource_probe_adapter(
    adapter: Option<&HostResourceProbeVulkanAdapter>,
) -> Result<(), String> {
    let Some(adapter) = adapter else {
        return Ok(());
    };
    if adapter.uuid.is_none() && adapter.luid.is_none() {
        return Err("empty Vulkan adapter override is not allowed".to_owned());
    }
    for (label, value, exact_bytes) in [
        ("UUID", adapter.uuid.as_deref(), 16_usize),
        ("LUID", adapter.luid.as_deref(), 8_usize),
    ] {
        let Some(value) = value else {
            continue;
        };
        if value.len() != exact_bytes * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!(
                "resource preflight Vulkan adapter {label} must be exactly {exact_bytes} hexadecimal bytes"
            ));
        }
    }
    Ok(())
}

fn bounded_error_message(error: &str) -> String {
    let value = error.chars().take(1_000).collect::<String>();
    if value.is_empty() {
        "unknown host process error".to_owned()
    } else {
        value
    }
}

fn replace_build(builds: &mut Vec<BuildSnapshot>, value: BuildSnapshot) {
    if let Some(slot) = builds
        .iter_mut()
        .find(|item| item.build_id == value.build_id)
    {
        *slot = value;
    } else {
        builds.push(value);
    }
}

fn replace_artifact(artifacts: &mut Vec<ArtifactSnapshot>, value: ArtifactSnapshot) {
    if let Some(slot) = artifacts.iter_mut().find(|item| {
        item.scene_identity == value.scene_identity && item.generation == value.generation
    }) {
        *slot = value;
    } else {
        artifacts.push(value);
    }
}

fn replace_viewer(viewers: &mut Vec<ViewerSnapshot>, value: ViewerSnapshot) {
    if let Some(slot) = viewers
        .iter_mut()
        .find(|item| item.session_id == value.session_id)
    {
        *slot = value;
    } else {
        viewers.push(value);
    }
}

#[cfg(any(windows, unix))]
fn connect_with_service_startup_retry(pipe_name: &OsStr) -> Result<NamedPipeStream, String> {
    let started = std::time::Instant::now();
    loop {
        match NamedPipeStream::connect(pipe_name) {
            Ok(stream) => return Ok(stream),
            Err(error) if started.elapsed() < std::time::Duration::from_secs(10) => {
                std::thread::sleep(std::time::Duration::from_millis(25));
                let _ = error;
            }
            Err(error) => return Err(format!("connect render service named pipe: {error}")),
        }
    }
}

#[cfg(not(any(windows, unix)))]
fn connect_with_service_startup_retry(_pipe_name: &OsStr) -> Result<NamedPipeStream, String> {
    Err("nova-render-service named pipe transport is currently Windows-only".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_render_protocol::host_supervision::HostLaunchIdentity;
    use nova_render_protocol::{
        AdmittedBuildStart, BuildAccepted, BuildAdmission, BuildStart, PresentationSpec,
        RenderBackend, SceneContent,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn presentation_and_cancel_do_not_wait_for_new_task_grant() {
        let client = RenderServiceClient::default();
        let (writer_tx, writer_rx) = mpsc::channel();
        *client.inner.writer.lock().unwrap() = Some(writer_tx);
        let gate = client.inner.launch_gate.lock().unwrap();
        let responder = client.clone();
        let thread = std::thread::spawn(move || {
            for _ in 0..2 {
                let envelope = writer_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                let Payload::Request(Request { request_id, command, protected_task }) = envelope.payload else { panic!("not a request") };
                assert!(matches!(command, UiCommand::HideViewer { .. } | UiCommand::CancelBuild { .. }));
                assert!(protected_task.is_none());
                responder.inner.pending.lock().unwrap().remove(&request_id).unwrap().send(Ok(ResponseBody::Ack)).unwrap();
            }
        });
        tauri::async_runtime::block_on(async {
            for command in [UiCommand::HideViewer { session_id: "existing".into() }, UiCommand::CancelBuild { build_id: "existing".into() }] {
                assert!(matches!(tokio::time::timeout(Duration::from_secs(2), client.request(command)).await.unwrap().unwrap(), ResponseBody::Ack));
            }
        });
        drop(gate);
        thread.join().unwrap();
    }
    #[test]
    fn safe_controls_before_service_start_return_without_network() {
        let client = RenderServiceClient::default();
        tauri::async_runtime::block_on(async {
            for command in [UiCommand::HideViewer { session_id: "old".into() }, UiCommand::CancelBuild { build_id: "old".into() }, UiCommand::Shutdown] {
                assert!(matches!(client.request(command).await.unwrap(), ResponseBody::Ack));
            }
            assert!(client.request(UiCommand::ShowPopup { session_id: "none".into() }).await.is_err());
        });
    }
    #[test]
    fn connected_client_sends_start_build_and_observes_acceptance() {
        let client = RenderServiceClient::default();
        let (writer_tx, writer_rx) = mpsc::channel();
        *client.inner.writer.lock().unwrap() = Some(writer_tx);

        let start = BuildStart {
            source_path: PathBuf::from(r"C:\projections\probe.litematic"),
            display_mode: "full".to_owned(),
            build_mode: nova_render_protocol::RenderBuildMode::Prebuilt,
            content: SceneContent::default(),
            build_options: BTreeMap::new(),
            presentation: PresentationSpec {
                backend: RenderBackend::Auto,
                shaderpack: None,
                render_settings: BTreeMap::new(),
            },
        };
        let start = AdmittedBuildStart {
            build: start,
            admission: Some(BuildAdmission {
                token: "resource-admission-test".to_owned(),
                resource_warning_confirmed: false,
            }),
        };
        let expected = start.clone();
        let responder = client.clone();
        let response_thread = std::thread::spawn(move || {
            let envelope = writer_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("StartBuild request was not sent to the connected writer");
            let Payload::Request(Request {
                request_id,
                command: UiCommand::StartBuild(actual),
                ..
            }) = envelope.payload
            else {
                panic!("connected client sent a non-StartBuild envelope")
            };
            assert_eq!(actual, expected);
            let response = responder
                .inner
                .pending
                .lock()
                .unwrap()
                .remove(&request_id)
                .expect("StartBuild response channel was not registered");
            response
                .send(Ok(ResponseBody::BuildAccepted(BuildAccepted {
                    build_id: "probe-build".to_owned(),
                    generation: 1,
                    provisional_scene_key: "probe-scene".to_owned(),
                    viewer_session_id: None,
                    attached: false,
                })))
                .expect("StartBuild response receiver closed");
        });

        let response = tauri::async_runtime::block_on(client.request(UiCommand::StartBuild(start)))
            .expect("StartBuild request failed");
        let ResponseBody::BuildAccepted(accepted) = response else {
            panic!("StartBuild returned a non-BuildAccepted response")
        };
        assert_eq!(accepted.build_id, "probe-build");
        assert!(!accepted.attached);
        response_thread.join().unwrap();
    }

    fn host_identity(child_id: &str, generation: u64) -> HostLaunchIdentity {
        HostLaunchIdentity {
            child_id: child_id.to_owned(),
            owner_epoch: 41,
            generation,
            launch_token: format!("launch-{child_id}-{generation}"),
        }
    }

    fn log_target(runtime_root: &Path, name: &str) -> HostOutputTarget {
        HostOutputTarget::File {
            path: runtime_root.join("logs").join(name),
            append: false,
        }
    }

    #[test]
    fn render_service_diagnostics_are_written_beside_worker_and_viewer_logs() {
        let data_root = Path::new(r"C:\Users\viewer\AppData\Local\Litematica-Nova");
        let logs = data_root.join("render-service").join("logs");
        let (stdout, stderr) = render_service_output_targets(data_root);
        assert_eq!(
            stdout,
            OutputTarget::File {
                path: logs.join("render-service-stdout.log"),
                append: false,
            }
        );
        assert_eq!(
            stderr,
            OutputTarget::File {
                path: logs.join("render-service-stderr.log"),
                append: false,
            }
        );
    }

    #[test]
    fn one_shot_viewer_contract_is_narrow_and_carries_no_scene_payload() {
        let runtime_root =
            PathBuf::from(r"C:\Users\viewer\AppData\Local\Litematica-Nova\render-service");
        let contract = runtime_root
            .parent()
            .unwrap()
            .join("cache")
            .join("render")
            .join("litematica_nova_viewer_launch_contract_123_45.json");
        let spec = HostLaunchSpec {
            role: HostExecutableRole::NativeViewer,
            identity: host_identity("preview-9", 9),
            args: vec![format!("--launch-contract={}", contract.display())],
            environment: BTreeMap::new(),
            current_directory: None,
            stdout: log_target(&runtime_root, "preview-9-viewer-stdout.log"),
            stderr: log_target(&runtime_root, "preview-9-viewer-stderr.log"),
            hidden: true,
            new_process_group: true,
        };
        validate_render_child_shape(&spec, &runtime_root).unwrap();

        let mut injected = spec.clone();
        injected
            .environment
            .insert("SCENE_BYTES".to_owned(), "forbidden".to_owned());
        assert!(validate_render_child_shape(&injected, &runtime_root).is_err());

        let mut escaped = spec;
        escaped.args[0] =
            r"--launch-contract=C:\Temp\litematica_nova_viewer_launch_contract_bad.json".to_owned();
        assert!(validate_render_child_shape(&escaped, &runtime_root).is_err());
    }

    #[test]
    fn resource_preflight_viewer_accepts_only_one_owned_bounded_json_request() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let runtime_root = std::env::temp_dir()
            .join(format!("nova-resource-preflight-host-{unique}"))
            .join("render-service");
        let preflight_root = runtime_root.join("status").join("preflight");
        std::fs::create_dir_all(&preflight_root).unwrap();
        std::fs::create_dir_all(runtime_root.join("logs")).unwrap();
        let request_id = "host-shape-7";
        let request_path = preflight_root.join(format!(
            "{RESOURCE_PREFLIGHT_REQUEST_PREFIX}{request_id}.json"
        ));
        let result_path = preflight_root.join(format!(
            "{RESOURCE_PREFLIGHT_RESULT_PREFIX}{request_id}.json"
        ));
        let request = serde_json::json!({
            "schema": RESOURCE_PREFLIGHT_SCHEMA,
            "request_id": request_id,
            "backend": "auto",
            "iris_route": {
                "fallback": "vulkan",
                "shaderpack_requested": false
            },
            "vulkan_adapter": null,
            "result_path": result_path
        });
        std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
        let spec = HostLaunchSpec {
            role: HostExecutableRole::NativeViewer,
            identity: host_identity("resource-preflight-7", 7),
            args: vec![format!(
                "{RESOURCE_PREFLIGHT_ARGUMENT_PREFIX}{}",
                request_path.display()
            )],
            environment: BTreeMap::new(),
            current_directory: None,
            stdout: log_target(&runtime_root, "resource-preflight-7-stdout.log"),
            stderr: log_target(&runtime_root, "resource-preflight-7-stderr.log"),
            hidden: true,
            new_process_group: true,
        };
        validate_render_child_shape(&spec, &runtime_root).unwrap();

        let mut injected = spec.clone();
        injected.environment.insert(
            "SCENE_PATH".to_owned(),
            r"C:\secret\tree.litematic".to_owned(),
        );
        assert!(validate_render_child_shape(&injected, &runtime_root).is_err());

        let mut extra_argument = spec;
        extra_argument.args.push("--nova-stream-await".to_owned());
        assert!(validate_render_child_shape(&extra_argument, &runtime_root).is_err());
        std::fs::remove_dir_all(runtime_root.parent().unwrap()).unwrap();
    }

    #[test]
    fn resource_preflight_host_rejects_unknown_fields_and_result_path_escape() {
        let runtime_root = PathBuf::from(r"C:\Nova\runtime\render-service");
        let preflight_root = runtime_root.join("status").join("preflight");
        let request_path = preflight_root.join("nova-resource-preflight-request-shape-8.json");
        let valid = serde_json::json!({
            "schema": RESOURCE_PREFLIGHT_SCHEMA,
            "request_id": "shape-8",
            "backend": "vulkan",
            "iris_route": {
                "fallback": "fail",
                "shaderpack_requested": false
            },
            "vulkan_adapter": null,
            "result_path": preflight_root.join("nova-resource-preflight-result-shape-8.json")
        });
        let parsed: HostResourceProbeRequest = serde_json::from_value(valid.clone()).unwrap();
        validate_resource_preflight_request_shape(&request_path, &preflight_root, &parsed).unwrap();

        let mut unknown = valid.clone();
        unknown["source_path"] = serde_json::json!(r"C:\secret\tree.litematic");
        assert!(serde_json::from_value::<HostResourceProbeRequest>(unknown).is_err());

        let mut escaped = valid;
        escaped["result_path"] =
            serde_json::json!(r"C:\Temp\nova-resource-preflight-result-shape-8.json");
        let parsed: HostResourceProbeRequest = serde_json::from_value(escaped).unwrap();
        assert!(
            validate_resource_preflight_request_shape(&request_path, &preflight_root, &parsed)
                .is_err()
        );
    }

    #[test]
    fn resource_demand_host_accepts_owned_cpu_request_and_rejects_stale_or_injected_launch() {
        let root = std::env::temp_dir().join(format!("nova-demand-host-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let runtime = root.join("render-service");
        let preflight = runtime.join("status/preflight");
        std::fs::create_dir_all(&preflight).unwrap();
        std::fs::create_dir_all(runtime.join("contracts")).unwrap();
        let contract = runtime.join("contracts/demand-7.worker.json");
        std::fs::write(&contract, b"{}").unwrap();
        let request = preflight.join("nova-resource-demand-request-demand-7.json");
        let result = preflight.join("nova-resource-demand-result-demand-7.json");
        let value = serde_json::json!({"schema":1,"requestId":"demand-7",
            "launchContractPath":contract,"presentation":{"backend":"vulkan","shaderpack":null,"renderSettings":{}},
            "selectedBackend":"vulkan","viewport":[1200,700],"resultPath":result,
            "deviceIdentity":"actual-device-identity","irisCapabilities":null});
        std::fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
        let spec = HostLaunchSpec { role:HostExecutableRole::NativeViewer,
            identity:host_identity("demand-7",7), args:vec![format!("{RESOURCE_DEMAND_ARGUMENT_PREFIX}{}", request.display())],
            environment:BTreeMap::from([("LBA_DATA_ROOT".to_owned(),root.display().to_string())]),
            current_directory:None, stdout:log_target(&runtime,"demand-7-stdout.log"),
            stderr:log_target(&runtime,"demand-7-stderr.log"),hidden:true,new_process_group:false };
        validate_render_child_shape(&spec,&runtime).unwrap();
        let mut injected=spec.clone();
        injected.environment.insert("EXTRA_ENV".to_owned(),"1".to_owned());
        assert!(validate_render_child_shape(&injected,&runtime).is_err());
        std::fs::write(&result,b"stale").unwrap();
        assert!(validate_render_child_shape(&spec,&runtime).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn streaming_viewer_contract_binds_arguments_to_authenticated_environment() {
        let runtime_root = PathBuf::from(r"C:\Nova\runtime\render-service");
        let mut environment = BTreeMap::new();
        environment.insert(
            nova_render_protocol::PIPE_ENV.to_owned(),
            r"\\.\pipe\nova-render".to_owned(),
        );
        environment.insert(
            nova_render_protocol::AUTH_TOKEN_ENV.to_owned(),
            "auth-token".to_owned(),
        );
        environment.insert(
            nova_render_protocol::CLIENT_ROLE_ENV.to_owned(),
            "viewer".to_owned(),
        );
        environment.insert(
            nova_render_protocol::BUILD_ID_ENV.to_owned(),
            "build-3".to_owned(),
        );
        environment.insert(
            nova_render_protocol::BUILD_GENERATION_ENV.to_owned(),
            "3".to_owned(),
        );
        environment.insert(
            nova_render_protocol::VIEWER_SESSION_ENV.to_owned(),
            "viewer-session-3".to_owned(),
        );
        environment.insert(
            "LBA_DATA_ROOT".to_owned(),
            runtime_root.display().to_string(),
        );
        let spec = HostLaunchSpec {
            role: HostExecutableRole::NativeViewer,
            identity: host_identity("viewer-session-3", 3),
            args: vec![
                r"--nova-render-service-pipe=\\.\pipe\nova-render".to_owned(),
                "--nova-viewer-session=viewer-session-3".to_owned(),
                "--nova-build-id=build-3".to_owned(),
                "--nova-generation=3".to_owned(),
                "--nova-stream-await".to_owned(),
            ],
            environment,
            current_directory: Some(PathBuf::from(r"C:\Nova\bin")),
            stdout: log_target(&runtime_root, "viewer-stdout.log"),
            stderr: log_target(&runtime_root, "viewer-stderr.log"),
            hidden: true,
            new_process_group: true,
        };
        validate_render_child_shape(&spec, &runtime_root).unwrap();

        let mut stale = spec;
        stale.args[3] = "--nova-generation=2".to_owned();
        assert!(validate_render_child_shape(&stale, &runtime_root).is_err());
    }

    #[test]
    fn spawn_response_is_published_before_an_early_child_exit() {
        let client = RenderServiceClient::default();
        let (writer, messages) = mpsc::channel();
        *client.inner.writer.lock().unwrap() = Some(writer);
        let host_identity = host_identity("worker-early-exit", 12);
        let lease = ChildLease {
            id: ChildId::from_raw(77),
            role: ExecutableRole::BuildWorker,
            identity: LaunchIdentity {
                owner_epoch: crate::platform::process_supervisor::OwnerEpoch::from_raw(41),
                generation: 12,
                launch_token: LaunchToken::new("worker-early-exit-token").unwrap(),
            },
            pid: 7077,
        };
        client.inner.hosted_children.lock().unwrap().insert(
            lease.id,
            HostedChild {
                role: HostExecutableRole::BuildWorker,
                identity: host_identity.clone(),
                lease: lease.clone(),
                announced: false,
                pending_exit: Some(ProcessExit {
                    child: lease,
                    exit_code: Some(9),
                    termination_requested: false,
                    wait_error: None,
                }),
            },
        );

        client.publish_host_process_execution(
            HostProcessResponse {
                request_id: "spawn-worker-12".to_owned(),
                role: HostExecutableRole::BuildWorker,
                identity: host_identity,
                outcome: HostProcessOutcome::Spawned {
                    pid: 7077,
                    disposition: HostSpawnDisposition::Created,
                },
            },
            Some(ChildId::from_raw(77)),
        );

        assert!(matches!(
            messages.recv().unwrap().payload,
            Payload::HostProcessResponse(_)
        ));
        assert!(matches!(
            messages.recv().unwrap().payload,
            Payload::HostChildExited(_)
        ));
    }
}

#[cfg(test)]
mod resident_launch_tests {
    use super::*;
    use std::collections::BTreeMap;


    #[test]
    fn resource_only_gpu_viewer_launch_retains_exact_identity_and_environment_allowlist() {
        let root = PathBuf::from(r"C:\runtime\render");
        let pipe = r"\\.\pipe\nova-standard-gpu";
        let mut spec = HostLaunchSpec { role: HostExecutableRole::NativeViewer,
            identity: nova_render_protocol::host_supervision::HostLaunchIdentity { child_id: "gpu-context-1".to_owned(),
                owner_epoch: 1, generation: 17, launch_token: "gpu-context-owned-token".to_owned() },
            args: vec![format!("--nova-render-service-pipe={pipe}"), "--nova-viewer-session=gpu-context-1".to_owned(), "--nova-gpu-context".to_owned()],
            environment: BTreeMap::from([(nova_render_protocol::PIPE_ENV.to_owned(), pipe.to_owned()),
                (nova_render_protocol::AUTH_TOKEN_ENV.to_owned(), "gpu-context-local-auth".to_owned()),
                (nova_render_protocol::CLIENT_ROLE_ENV.to_owned(), "viewer".to_owned()),
                (nova_render_protocol::BUILD_ID_ENV.to_owned(), "gpu-context-1".to_owned()),
                (nova_render_protocol::BUILD_GENERATION_ENV.to_owned(), "17".to_owned()),
                (nova_render_protocol::VIEWER_SESSION_ENV.to_owned(), "gpu-context-1".to_owned()),
                ("LBA_DATA_ROOT".to_owned(), root.to_string_lossy().into_owned())]),
            current_directory: None, stdout: HostOutputTarget::Null, stderr: HostOutputTarget::Null,
            hidden: true, new_process_group: true };
        validate_render_child_shape(&spec, &root).unwrap();
        spec.args[1] = "--nova-viewer-session=other".to_owned();
        assert!(validate_render_child_shape(&spec, &root).is_err());
        spec.args[1] = "--nova-viewer-session=gpu-context-1".to_owned();
        spec.environment.insert("UNRELATED_EXECUTABLE".to_owned(), "bad".to_owned());
        assert!(validate_render_child_shape(&spec, &root).is_err());
        spec.environment.remove("UNRELATED_EXECUTABLE");
        spec.environment.insert(nova_render_protocol::BUILD_GENERATION_ENV.to_owned(), "18".to_owned());
        assert!(validate_render_child_shape(&spec, &root).is_err());
    }

    #[test]
    fn resident_worker_launch_keeps_exact_role_pipe_generation_and_environment_allowlist() {
        let root = PathBuf::from(r"C:\Nova 鐢ㄦ埛\render-service");
        let identity = nova_render_protocol::host_supervision::HostLaunchIdentity {
            child_id: "resource-worker-41".to_owned(), owner_epoch: 7,
            generation: 41, launch_token: "resident-launch-test-token".to_owned(),
        };
        let data_pipe = r"\\.\pipe\nova-owned-service";
        let mut spec = HostLaunchSpec { role: HostExecutableRole::BuildWorker, identity,
            args: vec![format!("--resident-build-worker={data_pipe}-resources-41")],
            environment: BTreeMap::from([
                (nova_render_protocol::PIPE_ENV.to_owned(), data_pipe.to_owned()),
                (nova_render_protocol::AUTH_TOKEN_ENV.to_owned(), "resident-launch-local-auth".to_owned()),
                (nova_render_protocol::CLIENT_ROLE_ENV.to_owned(), "build_worker".to_owned()),
                (nova_render_protocol::BUILD_ID_ENV.to_owned(), "resource-worker-41".to_owned()),
                (nova_render_protocol::BUILD_GENERATION_ENV.to_owned(), "41".to_owned()),
                ("LBA_DATA_ROOT".to_owned(), root.to_string_lossy().into_owned()),
            ]), current_directory: None, stdout: HostOutputTarget::Null, stderr: HostOutputTarget::Null,
            hidden: true, new_process_group: true,
        };
        validate_render_child_shape(&spec, &root).unwrap();
        spec.args[0] = format!("--resident-build-worker={data_pipe}-resources-42");
        assert!(validate_render_child_shape(&spec, &root).is_err());
        spec.args[0] = format!("--resident-build-worker={data_pipe}-resources-41");
        spec.environment.insert(nova_render_protocol::BUILD_GENERATION_ENV.to_owned(), "42".to_owned());
        assert!(validate_render_child_shape(&spec, &root).is_err());
        spec.environment.insert(nova_render_protocol::BUILD_GENERATION_ENV.to_owned(), "41".to_owned());
        spec.environment.insert("UNRELATED_EXECUTABLE".to_owned(), "extra".to_owned());
        assert!(validate_render_child_shape(&spec, &root).is_err());
        spec.environment.remove("UNRELATED_EXECUTABLE");
        spec.args[0] = format!("--launch-contract={}", root.join("contracts").join("job.json").display());
        validate_render_child_shape(&spec, &root).unwrap();
    }
}
