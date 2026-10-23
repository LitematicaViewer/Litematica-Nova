import { invoke } from "../platform/tauri";

export interface WorldExportInput {
  sourcePath: string;
  outputDirectory: string;
  worldName: string;
  origin: { x: number; y: number; z: number };
}
export interface WorldExportResult {
  outputPath: string;
  gameVersion: string;
  dataVersion: number;
  blockCount: number;
  entityCount: number;
  blockEntityCount: number;
  chunkCount: number;
  spawnPlatformBlocks: number;
  spawn: { x: number; y: number; z: number };
}
export function exportMinecraftWorld(input: WorldExportInput): Promise<WorldExportResult> {
  return invoke<WorldExportResult>("application_export_world", { input });
}
