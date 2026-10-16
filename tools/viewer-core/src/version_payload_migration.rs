//! Native, lossless migration of verified inventory and decorative payloads.
//! Evidence: actual 3465/4671 JAR ContainerHelper/ItemStack bytecode (official
//! client mappings verified in .tmp/beta-ui-corrections), and local 26.3 sources:
//! ContainerHelper, ItemStackWithSlot, ItemStack, BlockEntity, HopperBlockEntity.
//! Boundaries follow the local game's DataFixers.java and payload codecs. Unknown
//! migrations are reported as converter coverage, never as target incompatibility.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use fastnbt::Value;

use crate::version_conversion::target_has_item;

#[path = "version_item_components.rs"]
mod additional_items;

#[derive(Debug, Default)]
pub(crate) struct PayloadMigrationReport {
    pub changes: Vec<String>,
    pub issues: Vec<String>,
}

/// Accepts one Litematica region Compound. Only TileEntities may be transformed;
/// all other region data stays untouched. Any issue leaves the original unchanged.
pub(crate) fn migrate(
    region: &mut Value,
    source_version: i32,
    target_version: i32,
) -> Result<PayloadMigrationReport> {
    let mut report = PayloadMigrationReport::default();
    if crate::block_database::database()?
        .registry_by_data_version(source_version)
        .is_err()
        || crate::block_database::database()?
            .registry_by_data_version(target_version)
            .is_err()
    {
        report.issues.push(format!(
            "载荷缺少真实来源/目标注册表：{source_version} → {target_version}"
        ));
        return Ok(report);
    }
    if source_version == target_version {
        return Ok(report);
    }
    let Value::Compound(fields) = region else {
        report.issues.push("选区载荷不是 Compound".into());
        return Ok(report);
    };
    for field in ["BlockEntities"] {
        if let Some(payload) = fields.get(field) {
            if !matches!(payload, Value::List(entries) if entries.is_empty()) {
                report.issues.push(format!(
                    "{field}：此实体、计划刻或非标准载荷字段未覆盖，原数据保留"
                ));
            }
        }
    }
    let mut ticks = Vec::new();
    for field in ["PendingBlockTicks", "PendingFluidTicks"] {
        if let Some(payload) = fields.get(field) {
            let mut payload = payload.clone();
            if let Err(error) = migrate_ticks(&mut payload, field == "PendingFluidTicks", source_version, target_version) {
                report.issues.push(format!("{field}：{error}"));
            }
            ticks.push((field, payload));
        }
    }
    let mut entities = fields.get("Entities").cloned();
    if let Some(payload) = entities.as_mut() {
        match payload {
            Value::List(entries) => {
                for (index, entity) in entries.iter_mut().enumerate() {
                    if let Err(error) =
                        migrate_entity(entity, source_version, target_version, &mut report)
                    {
                        report.issues.push(format!(
                            "Entities[{index}]：转换器尚未实现此数据迁移（{error}）"
                        ));
                    }
                }
            }
            _ => report.issues.push("Entities 格式损坏，应为列表".into()),
        }
    }
    let mut candidate = fields.get("TileEntities").cloned();
    if let Some(payload) = candidate.as_mut() {
        match payload {
            Value::List(entries) => {
                for (index, tile) in entries.iter_mut().enumerate() {
                    migrate_container(
                        tile,
                        source_version,
                        target_version,
                        &format!("TileEntities[{index}]"),
                        &mut report,
                    )?;
                }
                // Official RemoveBlockEntityTagFix(v4885): beds no longer have
                // a block entity. Only remove its empty metadata carrier;
                // meaningful/custom payloads remain errors, not silent loss.
                if target_version >= 4885 || target_version < 1125 {
                    entries.retain(|tile| {
                        let empty = matches!(tile, Value::Compound(fields) if fields.get("id") == Some(&Value::String("minecraft:bed".into())) && fields.iter().all(|(key, value)| ["id", "x", "y", "z"].contains(&key.as_str()) || (key == "components" && matches!(value, Value::Compound(fields) if fields.is_empty()))));
                        if empty { report.changes.push("床：按原版v4885移除空方块实体，方块颜色/状态保留".into()); }
                        !empty
                    });
                }
            }
            _ => report.issues.push("TileEntities 不是 List".into()),
        }
    }
    if report.issues.is_empty() {
        for (field, payload) in ticks { fields.insert(field.into(), payload); }
        if let Some(payload) = candidate {
            fields.insert("TileEntities".into(), payload);
        }
        if let Some(payload) = entities {
            fields.insert("Entities".into(), payload);
        }
    } else {
        // Do not report proposed operations as completed changes.
        report.changes.clear();
    }
    Ok(report)
}

/// Bind payload identity to the actual replacement at each tile's stored
/// position. Replacing a container with stone cannot leave an orphan inventory,
/// while compatible containers reuse the existing slot/count/schema migration.
pub(crate) fn migrate_with_palette(
    region: &mut Value,
    source: i32,
    target: i32,
    original_names: &[String],
) -> Result<PayloadMigrationReport> {
    use crate::nbt::{
        BigEndianLongArray, Vec3i, bits_for_palette, palette_index_at, region_volume,
    };
    let Value::Compound(fields) = region else {
        bail!("选区不是Compound")
    };
    let Some(Value::List(original_tiles)) = fields.get("TileEntities") else {
        return migrate(region, source, target);
    };
    let original = original_tiles.clone();
    let mut tiles = original.clone();
    let mut changes = Vec::new();
    let mut issues = Vec::new();
    let Value::Compound(size) = fields.get("Size").context("选区缺少Size")? else {
        bail!("Size不是Compound")
    };
    let int = |field: Option<&Value>| -> Result<i32> {
        match field {
            Some(Value::Int(value)) => Ok(*value),
            _ => bail!("方块实体坐标不是Int"),
        }
    };
    let size = Vec3i {
        x: int(size.get("x"))?,
        y: int(size.get("y"))?,
        z: int(size.get("z"))?,
    };
    let volume = region_volume(&size)?;
    let (width, height, length) = (
        size.x.unsigned_abs() as usize,
        size.y.unsigned_abs() as usize,
        size.z.unsigned_abs() as usize,
    );
    let Some(Value::List(palette)) = fields.get("BlockStatePalette") else {
        bail!("选区缺少调色板")
    };
    let Some(Value::LongArray(packed)) = fields.get("BlockStates") else {
        bail!("选区缺少BlockStates")
    };
    let packed = BigEndianLongArray::new(packed.iter().copied().collect());
    let db = crate::block_database::database()?;
    for (index, tile) in tiles.iter_mut().enumerate() {
        let Value::Compound(tile_fields) = tile else {
            continue;
        };
        let Some(Value::String(id)) = tile_fields.get("id") else {
            continue;
        };
        let id = match id.as_str() {
            "Chest" => "chest",
            "Hopper" => "hopper",
            "Trap" => "dispenser",
            "Dropper" => "dropper",
            id => id.strip_prefix("minecraft:").unwrap_or(id),
        }
        .to_owned();
        if ![
            "chest",
            "barrel",
            "shulker_box",
            "hopper",
            "dropper",
            "dispenser",
        ]
        .contains(&id.as_str())
        {
            continue;
        }
        let (x, y, z) = (
            int(tile_fields.get("x"))?,
            int(tile_fields.get("y"))?,
            int(tile_fields.get("z"))?,
        );
        if x < 0
            || y < 0
            || z < 0
            || x as usize >= width
            || y as usize >= height
            || z as usize >= length
        {
            issues.push(format!("TileEntities[{index}]：坐标超出选区"));
            continue;
        }
        let position = (y as usize * length + z as usize) * width + x as usize;
        let palette_index = palette_index_at(
            &packed,
            volume,
            bits_for_palette(palette.len()),
            palette.len(),
            position,
        )?;
        let old_name = original_names.get(palette_index).context("原调色板缺失")?;
        if old_name != &format!("minecraft:{id}") && db.category(old_name) != Some(id.as_str()) {
            issues.push(format!("TileEntities[{index}]：原方块与容器实体不匹配"));
            continue;
        }
        let Some(Value::Compound(block)) = palette.get(palette_index) else {
            bail!("调色板条目不是Compound")
        };
        let Some(Value::String(target_name)) = block.get("Name") else {
            bail!("调色板缺少Name")
        };
        let Some(kind) = db.category(target_name).filter(|kind| {
            [
                "chest",
                "barrel",
                "shulker_box",
                "hopper",
                "dropper",
                "dispenser",
            ]
            .contains(kind)
        }) else {
            issues.push(format!(
                "TileEntities[{index}]：{target_name}不能保存原容器物品，选择兼容容器后再转换"
            ));
            continue;
        };
        if kind != id {
            let modern = format!("minecraft:{kind}");
            let output = if source < 704 {
                match kind {
                    "chest" => "Chest",
                    "hopper" => "Hopper",
                    "dispenser" => "Trap",
                    "dropper" => "Dropper",
                    _ => modern.as_str(),
                }
            } else {
                modern.as_str()
            };
            tile_fields.insert("id".into(), Value::String(output.into()));
            changes.push(format!(
                "TileEntities[{index}]：{id} → {kind}，保留库存并检查目标槽位"
            ));
            if source == target {
                let mut same = PayloadMigrationReport::default();
                migrate_container(
                    tile,
                    source,
                    target,
                    &format!("TileEntities[{index}]"),
                    &mut same,
                )?;
                issues.extend(same.issues);
                changes.extend(same.changes);
            }
        }
    }
    if !issues.is_empty() {
        return Ok(PayloadMigrationReport {
            changes: Vec::new(),
            issues,
        });
    }
    fields.insert("TileEntities".into(), Value::List(tiles));
    let mut report = migrate(region, source, target)?;
    if report.issues.is_empty() {
        changes.extend(report.changes);
        report.changes = changes
    } else {
        let Value::Compound(fields) = region else {
            unreachable!()
        };
        fields.insert("TileEntities".into(), Value::List(original));
    }
    Ok(report)
}

fn migrate_container(
    tile: &mut Value,
    source: i32,
    target: i32,
    path: &str,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    let Value::Compound(fields) = tile else {
        report.issues.push(format!("{path} 不是 Compound"));
        return Ok(());
    };
    let Some(Value::String(source_id)) = fields.get("id") else {
        report.issues.push(format!("{path}/id 缺失或不是 String"));
        return Ok(());
    };
    // Actual BlockEntityIdFix(v704) mapping, before namespaced IDs.
    let id = match source_id.as_str() {
        "Chest" => "minecraft:chest",
        "Hopper" => "minecraft:hopper",
        "Trap" => "minecraft:dispenser",
        "Dropper" => "minecraft:dropper",
        "Banner" => "minecraft:banner",
        "Sign" => "minecraft:sign",
        "Cauldron" => "minecraft:brewing_stand",
        "Beacon" => "minecraft:beacon",
        "Skull" => "minecraft:skull",
        "MobSpawner" => "minecraft:mob_spawner",
        "Furnace"=>"minecraft:furnace",
        "DLDetector"=>"minecraft:daylight_detector",
        id => id,
    }
    .to_owned();
    let slots = match id.as_str() {
        "minecraft:chest" | "minecraft:barrel" | "minecraft:shulker_box" => 27,
        "minecraft:hopper" => 5,
        "minecraft:dispenser" | "minecraft:dropper" => 9,
        "minecraft:furnace" | "minecraft:smoker" | "minecraft:blast_furnace"=>3,
        _ => {
            if let Err(error) = migrate_decorative(fields, &id, source, target, report) {
                report.issues.push(format!("{path}/{id}：{error}"));
            }
            return Ok(());
        }
    };
    let hopper = id == "minecraft:hopper";
    let furnace=["minecraft:furnace","minecraft:smoker","minecraft:blast_furnace"].contains(&id.as_str());
    if furnace { migrate_furnace(fields,source,target)?; }
    let target_registry = crate::block_database::database()?.registry_by_data_version(target)?;
    if !target_registry.blocks.contains_key(&id)
        && !(id == "minecraft:shulker_box"
            && target_registry
                .blocks
                .keys()
                .any(|id| id.ends_with("_shulker_box")))
    {
        report
            .issues
            .push(format!("{path}/{id}：目标没有对应方块实体"));
    }
    let mut keys: Vec<_> = fields.keys().cloned().collect();
    keys.sort();
    for key in keys {
        match key.as_str() {
            "id" | "Items" => {}
            "CustomName" => {
                if let Some(value) = fields.get(&key) {
                    match migrate_component(value, source, target) {
                        Ok(value) => {
                            fields.insert(key.clone(), value);
                            report
                                .changes
                                .push(format!("{path}/CustomName：按目标文本格式转换"));
                        }
                        Err(error) => report.issues.push(format!("{path}/CustomName：{error}")),
                    }
                }
            }
            "LootTable" if matches!(fields.get(&key), Some(Value::String(_))) => {
                let Some(Value::String(id)) = fields.get(&key) else {
                    unreachable!()
                };
                if target < 169 || id.parse::<render_ir::MinecraftIdentifier>().is_err() {
                    report
                        .issues
                        .push(format!("{path}/LootTable：目标不支持该有效资源标识"));
                }
            }
            "LootTableSeed" if matches!(fields.get(&key), Some(Value::Long(_))) => {}
            "x" | "y" | "z" => {
                if !matches!(fields.get(&key), Some(Value::Int(_))) {
                    report.issues.push(format!("{path}/{key} 必须为 Int 坐标"));
                }
            }
            // Modern BlockEntity.saveWithoutMetadata writes this even when
            // there are no components. Retain the empty carrier unchanged,
            // including on downgrade; no component data is discarded.
            "components" if source >= 3818 => {
                if !matches!(fields.get(&key), Some(Value::Compound(map)) if map.is_empty()) {
                    report
                        .issues
                        .push(format!("{path}/components：非空组件未覆盖"));
                }
            }
            "TransferCooldown" if hopper => {
                if !matches!(fields.get(&key), Some(Value::Int(_))) {
                    report
                        .issues
                        .push(format!("{path}/TransferCooldown 必须为 Int"));
                }
            }
            key if furnace && ["BurnTime","CookTime","CookTimeTotal","RecipesUsed","RecipesUsedSize","cooking_time_spent","cooking_total_time","lit_time_remaining","lit_total_time","speed_multiplier"].contains(&key)=>{}
            key if furnace && target<2501 && (key.starts_with("RecipeLocation")||key.starts_with("RecipeAmount"))=>{}
            _ => report
                .issues
                .push(format!("{path}/{key}：此方块实体字段未覆盖")),
        }
    }
    let output_id = if target < 704 {
        match id.as_str() {
            "minecraft:chest" => "Chest",
            "minecraft:hopper" => "Hopper",
            "minecraft:dispenser" => "Trap",
            "minecraft:dropper" => "Dropper",
            "minecraft:furnace"=>"Furnace",
            id => id,
        }
    } else {
        id.as_str()
    };
    fields.insert("id".into(), Value::String(output_id.into()));
    for coordinate in ["x", "y", "z"] {
        if !fields.contains_key(coordinate) {
            report
                .issues
                .push(format!("{path}/{coordinate}：缺少已核实的 Int 坐标"));
        }
    }
    // Missing Items is the ordinary empty inventory case (notably shulker
    // saveAllItems(..., false)); no empty list is synthesized or dropped.
    if let Some(items) = fields.get_mut("Items") {
        if let Value::List(entries) = items {
            let mut seen_slots = HashSet::new();
            for (index, stack) in entries.iter_mut().enumerate() {
                migrate_stack(
                    stack,
                    source,
                    target,
                    slots,
                    &mut seen_slots,
                    &format!("{path}/Items[{index}]"),
                    report,
                )?;
            }
        } else {
            report.issues.push(format!("{path}/Items 不是 List"));
        }
    }
    Ok(())
}

fn migrate_stack(
    stack: &mut Value,
    source: i32,
    target: i32,
    slots: u8,
    seen_slots: &mut HashSet<u8>,
    path: &str,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    let Value::Compound(fields) = stack else {
        report.issues.push(format!("{path} 不是 Compound"));
        return Ok(());
    };
    let issue_start = report.issues.len();
    let source_count = if source < 3818 { "Count" } else { "count" };
    let mut keys: Vec<_> = fields.keys().cloned().collect();
    keys.sort();
    for key in keys {
        if key != "id"
            && key != "Slot"
            && key != source_count
            && !(source < 1519 && key == "Damage")
            && !(source < 3818 && key == "tag")
            && !(source >= 3818 && key == "components")
        {
            report.issues.push(format!(
                "{path}/{key}：只覆盖无 tag/components 的普通 ItemStack"
            ));
        }
    }
    let payload_result = (|| -> Result<()> {
        additional_items::prepare_legacy_stack(fields, source, target, report)?;
        if source < 3818 {
            if let Some(tag) = fields.remove("tag") {
                let item_id = match fields.get("id") { Some(Value::String(id)) => id.as_str(), _ => "" };
                let mut components = legacy_item_components(tag, source, item_id, report)?;
                migrate_components(&mut components, 3818, target.max(3818), report)?;
                if target >= 3818 { fields.insert("components".into(), components); }
                else { fields.insert("tag".into(), components_legacy_item(components, target, report)?); }
            }
            if fields.contains_key("components") && target < 3818 { bail!("旧版来源不能带现代components") }
        } else if let Some(mut components) = fields.remove("components") {
            migrate_components(&mut components, source, target.max(3818), report)?;
            if target < 3818 { fields.insert("tag".into(), components_legacy_item(components, target, report)?); }
            else { fields.insert("components".into(), components); }
        }
        Ok(())
    })();
    if let Err(error) = payload_result { report.issues.push(format!("{path}：{error}")); }
    let slot = match fields.get("Slot") {
        Some(Value::Byte(value)) => *value as u8,
        // ContainerHelper 3465 uses getByte(...), default zero; both modern
        // ItemStackWithSlot codecs explicitly default a missing Slot to zero.
        None => 0,
        _ => {
            report.issues.push(format!("{path}/Slot 必须为 Byte"));
            return Ok(());
        }
    };
    if slot >= slots || !seen_slots.insert(slot) {
        report.issues.push(format!(
            "{path}/Slot={slot}：槽位越界或重复，拒绝静默覆盖物品"
        ));
    }
    let Some(Value::String(raw_id)) = fields.get("id") else {
        report.issues.push(format!("{path}/id 缺失或不是 String"));
        return Ok(());
    };
    let id = if source < 1519 {
        let registry = crate::block_database::database()?.registry_by_data_version(source)?;
        let damage = match fields.get("Damage") {
            Some(Value::Short(v)) => *v,
            None => 0,
            _ => {
                report.issues.push(format!("{path}/Damage 必须为 Short"));
                return Ok(());
            }
        };
        let numeric = registry
            .legacy_item_ids
            .iter()
            .find(|(_, name)| *name == raw_id)
            .map(|(id, _)| id)
            .context("旧版物品没有实际数值映射")?;
        registry
            .legacy_items
            .get(&format!("{numeric}:{damage}"))
            .cloned()
            .context("旧版物品状态尚无可无损迁移的映射")?
    } else {
        if source < 4541 && target >= 4541 && raw_id == "minecraft:chain" {
            "minecraft:iron_chain".into()
        } else if source >= 4541 && target < 4541 && raw_id == "minecraft:iron_chain" {
            "minecraft:chain".into()
        } else {
            raw_id.clone()
        }
    };
    if id == "minecraft:air" || (source >= 1519 && !target_has_item(source, raw_id)?) {
        report
            .issues
            .push(format!("{path}/id={id}：来源真实物品注册表未验证"));
    } else if target >= 1519 && !target_has_item(target, &id)? {
        report
            .issues
            .push(format!("{path}/id={id}：目标版本没有该真实物品"));
    }
    let count = match (source < 3818, fields.get(source_count)) {
        (true, Some(Value::Byte(value))) => i32::from(*value),
        (false, Some(Value::Int(value))) => *value,
        (false, None) => 1,
        _ => {
            report
                .issues
                .push(format!("{path}/{source_count}：未覆盖的数量类型"));
            return Ok(());
        }
    };
    let source_max = if source < 3818 { 127 } else { 99 };
    let target_max = if target < 3818 { 127 } else { 99 };
    if count < 1 || count > source_max || count > target_max {
        report.issues.push(format!(
            "{path}/{source_count}={count}：不在来源/目标可无损保存的数量范围内"
        ));
    }
    if target < 1519 {
        let registry = crate::block_database::database()?.registry_by_data_version(target)?;
        if let Some((numeric, _)) = registry.legacy_items.iter().find(|(_, name)| **name == id) {
            let (numeric, damage) = numeric.split_once(':').context("旧版物品映射损坏")?;
            let old_id = registry
                .legacy_item_ids
                .get(numeric)
                .context("旧版物品名称缺失")?;
            fields.insert("id".into(), Value::String(old_id.clone()));
            fields.insert("Damage".into(), Value::Short(damage.parse()?));
        } else {
            report
                .issues
                .push(format!("{path}/{id}：目标旧版没有对应物品"));
        }
    } else {
        fields.insert("id".into(), Value::String(id));
        if source < 1519 {
            fields.remove("Damage");
        }
    }
    if let Err(error) = additional_items::finish_legacy_stack(fields, source, target, report) {
        report.issues.push(format!("{path}：{error}"));
    }
    if report.issues.len() == issue_start && (source < 3818) != (target < 3818) {
        fields.remove(source_count);
        let (target_key, value) = if target < 3818 {
            ("Count", Value::Byte(count as i8))
        } else {
            ("count", Value::Int(count))
        };
        fields.insert(target_key.into(), value);
        report.changes.push(format!(
            "{path}：{source_count} → {target_key}，保留数量 {count} 与槽位 {slot}"
        ));
    }
    Ok(())
}

type Fields = std::collections::HashMap<String, Value>;

fn migrate_ticks(value: &mut Value, fluid: bool, source: i32, target: i32) -> Result<()> {
    let Value::List(ticks) = value else { bail!("计划刻必须为列表") };
    let registry = crate::block_database::database()?.registry_by_data_version(target)?;
    for tick in ticks {
        let fields = object(tick)?;
        let key = if fluid { "Fluid" } else { "Block" };
        for field in fields.keys() {
            if ![key, "x", "y", "z", "Time", "Priority", "SubTick"].contains(&field.as_str()) {
                bail!("计划刻字段 {field} 缺少来源schema，原数据保留")
            }
        }
        for axis in ["x", "y", "z"] { if !matches!(fields.get(axis), Some(Value::Int(_))) { bail!("计划刻坐标 {axis} 必须为Int") } }
        if !matches!(fields.get("Time"), Some(Value::Long(_) | Value::Int(_))) { bail!("计划刻缺少整数相对时间") }
        if !matches!(fields.get("Priority"), None | Some(Value::Int(-3..=3))) { bail!("计划刻优先级不在原版范围") }
        if !matches!(fields.get("SubTick"), None | Some(Value::Long(_))) { bail!("计划刻顺序必须为Long") }
        let Some(Value::String(id)) = fields.get_mut(key) else { bail!("计划刻缺少{key}资源名称") };
        if fluid {
            if !["minecraft:water", "minecraft:flowing_water", "minecraft:lava", "minecraft:flowing_lava", "minecraft:empty"].contains(&id.as_str()) { bail!("目标没有可验证的流体计划刻类型 {id}") }
            if target < 1519 { bail!("目标旧格式没有独立流体计划刻；保留原文件并选择1.13或更新目标") }
        } else {
            if source < 4541 && target >= 4541 && id == "minecraft:chain" { *id = "minecraft:iron_chain".into(); }
            else if source >= 4541 && target < 4541 && id == "minecraft:iron_chain" { *id = "minecraft:chain".into(); }
            if !registry.blocks.contains_key(id) { bail!("目标版本没有计划刻方块 {id}") }
        }
    }
    Ok(())
}

const EFFECT_NAMES: &[&str] = &["speed", "slowness", "haste", "mining_fatigue", "strength", "instant_health", "instant_damage", "jump_boost", "nausea", "regeneration", "resistance", "fire_resistance", "water_breathing", "invisibility", "blindness", "night_vision", "hunger", "weakness", "poison", "wither", "health_boost", "absorption", "saturation", "glowing", "levitation", "luck", "unluck", "slow_falling", "conduit_power", "dolphins_grace", "bad_omen", "hero_of_the_village", "darkness", "trial_omen", "raid_omen", "wind_charged", "weaving", "oozing", "infested"];

fn effect_id(value: &Value, source: i32, target: i32) -> Result<Value> {
    let name = if source < 3568 {
        let number = match value { Value::Byte(number) => i32::from(*number as u8), Value::Int(number) => *number, _ => bail!("旧效果ID不是数字") };
        EFFECT_NAMES.get(number.checked_sub(1).context("无效效果编号")? as usize).filter(|_| number <= 33).context("来源没有原版效果编号")?.to_string()
    } else { let Value::String(name) = value else { bail!("效果ID不是资源名称") }; name.strip_prefix("minecraft:").context("非原版效果需独立映射")?.to_string() };
    let number = EFFECT_NAMES.iter().position(|known| *known == name).context("来源效果没有原版注册映射")? + 1;
    let minimum = match number { 24..=27 => 169, 28..=30 => 1519, 31..=32 => 1952, 33 => 3105, 34.. => 3837, _ => 0 };
    if target < minimum { bail!("目标版本没有效果 minecraft:{name}") }
    Ok(if target < 3568 { Value::Byte(number as i8) } else { Value::String(format!("minecraft:{name}")) })
}

