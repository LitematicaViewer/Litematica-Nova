//! ChunkPalettedStorageFix cases 25/140 move data between per-position block
//! entities and the palette. Never use its lossy default/clamp for unknown data.
use super::{Fields, object};
use crate::{
    block_database::{Registry, database},
    nbt::{
        Vec3i, bits_for_palette, for_each_palette_index, pack_palette_indices,
        region_volume,
    },
    version_entity_registry::{self, Kind},
};
use anyhow::{Context, Result, bail};
use fastnbt::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::AtomicBool,
};

const FLOWER: &str = "minecraft:flower_pot";
const NOTE: &str = "minecraft:note_block";
const POTS: &[(&str, i32, &str)] = &[
    ("red_flower", 0, "poppy"),
    ("red_flower", 1, "blue_orchid"),
    ("red_flower", 2, "allium"),
    ("red_flower", 3, "azure_bluet"),
    ("red_flower", 4, "red_tulip"),
    ("red_flower", 5, "orange_tulip"),
    ("red_flower", 6, "white_tulip"),
    ("red_flower", 7, "pink_tulip"),
    ("red_flower", 8, "oxeye_daisy"),
    ("yellow_flower", 0, "dandelion"),
    ("sapling", 0, "oak_sapling"),
    ("sapling", 1, "spruce_sapling"),
    ("sapling", 2, "birch_sapling"),
    ("sapling", 3, "jungle_sapling"),
    ("sapling", 4, "acacia_sapling"),
    ("sapling", 5, "dark_oak_sapling"),
    ("red_mushroom", 0, "red_mushroom"),
    ("brown_mushroom", 0, "brown_mushroom"),
    ("deadbush", 0, "dead_bush"),
    ("tallgrass", 2, "fern"),
    ("cactus", 0, "cactus"),
];

fn name(v: &Value) -> Option<&str> {
    let Value::Compound(m) = v else { return None };
    let Value::String(n) = m.get("Name")? else {
        return None;
    };
    Some(n)
}
fn relevant(n: &str) -> bool {
    n == FLOWER || n == NOTE || n == "minecraft:noteblock" || n.starts_with("minecraft:potted_")
}
fn tile_kind(v: &Value) -> Option<&str> {
    let Value::Compound(m) = v else { return None };
    let Value::String(id) = m.get("id")? else {
        return None;
    };
    match id.as_str() {
        "Music" | "minecraft:noteblock" => Some("minecraft:noteblock"),
        "FlowerPot" | "minecraft:flower_pot" => Some(FLOWER),
        _ => None,
    }
}
fn int(m: &Fields, key: &str) -> Result<i32> {
    match m.get(key) {
        Some(Value::Int(v)) => Ok(*v),
        _ => bail!("{key} 不是 Int"),
    }
}
fn nbt(name: &str, props: &[(&str, String)]) -> Value {
    let mut m = Fields::from([("Name".into(), Value::String(name.to_owned()))]);
    if !props.is_empty() {
        m.insert(
            "Properties".into(),
            Value::Compound(
                props
                    .iter()
                    .map(|(k, v)| (k.to_string(), Value::String(v.clone())))
                    .collect(),
            ),
        );
    }
    Value::Compound(m)
}
fn props(v: &Value) -> Result<std::collections::BTreeMap<String, String>> {
    let Value::Compound(m) = v else {
        bail!("调色板不是 Compound")
    };
    if m.keys().any(|k| k != "Name" && k != "Properties") {
        bail!("花盆/音符盒调色板含未知字段")
    }
    match m.get("Properties") {
        None => Ok(Default::default()),
        Some(Value::Compound(p)) => p
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => Ok((k.clone(), s.clone())),
                _ => bail!("调色板属性 {k} 不是 String"),
            })
            .collect(),
        _ => bail!("Properties 不是 Compound"),
    }
}

