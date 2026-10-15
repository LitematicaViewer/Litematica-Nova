//! OS-only ownership for Nova child processes.
//!
//! This module deliberately knows nothing about render phases, artifacts, cache
//! validity, or scheduling. Callers choose *when* a role is needed; the
//! supervisor only validates the executable role and owns the resulting OS
//! process until it has been reaped.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, TerminateJobObject,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, GetExitCodeProcess, ResumeThread, TerminateProcess,
    WaitForSingleObject, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
};

const COMPLETED_HISTORY_LIMIT: usize = 256;
const MAX_LAUNCH_TOKEN_BYTES: usize = 512;
const TERMINATE_EXIT_RACE_WAIT: Duration = Duration::from_millis(250);

/// Executable identities the application is allowed to create.
///
/// Presentation mode does not affect this identity: interactive and preview
/// viewers are both [`ExecutableRole::NativeViewer`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ExecutableRole {
    RenderService,
    BuildWorker,
    NativeViewer,
    CoreHelper,
}

/// Exact, application-resolved executable paths indexed by role.
///
/// A launch request never carries an executable path, which prevents a render
/// peer from turning the host supervisor into a general-purpose process API.
#[derive(Clone, Debug, Default)]
pub(crate) struct ExecutableAllowlist {
    paths: HashMap<ExecutableRole, PathBuf>,
}

