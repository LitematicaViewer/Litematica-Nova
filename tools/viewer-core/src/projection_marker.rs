use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use fastnbt::Value;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::Serialize;

/// Coordinates are normalized to the same enclosing box as visual-meta / visualize-layer.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ProjectionMarkerPoint {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Serialize)]
pub struct ProjectionMarkersOutput {
    pub origin: Option<ProjectionMarkerPoint>,
}

#[derive(Debug, Serialize)]
pub struct ProjectionMarkerEditSummary {
    pub input: String,
    pub output: String,
    pub backup_path: Option<String>,
    pub origin: Option<ProjectionMarkerPoint>,
}

// An origin-only edit must retain unknown NBT, including preview images, pending
// ticks and exporter extensions. The renderer's typed read model deliberately
// omits some of those tags, so it is not a lossless file-edit representation.
struct MarkerDocument {
    root: Value,
    min: [i64; 3],
    max: [i64; 3],
    has_non_default_position: bool,
}

impl MarkerDocument {
    fn load(input: &Path) -> Result<Self> {
        let file =
            File::open(input).with_context(|| format!("failed to open {}", input.display()))?;
        let root: Value = fastnbt::from_reader(GzDecoder::new(BufReader::new(file)))
            .with_context(|| format!("failed to read {}", input.display()))?;
        Self::from_root(root)
    }

    fn from_root(root: Value) -> Result<Self> {
        let regions = compound_field(&root, "Regions")?;
        if regions.is_empty() {
            bail!("projection has no regions");
        }
        let mut min = [i64::MAX; 3];
        let mut max = [i64::MIN; 3];
        let mut has_non_default_position = false;
        for region in regions.values() {
            let position = coordinates(compound_field(region, "Position")?)?;
            let size = coordinates(compound_field(region, "Size")?)?;
            has_non_default_position |= position != [0; 3];
            for axis in 0..3 {
                if size[axis] == 0 {
                    bail!("projection region has a zero-sized axis");
                }
                let end = position[axis] + size[axis] - size[axis].signum();
                min[axis] = min[axis].min(position[axis].min(end));
                max[axis] = max[axis].max(position[axis].max(end));
            }
        }
        Ok(Self {
            root,
            min,
            max,
            has_non_default_position,
        })
    }

    fn origin(&self) -> Option<ProjectionMarkerPoint> {
        // A normalized minimum at zero is the implicit default, including
        // multi-region files after remove-origin. Do not recreate a marker merely
        // because a second region has a nonzero relative placement.
        if !self.has_non_default_position
            || self.min == [0; 3]
            || (0..3).any(|axis| self.min[axis] > 0 || self.max[axis] < 0)
        {
            return None;
        }
        Some(ProjectionMarkerPoint {
            x: i32::try_from(-self.min[0]).ok()?,
            y: i32::try_from(-self.min[1]).ok()?,
            z: i32::try_from(-self.min[2]).ok()?,
        })
    }

    fn move_origin(mut self, target: [i32; 3]) -> Result<Self> {
        for axis in 0..3 {
            if target[axis] < 0 || i64::from(target[axis]) > self.max[axis] - self.min[axis] {
                bail!(
                    "origin target out of bounds: ({}, {}, {})",
                    target[0],
                    target[1],
                    target[2]
                );
            }
        }
        let shift =
            std::array::from_fn::<_, 3, _>(|axis| -self.min[axis] - i64::from(target[axis]));
        let root = as_compound_mut(&mut self.root)?;
        let regions = as_compound_mut(
            root.get_mut("Regions")
                .ok_or_else(|| anyhow!("missing Regions"))?,
        )?;
        for region in regions.values_mut() {
            let region = as_compound_mut(region)?;
            let position = as_compound_mut(
                region
                    .get_mut("Position")
                    .ok_or_else(|| anyhow!("missing Position"))?,
            )?;
            for (axis, key) in ["x", "y", "z"].iter().enumerate() {
                let value = read_int(position.get(*key), key)? + shift[axis];
                position.insert(
                    (*key).to_string(),
                    Value::Int(
                        i32::try_from(value).with_context(|| {
                            format!("shifted region {key} coordinate exceeds i32")
                        })?,
                    ),
                );
            }
        }
        let metadata = as_compound_mut(
            root.get_mut("Metadata")
                .ok_or_else(|| anyhow!("missing Metadata"))?,
        )?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis()
            .min(i64::MAX as u128) as i64;
        metadata.insert("TimeModified".to_string(), Value::Long(now));
        Self::from_root(self.root)
    }
}

