use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ColorType, ImageEncoder};
use litematica_core::resource_pack_32x::{
    ALGORITHM_ID, derive_resource_pack_32x, directory_tree_v1_sha256,
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "lba-resource-pack-32x-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated test root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_rgba_png(path: &Path, width: u32, height: u32, pixels: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create png parent");
    }
    let output = fs::File::create(path).expect("create png");
    PngEncoder::new_with_quality(
        BufWriter::new(output),
        CompressionType::Best,
        FilterType::NoFilter,
    )
    .write_image(pixels, width, height, ColorType::Rgba8.into())
    .expect("encode png");
}

fn write_rgba16_png(path: &Path, width: u32, height: u32, pixels: &[u16]) {
    let native_bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|sample| sample.to_ne_bytes())
        .collect();
    let output = fs::File::create(path).expect("create 16-bit png");
    PngEncoder::new_with_quality(
        BufWriter::new(output),
        CompressionType::Best,
        FilterType::NoFilter,
    )
    .write_image(
        &native_bytes,
        width,
        height,
        image::ExtendedColorType::Rgba16,
    )
    .expect("encode 16-bit png");
}

#[test]
fn downsamples_in_linear_light_with_premultiplied_alpha_and_copies_metadata() {
    let temp = TempRoot::new();
    let input = temp.path().join("faithful-64x");
    let output = temp.path().join("faithful-32x");
    fs::create_dir_all(input.join("assets/minecraft/textures/block")).unwrap();

    write_rgba_png(
        &input.join("assets/minecraft/textures/block/linear.png"),
        2,
        2,
        &[
            0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
        ],
    );
    write_rgba_png(
        &input.join("assets/minecraft/textures/block/alpha.png"),
        2,
        2,
        &[255, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0, 0, 0, 255, 255],
    );
    let metadata = br#"{"animation":{"frametime":2,"width":2,"height":2,"frames":[0]}}"#;
    fs::write(
        input.join("assets/minecraft/textures/block/alpha.png.mcmeta"),
        metadata,
    )
    .unwrap();
    fs::write(input.join("pack.mcmeta"), b"pack metadata\n").unwrap();

    let report = derive_resource_pack_32x(&input, &output).expect("derive resource pack");

    assert_eq!(report.algorithm, ALGORITHM_ID);
    assert_eq!(report.file_count, 4);
    assert_eq!(report.png_count, 2);
    assert_eq!(report.animation_metadata_count, 1);
    assert_eq!(report.copied_count, 1);
    assert_eq!(
        report.tree_sha256,
        directory_tree_v1_sha256(&output).unwrap()
    );
    let derived_metadata: serde_json::Value = serde_json::from_slice(
        &fs::read(output.join("assets/minecraft/textures/block/alpha.png.mcmeta")).unwrap(),
    )
    .unwrap();
    assert_eq!(derived_metadata["animation"]["width"], 1);
    assert_eq!(derived_metadata["animation"]["height"], 1);
    assert_eq!(derived_metadata["animation"]["frametime"], 2);
    assert_eq!(
        derived_metadata["animation"]["frames"],
        serde_json::json!([0])
    );
    assert_eq!(
        fs::read(output.join("pack.mcmeta")).unwrap(),
        b"pack metadata\n"
    );

    let linear = image::open(output.join("assets/minecraft/textures/block/linear.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(linear.dimensions(), (1, 1));
    assert_eq!(linear.get_pixel(0, 0).0, [188, 188, 188, 255]);

    let alpha = image::open(output.join("assets/minecraft/textures/block/alpha.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(alpha.dimensions(), (1, 1));
    assert_eq!(alpha.get_pixel(0, 0).0, [0, 0, 255, 128]);
}

#[test]
fn rejects_odd_png_dimensions_without_installing_a_partial_tree() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir(&input).unwrap();
    write_rgba_png(&input.join("odd.png"), 3, 2, &[255; 3 * 2 * 4]);
    fs::write(input.join("copied-before-error.txt"), b"must not leak").unwrap();

    let error = derive_resource_pack_32x(&input, &output)
        .expect_err("odd dimensions must be rejected")
        .to_string();

    assert!(error.contains("odd.png"), "unexpected error: {error}");
    assert!(error.contains("even"), "unexpected error: {error}");
    assert!(!output.exists(), "failed derivation installed output tree");
    let leftovers: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().contains("partial"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temporary trees leaked: {leftovers:?}"
    );
}

#[test]
fn rejects_sixteen_bit_png_samples_instead_of_silently_truncating_them() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir(&input).unwrap();
    write_rgba16_png(
        &input.join("high-precision.png"),
        2,
        2,
        &[u16::MAX; 2 * 2 * 4],
    );

    let error = derive_resource_pack_32x(&input, &output)
        .expect_err("16-bit input must not be truncated to RGBA8")
        .to_string();

    assert!(error.contains("8-bit"), "unexpected error: {error}");
    assert!(error.contains("high-precision.png"));
    assert!(!output.exists());
}

#[test]
fn rejects_odd_explicit_animation_dimensions_atomically() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir(&input).unwrap();
    write_rgba_png(&input.join("animated.png"), 4, 4, &[255; 4 * 4 * 4]);
    fs::write(
        input.join("animated.png.mcmeta"),
        br#"{"animation":{"width":3,"height":4}}"#,
    )
    .unwrap();

    let error = derive_resource_pack_32x(&input, &output)
        .expect_err("odd animation dimensions must be rejected")
        .to_string();

    assert!(
        error.contains("animation.width"),
        "unexpected error: {error}"
    );
    assert!(!output.exists(), "failed derivation installed output tree");
}

#[test]
fn preserves_gui_scaling_metadata_byte_for_byte() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir(&input).unwrap();
    write_rgba_png(&input.join("button.png"), 4, 4, &[255; 4 * 4 * 4]);
    let metadata =
        br#"{"gui":{"scaling":{"type":"nine_slice","width":200,"height":20,"border":2}}}"#;
    fs::write(input.join("button.png.mcmeta"), metadata).unwrap();

    let report = derive_resource_pack_32x(&input, &output).unwrap();

    assert_eq!(report.animation_metadata_count, 0);
    assert_eq!(report.copied_count, 1);
    assert_eq!(
        fs::read(output.join("button.png.mcmeta")).unwrap(),
        metadata
    );
}

