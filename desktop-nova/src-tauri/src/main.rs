#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine;
use flate2::read::GzDecoder;
use image::{DynamicImage, GenericImageView, RgbaImage};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::env;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State, Window};
use tauri_plugin_dialog::DialogExt;
use zip::ZipArchive;

#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HMENU,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
};

struct BuildState {
    child: Option<Child>,
    progress_file: Option<PathBuf>,
    cache_file: Option<PathBuf>,
    stdout_file: Option<PathBuf>,
    stderr_file: Option<PathBuf>,
    started_at: Option<Instant>,
    last_progress_signature: Option<String>,
    last_progress_change: Option<Instant>,
}

#[derive(Default)]
struct EmbeddedViewerState {
    viewer: Option<EmbeddedViewerRecord>,
}

struct EmbeddedViewerRecord {
    child: Child,
    child_hwnd: isize,
    parent_hwnd: isize,
    purpose: String,
    file_path: String,
    display_mode: String,
    cache_input: Option<String>,
    bounds: EmbeddedRect,
    visible: bool,
    stdout_file: PathBuf,
    stderr_file: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct EmbeddedRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Serialize)]
struct EmbeddedViewerStatus {
    supported: bool,
    running: bool,
    visible: bool,
    parent_hwnd: Option<String>,
    child_hwnd: Option<String>,
    file: Option<String>,
    mode: Option<String>,
    rect_physical: Option<EmbeddedRect>,
    process_id: Option<u32>,
    purpose: Option<String>,
    status: String,
    error: Option<String>,
    stdout_tail: String,
    stderr_tail: String,
}

#[derive(Serialize)]
struct PathInfo {
    raw: String,
    normalized: String,
    parent_dir: String,
    parent_exists: bool,
    exists: bool,
    is_dir: bool,
    is_file: bool,
    is_absolute: bool,
    has_litematic_ext: bool,
}

#[derive(Serialize)]
struct DirectoryEntryInfo {
    path: String,
    name: String,
    is_dir: bool,
    is_file: bool,
    file_size: u64,
    mtime_ms: u128,
    extension: String,
}

#[derive(Serialize)]
struct BackendTrace {
    actual_core_exe_path: String,
    actual_core_exe_exists: bool,
    actual_core_exe_modified_time: Option<String>,
    actual_core_exe_file_size: Option<u64>,
    actual_core_exe_sha256: Option<String>,
    command_args: Vec<String>,
    backend_stdout: String,
    backend_stderr: String,
    backend_exit_code: Option<i32>,
}

#[derive(Serialize)]
struct CacheBuildLaunch {
    file_path: String,
    progress_file: String,
    cache_file: String,
    stdout_file: String,
    stderr_file: String,
}

#[derive(Serialize)]
struct CacheBuildSnapshot {
    running: bool,
    exit_code: Option<i32>,
    progress_file: Option<String>,
    cache_file: Option<String>,
    stdout_file: Option<String>,
    stderr_file: Option<String>,
    progress_json: Option<String>,
    stdout_tail: String,
    stderr_tail: String,
    cache_exists: bool,
}

#[derive(Serialize)]
struct RenderPreviewOutput {
    preview_path: String,
    data_url: String,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
}

#[derive(Serialize)]
struct ProjectionPreviewOutput {
    width: u32,
    height: u32,
    data_url: String,
}

#[derive(Serialize)]
struct CopyFileToDirectoryOutput {
    target_path: String,
    overwritten: bool,
    bytes_copied: u64,
}

#[derive(Deserialize, Serialize, Clone)]
struct UserConfig {
    theme: String,
    render_display_mode: String,
    preview_mode: String,
    #[serde(default = "default_material_list_window_behavior")]
    material_list_window_behavior: String,
    #[serde(default = "default_show_ui_test_page")]
    show_ui_test_page: bool,
    #[serde(default = "default_local_library_tail_path_count")]
    local_library_tail_path_count: u32,
}

#[derive(Deserialize)]
struct UserConfigInput {
    theme: Option<String>,
    render_display_mode: Option<String>,
    preview_mode: Option<String>,
    material_list_window_behavior: Option<String>,
    show_ui_test_page: Option<bool>,
    local_library_tail_path_count: Option<u32>,
}

#[derive(Serialize)]
struct UserConfigInfo {
    config_dir: String,
    default_config_dir: String,
    config: UserConfig,
}

#[derive(Deserialize, Serialize, Clone)]
struct AiStoredConfig {
    provider: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Deserialize)]
struct AiSaveConfigInput {
    provider: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Serialize)]
struct AiPublicConfig {
    provider: String,
    base_url: String,
    model: String,
    has_key: bool,
    key_status: String,
    storage_note: String,
}

