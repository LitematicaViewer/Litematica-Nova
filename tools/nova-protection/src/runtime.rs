//! Signed runtime compatibility is independent of the desktop product version.
//! Legacy module manifests and ciphertext stay byte-for-byte unchanged: this
//! signed statement binds their authenticated identity to the public Host ABI.
use crate::crypto::{self, SignedManifest};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const RUNTIME_ABI: u32 = 1;
pub const COMPATIBILITY_FILE: &str = "runtime-compatibility.json";

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedRuntime {
    pub target: String,
    pub runtime_build_id: String,
    pub module_manifest_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeContract {
    pub schema: String,
    pub runtime_abi: u32,
    pub required_capabilities: Vec<String>,
    pub targets: Vec<String>,
    pub trusted_manifest_public_keys: Vec<String>,
    pub approved_release_runtimes: Vec<ApprovedRuntime>,
}

pub fn contract() -> &'static RuntimeContract {
    static CONTRACT: OnceLock<RuntimeContract> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        let value: RuntimeContract = serde_json::from_str(include_str!("../runtime-contract.json"))
            .expect("the source-controlled runtime contract must be valid");
        assert_eq!(value.schema, "nova.runtime-contract.v1");
        assert_eq!(value.runtime_abi, RUNTIME_ABI);
        value
    })
}

/// Field order is the canonical JSON signing format, shared with the private
/// packaging script. AppVersion deliberately does not participate in this ABI.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCompatibility {
    pub schema: String,
    pub runtime_abi: u32,
    pub runtime_build_id: String,
    pub target: String,
    pub module_id: String,
    pub module_manifest_sha256: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub created_ms: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedRuntimeCompatibility {
    pub compatibility: RuntimeCompatibility,
    pub signature: String,
}

pub fn manifest_identity(manifest: &SignedManifest) -> Result<String> {
    Ok(crypto::sha256(&serde_json::to_vec(manifest)?))
}

pub fn sign_runtime_compatibility(
    compatibility: RuntimeCompatibility,
    signing: &ring::signature::Ed25519KeyPair,
) -> Result<SignedRuntimeCompatibility> {
    let signature = crypto::b64(signing.sign(&serde_json::to_vec(&compatibility)?).as_ref());
    Ok(SignedRuntimeCompatibility { compatibility, signature })
}

pub fn target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => anyhow::bail!("unsupported protected runtime target"),
    }
}

impl SignedRuntimeCompatibility {
    pub fn verify(&self, public_key: &str, expected_target: &str) -> Result<()> {
        let policy = contract();
        ensure!(
            policy.trusted_manifest_public_keys.iter().any(|key| key == public_key),
            "runtime signing key is not trusted by this Host"
        );
        crypto::verify_server(
            &crypto::unb64(public_key)?,
            &serde_json::to_vec(&self.compatibility)?,
            &self.signature,
        )?;
        let value = &self.compatibility;
        ensure!(value.schema == "nova.runtime-compatibility.v1", "runtime compatibility schema differs");
        ensure!(value.runtime_abi == policy.runtime_abi, "protected module ABI version differs");
        ensure!(value.target == expected_target && policy.targets.contains(&value.target), "protected runtime target differs");
        ensure!(value.created_ms > 0, "runtime compatibility time is invalid");
        ensure!(
            policy.required_capabilities.iter().all(|required| value.capabilities.contains(required)),
            "protected runtime is missing required Host capabilities"
        );
        Ok(())
    }

    pub fn verify_manifest(&self, manifest: &SignedManifest, public_key: &str) -> Result<()> {
        crypto::verify_manifest(manifest, &crypto::unb64(public_key)?)?;
        ensure!(
            self.compatibility.runtime_build_id == manifest.manifest.build_id
                && self.compatibility.module_id == manifest.manifest.module_id
                && self.compatibility.module_manifest_sha256 == manifest_identity(manifest)?,
            "runtime compatibility and signed module identity differ"
        );
        Ok(())
    }

