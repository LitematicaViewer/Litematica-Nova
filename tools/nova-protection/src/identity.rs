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
