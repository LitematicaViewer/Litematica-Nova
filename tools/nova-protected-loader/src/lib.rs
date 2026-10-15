//! Public, ordinary Windows loader. This crate contains no rendering algorithms.
//! The Host decrypts a signed module into its private runtime directory; this
//! loader only verifies the materialized file and calls its documented ABI.
use anyhow::{Context, Result, ensure};
use nova_protection::client::{ClientConfig, TaskTicket};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

pub const ABI_VERSION: u32 = 1;
pub const MODULE_ID: &str = "nova_protected_module.dll";
const DEFAULT_ENDPOINT: &str = "https://45-62-112-121.sslip.io:8448/nova-core";
const MAX_TICKET_BYTES: u64 = 128 * 1024;
const MAX_MODULE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Role {
    RenderService = 1,
    BuildWorker = 2,
    NativeViewer = 3,
    CpuAudit = 4,
}
impl TryFrom<u32> for Role {
    type Error = anyhow::Error;
    fn try_from(value: u32) -> Result<Self> {
        match value {
            1 => Ok(Self::RenderService),
            2 => Ok(Self::BuildWorker),
            3 => Ok(Self::NativeViewer),
            4 => Ok(Self::CpuAudit),
            _ => anyhow::bail!("unknown protected module role"),
        }
    }
}

/// Trust roots are compiled by the private release builder, never read from
/// process environment or accepted from the untrusted ticket itself.
pub fn trusted_config(identity_dir: impl Into<PathBuf>) -> Result<ClientConfig> {
    let public_key = option_env!("NOVA_PROTECTED_MANIFEST_PUBLIC_KEY")
        .filter(|value| !value.is_empty())
        .context("release manifest verification key is not configured")?;
    ensure!(
        nova_protection::crypto::unb64(public_key)?.len() == 32,
        "release manifest verification key is malformed"
    );
    let build_id = option_env!("NOVA_PROTECTED_BUILD_ID")
        .filter(|value| !value.is_empty())
        .context("protected release build identity is not configured")?;
    let endpoint = option_env!("NOVA_PROTECTED_ENDPOINT").unwrap_or(DEFAULT_ENDPOINT);
    ensure!(
        endpoint.starts_with("https://"),
        "protected production endpoint must use HTTPS"
    );
    Ok(ClientConfig {
        endpoint: endpoint.into(),
        build_id: build_id.into(),
        module_id: MODULE_ID.into(),
        manifest_public_key: public_key.into(),
        identity_dir: identity_dir.into(),
    })
}

pub fn config_for_ticket(ticket: &TaskTicket) -> Result<ClientConfig> {
    let identity_root = std::env::var_os("NOVA_USER_DATA_ROOT")
        .map(PathBuf::from)
        .context("Nova user data root is not configured")?;
    let config = trusted_config(identity_root.join("protection").join("devices"))?;
    ticket.verify(&config)?;
    Ok(config)
}

pub fn read_ticket(path: &Path) -> Result<TaskTicket> {
    let file = File::open(path).context("open protected task ticket")?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() <= MAX_TICKET_BYTES,
        "protected task ticket exceeds its byte budget"
    );
    let mut bytes = Vec::new();
    file.take(MAX_TICKET_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_TICKET_BYTES,
        "protected task ticket exceeds its byte budget"
    );
    let result = serde_json::from_slice(&bytes).context("parse protected task ticket");
    bytes.zeroize();
    result
}

fn verify_locked_module(file: &mut File, ticket: &TaskTicket) -> Result<()> {
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= MAX_MODULE_BYTES,
        "protected module exceeds its byte budget"
    );
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0_u8; 64 * 1024];
    let result = (|| -> Result<()> {
        let mut total = 0_u64;
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            ensure!(
                total <= MAX_MODULE_BYTES,
                "protected module exceeds its byte budget"
            );
            context.update(&buffer[..count]);
        }
        let hash = context
            .finish()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        ensure!(
            hash == ticket.response.signed_manifest.manifest.plaintext_sha256,
            "protected module authentication failed"
        );
        Ok(())
    })();
    buffer.zeroize();
    result
}

