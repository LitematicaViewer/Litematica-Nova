//! Public, ordinary OS loader. This crate contains no rendering algorithms.
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

pub const ABI_VERSION: u32 = nova_protection::runtime::RUNTIME_ABI;
#[cfg(windows)]
pub const MODULE_ID: &str = "nova_protected_module.dll";
#[cfg(target_os = "linux")]
pub const MODULE_ID: &str = "libnova_protected_module.so";
#[cfg(target_os = "macos")]
pub const MODULE_ID: &str = "libnova_protected_module.dylib";
pub const MODULE_EXTENSION: &str = std::env::consts::DLL_EXTENSION;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
compile_error!("Nova protected modules require a supported Windows, Linux or macOS target");
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
    ensure!(
        nova_protection::runtime::contract().trusted_manifest_public_keys.iter().any(|key| key == public_key),
        "release manifest verification key is not trusted by this Host"
    );
    let build_id = option_env!("NOVA_PROTECTED_BUILD_ID")
        .filter(|value| !value.is_empty())
        .context("protected release build identity is not configured")?;
    let endpoint = option_env!("NOVA_PROTECTED_ENDPOINT").unwrap_or(DEFAULT_ENDPOINT);
    ensure!(
        endpoint.starts_with("https://"),
        "protected production endpoint must use HTTPS"
    );
    if let Some(compatibility) = nova_protection::runtime::compiled_compatibility()? {
        compatibility.verify(public_key, nova_protection::runtime::target()?)?;
        ensure!(
            compatibility.compatibility.runtime_build_id == build_id
                && compatibility.compatibility.module_id == MODULE_ID
                && compatibility.compatibility.endpoint == endpoint,
            "compiled runtime compatibility and selected component differ"
        );
    }
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

fn verify_abi(version: u32) -> Result<()> {
    ensure!(
        version == ABI_VERSION,
        "protected module ABI version differs"
    );
    Ok(())
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
        verify_abi(abi())?;
        let entry = GetProcAddress(module, c"nova_protected_entry".as_ptr().cast())
            .context("protected module entry export is absent")?;
        let entry: unsafe extern "C" fn(u32) -> i32 = std::mem::transmute(entry);
        Ok(entry(role as u32))
    }
}

#[cfg(unix)]
fn open_private_module(path: &Path) -> Result<(PathBuf, File)> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    // The Host materializes each module as 0600 in its own 0700 directory.
    // Do not follow a symlink to a public or foreign module/runtime directory.
    let parent = path
        .parent()
        .context("protected module runtime directory is absent")?;
    let directory = std::fs::symlink_metadata(parent)?;
    let metadata = std::fs::symlink_metadata(path)?;
    let uid = unsafe { libc::geteuid() };
    ensure!(
        directory.is_dir() && directory.uid() == uid && directory.mode() & 0o077 == 0,
        "protected module runtime directory is not current-user private"
    );
    ensure!(
        metadata.is_file() && metadata.uid() == uid && metadata.mode() & 0o077 == 0,
        "protected module file is not current-user private"
    );
    let absolute = path
        .canonicalize()
        .context("resolve protected module path")?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&absolute)
        .context("open private protected module")?;
    let opened = file.metadata()?;
    ensure!(
        metadata.dev() == opened.dev()
            && metadata.ino() == opened.ino()
            && opened.uid() == uid
            && opened.mode() & 0o077 == 0,
        "protected module changed while opening"
    );
    Ok((absolute, file))
}