#[test]
fn downsamples_each_default_sized_animation_frame_without_cross_frame_bleed() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir(&input).unwrap();
    write_rgba_png(
        &input.join("animated.png"),
        2,
        4,
        &[
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0,
            255, 255, 0, 0, 255, 255, 0, 0, 255, 255,
        ],
    );
    let metadata = br#"{"animation":{"frames":[1,0],"frametime":2}}"#;
    fs::write(input.join("animated.png.mcmeta"), metadata).unwrap();

    let report = derive_resource_pack_32x(&input, &output).unwrap();

    assert_eq!(report.animation_metadata_count, 0);
    assert_eq!(
        fs::read(output.join("animated.png.mcmeta")).unwrap(),
        metadata
    );
    let derived = image::open(output.join("animated.png")).unwrap().to_rgba8();
    assert_eq!(derived.dimensions(), (1, 2));
    assert_eq!(derived.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(derived.get_pixel(0, 1).0, [0, 0, 255, 255]);
}

#[test]
fn downsamples_each_explicit_animation_grid_cell_and_scales_its_metadata() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir(&input).unwrap();
    write_rgba_png(
        &input.join("animated.png"),
        4,
        2,
        &[
            255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 0,
            0, 255, 0, 255, 0, 255, 0, 255, 0, 255,
        ],
    );
    fs::write(
        input.join("animated.png.mcmeta"),
        br#"{"animation":{"width":2,"height":2,"frames":[1,0]}}"#,
    )
    .unwrap();

    let report = derive_resource_pack_32x(&input, &output).unwrap();

    assert_eq!(report.animation_metadata_count, 1);
    let derived = image::open(output.join("animated.png")).unwrap().to_rgba8();
    assert_eq!(derived.dimensions(), (2, 1));
    assert_eq!(derived.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(derived.get_pixel(1, 0).0, [0, 255, 0, 255]);
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("animated.png.mcmeta")).unwrap()).unwrap();
    assert_eq!(metadata["animation"]["width"], 1);
    assert_eq!(metadata["animation"]["height"], 1);
    assert_eq!(metadata["animation"]["frames"], serde_json::json!([1, 0]));
}

