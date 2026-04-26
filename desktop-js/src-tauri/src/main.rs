#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Manager, State, Window};

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
        if candidate.ends_with("desktop-js") {
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

fn render_tmp_path(prefix: &str, ext: &str) -> Result<PathBuf, String> {
    let dir = get_root().join(".tmp").join("desktop-js").join("render");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    Ok(dir.join(format!("{prefix}_{now}_{:x}.{ext}", std::process::id())))
}

fn app_config_dir() -> Result<PathBuf, String> {
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| get_root().join(".tmp").join("app-config"));
    let dir = base.join("Litematica-BA");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn ai_config_path() -> Result<PathBuf, String> {
    Ok(app_config_dir()?.join("ai_config.json"))
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

#[cfg(windows)]
fn show_embedded_host(hwnd: isize, show: bool) {
    unsafe {
        let _ = ShowWindow(hwnd as HWND, if show { SW_SHOW } else { SW_HIDE });
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

#[tauri::command]
fn start_native_viewer(file_path: String, display_mode: String) -> Result<(), String> {
    let current_dir = get_root();
    let exe_path = backend_exe_path("litematica_native_viewer.exe");
    let mode = normalized_display_mode(&display_mode);

    Command::new(exe_path)
        .arg(file_path)
        .arg(format!("--display-mode={mode}"))
        .arg("--basic-lighting")
        .arg("--basic-shadows")
        .current_dir(&current_dir)
        .spawn()
        .map_err(|e| e.to_string())?;
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
) -> Result<EmbeddedViewerStatus, String> {
    let current_dir = get_root();
    let exe_path = backend_exe_path("litematica_native_viewer.exe");
    let mode = normalized_display_mode(&display_mode);
    let purpose = normalized_embed_purpose(&purpose);
    let preview_mode = purpose == "properties_preview";
    let preview_spin = purpose == "properties_preview";
    let interactive = purpose == "render_interactive";
    let resolved_input = normalized_input_path(&file_path);
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
    let record = EmbeddedViewerRecord {
        child,
        child_hwnd,
        parent_hwnd,
        purpose: purpose.to_string(),
        file_path: resolved_input.display().to_string(),
        display_mode: mode.to_string(),
        bounds: rect,
        visible: true,
        stdout_file,
        stderr_file,
    };
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
                let mut old = st.viewer.take().unwrap();
                #[cfg(windows)]
                unsafe {
                    let _ = DestroyWindow(old.child_hwnd as HWND);
                }
                let _ = old.child.wait();
                return embedded_status_from(None, "exited", error);
            }
            Ok(None) => return embedded_status_from(Some(record), "running", None),
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
fn render_preview_image(
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
    let completed = Command::new(exe_path)
        .arg(file_path)
        .arg(format!("--display-mode={mode}"))
        .arg(format!("--preview-output={}", output_path.display()))
        .arg("--auto-exit-seconds=4")
        .arg("--basic-lighting")
        .arg("--basic-shadows")
        .current_dir(&current_dir)
        .output()
        .map_err(|e| e.to_string())?;
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
    Ok(())
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
    if let Some(child) = st.child.as_mut() {
        match child.try_wait() {
            Ok(None) => running = true,
            Ok(Some(status)) => {
                exit_code = status.code();
                st.child = None;
            }
            Err(_) => {
                running = false;
            }
        }
    }
    let progress_json = progress_file
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok());
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
fn check_file_exists(path: String) -> bool {
    get_root().join(path).is_file()
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
    if let Ok(bytes) = std::fs::read(get_root().join(path)) {
        Ok(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        ))
    } else {
        Err("Not found".into())
    }
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
fn cleanup_js_temp_files() -> Result<String, String> {
    let dir = get_root().join(".tmp").join("desktop-js");
    if !dir.exists() {
        return Ok("No desktop-js temp directory exists.".to_string());
    }
    let mut removed = 0usize;
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
    Ok(format!("Removed {removed} desktop-js temp entries."))
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

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(BuildState {
            child: None,
            progress_file: None,
            cache_file: None,
            stdout_file: None,
            stderr_file: None,
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
            start_cache_build_task,
            kill_cache_build_task,
            poll_cache_build_task,
            read_file_string,
            write_file_string,
            check_file_exists,
            get_workspace_root,
            get_path_info,
            read_image_base64,
            open_workspace_path,
            cleanup_js_temp_files,
            ai_get_config,
            ai_save_config,
            ai_clear_key,
            ai_test_connection,
            ai_chat_completion
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
