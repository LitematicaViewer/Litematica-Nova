//! Codec families shared by registered entity types. A recognized ID grants
//! access to the base codec, not permission to carry unexamined fields.
//! Version cuts refer to DataFixers and the first supported release with the
//! field in its save/load methods. No game runtime is needed by the converter.
use super::*;

pub(super) fn is_minecart(id: &str) -> bool {
    matches!(
        id,
        "minecraft:minecart"
            | "minecraft:chest_minecart"
            | "minecraft:hopper_minecart"
            | "minecraft:furnace_minecart"
            | "minecraft:tnt_minecart"
            | "minecraft:spawner_minecart"
            | "minecraft:command_block_minecart"
    )
}

pub(super) fn boat_kind(id: &str) -> Option<(bool, Option<&str>)> {
    let name = id.strip_prefix("minecraft:")?;
    if name == "boat" || name == "chest_boat" {
        return Some((name == "chest_boat", None));
    }
    for (suffix, chest) in [
        ("_chest_boat", true),
        ("_chest_raft", true),
        ("_boat", false),
        ("_raft", false),
    ] {
        if let Some(wood) = name.strip_suffix(suffix) {
            return Some((chest, Some(wood)));
        }
    }
    None
}

pub(super) fn migrate_boat(
    fields: &mut Fields,
    chest: bool,
    split_wood: Option<&str>,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    let wood = if source < 4067 {
        match fields.remove("Type") {
            Some(Value::String(wood)) => wood,
            None => "oak".to_owned(),
            _ => bail!("船Type不是木材名称"),
        }
    } else {
        if fields.contains_key("Type") {
            bail!("拆分后的船同时携带Type与独立实体ID")
        }
        split_wood.context("船实体缺少木材身份")?.to_owned()
    };
    if wood.is_empty() || !wood.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        bail!("船木材名称无效")
    }
    let family = format!(
        "{wood}_{}{}",
        if chest { "chest_" } else { "" },
        if wood == "bamboo" { "raft" } else { "boat" }
    );
    let output = if target >= 4067 {
        format!("minecraft:{family}")
    } else {
        let item = if !chest && target < 1519 && wood == "oak" {
            "minecraft:boat".to_owned()
        } else {
            format!("minecraft:{family}")
        };
        let reg = crate::block_database::database()?.registry_by_data_version(target)?;
        if !reg.items.contains(&item) && !reg.legacy_items.values().any(|id| id == &item) {
            bail!("目标版本没有{family}对应的木材/容器")
        }
        fields.insert("Type".into(), Value::String(wood));
        format!("minecraft:{}", if chest { "chest_boat" } else { "boat" })
    };
    if chest {
        migrate_vehicle_inventory(fields, source, target, 27, report)?;
    }
    fields.insert("id".into(), Value::String(output));
    Ok(())
}

fn thrown_item(id: &str) -> bool {
    matches!(
        id,
        "minecraft:snowball"
            | "minecraft:egg"
            | "minecraft:ender_pearl"
            | "minecraft:experience_bottle"
            | "minecraft:potion"
            | "minecraft:splash_potion"
            | "minecraft:lingering_potion"
    )
}

fn arrow(id: &str) -> bool {
    matches!(
        id,
        "minecraft:arrow" | "minecraft:spectral_arrow" | "minecraft:trident"
    )
}
fn display(id: &str) -> bool {
    matches!(
        id,
        "minecraft:item_display" | "minecraft:block_display" | "minecraft:text_display"
    )
}

