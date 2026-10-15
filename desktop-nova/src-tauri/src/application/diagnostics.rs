//! Startup-scoped diagnostics. Business products and user configuration never
//! enter this tree; exporters visit only the explicit role directories below.

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

pub(crate) const SESSION_ID_ENV: &str = "NOVA_LOG_SESSION_ID";
pub(crate) const SESSION_DIR_ENV: &str = "NOVA_LOG_SESSION_DIR";
const KEEP_SESSIONS: usize = 3;
const ROLES: &[&str] = &["main", "core", "render-service", "quick-preview", "diagnostics"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionMetadata {
    pub session_id: String,
    pub started_at_epoch_ms: u64,
    pub pid: u32,
    pub version: String,
    pub git_sha: String,
    pub os: String,
    pub architecture: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogExportResult {
    pub path: PathBuf,
    pub session_ids: Vec<String>,
    pub file_count: usize,
    pub snapshot_bytes: u64,
}

struct Session {
    metadata: SessionMetadata,
    directory: PathBuf,
    // Sharing is denied, not merely checked against a PID. The OS releases the
    // handle on a crash, while normal exits deliberately retain the log files.
    _active: File,
    _stdout: File,
    _stderr: File,
}
static SESSION: OnceLock<Session> = OnceLock::new();

pub(crate) fn init(data_root: &Path, version: &str, git_sha: &str) -> Result<SessionMetadata, String> {
    if let Some(session) = SESSION.get() { return Ok(session.metadata.clone()); }
    let session = create_session(data_root, version, git_sha).map_err(|e| format!("初始化日志会话：{e}"))?;
    redirect_main_output(&session._stdout, &session._stderr).map_err(|e| format!("初始化主程序日志：{e}"))?;
    let metadata = session.metadata.clone();
    let directory = session.directory.clone();
    std::env::set_var(SESSION_ID_ENV, &metadata.session_id);
    std::env::set_var(SESSION_DIR_ENV, &directory);
    SESSION.set(session).map_err(|_| "日志会话已初始化".to_owned())?;
    eprintln!("[NOVA_LOG_SESSION] session_id={} version={} git_sha={} retention=3 startup=true", metadata.session_id, version, git_sha);
    let sessions_root = directory.parent().unwrap().to_path_buf();
    // Directory cleanup and hardware discovery must not hold up startup.
    if let Err(error) = std::thread::Builder::new().name("nova-session-diagnostics".into()).spawn(move || {
        if let Err(error) = cleanup_sessions(&sessions_root) { eprintln!("[NOVA_LOG_RETENTION] error={error}"); }
        let system = system_information();
        if let Err(error) = write_diagnostic_json(&directory.join("system-gpu.json"), &system) {
            eprintln!("[NOVA_LOG_SYSTEM] error={error}");
        }
    }) { eprintln!("[NOVA_LOG_SESSION] background_error={error}"); }
    Ok(metadata)
}

fn create_session(data_root: &Path, version: &str, git_sha: &str) -> io::Result<Session> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?;
    let session_id = format!("{:013}-{}-{:x}", now.as_millis(), std::process::id(), now.as_nanos());
    let directory = data_root.join("logs").join("sessions").join(&session_id);
    fs::create_dir_all(&directory)?;
    let active = active_file(&directory.join("active.lock"), true)?;
    for role in ROLES { fs::create_dir(directory.join(role))?; }
    let metadata = SessionMetadata {
        session_id, started_at_epoch_ms: now.as_millis() as u64, pid: std::process::id(),
        version: version.into(), git_sha: git_sha.into(),
        os: std::env::consts::OS.into(), architecture: std::env::consts::ARCH.into(),
    };
    fs::write(directory.join("session.json"), serde_json::to_vec_pretty(&metadata)?)?;
    fs::write(directory.join("system-gpu.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "os": std::env::consts::OS, "architecture": std::env::consts::ARCH,
        "hardwareQuery": "pending", "gpuDetails": "see Viewer and resource diagnostics"
    }))?)?;
    let stdout = OpenOptions::new().create(true).append(true).open(directory.join("main/stdout.log"))?;
    let stderr = OpenOptions::new().create(true).append(true).open(directory.join("main/stderr.log"))?;
    Ok(Session { metadata, directory, _active: active, _stdout: stdout, _stderr: stderr })
}

fn active_file(path: &Path, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(create);
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    fs2::FileExt::try_lock_exclusive(&file)?;
    Ok(file)
}

