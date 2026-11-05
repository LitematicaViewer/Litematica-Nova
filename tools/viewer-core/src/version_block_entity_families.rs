//! Sculk sensor/shrieker listeners, catalyst charge cursors and cooking state.
//! Release boundaries are the first supported releases with each codec, not
//! guesses from entity names: mapped 1.17/1.18, 1.19–1.19.2 VibrationListener,
//! 1.19.3/4 VibrationListener + VibrationSelector, and 1.20+/26.3
//! VibrationSystem.Data. ChargeCursor reads the same fields at both ends.

use super::Fields;
use anyhow::{Context, Result, bail};
use fastnbt::{IntArray, Value};

pub(super) fn fields(id: &str) -> &'static [&'static str] {
    match id {
        "minecraft:sculk_sensor" | "minecraft:calibrated_sculk_sensor" => {
            &["last_vibration_frequency", "listener"]
        }
        "minecraft:sculk_shrieker" => &["warning_level", "listener"],
        "minecraft:sculk_catalyst" => &["cursors"],
        _ => &[],
    }
}

const LISTENER: i32 = 3105; // 1.19: listener is persisted for the first time.
const SELECTOR: i32 = 3218; // 1.19.3: candidate selector replaces separate event_distance.
const DERIVED_SOURCE: i32 = 3463; // 1.20: sensor/shrieker supply their own position and radius.

fn compound<'a>(value: &'a Value, field: &str) -> Result<&'a Fields> {
    let Value::Compound(map) = value else {
        bail!("{field} 不是 Compound")
    };
    Ok(map)
}

fn keys(map: &Fields, allowed: &[&str], field: &str) -> Result<()> {
    if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        bail!("{field}/{key} 是未支持字段，原数据保留")
    }
    Ok(())
}

fn integer(map: &Fields, key: &str, min: i32, max: i32) -> Result<()> {
    if map
        .get(key)
        .is_some_and(|v| !matches!(v, Value::Int(n) if (min..=max).contains(n)))
    {
        bail!("{key} 不是 {min}..{max} 内的 Int")
    }
    Ok(())
}

fn cursors(value: &Value) -> Result<()> {
    let Value::List(cursors) = value else {
        bail!("cursors 不是列表")
    };
    if cursors.len() > 32 {
        bail!("原版催发体最多保存32个传播游标")
    }
    for cursor in cursors {
        let map = compound(cursor, "cursors")?;
        keys(
            map,
            &["pos", "charge", "decay_delay", "update_delay", "facings"],
            "cursors",
        )?;
        if !matches!(map.get("pos"), Some(Value::IntArray(p)) if p.len() == 3) {
            bail!("游标 pos 必须是3个 Int")
        }
        integer(map, "charge", 0, 1000)?;
        integer(map, "decay_delay", 0, 1)?;
        integer(map, "update_delay", 0, i32::MAX)?;
        if let Some(facings) = map.get("facings") {
            let Value::List(facings) = facings else {
                bail!("facings 不是列表")
            };
            let mut seen = std::collections::HashSet::new();
            for facing in facings {
                let Value::String(name) = facing else {
                    bail!("传播方向不是 String")
                };
                if !["up", "down", "north", "south", "west", "east"].contains(&name.as_str())
                    || !seen.insert(name)
                {
                    bail!("传播方向未知或重复：{name}")
                }
            }
        }
    }
    Ok(())
}

// Exact start/end ranges from minecraft:game_event in all supported 1.19+
// oracle/builtin_registries.jsonl releases. Removed events are not silently
// aliased to similar events (there need not be an equivalent game mechanic).
fn game_event(version: i32, name: &str) -> bool {
    let id = name.strip_prefix("minecraft:").unwrap_or(name);
    if version < LISTENER {
        return false;
    }
    match id {
        "block_activate"
        | "block_attach"
        | "block_change"
        | "block_close"
        | "block_deactivate"
        | "block_destroy"
        | "block_detach"
        | "block_open"
        | "block_place"
        | "container_close"
        | "container_open"
        | "drink"
        | "eat"
        | "elytra_glide"
        | "entity_damage"
        | "entity_die"
        | "entity_interact"
        | "entity_place"
        | "equip"
        | "explode"
        | "flap"
        | "fluid_pickup"
        | "fluid_place"
        | "hit_ground"
        | "instrument_play"
        | "item_interact_finish"
        | "item_interact_start"
        | "lightning_strike"
        | "note_block_play"
        | "prime_fuse"
        | "projectile_land"
        | "projectile_shoot"
        | "sculk_sensor_tendrils_clicking"
        | "shear"
        | "shriek"
        | "splash"
        | "step"
        | "swim"
        | "teleport" => true,
        "dispense_fail" | "piston_contract" | "piston_extend" => version < DERIVED_SOURCE,
        "entity_roar" | "entity_shake" => version < 3578,
        "entity_action" | "unequip" => version >= 3578,
        "jukebox_play" | "jukebox_stop_play" => version >= 3117,
        "entity_mount" | "entity_dismount" => version >= 3337,
        "resonate_1" | "resonate_2" | "resonate_3" | "resonate_4" | "resonate_5" | "resonate_6"
        | "resonate_7" | "resonate_8" | "resonate_9" | "resonate_10" | "resonate_11"
        | "resonate_12" | "resonate_13" | "resonate_14" | "resonate_15" => {
            version >= DERIVED_SOURCE
        }
        "bounce" => version >= 4903,
        _ => false,
    }
}