#[cfg(unix)]
fn run_verified_module(path: &Path, ticket: &TaskTicket, role: Role) -> Result<i32> {
    let (absolute, mut file) = open_private_module(path)?;
    verify_locked_module(&mut file, ticket)?;
    unsafe {
        // Use the system loader, eagerly resolving imports with process-local
        // symbols. No manual mapping, injection or unprotected local fallback.
        let library = libloading::os::unix::Library::open(
            Some(&absolute),
            libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_LOCAL,
        )
        .context("load authenticated protected module")?;
        // As on Windows, renderer threads may still own this code after entry
        // returns. Keep the normal OS library loaded until child-process exit.
        let library = std::mem::ManuallyDrop::new(library);
        let abi = library
            .get::<unsafe extern "C" fn() -> u32>(b"nova_protected_abi_version\0")
            .context("protected module ABI version export is absent")?;
        verify_abi(abi())?;
        let entry = library
            .get::<unsafe extern "C" fn(u32) -> i32>(b"nova_protected_entry\0")
            .context("protected module entry export is absent")?;
        // Keep the authenticated read descriptor alive through entry execution.
        let result = entry(role as u32);
        drop(file);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unsigned_ticket(hash: &str) -> TaskTicket {
        use nova_protection::{
            crypto::{ModuleManifest, SignedManifest, WrappedKey},
            protocol::{AcquireResponse, TaskBinding},
        };
        TaskTicket {
            endpoint: "https://fixture.invalid".into(),
            response: AcquireResponse {
                binding: TaskBinding {
                    device_id: "fixture-device".into(),
                    build_id: "fixture-build".into(),
                    module_id: MODULE_ID.into(),
                    task_id: "fixture-task".into(),
                    expires_ms: nova_protection::unix_ms() + 60_000,
                    ciphertext_sha256: "fixture-ciphertext".into(),
                },
                task_token: String::new(),
                wrapped_key: WrappedKey {
                    encapsulated_key: String::new(),
                    ciphertext: String::new(),
                },
                signed_manifest: SignedManifest {
                    manifest: ModuleManifest {
                        protocol: 1,
                        build_id: "fixture-build".into(),
                        module_id: MODULE_ID.into(),
                        plaintext_sha256: hash.into(),
                        ciphertext_sha256: "fixture-ciphertext".into(),
                        ciphertext_bytes: 1,
                        nonce: String::new(),
                        created_ms: 1,
                    },
                    signature: String::new(),
                },
                server_signature: String::new(),
            },
        }
    }
    #[test]
    fn module_names_follow_target_os_conventions() {
        assert_eq!(
            Path::new(MODULE_ID)
                .extension()
                .and_then(|value| value.to_str()),
            Some(MODULE_EXTENSION)
        );
        #[cfg(windows)]
        assert_eq!(MODULE_ID, "nova_protected_module.dll");
        #[cfg(target_os = "linux")]
        assert_eq!(MODULE_ID, "libnova_protected_module.so");
        #[cfg(target_os = "macos")]
        assert_eq!(MODULE_ID, "libnova_protected_module.dylib");
    }
    #[test]
    fn changed_abi_is_rejected_before_entry() {
        assert!(verify_abi(ABI_VERSION).is_ok());
        assert!(
            verify_abi(ABI_VERSION + 1)
                .unwrap_err()
                .to_string()
                .contains("ABI version differs")
        );
    }
    #[test]
    fn unauthenticated_ticket_and_module_are_rejected() {
        let ticket = unsigned_ticket("not-the-file-hash");
        let config = ClientConfig {
            endpoint: ticket.endpoint.clone(),
            build_id: "fixture-build".into(),
            module_id: MODULE_ID.into(),
            manifest_public_key: nova_protection::crypto::b64(&[0; 32]),
            identity_dir: PathBuf::new(),
        };
        assert!(
            ticket.verify(&config).is_err(),
            "unsigned tickets must never authorize loading"
        );
        let directory = tempfile::tempdir().unwrap();
        nova_protection::client::ensure_private_directory(directory.path()).unwrap();
        let path = directory.path().join(MODULE_ID);
        std::fs::write(&path, b"not a shared library").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(
            run_verified_module(&path, &ticket, Role::BuildWorker)
                .unwrap_err()
                .to_string()
                .contains("authentication failed"),
            "a changed module must fail before the OS loader sees it"
        );
    }
    #[cfg(unix)]
    #[test]
    fn unix_loader_rejects_public_runtime_and_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let module = directory.path().join(MODULE_ID);
        std::fs::write(&module, b"fixture").unwrap();
        std::fs::set_permissions(&module, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(open_private_module(&module).is_err());
        std::fs::set_permissions(&module, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(open_private_module(&module).is_ok());
        let link = directory.path().join(format!("link.{MODULE_EXTENSION}"));
        symlink(&module, &link).unwrap();
        assert!(open_private_module(&link).is_err());
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(open_private_module(&module).is_err());
    }
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
