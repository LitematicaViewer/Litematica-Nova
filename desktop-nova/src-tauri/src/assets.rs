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
//! Original CCVault, Wiki and GitHub resource flows share Host site lanes and
//! cancellation; resource writes stop when application shutdown is requested.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use crate::application::operations::{ApplicationOperations, ApplicationWriteGuard};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use zip::ZipArchive;

use crate::{current_user_config_dir, get_root, user_runtime_relative_path};

static RESOURCE_SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static RESOURCE_OPERATIONS: Mutex<Option<ApplicationOperations>> = Mutex::new(None);

pub(crate) fn bind_operations(operations: ApplicationOperations) {
    *RESOURCE_OPERATIONS.lock().expect("资源任务状态锁异常") = Some(operations);
}

/// Acquired only for the filesystem mutation phase; never during a HTTP request.
pub(crate) fn begin_resource_write() -> Result<ApplicationWriteGuard, String> {
    ensure_resource_work_allowed()?;
    let operations = RESOURCE_OPERATIONS.lock().map_err(|_| "资源任务状态锁异常")?
        .clone().ok_or("资源写入尚未绑定 Application Host")?;
    operations.begin_write()
}

pub(crate) fn cancel_for_shutdown() {
    RESOURCE_SHUTDOWN_REQUESTED.store(true, Ordering::Release);
}

pub(crate) fn resume_after_cancelled_shutdown() {
    RESOURCE_SHUTDOWN_REQUESTED.store(false, Ordering::Release);
}

pub(crate) fn ensure_resource_work_allowed() -> Result<(), String> {
    if RESOURCE_SHUTDOWN_REQUESTED.load(Ordering::Acquire) {
        return Err("资源任务已随应用退出取消。".to_string());
    }
    Ok(())
}

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
    ensure_resource_work_allowed()?;
    let target_dir = current_user_config_dir()?.join(target_relpath);
    let archive_path = get_root().join(archive_relpath);
    let archive_file = std::fs::File::open(&archive_path)
        .map_err(|e| format!("open {} failed: {}", archive_path.display(), e))?;
    let mut archive = ZipArchive::new(archive_file)
        .map_err(|e| format!("open zip {} failed: {}", archive_path.display(), e))?;

    let _write = begin_resource_write()?;
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    if force { clear_directory_contents(&target_dir)?; }

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
        if !force && output_path.is_file() { continue; }
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
    ensure_resource_work_allowed()?;
    let path = app_data_path(&relative_path)?;
    let _write = begin_resource_write()?;
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

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
// Minecraft Wiki (minecraft.wiki / zh.minecraft.wiki) download source.
//
// New Minecraft releases add blocks that ship no icon in any bundled pack, so the
// Wiki scraper is what keeps new blocks visible. `wiki_block_icons.rs` does the
// page/table parsing; the helpers below are the parent-module pieces it expects.
// ---------------------------------------------------------------------------

pub(crate) const WIKI_ENUM_BLOCKS_URL: &str =
    "https://minecraft.wiki/w/Java_Edition_data_values/Blocks";
const WIKI_ENUM_ITEMS_URL: &str = "https://minecraft.wiki/w/Java_Edition_data_values/Items";
const WIKI_ENUM_MAIN_URL: &str = "https://minecraft.wiki/w/Java_Edition_data_values";
const WIKI_ENUM_ENTITIES_URL: &str =
    "https://minecraft.wiki/w/Java_Edition_data_values/Entities";
pub(crate) const WIKI_ENUM_ROOT_RELPATH: &str = "enumerator/base/wiki";

/// Collapses runs of whitespace, matching the pre-merge normalisation.
pub(crate) fn normalize_html_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn wiki_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Litematica-Nova-enum-catalog/1.0")
        .build()
        .map_err(|e| e.to_string())
}

