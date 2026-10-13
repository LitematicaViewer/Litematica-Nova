import type { ApplicationRenderSnapshot, RenderOperationView } from "../platform/applicationRender";
import { checkFileExists, readWorkspaceFile } from "./backend";

// Compatibility for analysis consumers: no Full service is started or queried.
// Consumers receive no render-derived analysis and use the real project-statistics API.
const snapshot: ApplicationRenderSnapshot = { operations: [] };
export function ensureApplicationRenderSubscription(): Promise<void> { return Promise.resolve(); }
export function subscribeApplicationRender(_listener: () => void): () => void { return () => {}; }
export function getApplicationRenderState(): ApplicationRenderSnapshot { return snapshot; }
export function isApplicationRenderStateReady(): boolean { return true; }
export function findRenderOperationForSource(_path: string, _source = snapshot): RenderOperationView | null { return null; }
export type { ApplicationRenderSnapshot, RenderOperationView } from "../platform/applicationRender";

// ---------------------------------------------------------------------------
// 3D cache progress reader (restored; dropped when the merge removed the render
// cache build pipeline). The cache builder writes this JSON to the run directory.
// ---------------------------------------------------------------------------

export interface RenderProgress {
  ready: boolean;
  total_chunks: number;
  built_chunks: number;
  percent: number;
  phase: string;
  error?: string;
}

/** Reads a cache-build progress file, or `null` when it is absent/unreadable. */
export async function readProgressFile(path: string): Promise<RenderProgress | null> {
  const exists = await checkFileExists(path);
  if (!exists) return null;

  try {
    const raw = await readWorkspaceFile(path);
    if (!raw) return null;
    const json = JSON.parse(raw);
    return {
      ready: !!json.ready,
      total_chunks: json.total_chunks || 0,
      built_chunks: json.built_chunks || 0,
      percent: json.percent || 0.0,
      phase: json.phase || "",
      error: json.error || "",
    };
  } catch {
    return null;
  }
}
