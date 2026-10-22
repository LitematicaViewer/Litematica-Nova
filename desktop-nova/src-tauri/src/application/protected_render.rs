//! Host-owned task grants and authenticated module materialization. No renderer implementation.
use nova_protection::client::TaskSession;
use nova_render_protocol::ProtectedTask;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Default)]
pub(crate) struct ProtectedRender {
    admissions: HashMap<String, ProtectedTask>,
    retries: HashMap<String, String>,
    sequence: u64,
    runtime_root: Option<PathBuf>,
    suspended_until_ms: i64,
}
impl ProtectedRender {
    pub(crate) fn take_admission(&mut self, token: &str) -> Result<ProtectedTask, String> {
        self.admissions.remove(token).ok_or_else(|| "protected preflight task expired; run preflight again".into())
    }
    pub(crate) fn remember_admission(&mut self, token: String, task: ProtectedTask) {
        if self.admissions.len() >= 32 { self.admissions.clear(); }
        self.admissions.insert(token, task);
    }
    pub(crate) fn acquire(&mut self, data_root: &Path, backend: &Path, request: &[u8]) -> Result<ProtectedTask, String> {
        nova_protected_loader::require_private_runtime().map_err(|error| error.to_string())?;
        #[cfg(feature = "development-runtime")]
        {
            let _ = backend;
            return self.acquire_development(data_root, request);
        }
        #[cfg(not(feature = "development-runtime"))]
        {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e|e.to_string())?.as_millis() as i64;
        if now < self.suspended_until_ms { return Err(format!("设备新渲染任务已暂停，解封时间={}；已有任务、快速模式、日志与更新不受影响", self.suspended_until_ms)); }
        let config = nova_protected_loader::trusted_config(data_root.join("protection/devices")).map_err(|e|e.to_string())?;
        self.sequence += 1;
        let key = retry_fingerprint(request)?;
        let task_id = self.retries.entry(key.clone()).or_insert_with(|| format!("task-{}-{now}-{}", std::process::id(), self.sequence)).clone();
        let mut session = match TaskSession::acquire(&config, &task_id) {
            Ok(session) => session,
            Err(error) => {
                if let Some(limited) = error.downcast_ref::<nova_protection::client::RateLimited>() {
                    self.suspended_until_ms = limited.unban_ms.unwrap_or(now + 1_800_000);
                }
                return Err(error.to_string());
            }
        };
        let ticket = session.ticket();
        let root = self.runtime_root.get_or_insert_with(|| data_root.join("protection/runtime").join(format!("{}-{now}", std::process::id()))).clone();
        nova_protection::client::ensure_private_directory(&root).map_err(|e|e.to_string())?;
        let hash = &ticket.response.signed_manifest.manifest.plaintext_sha256;
        let module_path = root.join(format!("{hash}.{}", nova_protected_loader::MODULE_EXTENSION));
        // The only distributed implementation is authenticated ciphertext. Never
        // accept a module or trust root supplied by the request or source file.
        if module_path.exists() {
            TaskSession::verify_materialized_module(&config, &ticket, &module_path).map_err(|e|e.to_string())?;
        } else {
            let ciphertext = std::fs::read(backend.join("protected").join(format!("{}.enc", nova_protected_loader::MODULE_ID))).map_err(|e|format!("读取受保护模块: {e}"))?;
            session.decrypt_to_private_file(&ciphertext, &module_path).map_err(|e|e.to_string())?;
        }
        session.release_content_key();
        let ticket_path = root.join(format!("{task_id}.json"));
        std::fs::write(&ticket_path, serde_json::to_vec(&ticket).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        self.retries.remove(&key);
        Ok(ProtectedTask { ticket_path, module_path })
        }
    }