fn migrate_effect(value: &mut Value, source: i32, target: i32) -> Result<()> {
    let fields = object(value)?;
    let source_id = if source < 3568 { "Id" } else { "id" };
    let target_id = if target < 3568 { "Id" } else { "id" };
    let id = effect_id(fields.get(source_id).context("效果缺少ID")?, source, target)?;
    fields.remove(source_id); fields.insert(target_id.into(), id);
    for (old, new) in [("Ambient", "ambient"), ("Amplifier", "amplifier"), ("Duration", "duration"), ("ShowParticles", "show_particles"), ("ShowIcon", "show_icon"), ("HiddenEffect", "hidden_effect")] {
        let from = if source < 3568 { old } else { new }; let to = if target < 3568 { old } else { new };
        if let Some(mut value) = fields.remove(from) {
            if new == "hidden_effect" { if target < 2100 { bail!("目标旧版没有隐藏效果链") } migrate_effect(&mut value, source, target)?; }
            if new == "duration" && target < 3322 && value == Value::Int(-1) { bail!("目标旧版不能表示无限持续效果") }
            if new == "amplifier" {
                let amplifier = match value { Value::Byte(value) => i32::from(value as u8), Value::Int(value @ 0..=255) => value, _ => bail!("效果等级无效") };
                value = if target < 3568 { Value::Byte(amplifier as i8) } else { Value::Int(amplifier) };
            }
            fields.insert(to.into(), value);
        }
    }
    let duration = match fields.get(if target < 3568 { "Duration" } else { "duration" }) { Some(Value::Int(value)) => *value, _ => 0 };
    if let Some(Value::Compound(factor)) = fields.get_mut("FactorCalculationData") {
        if source < 3322 && target >= 3322 {
            if let Some(Value::Int(timestamp)) = factor.remove("effect_changed_timestamp") { factor.insert("ticks_active".into(), Value::Int(timestamp.checked_sub(duration).context("效果插值时间溢出")?)); }
        } else if source >= 3322 && target < 3322 { if let Some(Value::Int(ticks)) = factor.remove("ticks_active") { factor.insert("effect_changed_timestamp".into(), Value::Int(ticks.checked_add(duration).context("效果插值时间溢出")?)); } }
    }
    let names = if target < 3568 { ["Id", "Ambient", "Amplifier", "Duration", "ShowParticles", "ShowIcon", "HiddenEffect", "FactorCalculationData"] } else { ["id", "ambient", "amplifier", "duration", "show_particles", "show_icon", "hidden_effect", "FactorCalculationData"] };
    if fields.keys().any(|key| !names.contains(&key.as_str())) { bail!("效果包含来源codec以外的字段") }
    Ok(())
}

fn migrate_effects(value: &mut Value, source: i32, target: i32) -> Result<()> {
    let Value::List(effects) = value else { bail!("效果不是列表") };
    for effect in effects { migrate_effect(effect, source, target)?; }
    Ok(())
}

fn migrate_block_position(fields: &mut Fields, old: &str, new: &str, source: i32, target: i32) -> Result<()> {
    if source < 3813 && target >= 3813 {
        if let Some(value) = fields.remove(old) {
            let Value::Compound(map) = value else { bail!("旧位置{old}不是XYZ复合标签") };
            if map.len() != 3 { bail!("旧位置{old}含未知字段") }
            let axes = ["X", "Y", "Z"].into_iter().map(|axis| match map.get(axis) { Some(Value::Int(value)) => Ok(*value), _ => bail!("旧位置{old}/{axis}不是Int") }).collect::<Result<_>>()?;
            fields.insert(new.into(), Value::IntArray(fastnbt::IntArray::new(axes)));
        }
    } else if source >= 3813 && target < 3813 {
        if let Some(value) = fields.remove(new) {
            let Value::IntArray(axes) = value else { bail!("位置{new}不是IntArray") };
            if axes.len() != 3 { bail!("位置{new}缺少坐标") }
            fields.insert(old.into(), Value::Compound(["X", "Y", "Z"].into_iter().zip(axes.iter()).map(|(key, value)| (key.into(), Value::Int(*value))).collect()));
        }
    }
    Ok(())
}

fn migrate_bees(fields: &mut Fields, source: i32, target: i32, report: &mut PayloadMigrationReport) -> Result<()> {
    if target < 2225 { bail!("目标版本没有蜜蜂与蜂箱") }
    migrate_block_position(fields, "FlowerPos", "flower_pos", source, target)?;
    let from = if source < 3818 { "Bees" } else { "bees" }; let to = if target < 3818 { "Bees" } else { "bees" };
    if let Some(mut value) = fields.remove(from) {
        let Value::List(bees) = &mut value else { bail!("蜂群不是列表") };
        for bee in bees {
            let bee = object(bee)?;
            for (old, new) in [("EntityData", "entity_data"), ("TicksInHive", "ticks_in_hive"), ("MinOccupationTicks", "min_ticks_in_hive")] {
                let from = if source < 3818 { old } else { new }; let to = if target < 3818 { old } else { new };
                let mut value = bee.remove(from).with_context(|| format!("蜂群缺少{from}"))?;
                if new == "entity_data" { migrate_entity(&mut value, source, target, report)?; }
                else if !matches!(value, Value::Int(0..)) { bail!("蜜蜂占巢计时无效") }
                bee.insert(to.into(), value);
            }
            let allowed = if target < 3818 { ["EntityData", "TicksInHive", "MinOccupationTicks"] } else { ["entity_data", "ticks_in_hive", "min_ticks_in_hive"] };
            if bee.keys().any(|key| !allowed.contains(&key.as_str())) { bail!("蜜蜂占巢记录含未知字段") }
        }
        fields.insert(to.into(), value);
    }
    Ok(())
}

fn validate_spawn_data(fields: &Fields, target: i32) -> Result<()> {
    if fields.keys().any(|key| !["entity", "custom_spawn_rules", "equipment"].contains(&key.as_str())) { bail!("刷怪笼SpawnData含未知字段") }
    if let Some(rules) = fields.get("custom_spawn_rules") {
        if target < 2831 { bail!("目标旧版没有刷怪笼自定义光照规则") }
        let Value::Compound(rules) = rules else { bail!("刷怪笼自定义规则不是Compound") };
        if rules.keys().any(|key| key != "block_light_limit" && key != "sky_light_limit") { bail!("刷怪笼自定义规则字段未知") }
        for range in rules.values() {
            let (min,max) = match range {
                Value::Int(value) => (*value,*value),
                Value::List(values) if values.len()==2 => { let (Value::Int(min),Value::Int(max))=(&values[0],&values[1]) else { bail!("刷怪笼光照范围类型无效") }; (*min,*max) },
                Value::Compound(values) if values.len()==2 => { let (Some(Value::Int(min)),Some(Value::Int(max)))=(values.get("min_inclusive"),values.get("max_inclusive")) else { bail!("刷怪笼光照范围缺少上下限") }; (*min,*max) },
                _ => bail!("刷怪笼光照范围编码无效"),
            };
            if !(0..=15).contains(&min) || !(min..=15).contains(&max) { bail!("刷怪笼光照范围超出0..15") }
        }
    }
    if let Some(equipment) = fields.get("equipment") {
        if target < 3837 { bail!("目标1.20.5以前没有刷怪笼装备表编码") }
        let Value::Compound(equipment) = equipment else { bail!("刷怪笼装备表不是Compound") };
        if equipment.keys().any(|key| key!="loot_table" && key!="slot_drop_chances") || !matches!(equipment.get("loot_table"),Some(Value::String(id)) if id.contains(':')) { bail!("刷怪笼装备表字段无效") }
        if let Some(chances) = equipment.get("slot_drop_chances") {
            let chance = |value:&Value| -> Result<()> { if !matches!(value,Value::Float(n) if n.is_finite()) { bail!("刷怪笼装备掉落概率不是有限Float") } Ok(()) };
            match chances { Value::Float(_) => chance(chances)?, Value::Compound(chances) => { for (slot,value) in chances { if !["mainhand","offhand","feet","legs","chest","head","body","saddle"].contains(&slot.as_str()) || (slot=="saddle"&&target<4300) { bail!("目标版本没有刷怪笼装备槽位{slot}") } chance(value)?; } }, _=>bail!("刷怪笼装备掉落概率格式无效") }
        }
    }
    Ok(())
}

fn migrate_spawner(fields: &mut Fields, source: i32, target: i32, report: &mut PayloadMigrationReport) -> Result<()> {
    if source<106 {if let Some(Value::String(id))=fields.remove("EntityId"){let data=fields.entry("SpawnData".into()).or_insert(Value::Compound(Fields::new()));object(data)?.insert("id".into(),Value::String(if id.is_empty(){"Pig".into()}else{id}));}}
    if let Some(mut data)=fields.remove("SpawnData"){
        if source<2831{migrate_entity(&mut data,source,target,report)?;if target>=2831{data=Value::Compound([("entity".into(),data)].into());}}
        else{let map=object(&mut data)?;validate_spawn_data(map,target)?;let entity=map.get_mut("entity").context("刷怪笼SpawnData缺少entity")?;migrate_entity(entity,source,target,report)?;if target<2831{data=map.remove("entity").unwrap();}}
        if target<106{let map=object(&mut data)?;fields.insert("EntityId".into(),map.remove("id").context("刷怪笼缺少实体id")?);}
        fields.insert("SpawnData".into(),data);
    }
    if let Some(Value::List(entries))=fields.get_mut("SpawnPotentials"){for entry in entries{
        let map=object(entry)?;
        if source<106{if let Some(id)=map.remove("Type"){let mut entity=map.remove("Properties").unwrap_or(Value::Compound(Fields::new()));object(&mut entity)?.insert("id".into(),id);map.insert("Entity".into(),entity);}}
        let mut extras=Fields::new();
        let mut entity=if source<2831{map.remove("Entity").context("刷怪笼权重条目缺少Entity")?}else{let Some(Value::Compound(mut data))=map.remove("data")else{bail!("刷怪笼权重条目缺少data")};validate_spawn_data(&data,target)?;let entity=data.remove("entity").context("刷怪笼权重条目缺少entity")?;extras=data;entity};
        migrate_entity(&mut entity,source,target,report)?;
        let weight=map.remove(if source<2831{"Weight"}else{"weight"}).unwrap_or(Value::Int(1));if !matches!(weight,Value::Int(1..)){bail!("刷怪笼权重无效")}
        if !map.is_empty(){bail!("刷怪笼权重有未知字段")}
        if target>=2831{extras.insert("entity".into(),entity);map.insert("data".into(),Value::Compound(extras));map.insert("weight".into(),weight);}
        else{if target<106{let entity=object(&mut entity)?;map.insert("Type".into(),entity.remove("id").context("刷怪笼缺少类型")?);map.insert("Properties".into(),Value::Compound(entity.clone()));}else{map.insert("Entity".into(),entity);}map.insert("Weight".into(),weight);}
    }}
    Ok(())
}

fn migrate_furnace(fields:&mut Fields,source:i32,target:i32)->Result<()>{
    if (source<4181)!=(target<4181){
        for(old,new)in[("CookTime","cooking_time_spent"),("CookTimeTotal","cooking_total_time"),("BurnTime","lit_time_remaining")]{let(from,to)=if target>=4181{(old,new)}else{(new,old)};
            if let Some(value)=fields.remove(from){let number=match value{Value::Short(value)=>i32::from(value),Value::Int(value)=>value,_=>bail!("熔炉时间字段{from}类型无效")};fields.insert(to.into(),if target>=4181{Value::Int(number)}else{Value::Short(i16::try_from(number).context("目标旧版熔炉时间超过Short范围")?)});}
        }
        if target>=4181{if let Some(value)=fields.get("lit_time_remaining").cloned(){fields.insert("lit_total_time".into(),value);}}
        else{if let Some(total)=fields.remove("lit_total_time"){let n=match total{Value::Int(n)=>n,Value::Short(n)=>i32::from(n),_=>bail!("熔炉总燃烧时间无效")};let remaining=match fields.get("BurnTime"){Some(Value::Short(n))=>i32::from(*n),_=>0};if n!=remaining{bail!("目标旧版没有独立熔炉总燃烧时间，原文件保留")}}if fields.remove("speed_multiplier").is_some_and(|v|v!=Value::Float(1.0)){bail!("目标旧版不能保存自定义熔炼速度")}}
    }
    if source<2501&&target>=2501{if let Some(Value::Int(count))=fields.remove("RecipesUsedSize"){if !(0..=100000).contains(&count){bail!("熔炉配方记录数量无效")};let mut recipes=Fields::new();for i in 0..count{let Some(Value::String(id))=fields.remove(&format!("RecipeLocation{i}"))else{bail!("熔炉配方ID缺失")};let value=fields.remove(&format!("RecipeAmount{i}")).context("熔炉配方次数缺失")?;if recipes.insert(id,value).is_some(){bail!("重复熔炉配方记录")}}fields.insert("RecipesUsed".into(),Value::Compound(recipes));}}
    else if source>=2501&&target<2501{if let Some(Value::Compound(recipes))=fields.remove("RecipesUsed"){fields.insert("RecipesUsedSize".into(),Value::Int(recipes.len()as i32));let mut recipes=recipes.into_iter().collect::<Vec<_>>();recipes.sort_by(|a,b|a.0.cmp(&b.0));for(i,(id,count))in recipes.into_iter().enumerate(){fields.insert(format!("RecipeLocation{i}"),Value::String(id));fields.insert(format!("RecipeAmount{i}"),count);}}}
    Ok(())
}

// DataFixers v1451.5 changes dye damage order; v3818.1 replaces the
// complete BannerPatternFormatFix short-code registry and numeric dye IDs.
const DYE_NAMES: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];
const BANNER_PATTERNS: &[(&str, &str)] = &[
    ("b", "base"), ("bl", "square_bottom_left"), ("br", "square_bottom_right"), ("tl", "square_top_left"), ("tr", "square_top_right"),
    ("bs", "stripe_bottom"), ("ts", "stripe_top"), ("ls", "stripe_left"), ("rs", "stripe_right"), ("cs", "stripe_center"), ("ms", "stripe_middle"),
    ("drs", "stripe_downright"), ("dls", "stripe_downleft"), ("ss", "small_stripes"), ("cr", "cross"), ("sc", "straight_cross"),
    ("bt", "triangle_bottom"), ("tt", "triangle_top"), ("bts", "triangles_bottom"), ("tts", "triangles_top"),
    ("ld", "diagonal_left"), ("rd", "diagonal_up_right"), ("lud", "diagonal_up_left"), ("rud", "diagonal_right"),
    ("mc", "circle"), ("mr", "rhombus"), ("vh", "half_vertical"), ("hh", "half_horizontal"), ("vhr", "half_vertical_right"), ("hhb", "half_horizontal_bottom"),
    ("bo", "border"), ("cbo", "curly_border"), ("gra", "gradient"), ("gru", "gradient_up"), ("bri", "bricks"),
    ("glb", "globe"), ("cre", "creeper"), ("sku", "skull"), ("flo", "flower"), ("moj", "mojang"), ("pig", "piglin"),
];

pub(crate) fn legacy_banner_name(name: &str) -> Option<&'static str> {
    for color in DYE_NAMES {
        if name == format!("minecraft:{color}_banner") { return Some("minecraft:white_banner"); }
        if name == format!("minecraft:{color}_wall_banner") { return Some("minecraft:white_wall_banner"); }
    }
    None
}

/// The pre-flattening banner colour belongs to each block entity, not its
/// numeric block state. Split shared palette entries only for actual coloured
/// positions, preserving the original rotation/facing and all other blocks.
// BedBlockEntity color (v1125) is per position until flattening, while the
// legacy block registry contains only numeric block26 / red_bed states.
fn migrate_bed_palette(region:&mut Value,source:i32,target:i32)->Result<()> {
    use crate::nbt::{BigEndianLongArray,Vec3i,bits_for_palette,for_each_palette_index,pack_palette_indices,region_volume};
    if (source<1519)==(target<1519){return Ok(())}
    let fields=object(region)?;
    let Some(Value::List(original_palette))=fields.get("BlockStatePalette")else{return Ok(())};
    if !original_palette.iter().any(|entry|matches!(entry,Value::Compound(entry) if matches!(entry.get("Name"),Some(Value::String(name)) if DYE_NAMES.iter().any(|color|name==&format!("minecraft:{color}_bed"))))){return Ok(())}
    let Some(Value::Compound(size))=fields.get("Size")else{bail!("床颜色迁移缺少选区Size")};
    let int=|value:Option<&Value>|->Result<i32>{match value{Some(Value::Int(value))=>Ok(*value),_=>bail!("床坐标/尺寸不是Int")}};
    let size=Vec3i{x:int(size.get("x"))?,y:int(size.get("y"))?,z:int(size.get("z"))?};
    let(width,height,length)=(size.x.unsigned_abs()as usize,size.y.unsigned_abs()as usize,size.z.unsigned_abs()as usize);
    let Some(Value::LongArray(packed))=fields.get("BlockStates")else{bail!("床颜色迁移缺少方块索引")};
    let mut indices=Vec::with_capacity(region_volume(&size)?);
    for_each_palette_index(&BigEndianLongArray::new(packed.iter().copied().collect()),region_volume(&size)?,bits_for_palette(original_palette.len()),original_palette.len(),|_,index|{indices.push(index);Ok(())})?;
    let mut palette=original_palette.clone();
    let mut tiles=match fields.get("TileEntities"){Some(Value::List(tiles))=>tiles.clone(),None=>Vec::new(),_=>bail!("床颜色迁移TileEntities不是List")};
    if source<1519 {
        for tile in &mut tiles {
            let tile=object(tile)?;
            if !matches!(tile.get("id"),Some(Value::String(id)) if id=="minecraft:bed"){continue}
            let color=match tile.remove("color"){Some(Value::Byte(n))=>i32::from(n),Some(Value::Short(n))=>i32::from(n),Some(Value::Int(n))=>n,None=>14,_=>bail!("旧床color类型无效")};
            if !(0..=15).contains(&color){bail!("旧床color超出染料注册表")}
            let(x,y,z)=(int(tile.get("x"))?,int(tile.get("y"))?,int(tile.get("z"))?);
            if x<0||y<0||z<0||x as usize>=width||y as usize>=height||z as usize>=length{bail!("床坐标超出选区")}
            let position=(y as usize*length+z as usize)*width+x as usize;
            let mut block=palette[indices[position]].clone();let entry=object(&mut block)?;
            if entry.get("Name")!=Some(&Value::String("minecraft:red_bed".into())){bail!("旧床方块实体与实际方块不匹配")}
            entry.insert("Name".into(),Value::String(format!("minecraft:{}_bed",DYE_NAMES[color as usize])));
            let index=palette.iter().position(|entry|entry==&block).unwrap_or_else(||{palette.push(block);palette.len()-1});indices[position]=index;
        }
    } else {
        let mut colors=Vec::with_capacity(palette.len());
        for entry in &mut palette {
            let entry=object(entry)?;let color=match entry.get("Name"){Some(Value::String(name))=>DYE_NAMES.iter().position(|color|name==&format!("minecraft:{color}_bed")),_=>None};
            if let Some(color)=color {if target<1125&&color!=14{bail!("目标1.12以前只有红床，所选床颜色不能等价保留")};entry.insert("Name".into(),Value::String("minecraft:red_bed".into()));}
            colors.push(color);
        }
        for(position,index)in indices.iter().enumerate(){let Some(color)=colors[*index]else{continue};if target<1125{continue}
            let x=(position%width)as i32;let z=(position/width%length)as i32;let y=(position/(width*length))as i32;
            let tile_index=tiles.iter().position(|tile|matches!(tile,Value::Compound(tile) if tile.get("x")==Some(&Value::Int(x))&&tile.get("y")==Some(&Value::Int(y))&&tile.get("z")==Some(&Value::Int(z))));
            if let Some(index)=tile_index{let tile=object(&mut tiles[index])?;if tile.get("id")!=Some(&Value::String("minecraft:bed".into())){bail!("床位置有其他方块实体，拒绝覆盖")};tile.insert("color".into(),Value::Int(color as i32));}
            else{tiles.push(Value::Compound([("id".into(),Value::String("minecraft:bed".into())),("x".into(),Value::Int(x)),("y".into(),Value::Int(y)),("z".into(),Value::Int(z)),("color".into(),Value::Int(color as i32))].into()));}
        }
    }
    let packed=pack_palette_indices(&indices,bits_for_palette(palette.len()));
    fields.insert("BlockStatePalette".into(),Value::List(palette));fields.insert("BlockStates".into(),Value::LongArray(fastnbt::LongArray::new((0..packed.len()).map(|index|packed.i64_at(index)).collect())));fields.insert("TileEntities".into(),Value::List(tiles));
    Ok(())
}

pub(crate) fn migrate_banner_palette(region: &mut Value, source: i32, target: i32) -> Result<()> {
    migrate_bed_palette(region,source,target)?;
    use crate::nbt::{BigEndianLongArray, Vec3i, bits_for_palette, for_each_palette_index, pack_palette_indices, region_volume};
    if (source < 1519) == (target < 1519) { return Ok(()) }
    let fields = object(region)?;
    let Some(Value::List(tiles)) = fields.get("TileEntities") else { return Ok(()) };
    if !tiles.iter().any(|tile| matches!(tile, Value::Compound(t) if matches!(t.get("id"), Some(Value::String(id)) if id == "Banner" || id == "minecraft:banner"))) { return Ok(()) }
    let Value::Compound(size) = fields.get("Size").context("缺少Size")? else { bail!("Size格式无效") };
    let int = |value: Option<&Value>| -> Result<i32> { match value { Some(Value::Int(value)) => Ok(*value), _ => bail!("旗帜坐标/尺寸无效") } };
    let size = Vec3i { x: int(size.get("x"))?, y: int(size.get("y"))?, z: int(size.get("z"))? };
    let (width, height, length) = (size.x.unsigned_abs() as usize, size.y.unsigned_abs() as usize, size.z.unsigned_abs() as usize);
    let volume = region_volume(&size)?;
    let Some(Value::List(palette)) = fields.get("BlockStatePalette") else { bail!("缺少调色板") };
    let Some(Value::LongArray(packed)) = fields.get("BlockStates") else { bail!("缺少方块索引") };
    let mut indices = Vec::with_capacity(volume);
    for_each_palette_index(&BigEndianLongArray::new(packed.iter().copied().collect()), volume, bits_for_palette(palette.len()), palette.len(), |_, index| { indices.push(index); Ok(()) })?;
    let mut palette = palette.clone();
    let mut tiles = tiles.clone();
    for tile in &mut tiles {
        let Value::Compound(t) = tile else { continue };
        if !matches!(t.get("id"), Some(Value::String(id)) if id == "Banner" || id == "minecraft:banner") { continue }
        let (x, y, z) = (int(t.get("x"))?, int(t.get("y"))?, int(t.get("z"))?);
        if x < 0 || y < 0 || z < 0 || x as usize >= width || y as usize >= height || z as usize >= length { bail!("旗帜坐标超出选区") }
        let position = (y as usize * length + z as usize) * width + x as usize;
        let mut block = palette[indices[position]].clone();
        let b = object(&mut block)?;
        let Some(Value::String(name)) = b.get("Name") else { bail!("旗帜调色板缺少名称") };
        if source < 1519 {
            let Some(Value::Int(base @ 0..=15)) = t.remove("Base") else { bail!("旧版旗帜缺少实际Base颜色") };
            let color = DYE_NAMES[if source < 1451 { 15 - base } else { base } as usize];
            let suffix = match name.as_str() { "minecraft:wall_banner" | "minecraft:white_wall_banner" => "wall_banner", "minecraft:standing_banner" | "minecraft:white_banner" => "banner", _ => bail!("旗帜方块与实体类型不匹配") };
            b.insert("Name".into(), Value::String(format!("minecraft:{color}_{suffix}")));
            let index = palette.iter().position(|entry| entry == &block).unwrap_or_else(|| { palette.push(block); palette.len() - 1 });
            indices[position] = index;
        } else {
            let color = DYE_NAMES.iter().position(|color| name == &format!("minecraft:{color}_banner") || name == &format!("minecraft:{color}_wall_banner")).context("旗帜方块与实体类型不匹配")?;
            t.insert("Base".into(), Value::Int(color as i32));
        }
    }
    let packed = pack_palette_indices(&indices, bits_for_palette(palette.len()));
    fields.insert("BlockStatePalette".into(), Value::List(palette));
    fields.insert("BlockStates".into(), Value::LongArray(fastnbt::LongArray::new((0..packed.len()).map(|index| packed.i64_at(index)).collect())));
    fields.insert("TileEntities".into(), Value::List(tiles));
    Ok(())
}

