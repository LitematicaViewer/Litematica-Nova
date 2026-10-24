//! Content-driven format adapters. All consumers receive the same LitematicRoot
//! used by statistics, conversion and the existing render data plane.
//! Source contracts: vanilla StructureTemplate; Create SchematicExport;
//! AnvilCraft StructureSnapshotCodec; BG1 Template/TemplateIO/MathUtils;
//! BG2 Template/BG2Data. No extension selects a parser.
use crate::{
    composition_scene::{self, SceneDocument},
    model::{BlockStateNbt, EnclosingSize, EntityNbt},
    nbt::{self, LitematicRoot, MetadataNbt, RegionNbt, Vec3i},
};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use fastnbt::{IntArray, Value};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_INPUT: u64 = 512 << 20;
const MAX_EXPANDED: u64 = 2 << 30;
const MAX_TEXT: usize = 64 << 20;
const MAX_CELLS: usize = 128 << 20;
const SOURCE_EXTRAS: &str = "NovaFormatExtras";
const ANCHOR_DELTA: &str = "NovaStructureAnchorDelta";
const ANVIL_TICKS: &str = "NovaAnvilScheduledTicks";
const PRESENCE: &str = "NovaPresenceMask";
type Fields = HashMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatId {
    Litematic,
    Structure,
    Create,
    AnvilCraft,
    BuildingGadgetsLegacy,
    BuildingGadgets2,
}
impl FormatId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Litematic => "litematic",
            Self::Structure => "structure",
            Self::Create => "create",
            Self::AnvilCraft => "anvilCraft",
            Self::BuildingGadgetsLegacy => "buildingGadgetsLegacy",
            Self::BuildingGadgets2 => "buildingGadgets2",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatReport {
    pub format: FormatId,
    pub variant: String,
    pub data_version: Option<i32>,
    pub size: [i32; 3],
    pub origin: [i32; 3],
    pub regions: usize,
    pub blocks: u64,
    pub block_entities: usize,
    pub entities: usize,
    pub namespaces: Vec<String>,
    pub can_import: bool,
    pub can_export: bool,
    pub missing_semantics: Vec<String>,
    pub compatible_consumers: Vec<FormatId>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCapability {
    pub format: FormatId,
    pub can_export: bool,
    pub reason: Option<String>,
}

pub fn registry() -> Vec<FormatId> {
    vec![
        FormatId::Litematic,
        FormatId::Structure,
        FormatId::Create,
        FormatId::AnvilCraft,
        FormatId::BuildingGadgetsLegacy,
        FormatId::BuildingGadgets2,
    ]
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "格式操作已取消");
    Ok(())
}
fn compound(value: &Value) -> Result<&Fields> {
    if let Value::Compound(v) = value {
        Ok(v)
    } else {
        bail!("格式字段不是NBT Compound")
    }
}
fn list<'a>(fields: &'a Fields, key: &str) -> Result<&'a [Value]> {
    match fields.get(key) {
        Some(Value::List(v)) => Ok(v),
        _ => bail!("格式缺少合法列表 {key}"),
    }
}
fn number(v: &Value) -> Result<i32> {
    match v {
        Value::Byte(v) => Ok(i32::from(*v)),
        Value::Short(v) => Ok(i32::from(*v)),
        Value::Int(v) => Ok(*v),
        Value::Long(v) => Ok(i32::try_from(*v)?),
        _ => bail!("坐标/索引不是整数"),
    }
}
fn xyz(v: &Value) -> Result<[i32; 3]> {
    let a = match v {
        Value::List(v) => v.iter().map(number).collect::<Result<Vec<_>>>()?,
        Value::IntArray(v) => v.iter().copied().collect(),
        Value::Compound(v) => vec![
            number(v.get("X").or_else(|| v.get("x")).context("缺少X")?)?,
            number(v.get("Y").or_else(|| v.get("y")).context("缺少Y")?)?,
            number(v.get("Z").or_else(|| v.get("z")).context("缺少Z")?)?,
        ],
        _ => bail!("坐标不是三元组"),
    };
    ensure!(a.len() == 3, "坐标需要三个分量");
    Ok([a[0], a[1], a[2]])
}
fn float_xyz(v: &Value) -> Result<[f64; 3]> {
    let Value::List(v) = v else {
        bail!("实体坐标不是列表")
    };
    ensure!(v.len() == 3, "实体坐标需要三个分量");
    let mut out = [0.; 3];
    for (a, v) in v.iter().enumerate() {
        out[a] = match v {
            Value::Double(v) => *v,
            Value::Float(v) => f64::from(*v),
            _ => f64::from(number(v)?),
        };
        ensure!(out[a].is_finite(), "实体坐标不是有限数")
    }
    Ok(out)
}
fn int_list(v: [i32; 3]) -> Value {
    Value::List(v.into_iter().map(Value::Int).collect())
}
fn double_list(v: [f64; 3]) -> Value {
    Value::List(v.into_iter().map(Value::Double).collect())
}
fn pos_tag(v: [i32; 3]) -> Value {
    Value::Compound(
        [
            ("X".into(), Value::Int(v[0])),
            ("Y".into(), Value::Int(v[1])),
            ("Z".into(), Value::Int(v[2])),
        ]
        .into(),
    )
}
fn volume(size: [i32; 3]) -> Result<usize> {
    ensure!(size.iter().all(|v| *v > 0), "结构尺寸必须为正数");
    let total = size.into_iter().try_fold(1usize, |n, v| {
        n.checked_mul(v as usize).context("结构体积溢出")
    })?;
    ensure!(total <= MAX_CELLS, "结构声明体积超过当前适配器的内存容量");
    Ok(total)
}
fn state(v: &Value) -> Result<BlockStateNbt> {
    let s: BlockStateNbt = fastnbt::value::from_value(v)?;
    ensure!(
        !s.name.is_empty() && s.name.contains(':'),
        "方块状态缺少命名空间"
    );
    Ok(s)
}
fn is_air(s: &BlockStateNbt) -> bool {
    matches!(
        s.name.as_str(),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}
fn blank_state(name: &str) -> BlockStateNbt {
    BlockStateNbt {
        name: name.into(),
        properties: BTreeMap::new(),
    }
}
fn read_bounded(reader: impl Read, max: u64, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let mut reader = reader.take(max + 1);
    let mut data = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        cancelled(cancel)?;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        };
        ensure!(
            data.len().saturating_add(n) <= max as usize,
            "格式输入超过解压容量"
        );
        data.extend_from_slice(&chunk[..n]);
    }
    Ok(data)
}
fn read_nbt(bytes: &[u8], cancel: &AtomicBool) -> Result<Value> {
    let decoded = if bytes.starts_with(&[0x1f, 0x8b]) {
        read_bounded(GzDecoder::new(bytes), MAX_EXPANDED, cancel)?
    } else {
        ensure!(bytes.len() as u64 <= MAX_EXPANDED, "NBT超过容量");
        bytes.to_vec()
    };
    cancelled(cancel)?;
    let value = fastnbt::from_bytes(&decoded).context("NBT解码失败")?;
    compound(&value)?;
    Ok(value)
}