    #[cfg(feature = "development-runtime")]
    fn acquire_development(&mut self, data_root: &Path, request: &[u8]) -> Result<ProtectedTask, String> {
        use nova_protected_loader::development;
        let selected = development::runtime().map_err(|e| e.to_string())?;
        let value: serde_json::Value = serde_json::from_slice(request).map_err(|e| e.to_string())?;
        let mode = value.get("displayMode").or_else(|| value.pointer("/settings/displayMode"))
            .or_else(|| value.pointer("/buildOptions/displayMode"))
            .and_then(|value| value.as_str()).ok_or("development render request has no display mode")?;
        let requires_shared_light = match mode {
            "standard" => false,
            "full" => true,
            _ => return Err(format!("unsupported development core display mode: {mode}")),
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_millis();
        self.sequence += 1;
        let key = retry_fingerprint(request)?;
        let task_id = self.retries.entry(key.clone())
            .or_insert_with(|| format!("dev-{}-{now}-{}", std::process::id(), self.sequence)).clone();
        let root = self.runtime_root.get_or_insert_with(|| data_root.join("protection/runtime")
            .join(format!("dev-{}-{now}", std::process::id()))).clone();
        nova_protection::client::ensure_private_directory(&root).map_err(|e| e.to_string())?;
        let context = development::DevelopmentTask {
            schema: "nova.development-task.v1".into(),
            runtime_build_id: selected.runtime_build_id,
            module_sha256: selected.module_sha256,
            task_id: task_id.clone(),
            requires_shared_light,
            remote_ticket_path: requires_shared_light.then(|| root.join(format!("{task_id}.remote.json"))),
        };
        development::validate_task_context(&context).map_err(|e| e.to_string())?;
        let module_path = root.join(format!("{}.{}", context.module_sha256, nova_protected_loader::MODULE_EXTENSION));
        if !module_path.exists() {
            let source = development::source_module().map_err(|e| e.to_string())?;
            let mut file = std::fs::File::open(&source).map_err(|e| e.to_string())?;
            development::verify_module(&mut file, &context).map_err(|e| e.to_string())?;
            std::fs::copy(&source, &module_path).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&module_path, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
            }
        }
        let mut file = std::fs::File::open(&module_path).map_err(|e| e.to_string())?;
        development::verify_module(&mut file, &context).map_err(|e| e.to_string())?;
        development::prepare_remote_task(&context, data_root.join("protection/devices")).map_err(|e| e.to_string())?;
        let ticket_path = root.join(format!("{task_id}.json"));
        std::fs::write(&ticket_path, serde_json::to_vec(&context).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        self.retries.remove(&key);
        // This is an explicit local DevelopmentTask, never a forged signed grant.
        // Standard makes no network request; Full has one real shared-light
        // service grant, resumed across children independently of local loading.
        Ok(ProtectedTask { ticket_path, module_path })
    }
}

// Click timing is telemetry, not a new rendering input. Network retries retain
// their task identity even when the user clicks again after a failed response.
fn retry_fingerprint(request: &[u8]) -> Result<String, String> {
    let mut value: serde_json::Value = serde_json::from_slice(request).map_err(|e| e.to_string())?;
    if let Some(options) = value.get_mut("buildOptions").and_then(|v| v.as_object_mut()) {
        options.remove("clickStartedAtEpochMs");
    }
    let normalized = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(normalized)))
}

impl Drop for ProtectedRender {
    fn drop(&mut self) {
        // Only this Host's generated directory is owned here. A live module may
        // remain mapped by a child; never force unload or kill another process.
        if let Some(root) = &self.runtime_root {
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    let path=entry.path();
                    let extension = path.extension().and_then(|s|s.to_str());
                    if entry.file_type().is_ok_and(|kind|kind.is_file()) && (extension == Some("json") || extension == Some(nova_protected_loader::MODULE_EXTENSION)) {
                        let _=std::fs::remove_file(path);
                    }
                }
            }
            let _=std::fs::remove_dir(root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_identity_ignores_only_click_timing() {
        let first = br#"{"sourcePath":"a.litematic","buildOptions":{"clickStartedAtEpochMs":"1","quality":"high"}}"#;
        let retry = br#"{"sourcePath":"a.litematic","buildOptions":{"clickStartedAtEpochMs":"2","quality":"high"}}"#;
        let changed = br#"{"sourcePath":"a.litematic","buildOptions":{"clickStartedAtEpochMs":"2","quality":"low"}}"#;
        assert_eq!(retry_fingerprint(first).unwrap(), retry_fingerprint(retry).unwrap());
        assert_ne!(retry_fingerprint(first).unwrap(), retry_fingerprint(changed).unwrap());
    }
    #[test]
    fn normal_host_release_cleans_only_its_materialized_runtime() {
        let root=std::env::temp_dir().join(format!("nova-owned-runtime-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join(nova_protected_loader::MODULE_ID),b"fixture module").unwrap();
        std::fs::write(root.join("task.json"),b"fixture ticket").unwrap();
        let mut host=ProtectedRender::default();
        host.runtime_root=Some(root.clone());
        drop(host);
        assert!(!root.exists());
    }
    #[test]
    fn host_runtime_cleanup_preserves_other_file_types() {
        let root=std::env::temp_dir().join(format!("nova-owned-runtime-preserve-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join(nova_protected_loader::MODULE_ID),b"fixture module").unwrap();
        std::fs::write(root.join("keep.txt"),b"unrelated file").unwrap();
        let mut host=ProtectedRender::default();
        host.runtime_root=Some(root.clone());
        drop(host);
        assert!(!root.join(nova_protected_loader::MODULE_ID).exists());
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(),b"unrelated file");
        std::fs::remove_file(root.join("keep.txt")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn preflight_and_start_share_one_task_but_consume_admission_once() {
        let mut host=ProtectedRender::default();
        let task=ProtectedTask { ticket_path:PathBuf::from("task.json"),module_path:PathBuf::from("core.dll") };
        host.remember_admission("admission-a".into(),task.clone());
        assert_eq!(host.take_admission("admission-a").unwrap(),task);
        assert!(host.take_admission("admission-a").is_err());
    }
    #[test]
    fn concurrent_preflights_keep_distinct_task_contexts() {
        let mut host=ProtectedRender::default();
        for id in ["a","b"] {host.remember_admission(id.into(),ProtectedTask {ticket_path:PathBuf::from(id),module_path:PathBuf::from("core.dll")});}
        assert_eq!(host.take_admission("b").unwrap().ticket_path,PathBuf::from("b"));
        assert_eq!(host.take_admission("a").unwrap().ticket_path,PathBuf::from("a"));
    }
}
