use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::{self, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::mem::size_of;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use fastnbt::{Value, from_reader, to_writer};
use flate2::Compression;
use flate2::bufread::GzDecoder as BufReadGzDecoder;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use memmap2::MmapOptions;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_bytes::Bytes;
use sha2::{Digest, Sha256};

use crate::model::{BlockStateNbt, EnclosingSize, EntityNbt};
use crate::process_io::{open_process_read_file, run_process_io, wrap_process_io};

const NBT_STREAM_BUFFER_BYTES: usize = 8 << 10;
const NBT_SCAN_PROGRESS_INTERVAL_BYTES: u64 = 1 << 20;
const MAX_NBT_CONTAINER_DEPTH: usize = 512;
const MINECRAFT_DATA_VERSION_NAME: &[u8; 20] = b"MinecraftDataVersion";
const PREVIEW_IMAGE_DATA_NAME: &[u8; 16] = b"PreviewImageData";
const MAX_PREVIEW_IMAGE_EDGE: u32 = 4_096;
const PREFLIGHT_METADATA_NAME: &[u8] = b"Metadata";
const PREFLIGHT_TOTAL_BLOCKS_NAME: &[u8] = b"TotalBlocks";
const PREFLIGHT_TOTAL_VOLUME_NAME: &[u8] = b"TotalVolume";
const PREFLIGHT_REGION_COUNT_NAME: &[u8] = b"RegionCount";
const PREFLIGHT_ENCLOSING_SIZE_NAME: &[u8] = b"EnclosingSize";
const PREFLIGHT_METADATA_NAME_BUFFER_BYTES: usize = 13;
const PREFLIGHT_SKIP_BUFFER_BYTES: usize = 64 << 10;
const BLOCK_STATES_PAGE_BYTES: usize = 64 << 10;
// Section compilation preserves Minecraft's Z -> Y -> X visit order while
// Litematica stores X-contiguous rows under Z, then Y. Retain the ordinary
// section-plus-halo working set so those Y-layer switches do not reread the
// same immutable spool pages. Wider layouts degrade to bounded replacement.
// The hot entry keeps the ordinary lookup O(1); the page payload costs at most
// 4 MiB per participating thread.
const BLOCK_STATES_PAGE_CACHE_PAGES: usize = 64;
const BLOCK_STATES_RETAINED_PAGE_COUNT: usize = BLOCK_STATES_PAGE_CACHE_PAGES - 1;
static NEXT_BLOCK_STATES_SPOOL_ID: AtomicU64 = AtomicU64::new(1);

/// Default upper bounds for the metadata-only resource-preflight scan.
///
/// Some exporters place `Metadata` after `Regions`. Keep the scan bounded,
/// but allow the known large-scene fixture's 105 MiB NBT stream to reach its
/// trailing metadata without decoding any scene representation.
pub const DEFAULT_LITEMATIC_METADATA_SCAN_LIMITS: LitematicMetadataScanLimits =
    LitematicMetadataScanLimits {
        max_decompressed_bytes: 128 * 1024 * 1024,
        deadline: Duration::from_millis(950),
    };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LitematicMetadataScanLimits {
    pub max_decompressed_bytes: u64,
    pub deadline: Duration,
}

impl Default for LitematicMetadataScanLimits {
    fn default() -> Self {
        DEFAULT_LITEMATIC_METADATA_SCAN_LIMITS
    }
}

/// Why a metadata value cannot be used by resource admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LitematicMetadataUnknown {
    MetadataMissing,
    FieldMissing,
    ByteLimitExceeded,
    DeadlineExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LitematicMetadataValue<T> {
    Known(T),
    Unknown(LitematicMetadataUnknown),
}

impl<T> LitematicMetadataValue<T> {
    pub const fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LitematicMetadataEnclosingSize {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

/// The bounded subset of litematic metadata used for pre-build estimation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LitematicPreflightMetadata {
    pub total_blocks: LitematicMetadataValue<u64>,
    pub total_volume: LitematicMetadataValue<u64>,
    pub region_count: LitematicMetadataValue<u64>,
    pub enclosing_size: LitematicMetadataValue<LitematicMetadataEnclosingSize>,
    pub decompressed_bytes_read: u64,
}

/// Read only the bounded root `Metadata` compound needed by resource
/// preflight. This function never deserializes `Regions`, block states, block
/// entities, or geometry.
pub fn scan_litematic_preflight_metadata(
    path: &Path,
    limits: LitematicMetadataScanLimits,
) -> Result<LitematicPreflightMetadata> {
    let file = open_process_read_file(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let compressed = BufReader::with_capacity(NBT_STREAM_BUFFER_BYTES, file);
    scan_litematic_preflight_metadata_from_gzip(compressed, limits)
        .with_context(|| format!("failed to scan Metadata in {}", path.display()))
}

fn scan_litematic_preflight_metadata_from_gzip<R>(
    compressed: R,
    limits: LitematicMetadataScanLimits,
) -> Result<LitematicPreflightMetadata>
where
    R: BufRead,
{
    scan_litematic_preflight_metadata_from_nbt(BufReadGzDecoder::new(compressed), limits)
}

fn scan_litematic_preflight_metadata_from_nbt<R>(
    reader: R,
    limits: LitematicMetadataScanLimits,
) -> Result<LitematicPreflightMetadata>
where
    R: Read,
{
    let mut scanner = LitematicPreflightMetadataScanner::new(reader, limits);
    let unknown = match scanner.scan_root() {
        Ok(PreflightMetadataFinish::MetadataComplete) => LitematicMetadataUnknown::FieldMissing,
        Ok(PreflightMetadataFinish::MetadataMissing) => LitematicMetadataUnknown::MetadataMissing,
        Err(PreflightMetadataScanFailure::Limit(reason)) => {
            return Ok(scanner.finish_unknown(reason));
        }
        Err(PreflightMetadataScanFailure::Invalid(error)) => return Err(error),
    };
    Ok(scanner.finish_complete(unknown))
}

#[derive(Default)]
struct LitematicPreflightMetadataFields {
    total_blocks: Option<u64>,
    total_volume: Option<u64>,
    region_count: Option<u64>,
    enclosing_size: Option<LitematicMetadataEnclosingSize>,
}

impl LitematicPreflightMetadataFields {
    fn into_result(
        self,
        unknown: LitematicMetadataUnknown,
        decompressed_bytes_read: u64,
    ) -> LitematicPreflightMetadata {
        LitematicPreflightMetadata {
            total_blocks: self.total_blocks.map_or(
                LitematicMetadataValue::Unknown(unknown),
                LitematicMetadataValue::Known,
            ),
            total_volume: self.total_volume.map_or(
                LitematicMetadataValue::Unknown(unknown),
                LitematicMetadataValue::Known,
            ),
            region_count: self.region_count.map_or(
                LitematicMetadataValue::Unknown(unknown),
                LitematicMetadataValue::Known,
            ),
            enclosing_size: self.enclosing_size.map_or(
                LitematicMetadataValue::Unknown(unknown),
                LitematicMetadataValue::Known,
            ),
            decompressed_bytes_read,
        }
    }
}

enum PreflightMetadataFinish {
    MetadataComplete,
    MetadataMissing,
}

enum PreflightMetadataScanFailure {
    Limit(LitematicMetadataUnknown),
    Invalid(anyhow::Error),
}

type PreflightMetadataScanResult<T> = std::result::Result<T, PreflightMetadataScanFailure>;

struct LitematicPreflightMetadataScanner<R> {
    reader: R,
    decompressed_bytes: u64,
    max_decompressed_bytes: u64,
    started_at: Instant,
    deadline: Duration,
    fields: LitematicPreflightMetadataFields,
    frames: Vec<NbtContainerFrame>,
}

impl<R> LitematicPreflightMetadataScanner<R>
where
    R: Read,
{
    fn new(reader: R, limits: LitematicMetadataScanLimits) -> Self {
        Self {
            reader,
            decompressed_bytes: 0,
            max_decompressed_bytes: limits.max_decompressed_bytes,
            started_at: Instant::now(),
            deadline: limits.deadline,
            fields: LitematicPreflightMetadataFields::default(),
            frames: Vec::with_capacity(MAX_NBT_CONTAINER_DEPTH),
        }
    }

    fn finish_complete(self, unknown: LitematicMetadataUnknown) -> LitematicPreflightMetadata {
        self.fields.into_result(unknown, self.decompressed_bytes)
    }

    fn finish_unknown(self, unknown: LitematicMetadataUnknown) -> LitematicPreflightMetadata {
        // A limit reached before the Metadata compound terminates means its
        // apparent values have not been structurally validated for duplicate
        // fields. Fail closed instead of publishing a partial estimate.
        LitematicPreflightMetadataFields::default().into_result(unknown, self.decompressed_bytes)
    }

    fn scan_root(&mut self) -> PreflightMetadataScanResult<PreflightMetadataFinish> {
        let root_tag = self.read_u8("root tag type")?;
        if root_tag != 10 {
            return Err(Self::invalid(anyhow!(
                "litematic NBT root tag type is {root_tag}, expected TAG_Compound (10)"
            )));
        }
        self.skip_string_bytes("root tag name")?;

        loop {
            let tag = self.read_u8("root compound child tag type")?;
            if tag == 0 {
                return Ok(PreflightMetadataFinish::MetadataMissing);
            }
            self.validate_tag(tag, "root compound child")?;
            let name = self.read_bounded_name("root compound child name")?;
            if name.as_deref() == Some(PREFLIGHT_METADATA_NAME) {
                if tag != 10 {
                    return Err(Self::invalid(anyhow!(
                        "litematic root Metadata has tag type {tag}, expected TAG_Compound (10)"
                    )));
                }
                self.scan_metadata_compound()?;
                return Ok(PreflightMetadataFinish::MetadataComplete);
            }
            self.skip_payload(tag, 1)?;
        }
    }

    fn scan_metadata_compound(&mut self) -> PreflightMetadataScanResult<()> {
        let mut total_blocks_seen = false;
        let mut total_volume_seen = false;
        let mut region_count_seen = false;
        let mut enclosing_size_seen = false;
        loop {
            let tag = self.read_u8("Metadata child tag type")?;
            if tag == 0 {
                return Ok(());
            }
            self.validate_tag(tag, "Metadata child")?;
            let name = self.read_bounded_name("Metadata child name")?;
            match name.as_deref() {
                Some(PREFLIGHT_TOTAL_BLOCKS_NAME) => {
                    Self::reject_duplicate(&mut total_blocks_seen, "TotalBlocks")?;
                    self.require_tag(tag, 3, "Metadata TotalBlocks", "TAG_Int")?;
                    self.fields.total_blocks =
                        Some(self.read_nonnegative_i32("Metadata TotalBlocks value")?);
                }
                Some(PREFLIGHT_TOTAL_VOLUME_NAME) => {
                    Self::reject_duplicate(&mut total_volume_seen, "TotalVolume")?;
                    self.require_tag(tag, 3, "Metadata TotalVolume", "TAG_Int")?;
                    self.fields.total_volume =
                        Some(self.read_nonnegative_i32("Metadata TotalVolume value")?);
                }
                Some(PREFLIGHT_REGION_COUNT_NAME) => {
                    Self::reject_duplicate(&mut region_count_seen, "RegionCount")?;
                    self.require_tag(tag, 3, "Metadata RegionCount", "TAG_Int")?;
                    self.fields.region_count =
                        Some(self.read_nonnegative_i32("Metadata RegionCount value")?);
                }
                Some(PREFLIGHT_ENCLOSING_SIZE_NAME) => {
                    Self::reject_duplicate(&mut enclosing_size_seen, "EnclosingSize")?;
                    self.require_tag(tag, 10, "Metadata EnclosingSize", "TAG_Compound")?;
                    self.fields.enclosing_size = self.scan_enclosing_size()?;
                }
                _ => self.skip_payload(tag, 2)?,
            }
        }
    }

    fn scan_enclosing_size(
        &mut self,
    ) -> PreflightMetadataScanResult<Option<LitematicMetadataEnclosingSize>> {
        let mut x = None;
        let mut y = None;
        let mut z = None;
        loop {
            let tag = self.read_u8("EnclosingSize child tag type")?;
            if tag == 0 {
                return Ok(match (x, y, z) {
                    (Some(x), Some(y), Some(z)) => Some(LitematicMetadataEnclosingSize { x, y, z }),
                    _ => None,
                });
            }
            self.validate_tag(tag, "EnclosingSize child")?;
            let name = self.read_bounded_name("EnclosingSize child name")?;
            let axis = if name.as_deref() == Some(&b"X"[..]) || name.as_deref() == Some(&b"x"[..]) {
                Some((&mut x, "X"))
            } else if name.as_deref() == Some(&b"Y"[..]) || name.as_deref() == Some(&b"y"[..]) {
                Some((&mut y, "Y"))
            } else if name.as_deref() == Some(&b"Z"[..]) || name.as_deref() == Some(&b"z"[..]) {
                Some((&mut z, "Z"))
            } else {
                None
            };
            if let Some((value, axis_name)) = axis {
                if value.is_some() {
                    return Err(Self::invalid(anyhow!(
                        "litematic Metadata EnclosingSize has duplicate {axis_name} fields"
                    )));
                }
                self.require_tag(tag, 3, "Metadata EnclosingSize axis", "TAG_Int")?;
                *value = Some(self.read_nonnegative_i32("Metadata EnclosingSize axis value")?);
            } else {
                self.skip_payload(tag, 3)?;
            }
        }
    }

    fn reject_duplicate(seen: &mut bool, field: &'static str) -> PreflightMetadataScanResult<()> {
        if std::mem::replace(seen, true) {
            return Err(Self::invalid(anyhow!(
                "litematic Metadata has duplicate {field} fields"
            )));
        }
        Ok(())
    }

    fn require_tag(
        &self,
        actual: u8,
        expected: u8,
        context: &'static str,
        expected_name: &'static str,
    ) -> PreflightMetadataScanResult<()> {
        if actual != expected {
            return Err(Self::invalid(anyhow!(
                "litematic {context} has tag type {actual}, expected {expected_name} ({expected})"
            )));
        }
        Ok(())
    }

    fn validate_tag(&self, tag: u8, context: &'static str) -> PreflightMetadataScanResult<()> {
        validate_payload_tag(tag, context).map_err(PreflightMetadataScanFailure::Invalid)
    }

    fn skip_payload(&mut self, tag: u8, container_depth: usize) -> PreflightMetadataScanResult<()> {
        debug_assert!(self.frames.is_empty());
        let result = self.skip_payload_iterative(tag, container_depth);
        self.frames.clear();
        result
    }

    fn skip_payload_iterative(
        &mut self,
        tag: u8,
        parent_depth: usize,
    ) -> PreflightMetadataScanResult<()> {
        let mut pending = Some(tag);
        loop {
            if let Some(tag) = pending.take() {
                self.validate_tag(tag, "payload")?;
                match tag {
                    1 => self.skip_bytes(1, "TAG_Byte payload")?,
                    2 => self.skip_bytes(2, "TAG_Short payload")?,
                    3 | 5 => self.skip_bytes(4, "four-byte scalar payload")?,
                    4 | 6 => self.skip_bytes(8, "eight-byte scalar payload")?,
                    7 => self.skip_array_payload("TAG_Byte_Array", 1)?,
                    8 => self.skip_string_bytes("TAG_String payload")?,
                    9 => {
                        self.record_next_container_depth(parent_depth)?;
                        let element_tag = self.read_u8("TAG_List element tag type")?;
                        let length = self.read_nonnegative_i32("TAG_List length")?;
                        if element_tag == 0 {
                            if length != 0 {
                                return Err(Self::invalid(anyhow!(
                                    "TAG_List uses TAG_End elements with non-zero length {length}"
                                )));
                            }
                        } else {
                            self.validate_tag(element_tag, "TAG_List element")?;
                            if let Some(element_bytes) = fixed_scalar_payload_bytes(element_tag) {
                                let bytes = length.checked_mul(element_bytes).ok_or_else(|| {
                                    Self::invalid(anyhow!("TAG_List scalar byte length overflow"))
                                })?;
                                self.skip_bytes(bytes, "TAG_List scalar elements")?;
                            } else if length != 0 {
                                self.frames.push(NbtContainerFrame::List {
                                    element_tag,
                                    remaining: length - 1,
                                });
                                pending = Some(element_tag);
                                continue;
                            }
                        }
                    }
                    10 => {
                        self.record_next_container_depth(parent_depth)?;
                        self.frames.push(NbtContainerFrame::Compound);
                    }
                    11 => self.skip_array_payload("TAG_Int_Array", 4)?,
                    12 => self.skip_array_payload("TAG_Long_Array", 8)?,
                    _ => unreachable!("validated NBT payload tag"),
                }
            }

            loop {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                match frame {
                    NbtContainerFrame::Compound => {
                        let child_tag = self.read_u8("TAG_Compound child tag type")?;
                        if child_tag == 0 {
                            continue;
                        }
                        self.validate_tag(child_tag, "TAG_Compound child")?;
                        self.skip_string_bytes("TAG_Compound child name")?;
                        self.frames.push(NbtContainerFrame::Compound);
                        pending = Some(child_tag);
                        break;
                    }
                    NbtContainerFrame::List {
                        element_tag,
                        remaining,
                    } => {
                        if remaining == 0 {
                            continue;
                        }
                        self.frames.push(NbtContainerFrame::List {
                            element_tag,
                            remaining: remaining - 1,
                        });
                        pending = Some(element_tag);
                        break;
                    }
                }
            }
        }
    }

    fn record_next_container_depth(&self, parent_depth: usize) -> PreflightMetadataScanResult<()> {
        let depth = parent_depth
            .checked_add(self.frames.len())
            .and_then(|depth| depth.checked_add(1))
            .ok_or_else(|| Self::invalid(anyhow!("NBT container depth overflow")))?;
        if depth > MAX_NBT_CONTAINER_DEPTH {
            return Err(Self::invalid(anyhow!(
                "NBT container depth {depth} exceeds structural safety limit {MAX_NBT_CONTAINER_DEPTH}"
            )));
        }
        Ok(())
    }

    fn skip_array_payload(
        &mut self,
        kind: &'static str,
        element_bytes: u64,
    ) -> PreflightMetadataScanResult<()> {
        let length = self.read_nonnegative_i32(kind)?;
        let bytes = length
            .checked_mul(element_bytes)
            .ok_or_else(|| Self::invalid(anyhow!("{kind} byte length overflow")))?;
        self.skip_bytes(bytes, kind)
    }

    fn read_bounded_name(
        &mut self,
        context: &'static str,
    ) -> PreflightMetadataScanResult<Option<Vec<u8>>> {
        let length = usize::from(self.read_u16(context)?);
        if length > PREFLIGHT_METADATA_NAME_BUFFER_BYTES {
            self.skip_bytes(length as u64, context)?;
            return Ok(None);
        }
        let mut name = vec![0_u8; length];
        self.read_exact(&mut name, context)?;
        Ok(Some(name))
    }

    fn skip_string_bytes(&mut self, context: &'static str) -> PreflightMetadataScanResult<()> {
        let length = u64::from(self.read_u16(context)?);
        self.skip_bytes(length, context)
    }

    fn read_nonnegative_i32(&mut self, context: &'static str) -> PreflightMetadataScanResult<u64> {
        let value = self.read_i32(context)?;
        if value < 0 {
            return Err(Self::invalid(anyhow!(
                "{context} has negative value {value}"
            )));
        }
        Ok(value as u64)
    }

    fn read_u8(&mut self, context: &'static str) -> PreflightMetadataScanResult<u8> {
        let mut bytes = [0_u8; 1];
        self.read_exact(&mut bytes, context)?;
        Ok(bytes[0])
    }

    fn read_u16(&mut self, context: &'static str) -> PreflightMetadataScanResult<u16> {
        let mut bytes = [0_u8; 2];
        self.read_exact(&mut bytes, context)?;
        Ok(u16::from_be_bytes(bytes))
    }

    fn read_i32(&mut self, context: &'static str) -> PreflightMetadataScanResult<i32> {
        let mut bytes = [0_u8; 4];
        self.read_exact(&mut bytes, context)?;
        Ok(i32::from_be_bytes(bytes))
    }

    fn read_exact(
        &mut self,
        bytes: &mut [u8],
        context: &'static str,
    ) -> PreflightMetadataScanResult<()> {
        self.check_deadline()?;
        let byte_count = bytes.len() as u64;
        if byte_count
            > self
                .max_decompressed_bytes
                .saturating_sub(self.decompressed_bytes)
        {
            return Err(PreflightMetadataScanFailure::Limit(
                LitematicMetadataUnknown::ByteLimitExceeded,
            ));
        }
        self.reader
            .read_exact(bytes)
            .map_err(|error| Self::invalid(anyhow!("cannot read {context}: {error}")))?;
        self.decompressed_bytes = self
            .decompressed_bytes
            .checked_add(byte_count)
            .ok_or_else(|| Self::invalid(anyhow!("NBT decompressed byte count overflow")))?;
        self.check_deadline()
    }

    fn skip_bytes(
        &mut self,
        mut remaining: u64,
        context: &'static str,
    ) -> PreflightMetadataScanResult<()> {
        let mut discard = [0_u8; PREFLIGHT_SKIP_BUFFER_BYTES];
        while remaining != 0 {
            let chunk = remaining.min(PREFLIGHT_SKIP_BUFFER_BYTES as u64) as usize;
            self.read_exact(&mut discard[..chunk], context)?;
            remaining -= chunk as u64;
        }
        Ok(())
    }

    fn check_deadline(&self) -> PreflightMetadataScanResult<()> {
        if self.started_at.elapsed() >= self.deadline {
            return Err(PreflightMetadataScanFailure::Limit(
                LitematicMetadataUnknown::DeadlineExceeded,
            ));
        }
        Ok(())
    }

    fn invalid(error: anyhow::Error) -> PreflightMetadataScanFailure {
        PreflightMetadataScanFailure::Invalid(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LitematicDataVersionScan {
    minecraft_data_version: Option<i32>,
    decompressed_bytes: u64,
    logical_values: u64,
    max_depth: usize,
    max_read_request_bytes: usize,
    container_frame_bytes: usize,
}

impl LitematicDataVersionScan {
    pub(crate) const fn minecraft_data_version(self) -> Option<i32> {
        self.minecraft_data_version
    }

    pub(crate) const fn decompressed_bytes(self) -> u64 {
        self.decompressed_bytes
    }

    pub(crate) const fn logical_values(self) -> u64 {
        self.logical_values
    }

    pub(crate) const fn max_depth(self) -> usize {
        self.max_depth
    }

    pub(crate) const fn discard_buffer_bytes(self) -> usize {
        NBT_STREAM_BUFFER_BYTES
    }

    pub(crate) const fn max_read_request_bytes(self) -> usize {
        self.max_read_request_bytes
    }

    pub(crate) const fn container_frame_bytes(self) -> usize {
        self.container_frame_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LitematicDataVersionScanProgress {
    decompressed_bytes: u64,
    logical_values: u64,
}

impl LitematicDataVersionScanProgress {
    pub(crate) const fn decompressed_bytes(self) -> u64 {
        self.decompressed_bytes
    }

    pub(crate) const fn logical_values(self) -> u64 {
        self.logical_values
    }
}

/// The optional Litematica preview converted from NBT ARGB pixels into RGBA8.
///
/// Only this bounded pixel payload is retained. All unrelated NBT values are
/// structurally validated and discarded through the fixed streaming buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LitematicPreviewImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn load_litematic_preview_image(path: &Path) -> Result<Option<LitematicPreviewImage>> {
    let file = open_process_read_file(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let compressed = BufReader::with_capacity(NBT_STREAM_BUFFER_BYTES, file);
    scan_preview_image_from_gzip(compressed)
        .with_context(|| format!("failed to scan PreviewImageData in {}", path.display()))
}

/// Content identity captured from the same compressed byte stream that feeds
/// the file-backed litematic decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LitematicCompressedFingerprint {
    sha256: [u8; 32],
    byte_count: u64,
}

impl LitematicCompressedFingerprint {
    pub(crate) const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    pub(crate) const fn byte_count(self) -> u64 {
        self.byte_count
    }
}

pub(crate) fn scan_litematic_minecraft_data_version_with_progress(
    path: &Path,
    progress: &mut dyn FnMut(LitematicDataVersionScanProgress),
) -> Result<LitematicDataVersionScan> {
    let file = open_process_read_file(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let compressed = BufReader::with_capacity(NBT_STREAM_BUFFER_BYTES, file);
    scan_minecraft_data_version_from_gzip_with_progress(compressed, progress)
        .with_context(|| format!("failed to scan MinecraftDataVersion in {}", path.display()))
}

#[cfg(test)]
fn scan_minecraft_data_version_from_gzip<R>(compressed: R) -> Result<LitematicDataVersionScan>
where
    R: BufRead,
{
    scan_minecraft_data_version_from_gzip_with_progress(compressed, &mut |_| {})
}

fn scan_minecraft_data_version_from_gzip_with_progress<R>(
    compressed: R,
    progress: &mut dyn FnMut(LitematicDataVersionScanProgress),
) -> Result<LitematicDataVersionScan>
where
    R: BufRead,
{
    let decoder = BufReadGzDecoder::new(compressed);
    let (scan, mut decoder) =
        scan_minecraft_data_version_from_nbt_with_progress(decoder, progress)?;
    let mut decompressed_tail = [0_u8; 1];
    if decoder
        .read(&mut decompressed_tail)
        .context("cannot finish the litematic gzip member")?
        != 0
    {
        bail!("litematic NBT has decompressed bytes after the root compound");
    }
    let mut compressed = decoder.into_inner();
    if !compressed
        .fill_buf()
        .context("cannot check for data after the litematic gzip member")?
        .is_empty()
    {
        bail!("litematic gzip has a second member or trailing compressed bytes");
    }
    Ok(scan)
}

fn scan_preview_image_from_gzip<R>(compressed: R) -> Result<Option<LitematicPreviewImage>>
where
    R: BufRead,
{
    let decoder = BufReadGzDecoder::new(compressed);
    let (preview, mut decoder) = scan_preview_image_from_nbt(decoder)?;
    let mut decompressed_tail = [0_u8; 1];
    if decoder
        .read(&mut decompressed_tail)
        .context("cannot finish the litematic gzip member")?
        != 0
    {
        bail!("litematic NBT has decompressed bytes after the root compound");
    }
    let mut compressed = decoder.into_inner();
    if !compressed
        .fill_buf()
        .context("cannot check for data after the litematic gzip member")?
        .is_empty()
    {
        bail!("litematic gzip has a second member or trailing compressed bytes");
    }
    Ok(preview)
}

#[cfg(test)]
fn scan_minecraft_data_version_from_nbt<R>(reader: R) -> Result<(LitematicDataVersionScan, R)>
where
    R: Read,
{
    scan_minecraft_data_version_from_nbt_with_progress(reader, &mut |_| {})
}

fn scan_minecraft_data_version_from_nbt_with_progress<R>(
    reader: R,
    progress: &mut dyn FnMut(LitematicDataVersionScanProgress),
) -> Result<(LitematicDataVersionScan, R)>
where
    R: Read,
{
    let mut scanner = MinecraftDataVersionScanner::new(reader, progress);
    scanner.report_progress(true);
    let minecraft_data_version = scanner.scan_root()?;
    scanner.report_progress(true);
    let scan = LitematicDataVersionScan {
        minecraft_data_version,
        decompressed_bytes: scanner.decompressed_bytes,
        logical_values: scanner.logical_values,
        max_depth: scanner.max_depth,
        max_read_request_bytes: scanner.max_read_request_bytes,
        container_frame_bytes: scanner.frames.capacity() * std::mem::size_of::<NbtContainerFrame>(),
    };
    Ok((scan, scanner.reader))
}

fn scan_preview_image_from_nbt<R>(reader: R) -> Result<(Option<LitematicPreviewImage>, R)>
where
    R: Read,
{
    let mut progress = |_| {};
    let mut scanner = MinecraftDataVersionScanner::new(reader, &mut progress);
    let fields = scanner.scan_root_fields(true)?;
    Ok((fields.preview_image, scanner.reader))
}

#[derive(Default)]
struct LitematicRootFields {
    minecraft_data_version: Option<i32>,
    preview_image: Option<LitematicPreviewImage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootFieldName {
    MinecraftDataVersion,
    PreviewImageData,
    Other,
}

struct MinecraftDataVersionScanner<'progress, R> {
    reader: R,
    discard: [u8; NBT_STREAM_BUFFER_BYTES],
    decompressed_bytes: u64,
    logical_values: u64,
    max_depth: usize,
    max_read_request_bytes: usize,
    frames: Vec<NbtContainerFrame>,
    last_reported_decompressed_bytes: u64,
    progress: &'progress mut dyn FnMut(LitematicDataVersionScanProgress),
}

#[derive(Clone, Copy, Debug)]
enum NbtContainerFrame {
    Compound,
    List { element_tag: u8, remaining: u64 },
}

impl<'progress, R> MinecraftDataVersionScanner<'progress, R>
where
    R: Read,
{
    fn new(
        reader: R,
        progress: &'progress mut dyn FnMut(LitematicDataVersionScanProgress),
    ) -> Self {
        Self {
            reader,
            discard: [0; NBT_STREAM_BUFFER_BYTES],
            decompressed_bytes: 0,
            logical_values: 0,
            max_depth: 0,
            max_read_request_bytes: 0,
            frames: Vec::with_capacity(MAX_NBT_CONTAINER_DEPTH),
            last_reported_decompressed_bytes: 0,
            progress,
        }
    }

    fn report_progress(&mut self, force: bool) {
        if !force
            && self
                .decompressed_bytes
                .saturating_sub(self.last_reported_decompressed_bytes)
                < NBT_SCAN_PROGRESS_INTERVAL_BYTES
        {
            return;
        }
        (self.progress)(LitematicDataVersionScanProgress {
            decompressed_bytes: self.decompressed_bytes,
            logical_values: self.logical_values,
        });
        self.last_reported_decompressed_bytes = self.decompressed_bytes;
    }

    fn scan_root(&mut self) -> Result<Option<i32>> {
        Ok(self.scan_root_fields(false)?.minecraft_data_version)
    }

    fn scan_root_fields(&mut self, capture_preview: bool) -> Result<LitematicRootFields> {
        let root_tag = self.read_u8("root tag type")?;
        if root_tag != 10 {
            bail!("litematic NBT root tag type is {root_tag}, expected TAG_Compound (10)");
        }
        self.skip_string_bytes("root tag name")?;
        self.add_logical_values(1)?;
        self.record_container_depth(1)?;

        let mut fields = LitematicRootFields::default();
        let mut data_version_seen = false;
        let mut preview_image_seen = false;
        loop {
            let tag = self.read_u8("root compound child tag type")?;
            if tag == 0 {
                return Ok(fields);
            }
            validate_payload_tag(tag, "root compound child")?;
            match self.read_root_field_name()? {
                RootFieldName::MinecraftDataVersion => {
                    if data_version_seen {
                        bail!("litematic root has duplicate MinecraftDataVersion fields");
                    }
                    data_version_seen = true;
                    if tag != 3 {
                        bail!(
                            "litematic root MinecraftDataVersion has tag type {tag}, expected TAG_Int (3)"
                        );
                    }
                    self.add_logical_values(1)?;
                    fields.minecraft_data_version =
                        Some(self.read_i32("MinecraftDataVersion value")?);
                }
                RootFieldName::PreviewImageData if capture_preview => {
                    if preview_image_seen {
                        bail!("litematic root has duplicate PreviewImageData fields");
                    }
                    preview_image_seen = true;
                    if tag != 11 {
                        bail!(
                            "litematic root PreviewImageData has tag type {tag}, expected TAG_Int_Array (11)"
                        );
                    }
                    self.add_logical_values(1)?;
                    fields.preview_image = self.read_preview_image_data()?;
                }
                _ => self.skip_payload(tag, 1)?,
            }
        }
    }

    fn skip_payload(&mut self, tag: u8, container_depth: usize) -> Result<()> {
        debug_assert!(self.frames.is_empty());
        let result = self.skip_payload_iterative(tag, container_depth);
        self.frames.clear();
        result
    }

    fn skip_payload_iterative(&mut self, tag: u8, parent_depth: usize) -> Result<()> {
        let mut pending = Some(tag);
        loop {
            if let Some(tag) = pending.take() {
                validate_payload_tag(tag, "payload")?;
                self.add_logical_values(1)?;
                match tag {
                    1 => self.skip_bytes(1, "TAG_Byte payload")?,
                    2 => self.skip_bytes(2, "TAG_Short payload")?,
                    3 | 5 => self.skip_bytes(4, "four-byte scalar payload")?,
                    4 | 6 => self.skip_bytes(8, "eight-byte scalar payload")?,
                    7 => self.skip_array_payload("TAG_Byte_Array", 1)?,
                    8 => self.skip_string_bytes("TAG_String payload")?,
                    9 => {
                        self.record_next_container_depth(parent_depth)?;
                        let element_tag = self.read_u8("TAG_List element tag type")?;
                        let length = self.read_nonnegative_i32_length("TAG_List")?;
                        if element_tag == 0 {
                            if length != 0 {
                                bail!(
                                    "TAG_List uses TAG_End elements with non-zero length {length}"
                                );
                            }
                        } else {
                            validate_payload_tag(element_tag, "TAG_List element")?;
                            if let Some(element_bytes) = fixed_scalar_payload_bytes(element_tag) {
                                self.add_logical_values(length)?;
                                let bytes = length.checked_mul(element_bytes).ok_or_else(|| {
                                    anyhow!("TAG_List scalar byte length overflow")
                                })?;
                                self.skip_bytes(bytes, "TAG_List scalar elements")?;
                            } else if length != 0 {
                                self.frames.push(NbtContainerFrame::List {
                                    element_tag,
                                    remaining: length - 1,
                                });
                                pending = Some(element_tag);
                                continue;
                            }
                        }
                    }
                    10 => {
                        self.record_next_container_depth(parent_depth)?;
                        self.frames.push(NbtContainerFrame::Compound);
                    }
                    11 => self.skip_array_payload("TAG_Int_Array", 4)?,
                    12 => self.skip_array_payload("TAG_Long_Array", 8)?,
                    _ => unreachable!("validated NBT payload tag"),
                }
            }

            loop {
                let Some(frame) = self.frames.pop() else {
                    return Ok(());
                };
                match frame {
                    NbtContainerFrame::Compound => {
                        let child_tag = self.read_u8("TAG_Compound child tag type")?;
                        if child_tag == 0 {
                            continue;
                        }
                        validate_payload_tag(child_tag, "TAG_Compound child")?;
                        self.skip_string_bytes("TAG_Compound child name")?;
                        self.frames.push(NbtContainerFrame::Compound);
                        pending = Some(child_tag);
                        break;
                    }
                    NbtContainerFrame::List {
                        element_tag,
                        remaining,
                    } => {
                        if remaining == 0 {
                            continue;
                        }
                        self.frames.push(NbtContainerFrame::List {
                            element_tag,
                            remaining: remaining - 1,
                        });
                        pending = Some(element_tag);
                        break;
                    }
                }
            }
        }
    }

    fn skip_array_payload(&mut self, kind: &'static str, element_bytes: u64) -> Result<()> {
        let length = self.read_nonnegative_i32_length(kind)?;
        self.add_logical_values(length)?;
        let bytes = length
            .checked_mul(element_bytes)
            .ok_or_else(|| anyhow!("{kind} byte length overflow"))?;
        self.skip_bytes(bytes, kind)
    }

    fn record_next_container_depth(&mut self, parent_depth: usize) -> Result<()> {
        let depth = parent_depth
            .checked_add(self.frames.len())
            .and_then(|depth| depth.checked_add(1))
            .ok_or_else(|| anyhow!("NBT container depth overflow"))?;
        self.record_container_depth(depth)
    }

    fn record_container_depth(&mut self, depth: usize) -> Result<()> {
        if depth > MAX_NBT_CONTAINER_DEPTH {
            bail!(
                "NBT container depth {depth} exceeds structural safety limit {MAX_NBT_CONTAINER_DEPTH}"
            );
        }
        self.max_depth = self.max_depth.max(depth);
        Ok(())
    }

    fn add_logical_values(&mut self, count: u64) -> Result<()> {
        self.logical_values = self
            .logical_values
            .checked_add(count)
            .ok_or_else(|| anyhow!("NBT logical value count overflow"))?;
        Ok(())
    }

    fn read_root_field_name(&mut self) -> Result<RootFieldName> {
        let length = usize::from(self.read_u16("root compound child name length")?);
        let longest_name = MINECRAFT_DATA_VERSION_NAME
            .len()
            .max(PREVIEW_IMAGE_DATA_NAME.len());
        if length > longest_name {
            self.skip_bytes(length as u64, "root compound child name")?;
            return Ok(RootFieldName::Other);
        }
        let mut name = [0_u8; MINECRAFT_DATA_VERSION_NAME.len()];
        self.read_exact(&mut name[..length], "root compound child name")?;
        Ok(if &name[..length] == MINECRAFT_DATA_VERSION_NAME {
            RootFieldName::MinecraftDataVersion
        } else if &name[..length] == PREVIEW_IMAGE_DATA_NAME {
            RootFieldName::PreviewImageData
        } else {
            RootFieldName::Other
        })
    }

    fn read_preview_image_data(&mut self) -> Result<Option<LitematicPreviewImage>> {
        let pixel_count = self.read_nonnegative_i32_length("PreviewImageData")?;
        self.add_logical_values(pixel_count)?;
        if pixel_count == 0 {
            return Ok(None);
        }

        let edge = (pixel_count as f64).sqrt() as u64;
        if edge.checked_mul(edge) != Some(pixel_count) {
            bail!("PreviewImageData contains {pixel_count} pixels, expected a square pixel array");
        }
        if edge > u64::from(MAX_PREVIEW_IMAGE_EDGE) {
            bail!("PreviewImageData edge {edge} exceeds safety limit {MAX_PREVIEW_IMAGE_EDGE}");
        }
        let rgba_bytes = pixel_count
            .checked_mul(4)
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or_else(|| anyhow!("PreviewImageData RGBA byte length overflow"))?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(rgba_bytes)
            .map_err(|error| anyhow!("cannot allocate PreviewImageData RGBA buffer: {error}"))?;
        rgba.resize(rgba_bytes, 0);

        let mut argb = [0_u8; NBT_STREAM_BUFFER_BYTES];
        let mut pixel_offset = 0_usize;
        while pixel_offset < rgba_bytes {
            let byte_count = (rgba_bytes - pixel_offset).min(argb.len());
            // Both lengths are multiples of four, so every chunk contains
            // complete ARGB pixels and can be converted in place in the output.
            self.read_exact(&mut argb[..byte_count], "PreviewImageData pixel payload")?;
            for (source, destination) in argb[..byte_count]
                .chunks_exact(4)
                .zip(rgba[pixel_offset..pixel_offset + byte_count].chunks_exact_mut(4))
            {
                destination.copy_from_slice(&[source[1], source[2], source[3], source[0]]);
            }
            pixel_offset += byte_count;
        }

        Ok(Some(LitematicPreviewImage {
            width: edge as u32,
            height: edge as u32,
            rgba,
        }))
    }

    fn skip_string_bytes(&mut self, context: &'static str) -> Result<()> {
        let length = u64::from(self.read_u16(context)?);
        self.skip_bytes(length, context)
    }

    fn read_nonnegative_i32_length(&mut self, context: &'static str) -> Result<u64> {
        let length = self.read_i32(context)?;
        if length < 0 {
            bail!("{context} has negative length {length}");
        }
        Ok(length as u64)
    }

    fn read_u8(&mut self, context: &'static str) -> Result<u8> {
        let mut bytes = [0_u8; 1];
        self.read_exact(&mut bytes, context)?;
        Ok(bytes[0])
    }

    fn read_u16(&mut self, context: &'static str) -> Result<u16> {
        let mut bytes = [0_u8; 2];
        self.read_exact(&mut bytes, context)?;
        Ok(u16::from_be_bytes(bytes))
    }

    fn read_i32(&mut self, context: &'static str) -> Result<i32> {
        let mut bytes = [0_u8; 4];
        self.read_exact(&mut bytes, context)?;
        Ok(i32::from_be_bytes(bytes))
    }

    fn read_exact(&mut self, bytes: &mut [u8], context: &'static str) -> Result<()> {
        self.max_read_request_bytes = self.max_read_request_bytes.max(bytes.len());
        self.reader
            .read_exact(bytes)
            .with_context(|| format!("cannot read {context}"))?;
        self.decompressed_bytes = self
            .decompressed_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow!("NBT decompressed byte count overflow"))?;
        self.report_progress(false);
        Ok(())
    }

    fn skip_bytes(&mut self, mut remaining: u64, context: &'static str) -> Result<()> {
        while remaining != 0 {
            let chunk = remaining.min(NBT_STREAM_BUFFER_BYTES as u64) as usize;
            self.max_read_request_bytes = self.max_read_request_bytes.max(chunk);
            self.reader
                .read_exact(&mut self.discard[..chunk])
                .with_context(|| format!("cannot skip {context}"))?;
            self.decompressed_bytes = self
                .decompressed_bytes
                .checked_add(chunk as u64)
                .ok_or_else(|| anyhow!("NBT decompressed byte count overflow"))?;
            self.report_progress(false);
            remaining -= chunk as u64;
        }
        Ok(())
    }
}

fn validate_payload_tag(tag: u8, context: &'static str) -> Result<()> {
    if (1..=12).contains(&tag) {
        Ok(())
    } else {
        bail!("unknown NBT tag type {tag} in {context}")
    }
}

fn fixed_scalar_payload_bytes(tag: u8) -> Option<u64> {
    match tag {
        1 => Some(1),
        2 => Some(2),
        3 | 5 => Some(4),
        4 | 6 => Some(8),
        _ => None,
    }
}

struct SpoolingReader<'writer, R, W> {
    reader: R,
    writer: &'writer mut W,
    bytes: u64,
}

struct HashingReader<R> {
    reader: R,
    sha256: Sha256,
    bytes: u64,
}

impl<R> HashingReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            sha256: Sha256::new(),
            bytes: 0,
        }
    }

    fn finish(self) -> Result<(R, LitematicCompressedFingerprint)> {
        Ok((
            self.reader,
            LitematicCompressedFingerprint {
                sha256: self.sha256.finalize().into(),
                byte_count: self.bytes,
            },
        ))
    }
}

impl<R> Read for HashingReader<R>
where
    R: Read,
{
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.reader.read(buffer)?;
        self.sha256.update(&buffer[..read]);
        self.bytes = self
            .bytes
            .checked_add(read as u64)
            .ok_or_else(|| std::io::Error::other("compressed litematic byte count overflow"))?;
        Ok(read)
    }
}

impl<'writer, R, W> SpoolingReader<'writer, R, W> {
    fn new(reader: R, writer: &'writer mut W) -> Self {
        Self {
            reader,
            writer,
            bytes: 0,
        }
    }

    fn spooled_bytes(&self) -> u64 {
        self.bytes
    }

    fn into_inner(self) -> R {
        self.reader
    }
}

impl<R, W> Read for SpoolingReader<'_, R, W>
where
    R: Read,
    W: Write,
{
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.reader.read(buffer)?;
        if read == 0 {
            return Ok(0);
        }
        self.writer.write_all(&buffer[..read])?;
        self.bytes = self
            .bytes
            .checked_add(read as u64)
            .ok_or_else(|| std::io::Error::other("spooled NBT byte count overflow"))?;
        Ok(read)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LitematicRoot {
    pub metadata: MetadataNbt,
    pub regions: BTreeMap<String, RegionNbt>,
    pub version: i32,
    pub sub_version: Option<i32>,
    #[serde(rename = "MinecraftDataVersion")]
    pub minecraft_data_version: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MetadataNbt {
    pub author: Option<String>,
    pub description: Option<String>,
    pub name: Option<String>,
    pub time_created: Option<i64>,
    pub time_modified: Option<i64>,
    pub total_blocks: Option<i32>,
    pub total_volume: Option<i32>,
    pub region_count: Option<i32>,
    pub enclosing_size: Option<EnclosingSize>,
}

#[derive(Clone)]
pub struct BigEndianLongArray {
    storage: BigEndianLongArrayStorage,
}

#[derive(Clone)]
enum BigEndianLongArrayStorage {
    Owned(Vec<u8>),
    File(FileLongArray),
}

#[derive(Clone)]
struct FileLongArray {
    spool: Arc<BlockStatesSpool>,
    offset: u64,
    len: usize,
}

struct BlockStatesSpool {
    file: File,
    len: u64,
    id: u64,
}

#[derive(Clone)]
struct FileBackedParseContext {
    mmap_base: usize,
    mmap_len: usize,
    spool: Arc<BlockStatesSpool>,
}

#[derive(Default)]
struct BlockStatesPageCacheEntry {
    spool_id: u64,
    page_start: u64,
    valid: usize,
    bytes: Box<[u8]>,
    #[cfg(test)]
    fill_count: usize,
}

impl BlockStatesPageCacheEntry {
    fn matches(&self, spool_id: u64, page_start: u64) -> bool {
        self.spool_id == spool_id && self.page_start == page_start
    }
}

struct BlockStatesPageCache {
    hot: BlockStatesPageCacheEntry,
    retained: [BlockStatesPageCacheEntry; BLOCK_STATES_RETAINED_PAGE_COUNT],
    next_victim: usize,
}

impl Default for BlockStatesPageCache {
    fn default() -> Self {
        Self {
            hot: BlockStatesPageCacheEntry::default(),
            retained: std::array::from_fn(|_| BlockStatesPageCacheEntry::default()),
            next_victim: 0,
        }
    }
}

impl BlockStatesPageCache {
    fn select(&mut self, spool_id: u64, page_start: u64) -> &mut BlockStatesPageCacheEntry {
        if !self.hot.matches(spool_id, page_start) {
            if let Some(index) = self
                .retained
                .iter()
                .position(|entry| entry.matches(spool_id, page_start))
            {
                std::mem::swap(&mut self.hot, &mut self.retained[index]);
            } else {
                std::mem::swap(&mut self.hot, &mut self.retained[self.next_victim]);
                self.next_victim = (self.next_victim + 1) % self.retained.len();
            }
        }
        &mut self.hot
    }
}

thread_local! {
    static FILE_BACKED_PARSE_CONTEXT: RefCell<Option<FileBackedParseContext>> = const {
        RefCell::new(None)
    };
    static BLOCK_STATES_PAGE_CACHE: RefCell<BlockStatesPageCache> =
        RefCell::new(BlockStatesPageCache::default());
}

struct FileBackedParseContextGuard;

impl Drop for FileBackedParseContextGuard {
    fn drop(&mut self) {
        FILE_BACKED_PARSE_CONTEXT.with(|slot| {
            slot.replace(None);
        });
    }
}

impl fmt::Debug for BigEndianLongArray {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let storage = match self.storage {
            BigEndianLongArrayStorage::Owned(_) => "owned",
            BigEndianLongArrayStorage::File(_) => "file",
        };
        formatter
            .debug_struct("BigEndianLongArray")
            .field("len", &self.len())
            .field("storage", &storage)
            .finish()
    }
}

impl PartialEq for BigEndianLongArray {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        match (&self.storage, &other.storage) {
            (BigEndianLongArrayStorage::Owned(left), BigEndianLongArrayStorage::Owned(right)) => {
                left == right
            }
            (BigEndianLongArrayStorage::File(left), BigEndianLongArrayStorage::File(right))
                if Arc::ptr_eq(&left.spool, &right.spool)
                    && left.offset == right.offset
                    && left.len == right.len =>
            {
                true
            }
            _ => (0..self.len()).all(|index| self.u64_at(index) == other.u64_at(index)),
        }
    }
}

impl Eq for BigEndianLongArray {}

impl BigEndianLongArray {
    pub fn new(values: Vec<i64>) -> Self {
        let byte_len = values
            .len()
            .checked_mul(size_of::<i64>())
            .expect("long array byte length overflow");
        let mut bytes = Vec::with_capacity(byte_len);
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        Self {
            storage: BigEndianLongArrayStorage::Owned(bytes),
        }
    }

    pub fn len(&self) -> usize {
        self.byte_len() / size_of::<i64>()
    }

    pub fn is_empty(&self) -> bool {
        self.byte_len() == 0
    }

    #[track_caller]
    pub fn i64_at(&self, index: usize) -> i64 {
        i64::from_be_bytes(self.long_bytes_at(index))
    }

    #[track_caller]
    pub fn u64_at(&self, index: usize) -> u64 {
        u64::from_be_bytes(self.long_bytes_at(index))
    }

    fn byte_len(&self) -> usize {
        match &self.storage {
            BigEndianLongArrayStorage::Owned(bytes) => bytes.len(),
            BigEndianLongArrayStorage::File(range) => range.len,
        }
    }

    #[track_caller]
    fn long_bytes_at(&self, index: usize) -> [u8; size_of::<i64>()] {
        let start = index
            .checked_mul(size_of::<i64>())
            .expect("long array index overflow");
        let end = start
            .checked_add(size_of::<i64>())
            .expect("long array index overflow");
        assert!(end <= self.byte_len(), "long array index out of bounds");
        match &self.storage {
            BigEndianLongArrayStorage::Owned(bytes) => bytes[start..end]
                .try_into()
                .expect("validated long array index"),
            BigEndianLongArrayStorage::File(range) => {
                let absolute = range
                    .offset
                    .checked_add(start as u64)
                    .expect("long array file offset overflow");
                let mut bytes = [0_u8; size_of::<i64>()];
                read_block_states_cached(&range.spool, absolute, &mut bytes)
                    .unwrap_or_else(|error| panic!("cannot read file-backed long array: {error}"));
                bytes
            }
        }
    }

    fn from_big_endian_bytes(bytes: Vec<u8>) -> Result<Self, &'static str> {
        if !bytes.len().is_multiple_of(size_of::<i64>()) {
            return Err("NBT long array byte length must be a multiple of 8");
        }
        Ok(Self {
            storage: BigEndianLongArrayStorage::Owned(bytes),
        })
    }

    fn from_borrowed_big_endian_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if !bytes.len().is_multiple_of(size_of::<i64>()) {
            return Err("NBT long array byte length must be a multiple of 8");
        }
        if bytes.is_empty() {
            return Ok(Self {
                storage: BigEndianLongArrayStorage::Owned(Vec::new()),
            });
        }
        FILE_BACKED_PARSE_CONTEXT.with(|slot| {
            let context = slot.borrow();
            let Some(context) = context.as_ref() else {
                return Ok(Self {
                    storage: BigEndianLongArrayStorage::Owned(bytes.to_vec()),
                });
            };
            let mmap_end = context
                .mmap_base
                .checked_add(context.mmap_len)
                .ok_or("file-backed NBT mmap address range overflow")?;
            let start = bytes.as_ptr() as usize;
            let end = start
                .checked_add(bytes.len())
                .ok_or("borrowed NBT long array address range overflow")?;
            if start < context.mmap_base || end > mmap_end {
                return Err("borrowed NBT long array lies outside the parse mmap");
            }
            let offset = start - context.mmap_base;
            let file_end = (offset as u64)
                .checked_add(bytes.len() as u64)
                .ok_or("file-backed NBT long array file range overflow")?;
            if file_end > context.spool.len {
                return Err("borrowed NBT long array lies outside the spool file");
            }
            Ok(Self {
                storage: BigEndianLongArrayStorage::File(FileLongArray {
                    spool: Arc::clone(&context.spool),
                    offset: offset as u64,
                    len: bytes.len(),
                }),
            })
        })
    }

    fn is_file_backed(&self) -> bool {
        matches!(self.storage, BigEndianLongArrayStorage::File(_))
    }
}