    pub fn verify_release_approval(&self) -> Result<()> {
        let value = &self.compatibility;
        ensure!(
            contract().approved_release_runtimes.iter().any(|approved|
                approved.target == value.target
                    && approved.runtime_build_id == value.runtime_build_id
                    && approved.module_manifest_sha256 == value.module_manifest_sha256),
            "runtime combination is not approved for a formal release"
        );
        Ok(())
    }
}

/// The build entry embeds the already verified signed statement. This is never
/// read from the runtime process environment, package-selected keys or tickets.
pub fn compiled_compatibility() -> Result<Option<SignedRuntimeCompatibility>> {
    option_env!("NOVA_PROTECTED_RUNTIME_COMPATIBILITY")
        .filter(|value| !value.is_empty())
        .map(|value| serde_json::from_str(value).context("compiled runtime compatibility is invalid"))
        .transpose()
}

/// A local source build has integrity/ABI metadata, not a production signature.
/// Only explicitly feature-enabled development Hosts/loaders consume this type.
#[cfg(feature = "development-runtime")]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentRuntime {
    pub schema: String,
    pub runtime_abi: u32,
    pub runtime_build_id: String,
    pub target: String,
    pub module_id: String,
    pub module_sha256: String,
    pub capabilities: Vec<String>,
}

#[cfg(feature = "development-runtime")]
impl DevelopmentRuntime {
    pub fn verify(&self) -> Result<()> {
        ensure!(self.schema == "nova.development-runtime.v1", "development runtime schema differs");
        ensure!(self.runtime_abi == RUNTIME_ABI, "development runtime ABI differs");
        ensure!(self.target == target()?, "development runtime target differs");
        let module = if self.target.contains("windows") { "nova_protected_module.dll" }
            else if self.target.contains("apple") { "libnova_protected_module.dylib" }
            else { "libnova_protected_module.so" };
        ensure!(self.module_id == module, "development runtime module differs");
        ensure!(crate::protocol::valid_id(&self.runtime_build_id), "invalid development build identity");
        ensure!(valid_content_hash(&self.module_sha256), "invalid development module hash");
        ensure!(contract().required_capabilities.iter().all(|required| self.capabilities.contains(required)), "development runtime lacks required capabilities");
        Ok(())
    }
}

#[cfg(feature = "development-runtime")]
pub fn compiled_development_runtime() -> Result<DevelopmentRuntime> {
    let json = option_env!("NOVA_DEVELOPMENT_RUNTIME_JSON")
        .filter(|value| !value.is_empty())
        .context("development runtime metadata was not selected at compile time")?;
    let value: DevelopmentRuntime = serde_json::from_str(json).context("invalid compiled development runtime")?;
    value.verify()?;
    Ok(value)
}

/// This local launch context is deliberately not a TaskTicket and grants no
/// server access. Full separately obtains a genuine device-proven service task.
#[cfg(feature = "development-runtime")]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentTask {
    pub schema: String,
    pub runtime_build_id: String,
    pub module_sha256: String,
    pub task_id: String,
    pub requires_shared_light: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_ticket_path: Option<std::path::PathBuf>,
}

#[cfg(feature = "development-runtime")]
impl DevelopmentTask {
    pub fn verify(&self) -> Result<()> {
        ensure!(self.schema == "nova.development-task.v1", "development task schema differs");
        ensure!(crate::protocol::valid_id(&self.runtime_build_id) && crate::protocol::valid_id(&self.task_id), "invalid development task identity");
        ensure!(valid_content_hash(&self.module_sha256), "invalid development task module hash");
        ensure!(self.requires_shared_light == self.remote_ticket_path.is_some(), "development shared-light authorization path is incomplete");
        if let Some(path) = &self.remote_ticket_path {
            ensure!(path.is_absolute(), "development shared-light ticket path must be absolute");
        }
        Ok(())
    }
}

#[cfg(feature = "development-runtime")]
fn valid_content_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
