//! Shared product thumbnail cache. It accepts real GPU frames or embedded images,
//! never screenshots of UI or fabricated geometry. Embedded file data is authoritative.
use super::model::ProductPreviewImage;
use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;
static IMAGE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(crate) fn temporary_preview_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "nova-preview-{}-{}.png",
        std::process::id(),
        IMAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

pub(crate) fn cache_path(source: &Path) -> Result<PathBuf, String> {
    let name = source_cache_file_name(source)?;
    Ok(thumbnail_data_root()?.join("thumbnails").join(name))
}

#[derive(Clone, PartialEq, Eq)]
struct SourceStamp {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}
fn source_stamp(source: &Path) -> Result<SourceStamp, String> {
    let path = source.canonicalize().map_err(|e| e.to_string())?;
    let m = fs::metadata(&path).map_err(|e| e.to_string())?;
    Ok(SourceStamp {
        path,
        bytes: m.len(),
        modified: m.modified().map_err(|e| e.to_string())?,
    })
}
fn source_cache_file_name(source: &Path) -> Result<String, String> {
    static CACHE: OnceLock<Mutex<VecDeque<(SourceStamp, String)>>> = OnceLock::new();
    let stamp = source_stamp(source)?;
    let cache = CACHE.get_or_init(Default::default);
    if let Some((_, hash)) = cache
        .lock()
        .unwrap()
        .iter()
        .find(|(known, _)| known == &stamp)
    {
        return Ok(hash.clone());
    }
    let mut file = File::open(source).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&chunk[..count]);
    }
    if source_stamp(source)? != stamp {
        return Err("投影在内容身份读取期间发生变化".into());
    }
    let name = format!("{:x}.png", hash.finalize());
    let mut cache = cache.lock().unwrap();
    cache.retain(|(known, _)| known.path != stamp.path);
    while cache.len() >= 256 {
        cache.pop_front();
    }
    cache.push_back((stamp, name.clone()));
    Ok(name)
}

fn thumbnail_data_root() -> Result<PathBuf, String> {
    // Keep Explorer's historical cache. Unix uses the Host's resolved user root.
    #[cfg(windows)]
    {
        let root = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA缺失")?;
        Ok(PathBuf::from(root).join("Litematica-Nova"))
    }
    #[cfg(not(windows))]
    {
        let root = std::env::var_os("NOVA_USER_DATA_ROOT").ok_or("Nova用户数据目录尚未初始化")?;
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            return Err("Nova用户数据目录必须为绝对路径".into());
        }
        Ok(root)
    }
}

pub(crate) fn load(path: &Path) -> Result<ProductPreviewImage, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(ProductPreviewImage {
        preview_path: path.to_string_lossy().into(),
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
        ..Default::default()
    })
}

pub(crate) fn save(
    source: &Path,
    frame: &Path,
    expected: &Path,
) -> Result<ProductPreviewImage, String> {
    let name = source_cache_file_name(source)?;
    if expected.file_name() != Some(std::ffi::OsStr::new(&name)) {
        return Err("截图期间投影内容发生变化，旧帧未缓存".into());
    }
    let output = thumbnail_data_root()?.join("thumbnails").join(name);
    if output != expected {
        return Err("截图期间投影内容发生变化，旧帧未缓存".into());
    }
    let bytes = fs::read(frame).map_err(|error| error.to_string())?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(output.parent().ok_or("缩略图路径异常")?)
        .map_err(|error| error.to_string())?;
    let temp = output.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        IMAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    // Windows rename cannot replace an existing file. Publish complete bytes only.
    if output.exists() {
        fs::remove_file(&output).map_err(|error| error.to_string())?;
    }
    fs::rename(&temp, &output).map_err(|error| error.to_string())?;
    if frame != output {
        let _ = fs::remove_file(frame);
    }
    notify_shell(source);
    load(&output)
}

