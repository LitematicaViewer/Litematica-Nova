use crate::viewer_backend::ViewerBackendIdentity;
use crate::viewer_compatibility::ViewerCompatibilityPlan;
use ::viewer_launch_contract::{
    BuildCacheTask, InteractiveTask, PrepareViewerTask, PreviewTask, VIEWER_INTEGRITY_PREFIX,
    VIEWER_LAUNCH_ACK_PREFIX, VIEWER_READY_PREFIX, ViewerBackend, ViewerControls,
    ViewerFrameCapture, ViewerIntegrity, ViewerInteractivePurpose, ViewerLaunchAck,
    ViewerLaunchContract, ViewerPresentation, ViewerReady, ViewerSessionCommand, ViewerTask,
};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static VIEWER_LAUNCH_REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub(crate) struct ViewerLaunchContext {
    compatibility: ViewerCompatibilityPlan,
    controls: ViewerControls,
    backend: ViewerBackendIdentity,
    display_mode: &'static str,
}

impl ViewerLaunchContext {
    pub(crate) fn new(
        compatibility: ViewerCompatibilityPlan,
        controls: ViewerControls,
        backend: ViewerBackendIdentity,
    ) -> Self {
        Self {
            compatibility,
            controls,
            backend,
            display_mode: "full",
        }
    }

    pub(crate) const fn display_mode(&self) -> &'static str {
        self.display_mode
    }

    pub(crate) fn with_display_mode(mut self, mode: &str) -> Result<Self, String> {
        self.display_mode = match mode {
            "normal" | "standard" => "standard",
            "full" => "full",
            _ => return Err("Protected preview mode must be Standard or Full".into()),
        };
        Ok(self)
    }

    pub(crate) fn compatibility_fingerprint(&self) -> &str {
        self.compatibility.fingerprint()
    }

    pub(crate) fn controls(&self) -> ViewerControls {
        self.controls
    }

    pub(crate) fn backend(&self) -> &ViewerBackendIdentity {
        &self.backend
    }

    pub(crate) fn supports_persistent_vulkan_session(&self) -> bool {
        self.compatibility.renderer().is_some_and(|renderer| {
            renderer.shaderpack.is_none()
                && matches!(
                    renderer.backend,
                    ViewerBackend::Auto | ViewerBackend::Vulkan
                )
        })
    }

    pub(crate) fn prepare_interactive(
        &self,
        scene: PathBuf,
        presentation: ViewerPresentation,
        purpose: ViewerInteractivePurpose,
        cache_input: Option<PathBuf>,
        capture: Option<ViewerFrameCapture>,
    ) -> Result<ViewerLaunchContractFile, String> {
        let contract = ViewerLaunchContract::new(
            next_request_id()?,
            scene,
            self.compatibility.content().clone(),
            Some(
                self.compatibility
                    .renderer()
                    .ok_or_else(|| "interactive launch is missing renderer settings".to_owned())?
                    .clone(),
            ),
            ViewerTask::Interactive(InteractiveTask {
                presentation,
                purpose,
                cache_input,
                controls: self.controls,
                capture,
            }),
        )?;
        prepare_contract_file(&contract, &self.backend)
    }

    pub(crate) fn prepare_preview(
        &self,
        scene: PathBuf,
        output: PathBuf,
        cache_input: Option<PathBuf>,
    ) -> Result<ViewerLaunchContractFile, String> {
        let contract = ViewerLaunchContract::new(
            next_request_id()?,
            scene,
            self.compatibility.content().clone(),
            Some(
                self.compatibility
                    .renderer()
                    .ok_or_else(|| "preview launch is missing renderer settings".to_owned())?
                    .clone(),
            ),
            ViewerTask::Preview(PreviewTask {
                output,
                cache_input,
                effects_profile: if self.display_mode == "standard" {
                    viewer_launch_contract::ViewerEffectsProfile::Unlit
                } else {
                    viewer_launch_contract::ViewerEffectsProfile::Full
                },
            }),
        )?;
        prepare_contract_file(&contract, &self.backend)
    }

    pub(crate) fn prepare_build_cache(
        &self,
        scene: PathBuf,
        ready_file: PathBuf,
        cache_file: PathBuf,
    ) -> Result<ViewerLaunchContractFile, String> {
        let contract = ViewerLaunchContract::new(
            next_request_id()?,
            scene,
            self.compatibility.content().clone(),
            None,
            ViewerTask::BuildCache(BuildCacheTask {
                initial_layer_view: None,
                ready_file,
                cache_file,
                build_mode: ::viewer_launch_contract::RenderBuildMode::Prebuilt,
                effects_profile: viewer_launch_contract::ViewerEffectsProfile::Full,
            }),
        )?;
        prepare_contract_file(&contract, &self.backend)
    }

    pub(crate) fn prepare_viewer(
        &self,
        scene: PathBuf,
        ready_file: PathBuf,
        cache_input: PathBuf,
    ) -> Result<ViewerLaunchContractFile, String> {
        let contract = ViewerLaunchContract::new(
            next_request_id()?,
            scene,
            self.compatibility.content().clone(),
            Some(
                self.compatibility
                    .renderer()
                    .ok_or_else(|| {
                        "Viewer preparation launch is missing renderer settings".to_owned()
                    })?
                    .clone(),
            ),
            ViewerTask::PrepareViewer(PrepareViewerTask {
                ready_file,
                cache_input,
                presentation: ViewerPresentation::Popup,
                purpose: ViewerInteractivePurpose::Render,
                controls: self.controls,
                capture: None,
                initial_visible: false,
            }),
        )?;
        prepare_contract_file(&contract, &self.backend)
    }
}

