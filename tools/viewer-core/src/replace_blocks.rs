use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::model::BlockStateNbt;
use crate::nbt::{
    bits_for_palette, decode_palette_frequencies, for_each_palette_index, load_litematic_root,
    pack_palette_indices, region_volume, save_litematic_root,
};

#[derive(Debug, Deserialize)]
pub struct ReplaceRulesFile {
    pub rules: Vec<ReplaceRule>,
}

#[derive(Debug, Deserialize)]
pub struct ReplaceRule {
    #[serde(rename = "match")]
    pub r#match: MatchSpec,
    pub replace: ReplaceSpec,
    #[serde(default)]
    pub property_mode: PropertyMode,
}

#[derive(Debug, Deserialize)]
pub struct MatchSpec {
    pub name: String,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct ReplaceSpec {
    pub name: String,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PropertyMode {
    #[default]
    Replace,
    Merge,
    Drop,
}

#[derive(Debug, Serialize)]
pub struct ReplaceBlocksSummary {
    pub input_file: String,
    pub output_file: Option<String>,
    pub dry_run: bool,
    pub regions_scanned: usize,
    pub palette_entries_scanned: usize,
    pub palette_entries_changed: usize,
    pub block_positions_affected: u64,
    pub per_rule: Vec<RuleSummary>,
    pub warnings: Vec<String>,
    pub unchanged_reasons: UnchangedReasons,
}

#[derive(Debug, Serialize)]
pub struct RuleSummary {
    pub rule_index: usize,
    pub match_name: String,
    pub replace_name: String,
    pub matched_palette_entries: usize,
    pub affected_block_count: u64,
}

#[derive(Debug, Serialize, Default)]
pub struct UnchangedReasons {
    pub no_rule_matched: usize,
    pub property_mismatch: usize,
    pub invalid_replacement: usize,
    pub tile_entity_mismatch_warning: usize,
}

struct RewriteOutcome {
    changed: bool,
    new_state: BlockStateNbt,
    rule_index: Option<usize>,
    invalid_replacement: bool,
    name_matched: bool,
    property_mismatch: bool,
    warnings: Vec<String>,
}

pub fn replace_blocks(
    input: &Path,
    output: Option<&Path>,
    rules_path: &Path,
    dry_run: bool,
) -> Result<ReplaceBlocksSummary> {
    let rules_text = fs::read_to_string(rules_path)
        .with_context(|| format!("failed to read rules {}", rules_path.display()))?;
    let rules_file: ReplaceRulesFile =
        serde_json::from_str(rules_text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("failed to parse rules {}", rules_path.display()))?;
    validate_rules(&rules_file)?;

    if !dry_run {
        let output =
            output.ok_or_else(|| anyhow!("replace-blocks requires --output without --dry-run"))?;
        if input == output {
            bail!("refusing to overwrite input file: {}", input.display());
        }
        if output.exists() {
            bail!(
                "refusing to overwrite existing output file: {}",
                output.display()
            );
        }
    }

    let mut root = load_litematic_root(input)?;
    let mut summary = ReplaceBlocksSummary {
        input_file: input.display().to_string(),
        output_file: output.map(|path| path.display().to_string()),
        dry_run,
        regions_scanned: 0,
        palette_entries_scanned: 0,
        palette_entries_changed: 0,
        block_positions_affected: 0,
        per_rule: rules_file
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| RuleSummary {
                rule_index: index,
                match_name: rule.r#match.name.clone(),
                replace_name: rule.replace.name.clone(),
                matched_palette_entries: 0,
                affected_block_count: 0,
            })
            .collect(),
        warnings: Vec::new(),
        unchanged_reasons: UnchangedReasons::default(),
    };

    for (region_name, region) in root.regions.iter_mut() {
        summary.regions_scanned += 1;
        let volume = region_volume(&region.size)
            .with_context(|| format!("failed to compute volume for region {region_name}"))?;
        let old_bits = bits_for_palette(region.block_state_palette.len());
        let frequencies = decode_palette_frequencies(
            &region.block_states,
            volume,
            old_bits,
            region.block_state_palette.len(),
        )
        .with_context(|| format!("failed to decode block states for region {region_name}"))?;

        summary.palette_entries_scanned += region.block_state_palette.len();
        let mut old_to_new = Vec::with_capacity(region.block_state_palette.len());
        let mut new_palette = Vec::<BlockStateNbt>::new();
        let mut new_index_by_key = BTreeMap::<String, usize>::new();
        let mut region_changed = false;
        let mut region_tile_entity_warning = false;

        for (old_index, old_state) in region.block_state_palette.iter().enumerate() {
            let outcome = rewrite_state(old_state, &rules_file.rules);
            for warning in outcome.warnings {
                summary.warnings.push(format!(
                    "region {region_name} palette[{old_index}]: {warning}"
                ));
            }

            if outcome.changed {
                region_changed = true;
                summary.palette_entries_changed += 1;
                let affected = frequencies.get(old_index).copied().unwrap_or(0);
                summary.block_positions_affected += affected;
                if let Some(rule_index) = outcome.rule_index {
                    if let Some(rule_summary) = summary.per_rule.get_mut(rule_index) {
                        rule_summary.matched_palette_entries += 1;
                        rule_summary.affected_block_count += affected;
                    }
                }
                if !region.tile_entities.is_empty() && block_entity_like(&old_state.name) {
                    region_tile_entity_warning = true;
                }
            } else if outcome.invalid_replacement {
                summary.unchanged_reasons.invalid_replacement += 1;
            } else if outcome.property_mismatch {
                summary.unchanged_reasons.property_mismatch += 1;
            } else if !outcome.name_matched {
                summary.unchanged_reasons.no_rule_matched += 1;
            }

            let key = state_key(&outcome.new_state);
            let new_index = if let Some(index) = new_index_by_key.get(&key) {
                *index
            } else {
                let index = new_palette.len();
                new_palette.push(outcome.new_state);
                new_index_by_key.insert(key, index);
                index
            };
            old_to_new.push(new_index);
        }

        if region_tile_entity_warning {
            summary.unchanged_reasons.tile_entity_mismatch_warning += 1;
            summary.warnings.push(format!(
                "region {region_name}: tile entities are preserved, but replacement may no longer match some block states"
            ));
        }

        if !dry_run && region_changed {
            let mut remapped = Vec::with_capacity(volume);
            for_each_palette_index(
                &region.block_states,
                volume,
                old_bits,
                region.block_state_palette.len(),
                |_, old_index| {
                    let new_index = *old_to_new
                        .get(old_index)
                        .ok_or_else(|| anyhow!("palette index {old_index} out of range"))?;
                    remapped.push(new_index);
                    Ok(())
                },
            )
            .with_context(|| format!("failed to remap block states for region {region_name}"))?;

            let new_bits = bits_for_palette(new_palette.len());
            region.block_state_palette = new_palette;
            region.block_states = pack_palette_indices(&remapped, new_bits);
        }
    }

    if !dry_run {
        let output = output.expect("checked above");
        if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create output directory {}", parent.display())
            })?;
        }
        save_litematic_root(output, &root)?;
    }

    Ok(summary)
}

