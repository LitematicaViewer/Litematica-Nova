//! Explicit, lossless-envelope conversion of static Litematica block data.
//! Unsupported entity/item/tick schemas block writing instead of changing
//! DataVersion on partly converted data. No Java oracle is invoked at runtime.
use crate::block_database::{Registry as BlockRegistry, database};
use crate::nbt::{
    BigEndianLongArray, Vec3i, bits_for_palette, choose_palette_packing, region_volume,
};
use anyhow::{Context, Result, bail};
use fastnbt::Value;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};

pub(crate) fn target_has_item(data_version: i32, id: &str) -> Result<bool> {
    let registry = database()?.registry_by_data_version(data_version)?;
    Ok(registry.items.contains(id))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConversionRequest {
    pub target_version: String,
    #[serde(default)]
    pub replacements: BTreeMap<String, String>,
    pub expected_source_sha256: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub selection: Option<ConversionSelection>,
    #[serde(default)]
    pub property_edit: Option<ConversionPropertyEdit>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConversionSelection {
    pub source_state: String,
    pub target_block_id: String,
    #[serde(default)]
    pub bulk: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConversionPropertyEdit {
    pub source_state: String,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionEntry {
    pub source_state: String,
    pub target_state: Option<String>,
    pub count: u64,
    pub reason: String,
    pub valid: bool,
    pub source_name: String,
    pub source_icon_state: String,
    pub target_name: Option<String>,
    pub category: String,
    pub source_block_id: String,
    pub source_icon_id: String,
    pub source_properties: BTreeMap<String, String>,
    pub target_block_id: Option<String>,
    pub target_icon_id: Option<String>,
    pub target_properties: BTreeMap<String, String>,
    pub target_property_options: BTreeMap<String, Vec<String>>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionPreview {
    pub source_sha256: String,
    pub source_data_version: i32,
    pub source_version: String,
    pub target_version: String,
    pub target_data_version: i32,
    pub entries: Vec<ConversionEntry>,
    pub effective_replacements: BTreeMap<String, String>,
    pub blocked_payloads: Vec<String>,
    pub payload_issues: Vec<ConversionPayloadIssue>,
    pub payload_changes: Vec<String>,
    pub writable: bool,
    pub changed_block_count: u64,
    pub checked_entity_count: u64,
    pub checked_block_entity_count: u64,
    pub checked_entity_types: BTreeMap<String, u64>,
    pub checked_block_entity_types: BTreeMap<String, u64>,
    pub checked_item_stack_count: u64,
    pub changed_entity_count: u64,
    pub changed_block_entity_count: u64,
    pub output_path: Option<String>,
    pub output_extension: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionPayloadIssue {
    pub kind: String,
    pub type_name: String,
    pub count: usize,
    pub message: String,
    pub domain: String,
    pub source_id: String,
    pub source_data_version: i32,
    pub target_data_version: i32,
    pub locations: Vec<String>,
    pub locations_truncated: bool,
}

fn payload_type_name(id: &str) -> &str {
    match id {
        "minecraft:banner" | "Banner" => "旗帜",
        "minecraft:hanging_sign" => "悬挂告示牌",
        "minecraft:sign" | "Sign" => "告示牌",
        "minecraft:decorated_pot" => "饰纹陶罐",
        "minecraft:armor_stand" => "盔甲架",
        "minecraft:item_frame" => "物品展示框",
        "minecraft:glow_item_frame" => "荧光物品展示框",
        "minecraft:painting" => "画",
        "minecraft:tropical_fish" => "热带鱼",
        "minecraft:frog" => "青蛙",
        "minecraft:parrot" => "鹦鹉",
        "minecraft:boat" => "船",
        "minecraft:chest_minecart" | "MinecartChest" => "运输矿车",
        "minecraft:hopper_minecart" | "MinecartHopper" => "漏斗矿车",
        "minecraft:chest"
        | "minecraft:trapped_chest"
        | "minecraft:barrel"
        | "minecraft:hopper"
        | "minecraft:shulker_box"
        | "minecraft:dropper"
        | "minecraft:dispenser" => "容器物品",
        "PendingBlockTicks" | "PendingFluidTicks" => "计划刻",
        _ => "附加数据",
    }
}

fn group_payload_issue(region: &Value, issue: &str) -> (String, String, String, String, String, String) {
    let (field, index) = issue
        .split_once('[')
        .and_then(|(field, rest)| {
            rest.split_once(']')
                .and_then(|(index, _)| index.parse::<usize>().ok().map(|index| (field, index)))
        })
        .unwrap_or((issue.split('：').next().unwrap_or("附加数据"), usize::MAX));
    let id = compound(region)
        .ok()
        .and_then(|region| region.get(field))
        .and_then(|payload| match payload {
            Value::List(entries) => entries.get(index),
            _ => None,
        })
        .and_then(|entry| compound(entry).ok())
        .and_then(|entry| entry.get("id"))
        .and_then(|id| match id {
            Value::String(id) => Some(id.as_str()),
            _ => None,
        })
        .unwrap_or(field);
    let (kind, message) = if issue.contains("目标")
        && (issue.contains("没有") || issue.contains("不能") || issue.contains("不支持"))
    {
        (
            "unsupported",
            "目标版本不能无损保存，请选择兼容版本或调整对应内容。",
        )
    } else if ["尚未", "未覆盖", "需要单独迁移", "需单独", "只覆盖"]
        .iter()
        .any(|term| issue.contains(term))
    {
        (
            "unimplemented",
            "此类数据迁移暂未支持，原文件保留；可选择其他目标版本。",
        )
    } else {
        ("invalid", "数据格式异常，请检查原投影或选择其他目标版本。")
    };
    (
        kind.into(),
        match field { "Entities" => "entity", "TileEntities" | "BlockEntities" => "block_entity", "PendingBlockTicks" | "PendingFluidTicks" => "scheduled_tick", _ => "payload" }.into(),
        id.into(),
        payload_type_name(id).into(),
        format!("{field}[{index}]"),
        issue.split_once('：').map(|(_, detail)| detail).unwrap_or(message).into(),
    )
}

fn compound(value: &Value) -> Result<&HashMap<String, Value>> {
    match value {
        Value::Compound(map) => Ok(map),
        _ => bail!("投影 NBT 节点应为 Compound"),
    }
}
fn integer(value: Option<&Value>) -> Result<i32> {
    match value {
        Some(Value::Int(value)) => Ok(*value),
        _ => bail!("投影缺少有效的整数版本/尺寸字段"),
    }
}
fn state(value: &Value) -> Result<(String, BTreeMap<String, String>)> {
    let map = compound(value)?;
    let Some(Value::String(name)) = map.get("Name") else {
        bail!("调色板缺少 Name")
    };
    let mut props = BTreeMap::new();
    if let Some(value) = map.get("Properties") {
        for (key, value) in compound(value)? {
            let Value::String(value) = value else {
                bail!("方块状态属性不是字符串")
            };
            props.insert(key.clone(), value.clone());
        }
    }
    Ok((name.clone(), props))
}
fn key(name: &str, props: &BTreeMap<String, String>) -> String {
    if props.is_empty() {
        name.into()
    } else {
        format!(
            "{name}[{}]",
            props
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}
pub(crate) fn parse_key(value: &str) -> Result<(String, BTreeMap<String, String>)> {
    let Some((name, tail)) = value.split_once('[') else {
        if value.is_empty() || value.contains(']') {
            bail!("替代方块格式无效")
        }
        return Ok((value.into(), BTreeMap::new()));
    };
    let Some(tail) = tail.strip_suffix(']') else {
        bail!("替代方块状态缺少 ]")
    };
    let mut props = BTreeMap::new();
    for prop in tail.split(',') {
        let Some((key, value)) = prop.split_once('=') else {
            bail!("替代状态属性缺少 =")
        };
        if key.is_empty() || value.is_empty() || props.insert(key.into(), value.into()).is_some() {
            bail!("替代状态属性无效或重复")
        }
    }
    Ok((name.into(), props))
}
fn canonical(value: &str) -> Result<String> {
    let (name, props) = parse_key(value.trim())?;
    Ok(key(&name, &props))
}
fn valid(registry: &BlockRegistry, value: &str) -> bool {
    parse_key(value).ok().is_some_and(|(name, props)| {
        registry
            .blocks
            .get(&name)
            .is_some_and(|block| block.accepts(value, &props))
    })
}

/// Actual DataFixers v4541 uses LegacyBlockRenameFix (not replacement by a
/// different block family): every state property keeps its original meaning.
/// The inverse is explicit and applies only across that same schema boundary.
fn migrated_block_name<'a>(name: &'a str, source: i32, target: i32) -> &'a str {
    if source >= 1519 && target < 1519 {
        if let Some(legacy) = crate::version_payload_migration::legacy_banner_name(name) { return legacy; }
    }
    match (source < 4541, target < 4541, name) {
        (true, false, "minecraft:chain") => "minecraft:iron_chain",
        (false, true, "minecraft:iron_chain") => "minecraft:chain",
        _ => name,
    }
}

fn recommend(
    registry: &BlockRegistry,
    name: &str,
    props: &BTreeMap<String, String>,
) -> Option<String> {
    let target_name = if registry.blocks.contains_key(name) {
        name.to_owned()
    } else {
        // Same functional family, never a silent generic stone/air fallback.
        let family = database().ok()?.category(name)?;
        let target = format!("minecraft:oak_{family}");
        if !registry.blocks.contains_key(&target) {
            return None;
        }
        target
    };
    let block = registry.blocks.get(&target_name)?;
    let (_, defaults) = parse_key(&block.default).ok()?;
    if !block.legacy_states.is_empty() {
        return block
            .legacy_states
            .iter()
            .min_by_key(|candidate| {
                let (_, target) = parse_key(candidate).expect("catalog generated state");
                let mismatches = props
                    .iter()
                    .filter(|(key, value)| target.get(*key) != Some(*value))
                    .count();
                let default_delta = defaults
                    .iter()
                    .filter(|(key, value)| target.get(*key) != Some(*value))
                    .count();
                (mismatches, default_delta)
            })
            .cloned();
    }
    let next: BTreeMap<_, _> = block
        .properties
        .iter()
        .map(|(prop, values)| {
            let value = props
                .get(prop)
                .filter(|value| values.contains(*value))
                .or_else(|| defaults.get(prop))
                .or_else(|| values.first())?;
            Some((prop.clone(), value.clone()))
        })
        .collect::<Option<_>>()?;
    let next = key(&target_name, &next);
    valid(registry, &next).then_some(next)
}

fn semantic_state(
    source_data_version: i32,
    registry: &BlockRegistry,
    name: &str,
    props: &BTreeMap<String, String>,
) -> Result<(String, BTreeMap<String, String>)> {
    if source_data_version >= 1519 { return Ok((name.to_owned(), props.clone())); }
    database()?.registry_by_data_version(source_data_version)?
        .legacy_state_aliases.get(&key(name, props))
        .and_then(|states| states.iter().find(|state| parse_key(state).ok().is_some_and(|(id, _)| registry.blocks.contains_key(&id))).or_else(|| states.first()))
        .map(|state| parse_key(state)).transpose()
        .map(|state| state.unwrap_or_else(|| (name.to_owned(), props.clone())))
}

/// A cancellation result is distinct from malformed/unsupported NBT.
#[derive(Debug)]
pub struct ConversionCancelled;
impl std::fmt::Display for ConversionCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("版本转换已取消") }
}
impl std::error::Error for ConversionCancelled {}

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) { return Err(ConversionCancelled.into()); }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionProgress {
    pub stage: &'static str,
    pub completed: u64,
    pub total: u64,
    pub cache_hit: bool,
    pub elapsed_ms: u64,
}

#[derive(PartialEq, Eq)]
struct SourceIdentity {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
}
impl SourceIdentity {
    fn read(path: &Path) -> Result<Self> {
        let path = fs::canonicalize(path).context("定位转换源文件")?;
        let metadata = fs::metadata(&path)?;
        Ok(Self { path, bytes: metadata.len(), modified: metadata.modified()?, created: metadata.created().ok() })
    }
}
struct CachedSource {
    identity: SourceIdentity,
    sha256: String,
    root: Value,
    data_version: i32,
    heap_bytes: usize,
    frequencies: BTreeMap<String, RegionScan>,
}

#[derive(Clone)]
struct RegionScan {
    frequencies: Vec<u64>,
    layout: crate::nbt::PaletteLayout,
}

/// Owned by the existing long-lived Rust data worker, never by UI/renderer.
/// One source only; oversized inputs still convert normally without caching.
pub struct ConversionCache {
    max_source_bytes: usize,
    source: Option<CachedSource>,
}
impl ConversionCache {
    pub fn new(max_source_bytes: usize) -> Self { Self { max_source_bytes, source: None } }

    pub fn convert(
        &mut self,
        input: &Path,
        output: Option<&Path>,
        request: &ConversionRequest,
        cancel: &AtomicBool,
        mut progress: impl FnMut(ConversionProgress),
    ) -> Result<ConversionPreview> {
        let started = Instant::now();
        check_cancel(cancel)?;
        let identity = SourceIdentity::read(input)?;
        let cache_hit = self.source.as_ref().is_some_and(|source| source.identity == identity);
        let mut emit = |stage, completed, total| progress(ConversionProgress {
            stage, completed, total, cache_hit, elapsed_ms: started.elapsed().as_millis() as u64,
        });
        if !cache_hit { self.source = None; }
        let (mut source, loaded_target, source_load_elapsed) = match self.source.take() {
            Some(source) => (source, None, started.elapsed()),
            None => {
                // One bounded scoped task overlaps input-independent registry
                // initialization with source I/O/NBT decode. Both still belong
                // to this operation's measured time, and every exit joins it.
                let (source, target, source_load_elapsed) = std::thread::scope(|scope| -> Result<_> {
                    let registry = std::thread::Builder::new().name("nova-version-registry".into()).spawn_scoped(scope, || -> Result<i32> {
                        let registry_started = Instant::now();
                        // The worker's immutable game-data snapshot is TLS-
                        // scoped, so bind it on this temporary thread as well.
                        let _context = crate::game_data::operation_context()?;
                        let result = database()?.version(&request.target_version).map(|version| version.data_version);
                        eprintln!("[version-convert] stage=registry elapsed_ms={:.3} parallel=true", registry_started.elapsed().as_secs_f64() * 1000.0);
                        result
                    }).context("启动版本注册表读取任务")?;
                    let load_started = Instant::now();
                    let source = load_source(input, identity, cancel, &mut emit);
                    let source_load_elapsed = load_started.elapsed();
                    let target = registry.join().map_err(|_| anyhow::anyhow!("版本注册表读取任务异常"));
                    check_cancel(cancel)?;
                    // Join happens before propagating either source or registry
                    // errors, so no initializer survives cancellation/failure.
                    Ok((source?, target??, source_load_elapsed))
                })?;
                (source, Some(target), source_load_elapsed)
            }
        };
        eprintln!("[version-convert] stage=load elapsed_ms={:.3} wall_elapsed_ms={:.3} cache_hit={} source_bytes={} source_heap_bytes={}", source_load_elapsed.as_secs_f64() * 1000.0, started.elapsed().as_secs_f64() * 1000.0, cache_hit, source.identity.bytes, source.heap_bytes);
        // Preview reuse trusts normal file identity. Writes always re-hash the
        // current bytes, including edits that preserved length/timestamps.
        let result = (|| -> Result<ConversionPreview> {
            if output.is_some() {
                let actual = source_digest(input, cancel)?;
                if actual != source.sha256 { bail!("原文件已变化，请重新预览转换差异") }
            }
            if request.expected_source_sha256.as_ref().is_some_and(|hash| hash != &source.sha256) {
                bail!("原文件已变化，请重新预览转换差异")
            }
            let target = match loaded_target {
                Some(target) => target,
                None => {
                    let registry_started = Instant::now();
                    let target = database()?.version(&request.target_version)?.data_version;
                    eprintln!("[version-convert] stage=registry elapsed_ms={:.3} parallel=false", registry_started.elapsed().as_secs_f64() * 1000.0);
                    target
                }
            };
            let backup = ConversionRestore::capture(&source.root, (source.data_version < 1519) != (target < 1519))?;
            let result = convert_loaded(input, output, request, &mut source, cancel, &mut emit);
            // Only palettes/payloads mutate on modern paths; packed block arrays
            // are never cloned. Restore even after cancellation or an error.
            backup.restore(&mut source.root);
            result
        })();
        if source.heap_bytes.saturating_add(source.frequencies.values().map(|v| v.frequencies.capacity() * 8).sum::<usize>()) <= self.max_source_bytes {
            self.source = Some(source);
        }
        if result.is_ok() { emit("ready", 1, 1); }
        result
    }
}

fn nbt_heap_bytes(value: &Value) -> usize {
    use std::mem::size_of;
    match value {
        Value::String(value) => value.capacity(),
        Value::ByteArray(value) => value.len(),
        Value::IntArray(value) => value.len().saturating_mul(4),
        Value::LongArray(value) => value.len().saturating_mul(8),
        Value::List(values) => values.capacity().saturating_mul(size_of::<Value>()).saturating_add(values.iter().map(nbt_heap_bytes).sum::<usize>()),
        Value::Compound(fields) => fields.capacity().saturating_mul(size_of::<(String, Value)>() + 16).saturating_add(fields.iter().map(|(key, value)| key.capacity().saturating_add(nbt_heap_bytes(value))).sum::<usize>()),
        _ => 0,
    }
}
fn source_digest(input: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut file = fs::File::open(input)?;
    let mut buffer = vec![0; 1 << 20];
    let mut digest = Sha256::new();
    loop {
        check_cancel(cancel)?;
        let read = file.read(&mut buffer)?;
        if read == 0 { break; }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
struct ConversionReader<'a, R, F> {
    inner: R,
    cancel: &'a AtomicBool,
    emit: &'a mut F,
    bytes: u64,
    next_check: u64,
    next_progress: u64,
}
impl<R: Read, F: FnMut(&'static str, u64, u64)> Read for ConversionReader<'_, R, F> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.bytes >= self.next_check {
            if self.cancel.load(Ordering::Relaxed) { return Err(std::io::Error::other(ConversionCancelled)); }
            self.next_check = self.bytes.saturating_add(65536);
        }
        let read = self.inner.read(buffer)?;
        self.bytes = self.bytes.saturating_add(read as u64);
        if self.bytes >= self.next_progress {
            (self.emit)("decode", self.bytes, 0);
            self.next_progress = self.bytes.saturating_add(1 << 20);
        }
        Ok(read)
    }
}

fn load_source(input: &Path, identity: SourceIdentity, cancel: &AtomicBool, emit: &mut impl FnMut(&'static str, u64, u64)) -> Result<CachedSource> {
    let mut file = fs::File::open(input).with_context(|| format!("读取投影 {}", input.display()))?;
    let mut bytes = Vec::new();
    let mut buffer = vec![0; 1 << 20];
    let mut digest = Sha256::new();
    loop {
        check_cancel(cancel)?;
        let count = file.read(&mut buffer)?;
        if count == 0 { break; }
        digest.update(&buffer[..count]); bytes.extend_from_slice(&buffer[..count]);
        emit("read", bytes.len() as u64, identity.bytes);
    }
    let source_sha256 = format!("{:x}", digest.finalize());
    // Stream the decoder into fastnbt: no second full decompressed byte copy,
    // and cancellation is observable during both gzip and NBT deserialization.
    let mut reader = ConversionReader { inner: GzDecoder::new(&bytes[..]), cancel, emit, bytes: 0, next_check: 0, next_progress: 0 };
    let parsed = fastnbt::from_reader(&mut reader);
    check_cancel(cancel)?;
    let mut root: Value = parsed.context("解析完整投影 NBT")?;
    let trailing = std::io::copy(&mut reader, &mut std::io::sink())?;
    check_cancel(cancel)?;
    if trailing != 0 { bail!("投影NBT根节点后存在未解析数据，原文件保留") }
    let source_data_version = integer(compound(&root)?.get("MinecraftDataVersion"))?;
    if compound(&root)?.get("Materials") == Some(&Value::String("Alpha".into())) {
        root = crate::version_legacy_encoding::decode(&root, database()?.registry_by_data_version(source_data_version)?)?;
    }
    if SourceIdentity::read(input)? != identity { bail!("读取期间原文件已变化，请重试预检") }
    let heap_bytes = nbt_heap_bytes(&root);
    emit("parsed", 1, 1);
    Ok(CachedSource { identity, sha256: source_sha256, root, data_version: source_data_version, heap_bytes, frequencies: BTreeMap::new() })
}

struct ConversionRestore {
    root_fields: Vec<(String, Option<Value>)>,
    regions: Vec<(String, Vec<(String, Option<Value>)>)>,
}
impl ConversionRestore {
    fn capture(root: &Value, changes_packing: bool) -> Result<Self> {
        let root = compound(root)?;
        let regions = compound(root.get("Regions").context("投影缺少 Regions")?)?;
        let mut saved = Vec::with_capacity(regions.len());
        for (name, region) in regions {
            let fields = compound(region)?;
            let mut keys = vec!["BlockStatePalette", "TileEntities", "Entities", "BlockEntities", "PendingBlockTicks", "PendingFluidTicks"];
            if changes_packing { keys.push("BlockStates"); }
            saved.push((name.clone(), keys.into_iter().map(|key| (key.to_owned(), fields.get(key).cloned())).collect()));
        }
        Ok(Self { root_fields: ["MinecraftDataVersion", "Version", "SubVersion"].into_iter().map(|key| (key.into(), root.get(key).cloned())).collect(), regions: saved })
    }
    fn restore(self, root: &mut Value) {
        let Value::Compound(root) = root else { return; };
        for (key, value) in self.root_fields { if let Some(value) = value { root.insert(key, value); } else { root.remove(&key); } }
        let Some(Value::Compound(regions)) = root.get_mut("Regions") else { return; };
        for (name, fields) in self.regions {
            let Some(Value::Compound(region)) = regions.get_mut(&name) else { continue; };
            for (key, value) in fields { if let Some(value) = value { region.insert(key, value); } else { region.remove(&key); } }
        }
    }
}

pub fn convert(input: &Path, output: Option<&Path>, request: &ConversionRequest) -> Result<ConversionPreview> {
    ConversionCache::new(0).convert(input, output, request, &AtomicBool::new(false), |_| {})
}

fn convert_loaded(
    input: &Path,
    output: Option<&Path>,
    request: &ConversionRequest,
    source: &mut CachedSource,
    cancel: &AtomicBool,
    emit: &mut impl FnMut(&'static str, u64, u64),
) -> Result<ConversionPreview> {
    let source_data_version = source.data_version;
    let source_sha256 = source.sha256.clone();
    let frequencies_cache = &mut source.frequencies;
    let root = &mut source.root;
    let db = database()?;
    let target = db.version(&request.target_version)?;
    let registry = db.registry(&request.target_version)?;
    let selected_source = request.selection.as_ref().map(|selection| canonical(&selection.source_state)).transpose()?;
    let selected_category = if let Some(selection) = &request.selection {
        if !registry.blocks.contains_key(&selection.target_block_id) { bail!("目标真实注册表没有所选方块 {}", selection.target_block_id) }
        let (name, props) = parse_key(selected_source.as_deref().unwrap())?;
        let (name, _) = semantic_state(source_data_version, registry, &name, &props)?;
        db.category(&name).filter(|category| !category.is_empty())
    } else { None };
    let edited_source = request.property_edit.as_ref().map(|edit| canonical(&edit.source_state)).transpose()?;
    let mut selected_found = selected_source.is_none();
    let mut edited_found = edited_source.is_none();
    let mut effective_replacements = BTreeMap::new();
    let mut entries: BTreeMap<String, ConversionEntry> = BTreeMap::new();
    let mut blocked_payloads = Vec::new();
    let mut payload_changes = Vec::new();
    let mut checked_entity_count = 0;
    let mut checked_block_entity_count = 0;
    let mut checked_entity_types = BTreeMap::new();
    let mut checked_block_entity_types = BTreeMap::new();
    let mut checked_item_stack_count = 0;
    let mut changed_entity_count = 0;
    let mut changed_block_entity_count = 0;
    let mut frequency_elapsed = Duration::ZERO;
    let mut frequency_layout_elapsed = Duration::ZERO;
    let mut frequency_scan_elapsed = Duration::ZERO;
    let mut frequency_cache_hits = 0_u64;
    let mut frequency_blocks = 0_u64;
    let mut frequency_word_reads = 0_usize;
    let mut payload_elapsed = Duration::ZERO;
    let mut payload_issue_groups: BTreeMap<
        (String, String, String, String, String),
        std::collections::BTreeSet<String>,
    > = BTreeMap::new();
    let root_map = match &mut *root {
        Value::Compound(map) => map,
        _ => bail!("无效的投影根节点"),
    };
    let Some(Value::Compound(regions)) = root_map.get_mut("Regions") else {
        bail!("投影缺少 Regions")
    };
    let region_total = regions.len() as u64;
    for (region_index, (region_name, region)) in regions.iter_mut().enumerate() {
        check_cancel(cancel)?;
        emit("regions", region_index as u64, region_total);
        // Count the actual source row IDs before any palette/payload changes,
        // including rows whose codec migration will later report a blocker.
        let source_fields = compound(region)?;
        for (field, types, count) in [
            ("Entities", &mut checked_entity_types, &mut checked_entity_count),
            ("TileEntities", &mut checked_block_entity_types, &mut checked_block_entity_count),
        ] {
            if let Some(Value::List(values)) = source_fields.get(field) {
                *count += values.len() as u64;
                for value in values {
                    let id = compound(value).ok().and_then(|fields| fields.get("id")).and_then(|id| match id { Value::String(id) => Some(id.as_str()), _ => None }).unwrap_or("<missing-or-invalid-id>");
                    *types.entry(id.to_owned()).or_insert(0) += 1;
                }
            }
        }
        if let Some(Value::List(values)) = source_fields.get("BlockEntities") {
            checked_block_entity_count += values.len() as u64;
            for value in values {
                let id = compound(value).ok().and_then(|fields| fields.get("id")).and_then(|id| match id { Value::String(id) => Some(id.as_str()), _ => None }).unwrap_or("<missing-or-invalid-id>");
                *checked_block_entity_types.entry(id.to_owned()).or_insert(0) += 1;
            }
        }
        crate::version_payload_migration::migrate_head_palette(region, source_data_version, target.data_version)?;
        crate::version_payload_migration::migrate_banner_palette(region, source_data_version, target.data_version)?;
        let Value::Compound(fields) = region else {
            bail!("选区不是 Compound")
        };
        let size = compound(fields.get("Size").context("选区缺少 Size")?)?;
        let volume = region_volume(&Vec3i {
            x: integer(size.get("x"))?,
            y: integer(size.get("y"))?,
            z: integer(size.get("z"))?,
        })?;
        let Some(Value::List(palette)) = fields.get("BlockStatePalette") else {
            bail!("选区缺少调色板")
        };
        let Some(Value::LongArray(packed)) = fields.get("BlockStates") else {
            bail!("选区缺少 BlockStates")
        };
        let original_names: Vec<String> = palette
            .iter()
            .map(|value| state(value).map(|(name, _)| name))
            .collect::<Result<_>>()?;
        // Cross-flattening head/banner conversion may split/repack a palette;
        // all other target changes reuse the immutable source frequency index.
        let frequency_started = Instant::now();
        let repacked = (source_data_version < 1519) != (target.data_version < 1519);
        let scan = if !repacked { frequencies_cache.get(region_name).cloned() } else { None };
        let scan = match scan {
            Some(scan) => { frequency_cache_hits += 1; scan },
            None => {
                let layout_started = Instant::now();
                let packed = BigEndianLongArray::new(packed.iter().copied().collect());
                let layout = choose_palette_packing(&packed, volume, bits_for_palette(palette.len()), palette.len())?;
                frequency_layout_elapsed += layout_started.elapsed();
                let scan_started = Instant::now();
                let mut counts = vec![0_u64; palette.len().max(1)];
                let mut nonzero_count = 0_u64;
                for start in (0..volume).step_by(65536) {
                    check_cancel(cancel)?;
                    emit("blocks", start as u64, volume as u64);
                    let end = start.saturating_add(65536).min(volume);
                    let mut invalid_index = None;
                    frequency_word_reads += layout.visit_palette_range(&packed, start, end, true, |_, index| {
                        if let Some(count) = counts.get_mut(index) {
                            *count += 1;
                            nonzero_count += 1;
                        } else { invalid_index = Some(index); }
                    });
                    if let Some(index) = invalid_index { bail!("投影方块索引 {index} 超出调色板") }
                }
                // The shared reader skips encoded palette index zero. It need
                // not be AIR: conversion still counts every such block, so a
                // replacement of palette[0] has the complete actual frequency.
                counts[0] = (volume as u64).checked_sub(nonzero_count).context("投影非零调色板频率超过选区体积")?;
                frequency_scan_elapsed += scan_started.elapsed();
                frequency_blocks += volume as u64;
                let scan = RegionScan { frequencies: counts, layout };
                if !repacked { frequencies_cache.insert(region_name.clone(), scan.clone()); }
                scan
            }
        };
        frequency_elapsed += frequency_started.elapsed();
        let frequencies = &scan.frequencies;
        let Some(Value::List(palette)) = fields.get_mut("BlockStatePalette") else {
            unreachable!()
        };
        for (index, palette_value) in palette.iter_mut().enumerate() {
            check_cancel(cancel)?;
            let (name, props) = state(palette_value)?;
            let old = key(&name, &props);
            let (semantic_name, semantic_props) = semantic_state(source_data_version, registry, &name, &props)?;
            let target_name = migrated_block_name(&semantic_name, source_data_version, target.data_version);
            let compatible = valid(registry, &old);
            let mut chosen = if let Some(replacement) = request.replacements.get(&old).filter(|value| !value.trim().is_empty()) {
                let replacement = canonical(replacement)?;
                if !replacement.contains('[')
                    && registry.blocks.contains_key(&replacement)
                    && !valid(registry, &replacement)
                {
                    recommend(registry, &replacement, &semantic_props).or(Some(replacement))
                } else {
                    Some(replacement)
                }
            } else if compatible {
                Some(old.clone())
            } else {
                recommend(registry, target_name, &semantic_props)
            };
            if let Some(selection) = &request.selection {
                let selected = selected_source.as_deref() == Some(old.as_str());
                selected_found |= selected;
                // Match the displayed affected entries, not hidden compatible
                // blocks that happened to share a broad functional category.
                let affected = !compatible || chosen.as_ref() != Some(&old);
                let same_category = selected_category.is_some_and(|category| db.category(&semantic_name) == Some(category));
                if selected || (selection.bulk && affected && same_category) {
                    chosen = recommend(registry, &selection.target_block_id, &semantic_props);
                }
            }
            if edited_source.as_deref() == Some(old.as_str()) {
                edited_found = true;
                let edit = request.property_edit.as_ref().unwrap();
                let (target_id, _) = parse_key(chosen.as_deref().context("请先为该条目选择目标方块")?)?;
                let definition = registry.blocks.get(&target_id).context("所选目标方块没有真实注册表定义")?;
                let (_, mut properties) = parse_key(&definition.default)?;
                for (property, value) in &edit.properties {
                    if !definition.properties.get(property).is_some_and(|values| values.contains(value)) { bail!("目标方块 {target_id} 没有属性 {property}={value}") }
                    properties.insert(property.clone(), value.clone());
                }
                chosen = Some(key(&target_id, &properties));
            }
            let valid = chosen
                .as_ref()
                .is_some_and(|chosen| valid(registry, chosen));
            if !compatible || chosen.as_ref() != Some(&old) {
                effective_replacements.insert(old.clone(), chosen.clone().unwrap_or_default());
                let target_state = chosen.as_deref().map(parse_key).transpose()?;
                let target_block_id = target_state.as_ref().map(|(id, _)| id.clone());
                let target_properties = target_state.as_ref().map(|(_, props)| props.clone()).unwrap_or_default();
                let target_property_options = target_block_id.as_ref().and_then(|id| registry.blocks.get(id)).map(|block| block.properties.clone()).unwrap_or_default();
                let entry = entries
                    .entry(old.clone())
                    .or_insert_with(|| ConversionEntry {
                        source_state: old,
                        target_state: chosen.clone(),
                        count: 0,
                        reason: if target_name != semantic_name {
                            "方块名称按目标版本更新，保留原状态".into()
                        } else if !registry.blocks.contains_key(&name) {
                            "目标版本没有该方块；推荐同类替代".into()
                        } else {
                            "目标方块状态不兼容或用户指定替代".into()
                        },
                        valid,
                        source_icon_state: key(&semantic_name, &semantic_props),
                        source_name: db.name(
                            db.version_for_data_version(source_data_version)
                                .unwrap_or("26.3"),
                            &semantic_name,
                        ),
                        target_name: chosen
                            .as_deref()
                            .and_then(|state| parse_key(state).ok())
                            .map(|(id, _)| db.name(&request.target_version, &id)),
                        category: db.category(&semantic_name).unwrap_or("").into(),
                        source_block_id: name.clone(),
                        source_icon_id: semantic_name.clone(),
                        source_properties: props.clone(),
                        target_icon_id: target_block_id.clone(),
                        target_block_id,
                        target_properties,
                        target_property_options,
                    });
                entry.count += frequencies[index];
            }
            if valid {
                let (name, props) = parse_key(chosen.as_deref().unwrap())?;
                let Value::Compound(map) = palette_value else {
                    unreachable!()
                };
                map.insert("Name".into(), Value::String(name));
                if props.is_empty() {
                    map.remove("Properties");
                } else {
                    map.insert(
                        "Properties".into(),
                        Value::Compound(
                            props
                                .into_iter()
                                .map(|(key, value)| (key, Value::String(value)))
                                .collect(),
                        ),
                    );
                }
            }
        }
        let region_fields = compound(region)?;
        if region_fields.iter().any(|(field, payload)| {
            [
                "Entities",
                "TileEntities",
                "BlockEntities",
                "PendingBlockTicks",
                "PendingFluidTicks",
            ]
            .contains(&field.as_str())
                && !matches!(payload,Value::List(values) if values.is_empty())
        }) {
            emit("payloads", region_index as u64, region_total);
            let payload_started = Instant::now();
            let migration = crate::version_payload_migration::migrate_with_palette_controlled(
                region,
                source_data_version,
                target.data_version,
                &original_names,
                cancel,
                Some(scan.layout),
            );
            payload_elapsed += payload_started.elapsed();
            let migration = migration?;
            checked_item_stack_count += migration.checked_item_stack_count;
            changed_entity_count += migration.changed_entity_count;
            changed_block_entity_count += migration.changed_block_entity_count;
            for issue in &migration.issues {
                let (kind, domain, source_id, name, identity, message) = group_payload_issue(region, issue);
                payload_issue_groups
                    .entry((kind, domain, source_id, name, message))
                    .or_default()
                    .insert(format!("{region_name}/{identity}"));
            }
            blocked_payloads.extend(
                migration
                    .issues
                    .into_iter()
                    .map(|issue| format!("{region_name}/{issue}")),
            );
            payload_changes.extend(
                migration
                    .changes
                    .into_iter()
                    .map(|change| format!("{region_name}/{change}")),
            );
        }
    }
    eprintln!("[version-convert] stage=frequency elapsed_ms={:.3} layout_ms={:.3} scan_ms={:.3} cache_regions={} scanned_blocks={} word_reads={}", frequency_elapsed.as_secs_f64()*1000.0, frequency_layout_elapsed.as_secs_f64()*1000.0, frequency_scan_elapsed.as_secs_f64()*1000.0, frequency_cache_hits, frequency_blocks, frequency_word_reads);
    eprintln!("[version-convert] stage=payload elapsed_ms={:.3} entities={} block_entities={} checked_items={}", payload_elapsed.as_secs_f64()*1000.0, checked_entity_count, checked_block_entity_count, checked_item_stack_count);
    if !selected_found { bail!("所选源方块状态已变化，请重新预览转换差异") }
    if !edited_found { bail!("属性编辑条目的源方块状态已变化，请重新预览转换差异") }
    if target.legacy && blocked_payloads.is_empty() && entries.values().all(|entry| entry.valid) {
        if let Err(error) = crate::version_legacy_encoding::encode(&root, registry) {
            blocked_payloads.push(error.to_string());
        }
    }
    let writable = blocked_payloads.is_empty() && entries.values().all(|entry| entry.valid);
    let changed_block_count = entries
        .values()
        .filter(|entry| entry.valid)
        .map(|entry| entry.count)
        .sum();
    let mut preview = ConversionPreview {
        source_sha256: source_sha256.clone(),
        source_data_version,
        source_version: db.version_for_data_version(source_data_version).map(str::to_owned).unwrap_or_else(|| format!("DataVersion {source_data_version}")),
        target_version: request.target_version.clone(),
        target_data_version: target.data_version,
        entries: entries.into_values().collect(),
        effective_replacements,
        blocked_payloads,
        payload_issues: payload_issue_groups
            .into_iter()
            .map(
                |((kind, domain, source_id, type_name, message), identities)| ConversionPayloadIssue {
                    kind,
                    type_name,
                    count: identities.len(),
                    message,
                    domain,
                    source_id,
                    source_data_version,
                    target_data_version: target.data_version,
                    locations: identities.iter().take(128).cloned().collect(),
                    locations_truncated: identities.len() > 128,
                },
            )
            .collect(),
        payload_changes: payload_changes
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        writable,
        changed_block_count,
        checked_entity_count,
        checked_block_entity_count,
        checked_entity_types,
        checked_block_entity_types,
        checked_item_stack_count,
        changed_entity_count,
        changed_block_entity_count,
        output_path: None,
        output_extension: target.output_extension.clone(),
    };
    check_cancel(cancel)?;
    if let Some(output) = output {
        emit("write", 0, 1);
        if request.expected_source_sha256.is_none() {
            bail!("另存前须预览并确认原文件哈希")
        }
        if !preview.writable {
            bail!("仍有未转换的方块状态或版本相关 NBT，另存已停止")
        }
        if output.exists() && !request.overwrite {
            bail!("另存目标已存在，不覆盖投影或原件")
        }
        let Value::Compound(map) = &mut *root else {
            unreachable!()
        };
        map.insert(
            "MinecraftDataVersion".into(),
            Value::Int(target.data_version),
        );
        if !target.legacy && source_data_version < 1519 {
            map.insert("Version".into(), Value::Int(6));
            map.insert("SubVersion".into(), Value::Int(1));
        }
        let encoded;
        let expected: &Value = if target.legacy {
            encoded = crate::version_legacy_encoding::encode(root, registry)?;
            &encoded
        } else { root };
        let converted = if target.legacy {
            fastnbt::to_bytes_with_opts(expected, fastnbt::SerOpts::new().root_name("Schematic"))?
        } else {
            fastnbt::to_bytes(expected)?
        };
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&converted)?;
        let converted = gzip.finish()?;
        verify_converted_payload(&converted, expected)?;
        check_cancel(cancel)?;
        if source_digest(input, cancel)? != source_sha256 { bail!("另存期间原文件已变化，转换已停止") }
        if request.overwrite {
            atomic_replace(output, &converted)?;
            verify_converted_payload(&fs::read(output)?, expected)?;
            preview.output_path = Some(output.display().to_string());
            return Ok(preview);
        }
        // create_new atomically rejects an alias of the source/existing file.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?;
        if let Err(error) = file.write_all(&converted).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(output);
            return Err(error.into());
        }
        drop(file);
        if let Err(error) = fs::read(output).map_err(anyhow::Error::from).and_then(|bytes| verify_converted_payload(&bytes, expected)) {
            let _ = fs::remove_file(output);
            return Err(error.context("另存后重新读取校验失败，未改动原文件"));
        }
        preview.output_path = Some(output.display().to_string());
    }
    Ok(preview)
}

fn verify_converted_payload(bytes: &[u8], expected: &Value) -> Result<()> {
    let mut raw = Vec::new();
    GzDecoder::new(bytes).read_to_end(&mut raw).context("重新读取转换后的压缩NBT")?;
    let actual: Value = fastnbt::from_bytes(&raw).context("重新解析转换后的完整NBT")?;
    if &actual != expected { bail!("转换后重新读取的数据与已确认内容不一致") }
    Ok(())
}

fn atomic_replace(output: &Path, bytes: &[u8]) -> Result<()> {
    let name = output
        .file_name()
        .context("覆盖目标缺少文件名")?
        .to_string_lossy();
    let temp = output.with_file_name(format!(".{name}.nova-convert-{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if fs::read(&temp)? != bytes { bail!("覆盖前读取临时文件校验失败，原文件保留") }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let from: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<_> = output.as_os_str().encode_wide().chain(Some(0)).collect();
            if unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        #[cfg(not(windows))]
        fs::rename(&temp, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub fn run_cli(args: &[String]) -> Result<()> {
    let mut input = None;
    let mut request = None;
    let mut output = None;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter.next().context("版本转换参数缺少值")?;
        match flag.as_str() {
            "--input" => input = Some(value),
            "--request" => request = Some(value),
            "--output" => output = Some(value),
            _ => bail!("unknown version-convert argument: {flag}"),
        }
    }
    let input = Path::new(input.context("missing --input")?);
    let request: ConversionRequest =
        serde_json::from_slice(&fs::read(request.context("missing --request")?)?)?;
    let preview = convert(input, output.map(|path| Path::new(path)), &request)?;
    println!("{}", serde_json::to_string(&preview)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path, payload: bool) -> Vec<u8> {
        let position = Value::Compound(HashMap::from([
            ("x".into(), Value::Int(0)),
            ("y".into(), Value::Int(0)),
            ("z".into(), Value::Int(0)),
        ]));
        let size = Value::Compound(HashMap::from([
            ("x".into(), Value::Int(4)),
            ("y".into(), Value::Int(1)),
            ("z".into(), Value::Int(1)),
        ]));
        let air = Value::Compound(HashMap::from([(
            "Name".into(),
            Value::String("minecraft:air".into()),
        )]));
        let log = Value::Compound(HashMap::from([
            (
                "Name".into(),
                Value::String("minecraft:pale_oak_log".into()),
            ),
            (
                "Properties".into(),
                Value::Compound(HashMap::from([("axis".into(), Value::String("z".into()))])),
            ),
        ]));
        let region = Value::Compound(HashMap::from([
            ("Position".into(), position),
            ("Size".into(), size),
            ("BlockStatePalette".into(), Value::List(vec![air, log])),
            (
                "BlockStates".into(),
                Value::LongArray(fastnbt::LongArray::new(vec![69])),
            ),
            (
                "TileEntities".into(),
                Value::List(if payload {
                    vec![Value::Compound(HashMap::from([
                        ("id".into(), Value::String("minecraft:chest".into())),
                        ("x".into(), Value::Int(0)),
                        ("y".into(), Value::Int(0)),
                        ("z".into(), Value::Int(0)),
                        ("ModPayload".into(), Value::String("custom".into())),
                    ]))]
                } else {
                    vec![]
                }),
            ),
            (
                "PrivateUnknownField".into(),
                Value::String("preserve".into()),
            ),
        ]));
        let root = Value::Compound(HashMap::from([
            ("MinecraftDataVersion".into(), Value::Int(5023)),
            ("Version".into(), Value::Int(6)),
            (
                "Metadata".into(),
                Value::Compound(HashMap::from([(
                    "Name".into(),
                    Value::String("test".into()),
                )])),
            ),
            (
                "Regions".into(),
                Value::Compound(HashMap::from([("test".into(), region)])),
            ),
            (
                "RootMarker".into(),
                Value::LongArray(fastnbt::LongArray::new(vec![7, 9])),
            ),
        ]));
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        let bytes = gzip.finish().unwrap();
        fs::write(path, &bytes).unwrap();
        bytes
    }
    fn request() -> ConversionRequest {
        ConversionRequest {
            target_version: "1.20.1".into(),
            replacements: BTreeMap::new(),
            expected_source_sha256: None,
            overwrite: false,
            selection: None,
            property_edit: None,
        }
    }
    #[test]
    fn actual_chain_rename_is_automatically_recommended_saved_and_reversed() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("chain-1.21.litematic");
        let modern = dir.path().join("iron-chain-26.3.litematic");
        let back = dir.path().join("chain-roundtrip-1.21.litematic");
        fixture(&input, false);
        let mut source = read(&input);
        let Value::Compound(root) = &mut source else { unreachable!() };
        root.insert("MinecraftDataVersion".into(), Value::Int(3953));
        let Value::Compound(regions) = root.get_mut("Regions").unwrap() else { unreachable!() };
        let Value::Compound(region) = regions.get_mut("test").unwrap() else { unreachable!() };
        let Value::List(palette) = region.get_mut("BlockStatePalette").unwrap() else { unreachable!() };
        palette[1] = Value::Compound(HashMap::from([
            ("Name".into(), Value::String("minecraft:chain".into())),
            ("Properties".into(), Value::Compound(HashMap::from([
                ("axis".into(), Value::String("y".into())),
                ("waterlogged".into(), Value::String("false".into())),
            ]))),
        ]));
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&fastnbt::to_bytes(&source).unwrap()).unwrap();
        let original_bytes = gzip.finish().unwrap();
        fs::write(&input, &original_bytes).unwrap();
        let mut request = ConversionRequest { target_version: "26.3".into(), replacements: BTreeMap::new(), expected_source_sha256: None, overwrite: false, selection: None, property_edit: None };
        let preview = convert(&input, None, &request).unwrap();
        assert!(preview.writable, "{:?}", preview.blocked_payloads);
        assert_eq!(preview.entries.len(), 1);
        assert_eq!(preview.entries[0].target_state.as_deref(), Some("minecraft:iron_chain[axis=y,waterlogged=false]"));
        assert_eq!(preview.entries[0].count, 3);
        assert_eq!(preview.changed_block_count, 3);
        assert!(request.replacements.is_empty(), "automatic migration, not a hidden UI override");
        request.expected_source_sha256 = Some(preview.source_sha256);
        convert(&input, Some(&modern), &request).unwrap();
        let modern_root = read(&modern);
        assert_eq!(compound(&modern_root).unwrap()["MinecraftDataVersion"], Value::Int(5023));
        let regions = compound(&compound(&modern_root).unwrap()["Regions"]).unwrap();
        let region = compound(&regions["test"]).unwrap();
        let Value::List(palette) = &region["BlockStatePalette"] else { unreachable!() };
        assert_eq!(compound(&palette[1]).unwrap()["Name"], Value::String("minecraft:iron_chain".into()));
        assert_eq!(region["BlockStates"], Value::LongArray(fastnbt::LongArray::new(vec![69])));
        request.target_version = "1.21".into();
        request.expected_source_sha256 = None;
        let preview = convert(&modern, None, &request).unwrap();
        assert!(preview.writable);
        assert_eq!(preview.entries[0].target_state.as_deref(), Some("minecraft:chain[axis=y,waterlogged=false]"));
        request.expected_source_sha256 = Some(preview.source_sha256);
        convert(&modern, Some(&back), &request).unwrap();
        assert_eq!(read(&back), source);
        assert_eq!(fs::read(&input).unwrap(), original_bytes);
    }
    fn read(path: &Path) -> Value {
        let mut bytes = Vec::new();
        GzDecoder::new(fs::File::open(path).unwrap())
            .read_to_end(&mut bytes)
            .unwrap();
        fastnbt::from_bytes(&bytes).unwrap()
    }
    #[test]
    fn actual_catalogs_and_legacy_defaults_match_verified_versions() {
        let catalogs = database().unwrap();
        assert_eq!(catalogs.version("26.3").unwrap().data_version, 5023);
        assert_eq!(
            catalogs.registry("1.20.1").unwrap().blocks["minecraft:oak_log"].default,
            "minecraft:oak_log[axis=y]"
        );
        assert_eq!(
            catalogs.registry("1.20.1").unwrap().blocks["minecraft:oak_leaves"].default,
            "minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]"
        );
        assert_eq!(
            catalogs
                .registry("26.3")
                .unwrap()
                .blocks
                .values()
                .map(|block| block.state_count)
                .sum::<usize>(),
            35723
        );
    }
    #[test]
    fn conversion_changes_actual_palette_before_version_preserving_envelope_and_packed_data() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("target.litematic");
        let original = fixture(&input, false);
        let mut request = request();
        let preview = convert(&input, None, &request).unwrap();
        assert!(preview.writable);
        assert_eq!(
            preview.entries[0].target_state.as_deref(),
            Some("minecraft:oak_log[axis=z]")
        );
        assert_eq!(preview.changed_block_count, 3);
        request.expected_source_sha256 = Some(preview.source_sha256);
        convert(&input, Some(&output), &request).unwrap();
        let before = read(&input);
        let after = read(&output);
        assert_eq!(
            compound(&after).unwrap()["MinecraftDataVersion"],
            Value::Int(3465)
        );
        assert_eq!(
            compound(&after).unwrap()["RootMarker"],
            compound(&before).unwrap()["RootMarker"]
        );
        let region = &compound(&compound(&after).unwrap()["Regions"]).unwrap()["test"];
        let region = compound(region).unwrap();
        assert_eq!(
            region["BlockStates"],
            Value::LongArray(fastnbt::LongArray::new(vec![69]))
        );
        assert_eq!(
            region["PrivateUnknownField"],
            Value::String("preserve".into())
        );
        assert_eq!(fs::read(&input).unwrap(), original);
    }
    #[test]
    fn unresolved_replacement_never_writes_or_changes_original_version() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("target.litematic");
        let original = fixture(&input, false);
        let mut request = request();
        request.replacements.insert(
            "minecraft:pale_oak_log[axis=z]".into(),
            "minecraft:oak_log[axis=invalid]".into(),
        );
        let preview = convert(&input, None, &request).unwrap();
        assert!(!preview.writable);
        request.expected_source_sha256 = Some(preview.source_sha256);
        assert!(convert(&input, Some(&output), &request).is_err());
        assert!(!output.exists());
        assert_eq!(fs::read(&input).unwrap(), original);
    }
    #[test]
    fn entity_item_payloads_block_partial_downgrade() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("target.litematic");
        fixture(&input, true);
        let mut request = request();
        let preview = convert(&input, None, &request).unwrap();
        assert!(!preview.writable);
        assert!(preview.blocked_payloads[0].contains("TileEntities"));
        request.expected_source_sha256 = Some(preview.source_sha256);
        assert!(convert(&input, Some(&output), &request).is_err());
        assert!(!output.exists());
    }
    #[test]
    fn legacy_target_changes_real_ids_metadata_and_file_encoding_then_upgrades() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("target.schematic");
        let modern = dir.path().join("upgraded.litematic");
        let original = fixture(&input, false);
        let mut req = request();
        req.target_version = "1.12.2".into();
        let preview = convert(&input, None, &req).unwrap();
        assert!(preview.writable, "{:?}", preview.blocked_payloads);
        assert_eq!(preview.output_extension, ".schematic");
        assert_eq!(
            preview.entries[0].target_state.as_deref(),
            Some("minecraft:oak_log[axis=z]")
        );
        req.expected_source_sha256 = Some(preview.source_sha256);
        convert(&input, Some(&output), &req).unwrap();
        let legacy = read(&output);
        let root = compound(&legacy).unwrap();
        assert_eq!(root["Materials"], Value::String("Alpha".into()));
        let Value::ByteArray(ids) = &root["Blocks"] else {
            panic!("legacy numeric Blocks")
        };
        let Value::ByteArray(data) = &root["Data"] else {
            panic!("legacy metadata")
        };
        assert_eq!(&ids[..], &[17, 17, 0, 17]);
        assert_eq!(&data[..], &[8, 8, 0, 8]);
        assert_eq!(fs::read(&input).unwrap(), original);
        let mut req = request();
        let preview = convert(&output, None, &req).unwrap();
        assert!(preview.writable, "{:?}", preview.blocked_payloads);
        req.expected_source_sha256 = Some(preview.source_sha256);
        convert(&output, Some(&modern), &req).unwrap();
        assert_eq!(
            compound(&read(&modern)).unwrap()["MinecraftDataVersion"],
            Value::Int(3465)
        );
    }
    #[test]
    fn explicit_overwrite_is_atomic_and_invalid_preview_cannot_replace_source() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        fixture(&input, false);
        let mut req = request();
        req.overwrite = true;
        assert!(convert(&input, Some(&input), &req).is_err());
        let p = convert(&input, None, &req).unwrap();
        req.expected_source_sha256 = Some(p.source_sha256);
        convert(&input, Some(&input), &req).unwrap();
        assert_eq!(
            compound(&read(&input)).unwrap()["MinecraftDataVersion"],
            Value::Int(3465)
        );
        assert!(
            dir.path()
                .read_dir()
                .unwrap()
                .all(|entry| entry.unwrap().path() == input)
        );
    }
    #[test]
    fn actual_preflatten_litematic_names_and_properties_use_mojang_data_not_suffix_guesses() {
        let dir=tempfile::tempdir().unwrap();let input=dir.path().join("old.litematic");let output=dir.path().join("new.litematic");fixture(&input,false);let mut root=read(&input);let Value::Compound(fields)=&mut root else{unreachable!()};fields.insert("MinecraftDataVersion".into(),Value::Int(1343));fields.insert("Version".into(),Value::Int(4));let Value::Compound(regions)=fields.get_mut("Regions").unwrap()else{unreachable!()};let Value::Compound(region)=regions.get_mut("test").unwrap()else{unreachable!()};let Value::List(palette)=region.get_mut("BlockStatePalette").unwrap()else{unreachable!()};palette[1]=Value::Compound(HashMap::from([("Name".into(),Value::String("minecraft:log".into())),("Properties".into(),Value::Compound(HashMap::from([("axis".into(),Value::String("z".into())),("variant".into(),Value::String("oak".into()))])))]));let mut gzip=GzEncoder::new(Vec::new(),Compression::default());gzip.write_all(&fastnbt::to_bytes(&root).unwrap()).unwrap();fs::write(&input,gzip.finish().unwrap()).unwrap();let mut req=request();let preview=convert(&input,None,&req).unwrap();assert!(preview.writable,"{:?}",preview.blocked_payloads);assert_eq!(preview.entries[0].source_state,"minecraft:log[axis=z,variant=oak]");assert_eq!(preview.entries[0].source_icon_state,"minecraft:oak_log[axis=z]");assert_eq!(preview.entries[0].source_name,"橡木原木");assert_eq!(preview.entries[0].target_state.as_deref(),Some("minecraft:oak_log[axis=z]"));assert_eq!(preview.changed_block_count,3);req.expected_source_sha256=Some(preview.source_sha256);convert(&input,Some(&output),&req).unwrap();let result=read(&output);assert_eq!(compound(&result).unwrap()["Version"],Value::Int(6));assert_eq!(compound(&result).unwrap()["MinecraftDataVersion"],Value::Int(3465));
    }
    #[test]
    fn container_replacement_updates_payload_identity_and_stone_never_orphans_inventory() {
        let dir=tempfile::tempdir().unwrap();let input=dir.path().join("barrel.litematic");let output=dir.path().join("chest.litematic");fixture(&input,false);let mut root=read(&input);let Value::Compound(fields)=&mut root else{unreachable!()};let Value::Compound(regions)=fields.get_mut("Regions").unwrap()else{unreachable!()};let Value::Compound(region)=regions.get_mut("test").unwrap()else{unreachable!()};let Value::List(palette)=region.get_mut("BlockStatePalette").unwrap()else{unreachable!()};palette[1]=Value::Compound(HashMap::from([("Name".into(),Value::String("minecraft:barrel".into())),("Properties".into(),Value::Compound(HashMap::from([("facing".into(),Value::String("north".into())),("open".into(),Value::String("false".into()))])))]));region.insert("TileEntities".into(),Value::List(vec![Value::Compound(HashMap::from([("id".into(),Value::String("minecraft:barrel".into())),("x".into(),Value::Int(0)),("y".into(),Value::Int(0)),("z".into(),Value::Int(0)),("Items".into(),Value::List(vec![Value::Compound(HashMap::from([("id".into(),Value::String("minecraft:stone".into())),("count".into(),Value::Int(3)),("Slot".into(),Value::Byte(0))]))]))]))]));let mut gzip=GzEncoder::new(Vec::new(),Compression::default());gzip.write_all(&fastnbt::to_bytes(&root).unwrap()).unwrap();let original=gzip.finish().unwrap();fs::write(&input,&original).unwrap();let old="minecraft:barrel[facing=north,open=false]";let mut req=request();req.replacements.insert(old.into(),"minecraft:chest".into());let preview=convert(&input,None,&req).unwrap();assert!(preview.writable,"{:?}",preview.blocked_payloads);req.expected_source_sha256=Some(preview.source_sha256);convert(&input,Some(&output),&req).unwrap();let root=read(&output);let regions=compound(&compound(&root).unwrap()["Regions"]).unwrap();let Value::List(tiles)=&compound(&regions["test"]).unwrap()["TileEntities"]else{unreachable!()};assert_eq!(compound(&tiles[0]).unwrap()["id"],Value::String("minecraft:chest".into()));req.replacements.insert(old.into(),"minecraft:stone".into());let preview=convert(&input,None,&req).unwrap();assert!(!preview.writable);assert!(preview.blocked_payloads.iter().any(|value|value.contains("不能保存原容器物品")));assert_eq!(fs::read(&input).unwrap(),original);
    }
    #[test]
    fn plain_container_items_are_really_migrated_before_data_version_changes() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("target.litematic");
        fixture(&input, false);
        let mut root = read(&input);
        let Value::Compound(root_map) = &mut root else {
            panic!()
        };
        root_map.insert("MinecraftDataVersion".into(), Value::Int(3465));
        let Value::Compound(regions) = root_map.get_mut("Regions").unwrap() else {
            panic!()
        };
        let Value::Compound(region) = regions.get_mut("test").unwrap() else {
            panic!()
        };
        let tile = Value::Compound(HashMap::from([
            ("id".into(), Value::String("minecraft:chest".into())),
            ("x".into(), Value::Int(0)),
            ("y".into(), Value::Int(0)),
            ("z".into(), Value::Int(0)),
            (
                "Items".into(),
                Value::List(vec![Value::Compound(HashMap::from([
                    ("id".into(), Value::String("minecraft:stone".into())),
                    ("Count".into(), Value::Byte(64)),
                    ("Slot".into(), Value::Byte(3)),
                ]))]),
            ),
        ]));
        if let Some(Value::List(palette)) = region.get_mut("BlockStatePalette") {
            palette[1] = Value::Compound(HashMap::from([
                ("Name".into(), Value::String("minecraft:chest".into())),
                (
                    "Properties".into(),
                    Value::Compound(HashMap::from([
                        ("facing".into(), Value::String("north".into())),
                        ("type".into(), Value::String("single".into())),
                        ("waterlogged".into(), Value::String("false".into())),
                    ])),
                ),
            ]));
        }
        region.insert("TileEntities".into(), Value::List(vec![tile]));
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        let original = gzip.finish().unwrap();
        fs::write(&input, &original).unwrap();
        let mut request = ConversionRequest {
            target_version: "26.3".into(),
            replacements: BTreeMap::new(),
            expected_source_sha256: None,
            overwrite: false,
            selection: None,
            property_edit: None,
        };
        let preview = convert(&input, None, &request).unwrap();
        assert!(preview.writable);
        assert!(!preview.payload_changes.is_empty());
        request.expected_source_sha256 = Some(preview.source_sha256);
        convert(&input, Some(&output), &request).unwrap();
        let root = read(&output);
        assert_eq!(
            compound(&root).unwrap()["MinecraftDataVersion"],
            Value::Int(5023)
        );
        let regions = compound(&compound(&root).unwrap()["Regions"]).unwrap();
        let Value::List(tiles) = &compound(&regions["test"]).unwrap()["TileEntities"] else {
            panic!()
        };
        let Value::List(items) = &compound(&tiles[0]).unwrap()["Items"] else {
            panic!()
        };
        let item = compound(&items[0]).unwrap();
        assert_eq!(item["count"], Value::Int(64));
        assert!(!item.contains_key("Count"));
        assert_eq!(fs::read(&input).unwrap(), original);
    }

    #[test]
    fn stale_preview_and_existing_destination_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.litematic");
        let output = dir.path().join("target.litematic");
        let original = fixture(&input, false);
        let mut request = request();
        request.expected_source_sha256 = Some("stale".into());
        assert!(convert(&input, Some(&output), &request).is_err());
        request.expected_source_sha256 = Some(format!("{:x}", Sha256::digest(&original)));
        fs::write(&output, b"user data").unwrap();
        assert!(convert(&input, Some(&output), &request).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"user data");
        assert!(convert(&input, Some(&input), &request).is_err());
        assert_eq!(fs::read(&input).unwrap(), original);
    }
    #[test]
    fn cache_reuses_parsed_source_and_frequency_layout_then_restores_after_cancel() {
        let dir=tempfile::tempdir().unwrap();let input=dir.path().join("cache.litematic");let original=fixture(&input,false);
        let mut cache=ConversionCache::new(16 << 20);let cancel=AtomicBool::new(false);
        let first=cache.convert(&input,None,&request(),&cancel,|_|{}).unwrap();
        let cached_root=cache.source.as_ref().unwrap().root.clone();assert_eq!(cache.source.as_ref().unwrap().frequencies.len(),1);assert_eq!(cache.source.as_ref().unwrap().frequencies["test"].frequencies,vec![1,3]);
        let mut stages=Vec::new();let second=cache.convert(&input,None,&request(),&cancel,|event|{assert!(event.cache_hit);stages.push(event.stage);}).unwrap();
        assert_eq!(first.source_sha256,second.source_sha256);assert!(!stages.contains(&"decode")&&!stages.contains(&"blocks"));
        assert_eq!(cache.source.as_ref().unwrap().root,cached_root);
        let error=cache.convert(&input,None,&request(),&cancel,|event|{if event.stage=="regions" {cancel.store(true,Ordering::Relaxed);}}).unwrap_err();
        assert!(error.downcast_ref::<ConversionCancelled>().is_some());assert_eq!(cache.source.as_ref().unwrap().root,cached_root);assert_eq!(fs::read(&input).unwrap(),original);
        cancel.store(false,Ordering::Relaxed);
        assert!(cache.convert(&input,None,&request(),&cancel,|_|{}).is_ok());
        let mut uncacheable=ConversionCache::new(1);assert!(uncacheable.convert(&input,None,&request(),&cancel,|_|{}).is_ok());assert!(uncacheable.source.is_none());
    }

    #[test]
    fn cached_write_rehashes_source_and_cancel_never_creates_output() {
        let dir=tempfile::tempdir().unwrap();let input=dir.path().join("source.litematic");let output=dir.path().join("target.litematic");let original=fixture(&input,false);
        let cancel=AtomicBool::new(false);let mut cache=ConversionCache::new(16 << 20);let mut req=request();let preview=cache.convert(&input,None,&req,&cancel,|_|{}).unwrap();req.expected_source_sha256=Some(preview.source_sha256);
        let error=cache.convert(&input,Some(&output),&req,&cancel,|event|{if event.stage=="write" {cancel.store(true,Ordering::Relaxed);}}).unwrap_err();
        assert!(error.downcast_ref::<ConversionCancelled>().is_some());assert!(!output.exists());assert_eq!(fs::read(&input).unwrap(),original);
        cancel.store(false,Ordering::Relaxed);fs::write(&input,b"changed").unwrap();
        assert!(cache.convert(&input,Some(&output),&req,&cancel,|_|{}).is_err());assert!(!output.exists());
    }

    #[test]
    fn ui_selection_bulk_and_property_edits_return_backend_authoritative_states() {
        let dir=tempfile::tempdir().unwrap();let input=dir.path().join("selection.litematic");fixture(&input,false);
        let mut root=read(&input);
        let Value::Compound(regions)=(match &mut root {Value::Compound(root)=>root.get_mut("Regions").unwrap(),_=>panic!()})else{panic!()};
        let Value::Compound(region)=regions.get_mut("test").unwrap()else{panic!()};
        let Value::List(palette)=region.get_mut("BlockStatePalette").unwrap()else{panic!()};
        let mut second=palette[1].clone();let Value::Compound(state)=&mut second else{panic!()};state.insert("Properties".into(),Value::Compound([("axis".into(),Value::String("x".into()))].into()));palette.push(second);
        let packed=crate::nbt::pack_palette_indices(&[1,2,1,0],bits_for_palette(3));
        region.insert("BlockStates".into(),Value::LongArray(fastnbt::LongArray::new((0..packed.len()).map(|index|packed.i64_at(index)).collect())));
        let mut gzip=GzEncoder::new(Vec::new(),Compression::default());gzip.write_all(&fastnbt::to_bytes(&root).unwrap()).unwrap();let original=gzip.finish().unwrap();fs::write(&input,&original).unwrap();
        let mut req=request();req.selection=Some(ConversionSelection{source_state:"minecraft:pale_oak_log[axis=z]".into(),target_block_id:"minecraft:spruce_log".into(),bulk:true});
        let preview=convert(&input,None,&req).unwrap();assert!(preview.writable,"{:?}",preview.blocked_payloads);assert_eq!(preview.source_version,"26.3");
        assert_eq!(preview.effective_replacements["minecraft:pale_oak_log[axis=z]"],"minecraft:spruce_log[axis=z]");
        assert_eq!(preview.effective_replacements["minecraft:pale_oak_log[axis=x]"],"minecraft:spruce_log[axis=x]");
        let row=preview.entries.iter().find(|row|row.source_state=="minecraft:pale_oak_log[axis=z]").unwrap();
        assert_eq!(row.source_icon_id,"minecraft:pale_oak_log");assert_eq!(row.target_block_id.as_deref(),Some("minecraft:spruce_log"));assert_eq!(row.target_properties["axis"],"z");assert!(row.target_property_options["axis"].contains(&"y".into()));
        req.selection=None;req.replacements=preview.effective_replacements;
        req.property_edit=Some(ConversionPropertyEdit{source_state:"minecraft:pale_oak_log[axis=z]".into(),properties:[("axis".into(),"y".into())].into()});
        let preview=convert(&input,None,&req).unwrap();assert_eq!(preview.effective_replacements["minecraft:pale_oak_log[axis=z]"],"minecraft:spruce_log[axis=y]");assert_eq!(preview.effective_replacements["minecraft:pale_oak_log[axis=x]"],"minecraft:spruce_log[axis=x]");
        req.property_edit.as_mut().unwrap().properties.insert("axis".into(),"invalid".into());assert!(convert(&input,None,&req).is_err());assert_eq!(fs::read(&input).unwrap(),original);
    }

}