pub(super) fn handles(id: &str, key: &str) -> bool {
    if matches!((id,key), ("minecraft:warden","anger"|"listener") | ("minecraft:allay","listener")) {
        return true;
    }
    if falling_block::handles(id, key) {
        return true;
    }
    if id == "minecraft:firework_rocket" {
        return matches!(
            key,
            "Owner"
                | "LeftOwner"
                | "HasBeenShot"
                | "FireworksItem"
                | "Life"
                | "LifeTime"
                | "ShotAtAngle"
        );
    }
    if thrown_item(id) || arrow(id) {
        if matches!(key, "Owner" | "LeftOwner" | "HasBeenShot") {
            return true;
        }
        if thrown_item(id) {
            return key == "Item";
        }
        return matches!(
            key,
            "life"
                | "shake"
                | "inGround"
                | "inBlockState"
                | "pickup"
                | "damage"
                | "crit"
                | "PierceLevel"
                | "item"
                | "weapon"
                | "SoundEvent"
                | "ShotFromCrossbow"
        ) || id == "minecraft:spectral_arrow" && key == "Duration"
            || id == "minecraft:trident" && key == "DealtDamage";
    }
    if display(id) {
        return matches!(
            key,
            "transformation"
                | "brightness"
                | "billboard"
                | "interpolation_duration"
                | "start_interpolation"
                | "teleport_duration"
                | "view_range"
                | "shadow_radius"
                | "shadow_strength"
                | "width"
                | "height"
                | "glow_color_override"
        ) || id == "minecraft:item_display" && matches!(key, "item" | "item_display")
            || id == "minecraft:block_display" && key == "block_state"
            || id == "minecraft:text_display"
                && matches!(
                    key,
                    "text"
                        | "line_width"
                        | "background"
                        | "text_opacity"
                        | "shadow"
                        | "see_through"
                        | "default_background"
                        | "alignment"
                );
    }
    if is_minecart(id)
        && matches!(
            key,
            "CustomDisplayTile"
                | "DisplayState"
                | "DisplayTile"
                | "DisplayData"
                | "DisplayOffset"
                | "FlippedRotation"
                | "HasTicked"
        )
    {
        return true;
    }
    match id {
        "minecraft:snow_golem" => key == "Pumpkin",
        "minecraft:iron_golem" => key == "PlayerCreated",
        "minecraft:command_block_minecart" => matches!(
            key,
            "Command"
                | "SuccessCount"
                | "TrackOutput"
                | "LastOutput"
                | "LastExecution"
                | "UpdateLastExecution"
        ),
        "minecraft:spawner_minecart" => matches!(
            key,
            "SpawnData"
                | "EntityId"
                | "SpawnPotentials"
                | "Delay"
                | "MinSpawnDelay"
                | "MaxSpawnDelay"
                | "SpawnCount"
                | "MaxNearbyEntities"
                | "RequiredPlayerRange"
                | "SpawnRange"
        ),
        "minecraft:furnace_minecart" => matches!(key, "PushX" | "PushZ" | "Fuel"),
        "minecraft:tnt_minecart" => matches!(key, "TNTFuse" | "fuse"),
        "minecraft:salmon" => matches!(key, "type" | "FromBucket"),
        "minecraft:cod" | "minecraft:tadpole" => key == "FromBucket",
        "minecraft:goat" => matches!(key, "IsScreamingGoat" | "HasLeftHorn" | "HasRightHorn"),
        _ => false,
    }
}

fn flag(value: &Value) -> bool {
    matches!(value, Value::Byte(0 | 1))
}
fn finite_float(value: &Value) -> bool {
    matches!(value, Value::Float(n) if n.is_finite())
}
fn floats(value: &Value, len: usize) -> bool {
    matches!(value, Value::List(v) if v.len() == len && v.iter().all(finite_float))
}