fn compound_field<'a>(value: &'a Value, field: &str) -> Result<&'a HashMap<String, Value>> {
    match value {
        Value::Compound(map) => match map.get(field) {
            Some(Value::Compound(value)) => Ok(value),
            _ => bail!("missing or invalid {field} compound"),
        },
        _ => bail!("expected NBT compound"),
    }
}

fn as_compound_mut(value: &mut Value) -> Result<&mut HashMap<String, Value>> {
    match value {
        Value::Compound(map) => Ok(map),
        _ => bail!("expected NBT compound"),
    }
}

fn read_int(value: Option<&Value>, name: &str) -> Result<i64> {
    match value {
        Some(Value::Int(value)) => Ok(i64::from(*value)),
        _ => bail!("missing or invalid integer coordinate {name}"),
    }
}

fn coordinates(map: &HashMap<String, Value>) -> Result<[i64; 3]> {
    Ok([
        read_int(map.get("x"), "x")?,
        read_int(map.get("y"), "y")?,
        read_int(map.get("z"), "z")?,
    ])
}

pub fn read_projection_markers(input: &Path) -> Result<ProjectionMarkersOutput> {
    Ok(ProjectionMarkersOutput {
        origin: MarkerDocument::load(input)?.origin(),
    })
}

pub fn set_projection_origin(
    input: &Path,
    output: Option<&Path>,
    x: i32,
    y: i32,
    z: i32,
) -> Result<ProjectionMarkerEditSummary> {
    save_marker_document(
        input,
        output,
        MarkerDocument::load(input)?.move_origin([x, y, z])?,
    )
}

pub fn remove_projection_origin(
    input: &Path,
    output: Option<&Path>,
) -> Result<ProjectionMarkerEditSummary> {
    save_marker_document(
        input,
        output,
        MarkerDocument::load(input)?.move_origin([0; 3])?,
    )
}

fn save_marker_document(
    input: &Path,
    output: Option<&Path>,
    document: MarkerDocument,
) -> Result<ProjectionMarkerEditSummary> {
    let (target, backup) = match output {
        Some(target) => {
            // create_new also closes the check/write race. An explicit --output
            // never means overwrite, including an alias of the source itself.
            if target.exists() {
                bail!(
                    "refusing to overwrite existing output file: {}",
                    target.display()
                );
            }
            (target.to_path_buf(), None)
        }
        None => {
            let mut backup = input.to_path_buf();
            let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let name = input
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("projection.litematic");
            backup.set_file_name(format!("{name}.bak.{suffix}"));
            let mut backup_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&backup)
                .with_context(|| format!("failed to create backup {}", backup.display()))?;
            std::io::copy(&mut File::open(input)?, &mut backup_file)?;
            backup_file.sync_all()?;
            (input.to_path_buf(), Some(backup))
        }
    };
    let result = (|| -> Result<()> {
        let file = if backup.is_some() {
            File::create(&target)?
        } else {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?
        };
        write_document(file, &document.root)
    })();
    if let Err(error) = result {
        if let Some(backup) = &backup {
            fs::copy(backup, input).with_context(|| {
                format!(
                    "save failed ({error:#}); restoration also failed, original backup: {}",
                    backup.display()
                )
            })?;
        }
        return Err(error)
            .with_context(|| format!("failed to save projection {}", target.display()));
    }
    Ok(ProjectionMarkerEditSummary {
        input: input.display().to_string(),
        output: target.display().to_string(),
        backup_path: backup.map(|p| p.display().to_string()),
        origin: document.origin(),
    })
}

