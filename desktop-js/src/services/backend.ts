import { invoke } from "@tauri-apps/api/core";

export async function readWorkspaceFile(path: string): Promise<string> {
  return await invoke("read_file_string", { path });
}
export async function writeWorkspaceFile(path: string, content: string): Promise<void> {
  return await invoke("write_file_string", { path, content });
}
export async function checkFileExists(path: string): Promise<boolean> {
  return await invoke("check_file_exists", { path });
}
export interface PathInfo {
  raw: string;
  normalized: string;
  parent_dir: string;
  parent_exists: boolean;
  exists: boolean;
  is_dir: boolean;
  is_file: boolean;
  is_absolute: boolean;
  has_litematic_ext: boolean;
}
export async function getWorkspaceRoot(): Promise<string> {
  return await invoke("get_workspace_root");
}
export async function getPathInfo(path: string): Promise<PathInfo> {
  return await invoke("get_path_info", { path });
}
export async function openWorkspacePath(path: string): Promise<void> {
  return await invoke("open_workspace_path", { path });
}
export async function cleanupJsTempFiles(): Promise<string> {
  return await invoke("cleanup_js_temp_files");
}
export async function executeBackend(binaryName: string, args: string[]): Promise<string> {
  return await invoke("execute_backend", { binaryName, args });
}

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

export async function executeBackendTrace(binaryName: string, args: string[]): Promise<BackendTrace> {
  return await invoke("execute_backend_trace", { binaryName, args });
}
export async function startNativeViewer(filePath: string, displayMode: string = "full"): Promise<void> {
  return await invoke("start_native_viewer", { filePath, displayMode });
}

export interface RenderPreviewOutput {
  preview_path: string;
  data_url: string;
  stdout: string;
  stderr: string;
  exit_code: number | null;
}
export async function renderPreviewImage(filePath: string, displayMode: string): Promise<RenderPreviewOutput> {
  return await invoke("render_preview_image", { filePath, displayMode });
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
export async function startCacheBuildTask(filePath: string, buildMode: string): Promise<CacheBuildLaunch> {
  return await invoke("start_cache_build_task", { filePath, buildMode });
}
export async function killCacheBuildTask(): Promise<void> {
  return await invoke("kill_cache_build_task");
}
export async function pollCacheBuildTask(): Promise<CacheBuildSnapshot> {
  return await invoke("poll_cache_build_task");
}

export interface AiPublicConfig {
  provider: string;
  base_url: string;
  model: string;
  has_key: boolean;
  key_status: string;
  storage_note: string;
}

export interface AiSaveConfigInput {
  provider: string;
  base_url: string;
  model: string;
  api_key?: string | null;
}

export interface AiTestResult {
  ok: boolean;
  message: string;
}

export interface AiChatMessageWire {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface AiChatCompletionInput {
  messages: AiChatMessageWire[];
}

export interface AiChatCompletionOutput {
  provider: string;
  model: string;
  content: string;
}

export async function aiGetConfig(): Promise<AiPublicConfig> {
  return await invoke("ai_get_config");
}

export async function aiSaveConfig(input: AiSaveConfigInput): Promise<AiPublicConfig> {
  return await invoke("ai_save_config", { input });
}

export async function aiClearKey(): Promise<AiPublicConfig> {
  return await invoke("ai_clear_key");
}

export async function aiTestConnection(): Promise<AiTestResult> {
  return await invoke("ai_test_connection");
}

export async function aiChatCompletion(input: AiChatCompletionInput): Promise<AiChatCompletionOutput> {
  return await invoke("ai_chat_completion", { input });
}