fn migrate_display(
    fields: &mut Fields,
    id: &str,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    // Display.{BlockDisplay,ItemDisplay,TextDisplay}: BlockState, ItemStack,
    // Component and Transformation are distinct codecs, not opaque compounds.
    if let Some(value) = fields.get_mut("item") {
        item_payload(value, source, target, report)?;
    }
    if let Some(value) = fields.get_mut("block_state") {
        migrate_stored_block_state(value, source, target)?;
    }
    if let Some(value) = fields.get_mut("text") {
        *value = migrate_component(value, source, target)?;
    }
    for (key, v) in fields.iter() {
        if !handles(id, key) {
            continue;
        }
        let ok = match key.as_str() {
            "text" | "item" | "block_state" => true,
            "view_range" | "shadow_radius" | "shadow_strength" | "width" | "height" => {
                finite_float(v)
            }
            "line_width"
            | "background"
            | "glow_color_override"
            | "interpolation_duration"
            | "start_interpolation" => matches!(v, Value::Int(_)),
            "teleport_duration" => matches!(v, Value::Int(0..=59)),
            "text_opacity" => matches!(v, Value::Byte(_)),
            "shadow" | "see_through" | "default_background" => flag(v),
            "alignment" => {
                matches!(v, Value::String(s) if matches!(s.as_str(), "left" | "right" | "center"))
            }
            "billboard" => {
                matches!(v, Value::String(s) if matches!(s.as_str(), "fixed" | "vertical" | "horizontal" | "center"))
            }
            "item_display" => {
                matches!(v, Value::String(s) if matches!(s.as_str(), "none" | "thirdperson_lefthand" | "thirdperson_righthand" | "firstperson_lefthand" | "firstperson_righthand" | "head" | "gui" | "ground" | "fixed"))
            }
            "brightness" => {
                matches!(v, Value::Compound(m) if m.len() == 2 && ["sky","block"].iter().all(|k|matches!(m.get(*k), Some(Value::Int(0..=15)))))
            }
            "transformation" => {
                floats(v, 16)
                    || matches!(v, Value::Compound(m) if m.len() == 4 && [("translation",3),("scale",3),("left_rotation",4),("right_rotation",4)].iter().all(|(k,n)|m.get(*k).is_some_and(|v|floats(v,*n))))
            }
            _ => false,
        };
        if !ok {
            bail!("{id}/{key}不符合展示实体的目标字段编码，原数据保留")
        }
    }
    if fields.contains_key("teleport_duration") && source < 3578 {
        bail!("来源发行版没有展示实体teleport_duration")
    }
    if target < 3578
        && fields
            .remove("teleport_duration")
            .is_some_and(|v| v != Value::Int(0))
    {
        bail!("目标1.20.2之前没有展示实体传送插值")
    }
    Ok(())
}