fn validate_rules(rules_file: &ReplaceRulesFile) -> Result<()> {
    if rules_file.rules.is_empty() {
        bail!("rules must contain at least one rule");
    }
    for (index, rule) in rules_file.rules.iter().enumerate() {
        validate_block_name(&rule.r#match.name, true)
            .with_context(|| format!("invalid match.name in rule {index}"))?;
        validate_block_name(&rule.replace.name, false)
            .with_context(|| format!("invalid replace.name in rule {index}"))?;
        for (key, value) in &rule.r#match.properties {
            validate_property(key, value)
                .with_context(|| format!("invalid match.properties entry in rule {index}"))?;
        }
        for (key, value) in &rule.replace.properties {
            if value != "$keep" {
                validate_property(key, value)
                    .with_context(|| format!("invalid replace.properties entry in rule {index}"))?;
            } else if key.is_empty() {
                bail!("invalid replace.properties entry in rule {index}: empty property name");
            }
        }
    }
    Ok(())
}

fn validate_block_name(name: &str, allow_wildcard: bool) -> Result<()> {
    if name.is_empty() {
        bail!("block name is empty");
    }
    if !name.starts_with("minecraft:") {
        bail!("block name must start with minecraft:");
    }
    let local = name.trim_start_matches("minecraft:");
    if local.is_empty() {
        bail!("block name local part is empty");
    }
    if name.matches(':').count() != 1 {
        bail!("block name must contain exactly one namespace separator");
    }
    if name.contains('*') && !allow_wildcard {
        bail!("replacement block name cannot contain wildcard");
    }
    if name.matches('*').count() > 1 {
        bail!("only one wildcard is supported");
    }
    Ok(())
}

