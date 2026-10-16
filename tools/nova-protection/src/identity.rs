use anyhow::{Result, anyhow, ensure};
use ring::{
    rand::SystemRandom,
    signature::{self, KeyPair},
};
use std::{fs::OpenOptions, path::Path};
use zeroize::Zeroizing;

pub(crate) enum DeviceKey {
    #[cfg(windows)]
    Tpm(windows::TpmKey),
    Software(signature::EcdsaKeyPair),
}
impl DeviceKey {
    pub fn open(directory: &Path) -> Result<Self> {
        crate::client::ensure_private_directory(directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("identity.lock"))?;
        fs2::FileExt::lock_exclusive(&lock)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            return unix_storage::open_locked(directory, &unix_storage::SystemVault);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let path = directory.join("device-key.dpapi");
            // A persisted software key must not turn into a different identity when a TPM appears later.
            if path.exists() {
                return Self::software(&path);
            }
            #[cfg(windows)]
            {
                let marker = directory.join("device-provider");
                if marker.exists() {
                    ensure!(
                        std::fs::read(&marker)? == b"tpm-cng-v1",
                        "invalid device provider marker"
                    );
                    return Ok(Self::Tpm(windows::TpmKey::open()?));
                }
                if let Ok(key) = windows::TpmKey::open() {
                    std::fs::write(marker, b"tpm-cng-v1")?;
                    return Ok(Self::Tpm(key));
                }
            }
            Self::software(&path)
        }
    }
    fn software(path: &Path) -> Result<Self> {
        let bytes = if path.exists() {
            let encrypted = std::fs::read(path)?;
            #[cfg(windows)]
            {
                windows::dpapi(&encrypted, false)?
            }
            #[cfg(not(windows))]
            {
                Zeroizing::new(encrypted)
            }
        } else {
            let pkcs8 = signature::EcdsaKeyPair::generate_pkcs8(
                &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                &SystemRandom::new(),
            )
            .map_err(|_| anyhow!("device key generation failed"))?;
            let bytes = Zeroizing::new(pkcs8.as_ref().to_vec());
            #[cfg(windows)]
            let storage = windows::dpapi(&bytes, true)?;
            #[cfg(not(windows))]
            let storage = Zeroizing::new(bytes.to_vec());
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            use std::io::Write;
            options.open(path)?.write_all(&storage)?;
            bytes
        };
        let key = signature::EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &bytes,
            &SystemRandom::new(),
        )
        .map_err(|_| anyhow!("device key storage authentication failed"))?;
        Ok(Self::Software(key))
    }
    pub fn public_key(&self) -> Result<Vec<u8>> {
        match self {
            #[cfg(windows)]
            Self::Tpm(key) => key.public_key(),
            Self::Software(key) => Ok(key.public_key().as_ref().to_vec()),
        }
    }
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        match self {
            #[cfg(windows)]
            Self::Tpm(key) => key.sign(message),
            Self::Software(key) => Ok(key
                .sign(&SystemRandom::new(), message)
                .map_err(|_| anyhow!("device signing failed"))?
                .as_ref()
                .to_vec()),
        }
    }
}

