//! EntityBlockStateFix(v1451.3) and PrimedTntBlockStateFixer(v3683). Falling
//! blocks merge TileEntityData into the *landed block*, not a free-standing
//! entity: infer missing metadata only from that exact release's holder map.
use super::*;
use std::collections::BTreeMap;
use std::sync::LazyLock;

#[derive(serde::Deserialize)]
struct Catalog<'a> {
    #[serde(borrow)]
    holders: Vec<(&'a str, &'a str, Vec<(i32, i32)>)>,
    #[serde(borrow)]
    numeric: BTreeMap<&'a str, u16>,
}
static CATALOG: LazyLock<Catalog<'static>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/version_falling_block.json"))
        .expect("generated falling block relations")
});

fn state_parts(value: &Value, version: i32) -> Result<(&str, BTreeMap<String, String>)> {
    let Value::Compound(m) = value else {
        bail!("下落方块BlockState不是Compound")
    };
    let (id, props) = if version < 5006 {
        ("Name", "Properties")
    } else {
        ("id", "properties")
    };
    let Some(Value::String(name)) = m.get(id) else {
        bail!("内嵌状态缺少{id}")
    };
    let properties = match m.get(props) {
        None => BTreeMap::new(),
        Some(Value::Compound(props)) => props
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => Ok((k.clone(), s.clone())),
                _ => bail!("方块属性{k}不是String"),
            })
            .collect::<Result<_>>()?,
        _ => bail!("内嵌状态属性不是Compound"),
    };
    if m.keys().any(|k| k != id && k != props) {
        bail!("内嵌状态含有未知字段")
    }
    Ok((name, properties))
}
fn state(name: &str, properties: BTreeMap<String, String>, version: i32) -> Value {
    let (id, props) = if version < 5006 {
        ("Name", "Properties")
    } else {
        ("id", "properties")
    };
    let mut m = Fields::from([(id.into(), Value::String(name.into()))]);
    if !properties.is_empty() {
        m.insert(
            props.into(),
            Value::Compound(
                properties
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect(),
            ),
        );
    }
    Value::Compound(m)
}

fn legacy_state(fields: &mut Fields, source: i32) -> Result<Option<Value>> {
    let mut tags = ["Block", "TileID", "Tile"]
        .into_iter()
        .filter_map(|k| fields.remove(k).map(|v| (k, v)));
    let first = tags.next();
    if tags.next().is_some() {
        bail!("下落方块存在多种旧方块身份，保留原数据")
    };
    let meta = match fields.remove("Data") {
        None => 0,
        Some(Value::Byte(v @ 0..=15)) => v,
        _ => bail!("下落方块Data不是0..15的Byte"),
    };
    let id = match first {
        None if meta == 0 => return Ok(None),
        None => bail!("下落方块Data缺少方块身份"),
        Some(("Tile", Value::Byte(n))) => u16::from(n as u8),
        Some(("Block" | "TileID", Value::Int(n))) => u16::try_from(n)?,
        Some(("Block", Value::String(n))) => *CATALOG
            .numeric
            .get(n.as_str())
            .with_context(|| format!("下落方块旧Block名称没有原版映射 {n}"))?,
        _ => bail!("下落方块旧Block/TileID/Tile类型不符"),
    };
    let reg = crate::block_database::database()?.registry_by_data_version(source)?;
    let key = reg
        .legacy_blocks
        .get(&format!("{id}:{meta}"))
        .context("旧下落方块数值ID/元数据没有真实状态映射")?;
    let (name, props) = crate::version_conversion::parse_key(key)?;
    Ok(Some(state(&name, props, 1519)))
}

