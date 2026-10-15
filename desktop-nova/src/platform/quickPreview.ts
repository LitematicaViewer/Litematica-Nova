import { invoke } from "./tauri";
import type { PresentationIntent, PresentationPlacement } from "./applicationRender";
import { windowsPathKey } from "../services/filePathIdentity";

export interface QuickPreviewView {
  previewId: string;
  sourcePath: string;
  renderMode: "fast_experimental";
  state: "starting" | "ready" | "failed" | "closed";
  presentation: PresentationIntent;
  presentationRevision: number;
  visible: boolean;
  error: string | null;
  preload?: boolean;
  resource?: QuickResourceReport | null;
}

export interface QuickResourceReport {
  schema: "nova.quick.resource.v1";
  requestId: string;
  sourceBytes: number;
  preload: boolean;
  pressure: "unmeasured" | "normal" | "warning" | "hard" | "invalid";
  pressureBasisPoints: number | null;
  budget: { adapterName: string; budgetBytes: number; currentUsageBytes: number; safeAvailableBytes: number } | null;
  estimate: { gpuResidentUpperBytes: number; gpuPeakUpperBytes: number; uploadStagingUpperBytes: number } | null;
  invalidReason: string | null;
  decisionRequired: boolean;
}
export type QuickResourceAction = "preload" | "realtime" | "reduced_realtime" | "cancel";

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
  return value && windowsPathKey(value.sourcePath) === windowsPathKey(sourcePath) ? value : null;
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
  }, snapshot?.state === "starting" || (snapshot && desired.get(snapshot.previewId)?.visible && !snapshot.visible) ? 8 : 750);
}
export function subscribeQuickPreview(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) void refreshQuickPreview().catch(() => undefined).finally(schedule);
  return () => {
    listeners.delete(listener);
    if (!listeners.size && timer) { clearTimeout(timer); timer = undefined; }
  };
}
export async function startQuickPreview(sourcePath: string, preload = false, clickStartedAtEpochMs = Date.now()) {
  ++snapshotRevision;
  const value = await invoke<QuickPreviewView>("application_quick_preview_start", { input: { sourcePath, ...(preload ? { preload } : {}), clickStartedAtEpochMs } });
  ++snapshotRevision;
  publish(value);
  schedule();
  return value;
}
export async function decideQuickPreviewResources(previewId: string, action: QuickResourceAction, warningConfirmed = false) {
  if (snapshot?.previewId !== previewId) throw new Error("预算决策不属于当前快速预览");
  ++snapshotRevision;
  const value = await invoke<QuickPreviewView>("application_quick_preview_resource_decision", { previewId, action, warningConfirmed });
  ++snapshotRevision;
  if (snapshot?.previewId === previewId) publish(value);
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
  schedule();
  const send = async () => {
    if (presentationRevisions.get(previewId) !== revision) return;
    const value = await invoke<QuickPreviewView>("application_quick_preview_present", { previewId, intent: immutable, revision });
    ++snapshotRevision;
    if (snapshot?.previewId === previewId && presentationRevisions.get(previewId) === revision) { publish(value); schedule(); }
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
