#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod application;
mod platform;
mod viewer_backend;
mod viewer_launch_contract;

use application::model::ProductPreviewImage;
use application::operations::{
    ApplicationOperations, EditProjectMetadataInput, EditProjectOriginInput, EmbeddedPreview, GenerateProjectInput,
    GenerateProjectResult, InspectProjectInput, ProjectMaterialsInput, ProjectMutationResult,
    ProjectStatsInput, ReplaceBlocksInput, ReplaceBlocksResult, RuntimeHealth,
    LayerContainerData, LayerSliceData, LayerSliceMeta, ProjectionMarkerEditSummary,
    ProjectionMarkers, ReadLayerContainerInput, ReadLayerInput,
    ExportMaterialListInput, MaterialRecipeCacheInput,
};
use application::quick_preview::{QuickPreviewHost, QuickPreviewView, StartQuickPreviewInput, QuickPresentationIntent};
use application::updater::{UpdateHost, application_update_snapshot, application_update_check,
    application_update_download, application_update_install, application_update_cancel, application_update_skip, application_update_restart};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

static APP_RESOURCE_ROOT: OnceLock<PathBuf> = OnceLock::new();
static APP_LOCAL_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();
const PRODUCT_DATA_DIRECTORY: &str = "Litematica-Nova";

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
struct CopyFileToDirectoryOutput {
    target_path: String,
    overwritten: bool,
    bytes_copied: u64,
}

#[derive(Deserialize, Serialize, Clone)]
struct UserConfig {
    theme: String,
    #[serde(default = "default_color_mode")]
    color_mode: String,
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
    color_mode: Option<String>,
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
    if let Some(resource_root) = APP_RESOURCE_ROOT.get() {
        if resource_root.join("bin").join("public-beta").is_dir() {
            return resource_root.clone();
        }
    }
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    find_workspace_root(&current_dir)
        .or_else(|| find_workspace_root(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .unwrap_or(current_dir)
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join("bin").join("public-beta").is_dir() {
            return Some(candidate.to_path_buf());
        }
        if candidate.ends_with("src-tauri") {
            let maybe_root = candidate.parent()?.parent()?;
            if maybe_root.join("bin").join("public-beta").is_dir() {
                return Some(maybe_root.to_path_buf());
            }
        }
        if candidate.ends_with("desktop-nova") {
            let maybe_root = candidate.parent()?;
            if maybe_root.join("bin").join("public-beta").is_dir() {
                return Some(maybe_root.to_path_buf());
            }
        }
    }
    None
}

fn backend_exe_path(binary_name: &str) -> PathBuf {
    get_root()
        .join("bin")
        .join("public-beta")
        .join(binary_name)
}

fn required_backend_exe_path(binary_name: &str) -> Result<PathBuf, String> {
    if !matches!(binary_name, "litematica_core" | "litematica_native_viewer") {
        return Err(format!("不允许的后端可执行文件名 {binary_name:?}；Beta 公共版仅包含统计核心与快速预览。"));
    }
    let path = backend_exe_path(&format!("{binary_name}{}", std::env::consts::EXE_SUFFIX));
    if path.is_file() { return Ok(path); }
    Err(format!("缺少公共版后端可执行文件：{}。", path.display()))
}

#[cfg(test)]
mod backend_executable_path_tests {
    use super::required_backend_exe_path;

    #[test]
    fn backend_binary_names_are_an_exact_allowlist() {
        for alias in [
            r".\litematica_native_viewer.exe",
            "LITEMATICA_NATIVE_VIEWER.EXE",
            r"sub\..\litematica_native_viewer.exe",
            "../litematica_core.exe",
        ] {
            let error = required_backend_exe_path(alias).unwrap_err();
            assert!(error.contains("不允许的后端可执行文件名"));
        }
    }
}

#[cfg(test)]
mod product_runtime_path_tests {
    use super::{product_local_data_root, PRODUCT_DATA_DIRECTORY};
    use std::path::Path;

