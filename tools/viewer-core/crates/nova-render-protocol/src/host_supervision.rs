//! Narrow control-plane messages used when render-service asks the application
//! host to create or stop one of Nova's own child processes.
//!
//! These messages intentionally contain only launch metadata. Scene contents,
//! artifacts, renderer state and scheduler decisions stay on the render data
//! plane and must never be added here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

const MAX_ID_BYTES: usize = 160;
const MAX_LAUNCH_TOKEN_BYTES: usize = 512;
const MAX_ARGUMENTS: usize = 256;
const MAX_ARGUMENT_BYTES: usize = 8 * 1024;
const MAX_ENVIRONMENT_ENTRIES: usize = 128;
const MAX_ENVIRONMENT_NAME_BYTES: usize = 256;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 8 * 1024;
const MAX_TOTAL_LAUNCH_TEXT_BYTES: usize = 64 * 1024;
const MAX_PATH_BYTES: usize = 32 * 1024;
const MAX_ERROR_MESSAGE_BYTES: usize = 4 * 1024;

/// Executables the application host may resolve through its fixed allowlist.
///
/// No request carries an executable path. `RenderService` is included so the
/// host can use the same identity vocabulary for its locally initiated child;
/// accepting that role from a remote peer remains an application-host policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableRole {
    RenderService,
    BuildWorker,
    NativeViewer,
    CoreHelper,
}

/// The full lease identity of one supervised process.
///
/// A PID is deliberately not part of this tuple because Windows can reuse it.
/// Every response and exit notification is matched against all four fields so
/// an old generation cannot affect a replacement child.
#[derive(Clone, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HostLaunchIdentity {
    pub child_id: String,
    pub owner_epoch: u64,
    pub generation: u64,
    pub launch_token: String,
}

impl HostLaunchIdentity {
    pub fn validate(&self) -> Result<(), String> {
        require_id("childId", &self.child_id)?;
        if self.owner_epoch == 0 {
            return Err("ownerEpoch must be non-zero".to_owned());
        }
        if self.generation == 0 {
            return Err("generation must be non-zero".to_owned());
        }
        require_token(&self.launch_token)
    }

    /// Rejects a request from a previous application instance or render
    /// generation before any OS process operation is attempted.
    pub fn validate_current(&self, owner_epoch: u64, generation: u64) -> Result<(), String> {
        self.validate()?;
        if self.owner_epoch != owner_epoch || self.generation != generation {
            return Err("stale host launch identity".to_owned());
        }
        Ok(())
    }

    /// Verifies the complete lease tuple, including the opaque launch token.
    pub fn validate_same_lease(&self, expected: &Self) -> Result<(), String> {
        self.validate()?;
        expected.validate()?;
        if self != expected {
            return Err("stale host launch identity tuple".to_owned());
        }
        Ok(())
    }
}

impl fmt::Debug for HostLaunchIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostLaunchIdentity")
            .field("child_id", &self.child_id)
            .field("owner_epoch", &self.owner_epoch)
            .field("generation", &self.generation)
            .field("launch_token", &"[redacted]")
            .finish()
    }
}

/// Where the host connects one child output stream.
///
/// File contents never cross this protocol; only a bounded path and append
/// policy are transmitted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "target", rename_all = "snake_case")]
pub enum HostOutputTarget {
    Null,
    File { path: PathBuf, append: bool },
}

impl HostOutputTarget {
    fn validate(&self, field: &str) -> Result<(), String> {
        match self {
            Self::Null => Ok(()),
            Self::File { path, .. } => require_absolute_path(field, path),
        }
    }
}

/// Bounded metadata needed for one allowlisted process launch.
///
/// Arguments are opaque to the host's render semantics. The host validates the
/// role and basic launch contract, then resolves the executable itself.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LaunchSpec {
    pub role: ExecutableRole,
    pub identity: HostLaunchIdentity,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub current_directory: Option<PathBuf>,
    pub stdout: HostOutputTarget,
    pub stderr: HostOutputTarget,
    pub hidden: bool,
    pub new_process_group: bool,
}

