//! Shared product thumbnail cache. It accepts real GPU frames or embedded images,
//! never screenshots of UI or fabricated geometry. Explorer only reads this cache.
use super::model::ProductPreviewImage;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static IMAGE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(crate) fn temporary_preview_path() -> PathBuf {
    std::env::temp_dir().join(format!("nova-preview-{}-{}.png", std::process::id(), IMAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)))
}

pub(crate) fn cache_path(source: &Path) -> Result<PathBuf, String> {
    let name = source_cache_file_name(source)?;
    Ok(thumbnail_data_root()?.join("thumbnails").join(name))
}

fn source_cache_file_name(source: &Path) -> Result<String, String> {
    let mut file = File::open(source).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut chunk = [0u8; 64 * 1024];
    loop { let count = file.read(&mut chunk).map_err(|error| error.to_string())?; if count == 0 { break; } hash.update(&chunk[..count]); }
    Ok(format!("{:x}.png", hash.finalize()))
}

fn thumbnail_data_root() -> Result<PathBuf, String> {
    // Keep Explorer's historical cache. Unix uses the Host's resolved user root.
    #[cfg(windows)] {
        let root = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA缺失")?;
        Ok(PathBuf::from(root).join("Litematica-Nova"))
    }
    #[cfg(not(windows))] {
        let root = std::env::var_os("NOVA_USER_DATA_ROOT").ok_or("Nova用户数据目录尚未初始化")?;
        let root = PathBuf::from(root);
        if !root.is_absolute() { return Err("Nova用户数据目录必须为绝对路径".into()); }
        Ok(root)
    }
}

pub(crate) fn load(path: &Path) -> Result<ProductPreviewImage, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).map_err(|error| error.to_string())?;
    Ok(ProductPreviewImage { preview_path: path.to_string_lossy().into(), data_url: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)) })
}

pub(crate) fn save(source: &Path, frame: &Path, expected: &Path) -> Result<ProductPreviewImage, String> {
    let name = source_cache_file_name(source)?;
    if expected.file_name() != Some(std::ffi::OsStr::new(&name)) { return Err("截图期间投影内容发生变化，旧帧未缓存".into()); }
    let output = thumbnail_data_root()?.join("thumbnails").join(name);
    if output != expected { return Err("截图期间投影内容发生变化，旧帧未缓存".into()); }
    let bytes = fs::read(frame).map_err(|error| error.to_string())?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).map_err(|error| error.to_string())?;
    fs::create_dir_all(output.parent().ok_or("缩略图路径异常")?).map_err(|error| error.to_string())?;
    let temp = output.with_extension(format!("{}-{}.tmp", std::process::id(), IMAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    // Windows rename cannot replace an existing file. Publish complete bytes only.
    if output.exists() { fs::remove_file(&output).map_err(|error| error.to_string())?; }
    fs::rename(&temp, &output).map_err(|error| error.to_string())?;
    if frame != output { let _ = fs::remove_file(frame); }
    notify_shell(source);
    load(&output)
}

#[cfg(windows)]
fn notify_shell(source: &Path) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name="shell32")]
    extern "system" { fn SHChangeNotify(event: i32, flags: u32, item: *const std::ffi::c_void, other: *const std::ffi::c_void); }
    let path: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe { SHChangeNotify(0x0000_2000, 0x0005, path.as_ptr().cast(), std::ptr::null()); }
}
#[cfg(not(windows))]
fn notify_shell(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn cache_key_is_content_based_not_name_based() {
        let root = std::env::temp_dir().join(format!("nova-thumbnail-key-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let a = root.join("甲.litematic"); let b = root.join("乙.litematic");
        fs::write(&a, b"source").unwrap(); fs::write(&b, b"source").unwrap();
        assert_eq!(source_cache_file_name(&a).unwrap(), source_cache_file_name(&b).unwrap());
        fs::write(&b, b"changed").unwrap(); assert_ne!(source_cache_file_name(&a).unwrap(), source_cache_file_name(&b).unwrap());
        fs::remove_file(a).unwrap(); fs::remove_file(b).unwrap(); fs::remove_dir(root).unwrap();
    }
    #[test] fn temporary_names_are_unique_and_changed_sources_do_not_receive_old_frames() {
        assert_ne!(temporary_preview_path(), temporary_preview_path());
        let root = std::env::temp_dir().join(format!("nova-thumbnail-change-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("fixture.litematic"); let frame = root.join("unit.png");
        fs::write(&source, b"unit-source-original").unwrap();
        let expected = root.join(source_cache_file_name(&source).unwrap());
        image::save_buffer_with_format(&frame, &[255,0,0,255], 1, 1, image::ColorType::Rgba8, image::ImageFormat::Png).unwrap();
        fs::write(&source, b"unit-source-changed").unwrap();
        assert!(save(&source, &frame, &expected).unwrap_err().contains("旧帧未缓存"));
        fs::remove_file(source).unwrap(); fs::remove_file(frame).unwrap(); fs::remove_dir(root).unwrap();
    }
}