impl ExecutableAllowlist {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn allow(
        &mut self,
        role: ExecutableRole,
        executable: impl AsRef<Path>,
    ) -> io::Result<()> {
        if self.paths.contains_key(&role) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("an executable is already registered for {role:?}"),
            ));
        }
        let executable = fs::canonicalize(executable.as_ref())?;
        if !executable.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("allowed executable is not a file: {}", executable.display()),
            ));
        }
        self.paths.insert(role, executable);
        Ok(())
    }

    fn executable(&self, role: ExecutableRole) -> io::Result<&Path> {
        self.paths.get(&role).map(PathBuf::as_path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("executable role is not allowed: {role:?}"),
            )
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct OwnerEpoch(u64);

impl OwnerEpoch {
    pub(crate) fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub(crate) struct LaunchToken(String);

impl LaunchToken {
    pub(crate) fn new(value: impl Into<String>) -> io::Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_LAUNCH_TOKEN_BYTES || value.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "launch token must be non-empty, NUL-free, and at most 512 bytes",
            ));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LaunchToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LaunchToken([redacted])")
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LaunchIdentity {
    pub(crate) owner_epoch: OwnerEpoch,
    pub(crate) generation: u64,
    pub(crate) launch_token: LaunchToken,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ChildId(u64);

impl ChildId {
    pub(crate) fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChildLease {
    pub(crate) id: ChildId,
    pub(crate) role: ExecutableRole,
    pub(crate) identity: LaunchIdentity,
    pub(crate) pid: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OutputTarget {
    Null,
    File { path: PathBuf, append: bool },
}

/// An OS launch request. Arguments and environment are opaque to this module.
/// The executable itself is selected exclusively through the allowlist.
#[derive(Clone)]
pub(crate) struct LaunchSpec {
    pub(crate) role: ExecutableRole,
    pub(crate) identity: LaunchIdentity,
    pub(crate) args: Vec<OsString>,
    pub(crate) environment: Vec<(OsString, OsString)>,
    pub(crate) current_dir: Option<PathBuf>,
    pub(crate) stdout: OutputTarget,
    pub(crate) stderr: OutputTarget,
    pub(crate) hidden: bool,
    pub(crate) new_process_group: bool,
}

impl LaunchSpec {
    pub(crate) fn new(role: ExecutableRole, identity: LaunchIdentity) -> Self {
        Self {
            role,
            identity,
            args: Vec::new(),
            environment: Vec::new(),
            current_dir: None,
            stdout: OutputTarget::Null,
            stderr: OutputTarget::Null,
            hidden: true,
            new_process_group: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpawnDisposition {
    Created,
    Reused,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpawnResult {
    pub(crate) child: ChildLease,
    pub(crate) disposition: SpawnDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessExit {
    pub(crate) child: ChildLease,
    pub(crate) exit_code: Option<i32>,
    pub(crate) termination_requested: bool,
    pub(crate) wait_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProcessEvent {
    Exited(ProcessExit),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminateOutcome {
    Requested,
    AlreadyRequested,
    AlreadyExited,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WaitOutcome {
    Exited(ProcessExit),
    TimedOut,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShutdownReport {
    pub(crate) graceful_exits: usize,
    pub(crate) terminate_requests: usize,
    pub(crate) remaining: Vec<ChildLease>,
    pub(crate) elapsed_ms: u128,
    pub(crate) graceful_wait_ms: u128,
    pub(crate) forced_wait_ms: u128,
}

struct ChildRecord {
    lease: ChildLease,
    child: Arc<Mutex<SupervisedProcess>>,
    termination_requested: bool,
}

#[cfg(windows)]
struct SupervisedProcess {
    process: OwnedHandle,
    pid: u32,
}

#[cfg(unix)]
struct SupervisedProcess {
    child: std::process::Child,
    pid: u32,
    group_live: bool,
}

#[cfg(unix)]
impl SupervisedProcess {
    fn signal_group(&self, signal: i32) -> io::Result<()> {
        if !self.group_live { return Ok(()); }
        // The group is created by Command::process_group(0), never obtained
        // from a peer, external PID lookup, or the parent's process group.
        let group = libc::pid_t::try_from(self.pid)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid owned process group"))?;
        if group <= 1 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid owned process group")); }
        if unsafe { libc::kill(-group, signal) } == 0 { return Ok(()); }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) { Ok(()) } else { Err(error) }
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        use std::os::unix::process::ExitStatusExt;
        let Some(status) = self.child.try_wait()? else { return Ok(None); };
        if self.group_live {
            // A normally exited leader must not leave its own helpers running.
            self.signal_group(libc::SIGKILL)?;
            self.group_live = false;
        }
        Ok(Some(status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))))
    }

    fn terminate(&mut self, _exit_code: u32) -> io::Result<()> {
        self.signal_group(libc::SIGTERM)?;
        if !self.wait_for_exit(Duration::from_millis(250))? {
            self.signal_group(libc::SIGKILL)?;
        }
        Ok(())
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> io::Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.try_wait()?.is_some() { return Ok(true); }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Ok(false); }
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
}

#[cfg(unix)]
impl Drop for SupervisedProcess {
    fn drop(&mut self) {
        if self.group_live {
            let _ = self.signal_group(libc::SIGKILL);
            let _ = self.wait_for_exit(Duration::from_secs(1));
        }
    }
}

#[cfg(not(any(windows, unix)))]
struct SupervisedProcess {
    pid: u32,
}

#[cfg(not(any(windows, unix)))]
impl SupervisedProcess {
    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Nova process supervision requires Windows Job Objects",
        ))
    }

    fn terminate(&mut self, _exit_code: u32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Nova process supervision requires Windows Job Objects",
        ))
    }

    fn wait_for_exit(&mut self, _timeout: Duration) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Nova process supervision requires Windows Job Objects",
        ))
    }
}

#[derive(Default)]
struct ProcessTable {
    accepting: bool,
    admission_pauses: usize,
    active: HashMap<ChildId, ChildRecord>,
    completed: VecDeque<ProcessExit>,
}

struct SharedState {
    table: Mutex<ProcessTable>,
    changed: Condvar,
    events: mpsc::Sender<ProcessEvent>,
}

/// A reversible update barrier. Dropping it never reopens a closing application.
pub(crate) struct AdmissionPause(Arc<SharedState>);

impl Drop for AdmissionPause {
    fn drop(&mut self) {
        let mut table = self.0.table.lock().unwrap();
        table.admission_pauses = table.admission_pauses.saturating_sub(1);
    }
}

/// Sole owner of Nova child OS handles in the application process.
///
/// On Windows, dropping this value closes the application Job Object. The job
/// is configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so the kernel
/// remains the final orphan-process backstop even if graceful shutdown fails.
pub(crate) struct ProcessSupervisor {
    allowlist: ExecutableAllowlist,
    owner_epoch: OwnerEpoch,
    next_child_id: AtomicU64,
    shared: Arc<SharedState>,
    #[cfg(windows)]
    job: OwnedHandle,
}

impl ProcessSupervisor {
    pub(crate) fn new(
        allowlist: ExecutableAllowlist,
    ) -> io::Result<(Self, mpsc::Receiver<ProcessEvent>)> {
        #[cfg(not(any(windows, unix)))]
        {
            let _ = allowlist;
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Nova process supervision requires Windows Job Objects",
            ));
        }

        #[cfg(windows)]
        {
            let job = create_application_job()?;
            let (events, receiver) = mpsc::channel();
            let mut table = ProcessTable::default();
            table.accepting = true;
            let supervisor = Self {
                allowlist,
                owner_epoch: fresh_owner_epoch(),
                next_child_id: AtomicU64::new(1),
                shared: Arc::new(SharedState {
                    table: Mutex::new(table),
                    changed: Condvar::new(),
                    events,
                }),
                job,
            };
            Ok((supervisor, receiver))
        }

        #[cfg(unix)]
        {
            let (events, receiver) = mpsc::channel();
            let mut table = ProcessTable::default();
            table.accepting = true;
            Ok((Self {
                allowlist,
                owner_epoch: fresh_owner_epoch(),
                next_child_id: AtomicU64::new(1),
                shared: Arc::new(SharedState {
                    table: Mutex::new(table),
                    changed: Condvar::new(),
                    events,
                }),
            }, receiver))
        }
    }

    pub(crate) fn owner_epoch(&self) -> OwnerEpoch {
        self.owner_epoch
    }

    pub(crate) fn spawn(&self, spec: LaunchSpec) -> io::Result<SpawnResult> {
        validate_launch(self.owner_epoch, &spec)?;
        let executable = self.allowlist.executable(spec.role)?;

        // Serialize admission, creation, Job assignment, and table insertion
        // against shutdown. This also makes identical launch tokens idempotent.
        let mut table = self.shared.table.lock().unwrap();
        if !table.accepting || table.admission_pauses > 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "process supervisor is shutting down",
            ));
        }
        if let Some(record) = table
            .active
            .values()
            .find(|record| record.lease.identity.launch_token == spec.identity.launch_token)
        {
            if record.lease.role == spec.role && record.lease.identity == spec.identity {
                return Ok(SpawnResult {
                    child: record.lease.clone(),
                    disposition: SpawnDisposition::Reused,
                });
            }
            return Err(stale_token_error());
        }
        if table
            .completed
            .iter()
            .any(|exit| exit.child.identity.launch_token == spec.identity.launch_token)
        {
            return Err(stale_token_error());
        }

        #[cfg(windows)]
        let child = spawn_suspended_in_job(&self.job, executable, &spec)?;
        #[cfg(unix)]
        let child = spawn_in_owned_group(executable, &spec)?;
        #[cfg(not(any(windows, unix)))]
        let child = {
            let _ = executable;
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Nova process supervision requires Windows Job Objects",
            ));
        };

        let id = ChildId(self.next_child_id.fetch_add(1, Ordering::Relaxed));
        let lease = ChildLease {
            id,
            role: spec.role,
            identity: spec.identity,
            pid: child.pid,
        };
        let child = Arc::new(Mutex::new(child));
        table.active.insert(
            id,
            ChildRecord {
                lease: lease.clone(),
                child: child.clone(),
                termination_requested: false,
            },
        );

        let shared = self.shared.clone();
        let watcher_lease = lease.clone();
        let watcher = std::thread::Builder::new()
            .name(format!("nova-child-reaper-{}", id.get()))
            .spawn(move || watch_child(shared, watcher_lease, child));
        if let Err(error) = watcher {
            if let Some(record) = table.active.remove(&id) {
                let mut child = record.child.lock().unwrap();
                let _ = child.terminate(1);
                let _ = child.wait_for_exit(Duration::from_secs(2));
            }
            return Err(io::Error::new(
                error.kind(),
                format!("start child reaper for {}: {error}", lease.pid),
            ));
        }
        drop(table);

        Ok(SpawnResult {
            child: lease,
            disposition: SpawnDisposition::Created,
        })
    }

    pub(crate) fn active_children(&self) -> Vec<ChildLease> {
        let mut children = self
            .shared
            .table
            .lock()
            .unwrap()
            .active
            .values()
            .map(|record| record.lease.clone())
            .collect::<Vec<_>>();
        children.sort_by_key(|child| child.id);
        children
    }

    pub(crate) fn pause_admitting(&self) -> AdmissionPause {
        self.shared.table.lock().unwrap().admission_pauses += 1;
        AdmissionPause(Arc::clone(&self.shared))
    }

    /// Stop accepting launches. Callers should send their role-specific
    /// graceful shutdown transports after this method and before [`shutdown`].
    pub(crate) fn stop_admitting(&self) -> Vec<ChildLease> {
        let mut table = self.shared.table.lock().unwrap();
        table.accepting = false;
        let mut children = table
            .active
            .values()
            .map(|record| record.lease.clone())
            .collect::<Vec<_>>();
        children.sort_by_key(|child| child.id);
        children
    }

    pub(crate) fn terminate(
        &self,
        lease: &ChildLease,
        exit_code: u32,
    ) -> io::Result<TerminateOutcome> {
        let child = {
            let table = self.shared.table.lock().unwrap();
            let Some(record) = table.active.get(&lease.id) else {
                return Ok(if completed_matches(&table, lease) {
                    TerminateOutcome::AlreadyExited
                } else {
                    TerminateOutcome::Stale
                });
            };
            if record.lease != *lease {
                return Ok(TerminateOutcome::Stale);
            }
            if record.termination_requested {
                return Ok(TerminateOutcome::AlreadyRequested);
            }
            record.child.clone()
        };

        let mut child = child.lock().unwrap();
        {
            let table = self.shared.table.lock().unwrap();
            let Some(record) = table.active.get(&lease.id) else {
                return Ok(if completed_matches(&table, lease) {
                    TerminateOutcome::AlreadyExited
                } else {
                    TerminateOutcome::Stale
                });
            };
            if record.lease != *lease {
                return Ok(TerminateOutcome::Stale);
            }
            if record.termination_requested {
                return Ok(TerminateOutcome::AlreadyRequested);
            }
        }
        if child.try_wait()?.is_some() {
            return Ok(TerminateOutcome::AlreadyExited);
        }
        if let Err(error) = child.terminate(exit_code) {
            // A process may signal between the zero-time wait and
            // TerminateProcess. Windows can then reject termination even
            // though this exact HANDLE is simply finishing its natural exit.
            // Give that HANDLE a short bounded interval to become signaled;
            // do not special-case an OS error or reopen the process by PID.
            if matches!(child.wait_for_exit(TERMINATE_EXIT_RACE_WAIT), Ok(true)) {
                return Ok(TerminateOutcome::AlreadyExited);
            }
            return Err(error);
        }
        let mut table = self.shared.table.lock().unwrap();
        let Some(record) = table.active.get_mut(&lease.id) else {
            return Ok(if completed_matches(&table, lease) {
                TerminateOutcome::AlreadyExited
            } else {
                TerminateOutcome::Stale
            });
        };
        if record.lease != *lease {
            return Ok(TerminateOutcome::Stale);
        }
        record.termination_requested = true;
        Ok(TerminateOutcome::Requested)
    }

    pub(crate) fn wait_for_exit(&self, lease: &ChildLease, timeout: Duration) -> WaitOutcome {
        let deadline = Instant::now() + timeout;
        let mut table = self.shared.table.lock().unwrap();
        loop {
            if let Some(exit) = table.completed.iter().find(|exit| exit.child == *lease) {
                return WaitOutcome::Exited(exit.clone());
            }
            match table.active.get(&lease.id) {
                Some(record) if record.lease == *lease => {}
                _ => return WaitOutcome::Stale,
            }
            let now = Instant::now();
            if now >= deadline {
                return WaitOutcome::TimedOut;
            }
            let (next, result) = self
                .shared
                .changed
                .wait_timeout(table, deadline.saturating_duration_since(now))
                .unwrap();
            table = next;
            if result.timed_out() {
                return WaitOutcome::TimedOut;
            }
        }
    }

    /// Request cancellation concurrently. A natural-exit race on one child
    /// must not impose its bounded wait separately on all later children.
    pub(crate) fn terminate_many(&self, children: &[ChildLease], exit_code: u32) -> usize {
        std::thread::scope(|scope| {
            let workers = children.iter().map(|child| scope.spawn(move || {
                matches!(self.terminate(child, exit_code), Ok(TerminateOutcome::Requested | TerminateOutcome::AlreadyRequested))
            })).collect::<Vec<_>>();
            workers.into_iter().map(|worker| worker.join().unwrap_or(false)).filter(|requested| *requested).count()
        })
    }

    /// Complete bounded application shutdown after graceful transports were
    /// requested by the owning Application Host.
    pub(crate) fn shutdown(
        &self,
        graceful_wait: Duration,
        forced_wait: Duration,
    ) -> ShutdownReport {
        let started = Instant::now();
        let initially_active = self.stop_admitting().len();
        wait_until_empty(&self.shared, graceful_wait);
        let graceful_wait_ms = started.elapsed().as_millis();
        let remaining_before_terminate = self.active_children();
        let graceful_exits = initially_active.saturating_sub(remaining_before_terminate.len());
        let forced = Instant::now();
        let terminate_requests = self.terminate_many(&remaining_before_terminate, 1);
        wait_until_empty(&self.shared, forced_wait);
        #[cfg(windows)]
        if !self.active_children().is_empty() {
            // Owned descendants are in this exact Job from suspended birth.
            // This is a final fallback only after the Host has drained writes;
            // never reopen a PID or target a process outside Nova's Job.
            unsafe { TerminateJobObject(self.job.as_raw_handle() as HANDLE, 1); }
            wait_until_empty(&self.shared, Duration::from_millis(500));
        }
        ShutdownReport {
            graceful_exits,
            terminate_requests,
            remaining: self.active_children(),
            elapsed_ms: started.elapsed().as_millis(),
            graceful_wait_ms,
            forced_wait_ms: forced.elapsed().as_millis(),
        }
    }
}

