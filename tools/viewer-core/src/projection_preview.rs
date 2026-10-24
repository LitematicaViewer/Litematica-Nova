use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use serde::{Deserialize, Serialize};

use crate::nbt::load_litematic_preview_image;

/// Persistent derived preview lookup for the supervised data worker. Cold reads
/// hash the compressed stream while scanning NBT once; warm reads inspect only
/// source identity metadata and the small cached PNG. A missing embedded image
/// is cached as missing, never replaced with an automatically rendered picture.
pub fn read_cached(
    input: &serde_json::Value,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<serde_json::Value> {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::sync::atomic::Ordering;
    if input["action"] == "readEmbeddedBatch" {
        let paths = input["sourcePaths"]
            .as_array()
            .context("Preview source paths are missing")?;
        anyhow::ensure!(
            paths.len() <= 32,
            "Preview read batch exceeds its bounded visible window"
        );
        let mut rows = Vec::new();
        for path in paths {
            anyhow::ensure!(!cancel.load(Ordering::Relaxed), "Preview read cancelled");
            let value = read_cached(
                &serde_json::json!({"action":"readEmbedded","sourcePath":path,"storageDir":input["storageDir"]}),
                cancel,
            );
            rows.push(match value {
                Ok(image) => serde_json::json!({"sourcePath":path,"image":image}),
                Err(error) => {
                    serde_json::json!({"sourcePath":path,"image":null,"error":error.to_string()})
                }
            });
        }
        return Ok(serde_json::Value::Array(rows));
    }
    #[derive(Serialize, Deserialize)]
    struct Index {
        schema: u32,
        bytes: u64,
        modified: u128,
        content_id: String,
        found: bool,
        width: Option<u32>,
        height: Option<u32>,
    }
    struct HashingReader<'a> {
        file: fs::File,
        hash: Sha256,
        cancel: &'a std::sync::atomic::AtomicBool,
    }
    impl Read for HashingReader<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Preview read cancelled",
                ));
            }
            let count = self.file.read(bytes)?;
            self.hash.update(&bytes[..count]);
            Ok(count)
        }
    }
    anyhow::ensure!(!cancel.load(Ordering::Relaxed), "Preview read cancelled");
    anyhow::ensure!(
        input["action"] == "readEmbedded",
        "Unknown thumbnail data operation"
    );
    let source = fs::canonicalize(
        input["sourcePath"]
            .as_str()
            .context("Preview source path is missing")?,
    )?;
    if !source
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("litematic"))
    {
        return Ok(serde_json::Value::Null);
    }
    let root = Path::new(
        input["storageDir"]
            .as_str()
            .context("Preview cache directory is missing")?,
    )
    .join("cache/embedded-previews");
    fs::create_dir_all(&root)?;
    let stamp = || -> Result<(u64, u128)> {
        let meta = fs::metadata(&source)?;
        Ok((
            meta.len(),
            meta.modified()?
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
        ))
    };
    let (bytes, modified) = stamp()?;
    let pointer = root.join(format!(
        "path-{:x}.json",
        Sha256::digest(source.to_string_lossy().as_bytes())
    ));
    let index = fs::read(&pointer)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Index>(&bytes).ok())
        .filter(|index| {
            index.schema == 1
                && index.bytes == bytes
                && index.modified == modified
                && index.content_id.len() == 64
                && index.content_id.bytes().all(|c| c.is_ascii_hexdigit())
        });
    let index = if let Some(index) = index
        .filter(|index| !index.found || root.join(format!("{}.png", index.content_id)).is_file())
    {
        index
    } else {
        let temporary = tempfile::NamedTempFile::new_in(&root)?;
        let mut reader = HashingReader {
            file: fs::File::open(&source)?,
            hash: Sha256::new(),
            cancel,
        };
        let extracted = extract_projection_preview_stream(
            std::io::BufReader::new(&mut reader),
            temporary.path(),
        )?;
        // Finish hashing compressed bytes not consumed by the gzip reader.
        std::io::copy(&mut reader, &mut std::io::sink())?;
        anyhow::ensure!(
            stamp()? == (bytes, modified),
            "Projection changed while its embedded preview was read"
        );
        let content_id = format!("{:x}", reader.hash.finalize());
        let dimensions = if extracted.found {
            let image = image::open(temporary.path())?
                .thumbnail(PREVIEW_WIDTH, PREVIEW_HEIGHT)
                .to_rgba8();
            write_preview_png(&image, temporary.path())?;
            let dimensions = Some(image.dimensions());
            temporary
                .persist(root.join(format!("{content_id}.png")))
                .map_err(|error| error.error)?;
            dimensions
        } else {
            None
        };
        let index = Index {
            schema: 1,
            bytes,
            modified,
            content_id,
            found: extracted.found,
            width: dimensions.map(|v| v.0),
            height: dimensions.map(|v| v.1),
        };
        let mut temporary = tempfile::NamedTempFile::new_in(&root)?;
        serde_json::to_writer(temporary.as_file_mut(), &index)?;
        temporary.as_file_mut().sync_all()?;
        temporary.persist(&pointer).map_err(|error| error.error)?;
        index
    };
    anyhow::ensure!(!cancel.load(Ordering::Relaxed), "Preview read cancelled");
    if !index.found {
        return Ok(serde_json::Value::Null);
    }
    let path = root.join(format!("{}.png", index.content_id));
    let png = fs::read(&path)?;
    // Decode in Rust; the UI receives a final image only, never raw ARGB/NBT.
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?;
    anyhow::ensure!(
        Some(image.width()) == index.width && Some(image.height()) == index.height,
        "Cached embedded preview dimensions changed"
    );
    Ok(
        serde_json::json!({"preview_path":path,"source_identity":index.content_id,"origin":"embedded","width":image.width(),"height":image.height(),
        "data_url":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png))}),
    )
}

