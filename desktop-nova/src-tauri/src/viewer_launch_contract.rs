//! Public quick-preview launch file and executable identity validation only.
use crate::viewer_backend::ViewerBackendIdentity;
use ::viewer_launch_contract::{ViewerLaunchAck, ViewerLaunchContract, VIEWER_LAUNCH_ACK_PREFIX};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

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

    pub(crate) fn accepted_ack(&self, output: &str) -> Result<bool, String> {
        accepted_launch_ack(
            output,
            &self.request_id,
            &self.sha256,
            self.backend.exe_path(),
            self.backend.sha256(),
        )
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
