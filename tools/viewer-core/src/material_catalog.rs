//! Inventory requirements, not loot drops or renderer state. The registry data is exported
//! from the selected Minecraft version; placement rules use that catalog's classes.
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

#[derive(Deserialize)]
struct Catalog {
    minecraft_version: String,
    blocks: BTreeMap<String, Block>,
    items: BTreeMap<String, Item>,
    #[serde(default)]
    legacy_state_placements: BTreeMap<String, LegacyStatePlacement>,
}
#[derive(Deserialize)]
struct LegacyNbtInventory {
    minecraft_version: String,
    #[serde(default)] legacy_bed_items: BTreeMap<String, String>,
    #[serde(default)] legacy_bed_default_item: Option<String>,
    #[serde(default)] legacy_banner_items: BTreeMap<String, String>,
    #[serde(default)] legacy_skull_items: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct LegacyStatePlacement { item_id: String, count: u64 }
#[derive(Deserialize)]
struct Block { item_id: String, classes: Vec<String>, potted_item: Option<String> }
#[derive(Clone, Deserialize)]
pub(super) struct Item {
    pub stack_size: u64,
    pub translation_key: String,
    pub can_fit_container: bool,
}
fn catalog() -> Result<&'static Catalog> {
    static DATA: OnceLock<Mutex<BTreeMap<String, &'static Catalog>>> = OnceLock::new();
    let context = crate::game_data::active_context()?;
    let cache = DATA.get_or_init(Default::default);
    let mut cache = cache.lock().map_err(|_| anyhow!("inventory adapter cache lock poisoned"))?;
    if let Some(value) = cache.get(&context.content_id) { return Ok(*value); }
    let path = context.root.join("derived/inventory.json");
    let value: Catalog = serde_json::from_slice(&std::fs::read(&path)
        .with_context(|| format!("selected Minecraft {} data requires its inventory adapter: {}", context.game_version, path.display()))?)
        .context("invalid selected inventory adapter")?;
    if value.minecraft_version != context.game_version { bail!("inventory adapter version {} does not match selected {}", value.minecraft_version, context.game_version); }
    // Public callers historically borrow immutable registry records. Intern
    // once per content identity, not per UI language/app version or material.
    let value: &'static Catalog = Box::leak(Box::new(value));
    cache.insert(context.content_id, value);
    Ok(value)
}
pub(super) fn ensure_available() -> Result<()> { catalog().map(|_| ()) }
pub(super) fn item(id: &str) -> Option<&'static Item> { catalog().ok()?.items.get(id) }
pub(super) fn name(id: &str) -> String {
    let key = item(id).map(|i| i.translation_key.clone()).unwrap_or_else(|| format!("block.{}", id.replace(':', ".")));
    crate::item_names::selected_name(&key, "zh_cn").ok().flatten().unwrap_or_else(|| id.to_owned())
}
pub(super) fn status(id: &str) -> &'static str {
    if item(id).is_some() { "item" } else if catalog().ok().is_some_and(|catalog| catalog.blocks.contains_key(id)) { "no_item" } else { "unknown" }
}

// AIR is Block.asItem's explicit "no inventory item" result, not a placeholder
// item to bill. Keep these states in a separate structural diagnostic channel.
pub(super) fn non_item_status(id: &str) -> Option<&'static str> {
    let Some(block) = catalog().ok().and_then(|catalog| catalog.blocks.get(id)) else { return Some("unknown"); };
    if block.classes.iter().any(|class| class == "AirBlock" || class == "TallSeagrassBlock" || class == "CaveVinesPlantBlock") || block.potted_item.is_some() {
        return None;
    }
    (block.item_id == "minecraft:air").then_some("no_item")
}

// Mojang DataFixers v4541 renames the historical registry entry, not a name heuristic.
fn current_registry_id(id: &str, source_data_version: Option<i32>) -> &str {
    if source_data_version.is_some_and(|version| version < 4541) && id == "minecraft:chain"
        && catalog().ok().is_some_and(|target| target.blocks.contains_key("minecraft:iron_chain") && !target.blocks.contains_key("minecraft:chain")) {
        "minecraft:iron_chain"
    } else { id }
}

