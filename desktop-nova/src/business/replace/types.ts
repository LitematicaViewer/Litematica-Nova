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
  input: ReplaceEntry[];
  output: ReplaceOutputEntry[];
}

export interface ReplacePreset {
  schema_version: 2;
  units: ReplaceUnit[];
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