    #[test]
    fn product_runtime_root_uses_the_formal_product_directory() {
        let local_data = Path::new(r"C:\Users\viewer\AppData\Local");
        assert_eq!(
            product_local_data_root(local_data),
            local_data.join("Litematica-Nova")
        );
        assert_eq!(PRODUCT_DATA_DIRECTORY, "Litematica-Nova");
    }
}

fn data_root() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
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
    #[cfg(not(debug_assertions))]
    let dir = APP_LOCAL_DATA_ROOT.get().cloned().ok_or_else(|| {
        "Litematica-Nova local data directory is not initialized; refusing to write beside the installed executable"
            .to_owned()
    })?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn product_local_data_root(local_data_dir: &Path) -> PathBuf {
    local_data_dir.join(PRODUCT_DATA_DIRECTORY)
}

fn legacy_appdata_config_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(2);
    if let Some(base) = env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(base).join("com.litematicaba.nova"));
    }
    if let Some(base) = env::var_os("APPDATA").filter(|value| !value.is_empty()) {
        dirs.push(
            PathBuf::from(base)
                .join("Litematica-BA")
                .join("desktop-nova"),
        );
    }
    dirs
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

/// These editable defaults belong to the selected user directory, not to the
/// executable directory. Fill missing files without replacing custom content.
fn seed_flake_resources(resource_root: &Path, user_root: &Path) -> Result<(), String> {
    for name in ["enumerator", "flake"] {
        let source = resource_root.join("pack-in").join(name);
        if !source.is_dir() {
            return Err(format!("Missing bundled Flake resources: {}", source.display()));
        }
        copy_dir_files_if_missing(&source, &user_root.join(name))?;
    }
    Ok(())
}

fn initialize_flake_user_resources() -> Result<(), String> {
    // A Release must use its distributed resources, never the build machine's
    // CARGO_MANIFEST_DIR or a coincidental source checkout in the working dir.
    #[cfg(not(debug_assertions))]
    let resource_root = APP_RESOURCE_ROOT.get().cloned()
        .ok_or_else(|| "Application resource directory is not initialized".to_owned())?;
    #[cfg(debug_assertions)]
    let resource_root = get_root();
    seed_flake_resources(&resource_root, &current_user_config_dir()?)
}

#[cfg(test)]
mod flake_resource_tests {
    use super::seed_flake_resources;
    use std::fs;
    use std::path::PathBuf;

    fn fixture_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "nova-flake-seed-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(root.join("release/pack-in/enumerator/system_enum")).unwrap();
        fs::create_dir_all(root.join("release/pack-in/flake/state_hint")).unwrap();
        fs::write(root.join("release/pack-in/enumerator/system_enum/waterlogged.json"), b"[\"minecraft:stairs\"]").unwrap();
        fs::write(root.join("release/pack-in/flake/state_hint/waterlogged.png"), b"bundled image").unwrap();
        root
    }

    fn remove_fixture(root: PathBuf) {
        let absolute = fs::canonicalize(&root).unwrap();
        assert!(absolute.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("nova-flake-seed-"));
        fs::remove_dir_all(absolute).unwrap();
    }

    #[test]
    fn flake_defaults_initialize_selected_directory_and_preserve_custom_files() {
        let root = fixture_root();
        let resources = root.join("release");
        let user = root.join("selected-user-data");
        seed_flake_resources(&resources, &user).unwrap();
        let catalog = user.join("enumerator/system_enum/waterlogged.json");
        let image = user.join("flake/state_hint/waterlogged.png");
        assert_eq!(fs::read(&catalog).unwrap(), b"[\"minecraft:stairs\"]");
        assert_eq!(fs::read(&image).unwrap(), b"bundled image");
        fs::write(&catalog, b"user catalog").unwrap();
        fs::write(&image, b"user image").unwrap();
        fs::write(resources.join("pack-in/flake/state_hint/new.png"), b"new default").unwrap();
        seed_flake_resources(&resources, &user).unwrap();
        assert_eq!(fs::read(&catalog).unwrap(), b"user catalog");
        assert_eq!(fs::read(&image).unwrap(), b"user image");
        assert_eq!(fs::read(user.join("flake/state_hint/new.png")).unwrap(), b"new default");
        let second_user = root.join("new-selected-user-data");
        seed_flake_resources(&resources, &second_user).unwrap();
        assert_eq!(fs::read(second_user.join("flake/state_hint/new.png")).unwrap(), b"new default");
        remove_fixture(root);
    }

    #[test]
    fn flake_missing_distributed_resources_are_not_silently_accepted() {
        let root = fixture_root();
        assert!(seed_flake_resources(&root.join("not-a-release"), &root.join("user")).is_err());
        remove_fixture(root);
    }

    #[test]
    fn flake_bundle_resources_are_explicit_and_do_not_include_private_archives() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let resources = config["bundle"]["resources"].as_object().unwrap();
        assert_eq!(resources.get("../../pack-in/enumerator").unwrap(), "pack-in/enumerator");
        assert_eq!(resources.get("../../pack-in/flake").unwrap(), "pack-in/flake");
        assert_eq!(resources.get("../../pack-in/lang/zh_cn.json").unwrap(), "pack-in/lang/zh_cn.json");
        assert_eq!(resources.get("gen/block-icons").unwrap(), "block");
        assert!(!resources.keys().any(|key| key == "../../pack-in" || key.contains("arr-private")));
    }
}

