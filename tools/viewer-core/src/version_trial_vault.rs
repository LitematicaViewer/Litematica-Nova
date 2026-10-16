//! Trial/vault payloads: actual V3807, TrialSpawnerConfigFix(3825),
//! TrialSpawnerConfigInRegistryFix(4061), and their vanilla runtime codecs.
//! Registry expansion is restricted to the fourteen verified vanilla pairs.
use super::*;

const CONFIG_KEYS: &[&str] = &[
    "spawn_range", "total_mobs", "simultaneous_mobs", "total_mobs_added_per_player",
    "simultaneous_mobs_added_per_player", "ticks_between_spawn", "spawn_potentials",
    "loot_tables_to_eject", "items_to_drop_when_ominous",
];
const COMMON: &[&str] = &["id", "x", "y", "z", "CustomName", "components"];

fn only(fields: &Fields, allowed: &[&str], path: &str) -> Result<()> {
    if let Some(key) = fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        bail!("{path}.{key}：未知字段缺少等价迁移规则，原文件保留")
    }
    Ok(())
}

fn integer(value: &Value, path: &str) -> Result<i64> {
    Ok(match value {
        Value::Byte(v) => i64::from(*v), Value::Short(v) => i64::from(*v),
        Value::Int(v) => i64::from(*v), Value::Long(v) => *v,
        _ => bail!("{path}：应为整数"),
    })
}

fn number(value: &Value, path: &str) -> Result<f64> {
    let n = match value {
        Value::Float(v) => f64::from(*v), Value::Double(v) => *v,
        _ => integer(value, path)? as f64,
    };
    if !n.is_finite() { bail!("{path}：不是有限数值") }
    Ok(n)
}

fn int_field(fields: &Fields, name: &str, min: i64, max: i64, path: &str) -> Result<()> {
    if let Some(value) = fields.get(name) {
        let n = integer(value, &format!("{path}.{name}"))?;
        if !(min..=max).contains(&n) { bail!("{path}.{name}：超出原版codec范围 {min}..{max}") }
    }
    Ok(())
}

fn resource(value: &Value, path: &str) -> Result<()> {
    if !matches!(value, Value::String(id) if valid_resource_identifier(id)) {
        bail!("{path}：不是有效原版资源标识")
    }
    Ok(())
}

fn uuids(value: &Value, path: &str) -> Result<()> {
    let Value::List(values) = value else { bail!("{path}：UUID集合不是List") };
    for (index, value) in values.iter().enumerate() {
        uuid_text(value).map_err(|error| anyhow::anyhow!("{path}[{index}]：{error}"))?;
    }
    Ok(())
}

fn nested_item(value: &mut Value, source: i32, target: i32, path: &str, report: &mut PayloadMigrationReport) -> Result<()> {
    let mut local = PayloadMigrationReport::default();
    item_payload(value, source, target, &mut local).map_err(|e| anyhow::anyhow!("{path}：{e}"))?;
    if !local.issues.is_empty() { bail!("{path}：{}", local.issues.join("；")) }
    report.changes.extend(local.changes);
    Ok(())
}