// Pre-flattening palettes use their actual old names and properties. Reuse
// that source release's symbol aliases, not another version's inventory facts.
fn source_states(id: &str, properties: &BTreeMap<String, String>, source_data_version: Option<i32>) -> Result<Vec<(String, BTreeMap<String, String>)>> {
    if let Some(source) = source_data_version.filter(|version| *version < 1519) {
        let database = crate::block_database::database()?;
        let registry = database.registry_by_data_version(source)?;
        let key = if properties.is_empty() { id.to_owned() } else {
            format!("{id}[{}]", properties.iter().map(|(key,value)|format!("{key}={value}")).collect::<Vec<_>>().join(","))
        };
        if let Some(aliases) = registry.legacy_state_aliases.get(&key) {
            // The imported reference marks impossible metadata expansions with
            // this sentinel; it is not a Minecraft state or an empty material.
            let actual = aliases.iter().filter(|state| !state.starts_with("%%FILTER_ME%%")).collect::<Vec<_>>();
            if actual.is_empty() { bail!("legacy state {key} has no actual source-version state alias"); }
            return actual.into_iter().map(|state| crate::version_conversion::parse_key(state)).collect();
        }
    }
    Ok(vec![(current_registry_id(id, source_data_version).to_owned(), properties.clone())])
}

fn legacy_nbt_inventory(source: i32) -> Result<&'static LegacyNbtInventory> {
    static DATA: OnceLock<Mutex<BTreeMap<String, &'static LegacyNbtInventory>>> = OnceLock::new();
    let database = crate::block_database::database()?;
    let version = database.version_for_data_version(source).context("source version has no exact legacy inventory identity")?;
    let context = crate::game_data::active_context()?;
    let key = format!("{}:{version}", context.content_id);
    let mut cache = DATA.get_or_init(Default::default).lock().map_err(|_| anyhow!("legacy inventory cache lock poisoned"))?;
    if let Some(value) = cache.get(&key) { return Ok(*value); }
    let mut roots = vec![context.root.clone()];
    let bundled = crate::runtime_paths::app_root()?.join("pack-in/game-data/default");
    if bundled != context.root { roots.push(bundled); }
    let mut bytes = None;
    if context.game_version == version { bytes = Some(std::fs::read(context.root.join("derived/inventory.json"))?); }
    for root in roots {
        if bytes.is_some() { break; }
        let directory = root.join("derived/version-adapters").join(version);
        if directory.join("inventory.json").is_file() {
            bytes = Some(std::fs::read(directory.join("inventory.json"))?);
        } else {
            let archive = root.join("derived/version-adapters").join(format!("{version}.zip"));
            if archive.is_file() {
                let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
                let mut inventory = zip.by_name("inventory.json")?;
                if inventory.size() > 16 * 1024 * 1024 { bail!("source inventory is too large: {version}"); }
                let mut output = Vec::new(); inventory.read_to_end(&mut output)?; bytes = Some(output);
            }
        }
    }
    let value: LegacyNbtInventory = serde_json::from_slice(&bytes.with_context(|| format!("exact Minecraft {version} inventory is required for legacy block-entity materials"))?)?;
    if value.minecraft_version != version { bail!("legacy inventory version {} does not match source {version}", value.minecraft_version); }
    let value: &'static LegacyNbtInventory = Box::leak(Box::new(value));
    cache.insert(key, value);
    Ok(value)
}

pub(super) fn legacy_appearance_item(kind: &str, value: Option<i64>, source: i32) -> Result<Option<String>> {
    if source >= 1519 || (kind == "bed" && source < 1125) { return Ok(None); }
    let inventory = legacy_nbt_inventory(source)?;
    let (values, index) = match kind {
        "bed" => {
            if value.is_none() {
                let item = inventory.legacy_bed_default_item.clone().context("source inventory has no actual default bed colour")?;
                if self::item(&item).is_none() { bail!("selected game data has no item for legacy default bed {item}"); }
                return Ok(Some(item));
            }
            (&inventory.legacy_bed_items, value.unwrap_or(0))
        }
        "banner" => (&inventory.legacy_banner_items, value.unwrap_or(0)),
        "skull" => (&inventory.legacy_skull_items, value.unwrap_or(0)),
        _ => bail!("unknown legacy appearance material kind {kind}"),
    };
    // Both EnumDyeColor lookup methods map out-of-range numbers to entry 0.
    let index = if kind != "skull" && !(0..16).contains(&index) { 0 } else { index };
    let item = values.get(&index.to_string()).with_context(|| format!("source Minecraft {} has no {kind} material variant {index}", inventory.minecraft_version))?;
    if self::item(item).is_none() { bail!("selected game data has no item for legacy {kind} variant {item}"); }
    Ok(Some(item.clone()))
}