fn migrate_legacy_appdata_if_needed(target_root: &Path) -> Result<(), String> {
    for legacy_root in legacy_appdata_config_dirs() {
        if legacy_root == target_root || !legacy_root.is_dir() {
            continue;
        }
        for file in ["config.json", "config_dir.json", "ai_config.json"] {
            copy_file_if_missing(&legacy_root.join(file), &target_root.join(file))?;
        }
        for dir in ["projection-library", "generation-templates/custom"] {
            copy_dir_files_if_missing(&legacy_root.join(dir), &target_root.join(dir))?;
        }
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
        theme: "shadcn".to_string(),
        color_mode: default_color_mode(),
        render_display_mode: "fast_experimental".to_string(),
        preview_mode: "fast_experimental".to_string(),
        material_list_window_behavior: default_material_list_window_behavior(),
        show_ui_test_page: default_show_ui_test_page(),
        local_library_tail_path_count: default_local_library_tail_path_count(),
    }
}

#[cfg(test)]
mod public_user_config_tests {
    use super::UserConfig;
    #[test]
    fn legacy_private_settings_are_ignored_not_persisted() {
        let config = serde_json::from_value::<UserConfig>(serde_json::json!({
            "theme":"shadcn", "render_display_mode":"full", "preview_mode":"full",
            "v5_compatibility": {"shaderpack":"private.zip"},
            "viewer_controls": {"allow_weather_modification":true}
        })).unwrap();
        let serialized = serde_json::to_value(config).unwrap();
        assert!(serialized.get("v5_compatibility").is_none());
        assert!(serialized.get("viewer_controls").is_none());
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
        "8bitcn" => "8bitcn".to_string(),
        "minecraft" => "Minecraft".to_string(),
        _ => "shadcn".to_string(),
    }
}

fn default_color_mode() -> String {
    "system".to_string()
}

fn normalize_color_mode(value: &str) -> String {
    match value {
        "light" | "dark" => value.to_string(),
        _ => default_color_mode(),
    }
}

#[cfg(test)]
mod theme_config_tests {
    use super::{default_user_config, normalize_color_mode, normalize_theme, UserConfig};

    #[test]
    fn appearance_preference_defaults_to_system_and_round_trips() {
        let mut legacy = serde_json::to_value(default_user_config()).unwrap();
        legacy.as_object_mut().unwrap().remove("color_mode");
        assert_eq!(
            serde_json::from_value::<UserConfig>(legacy).unwrap().color_mode,
            "system"
        );
        for mode in ["light", "dark", "system"] {
            assert_eq!(normalize_color_mode(mode), mode);
            let mut config = default_user_config();
            config.color_mode = mode.to_string();
            let restored: UserConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(restored.color_mode, mode);
        }
        assert_eq!(normalize_color_mode("unknown"), "system");
    }

    #[test]
    fn three_theme_ids_are_canonical_and_legacy_values_migrate() {
        for value in ["", "WebDefault", "Bootstrap5", "Metro10", "QTDefault", "unknown"] {
            assert_eq!(normalize_theme(value), "shadcn");
        }
        assert_eq!(normalize_theme(" SHADCN "), "shadcn");
        assert_eq!(normalize_theme(" 8BITCN "), "8bitcn");
        assert_eq!(normalize_theme(" minecraft "), "Minecraft");
        assert_eq!(default_user_config().theme, "shadcn");
    }
}

fn normalize_mode_string(value: &str) -> String {
    let _ = value;
    "fast_experimental".to_string()
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
    config.color_mode = normalize_color_mode(&config.color_mode);
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

fn normalized_input_path(file_path: &str) -> PathBuf {
    let input = PathBuf::from(file_path);
    if input.is_absolute() {
        input
    } else {
        get_root().join(input)
    }
}

async fn run_application_operation<T>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("Application operation task failed: {error}"))?
}