fn migrate_banner_patterns(value: &mut Value, source: i32, target: i32) -> Result<()> {
    let Value::List(patterns) = value else { bail!("旗帜图案必须为列表") };
    for layer in patterns {
        let fields = object(layer)?;
        let (pattern, color) = if source < 3818 {
            if fields.keys().any(|key| key != "Pattern" && key != "Color") { bail!("旗帜图案包含未知字段") }
            let Some(Value::String(pattern)) = fields.get("Pattern") else { bail!("旗帜缺少Pattern") };
            let Some(Value::Int(color @ 0..=15)) = fields.get("Color") else { bail!("旗帜Color不是有效染料编号") };
            let pattern = BANNER_PATTERNS.iter().find(|(short, _)| *short == pattern).map(|(_, name)| *name).context("来源旗帜图案没有原版映射")?;
            (pattern.to_owned(), if source < 1451 { 15 - *color } else { *color })
        } else {
            if fields.keys().any(|key| key != "pattern" && key != "color") { bail!("旗帜图案包含未知字段") }
            let Some(Value::String(pattern)) = fields.get("pattern") else { bail!("旗帜缺少pattern") };
            let Some(Value::String(color)) = fields.get("color") else { bail!("旗帜缺少染料颜色") };
            (pattern.strip_prefix("minecraft:").context("非原版旗帜图案需单独映射")?.into(), DYE_NAMES.iter().position(|name| name == color).context("旗帜颜色没有原版映射")? as i32)
        };
        if (pattern == "globe" && target < 1901) || (pattern == "piglin" && target < 2566) || (["flow", "guster"].contains(&pattern.as_str()) && target < 3837) {
            bail!("目标版本没有旗帜图案 minecraft:{pattern}，请选择支持该图案的目标版本")
        }
        if target < 3818 {
            let short = BANNER_PATTERNS.iter().find(|(_, name)| *name == pattern).map(|(short, _)| *short).context("目标旧版没有该旗帜图案")?;
            *fields = [("Pattern".into(), Value::String(short.into())), ("Color".into(), Value::Int(if target < 1451 { 15 - color } else { color }))].into();
        } else {
            *fields = [("pattern".into(), Value::String(format!("minecraft:{pattern}"))), ("color".into(), Value::String(DYE_NAMES[color as usize].into()))].into();
        }
    }
    Ok(())
}

fn migrate_sign_text(fields: &mut Fields, source: i32, target: i32) -> Result<()> {
    // Sign JSON dates from v165, earlier than CustomName's v1458 boundary.
    let text_source = if source >= 165 { source.max(1458) } else { source };
    let text_target = if target >= 165 { target.max(1458) } else { target };
    let empty = Value::String(if source < 165 { String::new() } else { "\"\"".into() });
    if source < 3439 && target >= 3439 {
        let mut messages = Vec::new();
        let mut filtered = Vec::new();
        let had_filtered = (1..=4).any(|line| fields.contains_key(&format!("FilteredText{line}")));
        for line in 1..=4 {
            let value = fields.remove(&format!("Text{line}")).unwrap_or_else(|| if source < 165 { Value::String(String::new()) } else { empty.clone() });
            filtered.push(fields.remove(&format!("FilteredText{line}")).unwrap_or_else(|| value.clone()));
            messages.push(value);
        }
        let mut front: Fields = [("messages".into(), Value::List(messages)), ("color".into(), fields.remove("Color").unwrap_or(Value::String("black".into()))), ("has_glowing_text".into(), fields.remove("GlowingText").unwrap_or(Value::Byte(0)))].into();
        if had_filtered { front.insert("filtered_messages".into(), Value::List(filtered)); }
        fields.insert("front_text".into(), Value::Compound(front));
        fields.insert("back_text".into(), Value::Compound([("messages".into(), Value::List(vec![empty.clone(); 4])), ("color".into(), Value::String("black".into())), ("has_glowing_text".into(), Value::Byte(0))].into()));
        fields.insert("is_waxed".into(), Value::Byte(0));
    }
    if source >= 3439 || target >= 3439 {
        for side in ["front_text", "back_text"] {
            if let Some(text) = fields.get_mut(side) {
                let text = object(text)?;
                if text.keys().any(|key| !["messages", "filtered_messages", "color", "has_glowing_text"].contains(&key.as_str())) { bail!("告示牌文字包含未知字段") }
                for key in ["messages", "filtered_messages"] {
                    if let Some(value) = text.get_mut(key) {
                        let Value::List(messages) = value else { bail!("告示牌文字不是列表") };
                        if messages.len() != 4 { bail!("告示牌必须保留四行文字") }
                        for message in messages { *message = migrate_component(message, text_source, text_target)?; }
                    }
                }
            }
        }
    } else {
        for prefix in ["Text", "FilteredText"] { for line in 1..=4 {
            if let Some(value) = fields.get_mut(&format!("{prefix}{line}")) { *value = migrate_component(value, text_source, text_target)?; }
        }}
    }
    if source >= 3439 && target < 3439 {
        if fields.get("is_waxed").is_some_and(|value| value != &Value::Byte(0)) { bail!("目标旧版不能保存告示牌封蜡状态，原文件保留") }
        if let Some(Value::Compound(back)) = fields.get("back_text") {
            let empty_line = migrate_component(&empty, 3465, text_target)?;
            let blank = |value: &Value| value == &empty_line || matches!(value, Value::String(text) if text.is_empty() || text == "\"\"" || text == "{\"text\":\"\"}");
            if back.get("messages").is_some_and(|value| !matches!(value, Value::List(messages) if messages.iter().all(blank)))
                || back.contains_key("filtered_messages") || back.get("color").is_some_and(|v| v != &Value::String("black".into()))
                || back.get("has_glowing_text").is_some_and(|v| v != &Value::Byte(0)) { bail!("目标旧版不能保存告示牌背面内容/样式，原文件保留") }
        }
        if let Some(Value::Compound(mut front)) = fields.remove("front_text") {
            for (from, prefix) in [("messages", "Text"), ("filtered_messages", "FilteredText")] {
                if let Some(Value::List(messages)) = front.remove(from) { for (index, message) in messages.into_iter().enumerate() { fields.insert(format!("{prefix}{}", index + 1), message); } }
            }
            if let Some(value) = front.remove("color") { if target < 1519 && value != Value::String("black".into()) { bail!("目标旧版不能保存告示牌染色") } if target >= 1519 { fields.insert("Color".into(), value); } }
            if let Some(value) = front.remove("has_glowing_text") { if target < 2724 && value != Value::Byte(0) { bail!("目标旧版不能保存发光告示牌文字") } if target >= 2724 { fields.insert("GlowingText".into(), value); } }
        }
        fields.remove("back_text"); fields.remove("is_waxed");
    }
    Ok(())
}

fn object(value: &mut Value) -> Result<&mut Fields> {
    match value {
        Value::Compound(fields) => Ok(fields),
        _ => bail!("数据应为复合标签"),
    }
}

// Inverse of the actual v3818.5 ItemStackComponentizationFix for these
// gameplay fields. Unknown tags/components are never hidden in custom_data.
fn legacy_item_components(mut value: Value, source: i32, item_id: &str, report: &mut PayloadMigrationReport) -> Result<Value> {
    let tag = object(&mut value)?;
    let mut components = Fields::new();
    additional_items::legacy_to_components(tag, &mut components, source, item_id, report)?;
    let hide_flags=match tag.remove("HideFlags"){Some(Value::Int(value))=>value,None=>0,_=>bail!("物品HideFlags类型无效")};
    if hide_flags & !255 !=0{bail!("物品HideFlags含未知位，原数据保留")}
    if let Some(value)=tag.remove("Unbreakable"){if value!=Value::Byte(0)&&value!=Value::Byte(1){bail!("Unbreakable无效")};if value==Value::Byte(1){components.insert("minecraft:unbreakable".into(),Value::Compound(Fields::new()));}}
    if let Some(value)=tag.remove("CustomModelData"){if !matches!(value,Value::Int(_)){bail!("CustomModelData必须为Int")};components.insert("minecraft:custom_model_data".into(),value);}
    let mut potion = Fields::new();
    for (old, new) in [("Potion", "potion"), ("CustomPotionColor", "custom_color")] { if let Some(value) = tag.remove(old) { potion.insert(new.into(), value); } }
    if let Some(mut effects) = tag.remove(if source < 3568 { "CustomPotionEffects" } else { "custom_potion_effects" }) {
        migrate_effects(&mut effects, source, 3818)?; potion.insert("custom_effects".into(), effects);
    }
    if !potion.is_empty() { components.insert("minecraft:potion_contents".into(), Value::Compound(potion)); }
    if item_id == "minecraft:written_book" || item_id == "minecraft:writable_book" {
        let mut book = Fields::new();
        if let Some(Value::List(pages)) = tag.remove("pages") {
            let mut filtered = match tag.remove("filtered_pages") { Some(Value::Compound(map)) => map, None => Fields::new(), _ => bail!("书页过滤数据无效") };
            let mut output = Vec::new();
            for (index, raw) in pages.into_iter().enumerate() {
                let raw = if item_id == "minecraft:written_book" { migrate_component(&raw, if source >= 165 { source.max(1458) } else { 0 }, 3818)? } else { raw };
                let mut page = Fields::from([("raw".into(), raw)]);
                if let Some(value) = filtered.remove(&index.to_string()) { page.insert("filtered".into(), if item_id == "minecraft:written_book" { migrate_component(&value, if source >= 165 { source.max(1458) } else { 0 }, 3818)? } else { value }); }
                output.push(Value::Compound(page));
            }
            if !filtered.is_empty() { bail!("过滤书页索引超出实际页数") }
            book.insert("pages".into(), Value::List(output));
        }
        if item_id == "minecraft:written_book" {
            let mut title = Fields::from([("raw".into(), tag.remove("title").unwrap_or(Value::String(String::new())))]);
            if let Some(value) = tag.remove("filtered_title") { title.insert("filtered".into(), value); }
            book.insert("title".into(), Value::Compound(title));
            for key in ["author", "resolved", "generation"] { if let Some(value) = tag.remove(key) { book.insert(key.into(), value); } }
        }
        components.insert(if item_id == "minecraft:written_book" { "minecraft:written_book_content" } else { "minecraft:writable_book_content" }.into(), Value::Compound(book));
    }
    for (old, new) in [("Damage", "minecraft:damage"), ("RepairCost", "minecraft:repair_cost")] {
        if let Some(value) = tag.remove(old) { if !matches!(value, Value::Int(_)) { bail!("物品{old}必须为Int") } components.insert(new.into(), value); }
    }
    if let Some(mut display) = tag.remove("display") {
        let display = object(&mut display)?;
        if let Some(name) = display.remove("Name") { components.insert("minecraft:custom_name".into(), migrate_component(&name, if source >= 1519 { source } else { source.min(1457) }, 3818)?); }
        if let Some(Value::List(lore)) = display.remove("Lore") {
            let lore = lore.iter().map(|line| migrate_component(line, if source >= 1803 { source } else { 0 }, 3818)).collect::<Result<_>>()?;
            components.insert("minecraft:lore".into(), Value::List(lore));
        }
        if let Some(color) = display.remove("color") {
            if !matches!(color, Value::Int(_)) { bail!("物品染色必须为Int") }
            components.insert("minecraft:dyed_color".into(), Value::Compound([("rgb".into(), color)].into()));
        }
        if !display.is_empty() { bail!("物品display字段 {:?} 需要对应codec迁移", display.keys().collect::<Vec<_>>()) }
    }
    for (old, new) in [("Enchantments", "minecraft:enchantments"), ("StoredEnchantments", "minecraft:stored_enchantments")] {
        if let Some(Value::List(enchantments)) = tag.remove(old) {
            let mut levels = Fields::new();
            for entry in enchantments {
                let Value::Compound(entry) = entry else { bail!("附魔条目不是Compound") };
                let Some(Value::String(id)) = entry.get("id") else { bail!("数字附魔ID需按旧注册表显式映射") };
                let level = match entry.get("lvl") { Some(Value::Short(value)) => i32::from(*value), Some(Value::Int(value)) => *value, _ => bail!("附魔等级无效") };
                if entry.len() != 2 || !(1..=255).contains(&level) || levels.insert(id.clone(), Value::Int(level)).is_some() { bail!("目标附魔等级/重复条目不能无损表示") }
            }
            components.insert(new.into(), Value::Compound([("levels".into(), Value::Compound(levels))].into()));
        }
    }
    if let Some(mut block) = tag.remove("BlockEntityTag") {
        let block = object(&mut block)?;
        if let Some(mut patterns) = block.remove(if source < 3818 { "Patterns" } else { "patterns" }) {
            migrate_banner_patterns(&mut patterns, source, 3818)?;
            components.insert("minecraft:banner_patterns".into(), patterns);
        }
        if let Some(Value::Int(color @ 0..=15)) = block.remove("Base") {
            let color = if source < 1451 { 15 - color } else { color };
            components.insert("minecraft:base_color".into(), Value::String(DYE_NAMES[color as usize].into()));
        }
        if let Some(Value::List(items)) = block.remove("Items") {
            let mut container = Vec::new();
            let mut seen = HashSet::new();
            for mut item in items {
                migrate_stack(&mut item, source, 3818, 255, &mut seen, "嵌套容器", report)?;
                let slot = match object(&mut item)?.remove("Slot") { Some(Value::Byte(slot)) => slot as u8, None => 0, _ => bail!("嵌套容器Slot无效") };
                container.push(Value::Compound([("slot".into(), Value::Int(i32::from(slot))), ("item".into(), item)].into()));
            }
            components.insert("minecraft:container".into(), Value::List(container));
        }
        if let Some(mut sherds) = block.remove("sherds") { migrate_pot_decorations(&mut sherds, source, 3818)?; components.insert("minecraft:pot_decorations".into(), sherds); }
        if block.keys().any(|key| key != "id") {
            let id = match block.get("id") { Some(Value::String(id)) => id.clone(), _ if item_id.ends_with("_sign") || item_id == "minecraft:sign" => "minecraft:sign".into(), _ => bail!("嵌套方块实体缺少可验证id") };
            block.insert("id".into(), Value::String(id));
            for axis in ["x", "y", "z"] { block.insert(axis.into(), Value::Int(0)); }
            let mut nested = Value::Compound(block.clone());
            migrate_container(&mut nested, source, 3818, "物品方块实体", report)?;
            let block = object(&mut nested)?;
            for axis in ["x", "y", "z"] { block.remove(axis); }
            components.insert("minecraft:block_entity_data".into(), nested);
        }
    }
    if hide_flags&1!=0 { components.entry("minecraft:enchantments".into()).or_insert(Value::Compound([("levels".into(),Value::Compound(Fields::new()))].into())); }
    if hide_flags&64!=0 { components.entry("minecraft:dyed_color".into()).or_insert(Value::Compound([("rgb".into(),Value::Int(10511680))].into())); }
    for(bit,key)in[(1,"minecraft:enchantments"),(4,"minecraft:unbreakable"),(32,"minecraft:stored_enchantments"),(64,"minecraft:dyed_color")]{if hide_flags&bit!=0{if let Some(component)=components.get_mut(key){object(component)?.insert("show_in_tooltip".into(),Value::Byte(0));}}}
    if !tag.is_empty() { bail!("物品标签 {:?} 需要对应codec迁移，原数据保留", tag.keys().collect::<Vec<_>>()) }
    Ok(Value::Compound(components))
}

fn components_legacy_item(mut value: Value, target: i32, report: &mut PayloadMigrationReport) -> Result<Value> {
    let components = object(&mut value)?;
    let mut tag = Fields::new();
    let mut display = Fields::new();
    let mut block = Fields::new();
    let mut hide_flags=0;
    for key in components.keys().cloned().collect::<Vec<_>>() {
        let mut value = components.remove(&key).unwrap();
        if additional_items::component_to_legacy(&key, &mut value, &mut tag, &mut block, &mut display, target, &mut hide_flags, report)? {
            continue;
        }
        match key.as_str() {
            "minecraft:hide_additional_tooltip" => { if !object(&mut value)?.is_empty(){bail!("额外提示隐藏标记有未知字段")};hide_flags|=32; }
            "minecraft:hide_tooltip" => bail!("目标旧版HideFlags不能等价保存整项隐藏提示"),
            "minecraft:damage" => { tag.insert("Damage".into(), value); }
            "minecraft:repair_cost" => { tag.insert("RepairCost".into(), value); }
            "minecraft:custom_name" => { display.insert("Name".into(), migrate_component(&value, 3818, if target >= 1519 { target } else { target.min(1457) })?); }
            "minecraft:lore" => { let Value::List(lines) = value else { bail!("物品Lore必须为List") }; display.insert("Lore".into(), Value::List(lines.iter().map(|line| migrate_component(line, 3818, if target >= 1803 { target } else { 0 })).collect::<Result<_>>()?)); }
            "minecraft:dyed_color" => { let fields = object(&mut value)?; if fields.keys().any(|key| key != "rgb" && key != "show_in_tooltip") { bail!("染色组件有未知字段") }if fields.get("show_in_tooltip")==Some(&Value::Byte(0)){hide_flags|=64;} display.insert("color".into(), fields.remove("rgb").context("缺少rgb")?); }
            "minecraft:unbreakable"=>{let fields=object(&mut value)?;if fields.keys().any(|key|key!="show_in_tooltip"){bail!("耐久组件有未知字段")};if fields.get("show_in_tooltip")==Some(&Value::Byte(0)){hide_flags|=4;}tag.insert("Unbreakable".into(),Value::Byte(1));}
            "minecraft:custom_model_data"=>{if !matches!(value,Value::Int(_)){bail!("目标旧版自定义模型数据必须为单个Int")};tag.insert("CustomModelData".into(),value);}
            "minecraft:enchantments" | "minecraft:stored_enchantments" => {
                if target < 1519 { bail!("目标旧版数字附魔ID需按实际注册表映射") }
                let fields = object(&mut value)?;
                if fields.keys().any(|key| key != "levels" && key != "show_in_tooltip") { bail!("附魔组件有未知字段") }
                if fields.get("show_in_tooltip")==Some(&Value::Byte(0)){hide_flags|=if key=="minecraft:enchantments"{1}else{32};}
                let Some(Value::Compound(levels)) = fields.remove("levels") else { bail!("附魔levels缺失") };
                let mut entries = Vec::new();
                for (id, level) in levels { let Value::Int(level @ 1..=255) = level else { bail!("附魔等级无效") }; entries.push(Value::Compound([("id".into(), Value::String(id)), ("lvl".into(), Value::Short(level as i16))].into())); }
                entries.sort_by_key(|entry| match entry { Value::Compound(fields) => format!("{:?}", fields.get("id")), _ => unreachable!() });
                tag.insert(if key == "minecraft:enchantments" { "Enchantments".into() } else { "StoredEnchantments".into() }, Value::List(entries));
            }
            "minecraft:banner_patterns" => { migrate_banner_patterns(&mut value, 3818, target)?; block.insert("Patterns".into(), value); }
            "minecraft:base_color" => { let Value::String(color) = value else { bail!("旗帜底色无效") }; let index = DYE_NAMES.iter().position(|name| *name == color).context("旗帜底色无映射")? as i32; block.insert("Base".into(), Value::Int(if target < 1451 { 15 - index } else { index })); }
            "minecraft:pot_decorations" => { migrate_pot_decorations(&mut value, 3818, target)?; block.insert("sherds".into(), value); }
            "minecraft:container" => {
                let Value::List(entries) = value else { bail!("容器组件不是列表") }; let mut items = Vec::new();
                for mut entry in entries { let fields = object(&mut entry)?; let Some(Value::Int(slot @ 0..=255)) = fields.remove("slot") else { bail!("容器组件slot无效") }; let mut item = fields.remove("item").context("容器组件缺少item")?; item_payload(&mut item, 3818, target, report)?; object(&mut item)?.insert("Slot".into(), Value::Byte(slot as i8)); items.push(item); }
                block.insert("Items".into(), Value::List(items));
            }
            "minecraft:potion_contents" => {
                let fields = object(&mut value)?;
                for (from, to) in [("potion", "Potion"), ("custom_color", "CustomPotionColor")] { if let Some(value) = fields.remove(from) { tag.insert(to.into(), value); } }
                if let Some(mut effects) = fields.remove("custom_effects") { migrate_effects(&mut effects, 3818, target)?; tag.insert(if target < 3568 { "CustomPotionEffects" } else { "custom_potion_effects" }.into(), effects); }
                if !fields.is_empty() { bail!("目标旧版药水不能保存额外组件字段") }
            }
            "minecraft:written_book_content" | "minecraft:writable_book_content" => {
                let book = object(&mut value)?; let written = key == "minecraft:written_book_content";
                if let Some(Value::List(pages)) = book.remove("pages") {
                    let mut raw = Vec::new(); let mut filtered = Fields::new();
                    for (index, mut page) in pages.into_iter().enumerate() { let page = object(&mut page)?;
                        let value = page.remove("raw").context("书页缺少raw")?;
                        raw.push(if written { migrate_component(&value, 3818, if target >= 165 { target.max(1458) } else { 0 })? } else { value });
                        if let Some(value) = page.remove("filtered") { filtered.insert(index.to_string(), if written { migrate_component(&value, 3818, if target >= 165 { target.max(1458) } else { 0 })? } else { value }); }
                        if !page.is_empty() { bail!("书页有未知字段") }
                    }
                    tag.insert("pages".into(), Value::List(raw)); if !filtered.is_empty() { tag.insert("filtered_pages".into(), Value::Compound(filtered)); }
                }
                if let Some(mut title) = book.remove("title") { let title = object(&mut title)?; tag.insert("title".into(), title.remove("raw").context("书名缺少raw")?); if let Some(value) = title.remove("filtered") { tag.insert("filtered_title".into(), value); } if !title.is_empty() { bail!("书名有未知字段") } }
                for key in ["author", "resolved", "generation"] { if let Some(value) = book.remove(key) { tag.insert(key.into(), value); } }
                if !book.is_empty() { bail!("书籍组件有未知字段") }
            }
            "minecraft:block_entity_data" => {
                let fields = object(&mut value)?;
                for axis in ["x", "y", "z"] { fields.insert(axis.into(), Value::Int(0)); }
                migrate_container(&mut value, 3818, target, "物品方块实体", report)?;
                for (key, value) in object(&mut value)?.drain() { if !["x", "y", "z"].contains(&key.as_str()) && block.insert(key, value).is_some() { bail!("物品方块实体组件冲突") } }
            }
            _ => bail!("目标旧物品标签不能表达组件 {key}，原数据保留"),
        }
    }
    if !display.is_empty() { tag.insert("display".into(), Value::Compound(display)); }
    if hide_flags!=0{tag.insert("HideFlags".into(),Value::Int(hide_flags));}
    if !block.is_empty() { tag.insert("BlockEntityTag".into(), Value::Compound(block)); }
    Ok(Value::Compound(tag))
}

fn item_payload(
    stack: &mut Value,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    if matches!(stack, Value::Compound(fields) if fields.is_empty()) {
        return Ok(());
    }
    migrate_stack(
        stack,
        source,
        target,
        255,
        &mut HashSet::new(),
        "物品",
        report,
    )
}