fn pot_payload(tile: &Fields, reg: &Registry) -> Result<String> {
    let data = match tile.get("Data") {
        None => 0,
        Some(Value::Int(n)) => *n,
        _ => bail!("花盆 Data 不是 Int"),
    };
    let item = match tile.get("Item") {
        None => "",
        Some(Value::String(n)) => n.strip_prefix("minecraft:").unwrap_or(n),
        Some(Value::Int(0)) => "",
        Some(Value::Int(n)) => reg
            .legacy_item_ids
            .get(&n.to_string())
            .and_then(|v| v.strip_prefix("minecraft:"))
            .context("花盆 Item 数字ID在来源注册表不存在")?,
        _ => bail!("花盆 Item 不是旧版名称或 Int"),
    };
    if (item == "" || item == "air") && data == 0 {
        return Ok(FLOWER.into());
    }
    let (_, _, plant) = POTS
        .iter()
        .find(|(i, d, _)| *i == item && *d == data)
        .with_context(|| format!("花盆 Item={item},Data={data} 没有无损原版映射"))?;
    Ok(format!("minecraft:potted_{plant}"))
}
fn old_flower(tile: &mut Fields, n: &str, reg: &Registry) -> Result<()> {
    let (item, data) = if n == FLOWER {
        ("minecraft:air".into(), 0)
    } else {
        let plant = n.strip_prefix("minecraft:potted_").context("非花盆方块")?;
        let (item, data, _) = POTS
            .iter()
            .find(|(_, _, p)| *p == plant)
            .with_context(|| format!("旧花盆没有 {n} 对应的 Item/Data"))?;
        let item = format!("minecraft:{item}");
        if !reg
            .legacy_item_ids
            .iter()
            .any(|(i, k)| k == &item && reg.legacy_items.contains_key(&format!("{i}:{data}")))
        {
            bail!("目标旧物品注册表缺少花盆内容 {item}:{data}")
        }
        (item, *data)
    };
    tile.insert("Item".into(), Value::String(item));
    tile.insert("Data".into(), Value::Int(data));
    Ok(())
}

// Pre-1.13 TileEntityNote chooses its sound at play time from the block below;
// unlike note/powered, instrument cannot be written into its NBT. Only map
// support materials with a verified, unambiguous old rule. Outside the region
// there is no stored support; do not infer any non-default instrument.
fn instrument(support: Option<&str>, version: i32) -> Result<&'static str> {
    let n = support
        .unwrap_or("minecraft:air")
        .strip_prefix("minecraft:")
        .context("乐器下方为未知命名空间")?;
    Ok(match n {
        "air" | "dirt" | "grass_block" | "grass" | "coarse_dirt" | "podzol" => "harp",
        "stone" | "cobblestone" | "bedrock" | "stone_bricks" | "stonebrick" | "granite"
        | "andesite" | "diorite" => "basedrum",
        "sand" | "red_sand" | "gravel" => "snare",
        "glass" | "stained_glass" | "white_stained_glass" => "hat",
        "planks" | "oak_planks" | "spruce_planks" | "birch_planks" | "jungle_planks"
        | "acacia_planks" | "dark_oak_planks" | "log" | "log2" | "oak_log" | "spruce_log"
        | "birch_log" | "jungle_log" | "acacia_log" | "dark_oak_log" | "note_block"
        | "noteblock" => "bass",
        "clay" if version >= 1125 => "flute",
        "gold_block" if version >= 1125 => "bell",
        "packed_ice" if version >= 1125 => "chime",
        "bone_block" if version >= 1125 => "xylophone",
        _ => bail!("旧音符盒下方 {n} 的实际乐器材质尚未覆盖，保留原数据"),
    })
}

