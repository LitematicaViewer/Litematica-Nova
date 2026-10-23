use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use fastnbt::{IntArray, Value};
use std::io::{Read, Write};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use crate::projection_preview::{PreviewImageMode, embedded_preview_dimensions, normalize_preview_image};



#[derive(Debug, Deserialize)]
pub struct MetadataEditPatch {
    pub name: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub time_created: Option<i64>,
    pub time_modified: Option<i64>,
    pub litematic_version: Option<i32>,
    pub litematic_subversion: Option<i32>,
    pub minecraft_data_version: Option<i32>,
    pub preview_image_data: Option<Vec<i32>>,
    pub preview_image_path: Option<String>,
    pub preview_image_mode: Option<PreviewImageMode>,
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub regions: Vec<RegionRenamePatch>,
}

#[derive(Debug, Deserialize)]
pub struct RegionRenamePatch {
    pub old_name: String,
    pub new_name: String,
}

#[derive(Debug, Serialize)]
pub struct MetadataEditSummary {
    pub input: String,
    pub output: String,
    pub backup_path: Option<String>,
    pub changed_regions: Vec<RegionRenamePatchOutput>,
}

#[derive(Debug, Serialize)]
pub struct RegionRenamePatchOutput {
    pub old_name: String,
    pub new_name: String,
}