/// There is no fallback to a local unprotected renderer on authentication or
/// loading failure. Callers decide how to surface the current task failure.
pub fn run_role(role: Role) -> Result<i32> {
    let ticket_path = std::env::var_os("NOVA_PROTECTED_TICKET_PATH")
        .map(PathBuf::from)
        .context("protected task ticket path is absent")?;
    let module_path = std::env::var_os("NOVA_PROTECTED_MODULE_PATH")
        .map(PathBuf::from)
        .context("protected module path is absent")?;
    let ticket = read_ticket(&ticket_path)?;
    let _config = config_for_ticket(&ticket)?;
    run_verified_module(&module_path, &ticket, role)
}

pub fn maybe_run_native() -> Result<Option<i32>> {
    maybe_run(Role::NativeViewer)
}
pub fn maybe_run_cpu_audit() -> Result<Option<i32>> {
    maybe_run(Role::CpuAudit)
}
fn maybe_run(role: Role) -> Result<Option<i32>> {
    if std::env::var_os("NOVA_PROTECTED_MODULE_PATH").is_some()
        || std::env::var_os("NOVA_PROTECTED_TICKET_PATH").is_some()
    {
        run_role(role).map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(windows)]
fn run_verified_module(path: &Path, ticket: &TaskTicket, role: Role) -> Result<i32> {
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
    };
    let absolute = path
        .canonicalize()
        .context("resolve protected module path")?;
    // Keep a read-only share lock from hashing through entry execution: no
    // process can replace/write the file between authentication and loading.
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&absolute)
        .context("lock protected module against replacement")?;
    verify_locked_module(&mut file, ticket)?;
    let path_utf16 = absolute
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        let module = LoadLibraryExW(
            path_utf16.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        ensure!(
            !module.is_null(),
            "load authenticated protected module: {}",
            std::io::Error::last_os_error()
        );
        // The module owns renderer/service worker threads. It remains a normal
        // loaded DLL until this child process exits, avoiding unsafe unload of
        // code still running in those owned threads. No injection/manual map.
        let abi = GetProcAddress(module, c"nova_protected_abi_version".as_ptr().cast())
            .context("protected module ABI version export is absent")?;
        let abi: unsafe extern "C" fn() -> u32 = std::mem::transmute(abi);
        ensure!(abi() == ABI_VERSION, "protected module ABI version differs");
        let entry = GetProcAddress(module, c"nova_protected_entry".as_ptr().cast())
            .context("protected module entry export is absent")?;
        let entry: unsafe extern "C" fn(u32) -> i32 = std::mem::transmute(entry);
        Ok(entry(role as u32))
    }
}

#[cfg(not(windows))]
fn run_verified_module(_path: &Path, _ticket: &TaskTicket, _role: Role) -> Result<i32> {
    anyhow::bail!("protected module loading is supported only by the Windows candidate")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roles_are_stable_and_invalid_roles_fail() {
        assert_eq!(Role::try_from(1).unwrap(), Role::RenderService);
        assert_eq!(Role::try_from(4).unwrap(), Role::CpuAudit);
        assert!(Role::try_from(0).is_err());
        assert!(Role::try_from(u32::MAX).is_err());
    }
    #[test]
    fn ticket_reader_rejects_oversized_or_invalid_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ticket.json");
        std::fs::write(&path, vec![0_u8; MAX_TICKET_BYTES as usize + 1]).unwrap();
        assert!(
            read_ticket(&path)
                .err()
                .unwrap()
                .to_string()
                .contains("byte budget")
        );
        std::fs::write(&path, b"not-json").unwrap();
        assert!(
            read_ticket(&path)
                .err()
                .unwrap()
                .to_string()
                .contains("parse")
        );
    }
}