#[derive(Default)]
pub(crate) struct Bound {
    // Final palette is checked after ordinary replacement/property edits,
    // before adding any target-only tiles. There is no unchecked ID exemption.
    states: HashMap<usize, Value>,
    supports: Vec<(Option<usize>, String)>,
    pending: Vec<Value>,
    count: u64,
    target: i32,
}
impl Bound {
    pub(crate) fn finish(self, region: &mut Value) -> Result<u64> {
        if self.states.is_empty() {
            return Ok(0);
        }
        let m = object(region)?;
        let Some(Value::List(p)) = m.get("BlockStatePalette") else {
            bail!("最终调色板缺失")
        };
        if self.states.iter().any(|(i, v)| p.get(*i) != Some(v)) {
            bail!("最终替换与已迁移的花盆内容或音符盒状态冲突")
        }
        for (below, expected) in self.supports {
            let below = below
                .map(|i| p.get(i).and_then(name).context("音符盒下方调色板缺失"))
                .transpose()?;
            if instrument(below, self.target)? != expected {
                bail!("目标旧音符盒由下方实际方块决定乐器，不能等价保存 {expected}")
            }
        }
        if !self.pending.is_empty() {
            let Value::List(tiles) = m
                .entry("TileEntities".to_owned())
                .or_insert(Value::List(vec![]))
            else {
                bail!("TileEntities 不是 List")
            };
            tiles.extend(self.pending);
        }
        Ok(self.count)
    }
}

