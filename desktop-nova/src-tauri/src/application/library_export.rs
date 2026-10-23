//! Packaged export (flat zip) for the projection library (E.5, design 2.5.4).
//!
//! The module owns only the zip writing side of "打包导出": it copies the
//! projection body (a plain byte copy, so the file's embedded 140×140 preview and
//! `Metadata.NovaTags` travel with it — spec 5.9.1.6), optionally adds an external
//! preview image and a material list, and optionally writes a `manifest.json` with
//! the fixed schema (裁决 3). The material list text is produced by the TS layer
//! through `statsService` / `stats_api` and handed in, so this module stays a pure
//! filesystem/serde unit with no process or host dependencies (matching
//! `library_tasks.rs`).
//!
//! The zip is **flat** (按条目名组织, 不保留原文件夹层级, 5.9.1.4). A missing
//! source file only fails that entry and is reported; a target that is not
//! writable (bad parent, permission, disk full) aborts the whole export by writing
//! to a sibling temporary file and only renaming on success, so no half-product is
//! left behind (5.9.3.4).

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

/// One entry's export request (one logical projection).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportEntry {
    /// Flat zip stem (no extension); projection is `<stem>.litematic`.
    pub archive_name: String,
    /// Projection file to copy verbatim (required, `include.projection` is always true).
    pub source_path: String,
    /// The projection file's real name, recorded in the manifest.
    pub file_name: String,
    pub display_name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub checksum: Option<String>,
    /// Optional external preview image to ship alongside the body.
    #[serde(default)]
    pub preview_path: Option<String>,
    /// Optional material list text (txt by default) produced by the TS layer.
    #[serde(default)]
    pub material_list_text: Option<String>,
}

/// The whole export request. `include.projection` is not a field because it is
/// always true (本体必选, 裁决 16); the three optional flags below correspond to
/// the rest of the `{本体, 预览图, 材料清单, manifest.json}` set (5.9.1.5).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportRequest {
    /// Absolute destination `.zip` path.
    pub output_path: String,
    /// ISO-8601 timestamp stamped into the manifest.
    pub exported_at: String,
    #[serde(default)]
    pub include_preview: bool,
    #[serde(default)]
    pub include_material_list: bool,
    #[serde(default)]
    pub include_manifest: bool,
    #[serde(default)]
    pub entries: Vec<ExportEntry>,
}

#[derive(Debug)]
pub(crate) enum ExportOutcome {
    Completed {
        files: Vec<String>,
        entry_count: usize,
        failures: Vec<(String, String)>,
    },
    Cancelled,
}

/// Fixed manifest schema (裁决 3). `schemaVersion` is the manifest's own version,
/// independent of the library `schemaVersion`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    exported_at: String,
    nova_version: String,
    contents: Vec<String>,
    entries: Vec<ManifestEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestEntry {
    display_name: String,
    author: String,
    tags: Vec<String>,
    checksum: Option<String>,
    file_name: String,
    has_preview: bool,
    has_material_list: bool,
}

fn temporary_sibling(target: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    target.with_file_name(format!(
        ".nova-library-export-{}-{nonce}.tmp",
        std::process::id()
    ))
}

/// A flat archive stem is a single path segment: no separators, no traversal and
/// no empty value, so a malformed display name can never escape the zip.
fn valid_archive_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
}

fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}

/// Writes the flat zip. Returns per-entry failures for missing sources instead of
/// aborting; a target-level problem aborts the whole export and returns `Err`.
pub(crate) fn write_export_zip(
    request: &ExportRequest,
    should_cancel: &dyn Fn() -> bool,
) -> Result<ExportOutcome, String> {
    let target = PathBuf::from(&request.output_path);
    if target.as_os_str().is_empty() {
        return Err("导出目标路径不能为空".to_owned());
    }
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if !parent.is_dir() {
        return Err(format!("导出目标不可写：{}", parent.display()));
    }

    let temporary = temporary_sibling(&target);
    let result = write_into(&temporary, request, should_cancel);
    match result {
        Ok(ExportOutcome::Cancelled) => {
            let _ = fs::remove_file(&temporary);
            Ok(ExportOutcome::Cancelled)
        }
        Ok(outcome) => match fs::rename(&temporary, &target) {
            Ok(()) => Ok(outcome),
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(format!("写入导出文件失败：{error}"))
            }
        },
        Err(reason) => {
            let _ = fs::remove_file(&temporary);
            Err(reason)
        }
    }
}

