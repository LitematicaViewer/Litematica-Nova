//! Native exact-profile projection -> Java Anvil world writer.
//! No renderer or protected runtime is involved. A new directory is required.
use crate::{model::BlockStateNbt, nbt};
use anyhow::{Context, Result, bail, ensure};
use fastnbt::{ByteArray, IntArray, LongArray, Value};
use flate2::{
    Compression,
    write::{GzEncoder, ZlibEncoder},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

type Tag = HashMap<String, Value>;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldProfile {
    pub version: String,
    pub data_version: Option<i32>,
    pub family: String,
    pub heightmap_blocking: Vec<u32>,
    pub light_sources: Vec<String>,
    pub overworld_path: String,
    pub dragon_fight: bool,
    pub state_id_key: String,
    pub state_properties_key: String,
    pub min_y: i32,
    pub height: i32,
    pub external_entities: bool,
    pub external_chunks: bool,
    pub generator: String,
    pub spawn_compound: bool,
    pub namespace_status: bool,
    pub chunk_status: String,
    pub structure_overrides: bool,
    pub biome: String,
    pub runtime_validation: String,
}
impl WorldProfile {
    fn max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }
    fn modern(&self) -> bool {
        self.family == "root_sections"
    }
    fn numeric(&self) -> bool {
        self.family == "numeric"
    }
    fn padded(&self) -> bool {
        self.family != "palette_contiguous"
    }
}
pub fn profiles() -> Result<Vec<WorldProfile>> {
    #[derive(Deserialize)]
    struct Profiles {
        schema: u32,
        profiles: Vec<WorldProfile>,
    }
    let data: Profiles =
        serde_json::from_str(include_str!("../../../data/world-export/profiles.json"))?;
    ensure!(data.schema == 1, "世界导出 profile 版本不受支持");
    Ok(data.profiles)
}
struct ExportControl<'a> {
    cancel: Option<&'a Path>,
    progress: Option<&'a Path>,
}
impl ExportControl<'_> {
    fn check(&self) -> Result<()> {
        ensure!(
            !self.cancel.is_some_and(Path::exists),
            "世界导出已取消；本次临时文件已清理"
        );
        Ok(())
    }
    fn stage(&self, stage: &str, completed: usize, total: usize) -> Result<()> {
        self.check()?;
        if let Some(path) = self.progress {
            fs::write(
                path,
                serde_json::to_vec(
                    &serde_json::json!({"stage":stage,"completed":completed,"total":total}),
                )?,
            )?;
        }
        Ok(())
    }
}

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
    pub data_version: Option<i32>,
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
    block_ticks: Vec<Value>,
    fluid_ticks: Vec<Value>,
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
fn valid_position(p: [i32; 3], profile: &WorldProfile) -> Result<()> {
    ensure!(
        (profile.min_y..=profile.max_y()).contains(&p[1]),
        "方块坐标 Y={} 超出 {} 主世界高度 {}..{}，请调整高级选项中的统一偏移；未裁剪数据",
        p[1],
        profile.version,
        profile.min_y,
        profile.max_y()
    );
    ensure!(
        p[0].unsigned_abs() < 29_999_984 && p[2].unsigned_abs() < 29_999_984,
        "导出坐标超出世界边界"
    );
    Ok(())
}
fn put_block(
    chunks: &mut BTreeMap<(i32, i32), Chunk>,
    p: [i32; 3],
    state: u32,
    profile: &WorldProfile,
) -> Result<bool> {
    valid_position(p, profile)?;
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
fn state_tag(state: &BlockStateNbt, profile: &WorldProfile) -> Value {
    let mut tag = Tag::from([(profile.state_id_key.clone(), text(&state.name))]);
    if !state.properties.is_empty() {
        tag.insert(
            profile.state_properties_key.clone(),
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
fn packed_indices(indices: &[u32], palette_len: usize, padded: bool) -> Vec<i64> {
    let bits = (usize::BITS - palette_len.saturating_sub(1).leading_zeros()).max(4) as usize;
    let per_long = 64 / bits;
    let length = if padded {
        indices.len().div_ceil(per_long)
    } else {
        (indices.len() * bits).div_ceil(64)
    };
    let mut packed = vec![0i64; length];
    for (i, &value) in indices.iter().enumerate() {
        let (word, shift) = if padded {
            (i / per_long, (i % per_long) * bits)
        } else {
            (i * bits / 64, i * bits % 64)
        };
        packed[word] |= ((value as u64) << shift) as i64;
        if !padded && shift + bits > 64 {
            packed[word + 1] |= ((value as u64) >> (64 - shift)) as i64;
        }
    }
    packed
}
fn section_tag(
    y: i32,
    blocks: Option<&[u32; 4096]>,
    states: &[BlockStateNbt],
    numeric: &[(u16, u8)],
    profile: &WorldProfile,
) -> Value {
    if profile.numeric() {
        let mut ids = vec![0i8; 4096];
        let mut data = vec![0i8; 2048];
        let mut add = vec![0i8; 2048];
        for i in 0..4096 {
            let (id, meta) = numeric[blocks.map_or(0, |b| b[i]) as usize];
            ids[i] = id as u8 as i8;
            data[i / 2] |= ((meta & 15) << ((i % 2) * 4)) as i8;
            add[i / 2] |= (((id >> 8) as u8 & 15) << ((i % 2) * 4)) as i8;
        }
        let mut tag = Tag::from([
            ("Y".into(), Value::Byte(y as i8)),
            ("Blocks".into(), Value::ByteArray(ByteArray::new(ids))),
            ("Data".into(), Value::ByteArray(ByteArray::new(data))),
            (
                "BlockLight".into(),
                Value::ByteArray(ByteArray::new(vec![0; 2048])),
            ),
            (
                "SkyLight".into(),
                Value::ByteArray(ByteArray::new(vec![0; 2048])),
            ),
        ]);
        if add.iter().any(|x| *x != 0) {
            tag.insert("Add".into(), Value::ByteArray(ByteArray::new(add)));
        }
        return Value::Compound(tag);
    }
    let mut palette = Vec::new();
    let mut lookup = HashMap::<u32, u32>::new();
    let mut indices = Vec::with_capacity(4096);
    for i in 0..4096 {
        let state = blocks.map_or(0, |b| b[i]);
        let next = palette.len() as u32;
        let local = *lookup.entry(state).or_insert_with(|| {
            palette.push(state_tag(&states[state as usize], profile));
            next
        });
        indices.push(local);
    }
    if profile.modern() {
        let mut bs = Tag::from([("palette".into(), Value::List(palette))]);
        if lookup.len() > 1 {
            bs.insert(
                "data".into(),
                Value::LongArray(LongArray::new(packed_indices(&indices, lookup.len(), true))),
            );
        }
        compound([
            ("Y", Value::Byte(y as i8)),
            ("block_states", Value::Compound(bs)),
            (
                "biomes",
                compound([("palette", Value::List(vec![text(&profile.biome)]))]),
            ),
        ])
    } else {
        // Old PalettedContainer consumes packed longs even for one local state.
        let mut section = Tag::from([
            ("Y".into(), Value::Byte(y as i8)),
            ("Palette".into(), Value::List(palette)),
            (
                "BlockStates".into(),
                Value::LongArray(LongArray::new(packed_indices(
                    &indices,
                    lookup.len(),
                    profile.padded(),
                ))),
            ),
        ]);
        // 1.13's AnvilChunkLoader requires the nibble arrays even for a
        // decorated protochunk; its LIGHTED stage computes their real values.
        if profile.chunk_status == "decorated" {
            section.insert("BlockLight".into(), Value::ByteArray(ByteArray::new(vec![0; 2048])));
            section.insert("SkyLight".into(), Value::ByteArray(ByteArray::new(vec![0; 2048])));
        }
        Value::Compound(section)
    }
}
fn chunk_tag(
    cx: i32,
    cz: i32,
    chunk: &Chunk,
    states: &[BlockStateNbt],
    numeric: &[(u16, u8)],
    profile: &WorldProfile,
) -> Value {
    let sections = (profile.min_y.div_euclid(16)..(profile.min_y + profile.height).div_euclid(16))
        .map(|y| {
            section_tag(
                y,
                chunk.sections.get(&y).map(|b| b.as_ref()),
                states,
                numeric,
                profile,
            )
        })
        .collect();
    let mut data = Tag::from([
        ("xPos".into(), Value::Int(cx)),
        ("zPos".into(), Value::Int(cz)),
        ("LastUpdate".into(), Value::Long(0)),
        ("InhabitedTime".into(), Value::Long(0)),
    ]);
    if profile.numeric() {
        let mut heights = vec![0; 256];
        for (&sy, blocks) in &chunk.sections {
            for (i, &state) in blocks.iter().enumerate() {
                let (id, meta) = numeric[state as usize];
                if profile
                    .heightmap_blocking
                    .binary_search(&(u32::from(id) | (u32::from(meta) << 12)))
                    .is_ok()
                {
                    let column = (i & 15) | ((i >> 4 & 15) << 4);
                    heights[column] = heights[column].max(sy * 16 + (i >> 8) as i32 + 1);
                }
            }
        }
        data.extend([
            ("Sections".into(), Value::List(sections)),
            ("Entities".into(), Value::List(chunk.entities.clone())),
            (
                "TileEntities".into(),
                Value::List(chunk.block_entities.clone()),
            ),
            ("TileTicks".into(), Value::List(chunk.block_ticks.clone())),
            (
                "Biomes".into(),
                Value::ByteArray(ByteArray::new(vec![1; 256])),
            ),
            ("HeightMap".into(), Value::IntArray(IntArray::new(heights))),
            ("TerrainPopulated".into(), Value::Byte(1)),
            ("LightPopulated".into(), Value::Byte(0)),
        ]);
    } else {
        let status = profile.chunk_status.as_str();
        data.extend([
            ("Status".into(), text(status)),
            ("isLightOn".into(), Value::Byte(0)),
            ("Heightmaps".into(), compound([])),
            (
                "PostProcessing".into(),
                Value::List(
                    (0..profile.height / 16)
                        .map(|_| Value::List(vec![]))
                        .collect(),
                ),
            ),
        ]);
        if status == "decorated" {
            let emissive: Vec<_> = states.iter().map(|s| profile.light_sources.binary_search(&state_key(s)).is_ok()).collect();
            let lights = (0..16).map(|y| {
                Value::List(chunk.sections.get(&y).map(|blocks| {
                    blocks.iter().enumerate().filter(|(_, state)| emissive[**state as usize]).map(|(i, _)| {
                        Value::Short(((i & 15) | (((i >> 8) & 15) << 4) | (((i >> 4) & 15) << 8)) as i16)
                    }).collect()
                }).unwrap_or_default())
            }).collect();
            data.insert("Lights".into(), Value::List(lights));
        }
        if profile.modern() {
            data.extend([
                ("yPos".into(), Value::Int(profile.min_y / 16)),
                ("sections".into(), Value::List(sections)),
                (
                    "block_entities".into(),
                    Value::List(chunk.block_entities.clone()),
                ),
                ("block_ticks".into(), Value::List(chunk.block_ticks.clone())),
                ("fluid_ticks".into(), Value::List(chunk.fluid_ticks.clone())),
                (
                    "structures".into(),
                    compound([("starts", compound([])), ("References", compound([]))]),
                ),
            ]);
        } else {
            let biome_count = if profile.data_version.unwrap_or(0) >= 2225 {
                1024
            } else {
                256
            };
            data.extend([
                ("Sections".into(), Value::List(sections)),
                (
                    "TileEntities".into(),
                    Value::List(chunk.block_entities.clone()),
                ),
                ("TileTicks".into(), Value::List(chunk.block_ticks.clone())),
                ("LiquidTicks".into(), Value::List(chunk.fluid_ticks.clone())),
                (
                    "Biomes".into(),
                    Value::IntArray(IntArray::new(vec![1; biome_count])),
                ),
                (
                    "Structures".into(),
                    compound([("Starts", compound([])), ("References", compound([]))]),
                ),
            ]);
            if !profile.external_entities {
                data.insert("Entities".into(), Value::List(chunk.entities.clone()));
            }
        }
    }
    let mut root = if profile.modern() {
        data
    } else {
        Tag::from([("Level".into(), Value::Compound(data))])
    };
    if let Some(dv) = profile.data_version {
        root.insert("DataVersion".into(), Value::Int(dv));
    }
    Value::Compound(root)
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
    if let (Some(Value::Long(most)), Some(Value::Long(least))) =
        (tag.get("UUIDMost"), tag.get("UUIDLeast"))
    {
        ensure!(
            uuids.insert(vec![
                (*most >> 32) as i32,
                *most as i32,
                (*least >> 32) as i32,
                *least as i32
            ]),
            "重复实体 UUID：{}",
            entity_label(tag)
        );
    }
    for axes in [
        ["TileX", "TileY", "TileZ"],
        ["SleepingX", "SleepingY", "SleepingZ"],
        ["APX", "APY", "APZ"],
        ["BoundX", "BoundY", "BoundZ"],
    ] {
        if axes.iter().any(|axis| tag.contains_key(*axis)) {
            let p = offset(
                [
                    integer(tag.get(axes[0]), axes[0])?,
                    integer(tag.get(axes[1]), axes[1])?,
                    integer(tag.get(axes[2]), axes[2])?,
                ],
                delta,
            )?;
            for (axis, value) in axes.into_iter().zip(p) {
                tag.insert(axis.into(), Value::Int(value));
            }
        }
    }
    for name in ["Leash", "leash"] {
        if let Some(Value::Compound(anchor)) = tag.get_mut(name) {
            let axes = if anchor.contains_key("X") {
                ["X", "Y", "Z"]
            } else {
                ["x", "y", "z"]
            };
            if axes.iter().any(|axis| anchor.contains_key(*axis)) {
                let p = offset(
                    [
                        integer(anchor.get(axes[0]), axes[0])?,
                        integer(anchor.get(axes[1]), axes[1])?,
                        integer(anchor.get(axes[2]), axes[2])?,
                    ],
                    delta,
                )?;
                for (axis, value) in axes.into_iter().zip(p) {
                    anchor.insert(axis.into(), Value::Int(value));
                }
            }
        }
    }
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
    let file = OpenOptions::new().write(true).create_new(true).open(path).with_context(|| format!("创建世界数据文件 {}", path.display()))?;
    let mut gz = GzEncoder::new(file, Compression::default());
    gz.write_all(&fastnbt::to_bytes(value)?)?;
    gz.finish()?.sync_all()?;
    Ok(())
}
fn write_regions(
    directory: &Path,
    chunks: &BTreeMap<(i32, i32), Chunk>,
    states: &[BlockStateNbt],
    numeric: &[(u16, u8)],
    profile: &WorldProfile,
    entities: bool,
    control: &ExportControl,
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
    let region_total = regions.len();
    for (region_index, ((rx, rz), positions)) in regions.into_iter().enumerate() {
        control.stage(
            if entities { "entities" } else { "regions" },
            region_index,
            region_total,
        )?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("r.{rx}.{rz}.mca")))?;
        let mut header = [0u8; 8192];
        file.write_all(&header)?;
        let mut sector = 2u32;
        for (cx, cz) in positions {
            control.check()?;
            let chunk = &chunks[&(cx, cz)];
            let value = if entities {
                compound([
                    ("DataVersion", Value::Int(profile.data_version.unwrap_or(0))),
                    ("Position", Value::IntArray(IntArray::new(vec![cx, cz]))),
                    ("Entities", Value::List(chunk.entities.clone())),
                ])
            } else {
                chunk_tag(cx, cz, chunk, states, numeric, profile)
            };
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&fastnbt::to_bytes(&value)?)?;
            let compressed = encoder.finish()?;
            let needed = (compressed.len() + 5).div_ceil(4096);
            let external = needed > 255;
            ensure!(
                !external || profile.external_chunks,
                "{} 的区块 ({cx},{cz}) 超出此版本 Anvil 单区块大小；未写入不受支持的外部区块",
                profile.version
            );
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
fn flat_settings(profile: &WorldProfile) -> Value {
    let mut settings = Tag::from([
        (
            "layers".into(),
            Value::List(vec![compound([
                ("height", Value::Int(1)),
                ("block", text("minecraft:air")),
            ])]),
        ),
        ("biome".into(), text(&profile.biome)),
        ("lakes".into(), Value::Byte(0)),
        ("features".into(), Value::Byte(0)),
    ]);
    if profile.structure_overrides {
        settings.insert("structure_overrides".into(), Value::List(vec![]));
    } else {
        settings.insert(
            "structures".into(),
            if profile.generator == "flat_compound" {
                compound([])
            } else {
                compound([("structures", compound([]))])
            },
        );
    }
    Value::Compound(settings)
}
fn generation_settings(profile: &WorldProfile) -> Value {
    compound([
        ("seed", Value::Long(0)),
        ("generate_structures", Value::Byte(0)),
        ("bonus_chest", Value::Byte(0)),
        (
            "dimensions",
            compound([(
                "minecraft:overworld",
                compound([
                    ("type", text("minecraft:overworld")),
                    (
                        "generator",
                        compound([
                            ("type", text("minecraft:flat")),
                            ("settings", flat_settings(profile)),
                        ]),
                    ),
                ]),
            )]),
        ),
    ])
}
fn level_data(name: &str, spawn: WorldPosition, profile: &WorldProfile) -> Value {
    let mut data = Tag::from([
        ("version".into(), Value::Int(19133)),
        ("LevelName".into(), text(name)),
        ("GameType".into(), Value::Int(3)),
        ("allowCommands".into(), Value::Byte(1)),
        ("initialized".into(), Value::Byte(1)),
        ("Time".into(), Value::Long(6000)),
        ("DayTime".into(), Value::Long(6000)),
        ("LastPlayed".into(), Value::Long(now_ms())),
    ]);
    if profile.dragon_fight {
        data.insert("DragonFight".into(), compound([
            ("NeedsStateScanning", Value::Byte(1)),
            ("DragonKilled", Value::Byte(0)),
            ("PreviouslyKilled", Value::Byte(0)),
        ]));
    }
    if let Some(dv) = profile.data_version {
        data.insert("DataVersion".into(), Value::Int(dv));
        data.insert(
            "Version".into(),
            compound([
                ("Name", text(&profile.version)),
                ("Id", Value::Int(dv)),
                ("Snapshot", Value::Byte(0)),
                ("Series", text("main")),
            ]),
        );
    }
    if profile.spawn_compound {
        data.insert(
            "spawn".into(),
            compound([
                ("dimension", text("minecraft:overworld")),
                ("pos", coord([spawn.x, spawn.y, spawn.z])),
                ("yaw", Value::Float(-90.0)),
                ("pitch", Value::Float(0.0)),
            ]),
        );
    } else {
        for (k, v) in [
            ("SpawnX", spawn.x),
            ("SpawnY", spawn.y),
            ("SpawnZ", spawn.z),
        ] {
            data.insert(k.into(), Value::Int(v));
        }
        data.insert("SpawnAngle".into(), Value::Float(-90.0));
    }
    if profile.generator == "separate" {
        data.insert(
            "difficulty_settings".into(),
            compound([
                ("difficulty", text("normal")),
                ("hardcore", Value::Byte(0)),
                ("locked", Value::Byte(0)),
            ]),
        );
    } else {
        data.extend([
            ("Difficulty".into(), Value::Byte(2)),
            ("DifficultyLocked".into(), Value::Byte(0)),
            ("hardcore".into(), Value::Byte(0)),
            ("GameRules".into(), compound([("spawnRadius", text("0"))])),
        ]);
    }
    match profile.generator.as_str() {
        "flat_string" => {
            data.extend([
                ("RandomSeed".into(), Value::Long(0)),
                ("generatorName".into(), text("flat")),
                ("generatorVersion".into(), Value::Int(0)),
                ("generatorOptions".into(), text("3;minecraft:air;1;")),
                ("MapFeatures".into(), Value::Byte(0)),
            ]);
        }
        "flat_compound" => {
            data.extend([
                ("RandomSeed".into(), Value::Long(0)),
                ("generatorName".into(), text("flat")),
                ("generatorVersion".into(), Value::Int(0)),
                ("generatorOptions".into(), flat_settings(profile)),
                ("MapFeatures".into(), Value::Byte(0)),
            ]);
        }
        "dimensions" => {
            data.insert("WorldGenSettings".into(), generation_settings(profile));
        }
        "separate" => {}
        _ => unreachable!(),
    }
    if !profile.numeric() {
        data.insert(
            "DataPacks".into(),
            compound([
                ("Enabled", Value::List(vec![text("vanilla")])),
                ("Disabled", Value::List(vec![])),
            ]),
        );
    }
    if profile.data_version.is_some_and(|v| v >= 3218) {
        data.insert(
            "enabled_features".into(),
            Value::List(vec![text("minecraft:vanilla")]),
        );
    }
    compound([("Data", Value::Compound(data))])
}
struct OwnedStaging {
    path: PathBuf,
    committed: bool,
}
impl OwnedStaging {
    fn create(output: &Path) -> Result<Self> {
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        ensure!(parent.is_dir(), "输出父目录不存在");
        for nonce in 0..100 {
            let path = parent.join(format!(
                ".nova-world-{}-{}-{nonce}.tmp",
                std::process::id(),
                now_ms()
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path,
                        committed: false,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        bail!("创建独占世界临时目录失败")
    }
    fn commit(&mut self, output: &Path) -> Result<()> {
        ensure!(!output.exists(), "输出世界目录已出现，原有目录保留");
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
            let a: Vec<u16> = self.path.as_os_str().encode_wide().chain(Some(0)).collect();
            let b: Vec<u16> = output.as_os_str().encode_wide().chain(Some(0)).collect();
            // A filesystem observer can briefly hold a newly closed child file
            // without FILE_SHARE_DELETE. Retry only this final no-replace rename;
            // never restart the export or hide a persistent permission failure.
            for attempt in 0..=10 {
                ensure!(!output.exists(), "输出世界目录已出现，原有目录保留");
                if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), MOVEFILE_WRITE_THROUGH) } != 0 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if !matches!(error.raw_os_error(), Some(5 | 32)) || attempt == 10 {
                    return Err(error.into());
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        #[cfg(target_os = "linux")]
        {
            use std::{ffi::CString, os::unix::ffi::OsStrExt};
            unsafe extern "C" {
                fn renameat2(
                    olddirfd: i32,
                    oldpath: *const std::ffi::c_char,
                    newdirfd: i32,
                    newpath: *const std::ffi::c_char,
                    flags: u32,
                ) -> i32;
            }
            let a = CString::new(self.path.as_os_str().as_bytes())?;
            let b = CString::new(output.as_os_str().as_bytes())?;
            if unsafe { renameat2(-100, a.as_ptr(), -100, b.as_ptr(), 1) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        #[cfg(target_os = "macos")]
        {
            use std::{ffi::CString, os::unix::ffi::OsStrExt};
            unsafe extern "C" {
                fn renamex_np(
                    oldpath: *const std::ffi::c_char,
                    newpath: *const std::ffi::c_char,
                    flags: u32,
                ) -> i32;
            }
            let a = CString::new(self.path.as_os_str().as_bytes())?;
            let b = CString::new(output.as_os_str().as_bytes())?;
            if unsafe { renamex_np(a.as_ptr(), b.as_ptr(), 4) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        self.committed = true;
        Ok(())
    }
}
impl Drop for OwnedStaging {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExtraRegion {
    #[serde(default)]
    pending_block_ticks: Vec<Value>,
    #[serde(default)]
    pending_fluid_ticks: Vec<Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExtraRoot {
    regions: BTreeMap<String, ExtraRegion>,
}
fn saved_tick(
    value: &Value,
    fluid: bool,
    delta: [i32; 3],
    profile: &WorldProfile,
) -> Result<([i32; 3], Value)> {
    let Value::Compound(tag) = value else {
        bail!("计划刻不是 Compound")
    };
    let key = if fluid { "Fluid" } else { "Block" };
    ensure!(
        tag.keys()
            .all(|k| [key, "x", "y", "z", "Time", "Priority", "SubTick"].contains(&k.as_str())),
        "计划刻包含未识别字段，未丢弃"
    );
    let Some(Value::String(id)) = tag.get(key) else {
        bail!("计划刻缺少 {key} 类型")
    };
    ensure!(
        !fluid || !profile.numeric(),
        "旧世界格式没有独立流体计划刻；请明确转换后再导出"
    );
    let pos = offset(
        [
            integer(tag.get("x"), "tick x")?,
            integer(tag.get("y"), "tick y")?,
            integer(tag.get("z"), "tick z")?,
        ],
        delta,
    )?;
    valid_position(pos, profile)?;
    let delay = match tag.get("Time") {
        Some(Value::Int(v)) => *v,
        Some(Value::Long(v)) => i32::try_from(*v).context("计划刻延迟超出世界格式范围")?,
        _ => bail!("计划刻缺少相对时间"),
    };
    let priority = tag
        .get("Priority")
        .map(|v| integer(Some(v), "tick priority"))
        .transpose()?
        .unwrap_or(0);
    ensure!((-3..=3).contains(&priority), "计划刻优先级无效");
    Ok((
        pos,
        compound([
            ("i", text(id)),
            ("x", Value::Int(pos[0])),
            ("y", Value::Int(pos[1])),
            ("z", Value::Int(pos[2])),
            ("t", Value::Int(delay)),
            ("p", Value::Int(priority)),
        ]),
    ))
}

pub fn export_world(
    input: &Path,
    output: &Path,
    name: &str,
    origin: [i32; 3],
    target_version: &str,
    cancel: Option<&Path>,
    progress: Option<&Path>,
) -> Result<WorldExportResult> {
    ensure!(input.is_file(), "源投影不存在");
    ensure!(!output.exists(), "输出目录已存在；请选择新目录，不覆盖存档");
    ensure!(!name.trim().is_empty(), "世界名称不能为空");
    let profile = profiles()?
        .into_iter()
        .find(|p| p.version == target_version)
        .context("目标版本没有原生世界导出 profile")?;
    let control = ExportControl { cancel, progress };
    control.stage("reading", 0, 0)?;
    let database = crate::block_database::database()?;
    let target = database.version(target_version)?;
    let registry = database.registry(target_version)?;
    let root = nbt::load_litematic_root_file_backed_with_progress(input, &mut |_| {})?;
    control.check()?;
    let bounds = crate::visual::compute_bounds(&root).context("投影没有选区")?;
    let delta = [
        origin[0].checked_sub(bounds.min_x).context("X 坐标溢出")?,
        origin[1].checked_sub(bounds.min_y).context("Y 坐标溢出")?,
        origin[2].checked_sub(bounds.min_z).context("Z 坐标溢出")?,
    ];
    // Validate every independent palette/payload before creating any world.
    // Recommendations from the conversion dialog are not silently accepted.
    let mut root = root;
    let mut issues = Vec::new();
    for (region_name, region) in &mut root.regions {
        let frequencies = nbt::decode_palette_frequencies(
            &region.block_states,
            nbt::region_volume(&region.size)?,
            nbt::bits_for_palette(region.block_state_palette.len()),
            region.block_state_palette.len(),
        )?;
        for (index, state) in region.block_state_palette.iter_mut().enumerate() {
            if frequencies[index] == 0 {
                *state = BlockStateNbt {
                    name: "minecraft:air".into(),
                    properties: BTreeMap::new(),
                };
                continue;
            }
            match resolve_target_state(state, registry) {
                Ok(resolved) => *state = resolved,
                Err(error) => issues.push(format!(
                    "{region_name}: {} × {} — {error}",
                    state_key(state),
                    frequencies[index]
                )),
            }
        }
        if (!region.entities.is_empty() || !region.tile_entities.is_empty())
            && root.minecraft_data_version != Some(target.data_version)
        {
            if let Some(source) = root.minecraft_data_version {
                let entities = region
                    .entities
                    .iter()
                    .map(|e| fastnbt::to_bytes(e).and_then(|b| fastnbt::from_bytes::<Value>(&b)))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let mut payload = compound([
                    ("Entities", Value::List(entities)),
                    ("TileEntities", Value::List(region.tile_entities.clone())),
                ]);
                let report = crate::version_payload_migration::migrate(
                    &mut payload,
                    source,
                    target.data_version,
                )?;
                if report.issues.is_empty() {
                    let Value::Compound(mut fields) = payload else {
                        unreachable!()
                    };
                    if let Some(Value::List(tiles)) = fields.remove("TileEntities") {
                        region.tile_entities = tiles;
                    }
                    if let Some(Value::List(entities)) = fields.remove("Entities") {
                        region.entities = entities
                            .into_iter()
                            .map(|e| fastnbt::to_bytes(&e).and_then(|b| fastnbt::from_bytes(&b)))
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                    }
                } else {
                    issues.extend(
                        report
                            .issues
                            .into_iter()
                            .map(|issue| format!("{region_name}: {issue}")),
                    );
                }
            } else {
                issues.push(format!(
                    "{region_name}: 源 DataVersion 不明，实体及容器保留需要明确的来源版本"
                ));
            }
        }
        control.check()?;
    }
    ensure!(
        issues.is_empty(),
        "目标 {target_version} 存在实际内容冲突，尚未写入世界；请先通过版本转换明确处理：\n{}",
        issues.join("\n")
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
    for (region_index, (region_name, region)) in root.regions.iter().enumerate() {
        control.stage("blocks", region_index, root.regions.len())?;
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
                format!(
                    "{target_version} 没有方块 {}（选区 {region_name}）",
                    state.name
                )
            })?;
            ensure!(
                block.accepts(&state_key(state), &state.properties),
                "{target_version} 不接受方块状态 {} {:?}；请先做明确的版本转换",
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
                if i % 4096 == 0 {
                    control.check()?;
                }
                let state = mapped[palette];
                if state == 0 {
                    return Ok(());
                }
                let y = i / (width * length);
                let z = (i % (width * length)) / width;
                let x = i % width;
                let (x, y, z) = nbt::storage_to_region_coords(x, y, z, &region.size);
                if put_block(&mut chunks, offset([x, y, z], rd)?, state, &profile)? {
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
            valid_position(p, &profile)?;
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
    // The scene-oriented RegionNbt intentionally excludes scheduled ticks.
    // Read just those fields with serde's ignored-value path; do not duplicate
    // geometry arrays or accidentally discard world behavior on export.
    let extras: ExtraRoot = fastnbt::from_reader(std::io::BufReader::new(
        flate2::read::GzDecoder::new(fs::File::open(input)?),
    ))?;
    for (name, extra) in extras.regions {
        let region = root.regions.get(&name).context("计划刻选区对应关系丢失")?;
        let rd = offset(
            [region.position.x, region.position.y, region.position.z],
            delta,
        )?;
        let mut payload = compound([
            ("PendingBlockTicks", Value::List(extra.pending_block_ticks)),
            ("PendingFluidTicks", Value::List(extra.pending_fluid_ticks)),
        ]);
        if root.minecraft_data_version != Some(target.data_version)
            && matches!(&payload,Value::Compound(f) if f.values().any(|v|matches!(v,Value::List(l) if !l.is_empty())))
        {
            let report = crate::version_payload_migration::migrate(
                &mut payload,
                root.minecraft_data_version
                    .context("计划刻来源 DataVersion 不明")?,
                target.data_version,
            )?;
            ensure!(
                report.issues.is_empty(),
                "{name} 计划刻转换未完成：{}",
                report.issues.join("；")
            );
        }
        let Value::Compound(mut fields) = payload else {
            unreachable!()
        };
        for (field, fluid) in [("PendingBlockTicks", false), ("PendingFluidTicks", true)] {
            if let Some(Value::List(mut ticks)) = fields.remove(field) {
                ticks.sort_by_key(|tick| {
                    if let Value::Compound(t) = tick {
                        match t.get("SubTick") {
                            Some(Value::Long(v)) => *v,
                            _ => 0,
                        }
                    } else {
                        0
                    }
                });
                for tick in ticks {
                    let (p, tick) = saved_tick(&tick, fluid, rd, &profile)
                        .with_context(|| format!("{name}/{field}"))?;
                    let chunk = chunks
                        .entry((p[0].div_euclid(16), p[2].div_euclid(16)))
                        .or_default();
                    if fluid {
                        chunk.fluid_ticks.push(tick)
                    } else {
                        chunk.block_ticks.push(tick)
                    }
                }
            }
        }
        control.check()?;
    }
    // Spectator spawn needs no platform and changes none of the projection.
    let spawn = WorldPosition {
        x: add(origin[0], -4)?,
        y: add(origin[1].clamp(profile.min_y, profile.max_y() - 2), 1)?,
        z: origin[2],
    };
    let numeric = if profile.numeric() {
        states
            .iter()
            .map(|state| numeric_state(state, registry))
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![]
    };
    control.stage("writing", 0, chunks.len())?;
    let mut staging = OwnedStaging::create(output)?;
    let dimension_path = staging.path.join(&profile.overworld_path);
    if !profile.overworld_path.is_empty() {
        fs::create_dir_all(&dimension_path)?;
    }
    write_regions(
        &dimension_path.join("region"),
        &chunks,
        &states,
        &numeric,
        &profile,
        false,
        &control,
    )?;
    if profile.external_entities {
        write_regions(
            &dimension_path.join("entities"),
            &chunks,
            &states,
            &numeric,
            &profile,
            true,
            &control,
        )?;
    }
    if profile.generator == "separate" {
        fs::create_dir_all(staging.path.join("data/minecraft"))?;
        write_gzip(
            &staging.path.join("data/minecraft/world_gen_settings.dat"),
            &compound([
                ("DataVersion", Value::Int(target.data_version)),
                ("data", generation_settings(&profile)),
            ]),
        )?;
        write_gzip(
            &staging.path.join("data/minecraft/game_rules.dat"),
            &compound([
                ("DataVersion", Value::Int(target.data_version)),
                (
                    "data",
                    compound([("minecraft:respawn_radius", Value::Int(0))]),
                ),
            ]),
        )?;
    }
    write_gzip(
        &staging.path.join("level.dat"),
        &level_data(name.trim(), spawn, &profile),
    )?;
    control.stage("committing", 0, 1)?;
    staging.commit(output).with_context(|| format!("提交世界目录 {} → {}", staging.path.display(), output.display()))?;
    Ok(WorldExportResult {
        output_path: output.to_string_lossy().into_owned(),
        game_version: target_version.into(),
        data_version: profile.data_version,
        block_count,
        entity_count,
        block_entity_count,
        chunk_count: chunks.len(),
        spawn_platform_blocks: 0,
        spawn,
    })
}

fn resolve_target_state(
    state: &BlockStateNbt,
    registry: &crate::block_database::Registry,
) -> Result<BlockStateNbt> {
    let key = state_key(state);
    if registry
        .blocks
        .get(&state.name)
        .is_some_and(|b| b.accepts(&key, &state.properties))
    {
        return Ok(state.clone());
    }
    // Legacy aliases can describe one stored numeric state using multiple
    // modern names. Accept only a unique actual ID/meta, not an arbitrary name.
    if let Some(aliases) = registry.legacy_state_aliases.get(&key) {
        let candidates: Vec<_> = aliases
            .iter()
            .filter_map(|alias| crate::version_conversion::parse_key(alias).ok())
            .map(|(name, properties)| BlockStateNbt { name, properties })
            .collect();
        let numeric: Vec<_> = candidates
            .iter()
            .filter_map(|s| numeric_state(s, registry).ok())
            .collect();
        if !numeric.is_empty() && numeric.iter().all(|n| *n == numeric[0]) {
            return candidates
                .into_iter()
                .find(|s| numeric_state(s, registry).ok() == Some(numeric[0]))
                .context("旧版状态映射缺失");
        }
    }
    bail!("目标版本没有此方块或状态；未自动替换为空气/默认状态")
}

fn state_key(state: &BlockStateNbt) -> String {
    if state.properties.is_empty() {
        state.name.clone()
    } else {
        format!(
            "{}[{}]",
            state.name,
            state
                .properties
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}
fn numeric_state(
    state: &BlockStateNbt,
    registry: &crate::block_database::Registry,
) -> Result<(u16, u8)> {
    let key = state_key(state);
    let packed = registry
        .legacy_blocks
        .iter()
        .find(|(_, v)| *v == &key)
        .map(|(k, _)| k.as_str())
        .with_context(|| format!("目标版本没有 {key} 的真实数值 ID/metadata"))?;
    let (id, meta) = packed.split_once(':').context("旧版数值状态格式无效")?;
    let id: u16 = id.parse()?;
    let meta: u8 = meta.parse()?;
    ensure!(id < 4096 && meta < 16, "旧版数值状态超出 Anvil 编码范围");
    Ok((id, meta))
}
pub fn run_cli(args: &[String]) -> Result<()> {
    let mut options = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        ensure!(
            [
                "--input",
                "--output",
                "--name",
                "--x",
                "--y",
                "--z",
                "--target",
                "--cancel",
                "--progress",
                "--profiles"
            ]
            .contains(&args[i].as_str()),
            "未知 world-export 参数 {}",
            args[i]
        );
        if args[i] == "--profiles" {
            options.insert("--profiles", "true");
            i += 1;
            continue;
        }
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
    if options.contains_key("--profiles") {
        let profiles = profiles()?;
        let context = crate::game_data::active_context()?;
        let source_dv = options
            .get("--input")
            .map(|p| {
                nbt::scan_litematic_minecraft_data_version_with_progress(Path::new(p), &mut |_| {})
                    .map(|r| r.minecraft_data_version())
            })
            .transpose()?
            .flatten();
        let matching: Vec<_> = profiles
            .iter()
            .filter(|p| source_dv.is_some_and(|dv| p.data_version.unwrap_or(0) == dv))
            .map(|p| p.version.as_str())
            .collect();
        let default = if matching.len() == 1 {
            matching[0]
        } else {
            context.game_version.as_str()
        };
        println!(
            "{}",
            serde_json::to_string(
                &serde_json::json!({"profiles":profiles,"sourceDataVersion":source_dv,"defaultVersion":default})
            )?
        );
        return Ok(());
    }
    let input = options.get("--input").context("缺少 --input")?;
    let output = options.get("--output").context("缺少 --output")?;
    let target = options
        .get("--target")
        .context("缺少明确的 --target 世界目标版本")?;
    let name = options.get("--name").copied().unwrap_or("Nova World");
    let mut origin = [0; 3];
    for (i, key) in ["--x", "--y", "--z"].iter().enumerate() {
        origin[i] = options
            .get(key)
            .copied()
            .unwrap_or("0")
            .parse()
            .with_context(|| format!("{key} 必须为整数"))?;
    }
    let result = export_world(
        Path::new(input),
        Path::new(output),
        name,
        origin,
        target,
        options.get("--cancel").map(Path::new),
        options.get("--progress").map(Path::new),
    )?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_profiles_and_storage_boundaries() {
        let profiles = profiles().unwrap();
        assert_eq!(profiles.len(), 63);
        for p in &profiles {
            assert!(
                crate::block_database::database()
                    .unwrap()
                    .version(&p.version)
                    .is_ok()
            );
            assert_eq!(p.height % 16, 0);
        }
        let values: Vec<u32> = (0..4096).map(|i| (i % 31) as u32).collect();
        for padded in [false, true] {
            let words = packed_indices(&values, 31, padded);
            let bits = 5;
            let per = 64 / bits;
            for (i, expected) in values.iter().enumerate() {
                let (word, shift) = if padded {
                    (i / per, (i % per) * bits)
                } else {
                    (i * bits / 64, i * bits % 64)
                };
                let mut got = (words[word] as u64) >> shift;
                if !padded && shift + bits > 64 {
                    got |= (words[word + 1] as u64) << (64 - shift);
                }
                assert_eq!((got & 31) as u32, *expected, "padded={padded} i={i}");
            }
        }
        assert_eq!(
            profiles
                .iter()
                .find(|p| p.version == "1.13")
                .unwrap()
                .chunk_status,
            "decorated"
        );
        assert_eq!(
            profiles
                .iter()
                .find(|p| p.version == "1.8")
                .unwrap()
                .data_version,
            None
        );
    }
    #[test]
    fn world_staging_cancel_and_no_clobber() {
        let base = std::env::temp_dir().join(format!(
            "nova-world-transaction-test-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir(&base).unwrap();
        let output = base.join("world");
        let path = {
            let stage = OwnedStaging::create(&output).unwrap();
            fs::write(stage.path.join("partial"), b"owned").unwrap();
            stage.path.clone()
        };
        assert!(!path.exists());
        fs::create_dir(&output).unwrap();
        fs::write(output.join("keep"), b"user").unwrap();
        let mut stage = OwnedStaging::create(&output).unwrap();
        assert!(stage.commit(&output).is_err());
        assert_eq!(fs::read(output.join("keep")).unwrap(), b"user");
        drop(stage);
        let cancel = base.join("cancel");
        fs::write(&cancel, b"cancel").unwrap();
        assert!(
            ExportControl {
                cancel: Some(&cancel),
                progress: None
            }
            .check()
            .is_err()
        );
        fs::remove_dir_all(base).unwrap();
    }
}