#[cfg(unix)]
impl Drop for ProcessSupervisor {
    fn drop(&mut self) {
        // Graceful protocol shutdown is owned by Application Host. This is its
        // final bounded normal-exit/unwind backstop, not a SIGKILL guarantee.
        let _ = self.shutdown(Duration::ZERO, Duration::from_secs(2));
    }
}

#[cfg(unix)]
fn spawn_in_owned_group(executable: &Path, spec: &LaunchSpec) -> io::Result<SupervisedProcess> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut command = Command::new(executable);
    command.args(&spec.args).envs(spec.environment.iter().cloned())
        .current_dir(spec.current_dir.as_deref().or_else(|| executable.parent()).unwrap_or_else(|| Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::from(open_output(&spec.stdout)?))
        .stderr(Stdio::from(open_output(&spec.stderr)?))
        // All owned children need a group, even when a Windows caller does not
        // request CREATE_NEW_PROCESS_GROUP (that flag has different semantics).
        .process_group(0);
    let child = command.spawn()?;
    let pid = child.id();
    Ok(SupervisedProcess { child, pid, group_live: true })
}

fn validate_launch(owner_epoch: OwnerEpoch, spec: &LaunchSpec) -> io::Result<()> {
    if spec.identity.owner_epoch != owner_epoch {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "launch request belongs to a stale application owner epoch",
        ));
    }
    if let Some(current_dir) = &spec.current_dir {
        if !current_dir.is_absolute() || !current_dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "child working directory must be an existing absolute directory: {}",
                    current_dir.display()
                ),
            ));
        }
    }
    for (name, value) in &spec.environment {
        if name.is_empty() || os_contains_nul(name) || os_contains_nul(value) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "child environment contains an empty name or embedded NUL",
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
impl SupervisedProcess {
    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle() as HANDLE, 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut exit_code = 0;
                if unsafe {
                    GetExitCodeProcess(self.process.as_raw_handle() as HANDLE, &mut exit_code)
                } == 0
                {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(Some(exit_code as i32))
                }
            }
            WAIT_FAILED => Err(io::Error::last_os_error()),
            result => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("unexpected process wait result: {result}"),
            )),
        }
    }

    fn terminate(&mut self, exit_code: u32) -> io::Result<()> {
        if unsafe { TerminateProcess(self.process.as_raw_handle() as HANDLE, exit_code) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> io::Result<bool> {
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        match unsafe { WaitForSingleObject(self.process.as_raw_handle() as HANDLE, timeout_ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            result => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("unexpected process wait result: {result}"),
            )),
        }
    }
}

