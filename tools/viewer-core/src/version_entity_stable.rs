//! Typed, unchanged scalar fields. The generator requires a reviewed vanilla
//! field meaning and each exact JAR's typed read *and* write. Never widen this
//! to arbitrary user keys or fields of a different entity.
use crate::version_entity_registry::{known_version, scalar_field};
use anyhow::{Result, bail};
use fastnbt::Value;

pub(crate) fn handles(id: &str, key: &str) -> bool {
    scalar_field(id, key).is_some()
}

pub(crate) fn validate(id: &str, key: &str, value: &Value, source: i32, target: i32) -> Result<()> {
    let Some((codec, ranges)) = scalar_field(id, key) else {
        bail!("{id}.{key} 缺少真实标量读写证据");
    };
    if !known_version(source)
        || !known_version(target)
        || !ranges
            .iter()
            .any(|&(first, last)| first <= source.min(target) && last >= source.max(target))
    {
        bail!("{id}.{key} 在 {source} → {target} 缺少同一标量语义/读写类型的逐版证据");
    }
    // Out-of-range numeric variants and unknown gene strings often decode
    // as a default vanilla appearance. Never report these as a lossless copy.
    let valid = if let Some(domain) = codec.strip_prefix("index:") {
        matches!(value, Value::Int(index) if *index >= 0 && (*index as usize) < domain.split(',').count())
    } else if let Some(domain) = codec.strip_prefix("name:") {
        matches!(value, Value::String(name) if domain.split(',').any(|entry| entry == name.as_str()))
    } else {
        matches!(
            (codec, value),
            ("B", Value::Byte(_))
                | ("Z", Value::Byte(0 | 1))
                | ("I", Value::Int(_))
                | ("J", Value::Long(_))
        )
    };
    if !valid {
        bail!("{id}.{key} 与实际 NBT 类型 {codec} 不符");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_wrong_species_version_and_type() {
        assert!(validate("minecraft:bat", "BatFlags", &Value::Byte(1), 0, 5023).is_ok());
        assert!(validate("minecraft:bee", "HasStung", &Value::Byte(1), 2230, 5023).is_ok());
        assert!(validate("minecraft:bee", "HasStung", &Value::Byte(2), 2230, 5023).is_err());
        assert!(validate("minecraft:bee", "HasStung", &Value::Byte(1), 1631, 5023).is_err());
        assert!(validate("minecraft:bee", "HasStung", &Value::Byte(1), 3818, 5023).is_err());
        assert!(
            validate(
                "minecraft:dolphin",
                "Moistness",
                &Value::Short(2400),
                1519,
                5023
            )
            .is_err()
        );
        assert!(!handles("minecraft:pig", "GotFish"));
    }
    #[test]
    fn appearances_require_exact_version_semantics_and_domain() {
        assert!(validate("minecraft:axolotl", "Variant", &Value::Int(4), 3465, 5023).is_ok());
        assert!(validate("minecraft:axolotl", "Variant", &Value::Int(5), 3465, 5023).is_err());
        assert!(validate("minecraft:axolotl", "Variant", &Value::Byte(1), 3465, 5023).is_err());
        assert!(validate("minecraft:parrot", "Variant", &Value::Int(0), 1139, 5023).is_ok());
        assert!(validate("minecraft:parrot", "Variant", &Value::Int(-1), 1139, 5023).is_err());
        assert!(
            validate(
                "minecraft:panda",
                "HiddenGene",
                &Value::String("brown".into()),
                1952,
                5023
            )
            .is_ok()
        );
        assert!(
            validate(
                "minecraft:panda",
                "MainGene",
                &Value::String("pale".into()),
                1952,
                5023
            )
            .is_err()
        );
        assert!(validate("minecraft:sheep", "Sheared", &Value::Byte(1), 0, 5023).is_ok());
        assert!(validate("minecraft:fox", "Crouching", &Value::Byte(2), 1952, 5023).is_err());
        assert!(
            validate(
                "minecraft:camel",
                "LastPoseTick",
                &Value::Long(-300),
                3465,
                5023
            )
            .is_ok()
        );
    }
}