fn read_block_states_cached(
    spool: &BlockStatesSpool,
    offset: u64,
    destination: &mut [u8],
) -> std::io::Result<()> {
    let end = offset
        .checked_add(destination.len() as u64)
        .ok_or_else(|| std::io::Error::other("block state read offset overflow"))?;
    if end > spool.len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "file-backed block state range is truncated",
        ));
    }
    let page_start = offset / BLOCK_STATES_PAGE_BYTES as u64 * BLOCK_STATES_PAGE_BYTES as u64;
    let page_end = page_start
        .checked_add(BLOCK_STATES_PAGE_BYTES as u64)
        .ok_or_else(|| std::io::Error::other("block state page end overflow"))?;
    if end > page_end {
        return read_exact_at(&spool.file, destination, offset);
    }
    BLOCK_STATES_PAGE_CACHE.with(|slot| {
        let mut cache = slot.borrow_mut();
        let page = cache.select(spool.id, page_start);
        if !page.matches(spool.id, page_start) {
            if page.bytes.len() != BLOCK_STATES_PAGE_BYTES {
                page.bytes = vec![0_u8; BLOCK_STATES_PAGE_BYTES].into_boxed_slice();
            }
            let valid = (spool.len - page_start).min(BLOCK_STATES_PAGE_BYTES as u64) as usize;
            if let Err(error) = read_exact_at(&spool.file, &mut page.bytes[..valid], page_start) {
                page.spool_id = 0;
                page.valid = 0;
                return Err(error);
            }
            #[cfg(test)]
            {
                page.fill_count += 1;
            }
            page.spool_id = spool.id;
            page.page_start = page_start;
            page.valid = valid;
        }
        let start = (offset - page_start) as usize;
        let cached_end = start
            .checked_add(destination.len())
            .ok_or_else(|| std::io::Error::other("block state cache range overflow"))?;
        if cached_end > page.valid {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "file-backed block state page is truncated",
            ));
        }
        destination.copy_from_slice(&page.bytes[start..cached_end]);
        Ok(())
    })
}

