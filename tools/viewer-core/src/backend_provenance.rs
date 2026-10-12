use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::process_io::open_process_read_file;

pub const BACKEND_PROVENANCE_SCHEMA: &str = "lba.backend.provenance.v1";
pub const BACKEND_PROVENANCE_PREFIX: &str = "[LBA_BACKEND_PROVENANCE] ";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendProvenance {
    schema: &'static str,
    git_sha: &'static str,
    profile: &'static str,
    exe_path: PathBuf,
    sha256: String,
}

impl BackendProvenance {
    pub const fn schema(&self) -> &'static str {
        self.schema
    }

    pub const fn git_sha(&self) -> &'static str {
        self.git_sha
    }

    pub const fn profile(&self) -> &'static str {
        self.profile
    }

    pub fn exe_path(&self) -> &Path {
        &self.exe_path
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

pub fn collect_backend_provenance() -> Result<BackendProvenance> {
    let current_exe = std::env::current_exe().context("resolve current backend executable path")?;
    let exe_path = current_exe
        .canonicalize()
        .with_context(|| format!("canonicalize backend executable {}", current_exe.display()))?;
    let sha256 = sha256_file(&exe_path)
        .with_context(|| format!("hash backend executable {}", exe_path.display()))?;
    Ok(BackendProvenance {
        schema: BACKEND_PROVENANCE_SCHEMA,
        git_sha: env!("LBA_BACKEND_GIT_SHA"),
        profile: env!("LBA_BACKEND_PROFILE"),
        exe_path,
        sha256,
    })
}

pub fn emit_backend_provenance() -> Result<()> {
    let provenance = collect_backend_provenance()?;
    emit_collected_backend_provenance(&provenance)
}

pub fn emit_collected_backend_provenance(provenance: &BackendProvenance) -> Result<()> {
    println!(
        "{BACKEND_PROVENANCE_PREFIX}{}",
        serde_json::to_string(&provenance).context("serialize backend provenance")?
    );
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut reader = open_process_read_file(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_backend_provenance_is_exact_and_self_hashing() {
        let provenance = collect_backend_provenance().unwrap();
        assert_eq!(provenance.schema(), BACKEND_PROVENANCE_SCHEMA);
        assert!(matches!(provenance.git_sha().len(), 40 | 64));
        assert!(
            provenance
                .git_sha()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert!(matches!(provenance.profile(), "debug" | "release"));
        assert!(provenance.exe_path().is_absolute());
        assert_eq!(
            provenance.sha256(),
            sha256_file(provenance.exe_path()).unwrap()
        );
        assert_eq!(provenance.sha256().len(), 64);
        assert!(
            provenance
                .sha256()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );

        let json = serde_json::to_value(&provenance).unwrap();
        assert_eq!(json["schema"], BACKEND_PROVENANCE_SCHEMA);
        assert_eq!(json["gitSha"], provenance.git_sha());
        assert_eq!(json["profile"], provenance.profile());
        assert_eq!(json["sha256"], provenance.sha256());
        assert!(json.get("exePath").is_some());
    }
}
