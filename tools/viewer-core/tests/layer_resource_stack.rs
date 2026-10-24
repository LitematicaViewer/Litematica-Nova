//! CPU-only production resource access. Small packs are synthetic; exact-version
//! vanilla PNGs are read through the candidate's actual 63-version asset archive.
use std::{ffi::OsString, fs, io::Write, path::Path};

use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};
use litematica_core::{
    game_data::GameDataContext,
    layer_resources::{LayerResourceContext, ResourceAccess},
};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const SNAPSHOT: &str = "NOVA_GAME_DATA_SNAPSHOT";
const SAMPLE: &str = "assets/nova_fixture/textures/block/sample.png";
const ADDED: &str = "assets/nova_fixture/models/block/added.json";
const WHITE_GLASS: &str = "assets/minecraft/textures/block/white_stained_glass.png";
const PALE_OAK: &str = "assets/minecraft/textures/block/pale_oak_planks.png";

struct RestoreSnapshot(Option<OsString>);
impl RestoreSnapshot {
    fn new() -> Self {
        Self(std::env::var_os(SNAPSHOT))
    }
    fn install(context: &GameDataContext) {
        // This integration binary has one test; no parallel test mutates or
        // reads its environment. Restore the caller's snapshot on every exit.
        unsafe {
            std::env::set_var(SNAPSHOT, serde_json::to_string(context).unwrap());
        }
    }
}
impl Drop for RestoreSnapshot {
    fn drop(&mut self) {
        unsafe {
            match self.0.take() {
                Some(value) => std::env::set_var(SNAPSHOT, value),
                None => std::env::remove_var(SNAPSHOT),
            }
        }
    }
}

