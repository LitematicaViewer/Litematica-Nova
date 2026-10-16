//! The pre-flattening target uses real MCEdit Alpha numeric Blocks/Data, not
//! modern identifiers with a changed DataVersion. Mappings come from the same
//! bundled database and preserve all explicitly representable states.
use crate::{
    block_database::Registry,
    nbt::{
        BigEndianLongArray, Vec3i, bits_for_palette, for_each_palette_index, pack_palette_indices,
        region_volume, storage_to_region_coords,
    },
    version_conversion::parse_key,
};
use anyhow::{Context, Result, bail};
use fastnbt::Value;
use std::collections::{BTreeMap, HashMap};
fn fields(value: &Value) -> Result<&HashMap<String, Value>> {
    match value {
        Value::Compound(value) => Ok(value),
        _ => bail!("转换节点不是 Compound"),
    }
}
fn number(value: Option<&Value>) -> Result<i32> {
    match value {
        Some(Value::Int(n)) => Ok(*n),
        Some(Value::Short(n)) => Ok(i32::from(*n)),
        _ => bail!("转换节点缺少坐标或尺寸"),
    }
}
fn vector(value: &Value) -> Result<Vec3i> {
    let map = fields(value)?;
    Ok(Vec3i {
        x: number(map.get("x"))?,
        y: number(map.get("y"))?,
        z: number(map.get("z"))?,
    })
}
fn state(value: &Value) -> Result<String> {
    let map = fields(value)?;
    let Some(Value::String(name)) = map.get("Name") else {
        bail!("调色板缺少 Name")
    };
    let mut props = BTreeMap::new();
    if let Some(props_value) = map.get("Properties") {
        for (k, v) in fields(props_value)? {
            let Value::String(v) = v else {
                bail!("调色板状态不是字符串")
            };
            props.insert(k, v);
        }
    }
    Ok(if props.is_empty() {
        name.clone()
    } else {
        format!(
            "{name}[{}]",
            props
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    })
}
fn numeric(registry: &Registry, state: &str) -> Result<(u16, u8)> {
    let (key, _) = registry
        .legacy_blocks
        .iter()
        .find(|(_, v)| v.as_str() == state)
        .context("目标状态没有数值 ID / data 映射")?;
    let (id, data) = key.split_once(':').context("数值状态格式损坏")?;
    Ok((id.parse()?, data.parse()?))
}
pub(crate) fn encode(root: &Value, registry: &Registry) -> Result<Value> {
    let source = fields(root)?;
    let Some(Value::Compound(regions)) = source.get("Regions") else {
        bail!("投影缺少 Regions")
    };
    let mut min = [i32::MAX; 3];
    let mut max = [i32::MIN; 3];
    for region in regions.values() {
        let map = fields(region)?;
        let p = vector(map.get("Position").context("选区缺少 Position")?)?;
        let s = vector(map.get("Size").context("选区缺少 Size")?)?;
        for (axis, (p, s)) in [(p.x, s.x), (p.y, s.y), (p.z, s.z)].into_iter().enumerate() {
            if s == 0 {
                bail!("空选区没有可转换体积")
            };
            let end = p
                .checked_add(if s < 0 { s + 1 } else { s - 1 })
                .context("选区坐标溢出")?;
            min[axis] = min[axis].min(p.min(end));
            max[axis] = max[axis].max(p.max(end));
        }
    }
    let mut dims = [0usize; 3];
    for axis in 0..3 {
        let n = i64::from(max[axis]) - i64::from(min[axis]) + 1;
        if !(1..=32767).contains(&n) {
            bail!("MCEdit 目标尺寸超出格式的 32767 限制")
        };
        dims[axis] = n as usize;
    }
    let volume = dims
        .into_iter()
        .try_fold(1usize, |a, b| a.checked_mul(b).context("目标尺寸溢出"))?;
    if volume > 50_000_000 {
        bail!("旧格式目标体积超出转换内存预算")
    }
    let mut blocks = vec![0i8; volume];
    let mut data = vec![0i8; volume];
    let mut add = vec![0i8; volume.div_ceil(2)];
    let mut occupied = vec![false; volume];
    let mut tile_entities = Vec::new();
    let mut entities = Vec::new();
    let mut ticks = Vec::new();
    for region in regions.values() {
        let map = fields(region)?;
        let position = vector(map.get("Position").context("选区缺少 Position")?)?;
        let size = vector(map.get("Size").context("选区缺少 Size")?)?;
        let volume = region_volume(&size)?;
        let Some(Value::List(palette)) = map.get("BlockStatePalette") else {
            bail!("选区缺少调色板")
        };
        let numeric: Vec<_> = palette
            .iter()
            .map(|entry| numeric(registry, &state(entry)?))
            .collect::<Result<_>>()?;
        let Some(Value::LongArray(packed)) = map.get("BlockStates") else {
            bail!("选区缺少 BlockStates")
        };
        let packed = BigEndianLongArray::new(packed.iter().copied().collect());
        for_each_palette_index(
            &packed,
            volume,
            bits_for_palette(palette.len()),
            palette.len(),
            |index, palette_index| {
                let (id, metadata) = *numeric.get(palette_index).context("调色板索引越界")?;
                let sx = size.x.unsigned_abs() as usize;
                let sz = size.z.unsigned_abs() as usize;
                let (x, y, z) = storage_to_region_coords(
                    index % sx,
                    index / (sx * sz),
                    (index / sx) % sz,
                    &size,
                );
                let (x, y, z) = (
                    (x + position.x - min[0]) as usize,
                    (y + position.y - min[1]) as usize,
                    (z + position.z - min[2]) as usize,
                );
                let target = (y * dims[2] + z) * dims[0] + x;
                if occupied[target]
                    && (blocks[target] as u8 != id as u8 || data[target] as u8 != metadata)
                {
                    bail!("重叠选区含冲突方块，旧格式无法无损合并")
                };
                occupied[target] = true;
                blocks[target] = id as u8 as i8;
                data[target] = metadata as i8;
                let upper = ((id >> 8) & 15) as i8;
                add[target / 2] |= upper << ((target % 2) * 4);
                Ok(())
            },
        )?;
        if let Some(Value::List(values)) = map.get("TileEntities") {
            for value in values {
                let mut value = value.clone();
                let Value::Compound(fields) = &mut value else {
                    bail!("方块实体不是 Compound")
                };
                for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
                    let p = [position.x, position.y, position.z][axis]
                        + ([size.x, size.y, size.z][axis] + 1).min(0);
                    let n = number(fields.get(name))?;
                    fields.insert(name.into(), Value::Int(n + p - min[axis]));
                }
                tile_entities.push(value);
            }
        }
        if let Some(Value::List(values)) = map.get("Entities") {
            for value in values {
                let mut value = value.clone();
                shift_entity(&mut value, [position.x + (size.x + 1).min(0) - min[0], position.y + (size.y + 1).min(0) - min[1], position.z + (size.z + 1).min(0) - min[2]])?;
                entities.push(value);
            }
        }
        if let Some(Value::List(values)) = map.get("PendingBlockTicks") {
            for value in values {
                let source = fields(value)?;
                let Some(Value::String(id)) = source.get("Block") else { bail!("方块计划刻缺少Block") };
                let legacy_id = registry.legacy_state_aliases.iter().find_map(|(legacy, states)| states.iter().any(|state| parse_key(state).is_ok_and(|(name, _)| name == *id)).then(|| parse_key(legacy).map(|(name, _)| name))).transpose()?.unwrap_or_else(|| id.clone());
                let time = match source.get("Time") { Some(Value::Long(value)) => i32::try_from(*value).context("目标旧版计划刻时间超出Int范围")?, Some(Value::Int(value)) => *value, _ => bail!("计划刻时间无效") };
                if source.get("SubTick").is_some_and(|value| value != &Value::Long(0)) { bail!("目标旧版没有计划刻SubTick排序字段，原文件保留") }
                let mut tick = HashMap::from([("i".into(), Value::String(legacy_id)), ("t".into(), Value::Int(time)), ("p".into(), source.get("Priority").cloned().unwrap_or(Value::Int(0)))]);
                for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
                    let p = [position.x, position.y, position.z][axis] + ([size.x, size.y, size.z][axis] + 1).min(0);
                    tick.insert(name.into(), Value::Int(number(source.get(name))? + p - min[axis]));
                }
                ticks.push(Value::Compound(tick));
            }
        }
    }
    let mut output = HashMap::from([
        ("Materials".into(), Value::String("Alpha".into())),
        ("Width".into(), Value::Short(dims[0] as i16)),
        ("Height".into(), Value::Short(dims[1] as i16)),
        ("Length".into(), Value::Short(dims[2] as i16)),
        (
            "Blocks".into(),
            Value::ByteArray(fastnbt::ByteArray::new(blocks)),
        ),
        (
            "Data".into(),
            Value::ByteArray(fastnbt::ByteArray::new(data)),
        ),
        ("TileEntities".into(), Value::List(tile_entities)),
        ("Entities".into(), Value::List(entities)),
        ("TileTicks".into(), Value::List(ticks)),
        ("WEOffsetX".into(), Value::Int(min[0])),
        ("WEOffsetY".into(), Value::Int(min[1])),
        ("WEOffsetZ".into(), Value::Int(min[2])),
    ]);
    if add.iter().any(|v| *v != 0) {
        output.insert(
            "AddBlocks".into(),
            Value::ByteArray(fastnbt::ByteArray::new(add)),
        );
    }
    if let Some(dv) = source.get("MinecraftDataVersion") {
        output.insert("MinecraftDataVersion".into(), dv.clone());
    }
    // Preserve non-format metadata/extension fields in a namespaced envelope,
    // not inside legacy Minecraft block/entity/item payloads.
    let mut envelope = source.clone();
    let mut region_metadata = HashMap::new();
    for (name, region) in regions {
        let mut metadata = fields(region)?.clone();
        for key in [
            "BlockStates",
            "BlockStatePalette",
            "Entities",
            "TileEntities",
            "PendingBlockTicks",
            "PendingFluidTicks",
        ] {
            metadata.remove(key);
        }
        region_metadata.insert(name.clone(), Value::Compound(metadata));
    }
    envelope.insert(
        "NovaOriginalRegions".into(),
        Value::Compound(region_metadata),
    );
    envelope.remove("Regions");
    output.insert("NovaSourceMetadata".into(), Value::Compound(envelope));
    Ok(Value::Compound(output))
}

