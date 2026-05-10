import { invoke } from "./tauri";

export async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return await invoke<T>(command, args);
}
