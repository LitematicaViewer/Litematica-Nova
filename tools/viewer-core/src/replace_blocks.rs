use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::model::BlockStateNbt;
use crate::nbt::{
    bits_for_palette, decode_palette_frequencies, for_each_palette_index, load_litematic_root,
    pack_palette_indices, region_volume, save_litematic_root,
};

// ============================================================================
// V1 (legacy) data structures
// ============================================================================

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

// ============================================================================
// V2 data structures (schema_version: 2)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ReplaceUnitsFile {
    pub schema_version: u32,
    pub units: Vec<ReplaceUnit>,
}

/// A single replacement unit: match any block in `input`, replace with a
/// weighted-random pick from `output`.
#[derive(Debug, Deserialize)]
pub struct ReplaceUnit {
    pub input: Vec<InputEntry>,
    pub output: Vec<OutputEntry>,
}

/// One entry in the input list of a replace unit.
#[derive(Debug, Deserialize)]
pub struct InputEntry {
    pub name: String,
    /// Subset match: only keys listed here are checked. Empty = match all states.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

/// One entry in the output list of a replace unit.
#[derive(Debug, Clone, Deserialize)]
pub struct OutputEntry {
    pub name: String,
    /// Output properties to set. Keys not listed use Minecraft defaults.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_weight() -> u32 {
    1
}

// ============================================================================
// Summary / output types
// ============================================================================

/// Per-output-entry distribution in a unit summary.
#[derive(Debug, Serialize)]
pub struct OutputDistEntry {
    pub output_index: usize,
    pub name: String,
    /// Estimated count (weight-proportional from hit_count; always present).
    pub estimated_count: u64,
    /// Actual count as placed; None in dry-run.
    pub actual_count: Option<u64>,
}

/// Per-unit statistics returned in the summary.
#[derive(Debug, Serialize)]
pub struct UnitSummary {
    pub unit_index: usize,
    pub hit_count: u64,
    pub output_distribution: Vec<OutputDistEntry>,
    pub warnings: Vec<String>,
    pub invalid_entries: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ReplaceBlocksSummary {
    /// 1 for v1 rules format, 2 for v2 units format.
    pub schema_version: u32,
    pub input_file: String,
    pub output_file: Option<String>,
    pub dry_run: bool,
    pub regions_scanned: usize,
    pub palette_entries_scanned: usize,
    pub palette_entries_changed: usize,
    pub block_positions_affected: u64,
    /// Populated for v1 rules files (backward compat).
    pub per_rule: Vec<RuleSummary>,
    /// Populated for v2 units files.
    pub per_unit: Vec<UnitSummary>,
    pub warnings: Vec<String>,
    pub unchanged_reasons: UnchangedReasons,
    /// Diagnostic log lines (always populated, useful for debugging).
    pub debug_log: Vec<String>,
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

// Internal outcome used by v1 rewrite path.
struct RewriteOutcome {
    changed: bool,
    new_state: BlockStateNbt,
    rule_index: Option<usize>,
    invalid_replacement: bool,
    name_matched: bool,
    property_mismatch: bool,
    warnings: Vec<String>,
}

// ============================================================================
// Public entry point
// ============================================================================

pub fn replace_blocks(
    input: &Path,
    output: Option<&Path>,
    rules_path: &Path,
    dry_run: bool,
) -> Result<ReplaceBlocksSummary> {
    let rules_text = fs::read_to_string(rules_path)
        .with_context(|| format!("failed to read rules {}", rules_path.display()))?;
    let text = rules_text.trim_start_matches('\u{feff}');

    // Detect schema version by peeking at the raw JSON value.
    let raw: serde_json::Value = serde_json::from_str(text)
        .with_context(|| format!("failed to parse rules {}", rules_path.display()))?;

    let schema_version = raw
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u32;

    if schema_version >= 2 {
        let units_file: ReplaceUnitsFile = serde_json::from_str(text)
            .with_context(|| format!("failed to parse v2 rules {}", rules_path.display()))?;
        validate_units_file(&units_file)?;
        replace_blocks_v2(input, output, dry_run, &units_file)
    } else {
        let rules_file: ReplaceRulesFile = serde_json::from_str(text)
            .with_context(|| format!("failed to parse v1 rules {}", rules_path.display()))?;
        validate_rules(&rules_file)?;
        replace_blocks_v1(input, output, dry_run, &rules_file)
    }
}

// ============================================================================
// V1 implementation (palette fast path, backward compat)
// ============================================================================

fn replace_blocks_v1(
    input: &Path,
    output: Option<&Path>,
    dry_run: bool,
    rules_file: &ReplaceRulesFile,
) -> Result<ReplaceBlocksSummary> {
    if !dry_run {
        let output =
            output.ok_or_else(|| anyhow!("replace-blocks requires --output without --dry-run"))?;
        if input == output {
            bail!("refusing to overwrite input file: {}", input.display());
        }
        if output.exists() {
            bail!("refusing to overwrite existing output file: {}", output.display());
        }
    }

    let arc_root = load_litematic_root(input)?;
    let mut root = (*arc_root).clone();
    let mut summary = ReplaceBlocksSummary {
        schema_version: 1,
        input_file: input.display().to_string(),
        output_file: output.map(|p| p.display().to_string()),
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
        per_unit: Vec::new(),
        warnings: Vec::new(),
        unchanged_reasons: UnchangedReasons::default(),
        debug_log: Vec::new(),
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
                    if let Some(rs) = summary.per_rule.get_mut(rule_index) {
                        rs.matched_palette_entries += 1;
                        rs.affected_block_count += affected;
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
                "region {region_name}: tile entities preserved, but replacement may no longer match some block states"
            ));
        }

        if !dry_run && region_changed {
            let mut remapped = Vec::with_capacity(volume);
            for_each_palette_index(
                &region.block_states,
                volume,
                old_bits,
                region.block_state_palette.len(),
                |_, old_idx| {
                    remapped.push(*old_to_new.get(old_idx)
                        .ok_or_else(|| anyhow!("palette index {old_idx} out of range"))?);
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
        let out = output.expect("checked above");
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create output directory {}", parent.display()))?;
        }
        save_litematic_root(out, &root)?;
    }

    Ok(summary)
}

// ============================================================================
// V2 implementation (per-unit, with per-position random for multi-output)
// ============================================================================

fn replace_blocks_v2(
    input: &Path,
    output: Option<&Path>,
    dry_run: bool,
    units_file: &ReplaceUnitsFile,
) -> Result<ReplaceBlocksSummary> {
    if !dry_run {
        let output =
            output.ok_or_else(|| anyhow!("replace-blocks requires --output without --dry-run"))?;
        if input == output {
            bail!("refusing to overwrite input file: {}", input.display());
        }
        if output.exists() {
            bail!("refusing to overwrite existing output file: {}", output.display());
        }
    }

    let arc_root = load_litematic_root(input)?;
    let mut root = (*arc_root).clone();

    // Initialize per-unit summaries with output distribution slots.
    let mut unit_summaries: Vec<UnitSummary> = units_file
        .units
        .iter()
        .enumerate()
        .map(|(i, unit)| {
            let total_weight: u64 = unit.output.iter().map(|e| e.weight as u64).sum();
            UnitSummary {
                unit_index: i,
                hit_count: 0,
                output_distribution: unit
                    .output
                    .iter()
                    .enumerate()
                    .map(|(j, entry)| {
                        let _estimated_frac = if total_weight > 0 {
                            entry.weight as f64 / total_weight as f64
                        } else {
                            0.0
                        };
                        OutputDistEntry {
                            output_index: j,
                            name: entry.name.clone(),
                            estimated_count: 0, // filled per-region below
                            actual_count: if dry_run { None } else { Some(0) },
                        }
                    })
                    .collect(),
                warnings: Vec::new(),
                invalid_entries: Vec::new(),
            }
        })
        .collect();

    let mut rng = rand::thread_rng();
    let mut total_regions = 0;
    let mut total_palette_scanned = 0;
    let mut total_palette_changed = 0;
    let mut total_blocks_affected = 0u64;
    let mut global_warnings = Vec::new();

    // Check if ANY unit has multiple outputs → need per-position path
    let has_multi_output = units_file.units.iter().any(|u| u.output.len() > 1);

    for (region_name, region) in root.regions.iter_mut() {
        total_regions += 1;
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

        total_palette_scanned += region.block_state_palette.len();

        if has_multi_output {
            // Per-position path: iterate every block position, random pick for multi-output units.
            process_region_per_position(
                region,
                region_name,
                &units_file.units,
                &frequencies,
                &mut unit_summaries,
                &mut global_warnings,
                &mut total_palette_changed,
                &mut total_blocks_affected,
                &mut rng,
                dry_run,
            )
            .with_context(|| format!("failed to process region {region_name}"))?;
        } else {
            // Palette fast path: all units have single output, can rewrite palette entries.
            process_region_palette_fast(
                region,
                region_name,
                &units_file.units,
                &frequencies,
                &mut unit_summaries,
                &mut global_warnings,
                &mut total_palette_changed,
                &mut total_blocks_affected,
                dry_run,
            )
            .with_context(|| format!("failed to process region {region_name}"))?;
        }
    }

    if !dry_run {
        let out = output.expect("checked above");
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create output directory {}", parent.display()))?;
        }
        save_litematic_root(out, &root)?;
    }

    Ok(ReplaceBlocksSummary {
        schema_version: 2,
        input_file: input.display().to_string(),
        output_file: output.map(|p| p.display().to_string()),
        dry_run,
        regions_scanned: total_regions,
        palette_entries_scanned: total_palette_scanned,
        palette_entries_changed: total_palette_changed,
        block_positions_affected: total_blocks_affected,
        per_rule: Vec::new(),
        per_unit: unit_summaries,
        warnings: global_warnings,
        unchanged_reasons: UnchangedReasons::default(),
        debug_log: Vec::new(),
    })
}

// ============================================================================
// V2 region processors
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn process_region_palette_fast(
    region: &mut crate::nbt::RegionNbt,
    region_name: &str,
    units: &[ReplaceUnit],
    frequencies: &[u64],
    unit_summaries: &mut [UnitSummary],
    global_warnings: &mut Vec<String>,
    total_palette_changed: &mut usize,
    total_blocks_affected: &mut u64,
    dry_run: bool,
) -> Result<()> {
    let old_bits = bits_for_palette(region.block_state_palette.len());
    let volume = region.block_state_palette.len(); // reuse for capacity hint
    let mut old_to_new: Vec<usize> = Vec::with_capacity(volume);
    // Pre-seed palette with minecraft:air at index 0 (Litematica convention).
    let air_state = BlockStateNbt {
        name: "minecraft:air".to_string(),
        properties: BTreeMap::new(),
    };
    let air_key = state_key(&air_state);
    let mut new_palette: Vec<BlockStateNbt> = vec![air_state];
    let mut new_index_by_key: BTreeMap<String, usize> = BTreeMap::from([(air_key, 0)]);
    let mut region_changed = false;

    for (old_idx, old_state) in region.block_state_palette.iter().enumerate() {
        let match_result = find_matching_unit(old_state, units);
        let new_state = if let Some((unit_idx, _input_idx)) = match_result {
            let unit = &units[unit_idx];
            // Single-output guaranteed in this path.
            let out = &unit.output[0];
            if validate_block_name(&out.name, false).is_err() {
                unit_summaries[unit_idx].invalid_entries.push(format!(
                    "invalid output name '{}' in unit {unit_idx}",
                    out.name
                ));
                old_state.clone()
            } else {
                let freq = frequencies.get(old_idx).copied().unwrap_or(0);
                unit_summaries[unit_idx].hit_count += freq;
                if let Some(dist) = unit_summaries[unit_idx].output_distribution.get_mut(0) {
                    dist.estimated_count += freq;
                    if let Some(ref mut ac) = dist.actual_count {
                        *ac += freq;
                    }
                }
                if !region.tile_entities.is_empty() && block_entity_like(&old_state.name) {
                    global_warnings.push(format!(
                        "region {region_name}: tile entities preserved; replacement may no longer match some block states"
                    ));
                }
                BlockStateNbt {
                    name: out.name.clone(),
                    properties: out.properties.clone(),
                }
            }
        } else {
            old_state.clone()
        };

        let key = state_key(&new_state);
        let new_index = if let Some(&idx) = new_index_by_key.get(&key) {
            idx
        } else {
            let idx = new_palette.len();
            new_palette.push(new_state);
            new_index_by_key.insert(key, idx);
            idx
        };

        if new_index != old_idx || new_palette.len() > old_idx + 1 {
            // detect change by comparing state keys
        }
        old_to_new.push(new_index);
    }

    // Count palette-level changes.
    for (old_idx, &new_idx) in old_to_new.iter().enumerate() {
        if state_key(&region.block_state_palette[old_idx])
            != state_key(&new_palette[new_idx])
        {
            region_changed = true;
            *total_palette_changed += 1;
            *total_blocks_affected += frequencies.get(old_idx).copied().unwrap_or(0);
        }
    }

    if !dry_run && region_changed {
        let vol = region_volume(&region.size)?;
        let mut remapped = Vec::with_capacity(vol);
        for_each_palette_index(
            &region.block_states,
            vol,
            old_bits,
            region.block_state_palette.len(),
            |_, old_idx| {
                remapped.push(
                    *old_to_new
                        .get(old_idx)
                        .ok_or_else(|| anyhow!("palette index {old_idx} out of range"))?,
                );
                Ok(())
            },
        )?;
        let new_bits = bits_for_palette(new_palette.len());
        region.block_state_palette = new_palette;
        region.block_states = pack_palette_indices(&remapped, new_bits);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_region_per_position(
    region: &mut crate::nbt::RegionNbt,
    region_name: &str,
    units: &[ReplaceUnit],
    frequencies: &[u64],
    unit_summaries: &mut [UnitSummary],
    global_warnings: &mut Vec<String>,
    total_palette_changed: &mut usize,
    total_blocks_affected: &mut u64,
    rng: &mut impl Rng,
    dry_run: bool,
) -> Result<()> {
    let old_bits = bits_for_palette(region.block_state_palette.len());
    let volume = region_volume(&region.size)?;

    // Pre-compute per-palette-entry: which unit matches, and weight totals.
    // None means no unit matches this entry (pass through unchanged).
    let palette_match: Vec<Option<usize>> = region
        .block_state_palette
        .iter()
        .map(|state| find_matching_unit(state, units).map(|(ui, _)| ui))
        .collect();

    // Update estimated counts based on palette frequencies.
    for (old_idx, &maybe_unit) in palette_match.iter().enumerate() {
        if let Some(unit_idx) = maybe_unit {
            let freq = frequencies.get(old_idx).copied().unwrap_or(0);
            unit_summaries[unit_idx].hit_count += freq;
            let unit = &units[unit_idx];
            let total_w: u64 = unit.output.iter().map(|e| e.weight as u64).sum();
            for (j, out) in unit.output.iter().enumerate() {
                let est = if total_w > 0 {
                    (freq as f64 * out.weight as f64 / total_w as f64).round() as u64
                } else {
                    0
                };
                if let Some(dist) = unit_summaries[unit_idx].output_distribution.get_mut(j) {
                    dist.estimated_count += est;
                }
            }
        }
    }

    if dry_run {
        // Dry-run: only count statistics, don't modify.
        *total_blocks_affected += palette_match
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.map(|_| frequencies.get(i).copied().unwrap_or(0)))
            .sum::<u64>();
        return Ok(());
    }

    // Actual run: iterate every position and randomly pick output.
    // Pre-seed palette with minecraft:air at index 0 — Litematica's convention
    // requires air at palette[0] so that padding zeros in the packed long array
    // are interpreted as air rather than as an arbitrary replacement block.
    let air_state = BlockStateNbt {
        name: "minecraft:air".to_string(),
        properties: BTreeMap::new(),
    };
    let air_key = state_key(&air_state);
    let mut new_palette: Vec<BlockStateNbt> = vec![air_state];
    let mut new_index_by_key: BTreeMap<String, usize> = BTreeMap::from([(air_key, 0)]);
    let mut remapped: Vec<usize> = Vec::with_capacity(volume);
    let mut positions_changed = 0u64;

    for_each_palette_index(
        &region.block_states,
        volume,
        old_bits,
        region.block_state_palette.len(),
        |_, old_idx| {
            let old_state = &region.block_state_palette[old_idx];
            let new_state = if let Some(unit_idx) = palette_match[old_idx] {
                let unit = &units[unit_idx];
                let out = weighted_random_pick(&unit.output, rng);
                if let Some(dist) = unit_summaries[unit_idx]
                    .output_distribution
                    .iter_mut()
                    .find(|d| d.name == out.name)
                {
                    if let Some(ref mut ac) = dist.actual_count {
                        *ac += 1;
                    }
                }
                if state_key(old_state) != state_key_from_output(out) {
                    positions_changed += 1;
                }
                BlockStateNbt {
                    name: out.name.clone(),
                    properties: out.properties.clone(),
                }
            } else {
                old_state.clone()
            };

            let key = state_key(&new_state);
            let new_index = if let Some(&idx) = new_index_by_key.get(&key) {
                idx
            } else {
                let idx = new_palette.len();
                new_index_by_key.insert(key, idx);
                new_palette.push(new_state);
                idx
            };
            remapped.push(new_index);
            Ok(())
        },
    )
    .with_context(|| format!("failed to iterate block states for region {region_name}"))?;

    if !region.tile_entities.is_empty() {
        let any_entity_like = palette_match.iter().enumerate().any(|(i, m)| {
            m.is_some() && block_entity_like(&region.block_state_palette[i].name)
        });
        if any_entity_like {
            global_warnings.push(format!(
                "region {region_name}: tile entities preserved; replacement may no longer match some block states"
            ));
        }
    }

    *total_blocks_affected += positions_changed;
    // palette_changed: count palette entries that changed
    let palette_changed = palette_match.iter().filter(|m| m.is_some()).count();
    *total_palette_changed += palette_changed;

    let new_bits = bits_for_palette(new_palette.len());
    region.block_state_palette = new_palette;
    region.block_states = pack_palette_indices(&remapped, new_bits);
    Ok(())
}

// ============================================================================
// Helper functions
// ============================================================================

/// Find the first unit whose input matches `state`. Returns (unit_index, input_entry_index).
fn find_matching_unit(state: &BlockStateNbt, units: &[ReplaceUnit]) -> Option<(usize, usize)> {
    for (ui, unit) in units.iter().enumerate() {
        for (ii, input) in unit.input.iter().enumerate() {
            if block_name_matches(&input.name, &state.name)
                && properties_subset_match(&input.properties, &state.properties)
            {
                return Some((ui, ii));
            }
        }
    }
    None
}

/// Weighted random pick from output entries. Panics if outputs is empty or all weights are zero.
fn weighted_random_pick<'a>(outputs: &'a [OutputEntry], rng: &mut impl Rng) -> &'a OutputEntry {
    let total: u32 = outputs.iter().map(|e| e.weight).sum();
    if total == 0 {
        return &outputs[0]; // fallback
    }
    let mut roll = rng.gen_range(0..total);
    for out in outputs {
        if roll < out.weight {
            return out;
        }
        roll -= out.weight;
    }
    &outputs[outputs.len() - 1] // shouldn't reach here
}

fn state_key_from_output(out: &OutputEntry) -> String {
    let mut key = out.name.clone();
    key.push('{');
    for (k, v) in &out.properties {
        key.push_str(k);
        key.push('=');
        key.push_str(v);
        key.push(';');
    }
    key.push('}');
    key
}

fn state_key(state: &BlockStateNbt) -> String {
    let mut key = state.name.clone();
    key.push('{');
    for (k, v) in &state.properties {
        key.push_str(k);
        key.push('=');
        key.push_str(v);
        key.push(';');
    }
    key.push('}');
    key
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

/// Subset match: all keys in `required` must be present in `actual` with matching values.
fn properties_subset_match(
    required: &BTreeMap<String, String>,
    actual: &BTreeMap<String, String>,
) -> bool {
    required
        .iter()
        .all(|(k, v)| actual.get(k) == Some(v))
}

fn block_entity_like(name: &str) -> bool {
    const HINTS: &[&str] = &[
        "banner", "barrel", "beacon", "bed", "bell", "brewing_stand", "chest",
        "command_block", "comparator", "conduit", "daylight_detector", "dispenser",
        "dropper", "furnace", "hopper", "jukebox", "lectern", "piston", "sculk",
        "shulker_box", "sign", "skull", "spawner", "structure_block",
    ];
    HINTS.iter().any(|hint| name.contains(hint))
}

// ============================================================================
// Validation
// ============================================================================

fn validate_units_file(file: &ReplaceUnitsFile) -> Result<()> {
    if file.units.is_empty() {
        bail!("units must contain at least one replace unit");
    }
    for (ui, unit) in file.units.iter().enumerate() {
        if unit.input.is_empty() {
            bail!("unit {ui}: input must contain at least one entry");
        }
        if unit.output.is_empty() {
            bail!("unit {ui}: output must contain at least one entry");
        }
        let total_weight: u32 = unit.output.iter().map(|e| e.weight).sum();
        if total_weight == 0 {
            bail!("unit {ui}: all output weights are zero");
        }
        for (ii, input) in unit.input.iter().enumerate() {
            validate_block_name(&input.name, true)
                .with_context(|| format!("unit {ui} input[{ii}]: invalid name"))?;
        }
        for (oi, out) in unit.output.iter().enumerate() {
            validate_block_name(&out.name, false)
                .with_context(|| format!("unit {ui} output[{oi}]: invalid name"))?;
        }
    }
    Ok(())
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
        bail!("block name must start with 'minecraft:' (got '{name}')");
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
        bail!("property value for '{key}' is empty");
    }
    Ok(())
}

// ============================================================================
// V1 helper functions (palette rewrite)
// ============================================================================

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
        if !properties_subset_match(&rule.r#match.properties, &old_state.properties) {
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
                    "invalid replacement name '{}', keeping original state",
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
                    "requested $keep for missing property '{key}', property omitted"
                ));
            }
        } else {
            properties.insert(key.clone(), value.clone());
        }
    }
    if matches!(rule.property_mode, PropertyMode::Replace | PropertyMode::Drop) {
        for key in old_state.properties.keys() {
            if !properties.contains_key(key) {
                warnings.push(format!(
                    "dropped old property '{key}'; Minecraft state legality is not fully validated"
                ));
            }
        }
    }
    if matches!(rule.property_mode, PropertyMode::Merge) {
        for key in old_state.properties.keys() {
            if !rule.replace.properties.contains_key(key) {
                warnings.push(format!(
                    "kept old property '{key}' through merge; Minecraft state legality is not fully validated"
                ));
            }
        }
    }
    (properties, warnings)
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

    // ── fixtures ──────────────────────────────────────────────────────────────

    fn state(name: &str, properties: &[(&str, &str)]) -> BlockStateNbt {
        BlockStateNbt {
            name: name.to_string(),
            properties: properties
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        }
    }

    fn fixture_root() -> LitematicRoot {
        let palette = vec![
            state("minecraft:air", &[]),
            state("minecraft:stone", &[]),
            state("minecraft:oak_slab", &[("type", "bottom"), ("waterlogged", "false")]),
            state("minecraft:oak_slab", &[("type", "top"), ("waterlogged", "false")]),
            state("minecraft:red_concrete", &[]),
            state("minecraft:blue_concrete", &[]),
        ];
        let indices = [1, 2, 3, 4, 5, 4, 0, 1];
        let block_states: LongArray = pack_palette_indices(&indices, bits_for_palette(palette.len()));
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
                preview_image_data: None,
            },
            regions: BTreeMap::from([("ReplaceFixture".to_string(), region)]),
            version: 6,
            sub_version: Some(1),
            minecraft_data_version: Some(3700),
        }
    }

    fn tmpdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lba_replace_test_{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ── v1 backward-compat test ───────────────────────────────────────────────

    #[test]
    fn v1_replaces_palette_entries_dedupes_and_reloads_output() -> Result<()> {
        let dir = tmpdir();
        let input = dir.join("input.litematic");
        let output = dir.join("output.litematic");
        let dry_out = dir.join("dry-run-should-not-exist.litematic");
        let rules = dir.join("rules.json");

        save_litematic_root(&input, &fixture_root())?;
        fs::write(&rules, r#"{
  "rules": [
    {"match":{"name":"minecraft:stone"},"replace":{"name":"minecraft:smooth_stone","properties":{}},"property_mode":"replace"},
    {"match":{"name":"minecraft:oak_slab","properties":{"type":"bottom"}},"replace":{"name":"minecraft:spruce_slab","properties":{"type":"$keep","waterlogged":"$keep"}},"property_mode":"merge"},
    {"match":{"name":"minecraft:*_concrete"},"replace":{"name":"minecraft:white_concrete"},"property_mode":"drop"}
  ]
}"#)?;

        let dry = replace_blocks(&input, Some(&dry_out), &rules, true)?;
        assert_eq!(dry.schema_version, 1);
        assert_eq!(dry.palette_entries_changed, 4);
        assert!(!dry_out.exists());

        let applied = replace_blocks(&input, Some(&output), &rules, false)?;
        assert_eq!(applied.palette_entries_changed, 4);

        let reloaded = load_litematic_root(&output)?;
        let region = reloaded.regions.get("ReplaceFixture").unwrap();
        let names: Vec<_> = region.block_state_palette.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"minecraft:smooth_stone"));
        assert!(names.contains(&"minecraft:white_concrete"));
        assert!(!names.contains(&"minecraft:red_concrete"));
        assert!(!names.contains(&"minecraft:blue_concrete"));

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    // ── v2 single-output (palette fast path) ─────────────────────────────────

    #[test]
    fn v2_single_output_replaces_all_stone() -> Result<()> {
        let dir = tmpdir();
        let input = dir.join("input.litematic");
        let output = dir.join("output.litematic");
        let rules = dir.join("rules.json");

        save_litematic_root(&input, &fixture_root())?;
        fs::write(&rules, r#"{
  "schema_version": 2,
  "units": [
    {
      "input": [{"name": "minecraft:stone"}],
      "output": [{"name": "minecraft:granite", "weight": 1}]
    }
  ]
}"#)?;

        let dry = replace_blocks(&input, Some(&output), &rules, true)?;
        assert_eq!(dry.schema_version, 2);
        assert_eq!(dry.per_unit[0].hit_count, 2); // indices [1,7] in fixture = 2 stone blocks
        assert!(!output.exists());

        replace_blocks(&input, Some(&output), &rules, false)?;
        let reloaded = load_litematic_root(&output)?;
        let region = reloaded.regions.get("ReplaceFixture").unwrap();
        let names: Vec<_> = region.block_state_palette.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"minecraft:granite"));
        assert!(!names.contains(&"minecraft:stone"));

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    // ── v2 multi-input / single-output ───────────────────────────────────────

    #[test]
    fn v2_multi_input_replaces_all_concrete() -> Result<()> {
        let dir = tmpdir();
        let input = dir.join("input.litematic");
        let output = dir.join("output.litematic");
        let rules = dir.join("rules.json");

        save_litematic_root(&input, &fixture_root())?;
        fs::write(&rules, r#"{
  "schema_version": 2,
  "units": [
    {
      "input": [
        {"name": "minecraft:red_concrete"},
        {"name": "minecraft:blue_concrete"}
      ],
      "output": [{"name": "minecraft:white_concrete", "weight": 1}]
    }
  ]
}"#)?;

        replace_blocks(&input, Some(&output), &rules, false)?;
        let reloaded = load_litematic_root(&output)?;
        let region = reloaded.regions.get("ReplaceFixture").unwrap();
        let names: Vec<_> = region.block_state_palette.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"minecraft:white_concrete"));
        assert!(!names.contains(&"minecraft:red_concrete"));
        assert!(!names.contains(&"minecraft:blue_concrete"));
        // deduplicated: only 1 white_concrete palette entry
        assert_eq!(names.iter().filter(|&&n| n == "minecraft:white_concrete").count(), 1);

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    // ── v2 multi-output weighted (per-position path) ─────────────────────────

    #[test]
    fn v2_multi_output_uses_per_position_path() -> Result<()> {
        let dir = tmpdir();
        let input = dir.join("input.litematic");
        let output = dir.join("output.litematic");
        let rules = dir.join("rules.json");

        save_litematic_root(&input, &fixture_root())?;
        // Replace all concrete (3 positions) with 50/50 red or blue
        fs::write(&rules, r#"{
  "schema_version": 2,
  "units": [
    {
      "input": [
        {"name": "minecraft:red_concrete"},
        {"name": "minecraft:blue_concrete"}
      ],
      "output": [
        {"name": "minecraft:red_concrete", "weight": 1},
        {"name": "minecraft:blue_concrete", "weight": 1}
      ]
    }
  ]
}"#)?;

        let dry = replace_blocks(&input, Some(&output), &rules, true)?;
        assert_eq!(dry.schema_version, 2);
        assert_eq!(dry.per_unit[0].hit_count, 3); // 3 concrete positions in fixture
        assert!(!output.exists());

        replace_blocks(&input, Some(&output), &rules, false)?;
        let reloaded = load_litematic_root(&output)?;
        let region = reloaded.regions.get("ReplaceFixture").unwrap();
        // After per-position random, palette should still contain valid states.
        assert!(region.block_state_palette.iter().all(|s| {
            s.name.starts_with("minecraft:")
        }));

        fs::remove_dir_all(dir)?;
        Ok(())
    }

    // ── v2 state subset matching ──────────────────────────────────────────────

    #[test]
    fn v2_input_subset_match_only_bottom_slabs() -> Result<()> {
        let dir = tmpdir();
        let input = dir.join("input.litematic");
        let output = dir.join("output.litematic");
        let rules = dir.join("rules.json");

        save_litematic_root(&input, &fixture_root())?;
        fs::write(&rules, r#"{
  "schema_version": 2,
  "units": [
    {
      "input": [{"name": "minecraft:oak_slab", "properties": {"type": "bottom"}}],
      "output": [{"name": "minecraft:spruce_slab", "weight": 1}]
    }
  ]
}"#)?;

        replace_blocks(&input, Some(&output), &rules, false)?;
        let reloaded = load_litematic_root(&output)?;
        let region = reloaded.regions.get("ReplaceFixture").unwrap();
        let names: Vec<_> = region.block_state_palette.iter().map(|s| s.name.as_str()).collect();
        // Bottom slab replaced; top slab kept
        assert!(names.contains(&"minecraft:spruce_slab"));
        assert!(names.contains(&"minecraft:oak_slab")); // top slab still there

        fs::remove_dir_all(dir)?;
        Ok(())
    }
}