pub const PREVIEW_WIDTH: u32 = 320;
pub const PREVIEW_HEIGHT: u32 = 240;
pub const PREVIEW_CONTENT_MARGIN: u32 = 5;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewImageMode {
    #[default]
    Import,
    #[serde(alias = "fast")]
    FastExperimental,
    Standard,
    Full,
}

/// The fixed new format is distinguishable from every legacy square ARGB
/// array by its length. No external cache or second persistent image is used.
pub(crate) fn embedded_preview_dimensions(pixel_count: u64) -> Option<(u32, u32)> {
    if pixel_count == u64::from(PREVIEW_WIDTH) * u64::from(PREVIEW_HEIGHT) {
        return Some((PREVIEW_WIDTH, PREVIEW_HEIGHT));
    }
    let edge = (pixel_count as f64).sqrt() as u64;
    (edge > 0 && edge.checked_mul(edge) == Some(pixel_count)).then_some((edge as u32, edge as u32))
}

/// Post-processes the actual captured pixels only; camera, scene and renderer
/// state are deliberately not part of this operation.
pub fn normalize_preview_image(
    source: &image::RgbaImage,
    mode: PreviewImageMode,
) -> Result<image::RgbaImage> {
    let (width, height) = source.dimensions();
    if width == 0 || height == 0 {
        return Ok(image::RgbaImage::new(PREVIEW_WIDTH, PREVIEW_HEIGHT));
    }
    if mode == PreviewImageMode::Full {
        // Fractional source coordinates retain an exact 4:3 centred crop even
        // for tiny/odd images. A full-size intermediate cover canvas would be
        // unbounded for very tall/narrow inputs, so sample the crop directly.
        let scale = (f64::from(width) / f64::from(PREVIEW_WIDTH))
            .min(f64::from(height) / f64::from(PREVIEW_HEIGHT));
        let crop_width = scale * f64::from(PREVIEW_WIDTH);
        let crop_height = scale * f64::from(PREVIEW_HEIGHT);
        let left = (f64::from(width) - crop_width) * 0.5;
        let top = (f64::from(height) - crop_height) * 0.5;
        return Ok(image::RgbaImage::from_fn(
            PREVIEW_WIDTH,
            PREVIEW_HEIGHT,
            |x, y| {
                let sx = (left + (f64::from(x) + 0.5) * scale - 0.5)
                    .clamp(0.0, f64::from(width - 1)) as f32;
                let sy = (top + (f64::from(y) + 0.5) * scale - 0.5)
                    .clamp(0.0, f64::from(height - 1)) as f32;
                image::imageops::interpolate_bilinear(source, sx, sy)
                    .expect("nonempty image coordinates are clamped")
            },
        ));
    }

    let mut left = 0;
    let mut top = 0;
    let mut right = width;
    let mut bottom = height;
    let margin = if matches!(
        mode,
        PreviewImageMode::FastExperimental | PreviewImageMode::Standard
    ) {
        left = width;
        top = height;
        right = 0;
        bottom = 0;
        for (x, y, pixel) in source.enumerate_pixels() {
            if pixel[3] > 0 {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
        if right == 0 || bottom == 0 {
            return Ok(image::RgbaImage::new(PREVIEW_WIDTH, PREVIEW_HEIGHT));
        }
        PREVIEW_CONTENT_MARGIN
    } else {
        0
    };
    let content_width = right - left;
    let content_height = bottom - top;
    let target_width = PREVIEW_WIDTH - 2 * margin;
    let target_height = PREVIEW_HEIGHT - 2 * margin;
    // Exact integer form of min(target_width/content_width,
    // target_height/content_height): the limiting axis is exact, and floor on
    // the other axis never consumes the 5px border through float rounding.
    let (draw_width, draw_height) = if u64::from(target_width) * u64::from(content_height)
        <= u64::from(target_height) * u64::from(content_width)
    {
        (
            target_width,
            (u64::from(content_height) * u64::from(target_width) / u64::from(content_width)).max(1)
                as u32,
        )
    } else {
        (
            (u64::from(content_width) * u64::from(target_height) / u64::from(content_height)).max(1)
                as u32,
            target_height,
        )
    };
    let content = image::imageops::crop_imm(source, left, top, content_width, content_height);
    let resized = image::imageops::resize(
        &*content,
        draw_width,
        draw_height,
        image::imageops::FilterType::Lanczos3,
    );
    let mut canvas = image::RgbaImage::new(PREVIEW_WIDTH, PREVIEW_HEIGHT);
    image::imageops::replace(
        &mut canvas,
        &resized,
        i64::from((PREVIEW_WIDTH - draw_width) / 2),
        i64::from((PREVIEW_HEIGHT - draw_height) / 2),
    );
    Ok(canvas)
}

pub fn write_generated_preview(
    source: &image::RgbaImage,
    mode: PreviewImageMode,
    output: &Path,
) -> Result<()> {
    let image = normalize_preview_image(source, mode)?;
    write_preview_png(&image, output)
}

pub fn write_preview_png(image: &image::RgbaImage, output: &Path) -> Result<()> {
    write_preview_rgba_png(image.as_raw(), image.width(), image.height(), output)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectionPreviewOutput {
    pub found: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub output_path: Option<String>,
}

impl ProjectionPreviewOutput {
    fn missing() -> Self {
        Self {
            found: false,
            width: None,
            height: None,
            output_path: None,
        }
    }
}

/// Extracts the optional root `PreviewImageData` without materializing the
/// complete decompressed litematic. The PNG is published only after the gzip
/// member and NBT structure have both been fully validated.
pub fn extract_projection_preview(input: &Path, output: &Path) -> Result<ProjectionPreviewOutput> {
    require_absolute("projection preview input", input)?;
    require_absolute("projection preview output", output)?;
    if !input.is_file() {
        bail!(
            "projection preview input is not a file: {}",
            input.display()
        );
    }
    if paths_refer_to_same_file(input, output) {
        bail!("projection preview output must not overwrite its input");
    }

    let Some(preview) = load_litematic_preview_image(input)? else {
        return Ok(ProjectionPreviewOutput::missing());
    };
    write_projection_preview(preview, output)
}

pub fn extract_projection_preview_stream(
    input: impl std::io::BufRead,
    output: &Path,
) -> Result<ProjectionPreviewOutput> {
    require_absolute("projection preview output", output)?;
    let Some(preview) = crate::nbt::scan_preview_image_from_gzip(input)? else {
        return Ok(ProjectionPreviewOutput::missing());
    };
    write_projection_preview(preview, output)
}

fn write_projection_preview(
    preview: crate::nbt::LitematicPreviewImage,
    output: &Path,
) -> Result<ProjectionPreviewOutput> {
    write_preview_rgba_png(&preview.rgba, preview.width, preview.height, output)?;
    Ok(ProjectionPreviewOutput {
        found: true,
        width: Some(preview.width),
        height: Some(preview.height),
        output_path: Some(output.display().to_string()),
    })
}

fn write_preview_rgba_png(rgba: &[u8], width: u32, height: u32, output: &Path) -> Result<()> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| anyhow::anyhow!("projection preview output has no parent directory"))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "create projection preview output directory {}",
            parent.display()
        )
    })?;
    if output.is_dir() {
        bail!(
            "projection preview output is an existing directory: {}",
            output.display()
        );
    }

    let mut temporary = tempfile::NamedTempFile::new_in(parent).with_context(|| {
        format!(
            "create projection preview temporary file beside {}",
            output.display()
        )
    })?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        PngEncoder::new(&mut writer)
            .write_image(rgba, width, height, ExtendedColorType::Rgba8)
            .with_context(|| format!("encode projection preview PNG for {}", output.display()))?;
        writer.flush().with_context(|| {
            format!(
                "flush projection preview temporary PNG for {}",
                output.display()
            )
        })?;
    }
    temporary.as_file().sync_all().with_context(|| {
        format!(
            "sync projection preview temporary PNG for {}",
            output.display()
        )
    })?;
    temporary.persist(output).map_err(|error| {
        anyhow::anyhow!(
            "publish projection preview PNG {}: {}",
            output.display(),
            error.error
        )
    })?;

    Ok(())
}