pub(crate) fn fetch_wiki_html(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<String, String> {
    ensure_resource_work_allowed()?;
    let response = crate::online_sources::send_request(client
        .get(url)
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml"), None)
        .map_err(|e| format!("请求 {url} 失败: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("请求 {url} 失败: HTTP {status}"));
    }
    let text = response.text().map_err(|e| e.to_string())?;
    ensure_resource_work_allowed()?;
    Ok(text)
}

pub(crate) fn element_text(element: &scraper::ElementRef<'_>) -> String {
    normalize_html_text(&element.text().collect::<Vec<_>>().join(" "))
}

pub(crate) fn extract_code_values_from_cell(
    cell: &scraper::ElementRef<'_>,
    code_selector: &scraper::Selector,
) -> Vec<String> {
    cell.select(code_selector)
        .map(|code| normalize_html_text(&code.text().collect::<Vec<_>>().join(" ")))
        .map(|value| value.trim_matches('`').trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

/// Normalises a namespaced resource location (`minecraft:stone`).
pub(crate) fn normalize_resource_location(value: &str) -> String {
    let trimmed = value.trim().trim_matches('`').trim().to_lowercase();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains(':') {
        trimmed
    } else {
        format!("minecraft:{trimmed}")
    }
}

// ---------------------------------------------------------------------------
// CCVault (https://ccvaults.com/) download source.
//
// The pre-merge build shipped hardcoded API keys in the source. Those are gone:
// the key is now discovered at runtime from the site's own JS bundle
// (`static/js/index.*.js`), which is where the site itself reads it from. No
// credential is stored in this repository.
// ---------------------------------------------------------------------------

const VAULT_SITE_LABEL: &str = "https://ccvaults.com/";
const VAULT_API_TOKEN_PATH: &str = "/api/token";
const VAULT_API_BLOCKS_PATH: &str = "/api/assets/20.%20Blocks";
const VAULT_API_ITEMS_PATH: &str = "/api/assets/10.%20Items";
const VAULT_API_ALL_ASSETS_PATH: &str = "/api/assets/all";
const VAULT_BLOCK_ICON_ROOT_RELPATH: &str = "minecraft-assets/block_icon/vault";
const VAULT_ITEM_ICON_ROOT_RELPATH: &str = "minecraft-assets/item/vault";
const VAULT_BLOCK_CATEGORY_NAME: &str = "20. Blocks";
const VAULT_ITEM_CATEGORY_NAME: &str = "10. Items";

fn vault_api_url(path: &str) -> Result<reqwest::Url, String> {
    reqwest::Url::parse(VAULT_SITE_LABEL)
        .and_then(|base| base.join(path))
        .map_err(|e| e.to_string())
}

fn vault_asset_url(parts: &[&str]) -> Result<String, String> {
    let mut url = reqwest::Url::parse(VAULT_SITE_LABEL).map_err(|e| e.to_string())?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "failed to build Vault asset URL".to_string())?;
        segments.clear();
        for part in parts {
            segments.push(part);
        }
    }
    Ok(url.to_string())
}

fn vault_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .cookie_store(true)
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| e.to_string())
}

/// Locates the site's own bundle (`static/js/index.<hash>.js`) in its HTML.
fn find_vault_index_script_path(html: &str) -> Option<String> {
    let marker = "src=\"";
    let mut offset = 0;
    while let Some(index) = html[offset..].find(marker) {
        let start = offset + index + marker.len();
        let Some(end) = html[start..].find('"').map(|value| start + value) else {
            break;
        };
        let src = &html[start..end];
        if src.contains("static/js/index.") && src.ends_with(".js") {
            return Some(src.to_string());
        }
        offset = end + 1;
    }
    None
}

fn find_between(haystack: &str, start: &str, end: &str) -> Option<String> {
    let start_index = haystack.find(start)? + start.len();
    let rest = &haystack[start_index..];
    let end_index = rest.find(end)?;
    Some(rest[..end_index].to_string())
}

/// Pulls the API key out of the site bundle.
///
/// Two shapes are recognised: the key bound right before the `/api/token` POST,
/// and a literal `x-api-key` header value.
fn extract_vault_api_key_from_script(script: &str) -> Option<String> {
    if let Some(prefix_index) = script.find("post(\"/api/token\"") {
        let prefix = &script[..prefix_index];
        if let Some(key_index) = prefix.rfind("const e=\"") {
            if let Some(candidate) = prefix[key_index + "const e=\"".len()..]
                .split('"')
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(candidate.to_string());
            }
        }
    }
    let candidate = find_between(script, "x-api-key\":\"", "\"")
        .or_else(|| find_between(script, "x-api-key\":", "}"));
    candidate.and_then(|value| {
        let trimmed = value.trim().trim_matches('"').trim();
        if trimmed.is_empty() || trimmed.contains(':') || trimmed.contains('{') {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Discovers the CCVault API key from the live site (no stored credential).
fn discover_vault_api_key(
    client: &reqwest::blocking::Client,
    home_html: &str,
) -> Option<String> {
    let script_path = find_vault_index_script_path(home_html)?;
    let script_url = reqwest::Url::parse(VAULT_SITE_LABEL)
        .ok()?
        .join(&script_path)
        .ok()?;
    let script = crate::online_sources::send_request(client.get(script_url), None).ok()?.text().ok()?;
    extract_vault_api_key_from_script(&script)
}

fn request_vault_token(
    client: &reqwest::blocking::Client,
    api_key: &str,
) -> Result<String, String> {
    let token_response = crate::online_sources::send_resource_request(client
        .post(vault_api_url(VAULT_API_TOKEN_PATH)?)
        .header("x-api-key", api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json;charset=UTF-8")
        .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
        .header(reqwest::header::ORIGIN, VAULT_SITE_LABEL.trim_end_matches('/'))
        .header(reqwest::header::REFERER, VAULT_SITE_LABEL)
        .header("X-Requested-With", "XMLHttpRequest")
        .body("{}"), None)
        .map_err(|e| format!("获取 Vault 令牌失败: {e}"))?;
    let status = token_response.status();
    let token_text = token_response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("获取 Vault 令牌失败: HTTP {status}"));
    }
    let token_payload: serde_json::Value = serde_json::from_str(&token_text)
        .map_err(|e| format!("Vault 令牌响应解析失败: {e}"))?;
    token_payload
        .get("token")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "Vault 未返回可用访问令牌".to_string())
}

/// Obtains a Vault token using a key discovered from the site itself.
fn acquire_vault_token(
    client: &reqwest::blocking::Client,
    home_html: &str,
) -> Result<String, String> {
    let Some(discovered_key) = discover_vault_api_key(client, home_html) else {
        return Err(
            "未能从 ccvaults.com 自动获取 API Key（站点 JS 结构可能已变化）".to_string(),
        );
    };
    request_vault_token(client, &discovered_key)
}

fn vault_fetch_json_with_auth(
    client: &reqwest::blocking::Client,
    path: &str,
    token: &str,
) -> Result<serde_json::Value, String> {
    let response = crate::online_sources::send_request(client
        .get(vault_api_url(path)?)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
        .header(reqwest::header::ORIGIN, VAULT_SITE_LABEL.trim_end_matches('/'))
        .header(reqwest::header::REFERER, VAULT_SITE_LABEL), None)
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Vault API failed: status={} body={}", status, text));
    }
    serde_json::from_str(&text).map_err(|e| format!("Vault JSON parse failed: {e}; body={text}"))
}

fn normalize_vault_block_id(file_name: &str) -> Option<String> {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name)
        .trim()
        .to_lowercase();
    if stem.is_empty() {
        return None;
    }
    let sanitized: String = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

fn collect_vault_category_entries(
    payload: &serde_json::Value,
    category_name: &str,
) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let Some(rows) = payload.as_array() else {
        return entries;
    };
    for row in rows {
        let Some(subcategories) = row.get("subcategories").and_then(|value| value.as_array()) else {
            continue;
        };
        for subcategory in subcategories {
            let Some(sub_name) = subcategory
                .get("name")
                .and_then(|value| value.as_str())
                .map(str::trim)
            else {
                continue;
            };
            if sub_name.is_empty() {
                continue;
            }
            let Some(files) = subcategory.get("files").and_then(|value| value.as_array()) else {
                continue;
            };
            for file in files {
                let Some(file_name) = file.as_str().map(str::trim) else {
                    continue;
                };
                if !file_name.to_lowercase().ends_with(".png") {
                    continue;
                }
                let Some(block_id) = normalize_vault_block_id(file_name) else {
                    continue;
                };
                if !seen.insert(block_id.clone()) {
                    continue;
                }
                if let Ok(url) = vault_asset_url(&["assets", category_name, sub_name, file_name]) {
                    entries.push((block_id, url));
                }
            }
        }
    }
    entries
}

