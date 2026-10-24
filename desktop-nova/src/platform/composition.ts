import { invoke } from "./tauri";
import { getDataOperationStatus } from "./applicationOperations";

export type Vec3 = [number, number, number];
export interface SceneBounds { min: Vec3; max: Vec3 }
export interface CompositionTransform { offset: Vec3; origin: Vec3; rotation: 0 | 90 | 180 | 270; mirrorX: boolean; mirrorZ: boolean }
export interface CompositionPart { id: string; name: string; sourcePath: string; snapshotId: string; selection: SceneBounds | null; regions: string[]; transform: CompositionTransform; visible: boolean; locked: boolean; clearAir: boolean }
export interface CompositionState { name: string; parts: CompositionPart[]; targetDataVersion: number | null; conflictPolicy: "reject" | "keepFirst" | "keepLast" }
export interface CompositionProject { schema: number; id: string; generation: number; state: CompositionState; }
export interface CompositionPartView { part: CompositionPart; bounds: SceneBounds | null; size: Vec3 | null; blockCount: number; dataVersion: number | null; regions: string[] }
export interface CompositionReport { blockCount: number; blockEntityCount: number; entityCount: number; sectionCount: number; collisionCount: number; collisions: { position: Vec3; keptInstance: string; incomingInstance: string }[]; issues: string[]; bounds: SceneBounds | null; canExport: boolean }
export interface CompositionResult { project?: CompositionProject; parts?: CompositionPartView[]; projects?: CompositionProject[]; canUndo?: boolean; canRedo?: boolean; projectId?: string; generation?: number; outputPath?: string; outputPaths?: string[]; report?: CompositionReport; outputs?: { name: string; bounds: SceneBounds; report: CompositionReport }[]; writable?: boolean; targetVersion?: string; migrationReports?: { partId: string; name: string; conversion: { writable: boolean; sourceVersion: string; targetVersion: string; blockedPayloads: string[]; payloadIssues: { message: string; typeName: string }[] } }[] }
export interface CompositionIntent { action: "list" | "create" | "open" | "add" | "update" | "duplicate" | "remove" | "reorder" | "undo" | "redo" | "save" | "report" | "preview" | "export" | "splitReport" | "split" | "migrationReport" | "migrate"; projectId?: string; generation?: number; name?: string; sourcePaths?: string[]; placement?: "original" | "beside"; partId?: string; partIds?: string[]; patch?: Partial<CompositionPart>; conflictPolicy?: CompositionState["conflictPolicy"]; format?: string; outputPath?: string; splitMode?: "regions" | "boxes" | "plane"; axis?: "x" | "y" | "z"; at?: number; boxes?: { name: string; bounds: SceneBounds }[]; outputDirectory?: string; targetVersion?: string; splitOutput?: "files" | "project" }
export function queryComposition(input: CompositionIntent, operationId: string): Promise<CompositionResult> {
  // Host injects the trusted user-data directory; callers cannot choose storage.
  return invoke("application_data_query", { input: { operationId, kind: "composition", input } });
}
export const cancelComposition = (operationId: string) => getDataOperationStatus(operationId, true);