fn read_exact_at(file: &File, mut destination: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    const POSITIONED_READ_CHUNK_BYTES: usize = 32 * 1024;
    while !destination.is_empty() {
        let chunk_len = destination.len().min(POSITIONED_READ_CHUNK_BYTES);
        let chunk = &mut destination[..chunk_len];
        #[cfg(unix)]
        let read = run_process_io(|| {
            use std::os::unix::fs::FileExt;
            file.read_at(chunk, offset)
        });
        #[cfg(windows)]
        let read = run_process_io(|| {
            use std::os::windows::fs::FileExt;
            file.seek_read(chunk, offset)
        });
        #[cfg(not(any(unix, windows)))]
        let read = run_process_io(|| {
            use std::io::{Seek, SeekFrom};
            let mut file = file.try_clone()?;
            file.seek(SeekFrom::Start(offset))?;
            file.read(chunk)
        });
        match read {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "file-backed block state range is truncated",
                ));
            }
            Ok(read) => {
                offset = offset
                    .checked_add(read as u64)
                    .ok_or_else(|| std::io::Error::other("block state read offset overflow"))?;
                destination = &mut destination[read..];
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

impl Serialize for BigEndianLongArray {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct LongArrayBytes<'a> {
            __fastnbt_long_array: &'a Bytes,
        }

        match &self.storage {
            BigEndianLongArrayStorage::Owned(bytes) => LongArrayBytes {
                __fastnbt_long_array: Bytes::new(bytes),
            }
            .serialize(serializer),
            BigEndianLongArrayStorage::File(range) if range.len == 0 => LongArrayBytes {
                __fastnbt_long_array: Bytes::new(&[]),
            }
            .serialize(serializer),
            BigEndianLongArrayStorage::File(range) => {
                // The spool is immutable after construction and remains open for the map lifetime.
                let mmap = unsafe {
                    MmapOptions::new()
                        .offset(range.offset)
                        .len(range.len)
                        .map(&range.spool.file)
                }
                .map_err(serde::ser::Error::custom)?;
                LongArrayBytes {
                    __fastnbt_long_array: Bytes::new(&mmap),
                }
                .serialize(serializer)
            }
        }
    }
}