#[derive(Serialize)]
struct AiTestResult {
    ok: bool,
    message: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct AiChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AiChatCompletionInput {
    messages: Vec<AiChatMessage>,
}

#[derive(Serialize)]
struct AiChatCompletionOutput {
    provider: String,
    model: String,
    content: String,
}

fn get_root() -> PathBuf {
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    find_workspace_root(&current_dir)
        .or_else(|| find_workspace_root(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .unwrap_or(current_dir)
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join("bin").join("viewer-backend").is_dir() {
            return Some(candidate.to_path_buf());
        }
        if candidate.ends_with("src-tauri") {
            let maybe_root = candidate.parent()?.parent()?;
            if maybe_root.join("bin").join("viewer-backend").is_dir() {
                return Some(maybe_root.to_path_buf());
            }
        }
        if candidate.ends_with("desktop-nova") {
            let maybe_root = candidate.parent()?;
            if maybe_root.join("bin").join("viewer-backend").is_dir() {
                return Some(maybe_root.to_path_buf());
            }
        }
    }
    None
}

fn backend_exe_path(binary_name: &str) -> PathBuf {
    get_root()
        .join("bin")
        .join("viewer-backend")
        .join(binary_name)
}

fn data_root() -> Result<PathBuf, String> {
    let dir = env::var_os("LBA_DATA_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                get_root().join(path)
            }
        })
        .unwrap_or_else(|| get_root().join("data"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn legacy_appdata_config_dir() -> Option<PathBuf> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|base| base.join("Litematica-BA").join("desktop-nova"))
}

fn render_tmp_path(prefix: &str, ext: &str) -> Result<PathBuf, String> {
    let dir = data_root()?.join("cache").join("render");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    Ok(dir.join(format!("{prefix}_{now}_{:x}.{ext}", std::process::id())))
}

fn run_command_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Output, String> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return child.wait_with_output().map_err(|e| e.to_string());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Preview generation timed out after {} seconds.\nStdout: {}\nStderr: {}",
                timeout.as_secs(),
                stdout,
                stderr
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn app_config_dir() -> Result<PathBuf, String> {
    let dir = data_root()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    migrate_legacy_appdata_if_needed(&dir)?;
    Ok(dir)
}

fn copy_file_if_missing(source: &Path, target: &Path) -> Result<(), String> {
    if !source.is_file() || target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(source, target)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn copy_dir_files_if_missing(source: &Path, target: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            copy_dir_files_if_missing(&source_path, &target_path)?;
        } else if metadata.is_file() {
            copy_file_if_missing(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn migrate_legacy_appdata_if_needed(target_root: &Path) -> Result<(), String> {
    let Some(legacy_root) = legacy_appdata_config_dir() else {
        return Ok(());
    };
    if legacy_root == target_root || !legacy_root.is_dir() {
        return Ok(());
    }
    for file in ["config.json", "config_dir.json", "ai_config.json"] {
        copy_file_if_missing(&legacy_root.join(file), &target_root.join(file))?;
    }
    for dir in ["projection-library", "generation-templates/custom"] {
        copy_dir_files_if_missing(&legacy_root.join(dir), &target_root.join(dir))?;
    }
    Ok(())
}

fn ai_config_path() -> Result<PathBuf, String> {
    let path = app_config_dir()?.join("ai_config.json");
    if !path.exists() {
        if let Some(base) = env::var_os("APPDATA") {
            let old_path = PathBuf::from(base)
                .join("Litematica-BA")
                .join("ai_config.json");
            if old_path.is_file() {
                let _ = std::fs::copy(old_path, &path);
            }
        }
    }
    Ok(path)
}

fn default_user_config_dir() -> Result<PathBuf, String> {
    app_config_dir()
}

fn config_dir_pointer_path() -> Result<PathBuf, String> {
    Ok(default_user_config_dir()?.join("config_dir.json"))
}

fn read_selected_config_dir() -> Option<PathBuf> {
    let pointer = config_dir_pointer_path().ok()?;
    let text = std::fs::read_to_string(pointer).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("config_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
}

fn current_user_config_dir() -> Result<PathBuf, String> {
    let dir = read_selected_config_dir().unwrap_or(default_user_config_dir()?);
    ensure_user_config_layout(&dir)?;
    Ok(dir)
}

fn ensure_user_config_layout(dir: &Path) -> Result<(), String> {
    for relative in [
        "",
        "projection-library",
        "projection-library/previews",
        "cache/render",
        "tmp/render",
        "reden/downloads",
        "exports",
        "generation-templates",
        "generation-templates/custom",
    ] {
        std::fs::create_dir_all(dir.join(relative)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn default_user_config() -> UserConfig {
    UserConfig {
        theme: "WebDefault".to_string(),
        render_display_mode: "normal".to_string(),
        preview_mode: "normal".to_string(),
        material_list_window_behavior: default_material_list_window_behavior(),
        show_ui_test_page: default_show_ui_test_page(),
        local_library_tail_path_count: default_local_library_tail_path_count(),
    }
}

fn default_material_list_window_behavior() -> String {
    "independent_window".to_string()
}

fn default_show_ui_test_page() -> bool {
    true
}

fn default_local_library_tail_path_count() -> u32 {
    3
}

fn normalize_material_list_window_behavior(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "main_window_overlay" => "main_window_overlay".to_string(),
        _ => "independent_window".to_string(),
    }
}

fn normalize_local_library_tail_path_count(value: u32) -> u32 {
    if value >= 1 {
        value
    } else {
        default_local_library_tail_path_count()
    }
}

fn normalize_theme(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "bootstrap5" => "Bootstrap5".to_string(),
        "metro10" => "Metro10".to_string(),
        "minecraft" => "Minecraft".to_string(),
        _ => "WebDefault".to_string(),
    }
}

fn normalize_mode_string(value: &str) -> String {
    normalized_display_mode(value).to_string()
}

fn user_config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

fn read_user_config_data(dir: &Path) -> UserConfig {
    let path = user_config_path(dir);
    let mut config = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<UserConfig>(&text).ok())
        .unwrap_or_else(default_user_config);
    config.theme = normalize_theme(&config.theme);
    config.render_display_mode = normalize_mode_string(&config.render_display_mode);
    config.preview_mode = normalize_mode_string(&config.preview_mode);
    config.material_list_window_behavior =
        normalize_material_list_window_behavior(&config.material_list_window_behavior);
    config.local_library_tail_path_count =
        normalize_local_library_tail_path_count(config.local_library_tail_path_count);
    config
}

fn write_user_config_data(dir: &Path, config: &UserConfig) -> Result<(), String> {
    ensure_user_config_layout(dir)?;
    std::fs::write(
        user_config_path(dir),
        serde_json::to_string_pretty(config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn migrate_legacy_library_if_needed(dir: &Path) -> Result<(), String> {
    let target = dir.join("projection-library").join("js_library.json");
    if target.exists() {
        return Ok(());
    }
    let legacy = get_root()
        .join("data")
        .join("projection-library")
        .join("js_library.json");
    if legacy.is_file() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(legacy, target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn safe_user_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Invalid user config relative path.".to_string());
    }
    Ok(path)
}

fn user_runtime_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let path = safe_user_relative_path(relative_path)?;
    let mut components = path.components();
    let Some(first) = components.next() else {
        return Ok(path);
    };
    let first = first.as_os_str().to_string_lossy();
    if first == "render" {
        return Ok(PathBuf::from("tmp").join(path));
    }
    if first == "previews" {
        return Ok(PathBuf::from("projection-library").join(path));
    }
    Ok(path)
}

fn default_ai_config() -> AiStoredConfig {
    AiStoredConfig {
        provider: "mock".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        model: "gpt-4.1-mini".to_string(),
        api_key: None,
    }
}

fn read_ai_config() -> AiStoredConfig {
    let Ok(path) = ai_config_path() else {
        return default_ai_config();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return default_ai_config();
    };
    serde_json::from_str(&text).unwrap_or_else(|_| default_ai_config())
}

fn write_ai_config(config: &AiStoredConfig) -> Result<(), String> {
    let path = ai_config_path()?;
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn public_ai_config(config: AiStoredConfig, key_status: &str) -> AiPublicConfig {
    AiPublicConfig {
        provider: config.provider,
        base_url: config.base_url,
        model: config.model,
        has_key: config.api_key.as_ref().is_some_and(|key| !key.is_empty()),
        key_status: key_status.to_string(),
        storage_note:
            "API Key is stored by the Tauri backend in local app config, not localStorage."
                .to_string(),
    }
}

fn tail_text(path: &Path, max_chars: usize) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        text
    } else {
        chars[chars.len().saturating_sub(max_chars)..]
            .iter()
            .collect()
    }
}

fn apply_build_mode_env(cmd: &mut Command, build_mode: &str) {
    let mode = match build_mode {
        "fast_experimental" => "fast_experimental",
        "full" => "full",
        _ => "normal",
    };
    cmd.env("LBA_VIEWER_BUILD_MODE", mode);
    for key in [
        "LBA_ENABLE_COMPACT_CACHE_V2",
        "LBA_ENABLE_FAST_PATH_NON_OCCLUDING",
        "LBA_ENABLE_FAST_PATH_HALF_SLAB",
        "LBA_ENABLE_FAST_PATH_CARPET",
        "LBA_ENABLE_FAST_PATH_STAIR_HALF",
    ] {
        cmd.env_remove(key);
    }
    if mode == "fast_experimental" {
        cmd.env("LBA_ENABLE_COMPACT_CACHE_V2", "1");
        cmd.env("LBA_ENABLE_FAST_PATH_NON_OCCLUDING", "1");
        cmd.env("LBA_ENABLE_FAST_PATH_HALF_SLAB", "1");
        cmd.env("LBA_ENABLE_FAST_PATH_CARPET", "1");
        cmd.env("LBA_ENABLE_FAST_PATH_STAIR_HALF", "1");
    }
}

fn normalized_display_mode(display_mode: &str) -> &'static str {
    match display_mode {
        "fast_experimental" => "fast_experimental",
        "full" => "full",
        _ => "normal",
    }
}

fn normalized_embed_purpose(purpose: &str) -> &'static str {
    match purpose {
        "properties_preview" => "properties_preview",
        _ => "render_interactive",
    }
}

fn normalized_input_path(file_path: &str) -> PathBuf {
    let input = PathBuf::from(file_path);
    if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    }
}

fn stop_embedded_record(record: &mut EmbeddedViewerRecord) {
    let _ = record.child.kill();
    let _ = record.child.wait();
    #[cfg(windows)]
    unsafe {
        if record.child_hwnd != 0 {
            let _ = DestroyWindow(record.child_hwnd as HWND);
        }
    }
}

fn is_valid_embed_rect(rect: EmbeddedRect) -> bool {
    rect.width > 8 && rect.height > 8
}

fn log_embed(message: &str) {
    println!("[LBA_EMBED_VIEWER] {message}");
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
unsafe extern "system" fn embedded_host_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[cfg(windows)]
fn ensure_embed_window_class() -> Result<Vec<u16>, String> {
    let class_name = wide_null("LBAEmbeddedViewerHost");
    unsafe {
        let instance = GetModuleHandleW(null_mut());
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(embedded_host_wnd_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..std::mem::zeroed()
        };
        let atom = RegisterClassW(&wc);
        if atom == 0 {
            // RegisterClassW returns 0 if already registered too; CreateWindowExW below is the real check.
        }
    }
    Ok(class_name)
}

#[cfg(windows)]
fn create_embedded_host(parent_hwnd: isize, rect: EmbeddedRect) -> Result<isize, String> {
    let class_name = ensure_embed_window_class()?;
    let title = wide_null("LBA Embedded Viewer Host");
    let width = rect.width.max(1);
    let height = rect.height.max(1);
    unsafe {
        let hwnd = CreateWindowExW(
            0 as WINDOW_EX_STYLE,
            class_name.as_ptr(),
            title.as_ptr(),
            (WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS) as WINDOW_STYLE,
            rect.x,
            rect.y,
            width,
            height,
            parent_hwnd as HWND,
            0 as HMENU,
            GetModuleHandleW(null_mut()),
            null_mut(),
        );
        if hwnd.is_null() {
            return Err("CreateWindowExW failed for embedded viewer host".to_string());
        }
        Ok(hwnd as isize)
    }
}

#[cfg(windows)]
fn move_embedded_host(hwnd: isize, rect: EmbeddedRect) -> Result<(), String> {
    if !is_valid_embed_rect(rect) {
        return Err(format!("skip zero/small embedded bounds: {:?}", rect));
    }
    unsafe {
        let _ = ShowWindow(hwnd as HWND, SW_SHOW);
        let ok = SetWindowPos(
            hwnd as HWND,
            null_mut(),
            rect.x,
            rect.y,
            rect.width.max(1),
            rect.height.max(1),
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        if ok == 0 {
            return Err(format!(
                "SetWindowPos failed child_hwnd={} rect={:?}",
                hwnd, rect
            ));
        }
    }
    Ok(())
}

fn embedded_process_alive(record: &mut EmbeddedViewerRecord) -> bool {
    matches!(record.child.try_wait(), Ok(None))
}

fn log_embedded_output_tail(record: &EmbeddedViewerRecord, status: &str) {
    let stdout_tail = tail_text(&record.stdout_file, 4000);
    let stderr_tail = tail_text(&record.stderr_file, 4000);
    if !stdout_tail.trim().is_empty() {
        log_embed(&format!(
            "status={} viewer_pid={} stdout_tail_begin\n{}\n[LBA_EMBED_VIEWER] stdout_tail_end",
            status,
            record.child.id(),
            stdout_tail.trim_end()
        ));
    }
    if !stderr_tail.trim().is_empty() {
        log_embed(&format!(
            "status={} viewer_pid={} stderr_tail_begin\n{}\n[LBA_EMBED_VIEWER] stderr_tail_end",
            status,
            record.child.id(),
            stderr_tail.trim_end()
        ));
    }
}

fn wait_for_embedded_viewer_start(record: &mut EmbeddedViewerRecord) -> Result<(), String> {
    let started_at = std::time::Instant::now();
    loop {
        if let Some(status) = record.child.try_wait().map_err(|e| e.to_string())? {
            let stdout_tail = tail_text(&record.stdout_file, 8000);
            let stderr_tail = tail_text(&record.stderr_file, 8000);
            return Err(format!(
                "embedded native viewer exited before window attach: status={status:?}\nstdout:\n{stdout_tail}\nstderr:\n{stderr_tail}"
            ));
        }
        let stdout_tail = tail_text(&record.stdout_file, 8000);
        if stdout_tail.contains("launch_mode=embedded") {
            log_embedded_output_tail(record, "native_embedded_ready");
            return Ok(());
        }
        if started_at.elapsed() > std::time::Duration::from_secs(8) {
            log_embedded_output_tail(record, "native_embedded_start_timeout");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(windows)]
fn show_embedded_host(hwnd: isize, show: bool) {
    unsafe {
        let _ = ShowWindow(hwnd as HWND, if show { SW_SHOW } else { SW_HIDE });
        if show {
            let _ = SetWindowPos(
                hwnd as HWND,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | 0x0001 | 0x0002,
            );
        }
    }
}

#[cfg(windows)]
fn tauri_parent_hwnd(window: &Window) -> Result<isize, String> {
    window
        .hwnd()
        .map(|hwnd| hwnd.0 as isize)
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn set_host_user_data(hwnd: isize, value: isize) {
    unsafe {
        let _ = GetWindowLongPtrW(hwnd as HWND, GWLP_USERDATA);
        let _ = SetWindowLongPtrW(hwnd as HWND, GWLP_USERDATA, value);
    }
}

fn file_modified_time(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_secs().to_string())
}

fn file_sha256(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let digest = Sha256::digest(bytes);
    Some(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn run_backend(binary_name: &str, args: &[String]) -> Result<BackendTrace, String> {
    let current_dir = get_root();
    let exe_path = backend_exe_path(binary_name);
    let metadata = std::fs::metadata(&exe_path).ok();
    let output = Command::new(&exe_path)
        .args(args)
        .current_dir(&current_dir)
        .output()
        .map_err(|e| e.to_string())?;

    Ok(BackendTrace {
        actual_core_exe_path: exe_path.display().to_string(),
        actual_core_exe_exists: exe_path.is_file(),
        actual_core_exe_modified_time: file_modified_time(&exe_path),
        actual_core_exe_file_size: metadata.as_ref().map(|m| m.len()),
        actual_core_exe_sha256: file_sha256(&exe_path),
        command_args: args.to_vec(),
        backend_stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        backend_stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        backend_exit_code: output.status.code(),
    })
}

#[tauri::command]
async fn execute_backend(binary_name: String, args: Vec<String>) -> Result<String, String> {
    let trace = run_backend(&binary_name, &args)?;
    if trace.backend_exit_code == Some(0) {
        Ok(trace.backend_stdout)
    } else {
        Err(format!(
            "Exit code: {:?}\nStdout: {}\nStderr: {}",
            trace.backend_exit_code, trace.backend_stdout, trace.backend_stderr
        ))
    }
}

#[tauri::command]
async fn execute_backend_trace(
    binary_name: String,
    args: Vec<String>,
) -> Result<BackendTrace, String> {
    run_backend(&binary_name, &args)
}

fn render_cache_kind(path: &Path) -> String {
    if !path.is_file() {
        return "missing".to_string();
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
        return "not_json".to_string();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return "unreadable_json".to_string();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return "invalid_json".to_string();
    };
    if value.get("chunk_data_dir").is_some() && value.get("chunks").is_some() {
        let format = value
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let color_chain = value
            .get("color_chain")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        return format!("native_3d_cache format={format} color_chain={color_chain}");
    }
    if value.get("ready").is_some() && value.get("phase").is_some() {
        return "progress_json".to_string();
    }
    if value.get("layers").is_some() || value.get("layer_count").is_some() {
        return "layer_meta_json".to_string();
    }
    "json_unknown".to_string()
}

fn log_render_cache_input(target: &str, display_mode: &str, cache_path: &Path) {
    let exists = cache_path.is_file();
    let size = std::fs::metadata(cache_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    println!(
        "[LBA_RENDER_CACHE] target={} display_mode={} cacheFile={} cacheKind=\"{}\" exists={} size={}",
        target,
        display_mode,
        cache_path.display(),
        render_cache_kind(cache_path),
        exists,
        size
    );
}

#[tauri::command]
fn start_native_viewer(
    file_path: String,
    display_mode: String,
    cache_input: Option<String>,
) -> Result<(), String> {
    let current_dir = get_root();
    let exe_path = backend_exe_path("litematica_native_viewer.exe");
    let mode = normalized_display_mode(&display_mode);
    let resolved_input = normalized_input_path(&file_path);
    let resolved_cache_input = cache_input
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(normalized_input_path);
    let stdout_file = render_tmp_path("lba_popup_viewer_stdout", "log")?;
    let stderr_file = render_tmp_path("lba_popup_viewer_stderr", "log")?;
    let stdout = std::fs::File::create(&stdout_file).map_err(|e| e.to_string())?;
    let stderr = std::fs::File::create(&stderr_file).map_err(|e| e.to_string())?;

    let mut command = Command::new(exe_path);
    command
        .arg(resolved_input.display().to_string())
        .arg(format!("--display-mode={mode}"))
        .arg("--basic-lighting")
        .arg("--basic-shadows");
    if let Some(cache_path) = resolved_cache_input.as_ref() {
        log_render_cache_input("popup", &mode, cache_path);
        command.arg(format!("--cache-input={}", cache_path.display()));
    }
    let mut child = command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .current_dir(&current_dir)
        .spawn()
        .map_err(|e| e.to_string())?;

    std::thread::sleep(std::time::Duration::from_millis(700));
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
        let stdout_tail = tail_text(&stdout_file, 8000);
        let stderr_tail = tail_text(&stderr_file, 8000);
        return Err(format!(
            "native viewer exited immediately: status={status:?}\nstdout:\n{stdout_tail}\nstderr:\n{stderr_tail}"
        ));
    }
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
fn start_embedded_viewer(
    window: Window,
    state: State<'_, Mutex<EmbeddedViewerState>>,
    file_path: String,
    display_mode: String,
    rect: EmbeddedRect,
    purpose: String,
    cache_input: Option<String>,
) -> Result<EmbeddedViewerStatus, String> {
    let current_dir = get_root();
    let exe_path = backend_exe_path("litematica_native_viewer.exe");
    let mode = normalized_display_mode(&display_mode);
    let purpose = normalized_embed_purpose(&purpose);
    let preview_mode = purpose == "properties_preview";
    let preview_spin = purpose == "properties_preview";
    let interactive = purpose == "render_interactive";
    let resolved_input = normalized_input_path(&file_path);
    let resolved_cache_input = cache_input
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(normalized_input_path);
    let resolved_cache_input_string = resolved_cache_input
        .as_ref()
        .map(|path| path.display().to_string());
    let parent_hwnd = tauri_parent_hwnd(&window)?;
    if !is_valid_embed_rect(rect) {
        log_embed(&format!(
            "status=start_skipped reason=invalid_bounds parent_hwnd={} file={} purpose={} display_mode={} preview_mode={} preview_spin={} interactive={} rect_physical={:?}",
            parent_hwnd,
            resolved_input.display(),
            purpose,
            mode,
            preview_mode,
            preview_spin,
            interactive,
            rect
        ));
        return Err(format!("invalid embedded viewer bounds: {:?}", rect));
    }

    let mut st = state.lock().unwrap();
    if let Some(existing) = st.viewer.as_mut() {
        let alive = embedded_process_alive(existing);
        if alive
            && existing.file_path == resolved_input.display().to_string()
            && existing.display_mode == mode
            && existing.purpose == purpose
            && existing.cache_input == resolved_cache_input_string
        {
            #[cfg(windows)]
            {
                move_embedded_host(existing.child_hwnd, rect)?;
                show_embedded_host(existing.child_hwnd, true);
            }
            existing.bounds = rect;
            existing.visible = true;
            log_embed(&format!(
                "status=reused reason=same_purpose_file_mode parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?} purpose={} display_mode={} preview_mode={} preview_spin={} interactive={}",
                existing.parent_hwnd,
                existing.child_hwnd,
                existing.child.id(),
                true,
                existing.visible,
                existing.bounds,
                existing.purpose,
                existing.display_mode,
                preview_mode,
                preview_spin,
                interactive
            ));
            return Ok(embedded_status_from(Some(existing), "reused", None));
        }
    }
    if let Some(mut old) = st.viewer.take() {
        let alive = embedded_process_alive(&mut old);
        log_embed(&format!(
            "status=stopping_old reason=purpose_file_or_mode_changed parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?} purpose={} display_mode={}",
            old.parent_hwnd,
            old.child_hwnd,
            old.child.id(),
            alive,
            old.visible,
            old.bounds,
            old.purpose,
            old.display_mode
        ));
        stop_embedded_record(&mut old);
    }

    let child_hwnd = create_embedded_host(parent_hwnd, rect)?;
    set_host_user_data(child_hwnd, 1);
    let stdout_file = render_tmp_path("lba_embedded_stdout", "log")?;
    let stderr_file = render_tmp_path("lba_embedded_stderr", "log")?;
    let stdout = std::fs::File::create(&stdout_file).map_err(|e| e.to_string())?;
    let stderr = std::fs::File::create(&stderr_file).map_err(|e| e.to_string())?;
    let mut command_args = vec![
        resolved_input.display().to_string(),
        format!("--display-mode={mode}"),
        "--basic-lighting".to_string(),
        "--basic-shadows".to_string(),
        format!("--embed-parent-hwnd={child_hwnd}"),
    ];
    if preview_mode {
        command_args.push("--preview-mode".to_string());
    }
    if preview_spin {
        command_args.push("--preview-spin".to_string());
    }
    if let Some(cache_path) = resolved_cache_input.as_ref() {
        log_render_cache_input("embedded", &mode, cache_path);
        command_args.push(format!("--cache-input={}", cache_path.display()));
    }
    log_embed(&format!(
        "status=starting reason=new_viewer parent_hwnd={} child_hwnd={} file={} purpose={} display_mode={} preview_mode={} preview_spin={} interactive={} rect_physical={:?} command_args={:?}",
        parent_hwnd,
        child_hwnd,
        resolved_input.display(),
        purpose,
        mode,
        preview_mode,
        preview_spin,
        interactive,
        rect,
        command_args
    ));
    let child = Command::new(&exe_path)
        .args(&command_args)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .current_dir(&current_dir)
        .spawn()
        .map_err(|error| {
            unsafe {
                let _ = DestroyWindow(child_hwnd as HWND);
            }
            format!("failed to spawn embedded native viewer: {error}")
        })?;
    let process_id = child.id();
    let mut record = EmbeddedViewerRecord {
        child,
        child_hwnd,
        parent_hwnd,
        purpose: purpose.to_string(),
        file_path: resolved_input.display().to_string(),
        display_mode: mode.to_string(),
        cache_input: resolved_cache_input_string,
        bounds: rect,
        visible: true,
        stdout_file,
        stderr_file,
    };
    if let Err(error) = wait_for_embedded_viewer_start(&mut record) {
        log_embed(&format!(
            "status=start_failed reason=native_start_error error={error}"
        ));
        stop_embedded_record(&mut record);
        return Err(error);
    }
    #[cfg(windows)]
    {
        move_embedded_host(child_hwnd, rect)?;
        show_embedded_host(child_hwnd, true);
    }
    st.viewer = Some(record);
    log_embed(&format!(
        "status=started reason=spawned parent_hwnd={} child_hwnd={} viewer_pid={} process_alive=true visible=true bounds={:?} purpose={} display_mode={} preview_mode={} preview_spin={} interactive={}",
        parent_hwnd, child_hwnd, process_id, rect, purpose, mode, preview_mode, preview_spin, interactive
    ));
    Ok(EmbeddedViewerStatus {
        supported: true,
        running: true,
        visible: true,
        parent_hwnd: Some(parent_hwnd.to_string()),
        child_hwnd: Some(child_hwnd.to_string()),
        file: Some(resolved_input.display().to_string()),
        mode: Some(mode.to_string()),
        rect_physical: Some(rect),
        process_id: Some(process_id),
        purpose: Some(purpose.to_string()),
        status: "started".to_string(),
        error: None,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
    })
}

#[cfg(not(windows))]
#[tauri::command]
fn start_embedded_viewer(
    _window: Window,
    _state: State<'_, Mutex<EmbeddedViewerState>>,
    _file_path: String,
    _display_mode: String,
    _rect: EmbeddedRect,
    _purpose: String,
    _cache_input: Option<String>,
) -> Result<EmbeddedViewerStatus, String> {
    Err("embedded viewer is Windows-only".to_string())
}

#[tauri::command]
fn update_embedded_viewer_bounds(
    state: State<'_, Mutex<EmbeddedViewerState>>,
    rect: EmbeddedRect,
) -> Result<EmbeddedViewerStatus, String> {
    let mut st = state.lock().unwrap();
    let Some(record) = st.viewer.as_mut() else {
        return Ok(embedded_status_from(None, "not_running", None));
    };
    if !is_valid_embed_rect(rect) {
        let alive = embedded_process_alive(record);
        log_embed(&format!(
            "status=bounds_skipped reason=invalid_bounds parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?} requested_bounds={:?}",
            record.parent_hwnd,
            record.child_hwnd,
            record.child.id(),
            alive,
            record.visible,
            record.bounds,
            rect
        ));
        return Ok(embedded_status_from(Some(record), "bounds_skipped", None));
    }
    if env::var("LBA_EMBED_VIEWER_LOCK_BOUNDS").ok().as_deref() == Some("1") {
        let alive = embedded_process_alive(record);
        log_embed(&format!(
            "status=bounds_skipped reason=lock_bounds parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?} requested_bounds={:?}",
            record.parent_hwnd,
            record.child_hwnd,
            record.child.id(),
            alive,
            record.visible,
            record.bounds,
            rect
        ));
        return Ok(embedded_status_from(Some(record), "bounds_locked", None));
    }
    #[cfg(windows)]
    move_embedded_host(record.child_hwnd, rect)?;
    record.bounds = rect;
    record.visible = true;
    #[cfg(windows)]
    show_embedded_host(record.child_hwnd, true);
    let alive = embedded_process_alive(record);
    log_embed(&format!(
        "status=bounds_updated reason=update_bounds parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?}",
        record.parent_hwnd,
        record.child_hwnd,
        record.child.id(),
        alive,
        record.visible,
        record.bounds
    ));
    Ok(embedded_status_from(Some(record), "bounds_updated", None))
}

#[tauri::command]
fn hide_embedded_viewer(
    state: State<'_, Mutex<EmbeddedViewerState>>,
) -> Result<EmbeddedViewerStatus, String> {
    let mut st = state.lock().unwrap();
    let Some(record) = st.viewer.as_mut() else {
        return Ok(embedded_status_from(None, "not_running", None));
    };
    if env::var("LBA_EMBED_VIEWER_NO_AUTO_HIDE").ok().as_deref() == Some("1") {
        let alive = embedded_process_alive(record);
        log_embed(&format!(
            "status=hide_skipped reason=no_auto_hide parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?}",
            record.parent_hwnd,
            record.child_hwnd,
            record.child.id(),
            alive,
            record.visible,
            record.bounds
        ));
        return Ok(embedded_status_from(Some(record), "hide_skipped", None));
    }
    #[cfg(windows)]
    show_embedded_host(record.child_hwnd, false);
    record.visible = false;
    let alive = embedded_process_alive(record);
    log_embed(&format!(
        "status=hidden reason=page_inactive parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?}",
        record.parent_hwnd,
        record.child_hwnd,
        record.child.id(),
        alive,
        record.visible,
        record.bounds
    ));
    Ok(embedded_status_from(Some(record), "hidden", None))
}

#[tauri::command]
fn show_embedded_viewer(
    state: State<'_, Mutex<EmbeddedViewerState>>,
) -> Result<EmbeddedViewerStatus, String> {
    let mut st = state.lock().unwrap();
    let Some(record) = st.viewer.as_mut() else {
        return Ok(embedded_status_from(None, "not_running", None));
    };
    #[cfg(windows)]
    show_embedded_host(record.child_hwnd, true);
    record.visible = true;
    let alive = embedded_process_alive(record);
    log_embed(&format!(
        "status=shown reason=page_active parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?}",
        record.parent_hwnd,
        record.child_hwnd,
        record.child.id(),
        alive,
        record.visible,
        record.bounds
    ));
    Ok(embedded_status_from(Some(record), "shown", None))
}

#[tauri::command]
fn stop_embedded_viewer(
    state: State<'_, Mutex<EmbeddedViewerState>>,
) -> Result<EmbeddedViewerStatus, String> {
    let mut st = state.lock().unwrap();
    if let Some(mut record) = st.viewer.take() {
        let alive = embedded_process_alive(&mut record);
        log_embed(&format!(
            "status=stopping reason=explicit_stop parent_hwnd={} child_hwnd={} viewer_pid={} process_alive={} visible={} bounds={:?}",
            record.parent_hwnd,
            record.child_hwnd,
            record.child.id(),
            alive,
            record.visible,
            record.bounds
        ));
        stop_embedded_record(&mut record);
        return Ok(embedded_status_from(None, "stopped", None));
    }
    Ok(embedded_status_from(None, "not_running", None))
}

#[tauri::command]
fn get_embedded_viewer_status(
    state: State<'_, Mutex<EmbeddedViewerState>>,
) -> EmbeddedViewerStatus {
    let mut st = state.lock().unwrap();
    if let Some(record) = st.viewer.as_mut() {
        match record.child.try_wait() {
            Ok(Some(status)) => {
                let error = Some(format!("process exited: {:?}", status.code()));
                log_embedded_output_tail(record, "native_exited");
                let mut old = st.viewer.take().unwrap();
                #[cfg(windows)]
                unsafe {
                    let _ = DestroyWindow(old.child_hwnd as HWND);
                }
                let _ = old.child.wait();
                return embedded_status_from(None, "exited", error);
            }
            Ok(None) => {
                log_embedded_output_tail(record, "running");
                return embedded_status_from(Some(record), "running", None);
            }
            Err(error) => {
                return embedded_status_from(Some(record), "poll_error", Some(error.to_string()))
            }
        }
    }
    embedded_status_from(None, "not_running", None)
}

fn embedded_status_from(
    record: Option<&EmbeddedViewerRecord>,
    status: &str,
    error: Option<String>,
) -> EmbeddedViewerStatus {
    if let Some(record) = record {
        let stdout_tail = tail_text(&record.stdout_file, 8000);
        let stderr_tail = tail_text(&record.stderr_file, 8000);
        EmbeddedViewerStatus {
            supported: cfg!(windows),
            running: true,
            visible: record.visible,
            parent_hwnd: Some(record.parent_hwnd.to_string()),
            child_hwnd: Some(record.child_hwnd.to_string()),
            file: Some(record.file_path.clone()),
            mode: Some(record.display_mode.clone()),
            rect_physical: Some(record.bounds),
            process_id: Some(record.child.id()),
            purpose: Some(record.purpose.clone()),
            status: status.to_string(),
            error,
            stdout_tail,
            stderr_tail,
        }
    } else {
        EmbeddedViewerStatus {
            supported: cfg!(windows),
            running: false,
            visible: false,
            parent_hwnd: None,
            child_hwnd: None,
            file: None,
            mode: None,
            rect_physical: None,
            process_id: None,
            purpose: None,
            status: status.to_string(),
            error,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
        }
    }
}

#[tauri::command]
async fn render_preview_image(
    file_path: String,
    display_mode: String,
) -> Result<RenderPreviewOutput, String> {
    tauri::async_runtime::spawn_blocking(move || render_preview_image_sync(file_path, display_mode))
        .await
        .map_err(|e| e.to_string())?
}

fn render_preview_image_sync(
    file_path: String,
    display_mode: String,
) -> Result<RenderPreviewOutput, String> {
    let current_dir = get_root();
    let exe_path = backend_exe_path("litematica_native_viewer.exe");
    let mode = match display_mode.as_str() {
        "fast_experimental" => "fast_experimental",
        "full" => "full",
        _ => "normal",
    };
    let output_path = render_tmp_path("lba_native_preview", "png")?;
    let mut cmd = Command::new(exe_path);
    cmd.arg(file_path)
        .arg(format!("--display-mode={mode}"))
        .arg(format!("--preview-output={}", output_path.display()))
        .arg("--auto-exit-seconds=4")
        .arg("--basic-lighting")
        .arg("--basic-shadows")
        .current_dir(&current_dir);
    let completed = run_command_with_timeout(cmd, Duration::from_secs(30))?;
    let stdout = String::from_utf8_lossy(&completed.stdout).to_string();
    let stderr = String::from_utf8_lossy(&completed.stderr).to_string();
    if !completed.status.success() {
        return Err(format!(
            "Exit code: {:?}\nStdout: {}\nStderr: {}",
            completed.status.code(),
            stdout,
            stderr
        ));
    }
    let bytes = std::fs::read(&output_path).map_err(|e| e.to_string())?;
    Ok(RenderPreviewOutput {
        preview_path: output_path.display().to_string(),
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
        stdout,
        stderr,
        exit_code: completed.status.code(),
    })
}

#[tauri::command]
async fn generate_preview_image(
    file_path: String,
    display_mode: String,
) -> Result<RenderPreviewOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        generate_preview_image_sync(file_path, display_mode)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn generate_preview_image_sync(
    file_path: String,
    display_mode: String,
) -> Result<RenderPreviewOutput, String> {
    let current_dir = get_root();
    let exe_path = backend_exe_path("litematica_native_viewer.exe");
    let mode = normalized_display_mode(&display_mode);
    let resolved_input = normalized_input_path(&file_path);
    let preview_dir = data_root()?.join("projection-library").join("previews");
    std::fs::create_dir_all(&preview_dir).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(resolved_input.to_string_lossy().as_bytes());
    hasher.update(mode.as_bytes());
    if let Ok(meta) = std::fs::metadata(&resolved_input) {
        hasher.update(meta.len().to_le_bytes());
        if let Ok(modified) = meta.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(duration.as_nanos().to_le_bytes());
            }
        }
    }
    let digest = hasher.finalize();
    let output_path = preview_dir.join(format!("{mode}_{:x}.png", digest));
    let mut cmd = Command::new(exe_path);
    cmd.arg(resolved_input)
        .arg(format!("--display-mode={mode}"))
        .arg("--basic-lighting")
        .arg("--basic-shadows")
        .arg(format!("--preview-output={}", output_path.display()))
        .arg("--auto-exit-seconds=4")
        .current_dir(&current_dir);
    let completed = run_command_with_timeout(cmd, Duration::from_secs(30))?;
    let stdout = String::from_utf8_lossy(&completed.stdout).to_string();
    let stderr = String::from_utf8_lossy(&completed.stderr).to_string();
    if !completed.status.success() {
        return Err(format!(
            "Exit code: {:?}\nStdout: {}\nStderr: {}",
            completed.status.code(),
            stdout,
            stderr
        ));
    }
    let bytes = std::fs::read(&output_path).map_err(|e| e.to_string())?;
    Ok(RenderPreviewOutput {
        preview_path: output_path.display().to_string(),
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
        stdout,
        stderr,
        exit_code: completed.status.code(),
    })
}

#[tauri::command]
fn get_user_config() -> Result<UserConfigInfo, String> {
    let dir = current_user_config_dir()?;
    migrate_legacy_library_if_needed(&dir)?;
    let config = read_user_config_data(&dir);
    write_user_config_data(&dir, &config)?;
    Ok(UserConfigInfo {
        config_dir: dir.display().to_string(),
        default_config_dir: default_user_config_dir()?.display().to_string(),
        config,
    })
}

#[tauri::command]
fn save_user_config(input: UserConfigInput) -> Result<UserConfigInfo, String> {
    let dir = current_user_config_dir()?;
    let mut config = read_user_config_data(&dir);
    if let Some(theme) = input.theme {
        config.theme = normalize_theme(&theme);
    }
    if let Some(mode) = input.render_display_mode {
        config.render_display_mode = normalize_mode_string(&mode);
    }
    if let Some(mode) = input.preview_mode {
        config.preview_mode = normalize_mode_string(&mode);
    }
    if let Some(behavior) = input.material_list_window_behavior {
        config.material_list_window_behavior = normalize_material_list_window_behavior(&behavior);
    }
    if let Some(show) = input.show_ui_test_page {
        config.show_ui_test_page = show;
    }
    if let Some(count) = input.local_library_tail_path_count {
        config.local_library_tail_path_count = normalize_local_library_tail_path_count(count);
    }
    write_user_config_data(&dir, &config)?;
    Ok(UserConfigInfo {
        config_dir: dir.display().to_string(),
        default_config_dir: default_user_config_dir()?.display().to_string(),
        config,
    })
}

#[tauri::command]
fn choose_user_config_dir(app: AppHandle) -> Result<Option<String>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
fn open_user_config_dir() -> Result<(), String> {
    let dir = current_user_config_dir()?;
    #[cfg(windows)]
    {
        Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if !dst_path.exists() {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn set_user_config_dir(path: String, migrate: bool) -> Result<UserConfigInfo, String> {
    let target = PathBuf::from(path);
    if !target.is_absolute() {
        return Err("User config directory must be an absolute path.".to_string());
    }
    let current = current_user_config_dir()?;
    ensure_user_config_layout(&target)?;
    if migrate {
        copy_dir_recursive(&current, &target)?;
    }
    let pointer = config_dir_pointer_path()?;
    std::fs::write(
        pointer,
        serde_json::json!({ "config_dir": target.display().to_string() }).to_string(),
    )
    .map_err(|e| e.to_string())?;
    get_user_config()
}

#[tauri::command]
fn reset_user_config_dir(migrate: bool) -> Result<UserConfigInfo, String> {
    let current = current_user_config_dir()?;
    let default_dir = default_user_config_dir()?;
    ensure_user_config_layout(&default_dir)?;
    if migrate && current != default_dir {
        copy_dir_recursive(&current, &default_dir)?;
    }
    let pointer = config_dir_pointer_path()?;
    if pointer.exists() {
        std::fs::remove_file(pointer).map_err(|e| e.to_string())?;
    }
    get_user_config()
}

#[tauri::command]
fn read_user_config_file(relative_path: String) -> Result<String, String> {
    let dir = current_user_config_dir()?;
    migrate_legacy_library_if_needed(&dir)?;
    let path = dir.join(user_runtime_relative_path(&relative_path)?);
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_user_config_file(relative_path: String, content: String) -> Result<(), String> {
    let dir = current_user_config_dir()?;
    let path = dir.join(user_runtime_relative_path(&relative_path)?);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_user_config_file_path(relative_path: String) -> Result<String, String> {
    let dir = current_user_config_dir()?;
    let path = dir.join(user_runtime_relative_path(&relative_path)?);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(path.display().to_string())
}

#[tauri::command]
fn start_cache_build_task(
    state: State<'_, Mutex<BuildState>>,
    file_path: String,
    build_mode: String,
) -> Result<CacheBuildLaunch, String> {
    let current_dir = get_root();
    let exe_path = backend_exe_path("litematica_native_viewer.exe");
    let input = PathBuf::from(&file_path);
    let resolved_input = if input.is_absolute() {
        input
    } else {
        current_dir.join(input)
    };
    let progress_file = render_tmp_path("lba_native_progress", "json")?;
    let cache_file = render_tmp_path("lba_native_cache", "json")?;
    let stdout_file = render_tmp_path("lba_native_stdout", "log")?;
    let stderr_file = render_tmp_path("lba_native_stderr", "log")?;
    let stdout = std::fs::File::create(&stdout_file).map_err(|e| e.to_string())?;
    let stderr = std::fs::File::create(&stderr_file).map_err(|e| e.to_string())?;

    let mut cmd = Command::new(exe_path);
    let mode = match build_mode.as_str() {
        "fast_experimental" => "fast_experimental",
        "full" => "full",
        _ => "normal",
    };
    cmd.arg(&resolved_input)
        .arg("--chunk-size=32")
        .arg("--prebuild-before-show")
        .arg("--prebuild-only")
        .arg(format!("--display-mode={mode}"))
        .arg(format!("--ready-file={}", progress_file.display()))
        .arg(format!("--cache-file={}", cache_file.display()))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .current_dir(&current_dir);
    apply_build_mode_env(&mut cmd, &build_mode);
    println!(
        "[LBA_RENDER_CACHE] target=build display_mode={} input={} progressFile={} cacheFile={} cacheKind=\"new_native_3d_cache\" exists=false size=0",
        mode,
        resolved_input.display(),
        progress_file.display(),
        cache_file.display()
    );

    let child = cmd.spawn().map_err(|e| e.to_string())?;

    let mut st = state.lock().unwrap();
    if let Some(mut old_child) = st.child.take() {
        let _ = old_child.kill();
        let _ = old_child.wait();
    }
    st.child = Some(child);
    st.progress_file = Some(progress_file.clone());
    st.cache_file = Some(cache_file.clone());
    st.stdout_file = Some(stdout_file.clone());
    st.stderr_file = Some(stderr_file.clone());
    st.started_at = Some(Instant::now());
    st.last_progress_signature = None;
    st.last_progress_change = Some(Instant::now());
    Ok(CacheBuildLaunch {
        file_path: resolved_input.display().to_string(),
        progress_file: progress_file.display().to_string(),
        cache_file: cache_file.display().to_string(),
        stdout_file: stdout_file.display().to_string(),
        stderr_file: stderr_file.display().to_string(),
    })
}

#[tauri::command]
fn kill_cache_build_task(state: State<'_, Mutex<BuildState>>) -> Result<(), String> {
    let mut st = state.lock().unwrap();
    if let Some(mut child) = st.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    st.started_at = None;
    st.last_progress_signature = None;
    st.last_progress_change = None;
    Ok(())
}

fn cache_progress_signature(raw: &str) -> Option<(String, bool)> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let ready = value
        .get("ready")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let signature = serde_json::json!({
        "ready": ready,
        "phase": value.get("phase").and_then(|v| v.as_str()).unwrap_or(""),
        "total_chunks": value.get("total_chunks").and_then(|v| v.as_u64()).unwrap_or(0),
        "built_chunks": value.get("built_chunks").and_then(|v| v.as_u64()).unwrap_or(0),
        "renderable_chunks": value.get("renderable_chunks").and_then(|v| v.as_u64()).unwrap_or(0),
        "empty_mesh_chunks": value.get("empty_mesh_chunks").and_then(|v| v.as_u64()).unwrap_or(0),
        "cache_file_bytes": value.get("cache_file_bytes").and_then(|v| v.as_u64()).unwrap_or(0),
    })
    .to_string();
    Some((signature, ready))
}

fn cache_build_error_progress(message: &str) -> String {
    serde_json::json!({
        "ready": false,
        "total_chunks": 0,
        "built_chunks": 0,
        "percent": 0.0,
        "phase": "error",
        "error": message,
    })
    .to_string()
}

#[tauri::command]
fn poll_cache_build_task(state: State<'_, Mutex<BuildState>>) -> CacheBuildSnapshot {
    let mut st = state.lock().unwrap();
    let progress_file = st.progress_file.clone();
    let cache_file = st.cache_file.clone();
    let stdout_file = st.stdout_file.clone();
    let stderr_file = st.stderr_file.clone();
    let mut running = false;
    let mut exit_code = None;
    let mut forced_error: Option<String> = None;
    let mut progress_json = progress_file
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok());
    let wait_result = if let Some(child) = st.child.as_mut() {
        Some(child.try_wait())
    } else {
        None
    };
    if let Some(result) = wait_result {
        match result {
            Ok(None) => {
                running = true;
                let now = Instant::now();
                if let Some(raw) = progress_json.as_deref() {
                    if let Some((signature, ready)) = cache_progress_signature(raw) {
                        if st.last_progress_signature.as_deref() != Some(signature.as_str()) {
                            st.last_progress_signature = Some(signature);
                            st.last_progress_change = Some(now);
                        } else if !ready
                            && st
                                .last_progress_change
                                .map(|instant| instant.elapsed() >= Duration::from_secs(60))
                                .unwrap_or(false)
                        {
                            forced_error = Some("3D cache 构建 60 秒没有实质进度变化，已自动停止。请查看 stdout/stderr，通常是 native viewer 卡在某个 mesh/semantic 阶段。".to_string());
                        }
                    }
                } else if st
                    .started_at
                    .map(|instant| instant.elapsed() >= Duration::from_secs(90))
                    .unwrap_or(false)
                {
                    forced_error =
                        Some("3D cache 构建 90 秒仍未写出 progress JSON，已自动停止。".to_string());
                }
                if forced_error.is_some() {
                    if let Some(mut child) = st.child.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    running = false;
                    exit_code = None;
                    st.started_at = None;
                    st.last_progress_signature = None;
                    st.last_progress_change = None;
                }
            }
            Ok(Some(status)) => {
                exit_code = status.code();
                st.child = None;
                st.started_at = None;
            }
            Err(_) => {
                running = false;
            }
        }
    }
    if let Some(message) = forced_error {
        progress_json = Some(cache_build_error_progress(&message));
    }
    let stdout_tail = stdout_file
        .as_ref()
        .map(|path| tail_text(path, 12000))
        .unwrap_or_default();
    let stderr_tail = stderr_file
        .as_ref()
        .map(|path| tail_text(path, 12000))
        .unwrap_or_default();
    let cache_exists = cache_file
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
    CacheBuildSnapshot {
        running,
        exit_code,
        progress_file: progress_file.as_ref().map(|p| p.display().to_string()),
        cache_file: cache_file.as_ref().map(|p| p.display().to_string()),
        stdout_file: stdout_file.as_ref().map(|p| p.display().to_string()),
        stderr_file: stderr_file.as_ref().map(|p| p.display().to_string()),
        progress_json,
        stdout_tail,
        stderr_tail,
        cache_exists,
    }
}

#[tauri::command]
fn read_file_string(path: String) -> Result<String, String> {
    std::fs::read_to_string(get_root().join(path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_file_string(path: String, content: String) -> Result<(), String> {
    let full_path = get_root().join(path);
    if let Some(p) = full_path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(&full_path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_text_file_absolute(path: String, content: String) -> Result<(), String> {
    let full_path = PathBuf::from(path);
    if !full_path.is_absolute() {
        return Err("output path must be absolute".to_string());
    }
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&full_path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn check_file_exists(path: String) -> bool {
    get_root().join(path).is_file()
}

fn collect_litematic_files_in_directory(
    current_dir: &Path,
    recursive: bool,
    files: &mut Vec<String>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(current_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_litematic_files_in_directory(&path, true, files)?;
            }
            continue;
        }
        let has_litematic_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("litematic"))
            .unwrap_or(false);
        if has_litematic_ext {
            files.push(path.display().to_string());
        }
    }
    Ok(())
}

fn metadata_modified_ms(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn directory_entry_info(path: PathBuf) -> Result<DirectoryEntryInfo, String> {
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    Ok(DirectoryEntryInfo {
        path: path.display().to_string(),
        name,
        is_dir: metadata.is_dir(),
        is_file: metadata.is_file(),
        file_size: if metadata.is_file() {
            metadata.len()
        } else {
            0
        },
        mtime_ms: metadata_modified_ms(&metadata),
        extension,
    })
}

fn collect_litematic_file_entries_in_directory(
    current_dir: &Path,
    recursive: bool,
    files: &mut Vec<DirectoryEntryInfo>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(current_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_litematic_file_entries_in_directory(&path, true, files)?;
            }
            continue;
        }
        let has_litematic_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("litematic"))
            .unwrap_or(false);
        if has_litematic_ext {
            files.push(directory_entry_info(path)?);
        }
    }
    Ok(())
}

fn resolve_input_path(path: &str) -> PathBuf {
    let input = PathBuf::from(path);
    if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    }
}

#[tauri::command]
fn list_litematic_files_in_directory(path: String, recursive: bool) -> Result<Vec<String>, String> {
    let full_path = resolve_input_path(&path);
    if !full_path.exists() {
        return Err(format!("directory not found: {}", full_path.display()));
    }
    if !full_path.is_dir() {
        return Err(format!("path is not a directory: {}", full_path.display()));
    }
    let mut files = Vec::new();
    collect_litematic_files_in_directory(&full_path, recursive, &mut files)?;
    files.sort_unstable();
    Ok(files)
}

#[tauri::command]
fn list_directory_entries(path: String) -> Result<Vec<DirectoryEntryInfo>, String> {
    let full_path = resolve_input_path(&path);
    if !full_path.exists() {
        return Err(format!("directory not found: {}", full_path.display()));
    }
    if !full_path.is_dir() {
        return Err(format!("path is not a directory: {}", full_path.display()));
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&full_path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        entries.push(directory_entry_info(entry.path())?);
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
fn list_litematic_file_entries_in_directory(
    path: String,
    recursive: bool,
) -> Result<Vec<DirectoryEntryInfo>, String> {
    let full_path = resolve_input_path(&path);
    if !full_path.exists() {
        return Err(format!("directory not found: {}", full_path.display()));
    }
    if !full_path.is_dir() {
        return Err(format!("path is not a directory: {}", full_path.display()));
    }
    let mut files = Vec::new();
    collect_litematic_file_entries_in_directory(&full_path, recursive, &mut files)?;
    files.sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
    Ok(files)
}

#[tauri::command]
fn get_workspace_root() -> String {
    get_root().display().to_string()
}

#[tauri::command]
fn get_path_info(path: String) -> PathInfo {
    let input = PathBuf::from(&path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };
    let parent = full_path.parent().map(PathBuf::from).unwrap_or_default();
    let metadata = std::fs::metadata(&full_path).ok();
    let parent_exists = parent.is_dir();
    let has_litematic_ext = full_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("litematic"))
        .unwrap_or(false);

    PathInfo {
        raw: path,
        normalized: full_path.display().to_string(),
        parent_dir: parent.display().to_string(),
        parent_exists,
        exists: metadata.is_some(),
        is_dir: metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
        is_file: metadata.as_ref().map(|m| m.is_file()).unwrap_or(false),
        is_absolute: full_path.is_absolute(),
        has_litematic_ext,
    }
}

#[tauri::command]
fn read_image_base64(path: String) -> Result<String, String> {
    let input = PathBuf::from(path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };
    if let Ok(bytes) = std::fs::read(full_path) {
        Ok(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        ))
    } else {
        Err("Not found".into())
    }
}

#[tauri::command]
fn read_projection_preview_image(
    file_path: String,
) -> Result<Option<ProjectionPreviewOutput>, String> {
    let input = PathBuf::from(file_path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };

    let file = std::fs::File::open(&full_path)
        .map_err(|e| format!("open {} failed: {}", full_path.display(), e))?;
    let mut decoder = GzDecoder::new(file);
    let mut nbt_bytes = Vec::new();
    decoder
        .read_to_end(&mut nbt_bytes)
        .map_err(|e| format!("decompress {} failed: {}", full_path.display(), e))?;

    let Some(pixels) = extract_preview_image_data_from_nbt_bytes(&nbt_bytes)? else {
        return Ok(None);
    };
    if pixels.is_empty() {
        return Ok(None);
    }

    let size = (pixels.len() as f64).sqrt() as usize;
    if size == 0 || size * size != pixels.len() {
        return Err(format!(
            "invalid PreviewImageData length {}, expected square pixel array",
            pixels.len()
        ));
    }

    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        let argb = pixel as u32;
        rgba.push(((argb >> 16) & 0xff) as u8);
        rgba.push(((argb >> 8) & 0xff) as u8);
        rgba.push((argb & 0xff) as u8);
        rgba.push(((argb >> 24) & 0xff) as u8);
    }

    let image = RgbaImage::from_raw(size as u32, size as u32, rgba)
        .ok_or_else(|| "failed to build preview image buffer".to_string())?;
    let mut png_bytes = Vec::new();
    DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| format!("encode preview png failed: {}", e))?;

    Ok(Some(ProjectionPreviewOutput {
        width: size as u32,
        height: size as u32,
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png_bytes)
        ),
    }))
}

fn extract_preview_image_data_from_nbt_bytes(bytes: &[u8]) -> Result<Option<Vec<i32>>, String> {
    const FIELD_NAME: &[u8] = b"PreviewImageData";
    let Some(name_offset) = bytes
        .windows(FIELD_NAME.len())
        .position(|window| window == FIELD_NAME)
    else {
        return Ok(None);
    };
    if name_offset < 3 {
        return Err("PreviewImageData field is truncated".to_string());
    }

    let tag_type = bytes[name_offset - 3];
    let name_len = u16::from_be_bytes([bytes[name_offset - 2], bytes[name_offset - 1]]) as usize;
    if tag_type != 11 {
        return Err(format!(
            "PreviewImageData tag type mismatch: expected 11 (IntArray), got {}",
            tag_type
        ));
    }
    if name_len != FIELD_NAME.len() {
        return Err(format!(
            "PreviewImageData name length mismatch: expected {}, got {}",
            FIELD_NAME.len(),
            name_len
        ));
    }

    let data_start = name_offset + FIELD_NAME.len();
    if data_start + 4 > bytes.len() {
        return Err("PreviewImageData array length is truncated".to_string());
    }
    let array_len = i32::from_be_bytes([
        bytes[data_start],
        bytes[data_start + 1],
        bytes[data_start + 2],
        bytes[data_start + 3],
    ]);
    if array_len < 0 {
        return Err(format!(
            "PreviewImageData length is negative: {}",
            array_len
        ));
    }
    let array_len = array_len as usize;
    let payload_start = data_start + 4;
    let payload_end = payload_start + array_len * 4;
    if payload_end > bytes.len() {
        return Err(format!(
            "PreviewImageData payload truncated: expected {} bytes, got {}",
            array_len * 4,
            bytes.len().saturating_sub(payload_start)
        ));
    }

    let mut pixels = Vec::with_capacity(array_len);
    for chunk in bytes[payload_start..payload_end].chunks_exact(4) {
        pixels.push(i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(Some(pixels))
}

#[tauri::command]
fn open_file_parent_dir(file_path: String) -> Result<(), String> {
    let input = PathBuf::from(&file_path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };
    let parent = full_path
        .parent()
        .ok_or_else(|| "File path has no parent directory.".to_string())?
        .to_path_buf();
    let exists = full_path.is_file();
    if !exists {
        let message = format!(
            "[LBA_OPEN_FOLDER] file_path={} parent_dir={} exists=false result=error error=file_not_found",
            full_path.display(),
            parent.display()
        );
        eprintln!("{message}");
        return Err(message);
    }
    let result = Command::new("explorer").arg(&parent).spawn();
    match result {
        Ok(_) => {
            eprintln!(
                "[LBA_OPEN_FOLDER] file_path={} parent_dir={} exists=true result=ok error=",
                full_path.display(),
                parent.display()
            );
            Ok(())
        }
        Err(error) => {
            let message = format!(
                "[LBA_OPEN_FOLDER] file_path={} parent_dir={} exists=true result=error error={}",
                full_path.display(),
                parent.display(),
                error
            );
            eprintln!("{message}");
            Err(message)
        }
    }
}

#[tauri::command]
fn copy_file_to_directory(
    source_path: String,
    target_directory: String,
    target_file_name: String,
    overwrite: bool,
) -> Result<CopyFileToDirectoryOutput, String> {
    let source_input = PathBuf::from(&source_path);
    let source_full_path = if source_input.is_absolute() {
        source_input
    } else {
        get_root().join(source_input)
    };
    if !source_full_path.is_file() {
        return Err(format!(
            "source file not found: {}",
            source_full_path.display()
        ));
    }

    let trimmed_file_name = target_file_name.trim();
    if trimmed_file_name.is_empty() {
        return Err("target file name must not be empty".to_string());
    }
    if trimmed_file_name.contains('/') || trimmed_file_name.contains('\\') {
        return Err("target file name must not contain path separators".to_string());
    }

    let target_input = PathBuf::from(&target_directory);
    let target_dir_path = if target_input.is_absolute() {
        target_input
    } else {
        get_root().join(target_input)
    };
    if target_dir_path.exists() && !target_dir_path.is_dir() {
        return Err(format!(
            "target path is not a directory: {}",
            target_dir_path.display()
        ));
    }
    std::fs::create_dir_all(&target_dir_path).map_err(|e| e.to_string())?;

    let target_full_path = target_dir_path.join(trimmed_file_name);
    if target_full_path == source_full_path {
        return Err("source and target path are identical".to_string());
    }

    let overwritten = target_full_path.exists();
    if overwritten && !overwrite {
        return Err(format!(
            "target file already exists: {}",
            target_full_path.display()
        ));
    }
    if overwritten && !target_full_path.is_file() {
        return Err(format!(
            "target path is not a file: {}",
            target_full_path.display()
        ));
    }

    let bytes_copied =
        std::fs::copy(&source_full_path, &target_full_path).map_err(|e| e.to_string())?;
    Ok(CopyFileToDirectoryOutput {
        target_path: target_full_path.display().to_string(),
        overwritten,
        bytes_copied,
    })
}

#[tauri::command]
fn open_workspace_path(path: String) -> Result<(), String> {
    let input = PathBuf::from(&path);
    let full_path = if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    };
    Command::new("explorer")
        .arg(full_path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn cleanup_local_temp_files() -> Result<String, String> {
    let mut removed = 0usize;
    for dir in [
        data_root()?.join("cache").join("render"),
        data_root()?.join("tmp").join("render"),
    ] {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
            } else {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
            removed += 1;
        }
    }
    Ok(format!(
        "Removed {removed} desktop-nova render temp entries."
    ))
}

#[derive(Deserialize)]
struct RedenSizesInput {
    #[serde(rename = "xSize")]
    x_size: Option<u32>,
    #[serde(rename = "ySize")]
    y_size: Option<u32>,
    #[serde(rename = "zSize")]
    z_size: Option<u32>,
}

#[derive(Serialize)]
struct RedenDownloadOutput {
    path: String,
    file_name: String,
    bytes: usize,
}

#[derive(Clone, Serialize)]
struct VaultBlockIconProgress {
    current: usize,
    total: usize,
    downloaded: usize,
    status: String,
}

#[derive(Serialize)]
struct VaultBlockIconDownloadOutput {
    total: usize,
    downloaded: usize,
    target_dir: String,
    root_relpath: String,
}

#[derive(Serialize)]
struct BuiltinIconExtractOutput {
    target_dir: String,
    root_relpath: String,
    extracted: usize,
}

#[derive(Serialize)]
struct WikiEnumCatalogDownloadOutput {
    target_dir: String,
    root_relpath: String,
    blocks: usize,
    items: usize,
    enchantments: usize,
    entities: usize,
}

const VAULT_SITE_LABEL: &str = "https://ccvaults.com/";
const VAULT_API_TOKEN_PATH: &str = "/api/token";
const VAULT_API_BLOCKS_PATH: &str = "/api/assets/20.%20Blocks";
const VAULT_API_ITEMS_PATH: &str = "/api/assets/10.%20Items";
const VAULT_API_ALL_ASSETS_PATH: &str = "/api/assets/all";
const VAULT_API_KEY: &str = "242gag58XGJjOfPPl9nFE8xz92YjMHysKyvVaJ";
const VAULT_LEGACY_API_KEY: &str = "mcicons-apikey-0201osaiudx-24493534";
const VAULT_BLOCK_ICON_ROOT_RELPATH: &str = "minecraft-assets/block_icon/vault";
const VAULT_ITEM_ICON_ROOT_RELPATH: &str = "minecraft-assets/item/vault";
const VAULT_BLOCK_CATEGORY_NAME: &str = "20. Blocks";
const VAULT_ITEM_CATEGORY_NAME: &str = "10. Items";
const WIKI_ENUM_BLOCKS_URL: &str = "https://minecraft.wiki/w/Java_Edition_data_values/Blocks";
const WIKI_ENUM_ITEMS_URL: &str = "https://minecraft.wiki/w/Java_Edition_data_values/Items";
const WIKI_ENUM_MAIN_URL: &str = "https://minecraft.wiki/w/Java_Edition_data_values";
const WIKI_ENUM_ENTITIES_URL: &str = "https://minecraft.wiki/w/Java_Edition_data_values/Entities";
const WIKI_ENUM_ROOT_RELPATH: &str = "enumerator/base/wiki";
const BUILTIN_BLOCK_ARCHIVE_RELPATH: &str = "pack-in/arr-private/block.zip";
const BUILTIN_BLOCK_ICON_ROOT_RELPATH: &str = "minecraft-assets/block_2d/initial";
const BUILTIN_ITEM_ARCHIVE_RELPATH: &str = "pack-in/arr-private/item.zip";
const BUILTIN_ITEM_ICON_ROOT_RELPATH: &str = "minecraft-assets/item/initial";

fn directory_contains_files(dir: &Path) -> Result<bool, String> {
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

fn clear_directory_contents(dir: &Path) -> Result<(), String> {
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

fn extract_builtin_icon_archive(
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

fn emit_vault_block_icon_progress(
    app: &AppHandle,
    current: usize,
    total: usize,
    downloaded: usize,
    status: impl Into<String>,
) {
    let _ = app.emit(
        "vault-block-icons-download-progress",
        VaultBlockIconProgress {
            current,
            total,
            downloaded,
            status: status.into(),
        },
    );
}

fn emit_vault_item_icon_progress(
    app: &AppHandle,
    current: usize,
    total: usize,
    downloaded: usize,
    status: impl Into<String>,
) {
    let _ = app.emit(
        "vault-item-icons-download-progress",
        VaultBlockIconProgress {
            current,
            total,
            downloaded,
            status: status.into(),
        },
    );
}

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

fn find_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_index = text.find(start)? + start.len();
    let end_index = text[start_index..].find(end)? + start_index;
    Some(&text[start_index..end_index])
}

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
    find_between(script, "x-api-key\":\"", "\"")
        .or_else(|| find_between(script, "x-api-key\":", "}"))
        .map(str::trim)
        .map(|value| value.trim_matches('"'))
        .filter(|value| !value.is_empty() && !value.contains(':') && !value.contains('{'))
        .map(ToString::to_string)
}

fn discover_vault_api_key(client: &reqwest::blocking::Client, home_html: &str) -> Option<String> {
    let script_path = find_vault_index_script_path(home_html)?;
    let script_url = reqwest::Url::parse(VAULT_SITE_LABEL)
        .ok()?
        .join(&script_path)
        .ok()?;
    let script = client.get(script_url).send().ok()?.text().ok()?;
    extract_vault_api_key_from_script(&script)
}

fn request_vault_token(
    client: &reqwest::blocking::Client,
    api_key: &str,
) -> Result<String, String> {
    let token_response = client
        .post(vault_api_url(VAULT_API_TOKEN_PATH)?)
        .header("x-api-key", api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json;charset=UTF-8")
        .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
        .header(reqwest::header::ORIGIN, VAULT_SITE_LABEL.trim_end_matches('/'))
        .header(reqwest::header::REFERER, VAULT_SITE_LABEL)
        .header("X-Requested-With", "XMLHttpRequest")
        .body("{}")
        .send()
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

fn acquire_vault_token(
    client: &reqwest::blocking::Client,
    home_html: &str,
) -> Result<String, String> {
    let mut token_error = String::new();
    for api_key in [VAULT_API_KEY, VAULT_LEGACY_API_KEY] {
        match request_vault_token(client, api_key) {
            Ok(value) => return Ok(value),
            Err(error) => token_error = error,
        }
    }
    if let Some(discovered_key) = discover_vault_api_key(client, home_html) {
        match request_vault_token(client, &discovered_key) {
            Ok(value) => return Ok(value),
            Err(error) => token_error = error,
        }
    }
    Err(token_error)
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

fn vault_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .cookie_store(true)
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| e.to_string())
}

fn vault_fetch_json_with_auth(
    client: &reqwest::blocking::Client,
    path: &str,
    token: &str,
) -> Result<serde_json::Value, String> {
    let response = client
        .get(vault_api_url(path)?)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
        .header(reqwest::header::ORIGIN, VAULT_SITE_LABEL.trim_end_matches('/'))
        .header(reqwest::header::REFERER, VAULT_SITE_LABEL)
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Vault API failed: status={} body={}", status, text));
    }
    serde_json::from_str(&text).map_err(|e| format!("Vault JSON parse failed: {e}; body={text}"))
}

fn collect_vault_block_entries(payload: &serde_json::Value) -> Vec<(String, String)> {
    collect_vault_category_entries(payload, VAULT_BLOCK_CATEGORY_NAME)
}

fn collect_vault_item_entries(payload: &serde_json::Value) -> Vec<(String, String)> {
    collect_vault_category_entries(payload, VAULT_ITEM_CATEGORY_NAME)
}

fn collect_vault_category_entries(
    payload: &serde_json::Value,
    category_name: &str,
) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let Some(rows) = payload.as_array() else {
        return entries;
    };
    for row in rows {
        let Some(subcategories) = row.get("subcategories").and_then(|value| value.as_array()) else {
            continue;
        };
        for subcategory in subcategories {
            let Some(sub_name) = subcategory.get("name").and_then(|value| value.as_str()).map(str::trim) else {
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

fn collect_vault_block_entries_from_all(payload: &serde_json::Value) -> Vec<(String, String)> {
    collect_vault_category_entries_from_all(payload, VAULT_BLOCK_CATEGORY_NAME)
}

fn collect_vault_item_entries_from_all(payload: &serde_json::Value) -> Vec<(String, String)> {
    collect_vault_category_entries_from_all(payload, VAULT_ITEM_CATEGORY_NAME)
}

fn collect_vault_category_entries_from_all(
    payload: &serde_json::Value,
    category_name: &str,
) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
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

fn download_one_vault_block_icon(
    client: &reqwest::blocking::Client,
    target_dir: &Path,
    block_id: &str,
    file_url: &str,
) -> bool {
    download_one_vault_icon(
        client,
        target_dir,
        block_id,
        file_url,
        "Litematica-Nova-icon-manager/1.0",
        image::imageops::FilterType::Lanczos3,
    )
}

fn download_one_vault_item_icon(
    client: &reqwest::blocking::Client,
    target_dir: &Path,
    item_id: &str,
    file_url: &str,
) -> bool {
    download_one_vault_icon(
        client,
        target_dir,
        item_id,
        file_url,
        "Litematica-Nova-item-icon-manager/1.0",
        image::imageops::FilterType::Nearest,
    )
}

fn download_one_vault_icon(
    client: &reqwest::blocking::Client,
    target_dir: &Path,
    icon_id: &str,
    file_url: &str,
    user_agent: &str,
    resize_filter: image::imageops::FilterType,
) -> bool {
    let response = match client
        .get(file_url)
        .header(reqwest::header::USER_AGENT, user_agent)
        .header(reqwest::header::REFERER, VAULT_SITE_LABEL)
        .send()
    {
        Ok(value) => value,
        Err(_) => return false,
    };
    if !response.status().is_success() {
        return false;
    }
    let bytes = match response.bytes() {
        Ok(value) => value,
        Err(_) => return false,
    };
    let mut image = match image::load_from_memory(&bytes) {
        Ok(value) => value,
        Err(_) => return false,
    };
    if image.dimensions() != (32, 32) {
        image = image.resize_exact(32, 32, resize_filter);
    }
    let mut out = Vec::new();
    if image
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .is_err()
    {
        return false;
    }
    std::fs::write(target_dir.join(format!("{icon_id}.png")), out).is_ok()
}

fn download_vault_block_icons_sync(app: AppHandle) -> Result<VaultBlockIconDownloadOutput, String> {
    emit_vault_block_icon_progress(&app, 0, 1, 0, "正在连接到 ccvaults.com...");
    let client = vault_client()?;
    let home = client
        .get(VAULT_SITE_LABEL)
        .send()
        .map_err(|e| format!("无法连接 Vault 首页: {e}"))?;
    if !home.status().is_success() {
        return Err(format!("无法连接 Vault 首页: HTTP {}", home.status()));
    }
    let home_html = home.text().unwrap_or_default();

    emit_vault_block_icon_progress(&app, 0, 1, 0, "正在获取访问令牌...");
    let token = acquire_vault_token(&client, &home_html)?;

    emit_vault_block_icon_progress(&app, 0, 1, 0, "正在获取方块图标索引...");
    let blocks_payload = vault_fetch_json_with_auth(&client, VAULT_API_BLOCKS_PATH, &token)?;
    let mut entries = collect_vault_block_entries(&blocks_payload);
    if entries.is_empty() {
        let all_payload = vault_fetch_json_with_auth(&client, VAULT_API_ALL_ASSETS_PATH, &token)?;
        entries = collect_vault_block_entries_from_all(&all_payload);
    }
    if entries.is_empty() {
        return Err("未解析到任何方块图标索引".to_string());
    }

    let target_dir = current_user_config_dir()?.join(VAULT_BLOCK_ICON_ROOT_RELPATH);
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    let total = entries.len();
    emit_vault_block_icon_progress(&app, 0, total, 0, format!("准备并行下载 {total} 个图标..."));

    let worker_count = total.min(12).max(1);
    let mut buckets: Vec<Vec<(String, String)>> = (0..worker_count).map(|_| Vec::new()).collect();
    for (index, entry) in entries.into_iter().enumerate() {
        buckets[index % worker_count].push(entry);
    }

    let target_dir = Arc::new(target_dir);
    let (sender, receiver) = mpsc::channel::<(String, bool)>();
    let mut handles = Vec::new();
    for bucket in buckets {
        let sender = sender.clone();
        let client = client.clone();
        let target_dir = target_dir.clone();
        handles.push(std::thread::spawn(move || {
            for (block_id, file_url) in bucket {
                let ok = download_one_vault_block_icon(&client, &target_dir, &block_id, &file_url);
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

    Ok(VaultBlockIconDownloadOutput {
        total,
        downloaded,
        target_dir: target_dir.display().to_string(),
        root_relpath: VAULT_BLOCK_ICON_ROOT_RELPATH.to_string(),
    })
}

#[tauri::command]
async fn download_vault_block_icons(app: AppHandle) -> Result<VaultBlockIconDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(move || download_vault_block_icons_sync(app))
        .await
        .map_err(|e| e.to_string())?
}

fn download_vault_item_icons_sync(app: AppHandle) -> Result<VaultBlockIconDownloadOutput, String> {
    emit_vault_item_icon_progress(&app, 0, 1, 0, "正在连接到 ccvaults.com...");
    let client = vault_client()?;
    let home = client
        .get(VAULT_SITE_LABEL)
        .send()
        .map_err(|e| format!("无法连接 Vault 首页: {e}"))?;
    if !home.status().is_success() {
        return Err(format!("无法连接 Vault 首页: HTTP {}", home.status()));
    }
    let home_html = home.text().unwrap_or_default();

    emit_vault_item_icon_progress(&app, 0, 1, 0, "正在获取访问令牌...");
    let token = acquire_vault_token(&client, &home_html)?;

    emit_vault_item_icon_progress(&app, 0, 1, 0, "正在获取物品图标索引...");
    let items_payload = vault_fetch_json_with_auth(&client, VAULT_API_ITEMS_PATH, &token)?;
    let mut entries = collect_vault_item_entries(&items_payload);
    if entries.is_empty() {
        let all_payload = vault_fetch_json_with_auth(&client, VAULT_API_ALL_ASSETS_PATH, &token)?;
        entries = collect_vault_item_entries_from_all(&all_payload);
    }
    if entries.is_empty() {
        return Err("未解析到任何物品图标索引".to_string());
    }

    let target_dir = current_user_config_dir()?.join(VAULT_ITEM_ICON_ROOT_RELPATH);
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    let total = entries.len();
    emit_vault_item_icon_progress(&app, 0, total, 0, format!("准备并行下载 {total} 个图标..."));

    let worker_count = total.min(12).max(1);
    let mut buckets: Vec<Vec<(String, String)>> = (0..worker_count).map(|_| Vec::new()).collect();
    for (index, entry) in entries.into_iter().enumerate() {
        buckets[index % worker_count].push(entry);
    }

    let target_dir = Arc::new(target_dir);
    let (sender, receiver) = mpsc::channel::<(String, bool)>();
    let mut handles = Vec::new();
    for bucket in buckets {
        let sender = sender.clone();
        let client = client.clone();
        let target_dir = target_dir.clone();
        handles.push(std::thread::spawn(move || {
            for (item_id, file_url) in bucket {
                let ok = download_one_vault_item_icon(&client, &target_dir, &item_id, &file_url);
                let _ = sender.send((item_id, ok));
            }
        }));
    }
    drop(sender);

    let mut completed = 0;
    let mut downloaded = 0;
    for (item_id, ok) in receiver {
        completed += 1;
        if ok {
            downloaded += 1;
        }
        emit_vault_item_icon_progress(
            &app,
            completed,
            total,
            downloaded,
            format!("正在下载图标: {item_id} ({completed}/{total})，成功 {downloaded}"),
        );
    }
    for handle in handles {
        let _ = handle.join();
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

    Ok(VaultBlockIconDownloadOutput {
        total,
        downloaded,
        target_dir: target_dir.display().to_string(),
        root_relpath: VAULT_ITEM_ICON_ROOT_RELPATH.to_string(),
    })
}

#[tauri::command]
async fn download_vault_item_icons(app: AppHandle) -> Result<VaultBlockIconDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(move || download_vault_item_icons_sync(app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn ensure_builtin_block_icons_extracted(
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
async fn ensure_builtin_item_icons_extracted(
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

fn reden_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

fn reden_get_json(url: reqwest::Url) -> Result<serde_json::Value, String> {
    let response = reden_client()?
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "Litematica-BA desktop-nova/0.1",
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "RedenMC API failed: status={} body={}",
            status, text
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("RedenMC JSON parse failed: {e}; body={text}"))
}

fn reden_download_dir() -> Result<PathBuf, String> {
    let dir = data_root()?.join("reden").join("downloads");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn sanitize_reden_file_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = sanitized.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "reden-download.litematic".to_string()
    } else if trimmed.to_lowercase().ends_with(".litematic") {
        trimmed
    } else {
        format!("{trimmed}.litematic")
    }
}

fn file_name_from_headers_or_url(
    headers: &reqwest::header::HeaderMap,
    url: &reqwest::Url,
    fallback: &str,
) -> String {
    if let Some(value) = headers.get(reqwest::header::CONTENT_DISPOSITION) {
        if let Ok(text) = value.to_str() {
            for part in text.split(';') {
                let part = part.trim();
                if let Some(name) = part.strip_prefix("filename=") {
                    return sanitize_reden_file_name(name.trim_matches('"'));
                }
            }
        }
    }
    let from_url = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .unwrap_or(fallback);
    sanitize_reden_file_name(from_url)
}

fn reden_fetch_download(
    start_url: reqwest::Url,
    fallback_name: &str,
) -> Result<RedenDownloadOutput, String> {
    let client = reden_client()?;
    let mut url = start_url;
    for _ in 0..6 {
        let response = client
            .get(url.clone())
            .header(
                reqwest::header::USER_AGENT,
                "Litematica-BA desktop-nova/0.1",
            )
            .header(reqwest::header::REFERER, "https://redenmc.com/")
            .header(reqwest::header::ORIGIN, "https://redenmc.com")
            .send()
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if status.is_redirection() {
            let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
                return Err(format!(
                    "RedenMC download redirected without Location: status={status}"
                ));
            };
            let location = location.to_str().map_err(|e| e.to_string())?;
            url = url.join(location).map_err(|e| e.to_string())?;
            continue;
        }
        let final_url = url.clone();
        let headers = response.headers().clone();
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = response.bytes().map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "RedenMC download failed: status={} content-type={} bytes={}",
                status,
                content_type,
                bytes.len()
            ));
        }
        let looks_html = content_type.contains("text/html")
            || bytes.starts_with(b"<!DOCTYPE")
            || bytes.starts_with(b"<!doctype")
            || bytes.starts_with(b"<html");
        if looks_html {
            return Err(format!(
                "RedenMC returned an HTML/external-drive page instead of .litematic: url={} content-type={} bytes={}",
                final_url,
                content_type,
                bytes.len()
            ));
        }
        if bytes.is_empty() {
            return Err("RedenMC download returned an empty file.".to_string());
        }
        let file_name = file_name_from_headers_or_url(&headers, &final_url, fallback_name);
        let path = reden_download_dir()?.join(file_name);
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        return Ok(RedenDownloadOutput {
            path: path.display().to_string(),
            file_name: path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("reden-download.litematic")
                .to_string(),
            bytes: bytes.len(),
        });
    }
    Err("RedenMC download redirected too many times.".to_string())
}

#[tauri::command]
async fn reden_search_litematica(query: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = reqwest::Url::parse_with_params(
            "https://redenmc.com/api/mc-services/litematica/search",
            &[("q", query.as_str())],
        )
        .map_err(|e| e.to_string())?;
        reden_get_json(url)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reden_machine_detail(machine_id: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = reqwest::Url::parse(&format!(
            "https://redenmc.com/api/mc-services/yisibite/{}/info/en",
            machine_id
        ))
        .map_err(|e| e.to_string())?;
        reden_get_json(url)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reden_download_attachment(
    machine_id: String,
    attachment_index: u32,
) -> Result<RedenDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if attachment_index == 0 {
            return Err("RedenMC attachment index is 1-based; index 0 is invalid.".to_string());
        }
        let url = reqwest::Url::parse(&format!(
            "https://redenmc.com/api/mc-services/yisibite/{}/download/{}",
            machine_id, attachment_index
        ))
        .map_err(|e| e.to_string())?;
        reden_fetch_download(url, &format!("{machine_id}-{attachment_index}.litematic"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reden_download_parametric(
    machine_id: String,
    sizes: RedenSizesInput,
) -> Result<RedenDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut url = reqwest::Url::parse(&format!(
            "https://redenmc.com/api/mc-services/yisibite/{}",
            machine_id
        ))
        .map_err(|e| e.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(value) = sizes.x_size {
                pairs.append_pair("xSize", &value.to_string());
            }
            if let Some(value) = sizes.y_size {
                pairs.append_pair("ySize", &value.to_string());
            }
            if let Some(value) = sizes.z_size {
                pairs.append_pair("zSize", &value.to_string());
            }
        }
        reden_fetch_download(url, &format!("{machine_id}.litematic"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn ai_get_config() -> Result<AiPublicConfig, String> {
    Ok(public_ai_config(read_ai_config(), "loaded"))
}

#[tauri::command]
fn ai_save_config(input: AiSaveConfigInput) -> Result<AiPublicConfig, String> {
    let mut current = read_ai_config();
    current.provider = match input.provider.as_str() {
        "openai_compatible" => "openai_compatible".to_string(),
        "gemini_compatible" => "gemini_compatible".to_string(),
        _ => "mock".to_string(),
    };
    current.base_url = input.base_url.trim().trim_end_matches('/').to_string();
    current.model = input.model.trim().to_string();
    if let Some(api_key) = input.api_key {
        if !api_key.trim().is_empty() {
            current.api_key = Some(api_key.trim().to_string());
        }
    }
    write_ai_config(&current)?;
    Ok(public_ai_config(current, "saved"))
}

#[tauri::command]
fn ai_clear_key() -> Result<AiPublicConfig, String> {
    let mut current = read_ai_config();
    current.api_key = None;
    write_ai_config(&current)?;
    Ok(public_ai_config(current, "cleared"))
}

#[tauri::command]
fn ai_test_connection() -> Result<AiTestResult, String> {
    let config = read_ai_config();
    match config.provider.as_str() {
        "mock" => {
            if config.base_url.trim().starts_with("http")
                && config.api_key.as_ref().is_some_and(|key| !key.is_empty())
            {
                test_openai_models_endpoint(&config, "Mock provider /models probe")
            } else {
                Ok(AiTestResult {
                    ok: true,
                    message: "Mock provider OK.".to_string(),
                })
            }
        }
        "gemini_compatible" => Ok(AiTestResult {
            ok: false,
            message: "Gemini Compatible is reserved but not implemented yet.".to_string(),
        }),
        "openai_compatible" => test_openai_models_endpoint(&config, "OpenAI Compatible"),
        _ => Ok(AiTestResult {
            ok: false,
            message: "Unknown provider.".to_string(),
        }),
    }
}

fn test_openai_models_endpoint(
    config: &AiStoredConfig,
    label: &str,
) -> Result<AiTestResult, String> {
    if config.base_url.trim().is_empty() {
        return Ok(AiTestResult {
            ok: false,
            message: "Base URL is empty.".to_string(),
        });
    }
    let Some(api_key) = config.api_key.as_ref().filter(|key| !key.is_empty()) else {
        return Ok(AiTestResult {
            ok: false,
            message: "API Key is not saved.".to_string(),
        });
    };
    let url = format!("{}/models", config.base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Ok(AiTestResult {
            ok: false,
            message: format!(
                "{label} connection failed. GET /models status={status}. body={}",
                body.chars().take(600).collect::<String>()
            ),
        });
    }

    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(err) => {
            return Ok(AiTestResult {
                ok: false,
                message: format!(
                    "{label} returned non-JSON from GET /models. status={status}. error={err}"
                ),
            });
        }
    };
    let Some(models) = parsed.get("data").and_then(|data| data.as_array()) else {
        return Ok(AiTestResult {
            ok: false,
            message: format!("{label} returned JSON but no data[] model list. status={status}"),
        });
    };
    let configured_model_found = models
        .iter()
        .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
        .any(|id| id == config.model);

    Ok(AiTestResult {
        ok: true,
        message: format!(
            "{label} connection OK. GET /models status={status}. models={}. configured_model_found={configured_model_found}",
            models.len()
        ),
    })
}

#[tauri::command]
fn ai_chat_completion(input: AiChatCompletionInput) -> Result<AiChatCompletionOutput, String> {
    let config = read_ai_config();
    match config.provider.as_str() {
        "mock" => Ok(AiChatCompletionOutput {
            provider: "mock".to_string(),
            model: config.model,
            content: mock_ai_plan_response(),
        }),
        "gemini_compatible" => {
            Err("Gemini Compatible is reserved but not implemented yet.".to_string())
        }
        "openai_compatible" => openai_chat_completion(&config, input.messages),
        _ => Err("Unknown AI provider.".to_string()),
    }
}

fn openai_chat_completion(
    config: &AiStoredConfig,
    messages: Vec<AiChatMessage>,
) -> Result<AiChatCompletionOutput, String> {
    if config.base_url.trim().is_empty() {
        return Err("Base URL is empty.".to_string());
    }
    if config.model.trim().is_empty() {
        return Err("Model is empty.".to_string());
    }
    let Some(api_key) = config.api_key.as_ref().filter(|key| !key.is_empty()) else {
        return Err("API Key is not saved.".to_string());
    };
    if messages.is_empty() {
        return Err("messages is empty.".to_string());
    }
    let sanitized_messages = messages
        .into_iter()
        .map(|message| {
            let role = match message.role.as_str() {
                "system" | "assistant" | "user" => message.role,
                _ => "user".to_string(),
            };
            serde_json::json!({
                "role": role,
                "content": message.content,
            })
        })
        .collect::<Vec<_>>();
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": config.model,
        "messages": sanitized_messages,
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "POST /chat/completions failed. status={status}. body={}",
            text.chars().take(800).collect::<String>()
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("POST /chat/completions returned non-JSON. error={e}"))?;
    let content = value
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .ok_or_else(|| {
            "POST /chat/completions JSON missing choices[0].message.content".to_string()
        })?;
    Ok(AiChatCompletionOutput {
        provider: config.provider.clone(),
        model: config.model.clone(),
        content: content.to_string(),
    })
}

fn mock_ai_plan_response() -> String {
    r#"{
  "version": 1,
  "metadata": {
    "name": "Mock API Plan",
    "author": "Litematica-BA",
    "description": "Mock provider returned a local projection plan."
  },
  "minecraft_data_version": 3953,
  "regions": [
    {
      "name": "main",
      "origin": [0, 0, 0],
      "size": [24, 8, 24],
      "operations": [
        {
          "type": "floor",
          "from": [0, 0, 0],
          "to": [23, 0, 23],
          "block": {
            "name": "minecraft:stone_bricks",
            "properties": {}
          }
        },
        {
          "type": "outline_box",
          "from": [2, 1, 2],
          "to": [21, 5, 21],
          "block": {
            "name": "minecraft:oak_planks",
            "properties": {}
          }
        }
      ]
    }
  ]
}"#
    .to_string()
}

fn query_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn wiki_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Litematica-Nova-enum-catalog/1.0")
        .build()
        .map_err(|e| e.to_string())
}

fn fetch_wiki_html(client: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
        .send()
        .map_err(|e| format!("请求 {url} 失败: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("请求 {url} 失败: HTTP {status}"));
    }
    response.text().map_err(|e| e.to_string())
}

fn normalize_html_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn element_text(element: &ElementRef<'_>) -> String {
    normalize_html_text(&element.text().collect::<Vec<_>>().join(" "))
}

fn extract_code_values_from_cell(cell: &ElementRef<'_>, code_selector: &Selector) -> Vec<String> {
    cell.select(code_selector)
        .map(|code| normalize_html_text(&code.text().collect::<Vec<_>>().join(" ")))
        .map(|value| value.trim_matches('`').trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn extract_resource_locations_from_html(html: &str) -> Result<Vec<String>, String> {
    let table_selector = Selector::parse("table").map_err(|e| e.to_string())?;
    let row_selector = Selector::parse("tr").map_err(|e| e.to_string())?;
    let cell_selector = Selector::parse("th, td").map_err(|e| e.to_string())?;
    let code_selector = Selector::parse("code").map_err(|e| e.to_string())?;
    let document = Html::parse_document(html);
    let mut seen = HashSet::new();
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
            if let Some(column_index) = labels.iter().position(|label| label.contains("resource location")) {
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

fn extract_enchantment_section_html<'a>(html: &'a str) -> Option<&'a str> {
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
async fn download_minecraft_wiki_enum_catalogs() -> Result<WikiEnumCatalogDownloadOutput, String> {
    tauri::async_runtime::spawn_blocking(download_minecraft_wiki_enum_catalogs_sync)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_material_list_window(
    app: AppHandle,
    active_file: Option<String>,
) -> Result<(), String> {
    const LABEL: &str = "material-list";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        app.emit_to(LABEL, "material-list-open-file", active_file)
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    let url = match active_file
        .as_deref()
        .filter(|file| !file.trim().is_empty())
    {
        Some(file) => format!("material_list.html?file={}", query_encode(file)),
        None => "material_list.html".to_string(),
    };

    tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::App(url.into()))
        .title("Material List")
        .inner_size(720.0, 520.0)
        .min_inner_size(560.0, 420.0)
        .build()
        .map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
async fn open_enumerator_window(
    app: AppHandle,
    active_file: Option<String>,
) -> Result<(), String> {
    const LABEL: &str = "enumerator";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        app.emit_to(LABEL, "enumerator-open-file", active_file)
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    let url = match active_file.as_deref().filter(|file| !file.trim().is_empty()) {
        Some(file) => format!("enumerator.html?file={}", query_encode(file)),
        None => "enumerator.html".to_string(),
    };

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App(url.into()),
    )
    .title("Enumerator")
    .inner_size(1180.0, 760.0)
    .min_inner_size(900.0, 560.0)
    .build()
    .map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
async fn open_reden_library_window(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "reden-library";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("reden_library.html".into()),
    )
    .title("Online Projection Library")
    .inner_size(1080.0, 760.0)
    .min_inner_size(760.0, 560.0)
    .build()
    .map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
async fn open_local_library_folders_window(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "local-library-folders";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("local_library_folders.html".into()),
    )
    .title("Local Library Folders")
    .inner_size(920.0, 680.0)
    .min_inner_size(700.0, 520.0)
    .build()
    .map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
async fn open_ui_demo_window(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "ui-demo-window";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("demo_window.html".into()),
    )
    .title("UI Demo Window")
    .inner_size(720.0, 520.0)
    .min_inner_size(560.0, 420.0)
    .build()
    .map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
async fn open_asset_manager_window(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "asset-manager";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("asset_manager.html".into()),
    )
    .title("Game Asset Manager")
    .inner_size(1040.0, 700.0)
    .min_inner_size(760.0, 520.0)
    .build()
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(BuildState {
            child: None,
            progress_file: None,
            cache_file: None,
            stdout_file: None,
            stderr_file: None,
            started_at: None,
            last_progress_signature: None,
            last_progress_change: None,
        }))
        .manage(Mutex::new(EmbeddedViewerState::default()))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                if let Some(state) = window.try_state::<Mutex<EmbeddedViewerState>>() {
                    let mut st = state.lock().unwrap();
                    if let Some(mut record) = st.viewer.take() {
                        log_embed(&format!(
                            "status=app_close_cleanup child_hwnd={} process_id={}",
                            record.child_hwnd,
                            record.child.id()
                        ));
                        stop_embedded_record(&mut record);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            execute_backend,
            execute_backend_trace,
            start_native_viewer,
            start_embedded_viewer,
            update_embedded_viewer_bounds,
            stop_embedded_viewer,
            hide_embedded_viewer,
            show_embedded_viewer,
            get_embedded_viewer_status,
            render_preview_image,
            generate_preview_image,
            start_cache_build_task,
            kill_cache_build_task,
            poll_cache_build_task,
            get_user_config,
            save_user_config,
            choose_user_config_dir,
            open_user_config_dir,
            set_user_config_dir,
            reset_user_config_dir,
            read_user_config_file,
            write_user_config_file,
            get_user_config_file_path,
            read_file_string,
            write_file_string,
            write_text_file_absolute,
            check_file_exists,
            list_litematic_files_in_directory,
            list_directory_entries,
            list_litematic_file_entries_in_directory,
            get_workspace_root,
            get_path_info,
            read_image_base64,
            read_projection_preview_image,
            open_file_parent_dir,
            copy_file_to_directory,
            download_vault_block_icons,
            download_vault_item_icons,
            ensure_builtin_block_icons_extracted,
            ensure_builtin_item_icons_extracted,
            download_minecraft_wiki_enum_catalogs,
            open_workspace_path,
            cleanup_local_temp_files,
            reden_search_litematica,
            reden_machine_detail,
            reden_download_attachment,
            reden_download_parametric,
            ai_get_config,
            ai_save_config,
            ai_clear_key,
            ai_test_connection,
            ai_chat_completion,
            open_material_list_window,
            open_enumerator_window,
            open_reden_library_window,
            open_local_library_folders_window,
            open_asset_manager_window,
            open_ui_demo_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