/// Exact component boundaries verified in TooltipDisplayComponentFix(v4307),
/// UnflattenTextComponentFix(v4290) and the corresponding ItemStack codecs.
fn migrate_components(
    value: &mut Value,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    let fields = object(value)?;
    let mut hidden = Vec::<String>::new();
    const ADDITIONAL: &[&str] = &["minecraft:banner_patterns","minecraft:bees","minecraft:block_entity_data","minecraft:block_state","minecraft:bundle_contents","minecraft:charged_projectiles","minecraft:container","minecraft:container_loot","minecraft:firework_explosion","minecraft:fireworks","minecraft:instrument","minecraft:map_id","minecraft:painting/variant","minecraft:pot_decorations","minecraft:potion_contents","minecraft:tropical_fish/pattern","minecraft:written_book_content"];
    let mut hide_tooltip=false;
    if source<4307&&target>=4307 {
        if let Some(mut marker)=fields.remove("minecraft:hide_additional_tooltip"){if !object(&mut marker)?.is_empty(){bail!("额外提示隐藏标记有未知字段")};for key in ADDITIONAL{if fields.contains_key(*key){hidden.push((*key).into());}}}
        if let Some(mut marker)=fields.remove("minecraft:hide_tooltip"){if !object(&mut marker)?.is_empty(){bail!("提示隐藏标记有未知字段")};hide_tooltip=true;}
    }
    let keys: Vec<_> = fields.keys().cloned().collect();
    for key in keys {
        let component = fields.get_mut(&key).unwrap();
        if additional_items::migrate_component(&key, component, source, target, report, &mut hidden)? {
            continue;
        }
        match key.as_str() {
            "minecraft:custom_name" | "minecraft:item_name" => {
                *component = migrate_component(component, source, target)?
            }
            "minecraft:lore" => {
                let Value::List(lines) = component else { bail!("物品lore不是列表") };
                for line in lines { *line = migrate_component(line, source, target)?; }
            }
            "minecraft:base_color" if matches!(component, Value::String(color) if DYE_NAMES.contains(&color.as_str())) => {}
            "minecraft:written_book_content" | "minecraft:writable_book_content" => {
                let book = object(component)?;
                if book.keys().any(|key| !["pages", "title", "author", "resolved", "generation"].contains(&key.as_str())) { bail!("书籍组件有未知字段") }
                if let Some(Value::List(pages)) = book.get_mut("pages") {
                    for page in pages {
                        let page = object(page)?;
                        if page.keys().any(|key| key != "raw" && key != "filtered") { bail!("书页包含未知字段") }
                        if key == "minecraft:written_book_content" { for value in page.values_mut() { *value = migrate_component(value, source, target)?; } }
                    }
                }
            }
            "minecraft:block_entity_data" => {
                let fields = object(component)?;
                for axis in ["x", "y", "z"] { fields.insert(axis.into(), Value::Int(0)); }
                migrate_container(component, source, target, "物品方块实体", report)?;
                for axis in ["x", "y", "z"] { object(component)?.remove(axis); }
            }
            "minecraft:dyed_color" | "minecraft:enchantments" | "minecraft:stored_enchantments" => {
                if source < 4307 && target >= 4307 {
                    let map = object(component)?;
                    let field = if key == "minecraft:dyed_color" {
                        "rgb"
                    } else {
                        "levels"
                    };
                    if map.get("show_in_tooltip") == Some(&Value::Byte(0)) {
                        hidden.push(key.clone());
                    }
                    if map
                        .keys()
                        .any(|key| key != field && key != "show_in_tooltip")
                    {
                        bail!("物品组件包含需单独迁移的字段")
                    }
                    *component = map.get(field).context("物品组件缺少实际值")?.clone();
                } else if source >= 4307 && target < 4307 {
                    let field = if key == "minecraft:dyed_color" {
                        "rgb"
                    } else {
                        "levels"
                    };
                    *component = Value::Compound([(field.into(), component.clone())].into());
                }
            }
            "minecraft:container" => {
                let Value::List(entries) = component else {
                    bail!("容器组件不是列表")
                };
                for entry in entries {
                    let map = object(entry)?;
                    if map.keys().any(|key| key != "slot" && key != "item")
                        || !matches!(map.get("slot"), Some(Value::Int(0..=255)))
                    {
                        bail!("容器组件槽位数据无效")
                    }
                    item_payload(
                        map.get_mut("item").context("容器组件缺少物品")?,
                        source,
                        target,
                        report,
                    )?;
                }
            }
            "minecraft:banner_patterns" => migrate_banner_patterns(component, source, target)?,
            "minecraft:pot_decorations" => migrate_pot_decorations(component, source, target)?,
            "minecraft:repair_cost" | "minecraft:damage" | "minecraft:ominous_bottle_amplifier"
                if matches!(component, Value::Int(_)) => {}
            "minecraft:potion_contents" => {
                let map = object(component)?;
                if map.keys().any(|key| !["potion", "custom_color", "custom_effects", "custom_name"].contains(&key.as_str())) { bail!("药水组件包含未知字段") }
                if let Some(effects) = map.get_mut("custom_effects") { migrate_effects(effects, source, target)?; }
            }
            "minecraft:instrument" if matches!(component, Value::String(_)) => {}
            "minecraft:unbreakable"=>{let map=object(component)?;if map.keys().any(|key|key!="show_in_tooltip"){bail!("耐久组件有未知字段")};if source<4307&&target>=4307&&map.remove("show_in_tooltip")==Some(Value::Byte(0)){hidden.push(key.clone());}}
            "minecraft:custom_model_data"=>{
                if source<4285&&target>=4285{let Value::Int(value)=component else{bail!("自定义模型数据必须为Int")};let number=*value as f32;if f64::from(number)!=f64::from(*value){bail!("目标浮点模型数据不能无损表示原整数")};*component=Value::Compound([("floats".into(),Value::List(vec![Value::Float(number)]))].into());}
                else if source>=4285&&target<4285{let map=object(component)?;if map.iter().any(|(key,value)|key!="floats"&&!matches!(value,Value::List(values)if values.is_empty())){bail!("目标旧版不能保存多通道自定义模型数据")};let Some(Value::List(values))=map.remove("floats")else{bail!("模型数据缺少floats")};if values.len()!=1{bail!("目标旧版只能保存单个模型整数")};let Value::Float(number)=values[0]else{bail!("模型数据不是Float")};if f64::from(number)!=f64::from(number as i32){bail!("目标旧版不能保存小数模型数据")};*component=Value::Int(number as i32);}
            }
            "minecraft:tooltip_display" => {}
            "minecraft:hide_additional_tooltip" | "minecraft:hide_tooltip" if source<4307&&target<4307 => {if !object(component)?.is_empty(){bail!("提示隐藏标记有未知字段")}}
            _ => bail!("转换器尚未实现物品组件 {key} 的迁移"),
        }
    }
    if !hidden.is_empty() || hide_tooltip {
        if fields.contains_key("minecraft:tooltip_display"){bail!("物品提示组件存在冲突")}
        hidden.sort();hidden.dedup();
        fields.insert("minecraft:tooltip_display".into(),Value::Compound([("hide_tooltip".into(),Value::Byte(i8::from(hide_tooltip))),("hidden_components".into(),Value::List(hidden.into_iter().map(Value::String).collect()))].into()));
    }
    if source>=4307&&target<4307 {
        if let Some(mut display)=fields.remove("minecraft:tooltip_display") {
            let display=object(&mut display)?;
            if display.keys().any(|key|key!="hide_tooltip"&&key!="hidden_components"){bail!("物品提示组件包含未知字段")}
            match display.get("hide_tooltip"){Some(Value::Byte(1))=>{fields.insert("minecraft:hide_tooltip".into(),Value::Compound(Fields::new()));},None|Some(Value::Byte(0))=>{},_=>bail!("hide_tooltip不是Boolean")}
            if let Some(hidden)=display.remove("hidden_components") {
                let Value::List(hidden)=hidden else{bail!("隐藏提示名称不是List")};
                let hidden=hidden.iter().map(|value|match value{Value::String(key)=>Ok(key.as_str()),_=>bail!("隐藏提示名称不是String")}).collect::<Result<HashSet<_>>>()?;
                let any_additional=ADDITIONAL.iter().any(|key|hidden.contains(key));
                if any_additional {
                    if ADDITIONAL.iter().any(|key|fields.contains_key(*key)&&!hidden.contains(key)){bail!("目标旧版只能同时隐藏全部额外提示，不能等价保留所选部分")}
                    fields.insert("minecraft:hide_additional_tooltip".into(),Value::Compound(Fields::new()));
                }
                for key in hidden {
                    if ADDITIONAL.contains(&key){continue}
                    if !["minecraft:trim","minecraft:unbreakable","minecraft:dyed_color","minecraft:attribute_modifiers","minecraft:enchantments","minecraft:stored_enchantments","minecraft:jukebox_playable","minecraft:can_break","minecraft:can_place_on"].contains(&key){bail!("目标旧版没有组件{key}的独立提示隐藏字段")}
                    object(fields.get_mut(key).context("被隐藏组件缺少显式载荷，目标默认值待验证")?)?.insert("show_in_tooltip".into(),Value::Byte(0));
                }
            }
        }
    }
    Ok(())
}

fn validate_patterns(value: &Value) -> Result<()> {
    let Value::List(patterns) = value else {
        bail!("旗帜图案不是列表")
    };
    for pattern in patterns {
        let Value::Compound(pattern) = pattern else {
            bail!("旗帜图案格式无效")
        };
        if pattern.len() != 2
            || !matches!(pattern.get("color"), Some(Value::String(_)))
            || !matches!(pattern.get("pattern"), Some(Value::String(_)))
        {
            bail!("旗帜图案包含需单独迁移的数据")
        }
    }
    Ok(())
}

/// Official v4996.1 four-side mapping; the inverse checks that legacy IDs can
/// represent every modern decoration without removing item components.
fn migrate_pot_decorations(value: &mut Value, source: i32, target: i32) -> Result<()> {
    const SIDES: [&str; 4] = ["back", "left", "right", "front"];
    if source < 4996 && target >= 4996 {
        let Value::List(ids) = value else {
            bail!("旧版陶罐装饰不是列表")
        };
        if ids.len() > 4 {
            bail!("陶罐装饰超过四面")
        }
        let mut sides = Fields::new();
        for (index, side) in SIDES.iter().enumerate() {
            let id = match ids.get(index) {
                Some(Value::String(id)) => id.clone(),
                None => "minecraft:brick".into(),
                _ => bail!("陶罐装饰物品名称无效"),
            };
            if !target_has_item(target, &id)? {
                bail!("目标版本没有陶罐装饰物品 {id}")
            }
            sides.insert(
                (*side).into(),
                Value::Compound([("id".into(), Value::String(id))].into()),
            );
        }
        *value = Value::Compound(sides);
    } else if source >= 4996 && target < 4996 {
        let map = object(value)?;
        if map.keys().any(|key| !SIDES.contains(&key.as_str())) {
            bail!("陶罐装饰包含未知侧面")
        }
        let mut ids = Vec::new();
        for side in SIDES {
            let id = match map.get(side) {
                Some(Value::Compound(item)) if item.len() == 1 => match item.get("id") {
                    Some(Value::String(id)) => id.clone(),
                    _ => bail!("陶罐装饰物品名称无效"),
                },
                None => "minecraft:brick".into(),
                _ => bail!("目标旧版不能无损容纳带组件的陶罐装饰"),
            };
            if !target_has_item(target, &id)? {
                bail!("目标版本没有陶罐装饰物品 {id}")
            }
            ids.push(Value::String(id));
        }
        *value = Value::List(ids);
    }
    Ok(())
}

/// Save/load fields checked in the actual Sign/Banner/DecoratedPot,
/// BrewingStand/Campfire/Lectern/Beacon/Skull codecs. Stable data stays intact.
fn migrate_decorative(
    fields: &mut Fields,
    id: &str,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    let extra: &[&str] = match id {
        "minecraft:banner" => &["patterns", "Patterns", "Base"],
        "minecraft:sign" | "minecraft:hanging_sign" => {
            &["front_text", "back_text", "is_waxed", "allow_op_features", "Text1", "Text2", "Text3", "Text4", "FilteredText1", "FilteredText2", "FilteredText3", "FilteredText4", "Color", "GlowingText"]
        }
        "minecraft:decorated_pot" => &["sherds", "item", "LootTable", "LootTableSeed"],
        "minecraft:brewing_stand" => &[
            "Items",
            "BrewTime",
            "Fuel",
            "total_brew_time",
            "total_fuel",
            "speed_multiplier",
        ],
        "minecraft:campfire" => &["Items", "CookingTimes", "CookingTotalTimes"],
        "minecraft:lectern" => &["Book", "Page"],
        "minecraft:beacon" => &["Levels", "primary_effect", "secondary_effect", "Primary", "Secondary"],
        "minecraft:skull" => &["profile", "SkullOwner", "ExtraType", "note_block_sound", "custom_name"],
        "minecraft:chiseled_bookshelf" => &["Items", "last_interacted_slot"],
        "minecraft:daylight_detector" | "minecraft:bell" => &[],
        "minecraft:bed" => &["color"],
        "minecraft:beehive" => &["Bees", "bees", "FlowerPos", "flower_pos"],
        "minecraft:mob_spawner"=>&["EntityId","SpawnData","SpawnPotentials","Delay","MinSpawnDelay","MaxSpawnDelay","SpawnCount","MaxNearbyEntities","RequiredPlayerRange","SpawnRange"],
        _ => bail!("转换器尚未实现该类方块实体迁移，原数据保留"),
    };
    if (id == "minecraft:hanging_sign" && target < 3437) || (id == "minecraft:decorated_pot" && target < 3437) {
        bail!("目标版本没有{id}对应的方块实体，原文件保留")
    }
    for key in fields.keys() {
        if !["id", "x", "y", "z", "CustomName", "components"].contains(&key.as_str())
            && !extra.contains(&key.as_str())
        {
            bail!("转换器尚未实现字段 {key} 的迁移，原数据保留")
        }
    }
    for coordinate in ["x", "y", "z"] {
        if !matches!(fields.get(coordinate), Some(Value::Int(_))) {
            bail!("方块实体坐标数据损坏")
        }
    }
    if let Some(name) = fields.get_mut("CustomName") {
        *name = migrate_component(name, source, target)?;
    }
    if let Some(components) = fields.get_mut("components") {
        migrate_components(components, source, target, report)?;
    }
    match id {
        "minecraft:bed" => {
            if let Some(color)=fields.get("color"){let color=match color{Value::Byte(n)=>i32::from(*n),Value::Short(n)=>i32::from(*n),Value::Int(n)=>*n,_=>bail!("床color类型无效")};if !(0..=15).contains(&color){bail!("床color超出染料注册表")};if target<1125{if color!=14{bail!("目标1.12以前只有红床")};fields.remove("color");}else if target>=1519{bail!("床颜色必须先按位置迁移到实际方块调色板")}}
            if target<1125||target>=4885{if fields.keys().any(|key|!["id","x","y","z"].contains(&key.as_str())&&!(key=="components"&&matches!(fields.get(key),Some(Value::Compound(fields))if fields.is_empty()))){bail!("目标版本没有床方块实体，不能等价保存自定义床载荷")}}
        }
        "minecraft:skull" => {
            if fields.contains_key("SkullOwner")&&fields.contains_key("ExtraType"){bail!("头颅同时携带两种旧玩家资料，拒绝覆盖")}
            if let Some(name)=fields.get_mut("custom_name"){if target<3837{bail!("目标1.20.5以前没有头颅custom_name持久字段")};*name=migrate_component(name,source,target)?;}
            let from = if source < 3820 { "SkullOwner" } else { "profile" };
            let to = if target < 3820 { "SkullOwner" } else { "profile" };
            let owner = fields.remove(from).or_else(|| if source < 3820 { fields.remove("ExtraType") } else { None });
            if let Some(mut owner) = owner { migrate_profile(&mut owner, source, target, 3820)?; fields.insert(to.into(), owner); }
            if fields.contains_key("note_block_sound") && target < 3218 { bail!("目标1.19.3以前没有头颅自定义音符盒音色字段") }
        }
        "minecraft:banner" => {
            let source_key = if source < 3818 { "Patterns" } else { "patterns" };
            let target_key = if target < 3818 { "Patterns" } else { "patterns" };
            if let Some(mut patterns) = fields.remove(source_key) {
                migrate_banner_patterns(&mut patterns, source, target)?;
                fields.insert(target_key.into(), patterns);
            }
            if let Some(base) = fields.get_mut("Base") {
                let Value::Int(color @ 0..=15) = base else { bail!("旗帜Base不是有效染料编号") };
                if (source < 1451) != (target < 1451) { *color = 15 - *color; }
            }
        }
        "minecraft:sign" | "minecraft:hanging_sign" => {
            migrate_sign_text(fields, source, target)?;
            if source < 5002 && target >= 5002 {
                fields.insert("allow_op_features".into(), Value::Byte(1));
            }
            if source >= 5002 && target < 5002 {
                if fields
                    .get("allow_op_features")
                    .is_some_and(|value| value != &Value::Byte(1))
                {
                    bail!("目标版本不能保留禁用命令功能的告示牌设置")
                }
                fields.remove("allow_op_features");
            }
        }
        "minecraft:decorated_pot" => {
            if let Some(sherds) = fields.get_mut("sherds") {
                migrate_pot_decorations(sherds, source, target)?;
            }
            if let Some(item) = fields.get_mut("item") {
                item_payload(item, source, target, report)?;
            }
        }
        "minecraft:brewing_stand" | "minecraft:campfire" => {
            if let Some(Value::List(items)) = fields.get_mut("Items") {
                let mut slots = HashSet::new();
                for item in items {
                    migrate_stack(
                        item,
                        source,
                        target,
                        if id == "minecraft:campfire" { 4 } else { 5 },
                        &mut slots,
                        "容器物品",
                        report,
                    )?;
                }
            }
            // Both target getInt codecs accept Short/Byte numerics; keep the
            // value and representation rather than reset brew state.
        }
        "minecraft:lectern" => {
            if let Some(book) = fields.get_mut("Book") {
                item_payload(book, source, target, report)?;
            }
        }
        "minecraft:chiseled_bookshelf" => {
            if !crate::block_database::database()?.registry_by_data_version(target)?.blocks.contains_key(id) { bail!("目标版本没有雕纹书架") }
            if !matches!(fields.get("last_interacted_slot"), None | Some(Value::Int(-1..=5))) { bail!("雕纹书架最近交互槽位无效") }
            if let Some(Value::List(items)) = fields.get_mut("Items") { let mut seen = HashSet::new(); for item in items { migrate_stack(item, source, target, 6, &mut seen, "雕纹书架", report)?; } }
        }
        "minecraft:beehive" => migrate_bees(fields, source, target, report)?,
        "minecraft:mob_spawner"=>migrate_spawner(fields,source,target,report)?,
        "minecraft:beacon"=>{for(old,new)in[("Primary","primary_effect"),("Secondary","secondary_effect")]{let from=if source<3568{old}else{new};let to=if target<3568{old}else{new};if let Some(value)=fields.remove(from){if source<3568&&matches!(value,Value::Int(0)|Value::Byte(0)){continue};let value=effect_id(&value,source,target)?;fields.insert(to.into(),match value{Value::Byte(value)=>Value::Int(i32::from(value)),value=>value});}}}

        _ => {}
    }
    let output_id = if target < 704 {
        match id { "minecraft:banner" => "Banner", "minecraft:sign" => "Sign", "minecraft:brewing_stand" => "Cauldron", "minecraft:beacon" => "Beacon", "minecraft:skull" => "Skull", "minecraft:mob_spawner"=>"MobSpawner", "minecraft:daylight_detector"=>"DLDetector", _ => id }
    } else { id };
    fields.insert("id".into(), Value::String(output_id.into()));
    report.changes.push(format!("{id}：保留内容并迁移目标格式"));
    Ok(())
}

/// Applicable official fixes: BoatSplit(v4067), AttributeIdPrefix(v4055),
/// DropChances(v4293), Equipment(v4301), FallDistance(v4303), InlinePos(v4314).
const MODIFIER_UUIDS: &[(&str, &str)] = &[
    ("736565d2-e1a7-403d-a3f8-1aeb3e302542", "minecraft:creative_mode_block_range"),
    ("98491ef6-97b1-4584-ae82-71a8cc85cf73", "minecraft:creative_mode_entity_range"),
    ("91aeaa56-376b-4498-935b-2f7f68070635", "minecraft:effect.speed"),
    ("7107de5e-7ce8-4030-940e-514c1f160890", "minecraft:effect.slowness"),
    ("af8b6e3f-3328-4c0a-aa36-5ba2bb9dbef3", "minecraft:effect.haste"),
    ("55fced67-e92a-486e-9800-b47f202c4386", "minecraft:effect.mining_fatigue"),
    ("648d7064-6a60-4f59-8abe-c2c23a6dd7a9", "minecraft:effect.strength"),
    ("c0105bf3-aef8-46b0-9ebc-92943757ccbe", "minecraft:effect.jump_boost"),
    ("22653b89-116e-49dc-9b6b-9971489b5be5", "minecraft:effect.weakness"),
    ("5d6f0ba2-1186-46ac-b896-c61c5cee99cc", "minecraft:effect.health_boost"),
    ("eae29cf0-701e-4ed6-883a-96f798f3dab5", "minecraft:effect.absorption"),
    ("03c3c89d-7037-4b42-869f-b146bcb64d2e", "minecraft:effect.luck"),
    ("cc5af142-2bd2-4215-b636-2605aed11727", "minecraft:effect.unluck"),
    ("6555be74-63b3-41f1-a245-77833b3c2562", "minecraft:evil"),
    ("1eaf83ff-7207-4596-b37a-d7a07b3ec4ce", "minecraft:powder_snow"),
    ("662a6b8d-da3e-4c1c-8813-96ea6097278d", "minecraft:sprinting"),
    ("020e0dfb-87ae-4653-9556-831010e291a0", "minecraft:attacking"),
    ("766bfa64-11f3-11ea-8d71-362b9e155667", "minecraft:baby"),
    ("7e0292f2-9434-48d5-a29f-9583af7df27f", "minecraft:covered"),
    ("9e362924-01de-4ddd-a2b2-d0f7a405a174", "minecraft:suffocating"),
    ("5cd17e52-a79a-43d3-a529-90fde04b181e", "minecraft:drinking"),
    ("b9766b59-9566-4402-bc1f-2ee2a276d836", "minecraft:baby"),
    ("49455a49-7ec5-45ba-b886-3b90b23a1718", "minecraft:attacking"),
    ("845db27c-c624-495f-8c9f-6020a9a58b6b", "minecraft:armor.boots"),
    ("d8499b04-0e66-4726-ab29-64469d734e0d", "minecraft:armor.leggings"),
    ("9f3d476d-c118-4544-8365-64846904b48e", "minecraft:armor.chestplate"),
    ("2ad3f246-fee1-4e67-b886-69fd380bb150", "minecraft:armor.helmet"),
    ("c1c72771-8b8e-ba4a-ace0-81a93c8928b2", "minecraft:armor.body"),
    ("b572ecd2-ac0c-4071-abde-9594af072a37", "minecraft:enchantment.fire_protection"),
    ("40a9968f-5c66-4e2f-b7f4-2ec2f4b3e450", "minecraft:enchantment.blast_protection"),
    ("07a65791-f64d-4e79-86c7-f83932f007ec", "minecraft:enchantment.respiration"),
    ("60b1b7db-fffd-4ad0-817c-d6c6a93d8a45", "minecraft:enchantment.aqua_affinity"),
    ("11dc269a-4476-46c0-aff3-9e17d7eb6801", "minecraft:enchantment.depth_strider"),
    ("87f46a96-686f-4796-b035-22e16ee9e038", "minecraft:enchantment.soul_speed"),
    ("b9716dbd-50df-4080-850e-70347d24e687", "minecraft:enchantment.soul_speed"),
    ("92437d00-c3a7-4f2e-8f6c-1f21585d5dd0", "minecraft:enchantment.swift_sneak"),
    ("5d3d087b-debe-4037-b53e-d84f3ff51f17", "minecraft:enchantment.sweeping_edge"),
    ("3ceb37c0-db62-46b5-bd02-785457b01d96", "minecraft:enchantment.efficiency"),
    ("cb3f55d3-645c-4f38-a497-9c13a33db5cf", "minecraft:base_attack_damage"),
    ("fa233e1c-4180-4865-b01b-bcce9785aca3", "minecraft:base_attack_speed"),
];

// ItemStackComponentizationFix.fixProfile (3818), PlayerHeadBlockProfileFix
// (3820), and BlockEntityUUIDFix (2514). Do not discard invalid names/properties
// as vanilla's permissive forward fixer does: conversion must preserve data.
fn migrate_profile(value: &mut Value, source: i32, target: i32, boundary: i32) -> Result<()> {
    if source < boundary && target >= boundary {
        let mut old = match std::mem::replace(value, Value::Compound(Fields::new())) {
            Value::String(name) => Fields::from([("Name".into(), Value::String(name))]),
            Value::Compound(fields) => fields,
            _ => bail!("头颅玩家资料格式无效"),
        };
        if old.keys().any(|key| !["Name", "Id", "Properties"].contains(&key.as_str())) { bail!("头颅资料含未知字段") }
        let mut profile = Fields::new();
        if let Some(name) = old.remove("Name") {
            if !matches!(&name, Value::String(name) if name.len() <= 16 && name.bytes().all(|c| c > 32 && c < 127)) { bail!("头颅玩家名称超出实际profile编码范围") }
            profile.insert("name".into(), name);
        }
        if let Some(mut id) = old.remove("Id") {
            if source < 2514 { let Value::String(text) = id else { bail!("旧头颅UUID不是String") }; id = uuid_value(&text)?; }
            uuid_text(&id)?;
            profile.insert("id".into(), id);
        }
        if old.get("Properties").is_some_and(|value|!matches!(value,Value::Compound(_))){bail!("头颅Properties不是Compound")}
        if let Some(Value::Compound(properties)) = old.remove("Properties") {
            let mut output = Vec::new();
            let mut properties = properties.into_iter().collect::<Vec<_>>(); properties.sort_by(|a,b| a.0.cmp(&b.0));
            for (name, entries) in properties {
                let Value::List(entries) = entries else { bail!("头颅属性不是List") };
                for entry in entries {
                    let Value::Compound(mut entry) = entry else { bail!("头颅属性条目格式无效") };
                    if entry.keys().any(|key| key != "Value" && key != "Signature") { bail!("头颅属性含未知字段") }
                    let property = entry.remove("Value").context("头颅属性缺少Value")?;
                    if !matches!(property, Value::String(_)) { bail!("头颅属性Value不是String") }
                    let mut mapped = Fields::from([("name".into(), Value::String(name.clone())), ("value".into(), property)]);
                    if let Some(signature) = entry.remove("Signature") { if !matches!(signature, Value::String(_)) { bail!("头颅签名不是String") } mapped.insert("signature".into(), signature); }
                    output.push(Value::Compound(mapped));
                }
            }
            profile.insert("properties".into(), Value::List(output));
        }
        *value = Value::Compound(profile);
    } else if source >= boundary && target < boundary {
        let profile = object(value)?;
        if profile.keys().any(|key| !["name", "id", "properties"].contains(&key.as_str())) { bail!("目标旧版不能保存头颅资料扩展字段") }
        let mut old = Fields::new();
        if let Some(name) = profile.remove("name") { old.insert("Name".into(), name); }
        if let Some(id) = profile.remove("id") { let text = uuid_text(&id)?; old.insert("Id".into(), if target < 2514 { Value::String(text) } else { id }); }
        if let Some(properties) = profile.remove("properties") {
            let Value::List(properties) = properties else { bail!("头颅properties不是List") };
            let mut output = Fields::new();
            for property in properties {
                let Value::Compound(mut property) = property else { bail!("头颅属性不是Compound") };
                if property.keys().any(|key| !["name", "value", "signature"].contains(&key.as_str())) { bail!("目标旧版不能保存头颅属性扩展") }
                let Some(Value::String(name)) = property.remove("name") else { bail!("头颅属性缺少name") };
                let value = property.remove("value").context("头颅属性缺少value")?;
                if !matches!(value, Value::String(_)) { bail!("头颅属性value不是String") }
                let mut entry = Fields::from([("Value".into(), value)]);
                if let Some(signature) = property.remove("signature") { if !matches!(signature, Value::String(_)) { bail!("头颅签名不是String") } entry.insert("Signature".into(), signature); }
                let Value::List(entries) = output.entry(name).or_insert(Value::List(Vec::new())) else { unreachable!() };
                entries.push(Value::Compound(entry));
            }
            old.insert("Properties".into(), Value::Compound(output));
        }
        *value = Value::Compound(old);
    } else if source < boundary && target < boundary && (source < 2514) != (target < 2514) {
        if let Value::Compound(profile) = value { if let Some(id) = profile.get_mut("Id") { *id = if target >= 2514 { let Value::String(text) = id else { bail!("旧头颅UUID不是String") }; uuid_value(text)? } else { Value::String(uuid_text(id)?) }; } }
    }
    Ok(())
}

