import {
  executeCoreBackend,
  getUserConfigFilePath,
  writeUserConfigFile,
  readUserConfigFile,
  listDirectoryEntries,
} from "../../services/backend";
import type {
  ReplaceUnit,
  ReplacePreset,
  ReplacePreviewSummary,
} from "./types";

const REPLACE_PRESETS_DIR = "replace/presets";
const REPLACE_TEMP_RULES = "render/replace_units_js.json";

/**
 * Dry-run replacement with v2 units format. Returns parsed summary.
 */
export async function dryRunReplaceUnits(
  currentFile: string,
  units: ReplaceUnit[]
): Promise<ReplacePreviewSummary> {
  const rulesPath = await saveUnitsToTemp(units);
  const output = await executeCoreBackend([
    "replace-blocks",
    "--input",
    currentFile,
    "--rules",
    rulesPath,
    "--dry-run",
  ]);
  return JSON.parse(output) as ReplacePreviewSummary;
}

/**
 * Apply replacement with v2 units format to outputPath.
 */
export async function applyReplaceUnits(
  currentFile: string,
  outputPath: string,
  units: ReplaceUnit[]
): Promise<string> {
  const rulesPath = await saveUnitsToTemp(units);
  return executeCoreBackend([
    "replace-blocks",
    "--input",
    currentFile,
    "--output",
    outputPath,
    "--rules",
    rulesPath,
  ]);
}

// ── Preset management ──────────────────────────────────────────────────────

/**
 * List saved preset names (without .json extension).
 */
export async function listReplacePresets(): Promise<string[]> {
  const dirPath = await getUserConfigFilePath(REPLACE_PRESETS_DIR);
  const entries = await listDirectoryEntries(dirPath).catch(() => []);
  return entries
    .filter((e) => !e.is_dir && e.name.endsWith(".json"))
    .map((e) => e.name.slice(0, -5))
    .sort((a, b) => a.localeCompare(b));
}

/**
 * Save current units as a named preset.
 */
export async function saveReplacePreset(
  name: string,
  units: ReplaceUnit[]
): Promise<void> {
  const preset: ReplacePreset = { schema_version: 2, units };
  const relPath = `${REPLACE_PRESETS_DIR}/${sanitizePresetName(name)}.json`;
  await writeUserConfigFile(relPath, JSON.stringify(preset, null, 2));
}

/**
 * Load units from a named preset. Returns null if not found or parse fails.
 */
export async function loadReplacePreset(
  name: string
): Promise<ReplaceUnit[] | null> {
  const relPath = `${REPLACE_PRESETS_DIR}/${sanitizePresetName(name)}.json`;
  try {
    const raw = await readUserConfigFile(relPath);
    const preset = JSON.parse(raw) as ReplacePreset;
    return preset.units ?? null;
  } catch {
    return null;
  }
}

/**
 * Delete a named preset file.
 * Falls back to overwriting with a tombstone if deletion isn't supported.
 */
export async function deleteReplacePreset(name: string): Promise<void> {
  // We mark the preset as deleted by writing an empty units array.
  // The list function already filters by .json files, so we save an
  // empty-units preset that the UI should treat as "deleted".
  // If the platform exposes a delete command in a later iteration, swap this.
  const relPath = `${REPLACE_PRESETS_DIR}/${sanitizePresetName(name)}.json`;
  await writeUserConfigFile(relPath, JSON.stringify({ schema_version: 2, _deleted: true, units: [] }, null, 2));
}

/**
 * Open the presets directory in the OS file manager.
 */
export async function openReplacePresetFolder(): Promise<void> {
  const { openExternalPath } = await import("../actions");
  const dirPath = await getUserConfigFilePath(REPLACE_PRESETS_DIR);
  await openExternalPath(dirPath);
}

// ── Internal helpers ────────────────────────────────────────────────────────

async function saveUnitsToTemp(units: ReplaceUnit[]): Promise<string> {
  const preset: ReplacePreset = { schema_version: 2, units };
  await writeUserConfigFile(REPLACE_TEMP_RULES, JSON.stringify(preset, null, 2));
  return getUserConfigFilePath(REPLACE_TEMP_RULES);
}

function sanitizePresetName(name: string): string {
  // Replace characters invalid in filenames with underscore.
  return name.replace(/[/\\:*?"<>|]/g, "_").trim() || "preset";
}
