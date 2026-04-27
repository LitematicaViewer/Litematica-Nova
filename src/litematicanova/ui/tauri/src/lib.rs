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
        name: "LitematicaBA".to_string(),
        active_file: None,
        theme: "QTDefault".to_string(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_app_snapshot])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
