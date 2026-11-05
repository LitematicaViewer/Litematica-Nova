//! Explicit local-source development only. This code is absent from production
//! loaders; process environment variables cannot enable it in a release build.
//! The separate standalone feature selects a packaged, wholly local candidate.
use anyhow::{Context, Result, ensure};
use std::{fs::File, io::Read, path::{Path, PathBuf}};
pub use nova_protection::runtime::{DevelopmentRuntime, DevelopmentTask};

pub const CAPABILITY_MASK: u64 = 255 | if cfg!(feature = "standalone-runtime") { 256 } else { 0 };

pub fn runtime() -> Result<DevelopmentRuntime> {
    let runtime = nova_protection::runtime::compiled_development_runtime()?;
    #[cfg(feature = "standalone-runtime")]
    ensure!(runtime.capabilities.iter().any(|value| value == "standalone-light"), "standalone runtime lacks local light capability");
    Ok(runtime)
}

#[cfg(not(feature = "standalone-runtime"))]
pub fn source_module() -> Result<PathBuf> {
    let path = option_env!("NOVA_DEVELOPMENT_MODULE_PATH")
        .filter(|value| !value.is_empty())
        .context("development module was not selected at compile time")?;
    let path = PathBuf::from(path);
    ensure!(path.is_absolute(), "compiled development module path must be absolute");
    path.canonicalize().context("resolve local development module")
}

/// A standalone candidate relocates with its package; its runtime never reads
/// the builder's private source path or a process-selected module location.
#[cfg(feature = "standalone-runtime")]
pub fn source_module(backend: &Path) -> Result<PathBuf> {
    backend.join("local").join(super::MODULE_ID).canonicalize()
        .context("resolve packaged standalone module")
}

fn verify_task_policy(context: &DevelopmentTask) -> Result<()> {
    context.verify()?;
    #[cfg(feature = "standalone-runtime")]
    ensure!(!context.requires_shared_light && context.remote_ticket_path.is_none(), "standalone task must not contain remote shared-light authorization");
    Ok(())
}

/// The private module cannot embed its own final DLL hash. It reads this local
/// context only after the external Host/loader has authenticated the DLL bytes.
pub fn read_task_context(path: &Path) -> Result<DevelopmentTask> {
    let mut file = File::open(path).context("open development task context")?;
    ensure!(file.metadata()?.is_file() && file.metadata()?.len() <= super::MAX_TICKET_BYTES, "development task context exceeds its byte budget");
    let mut bytes = Vec::new();
    file.by_ref().take(super::MAX_TICKET_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= super::MAX_TICKET_BYTES, "development task context exceeds its byte budget");
    let context: DevelopmentTask = serde_json::from_slice(&bytes).context("invalid development task context")?;
    verify_task_policy(&context)?;
    if let Some(ticket) = &context.remote_ticket_path {
        let ticket_parent = ticket.parent().context("development service ticket directory is absent")?.canonicalize()?;
        let context_parent = path.parent().context("development task directory is absent")?.canonicalize()?;
        ensure!(ticket_parent == context_parent, "development service ticket leaves its Host task directory");
    }
    Ok(context)
}

/// Hosts and public loaders require a compile-time selected, matching module.
pub fn validate_task_context(context: &DevelopmentTask) -> Result<()> {
    verify_task_policy(context)?;
    let selected = runtime()?;
    ensure!(context.runtime_build_id == selected.runtime_build_id && context.module_sha256 == selected.module_sha256, "development task and compiled module differ");
    Ok(())
}

pub fn verify_module(file: &mut File, context: &DevelopmentTask) -> Result<()> {
    validate_task_context(context)?;
    super::verify_module_hash(file, &context.module_sha256)
}

pub fn verify_capabilities(mask: u64) -> Result<()> {
    ensure!(mask & CAPABILITY_MASK == CAPABILITY_MASK, "development module capability exports are incomplete");
    Ok(())
}

/// Local module identity is NOT a production service identity. Standard never
/// contacts this service. Full uses an existing approved service build solely
/// for device-proven remote shared-light RPCs, without downloading its module.
#[cfg(not(feature = "standalone-runtime"))]
fn remote_config(identity_dir: PathBuf) -> Result<(nova_protection::client::ClientConfig, String)> {
    let policy = nova_protection::runtime::contract();
    let target = nova_protection::runtime::target()?;
    let service = policy.approved_release_runtimes.iter().find(|value| value.target == target)
        .context("no approved shared-light service identity for the development target")?;
    let public_key = policy.trusted_manifest_public_keys.first().context("shared-light verification key is absent")?;
    let config = nova_protection::client::ClientConfig {
        endpoint: nova_protection::client::service_url("nova-core")?,
        build_id: service.runtime_build_id.clone(),
        module_id: super::MODULE_ID.into(),
        manifest_public_key: public_key.clone(),
        identity_dir,
    };
    Ok((config, service.module_manifest_sha256.clone()))
}