fn collect_vault_category_entries_from_all(
    payload: &serde_json::Value,
    category_name: &str,
) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let Some(rows) = payload.as_array() else {
        return entries;
    };
    for row in rows {
        let Some(files) = row.get("files").and_then(|value| value.as_array()) else {
            continue;
        };
        for file in files {
            let Some(file_row) = file.as_object() else {
                continue;
            };
            let file_name = file_row
                .get("file")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or("");
            if !file_name.to_lowercase().ends_with(".png") {
                continue;
            }
            let category = file_row
                .get("category")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or("");
            if category != category_name {
                continue;
            }
            let Some(block_id) = normalize_vault_block_id(file_name) else {
                continue;
            };
            if !seen.insert(block_id.clone()) {
                continue;
            }
            let subcategory = file_row
                .get("subcategory")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or("");
            let url = if subcategory.is_empty() {
                vault_asset_url(&["assets", category, file_name])
            } else {
                vault_asset_url(&["assets", category, subcategory, file_name])
            };
            if let Ok(url) = url {
                entries.push((block_id, url));
            }
        }
    }
    entries
}

/// Downloads one icon, normalising it to `size`×`size` before storing it.
fn download_one_vault_icon(
    client: &reqwest::blocking::Client,
    target_dir: &Path,
    icon_id: &str,
    file_url: &str,
    size: u32,
    resize_filter: image::imageops::FilterType,
) -> bool {
    if ensure_resource_work_allowed().is_err() { return false; }
    let response = match crate::online_sources::send_request(client
        .get(file_url)
        .header(reqwest::header::USER_AGENT, "Litematica-Nova-icon-manager/1.0")
        .header(reqwest::header::REFERER, VAULT_SITE_LABEL), None)
    {
        Ok(value) => value,
        Err(_) => return false,
    };
    if !response.status().is_success() {
        return false;
    }
    let Ok(bytes) = response.bytes() else {
        return false;
    };
    let Ok(mut image) = image::load_from_memory(&bytes) else {
        return false;
    };
    use image::GenericImageView;
    if image.dimensions() != (size, size) {
        image = image.resize_exact(size, size, resize_filter);
    }
    let mut out = Vec::new();
    if image
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .is_err()
    {
        return false;
    }
    let Ok(_write) = begin_resource_write() else { return false; };
    std::fs::write(target_dir.join(format!("{icon_id}.png")), out).is_ok()
}

