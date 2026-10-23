//! Public, lossless projection -> Java 26.3 Anvil world writer.
//! No renderer or protected runtime is involved. A new directory is required.
use crate::{model::BlockStateNbt, nbt};
use anyhow::{Context, Result, bail, ensure};
use fastnbt::{IntArray, LongArray, Value};
use flate2::{
    Compression,
    write::{GzEncoder, ZlibEncoder},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

type Tag = HashMap<String, Value>;
const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;
const TARGET: &str = "26.3";

#[derive(Clone, Copy, Debug, Serialize)]
pub struct WorldPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldExportResult {
    pub output_path: String,
    pub game_version: String,
    pub data_version: i32,
    pub block_count: u64,
    pub entity_count: usize,
    pub block_entity_count: usize,
    pub chunk_count: usize,
    pub spawn_platform_blocks: usize,
    pub spawn: WorldPosition,
}
#[derive(Default)]
struct Chunk {
    sections: BTreeMap<i32, Box<[u32; 4096]>>,
    block_entities: Vec<Value>,
    entities: Vec<Value>,
}
fn compound(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Compound(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}
fn text(s: impl Into<String>) -> Value {
    Value::String(s.into())
}
fn coord(pos: [i32; 3]) -> Value {
    Value::IntArray(IntArray::new(pos.to_vec()))
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn add(a: i32, b: i32) -> Result<i32> {
    a.checked_add(b).context("导出坐标溢出")
}
fn offset(p: [i32; 3], d: [i32; 3]) -> Result<[i32; 3]> {
    Ok([add(p[0], d[0])?, add(p[1], d[1])?, add(p[2], d[2])?])
}
fn valid_position(p: [i32; 3]) -> Result<()> {
    ensure!(
        (MIN_Y..=MAX_Y).contains(&p[1]),
        "方块坐标 Y={} 超出 26.3 主世界高度 {}..{}，未裁剪数据",
        p[1],
        MIN_Y,
        MAX_Y
    );
    ensure!(
        p[0].unsigned_abs() < 29_999_984 && p[2].unsigned_abs() < 29_999_984,
        "导出坐标超出世界边界"
    );
    Ok(())
}
fn put_block(chunks: &mut BTreeMap<(i32, i32), Chunk>, p: [i32; 3], state: u32) -> Result<bool> {
    valid_position(p)?;
    let section = chunks
        .entry((p[0].div_euclid(16), p[2].div_euclid(16)))
        .or_default()
        .sections
        .entry(p[1].div_euclid(16))
        .or_insert_with(|| Box::new([0; 4096]));
    let index =
        ((p[1].rem_euclid(16) << 8) | (p[2].rem_euclid(16) << 4) | p[0].rem_euclid(16)) as usize;
    let previous = section[index];
    ensure!(
        previous == 0 || previous == state,
        "选区在 {},{},{} 有冲突的非空气方块，未覆盖数据",
        p[0],
        p[1],
        p[2]
    );
    section[index] = state;
    Ok(previous == 0)
}
fn state_tag(state: &BlockStateNbt) -> Value {
    let mut tag = Tag::from([("Name".into(), text(&state.name))]);
    if !state.properties.is_empty() {
        tag.insert(
            "Properties".into(),
            Value::Compound(
                state
                    .properties
                    .iter()
                    .map(|(k, v)| (k.clone(), text(v)))
                    .collect(),
            ),
        );
    }
    Value::Compound(tag)
}
fn section_tag(y: i32, blocks: Option<&[u32; 4096]>, states: &[BlockStateNbt]) -> Value {
    let mut palette = Vec::new();
    let mut lookup = HashMap::<u32, u32>::new();
    let mut indices = Vec::with_capacity(4096);
    for i in 0..4096 {
        let state = blocks.map_or(0, |b| b[i]);
        let next = palette.len() as u32;
        let local = *lookup.entry(state).or_insert_with(|| {
            palette.push(state_tag(&states[state as usize]));
            next
        });
        indices.push(local);
    }
    let mut block_states = Tag::from([("palette".into(), Value::List(palette))]);
    if lookup.len() > 1 {
        // Modern Anvil does not permit one entry to straddle two longs.
        let bits = (usize::BITS - (lookup.len() - 1).leading_zeros()).max(4) as usize;
        let per_long = 64 / bits;
        let mut packed = vec![0i64; 4096usize.div_ceil(per_long)];
        for (i, value) in indices.into_iter().enumerate() {
            packed[i / per_long] |= ((value as u64) << ((i % per_long) * bits)) as i64;
        }
        block_states.insert("data".into(), Value::LongArray(LongArray::new(packed)));
    }
    compound([
        ("Y", Value::Byte(y as i8)),
        ("block_states", Value::Compound(block_states)),
        (
            "biomes",
            compound([("palette", Value::List(vec![text("minecraft:the_void")]))]),
        ),
    ])
}
fn chunk_tag(cx: i32, cz: i32, chunk: &Chunk, states: &[BlockStateNbt], dv: i32) -> Value {
    compound([
        ("DataVersion", Value::Int(dv)),
        ("xPos", Value::Int(cx)),
        ("yPos", Value::Int(-4)),
        ("zPos", Value::Int(cz)),
        ("Status", text("minecraft:full")),
        ("LastUpdate", Value::Long(0)),
        ("InhabitedTime", Value::Long(0)),
        ("isLightOn", Value::Byte(0)),
        (
            "sections",
            Value::List(
                (-4..20)
                    .map(|y| section_tag(y, chunk.sections.get(&y).map(|b| b.as_ref()), states))
                    .collect(),
            ),
        ),
        ("block_entities", Value::List(chunk.block_entities.clone())),
        ("block_ticks", Value::List(vec![])),
        ("fluid_ticks", Value::List(vec![])),
        (
            "PostProcessing",
            Value::List((0..24).map(|_| Value::List(vec![])).collect()),
        ),
        ("Heightmaps", compound([])),
        (
            "structures",
            compound([("starts", compound([])), ("References", compound([]))]),
        ),
    ])
}
fn integer(v: Option<&Value>, label: &str) -> Result<i32> {
    match v {
        Some(Value::Int(v)) => Ok(*v),
        Some(Value::Short(v)) => Ok(*v as i32),
        Some(Value::Byte(v)) => Ok(*v as i32),
        _ => bail!("{} 缺少有效整数坐标", label),
    }
}
fn entity_position(tag: &Tag) -> Result<[f64; 3]> {
    let Some(Value::List(pos)) = tag.get("Pos") else {
        bail!("实体 {} 缺少 Pos", entity_label(tag));
    };
    ensure!(pos.len() == 3, "实体 Pos 不是三维坐标");
    let mut out = [0.0; 3];
    for (i, v) in pos.iter().enumerate() {
        out[i] = match v {
            Value::Double(n) => *n,
            Value::Float(n) => *n as f64,
            _ => bail!("实体 Pos 不是数值"),
        };
        ensure!(out[i].is_finite(), "实体坐标非有限数");
    }
    Ok(out)
}
fn entity_label(tag: &Tag) -> &str {
    match tag.get("id") {
        Some(Value::String(id)) => id,
        _ => "<missing id>",
    }
}
fn move_entity(tag: &mut Tag, delta: [i32; 3], uuids: &mut HashSet<Vec<i32>>) -> Result<()> {
    ensure!(
        matches!(tag.get("id"), Some(Value::String(_))),
        "实体缺少 id"
    );
    let p = entity_position(tag)?;
    tag.insert(
        "Pos".into(),
        Value::List(
            (0..3)
                .map(|i| Value::Double(p[i] + delta[i] as f64))
                .collect(),
        ),
    );
    if let Some(Value::IntArray(uuid)) = tag.get("UUID") {
        ensure!(
            uuids.insert(uuid.iter().copied().collect()),
            "重复实体 UUID：{}，未让游戏静默丢弃实体",
            entity_label(tag)
        );
    }
    if let Some(Value::IntArray(pos)) = tag.get("block_pos") {
        ensure!(pos.len() == 3, "悬挂实体 block_pos 无效");
        let p = offset([pos[0], pos[1], pos[2]], delta)?;
        tag.insert("block_pos".into(), coord(p));
    }
    if let Some(Value::List(passengers)) = tag.get_mut("Passengers") {
        for passenger in passengers {
            let Value::Compound(child) = passenger else {
                bail!("Passengers 包含无效实体")
            };
            move_entity(child, delta, uuids)?;
        }
    }
    Ok(())
}
fn write_gzip(path: &Path, value: &Value) -> Result<()> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut gz = GzEncoder::new(file, Compression::default());
    gz.write_all(&fastnbt::to_bytes(value)?)?;
    gz.finish()?.sync_all()?;
    Ok(())
}
fn write_regions(
    directory: &Path,
    chunks: &BTreeMap<(i32, i32), Chunk>,
    states: &[BlockStateNbt],
    dv: i32,
    entities: bool,
) -> Result<()> {
    fs::create_dir(directory)?;
    let mut regions = BTreeMap::<(i32, i32), Vec<(i32, i32)>>::new();
    for (&(cx, cz), chunk) in chunks {
        if !entities || !chunk.entities.is_empty() {
            regions
                .entry((cx.div_euclid(32), cz.div_euclid(32)))
                .or_default()
                .push((cx, cz));
        }
    }
    for ((rx, rz), positions) in regions {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("r.{rx}.{rz}.mca")))?;
        let mut header = [0u8; 8192];
        file.write_all(&header)?;
        let mut sector = 2u32;
        for (cx, cz) in positions {
            let chunk = &chunks[&(cx, cz)];
            let value = if entities {
                compound([
                    ("DataVersion", Value::Int(dv)),
                    ("Position", Value::IntArray(IntArray::new(vec![cx, cz]))),
                    ("Entities", Value::List(chunk.entities.clone())),
                ])
            } else {
                chunk_tag(cx, cz, chunk, states, dv)
            };
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&fastnbt::to_bytes(&value)?)?;
            let compressed = encoder.finish()?;
            let needed = (compressed.len() + 5).div_ceil(4096);
            let external = needed > 255;
            let count = if external { 1 } else { needed };
            ensure!(sector < (1 << 24), "Anvil 区域地址溢出");
            let index = (cx.rem_euclid(32) + cz.rem_euclid(32) * 32) as usize;
            header[index * 4..index * 4 + 4]
                .copy_from_slice(&((sector << 8) | count as u32).to_be_bytes());
            header[4096 + index * 4..4100 + index * 4]
                .copy_from_slice(&((now_ms() / 1000) as u32).to_be_bytes());
            if external {
                let mut ext = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(directory.join(format!("c.{cx}.{cz}.mcc")))?;
                ext.write_all(&compressed)?;
                ext.sync_all()?;
                file.write_all(&1u32.to_be_bytes())?;
                file.write_all(&[0x82])?;
                file.write_all(&vec![0; 4091])?;
            } else {
                file.write_all(&((compressed.len() + 1) as u32).to_be_bytes())?;
                file.write_all(&[2])?;
                file.write_all(&compressed)?;
                file.write_all(&vec![0; count * 4096 - compressed.len() - 5])?;
            }
            sector += count as u32;
        }
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header)?;
        file.sync_all()?;
    }
    Ok(())
}
fn world_generation(dv: i32) -> Value {
    let dimension = |kind: &str| {
        compound([
            ("type", text(kind)),
            (
                "generator",
                compound([
                    ("type", text("minecraft:flat")),
                    (
                        "settings",
                        compound([
                            ("layers", Value::List(vec![])),
                            ("biome", text("minecraft:the_void")),
                            ("structure_overrides", Value::List(vec![])),
                            ("lakes", Value::Byte(0)),
                            ("features", Value::Byte(0)),
                        ]),
                    ),
                ]),
            ),
        ])
    };
    compound([
        ("DataVersion", Value::Int(dv)),
        (
            "data",
            compound([
                ("seed", Value::Long(0)),
                ("generate_structures", Value::Byte(0)),
                ("bonus_chest", Value::Byte(0)),
                (
                    "dimensions",
                    compound([
                        ("minecraft:overworld", dimension("minecraft:overworld")),
                        ("minecraft:the_nether", dimension("minecraft:the_nether")),
                        ("minecraft:the_end", dimension("minecraft:the_end")),
                    ]),
                ),
            ]),
        ),
    ])
}
fn level_data(name: &str, spawn: WorldPosition, dv: i32) -> Value {
    compound([(
        "Data",
        compound([
            ("DataVersion", Value::Int(dv)),
            ("version", Value::Int(19133)),
            (
                "Version",
                compound([
                    ("Name", text(TARGET)),
                    ("Id", Value::Int(dv)),
                    ("Snapshot", Value::Byte(0)),
                    ("Series", text("main")),
                ]),
            ),
            ("LevelName", text(name)),
            ("GameType", Value::Int(1)),
            ("allowCommands", Value::Byte(1)),
            ("initialized", Value::Byte(1)),
            ("Time", Value::Long(6000)),
            ("DayTime", Value::Long(6000)),
            ("LastPlayed", Value::Long(now_ms())),
            (
                "spawn",
                compound([
                    ("dimension", text("minecraft:overworld")),
                    ("pos", coord([spawn.x, spawn.y, spawn.z])),
                    ("yaw", Value::Float(-90.0)),
                    ("pitch", Value::Float(0.0)),
                ]),
            ),
            (
                "difficulty_settings",
                compound([
                    ("difficulty", text("normal")),
                    ("hardcore", Value::Byte(0)),
                    ("locked", Value::Byte(0)),
                ]),
            ),
            (
                "DataPacks",
                compound([
                    ("Enabled", Value::List(vec![text("vanilla")])),
                    ("Disabled", Value::List(vec![])),
                ]),
            ),
            (
                "enabled_features",
                Value::List(vec![text("minecraft:vanilla")]),
            ),
        ]),
    )])
}

