//! Supervised persistent Core transport; no statistics or conversion algorithms.
#[path = "../../../../tools/viewer-core/src/data_worker_protocol.rs"]
pub(crate) mod protocol;
use crate::platform::process_supervisor::{
    ChildLease, ExecutableRole, LaunchIdentity, LaunchSpec, LaunchToken, OutputTarget,
    ProcessSupervisor, WaitOutcome,
};
use protocol::{DataKind, DataRequest, DataResponse, DataState};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::{Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

fn configure_data_stream(stream: &TcpStream) -> std::io::Result<()> {
    // Windows inherits FIONBIO from the nonblocking listener. Framed read_exact
    // runs on its own reader thread and must wait through idle/partial frames.
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)
}

struct Shared {
    closed: Mutex<Option<String>>,
    pending: Mutex<BTreeMap<String, mpsc::Sender<Result<Value, String>>>>,
    states: Mutex<BTreeMap<String, DataResponse>>,
    reads: Mutex<std::collections::BTreeSet<String>>,
}
impl Shared {
    fn new() -> Self {
        Self {
            closed: Mutex::new(None),
            pending: Mutex::new(BTreeMap::new()),
            states: Mutex::new(BTreeMap::new()),
            reads: Mutex::new(Default::default()),
        }
    }
    fn is_open(&self) -> bool {
        self.closed.lock().is_ok_and(|closed| closed.is_none())
    }
    fn register(
        &self,
        request: &DataRequest,
        sender: mpsc::Sender<Result<Value, String>>,
    ) -> Result<(), String> {
        // Registration and channel failure share this lock. No request may be
        // inserted after the reader has drained its waiters and exited.
        let closed = self
            .closed
            .lock()
            .map_err(|_| "Data channel state lock failed")?;
        if let Some(error) = closed.as_ref() {
            return Err(error.clone());
        }
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Data operation state lock failed")?;
        if pending.contains_key(&request.operation_id) {
            return Err("Data operation is already active".into());
        }
        pending.insert(request.operation_id.clone(), sender);
        let mut states = self.states.lock().unwrap();
        if states.len() >= 128 {
            states.retain(|id, _| pending.contains_key(id));
        }
        states.insert(
            request.operation_id.clone(),
            DataResponse::state(&request.operation_id, DataState::Pending),
        );
        if !request.kind.writes(&request.input)
        {
            self.reads
                .lock()
                .unwrap()
                .insert(request.operation_id.clone());
        }
        Ok(())
    }
    fn close(&self, error: String) {
        let (error, senders) = {
            let mut closed = self.closed.lock().unwrap();
            let error = closed.get_or_insert(error).clone();
            let senders = std::mem::take(&mut *self.pending.lock().unwrap());
            (error, senders)
        };
        self.reads.lock().unwrap().clear();
        for (id, sender) in senders {
            let mut state = DataResponse::state(&id, DataState::Failed);
            state.error = Some(error.clone());
            self.states.lock().unwrap().insert(id, state);
            let _ = sender.send(Err(error.clone()));
        }
    }
}
struct Client {
    stream: Mutex<TcpStream>,
    shared: Arc<Shared>,
    child: ChildLease,
    supervisor: Arc<ProcessSupervisor>,
    context: String,
}
impl Drop for Client {
    fn drop(&mut self) {
        if let Ok(stream) = self.stream.get_mut() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let _ = self.supervisor.terminate(&self.child, 0);
    }
}
pub(crate) struct DataWorkerHost {
    supervisor: Arc<ProcessSupervisor>,
    data_root: PathBuf,
    backend_dir: PathBuf,
    client: Mutex<Option<Arc<Client>>>,
    next: std::sync::atomic::AtomicU64,
}
impl DataWorkerHost {
    pub fn new(
        supervisor: Arc<ProcessSupervisor>,
        data_root: PathBuf,
        backend_dir: PathBuf,
    ) -> Self {
        Self {
            supervisor,
            data_root,
            backend_dir,
            client: Mutex::new(None),
            next: std::sync::atomic::AtomicU64::new(1),
        }
    }
    fn client(&self) -> Result<Arc<Client>, String> {
        let context = crate::game_data::resolve_context(
            &crate::current_user_config_dir()?.join("game-data"),
            &crate::get_root().join("pack-in/game-data/default"),
        )
        .ok()
        .map(|v| serde_json::to_string(&v))
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
        let mut slot = self
            .client
            .lock()
            .map_err(|_| "Data worker state lock failed")?;
        if let Some(client) = slot.as_ref() {
            if client.context == context
                && client.shared.is_open()
                && matches!(
                    self.supervisor.wait_for_exit(&client.child, Duration::ZERO),
                    WaitOutcome::TimedOut
                )
            {
                return Ok(client.clone());
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("Create Core data channel: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let generation = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).map_err(|e| format!("Create data channel identity: {e}"))?;
        let nonce = nonce.iter().map(|v| format!("{v:02x}")).collect::<String>();
        let mut spec = LaunchSpec::new(
            ExecutableRole::CoreHelper,
            LaunchIdentity {
                owner_epoch: self.supervisor.owner_epoch(),
                generation,
                launch_token: LaunchToken::new(format!(
                    "core-data-{}-{generation}",
                    std::process::id()
                ))
                .map_err(|e| e.to_string())?,
            },
        );
        spec.args = vec!["data-worker".into()];
        spec.current_dir = Some(self.backend_dir.clone());
        spec.environment=vec![("LBA_DATA_ROOT".into(),self.data_root.as_os_str().to_owned()),("NOVA_DATA_WORKER_CONNECT".into(),json!({"address":listener.local_addr().map_err(|e|e.to_string())?.to_string(),"nonce":nonce}).to_string().into())];
        if !context.is_empty() {
            spec.environment
                .push(("NOVA_GAME_DATA_SNAPSHOT".into(), context.clone().into()));
        }
        let path = super::diagnostics::log_path(
            "core",
            "data-worker-stderr.log",
            &self.data_root.join("application/operations"),
        )?;
        spec.stderr = OutputTarget::File { path, append: true };
        let child = self
            .supervisor
            .spawn(spec)
            .map_err(|e| format!("Start Core data worker: {e}"))?
            .child;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if !matches!(
                        self.supervisor
                            .wait_for_exit(&child, Duration::from_millis(20)),
                        WaitOutcome::TimedOut
                    ) {
                        return Err("Core data worker exited before establishing its channel; see the current session log".into());
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        configure_data_stream(&stream).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        let proof: Value =
            protocol::read_frame(&mut stream).map_err(|e| format!("Core data handshake: {e}"))?;
        if proof.as_str() != Some(nonce.as_str()) {
            let _ = self.supervisor.terminate(&child, 1);
            return Err("Core data channel identity mismatch".into());
        }
        stream.set_read_timeout(None).map_err(|e| e.to_string())?;
        let shared = Arc::new(Shared::new());
        let mut reader = stream.try_clone().map_err(|e| e.to_string())?;
        let shared_reader = shared.clone();
        std::thread::spawn(move || {
            loop {
                let response: DataResponse = match protocol::read_frame(&mut reader) {
                    Ok(response) => response,
                    Err(error) => {
                        let message = format!("Core data channel closed: {error}");
                        shared_reader.close(message);
                        break;
                    }
                };
                let status = DataResponse {
                    operation_id: response.operation_id.clone(),
                    state: response.state,
                    progress: response.progress.clone(),
                    result: None,
                    error: response.error.clone(),
                };
                shared_reader
                    .states
                    .lock()
                    .unwrap()
                    .insert(response.operation_id.clone(), status);
                if matches!(
                    response.state,
                    DataState::Ready | DataState::Failed | DataState::Cancelled
                ) {
                    shared_reader
                        .reads
                        .lock()
                        .unwrap()
                        .remove(&response.operation_id);
                    if let Some(sender) = shared_reader
                        .pending
                        .lock()
                        .unwrap()
                        .remove(&response.operation_id)
                    {
                        let _ = sender.send(if response.state == DataState::Ready {
                            response
                                .result
                                .ok_or_else(|| "Core data response has no result".into())
                        } else {
                            Err(response
                                .error
                                .unwrap_or_else(|| "Data operation cancelled".into()))
                        });
                    }
                }
            }
        });
        let client = Arc::new(Client {
            stream: Mutex::new(stream),
            shared,
            child,
            supervisor: self.supervisor.clone(),
            context,
        });
        *slot = Some(client.clone());
        Ok(client)
    }
    pub fn execute(&self, request: DataRequest) -> Result<Value, String> {
        if request.operation_id.is_empty()
            || request.operation_id.len() > 128
            || !request
                .operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("Invalid data operation identity".into());
        }
        let client = self.client()?;
        let (tx, rx) = mpsc::channel();
        client.shared.register(&request, tx)?;
        let result = protocol::write_frame(
            &mut *client
                .stream
                .lock()
                .map_err(|_| "Data transport lock failed")?,
            &request,
        );
        if let Err(error) = result {
            let message = format!("Submit data operation: {error}");
            client.shared.close(message.clone());
            let _ = client.stream.lock().unwrap().shutdown(Shutdown::Both);
            return Err(message);
        }
        rx.recv()
            .map_err(|_| "Core data operation lost its response channel".to_owned())?
    }
    pub fn status(&self, id: &str, cancel: bool) -> Result<Value, String> {
        let client = self
            .client
            .lock()
            .map_err(|_| "Data worker state lock failed")?
            .clone();
        let Some(client) = client else {
            return Ok(json!({"operationId":id,"state":"Cancelled"}));
        };
        if cancel && client.shared.pending.lock().unwrap().contains_key(id) {
            protocol::write_frame(
                &mut *client.stream.lock().unwrap(),
                &DataRequest {
                    operation_id: id.into(),
                    kind: DataKind::Cancel,
                    input: Value::Null,
                },
            )
            .map_err(|e| e.to_string())?;
        }
        let state = client.shared.states.lock().unwrap().get(id).cloned();
        serde_json::to_value(state.unwrap_or_else(|| DataResponse::state(id, DataState::Cancelled)))
            .map_err(|e| e.to_string())
    }
    pub fn cancel_reads(&self) {
        if let Ok(client) = self.client.lock() {
            if let Some(client) = client.as_ref() {
                let ids = client
                    .shared
                    .reads
                    .lock()
                    .unwrap()
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>();
                for id in ids {
                    let _ = protocol::write_frame(
                        &mut *client.stream.lock().unwrap(),
                        &DataRequest {
                            operation_id: id,
                            kind: DataKind::Cancel,
                            input: Value::Null,
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn closed_channel_fails_waiters_and_rejects_new_requests_without_replay() {
        let shared = Shared::new();
        let request = DataRequest {
            operation_id: "saved-task-write".into(),
            kind: DataKind::Stockpile,
            input: json!({"action":"mutate"}),
        };
        let (tx, rx) = mpsc::channel();
        shared.register(&request, tx).unwrap();
        assert!(shared.is_open());
        shared.close("test peer disconnected".into());
        assert_eq!(rx.recv().unwrap(), Err("test peer disconnected".into()));
        assert!(!shared.is_open());
        assert_eq!(
            shared.states.lock().unwrap()[&request.operation_id].state,
            DataState::Failed
        );
        let (tx, _rx) = mpsc::channel();
        assert_eq!(
            shared.register(&request, tx),
            Err("test peer disconnected".into())
        );
        assert!(shared.pending.lock().unwrap().is_empty());
        assert!(shared.reads.lock().unwrap().is_empty());
    }

    #[test]
    fn accepted_channel_reads_delayed_fragmented_frames_after_idle() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            protocol::write_frame(&mut stream, &json!("test-proof")).unwrap();
            ready_tx.send(()).unwrap();
            for index in 0..3 {
                let request: DataRequest = protocol::read_frame(&mut stream).unwrap();
                assert_eq!(request.operation_id, format!("operation-{index}"));
                std::thread::sleep(Duration::from_millis(30));
                let mut response = DataResponse::state(&request.operation_id, DataState::Ready);
                response.result = Some(json!({"index":index}));
                let mut bytes = Vec::new();
                protocol::write_frame(&mut bytes, &response).unwrap();
                for part in [&bytes[..1], &bytes[1..4], &bytes[4..7], &bytes[7..]] {
                    stream.write_all(part).unwrap();
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        // Unix accept does not inherit this flag; exercise the same inherited
        // nonblocking state that Windows supplies from this listener.
        #[cfg(not(windows))]
        stream.set_nonblocking(true).unwrap();
        configure_data_stream(&stream).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let proof: Value = protocol::read_frame(&mut stream).unwrap();
        assert_eq!(proof, json!("test-proof"));
        let mut reader = stream.try_clone().unwrap();
        for index in 0..3 {
            protocol::write_frame(
                &mut stream,
                &DataRequest {
                    operation_id: format!("operation-{index}"),
                    kind: DataKind::Stats,
                    input: json!({}),
                },
            )
            .unwrap();
            let response: DataResponse = protocol::read_frame(&mut reader)
                .expect("accepted data channel must wait for the complete delayed frame");
            assert_eq!(response.result, Some(json!({"index":index})));
        }
        worker.join().unwrap();
        let error = protocol::read_frame::<DataResponse>(&mut reader).unwrap_err();
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
        ));
    }
}