fn require_absolute(label: &str, path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("{label} must be an absolute path: {}", path.display());
    }
    Ok(())
}

fn paths_refer_to_same_file(input: &Path, output: &Path) -> bool {
    if input == output {
        return true;
    }
    let Ok(input) = input.canonicalize() else {
        return false;
    };
    output.canonicalize().is_ok_and(|output| output == input)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::fs::File;

    const PREVIEW_NAME: &[u8] = b"PreviewImageData";

    pub(crate) fn alpha_bounds(image: &image::RgbaImage) -> Option<(u32, u32, u32, u32)> {
        let mut bounds = (image.width(), image.height(), 0, 0);
        for (x, y, pixel) in image.enumerate_pixels() {
            if pixel[3] > 0 {
                bounds.0 = bounds.0.min(x);
                bounds.1 = bounds.1.min(y);
                bounds.2 = bounds.2.max(x + 1);
                bounds.3 = bounds.3.max(y + 1);
            }
        }
        (bounds.2 > 0).then_some(bounds)
    }

    #[test]
    fn generated_alpha_previews_fit_towers_wide_buildings_and_normal_structures() {
        for (width, height, expected) in [
            (100, 1000, (148, 5, 171, 235)),
            (1000, 100, (5, 104, 315, 135)),
            (400, 300, (7, 5, 313, 235)),
            // Width > height is not itself a reason to constrain by width.
            (120, 100, (22, 5, 298, 235)),
        ] {
            let mut capture =
                image::RgbaImage::from_pixel(width + 80, height + 50, image::Rgba([240, 1, 2, 0]));
            let content =
                image::RgbaImage::from_pixel(width, height, image::Rgba([23, 91, 181, 255]));
            image::imageops::replace(&mut capture, &content, 31, 17);
            let fast =
                normalize_preview_image(&capture, PreviewImageMode::FastExperimental).unwrap();
            let standard = normalize_preview_image(&capture, PreviewImageMode::Standard).unwrap();
            assert_eq!(fast, standard, "Fast and Standard share one algorithm");
            assert_eq!(fast.dimensions(), (320, 240));
            assert_eq!(alpha_bounds(&fast), Some(expected), "{width}x{height}");
            assert_eq!(fast.get_pixel(0, 0).0, [0, 0, 0, 0]);
            assert_eq!(fast.get_pixel(160, 120).0, [23, 91, 181, 255]);
        }
    }

    #[test]
    fn alpha_boundary_includes_single_low_alpha_pixel_and_handles_empty_or_tiny_sources() {
        for size in [(0, 0), (2, 8), (400, 300)] {
            let empty = image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([255, 0, 255, 0]));
            let result = normalize_preview_image(&empty, PreviewImageMode::Standard).unwrap();
            assert_eq!(result.dimensions(), (320, 240));
            assert!(result.pixels().all(|p| p.0 == [0, 0, 0, 0]));
        }
        let mut single = image::RgbaImage::new(17, 19);
        single.put_pixel(3, 13, image::Rgba([11, 22, 33, 1]));
        let result = normalize_preview_image(&single, PreviewImageMode::FastExperimental).unwrap();
        assert_eq!(alpha_bounds(&result), Some((45, 5, 275, 235)));
        assert_eq!(result.get_pixel(160, 120).0, [11, 22, 33, 1]);
        let tiny = image::RgbaImage::from_pixel(2, 1, image::Rgba([0, 255, 70, 255]));
        assert_eq!(
            alpha_bounds(&normalize_preview_image(&tiny, PreviewImageMode::Standard).unwrap()),
            Some((5, 42, 315, 197))
        );
    }

    #[test]
    fn full_preview_crops_real_frame_to_four_three_without_alpha_trimming_or_border() {
        let mut wide = image::RgbaImage::from_pixel(640, 240, image::Rgba([120, 180, 240, 255]));
        for y in 0..240 {
            for x in 0..160 {
                wide.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
            for x in 480..640 {
                wide.put_pixel(x, y, image::Rgba([0, 255, 0, 255]));
            }
        }
        let result = normalize_preview_image(&wide, PreviewImageMode::Full).unwrap();
        assert_eq!(result.dimensions(), (320, 240));
        assert!(
            result.pixels().all(|p| p.0 == [120, 180, 240, 255]),
            "retain sky/background and crop equal left/right strips"
        );
        let tall = image::RgbaImage::from_fn(320, 480, |_, y| {
            if y < 120 {
                image::Rgba([255, 0, 0, 255])
            } else if y >= 360 {
                image::Rgba([0, 255, 0, 255])
            } else {
                image::Rgba([71, 81, 91, 255])
            }
        });
        let result = normalize_preview_image(&tall, PreviewImageMode::Full).unwrap();
        assert!(result.pixels().all(|p| p.0 == [71, 81, 91, 255]));
        // Full does not search for opaque content or invent opaque pixels.
        let transparent = image::RgbaImage::from_pixel(1, 2, image::Rgba([9, 19, 29, 0]));
        assert!(
            normalize_preview_image(&transparent, PreviewImageMode::Full)
                .unwrap()
                .pixels()
                .all(|p| p.0 == [9, 19, 29, 0])
        );
        for size in [(1, 1), (2, 3), (3, 2)] {
            let small =
                image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([73, 83, 93, 255]));
            let result = normalize_preview_image(&small, PreviewImageMode::Full).unwrap();
            assert_eq!(result.dimensions(), (320, 240));
            assert!(result.pixels().all(|p| p.0 == [73, 83, 93, 255]));
        }
    }

    #[test]
    fn generated_previews_share_png_encoder_and_fixed_dimensions() {
        let directory = tempfile::tempdir().unwrap();
        let source = image::RgbaImage::from_pixel(19, 27, image::Rgba([21, 51, 91, 173]));
        for mode in [
            PreviewImageMode::FastExperimental,
            PreviewImageMode::Standard,
            PreviewImageMode::Full,
        ] {
            let output = directory.path().join(format!("{mode:?}.png"));
            write_generated_preview(&source, mode, &output).unwrap();
            assert_eq!(&fs::read(&output).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
            let decoded = image::open(&output).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), (320, 240));
            assert_eq!(decoded, normalize_preview_image(&source, mode).unwrap());
        }
    }

    fn write_litematic(path: &Path, pixels: Option<&[u32]>) {
        let mut nbt = vec![10, 0, 0];
        if let Some(pixels) = pixels {
            nbt.push(11);
            nbt.extend_from_slice(&(PREVIEW_NAME.len() as u16).to_be_bytes());
            nbt.extend_from_slice(PREVIEW_NAME);
            nbt.extend_from_slice(&(pixels.len() as i32).to_be_bytes());
            for pixel in pixels {
                nbt.extend_from_slice(&pixel.to_be_bytes());
            }
        }
        nbt.push(0);
        let mut encoder = GzEncoder::new(File::create(path).unwrap(), Compression::default());
        encoder.write_all(&nbt).unwrap();
        encoder.finish().unwrap();
    }

    #[test]
    fn derived_cache_reuses_embedded_identity_and_invalidates_removed_preview() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("source.litematic");
        let cache = directory.path().join("user");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        write_litematic(
            &input,
            Some(&[0xff11_2233, 0xff44_5566, 0xff77_8899, 0xffaa_bbcc]),
        );
        let query =
            || serde_json::json!({"action":"readEmbedded","sourcePath":input,"storageDir":cache});
        let first = read_cached(&query(), &cancel).unwrap();
        let warm = read_cached(&query(), &cancel).unwrap();
        assert_eq!(first, warm);
        assert_eq!(first["origin"], "embedded");
        assert!(Path::new(first["preview_path"].as_str().unwrap()).is_file());
        let renamed = directory.path().join("renamed.litematic");
        fs::copy(&input, &renamed).unwrap();
        let renamed = read_cached(
            &serde_json::json!({"action":"readEmbedded","sourcePath":renamed,"storageDir":cache}),
            &cancel,
        )
        .unwrap();
        assert_eq!(renamed["source_identity"], first["source_identity"]);
        write_litematic(&input, None);
        assert!(read_cached(&query(), &cancel).unwrap().is_null());
    }

    #[test]
    fn visible_preview_batch_is_bounded_and_cancel_stops_before_io() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.litematic");
        write_litematic(&source, Some(&[0xff00_0000]));
        let cancel = std::sync::atomic::AtomicBool::new(true);
        let input = serde_json::json!({"action":"readEmbedded","sourcePath":source,"storageDir":directory.path().join("unused")});
        assert!(
            read_cached(&input, &cancel)
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(!directory.path().join("unused").exists());
        cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        let oversized = serde_json::json!({"action":"readEmbeddedBatch","sourcePaths":vec![source;33],"storageDir":directory.path()});
        assert!(
            read_cached(&oversized, &cancel)
                .unwrap_err()
                .to_string()
                .contains("bounded")
        );
    }

    #[test]
    fn extracts_png_and_reports_small_json_contract() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("scene.litematic");
        let output = directory.path().join("preview.png");
        write_litematic(
            &input,
            Some(&[0xff11_2233, 0x8044_5566, 0x0077_8899, 0xffaa_bbcc]),
        );

        let result = extract_projection_preview(&input, &output).unwrap();

        assert!(result.found);
        assert_eq!((result.width, result.height), (Some(2), Some(2)));
        assert_eq!(result.output_path, Some(output.display().to_string()));
        let decoded = image::open(&output).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert_eq!(
            decoded.as_raw(),
            &[
                0x11, 0x22, 0x33, 0xff, 0x44, 0x55, 0x66, 0x80, 0x77, 0x88, 0x99, 0x00, 0xaa, 0xbb,
                0xcc, 0xff,
            ]
        );
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["found"], true);
        assert_eq!(json["width"], 2);
        assert_eq!(json["height"], 2);
        assert!(json["output_path"].is_string());
    }

    #[test]
    fn extracts_new_rectangular_preview_and_legacy_140_square_through_file_and_stream() {
        let directory = tempfile::tempdir().unwrap();
        for (width, height) in [(320_u32, 240_u32), (140, 140)] {
            let input = directory.path().join(format!("{width}x{height}.litematic"));
            let pixels: Vec<_> = (0..width * height).map(|i| 0x8000_0000 | i).collect();
            write_litematic(&input, Some(&pixels));
            let output = directory.path().join(format!("{width}x{height}.png"));
            let report = extract_projection_preview(&input, &output).unwrap();
            assert_eq!((report.width, report.height), (Some(width), Some(height)));
            let stream_output = directory
                .path()
                .join(format!("{width}x{height}-stream.png"));
            let stream_report = extract_projection_preview_stream(
                std::io::BufReader::new(File::open(&input).unwrap()),
                &stream_output,
            )
            .unwrap();
            assert_eq!(
                (stream_report.width, stream_report.height),
                (Some(width), Some(height))
            );
            assert_eq!(
                fs::read(&output).unwrap(),
                fs::read(&stream_output).unwrap()
            );
            assert_eq!(
                image::open(output).unwrap().to_rgba8().dimensions(),
                (width, height)
            );
        }
    }

    #[test]
    fn missing_preview_succeeds_without_creating_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("scene.litematic");
        let output = directory.path().join("preview.png");
        write_litematic(&input, None);

        let result = extract_projection_preview(&input, &output).unwrap();

        assert_eq!(result, ProjectionPreviewOutput::missing());
        assert!(!output.exists());
    }

    #[test]
    fn invalid_input_never_replaces_existing_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("corrupt.litematic");
        let output = directory.path().join("preview.png");
        fs::write(&input, b"not gzip").unwrap();
        fs::write(&output, b"existing preview").unwrap();

        assert!(extract_projection_preview(&input, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"existing preview");
    }
}