// The selection policy is compiled for tests on Windows without opening an OS
// vault. Native adapters are included only on their actual Unix target.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod unix_storage {
    use super::*;
    use anyhow::Context;
    use serde::{Deserialize, Serialize};
    use std::{
        io::{Read, Write},
        path::PathBuf,
    };

    const SOFTWARE_PROVIDER: &str = "private-file-pkcs8-v1";
    const MAX_KEY_BYTES: u64 = 16 * 1024;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum VaultError {
        Unavailable,
        Locked,
        Invalid,
    }
    impl std::fmt::Display for VaultError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(match self {
                Self::Unavailable => "system device key storage is unavailable",
                Self::Locked => "system device key storage is locked or access was denied",
                Self::Invalid => "system device key storage has an invalid or ambiguous entry",
            })
        }
    }
    impl std::error::Error for VaultError {}
    pub(super) trait Vault {
        fn provider(&self) -> &'static str;
        fn get(&self) -> std::result::Result<Option<Zeroizing<Vec<u8>>>, VaultError>;
        fn set(&self, bytes: &[u8]) -> std::result::Result<(), VaultError>;
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) struct SystemVault;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    impl SystemVault {
        fn entry() -> std::result::Result<keyring::Entry, VaultError> {
            // Stable across versions, restarts and per-task runtime directories;
            // this is an application label, not an account or hardware identity.
            keyring::Entry::new("Litematica-Nova.DeviceSigning.v1", "device-key")
                .map_err(Self::error)
        }
        fn error(error: keyring::Error) -> VaultError {
            match error {
                keyring::Error::NoStorageAccess(_) => VaultError::Locked,
                keyring::Error::PlatformFailure(_) => VaultError::Unavailable,
                _ => VaultError::Invalid,
            }
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    impl Vault for SystemVault {
        fn provider(&self) -> &'static str {
            #[cfg(target_os = "linux")]
            {
                "secret-service-v1"
            }
            #[cfg(target_os = "macos")]
            {
                "macos-keychain-v1"
            }
        }
        fn get(&self) -> std::result::Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
            match Self::entry()?.get_secret() {
                Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(error) => Err(Self::error(error)),
            }
        }
        fn set(&self, bytes: &[u8]) -> std::result::Result<(), VaultError> {
            Self::entry()?.set_secret(bytes).map_err(Self::error)
        }
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ProviderState {
        schema: u32,
        provider: String,
        public_key_sha256: String,
    }

    fn key(bytes: &[u8]) -> Result<DeviceKey> {
        ensure!(
            !bytes.is_empty() && bytes.len() as u64 <= MAX_KEY_BYTES,
            "device key exceeds its byte budget"
        );
        let key = signature::EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            bytes,
            &SystemRandom::new(),
        )
        .map_err(|_| anyhow!("device key storage authentication failed"))?;
        Ok(DeviceKey::Software(key))
    }
    fn fingerprint(key: &DeviceKey) -> Result<String> {
        Ok(crate::crypto::sha256(&key.public_key()?))
    }

    fn read_software(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
        let metadata = std::fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_file() && metadata.len() <= MAX_KEY_BYTES,
            "invalid private software identity file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
                "software device key is not current-user private"
            );
        }
        let mut bytes = Zeroizing::new(Vec::new());
        std::fs::File::open(path)?
            .take(MAX_KEY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        key(&bytes)?;
        Ok(bytes)
    }
    fn software_bytes(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
        if path.exists() {
            return read_software(path);
        }
        let pkcs8 = signature::EcdsaKeyPair::generate_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &SystemRandom::new(),
        )
        .map_err(|_| anyhow!("device key generation failed"))?;
        let bytes = Zeroizing::new(pkcs8.as_ref().to_vec());
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(bytes)
    }
    fn record(directory: &Path, provider: &str, selected: &DeviceKey) -> Result<()> {
        let state = ProviderState {
            schema: 1,
            provider: provider.into(),
            public_key_sha256: fingerprint(selected)?,
        };
        let temporary = directory.join(format!(
            "device-provider-{}.tmp",
            crate::crypto::random_id()?
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(&state)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, directory.join("device-provider"))?;
        Ok(())
    }
    fn finish(
        software_path: &Path,
        state: &ProviderState,
        bytes: Zeroizing<Vec<u8>>,
    ) -> Result<DeviceKey> {
        let selected = key(&bytes)?;
        ensure!(
            fingerprint(&selected)? == state.public_key_sha256,
            "stored device public key identity changed"
        );
        if state.provider != SOFTWARE_PROVIDER && software_path.exists() {
            let old = read_software(software_path)?;
            ensure!(
                fingerprint(&key(&old)?)? == state.public_key_sha256,
                "software migration identity differs"
            );
            std::fs::remove_file(software_path)?;
        }
        eprintln!(
            "[NOVA_DEVICE_IDENTITY] provider={} hardware_signing=false",
            state.provider
        );
        Ok(selected)
    }

    // DeviceKey::open owns identity.lock while this whole transaction executes.
    // A persisted provider is authoritative, including failures: a locked vault
    // is never an instruction to generate a new identity or fall back locally.
    pub(super) fn open_locked(directory: &Path, vault: &impl Vault) -> Result<DeviceKey> {
        let marker = directory.join("device-provider");
        // This legacy filename contains raw PKCS8 on Unix, never DPAPI bytes.
        let software_path: PathBuf = directory.join("device-key.dpapi");
        if marker.exists() {
            ensure!(
                std::fs::metadata(&marker)?.len() <= 4096,
                "invalid device provider marker size"
            );
            let state: ProviderState = serde_json::from_slice(&std::fs::read(&marker)?)?;
            ensure!(state.schema == 1, "invalid device provider marker version");
            let bytes = if state.provider == SOFTWARE_PROVIDER {
                read_software(&software_path)?
            } else {
                ensure!(
                    state.provider == vault.provider(),
                    "device provider differs from current platform"
                );
                match vault.get()? {
                    Some(bytes) => bytes,
                    None => {
                        // Recover an interrupted first migration only from the
                        // already persisted identical key. Missing means fail,
                        // not generate/re-register a replacement identity.
                        let bytes = read_software(&software_path)
                            .context("selected system device key is missing")?;
                        ensure!(
                            fingerprint(&key(&bytes)?)? == state.public_key_sha256,
                            "pending device key identity differs"
                        );
                        vault.set(&bytes)?;
                        vault
                            .get()?
                            .context("system device key write was not retained")?
                    }
                }
            };
            return finish(&software_path, &state, bytes);
        }

        match vault.get() {
            Ok(Some(bytes)) => {
                let selected = key(&bytes)?;
                if software_path.exists() {
                    ensure!(
                        fingerprint(&key(&read_software(&software_path)?)?)?
                            == fingerprint(&selected)?,
                        "existing software and system device identities differ"
                    );
                }
                record(directory, vault.provider(), &selected)?;
                let state = ProviderState {
                    schema: 1,
                    provider: vault.provider().into(),
                    public_key_sha256: fingerprint(&selected)?,
                };
                finish(&software_path, &state, bytes)
            }
            Ok(None) => {
                let bytes = software_bytes(&software_path)?;
                let selected = key(&bytes)?;
                // Mark the exact identity before writing. An interrupted write
                // can retry the same PKCS8, never produce another device key.
                record(directory, vault.provider(), &selected)?;
                vault.set(&bytes)?;
                let bytes = vault
                    .get()?
                    .context("system device key write was not retained")?;
                let state = ProviderState {
                    schema: 1,
                    provider: vault.provider().into(),
                    public_key_sha256: fingerprint(&selected)?,
                };
                finish(&software_path, &state, bytes)
            }
            Err(VaultError::Unavailable) => {
                let bytes = software_bytes(&software_path)?;
                let selected = key(&bytes)?;
                record(directory, SOFTWARE_PROVIDER, &selected)?;
                eprintln!(
                    "[NOVA_DEVICE_IDENTITY] provider={SOFTWARE_PROVIDER} reason=system_vault_unavailable hardware_signing=false"
                );
                Ok(selected)
            }
            Err(error) => Err(error.into()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::{Arc, Mutex};

        #[derive(Default)]
        struct State {
            secret: Option<Vec<u8>>,
            error: Option<VaultError>,
            write_error: Option<VaultError>,
            writes: usize,
        }
        #[derive(Default)]
        struct MockVault(Mutex<State>);
        impl Vault for MockVault {
            fn provider(&self) -> &'static str {
                "fixture-system-vault-v1"
            }
            fn get(&self) -> std::result::Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
                let state = self.0.lock().unwrap();
                if let Some(error) = state.error {
                    return Err(error);
                }
                Ok(state.secret.clone().map(Zeroizing::new))
            }
            fn set(&self, bytes: &[u8]) -> std::result::Result<(), VaultError> {
                let mut state = self.0.lock().unwrap();
                if let Some(error) = state.write_error.or(state.error) {
                    return Err(error);
                }
                state.secret = Some(bytes.to_vec());
                state.writes += 1;
                Ok(())
            }
        }
        fn open(directory: &Path, vault: &MockVault) -> Result<DeviceKey> {
            crate::client::ensure_private_directory(directory)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(directory.join("identity.lock"))?;
            fs2::FileExt::lock_exclusive(&lock)?;
            open_locked(directory, vault)
        }
        #[test]
        fn unix_vault_migration_retains_legacy_public_key_and_removes_private_copy() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("device-key.dpapi");
            let legacy = software_bytes(&path).unwrap();
            let public = key(&legacy).unwrap().public_key().unwrap();
            let vault = MockVault::default();
            assert_eq!(
                open(directory.path(), &vault)
                    .unwrap()
                    .public_key()
                    .unwrap(),
                public
            );
            assert!(!path.exists());
            assert_eq!(
                open(directory.path(), &vault)
                    .unwrap()
                    .public_key()
                    .unwrap(),
                public
            );
            assert_eq!(vault.0.lock().unwrap().writes, 1);
        }
        #[test]
        fn unix_selected_vault_lock_or_missing_entry_never_regenerates_identity() {
            let directory = tempfile::tempdir().unwrap();
            let vault = MockVault::default();
            let public = open(directory.path(), &vault)
                .unwrap()
                .public_key()
                .unwrap();
            vault.0.lock().unwrap().error = Some(VaultError::Locked);
            assert!(open(directory.path(), &vault).is_err());
            {
                let mut state = vault.0.lock().unwrap();
                state.error = None;
                state.secret = None;
            }
            assert!(open(directory.path(), &vault).is_err());
            assert!(!directory.path().join("device-key.dpapi").exists());
            assert_eq!(vault.0.lock().unwrap().writes, 1);
            assert!(!public.is_empty());
        }
        #[test]
        fn unix_without_vault_keeps_one_private_file_identity_when_vault_appears() {
            let directory = tempfile::tempdir().unwrap();
            let vault = MockVault::default();
            vault.0.lock().unwrap().error = Some(VaultError::Unavailable);
            let first = open(directory.path(), &vault).unwrap();
            let public = first.public_key().unwrap();
            vault.0.lock().unwrap().error = None;
            let reopened = open(directory.path(), &vault).unwrap();
            assert_eq!(reopened.public_key().unwrap(), public);
            crate::crypto::verify_p256(
                &public,
                b"device possession",
                &reopened.sign(b"device possession").unwrap(),
            )
            .unwrap();
            assert_eq!(vault.0.lock().unwrap().writes, 0);
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert_eq!(
                    std::fs::metadata(directory.path().join("device-key.dpapi"))
                        .unwrap()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }
        #[test]
        fn unix_interrupted_vault_write_retries_the_same_staged_identity() {
            let directory = tempfile::tempdir().unwrap();
            let vault = MockVault::default();
            vault.0.lock().unwrap().write_error = Some(VaultError::Locked);
            assert!(open(directory.path(), &vault).is_err());
            let legacy_path = directory.path().join("device-key.dpapi");
            let public = key(&read_software(&legacy_path).unwrap())
                .unwrap()
                .public_key()
                .unwrap();
            vault.0.lock().unwrap().write_error = None;
            assert_eq!(
                open(directory.path(), &vault)
                    .unwrap()
                    .public_key()
                    .unwrap(),
                public
            );
            assert!(!legacy_path.exists());
            assert_eq!(vault.0.lock().unwrap().writes, 1);
        }
        #[test]
        fn unix_conflicting_system_and_legacy_identity_is_not_overwritten() {
            let directory = tempfile::tempdir().unwrap();
            let other = tempfile::tempdir().unwrap();
            let path = directory.path().join("device-key.dpapi");
            let old = software_bytes(&path).unwrap();
            let different = software_bytes(&other.path().join("device-key.dpapi")).unwrap();
            let vault = MockVault::default();
            vault.0.lock().unwrap().secret = Some(different.to_vec());
            assert!(open(directory.path(), &vault).is_err());
            assert_eq!(&*read_software(&path).unwrap(), &*old);
            assert_eq!(vault.0.lock().unwrap().writes, 0);
            assert!(!directory.path().join("device-provider").exists());
        }
        #[test]
        fn unix_multiple_instances_serialize_creation_to_one_identity() {
            let directory = tempfile::tempdir().unwrap();
            let vault = Arc::new(MockVault::default());
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    let path = directory.path().to_owned();
                    let vault = Arc::clone(&vault);
                    std::thread::spawn(move || {
                        open(&path, vault.as_ref()).unwrap().public_key().unwrap()
                    })
                })
                .collect();
            let public_keys: Vec<_> = handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect();
            assert!(public_keys.iter().all(|key| key == &public_keys[0]));
            assert_eq!(vault.0.lock().unwrap().writes, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_protected_software_identity_is_stable_across_opens() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test-device-key.dpapi");
        let first = DeviceKey::software(&path).unwrap();
        let public = first.public_key().unwrap();
        let signature = first.sign(b"canonical request").unwrap();
        crate::crypto::verify_p256(&public, b"canonical request", &signature).unwrap();
        drop(first);
        let second = DeviceKey::software(&path).unwrap();
        assert_eq!(public, second.public_key().unwrap());
        crate::crypto::verify_p256(
            &public,
            b"second process request",
            &second.sign(b"second process request").unwrap(),
        )
        .unwrap();
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    pub struct TpmKey {
        provider: usize,
        key: usize,
    }
    // NCrypt persisted provider handles support concurrent signing calls.
    unsafe impl Send for TpmKey {}
    unsafe impl Sync for TpmKey {}
    impl Drop for TpmKey {
        fn drop(&mut self) {
            unsafe {
                NCryptFreeObject(self.key);
                NCryptFreeObject(self.provider);
            }
        }
    }
    impl TpmKey {
        pub fn open() -> Result<Self> {
            let mut provider = 0;
            let status =
                unsafe { NCryptOpenStorageProvider(&mut provider, MS_PLATFORM_CRYPTO_PROVIDER, 0) };
            ensure!(status == 0, "TPM provider unavailable");
            let name = wide("Litematica-Nova.DeviceSigning.v1");
            let mut key = 0;
            let opened =
                unsafe { NCryptOpenKey(provider, &mut key, name.as_ptr(), 0, NCRYPT_SILENT_FLAG) };
            if opened != 0 {
                let created = unsafe {
                    NCryptCreatePersistedKey(
                        provider,
                        &mut key,
                        BCRYPT_ECDSA_P256_ALGORITHM,
                        name.as_ptr(),
                        0,
                        NCRYPT_SILENT_FLAG,
                    )
                };
                if created != 0 {
                    unsafe {
                        NCryptFreeObject(provider);
                    }
                    return Err(anyhow!("TPM key creation unavailable"));
                }
                if unsafe { NCryptFinalizeKey(key, NCRYPT_SILENT_FLAG) } != 0 {
                    unsafe {
                        NCryptDeleteKey(key, NCRYPT_SILENT_FLAG);
                        NCryptFreeObject(provider);
                    }
                    return Err(anyhow!("TPM key finalization unavailable"));
                }
            }
            let result = Self { provider, key };
            result.public_key()?;
            Ok(result)
        }
        pub fn public_key(&self) -> Result<Vec<u8>> {
            let mut blob = [0u8; 72];
            let mut written = 0;
            let status = unsafe {
                NCryptExportKey(
                    self.key,
                    0,
                    BCRYPT_ECCPUBLIC_BLOB,
                    null(),
                    blob.as_mut_ptr(),
                    blob.len() as u32,
                    &mut written,
                    0,
                )
            };
            ensure!(
                status == 0 && written == 72 && u32::from_le_bytes(blob[4..8].try_into()?) == 32,
                "TPM public key export failed"
            );
            let mut public = vec![4];
            public.extend_from_slice(&blob[8..72]);
            Ok(public)
        }
        pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
            let hash = ring::digest::digest(&ring::digest::SHA256, message);
            let mut signature = vec![0u8; 64];
            let mut written = 0;
            let status = unsafe {
                NCryptSignHash(
                    self.key,
                    null(),
                    hash.as_ref().as_ptr(),
                    32,
                    signature.as_mut_ptr(),
                    64,
                    &mut written,
                    NCRYPT_SILENT_FLAG,
                )
            };
            ensure!(status == 0 && written == 64, "TPM signing failed");
            Ok(signature)
        }
    }
    pub fn dpapi(bytes: &[u8], protect: bool) -> Result<Zeroizing<Vec<u8>>> {
        ensure!(bytes.len() <= u32::MAX as usize, "DPAPI input too large");
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };
        let ok = unsafe {
            if protect {
                CryptProtectData(
                    &input,
                    null(),
                    null(),
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    null_mut(),
                    null(),
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
        };
        ensure!(ok != 0, "DPAPI operation failed");
        let result = Zeroizing::new(
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec(),
        );
        unsafe {
            zeroize::Zeroize::zeroize(std::slice::from_raw_parts_mut(
                output.pbData,
                output.cbData as usize,
            ));
            LocalFree(output.pbData as *mut _);
        }
        Ok(result)
    }
}
