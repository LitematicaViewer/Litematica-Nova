use std::env;
#[cfg(windows)]
use std::fs;
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
                .expect("viewer-core build requires Git HEAD or LBA_BUILD_GIT_SHA")
        })
        .trim()
        .to_ascii_lowercase();
    assert!(
        valid_git_sha(&value),
        "viewer-core build Git SHA must be 40 or 64 lowercase hexadecimal characters"
    );
    value
}

fn build_profile() -> String {
    let profile = env::var("PROFILE").expect("Cargo PROFILE is required");
    assert!(
        matches!(profile.as_str(), "debug" | "release"),
        "viewer-core backend profile must be debug or release, found {profile:?}"
    );
    profile
}

#[cfg(windows)]
fn embed_windows_version_info() {
    let output_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR is required on Windows"));
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
        .join("../../desktop-nova/src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let icon = icon.to_string_lossy().replace('\\', "/");
    let version = env!("CARGO_PKG_VERSION");
    let version_major = env!("CARGO_PKG_VERSION_MAJOR");
    let version_minor = env!("CARGO_PKG_VERSION_MINOR");
    let version_patch = env!("CARGO_PKG_VERSION_PATCH");
    let resources = [
        (
            "litematica-native-viewer-version.rc",
            "litematica_native_viewer",
            "litematica_native_viewer.exe",
            "Litematica Nova 预览",
        ),
    ];

    for (resource_name, binary_name, executable_name, description) in resources {
        let resource_path = output_dir.join(resource_name);
        let resource = format!(
            r#"#pragma code_page(65001)

1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {version_major},{version_minor},{version_patch},0
PRODUCTVERSION {version_major},{version_minor},{version_patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "CompanyName", "Litematica-Nova\0"
            VALUE "FileDescription", "{description}\0"
            VALUE "FileVersion", "{version}\0"
            VALUE "InternalName", "{executable_name}\0"
            VALUE "OriginalFilename", "{executable_name}\0"
            VALUE "ProductName", "Litematica Nova\0"
            VALUE "ProductVersion", "{version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
        );
        fs::write(&resource_path, resource).unwrap_or_else(|error| {
            panic!(
                "failed to write Windows version information for {binary_name} to {}: {error}",
                resource_path.display()
            )
        });
        embed_resource::compile_for(resource_path, [binary_name], embed_resource::NONE)
            .manifest_required()
            .unwrap_or_else(|error| {
                panic!("failed to embed Windows version information for {binary_name}: {error}")
            });
    }
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is required"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("viewer-core must be nested under the repository tools directory");
    let has_explicit_git_sha = env::var("LBA_BUILD_GIT_SHA")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    if !has_explicit_git_sha {
        watch_git_identity(repo_root);
    }
    println!(
        "cargo:rustc-env=LBA_BACKEND_GIT_SHA={}",
        build_git_sha(repo_root)
    );
    println!("cargo:rustc-env=LBA_BACKEND_PROFILE={}", build_profile());
    #[cfg(windows)]
    embed_windows_version_info();
}