/// Acquire once in the Host. Child processes resume this genuine signed grant;
/// they must not create unrelated HPKE recipients for the same idempotent task.
#[cfg(not(feature = "standalone-runtime"))]
pub fn prepare_remote_task(context: &DevelopmentTask, identity_dir: impl Into<PathBuf>) -> Result<()> {
    context.verify()?;
    let Some(path) = &context.remote_ticket_path else { return Ok(()); };
    let (config, expected_manifest) = remote_config(identity_dir.into())?;
    if path.exists() {
        let ticket = super::read_ticket(path)?;
        ticket.verify(&config)?;
        ensure!(ticket.binding().task_id == context.task_id && nova_protection::runtime::manifest_identity(&ticket.response.signed_manifest)? == expected_manifest, "development service grant and task differ");
        return Ok(());
    }
    let mut session = nova_protection::client::TaskSession::acquire(&config, &context.task_id)?;
    let ticket = session.ticket();
    ensure!(nova_protection::runtime::manifest_identity(&ticket.response.signed_manifest)? == expected_manifest, "shared-light service returned an unapproved module identity");
    session.release_content_key();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options.open(path)?.write_all(&serde_json::to_vec(&ticket)?)?;
    Ok(())
}

#[cfg(not(feature = "standalone-runtime"))]
pub fn remote_session(context: &DevelopmentTask, identity_dir: impl Into<PathBuf>) -> Result<Option<nova_protection::client::TaskSession>> {
    context.verify()?;
    let Some(path) = &context.remote_ticket_path else { return Ok(None); };
    let (config, expected_manifest) = remote_config(identity_dir.into())?;
    let ticket = super::read_ticket(path).context("Host has not acquired the shared-light service task")?;
    ensure!(ticket.binding().task_id == context.task_id && nova_protection::runtime::manifest_identity(&ticket.response.signed_manifest)? == expected_manifest, "development service grant and task differ");
    Ok(Some(nova_protection::client::TaskSession::resume_ticket(&config, ticket)?))
}

#[cfg(feature = "standalone-runtime")]
pub fn prepare_remote_task(context: &DevelopmentTask, _identity_dir: impl Into<PathBuf>) -> Result<()> {
    verify_task_policy(context)
}

#[cfg(feature = "standalone-runtime")]
pub fn remote_session(context: &DevelopmentTask, _identity_dir: impl Into<PathBuf>) -> Result<Option<nova_protection::client::TaskSession>> {
    verify_task_policy(context)?;
    Ok(None)
}

#[cfg(all(test, feature = "standalone-runtime"))]
mod standalone_tests {
    use super::*;

    fn local_task() -> DevelopmentTask {
        DevelopmentTask {
            schema: "nova.development-task.v1".into(),
            runtime_build_id: "standalone-fixture".into(),
            module_sha256: "ab".repeat(32),
            task_id: "local-fixture".into(),
            requires_shared_light: false,
            remote_ticket_path: None,
        }
    }

    #[test]
    fn standalone_local_task_needs_no_service_identity() {
        let context = local_task();
        prepare_remote_task(&context, "unused-device-identity").unwrap();
        assert!(remote_session(&context, "unused-device-identity").unwrap().is_none());
    }

    #[test]
    fn standalone_rejects_remote_context_at_every_entry() {
        let directory = tempfile::tempdir().unwrap();
        let mut context = local_task();
        context.requires_shared_light = true;
        context.remote_ticket_path = Some(directory.path().join("remote.json"));
        context.verify().unwrap();
        let path = directory.path().join("task.json");
        std::fs::write(&path, serde_json::to_vec(&context).unwrap()).unwrap();
        assert!(read_task_context(&path).is_err());
        assert!(validate_task_context(&context).is_err());
        assert!(prepare_remote_task(&context, directory.path()).is_err());
        assert!(remote_session(&context, directory.path()).is_err());
    }

    #[test]
    fn standalone_requires_module_local_light_export() {
        assert!(verify_capabilities(255).is_err());
        verify_capabilities(511).unwrap();
    }

    #[test]
    fn standalone_source_resolves_only_the_package_module() {
        let directory = tempfile::tempdir().unwrap();
        let local = directory.path().join("local");
        std::fs::create_dir(&local).unwrap();
        let module = local.join(super::super::MODULE_ID);
        std::fs::write(&module, b"packaged fixture").unwrap();
        assert_eq!(source_module(directory.path()).unwrap(), module.canonicalize().unwrap());
    }
}
