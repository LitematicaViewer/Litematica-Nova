//! File association requests reuse the product window; no renderer starts here.
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub(crate) struct OpenFiles(Mutex<Vec<String>>);

fn projection_paths<'a>(args: impl Iterator<Item = &'a str>, cwd: &Path) -> Vec<String> {
    args.filter_map(|arg| {
        let input = PathBuf::from(arg);
        if !input.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("litematic")) { return None; }
        let path = if input.is_absolute() { input } else { cwd.join(input) };
        path.is_file().then(|| path.canonicalize().unwrap_or(path).to_string_lossy().into_owned())
    }).collect()
}

pub(crate) fn queue_open_files<'a>(app: &AppHandle, args: impl Iterator<Item = &'a str>, cwd: &Path) {
    let paths = projection_paths(args, cwd);
    if paths.is_empty() { return; }
    if let Some(pending) = app.try_state::<OpenFiles>() {
        if let Ok(mut queue) = pending.0.lock() { queue.extend(paths); }
        let _ = app.emit_to("main", "nova-open-projection", ());
    }
}

#[tauri::command]
pub(crate) fn application_take_open_files(pending: tauri::State<'_, OpenFiles>) -> Result<Vec<String>, String> {
    pending.0.lock().map(|mut queue| std::mem::take(&mut *queue)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_existing_unicode_projection_and_rejects_other_arguments() {
        let root = std::env::temp_dir().join(format!("nova-shell-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("炮 台.litematic"), b"fixture").unwrap();
        assert_eq!(projection_paths(["炮 台.litematic", "missing.litematic", "https://example.com", "--help"].into_iter(), &root).len(), 1);
        std::fs::remove_file(root.join("炮 台.litematic")).unwrap(); std::fs::remove_dir(root).unwrap();
    }
}
