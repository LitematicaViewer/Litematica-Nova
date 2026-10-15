use crate::{crypto, identity::DeviceKey, protocol::*};
use anyhow::{Context, Result, anyhow, ensure};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use zeroize::Zeroizing;

/// A normal per-user directory, protected using system ACLs rather than hidden loading.
pub fn ensure_private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    ensure!(path.is_dir(), "private runtime path is not a directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::{
            Foundation::{CloseHandle, LocalFree},
            Security::{Authorization::*, *},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        ensure!(
            std::fs::symlink_metadata(path)?.file_attributes() & 0x400 == 0,
            "private runtime directory must not be a reparse point"
        );
        unsafe {
            let mut token = null_mut();
            ensure!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0,
                "open current user token failed"
            );
            let mut needed = 0;
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed);
            let mut buffer =
                vec![0usize; (needed as usize + size_of::<usize>() - 1) / size_of::<usize>()];
            let result = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            );
            CloseHandle(token);
            ensure!(result != 0, "read current user identity failed");
            let user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
            let mut sid_text = null_mut();
            ensure!(
                ConvertSidToStringSidW(user.User.Sid, &mut sid_text) != 0,
                "format current user identity failed"
            );
            let mut len = 0;
            while *sid_text.add(len) != 0 {
                len += 1;
            }
            let sid = String::from_utf16(std::slice::from_raw_parts(sid_text, len));
            LocalFree(sid_text.cast());
            let sddl = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{})", sid?);
            let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
            let mut descriptor = null_mut();
            ensure!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    null_mut()
                ) != 0,
                "create current-user ACL failed"
            );
            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = null_mut();
            let extracted =
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
            if extracted == 0 || present == 0 {
                LocalFree(descriptor);
                return Err(anyhow!("read current-user ACL failed"));
            }
            let mut name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let status = SetNamedSecurityInfoW(
                name.as_mut_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            );
            LocalFree(descriptor);
            ensure!(status == 0, "apply current-user ACL failed: {}", status);
        }
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub endpoint: String,
    pub build_id: String,
    pub module_id: String,
    pub manifest_public_key: String,
    pub identity_dir: PathBuf,
}
/// Contains a server-signed grant and task bearer credential, never a content/private key.
/// Treat as current-user private runtime data; do not log or distribute.
#[derive(Clone, Serialize, Deserialize)]
pub struct TaskTicket {
    pub endpoint: String,
    pub response: AcquireResponse,
}
impl TaskTicket {
    pub fn binding(&self) -> &TaskBinding {
        &self.response.binding
    }
    pub fn verify(&self, config: &ClientConfig) -> Result<()> {
        ensure!(self.endpoint == config.endpoint, "task endpoint mismatch");
        let binding = &self.response.binding;
        ensure!(
            binding.build_id == config.build_id && binding.module_id == config.module_id,
            "task build/module mismatch"
        );
        ensure!(binding.expires_ms > crate::unix_ms(), "task grant expired");
        let public = crypto::unb64(&config.manifest_public_key)?;
        crypto::verify_manifest(&self.response.signed_manifest, &public)?;
        let manifest = &self.response.signed_manifest.manifest;
        ensure!(
            manifest.protocol == PROTOCOL
                && manifest.build_id == binding.build_id
                && manifest.module_id == binding.module_id
                && manifest.ciphertext_sha256 == binding.ciphertext_sha256,
            "task/manifest binding mismatch"
        );
        let mut unsigned = self.response.clone();
        unsigned.server_signature.clear();
        crypto::verify_server(
            &public,
            &serde_json::to_vec(&unsigned)?,
            &self.response.server_signature,
        )
    }
}
#[derive(Clone)]
pub struct TaskSession {
    config: ClientConfig,
    ticket: TaskTicket,
    identity: Arc<DeviceKey>,
    http: Client,
    content_key: Option<Arc<Zeroizing<Vec<u8>>>>,
}
#[derive(Debug)]
pub struct RateLimited {
    pub unban_ms: Option<i64>,
    pub retry_after_seconds: Option<u64>,
}
impl std::fmt::Display for RateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "protected new-task requests suspended; unban_ms={:?}; retry_after_seconds={:?}",
            self.unban_ms, self.retry_after_seconds
        )
    }
}
impl std::error::Error for RateLimited {}