fn event(value: &Value, source: i32, target: i32) -> Result<f32> {
    let map = compound(value, "event")?;
    keys(
        map,
        &[
            "game_event",
            "distance",
            "pos",
            "source",
            "projectile_owner",
        ],
        "event",
    )?;
    let Some(Value::String(id)) = map.get("game_event") else {
        bail!("振动事件缺少注册名")
    };
    if !game_event(source, id) || !game_event(target, id) {
        bail!("振动事件 {id} 未在两端同时注册")
    }
    let Some(Value::Float(distance)) = map.get("distance") else {
        bail!("振动事件缺少 Float 距离")
    };
    if !distance.is_finite() || *distance < 0.0 {
        bail!("振动事件距离无效")
    }
    if !matches!(map.get("pos"), Some(Value::List(pos)) if pos.len()==3 && pos.iter().all(|v| matches!(v,Value::Double(n) if n.is_finite())))
    {
        bail!("振动事件 pos 不是3个有限 Double")
    }
    for key in ["source", "projectile_owner"] {
        if map
            .get(key)
            .is_some_and(|v| !matches!(v, Value::IntArray(parts) if parts.len()==4))
        {
            bail!("振动事件 {key} 不是UUID")
        }
    }
    Ok(*distance)
}

fn empty_selector() -> Value {
    Value::Compound([("tick".into(), Value::Long(-1))].into())
}

fn position_source(value: &Value) -> Result<()> {
    let map = compound(value, "listener/source")?;
    let Some(Value::String(kind)) = map.get("type") else {
        bail!("振动监听位置缺少 type")
    };
    match kind.strip_prefix("minecraft:").unwrap_or(kind) {
        "block" => {
            keys(map, &["type", "pos"], "listener/source")?;
            if !matches!(map.get("pos"), Some(Value::IntArray(p)) if p.len()==3) {
                bail!("监听方块位置无效")
            }
        }
        "entity" => {
            keys(
                map,
                &["type", "source_entity", "y_offset"],
                "listener/source",
            )?;
            if !matches!(map.get("source_entity"), Some(Value::IntArray(p)) if p.len()==4)
                || map
                    .get("y_offset")
                    .is_some_and(|v| !matches!(v,Value::Float(n) if n.is_finite()))
            {
                bail!("监听实体位置或偏移无效")
            }
        }
        _ => bail!("未知振动监听位置类型 {kind}"),
    }
    Ok(())
}

enum ListenerOwner {
    Block {
        pos: [i32; 3],
        radius: i32,
    },
    Mob {
        uuid: Option<Value>,
        eye_height: f32,
    },
}

impl ListenerOwner {
    fn radius(&self) -> i32 {
        match self {
            Self::Block { radius, .. } => *radius,
            Self::Mob { .. } => 16,
        }
    }

    // Only needed when crossing from/to the legacy geometry carrier. An NBT
    // entity with no saved UUID still has a perfectly representable 1.20+ Data.
    fn geometry(&self) -> Result<Fields> {
        Ok(match self {
            Self::Block { pos, .. } => [
                ("type".into(), Value::String("minecraft:block".into())),
                ("pos".into(), Value::IntArray(IntArray::new(pos.to_vec()))),
            ]
            .into(),
            Self::Mob { uuid, eye_height } => {
                let Some(Value::IntArray(parts)) = uuid else {
                    bail!("旧实体监听器需要实际拥有者 UUID，拒绝伪造")
                };
                if parts.len() != 4 {
                    bail!("实体监听器拥有者 UUID 不是4个 Int")
                }
                [
                    ("type".into(), Value::String("minecraft:entity".into())),
                    ("source_entity".into(), Value::IntArray(parts.clone())),
                    ("y_offset".into(), Value::Float(*eye_height)),
                ]
                .into()
            }
        })
    }
}