pub fn export_world(
    input: &Path,
    output: &Path,
    name: &str,
    origin: [i32; 3],
) -> Result<WorldExportResult> {
    ensure!(input.is_file(), "源投影不存在");
    ensure!(!output.exists(), "输出目录已存在；请选择新目录，不覆盖存档");
    ensure!(!name.trim().is_empty(), "世界名称不能为空");
    let context = crate::game_data::active_context()?;
    ensure!(
        context.game_version == TARGET,
        "当前世界写出支持 {TARGET}；已选游戏数据 {} 不具备世界写出适配",
        context.game_version
    );
    let database = crate::block_database::database()?;
    let target = database.version(TARGET)?;
    let registry = database.registry(TARGET)?;
    let root = nbt::load_litematic_root_file_backed_with_progress(input, &mut |_| {})?;
    let bounds = crate::visual::compute_bounds(&root).context("投影没有选区")?;
    let delta = [
        origin[0].checked_sub(bounds.min_x).context("X 坐标溢出")?,
        origin[1].checked_sub(bounds.min_y).context("Y 坐标溢出")?,
        origin[2].checked_sub(bounds.min_z).context("Z 坐标溢出")?,
    ];
    let has_payload = root
        .regions
        .values()
        .any(|r| !r.entities.is_empty() || !r.tile_entities.is_empty());
    ensure!(
        !has_payload || root.minecraft_data_version == Some(target.data_version),
        "源数据版本 {:?} 的实体/方块实体尚无到 {TARGET}（{}）的等价世界写出适配；未丢弃或重标数据版本",
        root.minecraft_data_version,
        target.data_version
    );
    let air = BlockStateNbt {
        name: "minecraft:air".into(),
        properties: BTreeMap::new(),
    };
    let mut states = vec![air.clone()];
    let mut state_ids = BTreeMap::from([(serde_json::to_string(&air)?, 0u32)]);
    let mut chunks = BTreeMap::<(i32, i32), Chunk>::new();
    let mut block_count = 0u64;
    let mut entity_count = 0;
    let mut block_entity_count = 0;
    let mut block_entity_positions = HashSet::new();
    let mut uuids = HashSet::new();
    for (region_name, region) in &root.regions {
        let rd = offset(
            [region.position.x, region.position.y, region.position.z],
            delta,
        )?;
        let mut mapped = Vec::with_capacity(region.block_state_palette.len());
        for state in &region.block_state_palette {
            if state.name == "minecraft:air" {
                mapped.push(0);
                continue;
            }
            let block = registry.blocks.get(&state.name).with_context(|| {
                format!("{TARGET} 没有方块 {}（选区 {region_name}）", state.name)
            })?;
            ensure!(
                block.accepts(&state.name, &state.properties),
                "{TARGET} 不接受方块状态 {} {:?}；请先做明确的版本转换",
                state.name,
                state.properties
            );
            let key = serde_json::to_string(state)?;
            let next = states.len() as u32;
            let id = *state_ids.entry(key).or_insert_with(|| {
                states.push(state.clone());
                next
            });
            mapped.push(id);
        }
        let width = region.size.x.unsigned_abs() as usize;
        let length = region.size.z.unsigned_abs() as usize;
        nbt::for_each_palette_index(
            &region.block_states,
            nbt::region_volume(&region.size)?,
            nbt::bits_for_palette(mapped.len()),
            mapped.len(),
            |i, palette| {
                let state = mapped[palette];
                if state == 0 {
                    return Ok(());
                }
                let y = i / (width * length);
                let z = (i % (width * length)) / width;
                let x = i % width;
                let (x, y, z) = nbt::storage_to_region_coords(x, y, z, &region.size);
                if put_block(&mut chunks, offset([x, y, z], rd)?, state)? {
                    block_count += 1;
                }
                Ok(())
            },
        )
        .with_context(|| format!("导出选区 {region_name}"))?;
        for entity in &region.tile_entities {
            let Value::Compound(original) = entity else {
                bail!("选区 {region_name} 包含非复合方块实体")
            };
            let mut tag = original.clone();
            let p = offset(
                [
                    integer(tag.get("x"), "方块实体 x")?,
                    integer(tag.get("y"), "方块实体 y")?,
                    integer(tag.get("z"), "方块实体 z")?,
                ],
                rd,
            )?;
            valid_position(p)?;
            ensure!(block_entity_positions.insert(p), "重复方块实体坐标 {p:?}");
            ensure!(
                matches!(tag.get("id"), Some(Value::String(_))),
                "方块实体 {p:?} 缺少 id"
            );
            for (axis, value) in ["x", "y", "z"].into_iter().zip(p) {
                tag.insert(axis.into(), Value::Int(value));
            }
            chunks
                .entry((p[0].div_euclid(16), p[2].div_euclid(16)))
                .or_default()
                .block_entities
                .push(Value::Compound(tag));
            block_entity_count += 1;
        }
        for entity in &region.entities {
            let mut tag: Tag = entity
                .data
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            tag.insert(
                "id".into(),
                text(entity.entity_id().context("实体缺少 id")?),
            );
            move_entity(&mut tag, rd, &mut uuids)?;
            let p = entity_position(&tag)?;
            let cx = (p[0].floor() as i64).div_euclid(16);
            let cz = (p[2].floor() as i64).div_euclid(16);
            ensure!(
                cx.unsigned_abs() < 1_875_000 && cz.unsigned_abs() < 1_875_000,
                "实体超出世界边界"
            );
            chunks
                .entry((cx as i32, cz as i32))
                .or_default()
                .entities
                .push(Value::Compound(tag));
            entity_count += 1;
        }
    }
    // A small, explicit platform next to (not inside) the complete projection.
    let spawn = WorldPosition {
        x: add(origin[0], -4)?,
        y: add(origin[1].clamp(MIN_Y, MAX_Y - 2), 1)?,
        z: origin[2],
    };
    let glass = BlockStateNbt {
        name: "minecraft:glass".into(),
        properties: BTreeMap::new(),
    };
    ensure!(
        registry
            .blocks
            .get(&glass.name)
            .is_some_and(|b| b.accepts(&glass.name, &glass.properties)),
        "当前数据缺少出生平台方块"
    );
    let glass_id = states.len() as u32;
    states.push(glass);
    for dx in -1..=1 {
        for dz in -1..=1 {
            put_block(
                &mut chunks,
                [add(spawn.x, dx)?, spawn.y - 1, add(spawn.z, dz)?],
                glass_id,
            )?;
        }
    }
    fs::create_dir(output).with_context(|| format!("创建新存档目录 {}", output.display()))?;
    // level.dat is committed last: an interrupted export never resembles a finished world.
    let write_result = (|| -> Result<()> {
        write_regions(
            &output.join("region"),
            &chunks,
            &states,
            target.data_version,
            false,
        )?;
        write_regions(
            &output.join("entities"),
            &chunks,
            &states,
            target.data_version,
            true,
        )?;
        fs::create_dir_all(output.join("data/minecraft"))?;
        write_gzip(
            &output.join("data/minecraft/world_gen_settings.dat"),
            &world_generation(target.data_version),
        )?;
        write_gzip(
            &output.join("data/minecraft/game_rules.dat"),
            &compound([
                ("DataVersion", Value::Int(target.data_version)),
                (
                    "data",
                    compound([("minecraft:respawn_radius", Value::Int(0))]),
                ),
            ]),
        )?;
        write_gzip(
            &output.join("level.dat"),
            &level_data(name.trim(), spawn, target.data_version),
        )?;
        Ok(())
    })();
    write_result.with_context(|| {
        format!(
            "世界导出未完成，未报告成功；已写部分文件位于 {}",
            output.display()
        )
    })?;
    Ok(WorldExportResult {
        output_path: output.to_string_lossy().into_owned(),
        game_version: TARGET.into(),
        data_version: target.data_version,
        block_count,
        entity_count,
        block_entity_count,
        chunk_count: chunks.len(),
        spawn_platform_blocks: 9,
        spawn,
    })
}

pub fn run_cli(args: &[String]) -> Result<()> {
    let mut options = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        ensure!(
            ["--input", "--output", "--name", "--x", "--y", "--z"].contains(&args[i].as_str()),
            "未知 world-export 参数 {}",
            args[i]
        );
        ensure!(i + 1 < args.len(), "缺少 {} 参数值", args[i]);
        ensure!(
            options
                .insert(args[i].as_str(), args[i + 1].as_str())
                .is_none(),
            "重复参数 {}",
            args[i]
        );
        i += 2;
    }
    let input = options.get("--input").context("缺少 --input")?;
    let output = options.get("--output").context("缺少 --output")?;
    let name = options.get("--name").copied().unwrap_or("Nova World");
    let mut origin = [0; 3];
    for (i, key) in ["--x", "--y", "--z"].into_iter().enumerate() {
        origin[i] = options
            .get(key)
            .copied()
            .unwrap_or("0")
            .parse()
            .with_context(|| format!("{key} 必须为整数"))?;
    }
    let result = export_world(Path::new(input), Path::new(output), name, origin)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