fn http(config: &ClientConfig) -> Result<Client> {
    let url = reqwest::Url::parse(&config.endpoint)?;
    // Loopback HTTP is exclusively for local CLI business tests.
    ensure!(
        url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))),
        "protected endpoint must use HTTPS"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "invalid endpoint"
    );
    Ok(Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn checked(mut response: reqwest::blocking::Response, max: usize) -> Result<Vec<u8>> {
    let status = response.status();
    ensure!(
        response
            .content_length()
            .is_none_or(|len| len <= max as u64),
        "service response exceeds byte budget"
    );
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= max, "service response exceeds byte budget");
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let error: ApiError = serde_json::from_slice(&bytes).unwrap_or(ApiError {
            error: "rate limited".into(),
            unban_ms: None,
            retry_after_seconds: None,
        });
        return Err(RateLimited {
            unban_ms: error.unban_ms,
            retry_after_seconds: error.retry_after_seconds,
        }
        .into());
    }
    ensure!(
        status.is_success(),
        "protected service returned HTTP {}",
        status.as_u16()
    );
    Ok(bytes)
}
impl TaskSession {
    pub fn acquire(config: &ClientConfig, task_id: &str) -> Result<Self> {
        ensure!(
            valid_id(task_id) && valid_id(&config.build_id) && valid_id(&config.module_id),
            "invalid task/build/module identity"
        );
        let started = std::time::Instant::now();
        let identity = Arc::new(DeviceKey::open(&config.identity_dir)?);
        let http = http(config)?;
        let device_id = register(&http, config, &identity)?;
        // A process-wide pending request retains the same recipient and request_id on retry.
        // Cross-process continuation uses the signed ticket after Host has decrypted the module.
        let pending_key = format!(
            "{}:{}:{}:{}",
            device_id, config.build_id, config.module_id, task_id
        );
        static PENDING: std::sync::OnceLock<
            Mutex<std::collections::HashMap<String, Arc<Pending>>>,
        > = std::sync::OnceLock::new();
        let pending = {
            let mut map = PENDING
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| anyhow!("task request lock failed"))?;
            map.retain(|_, v| crate::unix_ms() - v.created_ms < 600_000);
            if map.len() >= 64 && !map.contains_key(&pending_key) {
                return Err(anyhow!("pending task queue capacity reached"));
            }
            map.entry(pending_key.clone())
                .or_insert_with(|| {
                    Arc::new(Pending {
                        recipient: crypto::Recipient::new(),
                        request_id: crypto::sha256(pending_key.as_bytes()),
                        created_ms: crate::unix_ms(),
                    })
                })
                .clone()
        };
        let body = serde_json::to_vec(&AcquireRequest {
            build_id: config.build_id.clone(),
            module_id: config.module_id.clone(),
            task_id: task_id.into(),
            recipient_key: pending.recipient.public_key.clone(),
        })?;
        pace_new_task(&config.identity_dir, task_id)?;
        let proof = signed_proof(
            &identity,
            &device_id,
            &pending.request_id,
            "/v1/tasks/acquire",
            &body,
        )?;
        let response = http
            .post(format!(
                "{}/v1/tasks/acquire",
                config.endpoint.trim_end_matches('/')
            ))
            .timeout(std::time::Duration::from_secs(30))
            .header("x-nova-proof", crypto::b64(&serde_json::to_vec(&proof)?))
            .body(body)
            .send()
            .context("acquire protected task")?;
        let response: AcquireResponse = serde_json::from_slice(&checked(response, 128 * 1024)?)?;
        let ticket = TaskTicket {
            endpoint: config.endpoint.clone(),
            response,
        };
        ticket.verify(config)?;
        ensure!(
            ticket.binding().device_id == device_id && ticket.binding().task_id == task_id,
            "task grant identity mismatch"
        );
        let content_key = pending.recipient.unwrap(
            &ticket.response.wrapped_key,
            &serde_json::to_vec(ticket.binding())?,
        )?;
        PENDING
            .get()
            .expect("initialized pending map")
            .lock()
            .map_err(|_| anyhow!("task request lock failed"))?
            .remove(&pending_key);
        eprintln!(
            "protection_task_acquire task={} module={} elapsed_ms={} wrapped_bytes={} content_key_rotation=false",
            task_id,
            config.module_id,
            started.elapsed().as_millis(),
            crypto::unb64(&ticket.response.wrapped_key.ciphertext)?.len()
        );
        Ok(Self {
            config: config.clone(),
            ticket,
            identity,
            http,
            content_key: Some(Arc::new(content_key)),
        })
    }
    pub fn resume_ticket(config: &ClientConfig, ticket: TaskTicket) -> Result<Self> {
        ticket.verify(config)?;
        let identity = Arc::new(DeviceKey::open(&config.identity_dir)?);
        let http = http(config)?;
        let device = register(&http, config, &identity)?;
        ensure!(
            device == ticket.binding().device_id,
            "ticket belongs to another device"
        );
        Ok(Self {
            config: config.clone(),
            ticket,
            identity,
            http,
            content_key: None,
        })
    }
    pub fn ticket(&self) -> TaskTicket {
        self.ticket.clone()
    }
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }
    /// Drops this handle's CEK immediately; other explicit session clones retain their own Arc.
    pub fn release_content_key(&mut self) {
        self.content_key = None;
    }
    pub fn compute(&self, operation: &str, input: Vec<u8>) -> Result<Vec<u8>> {
        ensure!(
            valid_id(operation) && input.len() <= 64 * 1024 * 1024,
            "compute input exceeds operation/byte budget"
        );
        self.call(
            &format!("/v1/compute/{operation}"),
            input,
            256 * 1024 * 1024,
        )
    }
    fn call(&self, endpoint: &str, body: Vec<u8>, limit: usize) -> Result<Vec<u8>> {
        let request_id = crypto::random_id()?;
        let proof = signed_proof(
            &self.identity,
            &self.ticket.binding().device_id,
            &request_id,
            endpoint,
            &body,
        )?;
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.config.endpoint.trim_end_matches('/'),
                endpoint
            ))
            .header("x-nova-proof", crypto::b64(&serde_json::to_vec(&proof)?))
            .header("x-nova-task", &self.ticket.binding().task_id)
            .header("x-nova-module", &self.ticket.binding().module_id)
            .header("x-nova-token", &self.ticket.response.task_token)
            .body(body)
            .send()?;
        checked(response, limit)
    }
    pub fn download_ciphertext(&self) -> Result<Vec<u8>> {
        self.call("/v1/modules/download", Vec::new(), 512 * 1024 * 1024)
    }
    pub fn decrypt_module(&self, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let key = self
            .content_key
            .as_ref()
            .ok_or_else(|| anyhow!("resumed tickets never contain a module content key"))?;
        let start = std::time::Instant::now();
        let bytes = crypto::decrypt_module(
            ciphertext,
            key,
            &self.ticket.response.signed_manifest,
            &crypto::unb64(&self.config.manifest_public_key)?,
        )?;
        eprintln!(
            "protection_module_decrypt module={} ciphertext_bytes={} elapsed_ms={}",
            self.config.module_id,
            ciphertext.len(),
            start.elapsed().as_millis()
        );
        Ok(bytes)
    }
    pub fn decrypt_to_private_file(&self, ciphertext: &[u8], destination: &Path) -> Result<()> {
        let bytes = self.decrypt_module(ciphertext)?;
        let parent = destination.parent().context("module destination parent")?;
        ensure!(
            parent.is_dir(),
            "create a current-user private runtime directory before decrypting"
        );
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(destination)?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(destination);
            return Err(error.into());
        }
        Ok(())
    }
    pub fn verify_materialized_module(
        config: &ClientConfig,
        ticket: &TaskTicket,
        path: &Path,
    ) -> Result<()> {
        ticket.verify(config)?;
        let bytes = Zeroizing::new(std::fs::read(path)?);
        ensure!(
            crypto::sha256(&bytes) == ticket.response.signed_manifest.manifest.plaintext_sha256,
            "materialized module hash mismatch"
        );
        Ok(())
    }
}
struct Pending {
    recipient: crypto::Recipient,
    request_id: String,
    created_ms: i64,
}
fn signed_proof(
    identity: &DeviceKey,
    device: &str,
    id: &str,
    endpoint: &str,
    body: &[u8],
) -> Result<Proof> {
    let mut proof = Proof {
        device_id: device.into(),
        request_id: id.into(),
        unix_ms: crate::unix_ms(),
        signature: String::new(),
    };
    proof.signature = crypto::b64(&identity.sign(&canonical(&proof, endpoint, body))?);
    Ok(proof)
}
fn register(http: &Client, config: &ClientConfig, identity: &DeviceKey) -> Result<String> {
    let public_key = crypto::b64(&identity.public_key()?);
    let key_identity = crypto::sha256(public_key.as_bytes());
    let cache_path = config.identity_dir.join("device-registration.json");
    if let Ok(bytes) = std::fs::read(&cache_path) {
        if let Ok((endpoint, key, id)) = serde_json::from_slice::<(String, String, String)>(&bytes)
        {
            if endpoint == config.endpoint && key == key_identity && valid_id(&id) {
                return Ok(id);
            }
        }
    }
    // Registration is idempotent by the cryptographic public key, independently of network/IP/version.
    let request_id = crypto::random_id()?;
    let time = crate::unix_ms();
    let signature =
        crypto::b64(&identity.sign(&registration_message(&public_key, &request_id, time))?);
    let response = http
        .post(format!(
            "{}/v1/devices/register",
            config.endpoint.trim_end_matches('/')
        ))
        .json(&RegisterRequest {
            public_key,
            request_id,
            unix_ms: time,
            signature,
        })
        .send()?;
    let response: RegisterResponse = serde_json::from_slice(&checked(response, 8192)?)?;
    ensure!(
        valid_id(&response.device_id),
        "invalid registered device identity"
    );
    let bytes = serde_json::to_vec(&(&config.endpoint, key_identity, &response.device_id))?;
    let temporary = config
        .identity_dir
        .join(format!("device-registration-{}.tmp", crypto::random_id()?));
    std::fs::write(&temporary, &bytes)?;
    // Same-key concurrent registration returns the same server identity.
    if std::fs::rename(&temporary, &cache_path).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok(response.device_id)
}
fn pace_new_task(directory: &Path, task: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("task-queue.lock"))?;
    fs2::FileExt::lock_exclusive(&file)?;
    let mut old = String::new();
    file.read_to_string(&mut old)?;
    let last: Option<(String, i64)> = serde_json::from_str(&old).ok();
    let now = crate::unix_ms();
    if let Some((previous, time)) = &last {
        if previous != task && now - *time < 300 {
            std::thread::sleep(std::time::Duration::from_millis(
                (300 - (now - *time)).clamp(0, 300) as u64,
            ));
        }
    }
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    file.write_all(&serde_json::to_vec(&(task, crate::unix_ms()))?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multi_instance_file_queue_preserves_headroom() {
        let directory = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let path = directory.path().to_owned();
                std::thread::spawn(move || pace_new_task(&path, &format!("task-{i}")).unwrap())
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert!(started.elapsed() >= std::time::Duration::from_millis(850));
    }
}