/// Create a process suspended, attach it to the one application Job Object,
/// and only then allow its primary thread to execute. A child can therefore
/// never escape the Job by spawning descendants in a pre-assignment window.
#[cfg(windows)]
fn spawn_suspended_in_job(
    job: &OwnedHandle,
    executable: &Path,
    spec: &LaunchSpec,
) -> io::Result<SupervisedProcess> {
    let mut executable_wide = wide_nul(executable.as_os_str(), "executable path")?;
    let mut command_line = windows_command_line(executable.as_os_str(), &spec.args)?;
    let environment = windows_environment_block(&spec.environment)?;
    let current_dir = spec
        .current_dir
        .as_deref()
        .or_else(|| executable.parent())
        .unwrap_or_else(|| Path::new("."));
    let current_dir_wide = wide_nul(current_dir.as_os_str(), "working directory")?;

    let stdin = inheritable_file(&open_null(true)?)?;
    let stdout = inheritable_file(&open_output(&spec.stdout)?)?;
    let stderr = inheritable_file(&open_output(&spec.stderr)?)?;
    let mut startup = unsafe { std::mem::zeroed::<STARTUPINFOW>() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = stdin.as_raw_handle() as HANDLE;
    startup.hStdOutput = stdout.as_raw_handle() as HANDLE;
    startup.hStdError = stderr.as_raw_handle() as HANDLE;
    let mut process_info = unsafe { std::mem::zeroed::<PROCESS_INFORMATION>() };
    let mut creation_flags = CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT;
    if spec.hidden {
        creation_flags |= CREATE_NO_WINDOW;
    }
    if spec.new_process_group {
        creation_flags |= CREATE_NEW_PROCESS_GROUP;
    }

    let created = unsafe {
        CreateProcessW(
            executable_wide.as_mut_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            creation_flags,
            environment.as_ptr().cast::<std::ffi::c_void>(),
            current_dir_wide.as_ptr(),
            &startup,
            &mut process_info,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: CreateProcessW returned two new owned handles.
    let process = unsafe { OwnedHandle::from_raw_handle(process_info.hProcess) };
    let primary_thread = unsafe { OwnedHandle::from_raw_handle(process_info.hThread) };
    if unsafe {
        AssignProcessToJobObject(
            job.as_raw_handle() as HANDLE,
            process.as_raw_handle() as HANDLE,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        terminate_unstarted_process(&process);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "assign suspended process {} to the application Job Object: {error}",
                process_info.dwProcessId
            ),
        ));
    }

    if unsafe { ResumeThread(primary_thread.as_raw_handle() as HANDLE) } == u32::MAX {
        let error = io::Error::last_os_error();
        terminate_unstarted_process(&process);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "resume supervised process {}: {error}",
                process_info.dwProcessId
            ),
        ));
    }
    drop(primary_thread);

    Ok(SupervisedProcess {
        process,
        pid: process_info.dwProcessId,
    })
}

