use serde::Serialize;

#[derive(Serialize)]
struct AppSnapshot {
    name: String,
    active_file: Option<String>,
    theme: String,
}

#[tauri::command]
fn get_app_snapshot() -> AppSnapshot {
    AppSnapshot {
        name: "LitematicaNova".to_string(),
        active_file: None,
        theme: "QTDefault".to_string(),
    }
}

#[tauri::command]
async fn open_material_list_window(app: tauri::AppHandle, _active_file: Option<String>) -> Result<(), String> {
    use tauri::Manager;

    const LABEL: &str = "material-list";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::App("material_list.html".into()))
        .title("材料列表")
        .inner_size(720.0, 520.0)
        .min_inner_size(560.0, 420.0)
        .build()
        .map_err(|err| err.to_string())?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_app_snapshot, open_material_list_window])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