fn download_vault_block_icons_sync(app: AppHandle) -> Result<VaultBlockIconDownloadOutput, String> {
    ensure_resource_work_allowed()?;
    emit_vault_block_icon_progress(&app, 0, 1, 0, "正在连接到 ccvaults.com...");
    let client = vault_client()?;
    let home = crate::online_sources::send_request(client
        .get(VAULT_SITE_LABEL), None)
        .map_err(|e| format!("无法连接 Vault 首页: {e}"))?;
    if !home.status().is_success() {
        return Err(format!("无法连接 Vault 首页: HTTP {}", home.status()));
    }
    let home_html = home.text().unwrap_or_default();

    emit_vault_block_icon_progress(&app, 0, 1, 0, "正在自动获取访问令牌...");
    let token = acquire_vault_token(&client, &home_html)?;

    emit_vault_block_icon_progress(&app, 0, 1, 0, "正在获取方块图标索引...");
    let blocks_payload = vault_fetch_json_with_auth(&client, VAULT_API_BLOCKS_PATH, &token)?;
    let mut entries = collect_vault_category_entries(&blocks_payload, VAULT_BLOCK_CATEGORY_NAME);
    if entries.is_empty() {
        let all_payload = vault_fetch_json_with_auth(&client, VAULT_API_ALL_ASSETS_PATH, &token)?;
        entries = collect_vault_category_entries_from_all(&all_payload, VAULT_BLOCK_CATEGORY_NAME);
    }
    if entries.is_empty() {
        return Err("未解析到任何方块图标索引".to_string());
    }

    let target_dir = current_user_config_dir()?.join(VAULT_BLOCK_ICON_ROOT_RELPATH);
    { let _write = begin_resource_write()?; std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?; }
    let total = entries.len();
    emit_vault_block_icon_progress(&app, 0, total, 0, format!("准备并行下载 {total} 个图标..."));

    let worker_count = total.min(12).max(1);
    let mut buckets: Vec<Vec<(String, String)>> = (0..worker_count).map(|_| Vec::new()).collect();
    for (index, entry) in entries.into_iter().enumerate() {
        buckets[index % worker_count].push(entry);
    }

    let target_dir = std::sync::Arc::new(target_dir);
    let (sender, receiver) = std::sync::mpsc::channel::<(String, bool)>();
    let mut handles = Vec::new();
    for bucket in buckets {
        let sender = sender.clone();
        let client = client.clone();
        let target_dir = target_dir.clone();
        handles.push(std::thread::spawn(move || {
            for (block_id, file_url) in bucket {
                if ensure_resource_work_allowed().is_err() { break; }
                let ok = download_one_vault_icon(
                    &client,
                    &target_dir,
                    &block_id,
                    &file_url,
                    32,
                    image::imageops::FilterType::Lanczos3,
                );
                let _ = sender.send((block_id, ok));
            }
        }));
    }
    drop(sender);

    let mut completed = 0;
    let mut downloaded = 0;
    for (block_id, ok) in receiver {
        completed += 1;
        if ok {
            downloaded += 1;
        }
        emit_vault_block_icon_progress(
            &app,
            completed,
            total,
            downloaded,
            format!("正在下载图标: {block_id} ({completed}/{total})，成功 {downloaded}"),
        );
    }
    for handle in handles {
        let _ = handle.join();
    }
    if downloaded == 0 {
        return Err("未成功下载任何方块图标".to_string());
    }
    emit_vault_block_icon_progress(
        &app,
        total,
        total,
        downloaded,
        format!("下载完成：{downloaded}/{total}"),
    );

    ensure_resource_work_allowed()?;
    Ok(VaultBlockIconDownloadOutput {
        total,
        downloaded,
        target_dir: target_dir.display().to_string(),
        root_relpath: VAULT_BLOCK_ICON_ROOT_RELPATH.to_string(),
    })
}

