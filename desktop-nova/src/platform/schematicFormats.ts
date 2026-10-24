import { invoke } from "./tauri";
export type SchematicFormat = "litematic" | "structure" | "create" | "anvilCraft" | "buildingGadgetsLegacy" | "buildingGadgets2";
export interface FormatReport {
  format: SchematicFormat; variant: string; dataVersion: number | null; size: [number, number, number]; origin: [number, number, number];
  regions: number; blocks: number; blockEntities: number; entities: number; namespaces: string[];
  canImport: boolean; canExport: boolean; missingSemantics: string[];
}
export interface FormatResult {
  sourcePath?: string; sourceIdentity?: string; snapshotPath?: string; snapshotIdentity?: string;
  nativePassthrough?: boolean; report?: FormatReport;
  clipboardText?: string;
  sourceVersions?: { version: string; dataVersion: number | null }[];
  capabilities?: { format: SchematicFormat; canExport: boolean; reason?: string }[];
}
export interface FormatIntent {
  action: "registry" | "inspect" | "libraryInspect" | "runtimeInput" | "decodeToSnapshot" | "export" | "encodeText";
  input?: string; clipboardText?: string; sourceDataVersion?: number; format?: SchematicFormat; output?: string;
}
export function querySchematicFormat(input: FormatIntent, operationId = crypto.randomUUID()): Promise<FormatResult> {
  return invoke("application_data_query", { input: { operationId, kind: "format", input } });
}