struct DeserializedLongArrayPayload(BigEndianLongArray);

impl<'de> Deserialize<'de> for DeserializedLongArrayPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PayloadVisitor;

        impl<'de> Visitor<'de> for PayloadVisitor {
            type Value = DeserializedLongArrayPayload;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str("big-endian NBT long array bytes")
            }

            fn visit_borrowed_bytes<E>(self, value: &'de [u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                BigEndianLongArray::from_borrowed_big_endian_bytes(value)
                    .map(DeserializedLongArrayPayload)
                    .map_err(E::custom)
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let file_backed_parse =
                    FILE_BACKED_PARSE_CONTEXT.with(|slot| slot.borrow().is_some());
                if file_backed_parse && !value.is_empty() {
                    return Err(E::custom(
                        "file-backed NBT parser copied a non-empty long array",
                    ));
                }
                BigEndianLongArray::from_big_endian_bytes(value.to_vec())
                    .map(DeserializedLongArrayPayload)
                    .map_err(E::custom)
            }

            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let file_backed_parse =
                    FILE_BACKED_PARSE_CONTEXT.with(|slot| slot.borrow().is_some());
                if file_backed_parse && !value.is_empty() {
                    return Err(E::custom(
                        "file-backed NBT parser allocated a non-empty long array",
                    ));
                }
                BigEndianLongArray::from_big_endian_bytes(value)
                    .map(DeserializedLongArrayPayload)
                    .map_err(E::custom)
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut bytes = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
                while let Some(byte) = sequence.next_element::<u8>()? {
                    bytes.push(byte);
                }
                BigEndianLongArray::from_big_endian_bytes(bytes)
                    .map(DeserializedLongArrayPayload)
                    .map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_bytes(PayloadVisitor)
    }
}

impl<'de> Deserialize<'de> for BigEndianLongArray {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LongArrayVisitor;

