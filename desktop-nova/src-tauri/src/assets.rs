//! Game-resource manager backend (L1 of the restoration).
//!
//! Ported from the pre-merge `desktop-nova/src-tauri/src/main.rs`. This module owns
//! the file/archive side of the resource registry:
//!
//!   * constants that describe where each resource kind lives inside the app-data dir,
//!   * the output types shared with `desktop-nova/src/services/gameResources.ts`,
//!   * directory helpers,
//!   * unpacking the bundled icon archives (`pack-in/game-icons/{block,item}.zip`)
//!     into the app-data icon roots,
//!   * small app-data file operations used by the registry (delete).
//!
//! Remote sources (CCVault / Minecraft Wiki / GitHub language packs) are a later
//! step; see `docs/ASSET_MANAGER_DIFF.md`.

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use zip::ZipArchive;

use crate::{current_user_config_dir, get_root, user_runtime_relative_path};

// ---------------------------------------------------------------------------
// Archive + icon root locations.
//
// NOTE: the pre-merge branch pointed these at `pack-in/arr-private/`. The current
// tree ships the same archives as `pack-in/game-icons/` (they are inside the
// distributable resource set listed in docs/THIRD_PARTY_NOTICES.txt), so the
// app-data icon roots are populated from there instead of committing the ~940
// Mojang PNGs into the repository.
// ---------------------------------------------------------------------------
pub(crate) const BUILTIN_BLOCK_ARCHIVE_RELPATH: &str = "pack-in/game-icons/block.zip";
pub(crate) const BUILTIN_ITEM_ARCHIVE_RELPATH: &str = "pack-in/game-icons/item.zip";
pub(crate) const BUILTIN_BLOCK_ICON_ROOT_RELPATH: &str = "minecraft-assets/block_2d/initial";
pub(crate) const BUILTIN_ITEM_ICON_ROOT_RELPATH: &str = "minecraft-assets/item/initial";

// ---------------------------------------------------------------------------
// Output types (mirrored by the TypeScript interfaces in platform/files.ts).
// ---------------------------------------------------------------------------
#[derive(Serialize)]
pub(crate) struct BuiltinIconExtractOutput {
    pub target_dir: String,
    pub root_relpath: String,
    pub extracted: usize,
}

#[derive(Serialize)]
pub(crate) struct VaultBlockIconDownloadOutput {
    pub total: usize,
    pub downloaded: usize,
    pub target_dir: String,
    pub root_relpath: String,
}

#[derive(Serialize)]
pub(crate) struct WikiEnumCatalogDownloadOutput {
    pub target_dir: String,
    pub root_relpath: String,
    pub blocks: usize,
    pub items: usize,
    pub enchantments: usize,
    pub entities: usize,
}

#[derive(Clone, Serialize)]
pub(crate) struct VaultBlockIconProgress {
    pub current: usize,
    pub total: usize,
    pub downloaded: usize,
    pub status: String,
}

// ---------------------------------------------------------------------------
// Directory helpers.
// ---------------------------------------------------------------------------

