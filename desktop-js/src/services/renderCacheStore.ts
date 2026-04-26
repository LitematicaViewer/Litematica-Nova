import { CacheBuildLaunch } from "./backend";
import { RenderProgress } from "./renderService";

export type RenderCacheStatus = "idle" | "building" | "ready" | "error";

export interface RenderCacheState {
  currentFile: string;
  displayMode: string;
  cacheFile: string;
  readyFile: string;
  progressFile: string;
  stdoutFile: string;
  stderrFile: string;
  previewPath?: string;
  status: RenderCacheStatus;
  progress: RenderProgress | null;
  stage: string;
  updatedAt: number;
}

const STORAGE_KEY = "lba.renderCacheState.v1";
const states = new Map<string, RenderCacheState>();
const listeners = new Set<() => void>();

function keyFor(file: string, mode: string) {
  return `${file}::${mode || "normal"}`;
}

function save() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(Array.from(states.values())));
  } catch {
    // localStorage is only a convenience cache; runtime state still lives in memory.
  }
}

export function hydrateRenderCacheStore() {
  if (states.size > 0) return;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    if (Array.isArray(parsed)) {
      parsed.forEach((state) => {
        if (state?.currentFile && state?.displayMode) {
          states.set(keyFor(state.currentFile, state.displayMode), state);
        }
      });
    }
  } catch {
    // Ignore corrupt cache state.
  }
}

export function subscribeRenderCacheStore(listener: () => void) {
  hydrateRenderCacheStore();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function publish() {
  save();
  listeners.forEach((listener) => listener());
}

export function upsertRenderCacheState(state: RenderCacheState) {
  hydrateRenderCacheStore();
  states.set(keyFor(state.currentFile, state.displayMode), { ...state, updatedAt: Date.now() });
  publish();
}

export function updateRenderCacheState(file: string, mode: string, patch: Partial<RenderCacheState>) {
  hydrateRenderCacheStore();
  const key = keyFor(file, mode);
  const prev = states.get(key);
  if (!prev) return;
  states.set(key, { ...prev, ...patch, updatedAt: Date.now() });
  publish();
}

export function getRenderCacheState(file: string, mode: string) {
  hydrateRenderCacheStore();
  return states.get(keyFor(file, mode)) || null;
}

export function getLatestRenderCacheState(file: string) {
  hydrateRenderCacheStore();
  return (
    Array.from(states.values())
      .filter((state) => state.currentFile === file)
      .sort((a, b) => b.updatedAt - a.updatedAt)[0] || null
  );
}

export function stateFromLaunch(file: string, mode: string, launch: CacheBuildLaunch): RenderCacheState {
  return {
    currentFile: file,
    displayMode: mode,
    cacheFile: launch.cache_file,
    readyFile: launch.progress_file,
    progressFile: launch.progress_file,
    stdoutFile: launch.stdout_file,
    stderrFile: launch.stderr_file,
    status: "building",
    progress: null,
    stage: "3D cache building",
    updatedAt: Date.now(),
  };
}
