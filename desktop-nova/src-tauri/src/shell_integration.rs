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

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{fs, io::Read, os::unix::fs::PermissionsExt, process::{Command, Stdio}, time::{Duration, Instant}};

    const MARKER: &str = "# Managed by Litematica-Nova shell integration v1";
    const DESKTOP_ID: &str = "com.litematica.nova.desktop";

    fn data_home() -> Result<PathBuf, String> {
        let root = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
            .ok_or("Linux user data directory is missing")?;
        if !root.is_absolute() { return Err("Linux user data directory must be absolute".into()); }
        Ok(root)
    }

    fn desktop_argument(path: &Path) -> Result<String, String> {
        let path = path.to_str().ok_or("desktop launch path is not UTF-8")?;
        if path.contains(['\n', '\r', '\0']) { return Err("desktop launch path contains a control character".into()); }
        Ok(format!("\"{}\"", path.replace('\\', "\\\\\\\\").replace('"', "\\\\\\\"")
            .replace('`', "\\\\`").replace('$', "\\\\$").replace('%', "%%")))
    }

    fn write_owned(path: &Path, content: &str) -> Result<(), String> {
        if let Ok(existing) = fs::read_to_string(path) {
            if existing == content { return Ok(()); }
            if !existing.contains(MARKER) { return Err(format!("shell integration path is owned by another installer: {}", path.display())); }
        }
        fs::create_dir_all(path.parent().ok_or("shell path has no parent")?).map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("nova-{}.tmp", std::process::id()));
        fs::write(&temporary, content).map_err(|error| error.to_string())?;
        fs::rename(temporary, path).map_err(|error| error.to_string())
    }

    fn refresh_databases(data: &Path) {
        for (program, directory) in [("update-mime-database", data.join("mime")),
            ("update-desktop-database", data.join("applications"))] {
            match Command::new(program).arg(directory).stdin(Stdio::null()).stdout(Stdio::null()).status() {
                Ok(status) if status.success() => {},
                result => eprintln!("[NOVA_SHELL] refresh={program} result={result:?}"),
            }
        }
    }

    fn install(package: &tauri::PackageInfo, user_root: &Path) -> Result<(), String> {
        let environment = tauri::Env::default();
        let launch = environment.appimage.as_ref().map(PathBuf::from)
            .unwrap_or(std::env::current_exe().map_err(|error| error.to_string())?);
        if !launch.is_absolute() || !launch.is_file()
            || environment.appdir.as_ref().is_some_and(|root| launch.starts_with(Path::new(root))) {
            return Err("no persistent application entry exists; temporary AppImage mount is not registered".into());
        }
        let resources = tauri::utils::platform::resource_dir(package, &environment).map_err(|error| error.to_string())?;
        if !resources.join("bin/viewer-backend/litematica_core").is_file() {
            return Err("bundled Core thumbnail entry is missing".into());
        }
        let data = data_home()?;
        let executable = desktop_argument(&launch)?;
        let cache_argument = desktop_argument(user_root)?;
        let icon = data.join("icons/hicolor/64x64/apps/com.litematica.nova.png");
        fs::create_dir_all(icon.parent().unwrap()).map_err(|error| error.to_string())?;
        fs::write(&icon, include_bytes!("../icons/icon.png")).map_err(|error| error.to_string())?;
        write_owned(&data.join("applications").join(DESKTOP_ID), &format!(
            "{MARKER}\n[Desktop Entry]\nType=Application\nName=Litematica-Nova\nComment=Minecraft projection manager\nExec={executable} %f\nIcon=com.litematica.nova\nTerminal=false\nCategories=Graphics;Utility;\nMimeType=application/x-litematic;\n"))?;
        write_owned(&data.join("mime/packages/com.litematica.nova.xml"), &format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- {MARKER} -->\n<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\"><mime-type type=\"application/x-litematic\"><comment>Minecraft Litematica projection</comment><glob pattern=\"*.litematic\"/></mime-type></mime-info>\n"))?;
        write_owned(&data.join("thumbnailers/com.litematica.nova.thumbnailer"), &format!(
            "{MARKER}\n[Thumbnailer Entry]\nExec={executable} --system-thumbnail %i %o %s {cache_argument}\nMimeType=application/x-litematic;\n"))?;
        let uninstall = user_root.join("shell-integration/uninstall.sh");
        write_owned(&uninstall, include_str!("../../scripts/linux-shell-uninstall.sh"))?;
        fs::set_permissions(&uninstall, fs::Permissions::from_mode(0o700)).map_err(|error| error.to_string())?;
        refresh_databases(&data);
        eprintln!("[NOVA_SHELL] linux_registered=true default_handler_changed=false appimage={}", environment.appimage.is_some());
        Ok(())
    }

    pub(super) fn initialize(package: tauri::PackageInfo, user_root: PathBuf) {
        let _ = std::thread::Builder::new().name("nova-shell-register".into()).spawn(move || {
            if let Err(error) = install(&package, &user_root) { eprintln!("[NOVA_SHELL] registration_failed={error}"); }
        });
    }

    fn digest(path: &Path) -> Result<String, String> {
        let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut hash = Sha256::new(); let mut bytes = [0u8; 64 * 1024];
        loop { let count = file.read(&mut bytes).map_err(|error| error.to_string())?; if count == 0 { break; } hash.update(&bytes[..count]); }
        Ok(format!("{:x}", hash.finalize()))
    }

    fn thumbnail(args: &[std::ffi::OsString], package: &tauri::PackageInfo) -> Result<(), String> {
        if args.len() != 5 { return Err("usage: --system-thumbnail INPUT OUTPUT SIZE USER_DATA_ROOT".into()); }
        let input = PathBuf::from(&args[1]).canonicalize().map_err(|error| error.to_string())?;
        let output = PathBuf::from(&args[2]); let user_root = PathBuf::from(&args[4]);
        let size = args[3].to_str().and_then(|value| value.parse::<u32>().ok()).filter(|size| (1..=4096).contains(size)).ok_or("invalid thumbnail size")?;
        if !input.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("litematic"))
            || !output.is_absolute() || !user_root.is_absolute() || output.canonicalize().is_ok_and(|path| path == input) {
            return Err("thumbnail paths are invalid or would replace the projection".into());
        }
        let key = digest(&input)?;
        let cache = user_root.join("thumbnails").join(format!("{key}.png"));
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
        let temporary = output.with_extension(format!("nova-{}-{nonce}-raw.png", std::process::id()));
        let source = if cache.is_file() { cache } else {
            // Reserve an empty unique target, so a missing embedded preview
            // can never accidentally reuse an earlier extraction's PNG.
            drop(fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|error| error.to_string())?);
            let resources = tauri::utils::platform::resource_dir(package, &tauri::Env::default()).map_err(|error| error.to_string())?;
            let core = resources.join("bin/viewer-backend/litematica_core");
            let mut child = Command::new(core).current_dir(&resources).env("LBA_DATA_ROOT", &user_root)
                .arg("projection-preview").arg("--input").arg(&input)
                .arg("--output").arg(&temporary).stdin(Stdio::null()).stdout(Stdio::null()).spawn().map_err(|error| error.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(25);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => break,
                    Ok(Some(status)) => { let _ = fs::remove_file(&temporary); return Err(format!("Core thumbnail extraction failed: {status}")); },
                    Err(error) => { let _ = child.kill(); let _ = child.wait(); let _ = fs::remove_file(&temporary); return Err(error.to_string()); },
                    Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                    Ok(None) => { let _ = child.kill(); let _ = child.wait(); let _ = fs::remove_file(&temporary); return Err("Core thumbnail extraction timed out".into()); },
                }
            }
            if fs::metadata(&temporary).map(|metadata| metadata.len()).unwrap_or(0) == 0 {
                let _ = fs::remove_file(&temporary);
                return Err("projection has no saved or embedded preview image".into());
            }
            temporary.clone()
        };
        let result = (|| {
            let mut reader = image::ImageReader::open(&source).map_err(|error| error.to_string())?.with_guessed_format().map_err(|error| error.to_string())?;
            let mut limits = image::Limits::default(); limits.max_image_width = Some(8192); limits.max_image_height = Some(8192); limits.max_alloc = Some(128 * 1024 * 1024); reader.limits(limits);
            let image = reader.decode().map_err(|error| error.to_string())?;
            if digest(&input)? != key { return Err("projection changed during thumbnail extraction".into()); }
            image.thumbnail(size, size).save_with_format(&output, image::ImageFormat::Png).map_err(|error| error.to_string())
        })();
        let _ = fs::remove_file(temporary);
        result
    }

    pub(super) fn dispatch(args: &[std::ffi::OsString], package: &tauri::PackageInfo) -> Option<i32> {
        if args.first().is_none_or(|argument| argument != "--system-thumbnail") { return None; }
        Some(match thumbnail(args, package) { Ok(()) => 0, Err(error) => { eprintln!("[NOVA_SHELL] thumbnail_failed={error}"); 1 } })
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn initialize_linux(package: tauri::PackageInfo, user_root: PathBuf) { linux::initialize(package, user_root); }

#[cfg(target_os = "linux")]
pub(crate) fn dispatch_before_gui(args: &[std::ffi::OsString], package: &tauri::PackageInfo) -> Option<i32> { linux::dispatch(args, package) }

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