fn migrate_entity_uuid(fields: &mut Fields, source: i32, target: i32) -> Result<()> {
    if source < 2514 && target >= 2514 {
        if fields.contains_key("UUIDMost") || fields.contains_key("UUIDLeast") {
            let Some(Value::Long(most)) = fields.remove("UUIDMost") else { bail!("实体UUIDMost无效") };
            let Some(Value::Long(least)) = fields.remove("UUIDLeast") else { bail!("实体UUIDLeast无效") };
            fields.insert("UUID".into(), Value::IntArray(fastnbt::IntArray::new(vec![(most >> 32) as i32, most as i32, (least >> 32) as i32, least as i32])));
        }
    } else if source >= 2514 && target < 2514 {
        if let Some(value) = fields.remove("UUID") {
            let Value::IntArray(parts) = value else { bail!("实体UUID不是IntArray") };
            if parts.len() != 4 { bail!("实体UUID应有四个Int") }
            fields.insert("UUIDMost".into(), Value::Long((i64::from(parts[0]) << 32) | i64::from(parts[1] as u32)));
            fields.insert("UUIDLeast".into(), Value::Long((i64::from(parts[2]) << 32) | i64::from(parts[3] as u32)));
        }
    }
    if (source < 2514) != (target < 2514) {
        if fields.contains_key("LoveCause") || fields.contains_key("LoveCauseMost") || fields.contains_key("LoveCauseLeast") {
            let mut cause=Fields::new();
            for key in ["LoveCause","LoveCauseMost","LoveCauseLeast"] { if let Some(value)=fields.remove(key) { cause.insert(key.replacen("LoveCause","UUID",1),value); } }
            migrate_entity_uuid(&mut cause,source,target)?;
            for (key,value) in cause { fields.insert(key.replacen("UUID","LoveCause",1),value); }
        }
        if matches!(fields.get("id"),Some(Value::String(id)) if ["minecraft:horse","minecraft:parrot"].contains(&id.as_str())) {
            let (from,to)=if target>=2514{("OwnerUUID","Owner")}else{("Owner","OwnerUUID")};
            if let Some(value)=fields.remove(from) { let value=if target>=2514{let Value::String(text)=value else{bail!("动物OwnerUUID不是String")};uuid_value(&text)?}else{Value::String(uuid_text(&value)?)};fields.insert(to.into(),value); }
        }
        if matches!(fields.get("id"),Some(Value::String(id)) if id=="minecraft:item") {
            for key in ["Owner","Thrower"] { if let Some(value)=fields.get_mut(key) {
                if target>=2514 { let old=object(value)?;if old.len()!=2{bail!("掉落物{key} UUID字段无效")};let(Some(Value::Long(most)),Some(Value::Long(least)))=(old.get("M"),old.get("L"))else{bail!("掉落物{key} UUID缺少M/L")};*value=Value::IntArray(fastnbt::IntArray::new(vec![(most>>32)as i32,*most as i32,(least>>32)as i32,*least as i32])); }
                else { let Value::IntArray(parts)=value else{bail!("掉落物{key}不是UUID IntArray")};if parts.len()!=4{bail!("掉落物{key} UUID长度错误")};*value=Value::Compound([("M".into(),Value::Long((i64::from(parts[0])<<32)|i64::from(parts[1]as u32))),("L".into(),Value::Long((i64::from(parts[2])<<32)|i64::from(parts[3]as u32)))].into()); }
            }}
        }
    }
    Ok(())
}