/// True when the directory exists and contains at least one file, at any depth.
pub(crate) fn directory_contains_files(dir: &Path) -> Result<bool, String> {
    if !dir.is_dir() {
        return Ok(false);
    }
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if metadata.is_file() {
            return Ok(true);
        }
        if metadata.is_dir() && directory_contains_files(&path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Removes the contents of a directory without removing the directory itself.
pub(crate) fn clear_directory_contents(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            std::fs::remove_dir_all(path).map_err(|e| e.to_string())?;
        } else {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Resolves an app-data relative path into an absolute path inside the config dir.
pub(crate) fn app_data_path(relative_path: &str) -> Result<PathBuf, String> {
    Ok(current_user_config_dir()?.join(user_runtime_relative_path(relative_path)?))
}

// ---------------------------------------------------------------------------
// Builtin icon archives.
// ---------------------------------------------------------------------------

/// Extracts a bundled icon archive into its app-data icon root.
///
/// Skips work when the target already holds files, unless `force` is set.
pub(crate) fn extract_builtin_icon_archive(
    archive_relpath: &str,
    target_relpath: &str,
    force: bool,
) -> Result<BuiltinIconExtractOutput, String> {
    let target_dir = current_user_config_dir()?.join(target_relpath);
    if !force && directory_contains_files(&target_dir)? {
        return Ok(BuiltinIconExtractOutput {
            target_dir: target_dir.display().to_string(),
            root_relpath: target_relpath.to_string(),
            extracted: 0,
        });
    }

    let archive_path = get_root().join(archive_relpath);
    let archive_file = std::fs::File::open(&archive_path)
        .map_err(|e| format!("open {} failed: {}", archive_path.display(), e))?;
    let mut archive = ZipArchive::new(archive_file)
        .map_err(|e| format!("open zip {} failed: {}", archive_path.display(), e))?;

    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    clear_directory_contents(&target_dir)?;

    let mut extracted = 0usize;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let Some(enclosed_name) = file.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };
        if enclosed_name.as_os_str().is_empty() {
            continue;
        }
        let output_path = target_dir.join(&enclosed_name);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut output_file = std::fs::File::create(&output_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut output_file).map_err(|e| e.to_string())?;
        extracted += 1;
    }

    Ok(BuiltinIconExtractOutput {
        target_dir: target_dir.display().to_string(),
        root_relpath: target_relpath.to_string(),
        extracted,
    })
}

// ---------------------------------------------------------------------------
// Tauri commands: builtin extraction + app-data file operations.
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn ensure_builtin_block_icons_extracted(
    force: bool,
) -> Result<BuiltinIconExtractOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        extract_builtin_icon_archive(
            BUILTIN_BLOCK_ARCHIVE_RELPATH,
            BUILTIN_BLOCK_ICON_ROOT_RELPATH,
            force,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn ensure_builtin_item_icons_extracted(
    force: bool,
) -> Result<BuiltinIconExtractOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        extract_builtin_icon_archive(
            BUILTIN_ITEM_ARCHIVE_RELPATH,
            BUILTIN_ITEM_ICON_ROOT_RELPATH,
            force,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn delete_user_config_file(relative_path: String) -> Result<(), String> {
    let path = app_data_path(&relative_path)?;
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

/// Opens the native picker for a `.litematic` file and returns the chosen path.
#[tauri::command]
pub(crate) async fn pick_litematic_file(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Litematica", &["litematic"])
        .blocking_pick_file();
    Ok(picked.map(|path| path.to_string()))
}

// ---------------------------------------------------------------------------
// Remote download sources: contract only, not enabled yet.
//
// The pre-merge implementation fetched these from the network:
//   * CCVault (https://ccvaults.com/) for 2D/3D block icons and item icons, and
//   * Minecraft Wiki (English + Chinese) for block icons and the enum catalogs.
//
// They are intentionally not wired up in this step because
//   a) the CCVault path depends on API keys hardcoded in the pre-merge source
//      (`VAULT_API_KEY` / `VAULT_LEGACY_API_KEY`, plus runtime key discovery from
//      the site's own JS bundle) - credentials that must not be carried over
//      as-is, and
//   b) the Wiki path is HTML scraping (`wiki_block_icons.rs`, 682 lines) whose
//      parent-module helpers still need porting.
//
// These commands keep the HTTP contract stable so the TypeScript layer and the
// (user-owned) L3 UI can bind to them today; every call fails with an explicit,
// actionable message instead of a silent no-op. `pack-in/game-icons/*.zip`
// extraction above is the offline source that always works.
// ---------------------------------------------------------------------------

fn remote_source_unavailable(source: &str) -> String {
    format!(
        "{source} 下载源尚未接入：本地内建图标解包可用（pack-in/game-icons/*.zip），\
         远端下载需要先处理凭据与抓取逻辑，详见 docs/ASSET_MANAGER_DIFF.md 的「恢复进展」。"
    )
}

#[tauri::command]
pub(crate) async fn download_vault_block_icons() -> Result<VaultBlockIconDownloadOutput, String> {
    Err(remote_source_unavailable("CCVault 方块图标"))
}

#[tauri::command]
pub(crate) async fn download_vault_item_icons() -> Result<VaultBlockIconDownloadOutput, String> {
    Err(remote_source_unavailable("CCVault 物品图标"))
}

#[tauri::command]
pub(crate) async fn download_minecraft_wiki_block_icons(
    _slot: String,
    _source: String,
) -> Result<VaultBlockIconDownloadOutput, String> {
    Err(remote_source_unavailable("Minecraft Wiki 方块图标"))
}

#[tauri::command]
pub(crate) async fn download_minecraft_wiki_enum_catalogs(
) -> Result<WikiEnumCatalogDownloadOutput, String> {
    Err(remote_source_unavailable("Minecraft Wiki 枚举全集"))
}
