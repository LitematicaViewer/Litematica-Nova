use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use sha2::{Digest, Sha256};
use viewer_launch_contract::{ViewerFrameCapture, ViewerFrameCaptureReceipt};

const HASH_BUFFER_BYTES: usize = 64 * 1024;
const MAX_TEMP_NAME_ATTEMPTS: usize = 64;

static CAPTURE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_frame_capture_completion(
    capture: &ViewerFrameCapture,
    image: &RgbaImage,
) -> Result<ViewerFrameCaptureReceipt> {
    write_frame_capture_completion_with_hook(capture, image, || Ok(()))
}

fn write_frame_capture_completion_with_hook(
    capture: &ViewerFrameCapture,
    image: &RgbaImage,
    after_png_publish: impl FnOnce() -> Result<()>,
) -> Result<ViewerFrameCaptureReceipt> {
    capture
        .validate()
        .map_err(|error| anyhow!("validate frame capture contract: {error}"))?;
    if image.width() == 0 || image.height() == 0 {
        bail!("frame capture image dimensions must be non-zero");
    }
    ensure_final_absent("frame capture PNG", &capture.output)?;
    ensure_final_absent("frame capture receipt", &capture.receipt)?;

    let (mut png_temp, png_file) = create_unique_temp(&capture.output, "png")?;
    let mut png_writer = BufWriter::new(png_file);
    PngEncoder::new(&mut png_writer)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .with_context(|| {
            format!(
                "encode frame capture PNG temporary file {}",
                png_temp.path().display()
            )
        })?;
    png_writer.flush().with_context(|| {
        format!(
            "flush frame capture PNG temporary file {}",
            png_temp.path().display()
        )
    })?;
    png_writer.get_ref().sync_all().with_context(|| {
        format!(
            "sync frame capture PNG temporary file {}",
            png_temp.path().display()
        )
    })?;
    drop(png_writer);

    let (png_bytes, png_sha256) = hash_file(png_temp.path())?;
    publish_temp(&mut png_temp, &capture.output, "frame capture PNG")?;

    // The receipt is the only completion marker. A failure from this point on
    // may leave a complete PNG, but must never publish a receipt.
    after_png_publish()?;

    let receipt = ViewerFrameCaptureReceipt::new(
        capture,
        image.width(),
        image.height(),
        png_bytes,
        png_sha256,
    )
    .map_err(|error| anyhow!("build frame capture completion receipt: {error}"))?;
    let receipt_json = receipt
        .to_json_bytes()
        .map_err(|error| anyhow!("serialize frame capture completion receipt: {error}"))?;
    let (mut receipt_temp, receipt_file) = create_unique_temp(&capture.receipt, "receipt")?;
    let mut receipt_writer = BufWriter::new(receipt_file);
    receipt_writer.write_all(&receipt_json).with_context(|| {
        format!(
            "write frame capture receipt temporary file {}",
            receipt_temp.path().display()
        )
    })?;
    receipt_writer.flush().with_context(|| {
        format!(
            "flush frame capture receipt temporary file {}",
            receipt_temp.path().display()
        )
    })?;
    receipt_writer.get_ref().sync_all().with_context(|| {
        format!(
            "sync frame capture receipt temporary file {}",
            receipt_temp.path().display()
        )
    })?;
    drop(receipt_writer);
    publish_temp(&mut receipt_temp, &capture.receipt, "frame capture receipt")?;
    Ok(receipt)
}

fn ensure_final_absent(label: &str, path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => bail!("{label} already exists: {}", path.display()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", path.display())),
    }
}