fn uuid_text(value: &Value) -> Result<String> {
    let Value::IntArray(parts) = value else { bail!("属性修饰器UUID不是IntArray") };
    if parts.len() != 4 { bail!("属性修饰器UUID长度错误") }
    let hex = parts.iter().map(|part| format!("{:08x}", *part as u32)).collect::<String>();
    Ok(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

fn uuid_value(text: &str) -> Result<Value> {
    let hex = text.replace('-', "");
    if hex.len() != 32 || !hex.is_ascii() { bail!("目标旧版不能无损恢复属性修饰器的UUID {text}") }
    Ok(Value::IntArray(fastnbt::IntArray::new((0..4).map(|i| u32::from_str_radix(&hex[i*8..i*8+8], 16).map(|n| n as i32)).collect::<std::result::Result<_, _>>()?)))
}

fn migrate_entity_attributes(fields: &mut Fields, source: i32, target: i32) -> Result<()> {
    let from = if source < 3945 { "Attributes" } else { "attributes" };
    let to = if target < 3945 { "Attributes" } else { "attributes" };
    let Some(mut attributes) = fields.remove(from) else { return Ok(()) };
    let Value::List(entries) = &mut attributes else { bail!("实体属性不是列表") };
    const NAMES: &[(&str, &str)] = &[("generic.maxHealth", "minecraft:generic.max_health"), ("generic.followRange", "minecraft:generic.follow_range"), ("generic.knockbackResistance", "minecraft:generic.knockback_resistance"), ("generic.movementSpeed", "minecraft:generic.movement_speed"), ("generic.flyingSpeed", "minecraft:generic.flying_speed"), ("generic.attackDamage", "minecraft:generic.attack_damage"), ("generic.attackKnockback", "minecraft:generic.attack_knockback"), ("generic.attackSpeed", "minecraft:generic.attack_speed"), ("generic.armor", "minecraft:generic.armor"), ("generic.armorToughness", "minecraft:generic.armor_toughness"), ("generic.luck", "minecraft:generic.luck"), ("horse.jumpStrength", "minecraft:horse.jump_strength"), ("zombie.spawnReinforcements", "minecraft:zombie.spawn_reinforcements")];
    for entry in entries {
        let map = object(entry)?;
        if (source < 3945) != (target < 3945) {
            for (old, new) in [("Name", "id"), ("Base", "base"), ("Modifiers", "modifiers")] {
                let (old, new) = if target >= 3945 { (old, new) } else { (new, old) };
                if let Some(value) = map.remove(old) { map.insert(new.into(), value); }
            }
        }
        let name_key = if target < 3945 { "Name" } else { "id" };
        if let Some(Value::String(id)) = map.get_mut(name_key) {
            // Normalize each historical name in chronological order, then encode
            // the target in reverse order. Cross-many-node conversion must not
            // apply 4055 before 2523 and leave a newly prefixed old attribute.
            if source<2523 { if let Some((_,new))=NAMES.iter().find(|(old,_)|*old==id){*id=(*new).into();} }
            if source<3814&&id=="minecraft:horse.jump_strength"{*id="minecraft:generic.jump_strength".into();}
            if source<4055 {for prefix in ["minecraft:generic.","minecraft:horse.","minecraft:player.","minecraft:zombie."]{if let Some(tail)=id.strip_prefix(prefix){*id=format!("minecraft:{tail}");break;}}}
            if target<4055 {let tail=id.strip_prefix("minecraft:").context("非原版属性名称没有目标映射")?;let prefix=match tail{"jump_strength" if target<3814=>"horse","block_break_speed"|"block_interaction_range"|"entity_interaction_range"=>"player","spawn_reinforcements"=>"zombie",_=>"generic"};*id=format!("minecraft:{prefix}.{tail}");}
            if target<2523 {if let Some((old,_))=NAMES.iter().find(|(_,new)|*new==id){*id=(*old).into();}else{bail!("目标旧版没有属性映射{id}")}}
        }
        let key = if target < 3945 { "Modifiers" } else { "modifiers" };
        if let Some(Value::List(modifiers)) = map.get_mut(key) { for modifier in modifiers {
            let m = object(modifier)?;
            if source < 2514 && target >= 2514 { migrate_entity_uuid(m, source, target)?; }
            if source < 3945 && target >= 3945 {
                let uuid = uuid_text(m.get("UUID").context("属性修饰器缺少UUID")?)?;
                let name = match m.get("Name") { Some(Value::String(name)) => name.as_str(), _ => "" };
                let id = MODIFIER_UUIDS.iter().find(|(known, _)| *known == uuid).map(|(_, id)| (*id).to_owned())
                    .or_else(|| match name { "Random spawn bonus" => Some("minecraft:random_spawn_bonus".into()), "Random zombie-spawn bonus" => Some("minecraft:zombie_random_spawn_bonus".into()), "Leader zombie bonus" => Some("minecraft:leader_zombie_bonus".into()), "Zombie reinforcement callee charge" => Some("minecraft:reinforcement_callee_charge".into()), "Zombie reinforcement caller charge" => Some("minecraft:reinforcement_caller_charge".into()), _ => None }).unwrap_or(format!("minecraft:{uuid}"));
                m.remove("UUID"); m.remove("Name"); m.insert("id".into(), Value::String(id));
                if let Some(value) = m.remove("Amount") { m.insert("amount".into(), value); }
                let operation = match m.remove("Operation") { Some(Value::Int(0)) => "add_value", Some(Value::Int(1)) => "add_multiplied_base", Some(Value::Int(2)) => "add_multiplied_total", _ => bail!("属性修饰器Operation无效") };
                m.insert("operation".into(), Value::String(operation.into()));
            } else if source >= 3945 && target < 3945 {
                let Some(Value::String(id)) = m.remove("id") else { bail!("属性修饰器id缺失") };
                let uuid = MODIFIER_UUIDS.iter().find(|(_, known)| *known == id).map(|(uuid, _)| *uuid).unwrap_or_else(|| id.strip_prefix("minecraft:").unwrap_or(&id));
                m.insert("UUID".into(), uuid_value(uuid)?);
                m.insert("Name".into(), Value::String(id));
                if let Some(value) = m.remove("amount") { m.insert("Amount".into(), value); }
                let operation = match m.remove("operation") { Some(Value::String(op)) => match op.as_str() { "add_value" => 0, "add_multiplied_base" => 1, "add_multiplied_total" => 2, _ => bail!("属性修饰器operation无效") }, _ => bail!("属性修饰器operation缺失") };
                m.insert("Operation".into(), Value::Int(operation));
            }
            if source >= 2514 && target < 2514 { migrate_entity_uuid(m, source, target)?; }
        }}
    }
    fields.insert(to.into(), attributes);
    Ok(())
}

fn migrate_villager(fields: &mut Fields, source: i32, target: i32, report: &mut PayloadMigrationReport) -> Result<()> {
    const CAREERS: &[(i32, i32, &str)] = &[(0,1,"farmer"),(0,2,"fisherman"),(0,3,"shepherd"),(0,4,"fletcher"),(1,1,"librarian"),(1,2,"cartographer"),(2,1,"cleric"),(3,1,"armorer"),(3,2,"weaponsmith"),(3,3,"toolsmith"),(4,1,"butcher"),(4,2,"leatherworker"),(5,1,"nitwit")];
    if source < 1918 && target >= 1918 {
        let profession = match fields.remove("Profession") { Some(Value::Int(value)) => value, None => 0, _ => bail!("村民Profession类型无效") };
        let career = match fields.remove("Career") { Some(Value::Int(value)) => value, None => 1, _ => bail!("村民Career类型无效") };
        let job = CAREERS.iter().find(|(p,c,_)| *p == profession && *c == career).map(|(_,_,name)| *name).context("村民旧职业没有原版映射")?;
        let level = fields.remove("CareerLevel").unwrap_or(Value::Int(1));
        fields.insert("VillagerData".into(), Value::Compound([("type".into(), Value::String("minecraft:plains".into())), ("profession".into(), Value::String(format!("minecraft:{job}"))), ("level".into(), level)].into()));
    } else if source >= 1918 && target < 1918 {
        let Some(Value::Compound(mut data)) = fields.remove("VillagerData") else { bail!("村民缺少VillagerData") };
        if data.remove("type") != Some(Value::String("minecraft:plains".into())) { bail!("目标旧版不能保存村民生物群系外观") }
        let Some(Value::String(job)) = data.remove("profession") else { bail!("村民缺少profession") };
        let (profession, career, _) = CAREERS.iter().find(|(_,_,name)| job == format!("minecraft:{name}")).context("目标旧版没有该村民职业")?;
        fields.insert("Profession".into(), Value::Int(*profession)); fields.insert("Career".into(), Value::Int(*career));
        fields.insert("CareerLevel".into(), data.remove("level").unwrap_or(Value::Int(1)));
        if !data.is_empty() { bail!("村民职业数据有未知字段") }
        for key in ["Xp", "LastRestock", "LastGossipDecay", "RestocksToday"] { if let Some(value) = fields.remove(key) { if !matches!(value, Value::Int(0) | Value::Long(0)) { bail!("目标旧版不能保存村民{key}状态") } } }
    }
    if let Some(Value::List(inventory)) = fields.get_mut("Inventory") { for item in inventory { item_payload(item, source, target, report)?; } }
    if let Some(Value::Compound(offers)) = fields.get_mut("Offers") {
        if offers.keys().any(|key| key != "Recipes") { bail!("村民交易有未知字段") }
        if let Some(Value::List(recipes)) = offers.get_mut("Recipes") { for recipe in recipes {
            let recipe = object(recipe)?;
            if recipe.keys().any(|key| !["buy", "buyB", "sell", "uses", "maxUses", "rewardExp", "specialPrice", "demand", "priceMultiplier", "xp"].contains(&key.as_str())) { bail!("村民交易条目有未知字段") }
            for key in ["buy", "buyB", "sell"] { if let Some(item) = recipe.get_mut(key) { item_payload(item, source, target, report)?; } }
            if target < 1918 { for key in ["specialPrice", "demand", "priceMultiplier", "xp"] { if let Some(value) = recipe.remove(key) { if !matches!(value, Value::Int(0) | Value::Float(0.0)) { bail!("目标旧版不能保存村民交易{key}数据") } } } }
        }}
    }
    if let Some(Value::Compound(brain)) = fields.get_mut("Brain") {
        if let Some(Value::Compound(memories)) = brain.get_mut("memories") {
            for memory in memories.values_mut() {
                if source < 2505 && target >= 2505 { *memory = Value::Compound([("value".into(), memory.clone())].into()); }
                else if source >= 2505 && target < 2505 { let map = object(memory)?; if map.keys().any(|key| key != "value") { bail!("目标旧版不能保存村民记忆过期时间") } *memory = map.remove("value").context("村民记忆缺少value")?; }
            }
            if target < 1918 && !memories.is_empty() { bail!("目标旧版没有村民Brain记忆") }
        }
        if target < 1918 { fields.remove("Brain"); }
    }
    if let Some(Value::List(gossips)) = fields.get_mut("Gossips") {
        if target < 1918 && !gossips.is_empty() { bail!("目标旧版没有村民流言与信誉记录") }
        for gossip in gossips {
            let gossip = object(gossip)?;
            for (key, _) in gossip.clone() { if key == "Target" { continue; } if !["TargetMost", "TargetLeast", "Type", "Value"].contains(&key.as_str()) { bail!("村民流言有未知字段{key}") } }
            if source < 2516 && target >= 2516 {
                let mut uuid = Fields::new(); for (from,to) in [("TargetMost","UUIDMost"),("TargetLeast","UUIDLeast")] { if let Some(value)=gossip.remove(from){uuid.insert(to.into(),value);} }
                migrate_entity_uuid(&mut uuid, 0, 2514)?; if let Some(value)=uuid.remove("UUID"){gossip.insert("Target".into(),value);}
            } else if source >= 2516 && target < 2516 {
                let mut uuid = Fields::new();if let Some(value)=gossip.remove("Target"){uuid.insert("UUID".into(),value);} migrate_entity_uuid(&mut uuid,2514,0)?;
                for (from,to) in [("UUIDMost","TargetMost"),("UUIDLeast","TargetLeast")]{if let Some(value)=uuid.remove(from){gossip.insert(to.into(),value);}}
            }
        }
    }
    if source < 4763 && target >= 4763 { fields.insert("VillagerDataFinalized".into(),Value::Byte(1)); }
    else if source >= 4763 && target < 4763 { if fields.remove("VillagerDataFinalized").is_some_and(|value| value != Value::Byte(1)) { bail!("目标旧版不能保存未定型的村民职业") } }
    Ok(())
}

fn migrate_special_equipment(fields: &mut Fields, id: &str, source: i32, target: i32, before: bool, report: &mut PayloadMigrationReport) -> Result<()> {
    let horse = id == "minecraft:horse";
    if id != "minecraft:pig" && !horse { return Ok(()) }
    if before && source >=4300 && target <4300 {
        let saddle = if source >=4301 { match fields.get_mut("equipment") { Some(Value::Compound(map))=>map.remove("saddle"),_=>None } } else { fields.remove("saddle") };
        if let Some(mut saddle)=saddle {
            item_payload(&mut saddle,source,target,report)?;
            if horse { fields.insert("SaddleItem".into(),saddle); } else {
                let map=object(&mut saddle)?;
                if map.len()!=2 || map.get("id")!=Some(&Value::String("minecraft:saddle".into())) || map.get(if target<3818{"Count"}else{"count"})!=Some(&if target<3818{Value::Byte(1)}else{Value::Int(1)}) { bail!("目标旧版猪鞍标志不能保存鞍物品的组件或额外数量") }
                fields.insert("Saddle".into(),Value::Byte(1));
            }
            if let Some(Value::Compound(chances))=fields.get_mut("drop_chances"){if chances.remove("saddle").is_some_and(|value|value!=Value::Float(2.0)){bail!("目标旧版不能保存自定义鞍掉落概率")}}
        }else if !horse{fields.insert("Saddle".into(),Value::Byte(0));}
    }else if !before && source<4300 && target>=4300 {
        let mut saddle=fields.remove("SaddleItem");
        if let Some(flag)=fields.remove("Saddle") { if flag!=Value::Byte(0) && flag!=Value::Byte(1){bail!("鞍标志无效")};if flag==Value::Byte(1){saddle=Some(Value::Compound([("id".into(),Value::String("minecraft:saddle".into())),(if source<3818{"Count"}else{"count"}.into(),if source<3818{Value::Byte(1)}else{Value::Int(1)})].into()));} }
        if let Some(mut saddle)=saddle{item_payload(&mut saddle,source,target,report)?;
            if target>=4301{let equipment=fields.entry("equipment".into()).or_insert(Value::Compound(Fields::new()));object(equipment)?.insert("saddle".into(),saddle);}else{fields.insert("saddle".into(),saddle);}
            let chances=fields.entry("drop_chances".into()).or_insert(Value::Compound(Fields::new()));object(chances)?.insert("saddle".into(),Value::Float(2.0));
        }
    }
    if horse && ((source<3808)!=(target<3808)||(source<4301)!=(target<4301)) && ((target<4301)==before) {
        let armor=if source>=4301{match fields.get_mut("equipment"){Some(Value::Compound(map))=>map.remove("body"),_=>None}}else{fields.remove(if source<3808{"ArmorItem"}else{"body_armor_item"})};
        if let Some(mut armor)=armor{item_payload(&mut armor,source,target,report)?;if target>=4301{let equipment=fields.entry("equipment".into()).or_insert(Value::Compound(Fields::new()));object(equipment)?.insert("body".into(),armor);}else{fields.insert(if target<3808{"ArmorItem"}else{"body_armor_item"}.into(),armor);}}
    }
    Ok(())
}

fn migrate_equipment_100(fields: &mut Fields, source: i32, target: i32, before: bool) -> Result<()> {
    for (old, armor, hand, empty) in [("Equipment","ArmorItems","HandItems",Value::Compound(Fields::new())),("DropChances","ArmorDropChances","HandDropChances",Value::Float(0.0))] {
        if source < 100 && target >= 100 && before {
            if let Some(Value::List(items))=fields.remove(old){if items.len()>5{bail!("1.8装备超过五槽")};if !items.is_empty(){fields.insert(hand.into(),Value::List(vec![items[0].clone(),empty.clone()]));}if items.len()>1{let mut output=vec![empty.clone();4];for(i,item)in items.into_iter().skip(1).enumerate(){output[i]=item;}fields.insert(armor.into(),Value::List(output));}}
        } else if source >=100 && target <100 && !before {
            let hands=match fields.remove(hand){Some(Value::List(items))=>items,None=>vec![],_=>bail!("装备手部数据无效")};
            if hands.get(1).is_some_and(|value|value!=&empty){bail!("目标1.8没有副手装备/掉落概率")}
            let mut output=vec![hands.first().cloned().unwrap_or_else(||empty.clone())];
            match fields.remove(armor){Some(Value::List(items))if items.len()<=4=>output.extend(items),None=>output.extend(vec![empty;4]),_=>bail!("装备护甲数据无效")}
            fields.insert(old.into(),Value::List(output));
        }
    }Ok(())
}

// EntityRidingToPassengersFix v135 reverses the serialized chain root. A
// linear passenger chain is exactly reversible; several simultaneous riders
// have no pre-1.9 representation and remain an explicit non-equivalence.
fn migrate_entity(entity:&mut Value,source:i32,target:i32,report:&mut PayloadMigrationReport)->Result<()> {
    if source<135 {
        let mut node=std::mem::replace(entity,Value::Compound(Fields::new()));
        if object(&mut node)?.contains_key("Passengers"){bail!("来源1.8实体不应携带Passengers格式")}
        for depth in 0..=128 {
            let fields=object(&mut node)?;
            let Some(mut mount)=fields.remove("Riding")else{*entity=node;break};
            if depth==128{bail!("旧Riding链超过128层")}
            // Keep the already normalized rider subtree while advancing to the
            // mount. Its own Riding pointer is removed on the next iteration.
            let fields=object(&mut mount)?;if fields.contains_key("Passengers"){bail!("来源1.8实体不应携带Passengers格式")};fields.insert("Passengers".into(),Value::List(vec![node]));
            node=mount;
        }
    }
    migrate_entity_payload(entity,source,target,report)?;
    if target<135 {
        let mut node=std::mem::replace(entity,Value::Compound(Fields::new()));
        let mut mount=None;
        for depth in 0..=128 {
            let fields=object(&mut node)?;
            let passengers=match fields.remove("Passengers"){None=>Vec::new(),Some(Value::List(list))=>list,_=>bail!("Passengers不是List")};
            if passengers.len()>1{bail!("目标1.8的Riding链不能等价保存同一载具的多个乘客")}
            if let Some(mount)=mount{fields.insert("Riding".into(),mount);}
            if let Some(passenger)=passengers.into_iter().next(){if depth==128{bail!("Passengers链超过128层")};mount=Some(node);node=passenger;}else{*entity=node;break}
        }
    }
    Ok(())
}

fn migrate_entity_payload(
    entity: &mut Value,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    let fields = object(entity)?;
    let Some(Value::String(id)) = fields.get("id") else {
        bail!("实体名称缺失")
    };
    let id = match id.as_str() {
        "ArmorStand" => "minecraft:armor_stand", "ItemFrame" => "minecraft:item_frame", "Painting" => "minecraft:painting", "Boat" => "minecraft:boat", "Item" => "minecraft:item", "MinecartRideable" => "minecraft:minecart", "Villager" => "minecraft:villager", "Chicken" => "minecraft:chicken", "Cow" => "minecraft:cow", "Pig" => "minecraft:pig", "Sheep" => "minecraft:sheep", "Rabbit" => "minecraft:rabbit", "Horse" | "EntityHorse" => "minecraft:horse", "Zombie" => "minecraft:zombie", "Skeleton" => "minecraft:skeleton", other => other,
    }.to_owned();
    fields.insert("id".into(), Value::String(id.clone()));
    migrate_entity_uuid(fields, source, target)?;
    migrate_entity_attributes(fields, source, target)?;
    let boat = id == "minecraft:boat"
        || [
            "oak", "spruce", "birch", "jungle", "acacia", "cherry", "dark_oak", "mangrove",
            "bamboo",
        ]
        .iter()
        .any(|wood| {
            id == format!("minecraft:{wood}_boat") || id == format!("minecraft:{wood}_raft")
        });
    let extra: &[&str] = if boat {
        &["Type"]
    } else {
        match id.as_str() {
            "minecraft:armor_stand" => &[
                "Pose",
                "ShowArms",
                "Invisible",
                "NoBasePlate",
                "Small",
                "DisabledSlots",
                "Marker",
            ],
            "minecraft:item_frame" | "minecraft:glow_item_frame" => &[
                "Item",
                "ItemRotation",
                "ItemDropChance",
                "Facing",
                "Invisible",
                "Fixed",
                "TileX",
                "TileY",
                "TileZ",
                "block_pos",
            ],
            "minecraft:painting" => &["variant", "facing", "Motive", "Facing", "TileX", "TileY", "TileZ", "block_pos"],
            "minecraft:minecart" => &["CustomDisplayTile", "DisplayState", "DisplayOffset", "FlippedRotation", "HasTicked"],
            "minecraft:item" => &["Item", "Age", "PickupDelay", "Owner", "Thrower", "Health"],
            "minecraft:bee" => &["HasNectar", "HasStung", "TicksSincePollination", "CannotEnterHiveTicks", "CropsGrownSincePollination", "AngerTime", "AngryAt", "HivePos", "FlowerPos", "hive_pos", "flower_pos"],
            "minecraft:villager" => &["Profession", "Career", "CareerLevel", "VillagerData", "VillagerDataFinalized", "Inventory", "Offers", "Riches", "Willing", "FoodLevel", "Gossips", "Xp", "LastRestock", "LastGossipDecay", "RestocksToday", "SleepingX", "SleepingY", "SleepingZ", "sleeping_pos"],
            "minecraft:chicken" => &["EggLayTime", "IsChickenJockey", "variant"],
            "minecraft:pig" => &["Saddle", "variant", "saddle"],
            "minecraft:cow" => &["variant"],
            "minecraft:sheep" => &["Color", "Sheared"],
            "minecraft:rabbit" => &["RabbitType", "MoreCarrotTicks"],
            "minecraft:horse" => &["Variant", "Tame", "Temper", "Bred", "EatingHaystack", "Owner", "OwnerUUID", "SaddleItem", "ArmorItem", "ArmorItems", "Saddle", "body_armor_item", "body_armor_drop_chance", "saddle"],
            "minecraft:zombie" => &["IsBaby", "CanBreakDoors", "InWaterTime", "DrownedConversionTime"],
            "minecraft:skeleton" => &["StrayConversionTime"],
            "minecraft:tropical_fish" => &["Variant", "FromBucket"],
            "minecraft:pufferfish" => &["PuffState", "FromBucket"],
            "minecraft:frog" => &["variant", "Age", "ForcedAge", "InLove"],
            "minecraft:parrot" => &["Variant", "Age", "ForcedAge", "InLove", "Owner", "OwnerUUID", "Sitting"],
            _ => bail!("实体 {id} 的转换分支尚未实现"),
        }
    };
    const COMMON: &[&str] = &[
        "id",
        "Pos",
        "Motion",
        "Rotation",
        "UUID",
        "UUIDMost", "UUIDLeast", "Passengers",
        "Air",
        "OnGround",
        "Invulnerable",
        "Fire",
        "PortalCooldown",
        "FallDistance",
        "fall_distance",
        "CustomName",
        "CustomNameVisible",
        "Tags",
        "Silent",
        "NoGravity",
        "Glowing",
        "Brain",
        "HurtByTimestamp",
        "FallFlying",
        "AbsorptionAmount",
        "DeathTime",
        "PersistenceRequired",
        "Health",
        "LeftHanded",
        "HurtTime",
        "CanPickUpLoot",
        "attributes",
        "Attributes",
        "ArmorItems",
        "HandItems",
        "equipment",
        "ArmorDropChances",
        "HandDropChances",
        "drop_chances",
        "locator_bar_icon",
        "Age", "ForcedAge", "InLove", "LoveCause", "LoveCauseMost", "LoveCauseLeast",
        "ActiveEffects", "active_effects", "Equipment", "DropChances", "HealF",
        "SleepingX", "SleepingY", "SleepingZ", "sleeping_pos",
    ];
    for key in fields.keys() {
        if !COMMON.contains(&key.as_str()) && !extra.contains(&key.as_str()) {
            bail!("实体 {id} 字段 {key} 的迁移尚未实现")
        }
    }
    if let Some(name) = fields.get_mut("CustomName") {
        *name = migrate_component(name, source, target)?;
    }
    for (entity,item) in [("minecraft:glow_item_frame","minecraft:glow_item_frame"),("minecraft:frog","minecraft:frog_spawn_egg"),("minecraft:tropical_fish","minecraft:tropical_fish_spawn_egg"),("minecraft:pufferfish","minecraft:pufferfish_spawn_egg")] {
        if id==entity { let registry=crate::block_database::database()?.registry_by_data_version(target)?;
            if !registry.items.contains(item) && !(target<1519&&registry.legacy_items.values().any(|value|value==item)) { bail!("目标真实注册表没有实体{id}的对应物品{item}") }
        }
    }
    if id=="minecraft:parrot"&&target<1139{bail!("目标1.12以前没有鹦鹉实体")}
    if source<4314&&target>=4314&&["SleepingX","SleepingY","SleepingZ"].iter().any(|key|fields.contains_key(*key)) {
        let mut coordinates=Vec::new();for key in ["SleepingX","SleepingY","SleepingZ"]{let Some(Value::Int(n))=fields.remove(key)else{bail!("睡眠坐标{key}缺失或无效")};coordinates.push(n);}
        fields.insert("sleeping_pos".into(),Value::IntArray(fastnbt::IntArray::new(coordinates)));
    }else if source>=4314&&target<4314 { if let Some(Value::IntArray(coordinates))=fields.remove("sleeping_pos"){if coordinates.len()!=3{bail!("睡眠坐标应有三个Int")};for(key,value)in["SleepingX","SleepingY","SleepingZ"].into_iter().zip(coordinates.iter()){fields.insert(key.into(),Value::Int(*value));}} }
    if id == "minecraft:bee" {
        if target < 2225 { bail!("目标版本没有蜜蜂") }
        migrate_block_position(fields,"HivePos","hive_pos",source,target)?;
        migrate_block_position(fields,"FlowerPos","flower_pos",source,target)?;
    }
    if id == "minecraft:villager" { migrate_villager(fields, source, target, report)?; }
    let effect_from = if source < 3568 { "ActiveEffects" } else { "active_effects" };
    let effect_to = if target < 3568 { "ActiveEffects" } else { "active_effects" };
    if let Some(mut effects) = fields.remove(effect_from) { migrate_effects(&mut effects, source, target)?; fields.insert(effect_to.into(),effects); }
    if source < 109 && target >=109 {
        if let Some(value) = fields.remove("HealF").or_else(||fields.remove("Health")) {
            let health=match value{Value::Short(value)=>f32::from(value),Value::Float(value)=>value,_=>bail!("旧Health类型无效")};fields.insert("Health".into(),Value::Float(health));
        }
    }else if source>=109&&target<109{if let Some(value)=fields.remove("Health"){let Value::Float(health)=value else{bail!("Health不是Float")};fields.insert("HealF".into(),Value::Float(health));}}
    if ["minecraft:pig","minecraft:cow","minecraft:chicken"].contains(&id.as_str()) && target <4300 {
        if let Some(value)=fields.remove("variant"){if value!=Value::String("minecraft:temperate".into()){bail!("目标旧版不能保存{id}的温度品种外观")}}
    }
    if let Some(Value::List(passengers)) = fields.get_mut("Passengers") {
        for passenger in passengers { migrate_entity_payload(passenger, source, target, report)?; }
    }
    if source < 4303 && target >= 4303 {
        if let Some(value) = fields.remove("FallDistance") {
            let Value::Float(value) = value else {
                bail!("摔落距离不是实际来源 Float")
            };
            fields.insert("fall_distance".into(), Value::Double(f64::from(value)));
        }
    } else if source >= 4303 && target < 4303 {
        if let Some(value) = fields.remove("fall_distance") {
            let Value::Double(value) = value else {
                bail!("摔落距离不是实际来源 Double")
            };
            if f64::from(value as f32) != value {
                bail!("目标旧版摔落距离无法无损表示")
            }
            fields.insert("FallDistance".into(), Value::Float(value as f32));
        }
    }
    if [
        "minecraft:armor_stand",
        "minecraft:tropical_fish",
        "minecraft:pufferfish",
        "minecraft:frog",
        "minecraft:parrot",
        "minecraft:bee", "minecraft:villager", "minecraft:chicken", "minecraft:pig", "minecraft:cow", "minecraft:sheep", "minecraft:rabbit", "minecraft:horse", "minecraft:zombie", "minecraft:skeleton",
    ]
    .contains(&id.as_str())
    {
        migrate_equipment_100(fields, source, target, true)?;
        migrate_special_equipment(fields, &id, source, target, true, report)?;
        migrate_equipment(fields, source, target, report)?;
        migrate_special_equipment(fields, &id, source, target, false, report)?;
        migrate_equipment_100(fields, source, target, false)?;
    }
    if [
        "minecraft:item_frame",
        "minecraft:glow_item_frame",
        "minecraft:painting",
    ]
    .contains(&id.as_str())
    {
        if source < 4314 && target >= 4314 {
            let mut coordinates = Vec::new();
            for key in ["TileX", "TileY", "TileZ"] {
                match fields.remove(key) {
                    Some(Value::Int(value)) => coordinates.push(value),
                    _ => bail!("悬挂实体方块坐标缺失"),
                }
            }
            fields.insert(
                "block_pos".into(),
                Value::IntArray(fastnbt::IntArray::new(coordinates)),
            );
        } else if source >= 4314 && target < 4314 {
            let Some(Value::IntArray(coordinates)) = fields.remove("block_pos") else {
                bail!("悬挂实体目标坐标缺失")
            };
            if coordinates.len() != 3 {
                bail!("悬挂实体目标坐标必须有三个轴")
            }
            for (key, value) in ["TileX", "TileY", "TileZ"]
                .into_iter()
                .zip(coordinates.iter())
            {
                fields.insert(key.into(), Value::Int(*value));
            }
        }
        if let Some(item) = fields.get_mut("Item") {
            item_payload(item, source, target, report)?;
        }
    }
    if boat {
        let wood = if source < 4067 { match fields.get("Type") { Some(Value::String(wood))=>wood.as_str(),None=>"oak",_=>bail!("船木材类型无效") } }
            else { id.strip_prefix("minecraft:").and_then(|id|id.strip_suffix("_boat").or_else(||id.strip_suffix("_raft"))).context("船木材类型无效")? };
        let item=if wood=="oak"&&target<1519{"minecraft:boat".into()}else{format!("minecraft:{wood}_{}",if wood=="bamboo"{"raft"}else{"boat"})};
        if !target_has_item(target,&item)? { bail!("目标真实注册表没有{wood}船，不能保持木材外观") }
        if source < 4067 && target >= 4067 {
            let wood = match fields.remove("Type") {
                Some(Value::String(wood)) => wood,
                None => "oak".into(),
                _ => bail!("船的木材类型格式无效"),
            };
            if ![
                "oak", "spruce", "birch", "jungle", "acacia", "cherry", "dark_oak", "mangrove",
                "bamboo",
            ]
            .contains(&wood.as_str())
            {
                bail!("船的木材类型不能无损迁移")
            }
            let suffix = if wood == "bamboo" { "raft" } else { "boat" };
            fields.insert(
                "id".into(),
                Value::String(format!("minecraft:{wood}_{suffix}")),
            );
        } else if source >= 4067 && target < 4067 {
            let wood = id
                .strip_prefix("minecraft:")
                .and_then(|id| {
                    id.strip_suffix("_boat")
                        .or_else(|| id.strip_suffix("_raft"))
                })
                .context("船的目标木材类型无法降级")?;
            fields.insert("id".into(), Value::String("minecraft:boat".into()));
            fields.insert("Type".into(), Value::String(wood.into()));
        }
    }
    if id == "minecraft:item" { if let Some(item) = fields.get_mut("Item") { item_payload(item, source, target, report)?; } }
    if id == "minecraft:painting" && (source < 1460) != (target < 1460) {
        let key=if source<3090{"Motive"}else{"variant"};
        if let Some(value)=fields.get_mut(key){let Value::String(motive)=value else{bail!("画作类型不是String")};
            const MOTIVES:&[(&str,&str)]=&[("Kebab","kebab"),("Aztec","aztec"),("Alban","alban"),("Aztec2","aztec2"),("Bomb","bomb"),("Plant","plant"),("Wasteland","wasteland"),("Pool","pool"),("Courbet","courbet"),("Sea","sea"),("Sunset","sunset"),("Creebet","creebet"),("Wanderer","wanderer"),("Graham","graham"),("Match","match"),("Bust","bust"),("Stage","stage"),("Void","void"),("SkullAndRoses","skull_and_roses"),("Wither","wither"),("Fighters","fighters"),("Pointer","pointer"),("Pigscene","pigscene"),("BurningSkull","burning_skull"),("Skeleton","skeleton"),("DonkeyKong","donkey_kong")];
            if target>=1460 {let lower=motive.to_ascii_lowercase();let modern=match lower.as_str(){"donkeykong"=>"donkey_kong","burningskull"=>"burning_skull","skullandroses"=>"skull_and_roses",other=>other};*motive=if modern.contains(':'){modern.into()}else{format!("minecraft:{modern}")};}
            else{*motive=MOTIVES.iter().find(|(_,modern)|motive==&format!("minecraft:{modern}")).context("目标旧版没有该画作类型")?.0.into();}
        }
    }
    if id == "minecraft:painting" && (source < 3090) != (target < 3090) {
        for (old, new) in [("Motive", "variant"), ("Facing", "facing")] {
            let (from, to) = if target >= 3090 { (old, new) } else { (new, old) };
            if let Some(value) = fields.remove(from) { fields.insert(to.into(), value); }
        }
    }
    if target < 705 {
        // BoatSplit may already have changed a wood-specific ID to boat.
        // Encode the actual migrated identity, not the immutable source ID.
        let canonical_id=match fields.get("id"){Some(Value::String(id))=>id.as_str(),_=>bail!("目标实体ID缺失")};
        let old = match canonical_id { "minecraft:armor_stand" => "ArmorStand", "minecraft:item_frame" => "ItemFrame", "minecraft:painting" => "Painting", "minecraft:boat" => "Boat", "minecraft:item" => "Item", "minecraft:minecart" => "MinecartRideable", "minecraft:villager"=>"Villager", "minecraft:chicken"=>"Chicken", "minecraft:pig"=>"Pig", "minecraft:cow"=>"Cow", "minecraft:sheep"=>"Sheep", "minecraft:rabbit"=>"Rabbit", "minecraft:horse"=>"EntityHorse", "minecraft:zombie"=>"Zombie", "minecraft:skeleton"=>"Skeleton", _ => bail!("目标旧版没有该实体ID {id}") };
        fields.insert("id".into(), Value::String(old.into()));
    }
    report
        .changes
        .push(format!("{id}：保留实体与物品并迁移目标格式"));
    Ok(())
}

fn migrate_equipment(
    fields: &mut Fields,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    const GROUPS: [(&str, &[&str]); 2] = [
        ("ArmorItems", &["feet", "legs", "chest", "head"]),
        ("HandItems", &["mainhand", "offhand"]),
    ];
    if source < 4301 && target >= 4301 {
        let mut equipment = Fields::new();
        for (key, slots) in GROUPS {
            if let Some(value) = fields.remove(key) {
                let Value::List(items) = value else {
                    bail!("装备不是列表")
                };
                if items.len() > slots.len() {
                    bail!("装备槽位超过目标范围")
                }
                for (mut item, slot) in items.into_iter().zip(slots) {
                    if matches!(&item, Value::Compound(fields) if fields.is_empty()) {
                        continue;
                    }
                    item_payload(&mut item, source, target, report)?;
                    equipment.insert((*slot).into(), item);
                }
            }
        }
        if !equipment.is_empty() {
            fields.insert("equipment".into(), Value::Compound(equipment));
        }
    } else if source >= 4301 && target < 4301 {
        // Empty placeholder arrays preserve the old codec's actual defaults.
        let mut equipment = match fields.remove("equipment") {
            Some(Value::Compound(fields)) => fields,
            None => Fields::new(),
            _ => bail!("装备不是复合标签"),
        };
        for (key, slots) in GROUPS {
            let mut items = Vec::new();
            for slot in slots {
                let mut item = equipment
                    .remove(*slot)
                    .unwrap_or(Value::Compound(Fields::new()));
                item_payload(&mut item, source, target, report)?;
                items.push(item);
            }
            fields.insert(key.into(), Value::List(items));
        }
        if !equipment.is_empty() {
            bail!("旧版不能无损保存这些额外装备槽位")
        }
    } else {
        for (key, _) in GROUPS {
            if let Some(Value::List(items)) = fields.get_mut(key) {
                for item in items {
                    item_payload(item, source, target, report)?;
                }
            }
        }
        if let Some(Value::Compound(equipment)) = fields.get_mut("equipment") {
            for item in equipment.values_mut() {
                item_payload(item, source, target, report)?;
            }
        }
    }
    const CHANCES: [(&str, &[&str]); 2] = [
        ("ArmorDropChances", &["feet", "legs", "chest", "head"]),
        ("HandDropChances", &["mainhand", "offhand"]),
    ];
    if source < 4293 && target >= 4293 {
        let mut chances = Fields::new();
        for (key, slots) in CHANCES {
            if let Some(value) = fields.remove(key) {
                let Value::List(values) = value else {
                    bail!("掉落比例不是列表")
                };
                if values.len() > slots.len() {
                    bail!("掉落比例槽位超过目标范围")
                }
                // Retain even default values. This is codec-compatible and makes
                // a reverse conversion retain original drop chances exactly.
                for (value, slot) in values.into_iter().zip(slots) {
                    chances.insert((*slot).into(), value);
                }
            }
        }
        if !chances.is_empty() {
            fields.insert("drop_chances".into(), Value::Compound(chances));
        }
    } else if source >= 4293 && target < 4293 {
        if let Some(value) = fields.remove("drop_chances") {
            let Value::Compound(mut chances) = value else {
                bail!("掉落比例不是复合标签")
            };
            for (key, slots) in CHANCES {
                fields.insert(
                    key.into(),
                    Value::List(
                        slots
                            .iter()
                            .map(|slot| chances.remove(*slot).unwrap_or(Value::Float(0.085)))
                            .collect(),
                    ),
                );
            }
            if !chances.is_empty() {
                bail!("旧版不能无损保存额外掉落比例")
            }
        }
    }
    Ok(())
}

// Actual DataFixers schema boundaries: v1458 wraps plain CustomName in a JSON
// component; v4290 unflattens that JSON into NBT. Rich click/hover-event rewrites
// are not silently retained across versions with different codecs.
// Legacy hover contents use vanilla SNBT, not JSON. Parse typed NBT first and
// then run the same item/entity codecs; no opaque source payload is carried
// across a format boundary. The quoted-string escapes match Brigadier SNBT.
fn read_snbt(input:&str)->Result<Value>{
    if input.len()>1024*1024{bail!("悬停SNBT超过1MiB")}
    struct Parser<'a>{text:&'a str,at:usize}
    impl Parser<'_>{
        fn peek(&self)->Option<char>{self.text[self.at..].chars().next()}
        fn next(&mut self)->Option<char>{let c=self.peek()?;self.at+=c.len_utf8();Some(c)}
        fn ws(&mut self){while self.peek().is_some_and(char::is_whitespace){self.next();}}
        fn take(&mut self,c:char)->bool{self.ws();if self.peek()==Some(c){self.next();true}else{false}}
        fn expect(&mut self,c:char)->Result<()>{if !self.take(c){bail!("SNBT位置{}缺少{c}",self.at)}Ok(())}
        fn quoted(&mut self)->Result<String>{let quote=self.next().context("SNBT缺少引号")?;let mut out=String::new();loop{match self.next(){Some(c)if c==quote=>break,Some('\\')=>{let escaped=self.next().context("SNBT转义未结束")?;if escaped!=quote&&escaped!='\\'{bail!("SNBT有无效转义")};out.push(escaped);},Some(c)=>out.push(c),None=>bail!("SNBT引号未闭合")}}Ok(out)}
        fn token(&mut self,key:bool)->Result<String>{self.ws();if matches!(self.peek(),Some('\''|'"')){return self.quoted()}let start=self.at;while self.peek().is_some_and(|c|!c.is_whitespace()&&!matches!(c,','|']'|'}'|'{'|'[')&&(!key||c!=':')){self.next();}if self.at==start{bail!("SNBT位置{}缺少值",self.at)}Ok(self.text[start..self.at].into())}
        fn value(&mut self,depth:usize)->Result<Value>{
            if depth>128{bail!("SNBT超过128层嵌套")};self.ws();
            if matches!(self.peek(),Some('\''|'"')){return Ok(Value::String(self.quoted()?))}
            if self.take('{'){let mut map=Fields::new();if self.take('}'){return Ok(Value::Compound(map))}loop{let key=self.token(true)?;self.expect(':')?;let value=self.value(depth+1)?;if map.insert(key,value).is_some(){bail!("SNBT有重复键，拒绝覆盖")};if self.take('}'){break}self.expect(',')?;}return Ok(Value::Compound(map))}
            if self.take('['){
                self.ws();let begin=self.at;let typed=match self.peek(){Some(c@('B'|'I'|'L'))=>{self.next();if self.take(';'){Some(c)}else{self.at=begin;None}},_=>None};
                let mut values=Vec::new();if !self.take(']'){loop{values.push(self.value(depth+1)?);if self.take(']'){break}self.expect(',')?;}}
                return Ok(match typed{
                    Some('B')=>Value::ByteArray(fastnbt::ByteArray::new(values.into_iter().map(|v|match v{Value::Byte(n)=>Ok(n),_=>bail!("SNBT ByteArray条目不是Byte")}).collect::<Result<_>>()?)),
                    Some('I')=>Value::IntArray(fastnbt::IntArray::new(values.into_iter().map(|v|match v{Value::Int(n)=>Ok(n),_=>bail!("SNBT IntArray条目不是Int")}).collect::<Result<_>>()?)),
                    Some('L')=>Value::LongArray(fastnbt::LongArray::new(values.into_iter().map(|v|match v{Value::Long(n)=>Ok(n),_=>bail!("SNBT LongArray条目不是Long")}).collect::<Result<_>>()?)),
                    None=>{if let Some(first)=values.first(){if values.iter().any(|v|std::mem::discriminant(v)!=std::mem::discriminant(first)){bail!("SNBT List条目类型不一致")}}Value::List(values)},_=>unreachable!(),
                })
            }
            let token=self.token(false)?;let lower=token.to_ascii_lowercase();
            if lower=="true"{return Ok(Value::Byte(1))}if lower=="false"{return Ok(Value::Byte(0))}
            let numeric=|value:&str|value.chars().all(|c|c.is_ascii_digit()||matches!(c,'+'|'-'|'.'|'e'|'E'))&&value.chars().any(|c|c.is_ascii_digit());
            if token.len()>1&&token.is_ascii(){let(body,suffix)=token.split_at(token.len()-1);if numeric(body){let value=match suffix.to_ascii_lowercase().as_str(){"b"=>body.parse::<i8>().ok().map(Value::Byte),"s"=>body.parse::<i16>().ok().map(Value::Short),"l"=>body.parse::<i64>().ok().map(Value::Long),"f"=>body.parse::<f32>().ok().filter(|v|v.is_finite()).map(Value::Float),"d"=>body.parse::<f64>().ok().filter(|v|v.is_finite()).map(Value::Double),_=>None};if let Some(value)=value{return Ok(value)}}}
            if numeric(&token){if !token.contains(['.','e','E']){if let Ok(value)=token.parse::<i32>(){return Ok(Value::Int(value))}}else if let Ok(value)=token.parse::<f64>(){if value.is_finite(){return Ok(Value::Double(value))}}}
            Ok(Value::String(token))
        }
    }
    let mut parser=Parser{text:input,at:0};let value=parser.value(0)?;parser.ws();if parser.at!=input.len(){bail!("SNBT有尾随内容")};Ok(value)
}