pub(crate) fn prepare(
    region: &mut Value,
    source: i32,
    target: i32,
    cancel: &AtomicBool,
) -> Result<Bound> {
    if (source < 1519) == (target < 1519) {
        return Ok(Bound::default());
    }
    let m = object(region)?;
    let Some(Value::List(original)) = m.get("BlockStatePalette") else {
        return Ok(Bound::default());
    };
    if !original.iter().any(|v| name(v).is_some_and(relevant)) {
        return Ok(Bound::default());
    }
    let Some(Value::Compound(sz)) = m.get("Size") else {
        bail!("花盆/音符盒缺少 Size")
    };
    let size = Vec3i {
        x: int(sz, "x")?,
        y: int(sz, "y")?,
        z: int(sz, "z")?,
    };
    let (w, h, l) = (
        size.x.unsigned_abs() as usize,
        size.y.unsigned_abs() as usize,
        size.z.unsigned_abs() as usize,
    );
    let plane = w.checked_mul(l).context("选区过大")?;
    let Some(Value::LongArray(bits)) = m.get("BlockStates") else {
        bail!("花盆/音符盒缺少方块索引")
    };
    let mut indices = Vec::with_capacity(region_volume(&size)?);
    for_each_palette_index(
        bits,
        region_volume(&size)?,
        bits_for_palette(original.len()),
        original.len(),
        |i, b| {
            if i % 65536 == 0 {
                crate::version_conversion::check_cancel(cancel)?
            }
            if b >= original.len() {
                bail!("花盆/音符盒方块索引越过调色板")
            }
            indices.push(b);
            Ok(())
        },
    )?;
    let source_indices = indices.clone();
    let mut palette = original.clone();
    let mut first_use = HashSet::new();
    let mut removed = HashSet::new();
    let tiles = match m.get("TileEntities") {
        Some(Value::List(t)) => t.as_slice(),
        None => &[],
        _ => bail!("TileEntities 不是 List"),
    };
    let mut positions = HashMap::new();
    for (i, v) in tiles.iter().enumerate() {
        let Value::Compound(t) = v else { continue };
        let (Ok(x), Ok(y), Ok(z)) = (int(t, "x"), int(t, "y"), int(t, "z")) else {
            if tile_kind(v).is_some() {
                bail!("花盆/音符盒实体坐标无效")
            };
            continue;
        };
        if x < 0 || y < 0 || z < 0 || x as usize >= w || y as usize >= h || z as usize >= l {
            if tile_kind(v).is_some() {
                bail!("花盆/音符盒实体坐标越界")
            };
            continue;
        }
        let pos = (y as usize * l + z as usize) * w + x as usize;
        if name(&original[source_indices[pos]]).is_some_and(relevant)
            && positions.insert(pos, i).is_some()
        {
            bail!("花盆/音符盒位置有重复方块实体")
        }
    }
    let upgrade = source < 1519;
    let db = database()?;
    let legacy = db.registry_by_data_version(if upgrade { source } else { target })?;
    let dest = db.registry_by_data_version(target)?;
    let mut bound = Bound {
        target,
        ..Bound::default()
    };
    for (pos, idx) in indices.iter_mut().enumerate() {
        if pos % 65536 == 0 {
            crate::version_conversion::check_cancel(cancel)?
        }
        let original_idx = *idx;
        let block = &original[original_idx];
        let Some(n) = name(block).filter(|n| relevant(n)) else {
            continue;
        };
        let note = n == NOTE || n == "minecraft:noteblock";
        let kind = if note { "minecraft:noteblock" } else { FLOWER };
        let properties = props(block)?;
        let t = positions.get(&pos).copied();
        if t.is_some_and(|i| !upgrade || tile_kind(&tiles[i]) != Some(kind)) {
            bail!("花盆/音符盒位置 {pos} 存在不匹配的方块实体，拒绝覆盖")
        }
        let mut data = Fields::new();
        if let Some(i) = t {
            let Value::Compound(existing) = &tiles[i] else {
                unreachable!()
            };
            let Some(Value::String(id)) = existing.get("id") else {
                bail!("花盆/音符盒缺少 id")
            };
            if version_entity_registry::canonical(Kind::BlockEntity, source, id)? != Some(kind) {
                bail!("旧版来源注册表没有 {id}")
            }
            let specific = if note {
                ["note", "powered"]
            } else {
                ["Item", "Data"]
            };
            if let Some(k) = existing.keys().find(|k| {
                !["x", "y", "z", "id"].contains(&k.as_str()) && !specific.contains(&k.as_str())
            }) {
                bail!("花盆/音符盒 {id}/{k} 没有现代方块状态载体")
            }
            data = existing.clone();
            removed.insert(i);
        }
        if !upgrade {
            let id = version_entity_registry::encode(Kind::BlockEntity, target, kind)?
                .context("目标旧版缺少方块实体注册项")?;
            data.extend([
                ("id".into(), Value::String(id.into())),
                ("x".into(), Value::Int((pos % w) as i32)),
                ("y".into(), Value::Int((pos / plane) as i32)),
                ("z".into(), Value::Int((pos / w % l) as i32)),
            ]);
        }
        let below = pos.checked_sub(plane).map(|p| source_indices[p]);
        let converted = if note {
            if upgrade {
                if !properties.is_empty() {
                    bail!("旧音符盒含未知调色板属性")
                }
                let pitch = match data.get("note") {
                    None => 0,
                    Some(Value::Byte(v @ 0..=24)) => *v,
                    _ => bail!("旧音符盒 note 必须为 0..24 Byte"),
                };
                let power = match data.get("powered") {
                    None | Some(Value::Byte(0)) => false,
                    Some(Value::Byte(1)) if source >= 169 => true,
                    _ => bail!("来源旧音符盒不支持该 powered 状态"),
                };
                let inst = instrument(
                    below
                        .map(|i| name(&original[i]).context("音符盒下方缺少方块名称"))
                        .transpose()?,
                    source,
                )?;
                nbt(
                    NOTE,
                    &[
                        ("instrument", inst.into()),
                        ("note", pitch.to_string()),
                        ("powered", power.to_string()),
                    ],
                )
            } else {
                if properties
                    .keys()
                    .any(|k| !["note", "powered", "instrument"].contains(&k.as_str()))
                {
                    bail!("音符盒含未知方块属性")
                }
                let pitch = properties
                    .get("note")
                    .map(String::as_str)
                    .unwrap_or("0")
                    .parse::<i8>()?;
                if !(0..=24).contains(&pitch) {
                    bail!("音符盒音高超出 0..24")
                }
                let power = match properties.get("powered").map(String::as_str) {
                    Some("true") => true,
                    None | Some("false") => false,
                    _ => bail!("音符盒 powered 无效"),
                };
                if power && target < 169 {
                    bail!("1.8 音符盒没有持久 powered 状态")
                }
                data.insert("note".into(), Value::Byte(pitch));
                if target >= 169 {
                    data.insert("powered".into(), Value::Byte(i8::from(power)));
                }
                bound.supports.push((
                    below,
                    properties
                        .get("instrument")
                        .cloned()
                        .unwrap_or_else(|| "harp".into()),
                ));
                nbt(NOTE, &[])
            }
        } else if upgrade {
            let mut carried = (n != FLOWER).then(|| n.to_string());
            if properties
                .keys()
                .any(|k| k != "contents" && k != "legacy_data")
            {
                bail!("旧花盆含未知调色板属性")
            }
            if let Some(raw) = properties.get("legacy_data") {
                let k = raw.parse::<u8>()?;
                if k > 15 {
                    bail!("旧花盆 legacy_data 超过四位")
                }
                if carried.is_none() && k != 0 {
                    carried = Some(
                        legacy
                            .legacy_blocks
                            .get(&format!("140:{k}"))
                            .context("旧花盆元数据缺少映射")?
                            .clone(),
                    );
                }
            }
            if let Some(contents) = properties.get("contents").filter(|s| s.as_str() != "empty") {
                let plant = match contents.as_str() {
                    "dead_bush" => "dead_bush",
                    "houstonia" => "azure_bluet",
                    v => v,
                };
                let c = format!("minecraft:potted_{plant}");
                if !POTS.iter().any(|(_, _, v)| *v == plant) {
                    bail!("未知旧花盆 contents={contents}")
                }
                if carried.as_ref().is_some_and(|v| v != &c) {
                    bail!("旧花盆调色板内的花种冲突")
                }
                carried = Some(c);
            }
            let plant = if t.is_some() {
                let explicit = pot_payload(&data, legacy)?;
                if carried.as_ref().is_some_and(|v| v != &explicit) {
                    bail!("旧花盆 Item/Data 与调色板记录的内容冲突")
                }
                explicit
            } else {
                carried.unwrap_or_else(|| FLOWER.into())
            };
            nbt(&plant, &[])
        } else {
            if !properties.is_empty() {
                bail!("现代花盆有旧版不能保存的属性")
            }
            old_flower(&mut data, n, legacy)?;
            nbt(FLOWER, &[])
        };
        let key = name(&converted).unwrap();
        let p = props(&converted)?;
        let state = if p.is_empty() {
            key.to_string()
        } else {
            format!(
                "{key}[{}]",
                p.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        if !dest
            .blocks
            .get(key)
            .is_some_and(|def| def.accepts(&state, &p))
        {
            bail!("目标原版注册表没有花盆/音符盒状态 {state}")
        }
        if !upgrade {
            bound.pending.push(Value::Compound(data))
        }
        if t.is_some() || !upgrade {
            bound.count += 1
        }
        if first_use.insert(original_idx) {
            palette[original_idx] = converted.clone();
        } else if palette[original_idx] != converted {
            *idx = palette
                .iter()
                .enumerate()
                .find(|(i, v)| (*i >= original.len() || first_use.contains(i)) && *v == &converted)
                .map(|(i, _)| i)
                .unwrap_or_else(|| {
                    palette.push(converted.clone());
                    palette.len() - 1
                });
        }
        bound.states.insert(*idx, converted);
    }
    if tiles
        .iter()
        .enumerate()
        .any(|(i, v)| tile_kind(v).is_some() && !removed.contains(&i))
    {
        bail!("旧花盆/音符盒实体与实际方块位置不匹配")
    }
    // Remove unused family entries, including old unsupported default states;
    // all unrelated palette indices retain their relative order.
    let used: HashSet<_> = indices.iter().copied().collect();
    let mut remap = vec![0; palette.len()];
    let mut out = Vec::new();
    for (i, b) in palette.into_iter().enumerate() {
        if used.contains(&i) || !name(&b).is_some_and(relevant) {
            remap[i] = out.len();
            out.push(b)
        }
    }
    for i in &mut indices {
        *i = remap[*i]
    }
    bound.states = bound
        .states
        .into_iter()
        .map(|(i, b)| (remap[i], b))
        .collect();
    for (i, _) in &mut bound.supports {
        if let Some(v) = i {
            *v = remap[*v]
        }
    }
    let rest: Vec<_> = tiles
        .iter()
        .enumerate()
        .filter(|(i, _)| !removed.contains(i))
        .map(|(_, t)| t.clone())
        .collect();
    let packed = pack_palette_indices(&indices, bits_for_palette(out.len()));
    m.insert("BlockStatePalette".into(), Value::List(out));
    m.insert(
        "BlockStates".into(),
        Value::LongArray(fastnbt::LongArray::new(
            (0..packed.len()).map(|i| packed.i64_at(i)).collect(),
        )),
    );
    if !removed.is_empty() {
        m.insert("TileEntities".into(), Value::List(rest));
    }
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn map<const N: usize>(fields: [(&str, Value); N]) -> Value {
        Value::Compound(fields.map(|(k, v)| (k.to_string(), v)).into())
    }
    fn tile(x: i32, id: &str, values: &[(&str, Value)]) -> Value {
        let mut t = Fields::from([
            ("id".into(), Value::String(id.into())),
            ("x".into(), Value::Int(x)),
            ("y".into(), Value::Int(1)),
            ("z".into(), Value::Int(0)),
        ]);
        t.extend(values.iter().map(|(k, v)| (k.to_string(), v.clone())));
        Value::Compound(t)
    }
    fn sample() -> Value {
        let palette = vec![
            nbt("minecraft:air", &[]),
            nbt("minecraft:stone", &[]),
            nbt("minecraft:noteblock", &[]),
            nbt(FLOWER, &[]),
        ];
        let packed =
            pack_palette_indices(&[0, 1, 0, 0, 2, 2, 3, 3], bits_for_palette(palette.len()));
        map([
            (
                "Size",
                map([
                    ("x", Value::Int(4)),
                    ("y", Value::Int(2)),
                    ("z", Value::Int(1)),
                ]),
            ),
            (
                "Position",
                map([
                    ("x", Value::Int(0)),
                    ("y", Value::Int(0)),
                    ("z", Value::Int(0)),
                ]),
            ),
            ("BlockStatePalette", Value::List(palette)),
            (
                "BlockStates",
                Value::LongArray(fastnbt::LongArray::new(
                    (0..packed.len()).map(|i| packed.i64_at(i)).collect(),
                )),
            ),
            (
                "TileEntities",
                Value::List(vec![
                    tile(
                        0,
                        "minecraft:noteblock",
                        &[("note", Value::Byte(9)), ("powered", Value::Byte(1))],
                    ),
                    tile(
                        1,
                        "minecraft:noteblock",
                        &[("note", Value::Byte(22)), ("powered", Value::Byte(0))],
                    ),
                    tile(
                        2,
                        FLOWER,
                        &[("Item", Value::Int(38)), ("Data", Value::Int(4))],
                    ),
                    tile(
                        3,
                        FLOWER,
                        &[
                            ("Item", Value::String("minecraft:sapling".into())),
                            ("Data", Value::Int(2)),
                        ],
                    ),
                ]),
            ),
        ])
    }
    fn convert(v: &mut Value, source: i32, target: i32) -> Result<u64> {
        let mut candidate = v.clone();
        let bound = prepare(&mut candidate, source, target, &AtomicBool::new(false))?;
        let report = super::super::migrate(&mut candidate, source, target)?;
        if !report.issues.is_empty() {
            bail!("{:?}", report.issues)
        }
        let count = bound.finish(&mut candidate)?;
        *v = candidate;
        Ok(count)
    }
    fn blocks(v: &Value) -> Vec<Value> {
        let Value::Compound(m) = v else { panic!() };
        let Value::List(p) = &m["BlockStatePalette"] else {
            panic!()
        };
        let Value::LongArray(b) = &m["BlockStates"] else {
            panic!()
        };
        let mut out = vec![];
        for_each_palette_index(
            b,
            8,
            bits_for_palette(p.len()),
            p.len(),
            |_, i| {
                out.push(p[i].clone());
                Ok(())
            },
        )
        .unwrap();
        out
    }
    #[test]
    fn music_pots_split_shared_positions_preserve_sound_power_plants_and_reopen_numeric_schematic()
    {
        let mut value = sample();
        assert_eq!(convert(&mut value, 1343, 5023).unwrap(), 4);
        let modern = blocks(&value);
        assert_eq!(
            &modern[4..],
            &[
                nbt(
                    NOTE,
                    &[
                        ("instrument", "harp".into()),
                        ("note", "9".into()),
                        ("powered", "true".into())
                    ]
                ),
                nbt(
                    NOTE,
                    &[
                        ("instrument", "basedrum".into()),
                        ("note", "22".into()),
                        ("powered", "false".into())
                    ]
                ),
                nbt("minecraft:potted_red_tulip", &[]),
                nbt("minecraft:potted_birch_sapling", &[])
            ]
        );
        assert_eq!(
            object(&mut value).unwrap()["TileEntities"],
            Value::List(vec![])
        );
        assert_eq!(convert(&mut value, 5023, 1343).unwrap(), 4);
        let db = database().unwrap();
        let legacy = crate::version_legacy_encoding::encode(
            &map([("Regions", map([("r", value.clone())]))]),
            db.registry_by_data_version(1343).unwrap(),
        )
        .unwrap();
        let encoded = fastnbt::to_bytes(&legacy).unwrap();
        let Value::Compound(reopened) = fastnbt::from_bytes(&encoded).unwrap() else {
            panic!()
        };
        let Value::ByteArray(ids) = &reopened["Blocks"] else {
            panic!()
        };
        assert_eq!(&ids[4..], &[25, 25, 140u8 as i8, 140u8 as i8]);
        let Value::List(tiles) = &reopened["TileEntities"] else {
            panic!()
        };
        assert_eq!(
            tiles[0],
            tile(
                0,
                "minecraft:noteblock",
                &[("note", Value::Byte(9)), ("powered", Value::Byte(1))]
            )
        );
        assert_eq!(
            tiles[2],
            tile(
                2,
                FLOWER,
                &[
                    ("Item", Value::String("minecraft:red_flower".into())),
                    ("Data", Value::Int(4))
                ]
            )
        );
        assert_eq!(convert(&mut value, 1343, 5023).unwrap(), 4);
        assert_eq!(blocks(&value), modern);
    }
    #[test]
    fn unknown_flower_nbt_pitch_and_unrepresentable_power_or_instrument_leave_original() {
        for (slot, key, val) in [
            (0, "note", Value::Byte(25)),
            (2, "Data", Value::Int(127)),
            (2, "CustomName", Value::String("keep".into())),
        ] {
            let mut value = sample();
            let Value::List(tiles) = object(&mut value).unwrap().get_mut("TileEntities").unwrap()
            else {
                panic!()
            };
            object(&mut tiles[slot]).unwrap().insert(key.into(), val);
            let before = value.clone();
            assert!(prepare(&mut value, 1343, 5023, &AtomicBool::new(false)).is_err());
            assert_eq!(value, before);
        }
        let mut value = sample();
        convert(&mut value, 1343, 5023).unwrap();
        let before = value.clone();
        assert!(
            convert(&mut value, 5023, 0)
                .unwrap_err()
                .to_string()
                .contains("powered")
        );
        assert_eq!(value, before);
        let Value::List(palette) = object(&mut value)
            .unwrap()
            .get_mut("BlockStatePalette")
            .unwrap()
        else {
            panic!()
        };
        let block = palette.iter_mut().find(|v| name(v) == Some(NOTE)).unwrap();
        let p = object(block).unwrap().get_mut("Properties").unwrap();
        object(p)
            .unwrap()
            .insert("instrument".into(), Value::String("banjo".into()));
        let before = value.clone();
        assert!(
            convert(&mut value, 5023, 1343)
                .unwrap_err()
                .to_string()
                .contains("banjo")
        );
        assert_eq!(value, before);
    }
    #[test]
    fn target_replacement_cannot_orphan_a_generated_tile() {
        let mut v = sample();
        convert(&mut v, 1343, 5023).unwrap();
        let bound = prepare(&mut v, 5023, 1343, &AtomicBool::new(false)).unwrap();
        let Value::List(palette) = object(&mut v)
            .unwrap()
            .get_mut("BlockStatePalette")
            .unwrap()
        else {
            panic!()
        };
        *palette
            .iter_mut()
            .find(|b| name(b) == Some(FLOWER))
            .unwrap() = nbt("minecraft:stone", &[]);
        assert!(
            bound
                .finish(&mut v)
                .unwrap_err()
                .to_string()
                .contains("最终替换")
        );
        assert_eq!(object(&mut v).unwrap()["TileEntities"], Value::List(vec![]));
    }
}