#[cfg(windows)]
fn terminate_unstarted_process(process: &OwnedHandle) {
    unsafe {
        TerminateProcess(process.as_raw_handle() as HANDLE, 1);
        WaitForSingleObject(process.as_raw_handle() as HANDLE, 2_000);
    }
}

#[cfg(windows)]
fn open_null(read: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(read).write(!read);
    options.open("NUL")
}

#[cfg(unix)]
fn open_null(read: bool) -> io::Result<File> {
    OpenOptions::new().read(read).write(!read).open("/dev/null")
}

#[cfg(any(windows, unix))]
fn open_output(target: &OutputTarget) -> io::Result<File> {
    match target {
        OutputTarget::Null => open_null(false),
        OutputTarget::File { path, append } => {
            if !path.is_absolute() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("child log path must be absolute: {}", path.display()),
                ));
            }
            let mut options = OpenOptions::new();
            options.create(true).write(true);
            if *append {
                options.append(true);
            } else {
                options.truncate(true);
            }
            options.open(path)
        }
    }
}

#[cfg(windows)]
fn inheritable_file(file: &File) -> io::Result<OwnedHandle> {
    let current_process = unsafe { GetCurrentProcess() };
    let mut duplicate: HANDLE = std::ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            current_process,
            file.as_raw_handle() as HANDLE,
            current_process,
            &mut duplicate,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: DuplicateHandle returned a new owned HANDLE.
    Ok(unsafe { OwnedHandle::from_raw_handle(duplicate) })
}

#[cfg(windows)]
fn wide_nul(value: &OsStr, label: &str) -> io::Result<Vec<u16>> {
    let mut wide = value.encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} contains an embedded NUL"),
        ));
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(windows)]
fn windows_command_line(executable: &OsStr, args: &[OsString]) -> io::Result<Vec<u16>> {
    let mut command_line = Vec::new();
    append_windows_argument(&mut command_line, executable)?;
    for arg in args {
        command_line.push(b' ' as u16);
        append_windows_argument(&mut command_line, arg)?;
    }
    if command_line.len() >= 32_767 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows child command line exceeds 32766 UTF-16 code units",
        ));
    }
    command_line.push(0);
    Ok(command_line)
}

#[cfg(windows)]
fn append_windows_argument(output: &mut Vec<u16>, value: &OsStr) -> io::Result<()> {
    let value = value.encode_wide().collect::<Vec<_>>();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "child argument contains an embedded NUL",
        ));
    }
    let quoted = value.is_empty() || value.iter().any(|unit| matches!(*unit, 0x20 | 0x09 | 0x22));
    if !quoted {
        output.extend(value);
        return Ok(());
    }

    output.push(b'"' as u16);
    let mut backslashes = 0;
    for unit in value {
        if unit == b'\\' as u16 {
            backslashes += 1;
        } else if unit == b'"' as u16 {
            output.extend(std::iter::repeat(b'\\' as u16).take(backslashes * 2 + 1));
            output.push(unit);
            backslashes = 0;
        } else {
            output.extend(std::iter::repeat(b'\\' as u16).take(backslashes));
            output.push(unit);
            backslashes = 0;
        }
    }
    output.extend(std::iter::repeat(b'\\' as u16).take(backslashes * 2));
    output.push(b'"' as u16);
    Ok(())
}