pub fn edit_metadata(
    input: &Path,
    output: Option<&Path>,
    patch_path: &Path,
) -> Result<MetadataEditSummary> {
    let patch_text = fs::read_to_string(patch_path)
        .with_context(|| format!("failed to read patch {}", patch_path.display()))?;
    let patch: MetadataEditPatch = serde_json::from_str(patch_text.trim_start_matches('\u{feff}'))
        .with_context(|| format!("failed to parse patch {}", patch_path.display()))?;
    // Value retains unknown root, region, entity and metadata fields. The typed
    // scene representation is deliberately not used for editing persistent NBT.
    let original = fs::read(input)?;
    let mut plain = Vec::new();
    GzDecoder::new(original.as_slice()).read_to_end(&mut plain)?;
    let mut value: Value = fastnbt::from_bytes(&plain)?;
    let Value::Compound(root) = &mut value else { bail!("projection root must be a compound"); };
    if let Some(version) = patch.minecraft_data_version {
        if root.get("MinecraftDataVersion") != Some(&Value::Int(version)) {
            bail!("Minecraft 数据版本变更须使用版本转换：先转换方块状态及相关 NBT，再另存新文件");
        }
    }
    if let Some(version) = patch.litematic_version { root.insert("Version".into(), Value::Int(version)); }
    if let Some(version) = patch.litematic_subversion { root.insert("SubVersion".into(), Value::Int(version)); }
    let preview = if let Some(path) = patch.preview_image_path.as_deref() {
        Some(load_preview_image_data(Path::new(path), patch.preview_image_mode.unwrap_or_default())?)
    } else if let Some(pixels) = patch.preview_image_data {
        if !pixels.is_empty() && !embedded_preview_dimensions(pixels.len() as u64)
            .is_some_and(|(width, height)| width <= 4096 && height <= 4096) {
            bail!("preview_image_data must be a 320x240 or legacy square ARGB pixel array with edge at most 4096");
        }
        Some(IntArray::new(pixels))
    } else { None };
    if preview.is_some() { root.remove("PreviewImageData"); }
    let metadata = root.entry("Metadata".into()).or_insert_with(|| Value::Compound(Default::default()));
    let Value::Compound(metadata) = metadata else { bail!("Metadata must be a compound"); };
    if let Some(text) = patch.name { metadata.insert("Name".into(), Value::String(text)); }
    if let Some(text) = patch.author { metadata.insert("Author".into(), Value::String(text)); }
    // NovaTags lives beside Name/Author/Description as `Metadata.NovaTags`,
    // an untyped NBT List<String>. Omitting `tags` leaves any existing value
    // untouched; a missing NovaTags stays absent instead of forcing a default.
    //
    // E.8 / 裁决 2: the library's `LibraryEntry.tags` is authoritative, but a
    // third-party tool may drop an unknown `NovaTags` field. No matter what the
    // verification concludes we *always* mirror a readable tag summary into
    // `Description` (the file is secondary), stripping any previously written
    // summary first so reapplying tags is idempotent.
    match patch.tags {
        Some(tags) => {
            metadata.insert(
                "NovaTags".into(),
                Value::List(tags.iter().cloned().map(Value::String).collect()),
            );
            let base = patch
                .description
                .clone()
                .or_else(|| match metadata.get("Description") {
                    Some(Value::String(text)) => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            metadata.insert(
                "Description".into(),
                Value::String(merge_tags_into_description(&base, &tags)),
            );
        }
        None => {
            if let Some(text) = patch.description {
                metadata.insert("Description".into(), Value::String(text));
            }
        }
    }
    for (name, time) in [("TimeCreated", patch.time_created), ("TimeModified", patch.time_modified)] {
        if let Some(time) = time { metadata.insert(name.into(), Value::Long(time)); }
    }
    if let Some(pixels) = preview {
        if pixels.is_empty() { metadata.remove("PreviewImageData"); }
        else { metadata.insert("PreviewImageData".into(), Value::IntArray(pixels)); }
    }
    let mut changed_regions = Vec::new();
    if !patch.regions.is_empty() {
        let Some(Value::Compound(regions)) = root.get_mut("Regions") else { bail!("Regions must be a compound"); };
        let mut targets = BTreeSet::new();
        let mut lookup = BTreeMap::new();
        for rename in &patch.regions {
            let next = rename.new_name.trim();
            if next.is_empty() || !regions.contains_key(&rename.old_name) || !targets.insert(next.to_owned())
                || lookup.insert(rename.old_name.clone(), next.to_owned()).is_some() {
                bail!("invalid or duplicate region rename: {}", rename.old_name);
            }
        }
        let mut next = std::collections::HashMap::new();
        for (name, region) in std::mem::take(regions) {
            let target = lookup.get(&name).cloned().unwrap_or(name);
            if next.insert(target.clone(), region).is_some() { bail!("duplicate region name after rename: {target}"); }
        }
        *regions = next;
        changed_regions = patch.regions.iter().map(|rename| RegionRenamePatchOutput {
            old_name: rename.old_name.clone(), new_name: rename.new_name.trim().to_owned(),
        }).collect();
    }
    let target = output.unwrap_or(input);
    if output.is_some() && target.exists() { bail!("refusing to overwrite existing output file: {}", target.display()); }
    if output.is_none() && fs::metadata(input)?.permissions().readonly() { bail!("投影为只读文件，请另存后重试"); }
    let parent = target.parent().context("projection output has no parent")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut gzip = GzEncoder::new(temporary.as_file_mut(), Compression::default());
        gzip.write_all(&fastnbt::to_bytes(&value)?)?;
        gzip.finish()?;
    }
    temporary.as_file().sync_all()?;
    // Reopen and compare the complete semantic document before committing it.
    let mut check = Vec::new();
    GzDecoder::new(fs::File::open(temporary.path())?).read_to_end(&mut check)?;
    let reopened: Value = fastnbt::from_bytes(&check)?;
    anyhow::ensure!(reopened == value, "metadata write-back verification failed");
    anyhow::ensure!(fs::read(input)? == original, "投影在写入期间已改变，请重新加载");
    let backup_path = None;
    if output.is_some() { temporary.persist_noclobber(target).map_err(|error| error.error)?; }
    else { temporary.persist(target).map_err(|error| error.error)?; }

    Ok(MetadataEditSummary {
        input: input.display().to_string(),
        output: target.display().to_string(),
        backup_path,
        changed_regions,
    })
}

fn load_preview_image_data(path: &Path, mode: PreviewImageMode) -> Result<IntArray> {
    let image = image::open(path).with_context(|| format!("failed to open preview image {}", path.display()))?;
    let resized = normalize_preview_image(&image.to_rgba8(), mode)?;
    let pixels = resized.pixels().map(|pixel| {
        let [r, g, b, a] = pixel.0;
        (((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32) as i32
    }).collect();
    Ok(IntArray::new(pixels))
}

#[cfg(test)]
mod version_guard_tests {
    use super::*;
    use fastnbt::Value;
    use flate2::{write::GzEncoder, Compression};
    use std::collections::HashMap;
    use std::io::Write;

    #[test]
    fn version_change_fails_before_output_and_preserves_input() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("converted.litematic");
        let patch = dir.path().join("patch.json");
        let root = Value::Compound(HashMap::from([
            ("Metadata".into(), Value::Compound(HashMap::new())),
            ("Regions".into(), Value::Compound(HashMap::new())),
            ("Version".into(), Value::Int(6)),
            ("MinecraftDataVersion".into(), Value::Int(5023)),
        ]));
        let nbt = fastnbt::to_bytes(&root).unwrap();
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&nbt).unwrap();
        let original = gzip.finish().unwrap();
        fs::write(&input, &original).unwrap();
        fs::write(&patch, r#"{"name":"changed","minecraft_data_version":4903}"#).unwrap();
        let error = edit_metadata(&input, Some(&output), &patch).err().expect("version change must fail");
        assert!(error.to_string().contains("版本转换"));
        assert!(!output.exists());
        assert_eq!(fs::read(&input).unwrap(), original);
    }
}

#[cfg(test)]
mod preview_image_tests {
    use super::*;
    use crate::nbt::{load_litematic_preview_image, load_litematic_root};

    #[test]
    fn preview_edit_preserves_unknown_root_metadata_regions_entities_and_stream_thumbnail() {
        use std::collections::HashMap;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("未知字段 中文.litematic");
        let payload = Value::Compound(HashMap::from([
            ("Version".into(), Value::Int(6)),
            ("Metadata".into(), Value::Compound(HashMap::from([("CustomMetadata".into(), Value::String("kept".into()))]))),
            ("Regions".into(), Value::Compound(HashMap::from([("A".into(), Value::Compound(HashMap::from([
                ("CustomRegion".into(), Value::LongArray(fastnbt::LongArray::new(vec![1, i64::MAX]))),
                ("Entities".into(), Value::List(vec![Value::Compound(HashMap::from([("custom".into(), Value::Int(42))]))])),
            ])))]))),
            ("UnknownRoot".into(), Value::ByteArray(fastnbt::ByteArray::new(vec![-1, 0, 1]))),
        ]));
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&fastnbt::to_bytes(&payload).unwrap()).unwrap();
        fs::write(&source, gzip.finish().unwrap()).unwrap();
        let patch = dir.path().join("patch.json");
        fs::write(&patch, r#"{"preview_image_data":[-65536]}"#).unwrap();
        edit_metadata(&source, None, &patch).unwrap();
        let mut plain = Vec::new();
        GzDecoder::new(fs::File::open(&source).unwrap()).read_to_end(&mut plain).unwrap();
        let Value::Compound(after) = fastnbt::from_bytes::<Value>(&plain).unwrap() else { panic!() };
        let Value::Compound(before) = payload else { panic!() };
        assert_eq!(after.get("Regions"), before.get("Regions"));
        assert_eq!(after.get("UnknownRoot"), before.get("UnknownRoot"));
        let Some(Value::Compound(metadata)) = after.get("Metadata") else { panic!() };
        assert_eq!(metadata.get("CustomMetadata"), Some(&Value::String("kept".into())));
        let out = dir.path().join("stream.png");
        let stream = std::io::BufReader::new(fs::File::open(source).unwrap());
        assert!(crate::projection_preview::extract_projection_preview_stream(stream, &out).unwrap().found);
        assert_eq!(image::open(out).unwrap().to_rgba8().get_pixel(0,0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn png_preview_import_save_as_readback_and_clear_preserve_projection() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = Path::new(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/stats_water_fixture.litematic"));
        let input = dir.path().join("source.litematic");
        fs::copy(fixture,&input).unwrap();
        let original = fs::read(&input).unwrap();
        let root = load_litematic_root(&input).unwrap();
        let regions_before = fastnbt::to_bytes(&root.regions).unwrap();
        let data_version = root.minecraft_data_version;
        let image_path = dir.path().join("contained.png");
        let image = image::RgbaImage::from_fn(140,70,|x,_|if (20..120).contains(&x) { image::Rgba([79,123,211,143]) } else { image::Rgba([255,0,0,255]) });
        image.save(&image_path).unwrap();
        let patch_path = dir.path().join("import.json");
        fs::write(&patch_path,serde_json::to_vec(&serde_json::json!({"preview_image_path":image_path.to_string_lossy()})).unwrap()).unwrap();
        let output = dir.path().join("with-preview.litematic");
        let summary = edit_metadata(&input,Some(&output),&patch_path).unwrap();
        assert!(summary.backup_path.is_none());
        assert_eq!(fs::read(&input).unwrap(),original);
        let reloaded = load_litematic_root(&output).unwrap();
        assert_eq!(fastnbt::to_bytes(&reloaded.regions).unwrap(),regions_before);
        assert_eq!(reloaded.minecraft_data_version,data_version);
        assert_eq!(reloaded.metadata.preview_image_data.as_ref().unwrap().len(),320*240);
        let preview = load_litematic_preview_image(&output).unwrap().unwrap();
        assert_eq!((preview.width,preview.height),(320,240));
        let pixel = |x: usize, y: usize| &preview.rgba[(y*320+x)*4..(y*320+x)*4+4];
        assert_eq!(pixel(160,120), [79,123,211,143]);
        assert_eq!(pixel(0,120), [255,0,0,255], "left edge must not be cropped");
        assert_eq!(pixel(319,120), [255,0,0,255], "right edge must not be cropped");
        assert_eq!(pixel(160,0), [0,0,0,0]);
        assert_eq!(pixel(160,239), [0,0,0,0]);
        let extracted = dir.path().join("extracted.png");
        let extracted_result = crate::projection_preview::extract_projection_preview(&output,&extracted).unwrap();
        assert!(extracted_result.found);
        let decoded = image::open(&extracted).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(160,120).0,[79,123,211,143]);
        let clear_path = dir.path().join("clear.json");
        fs::write(&clear_path,r#"{"preview_image_data":[]}"#).unwrap();
        let cleared = dir.path().join("cleared.litematic");
        edit_metadata(&output,Some(&cleared),&clear_path).unwrap();
        assert!(load_litematic_preview_image(&cleared).unwrap().is_none());
        let reread = load_litematic_root(&cleared).unwrap();
        assert_eq!(fastnbt::to_bytes(&reread.regions).unwrap(),regions_before);
        assert_eq!(reread.minecraft_data_version,data_version);
    }

    #[test]
    fn preview_import_contains_entire_image_without_alpha_content_trimming() {
        let dir = tempfile::tempdir().unwrap();
        for (width, height) in [(140,70), (70,140), (140,140), (5,2)] {
            let input = image::RgbaImage::from_fn(width,height,|x,y| {
                if x == 0 { image::Rgba([255,0,0,255]) }
                else if x == width-1 { image::Rgba([0,255,0,255]) }
                else if y == 0 { image::Rgba([0,0,255,255]) }
                else if y == height-1 { image::Rgba([255,255,0,255]) }
                else { image::Rgba([79,123,211,143]) }
            });
            let path = dir.path().join(format!("{width}x{height}.png"));
            input.save(&path).unwrap();
            let actual = load_preview_image_data(&path, PreviewImageMode::Import).unwrap();
            assert_eq!(actual.len(),320*240);
            let decoded = image::RgbaImage::from_raw(320,240,actual.iter().flat_map(|argb| {
                let p = *argb as u32;
                [(p>>16) as u8,(p>>8) as u8,p as u8,(p>>24) as u8]
            }).collect()).unwrap();
            // The coloured source edges remain, while all unoccupied canvas
            // pixels retain alpha zero. Import does not trim source padding.
            let bounds = crate::projection_preview::tests::alpha_bounds(&decoded).unwrap();
            let scale = (320.0/f64::from(width)).min(240.0/f64::from(height));
            let dw = (f64::from(width)*scale).floor() as u32;
            let dh = (f64::from(height)*scale).floor() as u32;
            assert_eq!(bounds,((320-dw)/2,(240-dh)/2,(320-dw)/2+dw,(240-dh)/2+dh));
        }
    }

    #[test]
    fn generated_preview_modes_roundtrip_as_new_rectangular_embedded_png() {
        let dir = tempfile::tempdir().unwrap();
        let source = Path::new(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/stats_water_fixture.litematic"));
        let pixels = image::RgbaImage::from_fn(400,200,|x,y| {
            if (190..210).contains(&x) && (40..180).contains(&y) { image::Rgba([52,104,208,255]) }
            else { image::Rgba([0,0,0,0]) }
        });
        let image_path = dir.path().join("capture.png");
        pixels.save(&image_path).unwrap();
        for mode in ["fast_experimental","standard","full"] {
            let patch = dir.path().join(format!("{mode}.json"));
            let output = dir.path().join(format!("{mode}.litematic"));
            fs::write(&patch,serde_json::to_vec(&serde_json::json!({
                "preview_image_path":image_path.to_string_lossy(), "preview_image_mode":mode
            })).unwrap()).unwrap();
            edit_metadata(source,Some(&output),&patch).unwrap();
            let preview = load_litematic_preview_image(&output).unwrap().unwrap();
            assert_eq!((preview.width,preview.height),(320,240));
            let extracted = dir.path().join(format!("{mode}.png"));
            crate::projection_preview::extract_projection_preview(&output,&extracted).unwrap();
            let png = image::open(&extracted).unwrap().to_rgba8();
            assert_eq!(png.dimensions(),(320,240));
            assert_eq!(png.as_raw(),&preview.rgba);
            if mode != "full" {
                assert_eq!(crate::projection_preview::tests::alpha_bounds(&png),Some((144,5,176,235)));
            }
        }
    }

    #[test]
    fn raw_argb_import_preserves_channels_and_rejects_invalid_pixels_before_write() {
        let dir = tempfile::tempdir().unwrap();
        let input = Path::new(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/stats_water_fixture.litematic"));
        let output = dir.path().join("raw.litematic");
        let patch = dir.path().join("raw.json");
        let pixel = 0x807fdf02_u32 as i32;
        fs::write(&patch,serde_json::to_vec(&serde_json::json!({"preview_image_data":[pixel]})).unwrap()).unwrap();
        edit_metadata(input,Some(&output),&patch).unwrap();
        let image = load_litematic_preview_image(&output).unwrap().unwrap();
        assert_eq!((image.width,image.height),(1,1));
        assert_eq!(image.rgba,vec![127,223,2,128]);
        let invalid = dir.path().join("invalid.litematic");
        fs::write(&patch,r#"{"preview_image_data":[1,2,3]}"#).unwrap();
        assert!(edit_metadata(input,Some(&invalid),&patch).is_err());
        assert!(!invalid.exists());
    }
}

/// Separates the human description from the redundant tag summary (E.8 / 裁决 2).
const TAGS_SUMMARY_PREFIX: &str = "标签：";
const TAGS_SUMMARY_SEPARATOR: &str = "\n\n";

/// Drops a previously mirrored tag summary so writing tags twice never accumulates.
fn strip_tags_summary(description: &str) -> &str {
    if let Some(index) = description.rfind(TAGS_SUMMARY_SEPARATOR) {
        let tail = &description[index + TAGS_SUMMARY_SEPARATOR.len()..];
        if tail.starts_with(TAGS_SUMMARY_PREFIX) && !tail.contains('\n') {
            return &description[..index];
        }
    }
    description
}

fn tags_summary(tags: &[String]) -> Option<String> {
    if tags.is_empty() {
        None
    } else {
        Some(format!("{TAGS_SUMMARY_PREFIX}{}", tags.join("、")))
    }
}

/// `LibraryEntry.tags` stays authoritative; the file's `Description` carries a
/// redundant, human-readable tag summary so an external tool that drops `NovaTags`
/// still shows the tags (spec 5.7.1.2b / 5.7.3.1).
fn merge_tags_into_description(description: &str, tags: &[String]) -> String {
    let base = strip_tags_summary(description);
    match tags_summary(tags) {
        None => base.to_owned(),
        Some(summary) if base.is_empty() => summary,
        Some(summary) => format!("{base}{TAGS_SUMMARY_SEPARATOR}{summary}"),
    }
}

#[cfg(test)]
mod nova_tags_tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn fixture_source(dir: &Path) -> PathBuf {
        let source = dir.join("source.litematic");
        let payload = Value::Compound(HashMap::from([
            ("Version".into(), Value::Int(6)),
            ("Metadata".into(), Value::Compound(HashMap::from([
                ("Name".into(), Value::String("kept".into())),
                ("Description".into(), Value::String("summary".into())),
                ("CustomMetadata".into(), Value::String("preserved".into())),
            ]))),
            ("Regions".into(), Value::Compound(HashMap::from([("A".into(), Value::Compound(HashMap::new()))]))),
            ("UnknownRoot".into(), Value::String("kept".into())),
        ]));
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&fastnbt::to_bytes(&payload).unwrap()).unwrap();
        fs::write(&source, gzip.finish().unwrap()).unwrap();
        source
    }

    fn read_root(path: &Path) -> std::collections::HashMap<String, Value> {
        let mut plain = Vec::new();
        GzDecoder::new(fs::File::open(path).unwrap()).read_to_end(&mut plain).unwrap();
        let Value::Compound(root) = fastnbt::from_bytes::<Value>(&plain).unwrap() else { panic!() };
        root
    }

    #[test]
    fn tags_write_to_metadata_as_string_list_and_preserve_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture_source(dir.path());
        let patch = dir.path().join("tags.json");
        fs::write(&patch, r#"{"tags":["红石","tnt"]}"#).unwrap();
        edit_metadata(&source, None, &patch).unwrap();
        let root = read_root(&source);
        assert_eq!(root.get("UnknownRoot"), Some(&Value::String("kept".into())));
        let Some(Value::Compound(metadata)) = root.get("Metadata") else { panic!() };
        assert_eq!(metadata.get("Name"), Some(&Value::String("kept".into())));
        assert_eq!(metadata.get("CustomMetadata"), Some(&Value::String("preserved".into())));
        assert_eq!(
            metadata.get("NovaTags"),
            Some(&Value::List(vec![Value::String("红石".into()), Value::String("tnt".into())]))
        );
    }

    #[test]
    fn absent_tags_preserves_existing_nova_tags_and_missing_stays_absent() {
        let dir = tempfile::tempdir().unwrap();
        let with_tags = dir.path().join("with-tags.litematic");
        let root = Value::Compound(HashMap::from([
            ("Metadata".into(), Value::Compound(HashMap::from([
                ("NovaTags".into(), Value::List(vec![Value::String("existing".into())])),
            ]))),
        ]));
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        fs::write(&with_tags, gzip.finish().unwrap()).unwrap();
        let patch = dir.path().join("description.json");
        // Omitting `tags` must behave exactly like before this change.
        fs::write(&patch, r#"{"description":"updated"}"#).unwrap();
        edit_metadata(&with_tags, None, &patch).unwrap();
        let after = read_root(&with_tags);
        let Some(Value::Compound(metadata)) = after.get("Metadata") else { panic!() };
        assert_eq!(
            metadata.get("NovaTags"),
            Some(&Value::List(vec![Value::String("existing".into())])),
            "omitting tags must not clear an existing NovaTags list"
        );
        assert_eq!(metadata.get("Description"), Some(&Value::String("updated".into())));

        let without_tags = fixture_source(dir.path());
        edit_metadata(&without_tags, None, &patch).unwrap();
        let bare = read_root(&without_tags);
        let Some(Value::Compound(metadata)) = bare.get("Metadata") else { panic!() };
        assert!(metadata.get("NovaTags").is_none(), "a missing NovaTags must stay absent, not be backfilled");
    }

    #[test]
    fn writing_tags_mirrors_a_readable_summary_into_description() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture_source(dir.path());
        let patch = dir.path().join("tags.json");
        fs::write(&patch, r#"{"tags":["红石","tnt"]}"#).unwrap();
        edit_metadata(&source, None, &patch).unwrap();
        let root = read_root(&source);
        let Some(Value::Compound(metadata)) = root.get("Metadata") else { panic!() };
        // A previously written summary is stripped before the new one is appended.
        assert_eq!(metadata.get("Description"), Some(&Value::String("summary\n\n标签：红石、tnt".into())));
    }

    #[test]
    fn reapplying_tags_does_not_accumulate_the_description_summary() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture_source(dir.path());
        let patch = dir.path().join("tags.json");
        fs::write(&patch, r#"{"tags":["红石"]}"#).unwrap();
        edit_metadata(&source, None, &patch).unwrap();
        fs::write(&patch, r#"{"tags":["红石","铜"]}"#).unwrap();
        edit_metadata(&source, None, &patch).unwrap();
        let root = read_root(&source);
        let Some(Value::Compound(metadata)) = root.get("Metadata") else { panic!() };
        assert_eq!(metadata.get("Description"), Some(&Value::String("summary\n\n标签：红石、铜".into())));
    }

    #[test]
    fn clearing_tags_removes_the_description_summary_but_keeps_the_base_description() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture_source(dir.path());
        let patch = dir.path().join("tags.json");
        fs::write(&patch, r#"{"tags":["红石"]}"#).unwrap();
        edit_metadata(&source, None, &patch).unwrap();
        fs::write(&patch, r#"{"tags":[]}"#).unwrap();
        edit_metadata(&source, None, &patch).unwrap();
        let root = read_root(&source);
        let Some(Value::Compound(metadata)) = root.get("Metadata") else { panic!() };
        assert_eq!(metadata.get("Description"), Some(&Value::String("summary".into())));
        assert_eq!(metadata.get("NovaTags"), Some(&Value::List(Vec::new())));
    }
}