fn listener(map: &mut Fields, source: i32, target: i32, owner: ListenerOwner) -> Result<bool> {
    if source < LISTENER {
        bail!("来源版本没有持久振动监听器")
    }
    let allowed: &[&str] = if source < SELECTOR {
        &["source", "range", "event", "event_distance", "event_delay"]
    } else if source < DERIVED_SOURCE {
        &["source", "range", "event", "selector", "event_delay"]
    } else {
        &["event", "selector", "event_delay"]
    };
    keys(map, allowed, "listener")?;
    integer(map, "event_delay", 0, i32::MAX)?;
    let distance = map
        .get("event")
        .map(|v| event(v, source, target))
        .transpose()?;
    if source >= SELECTOR {
        let selector = compound(
            map.get("selector").context("监听器缺少 selector")?,
            "selector",
        )?;
        keys(selector, &["tick", "event"], "selector")?;
        let Some(Value::Long(tick)) = selector.get("tick") else {
            bail!("候选振动缺少 Long tick")
        };
        if let Some(v) = selector.get("event") {
            event(v, source, target)?;
        } else if *tick != -1 {
            bail!("空候选振动的 tick 不是 -1，原版会丢弃该值")
        }
    } else if map
        .get("event_distance")
        .is_some_and(|v| !matches!(v, Value::Float(n) if n.is_finite() && *n>=0.0))
    {
        bail!("event_distance 不是非负有限 Float")
    }
    let source_derived = source >= DERIVED_SOURCE;
    let target_stores_geometry = (LISTENER..DERIVED_SOURCE).contains(&target);
    if !source_derived {
        let geometry = map.get("source").context("监听器缺少 source")?;
        position_source(geometry)?;
        let Some(Value::Int(range @ 0..)) = map.get("range") else {
            bail!("监听器缺少有效范围")
        };
        if !target_stores_geometry {
            let geom = compound(geometry, "listener/source")?;
            let mut expected = owner.geometry()?;
            // PositionSource dispatch accepts names with or without minecraft:.
            if let (Some(Value::String(input)), Some(Value::String(kind))) =
                (geom.get("type"), expected.get_mut("type"))
            {
                if kind.strip_prefix("minecraft:") == Some(input.as_str()) {
                    *kind = input.clone();
                }
            }
            if *range != owner.radius() || geom != &expected {
                bail!("目标监听位置/范围由拥有者决定，不能保留自定义监听位置/范围")
            }
            map.remove("source");
            map.remove("range");
        }
    } else if target_stores_geometry {
        map.insert("source".into(), Value::Compound(owner.geometry()?));
        map.insert("range".into(), Value::Int(owner.radius()));
    }
    if source < SELECTOR && (target >= SELECTOR || target < LISTENER) {
        let old = match map.remove("event_distance") {
            Some(Value::Float(n)) => n,
            _ => 0.0,
        };
        if old != distance.unwrap_or(0.0) {
            bail!("目标没有单独的 event_distance，保存值与事件距离不相等")
        }
        if target >= SELECTOR {
            map.insert("selector".into(), empty_selector());
        }
    } else if source >= SELECTOR && target < SELECTOR {
        if map.get("selector") != Some(&empty_selector()) {
            bail!("目标版本没有待选择振动事件的载体")
        }
        map.remove("selector");
        if target >= LISTENER {
            map.insert(
                "event_distance".into(),
                Value::Float(distance.unwrap_or(0.0)),
            );
        }
    }
    if target < LISTENER {
        if distance.is_some() || map.get("event_delay").is_some_and(|v| v != &Value::Int(0)) {
            bail!("目标版本没有持久监听器，不能保留正在传输的振动")
        }
        return Ok(false);
    }
    Ok(true)
}

// The shared inventory path still migrates each ItemStack, rejects duplicates
// and checks Slot types. Check the version-dependent fourth/fifth slot here.
fn cooking_inventory(tile: &Fields, four_slots: bool) -> Result<()> {
    if let Some(items) = tile.get("Items") {
        let Value::List(items) = items else {
            bail!("烹饪库存 Items 不是 List")
        };
        if four_slots && items.iter().any(|item| matches!(item,Value::Compound(stack) if stack.get("Slot")==Some(&Value::Byte(4)))) {
            bail!("旧酿造台只有槽位 0..3，没有燃料槽")
        }
    }
    Ok(())
}

