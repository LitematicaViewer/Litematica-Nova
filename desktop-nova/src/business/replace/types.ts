/**
 * Replace module types (schema_version 2)
 */

export interface ReplaceEntry {
  name: string;
  properties?: Record<string, string>;
}

export interface ReplaceOutputEntry extends ReplaceEntry {
  weight: number;
}

export interface ReplaceUnit {
  label?: string;
  input: ReplaceEntry[];
  output: ReplaceOutputEntry[];
}

/** 串行分隔符：插入两个替换单元之间，分隔符上方的单元全部完成后才开始下方单元。 */
export interface SerialSeparator {
  _kind: 'separator';
}

/** 替换列表中的单个元素（替换单元或串行分隔符）。 */
export type ReplaceItem = ReplaceUnit | SerialSeparator;

/** 判断元素是否为串行分隔符。 */
export function isSeparatorItem(item: ReplaceItem): item is SerialSeparator {
  return (item as any)._kind === 'separator';
}

export interface ReplacePreset {
  schema_version: 2;
  /** 兼容旧版：仅包含 ReplaceUnit 列表（无分隔符）。 */
  units: ReplaceUnit[];
  /** 扩展版：可包含分隔符的完整列表，优先级高于 units。 */
  items?: ReplaceItem[];
}

// Summary types returned from backend
export interface OutputDistEntry {
  output_index: number;
  name: string;
  estimated_count: number;
  actual_count: number | null;
}

export interface UnitPreviewSummary {
  unit_index: number;
  hit_count: number;
  output_distribution: OutputDistEntry[];
  warnings: string[];
  invalid_entries: string[];
}

export interface ReplacePreviewSummary {
  schema_version: number;
  input_file: string;
  output_file: string | null;
  dry_run: boolean;
  regions_scanned: number;
  palette_entries_scanned: number;
  palette_entries_changed: number;
  block_positions_affected: number;
  per_unit: UnitPreviewSummary[];
  warnings: string[];
  /** Diagnostic log lines populated by the backend for debugging. */
  debug_log?: string[];
}