fn png(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut output = Vec::new();
    PngEncoder::new(&mut output)
        .write_image(pixels, width, height, ColorType::Rgba8.into())
        .unwrap();
    output
}
fn write_asset(root: &Path, key: &str, bytes: &[u8]) {
    let path = root.join(key);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (key, bytes) in entries {
        zip.start_file(*key, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

/// Minimal, explicitly synthetic imported package. ResourceAccess itself reads
/// the real package layout; this does not stub or copy the production resolver.
fn selected_package(root: &Path, version: &str, white: &[u8]) -> GameDataContext {
    fs::create_dir_all(root).unwrap();
    let root = root.canonicalize().unwrap();
    write_asset(&root, WHITE_GLASS, white);
    write_asset(&root, SAMPLE, white);
    write_asset(&root, PALE_OAK, white);
    let index = serde_json::json!({
        (WHITE_GLASS): { "bytes": white.len(), "sha256": format!("{:x}", Sha256::digest(white)) },
        (SAMPLE): { "bytes": white.len(), "sha256": format!("{:x}", Sha256::digest(white)) },
        (PALE_OAK): { "bytes": white.len(), "sha256": format!("{:x}", Sha256::digest(white)) },
    });
    let index_bytes = serde_json::to_vec(&index).unwrap();
    let mut package_hash = Sha256::new();
    package_hash.update(format!("nova.game-data.v1\0{version}\0"));
    package_hash.update(&index_bytes);
    fs::write(root.join("index.json"), &index_bytes).unwrap();
    write_zip(
        &root.join("resources.jar"),
        &[(WHITE_GLASS, white), (SAMPLE, white), (PALE_OAK, white)],
    );
    let context = GameDataContext {
        schema_version: 1,
        id: format!("layer-resource-fixture-{}", version.replace('.', "-")),
        game_version: version.into(),
        content_id: format!("{:x}", package_hash.finalize()),
        root: root.clone(),
        resource_jar: root.join("resources.jar"),
        source: "synthetic-test-package".into(),
    };
    let mut manifest = serde_json::to_value(&context).unwrap();
    let object = manifest.as_object_mut().unwrap();
    object.insert("imported_ms".into(), 0.into());
    object.insert(
        "source_jar_sha256".into(),
        format!(
            "{:x}",
            Sha256::digest(fs::read(&context.resource_jar).unwrap())
        )
        .into(),
    );
    object.insert("data_version".into(), serde_json::Value::Null);
    object.insert("resource_counts".into(), serde_json::json!({"assets": 3}));
    object.insert("missing".into(), serde_json::json!([]));
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    context
}

#[test]
fn exact_versions_pack_order_invalidation_recovery_and_transparent_pixels() {
    let temp = tempfile::tempdir().unwrap();
    let _restore = RestoreSnapshot::new();
    let rgba = [11, 23, 47, 0, 31, 63, 127, 96];
    let transparent_png = png(&rgba, 2, 1);
    let selected = selected_package(
        &temp.path().join("selected-future"),
        "fixture-future",
        &transparent_png,
    );
    RestoreSnapshot::install(&selected);
    assert_eq!(ResourceAccess::identity(&selected), selected.content_id);
    assert_eq!(
        selected.read(WHITE_GLASS).unwrap().unwrap(),
        transparent_png
    );

    // These identities are pinned to the actual archive supplied with this
    // candidate, not to guessed model names or a latest-version fallback.
    for (version, source_sha, png_sha) in [
        (
            "1.20.4",
            "9221ab461a491bf9661cd8e773a5e662aaa43d600fa7970b8c12bbfb0431b838",
            "696d5d7f921d0e39889c3e051e2cb9b1e740cbf9c1468ab8ff31bde854721693",
        ),
        (
            "1.21",
            "4b56bc9b44de7e88f9fbb52e716c18240da34469fcbafa5ed2e75b04d60617e8",
            "696d5d7f921d0e39889c3e051e2cb9b1e740cbf9c1468ab8ff31bde854721693",
        ),
        (
            "26.3",
            "2484967e9fc36516dc46819471390fd42cd5edddc73cca7cf81025903dae74ba",
            "2d4504957fa123a6e9f35d93e05f94fb053ec213541f01fcecbc0097eaa5df75",
        ),
    ] {
        let context = LayerResourceContext::resolve(version, &[]).unwrap();
        assert_eq!(
            context.origin(WHITE_GLASS).unwrap().unwrap(),
            format!("vanilla:{version}:{source_sha}")
        );
        let bytes = context.read(WHITE_GLASS).unwrap().unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            png_sha,
            "exact {version} PNG bytes"
        );
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert!(image.width() > 0 && image.height() > 0);
        assert!(
            image.pixels().any(|pixel| pixel[3] > 0 && pixel[3] < 255),
            "{version} glass keeps genuine partial alpha"
        );
        assert!(context.is_current().unwrap());
        if version != "26.3" {
            assert!(
                context.read(PALE_OAK).unwrap().is_none(),
                "{version} must not borrow future selected/archive assets"
            );
            assert!(context.origin(PALE_OAK).unwrap().is_none());
        } else {
            assert_ne!(
                context.read(PALE_OAK).unwrap().unwrap(),
                transparent_png,
                "26.3 uses its own archive, not the selected future package"
            );
        }
    }
    assert!(LayerResourceContext::resolve("fixture-version-not-in-archive", &[]).is_err());

    let low = temp.path().join("low.zip");
    let high = temp.path().join("high-directory");
    let opaque_png = png(&[210, 40, 20, 255], 1, 1);
    write_zip(&low, &[(SAMPLE, &opaque_png)]);
    write_asset(&high, SAMPLE, &transparent_png);
    let low_then_high =
        LayerResourceContext::resolve("1.21", &[low.clone(), high.clone()]).unwrap();
    let high_then_low =
        LayerResourceContext::resolve("1.21", &[high.clone(), low.clone()]).unwrap();
    assert_eq!(
        low_then_high.read(SAMPLE).unwrap().unwrap(),
        transparent_png,
        "last selected pack wins"
    );
    assert_eq!(high_then_low.read(SAMPLE).unwrap().unwrap(), opaque_png);
    assert_ne!(
        low_then_high.identity(),
        high_then_low.identity(),
        "stack order participates in identity"
    );
    assert!(
        low_then_high
            .origin(SAMPLE)
            .unwrap()
            .unwrap()
            .starts_with("pack:")
    );
    assert_eq!(
        image::load_from_memory(&low_then_high.read(SAMPLE).unwrap().unwrap())
            .unwrap()
            .to_rgba8()
            .as_raw(),
        &rgba,
        "transparent RGB/alpha survive real pack retrieval and PNG decoding"
    );

    let original_zip = LayerResourceContext::resolve("1.21", &[low.clone()]).unwrap();
    // An extra stored entry guarantees a different length even on filesystems
    // whose modification timestamp cannot distinguish two immediate writes.
    write_zip(&low, &[(SAMPLE, &transparent_png), (ADDED, br#"{"parent":"minecraft:block/cube_all","padding":"changed zip resource identity"}"#)]);
    assert!(!original_zip.is_current().unwrap());
    let replaced_zip = LayerResourceContext::resolve("1.21", &[low.clone()]).unwrap();
    assert_ne!(original_zip.identity(), replaced_zip.identity());
    assert_eq!(replaced_zip.read(SAMPLE).unwrap().unwrap(), transparent_png);

    let original_directory = LayerResourceContext::resolve("1.21", &[high.clone()]).unwrap();
    write_asset(&high, ADDED, br#"{"parent":"minecraft:block/cube_all"}"#);
    assert!(
        !original_directory.is_current().unwrap(),
        "new directory assets invalidate the old context"
    );
    let added_directory = LayerResourceContext::resolve("1.21", &[high.clone()]).unwrap();
    assert_ne!(original_directory.identity(), added_directory.identity());
    assert!(added_directory.read(ADDED).unwrap().is_some());
    write_asset(
        &high,
        ADDED,
        br#"{"parent":"minecraft:block/cube_all","textures":{"all":"nova_fixture:block/changed"}}"#,
    );
    assert!(
        !added_directory.is_current().unwrap(),
        "changed directory bytes invalidate the old context"
    );
    let changed_directory = LayerResourceContext::resolve("1.21", &[high.clone()]).unwrap();
    assert_ne!(added_directory.identity(), changed_directory.identity());
    assert!(
        String::from_utf8(changed_directory.read(ADDED).unwrap().unwrap())
            .unwrap()
            .contains("changed")
    );

    let unavailable = temp.path().join("temporarily-missing.zip");
    assert!(LayerResourceContext::resolve("1.21", &[unavailable.clone()]).is_err());
    write_zip(&unavailable, &[(SAMPLE, &opaque_png)]);
    let recovered_missing = LayerResourceContext::resolve("1.21", &[unavailable.clone()]).unwrap();
    assert_eq!(
        recovered_missing.read(SAMPLE).unwrap().unwrap(),
        opaque_png,
        "missing pack failures are not negative-cached"
    );
    fs::write(&unavailable, b"a temporary incomplete zip download").unwrap();
    assert!(LayerResourceContext::resolve("1.21", &[unavailable.clone()]).is_err());
    write_zip(&unavailable, &[(SAMPLE, &transparent_png), (ADDED, b"{}")]);
    let recovered_zip = LayerResourceContext::resolve("1.21", &[unavailable]).unwrap();
    assert_eq!(
        recovered_zip.read(SAMPLE).unwrap().unwrap(),
        transparent_png,
        "decode/open failures are not negative-cached"
    );

    // Matching selected JAR metadata is authoritative. Its omissions must not
    // silently be filled by the unmodified vanilla archive for that version.
    let exact_selected = selected_package(
        &temp.path().join("selected-exact"),
        "26.3",
        &transparent_png,
    );
    RestoreSnapshot::install(&exact_selected);
    let exact = LayerResourceContext::resolve("26.3", &[]).unwrap();
    assert_eq!(
        exact.origin(WHITE_GLASS).unwrap().unwrap(),
        format!("game:{}", exact_selected.content_id)
    );
    assert_eq!(exact.read(WHITE_GLASS).unwrap().unwrap(), transparent_png);
    assert!(
        exact
            .read("assets/minecraft/textures/block/stone.png")
            .unwrap()
            .is_none()
    );
    assert!(
        exact
            .origin("assets/minecraft/textures/block/stone.png")
            .unwrap()
            .is_none(),
        "origin and read agree on genuine selected-package omissions"
    );
    assert!(
        !changed_directory.is_current().unwrap(),
        "active package changes retire an old resource scope"
    );
}
