import { invoke } from "../platform/tauri";
export interface WorldExportInput {
  taskId: string;
  sourcePath: string;
  outputDirectory: string;
  worldName: string;
  targetVersion: string;
  origin: { x: number; y: number; z: number };
}
export interface WorldExportProfile {
  version: string;
  data_version: number | null;
  family: string;
  min_y: number;
  height: number;
  generator: string;
  runtime_validation: string;
}
export interface WorldExportOptions {
  profiles: WorldExportProfile[];
  sourceDataVersion: number | null;
  defaultVersion: string;
}
export interface WorldExportProgress { stage: string; completed: number; total: number }
export interface WorldExportResult {
  outputPath: string;
  gameVersion: string;
  dataVersion: number | null;
  blockCount: number;
  entityCount: number;
  blockEntityCount: number;
  chunkCount: number;
  spawnPlatformBlocks: number;
  spawn: { x: number; y: number; z: number };
}
export function worldExportOptions(sourcePath: string): Promise<WorldExportOptions> {
  return invoke("application_world_export_profiles", { sourcePath });
}
export function worldExportStatus(taskId: string, cancel = false): Promise<{ active: boolean; progress?: WorldExportProgress }> {
  return invoke("application_world_export_status", { taskId, cancel });
}
export function exportMinecraftWorld(input: WorldExportInput): Promise<WorldExportResult> {
  return invoke<WorldExportResult>("application_export_world", { input });
}