fn legacy_hover_string(value:&Value)->Result<String>{
    match value{Value::String(text)=>Ok(text.clone()),Value::Compound(fields)if fields.len()==1=>match fields.get("text"){Some(Value::String(text))=>Ok(text.clone()),_=>bail!("旧SNBT悬停值不是字面文本")},_=>bail!("旧SNBT悬停值不能从非字面富文本等价提取")}
}

// Text stays in its real typed NBT representation while nested hover items are
// migrated. JSON is only the envelope before UnflattenTextComponentFix v4290.
fn text_from_json(value: &serde_json::Value) -> Result<Value> {
    Ok(match value {
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Bool(b) => Value::Byte(i8::from(*b)),
        serde_json::Value::Number(n) => if let Some(n)=n.as_i64() { if let Ok(n)=i32::try_from(n){Value::Int(n)}else{Value::Long(n)} } else { Value::Double(n.as_f64().context("文本数字无效")?) },
        serde_json::Value::Array(values) => Value::List(values.iter().map(text_from_json).collect::<Result<_>>()?),
        serde_json::Value::Object(fields) => Value::Compound(fields.iter().map(|(k,v)|Ok((k.clone(),text_from_json(v)?))).collect::<Result<_>>()?),
        _ => bail!("文本包含null节点，目标编码不能保留"),
    })
}

fn text_to_json(value: &Value, key: &str) -> Result<serde_json::Value> {
    use serde_json::Value as J;
    Ok(match value {
        Value::String(s)=>J::String(s.clone()),
        Value::Byte(n) if ["bold","italic","underlined","strikethrough","obfuscated","interpret","plain","shadow","show_in_tooltip","resolved","hide_tooltip"].contains(&key)=>match n{0=>J::Bool(false),1=>J::Bool(true),_=>bail!("文本布尔字段{key}无效")},
        Value::Byte(n)=>J::from(*n),Value::Short(n)=>J::from(*n),Value::Int(n)=>J::from(*n),Value::Long(n)=>J::from(*n),
        Value::Float(n)=>J::Number(serde_json::Number::from_f64(f64::from(*n)).context("文本包含非有限数")?),
        Value::Double(n)=>J::Number(serde_json::Number::from_f64(*n).context("文本包含非有限数")?),
        Value::List(values)=>J::Array(values.iter().map(|v|text_to_json(v,"")).collect::<Result<_>>()?),
        Value::IntArray(values)=>J::Array(values.iter().map(|n|J::from(*n)).collect()),
        Value::Compound(fields)=>{let mut fields=fields.iter().collect::<Vec<_>>();fields.sort_by(|a,b|a.0.cmp(b.0));J::Object(fields.into_iter().map(|(k,v)|Ok((k.clone(),text_to_json(v,k)?))).collect::<Result<_>>()?)},
        _=>bail!("文本包含未覆盖的数组类型"),
    })
}

fn migrate_text_node(node:&mut Value,source:i32,target:i32,depth:usize)->Result<()> {
    if depth>128{bail!("文本嵌套超过128层")}
    match node {
        Value::String(_)=>return Ok(()),
        Value::List(nodes)=>{for node in nodes{migrate_text_node(node,source,target,depth+1)?;}return Ok(())},
        Value::Compound(_)=>{},_=>bail!("文本节点不是字符串、List或Compound"),
    }
    let fields=object(node)?;
    for key in fields.keys(){if !["text","translate","fallback","with","score","selector","keybind","nbt","block","entity","storage","interpret","plain","separator","type","color","bold","italic","underlined","strikethrough","obfuscated","insertion","font","extra","hoverEvent","hover_event","clickEvent","click_event","shadow_color"].contains(&key.as_str()){bail!("文本字段{key}未覆盖真实目标编码")}}
    for key in ["bold","italic","underlined","strikethrough","obfuscated","interpret"]{if let Some(value)=fields.get_mut(key){if source<4291{if let Value::String(text)=value{*value=match text.as_str(){"true"=>Value::Byte(1),"false"=>Value::Byte(0),_=>bail!("文本布尔字符串{key}无效")};}}if !matches!(value,Value::Byte(0|1)){bail!("文本布尔字段{key}类型无效")}}}
    if fields.contains_key("plain")&&target<5023{bail!("目标版本没有NBT文本plain语义，原文字保留")}
    if fields.contains_key("shadow_color")&&target<4290{bail!("目标旧版没有独立文本阴影颜色")}
    if let Some(Value::String(kind))=fields.get("type"){if !["text","translatable","keybind","score","selector","nbt"].contains(&kind.as_str()){bail!("目标文本内容类型{kind}未覆盖")}}
    if fields.contains_key("keybind")&&target<1139{bail!("目标1.12以前没有按键文本内容")}
    if fields.contains_key("nbt")&&target<1901{bail!("目标1.14以前没有NBT文本内容")}
    if fields.contains_key("storage")&&target<2200{bail!("目标1.15以前没有storage文本数据源")}
    if let Some(score)=fields.get("score"){let Value::Compound(score)=score else{bail!("score文本不是Compound")};if score.keys().any(|key|!["name","objective","value"].contains(&key.as_str())){bail!("score文本含未知字段")}}
    for key in ["extra","with"]{if let Some(value)=fields.get_mut(key){let Value::List(nodes)=value else{bail!("文本{key}不是List")};for node in nodes{if key=="with"&&matches!(node,Value::Byte(_)|Value::Short(_)|Value::Int(_)|Value::Long(_)|Value::Float(_)|Value::Double(_)){continue}migrate_text_node(node,source,target,depth+1)?;}}}
    if let Some(node)=fields.get_mut("separator"){migrate_text_node(node,source,target,depth+1)?;}
    let hover_key=if source<4292{"hoverEvent"}else{"hover_event"};
    if let Some(mut event)=fields.remove(hover_key){
        let hover=object(&mut event)?;
        let Some(Value::String(action))=hover.get("action")else{bail!("悬停文本缺少action")};let action=action.clone();
        let legacy_value=source<4292&&!hover.contains_key("contents")&&hover.contains_key("value")&&action!="show_text";
        let mut contents=if source<4292{hover.remove("contents").or_else(||hover.remove("value")).context("悬停事件缺少contents/value")?}else{
            match action.as_str(){"show_text"=>hover.remove("value").context("悬停事件缺少value")?,"show_item"|"show_entity"=>{let keys=if action=="show_item"{&["id","count","components"][..]}else{&["id","uuid","name"][..]};let mut data=Fields::new();for key in keys{if let Some(value)=hover.remove(*key){data.insert((*key).into(),value);}}Value::Compound(data)},_=>bail!("悬停事件action {action}未知")}
        };
        if hover.keys().any(|key|key!="action"){bail!("悬停事件含未知字段")}
        if legacy_value {contents=read_snbt(&legacy_hover_string(&contents)?)?;}
        match action.as_str(){
            "show_text"=>migrate_text_node(&mut contents,source,target,depth+1)?,
            "show_item"=>{
                let item=object(&mut contents)?;
                if item.keys().any(|key|!["id","count","Count","Damage","components","tag"].contains(&key.as_str())){bail!("悬停物品含未知字段")}
                if let Some(tag)=item.get_mut("tag"){if let Value::String(text)=tag{*tag=read_snbt(text)?;}if !matches!(tag,Value::Compound(_)){bail!("悬停物品tag不是SNBT Compound")}}
                if source<3818{let count=if legacy_value{match item.remove("Count"){Some(Value::Byte(n))=>i32::from(n),Some(Value::Short(n))=>i32::from(n),Some(Value::Int(n))=>n,None=>1,_=>bail!("旧悬停Count类型无效")}}else{match item.remove("count"){Some(Value::Int(n))=>n,None=>1,_=>bail!("悬停物品count不是Int")}};item.insert("Count".into(),Value::Byte(i8::try_from(count).context("悬停物品数量超过Byte")?));}
                let mut report=PayloadMigrationReport::default();item_payload(&mut contents,source,target,&mut report)?;if !report.issues.is_empty(){bail!("悬停物品：{}",report.issues.join("；"))}
                if target<2566{contents=Value::String(additional_items::write_snbt(&contents)?);}
                else if target<3818{let item=object(&mut contents)?;if let Some(tag)=item.get_mut("tag"){*tag=Value::String(additional_items::write_snbt(tag)?);}if let Some(Value::Byte(count))=item.remove("Count"){item.insert("count".into(),Value::Int(i32::from(count)));}item.remove("Damage");}
            }
            "show_entity"=>{
                let data=object(&mut contents).context("旧悬停show_entity为SNBT文本，尚需SNBT解码，原文件保留")?;
                let (kind,uuid)=if source<4292{("type","id")}else{("id","uuid")};
                let kind=data.remove(kind).context("悬停实体缺少type/id")?;let mut uuid=data.remove(uuid).context("悬停实体缺少UUID")?;
                if !matches!(kind,Value::String(_)){bail!("悬停实体类型不是String")}
                if target<4290&&matches!(uuid,Value::IntArray(_)){uuid=Value::String(uuid_text(&uuid)?)}
                match &uuid{Value::String(text)=>{uuid_value(text)?;},Value::IntArray(_)=>{uuid_text(&uuid)?;},_=>bail!("悬停实体UUID编码无效")}
                if let Some(name)=data.get_mut("name"){if legacy_value{if let Value::String(json)=name{*name=text_from_json(&serde_json::from_str(json).context("旧悬停实体name不是文本JSON")?)?;}}migrate_text_node(name,source,target,depth+1)?;}
                if data.keys().any(|key|key!="name"){bail!("悬停实体含未知字段")}
                data.insert(if target<4292{"type"}else{"id"}.into(),kind);data.insert(if target<4292{"id"}else{"uuid"}.into(),uuid);
                if target<2566{if let Some(name)=data.get_mut("name"){*name=Value::String(serde_json::to_string(&text_to_json(name,"")?)?);}contents=Value::String(additional_items::write_snbt(&contents)?);}
            }
            _=>bail!("悬停事件action {action}未知"),
        }
        if target<4292{hover.insert(if target<2566{"value"}else{"contents"}.into(),contents);}else if action=="show_text"{hover.insert("value".into(),contents);}else{for(key,value)in object(&mut contents)?.drain(){hover.insert(key,value);}}
        fields.insert(if target<4292{"hoverEvent"}else{"hover_event"}.into(),event);
    }
    let click_key=if source<4292{"clickEvent"}else{"click_event"};
    if let Some(mut event)=fields.remove(click_key){
        let click=object(&mut event)?;let Some(Value::String(action))=click.get("action")else{bail!("点击事件缺少action")};let action=action.clone();
        let modern=match action.as_str(){"open_url"=>"url","open_file"=>"path","run_command"|"suggest_command"=>"command","change_page"=>"page","copy_to_clipboard"=>"value",_=>bail!("点击事件action {action}未知")};
        let mut value=click.remove(if source<4292{"value"}else{modern}).context("点击事件缺少值")?;
        if click.keys().any(|key|key!="action"){bail!("点击事件有未知字段")}
        if action=="change_page"{let page=match &value{Value::Int(n)=>*n,Value::String(s)=>s.parse::<i32>().context("点击页码不是整数")?,_=>bail!("点击页码类型无效")};if page<1{bail!("目标页码下限为1，原负值/零不能等价保留")}value=if target<4292{Value::String(page.to_string())}else{Value::Int(page)};}
        else{let Value::String(text)=&value else{bail!("点击事件值不是String")};if action=="open_url"{let lower=text.to_ascii_lowercase();if !(lower.starts_with("https:")||lower.starts_with("http:"))||text.chars().any(|c|c.is_whitespace()||c.is_control()){bail!("目标open_url编码会丢弃无效HTTP(S) URI，原文字保留")}}if (action=="run_command"||action=="suggest_command")&&text.chars().any(|c|c=='§'||c<' '||c=='\u{7f}'){bail!("目标命令文本编码会丢弃控制字符，原文字保留")}}
        click.insert(if target<4292{"value"}else{modern}.into(),value);fields.insert(if target<4292{"clickEvent"}else{"click_event"}.into(),event);
    }
    Ok(())
}

