use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

const HASH_BUFFER_BYTES: usize = 256 * 1024;

/// Fail closed before the host admits either public helper. This validates the
/// resource manifest and bytes, never probes the legacy private backend path.
pub(crate) fn verify_public_deployment(directory: &Path) -> Result<(), String> {
    use std::collections::BTreeSet;
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(directory.join("backend-deployment.json"))
        .map_err(|error| format!("读取公共版部署清单失败：{error}"))?)
        .map_err(|error| format!("公共版部署清单格式错误：{error}"))?;
    if manifest["schema"] != "nova.public.beta.backends.v1" || manifest["publicBeta"] != true || manifest["flavor"] != "fast-only" {
        return Err("后端不是已隔离的 Beta 公共版部署。".into());
    }
    let expected: BTreeSet<_> = ["litematica_core", "litematica_native_viewer"].into_iter()
        .map(|name| format!("{name}{}", std::env::consts::EXE_SUFFIX)).collect();
    let records = manifest["artifacts"].as_object().ok_or("公共版部署清单缺少后端校验记录。")?;
    if records.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err("公共版部署清单包含非公共后端或缺少快速预览后端。".into());
    }
    let mut files = BTreeSet::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry.file_type().map_err(|error| error.to_string())?.is_file() { return Err("公共版后端目录包含非普通文件。".into()); }
        files.insert(entry.file_name().to_string_lossy().into_owned());
    }
    let mut expected_files = expected.clone();
    expected_files.insert("backend-deployment.json".into());
    if files != expected_files { return Err("公共版后端目录包含额外文件。".into()); }
    for name in expected {
        let identity = ViewerBackendIdentity::capture(&directory.join(&name))?;
        if records[&name]["sha256"].as_str() != Some(identity.sha256()) || records[&name]["bytes"].as_u64() != Some(identity.bytes()) {
            return Err(format!("公共版后端完整性校验失败：{name}"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ViewerBackendIdentity {
    exe_path: PathBuf,
    bytes: u64,
    sha256: String,
}

impl ViewerBackendIdentity {
    pub(crate) fn capture(exe_path: &Path) -> Result<Self, String> {
        let canonical = fs::canonicalize(exe_path).map_err(|error| {
            format!(
                "resolve expected Viewer executable {}: {error}",
                exe_path.display()
            )
        })?;
        let path_before = fs::metadata(&canonical).map_err(|error| {
            format!(
                "read expected Viewer executable metadata {}: {error}",
                canonical.display()
            )
        })?;
        if !path_before.is_file() {
            return Err(format!(
                "expected Viewer executable is not a file: {}",
                canonical.display()
            ));
        }

        let mut file = File::open(&canonical).map_err(|error| {
            format!(
                "open expected Viewer executable {}: {error}",
                canonical.display()
            )
        })?;
        let handle_before = file.metadata().map_err(|error| {
            format!(
                "read open Viewer executable metadata {}: {error}",
                canonical.display()
            )
        })?;
        if !same_file_snapshot(&path_before, &handle_before) {
            return Err(format!(
                "Viewer executable changed before hashing: {}",
                canonical.display()
            ));
        }

        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
        loop {
            let read = file.read(&mut buffer).map_err(|error| {
                format!(
                    "hash expected Viewer executable {}: {error}",
                    canonical.display()
                )
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }

        let handle_after = file.metadata().map_err(|error| {
            format!(
                "re-read open Viewer executable metadata {}: {error}",
                canonical.display()
            )
        })?;
        let path_after = fs::metadata(&canonical).map_err(|error| {
            format!(
                "re-read Viewer executable metadata {}: {error}",
                canonical.display()
            )
        })?;
        let canonical_after = fs::canonicalize(exe_path).map_err(|error| {
            format!(
                "re-resolve expected Viewer executable {}: {error}",
                exe_path.display()
            )
        })?;
        if canonical_after != canonical
            || !same_file_snapshot(&path_before, &handle_after)
            || !same_file_snapshot(&path_before, &path_after)
        {
            return Err(format!(
                "Viewer executable changed while hashing: {}",
                canonical.display()
            ));
        }

        Ok(Self {
            exe_path: canonical,
            bytes: path_before.len(),
            sha256: format!("{:x}", hasher.finalize()),
        })
    }

    pub(crate) fn exe_path(&self) -> &Path {
        &self.exe_path
    }

    pub(crate) const fn bytes(&self) -> u64 {
        self.bytes
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

fn same_file_snapshot(left: &Metadata, right: &Metadata) -> bool {
    left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
        && left.created().ok() == right.created().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lba-viewer-backend-{name}-{}-{nonce}.exe",
            std::process::id()
        ))
    }

    fn public_fixture() -> PathBuf {
        let path = fixture_path("public-deployment");
        fs::create_dir(&path).unwrap();
        let mut artifacts = serde_json::Map::new();
        for name in ["litematica_core", "litematica_native_viewer"] {
            let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
            let bytes = b"test helper bytes";
            fs::write(path.join(&name), bytes).unwrap();
            artifacts.insert(name, serde_json::json!({"sha256":format!("{:x}", Sha256::digest(bytes)),"bytes":bytes.len()}));
        }
        fs::write(path.join("backend-deployment.json"), serde_json::to_vec(&serde_json::json!({
            "schema":"nova.public.beta.backends.v1","publicBeta":true,"flavor":"fast-only","artifacts":artifacts
        })).unwrap()).unwrap();
        path
    }

    #[test]
    fn public_deployment_accepts_exact_verified_helpers() {
        let path = public_fixture();
        verify_public_deployment(&path).unwrap();
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn public_deployment_rejects_extra_files_and_tampering() {
        let path = public_fixture();
        let extra = path.join("extra.bin");
        fs::write(&extra, b"unexpected").unwrap();
        assert!(verify_public_deployment(&path).unwrap_err().contains("额外文件"));
        fs::remove_file(extra).unwrap();
        fs::write(path.join(format!("litematica_core{}", std::env::consts::EXE_SUFFIX)), b"modified").unwrap();
        assert!(verify_public_deployment(&path).unwrap_err().contains("完整性校验失败"));
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn public_deployment_rejects_legacy_manifest() {
        let path = public_fixture();
        fs::write(path.join("backend-deployment.json"), br#"{"schema":"lba.backend.deployment.v1","publicBeta":false}"#).unwrap();
        assert!(verify_public_deployment(&path).is_err());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn backend_identity_hashes_on_the_heap_with_a_small_thread_stack() {
        let path = fixture_path("small-stack");
        fs::write(&path, vec![0x5a; 4 * 1024 * 1024]).unwrap();
        let expected = format!("{:x}", Sha256::digest(fs::read(&path).unwrap()));
        let worker_path = path.clone();
        let identity = std::thread::Builder::new()
            .name("viewer-backend-small-stack".to_owned())
            .stack_size(128 * 1024)
            .spawn(move || ViewerBackendIdentity::capture(&worker_path))
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(identity.exe_path(), fs::canonicalize(&path).unwrap());
        assert_eq!(identity.bytes(), 4 * 1024 * 1024);
        assert_eq!(identity.sha256(), expected);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn backend_identity_rejects_non_files() {
        let path = fixture_path("directory");
        fs::create_dir_all(&path).unwrap();
        let error = ViewerBackendIdentity::capture(&path).unwrap_err();
        assert!(error.contains("not a file"));
        fs::remove_dir(path).unwrap();
    }
}