fn write_into(
    temporary: &Path,
    request: &ExportRequest,
    should_cancel: &dyn Fn() -> bool,
) -> Result<ExportOutcome, String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(|error| format!("无法创建导出文件：{error}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut used_names: HashSet<String> = HashSet::new();
    used_names.insert("manifest.json".to_owned());
    let mut files: Vec<String> = Vec::new();
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut manifest_entries: Vec<ManifestEntry> = Vec::new();

    for entry in &request.entries {
        if should_cancel() {
            return Ok(ExportOutcome::Cancelled);
        }
        let stem = entry.archive_name.trim();
        if !valid_archive_name(stem) {
            failures.push((entry.source_path.clone(), "导出的条目名无效".to_owned()));
            continue;
        }
        let projection_name = format!("{stem}.litematic");
        if used_names.contains(&projection_name) {
            failures.push((entry.source_path.clone(), "导出的条目名重复".to_owned()));
            continue;
        }
        if !is_file(&entry.source_path) {
            failures.push((entry.source_path.clone(), "文件不存在或不可读".to_owned()));
            continue;
        }
        used_names.insert(projection_name.clone());

        let mut body = File::open(&entry.source_path)
            .map_err(|error| format!("读取投影失败 {}: {error}", entry.source_path))?;
        zip.start_file(&projection_name, options)
            .map_err(|error| format!("写入投影失败：{error}"))?;
        io::copy(&mut body, &mut zip).map_err(|error| format!("写入投影失败：{error}"))?;
        files.push(projection_name);

        let mut has_preview = false;
        if request.include_preview {
            if let Some(path) = entry.preview_path.as_deref() {
                if is_file(path) && !used_names.contains(&format!("{stem}.png")) {
                    used_names.insert(format!("{stem}.png"));
                    let mut image = File::open(path)
                        .map_err(|error| format!("读取预览图失败 {path}: {error}"))?;
                    zip.start_file(format!("{stem}.png"), options)
                        .map_err(|error| format!("写入预览图失败：{error}"))?;
                    io::copy(&mut image, &mut zip)
                        .map_err(|error| format!("写入预览图失败：{error}"))?;
                    files.push(format!("{stem}.png"));
                    has_preview = true;
                }
            }
        }

        let mut has_material_list = false;
        if request.include_material_list {
            if let Some(text) = entry.material_list_text.as_deref() {
                if !text.is_empty() && !used_names.contains(&format!("{stem}.txt")) {
                    used_names.insert(format!("{stem}.txt"));
                    zip.start_file(format!("{stem}.txt"), options)
                        .map_err(|error| format!("写入材料清单失败：{error}"))?;
                    io::copy(&mut text.as_bytes(), &mut zip)
                        .map_err(|error| format!("写入材料清单失败：{error}"))?;
                    files.push(format!("{stem}.txt"));
                    has_material_list = true;
                }
            }
        }

        manifest_entries.push(ManifestEntry {
            display_name: entry.display_name.clone(),
            author: entry.author.clone(),
            tags: entry.tags.clone(),
            checksum: entry.checksum.clone(),
            file_name: entry.file_name.clone(),
            has_preview,
            has_material_list,
        });
    }

    let entry_count = manifest_entries.len();
    if request.include_manifest {
        let manifest = Manifest {
            schema_version: 1,
            exported_at: request.exported_at.clone(),
            nova_version: env!("CARGO_PKG_VERSION").to_owned(),
            contents: contents_of(request),
            entries: manifest_entries,
        };
        let json = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("生成 manifest 失败：{error}"))?;
        zip.start_file("manifest.json", options)
            .map_err(|error| format!("写入 manifest 失败：{error}"))?;
        io::copy(&mut json.as_slice(), &mut zip)
            .map_err(|error| format!("写入 manifest 失败：{error}"))?;
        files.push("manifest.json".to_owned());
    }

    let file = zip.finish().map_err(|error| format!("收尾导出文件失败：{error}"))?;
    file.sync_all()
        .map_err(|error| format!("刷新导出文件失败：{error}"))?;

    Ok(ExportOutcome::Completed { files, entry_count, failures })
}