impl LaunchSpec {
    pub fn validate(&self) -> Result<(), String> {
        self.identity.validate()?;

        if self.args.len() > MAX_ARGUMENTS {
            return Err(format!("args must contain at most {MAX_ARGUMENTS} entries"));
        }
        if self.environment.len() > MAX_ENVIRONMENT_ENTRIES {
            return Err(format!(
                "environment must contain at most {MAX_ENVIRONMENT_ENTRIES} entries"
            ));
        }

        let mut total_text_bytes = 0_usize;
        for argument in &self.args {
            require_text("argument", argument, MAX_ARGUMENT_BYTES, true)?;
            total_text_bytes = total_text_bytes.saturating_add(argument.len());
        }
        for (name, value) in &self.environment {
            require_text("environment name", name, MAX_ENVIRONMENT_NAME_BYTES, false)?;
            if name.contains('=') {
                return Err("environment name must not contain '='".to_owned());
            }
            require_text(
                "environment value",
                value,
                MAX_ENVIRONMENT_VALUE_BYTES,
                true,
            )?;
            total_text_bytes = total_text_bytes
                .saturating_add(name.len())
                .saturating_add(value.len());
        }
        if total_text_bytes > MAX_TOTAL_LAUNCH_TEXT_BYTES {
            return Err(format!(
                "launch text metadata must contain at most {MAX_TOTAL_LAUNCH_TEXT_BYTES} bytes"
            ));
        }

        if let Some(path) = &self.current_directory {
            require_absolute_path("currentDirectory", path)?;
        }
        self.stdout.validate("stdout.path")?;
        self.stderr.validate("stderr.path")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    tag = "operation",
    content = "data",
    rename_all = "snake_case"
)]
pub enum HostProcessCommand {
    Spawn(LaunchSpec),
    Terminate {
        role: ExecutableRole,
        identity: HostLaunchIdentity,
    },
}

/// One render-service request to the application-owned supervisor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HostProcessRequest {
    pub request_id: String,
    pub command: HostProcessCommand,
}

impl HostProcessRequest {
    pub fn validate(&self) -> Result<(), String> {
        require_id("requestId", &self.request_id)?;
        match &self.command {
            HostProcessCommand::Spawn(spec) => spec.validate(),
            HostProcessCommand::Terminate { identity, .. } => identity.validate(),
        }
    }

    pub fn role(&self) -> ExecutableRole {
        match &self.command {
            HostProcessCommand::Spawn(spec) => spec.role,
            HostProcessCommand::Terminate { role, .. } => *role,
        }
    }

    pub fn identity(&self) -> &HostLaunchIdentity {
        match &self.command {
            HostProcessCommand::Spawn(spec) => &spec.identity,
            HostProcessCommand::Terminate { identity, .. } => identity,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostSpawnDisposition {
    Created,
    Reused,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    tag = "status",
    content = "data",
    rename_all = "snake_case"
)]
pub enum HostProcessOutcome {
    Spawned {
        pid: u32,
        disposition: HostSpawnDisposition,
    },
    Terminated {
        #[serde(rename = "alreadyExited")]
        already_exited: bool,
    },
    Failed {
        code: String,
        message: String,
        retryable: bool,
    },
}

/// Correlated result for exactly one host process request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HostProcessResponse {
    pub request_id: String,
    pub role: ExecutableRole,
    pub identity: HostLaunchIdentity,
    pub outcome: HostProcessOutcome,
}

impl HostProcessResponse {
    pub fn validate(&self) -> Result<(), String> {
        require_id("requestId", &self.request_id)?;
        self.identity.validate()?;
        match &self.outcome {
            HostProcessOutcome::Spawned { pid, .. } if *pid == 0 => {
                Err("spawned pid must be non-zero".to_owned())
            }
            HostProcessOutcome::Failed { code, message, .. } => {
                require_id("error code", code)?;
                require_text("error message", message, MAX_ERROR_MESSAGE_BYTES, false)
            }
            _ => Ok(()),
        }
    }

    /// Rejects mis-correlated, role-mismatched and stale-generation replies.
    pub fn validate_against(&self, request: &HostProcessRequest) -> Result<(), String> {
        self.validate()?;
        request.validate()?;
        if self.request_id != request.request_id {
            return Err("host response requestId does not match request".to_owned());
        }
        if self.role != request.role() {
            return Err("host response executable role does not match request".to_owned());
        }
        self.identity.validate_same_lease(request.identity())?;

        match (&request.command, &self.outcome) {
            (HostProcessCommand::Spawn(_), HostProcessOutcome::Terminated { .. }) => {
                Err("spawn request cannot receive a terminated response".to_owned())
            }
            (HostProcessCommand::Terminate { .. }, HostProcessOutcome::Spawned { .. }) => {
                Err("terminate request cannot receive a spawned response".to_owned())
            }
            _ => Ok(()),
        }
    }
}

/// Host-to-render-service crash/exit observation for a supervised child.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HostChildExited {
    pub role: ExecutableRole,
    pub identity: HostLaunchIdentity,
    pub pid: u32,
    pub exit_code: Option<i32>,
    pub termination_requested: bool,
}