fn download_vault_item_icons_sync(app: AppHandle) -> Result<VaultBlockIconDownloadOutput, String> {
    ensure_resource_work_allowed()?;
    emit_vault_item_icon_progress(&app, 0, 1, 0, "正在连接到 ccvaults.com...");
    let client = vault_client()?;
    let home = crate::online_sources::send_request(client
        .get(VAULT_SITE_LABEL), None)
        .map_err(|e| format!("无法连接 Vault 首页: {e}"))?;
    if !home.status().is_success() {
        return Err(format!("无法连接 Vault 首页: HTTP {}", home.status()));
    }
    let home_html = home.text().unwrap_or_default();

    emit_vault_item_icon_progress(&app, 0, 1, 0, "正在自动获取访问令牌...");
    let token = acquire_vault_token(&client, &home_html)?;

    emit_vault_item_icon_progress(&app, 0, 1, 0, "正在获取物品图标索引...");
    let items_payload = vault_fetch_json_with_auth(&client, VAULT_API_ITEMS_PATH, &token)?;
    let mut entries = collect_vault_category_entries(&items_payload, VAULT_ITEM_CATEGORY_NAME);
    if entries.is_empty() {
        let all_payload = vault_fetch_json_with_auth(&client, VAULT_API_ALL_ASSETS_PATH, &token)?;
        entries = collect_vault_category_entries_from_all(&all_payload, VAULT_ITEM_CATEGORY_NAME);
    }
    if entries.is_empty() {
        return Err("未解析到任何物品图标索引".to_string());
    }

    let target_dir = current_user_config_dir()?.join(VAULT_ITEM_ICON_ROOT_RELPATH);
    { let _write = begin_resource_write()?; std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?; }
    let total = entries.len();
    emit_vault_item_icon_progress(&app, 0, total, 0, format!("准备下载 {total} 个图标..."));

    let mut downloaded = 0;
    for (index, (item_id, file_url)) in entries.into_iter().enumerate() {
        ensure_resource_work_allowed()?;
        // Item icons keep their native size; no resampling is applied.
        if download_one_vault_icon(
            &client,
            &target_dir,
            &item_id,
            &file_url,
            16,
            image::imageops::FilterType::Nearest,
        ) {
            downloaded += 1;
        }
        emit_vault_item_icon_progress(
            &app,
            index + 1,
            total,
            downloaded,
            format!("正在下载图标: {item_id}"),
        );
    }
    if downloaded == 0 {
        return Err("未成功下载任何物品图标".to_string());
    }
    emit_vault_item_icon_progress(
        &app,
        total,
        total,
        downloaded,
        format!("下载完成：{downloaded}/{total}"),
    );

    ensure_resource_work_allowed()?;
    Ok(VaultBlockIconDownloadOutput {
        total,
        downloaded,
        target_dir: target_dir.display().to_string(),
        root_relpath: VAULT_ITEM_ICON_ROOT_RELPATH.to_string(),
    })
}