pub(super) fn legacy_appearance_kind(id: &str, properties: &BTreeMap<String, String>, source: i32) -> Result<Option<&'static str>> {
    let catalog = catalog()?;
    let mut kind = None;
    for (id, _) in source_states(id, properties, Some(source))? {
        let Some(block) = catalog.blocks.get(&id) else { continue; };
        for class in &block.classes {
            let candidate = match class.as_str() {
                "AbstractBedBlock" => Some("bed"),
                "AbstractBannerBlock" | "BannerBlock" | "WallBannerBlock" | "BlockBanner" => Some("banner"),
                "AbstractSkullBlock" | "SkullBlock" | "WallSkullBlock" | "BlockSkull" => Some("skull"),
                _ => None,
            };
            if let Some(candidate) = candidate {
                if kind.is_some_and(|prior| prior != candidate) { bail!("legacy block {id} has inconsistent appearance roles"); }
                kind = Some(candidate);
            }
        }
    }
    Ok(kind)
}

pub(super) fn non_item_status_for_version(id: &str, properties: &BTreeMap<String, String>, source_data_version: Option<i32>) -> Result<Option<&'static str>> {
    let mut statuses = source_states(id, properties, source_data_version)?.into_iter()
        .map(|(id, _)| non_item_status(&id)).collect::<Vec<_>>();
    statuses.sort(); statuses.dedup();
    if statuses.len() > 1 { bail!("legacy state {id} has ambiguous inventory diagnostic semantics"); }
    Ok(statuses.into_iter().next().flatten())
}

pub(super) fn requirements_for_version(id: &str, properties: &BTreeMap<String, String>, source_data_version: Option<i32>) -> Result<Vec<(String, u64)>> {
    let states = source_states(id, properties, source_data_version)?;
    let mut candidates = states.iter()
        .map(|(id, properties)| requirements(id, properties)).collect::<Vec<_>>();
    candidates.sort(); candidates.dedup();
    if candidates.len() > 1 {
        let catalog = catalog()?;
        // Legacy flower-pot metadata aliases can disagree about the plant:
        // TileEntityFlowerPot.Item/Data is authoritative for that component.
        // Keep only their proven common pot, not an arbitrary alias plant.
        if source_data_version.is_some_and(|source| source < 1519)
            && !states.is_empty()
            && states.iter().all(|(id, _)| catalog.blocks.get(id).is_some_and(|block|
                block.classes.iter().any(|class| matches!(class.as_str(), "FlowerPotBlock" | "BlockFlowerPot"))))
            && candidates.iter().all(|items| items.iter().any(|(item, count)| item == "minecraft:flower_pot" && *count == 1)) {
            return Ok(vec![("minecraft:flower_pot".into(), 1)]);
        }
        bail!("legacy state {id} has multiple different placement requirements; no item was guessed");
    }
    let requirements = candidates.into_iter().next().unwrap_or_default();
    if requirements.len() == 1 && requirements[0].1 == 1 {
        if let Some(source) = source_data_version.filter(|version| *version < 1519) {
            if let Some(kind) = legacy_appearance_kind(id, properties, source)? {
                if let Some(item) = legacy_appearance_item(kind, None, source)? {
                    return Ok(vec![(item, 1)]);
                }
            }
        }
    }
    Ok(requirements)
}