fn emit_legacy_state(fields: &mut Fields, value: &Value, schema: i32, target: i32) -> Result<()> {
    let (name, mut props) = state_parts(value, schema)?;
    // Resolve omitted defaults against their actual source, rather than
    // selecting an arbitrary matching legacy metadata variant.
    let db = crate::block_database::database()?;
    if schema >= 1519 {
        let reg = db.registry_by_data_version(schema)?;
        let def = reg.blocks.get(name).context("下落方块来源状态缺少定义")?;
        for (k, v) in crate::version_conversion::parse_key(&def.default)?.1 {
            props.entry(k).or_insert(v);
        }
    }
    let reg = db.registry_by_data_version(target)?;
    let best = reg
        .legacy_blocks
        .iter()
        .filter_map(|(code, key)| {
            let (n, p) = crate::version_conversion::parse_key(key).ok()?;
            if n != name || p != props {
                return None;
            }
            let (id, data) = code.split_once(':')?;
            Some((id.parse::<u16>().ok()?, data.parse::<i8>().ok()?))
        })
        .min()
        .with_context(|| format!("目标旧版没有等价下落方块 {name}{props:?}"))?;
    let old = CATALOG
        .numeric
        .iter()
        .find(|(_, n)| **n == best.0)
        .context("目标旧Block名称没有原版映射")?;
    fields.insert("Block".into(), Value::String((*old.0).into()));
    fields.insert("Data".into(), Value::Byte(best.1));
    Ok(())
}

fn holder(name: &str, version: i32) -> Result<&'static str> {
    CATALOG
        .holders
        .iter()
        .find(|(b, _, ranges)| *b == name && ranges.iter().any(|r| (r.0..=r.1).contains(&version)))
        .map(|(_, t, _)| *t)
        .with_context(|| format!("版本{version}的{name}无可验证TileEntityData载体"))
}
fn migrate_tile(
    value: &mut Value,
    from: &str,
    to: &str,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    let source_type = holder(from, source)?;
    let target_type = holder(to, target)?;
    let source_raw = report
        .output_id(Kind::BlockEntity, source, source_type)?
        .context("原方块实体保存名称缺失")?;
    let fields = object(value)?;
    let has_id = fields.contains_key("id");
    if let Some(v) = fields.get("id") {
        let Value::String(raw) = v else {
            bail!("TileEntityData/id不是String")
        };
        if report.input_id(Kind::BlockEntity, source, raw)? != Some(source_type) {
            bail!("TileEntityData/id={raw}与原方块{from}的实际载体不符")
        }
    } else {
        fields.insert("id".into(), Value::String(source_raw.into()));
    }
    if source_type != target_type {
        bail!("真实方块载体由{source_type}变为{target_type}，需要专门迁移")
    }
    let missing: Vec<_> = ["x", "y", "z"]
        .into_iter()
        .filter(|k| !fields.contains_key(*k))
        .collect();
    for k in &missing {
        fields.insert((*k).into(), Value::Int(0));
    }
    migrate_container(value, source, target, "下落方块/TileEntityData", report)?;
    let fields = object(value)?;
    if !has_id {
        fields.remove("id");
    }
    for k in missing {
        fields.remove(k);
    }
    Ok(())
}

pub(super) fn handles(id: &str, key: &str) -> bool {
    match id {
        "minecraft:falling_block" => matches!(
            key,
            "Block"
                | "Data"
                | "Tile"
                | "TileID"
                | "BlockState"
                | "Time"
                | "DropItem"
                | "HurtEntities"
                | "FallHurtAmount"
                | "FallHurtMax"
                | "TileEntityData"
                | "CancelDrop"
        ),
        "minecraft:tnt" => matches!(
            key,
            "Fuse" | "fuse" | "block_state" | "explosion_power" | "owner"
        ),
        _ => false,
    }
}
pub(super) fn migrate(
    fields: &mut Fields,
    id: &str,
    source: i32,
    target: i32,
    report: &mut PayloadMigrationReport,
) -> Result<()> {
    if id == "minecraft:tnt" {
        return tnt(fields, source, target);
    }
    if id != "minecraft:falling_block" {
        return Ok(());
    }
    if ["DropItem", "HurtEntities", "CancelDrop"].iter().any(|k| {
        fields
            .get(*k)
            .is_some_and(|v| !matches!(v, Value::Byte(0 | 1)))
    }) || fields
        .get("FallHurtAmount")
        .is_some_and(|v| !matches!(v,Value::Float(n) if n.is_finite()))
        || fields
            .get("FallHurtMax")
            .is_some_and(|v| !matches!(v, Value::Int(_)))
    {
        bail!("下落方块破坏/掉落/伤害字段类型无效")
    }
    if source < 3463 && fields.contains_key("CancelDrop") {
        bail!("来源1.20以前没有CancelDrop")
    }
    if target < 3463
        && fields
            .remove("CancelDrop")
            .is_some_and(|v| v != Value::Byte(0))
    {
        bail!("目标旧下落方块不保存CancelDrop")
    }
    if let Some(v) = fields.get_mut("Time") {
        let n = match (&*v, source < 169) {
            (Value::Byte(n), true) => i32::from(*n as u8),
            (Value::Int(n), false) => *n,
            _ => bail!("下落方块Time与来源版本不符"),
        };
        *v = if target < 169 {
            Value::Byte(u8::try_from(n).context("目标1.8的下落时间超出0..255")? as i8)
        } else {
            Value::Int(n)
        };
    }
    let schema = if source < 1519 { 1519 } else { source };
    let mut block = if source < 1519 {
        if fields.contains_key("BlockState") {
            bail!("来源旧下落方块混入现代BlockState")
        }
        legacy_state(fields, source)?
    } else {
        if ["Block", "Tile", "TileID", "Data"]
            .iter()
            .any(|k| fields.contains_key(*k))
        {
            bail!("现代下落方块混入旧方块身份")
        }
        fields.remove("BlockState")
    };
    if let Some(ref mut value) = block {
        let old_name = state_parts(value, schema)?.0.to_owned();
        if source >= 1519 {
            migrate_stored_block_state(value, source, source)?;
        }
        let new_name = if target >= 1519 {
            migrate_stored_block_state(value, schema, target)?;
            let name = state_parts(value, target)?.0.to_owned();
            fields.insert("BlockState".into(), value.clone());
            name
        } else {
            emit_legacy_state(fields, value, schema, target)?;
            old_name.clone()
        };
        if let Some(tile) = fields.get_mut("TileEntityData") {
            migrate_tile(tile, &old_name, &new_name, source, target, report)?;
        }
    } else if fields.contains_key("TileEntityData") {
        bail!("缺少方块状态，不能确定TileEntityData载体")
    }
    Ok(())
}