impl HostChildExited {
    pub fn validate(&self) -> Result<(), String> {
        self.identity.validate()?;
        if self.pid == 0 {
            return Err("exited child pid must be non-zero".to_owned());
        }
        Ok(())
    }

    /// Ensures a delayed exit notification cannot retire a replacement child
    /// that happens to have the same role or a reused PID.
    pub fn validate_against(
        &self,
        expected_role: ExecutableRole,
        expected_identity: &HostLaunchIdentity,
    ) -> Result<(), String> {
        self.validate()?;
        if self.role != expected_role {
            return Err("exited child executable role does not match lease".to_owned());
        }
        self.identity.validate_same_lease(expected_identity)
    }
}

fn require_id(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!(
            "{field} must contain 1..={MAX_ID_BYTES} ASCII letters, digits, '-', '_' or '.'"
        ));
    }
    Ok(())
}

fn require_token(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_LAUNCH_TOKEN_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "launchToken must contain 1..={MAX_LAUNCH_TOKEN_BYTES} non-control UTF-8 bytes"
        ));
    }
    Ok(())
}

fn require_text(
    field: &str,
    value: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), String> {
    if (!allow_empty && value.is_empty()) || value.len() > max_bytes || value.contains('\0') {
        return Err(format!(
            "{field} must be NUL-free and contain {}..={max_bytes} bytes",
            usize::from(!allow_empty)
        ));
    }
    Ok(())
}