#[cfg(windows)]
fn windows_environment_block(overrides: &[(OsString, OsString)]) -> io::Result<Vec<u16>> {
    let mut variables = BTreeMap::<String, (OsString, OsString)>::new();
    for (name, value) in std::env::vars_os() {
        if os_contains_nul(&name) || os_contains_nul(&value) {
            continue;
        }
        variables.insert(name.to_string_lossy().to_lowercase(), (name, value));
    }
    for (name, value) in overrides {
        variables.insert(
            name.to_string_lossy().to_lowercase(),
            (name.clone(), value.clone()),
        );
    }

    let mut block = Vec::new();
    for (_, (name, value)) in variables {
        block.extend(name.encode_wide());
        block.push(b'=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(block)
}

fn watch_child(shared: Arc<SharedState>, lease: ChildLease, child: Arc<Mutex<SupervisedProcess>>) {
    let (exit_code, wait_error) = loop {
        let result = child.lock().unwrap().try_wait();
        match result {
            Ok(Some(status)) => break (Some(status), None),
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => break (None, Some(error.to_string())),
        }
    };

    let exit = {
        let mut table = shared.table.lock().unwrap();
        let termination_requested = table
            .active
            .get(&lease.id)
            .filter(|record| record.lease == lease)
            .map(|record| record.termination_requested)
            .unwrap_or(false);
        table.active.remove(&lease.id);
        let exit = ProcessExit {
            child: lease,
            exit_code,
            termination_requested,
            wait_error,
        };
        table.completed.push_back(exit.clone());
        while table.completed.len() > COMPLETED_HISTORY_LIMIT {
            table.completed.pop_front();
        }
        shared.changed.notify_all();
        exit
    };
    let _ = shared.events.send(ProcessEvent::Exited(exit));
}

fn wait_until_empty(shared: &SharedState, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let mut table = shared.table.lock().unwrap();
    while !table.active.is_empty() {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let (next, result) = shared
            .changed
            .wait_timeout(table, deadline.saturating_duration_since(now))
            .unwrap();
        table = next;
        if result.timed_out() {
            break;
        }
    }
}

fn completed_matches(table: &ProcessTable, lease: &ChildLease) -> bool {
    table.completed.iter().any(|exit| exit.child == *lease)
}

fn stale_token_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        "launch token was already used by another child identity",
    )
}

fn os_contains_nul(value: &OsStr) -> bool {
    value.to_string_lossy().contains('\0')
}

fn fresh_owner_epoch() -> OwnerEpoch {
    static EPOCH_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    OwnerEpoch(
        time ^ ((std::process::id() as u64) << 32) ^ EPOCH_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    )
}

