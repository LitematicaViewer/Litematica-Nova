import { useEffect, useRef, useState } from "react";
import { getApplicationWindowLabel, subscribeApplicationWindowCloseRequested } from "../../../src/platform/nativeWindowGeometry";
import { listenEvent } from "../../../src/platform/events";
import { confirmApplicationClose, getApplicationCloseRequest, requestApplicationClose, type ApplicationCloseRequest } from "../../../src/platform/applicationOperations";

const KEY = "nova.update-recovery.v1";
const captures = new Map<string, () => unknown>();
const closingRequests = new Set<number>();
const handledCloseRequests = new Set<number>();

function readAll(): Record<string, unknown> {
  const raw = localStorage.getItem(KEY);
  if (!raw) return {};
  const value = JSON.parse(raw);
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("更新草稿格式错误，请保留本地数据后重试。");
  return value;
}

export function readUpdateDraft<T>(key: string): T | undefined {
  return readAll()[key] as T | undefined;
}

export function takeUpdateDraft<T>(key: string): T | undefined {
  const all = readAll(), value = all[key] as T | undefined;
  if (value !== undefined) { delete all[key]; localStorage.setItem(KEY, JSON.stringify(all)); }
  return value;
}

/** Only serializable user drafts; never API keys, credentials, renderer internals or binaries. */
export function useUpdateDraft(key: string, draft: unknown) {
  const latest = useRef(draft); latest.current = draft;
  useEffect(() => {
    const capture = () => latest.current;
    captures.set(key, capture);
    return () => { if (captures.get(key) === capture) captures.delete(key); };
  }, [key]);
}

export function preserveUpdateDrafts() {
  const all = readAll();
  for (const [key, capture] of captures) all[key] = capture();
  // A quota/storage error must abort installation, never silently lose edits.
  localStorage.setItem(KEY, JSON.stringify(all));
}

/** Native X stays visible until this synchronous storage transaction succeeds.
 * It preserves recovery drafts, never writes edits back into the source file. */
export async function preserveDraftsForClose(requestId: number): Promise<void> {
  if (closingRequests.has(requestId) || handledCloseRequests.has(requestId)) return;
  closingRequests.add(requestId);
  try {
    console.info("[NOVA_LIFECYCLE]", { stage: "close_drafts_capture", requestId });
    try { preserveUpdateDrafts(); }
    catch (error) {
      await confirmApplicationClose(requestId, false).catch(() => undefined);
      console.error("[NOVA_LIFECYCLE] draft preservation failed; close cancelled", error);
      throw new Error("草稿保留失败，请先保存当前编辑，再重试关闭。当前窗口仍保持打开。");
    }
    console.info("[NOVA_LIFECYCLE]", { stage: "close_drafts_ack_requested", requestId });
    await confirmApplicationClose(requestId, true);
    handledCloseRequests.add(requestId);
    if (handledCloseRequests.size > 32) handledCloseRequests.delete(handledCloseRequests.values().next().value!);
  } finally { closingRequests.delete(requestId); }
}

export function useApplicationCloseRecovery() {
  const [error, setError] = useState("");
  useEffect(() => {
    if (getApplicationWindowLabel() !== "main") return;
    let active = true;
    const accept = (request: ApplicationCloseRequest) => {
      if (!active) return;
      setError("");
      void preserveDraftsForClose(request.requestId).catch(reason => { if (active) setError(String(reason)); });
    };
    // Register independently of config hydration and network updater checks.
    const listening = listenEvent<ApplicationCloseRequest>("nova-close-requested", event => accept(event.payload));
    const recoverPendingClose = async () => {
      // An X pressed before the asynchronous listener finished is not lost.
      const pending = await getApplicationCloseRequest();
      if (pending) accept(pending);
    };
    void listening.then(recoverPendingClose).catch(reason => { if (active) setError(`关闭保护未连接，请重试关闭：${String(reason)}`); });
    // Native close is independent of renderer notifications and the custom
    // event listener. Both recover the SAME pending Host request; no extra
    // render wait, second close request, or direct WebView destruction.
    const nativeClose = subscribeApplicationWindowCloseRequested(() => {
      if (!active) return;
      console.info("[NOVA_LIFECYCLE]", { stage: "close_native_event_received" });
      void recoverPendingClose().catch(reason => { if (active) setError(String(reason)); });
    });
    void nativeClose.catch(reason => { if (active) setError(`窗口关闭保护未连接：${String(reason)}`); });
    const cancelled = listenEvent<{ message: string }>("nova-close-cancelled", event => { if (active) setError(event.payload.message); });
    const delayed = listenEvent<{ requestId: number; message: string }>("nova-close-delayed", event => {
      if (!active || handledCloseRequests.has(event.payload.requestId)) return;
      setError(event.payload.message);
      // Retry only the same outstanding request; in-flight preservation/ACK
      // remains deduplicated and no renderer-completion wait is introduced.
      void recoverPendingClose().catch(reason => { if (active) setError(String(reason)); });
    });
    return () => {
      active = false;
      void listening.then(stop => stop()).catch(() => undefined);
      void nativeClose.then(stop => stop()).catch(() => undefined);
      void cancelled.then(stop => stop()).catch(() => undefined);
      void delayed.then(stop => stop()).catch(() => undefined);
    };
  }, []);
  return { error, retry: async () => {
    if (getApplicationWindowLabel() !== "main") return;
    setError("");
    try { await requestApplicationClose(); } catch (reason) { setError(String(reason)); }
  } };
}

export function clearCurrentUpdateDrafts() {
  const all = readAll();
  for (const key of captures.keys()) delete all[key];
  localStorage.setItem(KEY, JSON.stringify(all));
}

export function releaseNoteSections(notes: string | null) {
  const labels = ["新增", "优化", "修复", "注意事项"];
  const sections = new Map(labels.map((label) => [label, [] as string[]]));
  let active = "注意事项";
  for (const raw of (notes || "").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || /^(?:#+\s*)?(?:版本|日期)\s*[：:]/.test(line)) continue;
    const heading = line.replace(/^#+\s*/, "").replace(/[：:]$/, "").trim();
    if (sections.has(heading)) { active = heading; continue; }
    sections.get(active)!.push(line.replace(/^[-*]\s+/, ""));
  }
  return labels.map((label) => ({ label, lines: sections.get(label)!.length ? sections.get(label)! : ["未提供"] }));
}