fn emit_vault_block_icon_progress(
    app: &AppHandle,
    current: usize,
    total: usize,
    downloaded: usize,
    status: impl Into<String>,
) {
    emit_vault_icon_progress(app, "vault-block-icons-download-progress", current, total, downloaded, status);
}

fn emit_vault_item_icon_progress(
    app: &AppHandle,
    current: usize,
    total: usize,
    downloaded: usize,
    status: impl Into<String>,
) {
    emit_vault_icon_progress(app, "vault-item-icons-download-progress", current, total, downloaded, status);
}

/// Emits one icon-download progress tick on the event name its window listens to.
fn emit_vault_icon_progress(
    app: &AppHandle,
    event: &str,
    current: usize,
    total: usize,
    downloaded: usize,
    status: impl Into<String>,
) {
    use tauri::Emitter;
    let _ = app.emit(
        event,
        VaultBlockIconProgress {
            current,
            total,
            downloaded,
            status: status.into(),
        },
    );
}

#[tauri::command]
pub(crate) async fn download_vault_block_icons(
    app: AppHandle,
) -> Result<VaultBlockIconDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(move || download_vault_block_icons_sync(app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn download_vault_item_icons(
    app: AppHandle,
) -> Result<VaultBlockIconDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(move || download_vault_item_icons_sync(app))
        .await
        .map_err(|e| e.to_string())?
}

/// Pulls every `resource location` cell value out of the HTML tables.
fn extract_resource_locations_from_html(html: &str) -> Result<Vec<String>, String> {
    let table_selector = scraper::Selector::parse("table").map_err(|e| e.to_string())?;
    let row_selector = scraper::Selector::parse("tr").map_err(|e| e.to_string())?;
    let cell_selector = scraper::Selector::parse("th, td").map_err(|e| e.to_string())?;
    let code_selector = scraper::Selector::parse("code").map_err(|e| e.to_string())?;
    let document = scraper::Html::parse_document(html);
    let mut seen = std::collections::HashSet::new();
    let mut output = Vec::new();

    for table in document.select(&table_selector) {
        let rows: Vec<_> = table.select(&row_selector).collect();
        let mut resource_column = None;
        let mut data_start = 0usize;
        for (row_index, row) in rows.iter().enumerate() {
            let cells: Vec<_> = row.select(&cell_selector).collect();
            let labels: Vec<_> = cells
                .iter()
                .map(|cell| element_text(cell).to_ascii_lowercase())
                .collect();
            if let Some(column_index) = labels
                .iter()
                .position(|label| label.contains("resource location"))
            {
                resource_column = Some(column_index);
                data_start = row_index + 1;
                break;
            }
        }
        let Some(resource_column) = resource_column else {
            continue;
        };
        for row in rows.iter().skip(data_start) {
            let cells: Vec<_> = row.select(&cell_selector).collect();
            let Some(cell) = cells.get(resource_column) else {
                continue;
            };
            for value in extract_code_values_from_cell(cell, &code_selector) {
                if seen.insert(value.clone()) {
                    output.push(value);
                }
            }
        }
    }

    Ok(output)
}

/// Cuts the enchantment section out of the aggregated data-values page.
fn extract_enchantment_section_html(html: &str) -> Option<&str> {
    let lower = html.to_ascii_lowercase();
    let markers = [
        "id=\"enchantments\"",
        "id='enchantments'",
        "id=\"enchantment\"",
        "id='enchantment'",
    ];
    let start_hint = markers.iter().filter_map(|marker| lower.find(marker)).max()?;
    let start = lower[..start_hint]
        .rfind("<div class=\"mw-heading")
        .or_else(|| lower[..start_hint].rfind("<h"))
        .unwrap_or(start_hint);
    let search_start = (start_hint + 1).min(html.len());
    let end = lower[search_start..]
        .find("<div class=\"mw-heading")
        .map(|offset| search_start + offset)
        .unwrap_or(html.len());
    Some(&html[start..end])
}

fn write_json_catalog(path: &Path, values: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(values).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())
}