#[cfg(windows)]
fn create_application_job() -> io::Result<OwnedHandle> {
    let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw_job.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateJobObjectW returned a fresh owned HANDLE above.
    let job = unsafe { OwnedHandle::from_raw_handle(raw_job) };
    let mut limits = unsafe { std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;

    fn supervisor() -> (ProcessSupervisor, mpsc::Receiver<ProcessEvent>) {
        let mut allowed = ExecutableAllowlist::new();
        allowed.allow(ExecutableRole::CoreHelper, "/bin/sh").unwrap();
        ProcessSupervisor::new(allowed).unwrap()
    }

    fn spec(supervisor: &ProcessSupervisor, token: &str, script: &str) -> LaunchSpec {
        let identity = LaunchIdentity {
            owner_epoch: supervisor.owner_epoch(), generation: 7,
            launch_token: LaunchToken::new(token).unwrap(),
        };
        let mut spec = LaunchSpec::new(ExecutableRole::CoreHelper, identity);
        spec.args = vec!["-c".into(), script.into()];
        spec
    }

    #[test]
    fn unix_reaper_preserves_real_child_exit_status() {
        let (supervisor, events) = supervisor();
        let child = supervisor.spawn(spec(&supervisor, "exit", "exit 7")).unwrap().child;
        let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(exit.child, child);
        assert_eq!(exit.exit_code, Some(7));
        assert!(!exit.termination_requested);
        assert!(supervisor.active_children().is_empty());
    }

    #[test]
    fn unix_owns_process_group_and_preserves_admission_and_lease_identity() {
        let (supervisor, events) = supervisor();
        let mut request = spec(&supervisor, "same", "exec sleep 30");
        request.new_process_group = false;
        let first = supervisor.spawn(request.clone()).unwrap();
        let repeated = supervisor.spawn(request).unwrap();
        assert_eq!(repeated.disposition, SpawnDisposition::Reused);
        assert_eq!(first.child, repeated.child);
        assert_eq!(unsafe { libc::getpgid(first.child.pid as i32) }, first.child.pid as i32);
        assert_ne!(first.child.pid as i32, unsafe { libc::getpgrp() });
        let mut foreign = first.child.clone();
        foreign.pid = std::process::id();
        assert_eq!(supervisor.terminate(&foreign, 1).unwrap(), TerminateOutcome::Stale);
        let pause = supervisor.pause_admitting();
        assert_eq!(supervisor.spawn(spec(&supervisor, "paused", "exit 0")).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        drop(pause);
        let report = supervisor.shutdown(Duration::ZERO, Duration::from_secs(2));
        assert!(report.remaining.is_empty());
        let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(exit.child, first.child);
        assert!(exit.termination_requested);
        assert_eq!(supervisor.terminate(&first.child, 1).unwrap(), TerminateOutcome::AlreadyExited);
        assert_eq!(supervisor.spawn(spec(&supervisor, "closed", "exit 0")).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn unix_ignored_sigterm_escalates_within_bounded_time() {
        let (supervisor, events) = supervisor();
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let ready = std::env::temp_dir().join(format!("nova-supervisor-ready-{}-{nonce}", std::process::id()));
        let mut request = spec(&supervisor, "ignore-term", "trap '' TERM; printf ready > \"$1\"; while :; do sleep 30; done");
        request.args.extend(["nova-test".into(), ready.as_os_str().to_owned()]);
        let child = supervisor.spawn(request).unwrap().child;
        let deadline = Instant::now() + Duration::from_secs(3);
        while !ready.exists() && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(10)); }
        assert!(ready.is_file(), "child installed its TERM handler");
        let started = Instant::now();
        assert_eq!(supervisor.terminate(&child, 1).unwrap(), TerminateOutcome::Requested);
        let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(exit.exit_code, Some(128 + libc::SIGKILL));
        assert!(exit.termination_requested);
        assert!(started.elapsed() < Duration::from_secs(3));
        fs::remove_file(ready).unwrap();
    }

    #[test]
    fn unix_drop_reaps_own_children_without_touching_another_supervisor_group() {
        let (first, first_events) = supervisor();
        let (second, second_events) = supervisor();
        let owned = first.spawn(spec(&first, "first", "exec sleep 30")).unwrap().child;
        let unrelated = second.spawn(spec(&second, "second", "exec sleep 30")).unwrap().child;
        drop(first);
        let ProcessEvent::Exited(exit) = first_events.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(exit.child, owned);
        assert!(second_events.try_recv().is_err());
        assert_eq!(second.active_children(), vec![unrelated]);
        assert!(second.shutdown(Duration::ZERO, Duration::from_secs(2)).remaining.is_empty());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn test_supervisor() -> (ProcessSupervisor, mpsc::Receiver<ProcessEvent>) {
        let command = std::env::var_os("COMSPEC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        let mut allowlist = ExecutableAllowlist::new();
        for role in [ExecutableRole::CoreHelper, ExecutableRole::RenderService, ExecutableRole::NativeViewer, ExecutableRole::BuildWorker] {
            allowlist.allow(role, &command).unwrap();
        }
        ProcessSupervisor::new(allowlist).unwrap()
    }

    fn identity(supervisor: &ProcessSupervisor, token: &str) -> LaunchIdentity {
        LaunchIdentity {
            owner_epoch: supervisor.owner_epoch(),
            generation: 7,
            launch_token: LaunchToken::new(token).unwrap(),
        }
    }

    fn command_spec(supervisor: &ProcessSupervisor, token: &str, command: &str) -> LaunchSpec {
        let mut spec = LaunchSpec::new(ExecutableRole::CoreHelper, identity(supervisor, token));
        spec.args = vec!["/d".into(), "/c".into(), command.into()];
        spec
    }

    #[test]
    fn observes_headless_child_exit() {
        let (supervisor, events) = test_supervisor();
        let spawned = supervisor
            .spawn(command_spec(&supervisor, "normal-exit", "exit 7"))
            .unwrap();
        let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(exit.child, spawned.child);
        assert_eq!(exit.exit_code, Some(7));
        assert!(!exit.termination_requested);
        assert!(supervisor.active_children().is_empty());
    }

    #[test]
    fn launch_token_is_idempotent_while_child_is_active() {
        let (supervisor, _events) = test_supervisor();
        let spec = command_spec(&supervisor, "same-request", "ping -n 30 127.0.0.1");
        let first = supervisor.spawn(spec.clone()).unwrap();
        let second = supervisor.spawn(spec).unwrap();
        assert_eq!(first.disposition, SpawnDisposition::Created);
        assert_eq!(second.disposition, SpawnDisposition::Reused);
        assert_eq!(first.child, second.child);
        let _ = supervisor.terminate(&first.child, 1).unwrap();
    }

    #[test]
    fn duplicate_terminate_never_targets_a_reused_pid() {
        let (supervisor, events) = test_supervisor();
        let child = supervisor
            .spawn(command_spec(
                &supervisor,
                "terminate-once",
                "ping -n 30 127.0.0.1",
            ))
            .unwrap()
            .child;
        assert_eq!(
            supervisor.terminate(&child, 23).unwrap(),
            TerminateOutcome::Requested
        );
        assert!(matches!(
            supervisor.terminate(&child, 23).unwrap(),
            TerminateOutcome::AlreadyRequested | TerminateOutcome::AlreadyExited
        ));
        let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(exit.child, child);
        assert!(exit.termination_requested);
    }

    #[test]
    fn exited_but_not_reaped_child_is_an_idempotent_terminate() {
        let (supervisor, _events) = test_supervisor();
        let spec = command_spec(&supervisor, "exited-not-reaped", "exit 7");
        let executable = supervisor
            .allowlist
            .executable(spec.role)
            .unwrap()
            .to_path_buf();
        let child = spawn_suspended_in_job(&supervisor.job, &executable, &spec).unwrap();
        let id = ChildId(supervisor.next_child_id.fetch_add(1, Ordering::Relaxed));
        let lease = ChildLease {
            id,
            role: spec.role,
            identity: spec.identity,
            pid: child.pid,
        };
        let child = Arc::new(Mutex::new(child));
        supervisor.shared.table.lock().unwrap().active.insert(
            id,
            ChildRecord {
                lease: lease.clone(),
                child: child.clone(),
                termination_requested: false,
            },
        );

        assert!(child
            .lock()
            .unwrap()
            .wait_for_exit(Duration::from_secs(5))
            .unwrap());
        assert_eq!(
            supervisor.terminate(&lease, 23).unwrap(),
            TerminateOutcome::AlreadyExited
        );
        assert!(
            !supervisor
                .shared
                .table
                .lock()
                .unwrap()
                .active
                .get(&id)
                .unwrap()
                .termination_requested
        );
        supervisor.shared.table.lock().unwrap().active.remove(&id);
    }

    #[test]
    fn rapid_natural_exit_and_terminate_race_is_idempotent() {
        let (supervisor, events) = test_supervisor();
        for sequence in 0..24 {
            let child = supervisor
                .spawn(command_spec(
                    &supervisor,
                    &format!("natural-exit-race-{sequence}"),
                    "exit 7",
                ))
                .unwrap()
                .child;
            let outcome = supervisor.terminate(&child, 23).unwrap();
            assert!(matches!(
                outcome,
                TerminateOutcome::Requested | TerminateOutcome::AlreadyExited
            ));
            let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(exit.child, child);
            assert_eq!(
                exit.termination_requested,
                outcome == TerminateOutcome::Requested
            );
            assert_eq!(
                supervisor.terminate(&child, 23).unwrap(),
                TerminateOutcome::AlreadyExited
            );
        }
    }

    #[test]
    fn closing_job_terminates_remaining_child() {
        let (supervisor, events) = test_supervisor();
        let child = supervisor
            .spawn(command_spec(
                &supervisor,
                "job-close",
                "ping -n 30 127.0.0.1",
            ))
            .unwrap()
            .child;
        drop(supervisor);
        let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(exit.child, child);
    }

    #[test]
    fn stale_owner_epoch_is_rejected_before_spawn() {
        let (supervisor, _events) = test_supervisor();
        let mut spec = command_spec(&supervisor, "stale-owner", "exit 0");
        spec.identity.owner_epoch = OwnerEpoch(supervisor.owner_epoch().get().wrapping_add(1));
        assert_eq!(
            supervisor.spawn(spec).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn shutdown_stops_admission_and_reaps_every_remaining_child() {
        let (supervisor, events) = test_supervisor();
        let child = supervisor
            .spawn(command_spec(
                &supervisor,
                "shutdown-child",
                "ping -n 30 127.0.0.1",
            ))
            .unwrap()
            .child;

        let report = supervisor.shutdown(Duration::ZERO, Duration::from_secs(5));
        assert!(report.remaining.is_empty());
        assert_eq!(report.terminate_requests, 1);
        let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(exit.child, child);
        assert!(exit.termination_requested);

        let error = supervisor
            .spawn(command_spec(&supervisor, "after-shutdown", "exit 0"))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn shutdown_idle_has_no_fixed_grace_delay() {
        let (supervisor, _) = test_supervisor();
        let report = supervisor.shutdown(Duration::from_millis(500), Duration::from_millis(1500));
        assert_eq!(report.terminate_requests, 0);
        assert!(report.remaining.is_empty());
        assert!(report.elapsed_ms < 1000, "idle must not spend its configured shutdown budget waiting");
        println!("[NOVA_LIFECYCLE_PROFILE] case=idle child_ms={} actual_children=0 remaining=0 window_response=UNMEASURED", report.elapsed_ms);
    }

    #[test]
    fn shutdown_multiple_children_share_one_deadline_and_leave_no_owned_process() {
        let (supervisor, events) = test_supervisor();
        let children = [ExecutableRole::CoreHelper, ExecutableRole::RenderService, ExecutableRole::NativeViewer, ExecutableRole::BuildWorker]
            .into_iter().enumerate().map(|(index, role)| {
                let mut spec = command_spec(&supervisor, &format!("parallel-exit-{index}"), "ping -n 30 127.0.0.1");
                spec.role = role;
                supervisor.spawn(spec).unwrap().child
            }).collect::<Vec<_>>();
        let started = Instant::now();
        let report = supervisor.shutdown(Duration::from_millis(25), Duration::from_millis(750));
        assert!(report.remaining.is_empty());
        assert_eq!(report.terminate_requests, 4);
        assert!(started.elapsed() < Duration::from_secs(2));
        for _ in children {
            let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(exit.termination_requested);
        }
        println!("[NOVA_LIFECYCLE_PROFILE] case=render_process_roles child_fixture=cmd_ping count=4 child_ms={} graceful_ms={} forced_ms={} remaining=0 gui=UNMEASURED gpu_release=UNMEASURED", report.elapsed_ms, report.graceful_wait_ms, report.forced_wait_ms);
    }

    #[test]
    fn exited_and_stale_leases_are_never_retargeted() {
        let (supervisor, events) = test_supervisor();
        let child = supervisor
            .spawn(command_spec(&supervisor, "already-exited", "exit 0"))
            .unwrap()
            .child;
        let ProcessEvent::Exited(exit) = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(exit.child, child);
        assert_eq!(
            supervisor.terminate(&child, 1).unwrap(),
            TerminateOutcome::AlreadyExited
        );
        assert!(matches!(
            supervisor.wait_for_exit(&child, Duration::ZERO),
            WaitOutcome::Exited(_)
        ));

        let mut forged = child;
        forged.identity.launch_token = LaunchToken::new("different-lease").unwrap();
        assert_eq!(
            supervisor.terminate(&forged, 1).unwrap(),
            TerminateOutcome::Stale
        );
        assert_eq!(
            supervisor.wait_for_exit(&forged, Duration::ZERO),
            WaitOutcome::Stale
        );
    }
}
