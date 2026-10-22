//! Explicit local-source development only. This code is absent from production
//! loaders; process environment variables cannot enable it in a release build.
use anyhow::{Context, Result, ensure};
use std::{fs::File, io::Read, path::{Path, PathBuf}};
pub use nova_protection::runtime::{DevelopmentRuntime, DevelopmentTask};

pub const CAPABILITY_MASK: u64 = 255;

pub fn runtime() -> Result<DevelopmentRuntime> {
    nova_protection::runtime::compiled_development_runtime()
}

pub fn source_module() -> Result<PathBuf> {
    let path = option_env!("NOVA_DEVELOPMENT_MODULE_PATH")
        .filter(|value| !value.is_empty())
        .context("development module was not selected at compile time")?;
    let path = PathBuf::from(path);
    ensure!(path.is_absolute(), "compiled development module path must be absolute");
    path.canonicalize().context("resolve local development module")
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
    context.verify()?;
    if let Some(ticket) = &context.remote_ticket_path {
        let ticket_parent = ticket.parent().context("development service ticket directory is absent")?.canonicalize()?;
        let context_parent = path.parent().context("development task directory is absent")?.canonicalize()?;
        ensure!(ticket_parent == context_parent, "development service ticket leaves its Host task directory");
    }
    Ok(context)
}

/// Hosts and public loaders require a compile-time selected, matching module.
pub fn validate_task_context(context: &DevelopmentTask) -> Result<()> {
    context.verify()?;
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
fn remote_config(identity_dir: PathBuf) -> Result<(nova_protection::client::ClientConfig, String)> {
    let policy = nova_protection::runtime::contract();
    let target = nova_protection::runtime::target()?;
    let service = policy.approved_release_runtimes.iter().find(|value| value.target == target)
        .context("no approved shared-light service identity for the development target")?;
    let public_key = policy.trusted_manifest_public_keys.first().context("shared-light verification key is absent")?;
    let config = nova_protection::client::ClientConfig {
        endpoint: super::DEFAULT_ENDPOINT.into(),
        build_id: service.runtime_build_id.clone(),
        module_id: super::MODULE_ID.into(),
        manifest_public_key: public_key.clone(),
        identity_dir,
    };
    Ok((config, service.module_manifest_sha256.clone()))
}

/// Acquire once in the Host. Child processes resume this genuine signed grant;
/// they must not create unrelated HPKE recipients for the same idempotent task.
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

pub fn remote_session(context: &DevelopmentTask, identity_dir: impl Into<PathBuf>) -> Result<Option<nova_protection::client::TaskSession>> {
    context.verify()?;
    let Some(path) = &context.remote_ticket_path else { return Ok(None); };
    let (config, expected_manifest) = remote_config(identity_dir.into())?;
    let ticket = super::read_ticket(path).context("Host has not acquired the shared-light service task")?;
    ensure!(ticket.binding().task_id == context.task_id && nova_protection::runtime::manifest_identity(&ticket.response.signed_manifest)? == expected_manifest, "development service grant and task differ");
    Ok(Some(nova_protection::client::TaskSession::resume_ticket(&config, ticket)?))
}