fn validate_property(key: &str, value: &str) -> Result<()> {
    if key.is_empty() {
        bail!("property name is empty");
    }
    if value.is_empty() {
        bail!("property value for {key} is empty");
    }
    Ok(())
}

fn rewrite_state(old_state: &BlockStateNbt, rules: &[ReplaceRule]) -> RewriteOutcome {
    let mut name_matched = false;
    let mut property_mismatch = false;

    for (rule_index, rule) in rules.iter().enumerate() {
        if old_state.name == "minecraft:air" && rule.r#match.name != "minecraft:air" {
            continue;
        }
        if !block_name_matches(&rule.r#match.name, &old_state.name) {
            continue;
        }
        name_matched = true;
        if !properties_match(&rule.r#match.properties, &old_state.properties) {
            property_mismatch = true;
            continue;
        }

        if validate_block_name(&rule.replace.name, false).is_err() {
            return RewriteOutcome {
                changed: false,
                new_state: old_state.clone(),
                rule_index: Some(rule_index),
                invalid_replacement: true,
                name_matched,
                property_mismatch,
                warnings: vec![format!(
                    "invalid replacement name {}, keeping original state",
                    rule.replace.name
                )],
            };
        }

        let (properties, warnings) = build_replacement_properties(old_state, rule);
        let new_state = BlockStateNbt {
            name: rule.replace.name.clone(),
            properties,
        };
        return RewriteOutcome {
            changed: state_key(old_state) != state_key(&new_state),
            new_state,
            rule_index: Some(rule_index),
            invalid_replacement: false,
            name_matched,
            property_mismatch,
            warnings,
        };
    }

    RewriteOutcome {
        changed: false,
        new_state: old_state.clone(),
        rule_index: None,
        invalid_replacement: false,
        name_matched,
        property_mismatch,
        warnings: Vec::new(),
    }
}

fn build_replacement_properties(
    old_state: &BlockStateNbt,
    rule: &ReplaceRule,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut properties = match rule.property_mode {
        PropertyMode::Merge => old_state.properties.clone(),
        PropertyMode::Replace | PropertyMode::Drop => BTreeMap::new(),
    };

    for (key, value) in &rule.replace.properties {
        if value == "$keep" {
            if let Some(old_value) = old_state.properties.get(key) {
                properties.insert(key.clone(), old_value.clone());
            } else {
                warnings.push(format!(
                    "requested $keep for missing property {key}, property omitted"
                ));
            }
        } else {
            properties.insert(key.clone(), value.clone());
        }
    }

    if matches!(
        rule.property_mode,
        PropertyMode::Replace | PropertyMode::Drop
    ) {
        for key in old_state.properties.keys() {
            if !properties.contains_key(key) {
                warnings.push(format!(
                    "dropped old property {key}; Minecraft state legality is not fully validated"
                ));
            }
        }
    }

    if matches!(rule.property_mode, PropertyMode::Merge) {
        for key in old_state.properties.keys() {
            if !rule.replace.properties.contains_key(key) {
                warnings.push(format!(
                    "kept old property {key} through merge; Minecraft state legality is not fully validated"
                ));
            }
        }
    }

    (properties, warnings)
}