#[test]
fn animation_width_only_and_height_only_follow_mc26_frame_size_rules() {
    let temp = TempRoot::new();

    let width_input = temp.path().join("width-input");
    let width_output = temp.path().join("width-output");
    fs::create_dir(&width_input).unwrap();
    write_rgba_png(&width_input.join("animated.png"), 4, 2, &[255; 4 * 2 * 4]);
    fs::write(
        width_input.join("animated.png.mcmeta"),
        br#"{"animation":{"width":2,"frames":[0,1]}}"#,
    )
    .unwrap();
    derive_resource_pack_32x(&width_input, &width_output).unwrap();
    let width_metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(width_output.join("animated.png.mcmeta")).unwrap())
            .unwrap();
    assert_eq!(width_metadata["animation"]["width"], 1);
    assert!(width_metadata["animation"].get("height").is_none());
    assert_eq!(
        image::open(width_output.join("animated.png"))
            .unwrap()
            .to_rgba8()
            .dimensions(),
        (2, 1)
    );

    let height_input = temp.path().join("height-input");
    let height_output = temp.path().join("height-output");
    fs::create_dir(&height_input).unwrap();
    write_rgba_png(&height_input.join("animated.png"), 2, 4, &[255; 2 * 4 * 4]);
    fs::write(
        height_input.join("animated.png.mcmeta"),
        br#"{"animation":{"height":2,"frames":[0,1]}}"#,
    )
    .unwrap();
    derive_resource_pack_32x(&height_input, &height_output).unwrap();
    let height_metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(height_output.join("animated.png.mcmeta")).unwrap())
            .unwrap();
    assert!(height_metadata["animation"].get("width").is_none());
    assert_eq!(height_metadata["animation"]["height"], 1);
    assert_eq!(
        image::open(height_output.join("animated.png"))
            .unwrap()
            .to_rgba8()
            .dimensions(),
        (1, 2)
    );
}

#[test]
fn rejects_animation_grid_that_does_not_divide_the_png() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir(&input).unwrap();
    write_rgba_png(&input.join("animated.png"), 8, 4, &[255; 8 * 4 * 4]);
    fs::write(
        input.join("animated.png.mcmeta"),
        br#"{"animation":{"width":6,"height":4}}"#,
    )
    .unwrap();

    let error = derive_resource_pack_32x(&input, &output)
        .expect_err("frame grid must divide the PNG")
        .to_string();

    assert!(error.contains("multiples"), "unexpected error: {error}");
    assert!(!output.exists());
}