#[cfg(windows)]
pub(crate) fn notify_shell(source: &Path) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(
            event: i32,
            flags: u32,
            item: *const std::ffi::c_void,
            other: *const std::ffi::c_void,
        );
    }
    let path: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        SHChangeNotify(0x0000_2000, 0x0005, path.as_ptr().cast(), std::ptr::null());
    }
}
#[cfg(not(windows))]
pub(crate) fn notify_shell(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_key_is_content_based_not_name_based() {
        let root = std::env::temp_dir().join(format!("nova-thumbnail-key-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let a = root.join("甲.litematic");
        let b = root.join("乙.litematic");
        fs::write(&a, b"source").unwrap();
        fs::write(&b, b"source").unwrap();
        assert_eq!(
            source_cache_file_name(&a).unwrap(),
            source_cache_file_name(&b).unwrap()
        );
        fs::write(&b, b"changed").unwrap();
        assert_ne!(
            source_cache_file_name(&a).unwrap(),
            source_cache_file_name(&b).unwrap()
        );
        fs::remove_file(a).unwrap();
        fs::remove_file(b).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn temporary_names_are_unique_and_changed_sources_do_not_receive_old_frames() {
        assert_ne!(temporary_preview_path(), temporary_preview_path());
        let root =
            std::env::temp_dir().join(format!("nova-thumbnail-change-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("fixture.litematic");
        let frame = root.join("unit.png");
        fs::write(&source, b"unit-source-original").unwrap();
        let expected = root.join(source_cache_file_name(&source).unwrap());
        image::save_buffer_with_format(
            &frame,
            &[255, 0, 0, 255],
            1,
            1,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .unwrap();
        fs::write(&source, b"unit-source-changed").unwrap();
        assert!(
            save(&source, &frame, &expected)
                .unwrap_err()
                .contains("旧帧未缓存")
        );
        fs::remove_file(source).unwrap();
        fs::remove_file(frame).unwrap();
        fs::remove_dir(root).unwrap();
    }
}

// Product commands use the existing signed render paths. Generation creates a
// session draft; only the separate confirmation command may change source NBT.
use crate::*;
struct Draft {
    source: PathBuf,
    stamp: SourceStamp,
    source_id: String,
    frame: PathBuf,
    frame_hash: String,
    mode: String,
}
fn drafts() -> &'static Mutex<BTreeMap<String, Draft>> {
    static VALUE: OnceLock<Mutex<BTreeMap<String, Draft>>> = OnceLock::new();
    VALUE.get_or_init(Default::default)
}
struct Generation {
    cancelled: AtomicBool,
    paused: AtomicBool,
    notify: tokio::sync::Notify,
    fast: Mutex<Option<(Arc<QuickPreviewHost>, String)>>,
    protected: Mutex<Option<String>>,
}
fn generations() -> &'static Mutex<BTreeMap<String, Arc<Generation>>> {
    static VALUE: OnceLock<Mutex<BTreeMap<String, Arc<Generation>>>> = OnceLock::new();
    VALUE.get_or_init(Default::default)
}
struct GenerationGuard(String);
impl Drop for GenerationGuard {
    fn drop(&mut self) {
        generations().lock().unwrap().remove(&self.0);
    }
}
fn begin_generation(
    id: Option<String>,
) -> Result<(String, Arc<Generation>, GenerationGuard), String> {
    let id = id.unwrap_or_else(|| {
        format!(
            "thumbnail-{}-{}",
            std::process::id(),
            IMAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        )
    });
    if id.is_empty() || id.len() > 160 {
        return Err("预览任务标识无效".into());
    }
    let state = Arc::new(Generation {
        cancelled: AtomicBool::new(false),
        paused: AtomicBool::new(false),
        notify: tokio::sync::Notify::new(),
        fast: Mutex::new(None),
        protected: Mutex::new(None),
    });
    let mut active = generations().lock().unwrap();
    if active.contains_key(&id) {
        return Err("预览任务已经存在".into());
    }
    active.insert(id.clone(), state.clone());
    Ok((id.clone(), state, GenerationGuard(id)))
}
fn generation_gate() -> &'static tokio::sync::Semaphore {
    static GATE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Semaphore::new(1))
}
fn check_generation(control: &Generation) -> Result<(), String> {
    if control.cancelled.load(Ordering::Acquire) {
        Err("预览生成已取消".into())
    } else {
        Ok(())
    }
}
fn selected_mode(value: Option<String>) -> Result<String, String> {
    let value = match value {
        Some(value) => value,
        None => read_user_config_data(&current_user_config_dir()?).preview_mode,
    };
    match value.as_str() {
        "fast" | "fast_experimental" => Ok("fast_experimental".into()),
        "normal" | "standard" => Ok("standard".into()),
        "full" => Ok(value),
        _ => Err("未知预览生成模式".into()),
    }
}
fn stage_preview(
    source: &Path,
    frame: &Path,
    expected: &Path,
    mode: &str,
) -> Result<ProductPreviewImage, String> {
    if cache_path(source)? != expected {
        return Err("生成期间投影内容发生变化，旧帧未暂存".into());
    }
    let stamp = source_stamp(source)?;
    let bytes = fs::read(frame).map_err(|e| e.to_string())?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let id = format!(
        "draft-{}-{}",
        std::process::id(),
        IMAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let directory = current_user_config_dir()?.join("tmp/preview-drafts");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = directory.join(format!("{id}.png"));
    fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    let source_id = expected
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or("预览内容身份无效")?
        .to_owned();
    let mut draft = drafts().lock().unwrap();
    while draft.len() >= 64 {
        if let Some(old) = draft.keys().next().cloned() {
            if let Some(old) = draft.remove(&old) {
                let _ = fs::remove_file(old.frame);
            }
        }
    }
    draft.insert(
        id.clone(),
        Draft {
            source: stamp.path.clone(),
            stamp,
            source_id: source_id.clone(),
            frame: path.clone(),
            frame_hash: format!("{:x}", Sha256::digest(&bytes)),
            mode: mode.into(),
        },
    );
    if frame != path {
        let _ = fs::remove_file(frame);
    }
    Ok(ProductPreviewImage {
        preview_path: path.to_string_lossy().into(),
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
        draft_id: Some(id),
        source_identity: Some(source_id),
        display_mode: Some(mode.into()),
        can_embed: source
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("litematic")),
        persisted: false,
    })
}

#[tauri::command]
pub(crate) async fn save_generated_preview(
    file_path: String,
    draft_id: String,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProductPreviewImage, String> {
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = fs::canonicalize(file_path).map_err(|e| e.to_string())?;
        let (frame, mode, source_identity) = {
            let known = drafts().lock().unwrap();
            let draft = known.get(&draft_id).ok_or("预览草稿已失效，请重新生成")?;
            if source != draft.source
                || source_stamp(&source)? != draft.stamp
                || source_cache_file_name(&source)? != format!("{}.png", draft.source_id)
            {
                return Err("投影已改变，旧预览未写入源文件".into());
            }
            if !source
                .extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("litematic"))
            {
                return Err("此格式不支持写入内嵌预览；生成结果仅为暂存派生图".into());
            }
            let bytes = fs::read(&draft.frame).map_err(|e| e.to_string())?;
            if format!("{:x}", Sha256::digest(&bytes)) != draft.frame_hash {
                return Err("预览草稿内容已变化，请重新生成".into());
            }
            (draft.frame.clone(), draft.mode.clone(), draft.source_id.clone())
        };
        operations.edit_project_metadata(EditProjectMetadataInput {
            source_path: source.to_string_lossy().into(),
            patch: serde_json::json!({"preview_image_path":frame,"preview_image_mode":mode,"expected_source_sha256":source_identity}),
            output_path: None,
        })?;
        let embedded = operations
            .read_embedded_preview(InspectProjectInput {
                source_path: source.to_string_lossy().into(),
            })?
            .ok_or("内嵌预览保存后未能重新读取")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(
                embedded
                    .data_url
                    .split_once(',')
                    .ok_or("投影预览编码错误")?
                    .1,
            )
            .map_err(|e| e.to_string())?;
        let temporary = temporary_preview_path();
        fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        let expected = cache_path(&source)?;
        let mut result = save(&source, &temporary, &expected)
            .map_err(|e| format!("内嵌预览已保存，但派生缓存更新失败：{e}"))?;
        let _ = fs::remove_file(&temporary);
        let _ = fs::remove_file(&frame);
        drafts().lock().unwrap().remove(&draft_id);
        result.source_identity = expected
            .file_stem()
            .and_then(|v| v.to_str())
            .map(str::to_owned);
        result.display_mode = Some(mode);
        result.can_embed = true;
        result.persisted = true;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn discard_generated_preview(draft_id: String) -> Result<(), String> {
    let mut known = drafts().lock().map_err(|_| "预览草稿锁异常")?;
    if let Some(draft) = known.get(&draft_id) {
        if draft.frame.is_file() {
            fs::remove_file(&draft.frame).map_err(|e| e.to_string())?;
        }
    }
    known.remove(&draft_id);
    Ok(())
}
#[tauri::command]
pub(crate) fn cancel_preview_generation(
    request_id: String,
    action: Option<String>,
    facade: tauri::State<'_, RenderFacade>,
) -> Result<(), String> {
    let control = generations()
        .lock()
        .map_err(|_| "预览任务锁异常")?
        .get(&request_id)
        .cloned();
    let Some(control) = control else {
        return Ok(());
    };
    match action.as_deref().unwrap_or("cancel") {
        "pause" => {
            control.paused.store(true, Ordering::Release);
            return Ok(());
        }
        "resume" => {
            control.paused.store(false, Ordering::Release);
            control.notify.notify_one();
            return Ok(());
        }
        "cancel" => {}
        _ => return Err("未知预览任务控制操作".into()),
    }
    control.cancelled.store(true, Ordering::Release);
    control.notify.notify_one();
    if let Some((host, id)) = control.fast.lock().unwrap().as_ref() {
        host.close(id)?;
    }
    if let Some(id) = control.protected.lock().unwrap().as_ref() {
        facade.cancel_viewer_preview(id)?;
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn render_preview_image(
    file_path: String,
    display_mode: String,
    cache_input: Option<String>,
    operation_id: Option<String>,
    facade: tauri::State<'_, RenderFacade>,
    host: tauri::State<'_, QuickPreviewHost>,
) -> Result<ProductPreviewImage, String> {
    let _ = cache_input;
    let source = fs::canonicalize(file_path).map_err(|e| e.to_string())?;
    let expected = cache_path(&source)?;
    let mode = selected_mode(Some(display_mode))?;
    if mode == "fast_experimental" {
        let id = operation_id.ok_or("请先启动该投影的快速渲染")?;
        let host = host.inner().clone();
        return tauri::async_runtime::spawn_blocking(move || {
            let view = host
                .snapshot()?
                .filter(|view| {
                    view.source_path == source && view.state == "ready" && view.preview_id == id
                })
                .ok_or("请先启动该投影的快速渲染")?;
            let (source, frame) = host.capture_frame(&view.preview_id)?;
            stage_preview(&source, &frame, &expected, &mode)
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let display = if mode == "standard" {
        RenderDisplayMode::Standard
    } else {
        RenderDisplayMode::Full
    };
    let id = operation_id.ok_or("请先启动对应渲染场景")?;
    let frame = facade
        .capture_frame(&OperationId(id), &source, display)
        .await?;
    tauri::async_runtime::spawn_blocking(move || stage_preview(&source, &frame, &expected, &mode))
        .await
        .map_err(|e| e.to_string())?
}
async fn generate_one(
    file_path: String,
    mode: String,
    facade: &RenderFacade,
    host: &QuickPreviewHost,
    window: &tauri::WebviewWindow,
    control: Arc<Generation>,
) -> Result<ProductPreviewImage, String> {
    check_generation(&control)?;
    let source = fs::canonicalize(file_path).map_err(|e| e.to_string())?;
    let expected = cache_path(&source)?;
    if mode == "fast_experimental" {
        let parent = platform::native_preview_host::host_window(window)?;
        let graphics_backend = read_user_config_data(&current_user_config_dir()?).graphics_backend;
        let temporary = Arc::new(host.thumbnail_session());
        return tauri::async_runtime::spawn_blocking(move || {
            check_generation(&control)?;
            let view = temporary.start(
                StartQuickPreviewInput {
                    composition_editor_path: None,
                    source_path: source,
                    preload: false,
                    click_started_at_epoch_ms: None,
                    graphics_backend,
                },
                parent,
            )?;
            *control.fast.lock().unwrap() = Some((temporary.clone(), view.preview_id.clone()));
            let output = (|| {
                loop {
                    check_generation(&control)?;
                    let current = temporary.snapshot()?.ok_or("缩略图预览会话结束")?;
                    if let Some(error) = current.error {
                        return Err(error);
                    }
                    if current.state == "ready" {
                        break;
                    }
                    if matches!(current.state, "failed" | "closed") {
                        return Err("缩略图预览准备未完成".into());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let (source, frame) = temporary.capture_frame(&view.preview_id)?;
                check_generation(&control)?;
                stage_preview(&source, &frame, &expected, &mode)
            })();
            let closed = temporary.close(&view.preview_id);
            *control.fast.lock().unwrap() = None;
            match (output, closed) {
                (Ok(result), Ok(())) => Ok(result),
                (Err(error), _) => Err(error),
                (_, Err(error)) => Err(error),
            }
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let launch = prepare_viewer_launch_context(CompatibilityScope::Render, mode.clone())
        .await?
        .with_display_mode(&mode)?;
    check_generation(&control)?;
    let output = temporary_preview_path();
    let contract = launch.prepare_preview(source.clone(), output.clone(), None)?;
    *control.protected.lock().unwrap() = Some(contract.request_id().into());
    // Publishing the identity precedes this check; cancel-before-spawn is also
    // recorded by the Host client and cannot escape into a late child launch.
    if let Err(error) = check_generation(&control) {
        let _ = facade.cancel_viewer_preview(contract.request_id());
        return Err(error);
    }
    let rendered = facade.run_viewer_preview(&launch, &contract, output).await;
    *control.protected.lock().unwrap() = None;
    check_generation(&control)?;
    let rendered = rendered?;
    tauri::async_runtime::spawn_blocking(move || {
        stage_preview(&source, Path::new(&rendered.preview_path), &expected, &mode)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn generate_preview_image(
    file_path: String,
    display_mode: Option<String>,
    request_id: Option<String>,
    facade: tauri::State<'_, RenderFacade>,
    window: tauri::WebviewWindow,
    host: tauri::State<'_, QuickPreviewHost>,
) -> Result<ProductPreviewImage, String> {
    let (_, control, _guard) = begin_generation(request_id)?;
    let _permit = generation_gate()
        .acquire()
        .await
        .map_err(|e| e.to_string())?;
    generate_one(
        file_path,
        selected_mode(display_mode)?,
        &facade,
        &host,
        &window,
        control,
    )
    .await
}
#[derive(Serialize)]
pub(crate) struct BatchItem {
    file_path: String,
    image: Option<ProductPreviewImage>,
    error: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct BatchOutput {
    items: Vec<BatchItem>,
    cancelled: bool,
}
#[tauri::command]
pub(crate) async fn generate_preview_images(
    file_paths: Vec<String>,
    display_mode: Option<String>,
    request_id: String,
    facade: tauri::State<'_, RenderFacade>,
    window: tauri::WebviewWindow,
    host: tauri::State<'_, QuickPreviewHost>,
) -> Result<BatchOutput, String> {
    let (id, control, _guard) = begin_generation(Some(request_id))?;
    let _permit = generation_gate()
        .acquire()
        .await
        .map_err(|e| e.to_string())?;
    let mode = selected_mode(display_mode)?;
    let total = file_paths.len();
    let mut items = Vec::new();
    for path in file_paths {
        while control.paused.load(Ordering::Acquire) && !control.cancelled.load(Ordering::Acquire) {
            control.notify.notified().await;
        }
        if check_generation(&control).is_err() {
            break;
        }
        let result = generate_one(
            path.clone(),
            mode.clone(),
            &facade,
            &host,
            &window,
            control.clone(),
        )
        .await;
        if control.cancelled.load(Ordering::Acquire) {
            break;
        }
        let item = match result {
            Ok(image) => BatchItem {
                file_path: path,
                image: Some(image),
                error: None,
            },
            Err(error) => BatchItem {
                file_path: path,
                image: None,
                error: Some(error),
            },
        };
        items.push(item);
        let _=window.emit("nova://thumbnail-progress",serde_json::json!({"requestId":id,"completed":items.len(),"total":total,"filePath":items.last().unwrap().file_path}));
    }
    Ok(BatchOutput {
        items,
        cancelled: control.cancelled.load(Ordering::Acquire),
    })
}