fn write_document(file: File, root: &Value) -> Result<()> {
    let mut encoder = GzEncoder::new(file, Compression::default());
    fastnbt::to_writer(&mut encoder, root)?;
    let mut file = encoder.finish()?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::{IntArray, LongArray};

    fn compound(values: &[(&str, Value)]) -> Value {
        Value::Compound(
            values
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        )
    }

    fn point(x: i32, y: i32, z: i32) -> Value {
        compound(&[
            ("x", Value::Int(x)),
            ("y", Value::Int(y)),
            ("z", Value::Int(z)),
        ])
    }

    fn fixture() -> Value {
        let region = |position, size| {
            compound(&[
                ("Position", position),
                ("Size", size),
                (
                    "BlockStates",
                    Value::LongArray(LongArray::new(vec![123456789])),
                ),
                (
                    "PendingBlockTicks",
                    Value::List(vec![compound(&[("custom", Value::Int(7))])]),
                ),
            ])
        };
        compound(&[
            ("Version", Value::Int(6)),
            (
                "Metadata",
                compound(&[
                    ("TimeModified", Value::Long(1)),
                    (
                        "PreviewImageData",
                        Value::IntArray(IntArray::new(vec![1, 2, 3])),
                    ),
                ]),
            ),
            ("ExporterCustomTag", Value::String("keep me".into())),
            (
                "Regions",
                compound(&[
                    ("negative", region(point(-1, -2, -3), point(-4, 4, -3))),
                    ("other", region(point(3, 1, 2), point(2, 2, 2))),
                ]),
            ),
        ])
    }

    fn write_fixture(path: &Path) {
        write_document(File::create(path).unwrap(), &fixture()).unwrap();
    }

    #[test]
    fn marker_roundtrip_preserves_unknown_tags_and_source() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("saved.litematic");
        write_fixture(&input);
        let before = fs::read(&input).unwrap();
        assert_eq!(
            read_projection_markers(&input).unwrap().origin,
            Some(ProjectionMarkerPoint { x: 4, y: 2, z: 5 })
        );
        let result = set_projection_origin(&input, Some(&output), 2, 1, 3).unwrap();
        assert_eq!(
            result.origin,
            Some(ProjectionMarkerPoint { x: 2, y: 1, z: 3 })
        );
        assert!(result.backup_path.is_none());
        assert_eq!(fs::read(&input).unwrap(), before);
        let mut edited = MarkerDocument::load(&output).unwrap().root;
        let original = fixture();
        let edited_root = as_compound_mut(&mut edited).unwrap();
        let original_root = match &original {
            Value::Compound(map) => map,
            _ => unreachable!(),
        };
        // Only positions and modified time are editable; compare everything else.
        as_compound_mut(edited_root.get_mut("Metadata").unwrap())
            .unwrap()
            .insert("TimeModified".into(), Value::Long(1));
        let regions = as_compound_mut(edited_root.get_mut("Regions").unwrap()).unwrap();
        for (name, region) in regions {
            let original_region = &compound_field(&original, "Regions").unwrap()[name];
            let position = compound_field(original_region, "Position").unwrap().clone();
            as_compound_mut(region)
                .unwrap()
                .insert("Position".into(), Value::Compound(position));
        }
        assert_eq!(&edited, &Value::Compound(original_root.clone()));
    }

    #[test]
    fn marker_save_as_never_overwrites_existing_file_or_source_alias() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("existing.litematic");
        write_fixture(&input);
        fs::write(&output, b"existing bytes").unwrap();
        let before = fs::read(&input).unwrap();
        assert!(set_projection_origin(&input, Some(&output), 1, 1, 1).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"existing bytes");
        assert!(set_projection_origin(&input, Some(&input), 1, 1, 1).is_err());
        assert_eq!(fs::read(&input).unwrap(), before);
    }

    #[test]
    fn marker_in_place_backup_and_multi_region_remove_are_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        write_fixture(&input);
        let before = fs::read(&input).unwrap();
        let result = remove_projection_origin(&input, None).unwrap();
        assert_eq!(result.origin, None);
        assert_eq!(read_projection_markers(&input).unwrap().origin, None);
        assert_eq!(
            fs::read(result.backup_path.as_ref().unwrap()).unwrap(),
            before
        );
        fs::copy(result.backup_path.unwrap(), &input).unwrap();
        assert_eq!(fs::read(&input).unwrap(), before);
    }

    #[test]
    fn marker_invalid_target_does_not_write_or_create_backup() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        write_fixture(&input);
        let before = fs::read(&input).unwrap();
        assert!(set_projection_origin(&input, None, -1, 0, 0).is_err());
        assert!(set_projection_origin(&input, None, 500, 0, 0).is_err());
        assert_eq!(fs::read(&input).unwrap(), before);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
