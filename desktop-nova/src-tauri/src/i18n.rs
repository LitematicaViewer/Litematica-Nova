//! Native presentation strings. Business state and diagnostics remain language-neutral.
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};
static ENGLISH: AtomicBool = AtomicBool::new(false);
static MATERIAL_EXPORT: AtomicBool = AtomicBool::new(false);
pub(crate) fn set_material_export(value: bool) { MATERIAL_EXPORT.store(value, Ordering::Relaxed); }
pub(crate) fn set_locale(locale: &str) { ENGLISH.store(locale == "en-US", Ordering::Relaxed); }
pub(crate) fn locale() -> &'static str { if ENGLISH.load(Ordering::Relaxed) { "en-US" } else { "zh-CN" } }
pub(crate) fn text(key: &str) -> &'static str {
    let pair = match key {
        "dialog.configDirectory" => ("选择用户数据目录", "Choose user data directory"),
        "dialog.openSchematic" => ("打开投影文件", "Open schematic"),
        "dialog.schematicFilter" => ("Litematica 投影", "Litematica schematic"),
        "window.materialList" => ("材料清单", "Material List"),
        "window.materialExport" => ("导出材料列表", "Export Material List"),
        "window.enumerator" => ("枚举器", "Enumerator"),
        "window.assets" => ("游戏资源管理", "Game Resource Manager"),
        "window.onlineLibrary" => ("在线投影库", "Online Schematic Library"),
        "window.localFolders" => ("本地投影库目录", "Schematic Library Folders"),
        "window.uiDemo" => ("UI 测试窗口", "UI Demo Window"),
        "window.closing" => ("Litematica-Nova — 正在保留草稿并关闭", "Litematica-Nova — Saving drafts and closing"),
        "window.saving" => ("Litematica-Nova — 保存仍在进行，请完成后再次关闭", "Litematica-Nova — Saving; close again when finished"),
        "close.savingDrafts" => ("正在保留草稿并关闭；可再次点击关闭重试，当前草稿不会丢失。", "Saving drafts and closing. You may click Close again to retry; your drafts are retained."),
        "close.stillSaving" => ("保存仍在进行，请完成后再次关闭。", "Saving is still in progress. Close again when it finishes."),
        _ => ("Litematica-Nova", "Litematica-Nova"),
    };
    if ENGLISH.load(Ordering::Relaxed) { pair.1 } else { pair.0 }
}
pub(crate) fn update_window_titles(app: &AppHandle) {
    for (label, key) in [
        ("material-list", if MATERIAL_EXPORT.load(Ordering::Relaxed) { "window.materialExport" } else { "window.materialList" }), ("enumerator", "window.enumerator"),
        ("asset-manager", "window.assets"), ("reden-library", "window.onlineLibrary"),
        ("local-library-folders", "window.localFolders"), ("ui-demo-window", "window.uiDemo"),
    ] {
        if let Some(window) = app.get_webview_window(label) { let _ = window.set_title(text(key)); }
    }
}
