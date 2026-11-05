//! Conversion-time registry of exact vanilla save IDs; no renderer, Java or
//! user-installed version adapters are required at runtime. Data is generated
//! by tools/mc26-oracle/scripts/generate-version-entity-registry.py.
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Kind {
    Entity,
    BlockEntity,
}

type Scalar<'a> = (&'a str, &'a str, &'a str, Vec<(i32, i32)>);
type Identity<'a> = (&'a str, &'a str, Vec<(i32, i32)>);

#[derive(Deserialize)]
struct Data<'a> {
    #[serde(borrow)]
    versions: Vec<(i32, &'a str, &'a str, &'a str)>,
    #[serde(borrow)]
    living: Vec<&'a str>,
    #[serde(borrow)]
    scalars: Vec<Scalar<'a>>,
    #[serde(borrow)]
    entity: Vec<Identity<'a>>,
    #[serde(borrow)]
    block_entity: Vec<Identity<'a>>,
}

#[derive(Default)]
struct Table {
    // Static strings borrow the compiled-in JSON; only indices are allocated.
    canonical: HashMap<&'static str, &'static str>,
    encoded: HashMap<&'static str, Option<&'static str>>,
}

struct Catalog {
    tables: HashMap<i32, [Table; 2]>,
    living: Vec<&'static str>,
    scalars: Vec<Scalar<'static>>,
}

static REGISTRIES: LazyLock<Catalog> = LazyLock::new(|| {
    let data: Data<'static> =
        serde_json::from_str(include_str!("../assets/version_entity_registry.json"))
            .expect("generated entity/block entity identity catalog");
    let mut out: HashMap<i32, [Table; 2]> = data
        .versions
        .into_iter()
        .map(|(v, ..)| (v, Default::default()))
        .collect();
    for (kind, identities) in [data.entity, data.block_entity].into_iter().enumerate() {
        for (raw, canonical, intervals) in identities {
            for (&version, tables) in &mut out {
                if intervals
                    .iter()
                    .any(|&(first, last)| (first..=last).contains(&version))
                {
                    let table = &mut tables[kind];
                    table.canonical.insert(raw, canonical);
                    if let Some(path) = raw.strip_prefix("minecraft:") {
                        table.canonical.insert(path, canonical);
                    }
                    // A many-to-one rename cannot be reversed without examining
                    // payload data. Keep that explicit rather than picking a row.
                    table
                        .encoded
                        .entry(canonical)
                        .and_modify(|v| *v = None)
                        .or_insert(Some(raw));
                }
            }
        }
    }
    Catalog {
        tables: out,
        living: data.living,
        scalars: data.scalars,
    }
});

fn table(kind: Kind, version: i32) -> Result<&'static Table> {
    let tables = REGISTRIES
        .tables
        .get(&version)
        .with_context(|| format!("数据版本 {version} 缺少真实实体/方块实体注册表"))?;
    Ok(&tables[match kind {
        Kind::Entity => 0,
        Kind::BlockEntity => 1,
    }])
}

/// Check the source or target *on-disk* name, including pre-1.11 case.
pub(crate) fn contains(kind: Kind, version: i32, id: &str) -> Result<bool> {
    Ok(table(kind, version)?.canonical.contains_key(id))
}

pub(crate) fn scalar_field(id: &str, key: &str) -> Option<(&'static str, &'static [(i32, i32)])> {
    REGISTRIES
        .scalars
        .iter()
        .find(|(owner, field, ..)| *owner == id && *field == key)
        .map(|(_, _, codec, ranges)| (*codec, ranges.as_slice()))
}

pub(crate) fn known_version(version: i32) -> bool {
    REGISTRIES.tables.contains_key(&version)
}

/// This tests the verified entity JVM superclass chain, not a mob category or egg.
pub(crate) fn is_living(canonical_id: &str) -> bool {
    REGISTRIES.living.binary_search(&canonical_id).is_ok()
}

/// Resolve a verified source identity to a stable codec name, not a subtype.
/// Callers handle payload-dependent splits (horse.Type, boats, etc.) separately.
pub(crate) fn canonical(kind: Kind, version: i32, id: &str) -> Result<Option<&'static str>> {
    Ok(table(kind, version)?.canonical.get(id).copied())
}

/// Return only a uniquely represented raw target spelling.
pub(crate) fn encode(kind: Kind, version: i32, canonical_id: &str) -> Result<Option<&'static str>> {
    Ok(table(kind, version)?
        .encoded
        .get(canonical_id)
        .copied()
        .flatten())
}

pub(crate) fn translate(
    kind: Kind,
    source: i32,
    target: i32,
    id: &str,
) -> Result<Option<&'static str>> {
    let source_table = table(kind, source)?;
    let target_table = table(kind, target)?;
    Ok(source_table
        .canonical
        .get(id)
        .and_then(|name| target_table.encoded.get(name))
        .copied()
        .flatten())
}

#[cfg(test)]
mod tests {
    use super::{Kind::*, *};

    #[test]
    fn validate_both_releases_and_registry_kind() {
        assert_eq!(REGISTRIES.tables.len(), 63);
        assert!(contains(Entity, 0, "Painting").unwrap());
        assert!(!contains(Entity, 0, "painting").unwrap());
        assert!(contains(BlockEntity, 0, "Airportal").unwrap());
        assert!(!contains(Entity, 0, "Airportal").unwrap());
        assert!(!contains(Entity, 512, "minecraft:polar_bear").unwrap());
        assert!(contains(Entity, 512, "PolarBear").unwrap());
        assert!(!contains(Entity, 184, "PolarBear").unwrap());
        assert!(!contains(Entity, 1631, "bee").unwrap());
        assert!(contains(Entity, 2230, "bee").unwrap());
        assert!(contains(Entity, 5023, "minecraft:firework_rocket").unwrap());
        assert!(!contains(BlockEntity, 1343, "minecraft:brushable_block").unwrap());
        assert!(contains(BlockEntity, 5023, "minecraft:brushable_block").unwrap());
        assert!(contains(Entity, 3818, "minecraft:pig").is_err());
        assert!(is_living("minecraft:armor_stand"));
        assert!(is_living("minecraft:creaking_transient"));
        assert!(!is_living("minecraft:boat"));
        assert!(!is_living("minecraft:item_frame"));
    }

    #[test]
    fn exact_save_names_follow_dfu_renames_without_guessing() {
        assert_eq!(
            translate(Entity, 0, 5023, "XPOrb").unwrap(),
            Some("minecraft:experience_orb")
        );
        assert_eq!(
            translate(Entity, 5023, 0, "minecraft:experience_orb").unwrap(),
            Some("XPOrb")
        );
        assert_eq!(
            translate(Entity, 0, 1343, "MinecartCommandBlock").unwrap(),
            Some("minecraft:commandblock_minecart")
        );
        assert_eq!(
            translate(Entity, 1343, 5023, "minecraft:commandblock_minecart").unwrap(),
            Some("minecraft:command_block_minecart")
        );
        assert_eq!(
            translate(Entity, 0, 5023, "PigZombie").unwrap(),
            Some("minecraft:zombified_piglin")
        );
        assert_eq!(
            translate(BlockEntity, 0, 5023, "MobSpawner").unwrap(),
            Some("minecraft:mob_spawner")
        );
        assert_eq!(
            translate(BlockEntity, 5023, 0, "minecraft:dispenser").unwrap(),
            Some("Trap")
        );
        assert_eq!(translate(Entity, 0, 5023, "Fake").unwrap(), None);
        assert_eq!(translate(Entity, 5023, 0, "minecraft:bee").unwrap(), None);
    }
}
