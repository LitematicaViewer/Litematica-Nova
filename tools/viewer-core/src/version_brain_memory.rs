//! Versioned Brain.memories. A vanilla module being registered is not enough:
//! a missing codec is transient and an absent slot is silently discarded by
//! Brain.setMemoryInternal. Both ends must have a proven serializable slot.
use crate::version_entity_registry::known_version;
use anyhow::{Context, Result, bail};
use fastnbt::Value;
use serde::Deserialize;
use std::{collections::HashMap, sync::LazyLock};

type Versions = Vec<(i32, i32)>;
type Row<'a> = (&'a str, &'a str, Versions);
#[derive(Deserialize)]
struct Data<'a> {
    #[serde(borrow)]
    types: Vec<Row<'a>>,
    #[serde(borrow)]
    owners: Vec<Row<'a>>,
}

static DATA: LazyLock<Data<'static>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/version_brain_memory.json"))
        .expect("generated brain memory registry")
});
const BRAIN: i32 = 1918;
const EXPIRY: i32 = 2505;

fn in_version(r: &[(i32, i32)], version: i32) -> bool {
    r.iter()
        .any(|&(first, last)| first <= version && version <= last)
}
fn codec(owner: &str, key: &str, version: i32) -> Result<&'static str> {
    let Some((_, codec, _)) = DATA
        .types
        .iter()
        .find(|(name, _, r)| *name == key && in_version(r, version))
    else {
        bail!("数据版本 {version} 没有注册记忆 minecraft:{key}")
    };
    match *codec {
        "none" => bail!("数据版本 {version} 的 minecraft:{key} 为瞬态记忆，没有持久化 codec"),
        "other" => bail!("数据版本 {version} 的 minecraft:{key} 使用尚未验证的专用 codec"),
        _ => (),
    };
    if !DATA
        .owners
        .iter()
        .any(|(entity, name, r)| *entity == owner && *name == key && in_version(r, version))
    {
        bail!("数据版本 {version} 的 minecraft:{owner} 缺少可证的持久记忆槽 minecraft:{key}");
    }
    Ok(codec)
}
fn pos(v: &Value, len: usize) -> bool {
    matches!(v, Value::IntArray(a) if a.len()==len)
}
fn global(v: &Value, version: i32) -> bool {
    let Value::Compound(m) = v else { return false };
    let Some(Value::String(dimension)) = m.get("dimension") else {
        return false;
    };
    let Some((namespace, path)) = dimension.split_once(':') else {
        return false;
    };
    // Vanilla DimensionType's pre-custom-dimensions registry contains precisely these IDs.
    let dimension_ok = if version < 2566 {
        matches!(
            dimension.as_str(),
            "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
        )
    } else {
        !namespace.is_empty()
            && !path.is_empty()
            && namespace
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-".contains(&c))
            && path
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-/".contains(&c))
    };
    m.len() == 2 && dimension_ok && m.get("pos").is_some_and(|p| pos(p, 3))
}
fn validate(v: &Value, codec: &str, version: i32) -> Result<()> {
    let valid = match codec {
        "bool" => matches!(v, Value::Byte(0 | 1)),
        "int" => matches!(v, Value::Int(_)),
        "long" => matches!(v, Value::Long(_)),
        "unit" => matches!(v,Value::Compound(m) if m.is_empty()),
        "uuid" => pos(v, 4),
        "pos" => pos(v, 3),
        "global" => global(v, version),
        "global[]" | "pos[]" => {
            matches!(v,Value::List(list) if !list.is_empty()&& list.iter().all(|item|if codec=="global[]"{global(item,version)}else{pos(item,3)}))
        }
        _ => false,
    };
    if !valid {
        bail!("数据版本 {version} 的值不符合原版 {codec} codec（空记忆列表会被游戏丢弃）")
    }
    Ok(())
}

