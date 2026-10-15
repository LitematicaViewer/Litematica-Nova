import {
  getUserConfigFilePath,
  writeUserConfigFile,
  deleteUserConfigFile,
  readUserConfigFile,
  listDirectoryEntries,
} from "../../services/backend";
import { replaceProjectBlocks } from "../../platform/applicationOperations";

import type {
  ReplaceUnit,
  ReplaceItem,
  ReplacePreset,
  ReplacePreviewSummary,
} from "./types";
import { isSeparatorItem } from "./types";

const REPLACE_PRESETS_DIR = "replace/presets";


// ── 分隔符工具 ───────────────────────────────────────────────────────────────

/**
 * 将 ReplaceItem 列表按串行分隔符切分为若干"阶段"，
 * 每个阶段是一个纯 ReplaceUnit 数组。空阶段会被过滤掉。
 */
export function splitIntoStages(items: ReplaceItem[]): ReplaceUnit[][] {
  const stages: ReplaceUnit[][] = [];
  let current: ReplaceUnit[] = [];
  for (const item of items) {
    if (isSeparatorItem(item)) {
      if (current.length > 0) {
        stages.push(current);
        current = [];
      }
    } else {
      current.push(item);
    }
  }
  if (current.length > 0) stages.push(current);
  return stages;
}

// ── 主业务函数 ───────────────────────────────────────────────────────────────

/**
 * 预览替换结果（dry-run）。Application Host 按真实串行阶段验证，
 * per_unit 的索引与 items 中实际 ReplaceUnit 的顺序对应。
 */
export async function dryRunReplaceUnits(
  currentFile: string,
  items: ReplaceItem[]
): Promise<ReplacePreviewSummary> {
  const result = await replaceProjectBlocks({
    sourcePath: currentFile, outputPath: null, rules: [], stages: splitIntoStages(items), dryRun: true,
  });
  return JSON.parse(result.message) as ReplacePreviewSummary;
}

/** Application Host owns the whole serial pipeline and all intermediate files. */
export async function applyReplaceUnits(currentFile: string, outputPath: string, items: ReplaceItem[]): Promise<string> {
  const result = await replaceProjectBlocks({
    sourcePath: currentFile, outputPath, rules: [], stages: splitIntoStages(items), dryRun: false,
  });
  return result.message;
}

// ── 预设管理 ─────────────────────────────────────────────────────────────────

/**
 * 列出已保存的预设名称（不含 .json 后缀）。
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
 * 将当前 items（含分隔符）保存为命名预设。
 */
export async function saveReplacePreset(
  name: string,
  items: ReplaceItem[]
): Promise<void> {
  const units = items.filter((item): item is ReplaceUnit => !isSeparatorItem(item));
  const preset: ReplacePreset = { schema_version: 2, units, items };
  const relPath = `${REPLACE_PRESETS_DIR}/${sanitizePresetName(name)}.json`;
  await writeUserConfigFile(relPath, JSON.stringify(preset, null, 2));
}

/**
 * 从命名预设加载 items 列表。兼容旧版（仅有 units 字段）预设。
 * 加载失败返回 null。
 */
export async function loadReplacePreset(
  name: string
): Promise<ReplaceItem[] | null> {
  const relPath = `${REPLACE_PRESETS_DIR}/${sanitizePresetName(name)}.json`;
  try {
    const raw = await readUserConfigFile(relPath);
    const preset = JSON.parse(raw) as ReplacePreset;
    // items 字段优先（含分隔符），否则回落到旧版 units 字段
    if (Array.isArray(preset.items)) return preset.items;
    return preset.units ?? null;
  } catch {
    return null;
  }
}

/**
 * 删除命名预设文件。
 */
export async function deleteReplacePreset(name: string): Promise<void> {
  const relPath = `${REPLACE_PRESETS_DIR}/${sanitizePresetName(name)}.json`;
  await deleteUserConfigFile(relPath);
}

/**
 * 在系统文件管理器中打开预设目录。
 */
export async function openReplacePresetFolder(): Promise<void> {
  const { openExternalPath } = await import("../actions");
  const dirPath = await getUserConfigFilePath(REPLACE_PRESETS_DIR);
  await openExternalPath(dirPath);
}

// ── 内部工具 ─────────────────────────────────────────────────────────────────

function sanitizePresetName(name: string): string {
  return name.replace(/[/\\:*?"<>|]/g, "_").trim() || "preset";
}