fn require_absolute_path(field: &str, path: &Path) -> Result<(), String> {
    let text = path.to_string_lossy();
    if !path.is_absolute() || text.is_empty() || text.len() > MAX_PATH_BYTES || text.contains('\0')
    {
        return Err(format!(
            "{field} must be a non-empty, NUL-free absolute path of at most {MAX_PATH_BYTES} bytes"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(generation: u64) -> HostLaunchIdentity {
        HostLaunchIdentity {
            child_id: "build-worker-17".to_owned(),
            owner_epoch: 23,
            generation,
            launch_token: "launch-token-abc_123".to_owned(),
        }
    }

    fn spawn_request() -> HostProcessRequest {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\Nova")
        } else {
            PathBuf::from("/tmp/nova")
        };
        HostProcessRequest {
            request_id: "spawn-17".to_owned(),
            command: HostProcessCommand::Spawn(LaunchSpec {
                role: ExecutableRole::BuildWorker,
                identity: identity(41),
                args: vec!["--pipe".to_owned(), "nova-pipe-7".to_owned()],
                environment: BTreeMap::from([("NOVA_BUILD_ID".to_owned(), "build-17".to_owned())]),
                current_directory: Some(root.clone()),
                stdout: HostOutputTarget::File {
                    path: root.join("logs").join("worker.out.log"),
                    append: true,
                },
                stderr: HostOutputTarget::File {
                    path: root.join("logs").join("worker.err.log"),
                    append: true,
                },
                hidden: true,
                new_process_group: true,
            }),
        }
    }

    #[test]
    fn host_request_round_trips_with_generation_and_token_intact() {
        let request = spawn_request();
        request.validate().unwrap();
        let encoded = serde_json::to_vec(&request).unwrap();
        let decoded: HostProcessRequest = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, request);
        assert_eq!(decoded.identity().generation, 41);
        assert_eq!(decoded.identity().launch_token, "launch-token-abc_123");
    }

    #[test]
    fn launch_contract_has_no_scene_or_artifact_payload_fields() {
        fn collect_keys(value: &serde_json::Value, keys: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(object) => {
                    for (key, child) in object {
                        keys.push(key.to_ascii_lowercase());
                        collect_keys(child, keys);
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        collect_keys(item, keys);
                    }
                }
                _ => {}
            }
        }

        let value = serde_json::to_value(spawn_request()).unwrap();
        let mut keys = Vec::new();
        collect_keys(&value, &mut keys);
        assert!(!keys.iter().any(|key| key.contains("scene")));
        assert!(!keys.iter().any(|key| key.contains("artifact")));
        assert!(!keys.iter().any(|key| key.contains("payload")));
        assert!(!keys.iter().any(|key| key.contains("bytes")));
    }

    #[test]
    fn stale_generation_and_token_are_rejected_as_a_complete_tuple() {
        let request = spawn_request();
        let mut stale_generation = identity(42);
        assert!(
            stale_generation
                .validate_same_lease(request.identity())
                .is_err()
        );

        stale_generation.generation = 41;
        stale_generation.launch_token = "replacement-token".to_owned();
        assert!(
            stale_generation
                .validate_same_lease(request.identity())
                .is_err()
        );
        assert!(request.identity().validate_current(23, 41).is_ok());
        assert!(request.identity().validate_current(24, 41).is_err());
    }

    #[test]
    fn response_must_match_request_role_identity_and_operation() {
        let request = spawn_request();
        let valid = HostProcessResponse {
            request_id: request.request_id.clone(),
            role: request.role(),
            identity: request.identity().clone(),
            outcome: HostProcessOutcome::Spawned {
                pid: 907,
                disposition: HostSpawnDisposition::Created,
            },
        };
        assert!(valid.validate_against(&request).is_ok());

        let mut wrong_role = valid.clone();
        wrong_role.role = ExecutableRole::NativeViewer;
        assert!(wrong_role.validate_against(&request).is_err());

        let mut stale = valid.clone();
        stale.identity.generation += 1;
        assert!(stale.validate_against(&request).is_err());

        let mut wrong_operation = valid;
        wrong_operation.outcome = HostProcessOutcome::Terminated {
            already_exited: false,
        };
        assert!(wrong_operation.validate_against(&request).is_err());
    }

    #[test]
    fn response_and_exit_notification_round_trip_without_losing_lease_identity() {
        let request = spawn_request();
        let response = HostProcessResponse {
            request_id: request.request_id.clone(),
            role: request.role(),
            identity: request.identity().clone(),
            outcome: HostProcessOutcome::Spawned {
                pid: 907,
                disposition: HostSpawnDisposition::Reused,
            },
        };
        let encoded = serde_json::to_vec(&response).unwrap();
        let decoded: HostProcessResponse = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, response);
        assert!(decoded.validate_against(&request).is_ok());

        let exited = HostChildExited {
            role: request.role(),
            identity: request.identity().clone(),
            pid: 907,
            exit_code: Some(5),
            termination_requested: true,
        };
        let encoded = serde_json::to_vec(&exited).unwrap();
        let decoded: HostChildExited = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, exited);
        assert!(
            decoded
                .validate_against(request.role(), request.identity())
                .is_ok()
        );
    }

    #[test]
    fn exit_notification_rejects_reused_pid_with_stale_lease() {
        let expected = identity(41);
        let mut exited = HostChildExited {
            role: ExecutableRole::BuildWorker,
            identity: expected.clone(),
            pid: 907,
            exit_code: Some(0),
            termination_requested: false,
        };
        assert!(
            exited
                .validate_against(ExecutableRole::BuildWorker, &expected)
                .is_ok()
        );

        exited.identity.launch_token = "older-launch-token".to_owned();
        assert!(
            exited
                .validate_against(ExecutableRole::BuildWorker, &expected)
                .is_err()
        );
    }

    #[test]
    fn empty_and_oversized_ids_or_tokens_are_rejected() {
        let mut value = identity(1);
        value.child_id.clear();
        assert!(value.validate().is_err());

        value = identity(1);
        value.launch_token.clear();
        assert!(value.validate().is_err());

        value.launch_token = "x".repeat(MAX_LAUNCH_TOKEN_BYTES + 1);
        assert!(value.validate().is_err());

        value = identity(0);
        assert!(value.validate().is_err());

        value = identity(1);
        value.owner_epoch = 0;
        assert!(value.validate().is_err());

        let mut request = spawn_request();
        request.request_id.clear();
        assert!(request.validate().is_err());
        request.request_id = "x".repeat(MAX_ID_BYTES + 1);
        assert!(request.validate().is_err());
    }
}
