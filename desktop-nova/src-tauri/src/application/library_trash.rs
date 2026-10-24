//! Physical operations for the projection library trash (`projection-library/trash`).
//!
//! The module owns only the filesystem side of the recycle bin: it ensures the
//! fixed trash directory exists, moves library-managed files in and back out,
//! and purges them. Library record semantics (`LibraryTrashEntry`) stay in TS.
//! Every operation reports a per-item result so a single failure never hides
//! the remaining plan (spec 4.2.5 / 5.6.3.2).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Fixed location of the self-managed recycle bin under the user config dir.
pub(crate) fn trash_directory(config_directory: &Path) -> PathBuf {
    config_directory.join("projection-library").join("trash")
}

/// Creates the trash directory on demand; missing means "create", never "fail".
pub(crate) fn ensure_trash_directory(config_directory: &Path) -> Result<PathBuf, String> {
    let directory = trash_directory(config_directory);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashMoveItem {
    pub source_path: String,
    pub trash_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashRestoreItem {
    pub trash_path: String,
    pub target_path: String,
    #[serde(default)]
    pub rename_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashPurgeItem {
    pub trash_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashMoveResult {
    pub source_path: String,
    pub trash_path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashRestoreResult {
    pub trash_path: String,
    pub target_path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashPurgeResult {
    pub trash_path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Moves each item into the trash directory, returning one result per item.
pub(crate) fn move_items(config_directory: &Path, items: &[TrashMoveItem]) -> Vec<TrashMoveResult> {
    let directory = match ensure_trash_directory(config_directory) {
        Ok(directory) => directory,
        Err(reason) => {
            return items
                .iter()
                .map(|item| TrashMoveResult {
                    source_path: item.source_path.clone(),
                    trash_path: String::new(),
                    ok: false,
                    reason: Some(reason.clone()),
                })
                .collect()
        }
    };
    items.iter().map(|item| move_one(&directory, item)).collect()
}

fn move_one(directory: &Path, item: &TrashMoveItem) -> TrashMoveResult {
    let source = PathBuf::from(&item.source_path);
    let name = match sanitize_file_name(&item.trash_name) {
        Ok(name) => name,
        Err(reason) => {
            return TrashMoveResult {
                source_path: item.source_path.clone(),
                trash_path: String::new(),
                ok: false,
                reason: Some(reason),
            }
        }
    };
    if !source.is_file() {
        return TrashMoveResult {
            source_path: item.source_path.clone(),
            trash_path: String::new(),
            ok: false,
            reason: Some("source-missing".to_owned()),
        };
    }
    let target = directory.join(&name);
    if target == source {
        return TrashMoveResult {
            source_path: item.source_path.clone(),
            trash_path: target.display().to_string(),
            ok: false,
            reason: Some("source-and-target-identical".to_owned()),
        };
    }
    if target.exists() {
        return TrashMoveResult {
            source_path: item.source_path.clone(),
            trash_path: target.display().to_string(),
            ok: false,
            reason: Some("occupied".to_owned()),
        };
    }
    match move_file(&source, &target) {
        Ok(()) => TrashMoveResult {
            source_path: item.source_path.clone(),
            trash_path: target.display().to_string(),
            ok: true,
            reason: None,
        },
        Err(reason) => TrashMoveResult {
            source_path: item.source_path.clone(),
            trash_path: target.display().to_string(),
            ok: false,
            reason: Some(reason),
        },
    }
}

/// Moves each item back to its original location. An occupied target is
/// reported as `ok=false, reason="occupied"` without touching the source.
pub(crate) fn restore_items(items: &[TrashRestoreItem]) -> Vec<TrashRestoreResult> {
    items.iter().map(restore_one).collect()
}

fn restore_one(item: &TrashRestoreItem) -> TrashRestoreResult {
    let source = PathBuf::from(&item.trash_path);
    let target = match restore_target(item) {
        Ok(target) => target,
        Err(reason) => {
            return TrashRestoreResult {
                trash_path: item.trash_path.clone(),
                target_path: item.target_path.clone(),
                ok: false,
                reason: Some(reason),
            }
        }
    };
    if !source.is_file() {
        return TrashRestoreResult {
            trash_path: item.trash_path.clone(),
            target_path: target.display().to_string(),
            ok: false,
            reason: Some("trash-missing".to_owned()),
        };
    }
    if target == source {
        return TrashRestoreResult {
            trash_path: item.trash_path.clone(),
            target_path: target.display().to_string(),
            ok: false,
            reason: Some("source-and-target-identical".to_owned()),
        };
    }
    if target.exists() {
        return TrashRestoreResult {
            trash_path: item.trash_path.clone(),
            target_path: target.display().to_string(),
            ok: false,
            reason: Some("occupied".to_owned()),
        };
    }
    if let Some(parent) = target.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return TrashRestoreResult {
                trash_path: item.trash_path.clone(),
                target_path: target.display().to_string(),
                ok: false,
                reason: Some(error.to_string()),
            };
        }
    }
    match move_file(&source, &target) {
        Ok(()) => TrashRestoreResult {
            trash_path: item.trash_path.clone(),
            target_path: target.display().to_string(),
            ok: true,
            reason: None,
        },
        Err(reason) => TrashRestoreResult {
            trash_path: item.trash_path.clone(),
            target_path: target.display().to_string(),
            ok: false,
            reason: Some(reason),
        },
    }
}

fn restore_target(item: &TrashRestoreItem) -> Result<PathBuf, String> {
    let original = PathBuf::from(&item.target_path);
    match item.rename_to.as_deref() {
        None => Ok(original),
        Some(name) => {
            let name = sanitize_file_name(name)?;
            let parent = original.parent().unwrap_or_else(|| Path::new(""));
            Ok(parent.join(name))
        }
    }
}

/// Physically deletes each item; unavailable files are skipped and reported.
pub(crate) fn purge_items(items: &[TrashPurgeItem]) -> Vec<TrashPurgeResult> {
    items.iter().map(purge_one).collect()
}

fn purge_one(item: &TrashPurgeItem) -> TrashPurgeResult {
    let path = PathBuf::from(&item.trash_path);
    if !path.exists() {
        return TrashPurgeResult {
            trash_path: item.trash_path.clone(),
            ok: false,
            reason: Some("missing".to_owned()),
        };
    }
    if path.is_dir() {
        return TrashPurgeResult {
            trash_path: item.trash_path.clone(),
            ok: false,
            reason: Some("not-a-file".to_owned()),
        };
    }
    match fs::remove_file(&path) {
        Ok(()) => TrashPurgeResult {
            trash_path: item.trash_path.clone(),
            ok: true,
            reason: None,
        },
        Err(error) => TrashPurgeResult {
            trash_path: item.trash_path.clone(),
            ok: false,
            reason: Some(error.to_string()),
        },
    }
}

/// A trash file name never carries separators or traversal segments: the file
/// always lands directly inside the trash directory.
fn sanitize_file_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("empty-name".to_owned());
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed == "." || trimmed == ".." {
        return Err("invalid-name".to_owned());
    }
    Ok(trimmed.to_owned())
}

/// Rename first (same volume); fall back to copy + remove for cross-volume
/// moves, which Windows rejects with a plain rename.
fn move_file(source: &Path, target: &Path) -> Result<(), String> {
    if fs::rename(source, target).is_ok() {
        return Ok(());
    }
    fs::copy(source, target).map_err(|error| error.to_string())?;
    fs::remove_file(source).map_err(|error| {
        let _ = fs::remove_file(target);
        error.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nova-library-trash-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn trash_directory_is_created_on_demand_and_round_trips() {
        let root = temp_dir("round-trip");
        let source_dir = root.join("library");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("a.litematic");
        fs::write(&source, b"projection-bytes").unwrap();

        let moved = move_items(
            &root,
            &[TrashMoveItem {
                source_path: source.display().to_string(),
                trash_name: "a.litematic".to_owned(),
            }],
        );
        assert!(moved[0].ok, "{:?}", moved[0].reason);
        assert!(trash_directory(&root).is_dir());
        assert!(!source.exists());
        let trashed = PathBuf::from(&moved[0].trash_path);
        assert!(trashed.is_file());

        let restored = restore_items(&[TrashRestoreItem {
            trash_path: trashed.display().to_string(),
            target_path: source.display().to_string(),
            rename_to: None,
        }]);
        assert!(restored[0].ok, "{:?}", restored[0].reason);
        assert_eq!(fs::read(&source).unwrap(), b"projection-bytes");
        assert!(!trashed.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn restore_refuses_an_occupied_target_and_keeps_the_source() {
        let root = temp_dir("occupied");
        let source = root.join("in-trash.litematic");
        fs::write(&source, b"trashed").unwrap();
        let target = root.join("original.litematic");
        fs::write(&target, b"existing").unwrap();

        let restored = restore_items(&[TrashRestoreItem {
            trash_path: source.display().to_string(),
            target_path: target.display().to_string(),
            rename_to: None,
        }]);
        assert!(!restored[0].ok);
        assert_eq!(restored[0].reason.as_deref(), Some("occupied"));
        assert_eq!(fs::read(&target).unwrap(), b"existing");
        assert!(source.is_file(), "a refused restore must not drop the trashed file");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn purge_deletes_files_and_skips_unavailable_items() {
        let root = temp_dir("purge");
        let present = root.join("present.litematic");
        fs::write(&present, b"bytes").unwrap();
        let missing = root.join("missing.litematic");

        let results = purge_items(&[
            TrashPurgeItem { trash_path: present.display().to_string() },
            TrashPurgeItem { trash_path: missing.display().to_string() },
        ]);
        assert!(results[0].ok);
        assert!(!present.exists());
        assert!(!results[1].ok);
        assert_eq!(results[1].reason.as_deref(), Some("missing"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn move_rejects_separators_and_does_not_silently_overwrite() {
        let root = temp_dir("guards");
        let source = root.join("source.litematic");
        fs::write(&source, b"a").unwrap();
        let rejected = move_items(
            &root,
            &[TrashMoveItem {
                source_path: source.display().to_string(),
                trash_name: "../escape.litematic".to_owned(),
            }],
        );
        assert!(!rejected[0].ok);
        assert_eq!(rejected[0].reason.as_deref(), Some("invalid-name"));
        assert!(source.is_file());

        let directory = ensure_trash_directory(&root).unwrap();
        fs::write(directory.join("taken.litematic"), b"occupied").unwrap();
        let occupied = move_items(
            &root,
            &[TrashMoveItem {
                source_path: source.display().to_string(),
                trash_name: "taken.litematic".to_owned(),
            }],
        );
        assert!(!occupied[0].ok);
        assert_eq!(occupied[0].reason.as_deref(), Some("occupied"));
        assert_eq!(
            fs::read(directory.join("taken.litematic")).unwrap(),
            b"occupied",
            "an occupied trash name must not be overwritten"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}