fn migrate_projectile(
    fields: &mut Fields,
    id: &str,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    if let Some(owner) = fields.get("Owner") {
        if source < 2514 || target < 2514 {
            bail!("投射物旧版所有者须使用该类特有的UUID编码，保留Owner")
        }
        uuid_text(owner)?;
    }
    for (k, since) in [("LeftOwner", 2566), ("HasBeenShot", 2724)] {
        if let Some(v) = fields.get(k) {
            if source < since || !flag(v) {
                bail!("投射物{k}与来源版本/类型不符")
            }
            if target < since {
                if *v != Value::Byte(0) {
                    bail!("目标版本没有投射物{k}状态")
                }
                fields.remove(k);
            }
        }
    }
    if id == "minecraft:firework_rocket" {
        if fields
            .get("Life")
            .is_some_and(|v| !matches!(v, Value::Int(_)))
            || fields
                .get("LifeTime")
                .is_some_and(|v| !matches!(v, Value::Int(_)))
        {
            bail!("烟花火箭寿命不是Int")
        }
        if fields.get("ShotAtAngle").is_some_and(|v| !flag(v)) {
            bail!("烟花发射角标记不是布尔Byte")
        }
        if target < 1952
            && fields
                .remove("ShotAtAngle")
                .is_some_and(|v| v != Value::Byte(0))
        {
            bail!("目标旧版没有弩发射角标记")
        }
        if let Some(item) = fields.get_mut("FireworksItem") {
            item_payload(item, source, target, report)?;
        }
        return Ok(());
    }
    if thrown_item(id) {
        // ThrownPotionSplitFix(v4306) selects the new entity from its stack.
        // Refuse a mismatched post-split id/stack on the way back.
        if matches!(
            id,
            "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion"
        ) && (source < 4306) != (target < 4306)
        {
            let stack = match fields.get("Item") {
                Some(Value::Compound(m)) => m.get("id"),
                None => None,
                _ => bail!("投掷药水Item不是Compound"),
            };
            let kind = match stack {
                Some(Value::String(s)) if s == "minecraft:lingering_potion" => {
                    "minecraft:lingering_potion"
                }
                None => "minecraft:splash_potion",
                Some(Value::String(s)) if s == "minecraft:splash_potion" => {
                    "minecraft:splash_potion"
                }
                _ => bail!("投掷药水物品不能对应真实药水类型"),
            };
            if target < 4306 && kind != id {
                bail!("药水实体身份与将决定旧版外观的Item不符")
            }
            fields.insert(
                "id".into(),
                Value::String(
                    if target < 4306 {
                        "minecraft:potion"
                    } else {
                        kind
                    }
                    .into(),
                ),
            );
        }
        if let Some(item) = fields.get_mut("Item") {
            if source < 1952 || target < 1952 {
                bail!("投射物旧版本没有已验证的Item保存格式")
            }
            item_payload(item, source, target, report)?;
        }
    } else {
        for (key, value) in fields.iter() {
            let ok = match key.as_str() {
                "shake" | "PierceLevel" => matches!(value, Value::Byte(_)),
                "pickup" => matches!(value, Value::Byte(0..=2)),
                "damage" => matches!(value, Value::Double(n) if n.is_finite()),
                "crit" | "inGround" | "DealtDamage" => flag(value),
                "life" => matches!(value, Value::Short(_)),
                "Duration" => matches!(value, Value::Int(_)),
                _ => true,
            };
            if !ok {
                bail!("箭矢{key}字段类型无效")
            }
        }
        if let Some(state) = fields.get_mut("inBlockState") {
            if source < 1519 || target < 1519 {
                bail!("箭矢旧数值方块状态尚未覆盖")
            }
            migrate_stored_block_state(state, source, target)?;
        }
        if let Some(v) = fields.get("SoundEvent") {
            // The ordinary impact sound is stable; custom registered sounds
            // need exact sound registry evidence, not a syntactically valid ID.
            if source < 1952 || v != &Value::String("minecraft:entity.arrow.hit".into()) {
                bail!("箭矢自定义SoundEvent没有可验证的双端音效注册表")
            }
            if target < 1952 {
                fields.remove("SoundEvent");
            }
        }
        if let Some(v) = fields.get("ShotFromCrossbow") {
            if !(1952..3938).contains(&source) || !flag(v) {
                bail!("ShotFromCrossbow来源格式不符")
            }
            if !(1952..3938).contains(&target) {
                if *v != Value::Byte(0) {
                    bail!("目标箭矢不能仅凭ShotFromCrossbow恢复实际发射武器")
                }
                fields.remove("ShotFromCrossbow");
            }
        }
        if target < 1952
            && fields
                .remove("PierceLevel")
                .is_some_and(|v| v != Value::Byte(0))
        {
            bail!("目标旧箭矢不能保存穿透等级")
        }
        for (key, since) in [("item", 3698), ("weapon", 3938)] {
            if let Some(stack) = fields.get_mut(key) {
                if source < since || target < since {
                    bail!("旧版本没有等价箭矢{key}物品载体")
                }
                item_payload(stack, source, target, report)?;
            }
        }
    }
    Ok(())
}

// Validate the incoming layout before a rename can overwrite an incompatible
// carrier. 26.3 DropChances uses NON_NEGATIVE_FLOAT; 2.0 is valid and common.
fn check_equipment(fields: &Fields, source: i32) -> Result<()> {
    let probability = |v: &Value| matches!(v,Value::Float(n) if n.is_finite() && *n>=0.0);
    let slot = |s: &str| {
        matches!(
            s,
            "mainhand" | "offhand" | "feet" | "legs" | "chest" | "head" | "body" | "saddle"
        )
    };
    for (key, min, max, slots, chance) in [
        ("Equipment", 0, 100, 5, false),
        ("DropChances", 0, 100, 5, true),
        ("HandItems", 100, 4301, 2, false),
        ("ArmorItems", 100, 4301, 4, false),
        ("HandDropChances", 100, 4293, 2, true),
        ("ArmorDropChances", 100, 4293, 4, true),
    ] {
        if let Some(v) = fields.get(key) {
            if !(min..max).contains(&source)
                || !matches!(v,Value::List(a) if a.len()<=slots && a.iter().all(|e| if chance { probability(e) } else { matches!(e,Value::Compound(_)) }))
            {
                bail!("实体{key}与来源装备槽位/字段类型不符");
            }
        }
    }
    for (key, since, chance) in [("equipment", 4301, false), ("drop_chances", 4293, true)] {
        if let Some(v) = fields.get(key) {
            if source < since
                || !matches!(v,Value::Compound(m) if m.iter().all(|(k,v)|slot(k) && if chance {probability(v)} else { matches!(v,Value::Compound(_)) }))
            {
                bail!("实体{key}字段与来源版本/装备槽位不符")
            }
        }
    }
    Ok(())
}

