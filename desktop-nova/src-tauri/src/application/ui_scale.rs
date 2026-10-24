//! User zoom is additional WebView zoom, never the operating-system DPI.
use tauri::{AppHandle, Emitter, Manager};

pub(crate) const DEFAULT: f64 = 1.0;

pub(crate) fn validate(value: f64) -> Result<f64, String> {
    if !value.is_finite() || !(0.5..=5.0).contains(&value)
        || (value * 100.0 - (value * 100.0).round()).abs() > 0.000001
    {
        return Err("界面缩放必须是 0.50–5.00 之间、最多两位小数的数值".into());
    }
    Ok(value)
}

pub(crate) fn apply(app: &AppHandle, value: f64) -> Result<(), String> {
    let value = validate(value)?;
    for window in app.webview_windows().values() {
        window.set_zoom(value).map_err(|error| format!("{}: {error}", window.label()))?;
    }
    // Native viewport owners observe layout/resize only. No scene/build commands.
    app.emit("nova-ui-scale-changed", value).map_err(|error| error.to_string())
}

pub(crate) fn install_reset_menu(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, Submenu};
    let reset = MenuItem::with_id(app, "nova-reset-ui-scale", "恢复界面缩放 / Reset UI scale", true,
        Some("CmdOrCtrl+Alt+0"))?;
    let view = Submenu::with_items(app, "视图 / View", true, &[&reset])?;
    if let Some(menu) = app.menu() {
        menu.append(&view)?;
    } else {
        app.set_menu(Menu::with_items(app, &[&view])?)?;
    }
    app.on_menu_event(|app, event| {
        if event.id().as_ref() != "nova-reset-ui-scale" { return; }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let result = crate::run_application_operation(|| {
                let input = serde_json::from_value(serde_json::json!({"ui_scale": DEFAULT}))
                    .map_err(|error| error.to_string())?;
                crate::save_user_config_inner(input)
            }).await;
            match result {
                Ok(info) => {
                    if let Err(error) = apply(&app, info.config.ui_scale) {
                        eprintln!("[NOVA_UI_SCALE] reset_failed={error}");
                    }
                    let _ = app.emit("user-config-changed", &info);
                }
                Err(error) => eprintln!("[NOVA_UI_SCALE] reset_failed={error}"),
            }
        });
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_entire_user_range_without_clamping() {
        for value in [0.5, 0.75, 0.9, 1.0, 1.25, 2.0, 3.0, 4.0, 5.0, 1.23] {
            assert_eq!(validate(value).unwrap(), value);
        }
        for value in [0.49, 5.01, f64::NAN, f64::INFINITY, 1.234] {
            assert!(validate(value).is_err());
        }
    }
    #[test]
    fn zoom_migrates_and_round_trips_independently_of_product_version() {
        let directory = std::env::temp_dir().join(format!("nova-ui-scale-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&directory).unwrap();
        let path = crate::user_config_path(&directory);
        let mut config = crate::default_user_config();
        for scale in [0.5, 0.75, 1.0, 1.25, 2.0, 3.0, 4.0, 5.0] {
            config.ui_scale = scale;
            std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
            assert_eq!(crate::read_user_config_data(&directory).ui_scale, scale);
        }
        let mut old = serde_json::to_value(config).unwrap();
        old.as_object_mut().unwrap().remove("ui_scale");
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(crate::read_user_config_data(&directory).ui_scale, DEFAULT);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
