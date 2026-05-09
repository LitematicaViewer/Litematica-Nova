import { checkFileExists, readWorkspaceFile } from "./backend";

export interface RenderProgress {
  ready: boolean;
  total_chunks: number;
  built_chunks: number;
  percent: number;
  phase: string;
  error?: string;
}

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
      error: json.error || ""
    };
  } catch (e) {
    return null;
  }
}