fn download_minecraft_wiki_enum_catalogs_sync() -> Result<WikiEnumCatalogDownloadOutput, String> {
    let client = wiki_client()?;
    let blocks_html = fetch_wiki_html(&client, WIKI_ENUM_BLOCKS_URL)?;
    let items_html = fetch_wiki_html(&client, WIKI_ENUM_ITEMS_URL)?;
    let main_html = fetch_wiki_html(&client, WIKI_ENUM_MAIN_URL)?;
    let entities_html = fetch_wiki_html(&client, WIKI_ENUM_ENTITIES_URL)?;

    let blocks = extract_resource_locations_from_html(&blocks_html)?;
    let items = extract_resource_locations_from_html(&items_html)?;
    let enchantment_section = extract_enchantment_section_html(&main_html)
        .ok_or_else(|| "无法定位 Minecraft Wiki 魔咒数据值分节".to_string())?;
    let enchantments = extract_resource_locations_from_html(enchantment_section)?;
    let entities = extract_resource_locations_from_html(&entities_html)?;

    if blocks.is_empty() || items.is_empty() || enchantments.is_empty() || entities.is_empty() {
        return Err("Minecraft Wiki 枚举全集解析结果为空。".to_string());
    }

    let target_dir = current_user_config_dir()?.join(WIKI_ENUM_ROOT_RELPATH);
    let _write = begin_resource_write()?;
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    write_json_catalog(&target_dir.join("DV_Blocks.json"), &blocks)?;
    write_json_catalog(&target_dir.join("DV_Items.json"), &items)?;
    write_json_catalog(&target_dir.join("DV_Enchantments.json"), &enchantments)?;
    write_json_catalog(&target_dir.join("DV_Entities.json"), &entities)?;
    let manifest = serde_json::json!({
        "source": "minecraft_wiki",
        "root_relpath": WIKI_ENUM_ROOT_RELPATH,
        "catalogs": {
            "blocks": { "file": "DV_Blocks.json", "url": WIKI_ENUM_BLOCKS_URL, "count": blocks.len() },
            "items": { "file": "DV_Items.json", "url": WIKI_ENUM_ITEMS_URL, "count": items.len() },
            "enchantments": { "file": "DV_Enchantments.json", "url": WIKI_ENUM_MAIN_URL, "count": enchantments.len() },
            "entities": { "file": "DV_Entities.json", "url": WIKI_ENUM_ENTITIES_URL, "count": entities.len() }
        }
    });
    std::fs::write(
        target_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    Ok(WikiEnumCatalogDownloadOutput {
        target_dir: target_dir.display().to_string(),
        root_relpath: WIKI_ENUM_ROOT_RELPATH.to_string(),
        blocks: blocks.len(),
        items: items.len(),
        enchantments: enchantments.len(),
        entities: entities.len(),
    })
}

#[tauri::command]
pub(crate) async fn download_minecraft_wiki_enum_catalogs(
) -> Result<WikiEnumCatalogDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(download_minecraft_wiki_enum_catalogs_sync)
        .await
        .map_err(|e| e.to_string())?
}

