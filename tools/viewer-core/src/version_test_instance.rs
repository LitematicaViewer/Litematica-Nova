//! Vanilla TestInstanceBlockEntity.Data and ErrorMarker save/load codecs.
//! V4302 describes nested components for DFU, but the shipped class did not
//! persist error markers until 1.21.9 (4554): 1.21.5–1.21.8 save only data.
use super::*;

// Vec3i.CODEC / BlockPos.CODEC both read a fixed-size int stream, not an
// {x,y,z} compound. Preserve the original serial form when it is lossless.
fn triple(value: Option<&Value>, field: &str) -> Result<()> {
    if !matches!(value, Some(Value::IntArray(n)) if n.len()==3)
        && !matches!(value, Some(Value::List(n)) if n.len()==3 && n.iter().all(|v|matches!(v,Value::Int(_))))
    {
        bail!("测试实例/{field}应为三个Int");
    }
    Ok(())
}

fn text(value: &mut Value, path: &str, source: i32, target: i32) -> Result<()> {
    match value {
        Value::String(_) => {}
        Value::List(v) if !v.is_empty() => {}
        Value::Compound(m)
            if ["text", "translate", "keybind", "score", "selector", "nbt"]
                .iter()
                .any(|key| m.contains_key(*key)) => {}
        _ => bail!("测试实例/{path}不是有效文本组件"),
    }
    *value =
        migrate_component(value, source, target).with_context(|| format!("测试实例/{path}"))?;
    Ok(())
}