        impl<'de> Visitor<'de> for LongArrayVisitor {
            type Value = BigEndianLongArray;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str("NBT long array wrapper")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let token = map.next_key::<&str>()?.ok_or_else(|| {
                    serde::de::Error::custom("expected NBT long array token, but got empty map")
                })?;
                if token != "__fastnbt_long_array" {
                    return Err(serde::de::Error::custom("expected NBT long array token"));
                }
                let value = map.next_value::<DeserializedLongArrayPayload>()?.0;
                if map.next_key::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom(
                        "NBT long array wrapper has multiple fields",
                    ));
                }
                Ok(value)
            }
        }

        deserializer.deserialize_map(LongArrayVisitor)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RegionNbt {
    pub position: Vec3i,
    pub size: Vec3i,
    pub block_state_palette: Vec<BlockStateNbt>,
    pub block_states: BigEndianLongArray,
    #[serde(default)]
    pub entities: Vec<EntityNbt>,
    #[serde(default, rename = "TileEntities", alias = "BlockEntities")]
    pub tile_entities: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Vec3i {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct RegionBounds {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub min_z: i32,
    pub max_z: i32,
}

pub fn load_litematic_root(path: &Path) -> Result<LitematicRoot> {
    let file = open_process_read_file(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let decoder = GzDecoder::new(file);
    from_reader(decoder).with_context(|| format!("failed to parse {}", path.display()))
}

/// Loads a litematic for the V5 renderer without retaining its potentially
/// hundreds-of-megabytes `BlockStates` arrays in the Rust heap.
///
/// The complete gzip member is streamed into an OS-managed unnamed temporary
/// file. The file is unlinked immediately on Unix and opened delete-on-close on
/// Windows. A read-only mmap exists only while `fastnbt` parses the root; every
/// non-empty `BlockStates` field retained by the returned root is an immutable
/// file range served through a fixed-size per-thread page cache.
#[cfg(any(test, feature = "datafix-oracle"))]
pub(crate) fn load_litematic_root_file_backed(path: &Path) -> Result<LitematicRoot> {
    load_litematic_root_file_backed_in(path, None)
}

pub(crate) fn load_litematic_root_file_backed_with_progress(
    path: &Path,
    progress: &mut dyn FnMut(LitematicDataVersionScanProgress),
) -> Result<LitematicRoot> {
    load_litematic_root_file_backed_in_with_progress(path, None, progress)
}

pub(crate) fn load_litematic_root_file_backed_with_progress_and_fingerprint(
    path: &Path,
    progress: &mut dyn FnMut(LitematicDataVersionScanProgress),
) -> Result<(LitematicRoot, LitematicCompressedFingerprint)> {
    load_litematic_root_file_backed_in_with_progress_and_fingerprint(path, None, progress)
}

#[cfg(any(test, feature = "datafix-oracle"))]
fn load_litematic_root_file_backed_in(
    path: &Path,
    temporary_directory: Option<&Path>,
) -> Result<LitematicRoot> {
    load_litematic_root_file_backed_in_with_progress(path, temporary_directory, &mut |_| {})
}

fn load_litematic_root_file_backed_in_with_progress(
    path: &Path,
    temporary_directory: Option<&Path>,
    progress: &mut dyn FnMut(LitematicDataVersionScanProgress),
) -> Result<LitematicRoot> {
    load_litematic_root_file_backed_in_with_progress_and_fingerprint(
        path,
        temporary_directory,
        progress,
    )
    .map(|(root, _)| root)
}

fn load_litematic_root_file_backed_in_with_progress_and_fingerprint(
    path: &Path,
    temporary_directory: Option<&Path>,
    progress: &mut dyn FnMut(LitematicDataVersionScanProgress),
) -> Result<(LitematicRoot, LitematicCompressedFingerprint)> {
    let file = open_process_read_file(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let compressed = BufReader::with_capacity(NBT_STREAM_BUFFER_BYTES, HashingReader::new(file));
    let decoder = BufReadGzDecoder::new(compressed);
    let mut spool_file = match temporary_directory {
        Some(directory) => tempfile::tempfile_in(directory).with_context(|| {
            format!(
                "failed to create litematic spool in {}",
                directory.display()
            )
        })?,
        None => tempfile::tempfile().context("failed to create litematic spool")?,
    };
    // Validate the complete NBT structure while decompression is already
    // streaming through a fixed buffer. The tee writes exactly those bytes to
    // the spool, avoiding a second full mmap scan that would fault every
    // BlockStates page back into the process working set.
    let mut spool = wrap_process_io(&mut spool_file);
    let spooling = SpoolingReader::new(decoder, &mut spool);
    let (scan, mut spooling) =
        scan_minecraft_data_version_from_nbt_with_progress(spooling, progress)
            .with_context(|| format!("failed to validate NBT in {}", path.display()))?;
    let mut decompressed_tail = [0_u8; 1];
    if spooling
        .read(&mut decompressed_tail)
        .with_context(|| format!("failed to finish gzip member in {}", path.display()))?
        != 0
    {
        bail!(
            "{} has decompressed bytes after the root compound",
            path.display()
        );
    }
    let decompressed_bytes = spooling.spooled_bytes();
    let decoder = spooling.into_inner();
    spool
        .flush()
        .with_context(|| format!("failed to flush spool for {}", path.display()))?;
    drop(spool);
    let mut compressed = decoder.into_inner();
    if !compressed
        .fill_buf()
        .with_context(|| format!("failed to inspect gzip tail in {}", path.display()))?
        .is_empty()
    {
        bail!(
            "{} has a second gzip member or trailing compressed bytes",
            path.display()
        );
    }
    // `fill_buf` above establishes EOF after the one accepted gzip member.
    // Consequently every compressed source byte has passed through this
    // reader before its digest is finalized, including the gzip footer.
    let (_, compressed_fingerprint) = compressed.into_inner().finish()?;
    let spool_len = spool_file
        .metadata()
        .with_context(|| format!("failed to inspect spool for {}", path.display()))?
        .len();
    if spool_len != decompressed_bytes {
        bail!(
            "spooled litematic length {spool_len} does not match decompressed length {decompressed_bytes}"
        );
    }
    if spool_len == 0 {
        bail!("{} has an empty gzip payload", path.display());
    }

    let spool = Arc::new(BlockStatesSpool {
        file: spool_file,
        len: spool_len,
        id: NEXT_BLOCK_STATES_SPOOL_ID.fetch_add(1, Ordering::Relaxed),
    });
    // The file is immutable after this point and outlives the read-only map.
    let mmap = unsafe { MmapOptions::new().map(&spool.file) }
        .with_context(|| format!("failed to mmap spool for {}", path.display()))?;

    if scan.decompressed_bytes() != spool_len {
        bail!(
            "validated NBT length {} does not match spool length {spool_len}",
            scan.decompressed_bytes()
        );
    }

    let parse_context = FileBackedParseContext {
        mmap_base: mmap.as_ptr() as usize,
        mmap_len: mmap.len(),
        spool: Arc::clone(&spool),
    };
    let nested = FILE_BACKED_PARSE_CONTEXT.with(|slot| slot.borrow().is_some());
    if nested {
        bail!("nested file-backed litematic parse is not supported");
    }
    FILE_BACKED_PARSE_CONTEXT.with(|slot| {
        slot.replace(Some(parse_context));
    });
    let guard = FileBackedParseContextGuard;
    let root: LitematicRoot = fastnbt::from_bytes(&mmap)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    drop(guard);
    for (region_name, region) in &root.regions {
        if !region.block_states.is_empty() && !region.block_states.is_file_backed() {
            bail!(
                "region {region_name:?} retained a non-empty in-memory BlockStates array in file-backed mode"
            );
        }
    }
    Ok((root, compressed_fingerprint))
}

pub fn save_litematic_root(path: &Path, root: &LitematicRoot) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let encoder = GzEncoder::new(file, Compression::default());
    to_writer(encoder, root).with_context(|| format!("failed to write {}", path.display()))
}

pub fn region_volume(size: &Vec3i) -> Result<usize> {
    let width = i64::from(size.x).unsigned_abs() as usize;
    let height = i64::from(size.y).unsigned_abs() as usize;
    let length = i64::from(size.z).unsigned_abs() as usize;
    width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(length))
        .ok_or_else(|| anyhow!("region volume overflow"))
}

pub fn bits_for_palette(palette_len: usize) -> usize {
    if palette_len <= 1 {
        return 2;
    }
    let bits = usize::BITS as usize - (palette_len - 1).leading_zeros() as usize;
    bits.max(2)
}

#[derive(Debug, Clone, Copy)]
enum PalettePacking {
    CompactBitstream,
    PaddedLongs,
}

#[derive(Debug, Clone, Copy)]
struct PaletteLayout {
    packing: PalettePacking,
    nbits: usize,
}

fn compact_block_states_len(size: usize, nbits: usize) -> usize {
    (size * nbits).div_ceil(64)
}

fn padded_block_states_len(size: usize, nbits: usize) -> usize {
    let values_per_long = (64 / nbits).max(1);
    size.div_ceil(values_per_long)
}

fn palette_mask(nbits: usize) -> u64 {
    if nbits >= 64 {
        u64::MAX
    } else {
        (1_u64 << nbits) - 1
    }
}

fn palette_index_at_with_packing(
    block_states: &BigEndianLongArray,
    nbits: usize,
    index: usize,
    packing: PalettePacking,
) -> usize {
    let mask = palette_mask(nbits);
    match packing {
        PalettePacking::CompactBitstream => {
            let start_offset = index * nbits;
            let start_arr_index = start_offset >> 6;
            let end_arr_index = (((index + 1) * nbits) - 1) >> 6;
            let start_bit_offset = start_offset & 0x3f;

            if start_arr_index == end_arr_index {
                ((block_states.u64_at(start_arr_index) >> start_bit_offset) & mask) as usize
            } else {
                let end_offset = 64 - start_bit_offset;
                (((block_states.u64_at(start_arr_index) >> start_bit_offset)
                    | (block_states.u64_at(end_arr_index) << end_offset))
                    & mask) as usize
            }
        }
        PalettePacking::PaddedLongs => {
            let values_per_long = (64 / nbits).max(1);
            let arr_index = index / values_per_long;
            let bit_offset = (index % values_per_long) * nbits;
            ((block_states.u64_at(arr_index) >> bit_offset) & mask) as usize
        }
    }
}

fn choose_palette_packing(
    block_states: &BigEndianLongArray,
    size: usize,
    nbits: usize,
    palette_len: usize,
) -> Result<PaletteLayout> {
    let compact_len = compact_block_states_len(size, nbits);
    let padded_len = padded_block_states_len(size, nbits);
    let actual_len = block_states.len();

    let compact_possible = actual_len >= compact_len;
    let padded_possible = actual_len >= padded_len;
    if !compact_possible && !padded_possible {
        if let Some(layout) = choose_legacy_underpacked_palette_layout(
            block_states,
            size,
            nbits,
            palette_len,
            actual_len,
        ) {
            return Ok(layout);
        }

        bail!(
            "block state long array length mismatch, expected at least compact {} or padded {}, got {}",
            compact_len,
            padded_len,
            actual_len
        );
    }

    let layout_is_valid = |packing: PalettePacking, layout_bits: usize| {
        (0..size).all(|index| {
            palette_index_at_with_packing(block_states, layout_bits, index, packing)
                < palette_len.max(1)
        })
    };

    match (compact_possible, padded_possible) {
        (true, false) => Ok(PaletteLayout {
            packing: PalettePacking::CompactBitstream,
            nbits,
        }),
        (false, true) => Ok(PaletteLayout {
            packing: PalettePacking::PaddedLongs,
            nbits,
        }),
        (true, true) => {
            let compact_valid = layout_is_valid(PalettePacking::CompactBitstream, nbits);
            let padded_valid = layout_is_valid(PalettePacking::PaddedLongs, nbits);
            match (compact_valid, padded_valid) {
                (true, _) => Ok(PaletteLayout {
                    packing: PalettePacking::CompactBitstream,
                    nbits,
                }),
                (false, true) => Ok(PaletteLayout {
                    packing: PalettePacking::PaddedLongs,
                    nbits,
                }),
                (false, false) => Ok(PaletteLayout {
                    packing: PalettePacking::CompactBitstream,
                    nbits,
                }),
            }
        }
        (false, false) => unreachable!("invalid palette packing length was rejected above"),
    }
}

fn choose_legacy_underpacked_palette_layout(
    block_states: &BigEndianLongArray,
    size: usize,
    expected_nbits: usize,
    palette_len: usize,
    actual_len: usize,
) -> Option<PaletteLayout> {
    if expected_nbits <= 2 {
        return None;
    }

    for candidate_bits in (2..expected_nbits).rev() {
        let compact_exact = actual_len == compact_block_states_len(size, candidate_bits);
        let padded_exact = actual_len == padded_block_states_len(size, candidate_bits);

        for packing in [
            PalettePacking::CompactBitstream,
            PalettePacking::PaddedLongs,
        ] {
            let matches_len = match packing {
                PalettePacking::CompactBitstream => compact_exact,
                PalettePacking::PaddedLongs => padded_exact,
            };
            if !matches_len {
                continue;
            }

            let layout_is_valid = (0..size).all(|index| {
                palette_index_at_with_packing(block_states, candidate_bits, index, packing)
                    < palette_len.max(1)
            });
            if layout_is_valid {
                return Some(PaletteLayout {
                    packing,
                    nbits: candidate_bits,
                });
            }
        }
    }

    None
}

pub fn region_bounds(region: &RegionNbt) -> RegionBounds {
    let min_x = std::cmp::min(region.position.x, region.position.x + region.size.x + 1);
    let max_x = std::cmp::max(region.position.x, region.position.x + region.size.x - 1);
    let min_y = std::cmp::min(region.position.y, region.position.y + region.size.y + 1);
    let max_y = std::cmp::max(region.position.y, region.position.y + region.size.y - 1);
    let min_z = std::cmp::min(region.position.z, region.position.z + region.size.z + 1);
    let max_z = std::cmp::max(region.position.z, region.position.z + region.size.z - 1);
    RegionBounds {
        min_x,
        max_x,
        min_y,
        max_y,
        min_z,
        max_z,
    }
}

pub fn decode_palette_frequencies(
    block_states: &BigEndianLongArray,
    size: usize,
    nbits: usize,
    palette_len: usize,
) -> Result<Vec<u64>> {
    let layout = choose_palette_packing(block_states, size, nbits, palette_len)?;
    let mut counts = vec![0_u64; palette_len.max(1)];

    for index in 0..size {
        let value =
            palette_index_at_with_packing(block_states, layout.nbits, index, layout.packing);
        if let Some(slot) = counts.get_mut(value) {
            *slot += 1;
        }
    }

    Ok(counts)
}

pub fn pack_palette_indices(indices: &[usize], nbits: usize) -> BigEndianLongArray {
    let packed_bits = indices
        .len()
        .checked_mul(nbits)
        .expect("packed palette bit length overflow");
    let expected_len = packed_bits.div_ceil(64);
    let mask = if nbits >= 64 {
        u64::MAX
    } else {
        (1_u64 << nbits) - 1
    };
    let byte_len = expected_len
        .checked_mul(size_of::<u64>())
        .expect("packed palette byte length overflow");
    let mut packed = vec![0_u8; byte_len];

    fn or_word(bytes: &mut [u8], index: usize, value: u64) {
        let start = index * size_of::<u64>();
        let end = start + size_of::<u64>();
        let current = u64::from_be_bytes(bytes[start..end].try_into().expect("word-sized slice"));
        bytes[start..end].copy_from_slice(&(current | value).to_be_bytes());
    }

    for (index, palette_index) in indices.iter().copied().enumerate() {
        let value = (palette_index as u64) & mask;
        let start_offset = index * nbits;
        let start_arr_index = start_offset >> 6;
        let end_arr_index = (((index + 1) * nbits) - 1) >> 6;
        let start_bit_offset = start_offset & 0x3f;

        or_word(&mut packed, start_arr_index, value << start_bit_offset);
        if start_arr_index != end_arr_index {
            let end_offset = 64 - start_bit_offset;
            or_word(&mut packed, end_arr_index, value >> end_offset);
        }
    }

    BigEndianLongArray::from_big_endian_bytes(packed).expect("packed long array is word-aligned")
}

pub fn for_each_palette_index<F>(
    block_states: &BigEndianLongArray,
    size: usize,
    nbits: usize,
    palette_len: usize,
    mut callback: F,
) -> Result<()>
where
    F: FnMut(usize, usize) -> Result<()>,
{
    let layout = choose_palette_packing(block_states, size, nbits, palette_len)?;

    for index in 0..size {
        let value =
            palette_index_at_with_packing(block_states, layout.nbits, index, layout.packing);
        callback(index, value)?;
    }

    Ok(())
}

pub fn palette_index_at(
    block_states: &BigEndianLongArray,
    size: usize,
    nbits: usize,
    palette_len: usize,
    index: usize,
) -> Result<usize> {
    if index >= size {
        bail!(
            "palette index request out of range: index {} size {}",
            index,
            size
        );
    }

    let layout = choose_palette_packing(block_states, size, nbits, palette_len)?;
    Ok(palette_index_at_with_packing(
        block_states,
        layout.nbits,
        index,
        layout.packing,
    ))
}

pub fn storage_to_region_coords(
    store_x: usize,
    store_y: usize,
    store_z: usize,
    size: &Vec3i,
) -> (i32, i32, i32) {
    let mut x = store_x as i32;
    let mut y = store_y as i32;
    let mut z = store_z as i32;
    if size.x < 0 {
        x += size.x + 1;
    }
    if size.y < 0 {
        y += size.y + 1;
    }
    if size.z < 0 {
        z += size.z + 1;
    }
    (x, y, z)
}

pub fn region_coord_to_storage_coord(region_coord: i32, axis_size: i32) -> Option<usize> {
    if axis_size >= 0 {
        if (0..axis_size).contains(&region_coord) {
            Some(region_coord as usize)
        } else {
            None
        }
    } else {
        let start = axis_size + 1;
        if (start..=0).contains(&region_coord) {
            Some((region_coord - start) as usize)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Cursor, Seek, SeekFrom, Write};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    fn fixture_region(values: Vec<i64>) -> RegionNbt {
        RegionNbt {
            position: Vec3i { x: 0, y: 0, z: 0 },
            size: Vec3i {
                x: values.len().max(1) as i32,
                y: 1,
                z: 1,
            },
            block_state_palette: vec![BlockStateNbt {
                name: "minecraft:stone".to_owned(),
                properties: BTreeMap::new(),
            }],
            block_states: BigEndianLongArray::new(values),
            entities: Vec::new(),
            tile_entities: Vec::new(),
        }
    }

    fn fixture_root(regions: BTreeMap<String, RegionNbt>) -> LitematicRoot {
        LitematicRoot {
            metadata: MetadataNbt {
                author: Some("BlockStates spool test".to_owned()),
                description: Some("multi-region exact round trip".to_owned()),
                name: Some("File-backed fixture".to_owned()),
                time_created: Some(1),
                time_modified: Some(2),
                total_blocks: None,
                total_volume: None,
                region_count: Some(regions.len() as i32),
                enclosing_size: None,
            },
            regions,
            version: 6,
            sub_version: Some(1),
            minecraft_data_version: Some(4_903),
        }
    }

    fn decompress_file(path: &Path) -> Vec<u8> {
        let mut bytes = Vec::new();
        GzDecoder::new(File::open(path).unwrap())
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }

    fn write_gzip_file(path: &Path, bytes: &[u8]) {
        let mut encoder = GzEncoder::new(File::create(path).unwrap(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap();
    }

    fn directory_is_empty(path: &Path) -> bool {
        fs::read_dir(path).unwrap().next().is_none()
    }

    #[test]
    fn big_endian_long_array_serialization_matches_fastnbt_exactly() {
        #[derive(Serialize)]
        struct Fixture<T> {
            #[serde(rename = "BlockStates")]
            block_states: T,
        }

        let values = vec![0, 1, -1, i64::MIN, i64::MAX, 0x0102_0304_0506_0708];
        let expected = fastnbt::to_bytes(&Fixture {
            block_states: fastnbt::LongArray::new(values.clone()),
        })
        .unwrap();
        let actual = fastnbt::to_bytes(&Fixture {
            block_states: BigEndianLongArray::new(values),
        })
        .unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn block_states_page_cache_retains_strided_section_and_halo_pages() {
        const LAYERS: usize = 18;
        const PAGE_STRIDE: usize = 22;
        const ROWS_PER_LAYER: usize = 16;

        let page_numbers = (0..LAYERS)
            .map(|layer| 3 + layer * PAGE_STRIDE)
            .collect::<Vec<_>>();
        let spool_len = (page_numbers[LAYERS - 1] + 1) * BLOCK_STATES_PAGE_BYTES;
        let mut file = tempfile::tempfile().unwrap();
        file.set_len(spool_len as u64).unwrap();
        for (layer, page_number) in page_numbers.iter().copied().enumerate() {
            let page_start = page_number * BLOCK_STATES_PAGE_BYTES;
            for row in 0..ROWS_PER_LAYER {
                file.seek(SeekFrom::Start((page_start + row * 8) as u64))
                    .unwrap();
                file.write_all(&[(layer * ROWS_PER_LAYER + row) as u8])
                    .unwrap();
            }
        }
        file.flush().unwrap();
        let spool = BlockStatesSpool {
            file,
            len: spool_len as u64,
            id: NEXT_BLOCK_STATES_SPOOL_ID.fetch_add(1, Ordering::Relaxed),
        };

        BLOCK_STATES_PAGE_CACHE.with(|slot| slot.replace(BlockStatesPageCache::default()));
        for _ in 0..2 {
            for row in 0..ROWS_PER_LAYER {
                for (layer, page_number) in page_numbers.iter().copied().enumerate() {
                    let offset = page_number * BLOCK_STATES_PAGE_BYTES + row * 8;
                    let mut actual = [0_u8; 1];
                    read_block_states_cached(&spool, offset as u64, &mut actual).unwrap();
                    assert_eq!(actual[0], (layer * ROWS_PER_LAYER + row) as u8);
                }
            }
        }
        BLOCK_STATES_PAGE_CACHE.with(|slot| {
            let cache = slot.borrow();
            let fill_count = cache.hot.fill_count
                + cache
                    .retained
                    .iter()
                    .map(|entry| entry.fill_count)
                    .sum::<usize>();
            assert_eq!(fill_count, LAYERS, "every strided page should fill once");
            assert_eq!(
                std::iter::once(&cache.hot)
                    .chain(cache.retained.iter())
                    .filter(|entry| entry.spool_id == spool.id)
                    .count(),
                LAYERS
            );
        });
        BLOCK_STATES_PAGE_CACHE.with(|slot| slot.replace(BlockStatesPageCache::default()));
    }

    #[test]
    fn block_states_page_cache_evicts_at_capacity_and_isolates_spool_identities() {
        fn publish(cache: &mut BlockStatesPageCache, spool_id: u64, page_number: u64) {
            let page_start = page_number * BLOCK_STATES_PAGE_BYTES as u64;
            let entry = cache.select(spool_id, page_start);
            assert!(!entry.matches(spool_id, page_start));
            entry.spool_id = spool_id;
            entry.page_start = page_start;
            entry.valid = BLOCK_STATES_PAGE_BYTES;
        }

        fn contains(cache: &BlockStatesPageCache, spool_id: u64, page_number: u64) -> bool {
            let page_start = page_number * BLOCK_STATES_PAGE_BYTES as u64;
            std::iter::once(&cache.hot)
                .chain(cache.retained.iter())
                .any(|entry| entry.matches(spool_id, page_start))
        }

        let mut cache = BlockStatesPageCache::default();
        publish(&mut cache, 1, 7);
        let page_start = 7 * BLOCK_STATES_PAGE_BYTES as u64;
        assert!(cache.select(1, page_start).matches(1, page_start));
        publish(&mut cache, 2, 7);
        assert!(contains(&cache, 1, 7));
        assert!(contains(&cache, 2, 7));

        let mut cache = BlockStatesPageCache::default();
        for page_number in 0..BLOCK_STATES_PAGE_CACHE_PAGES as u64 {
            publish(&mut cache, 3, page_number);
        }
        assert!(
            (0..BLOCK_STATES_PAGE_CACHE_PAGES as u64).all(|page_number| contains(
                &cache,
                3,
                page_number
            ))
        );
        publish(&mut cache, 3, BLOCK_STATES_PAGE_CACHE_PAGES as u64);
        assert!(!contains(&cache, 3, 0));
        assert!(
            (1..=BLOCK_STATES_PAGE_CACHE_PAGES as u64).all(|page_number| contains(
                &cache,
                3,
                page_number
            ))
        );
    }

    #[test]
    fn big_endian_long_array_deserializes_and_decodes_by_index() {
        #[derive(Serialize)]
        struct Source {
            values: fastnbt::LongArray,
        }

        #[derive(Deserialize)]
        struct Destination {
            values: BigEndianLongArray,
        }

        let values = vec![0, 1, -1, i64::MIN, i64::MAX, 0x0102_0304_0506_0708];
        let encoded = fastnbt::to_bytes(&Source {
            values: fastnbt::LongArray::new(values.clone()),
        })
        .unwrap();
        let decoded: Destination = fastnbt::from_bytes(&encoded).unwrap();

        assert_eq!(decoded.values.len(), values.len());
        assert!(!decoded.values.is_empty());
        for (index, expected) in values.into_iter().enumerate() {
            assert_eq!(decoded.values.i64_at(index), expected);
            assert_eq!(decoded.values.u64_at(index), expected as u64);
        }
    }

    #[test]
    fn big_endian_long_array_rejects_non_multiple_of_eight_byte_length() {
        let error = serde_json::from_str::<BigEndianLongArray>(
            r#"{"__fastnbt_long_array":[0,1,2,3,4,5,6]}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("multiple of 8"));
    }

    #[test]
    fn big_endian_long_array_rejects_extra_wrapper_fields() {
        let error = serde_json::from_str::<BigEndianLongArray>(
            r#"{"__fastnbt_long_array":[0,0,0,0,0,0,0,0],"unexpected":true}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("multiple fields"));
    }

    #[test]
    fn direct_big_endian_palette_packing_matches_reference_words_exactly() {
        #[derive(Serialize)]
        struct Fixture<T> {
            #[serde(rename = "BlockStates")]
            block_states: T,
        }

        fn reference_words(indices: &[usize], nbits: usize) -> Vec<i64> {
            let mask = if nbits == 64 {
                u64::MAX
            } else {
                (1_u64 << nbits) - 1
            };
            let mut words = vec![0_u64; (indices.len() * nbits).div_ceil(64)];
            for (index, value) in indices.iter().copied().enumerate() {
                let value = (value as u64) & mask;
                let start_offset = index * nbits;
                let start_word = start_offset >> 6;
                let end_word = (((index + 1) * nbits) - 1) >> 6;
                let bit_offset = start_offset & 0x3f;
                words[start_word] |= value << bit_offset;
                if start_word != end_word {
                    words[end_word] |= value >> (64 - bit_offset);
                }
            }
            words.into_iter().map(|word| word as i64).collect()
        }

        for nbits in 2..=64 {
            let mask = if nbits == 64 {
                u64::MAX
            } else {
                (1_u64 << nbits) - 1
            };
            let indices = (0..1_003)
                .map(|index| {
                    let mixed = (index as u64)
                        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                        .rotate_left((index % 64) as u32)
                        | (1_u64 << (nbits - 1));
                    (mixed & mask) as usize
                })
                .collect::<Vec<_>>();
            let expected = fastnbt::to_bytes(&Fixture {
                block_states: fastnbt::LongArray::new(reference_words(&indices, nbits)),
            })
            .unwrap();
            let actual = fastnbt::to_bytes(&Fixture {
                block_states: pack_palette_indices(&indices, nbits),
            })
            .unwrap();
            assert_eq!(actual, expected, "{nbits}-bit packing differs");
        }

        let expected_empty = fastnbt::to_bytes(&Fixture {
            block_states: fastnbt::LongArray::new(Vec::new()),
        })
        .unwrap();
        let actual_empty = fastnbt::to_bytes(&Fixture {
            block_states: pack_palette_indices(&[], 64),
        })
        .unwrap();
        assert_eq!(actual_empty, expected_empty);
    }

    #[test]
    fn file_backed_loader_round_trips_multiple_regions_across_pages_and_cleans_spool() {
        #[derive(Serialize)]
        struct Fixture<T> {
            #[serde(rename = "BlockStates")]
            block_states: T,
        }

        let temporary = tempfile::tempdir().unwrap();
        let spool_directory = temporary.path().join("spool");
        fs::create_dir(&spool_directory).unwrap();
        let input = temporary.path().join("input.litematic");
        let output = temporary.path().join("output.litematic");
        let large_values = (0..(BLOCK_STATES_PAGE_BYTES / 8 * 3 + 37))
            .map(|index| match index % 5 {
                0 => index as i64,
                1 => -(index as i64),
                2 => i64::MIN + index as i64,
                3 => i64::MAX - index as i64,
                _ => 0x0102_0304_0506_0708_i64 ^ index as i64,
            })
            .collect::<Vec<_>>();
        let negative_values = vec![0, -1, i64::MIN, i64::MAX, -9_876_543_210];
        let root = fixture_root(BTreeMap::from([
            ("large".to_owned(), fixture_region(large_values.clone())),
            (
                "negative".to_owned(),
                fixture_region(negative_values.clone()),
            ),
            ("empty".to_owned(), fixture_region(Vec::new())),
        ]));
        save_litematic_root(&input, &root).unwrap();
        let expected_nbt = decompress_file(&input);
        let expected_compressed = fs::read(&input).unwrap();
        let expected_compressed_sha256 = <[u8; 32]>::from(Sha256::digest(&expected_compressed));

        let (loaded, compressed_fingerprint) =
            load_litematic_root_file_backed_in_with_progress_and_fingerprint(
                &input,
                Some(&spool_directory),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(
            compressed_fingerprint.byte_count(),
            expected_compressed.len() as u64
        );
        assert_eq!(compressed_fingerprint.sha256(), expected_compressed_sha256);
        let large = &loaded.regions["large"].block_states;
        let BigEndianLongArrayStorage::File(range) = &large.storage else {
            panic!("non-empty BlockStates must be file-backed");
        };
        let first_page_end =
            (range.offset / BLOCK_STATES_PAGE_BYTES as u64 + 1) * BLOCK_STATES_PAGE_BYTES as u64;
        let page_transition = ((first_page_end - range.offset) as usize).div_ceil(8);
        for index in [
            0,
            page_transition.saturating_sub(1),
            page_transition,
            page_transition + 1,
            large_values.len() - 1,
        ] {
            assert_eq!(large.i64_at(index), large_values[index]);
        }
        for (index, expected) in negative_values.iter().copied().enumerate() {
            assert_eq!(
                loaded.regions["negative"].block_states.i64_at(index),
                expected
            );
        }
        assert!(loaded.regions["empty"].block_states.is_empty());
        assert!(!loaded.regions["empty"].block_states.is_file_backed());

        save_litematic_root(&output, &loaded).unwrap();
        assert_eq!(decompress_file(&output), expected_nbt);
        let cloned = loaded.regions["large"].block_states.clone();
        drop(loaded);
        assert_eq!(
            cloned.i64_at(page_transition),
            large_values[page_transition]
        );
        let expected_array = fastnbt::to_bytes(&Fixture {
            block_states: fastnbt::LongArray::new(large_values.clone()),
        })
        .unwrap();
        let cloned_array = fastnbt::to_bytes(&Fixture {
            block_states: cloned.clone(),
        })
        .unwrap();
        assert_eq!(cloned_array, expected_array);
        drop(cloned);
        assert!(
            directory_is_empty(&spool_directory),
            "unnamed spool must be deleted after its final array clone is dropped"
        );

        let ordinary = load_litematic_root(&input).unwrap();
        assert!(!ordinary.regions["large"].block_states.is_file_backed());
        assert_eq!(
            ordinary.regions["large"].block_states,
            root.regions["large"].block_states
        );
    }

    #[test]
    fn file_backed_loader_rejects_corrupt_or_ambiguous_streams_without_spool_leaks() {
        let temporary = tempfile::tempdir().unwrap();
        let spool_directory = temporary.path().join("spool");
        fs::create_dir(&spool_directory).unwrap();
        let valid = temporary.path().join("valid.litematic");
        save_litematic_root(
            &valid,
            &fixture_root(BTreeMap::from([(
                "region".to_owned(),
                fixture_region(vec![1, -2, 3]),
            )])),
        )
        .unwrap();
        let valid_compressed = fs::read(&valid).unwrap();

        let compressed_tail = temporary.path().join("compressed-tail.litematic");
        let mut bytes = valid_compressed.clone();
        bytes.push(1);
        fs::write(&compressed_tail, bytes).unwrap();
        assert!(
            load_litematic_root_file_backed_in(&compressed_tail, Some(&spool_directory)).is_err()
        );
        assert!(directory_is_empty(&spool_directory));

        let second_member = temporary.path().join("second-member.litematic");
        let mut bytes = valid_compressed.clone();
        bytes.extend_from_slice(&valid_compressed);
        fs::write(&second_member, bytes).unwrap();
        assert!(
            load_litematic_root_file_backed_in(&second_member, Some(&spool_directory)).is_err()
        );
        assert!(directory_is_empty(&spool_directory));

        let corrupt_crc = temporary.path().join("corrupt-crc.litematic");
        let mut bytes = valid_compressed.clone();
        let crc_offset = bytes.len() - 8;
        bytes[crc_offset] ^= 1;
        fs::write(&corrupt_crc, bytes).unwrap();
        assert!(load_litematic_root_file_backed_in(&corrupt_crc, Some(&spool_directory)).is_err());
        assert!(directory_is_empty(&spool_directory));

        let truncated = temporary.path().join("truncated.litematic");
        let mut bytes = valid_compressed;
        bytes.truncate(bytes.len() - 4);
        fs::write(&truncated, bytes).unwrap();
        assert!(load_litematic_root_file_backed_in(&truncated, Some(&spool_directory)).is_err());
        assert!(directory_is_empty(&spool_directory));

        let nbt_tail = temporary.path().join("nbt-tail.litematic");
        let mut raw = decompress_file(&valid);
        raw.push(1);
        write_gzip_file(&nbt_tail, &raw);
        let error = load_litematic_root_file_backed_in(&nbt_tail, Some(&spool_directory))
            .unwrap_err()
            .to_string();
        assert!(error.contains("after the root compound"), "{error}");
        assert!(directory_is_empty(&spool_directory));
    }

    #[test]
    fn file_backed_long_array_fails_closed_if_private_spool_is_truncated() {
        let temporary = tempfile::tempdir().unwrap();
        let spool_directory = temporary.path().join("spool");
        fs::create_dir(&spool_directory).unwrap();
        let input = temporary.path().join("input.litematic");
        let values = (0..(BLOCK_STATES_PAGE_BYTES / 8 + 17))
            .map(|index| index as i64)
            .collect::<Vec<_>>();
        save_litematic_root(
            &input,
            &fixture_root(BTreeMap::from([(
                "region".to_owned(),
                fixture_region(values.clone()),
            )])),
        )
        .unwrap();
        let loaded = load_litematic_root_file_backed_in(&input, Some(&spool_directory)).unwrap();
        let array = &loaded.regions["region"].block_states;
        let BigEndianLongArrayStorage::File(range) = &array.storage else {
            panic!("BlockStates must be file-backed");
        };
        range
            .spool
            .file
            .set_len(range.offset + range.len as u64 - 1)
            .unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| array.i64_at(values.len() - 1)));
        assert!(
            result.is_err(),
            "truncated private spool must not return data"
        );
        drop(loaded);
        assert!(directory_is_empty(&spool_directory));
    }

    #[test]
    fn file_backed_parse_rejects_borrowed_ranges_outside_the_active_mmap() {
        let mapped = vec![0_u8; 16];
        let outside = vec![0_u8; 8];
        let file = tempfile::tempfile().unwrap();
        file.set_len(mapped.len() as u64).unwrap();
        let spool = Arc::new(BlockStatesSpool {
            file,
            len: mapped.len() as u64,
            id: NEXT_BLOCK_STATES_SPOOL_ID.fetch_add(1, Ordering::Relaxed),
        });
        FILE_BACKED_PARSE_CONTEXT.with(|slot| {
            slot.replace(Some(FileBackedParseContext {
                mmap_base: mapped.as_ptr() as usize,
                mmap_len: mapped.len(),
                spool,
            }));
        });
        let guard = FileBackedParseContextGuard;
        let error = BigEndianLongArray::from_borrowed_big_endian_bytes(&outside)
            .unwrap_err()
            .to_owned();
        drop(guard);
        assert!(error.contains("outside the parse mmap"));
    }

    struct CappedReader<R> {
        inner: R,
        maximum_request: usize,
        observed_request: usize,
    }

    impl<R> CappedReader<R> {
        fn new(inner: R, maximum_request: usize) -> Self {
            Self {
                inner,
                maximum_request,
                observed_request: 0,
            }
        }
    }

    impl<R> Read for CappedReader<R>
    where
        R: Read,
    {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            assert!(
                bytes.len() <= self.maximum_request,
                "scanner requested {} bytes from a reader capped at {}",
                bytes.len(),
                self.maximum_request
            );
            self.observed_request = self.observed_request.max(bytes.len());
            self.inner.read(bytes)
        }
    }

    fn push_name(bytes: &mut Vec<u8>, name: &[u8]) {
        bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(name);
    }

    fn push_named_header(bytes: &mut Vec<u8>, tag: u8, name: &[u8]) {
        bytes.push(tag);
        push_name(bytes, name);
    }

    fn push_data_version(bytes: &mut Vec<u8>, version: i32) {
        push_named_header(bytes, 3, MINECRAFT_DATA_VERSION_NAME);
        bytes.extend_from_slice(&version.to_be_bytes());
    }

    fn push_preview_image(bytes: &mut Vec<u8>, pixels: &[u32]) {
        push_named_header(bytes, 11, PREVIEW_IMAGE_DATA_NAME);
        bytes.extend_from_slice(&(pixels.len() as i32).to_be_bytes());
        for pixel in pixels {
            bytes.extend_from_slice(&pixel.to_be_bytes());
        }
    }

    fn push_named_int(bytes: &mut Vec<u8>, name: &[u8], value: i32) {
        push_named_header(bytes, 3, name);
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn push_preflight_metadata(
        bytes: &mut Vec<u8>,
        total_blocks: Option<i32>,
        total_volume: Option<i32>,
        region_count: Option<i32>,
        enclosing_size: Option<[i32; 3]>,
    ) {
        push_named_header(bytes, 10, PREFLIGHT_METADATA_NAME);
        if let Some(value) = total_blocks {
            push_named_int(bytes, PREFLIGHT_TOTAL_BLOCKS_NAME, value);
        }
        if let Some(value) = total_volume {
            push_named_int(bytes, PREFLIGHT_TOTAL_VOLUME_NAME, value);
        }
        if let Some(value) = region_count {
            push_named_int(bytes, PREFLIGHT_REGION_COUNT_NAME, value);
        }
        if let Some([x, y, z]) = enclosing_size {
            push_named_header(bytes, 10, PREFLIGHT_ENCLOSING_SIZE_NAME);
            push_named_int(bytes, b"X", x);
            push_named_int(bytes, b"Y", y);
            push_named_int(bytes, b"Z", z);
            bytes.push(0);
        }
        bytes.push(0);
    }

    fn root_with_children(children: &[u8]) -> Vec<u8> {
        let mut bytes = vec![10, 0, 0];
        bytes.extend_from_slice(children);
        bytes.push(0);
        bytes
    }

    fn scan_raw(bytes: Vec<u8>) -> Result<LitematicDataVersionScan> {
        let expected_length = bytes.len() as u64;
        let (scan, mut reader) = scan_minecraft_data_version_from_nbt(Cursor::new(bytes))?;
        let mut trailing = [0_u8; 1];
        assert_eq!(reader.read(&mut trailing).unwrap(), 0);
        assert_eq!(scan.decompressed_bytes(), expected_length);
        Ok(scan)
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn metadata_limits(max_decompressed_bytes: u64) -> LitematicMetadataScanLimits {
        LitematicMetadataScanLimits {
            max_decompressed_bytes,
            deadline: Duration::from_secs(1),
        }
    }

    #[test]
    fn preflight_metadata_scan_returns_before_large_scene_payload() {
        let mut root = vec![10, 0, 0];
        push_preflight_metadata(
            &mut root,
            Some(123_456),
            Some(987_654),
            Some(7),
            Some([320, 192, 448]),
        );
        // A deliberately truncated, very large Regions-like field follows the
        // complete Metadata compound. Success proves the cheap scan never
        // reads or validates scene-scale payload after its target.
        push_named_header(&mut root, 7, b"RegionsPayload");
        root.extend_from_slice(&i32::MAX.to_be_bytes());

        let metadata = scan_litematic_preflight_metadata_from_gzip(
            Cursor::new(gzip(&root)),
            metadata_limits(512),
        )
        .unwrap();

        assert_eq!(
            metadata.total_blocks,
            LitematicMetadataValue::Known(123_456)
        );
        assert_eq!(
            metadata.total_volume,
            LitematicMetadataValue::Known(987_654)
        );
        assert_eq!(metadata.region_count, LitematicMetadataValue::Known(7));
        assert_eq!(
            metadata.enclosing_size,
            LitematicMetadataValue::Known(LitematicMetadataEnclosingSize {
                x: 320,
                y: 192,
                z: 448,
            })
        );
        assert!(metadata.decompressed_bytes_read < 256);
    }

    #[test]
    fn preflight_metadata_accepts_lowercase_axes_after_regions() {
        let mut root = vec![10, 0, 0];
        push_named_header(&mut root, 7, b"RegionsPayload");
        root.extend_from_slice(&4_096_i32.to_be_bytes());
        root.extend(std::iter::repeat_n(0, 4_096));
        push_named_header(&mut root, 10, PREFLIGHT_METADATA_NAME);
        push_named_int(&mut root, PREFLIGHT_TOTAL_BLOCKS_NAME, 4_389);
        push_named_int(&mut root, PREFLIGHT_TOTAL_VOLUME_NAME, 30_132);
        push_named_header(&mut root, 10, PREFLIGHT_ENCLOSING_SIZE_NAME);
        push_named_int(&mut root, b"x", 17);
        push_named_int(&mut root, b"y", 18);
        push_named_int(&mut root, b"z", 19);
        root.extend([0, 0, 0]);

        let metadata =
            scan_litematic_preflight_metadata_from_nbt(Cursor::new(root), metadata_limits(8_192))
                .unwrap();
        assert_eq!(metadata.total_blocks, LitematicMetadataValue::Known(4_389));
        assert_eq!(
            metadata.enclosing_size,
            LitematicMetadataValue::Known(LitematicMetadataEnclosingSize {
                x: 17,
                y: 18,
                z: 19
            })
        );
    }

    #[test]
    fn preflight_metadata_scan_reports_unknown_when_metadata_is_behind_byte_cap() {
        let mut root = vec![10, 0, 0];
        push_named_header(&mut root, 7, b"RegionsPayload");
        root.extend_from_slice(&4_096_i32.to_be_bytes());
        root.extend(std::iter::repeat_n(0, 4_096));
        push_preflight_metadata(&mut root, Some(1), Some(1), Some(1), Some([1, 1, 1]));
        root.push(0);

        let metadata =
            scan_litematic_preflight_metadata_from_nbt(Cursor::new(root), metadata_limits(64))
                .unwrap();

        assert!(metadata.decompressed_bytes_read <= 64);
        assert!(metadata.decompressed_bytes_read < 4_096);
        for value in [
            metadata.total_blocks,
            metadata.total_volume,
            metadata.region_count,
        ] {
            assert_eq!(
                value,
                LitematicMetadataValue::Unknown(LitematicMetadataUnknown::ByteLimitExceeded)
            );
        }
        assert_eq!(
            metadata.enclosing_size,
            LitematicMetadataValue::Unknown(LitematicMetadataUnknown::ByteLimitExceeded)
        );
    }

    #[test]
    fn preflight_metadata_scan_distinguishes_missing_compound_and_missing_field() {
        let missing_metadata = scan_litematic_preflight_metadata_from_nbt(
            Cursor::new(root_with_children(&[])),
            metadata_limits(512),
        )
        .unwrap();
        assert_eq!(
            missing_metadata.total_blocks,
            LitematicMetadataValue::Unknown(LitematicMetadataUnknown::MetadataMissing)
        );
        assert_eq!(
            missing_metadata.enclosing_size,
            LitematicMetadataValue::Unknown(LitematicMetadataUnknown::MetadataMissing)
        );

        let mut children = Vec::new();
        push_preflight_metadata(&mut children, Some(4), Some(8), None, Some([2, 2, 2]));
        let incomplete = scan_litematic_preflight_metadata_from_nbt(
            Cursor::new(root_with_children(&children)),
            metadata_limits(512),
        )
        .unwrap();
        assert_eq!(incomplete.total_blocks, LitematicMetadataValue::Known(4));
        assert_eq!(incomplete.total_volume, LitematicMetadataValue::Known(8));
        assert_eq!(
            incomplete.region_count,
            LitematicMetadataValue::Unknown(LitematicMetadataUnknown::FieldMissing)
        );
        assert_eq!(
            incomplete.enclosing_size,
            LitematicMetadataValue::Known(LitematicMetadataEnclosingSize { x: 2, y: 2, z: 2 })
        );
    }

    #[test]
    fn preflight_metadata_scan_enforces_deadline_without_reading_scene_data() {
        let metadata = scan_litematic_preflight_metadata_from_nbt(
            Cursor::new(root_with_children(&[])),
            LitematicMetadataScanLimits {
                max_decompressed_bytes: 512,
                deadline: Duration::ZERO,
            },
        )
        .unwrap();
        assert_eq!(metadata.decompressed_bytes_read, 0);
        assert_eq!(
            metadata.total_blocks,
            LitematicMetadataValue::Unknown(LitematicMetadataUnknown::DeadlineExceeded)
        );
    }

    #[test]
    fn preflight_metadata_scan_rejects_invalid_target_fields() {
        let mut wrong_type = Vec::new();
        push_named_header(&mut wrong_type, 10, PREFLIGHT_METADATA_NAME);
        push_named_header(&mut wrong_type, 4, PREFLIGHT_TOTAL_BLOCKS_NAME);
        wrong_type.extend_from_slice(&1_i64.to_be_bytes());
        wrong_type.push(0);
        let error = scan_litematic_preflight_metadata_from_nbt(
            Cursor::new(root_with_children(&wrong_type)),
            metadata_limits(512),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("TotalBlocks"), "{error}");
        assert!(error.contains("expected TAG_Int"), "{error}");

        let mut negative = Vec::new();
        push_preflight_metadata(&mut negative, Some(-1), Some(1), Some(1), Some([1, 1, 1]));
        let error = scan_litematic_preflight_metadata_from_nbt(
            Cursor::new(root_with_children(&negative)),
            metadata_limits(512),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("negative value"), "{error}");

        let error = scan_litematic_preflight_metadata_from_nbt(
            Cursor::new([9, 0, 0]),
            metadata_limits(512),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("expected TAG_Compound"), "{error}");
    }

    #[test]
    fn data_version_scan_skips_all_nbt_payload_types() {
        let mut children = Vec::new();
        push_named_header(&mut children, 1, b"byte");
        children.push(7);
        push_named_header(&mut children, 2, b"short");
        children.extend_from_slice(&7_i16.to_be_bytes());
        push_named_header(&mut children, 3, b"int");
        children.extend_from_slice(&7_i32.to_be_bytes());
        push_named_header(&mut children, 4, b"long");
        children.extend_from_slice(&7_i64.to_be_bytes());
        push_named_header(&mut children, 5, b"float");
        children.extend_from_slice(&7_f32.to_bits().to_be_bytes());
        push_named_header(&mut children, 6, b"double");
        children.extend_from_slice(&7_f64.to_bits().to_be_bytes());
        push_named_header(&mut children, 7, b"bytes");
        children.extend_from_slice(&3_i32.to_be_bytes());
        children.extend_from_slice(&[1, 2, 3]);
        push_named_header(&mut children, 8, b"string");
        push_name(&mut children, b"abc");
        push_named_header(&mut children, 9, b"list");
        children.push(3);
        children.extend_from_slice(&2_i32.to_be_bytes());
        children.extend_from_slice(&11_i32.to_be_bytes());
        children.extend_from_slice(&12_i32.to_be_bytes());
        push_named_header(&mut children, 10, b"compound");
        push_named_header(&mut children, 1, b"nested");
        children.push(9);
        children.push(0);
        push_named_header(&mut children, 11, b"ints");
        children.extend_from_slice(&2_i32.to_be_bytes());
        children.extend_from_slice(&11_i32.to_be_bytes());
        children.extend_from_slice(&12_i32.to_be_bytes());
        push_named_header(&mut children, 12, b"longs");
        children.extend_from_slice(&2_i32.to_be_bytes());
        children.extend_from_slice(&11_i64.to_be_bytes());
        children.extend_from_slice(&12_i64.to_be_bytes());
        push_data_version(&mut children, 4_903);

        let scan = scan_raw(root_with_children(&children)).unwrap();

        assert_eq!(scan.minecraft_data_version(), Some(4_903));
        assert_eq!(scan.max_depth(), 2);
        assert_eq!(scan.discard_buffer_bytes(), NBT_STREAM_BUFFER_BYTES);
        assert!(scan.max_read_request_bytes() <= NBT_STREAM_BUFFER_BYTES);
    }

    #[test]
    fn preview_scan_streams_past_large_payload_and_converts_argb_to_rgba() {
        let long_count = 256 * 1_024_i32;
        let mut prefix = vec![10, 0, 0];
        push_named_header(&mut prefix, 12, b"large-unrelated-payload");
        prefix.extend_from_slice(&long_count.to_be_bytes());
        let mut suffix = Vec::new();
        push_preview_image(
            &mut suffix,
            &[0xff11_2233, 0x8044_5566, 0x0077_8899, 0xffaa_bbcc],
        );
        suffix.push(0);
        let unrelated_bytes = u64::try_from(long_count).unwrap() * 8;
        let reader = Cursor::new(prefix)
            .chain(std::io::repeat(0).take(unrelated_bytes))
            .chain(Cursor::new(suffix));
        let reader = CappedReader::new(reader, NBT_STREAM_BUFFER_BYTES);

        let (preview, mut reader) = scan_preview_image_from_nbt(reader).unwrap();
        let preview = preview.expect("preview image");
        assert_eq!((preview.width, preview.height), (2, 2));
        assert_eq!(
            preview.rgba,
            [
                0x11, 0x22, 0x33, 0xff, 0x44, 0x55, 0x66, 0x80, 0x77, 0x88, 0x99, 0x00, 0xaa, 0xbb,
                0xcc, 0xff,
            ]
        );
        let mut trailing = [0_u8; 1];
        assert_eq!(reader.read(&mut trailing).unwrap(), 0);
        assert!(reader.observed_request <= NBT_STREAM_BUFFER_BYTES);
    }

    #[test]
    fn preview_scan_is_fail_closed_for_ambiguous_or_invalid_payloads() {
        let mut duplicate = Vec::new();
        push_preview_image(&mut duplicate, &[0; 4]);
        push_preview_image(&mut duplicate, &[0; 4]);
        assert!(
            scan_preview_image_from_nbt(Cursor::new(root_with_children(&duplicate)))
                .unwrap_err()
                .to_string()
                .contains("duplicate PreviewImageData")
        );

        let mut wrong_type = Vec::new();
        push_named_header(&mut wrong_type, 7, PREVIEW_IMAGE_DATA_NAME);
        wrong_type.extend_from_slice(&0_i32.to_be_bytes());
        assert!(
            scan_preview_image_from_nbt(Cursor::new(root_with_children(&wrong_type)))
                .unwrap_err()
                .to_string()
                .contains("expected TAG_Int_Array")
        );

        let mut nonsquare = Vec::new();
        push_preview_image(&mut nonsquare, &[0; 3]);
        assert!(
            scan_preview_image_from_nbt(Cursor::new(root_with_children(&nonsquare)))
                .unwrap_err()
                .to_string()
                .contains("expected a square pixel array")
        );

        let mut oversized = vec![10, 0, 0];
        push_named_header(&mut oversized, 11, PREVIEW_IMAGE_DATA_NAME);
        oversized.extend_from_slice(&((MAX_PREVIEW_IMAGE_EDGE + 1).pow(2) as i32).to_be_bytes());
        assert!(
            scan_preview_image_from_nbt(Cursor::new(oversized))
                .unwrap_err()
                .to_string()
                .contains("exceeds safety limit")
        );
    }

    #[test]
    fn preview_gzip_scan_rejects_trailing_members_and_accepts_missing_preview() {
        let root = root_with_children(&[]);
        assert_eq!(
            scan_preview_image_from_gzip(Cursor::new(gzip(&root))).unwrap(),
            None
        );

        let mut two_members = gzip(&root);
        two_members.extend_from_slice(&gzip(&root));
        assert!(
            scan_preview_image_from_gzip(Cursor::new(two_members))
                .unwrap_err()
                .to_string()
                .contains("second member or trailing compressed bytes")
        );
    }

    #[test]
    fn data_version_scan_memory_requests_do_not_grow_with_long_array() {
        fn scan_long_array(long_count: i32) -> LitematicDataVersionScan {
            let mut prefix = vec![10, 0, 0];
            push_named_header(&mut prefix, 12, b"BlockStates");
            prefix.extend_from_slice(&long_count.to_be_bytes());
            let mut suffix = Vec::new();
            push_data_version(&mut suffix, 3_700);
            suffix.push(0);
            let payload_bytes = u64::try_from(long_count).unwrap() * 8;
            let expected_bytes = prefix.len() as u64 + payload_bytes + suffix.len() as u64;
            let reader = Cursor::new(prefix)
                .chain(std::io::repeat(0).take(payload_bytes))
                .chain(Cursor::new(suffix));
            let reader = CappedReader::new(reader, NBT_STREAM_BUFFER_BYTES);
            let (scan, mut reader) = scan_minecraft_data_version_from_nbt(reader).unwrap();
            let mut trailing = [0_u8; 1];
            assert_eq!(reader.read(&mut trailing).unwrap(), 0);
            assert!(reader.observed_request <= NBT_STREAM_BUFFER_BYTES);
            assert_eq!(scan.minecraft_data_version(), Some(3_700));
            assert_eq!(scan.decompressed_bytes(), expected_bytes);
            assert_eq!(scan.logical_values(), payload_bytes / 8 + 3);
            scan
        }

        let small = scan_long_array(8);
        let large = scan_long_array(8 * 1_024 * 1_024);

        assert_eq!(small.discard_buffer_bytes(), NBT_STREAM_BUFFER_BYTES);
        assert_eq!(large.discard_buffer_bytes(), NBT_STREAM_BUFFER_BYTES);
        assert_eq!(small.container_frame_bytes(), large.container_frame_bytes());
        assert!(small.container_frame_bytes() > 0);
        assert!(small.max_read_request_bytes() <= NBT_STREAM_BUFFER_BYTES);
        assert_eq!(large.max_read_request_bytes(), NBT_STREAM_BUFFER_BYTES);
        assert_eq!(small.max_depth(), large.max_depth());
    }

    #[test]
    fn data_version_scan_reports_forward_progress_for_large_payloads() {
        let payload_bytes = 3 * NBT_SCAN_PROGRESS_INTERVAL_BYTES;
        let mut prefix = vec![10, 0, 0];
        push_named_header(&mut prefix, 7, b"payload");
        prefix.extend_from_slice(&(payload_bytes as i32).to_be_bytes());
        let mut suffix = Vec::new();
        push_data_version(&mut suffix, 3_700);
        suffix.push(0);
        let reader = Cursor::new(prefix)
            .chain(std::io::repeat(0).take(payload_bytes))
            .chain(Cursor::new(suffix));
        let mut observed = Vec::new();

        let (scan, _) =
            scan_minecraft_data_version_from_nbt_with_progress(reader, &mut |progress| {
                observed.push(progress)
            })
            .unwrap();

        assert_eq!(observed.first().unwrap().decompressed_bytes(), 0);
        assert_eq!(
            observed.last().unwrap().decompressed_bytes(),
            scan.decompressed_bytes()
        );
        assert_eq!(
            observed.last().unwrap().logical_values(),
            scan.logical_values()
        );
        assert!(
            observed.len() >= 5,
            "expected start, byte pulses, and final state"
        );
        assert!(observed.windows(2).all(|window| {
            window[0].decompressed_bytes() <= window[1].decompressed_bytes()
                && window[0].logical_values() <= window[1].logical_values()
        }));
    }

    #[test]
    fn data_version_scan_rejects_negative_lengths_unknown_tags_and_truncation() {
        for tag in [7, 11, 12] {
            let mut child = Vec::new();
            push_named_header(&mut child, tag, b"negative");
            child.extend_from_slice(&(-1_i32).to_be_bytes());
            let error = scan_raw(root_with_children(&child))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("negative length"),
                "unexpected error: {error}"
            );
        }

        let mut negative_list = Vec::new();
        push_named_header(&mut negative_list, 9, b"negative-list");
        negative_list.push(1);
        negative_list.extend_from_slice(&(-1_i32).to_be_bytes());
        assert!(
            scan_raw(root_with_children(&negative_list))
                .unwrap_err()
                .to_string()
                .contains("negative length")
        );

        let unknown = vec![10, 0, 0, 13];
        assert!(
            scan_minecraft_data_version_from_nbt(Cursor::new(unknown))
                .unwrap_err()
                .to_string()
                .contains("unknown NBT tag type 13")
        );

        let mut truncated = vec![10, 0, 0];
        push_named_header(&mut truncated, 3, MINECRAFT_DATA_VERSION_NAME);
        truncated.extend_from_slice(&[0, 0]);
        assert!(
            scan_minecraft_data_version_from_nbt(Cursor::new(truncated))
                .unwrap_err()
                .to_string()
                .contains("MinecraftDataVersion value")
        );

        assert!(
            scan_minecraft_data_version_from_nbt(Cursor::new([9, 0, 0]))
                .unwrap_err()
                .to_string()
                .contains("expected TAG_Compound")
        );
    }

    #[test]
    fn data_version_scan_rejects_ambiguous_or_invalid_target_fields() {
        let mut wrong_type = Vec::new();
        push_named_header(&mut wrong_type, 4, MINECRAFT_DATA_VERSION_NAME);
        wrong_type.extend_from_slice(&3_700_i64.to_be_bytes());
        assert!(
            scan_raw(root_with_children(&wrong_type))
                .unwrap_err()
                .to_string()
                .contains("expected TAG_Int")
        );

        let mut duplicate = Vec::new();
        push_data_version(&mut duplicate, 3_700);
        push_data_version(&mut duplicate, 3_700);
        assert!(
            scan_raw(root_with_children(&duplicate))
                .unwrap_err()
                .to_string()
                .contains("duplicate MinecraftDataVersion")
        );

        let mut end_list = Vec::new();
        push_named_header(&mut end_list, 9, b"invalid-end-list");
        end_list.push(0);
        end_list.extend_from_slice(&1_i32.to_be_bytes());
        assert!(
            scan_raw(root_with_children(&end_list))
                .unwrap_err()
                .to_string()
                .contains("TAG_End elements with non-zero length")
        );

        assert_eq!(
            scan_raw(root_with_children(&[]))
                .unwrap()
                .minecraft_data_version(),
            None
        );
    }

    #[test]
    fn data_version_scan_enforces_only_a_structural_depth_limit() {
        fn nested_compounds(levels: usize) -> Vec<u8> {
            let mut bytes = vec![10, 0, 0];
            for _ in 0..levels {
                push_named_header(&mut bytes, 10, b"nested");
            }
            bytes.extend(std::iter::repeat_n(0, levels + 1));
            bytes
        }

        let (scan, _) = scan_minecraft_data_version_from_nbt(Cursor::new(nested_compounds(
            MAX_NBT_CONTAINER_DEPTH - 1,
        )))
        .unwrap();
        assert_eq!(scan.max_depth(), MAX_NBT_CONTAINER_DEPTH);

        let error = scan_minecraft_data_version_from_nbt(Cursor::new(nested_compounds(
            MAX_NBT_CONTAINER_DEPTH,
        )))
        .unwrap_err()
        .to_string();
        assert!(error.contains("structural safety limit"));

        fn nested_lists(levels: usize) -> Vec<u8> {
            let mut bytes = vec![10, 0, 0];
            push_named_header(&mut bytes, 9, b"nested-list");
            for level in 0..levels {
                if level + 1 == levels {
                    bytes.push(0);
                    bytes.extend_from_slice(&0_i32.to_be_bytes());
                } else {
                    bytes.push(9);
                    bytes.extend_from_slice(&1_i32.to_be_bytes());
                }
            }
            bytes.push(0);
            bytes
        }

        let (scan, _) = scan_minecraft_data_version_from_nbt(Cursor::new(nested_lists(
            MAX_NBT_CONTAINER_DEPTH - 1,
        )))
        .unwrap();
        assert_eq!(scan.max_depth(), MAX_NBT_CONTAINER_DEPTH);
        assert!(
            scan_minecraft_data_version_from_nbt(Cursor::new(nested_lists(
                MAX_NBT_CONTAINER_DEPTH,
            )))
            .unwrap_err()
            .to_string()
            .contains("structural safety limit")
        );
    }

    #[test]
    fn gzip_data_version_scan_validates_footer_and_rejects_all_trailing_data() {
        let mut children = Vec::new();
        push_data_version(&mut children, 3_700);
        let root = root_with_children(&children);
        let valid = gzip(&root);
        assert_eq!(
            scan_minecraft_data_version_from_gzip(Cursor::new(valid.clone()))
                .unwrap()
                .minecraft_data_version(),
            Some(3_700)
        );

        let mut corrupt_crc = valid.clone();
        let crc_offset = corrupt_crc.len() - 8;
        corrupt_crc[crc_offset] ^= 1;
        assert!(scan_minecraft_data_version_from_gzip(Cursor::new(corrupt_crc)).is_err());

        let mut corrupt_isize = valid.clone();
        let isize_offset = corrupt_isize.len() - 4;
        corrupt_isize[isize_offset] ^= 1;
        assert!(scan_minecraft_data_version_from_gzip(Cursor::new(corrupt_isize)).is_err());

        let mut truncated_footer = valid.clone();
        truncated_footer.truncate(truncated_footer.len() - 4);
        assert!(scan_minecraft_data_version_from_gzip(Cursor::new(truncated_footer)).is_err());

        let mut decompressed_tail = root.clone();
        decompressed_tail.push(1);
        assert!(
            scan_minecraft_data_version_from_gzip(Cursor::new(gzip(&decompressed_tail)))
                .unwrap_err()
                .to_string()
                .contains("decompressed bytes after the root")
        );

        let mut compressed_tail = valid.clone();
        compressed_tail.push(1);
        assert!(
            scan_minecraft_data_version_from_gzip(Cursor::new(compressed_tail))
                .unwrap_err()
                .to_string()
                .contains("second member or trailing compressed bytes")
        );

        let mut second_member = valid;
        second_member.extend_from_slice(&gzip(&root));
        assert!(
            scan_minecraft_data_version_from_gzip(Cursor::new(second_member))
                .unwrap_err()
                .to_string()
                .contains("second member or trailing compressed bytes")
        );
    }

    #[test]
    fn decodes_legacy_underpacked_palette_when_extra_palette_entry_is_unused() {
        let indices: Vec<usize> = (0..1000).map(|index| index % 32).collect();
        let block_states = pack_palette_indices(&indices, 5);

        let mut decoded = Vec::new();
        for_each_palette_index(
            &block_states,
            indices.len(),
            bits_for_palette(33),
            33,
            |_, value| {
                decoded.push(value);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(decoded, indices);
    }

    #[test]
    fn rejects_underpacked_palette_when_actual_length_does_not_match_known_layout() {
        let block_states = BigEndianLongArray::new(vec![0]);
        let error =
            for_each_palette_index(&block_states, 1000, bits_for_palette(33), 33, |_, _| Ok(()))
                .unwrap_err()
                .to_string();

        assert!(error.contains("block state long array length mismatch"));
    }
}