fn migrate_component(value: &Value, source: i32, target: i32) -> Result<Value> {
    if source<1458&&target<1458{return Ok(value.clone())}
    let mut node=if source<1458{let Value::String(text)=value else{bail!("旧版名称不是String")};Value::Compound([("text".into(),Value::String(text.clone()))].into())}
        else if source<4290{let Value::String(json)=value else{bail!("文本不是JSON字符串")};text_from_json(&serde_json::from_str(json).context("文本JSON无效")?)?}else{value.clone()};
    migrate_text_node(&mut node,source,target,0)?;
    if target<1458{match node{Value::String(text)=>Ok(Value::String(text)),Value::Compound(mut fields)if fields.len()==1=>{let Some(Value::String(text))=fields.remove("text")else{bail!("目标旧版名称不能等价保存富文本")};Ok(Value::String(text))},_=>bail!("目标旧版名称不能等价保存富文本")}}
    else if target<4290{Ok(Value::String(serde_json::to_string(&text_to_json(&node,"")?)?))}else{Ok(node)}
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn legacy_bed_color_changes_actual_palette_and_inverse_recreates_color() {
        let mut bed=tile("minecraft:bed",vec![]);let t=fields(&mut bed);t.remove("Items");for key in ["x","y","z"]{t.insert(key.into(),Value::Int(0));}t.insert("color".into(),Value::Int(15));
        let mut value=region(bed);let f=fields(&mut value);f.insert("Size".into(),map(&[("x",Value::Int(1)),("y",Value::Int(1)),("z",Value::Int(1))]));f.insert("BlockStatePalette".into(),Value::List(vec![map(&[("Name",Value::String("minecraft:red_bed".into())),("Properties",map(&[("facing",Value::String("north".into())),("occupied",Value::String("false".into())),("part",Value::String("head".into()))]))])]));f.insert("BlockStates".into(),Value::LongArray(fastnbt::LongArray::new(vec![0])));
        migrate_bed_palette(&mut value,1343,5023).unwrap();let Value::List(palette)=&fields(&mut value)["BlockStatePalette"]else{panic!()};assert!(palette.iter().any(|entry|matches!(entry,Value::Compound(entry)if entry.get("Name")==Some(&Value::String("minecraft:black_bed".into())))));
        let report=migrate(&mut value,1343,5023).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);assert_eq!(fields(&mut value)["TileEntities"],Value::List(vec![]));
        let modern=value.clone();assert!(migrate_bed_palette(&mut value,5023,0).unwrap_err().to_string().contains("只有红床"));value=modern;
        migrate_bed_palette(&mut value,5023,1343).unwrap();let report=migrate(&mut value,5023,1343).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);let Value::List(tiles)=&fields(&mut value)["TileEntities"]else{panic!()};let Value::Compound(bed)=&tiles[0]else{panic!()};assert_eq!(bed["color"],Value::Int(15));
    }

    #[test]
    fn riding_chain_reverses_root_and_multiple_riders_have_exact_old_target_loss() {
        let mut value=map(&[("id",Value::String("Chicken".into())),("Riding",map(&[("id",Value::String("Pig".into())),("Riding",map(&[("id",Value::String("Boat".into()))]))]))]);let mut report=PayloadMigrationReport::default();
        migrate_entity(&mut value,0,5023,&mut report).unwrap();assert_eq!(fields(&mut value)["id"],Value::String("minecraft:oak_boat".into()));
        migrate_entity(&mut value,5023,0,&mut report).unwrap();let rider=fields(&mut value);assert_eq!(rider["id"],Value::String("Chicken".into()));let Value::Compound(pig)=&rider["Riding"]else{panic!()};assert_eq!(pig["id"],Value::String("Pig".into()));let Value::Compound(boat)=&pig["Riding"]else{panic!()};assert_eq!(boat["id"],Value::String("Boat".into()));
        let mut two=map(&[("id",Value::String("minecraft:oak_boat".into())),("Passengers",Value::List(vec![map(&[("id",Value::String("minecraft:pig".into()))]),map(&[("id",Value::String("minecraft:chicken".into()))])]))]);assert!(migrate_entity(&mut two,5023,0,&mut report).unwrap_err().to_string().contains("多个乘客"));
    }

    #[test]
    fn typed_snbt_hover_and_global_hideflags_use_real_item_bridge_in_both_directions() {
        let snbt=r#"{id:"minecraft:stone",Count:2b,tag:{display:{Name:'{"text":"Named"}'}}}"#;
        let old=Value::String(serde_json::json!({"text":"Hover","hoverEvent":{"action":"show_item","value":snbt}}).to_string());
        let modern=migrate_component(&old,2230,5023).unwrap();let Value::String(back)=migrate_component(&modern,5023,2230).unwrap()else{panic!()};let back:serde_json::Value=serde_json::from_str(&back).unwrap();let restored=read_snbt(back["hoverEvent"]["value"].as_str().unwrap()).unwrap();assert_eq!(restored,read_snbt(snbt).unwrap());
        let typed=map(&[("byte",Value::Byte(-2)),("short",Value::Short(3)),("long",Value::Long(4)),("float",Value::Float(1.5)),("double",Value::Double(2.25)),("a",Value::ByteArray(fastnbt::ByteArray::new(vec![1,-2]))),("i",Value::IntArray(fastnbt::IntArray::new(vec![1,2]))),("l",Value::LongArray(fastnbt::LongArray::new(vec![3,4]))),("text",Value::String("中文\\\"\n".into()))]);assert_eq!(read_snbt(&additional_items::write_snbt(&typed).unwrap()).unwrap(),typed);assert!(read_snbt("{x:1,x:2}").is_err());assert!(read_snbt("[B;1]").is_err());
        let mut potion=map(&[("id",Value::String("minecraft:potion".into())),("Count",Value::Byte(1)),("tag",map(&[("Potion",Value::String("minecraft:water".into())),("HideFlags",Value::Int(32))]))]);let original=potion.clone();let mut report=PayloadMigrationReport::default();item_payload(&mut potion,3700,5023,&mut report).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);item_payload(&mut potion,5023,3700,&mut report).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);assert_eq!(potion,original);
        let mut hidden=map(&[("minecraft:tooltip_display",map(&[("hide_tooltip",Value::Byte(1))]))]);migrate_components(&mut hidden,5023,3818,&mut report).unwrap();assert!(components_legacy_item(hidden,3700,&mut report).unwrap_err().to_string().contains("整项隐藏"));
    }

    #[test]
    fn rich_text_click_hover_items_and_score_cross_json_nbt_event_nodes() {
        let original=serde_json::json!({"text":"物品", "clickEvent":{"action":"open_url","value":"https://example.com/nova"},"hoverEvent":{"action":"show_item","contents":{"id":"minecraft:stone","count":2}},"extra":[{"score":{"name":"@s","objective":"points"}},{"keybind":"key.jump"}]});
        let modern=migrate_component(&Value::String(original.to_string()),3700,5023).unwrap();
        let Value::Compound(fields)=&modern else{panic!()};
        assert!(fields.contains_key("click_event")&&fields.contains_key("hover_event"));
        let old=migrate_component(&modern,5023,3700).unwrap();
        let Value::String(old)=old else{panic!()};
        assert_eq!(serde_json::from_str::<serde_json::Value>(&old).unwrap(),original);
        let page=Value::String(serde_json::json!({"text":"次页","clickEvent":{"action":"change_page","value":"2"},"hoverEvent":{"action":"show_entity","contents":{"type":"minecraft:pig","id":"00000000-0000-0001-0000-000000000002","name":{"text":"猪"}}}}).to_string());
        let modern=migrate_component(&page,3700,5023).unwrap();
        let Value::Compound(fields)=&modern else{panic!()};let Value::Compound(click)=&fields["click_event"]else{panic!()};assert_eq!(click["page"],Value::Int(2));
        let Value::String(back)=migrate_component(&modern,5023,3700).unwrap()else{panic!()};let Value::String(page)=page else{panic!()};assert_eq!(serde_json::from_str::<serde_json::Value>(&back).unwrap(),serde_json::from_str::<serde_json::Value>(&page).unwrap());
        let invalid=Value::String(serde_json::json!({"text":"bad","clickEvent":{"action":"run_command","value":"/say §x"}}).to_string());
        assert!(migrate_component(&invalid,3700,5023).unwrap_err().to_string().contains("控制字符"));
    }

    #[test]
    fn skull_profile_properties_and_old_uuid_really_cross_3820_and_reverse() {
        for source in [0,3700] {
            let mut owner=map(&[("Name",Value::String("Nova".into())),("Id",if source<2514{Value::String("00000000-0000-0001-0000-000000000002".into())}else{uuid_value("00000000-0000-0001-0000-000000000002").unwrap()}),("Properties",map(&[("textures",Value::List(vec![map(&[("Value",Value::String("texture-value".into())),("Signature",Value::String("signature".into()))])]))]))]);
            let original=owner.clone();migrate_profile(&mut owner,source,5023,3820).unwrap();
            assert!(fields(&mut owner).contains_key("properties"));migrate_profile(&mut owner,5023,source,3820).unwrap();assert_eq!(owner,original);
        }
        let mut skull=tile("minecraft:skull",vec![]);fields(&mut skull).remove("Items");fields(&mut skull).insert("SkullOwner".into(),map(&[("Name",Value::String("Nova".into()))]));
        let mut value=region(skull);let original=value.clone();let report=migrate(&mut value,3700,5023).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);let report=migrate(&mut value,5023,3700).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);assert_eq!(value,original);
        let mut invalid=map(&[("Name",Value::String("Nova".into())),("Properties",Value::Int(7))]);assert!(migrate_profile(&mut invalid,3700,5023,3820).is_err());
    }

    #[test]
    fn entity_owner_love_cause_sleep_and_multi_node_attributes_are_not_passthrough() {
        let mut horse=Fields::from([("id".into(),Value::String("minecraft:horse".into())),("OwnerUUID".into(),Value::String("00000000-0000-0001-0000-000000000002".into())),("LoveCauseMost".into(),Value::Long(3)),("LoveCauseLeast".into(),Value::Long(4))]);
        let original=horse.clone();migrate_entity_uuid(&mut horse,2230,5023).unwrap();assert!(horse.contains_key("Owner")&&horse.contains_key("LoveCause"));migrate_entity_uuid(&mut horse,5023,2230).unwrap();assert_eq!(horse,original);
        let mut attributes=Fields::from([("Attributes".into(),Value::List(vec![map(&[("Name",Value::String("generic.maxHealth".into())),("Base",Value::Double(20.0))])]))]);
        let original=attributes.clone();migrate_entity_attributes(&mut attributes,0,5023).unwrap();let Value::List(list)=&attributes["attributes"]else{panic!()};let Value::Compound(attr)=&list[0]else{panic!()};assert_eq!(attr["id"],Value::String("minecraft:max_health".into()));migrate_entity_attributes(&mut attributes,5023,0).unwrap();assert_eq!(attributes,original);
        let mut villager=map(&[("id",Value::String("minecraft:villager".into())),("SleepingX",Value::Int(1)),("SleepingY",Value::Int(2)),("SleepingZ",Value::Int(3))]);let mut report=PayloadMigrationReport::default();migrate_entity(&mut villager,3700,5023,&mut report).unwrap();assert_eq!(fields(&mut villager)["sleeping_pos"],Value::IntArray(fastnbt::IntArray::new(vec![1,2,3])));
        let mut parrot=map(&[("id",Value::String("minecraft:parrot".into()))]);migrate_entity(&mut parrot,1519,1343,&mut report).unwrap();
        let mut glow=map(&[("id",Value::String("minecraft:glow_item_frame".into()))]);assert!(migrate_entity(&mut glow,3700,2586,&mut report).unwrap_err().to_string().contains("注册表"));
    }

    #[test]
    fn modern_spawner_light_rules_equipment_and_numbered_furnace_recipes_retain_payload() {
        let rules=map(&[("block_light_limit",map(&[("min_inclusive",Value::Int(0)),("max_inclusive",Value::Int(7))])),("sky_light_limit",Value::Int(15))]);
        let equipment=map(&[("loot_table",Value::String("minecraft:equipment/trial_chamber_melee".into())),("slot_drop_chances",Value::Float(0.0))]);
        let data=map(&[("entity",map(&[("id",Value::String("minecraft:zombie".into()))])),("custom_spawn_rules",rules.clone()),("equipment",equipment.clone())]);
        let mut spawner=tile("minecraft:mob_spawner",vec![]);fields(&mut spawner).remove("Items");fields(&mut spawner).insert("SpawnData".into(),data.clone());fields(&mut spawner).insert("SpawnPotentials".into(),Value::List(vec![map(&[("data",data),("weight",Value::Int(3))])]));
        let mut value=region(spawner);let report=migrate(&mut value,3953,5023).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);let Value::List(tiles)=&fields(&mut value)["TileEntities"]else{panic!()};let Value::Compound(spawner_tile)=&tiles[0]else{panic!()};let Value::Compound(spawn)=&spawner_tile["SpawnData"]else{panic!()};assert_eq!(spawn["custom_spawn_rules"],rules);assert_eq!(spawn["equipment"],equipment);
        let before=value.clone();let report=migrate(&mut value,5023,2586).unwrap();assert!(report.issues.iter().any(|issue|issue.contains("光照规则")));assert_eq!(value,before);
        let mut furnace=tile("minecraft:furnace",vec![]);fields(&mut furnace).insert("RecipesUsed".into(),map(&[("minecraft:iron_ingot",Value::Int(2))]));let mut value=region(furnace);let report=migrate(&mut value,3700,2230).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);
        let Value::List(tiles)=&fields(&mut value)["TileEntities"]else{panic!()};let Value::Compound(tile)=&tiles[0]else{panic!()};assert_eq!(tile["RecipeLocation0"],Value::String("minecraft:iron_ingot".into()));assert_eq!(tile["RecipeAmount0"],Value::Int(2));
    }

    #[test]
    fn every_registered_release_migrates_actual_banner_pattern_schema_in_both_directions() {
        let catalog = crate::block_database::database().unwrap().query(None, "", None).unwrap();
        let versions: Vec<i32> = catalog["versions"].as_array().unwrap().iter().map(|version| version["dataVersion"].as_i64().unwrap() as i32).collect();
        assert!(versions.len() >= 63);
        for &source in &versions { for &target in &versions {
            let mut banner = tile(if source < 704 { "Banner" } else { "minecraft:banner" }, vec![]);
            fields(&mut banner).remove("Items");
            let layer = if source < 3818 { map(&[("Pattern", Value::String("mc".into())), ("Color", Value::Int(if source < 1451 { 1 } else { 14 }))]) }
                else { map(&[("pattern", Value::String("minecraft:circle".into())), ("color", Value::String("red".into()))]) };
            fields(&mut banner).insert(if source < 3818 { "Patterns" } else { "patterns" }.into(), Value::List(vec![layer]));
            let mut value = region(banner); let original = value.clone();
            let report = migrate(&mut value, source, target).unwrap();
            assert!(report.issues.is_empty(), "{source}->{target}: {:?}", report.issues);
            let report = migrate(&mut value, target, source).unwrap();
            assert!(report.issues.is_empty(), "{target}->{source}: {:?}", report.issues);
            assert_eq!(value, original, "banner roundtrip {source}->{target}->{source}");
        }}
    }

    #[test]
    fn pre_120_sign_four_lines_filtered_text_and_format_boundaries_really_migrate() {
        let mut sign = tile("minecraft:sign", vec![]); fields(&mut sign).remove("Items");
        for (line, text) in ["办公室", "第二行", "", "尾行"].iter().enumerate() { fields(&mut sign).insert(format!("Text{}", line + 1), Value::String(serde_json::to_string(text).unwrap())); }
        fields(&mut sign).insert("FilteredText1".into(), Value::String("\"过滤后\"".into()));
        fields(&mut sign).insert("Color".into(), Value::String("red".into()));
        fields(&mut sign).insert("GlowingText".into(), Value::Byte(1));
        let mut value = region(sign); let original = value.clone();
        let report = migrate(&mut value, 3120, 5023).unwrap(); assert!(report.issues.is_empty(), "{:?}", report.issues);
        let report = migrate(&mut value, 5023, 3120).unwrap(); assert!(report.issues.is_empty(), "{:?}", report.issues);
        // DFU fills absent filtered lines with their actual unfiltered text.
        let Value::List(tiles) = fields(&mut value).get_mut("TileEntities").unwrap() else { unreachable!() };
        for line in 2..=4 { let text = fields(&mut tiles[0])[&format!("Text{line}")].clone(); assert_eq!(fields(&mut tiles[0]).remove(&format!("FilteredText{line}")), Some(text)); }
        assert_eq!(value, original);
    }

    #[test]
    fn old_item_tags_names_enchantments_and_nested_inventory_cross_components_and_reverse() {
        let mut stack = item(3465, 3, 0);
        fields(&mut stack).insert("tag".into(), map(&[("display", map(&[("Name", Value::String("\"命名物品\"".into())), ("Lore", Value::List(vec![Value::String("\"说明\"".into())]))])), ("Enchantments", Value::List(vec![map(&[("id", Value::String("minecraft:unbreaking".into())), ("lvl", Value::Short(2))])]))]));
        let mut value = region(tile("minecraft:chest", vec![stack])); let original = value.clone();
        let report = migrate(&mut value, 3465, 5023).unwrap(); assert!(report.issues.is_empty(), "{:?}", report.issues);
        let report = migrate(&mut value, 5023, 3465).unwrap(); assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(value, original);
    }

    #[test]
    fn uuid_fields_and_plain_armor_stand_equipment_cross_pre_component_versions() {
        let original = map(&[("id", Value::String("minecraft:armor_stand".into())), ("UUIDMost", Value::Long(-2)), ("UUIDLeast", Value::Long(7)), ("ArmorItems", Value::List(vec![map(&[]); 4])), ("HandItems", Value::List(vec![map(&[]); 2])), ("FallDistance", Value::Float(0.25))]);
        let mut entity = original.clone(); let mut report = PayloadMigrationReport::default();
        migrate_entity(&mut entity, 2230, 5023, &mut report).unwrap();
        migrate_entity(&mut entity, 5023, 2230, &mut report).unwrap();
        assert!(report.issues.is_empty()); assert_eq!(entity, original);
    }

    #[test]
    fn scheduled_ticks_preserve_relative_time_priority_and_order_or_report_exact_loss() {
        let mut ticks = Value::List(vec![map(&[("Block", Value::String("minecraft:stone".into())), ("x", Value::Int(-2)), ("y", Value::Int(5)), ("z", Value::Int(8)), ("Time", Value::Long(27)), ("Priority", Value::Int(-1)), ("SubTick", Value::Long(19))])]);
        let original = ticks.clone(); migrate_ticks(&mut ticks, false, 3465, 5023).unwrap(); assert_eq!(ticks, original);
        let mut sign = tile("minecraft:sign", vec![]); fields(&mut sign).remove("Items"); fields(&mut sign).insert("is_waxed".into(), Value::Byte(1));
        let mut value = region(sign); let original = value.clone(); let report = migrate(&mut value, 3953, 3120).unwrap();
        assert!(report.issues.iter().any(|issue| issue.contains("封蜡"))); assert_eq!(value, original);
    }

    #[test]
    fn nonempty_beehive_migrates_bee_entity_flower_positions_and_occupation_timers() {
        let bee=map(&[("id",Value::String("minecraft:bee".into())),("HasNectar",Value::Byte(1)),("HasStung",Value::Byte(0)),("ArmorItems",Value::List(vec![map(&[]);4])),("HandItems",Value::List(vec![map(&[]);2])),("FlowerPos",map(&[("X",Value::Int(-1)),("Y",Value::Int(64)),("Z",Value::Int(5))]))]);
        let mut hive=tile("minecraft:beehive",vec![]);fields(&mut hive).remove("Items");fields(&mut hive).insert("Bees".into(),Value::List(vec![map(&[("EntityData",bee),("TicksInHive",Value::Int(432)),("MinOccupationTicks",Value::Int(2400))])]));
        let mut value=region(hive);let original=value.clone();let result=migrate(&mut value,3700,5023).unwrap();assert!(result.issues.is_empty(),"{:?}",result.issues);
        let result=migrate(&mut value,5023,3700).unwrap();assert!(result.issues.is_empty(),"{:?}",result.issues);assert_eq!(value,original);
    }

    #[test]
    fn villager_trade_inventory_written_books_and_gossip_are_migrated_without_reset() {
        let book=map(&[("id",Value::String("minecraft:written_book".into())),("Count",Value::Byte(1)),("tag",map(&[("title",Value::String("书名".into())),("author",Value::String("作者".into())),("pages",Value::List(vec![Value::String("{\"color\":\"red\",\"text\":\"正文\"}".into())]))]))]);
        let mut entity=map(&[("id",Value::String("minecraft:villager".into())),("VillagerData",map(&[("type",Value::String("minecraft:plains".into())),("profession",Value::String("minecraft:librarian".into())),("level",Value::Int(3))])),("Inventory",Value::List(vec![book.clone()])),("Offers",map(&[("Recipes",Value::List(vec![map(&[("buy",map(&[("id",Value::String("minecraft:emerald".into())),("Count",Value::Byte(17))])),("sell",book),("uses",Value::Int(4)),("maxUses",Value::Int(12)),("xp",Value::Int(5))])]))])),("Gossips",Value::List(vec![map(&[("Target",Value::IntArray(fastnbt::IntArray::new(vec![1,2,3,4]))),("Type",Value::String("minor_positive".into())),("Value",Value::Int(9))])])),("ArmorItems",Value::List(vec![map(&[]);4])),("HandItems",Value::List(vec![map(&[]);2]))]);
        let original=entity.clone();let mut report=PayloadMigrationReport::default();migrate_entity(&mut entity,3700,5023,&mut report).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);migrate_entity(&mut entity,5023,3700,&mut report).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);assert_eq!(entity,original);
    }

    #[test]
    fn vanilla_effects_hidden_chain_custom_potions_and_hideflags_cross_schema_boundaries() {
        let effect=map(&[("Id",Value::Byte(1)),("Amplifier",Value::Byte(2)),("Duration",Value::Int(80)),("ShowParticles",Value::Byte(1)),("HiddenEffect",map(&[("Id",Value::Byte(1)),("Amplifier",Value::Byte(0)),("Duration",Value::Int(200))]))]);
        let mut stack=map(&[("id",Value::String("minecraft:potion".into())),("Count",Value::Byte(1)),("tag",map(&[("Potion",Value::String("minecraft:swiftness".into())),("CustomPotionEffects",Value::List(vec![effect])),("CustomPotionColor",Value::Int(12345)),("Unbreakable",Value::Byte(1)),("HideFlags",Value::Int(4))]))]);
        let original=stack.clone();let mut report=PayloadMigrationReport::default();item_payload(&mut stack,3465,5023,&mut report).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);item_payload(&mut stack,5023,3465,&mut report).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);assert_eq!(stack,original);
    }

    #[test]
    fn spawner_weighted_entities_and_furnace_inventory_times_really_cross_formats() {
        let mut spawner=tile("minecraft:mob_spawner",vec![]);fields(&mut spawner).remove("Items");fields(&mut spawner).insert("SpawnData".into(),map(&[("id",Value::String("minecraft:zombie".into()))]));fields(&mut spawner).insert("SpawnPotentials".into(),Value::List(vec![map(&[("Entity",map(&[("id",Value::String("minecraft:zombie".into()))])),("Weight",Value::Int(5))])]));
        let mut value=region(spawner);let original=value.clone();let report=migrate(&mut value,2586,5023).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);let report=migrate(&mut value,5023,2586).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);
        // Empty equipment arrays may be synthesized by the target's own codec.
        assert!(!report.changes.is_empty());assert_ne!(value,original);
        let mut furnace=tile("minecraft:furnace",vec![item(3465,2,0)]);fields(&mut furnace).insert("BurnTime".into(),Value::Short(100));fields(&mut furnace).insert("CookTime".into(),Value::Short(30));fields(&mut furnace).insert("CookTimeTotal".into(),Value::Short(200));let mut value=region(furnace);let original=value.clone();let report=migrate(&mut value,3465,5023).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);let report=migrate(&mut value,5023,3465).unwrap();assert!(report.issues.is_empty(),"{:?}",report.issues);assert_eq!(value,original);
    }

    #[test]
    fn decorative_contents_cross_actual_boundaries_and_reverse_without_loss() {
        let text = map(&[
            (
                "messages",
                Value::List(vec![
                    Value::String("\"办公区\"".into()),
                    Value::String("\"\"".into()),
                    Value::String("\"\"".into()),
                    Value::String("\"\"".into()),
                ]),
            ),
            ("color", Value::String("black".into())),
            ("has_glowing_text", Value::Byte(0)),
        ]);
        let mut sign = tile("minecraft:hanging_sign", vec![]);
        fields(&mut sign).remove("Items");
        fields(&mut sign).insert("front_text".into(), text.clone());
        fields(&mut sign).insert("back_text".into(), text);
        fields(&mut sign).insert("is_waxed".into(), Value::Byte(1));
        let mut pot = tile("minecraft:decorated_pot", vec![]);
        fields(&mut pot).remove("Items");
        fields(&mut pot).insert(
            "sherds".into(),
            Value::List(
                [
                    "minecraft:archer_pottery_sherd",
                    "minecraft:brick",
                    "minecraft:angler_pottery_sherd",
                    "minecraft:arms_up_pottery_sherd",
                ]
                .into_iter()
                .map(|id| Value::String(id.into()))
                .collect(),
            ),
        );
        fields(&mut pot).insert(
            "item".into(),
            map(&[
                ("id", Value::String("minecraft:diamond".into())),
                ("count", Value::Int(7)),
            ]),
        );
        let mut banner = tile("minecraft:banner", vec![]);
        fields(&mut banner).remove("Items");
        fields(&mut banner).insert(
            "patterns".into(),
            Value::List(vec![map(&[
                ("color", Value::String("pink".into())),
                ("pattern", Value::String("minecraft:circle".into())),
            ])]),
        );
        let mut value = region(sign.clone());
        fields(&mut value).insert("TileEntities".into(), Value::List(vec![banner, pot, sign]));
        let original = value.clone();
        let report = migrate(&mut value, 3953, 5023).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_ne!(value, original);
        let report = migrate(&mut value, 5023, 3953).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(value, original);
    }

    #[test]
    fn hanging_entity_coordinates_items_and_boat_identity_really_migrate() {
        let mut frame = map(&[
            ("id", Value::String("minecraft:item_frame".into())),
            ("TileX", Value::Int(-8)),
            ("TileY", Value::Int(12)),
            ("TileZ", Value::Int(31)),
            ("FallDistance", Value::Float(0.25)),
            (
                "Item",
                map(&[
                    ("id", Value::String("minecraft:enchanted_book".into())),
                    ("count", Value::Int(1)),
                    (
                        "components",
                        map(&[(
                            "minecraft:stored_enchantments",
                            map(&[("levels", map(&[("minecraft:smite", Value::Int(5))]))]),
                        )]),
                    ),
                ]),
            ),
        ]);
        let original = frame.clone();
        let mut report = PayloadMigrationReport::default();
        migrate_entity(&mut frame, 3953, 5023, &mut report).unwrap();
        assert!(report.issues.is_empty());
        assert_eq!(
            fields(&mut frame)["block_pos"],
            Value::IntArray(fastnbt::IntArray::new(vec![-8, 12, 31]))
        );
        assert_eq!(fields(&mut frame)["fall_distance"], Value::Double(0.25));
        migrate_entity(&mut frame, 5023, 3953, &mut report).unwrap();
        assert_eq!(frame, original);
        let mut boat = map(&[
            ("id", Value::String("minecraft:boat".into())),
            ("Type", Value::String("oak".into())),
        ]);
        let original = boat.clone();
        migrate_entity(&mut boat, 3953, 5023, &mut report).unwrap();
        assert_eq!(
            fields(&mut boat)["id"],
            Value::String("minecraft:oak_boat".into())
        );
        migrate_entity(&mut boat, 5023, 3953, &mut report).unwrap();
        assert_eq!(boat, original);
    }

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Compound(
            entries
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect(),
        )
    }
    fn item(source: i32, count: i32, slot: i8) -> Value {
        map(&[
            ("id", Value::String("minecraft:stone".into())),
            ("Slot", Value::Byte(slot)),
            (
                if source == 3465 { "Count" } else { "count" },
                if source == 3465 {
                    Value::Byte(count as i8)
                } else {
                    Value::Int(count)
                },
            ),
        ])
    }
    #[test]
    fn plain_container_crosses_legacy_flattening_using_actual_item_and_entity_ids() {
        let stack = map(&[
            ("id", Value::String("minecraft:log".into())),
            ("Damage", Value::Short(0)),
            ("Count", Value::Byte(3)),
            ("Slot", Value::Byte(0)),
        ]);
        let mut value = region(tile("minecraft:chest", vec![stack]));
        let original = value.clone();
        let report = migrate(&mut value, 1343, 5023).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        let Value::List(tiles) = fields(&mut value).get_mut("TileEntities").unwrap() else {
            unreachable!()
        };
        let Value::List(items) = fields(&mut tiles[0]).get_mut("Items").unwrap() else {
            unreachable!()
        };
        assert_eq!(
            fields(&mut items[0])["id"],
            Value::String("minecraft:oak_log".into())
        );
        assert_eq!(fields(&mut items[0])["count"], Value::Int(3));
        assert!(!fields(&mut items[0]).contains_key("Damage"));
        let report = migrate(&mut value, 5023, 1343).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(value, original);
    }
    #[test]
    fn actual_plain_and_styled_custom_names_change_json_nbt_encoding_without_loss() {
        let value = Value::String("库房".into());
        let json = migrate_component(&value, 5023, 3465).unwrap();
        assert_eq!(json, Value::String("\"库房\"".into()));
        assert_eq!(migrate_component(&json, 3465, 5023).unwrap(), value);
        let plain = Value::String("旧仓库".into());
        let converted = migrate_component(&plain, 1343, 5023).unwrap();
        assert_eq!(migrate_component(&converted, 5023, 1343).unwrap(), plain);
        let rich = map(&[
            ("text", Value::String("红字".into())),
            ("color", Value::String("red".into())),
            ("bold", Value::Byte(1)),
        ]);
        let json = migrate_component(&rich, 5023, 3465).unwrap();
        assert_eq!(migrate_component(&json, 3465, 5023).unwrap(), rich);
        assert!(migrate_component(&rich, 5023, 1343).is_err());
    }
    #[test]
    fn supported_loot_table_identifier_seed_are_preserved_and_invalid_ids_block() {
        let mut container = tile("minecraft:chest", vec![]);
        fields(&mut container).insert(
            "LootTable".into(),
            Value::String("minecraft:chests/simple_dungeon".into()),
        );
        fields(&mut container).insert("LootTableSeed".into(), Value::Long(17));
        let mut value = region(container);
        let original = value.clone();
        assert!(migrate(&mut value, 3465, 5023).unwrap().issues.is_empty());
        assert_eq!(value, original);
        assert!(!migrate(&mut value, 3465, 0).unwrap().issues.is_empty());
        assert_eq!(value, original);
    }
    fn tile(id: &str, items: Vec<Value>) -> Value {
        map(&[
            ("id", Value::String(id.into())),
            ("x", Value::Int(2)),
            ("y", Value::Int(-3)),
            ("z", Value::Int(4)),
            ("Items", Value::List(items)),
        ])
    }
    fn region(tile: Value) -> Value {
        map(&[
            ("TileEntities", Value::List(vec![tile])),
            ("Entities", Value::List(vec![])),
            ("PendingBlockTicks", Value::List(vec![])),
            ("unrelated-region-field", Value::String("保持原样".into())),
        ])
    }
    fn fields(value: &mut Value) -> &mut HashMap<String, Value> {
        let Value::Compound(fields) = value else {
            panic!("compound fixture")
        };
        fields
    }

    #[test]
    fn empty_containers_all_verified_directions_are_lossless() {
        for source in [3465, 4671, 5023] {
            for target in [3465, 4671, 5023] {
                for id in [
                    "minecraft:chest",
                    "minecraft:barrel",
                    "minecraft:shulker_box",
                    "minecraft:hopper",
                ] {
                    let mut value = region(tile(id, vec![]));
                    let original = value.clone();
                    let result = migrate(&mut value, source, target).unwrap();
                    assert!(result.issues.is_empty());
                    assert!(result.changes.is_empty());
                    assert_eq!(value, original);
                }
            }
        }
    }

    #[test]
    fn legacy_plain_count_and_slot_round_trip_without_changing_coordinates() {
        let mut value = region(tile("minecraft:chest", vec![item(3465, 64, 26)]));
        let original = value.clone();
        let upgrade = migrate(&mut value, 3465, 5023).unwrap();
        assert!(upgrade.issues.is_empty());
        assert_eq!(upgrade.changes.len(), 1);
        let Value::List(tiles) = fields(&mut value).get_mut("TileEntities").unwrap() else {
            unreachable!()
        };
        let Value::List(items) = fields(&mut tiles[0]).get_mut("Items").unwrap() else {
            unreachable!()
        };
        assert_eq!(fields(&mut items[0]).get("count"), Some(&Value::Int(64)));
        assert!(!fields(&mut items[0]).contains_key("Count"));
        let down = migrate(&mut value, 5023, 3465).unwrap();
        assert!(down.issues.is_empty());
        assert_eq!(value, original);
    }

    #[test]
    fn modern_versions_preserve_plain_payload_and_default_missing_count() {
        let mut stack = item(5023, 1, 0);
        fields(&mut stack).remove("count");
        let mut value = region(tile("minecraft:barrel", vec![stack]));
        let original = value.clone();
        assert!(migrate(&mut value, 5023, 4671).unwrap().issues.is_empty());
        assert_eq!(value, original);
        let report = migrate(&mut value, 4671, 3465).unwrap();
        assert!(report.issues.is_empty());
        assert_eq!(report.changes.len(), 1);
    }

    #[test]
    fn unsupported_components_names_entities_ticks_and_unknown_keys_are_atomic() {
        let mut values = Vec::new();
        for key in ["components", "Damage", "mod:unknown"] {
            let mut stack = item(3465, 2, 0);
            fields(&mut stack).insert(key.into(), map(&[]));
            values.push(region(tile(
                "minecraft:chest",
                vec![item(3465, 1, 1), stack],
            )));
        }
        for key in ["CustomName", "Lock", "lock", "LootTable", "mod:payload"] {
            let mut container = tile("minecraft:barrel", vec![]);
            fields(&mut container).insert(key.into(), Value::String("保持原数据".into()));
            values.push(region(container));
        }
        for key in [
            "Entities",
            "PendingBlockTicks",
            "PendingFluidTicks",
            "BlockEntities",
        ] {
            let mut value = region(tile("minecraft:chest", vec![item(3465, 1, 0)]));
            fields(&mut value).insert(key.into(), Value::List(vec![map(&[])]));
            values.push(value);
        }
        for mut value in values {
            let original = value.clone();
            let report = migrate(&mut value, 3465, 5023).unwrap();
            assert!(!report.issues.is_empty());
            assert!(report.changes.is_empty());
            assert_eq!(value, original);
        }
    }

    #[test]
    fn registry_and_count_limits_do_not_replace_or_clamp_stacks() {
        assert!(target_has_item(5023, "minecraft:mace").unwrap());
        assert!(!target_has_item(3465, "minecraft:mace").unwrap());
        let mut unknown = item(5023, 2, 0);
        fields(&mut unknown).insert(
            "id".into(),
            Value::String("minecraft:not_a_real_item".into()),
        );
        let mut new_item = item(5023, 1, 0);
        fields(&mut new_item).insert("id".into(), Value::String("minecraft:mace".into()));
        for (source, target, stack) in [
            (5023, 3465, unknown),
            (5023, 3465, new_item),
            (3465, 5023, item(3465, 127, 0)),
            (5023, 3465, item(5023, 100, 0)),
            (3465, 5023, item(3465, 0, 0)),
        ] {
            let mut value = region(tile("minecraft:chest", vec![stack]));
            let original = value.clone();
            let report = migrate(&mut value, source, target).unwrap();
            assert!(!report.issues.is_empty());
            assert_eq!(value, original);
        }
    }

    #[test]
    fn invalid_or_duplicate_slots_and_unknown_container_types_block() {
        for (id, items) in [
            ("minecraft:hopper", vec![item(3465, 1, 5)]),
            ("minecraft:chest", vec![item(3465, 1, -1)]),
            ("minecraft:barrel", vec![item(3465, 1, 0), item(3465, 2, 0)]),
            ("minecraft:not_a_real_container", vec![]),
        ] {
            let mut value = region(tile(id, items));
            let original = value.clone();
            assert!(!migrate(&mut value, 3465, 5023).unwrap().issues.is_empty());
            assert_eq!(value, original);
        }
    }

    #[test]
    fn modern_empty_component_carrier_and_hopper_cooldown_are_preserved() {
        let mut container = tile("minecraft:hopper", vec![item(5023, 3, 4)]);
        fields(&mut container).insert("components".into(), map(&[]));
        fields(&mut container).insert("TransferCooldown".into(), Value::Int(-1));
        let mut value = region(container);
        assert!(migrate(&mut value, 5023, 3465).unwrap().issues.is_empty());
        let Value::List(tiles) = fields(&mut value).get_mut("TileEntities").unwrap() else {
            unreachable!()
        };
        assert_eq!(fields(&mut tiles[0]).get("components"), Some(&map(&[])));
        assert_eq!(
            fields(&mut tiles[0]).get("TransferCooldown"),
            Some(&Value::Int(-1))
        );
    }

    #[test]
    fn missing_inventory_stays_missing_and_unverified_versions_block() {
        let mut container = tile("minecraft:shulker_box", vec![]);
        fields(&mut container).remove("Items");
        let mut value = region(container);
        let original = value.clone();
        assert!(migrate(&mut value, 3465, 4671).unwrap().issues.is_empty());
        assert_eq!(value, original);
        assert!(!migrate(&mut value, 123456, 4671).unwrap().issues.is_empty());
        assert_eq!(value, original);
    }
}
