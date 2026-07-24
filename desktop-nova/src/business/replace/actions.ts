import {
  executeCoreBackend,
  getUserConfigFilePath,
  writeUserConfigFile,
  deleteUserConfigFile,
  readUserConfigFile,
  listDirectoryEntries,
} from "../../services/backend";
import type {
  ReplaceUnit,
  ReplaceItem,
  ReplacePreset,
  ReplacePreviewSummary,
} from "./types";
import { isSeparatorItem } from "./types";

const REPLACE_PRESETS_DIR = "replace/presets";
const REPLACE_TEMP_RULES = "render/replace_units_js.json";

// ── 分隔符工具 ───────────────────────────────────────────────────────────────

/**
 * 将 ReplaceItem 列表按串行分隔符切分为若干"阶段"，
 * 每个阶段是一个纯 ReplaceUnit 数组。空阶段会被过滤掉。
 */
function splitIntoStages(items: ReplaceItem[]): ReplaceUnit[][] {
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
 * 预览替换结果（dry-run）。分隔符会被忽略，所有单元作为一次扫描处理，
 * per_unit 的索引与 items 中实际 ReplaceUnit 的顺序对应。
 */
export async function dryRunReplaceUnits(
  currentFile: string,
  items: ReplaceItem[]
): Promise<ReplacePreviewSummary> {
  const units = items.filter((item): item is ReplaceUnit => !isSeparatorItem(item));
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
 * 执行替换。若存在串行分隔符，则将 items 切分为多个阶段，
 * 每个阶段完成后才将其输出作为下一阶段的输入，直到最终写入 outputPath。
 * 中间临时文件在完成后自动清理。
 */
export async function applyReplaceUnits(
  currentFile: string,
  outputPath: string,
  items: ReplaceItem[]
): Promise<string> {
  const stages = splitIntoStages(items);
  if (stages.length === 0) return "";

  if (stages.length === 1) {
    const rulesPath = await saveUnitsToTemp(stages[0]);
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

  // 多阶段串行执行：每阶段输出为下一阶段输入
  const tag = Date.now().toString(36);
  const tempRelPaths: string[] = [];
  let inputPath = currentFile;

  try {
    for (let i = 0; i < stages.length; i++) {
      const isLast = i === stages.length - 1;
      let stagePath: string;

      if (isLast) {
        stagePath = outputPath;
      } else {
        const relPath = `render/replace_stage_${tag}_${i}.litematic`;
        tempRelPaths.push(relPath);
        stagePath = await getUserConfigFilePath(relPath);
      }

      const rulesPath = await saveUnitsToTemp(stages[i]);
      await executeCoreBackend([
        "replace-blocks",
        "--input",
        inputPath,
        "--output",
        stagePath,
        "--rules",
        rulesPath,
      ]);
      inputPath = stagePath;
    }
  } finally {
    // 清理中间临时文件（忽略错误）
    for (const relPath of tempRelPaths) {
      deleteUserConfigFile(relPath).catch(() => {});
    }
  }

  return outputPath;
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

async function saveUnitsToTemp(units: ReplaceUnit[]): Promise<string> {
  const preset: ReplacePreset = { schema_version: 2, units };
  await writeUserConfigFile(REPLACE_TEMP_RULES, JSON.stringify(preset, null, 2));
  return getUserConfigFilePath(REPLACE_TEMP_RULES);
}

function sanitizePresetName(name: string): string {
  return name.replace(/[/\\:*?"<>|]/g, "_").trim() || "preset";
}
