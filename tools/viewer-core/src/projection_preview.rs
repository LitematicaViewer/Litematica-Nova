use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use serde::Serialize;

use crate::nbt::load_litematic_preview_image;

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

pub fn extract_projection_preview_stream(input: impl std::io::BufRead, output: &Path) -> Result<ProjectionPreviewOutput> {
    require_absolute("projection preview output", output)?;
    let Some(preview) = crate::nbt::scan_preview_image_from_gzip(input)? else { return Ok(ProjectionPreviewOutput::missing()); };
    write_projection_preview(preview, output)
}

fn write_projection_preview(preview: crate::nbt::LitematicPreviewImage, output: &Path) -> Result<ProjectionPreviewOutput> {
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
            .write_image(
                &preview.rgba,
                preview.width,
                preview.height,
                ExtendedColorType::Rgba8,
            )
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

    Ok(ProjectionPreviewOutput {
        found: true,
        width: Some(preview.width),
        height: Some(preview.height),
        output_path: Some(output.display().to_string()),
    })
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
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::fs::File;

    const PREVIEW_NAME: &[u8] = b"PreviewImageData";

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
