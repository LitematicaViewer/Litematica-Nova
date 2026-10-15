//! Explicit, lossless-envelope conversion of static Litematica block data.
//! Unsupported entity/item/tick schemas block writing instead of changing
//! DataVersion on partly converted data. No Java oracle is invoked at runtime.
use crate::block_database::{Registry as BlockRegistry, database};
use crate::nbt::{
    BigEndianLongArray, Vec3i, bits_for_palette, decode_palette_frequencies, region_volume,
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
    path::Path,
};

pub(crate) fn target_has_item(data_version: i32, id: &str) -> Result<bool> {
    let registry = database()?.registry_by_data_version(data_version)?;
    Ok(registry.items.contains(id))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConversionRequest {
    pub target_version: String,
    #[serde(default)]
    pub replacements: BTreeMap<String, String>,
    pub expected_source_sha256: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
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
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionPreview {
    pub source_sha256: String,
    pub source_data_version: i32,
    pub target_version: String,
    pub target_data_version: i32,
    pub entries: Vec<ConversionEntry>,
    pub blocked_payloads: Vec<String>,
    pub payload_issues: Vec<ConversionPayloadIssue>,
    pub payload_changes: Vec<String>,
    pub writable: bool,
    pub changed_block_count: u64,
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
}

fn payload_type_name(id: &str) -> &str {
    match id {
        "minecraft:banner" => "旗帜",
        "minecraft:hanging_sign" => "悬挂告示牌",
        "minecraft:sign" => "告示牌",
        "minecraft:decorated_pot" => "饰纹陶罐",
        "minecraft:armor_stand" => "盔甲架",
        "minecraft:item_frame" => "物品展示框",
        "minecraft:glow_item_frame" => "荧光物品展示框",
        "minecraft:painting" => "画",
        "minecraft:tropical_fish" => "热带鱼",
        "minecraft:frog" => "青蛙",
        "minecraft:parrot" => "鹦鹉",
        "minecraft:boat" => "船",
        "minecraft:chest"
        | "minecraft:barrel"
        | "minecraft:hopper"
        | "minecraft:shulker_box"
        | "minecraft:dropper"
        | "minecraft:dispenser" => "容器物品",
        "PendingBlockTicks" | "PendingFluidTicks" => "计划刻",
        _ => "附加数据",
    }
}

fn group_payload_issue(region: &Value, issue: &str) -> (String, String, String, String) {
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
        payload_type_name(id).into(),
        format!("{field}[{index}]"),
        message.into(),
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

pub fn convert(
    input: &Path,
    output: Option<&Path>,
    request: &ConversionRequest,
) -> Result<ConversionPreview> {
    let bytes = fs::read(input).with_context(|| format!("读取投影 {}", input.display()))?;
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    if request
        .expected_source_sha256
        .as_ref()
        .is_some_and(|expected| expected != &source_sha256)
    {
        bail!("原文件已变化，请重新预览转换差异")
    }
    let mut raw = Vec::new();
    GzDecoder::new(&bytes[..]).read_to_end(&mut raw)?;
    let mut root: Value = fastnbt::from_bytes(&raw).context("解析完整投影 NBT")?;
    let source_data_version = integer(compound(&root)?.get("MinecraftDataVersion"))?;
    let db = database()?;
    if compound(&root)?.get("Materials") == Some(&Value::String("Alpha".into())) {
        root = crate::version_legacy_encoding::decode(
            &root,
            db.registry_by_data_version(source_data_version)?,
        )?;
    }
    let target = db.version(&request.target_version)?;
    let registry = db.registry(&request.target_version)?;
    let mut entries: BTreeMap<String, ConversionEntry> = BTreeMap::new();
    let mut blocked_payloads = Vec::new();
    let mut payload_changes = Vec::new();
    let mut payload_issue_groups: BTreeMap<
        (String, String, String),
        std::collections::BTreeSet<String>,
    > = BTreeMap::new();
    let root_map = match &mut root {
        Value::Compound(map) => map,
        _ => bail!("无效的投影根节点"),
    };
    let Some(Value::Compound(regions)) = root_map.get_mut("Regions") else {
        bail!("投影缺少 Regions")
    };
    for (region_name, region) in regions {
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
        let frequencies = decode_palette_frequencies(
            &BigEndianLongArray::new(packed.iter().copied().collect()),
            volume,
            bits_for_palette(palette.len()),
            palette.len(),
        )?;
        let Some(Value::List(palette)) = fields.get_mut("BlockStatePalette") else {
            unreachable!()
        };
        for (index, palette_value) in palette.iter_mut().enumerate() {
            let (name, props) = state(palette_value)?;
            let old = key(&name, &props);
            let (semantic_name, semantic_props) = if source_data_version < 1519 {
                db.registry_by_data_version(source_data_version)?
                    .legacy_state_aliases
                    .get(&old)
                    .and_then(|states| {
                        states
                            .iter()
                            .find(|state| {
                                parse_key(state)
                                    .ok()
                                    .is_some_and(|(id, _)| registry.blocks.contains_key(&id))
                            })
                            .or_else(|| states.first())
                    })
                    .map(|state| parse_key(state))
                    .transpose()?
                    .unwrap_or_else(|| (name.clone(), props.clone()))
            } else {
                (name.clone(), props.clone())
            };
            let target_name = migrated_block_name(&semantic_name, source_data_version, target.data_version);
            let compatible = valid(registry, &old);
            let chosen = if let Some(replacement) = request.replacements.get(&old) {
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
            let valid = chosen
                .as_ref()
                .is_some_and(|chosen| valid(registry, chosen));
            if !compatible || chosen.as_ref() != Some(&old) {
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
        if compound(region)?.iter().any(|(field, payload)| {
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
            let migration = crate::version_payload_migration::migrate_with_palette(
                region,
                source_data_version,
                target.data_version,
                &original_names,
            )?;
            for issue in &migration.issues {
                let (kind, name, identity, message) = group_payload_issue(region, issue);
                payload_issue_groups
                    .entry((kind, name, message))
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
        source_sha256,
        source_data_version,
        target_version: request.target_version.clone(),
        target_data_version: target.data_version,
        entries: entries.into_values().collect(),
        blocked_payloads,
        payload_issues: payload_issue_groups
            .into_iter()
            .map(
                |((kind, type_name, message), identities)| ConversionPayloadIssue {
                    kind,
                    type_name,
                    count: identities.len(),
                    message,
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
        output_path: None,
        output_extension: target.output_extension.clone(),
    };
    if let Some(output) = output {
        if request.expected_source_sha256.is_none() {
            bail!("另存前须预览并确认原文件哈希")
        }
        if !preview.writable {
            bail!("仍有未转换的方块状态或版本相关 NBT，另存已停止")
        }
        if output.exists() && !request.overwrite {
            bail!("另存目标已存在，不覆盖投影或原件")
        }
        let Value::Compound(map) = &mut root else {
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
        if target.legacy {
            root = crate::version_legacy_encoding::encode(&root, registry)?;
        }
        let converted = if target.legacy {
            fastnbt::to_bytes_with_opts(&root, fastnbt::SerOpts::new().root_name("Schematic"))?
        } else {
            fastnbt::to_bytes(&root)?
        };
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&converted)?;
        let converted = gzip.finish()?;
        if request.overwrite {
            atomic_replace(output, &converted)?;
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
        preview.output_path = Some(output.display().to_string());
    }
    Ok(preview)
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
        let mut request = ConversionRequest { target_version: "26.3".into(), replacements: BTreeMap::new(), expected_source_sha256: None, overwrite: false };
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
}