#[tauri::command]
async fn application_inspect_project(
    input: InspectProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.inspect_project(input)).await
}

#[tauri::command]
async fn application_edit_project_metadata(
    input: EditProjectMetadataInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProjectMutationResult, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.edit_project_metadata(input)).await
}

#[tauri::command]
async fn application_read_layer_meta(
    input: InspectProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<LayerSliceMeta, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_layer_meta(input)).await
}

#[tauri::command]
async fn application_read_layer(
    input: ReadLayerInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<LayerSliceData, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_layer(input)).await
}

#[tauri::command]
async fn application_read_layer_container(
    input: ReadLayerContainerInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<Option<LayerContainerData>, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_layer_container(input)).await
}

#[tauri::command]
async fn application_read_project_markers(
    input: InspectProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProjectionMarkers, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_project_markers(input)).await
}

#[tauri::command]
async fn application_edit_project_origin(
    input: EditProjectOriginInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ProjectionMarkerEditSummary, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.edit_project_origin(input)).await
}

#[tauri::command]
async fn application_replace_blocks(
    input: ReplaceBlocksInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<ReplaceBlocksResult, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.replace_blocks(input)).await
}

#[tauri::command]
async fn application_get_project_stats(
    input: ProjectStatsInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.project_stats(input)).await
}

#[tauri::command]
async fn application_get_project_materials(
    input: ProjectMaterialsInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.project_materials(input)).await
}

#[tauri::command]
async fn application_generate_project(
    input: GenerateProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<GenerateProjectResult, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.generate_project(input)).await
}

#[tauri::command]
async fn application_runtime_health(
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<RuntimeHealth, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || Ok(operations.runtime_health())).await
}

#[tauri::command]
async fn application_read_embedded_preview(
    input: InspectProjectInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<Option<EmbeddedPreview>, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.read_embedded_preview(input)).await
}

const FULL_MODE_UNAVAILABLE: &str = "Beta 公共版未提供完整模式；请使用快速渲染。";

// Retained legacy command names fail explicitly; no private renderer is linked.
#[tauri::command]
async fn render_preview_image(file_path: String, display_mode: String, cache_input: Option<String>) -> Result<ProductPreviewImage, String> {
    let _ = (file_path, display_mode, cache_input);
    Err(FULL_MODE_UNAVAILABLE.into())
}

#[tauri::command]
async fn generate_preview_image(file_path: String, display_mode: String) -> Result<ProductPreviewImage, String> {
    let _ = (file_path, display_mode);
    Err(FULL_MODE_UNAVAILABLE.into())
}

#[tauri::command]
async fn application_export_material_list(input: ExportMaterialListInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.export_material_list(input)).await
}

#[tauri::command]
async fn application_prepare_material_recipes(input: MaterialRecipeCacheInput, operations: tauri::State<'_, ApplicationOperations>) -> Result<serde_json::Value, String> {
    let operations = operations.inner().clone();
    run_application_operation(move || operations.prepare_material_recipes(input)).await
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
    if let Some(mode) = input.color_mode {
        config.color_mode = normalize_color_mode(&mode);
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
    initialize_flake_user_resources()?;
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
    initialize_flake_user_resources()?;
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
#[serde(tag = "kind", rename_all = "snake_case")]
enum RedenDownloadOutput {
    Downloaded { path: String, file_name: String, bytes: usize },
    ExternalLink { url: String, provider: &'static str },
}

fn reden_external_drive(url: &reqwest::Url) -> Option<&'static str> {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() { return None; }
    match url.host_str()? {
        "pan.quark.cn" => Some("quark"),
        "pan.baidu.com" | "yun.baidu.com" => Some("baidu"),
        _ => None,
    }
}

fn reden_looks_html(content_type: &str, bytes: &[u8]) -> bool {
    let trimmed = bytes.iter().position(|byte| !byte.is_ascii_whitespace()).map(|at| &bytes[at..]).unwrap_or(bytes);
    content_type.to_ascii_lowercase().contains("text/html")
        || trimmed.get(..9).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"<!doctype"))
        || trimmed.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"<html"))
}