fn create_unique_temp(final_path: &Path, kind: &str) -> Result<(PendingTempFile, File)> {
    let parent = final_path.parent().ok_or_else(|| {
        anyhow!(
            "frame capture final path has no parent: {}",
            final_path.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create frame capture output directory {}", parent.display()))?;
    let file_name = final_path.file_name().ok_or_else(|| {
        anyhow!(
            "frame capture final path has no file name: {}",
            final_path.display()
        )
    })?;
    for _ in 0..MAX_TEMP_NAME_ATTEMPTS {
        let sequence = CAPTURE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(format!(
            ".litematica-nova-frame-capture-{kind}-{}-{sequence:016x}.tmp",
            std::process::id()
        ));
        let path = parent.join(temp_name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((PendingTempFile::new(path), file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "create frame capture {kind} temporary file {}",
                        path.display()
                    )
                });
            }
        }
    }
    bail!(
        "could not allocate a unique frame capture {kind} temporary file beside {}",
        final_path.display()
    )
}

fn publish_temp(temp: &mut PendingTempFile, final_path: &Path, label: &str) -> Result<()> {
    ensure_final_absent(label, final_path)?;
    fs::rename(temp.path(), final_path).with_context(|| {
        format!(
            "publish {label} {} -> {}",
            temp.path().display(),
            final_path.display()
        )
    })?;
    temp.disarm();
    Ok(())
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    let file =
        File::open(path).with_context(|| format!("open frame capture PNG {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    let mut bytes = 0_u64;
    let mut hasher = Sha256::new();
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("hash frame capture PNG {}", path.display()))?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .ok_or_else(|| anyhow!("frame capture PNG byte length exceeds u64"))?;
        hasher.update(&buffer[..read]);
    }
    if bytes == 0 {
        bail!("encoded frame capture PNG is empty: {}", path.display());
    }
    Ok((bytes, format!("{:x}", hasher.finalize())))
}

struct PendingTempFile {
    path: PathBuf,
    armed: bool,
}

impl PendingTempFile {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingTempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempFixture {
        root: PathBuf,
    }

    impl TempFixture {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "lba-frame-capture-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn capture(&self, frame: u64) -> ViewerFrameCapture {
            ViewerFrameCapture::new(self.root.join("capture.png"), frame).unwrap()
        }

        fn assert_no_temp_files(&self) {
            let temp_files = fs::read_dir(&self.root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .filter(|name| {
                    name.to_string_lossy()
                        .contains(".litematica-nova-frame-capture-")
                })
                .collect::<Vec<_>>();
            assert!(temp_files.is_empty(), "temporary files: {temp_files:?}");
        }
    }

    impl Drop for TempFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(4, 3, Rgba([0x12, 0x34, 0x56, 0xff]))
    }

    #[test]
    fn production_capture_temp_file_uses_formal_product_prefix() {
        let fixture = TempFixture::new("product-prefix");
        let capture = fixture.capture(3);
        let (pending, file) = create_unique_temp(&capture.output, "png").unwrap();
        let file_name = pending.path().file_name().unwrap().to_string_lossy();
        assert!(
            file_name.starts_with(".capture.png.litematica-nova-frame-capture-png-"),
            "temporary capture file name: {file_name}"
        );
        drop(file);
        drop(pending);
        fixture.assert_no_temp_files();
    }

    #[test]
    fn failure_after_png_publish_never_publishes_receipt() {
        let fixture = TempFixture::new("partial");
        let capture = fixture.capture(7);
        let error = write_frame_capture_completion_with_hook(&capture, &image(), || {
            bail!("injected failure before receipt")
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected failure"));
        assert!(capture.output.is_file());
        assert!(!capture.receipt.exists());
        fixture.assert_no_temp_files();
    }

    #[test]
    fn success_publishes_valid_png_before_completion_receipt() {
        let fixture = TempFixture::new("success");
        let capture = fixture.capture(11);
        let receipt = write_frame_capture_completion(&capture, &image()).unwrap();
        assert!(capture.output.is_file());
        assert!(capture.receipt.is_file());
        fixture.assert_no_temp_files();

        let png = fs::read(&capture.output).unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        let actual_sha256 = format!("{:x}", Sha256::digest(&png));
        let decoded_receipt =
            ViewerFrameCaptureReceipt::from_json_slice(&fs::read(&capture.receipt).unwrap())
                .unwrap();
        assert_eq!(decoded_receipt, receipt);
        decoded_receipt
            .validate_completion(
                &capture,
                decoded.width(),
                decoded.height(),
                png.len() as u64,
                &actual_sha256,
            )
            .unwrap();
    }

    #[test]
    fn existing_png_or_receipt_is_never_overwritten() {
        let png_conflict = TempFixture::new("png-conflict");
        let capture = png_conflict.capture(13);
        fs::write(&capture.output, b"legacy-png").unwrap();
        let error = write_frame_capture_completion(&capture, &image()).unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert_eq!(fs::read(&capture.output).unwrap(), b"legacy-png");
        assert!(!capture.receipt.exists());
        png_conflict.assert_no_temp_files();

        let receipt_conflict = TempFixture::new("receipt-conflict");
        let capture = receipt_conflict.capture(17);
        fs::write(&capture.receipt, b"legacy-receipt").unwrap();
        let error = write_frame_capture_completion(&capture, &image()).unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert!(!capture.output.exists());
        assert_eq!(fs::read(&capture.receipt).unwrap(), b"legacy-receipt");
        receipt_conflict.assert_no_temp_files();
    }
}