fn shift_entity(value: &mut Value, offset: [i32; 3]) -> Result<()> {
    let Value::Compound(fields) = value else { bail!("实体不是Compound") };
    if let Some(Value::List(position)) = fields.get_mut("Pos") {
        if position.len() != 3 { bail!("实体Pos必须有三个坐标") }
        for (axis, value) in position.iter_mut().enumerate() { let Value::Double(value) = value else { bail!("实体Pos必须为Double") }; *value += f64::from(offset[axis]); }
    } else { bail!("实体缺少Pos") }
    for (axis, key) in ["TileX", "TileY", "TileZ"].into_iter().enumerate() {
        if let Some(Value::Int(value)) = fields.get_mut(key) { *value = value.checked_add(offset[axis]).context("悬挂实体坐标溢出")?; }
    }
    if let Some(Value::List(passengers)) = fields.get_mut("Passengers") { for passenger in passengers { shift_entity(passenger, offset)?; } }
    if let Some(mount)=fields.get_mut("Riding"){shift_entity(mount,offset)?;}
    Ok(())
}
pub(crate) fn decode(root: &Value, registry: &Registry) -> Result<Value> {
    let map = fields(root)?;
    let (x, y, z) = (
        number(map.get("Width"))?,
        number(map.get("Height"))?,
        number(map.get("Length"))?,
    );
    let size = Vec3i { x, y, z };
    let volume = region_volume(&size)?;
    let Some(Value::ByteArray(blocks)) = map.get("Blocks") else {
        bail!("旧格式缺少 Blocks")
    };
    let Some(Value::ByteArray(data)) = map.get("Data") else {
        bail!("旧格式缺少 Data")
    };
    if blocks.len() != volume || data.len() != volume {
        bail!("旧格式方块数组长度错误")
    }
    let add = match map.get("AddBlocks") {
        Some(Value::ByteArray(a)) => {
            if a.len() != volume.div_ceil(2) {
                bail!("旧格式 AddBlocks 长度错误")
            }
            Some(a)
        }
        None => None,
        _ => bail!("旧格式 AddBlocks 类型错误"),
    };
    let mut palette = Vec::new();
    let mut lookup = BTreeMap::new();
    let mut indices = Vec::with_capacity(volume);
    for (index, block) in blocks.iter().enumerate() {
        let high = add
            .map(|a| ((a[index / 2] as u8) >> ((index % 2) * 4)) & 15)
            .unwrap_or(0);
        let id = u16::from(*block as u8) | (u16::from(high) << 8);
        let metadata = data[index] as u8;
        if metadata > 15 {
            bail!("旧格式 data 不是四位状态值")
        };
        let state = registry
            .legacy_blocks
            .get(&format!("{id}:{metadata}"))
            .context("旧格式存在未核实的数值状态")?;
        let p = if let Some(index) = lookup.get(state) {
            *index
        } else {
            let index = palette.len();
            let (name, props) = parse_key(state)?;
            let mut block = HashMap::from([("Name".into(), Value::String(name))]);
            if !props.is_empty() {
                block.insert(
                    "Properties".into(),
                    Value::Compound(
                        props
                            .into_iter()
                            .map(|(k, v)| (k, Value::String(v)))
                            .collect(),
                    ),
                );
            }
            palette.push(Value::Compound(block));
            lookup.insert(state.clone(), index);
            index
        };
        indices.push(p);
    }
    let packed = pack_palette_indices(&indices, bits_for_palette(palette.len()));
    let vector = |x, y, z| {
        Value::Compound(HashMap::from([
            ("x".into(), Value::Int(x)),
            ("y".into(), Value::Int(y)),
            ("z".into(), Value::Int(z)),
        ]))
    };
    let mut region = Value::Compound(HashMap::from([
        (
            "Position".into(),
            vector(
                number(map.get("WEOffsetX")).unwrap_or(0),
                number(map.get("WEOffsetY")).unwrap_or(0),
                number(map.get("WEOffsetZ")).unwrap_or(0),
            ),
        ),
        ("Size".into(), vector(x, y, z)),
        ("BlockStatePalette".into(), Value::List(palette)),
        (
            "BlockStates".into(),
            Value::LongArray(fastnbt::LongArray::new(
                (0..packed.len())
                    .map(|index| packed.i64_at(index))
                    .collect(),
            )),
        ),
        (
            "TileEntities".into(),
            map.get("TileEntities")
                .cloned()
                .unwrap_or(Value::List(vec![])),
        ),
        (
            "Entities".into(),
            map.get("Entities").cloned().unwrap_or(Value::List(vec![])),
        ),
    ]));
    if let Some(Value::List(ticks)) = map.get("TileTicks") {
        let mut scheduled = Vec::new();
        for tick in ticks {
            let tick = fields(tick)?;
            let Some(Value::String(id)) = tick.get("i") else { bail!("旧计划刻i不是资源名称") };
            let modern_id = registry.legacy_state_aliases.iter().find_map(|(legacy, states)| parse_key(legacy).ok().filter(|(name, _)| name == id).and_then(|_| states.first()).and_then(|state| parse_key(state).ok().map(|(name, _)| name))).unwrap_or_else(|| id.clone());
            let mut fields = HashMap::from([("Block".into(), Value::String(modern_id)), ("Time".into(), Value::Long(i64::from(number(tick.get("t"))?))), ("Priority".into(), tick.get("p").cloned().unwrap_or(Value::Int(0)))]);
            for axis in ["x", "y", "z"] { fields.insert(axis.into(), Value::Int(number(tick.get(axis))?)); }
            scheduled.push(Value::Compound(fields));
        }
        let Value::Compound(fields) = &mut region else { unreachable!() };
        fields.insert("PendingBlockTicks".into(), Value::List(scheduled));
    }
    let mut output = match map.get("NovaSourceMetadata") {
        Some(Value::Compound(m)) => m.clone(),
        _ => HashMap::new(),
    };
    output.insert("Version".into(), Value::Int(6));
    output.insert(
        "MinecraftDataVersion".into(),
        map.get("MinecraftDataVersion")
            .cloned()
            .context("旧格式缺少已核实的来源版本")?,
    );
    output.insert(
        "Regions".into(),
        Value::Compound(HashMap::from([("Schematic".into(), region)])),
    );
    if !output.contains_key("Metadata") {
        output.insert(
            "Metadata".into(),
            Value::Compound(HashMap::from([(
                "Name".into(),
                Value::String("Schematic".into()),
            )])),
        );
    }
    Ok(Value::Compound(output))
}