fn reden_save_projection(
    directory: &Path, file_name: &str, bytes: &[u8],
    validate: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<RedenDownloadOutput, String> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = directory.join(format!(".nova-reden-{}-{sequence}.litematic", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        drop(file);
        // Reuse the existing core NBT/projection parser, not extension, MIME, or magic-only checks.
        validate(&temporary).map_err(|error| format!("下载内容不是有效的 NBT 投影，未保存或入库：{error}"))?;
        let original = Path::new(file_name);
        let stem = original.file_stem().and_then(|name| name.to_str()).unwrap_or("reden-download");
        let mut destination = directory.join(file_name);
        let mut suffix = 1;
        while destination.exists() {
            destination = directory.join(format!("{stem}-{suffix}.litematic"));
            suffix += 1;
        }
        std::fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
        Ok(RedenDownloadOutput::Downloaded {
            path: destination.display().to_string(),
            file_name: destination.file_name().and_then(|name| name.to_str()).unwrap_or(file_name).to_string(),
            bytes: bytes.len(),
        })
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result
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
        .header(reqwest::header::USER_AGENT, "Litematica-Nova desktop/0.1")
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
    operations: &ApplicationOperations,
) -> Result<RedenDownloadOutput, String> {
    let client = reden_client()?;
    let mut url = start_url;
    for _ in 0..6 {
        if let Some(provider) = reden_external_drive(&url) {
            return Ok(RedenDownloadOutput::ExternalLink { url: url.to_string(), provider });
        }
        let response = client
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, "Litematica-Nova desktop/0.1")
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
        if reden_looks_html(&content_type, &bytes) {
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
        return reden_save_projection(&reden_download_dir()?, &file_name, &bytes, |temporary| {
            operations.inspect_project(InspectProjectInput { source_path: temporary.display().to_string() }).map(|_| ())
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
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<RedenDownloadOutput, String> {
    let operations = operations.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if attachment_index == 0 {
            return Err("RedenMC attachment index is 1-based; index 0 is invalid.".to_string());
        }
        let url = reqwest::Url::parse(&format!(
            "https://redenmc.com/api/mc-services/yisibite/{}/download/{}",
            machine_id, attachment_index
        ))
        .map_err(|e| e.to_string())?;
        reden_fetch_download(url, &format!("{machine_id}-{attachment_index}.litematic"), &operations)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reden_download_parametric(
    machine_id: String,
    sizes: RedenSizesInput,
    operations: tauri::State<'_, ApplicationOperations>,
) -> Result<RedenDownloadOutput, String> {
    let operations = operations.inner().clone();
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
        reden_fetch_download(url, &format!("{machine_id}.litematic"), &operations)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod reden_download_tests {
    use super::*;

    #[test]
    fn drive_links_are_explicit_and_never_match_lookalikes() {
        for (value, expected) in [("https://pan.quark.cn/s/abc", Some("quark")), ("https://pan.baidu.com/s/abc", Some("baidu")), ("https://yun.baidu.com/s/abc", Some("baidu")), ("https://pan.baidu.com.evil.test/a", None), ("http://pan.quark.cn/a", None), ("https://user:pass@pan.baidu.com/a", None)] {
            assert_eq!(reden_external_drive(&reqwest::Url::parse(value).unwrap()), expected);
        }
    }

    #[test]
    fn html_cannot_be_hidden_by_whitespace_case_or_mime() {
        assert!(reden_looks_html("application/octet-stream", b" \r\n<HTML><body>login</body>"));
        assert!(reden_looks_html("application/octet-stream", b"<!DoCtYpE html>"));
        assert!(reden_looks_html("TEXT/HTML; charset=utf8", b"page"));
        assert!(!reden_looks_html("application/octet-stream", &[0x1f, 0x8b, 8, 0]));
    }

    #[test]
    fn invalid_projection_is_removed_before_final_file_or_import() {
        let directory = std::env::temp_dir().join(format!("nova-reden-validation-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let result = reden_save_projection(&directory, "not-a-projection.litematic", b"not NBT", |temporary| {
            assert_eq!(std::fs::read(temporary).unwrap(), b"not NBT");
            Err("NBT decode rejected".into())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
        std::fs::remove_dir(&directory).unwrap();
    }
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
    "author": "Litematica-Nova",
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
async fn application_quick_preview_start(window: tauri::WebviewWindow, input: StartQuickPreviewInput, host: tauri::State<'_, QuickPreviewHost>) -> Result<QuickPreviewView, String> {
    if window.label() != "main" { return Err("快速预览只属于主窗口。".into()); }
    #[cfg(windows)]
    let host_window = ::viewer_launch_contract::QuickPreviewHostWindow {
        hwnd: (window.hwnd().map_err(|error| error.to_string())?.0 as isize).to_string(),
        process_id: std::process::id(),
    };
    #[cfg(not(windows))]
    return Err("原生页内预览当前以 Windows 为目标。".into());
    let host = host.inner().clone();
    #[cfg(windows)]
    return tauri::async_runtime::spawn_blocking(move || host.start(input, host_window)).await.map_err(|error| error.to_string())?;
}

#[tauri::command]
async fn application_quick_preview_snapshot(host: tauri::State<'_, QuickPreviewHost>) -> Result<Option<QuickPreviewView>, String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host.snapshot()).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn application_quick_preview_present(preview_id: String, intent: QuickPresentationIntent, revision: u64, host: tauri::State<'_, QuickPreviewHost>) -> Result<QuickPreviewView, String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host.present(&preview_id, intent, revision)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn application_quick_preview_close(preview_id: String, host: tauri::State<'_, QuickPreviewHost>) -> Result<(), String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host.close(&preview_id)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn application_render_preflight(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = input;
    Err(FULL_MODE_UNAVAILABLE.into())
}
#[tauri::command]
async fn application_render_use_basic_vulkan(preflight_id: String, source_path: PathBuf) -> Result<serde_json::Value, String> {
    let _ = (preflight_id, source_path);
    Err(FULL_MODE_UNAVAILABLE.into())
}
#[tauri::command]
async fn application_render_start(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = input;
    Err(FULL_MODE_UNAVAILABLE.into())
}
#[tauri::command]
fn application_render_discard_preflight(preflight_id: String) -> Result<(), String> {
    let _ = preflight_id;
    Err(FULL_MODE_UNAVAILABLE.into())
}
#[tauri::command]
async fn application_render_cancel(operation_id: String) -> Result<(), String> {
    let _ = operation_id;
    Err(FULL_MODE_UNAVAILABLE.into())
}
#[tauri::command]
async fn application_render_update_presentation(operation_id: String, intent: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = (operation_id, intent);
    Err(FULL_MODE_UNAVAILABLE.into())
}
#[tauri::command]
fn application_render_get_snapshot() -> serde_json::Value {
    serde_json::json!({ "operations": [] })
}

struct ApplicationHostComponents {
    supervisor: Arc<platform::process_supervisor::ProcessSupervisor>,
    operations: ApplicationOperations,
}

fn initialize_application_host(project_root: PathBuf, app_local_data_root: PathBuf) -> Result<ApplicationHostComponents, String> {
    if !cfg!(windows) {
        return Err("Beta 公共版桌面 Application Host 与原生嵌入当前仅支持 Windows；macOS/Linux 尚未提供。".into());
    }
    initialize_flake_user_resources()?;
    let viewer_exe = required_backend_exe_path("litematica_native_viewer")?;
    let core_exe = required_backend_exe_path("litematica_core")?;
    viewer_backend::verify_public_deployment(core_exe.parent().ok_or("public backend has no directory")?)?;
    let mut allowlist = platform::process_supervisor::ExecutableAllowlist::new();
    for (role, executable) in [
        (platform::process_supervisor::ExecutableRole::NativeViewer, &viewer_exe),
        (platform::process_supervisor::ExecutableRole::CoreHelper, &core_exe),
    ] { allowlist.allow(role, executable).map_err(|error| error.to_string())?; }
    let (supervisor, _process_events) = platform::process_supervisor::ProcessSupervisor::new(allowlist)
        .map_err(|error| format!("initialize Application process supervisor: {error}"))?;
    let supervisor = Arc::new(supervisor);
    let operations = ApplicationOperations::new(Arc::clone(&supervisor), project_root, app_local_data_root,
        core_exe.parent().map(Path::to_path_buf).ok_or_else(|| "backend executable has no directory".to_owned())?)?;
    Ok(ApplicationHostComponents { supervisor, operations })
}

#[cfg(test)]
mod public_host_tests {
    use super::*;
    #[tokio::test]
    async fn full_commands_are_explicitly_unavailable() {
        let err = application_render_start(serde_json::json!({"sourcePath":"test.litematic","buildMode":"prebuilt"})).await.unwrap_err();
        assert_eq!(err, FULL_MODE_UNAVAILABLE);
        assert_eq!(generate_preview_image("test.litematic".into(), "full".into()).await.unwrap_err(), FULL_MODE_UNAVAILABLE);
    }
    #[test]
    fn public_snapshot_never_claims_private_operations() {
        assert_eq!(application_render_get_snapshot(), serde_json::json!({"operations":[]}));
        assert_eq!(normalize_mode_string("full"), "fast_experimental");
    }
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--public-build-info" {
        println!("{}", serde_json::json!({"schema":"nova.public.beta.build.v1", "flavor":"fast-only", "publicBeta":true, "fullMode":false, "gitSha":env!("LBA_NOVA_GIT_SHA"), "desktopHostSupported":cfg!(windows)}));
        return;
    }
    if args.len() == 1 && args[0] == "--verify-public-resources" {
        let result = (|| -> Result<(), String> {
            let exe = env::current_exe().map_err(|error| error.to_string())?;
            let root = exe.parent().ok_or("public app executable has no directory")?;
            viewer_backend::verify_public_deployment(&root.join("bin/public-beta"))?;
            for directory in ["pack-in/enumerator", "pack-in/flake", "pack-in/item-icons-26.3", "data/generation-templates", "data/minecraft_blockstates", "block"] {
                let path = root.join(directory);
                if !path.is_dir() || std::fs::read_dir(&path).map_err(|error| error.to_string())?.next().is_none() {
                    return Err(format!("公共版运行资源缺失：{directory}"));
                }
            }
            if !root.join("pack-in/lang/zh_cn.json").is_file() { return Err("公共版运行语言资源缺失。".into()); }
            Ok(())
        })();
        match result {
            Ok(()) => println!("public_resources=verified; backends=fast-only; full_mode=absent"),
            Err(error) => { eprintln!("{error}"); std::process::exit(1); }
        }
        return;
    }
    tauri::Builder::default()
        .setup(|app| {
            let resource_root = app.path().resource_dir()?;
            let app_local_data_root = product_local_data_root(&app.path().local_data_dir()?);
            let _ = APP_RESOURCE_ROOT.set(resource_root);
            let _ = APP_LOCAL_DATA_ROOT.set(app_local_data_root.clone());
            app.manage(UpdateHost::new(app.package_info().version.to_string(), app_local_data_root.clone()));
            let host = initialize_application_host(
                get_root(),
                app_local_data_root.clone(),
            )
            .map_err(std::io::Error::other)?;
            app.manage(QuickPreviewHost::new(Arc::clone(&host.supervisor),
                required_backend_exe_path("litematica_native_viewer").map_err(std::io::Error::other)?, app_local_data_root.clone()));
            app.manage(Arc::clone(&host.supervisor));
            app.manage(host.operations);
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if window.try_state::<UpdateHost>().is_some_and(|host| host.prevents_close()) {
                        api.prevent_close();
                        return;
                    }
                }
            }
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                let supervisor = window
                    .try_state::<Arc<platform::process_supervisor::ProcessSupervisor>>();
                if let Some(supervisor) = supervisor.as_ref() {
                    supervisor.stop_admitting();
                }
                if let Some(supervisor) = supervisor {
                    let report = supervisor.shutdown(Duration::from_secs(3), Duration::from_secs(2));
                    if !report.remaining.is_empty() {
                        eprintln!(
                            "[NOVA_PROCESS_SUPERVISOR] shutdown left {} process handle(s) pending reap",
                            report.remaining.len()
                        );
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            application_update_snapshot,
            application_update_check,
            application_update_download,
            application_update_install,
            application_update_cancel,
            application_update_skip,
            application_update_restart,
            application_inspect_project,
            application_edit_project_metadata,
            application_read_layer_meta,
            application_read_layer,
            application_read_layer_container,
            application_read_project_markers,
            application_edit_project_origin,
            application_replace_blocks,
            application_get_project_stats,
            application_export_material_list,
            application_prepare_material_recipes,
            application_get_project_materials,
            application_generate_project,
            application_runtime_health,
            application_read_embedded_preview,
            render_preview_image,
            generate_preview_image,
            application_quick_preview_start,
            application_quick_preview_snapshot,
            application_quick_preview_present,
            application_quick_preview_close,
            application_render_preflight,
            application_render_use_basic_vulkan,
            application_render_start,
            application_render_discard_preflight,
            application_render_cancel,
            application_render_update_presentation,
            application_render_get_snapshot,
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
            open_file_parent_dir,
            copy_file_to_directory,
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
            open_reden_library_window,
            open_local_library_folders_window,
            open_ui_demo_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