/// Child processes inherit these two environment variables through the
/// supervisor's existing environment builder; no launch contract is changed.
pub(crate) fn role_directory(role: &str, legacy: impl AsRef<Path>) -> PathBuf {
    debug_assert!(ROLES.contains(&role));
    SESSION.get().map(|session| session.directory.join(role)).unwrap_or_else(|| legacy.as_ref().to_path_buf())
}

pub(crate) fn has_session() -> bool { SESSION.get().is_some() }

pub(crate) fn log_path(role: &str, name: &str, legacy: impl AsRef<Path>) -> Result<PathBuf, String> {
    if !ROLES.contains(&role) || Path::new(name).file_name().and_then(|s| s.to_str()) != Some(name) {
        return Err("日志文件名无效".into());
    }
    let directory = role_directory(role, legacy);
    fs::create_dir_all(&directory).map_err(|e| format!("创建日志目录：{e}"))?;
    Ok(directory.join(name))
}

pub(crate) fn record_resource(name: &str, report: &impl Serialize) {
    let Some(session) = SESSION.get() else { return; };
    if Path::new(name).file_name().and_then(|s| s.to_str()) != Some(name) { return; }
    let result = write_diagnostic_json(&session.directory.join("diagnostics").join(format!("resource-{name}.json")), report);
    if let Err(error) = result { eprintln!("[NOVA_LOG_RESOURCE] error={error}"); }
}

fn write_diagnostic_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value).map_err(io::Error::other)?)?;
    fs::rename(temporary, path)
}

#[derive(Clone)]
struct SavedSession { directory: PathBuf, metadata: SessionMetadata }

fn is_plain(path: &Path, directory: bool) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else { return false; };
    if metadata.file_type().is_symlink() || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) { return false; }
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return false; } // reparse point
    }
    true
}

fn saved_sessions(root: &Path) -> io::Result<Vec<SavedSession>> {
    let mut sessions = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let directory = entry.path();
        if !is_plain(&directory, true) { continue; }
        let manifest = directory.join("session.json");
        if !is_plain(&manifest, false) || fs::metadata(&manifest)?.len() > 16 * 1024 { continue; }
        let Ok(metadata) = serde_json::from_slice::<SessionMetadata>(&fs::read(manifest)?) else { continue; };
        if directory.file_name().and_then(|s| s.to_str()) != Some(metadata.session_id.as_str()) { continue; }
        sessions.push(SavedSession { directory, metadata });
    }
    sessions.sort_by(|a, b| b.metadata.started_at_epoch_ms.cmp(&a.metadata.started_at_epoch_ms)
        .then_with(|| b.metadata.session_id.cmp(&a.metadata.session_id)));
    Ok(sessions)
}

fn cleanup_sessions(root: &Path) -> io::Result<()> {
    let _catalog = catalog_guard(root)?;
    for session in saved_sessions(root)?.into_iter().skip(KEEP_SESSIONS) {
        // An unreadable/missing lock is conservatively preserved. The manifest
        // is published only after ownership is acquired, so startup is protected.
        let Ok(inactive) = active_file(&session.directory.join("active.lock"), false) else { continue; };
        drop(inactive);
        fs::remove_dir_all(&session.directory)?;
    }
    Ok(())
}

/// This function is intentionally blocking: call it from Tauri spawn_blocking.
/// Reads are bounded to each file's observed length, so a continuously writing
/// Viewer cannot keep an export waiting for a render task or EOF.
pub(crate) fn export_zip(target: &Path) -> Result<LogExportResult, String> {
    let session = SESSION.get().ok_or("日志会话尚未初始化")?;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    export_sessions(session.directory.parent().unwrap(), target).map_err(|e| format!("导出日志：{e}"))
}

fn export_sessions(root: &Path, target: &Path) -> io::Result<LogExportResult> {
    // A short-lived cross-process catalog lock serializes retention with the
    // complete ZIP snapshot. Acquiring it happens only on background threads;
    // starting another application/session never waits for this lock.
    let _catalog = catalog_guard(root)?;
    let sessions = saved_sessions(root)?.into_iter().take(KEEP_SESSIONS).collect::<Vec<_>>();
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
    let temporary = target.with_file_name(format!(".nova-log-export-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut file_count = 0;
        let mut snapshot_bytes = 0;
        let mut entries = Vec::new();
        for session in &sessions {
            for name in ["session.json", "system-gpu.json"] {
                let path = session.directory.join(name);
                if is_plain(&path, false) { entries.push((path, format!("{}/{name}", session.metadata.session_id))); }
            }
            for role in ROLES {
                let directory = session.directory.join(role);
                if !is_plain(&directory, true) { continue; }
                for entry in fs::read_dir(directory)? {
                    let entry = entry?; let path = entry.path();
                    if !is_plain(&path, false) { continue; }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let allowed = name.ends_with(".log") || (*role == "diagnostics" && name.starts_with("resource-") && name.ends_with(".json"));
                    if allowed { entries.push((path, format!("{}/{role}/{name}", session.metadata.session_id))); }
                }
            }
        }
        entries.sort_by(|a, b| a.1.cmp(&b.1));
        for (path, name) in entries {
            let source = File::open(&path)?;
            let length = source.metadata()?.len();
            zip.start_file(name, options)?;
            snapshot_bytes += io::copy(&mut source.take(length), &mut zip)?;
            file_count += 1;
        }
        let file = zip.finish()?;
        file.sync_all()?;
        fs::rename(&temporary, target)?;
        Ok(LogExportResult { path: target.to_path_buf(), session_ids: sessions.iter().map(|s| s.metadata.session_id.clone()).collect(), file_count, snapshot_bytes })
    })();
    if result.is_err() { let _ = fs::remove_file(temporary); }
    result
}

