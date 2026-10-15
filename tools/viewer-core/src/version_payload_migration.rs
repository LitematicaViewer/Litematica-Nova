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

// Ordinary payload syntax validation; no dependency on a renderer crate.
fn valid_payload_identifier(value: &str) -> bool {
    let (namespace, path) = value.split_once(':').unwrap_or(("minecraft", value));
    namespace != ".."
        && namespace.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.'))
        && path.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.' | b'/'))
}

#[cfg(test)]
#[test]
fn payload_identifier_preserves_existing_character_semantics() {
    for value in ["minecraft:chests/test", ":test", "test", "minecraft:", "minecraft:../a", "a.b:test"] { assert!(valid_payload_identifier(value), "{value}"); }
    for value in ["Minecraft:test", "minecraft:A", "a:b:c", "..:test", "minecraft:bad path", "minecraft:测试"] { assert!(!valid_payload_identifier(value), "{value}"); }
}

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
    for field in ["BlockEntities", "PendingBlockTicks", "PendingFluidTicks"] {
        if let Some(payload) = fields.get(field) {
            if !matches!(payload, Value::List(entries) if entries.is_empty()) {
                report.issues.push(format!(
                    "{field}：此实体、计划刻或非标准载荷字段未覆盖，原数据保留"
                ));
            }
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
            }
            _ => report.issues.push("TileEntities 不是 List".into()),
        }
    }
    if report.issues.is_empty() {
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
        id => id,
    }
    .to_owned();
    let slots = match id.as_str() {
        "minecraft:chest" | "minecraft:barrel" | "minecraft:shulker_box" => 27,
        "minecraft:hopper" => 5,
        "minecraft:dispenser" | "minecraft:dropper" => 9,
        _ if source >= 3818 && target >= 3818 => {
            if let Err(error) = migrate_decorative(fields, &id, source, target, report) {
                report.issues.push(format!("{path}/{id}：{error}"));
            }
            return Ok(());
        }
        _ => {
            report
                .issues
                .push(format!("{path}/{id}：该方块实体尚未覆盖"));
            return Ok(());
        }
    };
    let hopper = id == "minecraft:hopper";
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
                if target < 169 || !valid_payload_identifier(id) {
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
            && !(source >= 3818 && key == "components")
        {
            report.issues.push(format!(
                "{path}/{key}：只覆盖无 tag/components 的普通 ItemStack"
            ));
        }
    }
    if let Some(components) = fields.get_mut("components") {
        if source < 3818 || target < 3818 {
            report.issues.push(format!(
                "{path}：转换器尚未实现旧版物品标签与组件的无损迁移"
            ));
        } else if let Err(error) = migrate_components(components, source, target, report) {
            report.issues.push(format!("{path}：{error}"));
        }
    }
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