/// Called from the ordinary block-entity field dispatch. Common metadata,
/// coordinates and component fields remain the responsibility of the caller.
pub(super) fn migrate(
    fields: &mut Fields,
    source: i32,
    target: i32,
) -> Result<&'static [&'static str]> {
    if source < 4325 || target < 4325 {
        bail!("目标或来源版本没有可保存的测试实例方块实体")
    }
    if let Some(data) = fields.get_mut("data") {
        let m = object(data).context("测试实例/data不是Compound")?;
        if let Some(k) = m.keys().find(|k| {
            !matches!(
                k.as_str(),
                "test" | "size" | "rotation" | "ignore_entities" | "status" | "error_message"
            )
        }) {
            bail!("测试实例/data/{k}没有已验证的字段迁移")
        }
        triple(m.get("size"), "data/size")?;
        if !matches!(m.get("rotation"),Some(Value::String(v)) if matches!(v.as_str(),"none"|"clockwise_90"|"180"|"counterclockwise_90"))
        {
            bail!("测试实例/data/rotation不是原版旋转名")
        }
        if !matches!(m.get("ignore_entities"), Some(Value::Byte(0 | 1))) {
            bail!("测试实例/data/ignore_entities不是Boolean")
        }
        if !matches!(m.get("status"),Some(Value::String(v)) if matches!(v.as_str(),"cleared"|"running"|"finished"))
        {
            bail!("测试实例/data/status不是原版状态")
        }
        // The codec stores a ResourceKey, not a resolved Holder. Keep valid
        // datapack references; conversion does not imply the pack is installed.
        if let Some(test) = m.get("test") {
            let Value::String(id) = test else {
                bail!("测试实例/data/test不是ResourceKey")
            };
            let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
            if namespace.is_empty() || path.is_empty() || !valid_resource_identifier(id) {
                bail!("测试实例/data/test不是有效资源标识")
            }
        }
        if let Some(message) = m.get_mut("error_message") {
            text(message, "data/error_message", source, target)?;
        }
    }
    if let Some(markers) = fields.get_mut("errors") {
        if source < 4554 {
            bail!("来源发行版不保存测试实例/errors")
        }
        let Value::List(markers) = markers else {
            bail!("测试实例/errors不是List")
        };
        if target < 4554 {
            if !markers.is_empty() {
                bail!("目标1.21.9以前不保存测试实例/errors标记，保留原坐标和文本")
            }
            fields.remove("errors");
        } else {
            for (index, marker) in markers.iter_mut().enumerate() {
                let m = object(marker)
                    .with_context(|| format!("测试实例/errors[{index}]不是Compound"))?;
                if let Some(k) = m.keys().find(|k| !matches!(k.as_str(), "pos" | "text")) {
                    bail!("测试实例/errors[{index}]/{k}没有已验证的字段迁移")
                }
                triple(m.get("pos"), &format!("errors[{index}]/pos"))?;
                let msg = m
                    .get_mut("text")
                    .with_context(|| format!("测试实例/errors[{index}]/text缺失"))?;
                text(msg, &format!("errors[{index}]/text"), source, target)?;
            }
        }
    }
    Ok(&["data", "errors"])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region() -> Value {
        read_snbt(r#"{TileEntities:[{id:"minecraft:test_instance_block",x:1,y:2,z:3,data:{test:"pack:timed_door",size:[I;5,4,3],rotation:"180",ignore_entities:0b,status:"finished",error_message:{translate:"test_instance_block.invalid_test",color:"red"}},errors:[{pos:[I;4,-1,2],text:{text:"door remained open",bold:1b}}]}]}"#).unwrap()
    }

    #[test]
    fn registry_test_instance_data_and_errors_survive_modern_roundtrip() {
        let mut v = region();
        let original = v.clone();
        for (source, target) in [(4554, 5023), (5023, 4554)] {
            let report = super::super::migrate(&mut v, source, target).unwrap();
            assert!(report.issues.is_empty(), "{:?}", report.issues);
            assert_eq!(v, original);
        }
        let mut data_only=read_snbt(r#"{TileEntities:[{id:"minecraft:test_instance_block",x:1,y:2,z:3,data:{size:[I;3,2,1],rotation:"clockwise_90",ignore_entities:1b,status:"running",error_message:"diagnostic"}}]}"#).unwrap();
        let original = data_only.clone();
        for (source, target) in [(4325, 5023), (5023, 4325)] {
            let report = super::super::migrate(&mut data_only, source, target).unwrap();
            assert!(report.issues.is_empty(), "{:?}", report.issues);
            assert_eq!(data_only, original);
        }
    }

    #[test]
    fn registry_test_instance_unrepresentable_marker_or_invalid_data_preserves_region() {
        let original = region();
        for (source, target, reason) in [
            (5023, 4440, "不保存测试实例/errors"),
            (4440, 5023, "来源发行版不保存"),
            (5023, 4189, "注册表"),
        ] {
            let mut v = original.clone();
            let report = super::super::migrate(&mut v, source, target).unwrap();
            assert!(
                report.issues.iter().any(|s| s.contains(reason)),
                "{:?}",
                report.issues
            );
            assert_eq!(v, original);
        }
        for (parent, k, bad) in [
            ("data", "rotation", Value::String("clockwise_180".into())),
            ("errors", "pos", Value::Int(5)),
            ("data", "unknown", Value::Int(3)),
        ] {
            let mut v = original.clone();
            let Value::List(tiles) = object(&mut v).unwrap().get_mut("TileEntities").unwrap()
            else {
                panic!()
            };
            let node = object(&mut tiles[0]).unwrap().get_mut(parent).unwrap();
            let node = if let Value::List(markers) = node {
                &mut markers[0]
            } else {
                node
            };
            object(node).unwrap().insert(k.into(), bad);
            let before = v.clone();
            let report = super::super::migrate(&mut v, 4554, 5023).unwrap();
            assert!(
                report.issues.iter().any(|s| s.contains(k)),
                "{:?}",
                report.issues
            );
            assert_eq!(v, before);
        }
        let mut v = original.clone();
        let Value::List(tiles) = object(&mut v).unwrap().get_mut("TileEntities").unwrap() else {
            panic!()
        };
        object(&mut tiles[0])
            .unwrap()
            .insert("errors".into(), Value::List(vec![]));
        let report = super::super::migrate(&mut v, 5023, 4440).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        let Value::List(tiles) = object(&mut v).unwrap().get_mut("TileEntities").unwrap() else {
            panic!()
        };
        assert!(!object(&mut tiles[0]).unwrap().contains_key("errors"));
    }
}