pub(super) fn migrate(
    fields: &mut Fields,
    id: &str,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    falling_block::migrate(fields, id, source, target, report)?;
    block_entity_families::migrate_vibration_mob(fields, id, source, target)?;
    check_equipment(fields, source)?;
    if matches!(
        id,
        "minecraft:player"
            | "minecraft:leash_knot"
            | "minecraft:fishing_bobber"
            | "minecraft:lightning_bolt"
    ) {
        bail!("{id}不是可保存的普通投影实体，保留原数据");
    }
    // Non-living entities do not consume living-entity equipment/attribute
    // carriers. A coincidentally named custom tag must not look converted.
    if !type_registry::is_living(id)
        && [
            "Attributes",
            "attributes",
            "ArmorItems",
            "HandItems",
            "Equipment",
            "equipment",
            "drop_chances",
            "ArmorDropChances",
            "HandDropChances",
            "Brain",
            "active_effects",
            "ActiveEffects",
        ]
        .iter()
        .any(|k| fields.contains_key(*k))
    {
        bail!("非生物实体{id}带有并非该类型读取的生物字段");
    }
    for key in [
        "OnGround",
        "Invulnerable",
        "CustomNameVisible",
        "Silent",
        "NoGravity",
        "Glowing",
        "CanPickUpLoot",
        "PersistenceRequired",
        "LeftHanded",
        "FallFlying",
        "HasVisualFire",
    ] {
        if fields.get(key).is_some_and(|v| !flag(v)) {
            bail!("实体{id}/{key}不是布尔Byte")
        }
    }
    for (key, n, float) in [
        ("Pos", 3, false),
        ("Motion", 3, false),
        ("Rotation", 2, true),
    ] {
        if let Some(v) = fields.get(key) {
            if !matches!(v, Value::List(v) if v.len()==n && v.iter().all(|v| if float { finite_float(v) } else { matches!(v,Value::Double(x) if x.is_finite()) }))
            {
                bail!("实体{id}/{key}位置或方向编码无效")
            }
        }
    }
    if fields.get("Tags").is_some_and(
        |v| !matches!(v, Value::List(tags) if tags.iter().all(|v| matches!(v,Value::String(_)))),
    ) {
        bail!("实体Tags不是字符串列表")
    }
    if fields
        .get("TicksFrozen")
        .is_some_and(|v| !matches!(v, Value::Int(0..)))
    {
        bail!("TicksFrozen不是非负Int")
    }
    for (k, empty) in [
        ("HasVisualFire", Value::Byte(0)),
        ("TicksFrozen", Value::Int(0)),
    ] {
        if fields.contains_key(k) && source < 2724 {
            bail!("来源1.17以前没有{k}")
        }
        if target < 2724 && fields.remove(k).is_some_and(|v| v != empty) {
            bail!("目标1.17以前没有{k}状态")
        }
    }
    if target >= 4903
        && fields
            .remove("HurtByTimestamp")
            .is_some_and(|v| v != Value::Int(0))
    {
        bail!("26.2起没有旧HurtByTimestamp对应的世界时间基准")
    }
    crate::version_brain_memory::migrate(fields, id, source, target)?;
    if display(id) {
        migrate_display(fields, id, source, target, report)?;
    }
    if arrow(id) || thrown_item(id) || id == "minecraft:firework_rocket" {
        migrate_projectile(fields, id, source, target, report)?;
    }
    match id {
        "minecraft:snow_golem" => {
            // SnowGolem (3465 client bsj and 5023): absent defaults to a pumpkin.
            if let Some(v) = fields.get("Pumpkin") {
                if !flag(v) || source < 819 {
                    bail!("雪傀儡Pumpkin与来源保存编码不符")
                }
                if target < 819 {
                    if *v == Value::Byte(0) {
                        bail!("目标1.11以前不保存剪去南瓜的状态")
                    }
                    fields.remove("Pumpkin");
                }
            }
        }
        "minecraft:iron_golem" if fields.get("PlayerCreated").is_some_and(|v| !flag(v)) => {
            bail!("铁傀儡PlayerCreated不是布尔Byte")
        }
        "minecraft:command_block_minecart" => {
            if fields
                .get("Command")
                .is_some_and(|v| !matches!(v, Value::String(_)))
                || fields
                    .get("SuccessCount")
                    .is_some_and(|v| !matches!(v, Value::Int(_)))
                || fields
                    .get("LastExecution")
                    .is_some_and(|v| !matches!(v, Value::Long(_)))
                || ["TrackOutput", "UpdateLastExecution"]
                    .iter()
                    .any(|k| fields.get(*k).is_some_and(|v| !flag(v)))
            {
                bail!("命令矿车持久字段类型无效")
            }
            if target < 1139
                && (fields.contains_key("LastExecution")
                    || fields.contains_key("UpdateLastExecution"))
            {
                bail!("目标旧版没有命令执行时间状态")
            }
            if let Some(output) = fields.get_mut("LastOutput") {
                *output = migrate_component(output, source, target)?;
            }
        }
        "minecraft:spawner_minecart" => migrate_spawner(fields, source, target, report)?,
        "minecraft:item" => {
            for key in ["Age", "Health", "PickupDelay"] {
                if fields
                    .get(key)
                    .is_some_and(|v| !matches!(v, Value::Short(_)))
                {
                    bail!("掉落物{key}不是Short")
                }
            }
            for key in ["Owner", "Thrower"] {
                if let Some(v) = fields.get_mut(key) {
                    if source < 2514 {
                        let Value::Compound(old) = v else {
                            bail!("掉落物旧{key}不是M/L复合，原身份保留")
                        };
                        let (Some(Value::Long(m)), Some(Value::Long(l))) =
                            (old.get("M"), old.get("L"))
                        else {
                            bail!("掉落物{key}缺少旧UUID")
                        };
                        if old.len() != 2 {
                            bail!("掉落物{key}含有额外字段")
                        }
                        if target >= 2514 {
                            *v = Value::IntArray(fastnbt::IntArray::new(vec![
                                (m >> 32) as i32,
                                *m as i32,
                                (l >> 32) as i32,
                                *l as i32,
                            ]));
                        }
                    } else {
                        uuid_text(v)?;
                        if target < 2514 {
                            let Value::IntArray(a) = v else {
                                unreachable!()
                            };
                            *v = Value::Compound(
                                [
                                    (
                                        "M".into(),
                                        Value::Long(((a[0] as i64) << 32) | i64::from(a[1] as u32)),
                                    ),
                                    (
                                        "L".into(),
                                        Value::Long(((a[2] as i64) << 32) | i64::from(a[3] as u32)),
                                    ),
                                ]
                                .into(),
                            );
                        }
                    }
                }
            }
        }
        "minecraft:furnace_minecart" => {
            if ["PushX", "PushZ"].iter().any(|k| {
                fields
                    .get(*k)
                    .is_some_and(|v| !matches!(v,Value::Double(n) if n.is_finite()))
            }) || fields
                .get("Fuel")
                .is_some_and(|v| !matches!(v, Value::Short(_)))
            {
                bail!("动力矿车推动向量或燃料字段无效")
            }
        }
        "minecraft:tnt_minecart" => {
            let (from, to) = (
                if source < 4173 { "TNTFuse" } else { "fuse" },
                if target < 4173 { "TNTFuse" } else { "fuse" },
            );
            if fields.contains_key(if source < 4173 { "fuse" } else { "TNTFuse" }) {
                bail!("TNT矿车携带错误版本的引信字段")
            }
            if let Some(v) = fields.remove(from) {
                if !matches!(v, Value::Int(_)) {
                    bail!("TNT矿车引信不是Int")
                }
                fields.insert(to.into(), v);
            }
        }
        "minecraft:goat" => {
            for k in ["IsScreamingGoat", "HasLeftHorn", "HasRightHorn"] {
                if fields.get(k).is_some_and(|v| !flag(v)) {
                    bail!("山羊{k}不是布尔Byte")
                }
            }
            for k in ["HasLeftHorn", "HasRightHorn"] {
                if source < 3093 && fields.contains_key(k) {
                    bail!("旧山羊不保存{k}")
                }
                if source < 3093 && target >= 3093 {
                    fields.insert(k.into(), Value::Byte(1));
                }
                if target < 3093 && fields.remove(k).is_some_and(|v| v != Value::Byte(1)) {
                    bail!("目标旧山羊不能保存缺角状态")
                }
            }
        }
        "minecraft:salmon" => {
            // 1.21.2 (4080) already serializes all three names. v4081
            // replaces non-large values with medium: do not silently resize
            // an explicitly small 4080 fish when applying that fix.
            if source < 4080 && fields.contains_key("type") {
                bail!("来源旧鲑鱼没有可保存的体型字段")
            }
            if source == 4080
                && target >= 4081
                && fields.get("type") == Some(&Value::String("small".into()))
            {
                bail!("v4081会将旧小型鲑鱼改为中型，保留原数据")
            }
            if source < 4081 && target >= 4080 && !fields.contains_key("type") {
                fields.insert("type".into(), Value::String("medium".into()));
            }
            if let Some(v) = fields.get("type") {
                if !matches!(v,Value::String(s) if matches!(s.as_str(),"small"|"medium"|"large")) {
                    bail!("鲑鱼体型无有效映射")
                }
                if target < 4080 {
                    if v != &Value::String("medium".into()) {
                        bail!("目标旧鲑鱼不能等价保存此次指定的体型")
                    }
                    fields.remove("type");
                }
            }
        }
        _ => {}
    }
    if fields.get("FromBucket").is_some_and(|v| !flag(v)) {
        bail!("FromBucket不是布尔Byte")
    }
    Ok(())
}