pub(super) fn migrate(
    fields: &mut Fields, id: &str, source: i32, target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    if !crate::block_database::database()?.registry_by_data_version(target)?.blocks.contains_key(id) {
        bail!("{id}：目标DataVersion={target}的真实注册表没有该方块；其配置、库存和运行状态不能等价保存，原文件保留")
    }
    let mut candidate = fields.clone();
    let mut local = PayloadMigrationReport::default();
    let specific: &[&str] = if id == "minecraft:vault" {
        &["config", "shared_data", "server_data"]
    } else {
        &["normal_config", "ominous_config", "target_cooldown_length", "required_player_range",
          "registered_players", "current_mobs", "cooldown_ends_at", "next_mob_spawns_at",
          "total_mobs_spawned", "spawn_data", "ejecting_loot_table"]
    };
    for key in candidate.keys() {
        if !COMMON.contains(&key.as_str()) && !specific.contains(&key.as_str())
            && !(id == "minecraft:trial_spawner" && CONFIG_KEYS.contains(&key.as_str())) {
            bail!("{id}.{key}：未知字段缺少迁移规则，原文件保留")
        }
    }
    for axis in ["x", "y", "z"] {
        if !matches!(candidate.get(axis), Some(Value::Int(_))) { bail!("{id}.{axis}：方块实体坐标不是Int") }
    }
    if let Some(name) = candidate.get_mut("CustomName") { *name = migrate_component(name, source, target)?; }
    if let Some(components) = candidate.get_mut("components") { migrate_components(components, source, target, &mut local)?; }
    match id {
        "minecraft:vault" => vault(&mut candidate, source, target, &mut local)?,
        "minecraft:trial_spawner" => trial(&mut candidate, source, target, &mut local)?,
        _ => bail!("试炼/宝库迁移入口收到错误方块类型"),
    }
    if !local.issues.is_empty() { bail!("{id}嵌套载荷：{}", local.issues.join("；")) }
    *fields = candidate;
    report.changes.extend(local.changes);
    report.changes.push(format!("{id}：按实际试炼/宝库配置schema迁移，保留库存与状态"));
    Ok(())
}

fn vault(fields: &mut Fields, source: i32, target: i32, report: &mut PayloadMigrationReport) -> Result<()> {
    if let Some(value) = fields.get_mut("config") {
        let map = object(value)?;
        only(map, &["loot_table", "activation_range", "deactivation_range", "key_item", "override_loot_table_to_display"], "vault.config")?;
        for key in ["loot_table", "override_loot_table_to_display"] {
            if let Some(value) = map.get(key) { resource(value, &format!("vault.config.{key}"))?; }
        }
        let activation = map.get("activation_range").map(|v| number(v, "vault.config.activation_range")).transpose()?.unwrap_or(4.0);
        let deactivation = map.get("deactivation_range").map(|v| number(v, "vault.config.deactivation_range")).transpose()?.unwrap_or(4.5);
        if activation > deactivation { bail!("vault.config：激活范围大于离开范围，不符合原版codec") }
        if let Some(item) = map.get_mut("key_item") {
            nested_item(item, source, target, "vault.config.key_item", report)?;
        }
    }
    if let Some(value) = fields.get_mut("shared_data") {
        let map = object(value)?;
        only(map, &["display_item", "connected_players", "connected_particles_range"], "vault.shared_data")?;
        if let Some(value) = map.get("connected_players") { uuids(value, "vault.shared_data.connected_players")?; }
        if let Some(value) = map.get("connected_particles_range") { number(value, "vault.shared_data.connected_particles_range")?; }
        if let Some(item) = map.get_mut("display_item") {
            nested_item(item, source, target, "vault.shared_data.display_item", report)?;
        }
    }
    if let Some(value) = fields.get_mut("server_data") {
        let map = object(value)?;
        only(map, &["rewarded_players", "state_updating_resumes_at", "items_to_eject", "total_ejections_needed"], "vault.server_data")?;
        if let Some(value) = map.get("rewarded_players") { uuids(value, "vault.server_data.rewarded_players")?; }
        int_field(map, "state_updating_resumes_at", i64::MIN, i64::MAX, "vault.server_data")?;
        int_field(map, "total_ejections_needed", i64::from(i32::MIN), i64::from(i32::MAX), "vault.server_data")?;
        if let Some(value) = map.get_mut("items_to_eject") {
            let Value::List(items) = value else { bail!("vault.server_data.items_to_eject：不是ItemStack列表") };
            for (index, item) in items.iter_mut().enumerate() {
                nested_item(item, source, target, &format!("vault.server_data.items_to_eject[{index}]"), report)?;
            }
        }
    }
    Ok(())
}