/// The original GitHub language browser fetch, routed through Host cancellation and site lanes.
#[tauri::command]
pub(crate) async fn read_game_resource_remote_text(url: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure_resource_work_allowed()?;
        let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
        let allowed = parsed.scheme() == "https" && match parsed.host_str() {
            Some("api.github.com") => parsed.path().starts_with("/repos/InventivetalentDev/minecraft-assets/"),
            Some("raw.githubusercontent.com") => parsed.path().starts_with("/InventivetalentDev/minecraft-assets/"),
            Some("github.com") => parsed.path().starts_with("/InventivetalentDev/minecraft-assets/"),
            _ => false,
        };
        if !allowed { return Err("语言资源来源地址不匹配。".to_string()); }
        let client = wiki_client()?;
        let response = crate::online_sources::send_request(client.get(parsed).header(reqwest::header::ACCEPT, "application/json,text/html,*/*"), None)?;
        if !response.status().is_success() { return Err(format!("语言资源请求失败: HTTP {}", response.status())); }
        let text = response.text()?;
        ensure_resource_work_allowed()?;
        Ok(text)
    }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod resource_restoration_tests {
    use super::*;

    #[test]
    fn wiki_enum_parser_reads_only_resource_column_and_deduplicates() {
        let html = r#"<table><tr><th>Name</th><th>Resource location</th></tr>
          <tr><td><code>wrong_column</code></td><td><code>stone</code></td></tr>
          <tr><td>Dirt</td><td><code>minecraft:dirt</code></td></tr>
          <tr><td>Stone duplicate</td><td><code>stone</code></td></tr></table>"#;
        assert_eq!(extract_resource_locations_from_html(html).unwrap(), vec!["stone", "minecraft:dirt"]);
    }

    #[test]
    fn wiki_enchantment_section_does_not_capture_neighbor_catalog() {
        let html = r#"<div class="mw-heading"><h2 id="Enchantments">Enchantments</h2></div>
          <table><tr><th>Resource location</th></tr><tr><td><code>sharpness</code></td></tr></table>
          <div class="mw-heading"><h2 id="Entities">Entities</h2></div>
          <table><tr><th>Resource location</th></tr><tr><td><code>cow</code></td></tr></table>"#;
        let section = extract_enchantment_section_html(html).unwrap();
        assert_eq!(extract_resource_locations_from_html(section).unwrap(), vec!["sharpness"]);
    }

    #[test]
    #[cfg(windows)]
    fn resource_write_stage_drains_without_losing_accepted_catalog_files() {
        use crate::platform::process_supervisor::{ExecutableAllowlist, ExecutableRole, ProcessSupervisor};
        use std::sync::Arc;
        let command = PathBuf::from(std::env::var_os("COMSPEC").unwrap());
        let mut allowed = ExecutableAllowlist::new();
        allowed.allow(ExecutableRole::CoreHelper, &command).unwrap();
        let (supervisor, _) = ProcessSupervisor::new(allowed).unwrap();
        let root = std::env::temp_dir().join(format!("nova-resource-write-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let operations = ApplicationOperations::new(Arc::new(supervisor), root.clone(), root.clone(), command.parent().unwrap().to_path_buf()).unwrap();
        bind_operations(operations.clone());
        assert!(operations.wait_for_writes(Duration::ZERO), "network/read preparation holds no write stage");
        let write = begin_resource_write().unwrap();
        operations.begin_shutdown();
        assert!(begin_resource_write().is_err());
        assert!(!operations.wait_for_writes(Duration::ZERO));
        let destination = root.join("retained-catalog.json");
        let saved = destination.clone();
        let writer = std::thread::spawn(move || {
            // The accepted persist phase finishes, even after new work is denied.
            write_json_catalog(&saved, &["minecraft:stone".to_string(), "minecraft:dirt".to_string()]).unwrap();
            std::thread::sleep(Duration::from_millis(25));
            drop(write);
        });
        assert!(operations.wait_for_writes(Duration::from_secs(1)));
        writer.join().unwrap();
        assert_eq!(serde_json::from_str::<Vec<String>>(&std::fs::read_to_string(&destination).unwrap()).unwrap(), vec!["minecraft:stone", "minecraft:dirt"]);
        operations.cancel_shutdown();
        let resumed = begin_resource_write().unwrap(); drop(resumed);
        assert!(operations.wait_for_writes(Duration::ZERO));
        let absolute = std::fs::canonicalize(&root).unwrap();
        assert!(absolute.starts_with(std::fs::canonicalize(std::env::temp_dir()).unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("nova-resource-write-"));
        std::fs::remove_dir_all(absolute).unwrap();
    }

    #[test]
    fn resource_token_admission_and_cancel_do_not_send_network() {
        let client = reqwest::blocking::Client::new();
        for url in ["https://example.invalid/api/token", "http://ccvaults.com/api/token", "https://ccvaults.com/api/other", "https://ccvaults.com/api/token?extra=1"] {
            let error = crate::online_sources::send_resource_request(client.post(url), None).err().unwrap();
            assert!(error.contains("资源请求仅接受"));
        }
        let id = format!("resource-token-cancel-test-{}", std::process::id());
        crate::online_sources::application_online_cancel(id.clone()).unwrap();
        let error = crate::online_sources::send_resource_request(client.post("https://ccvaults.com/api/token").body("{}"), Some(&id)).err().unwrap();
        assert!(error.contains("取消"));
    }
}
