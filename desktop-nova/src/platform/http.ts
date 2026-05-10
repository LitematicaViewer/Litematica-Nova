import { invoke } from "./tauri";

export function httpFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  return fetch(input, init);
}

export function redenSearchLitematica<T>(query: string): Promise<T> {
  return invoke("reden_search_litematica", { query });
}

export function redenMachineDetail<T>(machineId: string): Promise<T> {
  return invoke("reden_machine_detail", { machineId });
}

export function redenDownloadAttachment<T>(machineId: string, attachmentIndex: number): Promise<T> {
  return invoke("reden_download_attachment", { machineId, attachmentIndex });
}

export function redenDownloadParametric<T>(machineId: string, sizes: unknown): Promise<T> {
  return invoke("reden_download_parametric", { machineId, sizes });
}