fn spawn_payload(config: &mut Fields, source: i32, target: i32, path: &str, report: &mut PayloadMigrationReport) -> Result<()> {
    let mut bridge = Fields::new();
    if let Some(value) = config.remove("spawn_data") { bridge.insert("SpawnData".into(), value); }
    if let Some(value) = config.remove("spawn_potentials") { bridge.insert("SpawnPotentials".into(), value); }
    let mut local = PayloadMigrationReport::default();
    migrate_spawner(&mut bridge, source, target, &mut local).map_err(|e| anyhow::anyhow!("{path}：{e}"))?;
    if !local.issues.is_empty() { bail!("{path}：{}", local.issues.join("；")) }
    report.changes.extend(local.changes);
    if let Some(value) = bridge.remove("SpawnData") { config.insert("spawn_data".into(), value); }
    if let Some(value) = bridge.remove("SpawnPotentials") { config.insert("spawn_potentials".into(), value); }
    Ok(())
}

fn inline_config(fields: &mut Fields, source: i32, target: i32, report: &mut PayloadMigrationReport) -> Result<()> {
    only(fields, CONFIG_KEYS, "trial_spawner.config")?;
    int_field(fields, "spawn_range", 1, 128, "trial_spawner.config")?;
    int_field(fields, "ticks_between_spawn", 0, i64::from(i32::MAX), "trial_spawner.config")?;
    for key in ["total_mobs", "simultaneous_mobs", "total_mobs_added_per_player", "simultaneous_mobs_added_per_player"] {
        if let Some(value) = fields.get(key) {
            let n = number(value, key)?;
            if n < 0.0 || n > f64::from(f32::MAX) { bail!("trial_spawner.config.{key}：超出原版非负Float范围") }
        }
    }
    if let Some(value) = fields.get("items_to_drop_when_ominous") { resource(value, "trial_spawner.config.items_to_drop_when_ominous")?; }
    if let Some(value) = fields.get("loot_tables_to_eject") {
        let Value::List(entries) = value else { bail!("trial_spawner.config.loot_tables_to_eject：不是加权列表") };
        for entry in entries {
            let Value::Compound(entry) = entry else { bail!("试炼掉落权重条目不是Compound") };
            only(entry, &["data", "weight"], "trial_spawner.config.loot_tables_to_eject")?;
            resource(entry.get("data").context("试炼掉落条目缺少data")?, "trial_spawner.config.loot_tables_to_eject.data")?;
            int_field(entry, "weight", 1, i64::from(i32::MAX), "trial_spawner.config.loot_tables_to_eject")?;
        }
    }
    if fields.get("spawn_potentials").is_some_and(|value| !matches!(value, Value::List(_))) {
        bail!("trial_spawner.config.spawn_potentials：不是列表")
    }
    spawn_payload(fields, source, target, "trial_spawner.config.spawn_potentials", report)
}

fn full_config(overrides: &Fields) -> Fields {
    let mut defaults = Fields::from([
        ("spawn_range".into(), Value::Int(4)), ("total_mobs".into(), Value::Float(6.0)),
        ("simultaneous_mobs".into(), Value::Float(2.0)), ("total_mobs_added_per_player".into(), Value::Float(2.0)),
        ("simultaneous_mobs_added_per_player".into(), Value::Float(1.0)), ("ticks_between_spawn".into(), Value::Int(40)),
        ("spawn_potentials".into(), Value::List(Vec::new())),
        ("loot_tables_to_eject".into(), Value::List(["consumables", "key"].into_iter().map(|kind|
            Value::Compound(Fields::from([("data".into(), Value::String(format!("minecraft:spawners/trial_chamber/{kind}"))), ("weight".into(), Value::Int(1))]))).collect())),
        ("items_to_drop_when_ominous".into(), Value::String("minecraft:spawners/trial_chamber/items_to_drop_when_ominous".into())),
    ]);
    defaults.extend(overrides.clone());
    defaults
}

fn merge(normal: &Fields, delta: &Fields) -> Fields {
    let mut result = normal.clone();
    for (key, value) in delta {
        let value = match (result.get(key), value) {
            (Some(Value::Compound(before)), Value::Compound(after)) => Value::Compound(merge(before, after)),
            (_, value) => value.clone(),
        };
        result.insert(key.clone(), value);
    }
    result
}