fn catalog_guard(root: &Path) -> io::Result<File> {
    let path = root.join("retention-export.lock");
    loop {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let opened = options.open(&path).and_then(|file| {
            #[cfg(unix)]
            fs2::FileExt::try_lock_exclusive(&file)?;
            Ok(file)
        });
        match opened {
            Ok(file) => return Ok(file),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock || matches!(error.raw_os_error(), Some(32 | 33)) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
fn redirect_main_output(stdout: &File, stderr: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    if unsafe { SetStdHandle(STD_OUTPUT_HANDLE, stdout.as_raw_handle() as _) } == 0 { return Err(io::Error::last_os_error()); }
    if unsafe { SetStdHandle(STD_ERROR_HANDLE, stderr.as_raw_handle() as _) } == 0 { return Err(io::Error::last_os_error()); }
    Ok(())
}

#[cfg(unix)]
fn redirect_main_output(stdout: &File, stderr: &File) -> io::Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    std::io::stdout().flush()?;
    std::io::stderr().flush()?;
    let previous = unsafe { libc::dup(libc::STDOUT_FILENO) };
    if previous < 0 { return Err(io::Error::last_os_error()); }
    let previous = unsafe { OwnedFd::from_raw_fd(previous) };
    if unsafe { libc::dup2(stdout.as_raw_fd(), libc::STDOUT_FILENO) } < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::dup2(stderr.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
        let error = io::Error::last_os_error();
        let _ = unsafe { libc::dup2(previous.as_raw_fd(), libc::STDOUT_FILENO) };
        return Err(error);
    }
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn redirect_main_output(_stdout: &File, _stderr: &File) -> io::Result<()> { Ok(()) }

fn system_information() -> serde_json::Value {
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        let script = "$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $os=Get-CimInstance Win32_OperatingSystem; $gpu=@(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,AdapterRAM,PNPDeviceID); [ordered]@{os=$os.Caption;version=$os.Version;build=$os.BuildNumber;totalPhysicalMemoryKiB=$os.TotalVisibleMemorySize;gpu=$gpu} | ConvertTo-Json -Depth 4 -Compress";
        let command = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(0x08000000).spawn();
        if let Ok(mut child) = command {
            let started = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            let mut bytes = Vec::new();
                            if let Some(stdout) = child.stdout.take() { let _ = stdout.take(64 * 1024).read_to_end(&mut bytes); }
                            if let Ok(value) = serde_json::from_slice(&bytes) { return value; }
                        }
                        break;
                    }
                    Ok(None) if started.elapsed() < std::time::Duration::from_secs(10) => std::thread::sleep(std::time::Duration::from_millis(100)),
                    _ => { let _ = child.kill(); let _ = child.wait(); break; }
                }
            }
        }
    }
    serde_json::json!({ "os": std::env::consts::OS, "architecture": std::env::consts::ARCH, "hardwareQuery": "unavailable", "gpuDetails": "see Viewer and resource diagnostics" })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture { fn new() -> Self {
        let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("nova-log-session-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).unwrap(); Self(root)
    }}
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fn saved(root: &Path, index: u64) -> Session {
        let mut session = create_session(root, "0.2.0", "fixture-commit").unwrap();
        session.metadata.started_at_epoch_ms = index;
        fs::write(session.directory.join("session.json"), serde_json::to_vec(&session.metadata).unwrap()).unwrap();
        session
    }
    #[test]
    fn diagnostics_export_keeps_three_sessions_and_excludes_products_config_and_keys() {
        let fixture = Fixture::new();
        for index in 1..=4 {
            let session = saved(&fixture.0, index);
            fs::write(session.directory.join("main/stderr.log"), format!("session-{index} exit capture=14277 resolve=0")).unwrap();
            fs::write(session.directory.join("quick-preview/valid.log"), "actual render diagnostic").unwrap();
            fs::write(session.directory.join("quick-preview/projection.litematic"), "projection must not export").unwrap();
            fs::write(session.directory.join("configuration.json"), "config must not export").unwrap();
            fs::write(session.directory.join("key.pem"), "key must not export").unwrap();
        }
        let target = fixture.0.join("chosen.zip");
        let result = export_sessions(&fixture.0.join("logs/sessions"), &target).unwrap();
        assert_eq!(result.session_ids.len(), 3);
        let mut zip = zip::ZipArchive::new(File::open(target).unwrap()).unwrap();
        let mut text = String::new();
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).unwrap();
            assert!(!entry.name().contains("projection") && !entry.name().contains("configuration") && !entry.name().contains(".pem"));
            entry.read_to_string(&mut text).unwrap();
        }
        assert!(!text.contains("session-1 exit"));
        assert!(text.contains("session-4 exit") && text.contains("session-3 exit") && text.contains("session-2 exit"));
    }
    #[cfg(any(windows, unix))]
    #[test]
    fn diagnostics_retention_preserves_active_old_instance_and_crashed_sessions() {
        let fixture = Fixture::new();
        let active = saved(&fixture.0, 1);
        let active_directory = active.directory.clone();
        let obsolete = saved(&fixture.0, 2).directory.clone();
        for index in 3..=5 { drop(saved(&fixture.0, index)); }
        cleanup_sessions(&fixture.0.join("logs/sessions")).unwrap();
        assert!(active_directory.is_dir(), "exclusive handle protects another live instance");
        assert!(!obsolete.exists());
        assert_eq!(saved_sessions(&fixture.0.join("logs/sessions")).unwrap().len(), 4);
        drop(active);
        cleanup_sessions(&fixture.0.join("logs/sessions")).unwrap();
        assert!(!active_directory.exists());
        assert_eq!(saved_sessions(&fixture.0.join("logs/sessions")).unwrap().len(), 3);
    }
    #[test]
    fn diagnostics_partial_session_without_manifest_is_never_deleted() {
        let fixture = Fixture::new();
        for index in 1..=4 { drop(saved(&fixture.0, index)); }
        let pending = fixture.0.join("logs/sessions/starting-instance");
        fs::create_dir(&pending).unwrap();
        cleanup_sessions(&fixture.0.join("logs/sessions")).unwrap();
        assert!(pending.exists());
    }
    #[cfg(any(windows, unix))]
    #[test]
    fn diagnostics_export_lock_defers_retention_without_blocking_session_creation() {
        let fixture = Fixture::new();
        for index in 1..=4 { drop(saved(&fixture.0, index)); }
        let root = fixture.0.join("logs/sessions");
        let exporting = catalog_guard(&root).unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let cleanup = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            finished_tx.send(cleanup_sessions(&root)).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(finished_rx.recv_timeout(std::time::Duration::from_millis(100)).is_err());
        let newest = saved(&fixture.0, 5);
        assert!(newest.directory.is_dir(), "new startup never acquires the export/retention lock");
        drop(exporting);
        finished_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap().unwrap();
        cleanup.join().unwrap();
        drop(newest);
    }

    #[cfg(unix)]
    #[test]
    fn diagnostics_unix_output_redirect_child() {
        let Some(root) = std::env::var_os("NOVA_DIAGNOSTIC_REDIRECT_TEST_DIR") else { return; };
        let root = PathBuf::from(root);
        let stdout = File::create(root.join("stdout.log")).unwrap();
        let stderr = File::create(root.join("stderr.log")).unwrap();
        redirect_main_output(&stdout, &stderr).unwrap();
        std::io::stdout().write_all(b"unix stdout diagnostic\n").unwrap();
        std::io::stderr().write_all(b"unix stderr diagnostic\n").unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn diagnostics_unix_redirect_preserves_stdout_and_stderr_in_session_files() {
        let fixture = Fixture::new();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", concat!(module_path!(), "::diagnostics_unix_output_redirect_child"), "--nocapture"])
            .env("NOVA_DIAGNOSTIC_REDIRECT_TEST_DIR", &fixture.0)
            .output().unwrap();
        assert!(output.status.success());
        assert!(fs::read_to_string(fixture.0.join("stdout.log")).unwrap().contains("unix stdout diagnostic"));
        assert!(fs::read_to_string(fixture.0.join("stderr.log")).unwrap().contains("unix stderr diagnostic"));
    }
}
