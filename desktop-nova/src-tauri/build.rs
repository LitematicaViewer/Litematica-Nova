use std::env;
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use sha2::{Digest, Sha256};
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

fn watch_git_identity(repo_root: &Path) {
    let Some(git_dir) = git_output(repo_root, &["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let git_dir = PathBuf::from(git_dir);
    println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join("packed-refs").display()
    );

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
        println!("cargo:rerun-if-changed={reference_path}");
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
    println!("cargo:rerun-if-changed={}", source.display());
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
fn validate_executable_target(path: &Path, target_os: &str, target_arch: &str) {
    let mut file = fs::File::open(path).expect("open public helper executable");
    let mut header = [0u8; 64];
    file.read_exact(&mut header).expect("public helper executable is truncated");
    match target_os {
        "windows" => {
            assert_eq!(&header[..2], b"MZ", "public helper is not a Windows PE executable");
            let offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as u64;
            file.seek(SeekFrom::Start(offset)).expect("read PE header offset");
            let mut pe = [0u8; 26];
            file.read_exact(&mut pe).expect("public helper PE header is truncated");
            assert_eq!(&pe[..4], b"PE\0\0");
            let machine = u16::from_le_bytes(pe[4..6].try_into().unwrap());
            assert_eq!(machine, if target_arch == "x86_64" { 0x8664 } else { 0xaa64 }, "public helper architecture differs");
            assert_eq!(u16::from_le_bytes(pe[22..24].try_into().unwrap()) & 0x2000, 0, "a DLL cannot replace a public helper");
            assert_eq!(u16::from_le_bytes(pe[24..26].try_into().unwrap()), 0x20b, "public helper must be PE32+");
        }
        "linux" => {
            assert_eq!(&header[..6], b"\x7fELF\x02\x01", "public helper must be little-endian ELF64");
            assert!(matches!(u16::from_le_bytes(header[16..18].try_into().unwrap()), 2 | 3), "public helper must be an executable or PIE");
            assert_eq!(u16::from_le_bytes(header[18..20].try_into().unwrap()), if target_arch == "x86_64" { 62 } else { 183 }, "public helper ELF architecture differs");
        }
        "macos" => {
            assert_eq!(u32::from_le_bytes(header[..4].try_into().unwrap()), 0xfeedfacf, "public helper must be target-specific Mach-O64");
            assert_eq!(u32::from_le_bytes(header[4..8].try_into().unwrap()), if target_arch == "x86_64" { 0x01000007 } else { 0x0100000c }, "public helper Mach-O architecture differs");
            assert_eq!(u32::from_le_bytes(header[12..16].try_into().unwrap()), 2, "public helper must be a Mach-O executable");
        }
        _ => panic!("unsupported public helper target OS"),
    }
}

fn validate_public_backends(repo_root: &Path) {
    println!("cargo:rerun-if-env-changed=NOVA_PUBLIC_BACKEND_DIR");
    println!("cargo:rerun-if-env-changed=NOVA_PROTECTED_BUILD_ID");
    let explicit_directory = env::var_os("NOVA_PUBLIC_BACKEND_DIR");
    let directory = explicit_directory.as_ref().map(PathBuf::from).unwrap_or_else(|| repo_root.join("bin/public-beta"));
    println!("cargo:rerun-if-changed={}", directory.display());
    // An isolated CLI debug/type check may have no staged resources. A selected
    // platform stage and every real release package must contain real helpers.
    if !directory.exists() {
        assert!(explicit_directory.is_none() && env::var("PROFILE").as_deref() != Ok("release"), "the selected public helper stage is missing");
        return;
    }
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("public helper target OS");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("public helper target architecture");
    assert!(matches!(target_arch.as_str(), "x86_64" | "aarch64"), "public helpers require a supported 64-bit target");
    let suffix = if target_os == "windows" { ".exe" } else { "" };
    let expected: HashSet<_> = [format!("litematica_core{suffix}"), format!("litematica_native_viewer{suffix}"), format!("nova-render-service{suffix}"), format!("nova-build-worker{suffix}"), "backend-deployment.json".into()].into_iter().collect();
    let mut actual = HashSet::new();
    for entry in fs::read_dir(&directory).expect("read isolated public backend directory") {
        let entry = entry.expect("read public backend entry");
        assert!(entry.file_type().expect("read public backend type").is_file(), "public backend resource must be a regular file");
        println!("cargo:rerun-if-changed={}", entry.path().display());
        actual.insert(entry.file_name().to_str().expect("public backend UTF-8 name").to_owned());
    }
    assert_eq!(actual, expected, "public backend directory must contain only the four public helpers and manifest");
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(directory.join("backend-deployment.json")).expect("read public deployment manifest")).expect("parse public deployment manifest");
    assert_eq!(manifest["schema"], "lba.backend.deployment.v1");
    let source = manifest["gitSha"].as_str().expect("public deployment source commit");
    assert!(valid_git_sha(source), "public deployment source identity is malformed");
    if env::var("PROFILE").as_deref() == Ok("release") || env::var_os("LBA_BUILD_GIT_SHA").is_some() {
        assert_eq!(source, build_git_sha(repo_root), "public helpers and Host source commits differ");
    }
    if let Some(triple) = manifest.get("targetTriple") {
        assert_eq!(triple.as_str(), env::var("TARGET").ok().as_deref(), "public helper platform/architecture identity differs");
    }
    if let Ok(build_id) = env::var("NOVA_PROTECTED_BUILD_ID") {
        assert_eq!(manifest["runtimeBuildId"].as_str(), Some(build_id.as_str()), "public helper runtime components differ from the Host build");
    }
    let artifacts = manifest["artifacts"].as_object().expect("public deployment artifacts");
    let names: HashSet<_> = artifacts.keys().cloned().collect();
    assert_eq!(names, expected.into_iter().filter(|name| name != "backend-deployment.json").collect());
    for (name, record) in artifacts {
        let path = directory.join(name);
        let bytes = fs::metadata(&path).expect("public helper metadata").len();
        assert!(bytes > 64 && bytes <= 512 * 1024 * 1024, "public helper size exceeds its expected bounds");
        assert_eq!(record["bytes"].as_u64(), Some(bytes), "public helper size differs from its manifest");
        validate_executable_target(&path, &target_os, &target_arch);
        let mut file = fs::File::open(&path).expect("open staged public helper");
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).expect("hash staged public helper");
            if count == 0 { break; }
            hash.update(&buffer[..count]);
        }
        assert_eq!(record["sha256"].as_str(), Some(format!("{:x}",hash.finalize()).as_str()), "public helper content differs from its manifest");
    }
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