#[derive(Debug)]
pub(crate) struct ViewerLaunchContractFile {
    path: PathBuf,
    staging_path: PathBuf,
    request_id: String,
    sha256: String,
    backend: ViewerBackendIdentity,
}

impl ViewerLaunchContractFile {
    pub(crate) fn create(
        path: PathBuf,
        contract: &ViewerLaunchContract,
        backend: &ViewerBackendIdentity,
    ) -> Result<Self, String> {
        let bytes = contract.to_json_bytes()?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let staging_path = path.with_extension("json.tmp");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "create Viewer launch contract directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staging_path)
                .map_err(|error| {
                    format!(
                        "create Viewer launch contract staging file {}: {error}",
                        staging_path.display()
                    )
                })?;
            file.write_all(&bytes).map_err(|error| {
                format!(
                    "write Viewer launch contract staging file {}: {error}",
                    staging_path.display()
                )
            })?;
            file.sync_all().map_err(|error| {
                format!(
                    "flush Viewer launch contract staging file {}: {error}",
                    staging_path.display()
                )
            })?;
            drop(file);
            fs::rename(&staging_path, &path).map_err(|error| {
                format!(
                    "publish Viewer launch contract {} -> {}: {error}",
                    staging_path.display(),
                    path.display()
                )
            })
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&staging_path);
            return Err(error);
        }
        Ok(Self {
            path,
            staging_path,
            request_id: contract.request_id.clone(),
            sha256,
            backend: backend.clone(),
        })
    }

    pub(crate) fn command_arg(&self) -> String {
        format!("--launch-contract={}", self.path.display())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }

    pub(crate) fn request_id(&self) -> &str {
        &self.request_id
    }

    pub(crate) fn accepted_ack(&self, output: &str) -> Result<bool, String> {
        accepted_launch_ack(
            output,
            &self.request_id,
            &self.sha256,
            self.backend.exe_path(),
            self.backend.sha256(),
        )
    }

    pub(crate) fn accepted_hidden_viewer_ready(
        &self,
        output: &str,
        expected_pid: u32,
    ) -> Result<Option<(ViewerReady, isize)>, String> {
        for payload in output
            .lines()
            .filter_map(|line| line.trim().strip_prefix(VIEWER_READY_PREFIX).map(str::trim))
        {
            let ready = serde_json::from_str::<ViewerReady>(payload)
                .map_err(|error| format!("parse hidden Viewer ready marker: {error}"))?;
            let hwnd =
                ready.validate_hidden_warm_viewer(&self.request_id, &self.sha256, expected_pid)?;
            return Ok(Some((ready, hwnd)));
        }
        Ok(None)
    }

    pub(crate) fn accepted_viewer_integrity(&self, output: &str) -> Result<bool, String> {
        for payload in output.lines().filter_map(|line| {
            line.trim()
                .strip_prefix(VIEWER_INTEGRITY_PREFIX)
                .map(str::trim)
        }) {
            let marker = serde_json::from_str::<ViewerIntegrity>(payload)
                .map_err(|error| format!("parse Viewer integrity marker: {error}"))?;
            marker.validate_for_contract(&self.request_id, &self.sha256)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn expected_backend_exe_path(&self) -> &Path {
        self.backend.exe_path()
    }

    pub(crate) fn expected_backend_sha256(&self) -> &str {
        self.backend.sha256()
    }

    pub(crate) fn publish_session_command(
        &self,
        session_directory: &Path,
        session_id: &str,
        sequence: u64,
    ) -> Result<PathBuf, String> {
        fs::create_dir_all(session_directory).map_err(|error| {
            format!(
                "create Viewer session directory {}: {error}",
                session_directory.display()
            )
        })?;
        let command =
            ViewerSessionCommand::new(session_id.to_owned(), sequence, self.path.clone())?;
        let bytes = command.to_json_bytes()?;
        let final_path = session_directory.join(format!("command-{sequence:020}.json"));
        let staging_path = session_directory.join(format!("command-{sequence:020}.json.tmp"));
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staging_path)
                .map_err(|error| {
                    format!(
                        "create Viewer session command staging file {}: {error}",
                        staging_path.display()
                    )
                })?;
            file.write_all(&bytes).map_err(|error| {
                format!(
                    "write Viewer session command staging file {}: {error}",
                    staging_path.display()
                )
            })?;
            file.sync_all().map_err(|error| {
                format!(
                    "flush Viewer session command staging file {}: {error}",
                    staging_path.display()
                )
            })?;
            drop(file);
            fs::rename(&staging_path, &final_path).map_err(|error| {
                format!(
                    "publish Viewer session command {} -> {}: {error}",
                    staging_path.display(),
                    final_path.display()
                )
            })
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&staging_path);
            return Err(error);
        }
        Ok(final_path)
    }
}