fn campfire(tile: &Fields) -> Result<()> {
    cooking_inventory(tile, false)?;
    for name in ["CookingTimes", "CookingTotalTimes"] {
        // CampfireBlockEntity copies only the first four values. Short arrays
        // have well-defined zero-filled tails; a fifth entry would be lost.
        if tile
            .get(name)
            .is_some_and(|v| !matches!(v,Value::IntArray(times) if times.len() <= 4))
        {
            bail!("营火 {name} 必须为最多四槽的 IntArray")
        }
    }
    Ok(())
}

fn brewing(tile: &mut Fields, source: i32, target: i32) -> Result<()> {
    // Real 1.8/1.9 TileEntityBrewingStand: no Fuel or fifth slot in 1.8.
    // 26.2 still calls getShortOr/getByteOr. The 26.3 save/load methods
    // introduce int timers, totalBrewTime=400, totalFuel=20 and multiplier=1.
    const EXTENDED: i32 = 5023;
    cooking_inventory(tile, source < 169 || target < 169)?;
    for (name, default) in [
        ("total_brew_time", Value::Int(400)),
        ("total_fuel", Value::Int(20)),
        ("speed_multiplier", Value::Float(1.0)),
    ] {
        if let Some(value) = tile.get(name) {
            if source < EXTENDED {
                bail!("来源旧酿造台不支持 {name}")
            }
            if target < EXTENDED {
                if value != &default {
                    bail!("目标旧酿造台不能保存自定义 {name}")
                }
                tile.remove(name);
            }
        } else if source < EXTENDED && target >= EXTENDED {
            tile.insert(name.into(), default);
        }
    }
    for name in ["BrewTime", "Fuel"] {
        let Some(value) = tile.get_mut(name) else {
            continue;
        };
        let n = match *value {
            Value::Byte(n) => i32::from(n),
            Value::Short(n) => i32::from(n),
            Value::Int(n) => n,
            _ => bail!("酿造台 {name} 不是整数"),
        };
        if name == "Fuel" && (source < 169 || target < 169) {
            if source < 169 || n != 0 {
                bail!("1.8 酿造台没有 Fuel，不能保存燃料")
            }
            tile.remove(name);
        } else if source < EXTENDED || target < EXTENDED {
            let narrow = if name == "BrewTime" {
                Value::Short(i16::try_from(n).context("酿造时间超过旧版 Short")?)
            } else {
                Value::Byte(i8::try_from(n).context("燃料超过旧版 Byte")?)
            };
            *value = if target >= EXTENDED {
                Value::Int(n)
            } else {
                narrow
            };
        }
    }
    Ok(())
}

// Warden and Allay use the same event/selector as the sculk blocks, but their
// pre-1.20 position source is the mob itself, never a block coordinate. Actual
// 1.19/1.19.3 VibrationListener constructors: owner.getEyeHeight(), radius 16.
// 1.20.1 and 26.3 save "listener" with VibrationSystem.Data.CODEC instead.
pub(super) fn migrate_vibration_mob(
    mob: &mut Fields,
    id: &str,
    source: i32,
    target: i32,
) -> Result<()> {
    let eye_height = match id {
        "minecraft:warden" => 2.9_f32 * 0.85,
        "minecraft:allay" => 0.6_f32 * 0.6,
        _ => return Ok(()),
    };
    if source < LISTENER || target < LISTENER {
        bail!("来源或目标版本没有实体 {id}")
    }
    if id == "minecraft:warden"
        && let Some(v) = mob.get("anger")
    {
        let anger = compound(v, "anger")?;
        keys(anger, &["suspects"], "anger")?;
        if let Some(v) = anger.get("suspects") {
            let Value::List(entries) = v else {
                bail!("anger/suspects 不是 List")
            };
            let mut seen = std::collections::HashSet::new();
            for suspect in entries {
                let suspect = compound(suspect, "anger/suspects")?;
                keys(suspect, &["uuid", "anger"], "anger/suspects")?;
                let Some(Value::IntArray(uuid)) = suspect.get("uuid") else {
                    bail!("嫌疑对象的 uuid 不是 IntArray")
                };
                let parts: [i32; 4] = uuid[..].try_into().context("嫌疑对象的 uuid 不是4个 Int")?;
                // Vanilla populates a UUID->anger map; later duplicates would
                // otherwise overwrite a suspect despite the List being valid NBT.
                if !seen.insert(parts) {
                    bail!("嫌疑对象的 uuid 重复，原版会覆盖前一条记录")
                }
                if !matches!(suspect.get("anger"), Some(Value::Int(0..))) {
                    bail!("嫌疑对象的 anger 不是非负 Int")
                }
            }
        }
    }
    let uuid = mob.get("UUID").cloned();
    if let Some(value) = mob.get_mut("listener") {
        let Value::Compound(data) = value else {
            bail!("实体 listener 不是 Compound")
        };
        // A schematic may lack the actual entity UUID. Old loadAdditional then
        // uses the constructor's live owner if the listener tag is absent.
        // Only a completely idle, default Data can take that path.
        if uuid.is_none()
            && source >= DERIVED_SOURCE
            && target < DERIVED_SOURCE
            && data.get("selector") == Some(&empty_selector())
            && data.keys().all(|k| k == "selector" || k == "event_delay")
            && data.get("event_delay").is_none_or(|v| v == &Value::Int(0))
        {
            mob.remove("listener");
        } else {
            listener(
                data,
                source,
                target,
                ListenerOwner::Mob { uuid, eye_height },
            )?;
        }
    }
    Ok(())
}