fn omit_numeric_defaults(fields: &Fields) -> Fields {
    let mut result = fields.clone();
    for (key, default) in full_config(&Fields::new()) {
        if ["spawn_range", "total_mobs", "simultaneous_mobs", "total_mobs_added_per_player", "simultaneous_mobs_added_per_player", "ticks_between_spawn"].contains(&key.as_str())
            && result.get(&key) == Some(&default) { result.remove(&key); }
    }
    result
}

fn vanilla_pair(normal: &Fields, ominous: &Fields) -> Option<&'static str> {
    vanilla_entries().iter().find(|entry| {
        let merged = merge(&entry.normal, &entry.ominous_delta);
        (normal == &entry.normal || normal == &full_config(&entry.normal))
            && (ominous == &entry.ominous_delta || ominous == &merged
                || ominous == &omit_numeric_defaults(&merged) || ominous == &full_config(&merged))
    }).map(|entry| entry.id)
}

fn resolve_config(value: &mut Value, target: i32, path: &str) -> Result<bool> {
    let mut expanded = false;
    if let Value::String(id) = value {
        let (entry, ominous) = vanilla_entries().iter().find_map(|entry| {
            if id == &format!("{}/normal", entry.id) { Some((entry, false)) }
            else if id == &format!("{}/ominous", entry.id) { Some((entry, true)) } else { None }
        }).with_context(|| format!("{path}：注册表ID {id} 没有已核实原版配置；缺少自定义数据包定义，原文件保留"))?;
        if target < 4061 {
            *value = Value::Compound(full_config(&if ominous { merge(&entry.normal, &entry.ominous_delta) } else { entry.normal.clone() }));
            expanded = true;
        }
    } else if !matches!(value, Value::Compound(_)) { bail!("{path}：配置既不是Compound也不是原版注册表ID") }
    Ok(expanded)
}

fn trial(fields: &mut Fields, source: i32, target: i32, report: &mut PayloadMigrationReport) -> Result<()> {
    let has_flat = CONFIG_KEYS.iter().any(|key| fields.contains_key(*key));
    if (source < 3825 && (fields.contains_key("normal_config") || fields.contains_key("ominous_config")))
        || (source >= 3825 && has_flat) { bail!("trial_spawner：来源同时携带不属于其版本的平铺/嵌套配置，拒绝覆盖") }
    let mut normal = if source < 3825 {
        let moved: Fields = CONFIG_KEYS.iter().filter_map(|key| fields.remove(*key).map(|value| ((*key).into(), value))).collect();
        (!moved.is_empty()).then_some(Value::Compound(moved))
    } else { fields.remove("normal_config") };
    let mut ominous = fields.remove("ominous_config");
    if source < 4061 && target >= 4061 {
        if let (Some(Value::Compound(n)), Some(Value::Compound(o))) = (&normal, &ominous) {
            if let Some(id) = vanilla_pair(n, o) {
                normal = Some(Value::String(format!("{id}/normal")));
                ominous = Some(Value::String(format!("{id}/ominous")));
            }
        }
    }
    for (name, value) in [("normal_config", &mut normal), ("ominous_config", &mut ominous)] {
        if let Some(value) = value {
            let expanded = resolve_config(value, target, &format!("trial_spawner.{name}"))?;
            if let Value::Compound(config) = value {
                // Registry values are reconstructed from the actual v4061
                // definition, not serialized using the caller's newer entity codec.
                inline_config(config, if expanded { 4061 } else { source }, target, report)
                    .map_err(|e| anyhow::anyhow!("trial_spawner.{name}：{e}"))?;
            }
        }
    }
    if target < 3825 {
        if let Some(Value::Compound(config)) = &ominous {
            if full_config(config) != full_config(&Fields::new()) { bail!("trial_spawner.ominous_config：目标v3825以前没有独立不祥配置，不能等价保存，原文件保留") }
        }
        if let Some(Value::Compound(config)) = normal { fields.extend(config); }
    } else {
        if let Some(value) = normal { fields.insert("normal_config".into(), value); }
        if let Some(value) = ominous { fields.insert("ominous_config".into(), value); }
    }
    for key in ["registered_players", "current_mobs"] {
        if let Some(value) = fields.get(key) { uuids(value, &format!("trial_spawner.{key}"))?; }
    }
    int_field(fields, "cooldown_ends_at", i64::MIN, i64::MAX, "trial_spawner")?;
    int_field(fields, "next_mob_spawns_at", i64::MIN, i64::MAX, "trial_spawner")?;
    int_field(fields, "total_mobs_spawned", 0, i64::from(i32::MAX), "trial_spawner")?;
    int_field(fields, "target_cooldown_length", 0, i64::from(i32::MAX), "trial_spawner")?;
    int_field(fields, "required_player_range", 1, 128, "trial_spawner")?;
    if let Some(value) = fields.get("ejecting_loot_table") { resource(value, "trial_spawner.ejecting_loot_table")?; }
    if fields.get("spawn_data").is_some_and(|value| !matches!(value, Value::Compound(_))) { bail!("trial_spawner.spawn_data不是Compound") }
    // The flat pre-3825 spawn_potentials was already migrated as config above.
    // Only the independent runtime spawn_data passes through this state bridge.
    let mut state = Fields::new();
    if let Some(value) = fields.remove("spawn_data") { state.insert("spawn_data".into(), value); }
    spawn_payload(&mut state, source, target, "trial_spawner.spawn_data", report)?;
    fields.extend(state);
    Ok(())
}