pub(crate) fn accepted_launch_ack(
    output: &str,
    expected_request_id: &str,
    expected_sha256: &str,
    expected_backend_exe_path: &Path,
    expected_backend_sha256: &str,
) -> Result<bool, String> {
    for payload in output.lines().filter_map(|line| {
        line.trim()
            .strip_prefix(VIEWER_LAUNCH_ACK_PREFIX)
            .map(str::trim)
    }) {
        let ack = serde_json::from_str::<ViewerLaunchAck>(payload)
            .map_err(|error| format!("parse Viewer launch ACK: {error}"))?;
        ack.validate_for_contract(expected_request_id, expected_sha256)?;
        let actual_backend_exe_path = fs::canonicalize(&ack.backend_exe_path).map_err(|error| {
            format!(
                "resolve Viewer launch ACK backend executable {}: {error}",
                ack.backend_exe_path.display()
            )
        })?;
        if actual_backend_exe_path != expected_backend_exe_path {
            return Err(format!(
                "launch ACK backend executable mismatch: expected {}, found {}",
                expected_backend_exe_path.display(),
                actual_backend_exe_path.display()
            ));
        }
        if ack.backend_sha256 != expected_backend_sha256 {
            return Err(format!(
                "launch ACK backend SHA-256 mismatch: expected {expected_backend_sha256}, found {}",
                ack.backend_sha256
            ));
        }
        return Ok(true);
    }
    Ok(false)
}

impl Drop for ViewerLaunchContractFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(&self.staging_path);
    }
}

fn prepare_contract_file(
    contract: &ViewerLaunchContract,
    backend: &ViewerBackendIdentity,
) -> Result<ViewerLaunchContractFile, String> {
    let path = crate::render_tmp_path("litematica_nova_viewer_launch_contract", "json")?;
    ViewerLaunchContractFile::create(path, contract, backend)
}