fn tnt(fields: &mut Fields, source: i32, target: i32) -> Result<()> {
    let (from, to) = (
        if source < 3683 { "Fuse" } else { "fuse" },
        if target < 3683 { "Fuse" } else { "fuse" },
    );
    if fields.contains_key(if from == "Fuse" { "fuse" } else { "Fuse" }) {
        bail!("TNT引信命名与来源不符")
    }
    if let Some(v) = fields.remove(from) {
        let n = match (v, source < 169) {
            (Value::Byte(n), true) => i16::from(n),
            (Value::Short(n), false) => n,
            _ => bail!("TNT引信类型与来源不符"),
        };
        fields.insert(
            to.into(),
            if target < 169 {
                Value::Byte(i8::try_from(n).context("目标1.8引信超过Byte范围")?)
            } else {
                Value::Short(n)
            },
        );
    }
    if source < 3683 && fields.contains_key("block_state") {
        bail!("旧TNT没有独立block_state")
    }
    if let Some(block) = fields.get_mut("block_state") {
        migrate_stored_block_state(block, source, source)?;
        if target < 3683 {
            let (name, props) = state_parts(block, source)?;
            let reg = crate::block_database::database()?.registry_by_data_version(source)?;
            let default = reg.blocks.get("minecraft:tnt").context("TNT缺少默认状态")?;
            let (_, defaults) = crate::version_conversion::parse_key(&default.default)?;
            if name != "minecraft:tnt" || props.iter().any(|(k, v)| defaults.get(k) != Some(v)) {
                bail!("目标旧TNT不能表示自定义block_state")
            }
            fields.remove("block_state");
        } else {
            migrate_stored_block_state(block, source, target)?;
        }
    } else if source < 3683 && target >= 3683 {
        fields.insert(
            "block_state".into(),
            state("minecraft:tnt", BTreeMap::new(), target),
        );
    }
    if let Some(v) = fields.get("explosion_power") {
        if source < 4080
            || !matches!(v,Value::Float(n) if n.is_finite() && (0.0..=128.0).contains(n))
        {
            bail!("TNT爆炸威力与来源codec不符")
        }
        if target < 4080 {
            if *v != Value::Float(4.0) {
                bail!("目标旧TNT只支持默认爆炸威力4")
            }
            fields.remove("explosion_power");
        }
    }
    if let Some(v) = fields.get("owner") {
        uuid_text(v)?;
        if source < 4435 || target < 4435 {
            bail!("旧TNT不保存owner，保留原所有者")
        }
    }
    Ok(())
}