struct VanillaEntry { id: &'static str, normal: Fields, ominous_delta: Fields }

// GENERATED_VANILLA_ENTRIES: regenerate only from the actual upstream 4061 fix.
// Source SHA256: 1d30f449ddc44e87f56478b60afa1d08ce69c5c4f095a82f539109f2d60892b4
fn vanilla_entries() -> &'static [VanillaEntry] {
    static ENTRIES: std::sync::OnceLock<Vec<VanillaEntry>> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(|| vec![
        VanillaEntry { id: "minecraft:trial_chamber/breeze", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(1.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:breeze".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20)),("total_mobs".into(),Value::Float(2.0)),("total_mobs_added_per_player".into(),Value::Float(1.0))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("simultaneous_mobs".into(),Value::Float(2.0)),("total_mobs".into(),Value::Float(4.0))]) },
        VanillaEntry { id: "minecraft:trial_chamber/melee/husk", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:husk".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:husk".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_melee".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/melee/spider", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:spider".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("simultaneous_mobs".into(),Value::Float(4.0)),("total_mobs".into(),Value::Float(12.0))]) },
        VanillaEntry { id: "minecraft:trial_chamber/melee/zombie", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:zombie".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:zombie".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_melee".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/ranged/poison_skeleton", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:bogged".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:bogged".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_ranged".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/ranged/skeleton", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:skeleton".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:skeleton".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_ranged".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/ranged/stray", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:stray".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:stray".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_ranged".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/slow_ranged/poison_skeleton", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(4.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(2.0)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:bogged".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(160))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:bogged".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_ranged".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/slow_ranged/skeleton", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(4.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(2.0)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:skeleton".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(160))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:skeleton".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_ranged".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/slow_ranged/stray", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(4.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(2.0)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:stray".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(160))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:stray".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_ranged".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/small_melee/baby_zombie", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(2.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("IsBaby".into(),Value::Byte(1)),("id".into(),Value::String("minecraft:zombie".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("IsBaby".into(),Value::Byte(1)),("id".into(),Value::String("minecraft:zombie".into()))]))),("equipment".into(),Value::Compound(Fields::from([("loot_table".into(),Value::String("minecraft:equipment/trial_chamber_melee".into())),("slot_drop_chances".into(),Value::Float(0.0))])))]))),("weight".into(),Value::Int(1))]))]))]) },
        VanillaEntry { id: "minecraft:trial_chamber/small_melee/cave_spider", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:cave_spider".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("simultaneous_mobs".into(),Value::Float(4.0)),("total_mobs".into(),Value::Float(12.0))]) },
        VanillaEntry { id: "minecraft:trial_chamber/small_melee/silverfish", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("id".into(),Value::String("minecraft:silverfish".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("simultaneous_mobs".into(),Value::Float(4.0)),("total_mobs".into(),Value::Float(12.0))]) },
        VanillaEntry { id: "minecraft:trial_chamber/small_melee/slime", normal: Fields::from([("simultaneous_mobs".into(),Value::Float(3.0)),("simultaneous_mobs_added_per_player".into(),Value::Float(0.5)),("spawn_potentials".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("Size".into(),Value::Int(1)),("id".into(),Value::String("minecraft:slime".into()))])))]))),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::Compound(Fields::from([("entity".into(),Value::Compound(Fields::from([("Size".into(),Value::Int(2)),("id".into(),Value::String("minecraft:slime".into()))])))]))),("weight".into(),Value::Int(1))]))])),("ticks_between_spawn".into(),Value::Int(20))]), ominous_delta: Fields::from([("loot_tables_to_eject".into(),Value::List(vec![Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/key".into())),("weight".into(),Value::Int(3))])),Value::Compound(Fields::from([("data".into(),Value::String("minecraft:spawners/ominous/trial_chamber/consumables".into())),("weight".into(),Value::Int(7))]))])),("simultaneous_mobs".into(),Value::Float(4.0)),("total_mobs".into(),Value::Float(12.0))]) },
    ]).as_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(id: &str) -> Fields {
        Fields::from([("id".into(), Value::String(id.into())), ("x".into(), Value::Int(0)),
            ("y".into(), Value::Int(64)), ("z".into(), Value::Int(0))])
    }

    #[test]
    fn trial_v3825_moves_all_nine_flat_fields_and_reverses_without_loss() {
        let original = full_config(&Fields::new());
        let mut fields = original.clone();
        let mut report = PayloadMigrationReport::default();
        trial(&mut fields, 3824, 3825, &mut report).unwrap();
        assert!(CONFIG_KEYS.iter().all(|key| !fields.contains_key(*key)));
        assert_eq!(fields.get("normal_config"), Some(&Value::Compound(original.clone())));
        trial(&mut fields, 3825, 3824, &mut report).unwrap();
        assert_eq!(fields, original);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn trial_v4061_matches_all_actual_vanilla_ominous_forms() {
        assert_eq!(vanilla_entries().len(), 14);
        for entry in vanilla_entries() {
            let merged = merge(&entry.normal, &entry.ominous_delta);
            for ominous in [entry.ominous_delta.clone(), merged.clone(), omit_numeric_defaults(&merged)] {
                assert_eq!(vanilla_pair(&entry.normal, &ominous), Some(entry.id));
                let mut fields = Fields::from([("normal_config".into(), Value::Compound(entry.normal.clone())),
                    ("ominous_config".into(), Value::Compound(ominous))]);
                trial(&mut fields, 3955, 4082, &mut PayloadMigrationReport::default()).unwrap();
                assert_eq!(fields["normal_config"], Value::String(format!("{}/normal", entry.id)));
                assert_eq!(fields["ominous_config"], Value::String(format!("{}/ominous", entry.id)));
            }
        }
    }

    #[test]
    fn trial_registry_ids_expand_to_complete_configs_and_round_trip_all_fourteen() {
        for entry in vanilla_entries() {
            let original = Fields::from([("normal_config".into(), Value::String(format!("{}/normal", entry.id))),
                ("ominous_config".into(), Value::String(format!("{}/ominous", entry.id)))]);
            let mut fields = original.clone();
            let mut report = PayloadMigrationReport::default();
            trial(&mut fields, 4082, 3955, &mut report).unwrap();
            for key in ["normal_config", "ominous_config"] {
                let Value::Compound(config) = &fields[key] else { panic!("registry ID was not expanded") };
                assert_eq!(config.len(), 9, "complete config includes codec defaults");
                assert!(CONFIG_KEYS.iter().all(|key| config.contains_key(*key)));
                assert!(matches!(&config["spawn_potentials"], Value::List(entries) if !entries.is_empty()),
                    "ominous expansion must inherit the vanilla mob, not expose only the DFU delta");
            }
            trial(&mut fields, 3955, 4082, &mut report).unwrap();
            assert_eq!(fields, original);
            assert!(report.issues.is_empty());
        }
    }

    #[test]
    fn trial_custom_registry_id_and_unsupported_target_are_precise_and_atomic() {
        let mut fields = tile("minecraft:trial_spawner");
        fields.insert("normal_config".into(), Value::String("custom:trial/not_available".into()));
        let original = fields.clone();
        let error = migrate(&mut fields, "minecraft:trial_spawner", 4082, 3955, &mut PayloadMigrationReport::default()).unwrap_err().to_string();
        assert!(error.contains("normal_config") && error.contains("custom:trial/not_available"));
        assert_eq!(fields, original);
        for id in ["minecraft:trial_spawner", "minecraft:vault"] {
            let mut fields = tile(id); let original = fields.clone();
            let error = migrate(&mut fields, id, 5023, 3465, &mut PayloadMigrationReport::default()).unwrap_err().to_string();
            assert!(error.contains(id) && error.contains("3465") && error.contains("注册表"));
            assert_eq!(fields, original);
        }
    }

    #[test]
    fn vault_all_three_nested_item_paths_use_actual_item_payload_migration() {
        let item = || Value::Compound(Fields::from([("id".into(), Value::String("minecraft:diamond".into())),
            ("count".into(), Value::Int(3)), ("components".into(), Value::Compound(Fields::from([
                ("minecraft:custom_name".into(), Value::String("{\"text\":\"vault item\"}".into()))])))]));
        let mut fields = tile("minecraft:vault");
        fields.insert("config".into(), Value::Compound(Fields::from([("key_item".into(), item())])));
        fields.insert("shared_data".into(), Value::Compound(Fields::from([("display_item".into(), item())])));
        fields.insert("server_data".into(), Value::Compound(Fields::from([("items_to_eject".into(), Value::List(vec![item()])),
            ("state_updating_resumes_at".into(), Value::Long(991)), ("total_ejections_needed".into(), Value::Int(4))])));
        let mut report = PayloadMigrationReport::default();
        migrate(&mut fields, "minecraft:vault", 3837, 4671, &mut report).unwrap();
        let get_item = |group: &str, key: &str| {
            let Value::Compound(group) = &fields[group] else { panic!() };
            match &group[key] { Value::List(items) => items[0].clone(), value => value.clone() }
        };
        for item in [get_item("config", "key_item"), get_item("shared_data", "display_item"), get_item("server_data", "items_to_eject")] {
            let Value::Compound(item) = item else { panic!() };
            assert_eq!(item["count"], Value::Int(3));
            let Value::Compound(components) = &item["components"] else { panic!() };
            assert!(matches!(components["minecraft:custom_name"], Value::Compound(_)));
        }
        let Value::Compound(server) = &fields["server_data"] else { panic!() };
        assert_eq!(server["state_updating_resumes_at"], Value::Long(991));
        assert_eq!(server["total_ejections_needed"], Value::Int(4));
        assert!(report.issues.is_empty());
    }

    #[test]
    fn vault_unregistered_precomponent_snapshot_is_rejected_without_mutation() {
        // V3807 introduced vault, but this product registers releases only:
        // the pre-component snapshot DataVersion 3817 has no exact registry.
        let item = || Value::Compound(Fields::from([("id".into(), Value::String("minecraft:diamond".into())), ("Count".into(), Value::Byte(2))]));
        let mut fields = tile("minecraft:vault");
        fields.extend(Fields::from([
            ("config".into(), Value::Compound(Fields::from([("key_item".into(), item())]))),
            ("shared_data".into(), Value::Compound(Fields::from([("display_item".into(), item())]))),
            ("server_data".into(), Value::Compound(Fields::from([("items_to_eject".into(), Value::List(vec![item()]))]))),
        ]));
        let original = fields.clone();
        let mut report = PayloadMigrationReport::default();
        let error = migrate(&mut fields, "minecraft:vault", 3817, 3839, &mut report).unwrap_err().to_string();
        assert_eq!(error, "vault.config.key_item：数据版本缺少真实注册表");
        assert_eq!(fields, original);
        assert!(report.changes.is_empty());
        assert!(report.issues.is_empty());
    }

    #[test]
    fn trial_spawn_data_and_potentials_reuse_spawner_entity_migration() {
        let spawn = || Value::Compound(Fields::from([("entity".into(), Value::Compound(Fields::from([
            ("id".into(), Value::String("minecraft:pig".into())), ("CustomName".into(), Value::String("{\"text\":\"spawn\"}".into()))])))]));
        let mut fields = tile("minecraft:trial_spawner");
        fields.insert("spawn_data".into(), spawn());
        fields.insert("normal_config".into(), Value::Compound(Fields::from([("spawn_potentials".into(), Value::List(vec![
            Value::Compound(Fields::from([("data".into(), spawn()), ("weight".into(), Value::Int(1))]))]))])));
        migrate(&mut fields, "minecraft:trial_spawner", 3955, 4671, &mut PayloadMigrationReport::default()).unwrap();
        let Value::Compound(data) = &fields["spawn_data"] else { panic!() };
        let Value::Compound(entity) = &data["entity"] else { panic!() };
        assert!(matches!(entity["CustomName"], Value::Compound(_)));
        let Value::Compound(config) = &fields["normal_config"] else { panic!() };
        let Value::List(potentials) = &config["spawn_potentials"] else { panic!() };
        let Value::Compound(potential) = &potentials[0] else { panic!() };
        let Value::Compound(data) = &potential["data"] else { panic!() };
        let Value::Compound(entity) = &data["entity"] else { panic!() };
        assert!(matches!(entity["CustomName"], Value::Compound(_)));
    }

    #[test]
    fn trial_flat_downgrade_migrates_spawn_potentials_exactly_once() {
        let entity = Value::Compound(Fields::from([("id".into(), Value::String("minecraft:pig".into())),
            ("CustomName".into(), Value::Compound(Fields::from([("text".into(), Value::String("once".into()))])))]));
        let data = Value::Compound(Fields::from([("entity".into(), entity)]));
        let potentials = Value::List(vec![Value::Compound(Fields::from([("data".into(), data), ("weight".into(), Value::Int(1))]))]);
        let mut fields = Fields::from([("normal_config".into(), Value::Compound(Fields::from([("spawn_potentials".into(), potentials)])))]);
        let mut report = PayloadMigrationReport::default();
        trial(&mut fields, 4671, 3700, &mut report).unwrap();
        assert!(!fields.contains_key("normal_config"));
        let Value::List(entries) = &fields["spawn_potentials"] else { panic!() };
        let Value::Compound(entry) = &entries[0] else { panic!() };
        let Value::Compound(data) = &entry["data"] else { panic!() };
        let Value::Compound(entity) = &data["entity"] else { panic!() };
        assert!(matches!(entity["CustomName"], Value::String(_)));
        assert!(report.issues.is_empty());
    }

    #[test]
    fn vault_nested_failure_keeps_original_and_names_the_exact_item_path() {
        let mut fields = tile("minecraft:vault");
        fields.insert("server_data".into(), Value::Compound(Fields::from([("items_to_eject".into(), Value::List(vec![
            Value::Compound(Fields::from([("id".into(), Value::Int(9))]))]))])));
        let original = fields.clone();
        let mut report = PayloadMigrationReport::default();
        let error = migrate(&mut fields, "minecraft:vault", 3837, 4671, &mut report).unwrap_err().to_string();
        assert!(error.contains("vault.server_data.items_to_eject[0]"));
        assert_eq!(fields, original);
        assert!(report.changes.is_empty());
    }
}
