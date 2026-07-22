import { invoke } from "./tauri";

export const CORE_BACKEND_EXE = "litematica_core.exe";
export const NATIVE_VIEWER_EXE = "litematica_native_viewer.exe";
export const CORE_BACKEND_RELATIVE_PATH = "bin/viewer-backend/litematica_core.exe";
export const NATIVE_VIEWER_RELATIVE_PATH = "bin/viewer-backend/litematica_native_viewer.exe";

export interface BackendTrace {
  actual_core_exe_path: string;
  actual_core_exe_exists: boolean;
  actual_core_exe_modified_time: string | null;
  actual_core_exe_file_size: number | null;
  actual_core_exe_sha256: string | null;
  command_args: string[];
  backend_stdout: string;
  backend_stderr: string;
  backend_exit_code: number | null;
}

export interface CacheBuildLaunch {
  file_path: string;
  progress_file: string;
  cache_file: string;
  stdout_file: string;
  stderr_file: string;
}

export interface CacheBuildSnapshot {
  running: boolean;
  exit_code: number | null;
  progress_file: string | null;
  cache_file: string | null;
  stdout_file: string | null;
  stderr_file: string | null;
  progress_json: string | null;
  stdout_tail: string;
  stderr_tail: string;
  cache_exists: boolean;
}

export function executeBackend(binaryName: string, args: string[]): Promise<string> {
  return invoke("execute_backend", { binaryName, args });
}

/**
 * Runs a read-only backend command through the Rust-side cache shared by all
 * windows. `cacheKeyPath` is the source file whose mtime validates the entry.
 */
export function executeBackendCached(binaryName: string, args: string[], cacheKeyPath: string): Promise<string> {
  return invoke("execute_backend_cached", { binaryName, args, cacheKeyPath });
}

/** Drops Rust-side cached results for a file path (empty string clears all). */
export function invalidateBackendCache(path: string): Promise<void> {
  return invoke("invalidate_backend_cache", { path });
}

export function executeBackendTrace(binaryName: string, args: string[]): Promise<BackendTrace> {
  return invoke("execute_backend_trace", { binaryName, args });
}

export function executeCoreBackend(args: string[]): Promise<string> {
  return executeBackend(CORE_BACKEND_EXE, args);
}

export function executeCoreBackendTrace(args: string[]): Promise<BackendTrace> {
  return executeBackendTrace(CORE_BACKEND_EXE, args);
}

export function executeNativeViewerBackend(args: string[]): Promise<string> {
  return executeBackend(NATIVE_VIEWER_EXE, args);
}

export function startCacheBuildTask(filePath: string, buildMode: string): Promise<CacheBuildLaunch> {
  return invoke("start_cache_build_task", { filePath, buildMode });
}

export function killCacheBuildTask(): Promise<void> {
  return invoke("kill_cache_build_task");
}

export function pollCacheBuildTask(): Promise<CacheBuildSnapshot> {
  return invoke("poll_cache_build_task");
}
