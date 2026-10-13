import { invoke } from "../platform/tauri";
import type { RedenDownloadOutput, RedenMachine, RedenSearchResponse, RedenSizes } from "./redenLibrary";

export function onlineRequestId(): string { return `online-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`; }
export function searchOnlineSource(sourceId: string, query: string, requestId: string): Promise<RedenSearchResponse> {
  return invoke("application_online_search", { input: { sourceId, query, requestId } });
}
export function fetchOnlineSourceDetail(sourceId: string, itemId: string, requestId: string, downloadFiles = false): Promise<RedenMachine> {
  return invoke("application_online_detail", { input: { sourceId, itemId, requestId, downloadFiles } });
}
export function downloadOnlineSource(sourceId: string, itemId: string, attachmentIndex: number, requestId: string, sizes?: RedenSizes): Promise<RedenDownloadOutput> {
  return invoke("application_online_download", { input: { sourceId, itemId, attachmentIndex, requestId, sizes: sizes || null } });
}
export function cancelOnlineRequest(requestId: string): Promise<void> { return invoke("application_online_cancel", { requestId }); }