fn contents_of(request: &ExportRequest) -> Vec<String> {
    let mut contents = vec!["projection".to_owned()];
    if request.include_preview {
        contents.push("preview".to_owned());
    }
    if request.include_material_list {
        contents.push("materialList".to_owned());
    }
    if request.include_manifest {
        contents.push("manifest".to_owned());
    }
    contents
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nova-library-export-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn request(output: &Path, source: &Path) -> ExportRequest {
        ExportRequest {
            output_path: output.display().to_string(),
            exported_at: "2026-01-01T00:00:00.000Z".to_owned(),
            include_preview: false,
            include_material_list: false,
            include_manifest: true,
            entries: vec![ExportEntry {
                archive_name: "redstone".to_owned(),
                source_path: source.display().to_string(),
                file_name: "redstone.litematic".to_owned(),
                display_name: "Redstone".to_owned(),
                author: "Nova".to_owned(),
                tags: vec!["红石".to_owned()],
                checksum: Some("abc".to_owned()),
                preview_path: None,
                material_list_text: None,
            }],
        }
    }

    #[test]
    fn flat_zip_copies_body_and_writes_manifest() {
        let root = temp_dir("flat");
        let source = root.join("redstone.litematic");
        fs::write(&source, b"projection-bytes").unwrap();
        let output = root.join("out.zip");
        let outcome = write_export_zip(&request(&output, &source), &|| false).unwrap();
        match outcome {
            ExportOutcome::Completed { files, entry_count, failures } => {
                assert_eq!(entry_count, 1);
                assert!(failures.is_empty());
                assert!(files.contains(&"redstone.litematic".to_owned()));
                assert!(files.contains(&"manifest.json".to_owned()));
            }
            ExportOutcome::Cancelled => panic!("unexpected cancellation"),
        }

        let file = File::open(&output).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert!(archive.by_name("redstone.litematic").is_ok());
        let mut manifest = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("manifest.json").unwrap(), &mut manifest)
            .unwrap();
        assert!(manifest.contains("\"schemaVersion\": 1"));
        assert!(manifest.contains("\"displayName\": \"Redstone\""));
        assert!(manifest.contains("\"fileName\": \"redstone.litematic\""));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unwritable_target_aborts_without_leftovers() {
        let root = temp_dir("unwritable");
        let source = root.join("a.litematic");
        fs::write(&source, b"bytes").unwrap();
        let missing_parent = root.join("does-not-exist").join("out.zip");
        assert!(write_export_zip(&request(&missing_parent, &source), &|| false).is_err());
        assert!(!missing_parent.exists());
    }

    #[test]
    fn missing_source_is_reported_not_fatal() {
        let root = temp_dir("missing-source");
        let source = root.join("gone.litematic");
        let output = root.join("out.zip");
        let outcome = write_export_zip(&request(&output, &source), &|| false).unwrap();
        match outcome {
            ExportOutcome::Completed { entry_count, failures, .. } => {
                assert_eq!(entry_count, 0);
                assert_eq!(failures.len(), 1);
            }
            ExportOutcome::Cancelled => panic!("unexpected cancellation"),
        }
        assert!(output.is_file());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancellation_removes_the_temporary_file() {
        let root = temp_dir("cancel");
        let source = root.join("a.litematic");
        fs::write(&source, b"bytes").unwrap();
        let output = root.join("out.zip");
        let outcome = write_export_zip(&request(&output, &source), &|| true).unwrap();
        assert!(matches!(outcome, ExportOutcome::Cancelled));
        assert!(!output.exists());
        fs::remove_dir_all(&root).unwrap();
    }
}