pub(crate) fn migrate(
    fields: &mut HashMap<String, Value>,
    owner: &str,
    source: i32,
    target: i32,
) -> Result<()> {
    let Some(brain) = fields.get("Brain") else {
        return Ok(());
    };
    let Value::Compound(root) = brain else {
        bail!("{owner}/Brain 不是 Compound")
    };
    if root.len() != 1 || !root.contains_key("memories") {
        bail!("{owner}/Brain 必须仅包含 memories")
    }
    let Some(Value::Compound(memories)) = root.get("memories") else {
        bail!("{owner}/Brain.memories 不是 Compound")
    };
    if !known_version(source) || !known_version(target) {
        bail!("{owner}/Brain 缺少 {source} → {target} 的真实版本资料")
    }
    if memories.is_empty() {
        if target < BRAIN {
            fields.remove("Brain");
        }
        return Ok(());
    }
    let owner = owner.strip_prefix("minecraft:").unwrap_or(owner);
    let mut next = HashMap::with_capacity(memories.len());
    for (id, raw) in memories {
        let Some(key) = id
            .strip_prefix("minecraft:")
            .or_else(|| (!id.contains(':')).then_some(id.as_str()))
        else {
            bail!("minecraft:{owner}/Brain.memories.{id} 不在原版记忆注册表")
        };
        let result = (|| -> Result<Value> {
            let source_codec = codec(owner, key, source)?;
            let target_codec = codec(owner, key, target)?;
            if source_codec != target_codec {
                bail!("记忆 codec {source_codec} → {target_codec} 不同，缺少恢复维度/上下文的证据")
            }
            let (val, ttl) = if source >= EXPIRY {
                let Value::Compound(wrapper) = raw else {
                    bail!("缺少 ExpirableValue 包装")
                };
                if wrapper.keys().any(|k| k != "value" && k != "ttl") {
                    bail!("ExpirableValue 含未知字段")
                }
                let ttl = match wrapper.get("ttl") {
                    None => None,
                    Some(Value::Long(n)) if *n >= 0 => Some(*n),
                    _ => bail!("ttl 须为非负 Long"),
                };
                (
                    wrapper.get("value").context("ExpirableValue 缺少 value")?,
                    ttl,
                )
            } else {
                (raw, None)
            };
            validate(val, source_codec, source)?;
            validate(val, target_codec, target)?;
            if target < EXPIRY {
                if ttl.is_some() {
                    bail!("旧版记忆没有 ttl，无法等价保留过期时间")
                }
                Ok(val.clone())
            } else {
                let mut wrapper = HashMap::from([("value".into(), val.clone())]);
                if let Some(ttl) = ttl {
                    wrapper.insert("ttl".into(), Value::Long(ttl));
                }
                Ok(Value::Compound(wrapper))
            }
        })();
        let standardized = format!("minecraft:{key}");
        let value =
            result.map_err(|e| anyhow::anyhow!("minecraft:{owner}/Brain.memories.{id}: {e}"))?;
        if next.insert(standardized, value).is_some() {
            bail!("minecraft:{owner}/Brain.memories.{id} 与已有记忆ID重复")
        }
    }
    fields.insert(
        "Brain".into(),
        Value::Compound(HashMap::from([("memories".into(), Value::Compound(next))])),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fields(id: &str, v: Value) -> HashMap<String, Value> {
        HashMap::from([(
            "Brain".into(),
            Value::Compound(HashMap::from([(
                "memories".into(),
                Value::Compound(HashMap::from([(id.into(), v)])),
            )])),
        )])
    }
    fn wrap(v: Value) -> Value {
        Value::Compound(HashMap::from([("value".into(), v)]))
    }
    #[test]
    fn villager_expiry_round_trip_and_failure_keeps_source() {
        let mut m = fields("minecraft:last_slept", Value::Long(12));
        let original = m.clone();
        migrate(&mut m, "minecraft:villager", 2230, 5023).unwrap();
        assert_eq!(m, fields("minecraft:last_slept", wrap(Value::Long(12))));
        migrate(&mut m, "minecraft:villager", 5023, 2230).unwrap();
        assert_eq!(m, original);
        let mut timed = fields(
            "minecraft:last_slept",
            Value::Compound(HashMap::from([
                ("value".into(), Value::Long(12)),
                ("ttl".into(), Value::Long(20)),
            ])),
        );
        let original = timed.clone();
        let err = migrate(&mut timed, "minecraft:villager", 5023, 2230)
            .unwrap_err()
            .to_string();
        assert!(err.contains("last_slept") && err.contains("ttl"));
        assert_eq!(timed, original);
    }
    #[test]
    fn piglin_memory_checks_codec_registration_and_owner() {
        let mut good = fields(
            "minecraft:angry_at",
            wrap(Value::IntArray(fastnbt::IntArray::new(vec![1, 2, 3, 4]))),
        );
        migrate(&mut good, "minecraft:piglin", 2566, 5023).unwrap();
        assert!(
            migrate(&mut good, "minecraft:bat", 2566, 5023)
                .unwrap_err()
                .to_string()
                .contains("angry_at")
        );
        let mut malformed = fields("minecraft:angry_at", wrap(Value::Int(4)));
        assert!(
            migrate(&mut malformed, "minecraft:piglin", 2566, 5023)
                .unwrap_err()
                .to_string()
                .contains("uuid")
        );
        let mut transient = fields("minecraft:attack_target", wrap(Value::Int(1)));
        assert!(
            migrate(&mut transient, "minecraft:piglin", 2566, 5023)
                .unwrap_err()
                .to_string()
                .contains("瞬态")
        );
    }
}
