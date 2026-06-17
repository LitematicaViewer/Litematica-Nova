import {
  CORE_BACKEND_EXE,
  executeBackendCached,
  invalidateBackendCache,
} from "./backend";

/**
 * Read-only `litematica_core` command results, cached in the Rust main process.
 *
 * The backend is invoked as a fresh subprocess on every call, so the Rust-side
 * LRU never survives across invocations, and a renderer-only Map cannot be shared
 * between windows (the material-list window is a separate WebView with its own JS
 * heap). The cache therefore lives in Rust (`execute_backend_cached`), shared by
 * every window of the single Tauri process and validated against the source
 * file's mtime.
 *
 * This module adds a thin per-window in-flight dedup so concurrent calls for the
 * same (path, args) — e.g. React StrictMode double effects — collapse to one IPC.
 */
const inFlight = new Map<string, Promise<string>>();

function flightKey(filePath: string, args: string[]): string {
  return `${filePath} ${JSON.stringify(args)}`;
}

/**
 * Runs a read-only core command via the shared Rust cache. `filePath` is the
 * source file whose mtime invalidates the entry.
 */
export async function getCachedCoreOutput(filePath: string, args: string[]): Promise<string> {
  const key = flightKey(filePath, args);
  const pending = inFlight.get(key);
  if (pending) {
    return pending;
  }

  const task = executeBackendCached(CORE_BACKEND_EXE, args, filePath).finally(() => {
    inFlight.delete(key);
  });
  inFlight.set(key, task);
  return task;
}

/** Convenience wrapper for the bare `analyze <file>` command. */
export function getAnalyzeOutput(filePath: string): Promise<string> {
  return getCachedCoreOutput(filePath, ["analyze", filePath]);
}

/**
 * Drops cached results for a file in the Rust process (empty/undefined clears
 * all). Call after a renderer-driven in-place edit of the file.
 */
export function invalidateCoreReadCache(filePath?: string): void {
  void invalidateBackendCache(filePath ?? "");
}
