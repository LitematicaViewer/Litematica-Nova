use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};
use viewer_launch_contract::{ViewerSessionCommand,ViewerSessionEvent,ViewerSessionState,ViewerSessionStatus};
pub(crate) const VIEWER_SESSION_DIRECTORY_ENV: &str = "LITEMATICA_NOVA_VIEWER_SESSION_DIRECTORY";
pub(crate) const VIEWER_SESSION_ID_ENV: &str = "LITEMATICA_NOVA_VIEWER_SESSION_ID";
const MAX_SESSION_COMMAND_BYTES: u64 = 64 * 1024;
#[derive(Debug)]
pub(crate) struct ViewerSessionWatcher {
    directory: PathBuf,
    session_id: Box<str>,
    last_sequence: u64,
    next_event_id: u64,
}

impl ViewerSessionWatcher {
    pub(crate) fn from_environment() -> Result<Option<Self>> {
        let directory = std::env::var_os(VIEWER_SESSION_DIRECTORY_ENV);
        let session_id = std::env::var_os(VIEWER_SESSION_ID_ENV);
        match (directory, session_id) {
            (None, None) => Ok(None),
            (Some(directory), Some(session_id)) => {
                let session_id = session_id
                    .into_string()
                    .map_err(|_| anyhow!("{VIEWER_SESSION_ID_ENV} must contain Unicode text"))?;
                Self::new(PathBuf::from(directory), session_id).map(Some)
            }
            _ => bail!(
                "{VIEWER_SESSION_DIRECTORY_ENV} and {VIEWER_SESSION_ID_ENV} must be configured together"
            ),
        }
    }

    fn new(directory: PathBuf, session_id: String) -> Result<Self> {
        if !directory.is_absolute() {
            bail!(
                "{VIEWER_SESSION_DIRECTORY_ENV} must be absolute: {}",
                directory.display()
            );
        }
        if session_id.trim().is_empty()
            || session_id.len() > 128
            || !session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            bail!("{VIEWER_SESSION_ID_ENV} has an invalid session identifier");
        }
        fs::create_dir_all(&directory).with_context(|| {
            format!(
                "create persistent Viewer session directory {}",
                directory.display()
            )
        })?;
        Ok(Self {
            directory,
            session_id: session_id.into_boxed_str(),
            last_sequence: 0,
            next_event_id: 1,
        })
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn poll_next(&self) -> Result<Option<ViewerSessionCommand>> {
        let mut next = None::<ViewerSessionCommand>;
        for entry in fs::read_dir(&self.directory).with_context(|| {
            format!(
                "read persistent Viewer session directory {}",
                self.directory.display()
            )
        })? {
            let entry = entry.context("read persistent Viewer session command entry")?;
            let path = entry.path();
            if command_sequence_from_path(&path).is_none() {
                continue;
            }
            let metadata = entry
                .metadata()
                .with_context(|| format!("inspect Viewer session command {}", path.display()))?;
            if !metadata.is_file() {
                continue;
            }
            if metadata.len() > MAX_SESSION_COMMAND_BYTES {
                bail!(
                    "Viewer session command exceeds {} bytes: {}",
                    MAX_SESSION_COMMAND_BYTES,
                    path.display()
                );
            }
            let bytes = fs::read(&path)
                .with_context(|| format!("read Viewer session command {}", path.display()))?;
            let command = ViewerSessionCommand::from_json_slice(&bytes).map_err(|error| {
                anyhow!("invalid Viewer session command {}: {error}", path.display())
            })?;
            if command.session_id != self.session_id.as_ref()
                || command.sequence <= self.last_sequence
            {
                continue;
            }
            if next
                .as_ref()
                .is_none_or(|current| command.sequence < current.sequence)
            {
                next = Some(command);
            }
        }
        Ok(next)
    }

    pub(crate) fn publish_status(
        &self,
        sequence: u64,
        state: ViewerSessionState,
        pid: u32,
        hwnd: isize,
    ) -> Result<PathBuf> {
        let status = ViewerSessionStatus::new(
            self.session_id.to_string(),
            sequence,
            pid,
            hwnd.to_string(),
            state,
        )
        .map_err(|error| anyhow!("construct Viewer session status: {error}"))?;
        let bytes = status
            .to_json_bytes()
            .map_err(|error| anyhow!("serialize Viewer session status: {error}"))?;
        self.publish_atomic_json("status", sequence, &bytes)
    }

    pub(crate) fn publish_event(
        &mut self,
        state: ViewerSessionState,
        pid: u32,
        hwnd: isize,
    ) -> Result<PathBuf> {
        let event_id = self.next_event_id;
        let event = ViewerSessionEvent::new(
            self.session_id.to_string(),
            event_id,
            pid,
            hwnd.to_string(),
            state,
        )
        .map_err(|error| anyhow!("construct Viewer session event: {error}"))?;
        let bytes = event
            .to_json_bytes()
            .map_err(|error| anyhow!("serialize Viewer session event: {error}"))?;
        let path = self.publish_atomic_json("event", event_id, &bytes)?;
        self.next_event_id = self
            .next_event_id
            .checked_add(1)
            .context("Viewer session event sequence exhausted")?;
        Ok(path)
    }

    fn publish_atomic_json(&self, prefix: &str, id: u64, bytes: &[u8]) -> Result<PathBuf> {
        let final_path = self.directory.join(format!("{prefix}-{id:020}.json"));
        let staging_path = self.directory.join(format!("{prefix}-{id:020}.json.tmp"));
        let write_result = (|| -> Result<()> {
            use std::io::Write;

            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staging_path)
                .with_context(|| {
                    format!(
                        "create Viewer session status staging file {}",
                        staging_path.display()
                    )
                })?;
            file.write_all(&bytes).with_context(|| {
                format!(
                    "write Viewer session status staging file {}",
                    staging_path.display()
                )
            })?;
            file.sync_all().with_context(|| {
                format!(
                    "flush Viewer session status staging file {}",
                    staging_path.display()
                )
            })?;
            drop(file);
            fs::rename(&staging_path, &final_path).with_context(|| {
                format!(
                    "publish Viewer session status {} -> {}",
                    staging_path.display(),
                    final_path.display()
                )
            })?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&staging_path);
            return Err(error);
        }
        Ok(final_path)
    }

    pub(crate) fn acknowledge(&mut self, sequence: u64) -> Result<()> {
        if sequence <= self.last_sequence {
            bail!(
                "Viewer session command sequence {} does not advance {}",
                sequence,
                self.last_sequence
            );
        }
        self.last_sequence = sequence;
        self.remove_commands_through(sequence)
    }

    fn remove_commands_through(&self, sequence: u64) -> Result<()> {
        for entry in fs::read_dir(&self.directory).with_context(|| {
            format!(
                "clean persistent Viewer session directory {}",
                self.directory.display()
            )
        })? {
            let entry = entry.context("read persistent Viewer session cleanup entry")?;
            let path = entry.path();
            let Some(entry_sequence) = command_sequence_from_path(&path) else {
                continue;
            };
            if entry_sequence <= sequence {
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("remove consumed Viewer session command {}", path.display())
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

fn command_sequence_from_path(path: &Path) -> Option<u64> {
    let file_name = path.file_name()?.to_str()?;
    file_name
        .strip_prefix("command-")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