/// Probe only root tag headers before Regions. A normal litematic reaches
/// this marker without reading BlockStates, hashing or materializing a scene.
/// An inconclusive bounded probe falls back to the complete format decoder.
pub fn is_native_litematic(path: &Path) -> Result<bool> {
    let mut file = BufReader::with_capacity(8192, File::open(path)?);
    let compressed = file.fill_buf()?.starts_with(&[0x1f, 0x8b]);
    if !compressed {
        return Ok(false);
    }
    let reader: Box<dyn Read> = if compressed {
        Box::new(GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    // Normal embedded 320x240 ARGB previews alone exceed 128 KiB. Permit the
    // existing small metadata payload without ever descending into Regions.
    let mut reader = reader.take(2 << 20);
    fn byte(reader: &mut impl Read) -> Result<u8> {
        let mut value = [0];
        reader.read_exact(&mut value)?;
        Ok(value[0])
    }
    fn short(reader: &mut impl Read) -> Result<u16> {
        let mut value = [0; 2];
        reader.read_exact(&mut value)?;
        Ok(u16::from_be_bytes(value))
    }
    fn integer(reader: &mut impl Read) -> Result<i32> {
        let mut value = [0; 4];
        reader.read_exact(&mut value)?;
        Ok(i32::from_be_bytes(value))
    }
    fn skip(reader: &mut impl Read, count: u64) -> Result<()> {
        let actual = std::io::copy(&mut reader.take(count), &mut std::io::sink())?;
        ensure!(actual == count, "NBT头探测容量不足");
        Ok(())
    }
    fn payload(reader: &mut impl Read, tag: u8, depth: usize) -> Result<()> {
        ensure!(depth <= 128, "NBT嵌套超过容量");
        match tag {
            1 => skip(reader, 1)?,
            2 => skip(reader, 2)?,
            3 | 5 => skip(reader, 4)?,
            4 | 6 => skip(reader, 8)?,
            7 | 11 | 12 => {
                let len = integer(reader)?;
                ensure!(len >= 0, "NBT数组长度为负");
                skip(
                    reader,
                    (len as u64)
                        * match tag {
                            11 => 4,
                            12 => 8,
                            _ => 1,
                        },
                )?;
            }
            8 => {
                let len = short(reader)?;
                skip(reader, u64::from(len))?;
            }
            9 => {
                let element = byte(reader)?;
                let len = integer(reader)?;
                ensure!(len >= 0, "NBT列表长度为负");
                for _ in 0..len {
                    payload(reader, element, depth + 1)?;
                }
            }
            10 => loop {
                let next = byte(reader)?;
                if next == 0 {
                    break;
                };
                let len = short(reader)?;
                skip(reader, u64::from(len))?;
                payload(reader, next, depth + 1)?;
            },
            _ => bail!("未知NBT类型{tag}"),
        };
        Ok(())
    }
    let probe = (|| -> Result<bool> {
        if byte(&mut reader)? != 10 {
            return Ok(false);
        }
        let root_name = short(&mut reader)?;
        skip(&mut reader, u64::from(root_name))?;
        loop {
            let tag = byte(&mut reader)?;
            if tag == 0 {
                return Ok(false);
            }
            let length = short(&mut reader)?;
            let mut name = vec![0; usize::from(length)];
            reader.read_exact(&mut name)?;
            if name == b"Regions" {
                return Ok(tag == 10);
            }
            // Foreign format's decisive markers cannot be litematic payload.
            if matches!(
                name.as_slice(),
                b"size" | b"blocks" | b"palette" | b"palettes" | b"blockstatemap" | b"statelist"
            ) {
                return Ok(false);
            }
            payload(&mut reader, tag, 0)?;
        }
    })();
    // Truncated/large metadata is not declared valid by this probe. The full
    // decoder retains the actual syntax error/capacity diagnostic afterward.
    Ok(probe.unwrap_or(false))
}

/// Sniff validates root markers; a .nbt suffix alone never implies Structure.
pub fn sniff(bytes: &[u8], cancel: &AtomicBool) -> Result<FormatId> {
    cancelled(cancel)?;
    if let Ok(text) = std::str::from_utf8(bytes) {
        if text.trim_start().starts_with('{') {
            ensure!(text.len() <= MAX_TEXT, "模板文本超过容量");
            let value: serde_json::Value = serde_json::from_str(text)?;
            if value
                .get("statePosArrayList")
                .is_some_and(serde_json::Value::is_string)
            {
                return Ok(FormatId::BuildingGadgets2);
            }
            if value
                .get("header")
                .is_some_and(serde_json::Value::is_object)
                && value.get("body").is_some_and(serde_json::Value::is_string)
            {
                return Ok(FormatId::BuildingGadgetsLegacy);
            }
            bail!("未知模板JSON结构");
        }
    }
    sniff_nbt(&read_nbt(bytes, cancel)?)
}
fn sniff_nbt(value: &Value) -> Result<FormatId> {
    let f = compound(value)?;
    if matches!(f.get("Regions"), Some(Value::Compound(_)))
        && f.contains_key("Metadata")
        && f.contains_key("Version")
    {
        return Ok(FormatId::Litematic);
    }
    if f.contains_key("size")
        && f.contains_key("blocks")
        && (f.contains_key("palette") || f.contains_key("palettes"))
    {
        return Ok(if f.keys().any(|s| s.starts_with("anvilcraft:")) {
            FormatId::AnvilCraft
        } else {
            FormatId::Structure
        });
    }
    if f.contains_key("data") && f.contains_key("pos") && f.contains_key("header") {
        return Ok(FormatId::BuildingGadgetsLegacy);
    }
    if f.contains_key("blockstatemap") && f.contains_key("statelist") && f.contains_key("startpos")
    {
        return Ok(FormatId::BuildingGadgets2);
    }
    bail!("内容不符合任何已登记结构格式")
}

pub fn decode(path: &Path, cancel: &AtomicBool) -> Result<SceneDocument> {
    let file = File::open(path).with_context(|| format!("打开结构文件 {}", path.display()))?;
    ensure!(file.metadata()?.len() <= MAX_INPUT, "格式输入超过文件容量");
    let bytes = read_bounded(file, MAX_INPUT, cancel)?;
    decode_bytes(
        &bytes,
        path.file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("import"),
        cancel,
    )
}

/// Import and composition share the same content-bound source-version choice.
/// Native consumers still use `decode`; they must not silently gain guessed
/// version semantics merely because a file happens to have a familiar name.
pub fn decode_for_import(
    path: &Path,
    cancel: &AtomicBool,
    storage: &Path,
    source_data_version: Option<i32>,
) -> Result<SceneDocument> {
    let mut scene = decode(path, cancel)?;
    apply_source_version(&mut scene, Some(storage), source_data_version, true, cancel)?;
    ensure!(
        scene.data_version().is_some(),
        "源模板没有声明Minecraft版本，请先选择真实源版本；未借用当前资源版本冒充"
    );
    Ok(scene)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceVersionChoice {
    schema: String,
    source_identity: String,
    format: String,
    data_version: i32,
}

fn registered_source_version(version: i32) -> Result<()> {
    ensure!(
        crate::world_export::profiles()?
            .iter()
            .any(|profile| profile.data_version == Some(version)),
        "源DataVersion {version} 未登记，不能猜测版本语义"
    );
    Ok(())
}

fn apply_source_version(
    scene: &mut SceneDocument,
    storage: Option<&Path>,
    explicit: Option<i32>,
    persist_explicit: bool,
    cancel: &AtomicBool,
) -> Result<()> {
    cancelled(cancel)?;
    let choice_path = storage.map(|root| {
        root.join("format-imports")
            .join(&scene.content_id)
            .join("source-version.json")
    });
    let cached = if let Some(path) = choice_path.as_ref().filter(|path| path.is_file()) {
        let bytes = read_bounded(File::open(path)?, 4096, cancel)?;
        let choice: SourceVersionChoice =
            serde_json::from_slice(&bytes).context("源版本选择记录无效")?;
        ensure!(
            choice.schema == "nova.format-source-version.v1"
                && choice.source_identity == scene.content_id
                && choice.format == scene.source_format,
            "源版本选择记录与实际内容身份不一致"
        );
        registered_source_version(choice.data_version)?;
        Some(choice.data_version)
    } else {
        None
    };
    if let Some(version) = explicit {
        registered_source_version(version)?;
    }
    if let Some(actual) = scene.data_version() {
        registered_source_version(actual)?;
        if let Some(selected) = explicit.or(cached) {
            ensure!(
                actual == selected,
                "输入声明DataVersion与源文件不一致：声明{actual}，选择{selected}"
            );
        }
        return Ok(());
    }
    let Some(version) = explicit.or(cached) else {
        return Ok(());
    };
    std::sync::Arc::get_mut(&mut scene.root)
        .context("源场景仍被共享，不能更改版本身份")?
        .minecraft_data_version = Some(version);
    if let Value::Compound(raw) = &mut scene.raw {
        raw.insert("MinecraftDataVersion".into(), Value::Int(version));
    }
    if persist_explicit && explicit.is_some() && cached != explicit {
        if let Some(path) = choice_path {
            let parent = path.parent().context("源版本选择目录无效")?;
            fs::create_dir_all(parent)?;
            let choice = SourceVersionChoice {
                schema: "nova.format-source-version.v1".into(),
                source_identity: scene.content_id.clone(),
                format: scene.source_format.clone(),
                data_version: version,
            };
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer(file.as_file_mut(), &choice)?;
            file.as_file().sync_all()?;
            cancelled(cancel)?;
            file.persist(&path).map_err(|error| error.error)?;
        }
    }
    Ok(())
}

fn safe_source_stem(source: Option<&str>) -> String {
    let stem = source
        .and_then(|source| Path::new(source).file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or("Clipboard");
    let safe: String = stem
        .chars()
        .take(80)
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_matches(['.', '_']);
    if safe.is_empty() {
        "schematic".into()
    } else {
        safe.into()
    }
}

pub fn decode_bytes(bytes: &[u8], name: &str, cancel: &AtomicBool) -> Result<SceneDocument> {
    ensure!(bytes.len() as u64 <= MAX_INPUT, "格式输入超过文件容量");
    let is_text = std::str::from_utf8(bytes).is_ok_and(|text| text.trim_start().starts_with('{'));
    let mut doc = if is_text {
        let kind = sniff(bytes, cancel)?;
        decode_gadget(bytes, name, kind, cancel, None)?
    } else {
        // Read and inflate binary input exactly once. inspect and decode do not
        // first deserialize a second full NBT tree merely to determine format.
        let raw = read_nbt(bytes, cancel)?;
        match sniff_nbt(&raw)? {
            FormatId::Litematic => composition_scene::from_raw_litematic(raw, "litematic")?,
            kind @ (FormatId::BuildingGadgetsLegacy | FormatId::BuildingGadgets2) => {
                decode_gadget(bytes, name, kind, cancel, Some(raw))?
            }
            kind => decode_structure(raw, name, kind, cancel)?,
        }
    };
    cancelled(cancel)?;
    doc.content_id = format!("{:x}", Sha256::digest(bytes));
    Ok(doc)
}
pub fn inspect(path: &Path, cancel: &AtomicBool) -> Result<FormatReport> {
    let scene = decode(path, cancel)?;
    report(&scene, cancel)
}
pub fn report(scene: &SceneDocument, cancel: &AtomicBool) -> Result<FormatReport> {
    let mut blocks = 0;
    let mut block_entities = 0;
    let mut entities = 0;
    let mut namespaces = BTreeSet::new();
    let mut bounds: Option<[i64; 6]> = None;
    for region in scene.root.regions.values() {
        cancelled(cancel)?;
        let b = nbt::region_bounds(region);
        let next = [
            b.min_x as i64,
            b.min_y as i64,
            b.min_z as i64,
            b.max_x as i64,
            b.max_y as i64,
            b.max_z as i64,
        ];
        bounds = Some(match bounds {
            None => next,
            Some(mut b) => {
                for a in 0..3 {
                    b[a] = b[a].min(next[a]);
                    b[a + 3] = b[a + 3].max(next[a + 3]);
                }
                b
            }
        });
        for s in &region.block_state_palette {
            if let Some((n, _)) = s.name.split_once(':') {
                namespaces.insert(n.to_owned());
            }
        }
        let size = nbt::region_volume(&region.size)?;
        nbt::for_each_palette_index(
            &region.block_states,
            size,
            nbt::bits_for_palette(region.block_state_palette.len()),
            region.block_state_palette.len(),
            |i, p| {
                if i & 4095 == 0 {
                    cancelled(cancel)?
                };
                let s = region
                    .block_state_palette
                    .get(p)
                    .context("调色板索引越界")?;
                if !is_air(s) {
                    blocks += 1
                };
                Ok(())
            },
        )?;
        block_entities += region.tile_entities.len();
        entities += region.entities.len();
        for entity in &region.entities {
            if let Some(id) = entity.entity_id() {
                if let Some((n, _)) = id.split_once(':') {
                    namespaces.insert(n.to_owned());
                }
            }
        }
    }
    let format = registry()
        .into_iter()
        .find(|f| f.as_str() == scene.source_format)
        .unwrap_or(FormatId::Litematic);
    let size = match bounds {
        None => [0; 3],
        Some(b) => [
            i32::try_from(b[3] - b[0] + 1)?,
            i32::try_from(b[4] - b[1] + 1)?,
            i32::try_from(b[5] - b[2] + 1)?,
        ],
    };
    let origin = bounds.map_or([0; 3], |b| [b[0] as i32, b[1] as i32, b[2] as i32]);
    Ok(FormatReport{format,variant:match format{FormatId::Structure=>"vanilla StructureTemplate / Create-compatible (origin not distinguishable from shared schema)",FormatId::Create=>"Create StructureTemplate",FormatId::AnvilCraft=>"AnvilCraft StructureSnapshotCodec",FormatId::BuildingGadgetsLegacy=>"BG1 2.x header + compressed NBT",FormatId::BuildingGadgets2=>"BG2 statePosArrayList",_=>"Litematica"}.into(),data_version:scene.data_version(),size,origin,regions:scene.root.regions.len(),blocks,block_entities,entities,namespaces:namespaces.into_iter().collect(),can_import:true,can_export:export_reason(scene,format).is_none(),missing_semantics:if scene.data_version().is_none(){vec!["源格式未声明Minecraft DataVersion；跨版本转换前必须选择真实源版本".into()]}else{vec![]},compatible_consumers:if matches!(format,FormatId::Structure|FormatId::Create|FormatId::AnvilCraft){vec![FormatId::Structure,FormatId::Create,FormatId::AnvilCraft]}else{vec![format]}})
}
pub fn capabilities(scene: &SceneDocument) -> Vec<FormatCapability> {
    registry()
        .into_iter()
        .map(|format| {
            let reason = export_reason(scene, format);
            FormatCapability {
                format,
                can_export: reason.is_none(),
                reason,
            }
        })
        .collect()
}
fn export_reason(scene: &SceneDocument, format: FormatId) -> Option<String> {
    if format == FormatId::Litematic {
        return None;
    }
    if scene.root.regions.is_empty() {
        return Some("场景没有可以写出的结构选区".into());
    }
    let region = scene.root.regions.values().next()?;
    if matches!(
        format,
        FormatId::BuildingGadgetsLegacy | FormatId::BuildingGadgets2
    ) && scene.root.regions.values().any(|r| !r.entities.is_empty())
    {
        return Some("该模板共享格式不保存普通实体，不能无损导出".into());
    }
    if format == FormatId::BuildingGadgets2
        && scene
            .root
            .regions
            .values()
            .any(|r| !r.tile_entities.is_empty())
    {
        return Some("上游BG2模板JSON不含方块实体数据，不能无损导出容器或其他方块实体".into());
    }
    if format == FormatId::BuildingGadgets2 {
        for (name, region) in &scene.root.regions {
            let volume = match nbt::region_volume(&region.size) {
                Ok(v) => v,
                Err(error) => return Some(error.to_string()),
            };
            match scene.presence_mask(name) {
                Err(error) => return Some(error.to_string()),
                Ok(Some(mask)) => {
                    if mask.iter().enumerate().any(|(index, byte)| {
                        let remaining = volume.saturating_sub(index * 8).min(8);
                        let expected = ((1u16 << remaining) - 1) as u8;
                        (*byte as u8) & expected != expected
                    }) {
                        return Some(
                            "BG2模板不支持缺格的非放置语义，不能无损导出为显式空气".into(),
                        );
                    }
                }
                Ok(None) => {}
            }
        }
    }
    if format == FormatId::BuildingGadgetsLegacy
        && scene.data_version().is_none()
        && !matches!(
            scene_extras(scene).get("BuildingGadgetsHeader"),
            Some(Value::String(_))
        )
    {
        return Some("BG1写出需要真实Minecraft版本；源格式未声明时请先选择源版本".into());
    }
    if format == FormatId::BuildingGadgetsLegacy {
        let version = bg1_minecraft_version(scene);
        let in_verified_range = version
            .as_deref()
            .and_then(|version| {
                let parts = version
                    .split('.')
                    .map(str::parse::<u32>)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .ok()?;
                Some(
                    parts.as_slice() >= [1, 14, 4].as_slice()
                        && parts.as_slice() <= [1, 17, 1].as_slice(),
                )
            })
            .unwrap_or(false);
        if !in_verified_range {
            return Some(
                "已核对的BG1 2.1.0消费者接受Minecraft 1.14.4至1.17.1；其他子版本未冒充可写".into(),
            );
        }
    }
    if matches!(
        format,
        FormatId::Structure | FormatId::Create | FormatId::AnvilCraft
    ) && scene.data_version().is_none()
    {
        return Some("结构写出需要真实Minecraft DataVersion，不能猜测源版本".into());
    }
    if format == FormatId::AnvilCraft
        && [region.size.x, region.size.y, region.size.z]
            .into_iter()
            .any(|v| v.unsigned_abs() > 16)
    {
        return Some("已核对的AnvilCraft扫描器每轴最多16格；请先拆分，未冒充完整磁盘支持".into());
    }
    None
}
fn make_scene(
    name: &str,
    size: [i32; 3],
    origin: [i32; 3],
    palette: Vec<BlockStateNbt>,
    indices: Vec<usize>,
    tiles: Vec<Value>,
    entities: Vec<EntityNbt>,
    data_version: Option<i32>,
    kind: FormatId,
    extras: Fields,
) -> Result<SceneDocument> {
    let total = volume(size)?;
    ensure!(indices.len() == total, "方块数组长度与尺寸不符");
    let count = indices
        .iter()
        .filter(|i| palette.get(**i).is_some_and(|s| !is_air(s)))
        .count();
    let blocks = nbt::pack_palette_indices(&indices, nbt::bits_for_palette(palette.len()));
    let root = LitematicRoot {
        metadata: MetadataNbt {
            author: None,
            description: Some(format!("Imported {}", kind.as_str())),
            name: Some(name.into()),
            time_created: None,
            time_modified: None,
            total_blocks: Some(i32::try_from(count)?),
            total_volume: Some(i32::try_from(total)?),
            region_count: Some(1),
            enclosing_size: Some(EnclosingSize {
                x: size[0],
                y: size[1],
                z: size[2],
            }),
            preview_image_data: None,
        },
        regions: [(
            "Imported".into(),
            RegionNbt {
                position: Vec3i {
                    x: origin[0],
                    y: origin[1],
                    z: origin[2],
                },
                size: Vec3i {
                    x: size[0],
                    y: size[1],
                    z: size[2],
                },
                block_state_palette: palette,
                block_states: blocks,
                entities,
                tile_entities: tiles,
            },
        )]
        .into(),
        version: 6,
        sub_version: Some(1),
        minecraft_data_version: data_version,
        preview_image_data: None,
    };
    let mut raw: Value = fastnbt::from_bytes(&fastnbt::to_bytes(&root)?)?;
    if matches!(
        kind,
        FormatId::Structure
            | FormatId::Create
            | FormatId::AnvilCraft
            | FormatId::BuildingGadgetsLegacy
    ) && indices.contains(&0)
    {
        // Imported absent cells use real air in the consumer root, never a
        // synthetic material. The shared raw presence mask retains non-place
        // semantics, distinct from explicit air or real structure_void.
        let mut mask = vec![0i8; total.div_ceil(8)];
        for (index, palette) in indices.iter().enumerate() {
            if *palette != 0 {
                mask[index / 8] |= (1u8 << (index & 7)) as i8;
            }
        }
        if let Value::Compound(fields) = &mut raw {
            if let Some(Value::Compound(regions)) = fields.get_mut("Regions") {
                if let Some(Value::Compound(region)) = regions.get_mut("Imported") {
                    region.insert(
                        PRESENCE.into(),
                        Value::ByteArray(fastnbt::ByteArray::new(mask)),
                    );
                }
            }
        }
    }
    if !extras.is_empty() {
        if let Value::Compound(f) = &mut raw {
            f.insert(SOURCE_EXTRAS.into(), Value::Compound(extras));
        }
    }
    let mut doc = composition_scene::from_litematic_root(root, Some(raw))?;
    doc.source_format = kind.as_str().into();
    Ok(doc)
}
fn decode_structure(
    raw: Value,
    name: &str,
    kind: FormatId,
    cancel: &AtomicBool,
) -> Result<SceneDocument> {
    let fields = compound(&raw)?;
    let size = xyz(fields.get("size").context("结构缺少size")?)?;
    let total = volume(size)?;
    let palette_values = if fields.contains_key("palette") {
        list(fields, "palette")?
    } else {
        let palettes = list(fields, "palettes")?;
        ensure!(
            palettes.len() == 1,
            "结构含多个随机调色板，必须明确选择后才可导入，未丢弃其他变体"
        );
        let Value::List(values) = &palettes[0] else {
            bail!("palettes内容不是列表")
        };
        values
    };
    ensure!(!palette_values.is_empty(), "结构调色板为空");
    let mut palette = vec![blank_state("minecraft:air")];
    palette.extend(
        palette_values
            .iter()
            .map(state)
            .collect::<Result<Vec<_>>>()?,
    );
    let mut indices = vec![0usize; total];
    let mut occupied = BTreeSet::new();
    let mut tiles = Vec::new();
    for (i, block) in list(fields, "blocks")?.iter().enumerate() {
        if i & 4095 == 0 {
            cancelled(cancel)?
        };
        let b = compound(block)?;
        let pos = xyz(b.get("pos").context("方块缺少pos")?)?;
        ensure!(
            (0..3).all(|a| pos[a] >= 0 && pos[a] < size[a]),
            "结构方块坐标越界 {pos:?}"
        );
        ensure!(
            occupied.insert(pos),
            "结构重复方块坐标 {pos:?}，拒绝无声覆盖"
        );
        let p = number(b.get("state").context("方块缺少state")?)?;
        ensure!(
            p >= 0 && (p as usize) < palette_values.len(),
            "结构调色板索引越界 {p}"
        );
        let index = pos[0] as usize
            + size[0] as usize * (pos[2] as usize + size[2] as usize * pos[1] as usize);
        indices[index] = p as usize + 1;
        if let Some(nbt) = b.get("nbt") {
            let mut nbt = compound(nbt)?.clone();
            for (a, k) in ["x", "y", "z"].into_iter().enumerate() {
                nbt.insert(k.into(), Value::Int(pos[a]));
            }
            tiles.push(Value::Compound(nbt));
        }
    }
    let mut entities = Vec::new();
    if let Some(Value::List(values)) = fields.get("entities") {
        for value in values {
            cancelled(cancel)?;
            let e = compound(value)?;
            let pos = float_xyz(e.get("pos").context("结构实体缺少pos")?)?;
            let anchor = xyz(e.get("blockPos").context("结构实体缺少blockPos")?)?;
            let mut nbt = compound(e.get("nbt").context("结构实体缺少nbt")?)?.clone();
            let delta: [i64; 3] =
                std::array::from_fn(|a| i64::from(anchor[a]) - pos[a].floor() as i64);
            let delta = delta
                .into_iter()
                .map(i32::try_from)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            nbt.insert(ANCHOR_DELTA.into(), Value::IntArray(IntArray::new(delta)));
            nbt.insert("Pos".into(), double_list(pos));
            // EntityNbt flattens its unknown fields. fastnbt 2.6.1's in-memory
            // ArrayAccess never ends its synthetic map; Serde's flatten
            // collector therefore loops on IntArray/ByteArray/LongArray.
            // Keep already-decoded typed NBT intact rather than sending it
            // through that generic deserializer a second time.
            let id_lower = nbt
                .remove("id")
                .map(|value| match value {
                    Value::String(id) => Ok(id),
                    _ => bail!("实体id不是字符串"),
                })
                .transpose()?;
            let id_upper = nbt
                .remove("Id")
                .map(|value| match value {
                    Value::String(id) => Ok(id),
                    _ => bail!("实体Id不是字符串"),
                })
                .transpose()?;
            entities.push(EntityNbt {
                id_lower,
                id_upper,
                data: nbt.into_iter().collect(),
            });
        }
    }
    let extras = fields
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "size"
                    | "palette"
                    | "palettes"
                    | "blocks"
                    | "entities"
                    | "DataVersion"
                    | "anvilcraft:scheduled_ticks"
            )
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut scene = make_scene(
        name,
        size,
        [0; 3],
        palette,
        indices,
        tiles,
        entities,
        fields.get("DataVersion").map(number).transpose()?,
        kind,
        extras,
    )?;
    if let Some(ticks) = fields.get("anvilcraft:scheduled_ticks") {
        let Value::List(values) = ticks else {
            bail!("AnvilCraft计划刻不是列表")
        };
        for value in values {
            let entry = compound(value)?;
            let pos = xyz(entry.get("pos").context("AnvilCraft计划刻缺少pos")?)?;
            ensure!(
                (0..3).all(|a| pos[a] >= 0 && pos[a] < size[a]),
                "AnvilCraft计划刻位置越界"
            );
        }
        if let Value::Compound(raw) = &mut scene.raw {
            if let Some(Value::Compound(regions)) = raw.get_mut("Regions") {
                if let Some(Value::Compound(region)) = regions.get_mut("Imported") {
                    region.insert(ANVIL_TICKS.into(), ticks.clone());
                }
            }
        }
    }
    Ok(scene)
}

fn decode_gadget(
    bytes: &[u8],
    name: &str,
    kind: FormatId,
    cancel: &AtomicBool,
    prepared: Option<Value>,
) -> Result<SceneDocument> {
    let mut extras = Fields::new();
    let (data, header, display_name) = if let Some(data) = prepared {
        (data, None, name.to_owned())
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        if text.trim_start().starts_with('{') {
            ensure!(text.len() <= MAX_TEXT, "模板文本超过容量");
            let json: serde_json::Value = serde_json::from_str(text)?;
            if kind == FormatId::BuildingGadgets2 {
                let body = json
                    .get("statePosArrayList")
                    .and_then(|v| v.as_str())
                    .context("BG2缺少statePosArrayList")?;
                let data = crate::version_payload_migration::read_snbt_bounded(body, MAX_TEXT)?;
                let mut metadata = json.clone();
                if let Some(fields) = metadata.as_object_mut() {
                    fields.remove("statePosArrayList");
                }
                extras.insert(
                    "BuildingGadgets2Header".into(),
                    Value::String(serde_json::to_string(&metadata)?),
                );
                (
                    data,
                    None,
                    json.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(name)
                        .to_owned(),
                )
            } else {
                let body = json
                    .get("body")
                    .and_then(|v| v.as_str())
                    .context("BG1缺少body")?;
                ensure!(body.len() <= MAX_TEXT, "BG1 base64超过容量");
                let compressed = STANDARD.decode(body).context("BG1 body不是合法base64")?;
                (
                    read_nbt(&compressed, cancel)?,
                    json.get("header").cloned(),
                    json.get("header")
                        .and_then(|header| header.get("name"))
                        .and_then(|value| value.as_str())
                        .unwrap_or(name)
                        .to_owned(),
                )
            }
        } else {
            (read_nbt(bytes, cancel)?, None, name.to_owned())
        }
    } else {
        (read_nbt(bytes, cancel)?, None, name.to_owned())
    };
    let f = compound(&data)?;
    let data_version = f.get("DataVersion").map(number).transpose()?.or_else(|| {
        let version = header.as_ref()?.get("mc_version")?.as_str()?;
        crate::world_export::profiles()
            .ok()?
            .into_iter()
            .find(|profile| profile.version == version)?
            .data_version
    });
    if kind == FormatId::BuildingGadgets2 {
        let start = xyz(f.get("startpos").context("BG2缺少startpos")?)?;
        let end = xyz(f.get("endpos").context("BG2缺少endpos")?)?;
        let min = std::array::from_fn(|a| start[a].min(end[a]));
        let max: [i32; 3] = std::array::from_fn(|a| start[a].max(end[a]));
        let size = [
            i32::try_from(i64::from(max[0]) - i64::from(min[0]) + 1)?,
            i32::try_from(i64::from(max[1]) - i64::from(min[1]) + 1)?,
            i32::try_from(i64::from(max[2]) - i64::from(min[2]) + 1)?,
        ];
        let total = volume(size)?;
        let palette = list(f, "blockstatemap")?
            .iter()
            .map(state)
            .collect::<Result<Vec<_>>>()?;
        ensure!(!palette.is_empty(), "BG2调色板为空");
        let Value::IntArray(states) = f.get("statelist").context("BG2缺少statelist")? else {
            bail!("BG2 statelist不是IntArray")
        };
        ensure!(states.len() == total, "BG2 statelist长度与边界体积不符");
        let mut indices = vec![0; total];
        // BlockPos.betweenClosed visits X fastest, then Y, then Z. Litematica
        // storage visits X fastest, then Z, then Y; no guessed ordinal copy.
        for (i, p) in states.iter().copied().enumerate() {
            if i & 4095 == 0 {
                cancelled(cancel)?
            };
            ensure!(
                p >= 0 && (p as usize) < palette.len(),
                "BG2调色板索引越界 {p}"
            );
            let x = i % size[0] as usize;
            let y = (i / size[0] as usize) % size[1] as usize;
            let z = i / (size[0] as usize * size[1] as usize);
            indices[x + size[0] as usize * (z + size[2] as usize * y)] = p as usize;
        }
        make_scene(
            &display_name,
            size,
            min,
            palette,
            indices,
            vec![],
            vec![],
            data_version,
            kind,
            extras,
        )
    } else {
        let h = f.get("header").and_then(|v| compound(v).ok());
        let bounds = header.as_ref().and_then(|h| h.get("bounding_box"));
        let bounds_nbt = h
            .and_then(|h| h.get("bounds"))
            .and_then(|v| compound(v).ok());
        let get_bound = |a: usize, max: bool| -> Result<i32> {
            let axis = ["x", "y", "z"][a];
            if let Some(bounds) = bounds {
                let key = format!("{}_{}", if max { "max" } else { "min" }, axis);
                return i32::try_from(
                    bounds
                        .get(&key)
                        .and_then(|n| n.as_i64())
                        .with_context(|| format!("BG1 header缺少{key}"))?,
                )
                .map_err(Into::into);
            }
            if let Some(b) = bounds_nbt {
                let key = format!("{}{}", if max { "max" } else { "min" }, ["X", "Y", "Z"][a]);
                return number(
                    b.get(&key)
                        .with_context(|| format!("BG1 bounds缺少{key}"))?,
                );
            }
            bail!("BG1缺少真实bounding_box，不能猜测空缺范围")
        };
        let min = [
            get_bound(0, false)?,
            get_bound(1, false)?,
            get_bound(2, false)?,
        ];
        let max = [
            get_bound(0, true)?,
            get_bound(1, true)?,
            get_bound(2, true)?,
        ];
        ensure!((0..3).all(|a| min[a] <= max[a]), "BG1边界顺序无效");
        let size = [
            max[0]
                .checked_sub(min[0])
                .and_then(|v| v.checked_add(1))
                .context("BG1边界溢出")?,
            max[1]
                .checked_sub(min[1])
                .and_then(|v| v.checked_add(1))
                .context("BG1边界溢出")?,
            max[2]
                .checked_sub(min[2])
                .and_then(|v| v.checked_add(1))
                .context("BG1边界溢出")?,
        ];
        let total = volume(size)?;
        let block_data = list(f, "data")?;
        let serializers = match f.get("serializer") {
            Some(Value::List(v)) => Some(v),
            None => None,
            _ => bail!("BG1 serializer不是列表"),
        };
        let mut palette = vec![blank_state("minecraft:air")];
        let mut tile_data = vec![None];
        for entry in block_data {
            let d = compound(entry)?;
            palette.push(state(d.get("state").context("BG1数据缺少state")?)?);
            let serializer = match d.get("serializer") {
                Some(Value::String(v)) => Some(v.as_str()),
                Some(index) => {
                    let index = number(index)?;
                    ensure!(index >= 0, "BG1 serializer索引为负");
                    match serializers.and_then(|v| v.get(index as usize)) {
                        Some(Value::String(v)) => Some(v.as_str()),
                        _ => bail!("BG1 serializer索引无效"),
                    }
                }
                None => None,
            };
            let data = d.get("data").map(compound).transpose()?;
            let tile = match serializer {
                Some("buildinggadgets:nbt_tile_data_serializer") => {
                    let data = data.context("BG1 NBT serializer缺少data")?;
                    Some(compound(data.get("data").context("BG1方块实体缺少data.data")?)?.clone())
                }
                Some("buildinggadgets:dummy_serializer") | None
                    if data.is_none_or(|v| v.is_empty()) =>
                {
                    None
                }
                Some(other) => bail!("BG1未知方块实体serializer {other}；未丢弃其数据"),
                _ => bail!("BG1缺少可识别的方块实体serializer"),
            };
            tile_data.push(tile);
        }
        let mut indices = vec![0usize; total];
        let mut tiles = Vec::new();
        let mut occupied = BTreeSet::new();
        for (i, value) in list(f, "pos")?.iter().enumerate() {
            if i & 4095 == 0 {
                cancelled(cancel)?
            };
            let Value::Long(value) = value else {
                bail!("BG1 pos条目不是Long")
            };
            let packed = *value as u64;
            let p = ((packed >> 40) & 0xffffff) as usize;
            ensure!(p < block_data.len(), "BG1状态索引越界 {p}");
            let pos = [
                ((packed >> 24) & 0xffff) as i32,
                ((packed >> 16) & 0xff) as i32,
                (packed & 0xffff) as i32,
            ];
            ensure!(
                (0..3).all(|a| pos[a] >= min[a] && pos[a] <= max[a]),
                "BG1方块坐标不在header范围"
            );
            ensure!(occupied.insert(pos), "BG1重复位置，拒绝无声覆盖");
            let relative = [pos[0] - min[0], pos[1] - min[1], pos[2] - min[2]];
            indices[relative[0] as usize
                + size[0] as usize
                    * (relative[2] as usize + size[2] as usize * relative[1] as usize)] = p + 1;
            if let Some(tile) = &tile_data[p + 1] {
                let mut tile = tile.clone();
                for (a, key) in ["x", "y", "z"].into_iter().enumerate() {
                    tile.insert(key.into(), Value::Int(relative[a]));
                }
                tiles.push(Value::Compound(tile));
            }
        }
        if let Some(header) = header {
            extras.insert(
                "BuildingGadgetsHeader".into(),
                Value::String(serde_json::to_string(&header)?),
            );
        }
        make_scene(
            &display_name,
            size,
            min,
            palette,
            indices,
            tiles,
            vec![],
            data_version,
            kind,
            extras,
        )
    }
}
pub fn encode(
    scene: &SceneDocument,
    format: FormatId,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<FormatReport> {
    cancelled(cancel)?;
    if let Some(reason) = export_reason(scene, format) {
        bail!("{reason}")
    }
    let mut report = report(scene, cancel)?;
    let bytes = encode_bytes(scene, format, cancel)?;
    let parent = path
        .parent()
        .filter(|v| !v.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    cancelled(cancel)?;
    // This adapter never silently overwrites the user's source or an existing
    // target; overwrite decisions remain the owning operation's transaction.
    temporary
        .persist_noclobber(path)
        .map_err(|e| e.error)
        .with_context(|| format!("保存结构文件 {}", path.display()))?;
    report.format = format;
    if matches!(
        format,
        FormatId::Structure
            | FormatId::Create
            | FormatId::AnvilCraft
            | FormatId::BuildingGadgetsLegacy
    ) && report.origin != [0; 3]
    {
        report.missing_semantics.push(format!(
            "目标格式使用局部结构坐标：原点{:?}已归一化到[0,0,0]，源文件未修改",
            report.origin
        ));
        report.origin = [0; 3];
    }
    Ok(report)
}
pub fn encode_bytes(
    scene: &SceneDocument,
    format: FormatId,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    cancelled(cancel)?;
    if format == FormatId::Litematic {
        return gzip(&scene.raw_litematic()?);
    }
    // The composition owner provides the sole conflict/air/void merge policy.
    // Dense flattening happens only on confirmed export, never on menu display.
    let flattened;
    let scene = if scene.root.regions.len() > 1 {
        flattened = composition_scene::flatten(scene, cancel)?;
        &flattened
    } else {
        scene
    };
    if let Some(reason) = export_reason(scene, format) {
        bail!("{reason}")
    }
    let (region_name, region) = scene
        .root
        .regions
        .first_key_value()
        .context("场景没有选区")?;
    let presence = scene.presence_mask(region_name)?;
    let size = [
        i32::try_from(region.size.x.unsigned_abs())?,
        i32::try_from(region.size.y.unsigned_abs())?,
        i32::try_from(region.size.z.unsigned_abs())?,
    ];
    let total = volume(size)?;
    let mut indices = vec![0usize; total];
    nbt::for_each_palette_index(
        &region.block_states,
        total,
        nbt::bits_for_palette(region.block_state_palette.len()),
        region.block_state_palette.len(),
        |i, p| {
            if i & 4095 == 0 {
                cancelled(cancel)?
            };
            ensure!(p < region.block_state_palette.len(), "导出调色板索引越界");
            indices[i] = p;
            Ok(())
        },
    )?;
    match format {
        FormatId::Structure | FormatId::Create | FormatId::AnvilCraft => {
            let mut fields = scene_extras(scene);
            fields.remove("BuildingGadgetsHeader");
            fields.remove("BuildingGadgets2Header");
            fields.insert(
                "DataVersion".into(),
                Value::Int(scene.data_version().context("结构需要真实DataVersion")?),
            );
            fields.insert("size".into(), int_list(size));
            fields.insert(
                "palette".into(),
                Value::List(
                    region
                        .block_state_palette
                        .iter()
                        .map(|s| typed_nbt_value(s))
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                ),
            );
            let mut blocks = Vec::new();
            let mut tiles = BTreeMap::new();
            let bounds = nbt::region_bounds(region);
            let local_min = [
                bounds.min_x - region.position.x,
                bounds.min_y - region.position.y,
                bounds.min_z - region.position.z,
            ];
            for tile in &region.tile_entities {
                let f = compound(tile)?;
                let pos = [
                    number(f.get("x").context("方块实体缺少x")?)?,
                    number(f.get("y").context("方块实体缺少y")?)?,
                    number(f.get("z").context("方块实体缺少z")?)?,
                ];
                ensure!(
                    tiles.insert(pos, tile.clone()).is_none(),
                    "重复方块实体位置"
                );
            }
            for (i, p) in indices.iter().copied().enumerate() {
                if i & 4095 == 0 {
                    cancelled(cancel)?
                };
                if !composition_scene::mask_contains(presence, i) {
                    continue;
                }
                let pos = [
                    (i % size[0] as usize) as i32,
                    (i / (size[0] as usize * size[2] as usize)) as i32,
                    ((i / size[0] as usize) % size[2] as usize) as i32,
                ];
                let mut block = Fields::from([
                    ("pos".into(), int_list(pos)),
                    ("state".into(), Value::Int(i32::try_from(p)?)),
                ]);
                let source = std::array::from_fn(|a| pos[a] + local_min[a]);
                if let Some(tile) = tiles.remove(&source) {
                    let mut tile = compound(&tile)?.clone();
                    for (a, k) in ["x", "y", "z"].into_iter().enumerate() {
                        tile.insert(k.into(), Value::Int(pos[a]));
                    }
                    block.insert("nbt".into(), Value::Compound(tile));
                }
                blocks.push(Value::Compound(block));
            }
            ensure!(tiles.is_empty(), "方块实体没有对应的可写方块，拒绝丢弃");
            fields.insert("blocks".into(), Value::List(blocks));
            let mut entities = Vec::new();
            for entity in &region.entities {
                cancelled(cancel)?;
                let mut nbt = compound(&typed_nbt_value(entity)?)?.clone();
                let pos = float_xyz(nbt.get("Pos").context("实体缺少Pos")?)?;
                let pos = std::array::from_fn(|a| pos[a] - f64::from(local_min[a]));
                let delta = nbt
                    .remove(ANCHOR_DELTA)
                    .as_ref()
                    .map(xyz)
                    .transpose()?
                    .unwrap_or([0; 3]);
                let mut anchor = [0; 3];
                for a in 0..3 {
                    let coordinate = pos[a].floor();
                    ensure!(
                        coordinate >= i32::MIN as f64 && coordinate <= i32::MAX as f64,
                        "实体锚点越界"
                    );
                    anchor[a] = (coordinate as i32)
                        .checked_add(delta[a])
                        .context("实体锚点溢出")?;
                }
                nbt.insert("Pos".into(), double_list(pos));
                entities.push(Value::Compound(Fields::from([
                    ("pos".into(), double_list(pos)),
                    ("blockPos".into(), int_list(anchor)),
                    ("nbt".into(), Value::Compound(nbt)),
                ])));
            }
            fields.insert("entities".into(), Value::List(entities));
            if let Some((name, _)) = scene.root.regions.first_key_value() {
                if let Some(Value::Compound(raw)) = scene.region_raw(name) {
                    if let Some(Value::List(values)) = raw.get(ANVIL_TICKS) {
                        let mut ticks = Vec::new();
                        for value in values {
                            cancelled(cancel)?;
                            let mut entry = compound(value)?.clone();
                            let pos = xyz(entry.get("pos").context("AnvilCraft计划刻缺少pos")?)?;
                            let pos: [i32; 3] = std::array::from_fn(|a| pos[a] - local_min[a]);
                            ensure!(
                                (0..3).all(|a| pos[a] >= 0 && pos[a] < size[a]),
                                "AnvilCraft计划刻位置越界"
                            );
                            entry.insert("pos".into(), Value::IntArray(IntArray::new(pos.into())));
                            ticks.push(Value::Compound(entry));
                        }
                        fields.insert("anvilcraft:scheduled_ticks".into(), Value::List(ticks));
                    }
                }
            }
            if format == FormatId::AnvilCraft {
                fields
                    .entry("anvilcraft:world_coordinates".into())
                    .or_insert(Value::Byte(1));
            }
            gzip(&Value::Compound(fields))
        }
        FormatId::BuildingGadgets2 => {
            ensure!(
                (0..total).all(|index| composition_scene::mask_contains(presence, index)),
                "BG2模板没有缺格跳过语义，不能把未声明位置偷换为显式空气"
            );
            let mut states = vec![0i32; total];
            for y in 0..size[1] as usize {
                for z in 0..size[2] as usize {
                    for x in 0..size[0] as usize {
                        let src = x + size[0] as usize * (z + size[2] as usize * y);
                        let dst = x + size[0] as usize * (y + size[1] as usize * z);
                        states[dst] = i32::try_from(indices[src])?;
                    }
                }
                cancelled(cancel)?;
            }
            let bounds = nbt::region_bounds(region);
            let data = Value::Compound(Fields::from([
                (
                    "startpos".into(),
                    pos_tag([bounds.min_x, bounds.min_y, bounds.min_z]),
                ),
                (
                    "endpos".into(),
                    pos_tag([bounds.max_x, bounds.max_y, bounds.max_z]),
                ),
                (
                    "blockstatemap".into(),
                    Value::List(
                        region
                            .block_state_palette
                            .iter()
                            .map(typed_nbt_value)
                            .collect::<std::result::Result<Vec<_>, _>>()?,
                    ),
                ),
                ("statelist".into(), Value::IntArray(IntArray::new(states))),
            ]));
            let text = crate::version_payload_migration::write_snbt_shared(&data)?;
            let mut metadata = if scene.source_format == "buildingGadgets2" {
                match scene_extras(scene).get("BuildingGadgets2Header") {
                    Some(Value::String(value)) => serde_json::from_str(value)?,
                    _ => serde_json::json!({"requiredItems":{}}),
                }
            } else {
                serde_json::json!({"requiredItems":{}})
            };
            metadata["name"] =
                serde_json::json!(scene.root.metadata.name.clone().unwrap_or_default());
            metadata["statePosArrayList"] = serde_json::json!(text);
            Ok(serde_json::to_vec_pretty(&metadata)?)
        }
        FormatId::BuildingGadgetsLegacy => {
            ensure!(
                size[0] <= 65536 && size[1] <= 256 && size[2] <= 65536,
                "BG1打包位置超出16/8/16位范围"
            );
            ensure!(
                region.block_state_palette.len() <= 0x1000000,
                "BG1状态表超过24位容量"
            );
            let mut tiles = BTreeMap::new();
            let bounds = nbt::region_bounds(region);
            let offset = [
                bounds.min_x - region.position.x,
                bounds.min_y - region.position.y,
                bounds.min_z - region.position.z,
            ];
            for tile in &region.tile_entities {
                let f = compound(tile)?;
                let p = [
                    number(f.get("x").context("缺少x")?)? - offset[0],
                    number(f.get("y").context("缺少y")?)? - offset[1],
                    number(f.get("z").context("缺少z")?)? - offset[2],
                ];
                ensure!(tiles.insert(p, tile.clone()).is_none(), "重复方块实体");
            }
            let mut data = Vec::new();
            let mut positions = Vec::new();
            let mut palette_lookup = BTreeMap::<String, usize>::new();
            for (i, p) in indices.iter().copied().enumerate() {
                if i & 4095 == 0 {
                    cancelled(cancel)?
                };
                let state = &region.block_state_palette[p];
                if !composition_scene::mask_contains(presence, i) {
                    continue;
                }
                let pos = [
                    (i % size[0] as usize) as i32,
                    (i / (size[0] as usize * size[2] as usize)) as i32,
                    ((i / size[0] as usize) % size[2] as usize) as i32,
                ];
                let tile = tiles.remove(&pos);
                let block = Value::Compound(Fields::from([
                    ("state".into(), typed_nbt_value(state)?),
                    ("serializer".into(), Value::Int(i32::from(tile.is_some()))),
                    (
                        "data".into(),
                        Value::Compound(if let Some(tile) = tile {
                            Fields::from([("data".into(), tile)])
                        } else {
                            Fields::new()
                        }),
                    ),
                ]));
                let key = crate::version_payload_migration::write_snbt_shared(&block)?;
                let next = data.len();
                let index = *palette_lookup.entry(key).or_insert_with(|| {
                    data.push(block);
                    next
                });
                ensure!(index < 0x1000000, "BG1数据表超过24位");
                let packed = ((index as u64) << 40)
                    | ((pos[0] as u64) << 24)
                    | ((pos[1] as u64) << 16)
                    | (pos[2] as u64);
                positions.push(Value::Long(packed as i64));
            }
            ensure!(tiles.is_empty(), "存在不可写出的方块实体");
            let mut header = scene_extras(scene)
                .get("BuildingGadgetsHeader")
                .and_then(|v| {
                    if let Value::String(v) = v {
                        serde_json::from_str::<serde_json::Value>(v).ok()
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| serde_json::json!({"version":"2.1.0"}));
            let mc_version = scene
                .data_version()
                .and_then(|v| {
                    crate::world_export::profiles()
                        .ok()?
                        .into_iter()
                        .find(|p| p.data_version == Some(v))
                        .map(|p| p.version)
                })
                .or_else(|| {
                    header
                        .get("mc_version")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
                .context("BG1写出需要真实mc_version，不能猜测")?;
            header["mc_version"] = serde_json::Value::String(mc_version);
            header["name"] =
                serde_json::Value::String(scene.root.metadata.name.clone().unwrap_or_default());
            header["bounding_box"] = serde_json::json!({"min_x":0,"min_y":0,"min_z":0,"max_x":size[0]-1,"max_y":size[1]-1,"max_z":size[2]-1});
            let bounds = Value::Compound(Fields::from([
                ("minX".into(), Value::Int(0)),
                ("minY".into(), Value::Int(0)),
                ("minZ".into(), Value::Int(0)),
                ("maxX".into(), Value::Int(size[0] - 1)),
                ("maxY".into(), Value::Int(size[1] - 1)),
                ("maxZ".into(), Value::Int(size[2] - 1)),
            ]));
            let nbt = Value::Compound(Fields::from([
                ("data".into(), Value::List(data)),
                ("pos".into(), Value::List(positions)),
                (
                    "serializer".into(),
                    Value::List(vec![
                        Value::String("buildinggadgets:dummy_serializer".into()),
                        Value::String("buildinggadgets:nbt_tile_data_serializer".into()),
                    ]),
                ),
                (
                    "header".into(),
                    Value::Compound(Fields::from([("bounds".into(), bounds)])),
                ),
            ]));
            Ok(serde_json::to_vec_pretty(
                &serde_json::json!({"header":header,"body":STANDARD.encode(gzip(&nbt)?)}),
            )?)
        }
        FormatId::Litematic => unreachable!(),
    }
}
// Use the production binary serializer for typed payloads. The separate
// in-memory Value serializer in fastnbt 2.6.1 panics on Option::None, whereas
// the binary serializer correctly omits absent entity id/Id fields and keeps
// all typed arrays. The envelope also supports list and scalar roots.
fn typed_nbt_value<T: Serialize + ?Sized>(value: &T) -> Result<Value> {
    #[derive(Serialize)]
    struct Envelope<'a, T: ?Sized> {
        value: &'a T,
    }
    let encoded = fastnbt::to_bytes(&Envelope { value })?;
    let Value::Compound(mut fields) = fastnbt::from_bytes(&encoded)? else {
        bail!("NBT编码器未生成Compound")
    };
    fields.remove("value").context("NBT值编码失败")
}

fn gzip(value: &Value) -> Result<Vec<u8>> {
    let mut writer = GzEncoder::new(Vec::new(), Compression::default());
    fastnbt::to_writer(&mut writer, &OrderedNbt(value))?;
    Ok(writer.finish()?)
}
// Compound order is not semantic. Stable serialization prevents the same
// import from producing a different artifact merely because HashMap's random
// seed changed between processes; typed arrays retain their native NBT tags.
struct OrderedNbt<'a>(&'a Value);
impl Serialize for OrderedNbt<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self.0 {
            Value::Compound(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields.iter().collect::<BTreeMap<_, _>>() {
                    map.serialize_entry(key, &OrderedNbt(value))?;
                }
                map.end()
            }
            Value::List(values) => {
                let mut list = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    list.serialize_element(&OrderedNbt(value))?;
                }
                list.end()
            }
            value => value.serialize(serializer),
        }
    }
}
fn scene_extras(scene: &SceneDocument) -> Fields {
    compound(&scene.raw)
        .ok()
        .and_then(|f| f.get(SOURCE_EXTRAS))
        .and_then(|v| compound(v).ok())
        .cloned()
        .unwrap_or_default()
}
fn bg1_minecraft_version(scene: &SceneDocument) -> Option<String> {
    scene
        .data_version()
        .and_then(|v| {
            crate::world_export::profiles()
                .ok()?
                .into_iter()
                .find(|p| p.data_version == Some(v))
                .map(|p| p.version)
        })
        .or_else(|| match scene_extras(scene).get("BuildingGadgetsHeader") {
            Some(Value::String(header)) => serde_json::from_str::<serde_json::Value>(header)
                .ok()?
                .get("mc_version")?
                .as_str()
                .map(str::to_owned),
            _ => None,
        })
}

/// Data-worker entrypoint. Host routes intent and paths; it does not decode,
/// flatten, serialize, choose a palette, or implement format capability rules.
pub fn execute(input: &serde_json::Value, cancel: &AtomicBool) -> Result<serde_json::Value> {
    let action = input
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("inspect");
    if action == "registry" {
        return Ok(
            serde_json::json!({"formats":registry(),"sourceVersions":crate::world_export::profiles()?.into_iter().map(|profile|serde_json::json!({"version":profile.version,"dataVersion":profile.data_version})).collect::<Vec<_>>()}),
        );
    }
    let source_path = input
        .get("input")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty());
    let clipboard = input.get("clipboardText").and_then(|v| v.as_str());
    ensure!(
        source_path.is_some() ^ clipboard.is_some(),
        "请提供一个输入文件或一份模板剪贴板文本，不能同时提供"
    );
    if let Some(text) = clipboard {
        ensure!(
            !text.trim().is_empty() && text.len() <= MAX_TEXT,
            "模板剪贴板为空或超过64MiB容量"
        );
    }
    let source = source_path.unwrap_or("clipboard");
    let explicit_version = match input
        .get("sourceDataVersion")
        .filter(|value| !value.is_null())
    {
        Some(value) => Some(i32::try_from(
            value.as_i64().context("源DataVersion必须是整数")?,
        )?),
        None => None,
    };
    let storage = input
        .get("storageDir")
        .or_else(|| input.get("cacheDirectory"))
        .and_then(|value| value.as_str())
        .map(Path::new);
    let display_name = source_path
        .and_then(|source| Path::new(source).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("Clipboard");
    if matches!(action, "runtimeInput" | "libraryInspect")
        && explicit_version.is_none()
        && clipboard.is_none()
        && is_native_litematic(Path::new(source))?
    {
        cancelled(cancel)?;
        return Ok(
            serde_json::json!({"sourcePath":source_path,"originalSourcePath":source_path,"sourceDisplayName":display_name,"snapshotPath":source,"format":FormatId::Litematic,"nativePassthrough":true}),
        );
    }
    let mut scene = if let Some(text) = clipboard {
        decode_bytes(text.as_bytes(), "Clipboard", cancel)?
    } else {
        decode(Path::new(source), cancel)?
    };
    apply_source_version(
        &mut scene,
        storage,
        explicit_version,
        !matches!(action, "inspect" | "libraryInspect"),
        cancel,
    )?;
    let details = report(&scene, cancel)?;
    let supported = capabilities(&scene);
    let result: Result<serde_json::Value> = match action {
        "inspect" | "libraryInspect" => Ok(
            serde_json::json!({"sourcePath":source_path,"sourceIdentity":scene.content_id,"report":details,"capabilities":supported}),
        ),
        "encodeText" => {
            let format: FormatId = serde_json::from_value(
                input
                    .get("format")
                    .cloned()
                    .context("共享字符串缺少目标格式")?,
            )?;
            ensure!(
                matches!(
                    format,
                    FormatId::BuildingGadgetsLegacy | FormatId::BuildingGadgets2
                ),
                "只有实际JSON模板共享格式支持复制字符串"
            );
            let bytes = encode_bytes(&scene, format, cancel)?;
            ensure!(bytes.len() <= MAX_TEXT, "模板共享字符串超过64MiB容量");
            cancelled(cancel)?;
            let text = String::from_utf8(bytes).context("模板共享字符串不是UTF-8")?;
            Ok(
                serde_json::json!({"format":format,"clipboardText":text,"mediaType":"application/json","sourceIdentity":scene.content_id,"report":details}),
            )
        }
        "decodeToSnapshot" | "canonicalize" | "runtimeInput" | "import" => {
            // Source identity stays the original bytes, separately from the
            // canonical artifact identity consumed by every existing renderer.
            if clipboard.is_none()
                && scene.source_format == "litematic"
                && is_native_litematic(Path::new(source))?
            {
                return Ok(
                    serde_json::json!({"sourcePath":source_path,"originalSourcePath":source_path,"sourceDisplayName":display_name,"sourceIdentity":scene.content_id,"snapshotPath":source,"snapshotIdentity":scene.content_id,"report":details,"capabilities":supported}),
                );
            }
            ensure!(
                scene.data_version().is_some(),
                "源模板没有声明Minecraft版本，请先选择真实源版本；未借用当前资源版本冒充"
            );
            let output = if let Some(output) = input.get("output").and_then(|v| v.as_str()) {
                std::path::PathBuf::from(output)
            } else {
                let cache = storage.context("规范化导入需要宿主提供既有用户缓存目录")?;
                // Revision 3 keeps content identity in directories, not the
                // filename shown throughout the existing application. Source
                // bytes and canonical semantics remain separate identities.
                let canonical = format!(
                    "{:x}",
                    Sha256::digest(format!(
                        "format-v3:{}",
                        composition_scene::content_identity(&scene.raw)
                    ))
                );
                let directory = cache
                    .join("format-imports")
                    .join(&scene.content_id)
                    .join(canonical);
                fs::create_dir_all(&directory)?;
                directory.join(format!(
                    "import-{}.litematic",
                    safe_source_stem(source_path)
                ))
            };
            ensure!(Path::new(source) != output, "规范化快照不能覆盖源文件");
            let bytes = encode_bytes(&scene, FormatId::Litematic, cancel)?;
            let identity = format!("{:x}", Sha256::digest(&bytes));
            if output.exists() {
                // A content-addressed entry may be reused only when its actual
                // bytes match; a corrupt cache is reported rather than hidden.
                let file = File::open(&output)?;
                let actual = read_bounded(file, MAX_INPUT, cancel)?;
                ensure!(
                    format!("{:x}", Sha256::digest(&actual)) == identity,
                    "规范化缓存读取内容不匹配，请清除此失效条目后重试"
                );
            } else {
                let parent = output.parent().unwrap_or_else(|| Path::new("."));
                let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
                temporary.write_all(&bytes)?;
                temporary.as_file().sync_all()?;
                cancelled(cancel)?;
                temporary.persist_noclobber(&output).map_err(|e| e.error)?;
            }
            Ok(
                serde_json::json!({"sourcePath":source_path,"sourceIdentity":scene.content_id,"snapshotPath":output,"snapshotIdentity":identity,"report":details,"capabilities":supported}),
            )
        }
        "encode" | "export" | "convert" => {
            let format: FormatId =
                serde_json::from_value(input.get("format").cloned().context("写出缺少目标格式")?)?;
            let output = input
                .get("output")
                .and_then(|v| v.as_str())
                .context("写出缺少目标路径")?;
            ensure!(
                Path::new(output) != Path::new(source),
                "格式转换不能覆盖输入文件"
            );
            let result = encode(&scene, format, Path::new(output), cancel)?;
            Ok(
                serde_json::json!({"sourcePath":source_path,"sourceIdentity":scene.content_id,"output":output,"report":result}),
            )
        }
        _ => bail!("未知格式操作{action}"),
    };
    let mut result = result?;
    result["originalSourcePath"] = serde_json::json!(source_path);
    result["sourceDisplayName"] = serde_json::json!(display_name);
    result["cacheRevision"] = serde_json::json!(3);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn control() -> AtomicBool {
        AtomicBool::new(false)
    }
    fn sample() -> Value {
        Value::Compound(Fields::from([
            ("DataVersion".into(), Value::Int(2584)),
            ("size".into(), int_list([2, 2, 3])),
            (
                "palette".into(),
                Value::List(vec![
                    typed_nbt_value(&blank_state("minecraft:stone")).unwrap(),
                    typed_nbt_value(&BlockStateNbt {
                        name: "example:chest".into(),
                        properties: [("facing".into(), "east".into())].into(),
                    })
                    .unwrap(),
                ]),
            ),
            (
                "blocks".into(),
                Value::List(vec![
                    Value::Compound(Fields::from([
                        ("pos".into(), int_list([0, 0, 0])),
                        ("state".into(), Value::Int(0)),
                    ])),
                    Value::Compound(Fields::from([
                        ("pos".into(), int_list([1, 1, 2])),
                        ("state".into(), Value::Int(1)),
                        (
                            "nbt".into(),
                            Value::Compound(Fields::from([
                                ("id".into(), Value::String("example:chest".into())),
                                (
                                    "Items".into(),
                                    Value::List(vec![Value::Compound(Fields::from([
                                        ("id".into(), Value::String("minecraft:diamond".into())),
                                        ("Count".into(), Value::Byte(3)),
                                        ("Slot".into(), Value::Byte(1)),
                                    ]))]),
                                ),
                            ])),
                        ),
                    ])),
                ]),
            ),
            (
                "entities".into(),
                Value::List(vec![Value::Compound(Fields::from([
                    ("pos".into(), double_list([0.2, 1.25, 1.7])),
                    ("blockPos".into(), int_list([1, 1, 1])),
                    (
                        "nbt".into(),
                        Value::Compound(Fields::from([
                            ("id".into(), Value::String("minecraft:painting".into())),
                            ("variant".into(), Value::String("minecraft:kebab".into())),
                            (
                                "Rotation".into(),
                                Value::List(vec![Value::Float(90.), Value::Float(0.)]),
                            ),
                        ])),
                    ),
                ]))]),
            ),
            ("example:opaque_extension".into(), Value::Long(778899)),
        ]))
    }
    fn semantic_cells(scene: &SceneDocument) -> BTreeMap<[i32; 3], String> {
        let mut out = BTreeMap::new();
        for region in scene.root.regions.values() {
            let size = nbt::region_volume(&region.size).unwrap();
            nbt::for_each_palette_index(
                &region.block_states,
                size,
                nbt::bits_for_palette(region.block_state_palette.len()),
                region.block_state_palette.len(),
                |i, p| {
                    let width = region.size.x.unsigned_abs() as usize;
                    let length = region.size.z.unsigned_abs() as usize;
                    let (x, y, z) = nbt::storage_to_region_coords(
                        i % width,
                        i / (width * length),
                        (i / width) % length,
                        &region.size,
                    );
                    let state = &region.block_state_palette[p];
                    out.insert(
                        [
                            x + region.position.x,
                            y + region.position.y,
                            z + region.position.z,
                        ],
                        format!("{}{:?}", state.name, state.properties),
                    );
                    Ok(())
                },
            )
            .unwrap();
        }
        out
    }
    #[test]
    fn structure_raw_gzip_and_unknown_namespaces_roundtrip() {
        let cancel = control();
        let raw = sample();
        let bytes = fastnbt::to_bytes(&raw).unwrap();
        assert_eq!(sniff(&bytes, &cancel).unwrap(), FormatId::Structure);
        let scene = decode_bytes(&bytes, "test", &cancel).unwrap();
        let report = report(&scene, &cancel).unwrap();
        assert_eq!(report.blocks, 2);
        assert_eq!(report.entities, 1);
        assert_eq!(report.block_entities, 1);
        assert!(report.namespaces.contains(&"example".into()));
        for format in [FormatId::Structure, FormatId::Create, FormatId::AnvilCraft] {
            let encoded = encode_bytes(&scene, format, &cancel).unwrap();
            let again = decode_bytes(&encoded, "again", &cancel).unwrap();
            assert_eq!(semantic_cells(&scene), semantic_cells(&again));
            assert_eq!(
                scene.root.regions["Imported"].tile_entities,
                again.root.regions["Imported"].tile_entities
            );
            assert_eq!(
                typed_nbt_value(&scene.root.regions["Imported"].entities).unwrap(),
                typed_nbt_value(&again.root.regions["Imported"].entities).unwrap()
            );
            let source = compound(&read_nbt(&encoded, &cancel).unwrap())
                .unwrap()
                .clone();
            assert_eq!(source["example:opaque_extension"], Value::Long(778899));
        }
    }
    #[test]
    fn rejects_palette_corruption_duplicates_and_unselected_random_variants() {
        let cancel = control();
        let mut raw = sample();
        let Value::Compound(f) = &mut raw else {
            unreachable!()
        };
        let Value::List(blocks) = f.get_mut("blocks").unwrap() else {
            unreachable!()
        };
        let Value::Compound(first) = &mut blocks[0] else {
            unreachable!()
        };
        first.insert("state".into(), Value::Int(7));
        assert!(
            decode_bytes(&gzip(&raw).unwrap(), "bad", &cancel)
                .unwrap_err()
                .to_string()
                .contains("索引")
        );
        let mut raw = sample();
        let Value::Compound(f) = &mut raw else {
            unreachable!()
        };
        let palette = f.remove("palette").unwrap();
        f.insert(
            "palettes".into(),
            Value::List(vec![palette.clone(), palette]),
        );
        assert!(
            decode_bytes(&gzip(&raw).unwrap(), "bad", &cancel)
                .unwrap_err()
                .to_string()
                .contains("随机调色板")
        );
        let mut raw = sample();
        let Value::Compound(f) = &mut raw else {
            unreachable!()
        };
        let Value::List(blocks) = f.get_mut("blocks").unwrap() else {
            unreachable!()
        };
        blocks.push(blocks[0].clone());
        assert!(
            decode_bytes(&gzip(&raw).unwrap(), "bad", &cancel)
                .unwrap_err()
                .to_string()
                .contains("重复方块")
        );
    }
    #[test]
    fn absent_cells_are_air_for_consumers_but_not_explicit_air_or_real_structure_void() {
        let cancel = control();
        let raw = Value::Compound(Fields::from([
            ("DataVersion".into(), Value::Int(2584)),
            ("size".into(), int_list([3, 1, 1])),
            (
                "palette".into(),
                Value::List(vec![
                    typed_nbt_value(&blank_state("minecraft:air")).unwrap(),
                    typed_nbt_value(&blank_state("minecraft:structure_void")).unwrap(),
                ]),
            ),
            (
                "blocks".into(),
                Value::List(vec![
                    Value::Compound(Fields::from([
                        ("pos".into(), int_list([0, 0, 0])),
                        ("state".into(), Value::Int(0)),
                    ])),
                    Value::Compound(Fields::from([
                        ("pos".into(), int_list([1, 0, 0])),
                        ("state".into(), Value::Int(1)),
                    ])),
                ]),
            ),
            ("entities".into(), Value::List(vec![])),
        ]));
        let scene = decode_bytes(&gzip(&raw).unwrap(), "presence", &cancel).unwrap();
        let mask = scene.presence_mask("Imported").unwrap();
        assert!(composition_scene::mask_contains(mask, 0));
        assert!(composition_scene::mask_contains(mask, 1));
        assert!(!composition_scene::mask_contains(mask, 2));
        assert_eq!(
            scene.root.regions["Imported"].block_state_palette[0].name,
            "minecraft:air"
        );
        assert_eq!(scene.root.metadata.total_blocks, Some(1));
        assert!(
            encode_bytes(&scene, FormatId::BuildingGadgets2, &cancel)
                .unwrap_err()
                .to_string()
                .contains("缺格")
        );
        let encoded = encode_bytes(&scene, FormatId::Structure, &cancel).unwrap();
        let raw = read_nbt(&encoded, &cancel).unwrap();
        assert_eq!(list(compound(&raw).unwrap(), "blocks").unwrap().len(), 2);
        let again = decode_bytes(&encoded, "again", &cancel).unwrap();
        assert_eq!(semantic_cells(&scene), semantic_cells(&again));
        assert_eq!(
            scene.presence_mask("Imported").unwrap(),
            again.presence_mask("Imported").unwrap()
        );
    }
    #[test]
    fn anvil_scheduled_ticks_have_one_region_owned_position_representation() {
        let cancel = control();
        let mut raw = sample();
        let tick = Value::Compound(Fields::from([
            ("pos".into(), Value::IntArray(IntArray::new(vec![1, 1, 2]))),
            ("type".into(), Value::String("minecraft:water".into())),
            ("delay".into(), Value::Int(12)),
            ("priority".into(), Value::Int(-1)),
            ("fluid".into(), Value::Byte(1)),
            ("order".into(), Value::Long(99)),
        ]));
        let Value::Compound(fields) = &mut raw else {
            unreachable!()
        };
        fields.insert(
            "anvilcraft:scheduled_ticks".into(),
            Value::List(vec![tick.clone()]),
        );
        let scene = decode_bytes(&gzip(&raw).unwrap(), "ticks", &cancel).unwrap();
        assert!(!scene_extras(&scene).contains_key("anvilcraft:scheduled_ticks"));
        let Value::Compound(region) = scene.region_raw("Imported").unwrap() else {
            unreachable!()
        };
        assert_eq!(region[ANVIL_TICKS], Value::List(vec![tick.clone()]));
        let output = read_nbt(
            &encode_bytes(&scene, FormatId::AnvilCraft, &cancel).unwrap(),
            &cancel,
        )
        .unwrap();
        assert_eq!(
            compound(&output).unwrap()["anvilcraft:scheduled_ticks"],
            Value::List(vec![tick])
        );
    }
    #[test]
    fn bg2_preserves_negative_origin_and_between_closed_order() {
        let cancel = control();
        let palette = vec![
            blank_state("minecraft:stone"),
            blank_state("minecraft:glass"),
        ];
        let original = (0..12).map(|i| i % 2).collect::<Vec<_>>();
        let scene = make_scene(
            "order",
            [2, 2, 3],
            [-9, -2, -4],
            palette,
            original.clone(),
            vec![],
            vec![],
            Some(2584),
            FormatId::Litematic,
            Fields::new(),
        )
        .unwrap();
        let encoded = encode_bytes(&scene, FormatId::BuildingGadgets2, &cancel).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        let nbt = crate::version_payload_migration::read_snbt_bounded(
            json["statePosArrayList"].as_str().unwrap(),
            MAX_TEXT,
        )
        .unwrap();
        let f = compound(&nbt).unwrap();
        let Value::IntArray(actual) = &f["statelist"] else {
            unreachable!()
        };
        for z in 0..3 {
            for y in 0..2 {
                for x in 0..2 {
                    assert_eq!(
                        actual[x + 2 * (y + 2 * z)] as usize,
                        original[x + 2 * (z + 3 * y)]
                    );
                }
            }
        }
        let again = decode_bytes(&encoded, "again", &cancel).unwrap();
        assert_eq!(semantic_cells(&scene), semantic_cells(&again));
        assert_eq!(again.data_version(), None);
        let mut data = compound(&nbt).unwrap().clone();
        data.insert("startpos".into(), pos_tag([-9, -2, -4]));
        data.insert("endpos".into(), pos_tag([-8, -1, -2]));
        let json = serde_json::json!({"name":"negative","statePosArrayList":crate::version_payload_migration::write_snbt_shared(&Value::Compound(data)).unwrap()});
        let negative = decode_bytes(&serde_json::to_vec(&json).unwrap(), "n", &cancel).unwrap();
        assert_eq!(negative.root.regions["Imported"].position.x, -9);
        assert_eq!(negative.root.regions["Imported"].position.y, -2);
    }
    #[test]
    fn bg1_serializer_roundtrip_preserves_container_nbt() {
        let cancel = control();
        let mut raw = sample();
        let Value::Compound(f) = &mut raw else {
            unreachable!()
        };
        f.insert("entities".into(), Value::List(vec![]));
        let scene = decode_bytes(&gzip(&raw).unwrap(), "legacy", &cancel).unwrap();
        let encoded = encode_bytes(&scene, FormatId::BuildingGadgetsLegacy, &cancel).unwrap();
        let again = decode_bytes(&encoded, "again", &cancel).unwrap();
        assert_eq!(semantic_cells(&scene), semantic_cells(&again));
        assert_eq!(
            scene.root.regions["Imported"].tile_entities,
            again.root.regions["Imported"].tile_entities
        );
        assert_eq!(again.data_version(), Some(2584));
    }
    #[test]
    fn invalid_cancelled_and_unsupported_writes_never_drop_source_data() {
        let cancelled = AtomicBool::new(true);
        assert!(
            sniff(b"not nbt", &cancelled)
                .unwrap_err()
                .to_string()
                .contains("取消")
        );
        let cancel = control();
        let scene = decode_bytes(&gzip(&sample()).unwrap(), "e", &cancel).unwrap();
        assert!(
            encode_bytes(&scene, FormatId::BuildingGadgets2, &cancel)
                .unwrap_err()
                .to_string()
                .contains("普通实体")
        );
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("out.nbt");
        assert!(encode(&scene, FormatId::Structure, &path, &cancelled).is_err());
        assert!(!path.exists());
        fs::write(&path, b"existing").unwrap();
        assert!(encode(&scene, FormatId::Structure, &path, &cancel).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"existing");
        assert!(sniff(b"{\"size\":3}", &cancel).is_err());
    }
    #[test]
    fn production_reader_and_native_runtime_passthrough_share_the_adapter() {
        let cancel = control();
        let temp = tempfile::tempdir().unwrap();
        let structure = temp.path().join("not-a-litematic.bin");
        fs::write(&structure, gzip(&sample()).unwrap()).unwrap();
        assert!(!is_native_litematic(&structure).unwrap());
        let root = nbt::load_litematic_root(&structure).unwrap();
        assert_eq!(root.metadata.total_blocks, Some(2));
        assert_eq!(root.regions["Imported"].entities.len(), 1);
        let scene = decode(&structure, &cancel).unwrap();
        let native = temp.path().join("also-not-extension.data");
        fs::write(
            &native,
            encode_bytes(&scene, FormatId::Litematic, &cancel).unwrap(),
        )
        .unwrap();
        assert!(is_native_litematic(&native).unwrap());
        let mut preview = scene.raw_litematic().unwrap();
        if let Value::Compound(root) = &mut preview {
            if let Some(Value::Compound(metadata)) = root.get_mut("Metadata") {
                metadata.insert(
                    "PreviewImageData".into(),
                    Value::IntArray(IntArray::new(vec![0; 320 * 240 + 1])),
                );
            }
        }
        let with_preview = temp.path().join("embedded-preview.data");
        fs::write(&with_preview, gzip(&preview).unwrap()).unwrap();
        assert!(
            is_native_litematic(&with_preview).unwrap(),
            "ordinary embedded preview must not defeat native header passthrough"
        );
        let result = execute(
            &serde_json::json!({"action":"runtimeInput","input":native}),
            &cancel,
        )
        .unwrap();
        assert_eq!(result["nativePassthrough"], true);
        assert_eq!(result["snapshotPath"], serde_json::json!(native));
        assert!(result.get("report").is_none());
    }
    #[test]
    fn clipboard_import_requires_real_source_version_and_reuses_identical_snapshot() {
        let cancel = control();
        let temp = tempfile::tempdir().unwrap();
        let scene = make_scene(
            "clipboard",
            [2, 1, 1],
            [0; 3],
            vec![blank_state("minecraft:stone")],
            vec![0, 0],
            vec![],
            vec![],
            Some(2584),
            FormatId::Litematic,
            Fields::new(),
        )
        .unwrap();
        let bytes = encode_bytes(&scene, FormatId::BuildingGadgets2, &cancel).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        let mut input = serde_json::json!({"action":"canonicalize","clipboardText":text,"storageDir":temp.path()});
        assert!(
            execute(&input, &cancel)
                .unwrap_err()
                .to_string()
                .contains("选择真实源版本")
        );
        input["sourceDataVersion"] = serde_json::json!(2584);
        let first = execute(&input, &cancel).unwrap();
        let second = execute(&input, &cancel).unwrap();
        assert_eq!(first["snapshotIdentity"], second["snapshotIdentity"]);
        assert_eq!(first["snapshotPath"], second["snapshotPath"]);
        assert!(first["sourcePath"].is_null());
        assert_eq!(
            first["sourceIdentity"],
            serde_json::json!(format!("{:x}", Sha256::digest(bytes)))
        );
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        input["input"] = serde_json::json!("also-a-file");
        assert!(execute(&input, &cancel).is_err());
    }

    #[test]
    fn source_version_choice_is_content_bound_persistent_and_never_overrides_a_declaration() {
        let cancel = control();
        let storage = tempfile::tempdir().unwrap();
        let inputs = tempfile::tempdir().unwrap();
        let path = inputs.path().join("同一路径+template.bgcp");
        let scene = make_scene(
            "choice",
            [1, 1, 1],
            [0; 3],
            vec![blank_state("minecraft:stone")],
            vec![0],
            vec![],
            vec![],
            Some(2584),
            FormatId::Litematic,
            Fields::new(),
        )
        .unwrap();
        let original = encode_bytes(&scene, FormatId::BuildingGadgets2, &cancel).unwrap();
        fs::write(&path, &original).unwrap();

        let inspect_input = serde_json::json!({"action":"inspect","input":path,"storageDir":storage.path(),"sourceDataVersion":2584});
        let inspected = execute(&inspect_input, &cancel).unwrap();
        assert_eq!(inspected["report"]["dataVersion"], 2584);
        assert_eq!(
            fs::read_dir(storage.path()).unwrap().count(),
            0,
            "inspect must not persist a choice"
        );
        assert!(decode_for_import(&path, &cancel, storage.path(), None).is_err());
        let first = decode_for_import(&path, &cancel, storage.path(), Some(2584)).unwrap();
        assert_eq!(first.data_version(), Some(2584));
        drop(first);
        let reopened = decode_for_import(&path, &cancel, storage.path(), None).unwrap();
        assert_eq!(reopened.data_version(), Some(2584));
        assert_eq!(
            reopened.content_id,
            format!("{:x}", Sha256::digest(&original))
        );

        let input =
            serde_json::json!({"action":"runtimeInput","input":path,"storageDir":storage.path()});
        let snapshot = execute(&input, &cancel).unwrap();
        let snapshot_path = Path::new(snapshot["snapshotPath"].as_str().unwrap());
        assert_eq!(
            snapshot_path.file_name().unwrap().to_str().unwrap(),
            "import-同一路径_template.litematic"
        );
        assert_eq!(snapshot["sourceDisplayName"], "同一路径+template.bgcp");
        assert_eq!(
            snapshot["sourceIdentity"],
            format!("{:x}", Sha256::digest(&original))
        );
        assert_eq!(
            snapshot["snapshotIdentity"],
            format!("{:x}", Sha256::digest(fs::read(snapshot_path).unwrap()))
        );
        assert_eq!(
            snapshot["snapshotPath"],
            execute(&input, &cancel).unwrap()["snapshotPath"]
        );

        // A changed template at the exact same path has no inherited choice.
        let mut changed: serde_json::Value = serde_json::from_slice(&original).unwrap();
        changed["name"] = serde_json::json!("changed content");
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(
            decode_for_import(&path, &cancel, storage.path(), None)
                .unwrap_err()
                .to_string()
                .contains("选择真实源版本")
        );
        assert!(decode_for_import(&path, &cancel, storage.path(), Some(-1)).is_err());

        // A real declaration is authoritative even when the requested version
        // is another registered profile. Missing/invalid input is never saved.
        let declared = inputs.path().join("declared.nbt");
        fs::write(&declared, gzip(&sample()).unwrap()).unwrap();
        let other = crate::world_export::profiles()
            .unwrap()
            .into_iter()
            .filter_map(|profile| profile.data_version)
            .find(|version| *version != 2584)
            .unwrap();
        assert!(
            decode_for_import(&declared, &cancel, storage.path(), Some(other))
                .unwrap_err()
                .to_string()
                .contains("与源文件不一致")
        );
        assert_eq!(
            decode_for_import(&declared, &cancel, storage.path(), None)
                .unwrap()
                .data_version(),
            Some(2584)
        );
        let cancelled = AtomicBool::new(true);
        assert!(decode_for_import(&path, &cancelled, storage.path(), Some(2584)).is_err());
    }
    #[test]
    #[ignore = "Requires the four actual upstream/user-created sample files in NOVA_FORMAT_SAMPLE_DIR"]
    fn actual_upstream_samples_roundtrip() {
        let directory = std::env::var_os("NOVA_FORMAT_SAMPLE_DIR")
            .map(std::path::PathBuf::from)
            .expect("NOVA_FORMAT_SAMPLE_DIR is required, not a synthetic fallback");
        let cancel = control();
        // Structure consumers address block entities by position, not by the
        // order of the blocks list. The encoder emits storage order (YZX),
        // while Create's source list is not required to use that order. Keep
        // every typed NBT field in the comparison; canonicalize only lookup.
        fn block_entities_by_position(scene: &SceneDocument) -> Result<BTreeMap<[i64; 3], Value>> {
            let mut values = BTreeMap::new();
            for region in scene.root.regions.values() {
                for tile in &region.tile_entities {
                    let fields = compound(tile)?;
                    let point = [
                        i64::from(region.position.x)
                            + i64::from(number(fields.get("x").context("block entity has no x")?)?),
                        i64::from(region.position.y)
                            + i64::from(number(fields.get("y").context("block entity has no y")?)?),
                        i64::from(region.position.z)
                            + i64::from(number(fields.get("z").context("block entity has no z")?)?),
                    ];
                    ensure!(
                        values.insert(point, tile.clone()).is_none(),
                        "duplicate block entity at {point:?}"
                    );
                }
            }
            Ok(values)
        }
        let mut failures = Vec::new();
        for (file, format) in [
            ("create-basin.nbt", FormatId::Create),
            ("anvil-drain.nbt", FormatId::AnvilCraft),
            ("bg1-cable-base.json", FormatId::BuildingGadgetsLegacy),
            ("bg2-cheap-basement.json", FormatId::BuildingGadgets2),
        ] {
            let result = std::panic::catch_unwind(|| -> Result<()> {
                let scene = decode(&directory.join(file), &cancel)?;
                let before = report(&scene, &cancel)?;
                let bytes = encode_bytes(&scene, format, &cancel)?;
                let after = decode_bytes(&bytes, file, &cancel)?;
                let after_report = report(&after, &cancel)?;
                ensure!(
                    semantic_cells(&scene) == semantic_cells(&after),
                    "complete block-state/position mismatch"
                );
                ensure!(
                    before.entities == after_report.entities,
                    "entity count mismatch"
                );
                ensure!(
                    before.block_entities == after_report.block_entities,
                    "block entity count mismatch"
                );
                ensure!(
                    before.data_version == after_report.data_version,
                    "source data version mismatch"
                );
                ensure!(
                    typed_nbt_value(&scene.root.regions["Imported"].entities)?
                        == typed_nbt_value(&after.root.regions["Imported"].entities)?,
                    "complete typed entity NBT mismatch"
                );
                ensure!(
                    block_entities_by_position(&scene)? == block_entities_by_position(&after)?,
                    "complete typed block entity NBT/position mismatch"
                );
                println!(
                    "{file}: blocks={} entities={} block_entities={} data_version={:?}; structural roundtrip OK; game consumer pending manual verification",
                    before.blocks, before.entities, before.block_entities, before.data_version
                );
                Ok(())
            });
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => failures.push(format!("{file}: {error:#}")),
                Err(panic) => failures.push(format!(
                    "{file}: panic: {}",
                    panic
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| panic.downcast_ref::<&str>().copied())
                        .unwrap_or("non-string panic")
                )),
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