fn object(value: &mut Value) -> Result<&mut Fields> {
    match value {
        Value::Compound(fields) => Ok(fields),
        _ => bail!("数据应为复合标签"),
    }
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
    let keys: Vec<_> = fields.keys().cloned().collect();
    for key in keys {
        let component = fields.get_mut(&key).unwrap();
        match key.as_str() {
            "minecraft:custom_name" | "minecraft:item_name" => {
                *component = migrate_component(component, source, target)?
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
            "minecraft:banner_patterns" => validate_patterns(component)?,
            "minecraft:pot_decorations" => migrate_pot_decorations(component, source, target)?,
            "minecraft:repair_cost" | "minecraft:damage" | "minecraft:ominous_bottle_amplifier"
                if matches!(component, Value::Int(_)) => {}
            "minecraft:potion_contents" => {
                let map = object(component)?;
                if map.keys().any(|key| key != "potion")
                    || !matches!(map.get("potion"), Some(Value::String(_)))
                {
                    bail!("转换器尚未实现此药水组件迁移")
                }
            }
            "minecraft:instrument" if matches!(component, Value::String(_)) => {}
            "minecraft:tooltip_display" => {}
            _ => bail!("转换器尚未实现物品组件 {key} 的迁移"),
        }
    }
    if !hidden.is_empty() {
        if fields.contains_key("minecraft:tooltip_display") {
            bail!("物品提示组件存在冲突")
        }
        fields.insert(
            "minecraft:tooltip_display".into(),
            Value::Compound(
                [
                    ("hide_tooltip".into(), Value::Byte(0)),
                    (
                        "hidden_components".into(),
                        Value::List(hidden.into_iter().map(Value::String).collect()),
                    ),
                ]
                .into(),
            ),
        );
    }
    if source >= 4307 && target < 4307 {
        if let Some(display) = fields.remove("minecraft:tooltip_display") {
            let Value::Compound(display) = display else {
                bail!("物品提示组件格式无效")
            };
            if display
                .get("hide_tooltip")
                .is_some_and(|value| value != &Value::Byte(0))
            {
                bail!("转换器尚未实现整项隐藏提示的降级迁移")
            }
            if display
                .keys()
                .any(|key| key != "hide_tooltip" && key != "hidden_components")
            {
                bail!("物品提示组件包含未知字段")
            }
            if let Some(Value::List(hidden)) = display.get("hidden_components") {
                for key in hidden {
                    let Value::String(key) = key else {
                        bail!("物品隐藏组件名称无效")
                    };
                    object(fields.get_mut(key).context("物品隐藏组件缺失")?)?
                        .insert("show_in_tooltip".into(), Value::Byte(0));
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
        "minecraft:banner" => &["patterns"],
        "minecraft:sign" | "minecraft:hanging_sign" => {
            &["front_text", "back_text", "is_waxed", "allow_op_features"]
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
        "minecraft:beacon" => &["Levels", "primary_effect", "secondary_effect"],
        "minecraft:skull" => &["profile", "note_block_sound"],
        _ => bail!("转换器尚未实现该类方块实体迁移，原数据保留"),
    };
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
        "minecraft:banner" => {
            if let Some(patterns) = fields.get("patterns") {
                validate_patterns(patterns)?;
            }
        }
        "minecraft:sign" | "minecraft:hanging_sign" => {
            for side in ["front_text", "back_text"] {
                if let Some(text) = fields.get_mut(side) {
                    let text = object(text)?;
                    if text.keys().any(|key| {
                        !["messages", "filtered_messages", "color", "has_glowing_text"]
                            .contains(&key.as_str())
                    }) {
                        bail!("告示牌文字包含需单独迁移的数据")
                    }
                    for key in ["messages", "filtered_messages"] {
                        if let Some(messages) = text.get_mut(key) {
                            let Value::List(messages) = messages else {
                                bail!("告示牌文字不是列表")
                            };
                            if messages.len() != 4 {
                                bail!("告示牌必须保留四行文字")
                            }
                            for message in messages {
                                *message = migrate_component(message, source, target)?;
                            }
                        }
                    }
                }
            }
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
        _ => {}
    }
    report.changes.push(format!("{id}：保留内容并迁移目标格式"));
    Ok(())
}

/// Applicable official fixes: BoatSplit(v4067), AttributeIdPrefix(v4055),
/// DropChances(v4293), Equipment(v4301), FallDistance(v4303), InlinePos(v4314).
fn migrate_entity(
    entity: &mut Value,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    if source < 3818 || target < 3818 {
        bail!("该实体的旧版载荷迁移尚未实现")
    }
    let fields = object(entity)?;
    let Some(Value::String(id)) = fields.get("id") else {
        bail!("实体名称缺失")
    };
    let id = id.clone();
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
            "minecraft:painting" => &["variant", "facing", "TileX", "TileY", "TileZ", "block_pos"],
            "minecraft:tropical_fish" => &["Variant", "FromBucket"],
            "minecraft:pufferfish" => &["PuffState", "FromBucket"],
            "minecraft:frog" => &["variant", "Age", "ForcedAge", "InLove"],
            "minecraft:parrot" => &["Variant", "Age", "ForcedAge", "InLove", "Owner", "Sitting"],
            _ => bail!("实体 {id} 的转换分支尚未实现"),
        }
    };
    const COMMON: &[&str] = &[
        "id",
        "Pos",
        "Motion",
        "Rotation",
        "UUID",
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
        "ArmorItems",
        "HandItems",
        "equipment",
        "ArmorDropChances",
        "HandDropChances",
        "drop_chances",
        "locator_bar_icon",
    ];
    for key in fields.keys() {
        if !COMMON.contains(&key.as_str()) && !extra.contains(&key.as_str()) {
            bail!("实体 {id} 字段 {key} 的迁移尚未实现")
        }
    }
    if let Some(name) = fields.get_mut("CustomName") {
        *name = migrate_component(name, source, target)?;
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
    if let Some(Value::List(attributes)) = fields.get_mut("attributes") {
        for attribute in attributes {
            let attribute = object(attribute)?;
            if let Some(Value::String(id)) = attribute.get_mut("id") {
                if source < 4055 && target >= 4055 {
                    for prefix in [
                        "minecraft:generic.",
                        "minecraft:horse.",
                        "minecraft:player.",
                        "minecraft:zombie.",
                    ] {
                        if let Some(tail) = id.strip_prefix(prefix) {
                            *id = format!("minecraft:{tail}");
                            break;
                        }
                    }
                } else if source >= 4055 && target < 4055 {
                    let tail = id
                        .strip_prefix("minecraft:")
                        .context("非原版属性需单独降级")?;
                    let prefix = match tail {
                        "jump_strength" => "horse",
                        "block_break_speed"
                        | "block_interaction_range"
                        | "entity_interaction_range" => "player",
                        "spawn_reinforcements" => "zombie",
                        _ => "generic",
                    };
                    *id = format!("minecraft:{prefix}.{tail}");
                }
            }
        }
    }
    if [
        "minecraft:armor_stand",
        "minecraft:tropical_fish",
        "minecraft:pufferfish",
        "minecraft:frog",
        "minecraft:parrot",
    ]
    .contains(&id.as_str())
    {
        migrate_equipment(fields, source, target, report)?;
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
fn migrate_component(value: &Value, source: i32, target: i32) -> Result<Value> {
    if (source < 1458 && target < 1458)
        || ((1458..4290).contains(&source) && (1458..4290).contains(&target))
        || (source >= 4292 && target >= 4292)
    {
        // No text codec boundary crossed. Preserve the exact component rather
        // than rejecting stable click/hover/translatable data as incompatibility.
        return Ok(value.clone());
    }
    fn to_json(value: &Value) -> Result<serde_json::Value> {
        match value {
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::List(values) => Ok(serde_json::Value::Array(
                values.iter().map(to_json).collect::<Result<_>>()?,
            )),
            Value::Compound(fields) => {
                let mut object = serde_json::Map::new();
                for (k, v) in fields {
                    if ![
                        "text",
                        "color",
                        "bold",
                        "italic",
                        "underlined",
                        "strikethrough",
                        "obfuscated",
                        "insertion",
                        "extra",
                        "translate",
                        "fallback",
                        "with",
                        "font",
                        "separator",
                    ]
                    .contains(&k.as_str())
                    {
                        bail!("文本字段 {k} 需要单独迁移")
                    };
                    let value = if [
                        "bold",
                        "italic",
                        "underlined",
                        "strikethrough",
                        "obfuscated",
                    ]
                    .contains(&k.as_str())
                    {
                        match v {
                            Value::Byte(0) => serde_json::Value::Bool(false),
                            Value::Byte(1) => serde_json::Value::Bool(true),
                            _ => bail!("文本样式 {k} 类型不匹配"),
                        }
                    } else {
                        to_json(v)?
                    };
                    object.insert(k.clone(), value);
                }
                Ok(serde_json::Value::Object(object))
            }
            _ => bail!("文本节点类型不匹配"),
        }
    }
    fn to_nbt(value: &serde_json::Value) -> Result<Value> {
        match value {
            serde_json::Value::String(s) => Ok(Value::String(s.clone())),
            serde_json::Value::Bool(b) => Ok(Value::Byte(i8::from(*b))),
            serde_json::Value::Array(values) => Ok(Value::List(
                values.iter().map(to_nbt).collect::<Result<_>>()?,
            )),
            serde_json::Value::Object(fields) => {
                let mut object = std::collections::HashMap::new();
                for (k, v) in fields {
                    if ![
                        "text",
                        "color",
                        "bold",
                        "italic",
                        "underlined",
                        "strikethrough",
                        "obfuscated",
                        "insertion",
                        "extra",
                        "translate",
                        "fallback",
                        "with",
                        "font",
                        "separator",
                    ]
                    .contains(&k.as_str())
                    {
                        bail!("文本字段 {k} 需要单独迁移")
                    };
                    object.insert(k.clone(), to_nbt(v)?);
                }
                Ok(Value::Compound(object))
            }
            _ => bail!("文本节点含不兼容内容"),
        }
    }
    let json = if source < 1458 {
        let Value::String(s) = value else {
            bail!("旧版名称不是字符串")
        };
        serde_json::json!({"text":s})
    } else if source < 4290 {
        let Value::String(s) = value else {
            bail!("名称不是 JSON 字符串")
        };
        let parsed = serde_json::from_str(s).context("名称的文本 JSON 无效")?;
        to_nbt(&parsed)?;
        parsed
    } else {
        to_json(value)?
    };
    if target < 1458 {
        let text = match &json {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Object(fields) if fields.len() == 1 => fields
                .get("text")
                .and_then(|v| v.as_str())
                .context("旧版名称不能无损容纳富文本")?
                .into(),
            _ => bail!("旧版名称不能无损容纳富文本"),
        };
        Ok(Value::String(text))
    } else if target < 4290 {
        Ok(Value::String(serde_json::to_string(&json)?))
    } else {
        to_nbt(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
        for key in ["tag", "components", "Damage", "mod:unknown"] {
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
            ("minecraft:furnace", vec![]),
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