pub(super) fn migrate(tile: &mut Fields, id: &str, source: i32, target: i32) -> Result<()> {
    let radius = match id {
        "minecraft:campfire" => return campfire(tile),
        "minecraft:brewing_stand" => return brewing(tile, source, target),
        "minecraft:sculk_catalyst" => {
            if let Some(v) = tile.get("cursors") {
                cursors(v)?;
            }
            return Ok(());
        }
        "minecraft:sculk_sensor" => 8,
        "minecraft:calibrated_sculk_sensor" => 16,
        "minecraft:sculk_shrieker" => 8,
        _ => return Ok(()),
    };
    if id == "minecraft:sculk_shrieker" {
        integer(tile, "warning_level", 0, 4)?;
    } else {
        integer(tile, "last_vibration_frequency", 0, 15)?;
    }
    if tile.contains_key("listener") {
        let mut pos = [0; 3];
        for (n, axis) in ["x", "y", "z"].iter().enumerate() {
            let Some(Value::Int(coord)) = tile.get(*axis) else {
                bail!("方块实体缺少 {axis} 坐标")
            };
            pos[n] = *coord;
        }
        let Some(Value::Compound(data)) = tile.get_mut("listener") else {
            bail!("listener 不是 Compound")
        };
        if !listener(data, source, target, ListenerOwner::Block { pos, radius })? {
            tile.remove("listener");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map<const N: usize>(pairs: [(&str, Value); N]) -> Value {
        Value::Compound(pairs.map(|(k, v)| (k.to_owned(), v)).into())
    }
    fn region(id: &str, values: &[(&str, Value)]) -> Value {
        let mut tile: Fields = [
            ("id".into(), Value::String(id.into())),
            ("x".into(), Value::Int(-3)),
            ("y".into(), Value::Int(64)),
            ("z".into(), Value::Int(9)),
        ]
        .into();
        tile.extend(values.iter().map(|(k, v)| (k.to_string(), v.clone())));
        map([("TileEntities", Value::List(vec![Value::Compound(tile)]))])
    }
    fn tile(region: &Value) -> &Fields {
        let Value::List(tiles) = &compound(region, "").unwrap()["TileEntities"] else {
            panic!()
        };
        compound(&tiles[0], "").unwrap()
    }
    fn run(v: &mut Value, source: i32, target: i32) {
        let report = super::super::migrate(v, source, target).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
    }
    fn mob_region(id: &str, extra: &[(&str, Value)]) -> Value {
        let mut fields = Fields::from([("id".into(), Value::String(id.into()))]);
        fields.extend(extra.iter().map(|(k, v)| (k.to_string(), v.clone())));
        map([("Entities", Value::List(vec![Value::Compound(fields)]))])
    }
    fn mob(v: &Value) -> &Fields {
        let Value::List(rows) = &compound(v, "").unwrap()["Entities"] else {
            panic!()
        };
        compound(&rows[0], "").unwrap()
    }
    #[test]
    fn reproduced_warden_anger_and_allay_listener_survive_real_1201_and_263_codecs() {
        let uuid = Value::IntArray(IntArray::new(vec![0, 1, 2, 3]));
        let suspect = map([("uuid", uuid.clone()), ("anger", Value::Int(85))]);
        let anger = map([("suspects", Value::List(vec![suspect]))]);
        let selector = map([
            ("event", vibration("minecraft:step")),
            ("tick", Value::Long(120)),
        ]);
        let listener = map([
            ("selector", selector),
            ("event", vibration("minecraft:note_block_play")),
            ("event_delay", Value::Int(2)),
        ]);
        for id in ["minecraft:warden", "minecraft:allay"] {
            let mut v = if id == "minecraft:warden" {
                mob_region(
                    id,
                    &[("anger", anger.clone()), ("listener", listener.clone())],
                )
            } else {
                mob_region(id, &[("listener", listener.clone())])
            };
            for (from, to) in [(3465, 5023), (5023, 3465)] {
                run(&mut v, from, to);
                assert_eq!(mob(&v)["listener"], listener);
                if id == "minecraft:warden" {
                    assert_eq!(mob(&v)["anger"], anger)
                }
            }
        }
    }
    #[test]
    fn mob_listener_crosses_legacy_geometry_only_for_its_actual_uuid_eye_height_and_radius() {
        let uuid = Value::IntArray(IntArray::new(vec![12, 34, 56, 78]));
        let event = vibration("minecraft:step");
        for (id, eye) in [
            ("minecraft:warden", 2.9_f32 * 0.85),
            ("minecraft:allay", 0.6_f32 * 0.6),
        ] {
            let old = map([
                (
                    "source",
                    map([
                        ("type", Value::String("minecraft:entity".into())),
                        ("source_entity", uuid.clone()),
                        ("y_offset", Value::Float(eye)),
                    ]),
                ),
                ("range", Value::Int(16)),
                ("event", event.clone()),
                ("event_delay", Value::Int(2)),
                ("event_distance", Value::Float(3.5)),
            ]);
            let mut v = mob_region(id, &[("UUID", uuid.clone()), ("listener", old.clone())]);
            run(&mut v, 3120, 5023);
            let modern = compound(&mob(&v)["listener"], "").unwrap();
            assert!(!modern.contains_key("source"));
            assert_eq!(modern["selector"], empty_selector());
            assert_eq!(modern["event"], event);
            run(&mut v, 5023, 3120);
            assert_eq!(mob(&v)["listener"], old);
            let mut bad = old.clone();
            let Value::Compound(data) = &mut bad else {
                panic!()
            };
            data.insert("range".into(), Value::Int(24));
            rejected(
                mob_region(id, &[("UUID", uuid.clone()), ("listener", bad)]),
                3120,
                5023,
                "自定义监听位置",
            );
            let active = map([("selector", empty_selector()), ("event", event.clone())]);
            rejected(
                mob_region(id, &[("listener", active)]),
                5023,
                3120,
                "实际拥有者 UUID",
            );
        }
    }
    #[test]
    fn malformed_warden_suspects_and_unknown_listener_keys_are_atomic() {
        let uuid = Value::IntArray(IntArray::new(vec![0, 1, 2, 3]));
        for (single, expected) in [
            (
                map([("uuid", uuid.clone()), ("anger", Value::Int(-1))]),
                "非负 Int",
            ),
            (
                map([
                    ("uuid", Value::IntArray(IntArray::new(vec![1, 2]))),
                    ("anger", Value::Int(85)),
                ]),
                "4个 Int",
            ),
            (
                map([
                    ("uuid", uuid.clone()),
                    ("anger", Value::Int(85)),
                    ("other", Value::Int(7)),
                ]),
                "other",
            ),
        ] {
            rejected(
                mob_region(
                    "minecraft:warden",
                    &[("anger", map([("suspects", Value::List(vec![single]))]))],
                ),
                3465,
                5023,
                expected,
            );
        }
        let row = map([("uuid", uuid), ("anger", Value::Int(85))]);
        rejected(
            mob_region(
                "minecraft:warden",
                &[("anger", map([("suspects", Value::List(vec![row; 2]))]))],
            ),
            3465,
            5023,
            "uuid 重复",
        );
        rejected(
            mob_region(
                "minecraft:allay",
                &[(
                    "listener",
                    map([("selector", empty_selector()), ("custom", Value::Int(42))]),
                )],
            ),
            3465,
            5023,
            "custom",
        );
    }
    fn rejected(mut v: Value, source: i32, target: i32, why: &str) {
        let before = v.clone();
        let report = super::super::migrate(&mut v, source, target).unwrap();
        assert!(
            report.issues.iter().any(|e| e.contains(why)),
            "{:?}",
            report.issues
        );
        assert_eq!(v, before);
    }
    fn source_at(x: i32) -> Value {
        map([
            ("type", Value::String("minecraft:block".into())),
            ("pos", Value::IntArray(IntArray::new(vec![x, 64, 9]))),
        ])
    }
    #[test]
    fn campfire_preserves_progress_and_migrates_occupied_slot() {
        let times = Value::IntArray(IntArray::new(vec![0, 0, 86, 0]));
        let totals = Value::IntArray(IntArray::new(vec![0, 0, 600, 0]));
        let item = map([
            ("id", Value::String("minecraft:cod".into())),
            ("Slot", Value::Byte(2)),
            ("Count", Value::Byte(1)),
        ]);
        let mut value = region(
            "minecraft:campfire",
            &[
                ("Items", Value::List(vec![item])),
                ("CookingTimes", times.clone()),
                ("CookingTotalTimes", totals),
            ],
        );
        let original = value.clone();
        run(&mut value, 3465, 5023);
        assert_eq!(tile(&value)["CookingTimes"], times);
        let Value::List(stacks) = &tile(&value)["Items"] else {
            panic!()
        };
        assert_eq!(compound(&stacks[0], "").unwrap()["count"], Value::Int(1));
        run(&mut value, 5023, 3465);
        assert_eq!(value, original);
        rejected(
            region(
                "minecraft:campfire",
                &[("CookingTimes", Value::IntArray(IntArray::new(vec![1; 5])))],
            ),
            5023,
            3465,
            "四槽",
        );
        rejected(
            region("minecraft:campfire", &[("Items", Value::Int(12))]),
            5023,
            3465,
            "Items 不是",
        );
    }
    #[test]
    fn brewing_timer_widths_defaults_and_legacy_fuel_slot_are_enforced() {
        let mut value = region(
            "minecraft:brewing_stand",
            &[("BrewTime", Value::Short(160)), ("Fuel", Value::Byte(13))],
        );
        let original = value.clone();
        run(&mut value, 4903, 5023);
        let modern = tile(&value);
        assert_eq!(modern["BrewTime"], Value::Int(160));
        assert_eq!(modern["Fuel"], Value::Int(13));
        assert_eq!(modern["total_brew_time"], Value::Int(400));
        assert_eq!(modern["speed_multiplier"], Value::Float(1.0));
        run(&mut value, 5023, 4903);
        assert_eq!(value, original);
        rejected(
            region(
                "minecraft:brewing_stand",
                &[("speed_multiplier", Value::Float(1.5))],
            ),
            5023,
            4903,
            "自定义 speed_multiplier",
        );
        rejected(
            region(
                "minecraft:brewing_stand",
                &[("BrewTime", Value::Int(40000))],
            ),
            5023,
            4903,
            "超过旧版 Short",
        );
        rejected(
            region("minecraft:brewing_stand", &[("Fuel", Value::Byte(9))]),
            1343,
            0,
            "没有 Fuel",
        );
        let fuel = map([
            ("id", Value::String("minecraft:blaze_powder".into())),
            ("Slot", Value::Byte(4)),
            ("Count", Value::Byte(1)),
        ]);
        rejected(
            region(
                "minecraft:brewing_stand",
                &[("Items", Value::List(vec![fuel]))],
            ),
            1343,
            0,
            "没有燃料槽",
        );
    }
    fn vibration(id: &str) -> Value {
        map([
            ("game_event", Value::String(id.into())),
            ("distance", Value::Float(3.5)),
            (
                "pos",
                Value::List(vec![
                    Value::Double(0.5),
                    Value::Double(64.5),
                    Value::Double(9.5),
                ]),
            ),
            (
                "source",
                Value::IntArray(IntArray::new(vec![11, 22, 33, 44])),
            ),
        ])
    }
    #[test]
    fn sensor_and_shrieker_move_in_flight_events_across_all_listener_codecs() {
        for (id, field) in [
            ("minecraft:sculk_sensor", "last_vibration_frequency"),
            ("minecraft:sculk_shrieker", "warning_level"),
        ] {
            let listener = map([
                ("source", source_at(-3)),
                ("range", Value::Int(8)),
                ("event", vibration("minecraft:step")),
                ("event_delay", Value::Int(2)),
                ("event_distance", Value::Float(3.5)),
            ]);
            let mut value = region(id, &[(field, Value::Int(3)), ("listener", listener)]);
            let original = value.clone();
            run(&mut value, 3120, 3218);
            let middle = compound(&tile(&value)["listener"], "").unwrap();
            assert!(middle.contains_key("source") && !middle.contains_key("event_distance"));
            assert_eq!(middle["selector"], empty_selector());
            run(&mut value, 3218, 5023);
            let modern = compound(&tile(&value)["listener"], "").unwrap();
            assert!(!modern.contains_key("source") && !modern.contains_key("range"));
            assert_eq!(modern["event"], vibration("minecraft:step"));
            assert_eq!(modern["event_delay"], Value::Int(2));
            run(&mut value, 5023, 3120);
            assert_eq!(value, original);
        }
    }
    #[test]
    fn calibrated_sensor_preserves_selected_event_but_older_codecs_refuse_loss() {
        let selector = map([
            ("tick", Value::Long(17)),
            ("event", vibration("minecraft:resonate_11")),
        ]);
        let data = map([("selector", selector), ("event_delay", Value::Int(1))]);
        let mut value = region(
            "minecraft:calibrated_sculk_sensor",
            &[
                ("listener", data),
                ("last_vibration_frequency", Value::Int(11)),
            ],
        );
        let original = value.clone();
        run(&mut value, 3465, 5023);
        assert_eq!(value, original);
        let selected = map([
            ("tick", Value::Long(41)),
            ("event", vibration("minecraft:step")),
        ]);
        rejected(
            region(
                "minecraft:sculk_sensor",
                &[("listener", map([("selector", selected)]))],
            ),
            5023,
            3120,
            "待选择",
        );
        rejected(
            region(
                "minecraft:sculk_sensor",
                &[(
                    "listener",
                    map([
                        ("selector", empty_selector()),
                        ("event", vibration("minecraft:bounce")),
                    ]),
                )],
            ),
            5023,
            4786,
            "未在两端同时注册",
        );
    }
    #[test]
    fn pre_listener_sensor_accepts_idle_state_but_never_drops_custom_geometry_or_delay() {
        let idle = map([
            ("selector", empty_selector()),
            ("event_delay", Value::Int(0)),
        ]);
        let mut v = region(
            "minecraft:sculk_sensor",
            &[
                ("last_vibration_frequency", Value::Int(15)),
                ("listener", idle),
            ],
        );
        run(&mut v, 5023, 2730);
        assert_eq!(
            v,
            region(
                "minecraft:sculk_sensor",
                &[("last_vibration_frequency", Value::Int(15))]
            )
        );
        let displaced = map([
            ("range", Value::Int(8)),
            ("source", source_at(12)),
            ("selector", empty_selector()),
        ]);
        rejected(
            region("minecraft:sculk_sensor", &[("listener", displaced)]),
            3337,
            5023,
            "自定义监听位置",
        );
        rejected(
            region(
                "minecraft:sculk_sensor",
                &[(
                    "listener",
                    map([
                        ("selector", empty_selector()),
                        ("event_delay", Value::Int(3)),
                    ]),
                )],
            ),
            5023,
            2730,
            "正在传输",
        );
        rejected(
            region(
                "minecraft:sculk_sensor",
                &[(
                    "listener",
                    map([
                        ("selector", empty_selector()),
                        ("mod:unknown", Value::Int(7)),
                    ]),
                )],
            ),
            5023,
            4671,
            "mod:unknown",
        );
    }
    #[test]
    fn catalyst_nonempty_cursors_survive_and_lenient_vanilla_fallbacks_are_never_used() {
        let cursor = map([
            ("pos", Value::IntArray(IntArray::new(vec![-3, 65, 8]))),
            ("charge", Value::Int(567)),
            ("decay_delay", Value::Int(1)),
            ("update_delay", Value::Int(8)),
            (
                "facings",
                Value::List(vec![
                    Value::String("north".into()),
                    Value::String("down".into()),
                ]),
            ),
        ]);
        let mut value = region(
            "minecraft:sculk_catalyst",
            &[("cursors", Value::List(vec![cursor.clone()]))],
        );
        let original = value.clone();
        run(&mut value, 3105, 5023);
        run(&mut value, 5023, 3105);
        assert_eq!(value, original);
        let Value::Compound(mut malformed) = cursor else {
            panic!()
        };
        malformed.insert("charge".into(), Value::Int(1001));
        rejected(
            region(
                "minecraft:sculk_catalyst",
                &[("cursors", Value::List(vec![Value::Compound(malformed)]))],
            ),
            5023,
            3105,
            "charge",
        );
    }
}