pub(super) fn migrate_jukebox(
    fields: &mut Fields,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    if source < 3117
        && ["TickCount", "RecordStartTick", "IsPlaying"]
            .iter()
            .any(|k| fields.contains_key(*k))
    {
        bail!("来源旧唱片机没有播放时钟编码")
    }
    if source < 3945 && target < 3117 && fields.contains_key("IsPlaying") {
        if fields
            .remove("IsPlaying")
            .is_some_and(|v| v != Value::Byte(0))
        {
            bail!("目标旧唱片机没有对应播放进度")
        }
        fields.remove("TickCount");
        fields.remove("RecordStartTick");
    }
    if fields.contains_key("Record") {
        bail!("旧唱片机数值Record尚无已验证的物品映射")
    }
    if let Some(record) = fields.get_mut("RecordItem") {
        item_payload(record, source, target, report)?;
    }
    for k in ["TickCount", "RecordStartTick", "ticks_since_song_started"] {
        if fields
            .get(k)
            .is_some_and(|v| !matches!(v, Value::Long(0..)))
        {
            bail!("唱片机{k}不是非负Long")
        }
    }
    if fields.get("IsPlaying").is_some_and(|v| !flag(v)) {
        bail!("唱片机IsPlaying不是布尔Byte")
    }
    if source < 3945 && fields.contains_key("ticks_since_song_started")
        || source >= 3945
            && ["TickCount", "RecordStartTick", "IsPlaying"]
                .iter()
                .any(|k| fields.contains_key(*k))
    {
        bail!("唱片机混合了两个版本的播放状态")
    }
    if source < 3945 && target >= 3945 {
        // Only a playing track has an elapsed time in the target; retaining the
        // stale counters of a stopped old jukebox would restart its disc.
        let elapsed = |v| match v {
            Some(Value::Long(n)) => n,
            _ => 0,
        };
        let tick = elapsed(fields.remove("TickCount"));
        let start = elapsed(fields.remove("RecordStartTick"));
        if fields.remove("IsPlaying") == Some(Value::Byte(1)) {
            if !fields.contains_key("RecordItem") || tick < start {
                bail!("唱片机播放时钟或唱片缺失")
            }
            fields.insert("ticks_since_song_started".into(), Value::Long(tick - start));
        }
    } else if source >= 3945 && target < 3945 {
        if let Some(elapsed) = fields.remove("ticks_since_song_started") {
            if target < 3117 || !fields.contains_key("RecordItem") {
                bail!("目标唱片机没有对应播放进度")
            }
            fields.insert("IsPlaying".into(), Value::Byte(1));
            fields.insert("TickCount".into(), elapsed);
            fields.insert("RecordStartTick".into(), Value::Long(0));
        }
    }
    Ok(())
}