fn block_name_matches(pattern: &str, name: &str) -> bool {
    if let Some(star) = pattern.find('*') {
        let (prefix, rest) = pattern.split_at(star);
        let suffix = &rest[1..];
        name.starts_with(prefix) && name.ends_with(suffix)
    } else {
        pattern == name
    }
}

fn properties_match(
    required: &BTreeMap<String, String>,
    actual: &BTreeMap<String, String>,
) -> bool {
    required
        .iter()
        .all(|(key, value)| actual.get(key) == Some(value))
}

fn state_key(state: &BlockStateNbt) -> String {
    let mut key = state.name.clone();
    key.push('{');
    for (property, value) in &state.properties {
        key.push_str(property);
        key.push('=');
        key.push_str(value);
        key.push(';');
    }
    key.push('}');
    key
}

fn block_entity_like(name: &str) -> bool {
    const HINTS: &[&str] = &[
        "banner",
        "barrel",
        "beacon",
        "bed",
        "bell",
        "brewing_stand",
        "chest",
        "command_block",
        "comparator",
        "conduit",
        "daylight_detector",
        "dispenser",
        "dropper",
        "furnace",
        "hopper",
        "jukebox",
        "lectern",
        "piston",
        "sculk",
        "shulker_box",
        "sign",
        "skull",
        "spawner",
        "structure_block",
    ];
    HINTS.iter().any(|hint| name.contains(hint))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use fastnbt::LongArray;

    use super::*;
    use crate::model::EnclosingSize;
    use crate::nbt::{
        LitematicRoot, MetadataNbt, RegionNbt, Vec3i, bits_for_palette, for_each_palette_index,
        load_litematic_root, pack_palette_indices, save_litematic_root,
    };

    #[test]
    fn replaces_palette_entries_dedupes_and_reloads_output() -> Result<()> {
        let temp_dir = std::env::temp_dir().join(format!(
            "lba_replace_blocks_{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&temp_dir)?;
        let input = temp_dir.join("input.litematic");
        let output = temp_dir.join("output.litematic");
        let dry_output = temp_dir.join("dry-run-should-not-exist.litematic");
        let rules = temp_dir.join("rules.json");

        save_litematic_root(&input, &fixture_root())?;
        fs::write(
            &rules,
            r#"{
  "rules": [
    {
      "match": { "name": "minecraft:stone" },
      "replace": { "name": "minecraft:smooth_stone", "properties": {} },
      "property_mode": "replace"
    },
    {
      "match": {
        "name": "minecraft:oak_slab",
        "properties": { "type": "bottom" }
      },
      "replace": {
        "name": "minecraft:spruce_slab",
        "properties": { "type": "$keep", "waterlogged": "$keep" }
      },
      "property_mode": "merge"
    },
    {
      "match": { "name": "minecraft:*_concrete" },
      "replace": { "name": "minecraft:white_concrete" },
      "property_mode": "drop"
    }
  ]
}"#,
        )?;

        let dry_summary = replace_blocks(&input, Some(&dry_output), &rules, true)?;
        assert_eq!(dry_summary.palette_entries_changed, 4);
        assert_eq!(dry_summary.block_positions_affected, 6);
        assert!(!dry_output.exists());

        let apply_summary = replace_blocks(&input, Some(&output), &rules, false)?;
        assert_eq!(apply_summary.palette_entries_changed, 4);
        assert_eq!(apply_summary.block_positions_affected, 6);

        let reloaded = load_litematic_root(&output)?;
        let region = reloaded
            .regions
            .get("ReplaceFixture")
            .ok_or_else(|| anyhow!("missing test region"))?;
        let names: Vec<_> = region
            .block_state_palette
            .iter()
            .map(|state| state.name.as_str())
            .collect();
        assert!(names.contains(&"minecraft:smooth_stone"));
        assert!(names.contains(&"minecraft:spruce_slab"));
        assert!(names.contains(&"minecraft:oak_slab"));
        assert!(names.contains(&"minecraft:white_concrete"));
        assert!(!names.contains(&"minecraft:red_concrete"));
        assert!(!names.contains(&"minecraft:blue_concrete"));
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "minecraft:white_concrete")
                .count(),
            1
        );

        let spruce = region
            .block_state_palette
            .iter()
            .find(|state| state.name == "minecraft:spruce_slab")
            .ok_or_else(|| anyhow!("missing spruce slab replacement"))?;
        assert_eq!(
            spruce.properties.get("type").map(String::as_str),
            Some("bottom")
        );
        assert_eq!(
            spruce.properties.get("waterlogged").map(String::as_str),
            Some("false")
        );

        let mut decoded = Vec::new();
        for_each_palette_index(
            &region.block_states,
            region_volume(&region.size)?,
            bits_for_palette(region.block_state_palette.len()),
            region.block_state_palette.len(),
            |_, palette_index| {
                decoded.push(region.block_state_palette[palette_index].name.clone());
                Ok(())
            },
        )?;
        assert_eq!(decoded[0], "minecraft:smooth_stone");
        assert_eq!(decoded[1], "minecraft:spruce_slab");
        assert_eq!(decoded[2], "minecraft:oak_slab");
        assert_eq!(decoded[3], "minecraft:white_concrete");
        assert_eq!(decoded[4], "minecraft:white_concrete");
        assert_eq!(decoded[5], "minecraft:white_concrete");
        assert_eq!(decoded[6], "minecraft:air");
        assert_eq!(decoded[7], "minecraft:smooth_stone");

        fs::remove_dir_all(temp_dir)?;
        Ok(())
    }

    fn fixture_root() -> LitematicRoot {
        let palette = vec![
            state("minecraft:air", &[]),
            state("minecraft:stone", &[]),
            state(
                "minecraft:oak_slab",
                &[("type", "bottom"), ("waterlogged", "false")],
            ),
            state(
                "minecraft:oak_slab",
                &[("type", "top"), ("waterlogged", "false")],
            ),
            state("minecraft:red_concrete", &[]),
            state("minecraft:blue_concrete", &[]),
        ];
        let indices = [1, 2, 3, 4, 5, 4, 0, 1];
        let block_states: LongArray =
            pack_palette_indices(&indices, bits_for_palette(palette.len()));
        let region = RegionNbt {
            position: Vec3i { x: 0, y: 0, z: 0 },
            size: Vec3i { x: 8, y: 1, z: 1 },
            block_state_palette: palette,
            block_states,
            entities: Vec::new(),
            tile_entities: Vec::new(),
        };
        LitematicRoot {
            metadata: MetadataNbt {
                author: Some("test".to_string()),
                description: Some("replace block fixture".to_string()),
                name: Some("ReplaceFixture".to_string()),
                time_created: None,
                time_modified: None,
                total_blocks: Some(7),
                total_volume: Some(8),
                region_count: Some(1),
                enclosing_size: Some(EnclosingSize { x: 8, y: 1, z: 1 }),
            },
            regions: BTreeMap::from([("ReplaceFixture".to_string(), region)]),
            version: 6,
            sub_version: Some(1),
            minecraft_data_version: Some(3700),
        }
    }

    fn state(name: &str, properties: &[(&str, &str)]) -> BlockStateNbt {
        BlockStateNbt {
            name: name.to_string(),
            properties: properties
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        }
    }
}
