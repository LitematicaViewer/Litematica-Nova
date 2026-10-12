import { invoke } from "./tauri";
import type { PresentationIntent, PresentationPlacement } from "./applicationRender";

export interface QuickPreviewView {
  previewId: string;
  sourcePath: string;
  renderMode: "fast_experimental";
  state: "starting" | "ready" | "failed" | "closed";
  presentation: PresentationIntent;
  presentationRevision: number;
  visible: boolean;
  error: string | null;
}

let snapshot: QuickPreviewView | null = null;
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setTimeout> | undefined;
let refreshing: Promise<void> | null = null;
let snapshotRevision = 0;
const commands = new Map<string, Promise<void>>();
const desired = new Map<string, PresentationIntent>();
const suspended = new Map<string, PresentationIntent>();
const pending = new Map<string, number>();
const presentationRevisions = new Map<string, number>();

function publish(next: QuickPreviewView | null) {
  if (next) presentationRevisions.set(next.previewId,
    Math.max(presentationRevisions.get(next.previewId) ?? 0, next.presentationRevision ?? 0));
  snapshot = next;
  if (next && !pending.get(next.previewId)) desired.set(next.previewId, next.presentation);
  for (const listener of listeners) listener();
}
export function getQuickPreviewState() { return snapshot; }
export function quickPreviewForSource(sourcePath: string, value = snapshot) {
  const key = (path: string) => path.replace(/^\\\\\?\\/, "").replace(/\//g, "\\").toLowerCase();
  return value && key(value.sourcePath) === key(sourcePath) ? value : null;
}
export function refreshQuickPreview(): Promise<void> {
  if (refreshing) return refreshing;
  const revision = snapshotRevision;
  refreshing = invoke<QuickPreviewView | null>("application_quick_preview_snapshot").then((value) => {
    if (revision === snapshotRevision) publish(value);
  }).finally(() => { refreshing = null; });
  return refreshing;
}
function schedule() {
  if (!listeners.size) return;
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = undefined;
    void refreshQuickPreview().catch((error) => {
      if (snapshot) publish({ ...snapshot, error: String(error) });
    }).finally(schedule);
  }, snapshot?.state === "starting" ? 250 : 750);
}
export function subscribeQuickPreview(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) void refreshQuickPreview().catch(() => undefined).finally(schedule);
  return () => {
    listeners.delete(listener);
    if (!listeners.size && timer) { clearTimeout(timer); timer = undefined; }
  };
}
export async function startQuickPreview(sourcePath: string) {
  ++snapshotRevision;
  const value = await invoke<QuickPreviewView>("application_quick_preview_start", { input: { sourcePath } });
  ++snapshotRevision;
  publish(value);
  return value;
}
export async function closeQuickPreview(previewId: string) {
  ++snapshotRevision;
  presentationRevisions.set(previewId, (presentationRevisions.get(previewId) ?? 0) + 1);
  suspended.delete(previewId);
  desired.delete(previewId);
  await invoke("application_quick_preview_close", { previewId });
  ++snapshotRevision;
  if (snapshot?.previewId === previewId) publish({ ...snapshot, state: "closed", visible: false,
    presentation: { mode: "hidden", visible: false, placement: null } });
}
function enqueue(previewId: string, intent: PresentationIntent): Promise<void> {
  ++snapshotRevision;
  const revision = (presentationRevisions.get(previewId) ?? 0) + 1;
  presentationRevisions.set(previewId, revision);
  pending.set(previewId, (pending.get(previewId) ?? 0) + 1);
  const immutable = { ...intent, placement: intent.placement ? { ...intent.placement } : null };
  desired.set(previewId, immutable);
  const send = async () => {
    if (presentationRevisions.get(previewId) !== revision) return;
    const value = await invoke<QuickPreviewView>("application_quick_preview_present", { previewId, intent: immutable, revision });
    ++snapshotRevision;
    if (snapshot?.previewId === previewId && presentationRevisions.get(previewId) === revision) publish(value);
  };
  // Hide is a visibility barrier, not a queued layout update. It must reach the
  // Host without waiting for an obsolete show; the Host rejects older revisions.
  const request = immutable.visible ? (commands.get(previewId) ?? Promise.resolve()).then(send) : send();
  commands.set(previewId, request.catch(() => undefined));
  return request.finally(() => pending.set(previewId, Math.max(0, (pending.get(previewId) ?? 1) - 1)));
}
export function presentQuickPreview(previewId: string, intent: PresentationIntent) {
  suspended.delete(previewId);
  return enqueue(previewId, intent);
}
export async function suspendQuickPreview(previewId: string, observed: PresentationIntent) {
  const intent = desired.get(previewId) ?? observed;
  if (intent.mode !== "embedded" || !intent.visible) return false;
  suspended.set(previewId, intent);
  await enqueue(previewId, { ...intent, visible: false });
  return true;
}
export async function restoreQuickPreview(previewId: string, placement: PresentationPlacement) {
  const saved = suspended.get(previewId);
  if (!saved) return false;
  const intent: PresentationIntent = { ...saved, visible: true, placement };
  await enqueue(previewId, intent);
  if (suspended.get(previewId) === saved) suspended.delete(previewId);
  return true;
}
export async function updateQuickPreviewPlacement(previewId: string, placement: PresentationPlacement, observed: PresentationIntent) {
  const intent = desired.get(previewId) ?? observed;
  if (intent.mode !== "embedded" || !intent.visible) return false;
  const next = { ...intent, placement };
  await enqueue(previewId, next);
  return true;
}