#[test]
fn refuses_to_overwrite_an_existing_output() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir(&input).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(output.join("sentinel"), b"keep").unwrap();

    let error = derive_resource_pack_32x(&input, &output)
        .expect_err("existing output must be rejected")
        .to_string();

    assert!(
        error.contains("already exists"),
        "unexpected error: {error}"
    );
    assert_eq!(fs::read(output.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn directory_tree_v1_hash_matches_the_frozen_known_vector() {
    let temp = TempRoot::new();
    fs::create_dir(temp.path().join("nested")).unwrap();
    fs::write(temp.path().join("a.txt"), b"A\n").unwrap();
    fs::write(temp.path().join("nested/b.bin"), [0, 1, 2, 255]).unwrap();

    assert_eq!(
        directory_tree_v1_sha256(temp.path()).unwrap(),
        "f0d25e78d75e7856fecc3254612246d555ff55c35d356bd4be74b48738df3095"
    );
}

#[test]
fn rejects_output_nested_inside_the_input_tree() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let output = input.join("generated/32x");
    fs::create_dir(&input).unwrap();

    let error = derive_resource_pack_32x(&input, &output)
        .expect_err("nested output would recursively consume itself")
        .to_string();

    assert!(error.contains("overlap"), "unexpected error: {error}");
    assert!(!output.exists());
}

#[cfg(windows)]
#[test]
fn rejects_output_parent_junction_that_resolves_inside_the_input_tree() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let junction_target = input.join("nested");
    let junction = temp.path().join("input-alias");
    fs::create_dir_all(&junction_target).unwrap();
    let escaped_junction = junction.to_string_lossy().replace('\'', "''");
    let escaped_target = junction_target.to_string_lossy().replace('\'', "''");
    let script = format!(
        "New-Item -ItemType Junction -Path '{escaped_junction}' -Target '{escaped_target}' | Out-Null"
    );
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .expect("create test junction");
    assert!(
        status.success(),
        "PowerShell failed to create test junction"
    );

    let output = junction.join("generated-32x");
    let result = derive_resource_pack_32x(&input, &output);
    fs::remove_dir(&junction).expect("remove test junction without traversing it");
    let error = result
        .expect_err("canonical output parent resolves inside input")
        .to_string();

    assert!(error.contains("overlap"), "unexpected error: {error}");
    assert!(!junction_target.join("generated-32x").exists());
}

#[test]
fn output_png_encoding_is_byte_deterministic() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::create_dir(&input).unwrap();
    write_rgba_png(
        &input.join("texture.png"),
        4,
        2,
        &[
            0, 0, 0, 255, 64, 32, 16, 192, 128, 64, 32, 128, 255, 255, 255, 0, 255, 255, 255, 0,
            128, 64, 32, 128, 64, 32, 16, 192, 0, 0, 0, 255,
        ],
    );

    let first_report = derive_resource_pack_32x(&input, &first).unwrap();
    let second_report = derive_resource_pack_32x(&input, &second).unwrap();

    assert_eq!(first_report.tree_sha256, second_report.tree_sha256);
    assert_eq!(
        fs::read(first.join("texture.png")).unwrap(),
        fs::read(second.join("texture.png")).unwrap()
    );
}

#[test]
fn cli_derives_the_tree_and_emits_a_machine_readable_report() {
    let temp = TempRoot::new();
    let input = temp.path().join("input");
    let first_output = temp.path().join("first-output");
    let second_output = temp.path().join("second-output");
    fs::create_dir(&input).unwrap();
    write_rgba_png(&input.join("texture.png"), 2, 2, &[64; 2 * 2 * 4]);

    let binary = option_env!("CARGO_BIN_EXE_derive_resource_pack_32x")
        .expect("derive_resource_pack_32x binary target must exist");
    let run = |output: &Path| {
        let result = std::process::Command::new(binary)
            .arg("--input")
            .arg(&input)
            .arg("--output")
            .arg(output)
            .output()
            .expect("run derivation CLI");
        assert!(
            result.status.success(),
            "CLI failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&result.stdout)
            .expect("CLI stdout must be one JSON report")
    };
    let report = run(&first_output);
    let second_report = run(&second_output);
    assert_eq!(report["algorithm"], ALGORITHM_ID);
    assert_eq!(report["file_count"], 1);
    assert_eq!(report["png_count"], 1);
    assert_eq!(report["animation_metadata_count"], 0);
    assert_eq!(report["copied_count"], 0);
    assert_eq!(
        report["tree_sha256"],
        directory_tree_v1_sha256(&first_output).unwrap()
    );
    assert_eq!(report, second_report);
    assert_eq!(
        fs::read(first_output.join("texture.png")).unwrap(),
        fs::read(second_output.join("texture.png")).unwrap()
    );
}