pub(super) fn requirements(id: &str, properties: &BTreeMap<String, String>) -> Vec<(String, u64)> {
    let Ok(catalog) = catalog() else { return Vec::new(); };
    let Some(block) = catalog.blocks.get(id) else { return Vec::new(); };
    let state_id = if properties.is_empty() { id.to_owned() } else {
        format!("{id}[{}]", properties.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(","))
    };
    let placement = catalog.legacy_state_placements.get(&state_id);
    let item_id = placement.map(|value| value.item_id.as_str()).unwrap_or(&block.item_id);
    let class = |name: &str| block.classes.iter().any(|c| c == name);
    let prop = |name: &str| properties.get(name).map(String::as_str).unwrap_or("");
    if class("AirBlock") { return Vec::new(); }
    // DoorBlock.setPlacedBy / DoublePlantBlock.setPlacedBy / AbstractBedBlock.setPlacedBy.
    if ((class("DoorBlock") || class("DoublePlantBlock")) && prop("half") == "upper")
        || (class("AbstractBedBlock") && prop("part") == "head") { return Vec::new(); }
    // TallSeagrassBlock.getCloneItemStack points at SEAGRASS. SeagrassBlock.performBonemeal
    // grows one placed seagrass into lower+upper states; only the lower half consumes inputs.
    if class("TallSeagrassBlock") {
        return vec![("minecraft:seagrass".into(), 1), ("minecraft:bone_meal".into(), 1)];
    }
    // FlowerPotBlock.useItemOn consumes one plant in addition to the existing pot.
    if let Some(plant) = &block.potted_item {
        if plant != "minecraft:air" { return vec![("minecraft:flower_pot".into(), 1), (plant.clone(), 1)]; }
    }
    // CaveVinesPlantBlock.getCloneItemStack returns GLOW_BERRIES like the head.
    if class("CaveVinesPlantBlock") { return vec![("minecraft:glow_berries".into(), 1)]; }
    // LiquidBlock.pickupBlock returns the actual fluid bucket only at level=0.
    // Flowing cells remain in structural counts; they are not extra buckets.
    if class("LiquidBlock") && (prop("level").is_empty() || prop("level") == "0") {
        let bucket = match id { "minecraft:water" => "minecraft:water_bucket", "minecraft:lava" => "minecraft:lava_bucket", _ => return Vec::new() };
        return vec![(bucket.into(), 1)];
    }
    // Other technical states have no directly placeable inventory item.
    // Structure counts and non_item_blocks diagnostics preserve them; never guess items.
    if item_id == "minecraft:air" { return Vec::new(); }
    let mut count = placement.map(|value| value.count).unwrap_or_else(|| if class("SlabBlock") && prop("type") == "double" { 2 } else { 1 });
    for (kind, property, maximum) in [
        ("SnowLayerBlock", "layers", 8), ("CandleBlock", "candles", 4),
        ("SeaPickleBlock", "pickles", 4), ("TurtleEggBlock", "eggs", 4),
        ("FlowerBedBlock", "flower_amount", 4), ("LeafLitterBlock", "segment_amount", 4),
    ] {
        if class(kind) { count = prop(property).parse::<u64>().unwrap_or(1).clamp(1, maximum); }
    }
    vec![(item_id.to_owned(), count)]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn material(id: &str, props: &[(&str, &str)]) -> Vec<(String, u64)> {
        requirements(&format!("minecraft:{id}"), &props.iter().map(|(k,v)| (k.to_string(),v.to_string())).collect())
    }
    #[test]
    fn registry_maps_variants_and_seeds_not_name_heuristics() {
        for (block, result) in [("redstone_wire", "redstone"), ("oak_wall_sign", "oak_sign"), ("wall_torch", "torch"), ("redstone_wall_torch", "redstone_torch"), ("wheat", "wheat_seeds"), ("carrots", "carrot"), ("tripwire", "string")] {
            assert_eq!(material(block, &[]), vec![(format!("minecraft:{result}"), 1)]);
        }
    }
    #[test]
    fn placement_quantities_and_non_items_are_explicit() {
        assert_eq!(material("stone_slab", &[("type", "double")]), vec![("minecraft:stone_slab".into(), 2)]);
        assert!(material("oak_door", &[("half", "upper")]).is_empty());
        assert!(material("red_bed", &[("part", "head")]).is_empty());
        assert!(material("air", &[]).is_empty());
        assert_eq!(material("potted_dandelion", &[]), vec![("minecraft:flower_pot".into(), 1), ("minecraft:dandelion".into(), 1)]);
        assert!(material("water", &[("level", "7")]).is_empty());
        assert_eq!(status("minecraft:water"), "no_item");
        assert_eq!(status("minecraft:piston_head"), "no_item");
        assert_eq!(status("example:custom"), "unknown");
    }
    #[test]
    fn source_bucket_and_cave_vines_clone_mappings_follow_actual_game_classes() {
        assert_eq!(material("water", &[("level","0")]),vec![("minecraft:water_bucket".into(),1)]);
        assert_eq!(material("lava", &[("level","0")]),vec![("minecraft:lava_bucket".into(),1)]);
        assert!(material("water", &[("level","3")]).is_empty());
        for block in ["cave_vines","cave_vines_plant"] { assert_eq!(material(block, &[("berries","false")]),vec![("minecraft:glow_berries".into(),1)]); }
        assert_eq!(non_item_status("minecraft:cave_vines_plant"),None);
        assert_eq!(non_item_status("minecraft:piston_head"),Some("no_item"));
    }
    #[test]
    fn every_air_mapping_is_diagnostic_not_an_inventory_requirement() {
        for (id, block) in &catalog().expect("selected inventory adapter").blocks {
            if block.item_id == "minecraft:air" && block.potted_item.is_none()
                && !block.classes.iter().any(|class| class == "AirBlock" || class == "TallSeagrassBlock" || class == "CaveVinesPlantBlock" || class == "LiquidBlock") {
                assert!(requirements(id, &BTreeMap::new()).is_empty(), "{id}");
                assert_eq!(non_item_status(id), Some("no_item"), "{id}");
            }
        }
        for block in ["piston_head", "moving_piston", "bubble_column", "fire", "end_portal", "attached_melon_stem", "candle_cake"] {
            assert!(material(block, &[]).is_empty(), "{block}");
        }
        assert!(requirements("example:unregistered", &BTreeMap::new()).is_empty());
        assert_eq!(non_item_status("example:unregistered"), Some("unknown"));
        assert_eq!(non_item_status("minecraft:potted_dandelion"), None);
    }
    #[test]
    fn historical_chain_rename_and_tall_seagrass_placement_are_source_backed() {
        assert_eq!(requirements_for_version("minecraft:chain", &BTreeMap::new(), Some(3465)).unwrap(), vec![("minecraft:iron_chain".into(), 1)]);
        assert_eq!(non_item_status_for_version("minecraft:chain", &BTreeMap::new(), Some(3465)).unwrap(), None);
        assert_eq!(non_item_status_for_version("minecraft:chain", &BTreeMap::new(), Some(5023)).unwrap(), Some("unknown"));
        assert_eq!(material("tall_seagrass", &[("half", "lower")]), vec![("minecraft:seagrass".into(), 1), ("minecraft:bone_meal".into(), 1)]);
        assert!(material("tall_seagrass", &[("half", "upper")]).is_empty());
        assert_eq!(non_item_status("minecraft:tall_seagrass"), None);
    }
    #[test]
    fn every_registered_multi_block_item_is_billed_once() {
        for (id, block) in &catalog().expect("selected inventory adapter").blocks {
            if block.item_id == "minecraft:air" { continue; }
            let (key, primary, secondary) = if block.classes.iter().any(|c| c == "AbstractBedBlock") {
                ("part", "foot", "head")
            } else if block.classes.iter().any(|c| c == "DoorBlock" || c == "DoublePlantBlock") {
                ("half", "lower", "upper")
            } else { continue; };
            assert_eq!(requirements(id, &BTreeMap::from([(key.into(), primary.into())])), vec![(block.item_id.clone(), 1)], "{id}");
            assert!(requirements(id, &BTreeMap::from([(key.into(), secondary.into())])).is_empty(), "{id}");
        }
        for block in ["piston", "sticky_piston"] {
            assert_eq!(material(block, &[("extended", "true")]), vec![(format!("minecraft:{block}"), 1)]);
        }
    }
    #[test]
    fn stack_sizes_and_names_come_from_bound_26_3_components() {
        for (id, size) in [("stone",64), ("oak_sign",16), ("bucket",16), ("water_bucket",1), ("ender_pearl",16), ("shulker_box",1), ("dragon_egg",64), ("conduit",64)] {
            assert_eq!(item(&format!("minecraft:{id}")).unwrap().stack_size, size);
        }
        assert!(!item("minecraft:shulker_box").unwrap().can_fit_container);
        assert_eq!(name("minecraft:redstone"), "红石粉");
    }
}
