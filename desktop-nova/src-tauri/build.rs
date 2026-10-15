use std::env;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git_output(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn valid_git_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Register a `rerun-if-changed` path only when it actually exists.
///
/// Cargo treats a registered path that is missing as modified on *every* build,
/// so watching a file that is not there reruns this build script - and rebuilds
/// the whole crate - forever. That is exactly what happened while
/// `.git/refs/heads/<branch>` was watched in a checkout whose branch tips live
/// in `packed-refs`, and it also hits a fresh clone before `bin/public-beta` is
/// staged.
fn watch_if_present(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn watch_git_identity(repo_root: &Path) {
    let Some(git_dir) = git_output(repo_root, &["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let git_dir = PathBuf::from(git_dir);
    watch_if_present(&git_dir.join("HEAD"));
    watch_if_present(&git_dir.join("packed-refs"));
    // A branch tip is either packed (packed-refs) or loose. Watching the
    // directory catches a tip that is written as a loose ref for the first time,
    // which is what a commit does even when packed-refs itself stays untouched.
    watch_if_present(&git_dir.join("refs/heads"));

    let Some(symbolic_ref) = git_output(repo_root, &["symbolic-ref", "-q", "HEAD"]) else {
        return;
    };
    if let Some(reference_path) = git_output(
        repo_root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &symbolic_ref,
        ],
    ) {
        watch_if_present(Path::new(&reference_path));
    }
}

fn build_git_sha(repo_root: &Path) -> String {
    println!("cargo:rerun-if-env-changed=LBA_BUILD_GIT_SHA");
    let value = env::var("LBA_BUILD_GIT_SHA")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            git_output(repo_root, &["rev-parse", "--verify", "HEAD"])
                .expect("desktop-nova build requires Git HEAD or LBA_BUILD_GIT_SHA")
        })
        .trim()
        .to_ascii_lowercase();
    assert!(
        valid_git_sha(&value),
        "desktop-nova build Git SHA must be 40 or 64 lowercase hexadecimal characters"
    );
    value
}

fn prepare_block_icons(repo_root: &Path, manifest_dir: &Path) {
    let source = repo_root.join("pack-in/game-icons/block.zip");
    watch_if_present(&source);
    let target = manifest_dir.join("gen/block-icons");
    fs::create_dir_all(&target).expect("create generated block icon directory");
    let mut archive = zip::ZipArchive::new(fs::File::open(source).expect("open bundled block icon archive"))
        .expect("read bundled block icon archive");
    let mut current_names = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("read block icon entry");
        if entry.is_dir() { continue; }
        let path = entry.enclosed_name().expect("block icon entry must be enclosed");
        // Only the public image payload is deployed, never the archive or other files.
        if path.extension().and_then(|ext| ext.to_str()) != Some("png") { continue; }
        assert_eq!(path.components().count(), 1, "block icon PNG must have a flat name");
        let name = path.file_name().unwrap().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("decode block icon PNG");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "invalid block icon PNG");
        let output = target.join(&name);
        if fs::read(&output).ok().as_deref() != Some(bytes.as_slice()) {
            fs::write(&output, bytes).expect("write generated block icon PNG");
        }
        current_names.insert(name);
    }
    assert!(!current_names.is_empty(), "bundled block icon archive is empty");
    // Remove only stale files in this exact build-owned flat directory.
    // Do not traverse another directory or touch deployed/user resources.
    for entry in fs::read_dir(&target).expect("list generated block icons") {
        let entry = entry.expect("read generated block icon entry");
        if entry.file_type().expect("read generated block icon type").is_file()
            && !current_names.contains(&entry.file_name())
        {
            fs::remove_file(entry.path()).expect("remove stale generated block icon");
        }
    }
}

// Only this freshly staged directory is bundled. Legacy private backend and
// Release directories never enter the public resource map.
fn validate_public_backends(repo_root: &Path) {
    let directory = repo_root.join("bin/public-beta");
    // CLI-only type/unit checks may override bundle.resources=[] before any
    // backend build. Real Tauri packaging itself rejects a missing directory.
    if !directory.exists() { return; }
    // Registered only once the directory is there, so an unstaged checkout does
    // not pin a missing path and recompile on every build (see watch_if_present).
    watch_if_present(&directory);
    let suffix = if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") { ".exe" } else { "" };
    let expected: HashSet<_> = [format!("litematica_core{suffix}"), format!("litematica_native_viewer{suffix}"), format!("nova-render-service{suffix}"), format!("nova-build-worker{suffix}"), "backend-deployment.json".into()].into_iter().collect();
    let mut actual = HashSet::new();
    for entry in fs::read_dir(&directory).expect("read isolated public backend directory") {
        let entry = entry.expect("read public backend entry");
        assert!(entry.file_type().expect("read public backend type").is_file(), "public backend resource must be a regular file");
        actual.insert(entry.file_name().to_str().expect("public backend UTF-8 name").to_owned());
    }
    assert_eq!(actual, expected, "public backend directory must contain only the four public helpers and manifest");
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(directory.join("backend-deployment.json")).expect("read public deployment manifest")).expect("parse public deployment manifest");
    assert_eq!(manifest["schema"], "lba.backend.deployment.v1");
    let names: HashSet<_> = manifest["artifacts"].as_object().expect("public deployment artifacts").keys().cloned().collect();
    assert_eq!(names, expected.into_iter().filter(|name| name != "backend-deployment.json").collect());
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is required"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("desktop-nova src-tauri must be nested under the repository");
    let has_explicit_git_sha = env::var("LBA_BUILD_GIT_SHA")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    if !has_explicit_git_sha {
        watch_git_identity(repo_root);
    }
    println!(
        "cargo:rustc-env=LBA_NOVA_GIT_SHA={}",
        build_git_sha(repo_root)
    );
    prepare_block_icons(repo_root, &manifest_dir);
    validate_public_backends(repo_root);
    tauri_build::build()
}