fn next_request_id() -> Result<String, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("build Viewer launch request ID: {error}"))?
        .as_nanos();
    let sequence = VIEWER_LAUNCH_REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(format!("nova-{}-{nanos}-{sequence}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::viewer_launch_contract::{
        BuildCacheTask, VIEWER_INTEGRITY_PREFIX, VIEWER_LAUNCH_ACK_SCHEMA, VIEWER_LAUNCH_SCHEMA,
        VIEWER_READY_PREFIX, VIEWER_READY_SCHEMA, ViewerContent, ViewerIntegrity, ViewerLaunchAck,
        ViewerReady, ViewerTask,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lba-viewer-contract-{name}-{}-{nonce}.json",
            std::process::id()
        ))
    }

    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    fn build_contract() -> ViewerLaunchContract {
        ViewerLaunchContract::new(
            "nova-contract-test".to_owned(),
            absolute("scene.litematic"),
            ViewerContent::default(),
            None,
            ViewerTask::BuildCache(BuildCacheTask {
                initial_layer_view: None,
                ready_file: absolute("ready.json"),
                cache_file: absolute("scene.cache"),
                build_mode: ::viewer_launch_contract::RenderBuildMode::Prebuilt,
                effects_profile: viewer_launch_contract::ViewerEffectsProfile::Full,
            }),
        )
        .unwrap()
    }

    #[test]
    fn contract_file_is_utf8_without_bom_and_removed_on_drop() {
        let path = fixture_path("utf8");
        let backend_path = fixture_path("backend").with_extension("exe");
        fs::write(&backend_path, b"viewer-backend-test").unwrap();
        let backend = ViewerBackendIdentity::capture(&backend_path).unwrap();
        let contract_file =
            ViewerLaunchContractFile::create(path.clone(), &build_contract(), &backend).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        assert_eq!(
            ViewerLaunchContract::from_json_slice(&bytes).unwrap(),
            build_contract()
        );
        drop(contract_file);
        assert!(!path.exists());
        fs::remove_file(backend_path).unwrap();
    }

    #[test]
    fn session_command_is_published_once_after_a_complete_atomic_write() {
        let path = fixture_path("session-contract");
        let session_directory = fixture_path("session-directory").with_extension("");
        let backend_path = fixture_path("session-backend").with_extension("exe");
        fs::write(&backend_path, b"viewer-backend-test").unwrap();
        let backend = ViewerBackendIdentity::capture(&backend_path).unwrap();
        let contract_file =
            ViewerLaunchContractFile::create(path, &build_contract(), &backend).unwrap();

        let command_path = contract_file
            .publish_session_command(&session_directory, "nova-session-test", 3)
            .unwrap();
        assert_eq!(
            command_path.file_name().unwrap(),
            "command-00000000000000000003.json"
        );
        assert!(!command_path.with_extension("json.tmp").exists());
        let command =
            ViewerSessionCommand::from_json_slice(&fs::read(&command_path).unwrap()).unwrap();
        assert_eq!(command.session_id, "nova-session-test");
        assert_eq!(command.sequence, 3);
        assert_eq!(
            command.launch_contract.as_deref(),
            Some(contract_file.path())
        );

        fs::remove_dir_all(session_directory).unwrap();
        drop(contract_file);
        fs::remove_file(backend_path).unwrap();
    }

    #[test]
    fn ack_must_match_the_exact_contract_hash() {
        let path = fixture_path("ack");
        let backend_path = fixture_path("backend").with_extension("exe");
        fs::write(&backend_path, b"viewer-backend-test").unwrap();
        let backend = ViewerBackendIdentity::capture(&backend_path).unwrap();
        let contract_file =
            ViewerLaunchContractFile::create(path, &build_contract(), &backend).unwrap();
        let ack = ViewerLaunchAck {
            schema: VIEWER_LAUNCH_ACK_SCHEMA.to_owned(),
            contract_schema: VIEWER_LAUNCH_SCHEMA.to_owned(),
            request_id: "nova-contract-test".to_owned(),
            contract_sha256: contract_file.sha256().to_owned(),
            backend_git_sha: "a".repeat(40),
            backend_profile: "debug".to_owned(),
            backend_exe_path: contract_file.expected_backend_exe_path().to_path_buf(),
            backend_sha256: contract_file.expected_backend_sha256().to_owned(),
        };
        let output = format!(
            "{VIEWER_LAUNCH_ACK_PREFIX}{}",
            serde_json::to_string(&ack).unwrap()
        );
        assert!(contract_file.accepted_ack(&output).unwrap());

        let mismatched = output.replace(contract_file.sha256(), &"c".repeat(64));
        assert!(
            contract_file
                .accepted_ack(&mismatched)
                .unwrap_err()
                .contains("SHA-256 mismatch")
        );
        fs::remove_file(backend_path).unwrap();
    }

    #[test]
    fn hidden_ready_requires_contract_bound_integrity_pid_and_first_frame() {
        let path = fixture_path("ready");
        let backend_path = fixture_path("backend").with_extension("exe");
        fs::write(&backend_path, b"viewer-backend-test").unwrap();
        let backend = ViewerBackendIdentity::capture(&backend_path).unwrap();
        let contract_file =
            ViewerLaunchContractFile::create(path, &build_contract(), &backend).unwrap();
        let integrity = ViewerIntegrity::new(
            "nova-contract-test".to_owned(),
            contract_file.sha256().to_owned(),
            0,
            0,
            0,
            0,
            Vec::new(),
        )
        .unwrap();
        let ready = ViewerReady {
            schema: VIEWER_READY_SCHEMA.to_owned(),
            request_id: "nova-contract-test".to_owned(),
            contract_sha256: contract_file.sha256().to_owned(),
            pid: 4242,
            hwnd: "9876".to_owned(),
            visible: false,
            runtime_ready: true,
            first_frame_presented: true,
            cache_file_bytes: 4096,
        };
        let output = format!(
            "{VIEWER_INTEGRITY_PREFIX}{}\n{VIEWER_READY_PREFIX}{}",
            serde_json::to_string(&integrity).unwrap(),
            serde_json::to_string(&ready).unwrap(),
        );
        assert!(contract_file.accepted_viewer_integrity(&output).unwrap());
        assert_eq!(
            contract_file
                .accepted_hidden_viewer_ready(&output, 4242)
                .unwrap()
                .unwrap()
                .1,
            9876
        );
        assert!(
            contract_file
                .accepted_hidden_viewer_ready(&output, 4243)
                .unwrap_err()
                .contains("PID mismatch")
        );
        fs::remove_file(backend_path).unwrap();
    }
}
