import { windowsPathKey } from "./filePathIdentity";
import {
  getApplicationRenderSnapshot,
  subscribeApplicationRenderState,
  type ApplicationRenderSnapshot,
  type RenderOperationView,
} from "../platform/applicationRender";

const EMPTY_SNAPSHOT: ApplicationRenderSnapshot = { operations: [] };

let snapshot = EMPTY_SNAPSHOT;
let snapshotReady = false;
let subscription: Promise<void> | null = null;
let notificationScheduled = false;
let eventRevision = 0;
const listeners = new Set<() => void>();

function publish(next: ApplicationRenderSnapshot) {
  snapshot = next;
  if (notificationScheduled) return;
  notificationScheduled = true;
  const notify = () => {
    notificationScheduled = false;
    for (const listener of listeners) listener();
  };
  // Render state is a control-plane receipt, not a paint callback. A native
  // child viewport or a minimized WebView may stop rAF while Ready/Cancel must
  // still reach React. Coalesce synchronous events without waiting for paint.
  queueMicrotask(notify);
}

export function ensureApplicationRenderSubscription(): Promise<void> {
  if (subscription) return subscription;
  subscription = (async () => {
    const unlisten = await subscribeApplicationRenderState((next) => {
      eventRevision += 1;
      publish(next);
    });
    try {
      const revisionBeforeSnapshot = eventRevision;
      const initialSnapshot = await getApplicationRenderSnapshot();
      snapshotReady = true;
      // Each Application event is a complete snapshot. If one arrived while
      // this request was in flight, it is newer than the requested snapshot.
      if (eventRevision === revisionBeforeSnapshot) publish(initialSnapshot);
    } catch (error) {
      unlisten();
      throw error;
    }
  })().catch((error) => {
    snapshotReady = false;
    subscription = null;
    throw error;
  });
  return subscription;
}

export function subscribeApplicationRender(listener: () => void): () => void {
  listeners.add(listener);
  void ensureApplicationRenderSubscription().catch(() => undefined);
  return () => listeners.delete(listener);
}

export function getApplicationRenderState(): ApplicationRenderSnapshot {
  return snapshot;
}

export function isApplicationRenderStateReady(): boolean {
  return snapshotReady;
}

export function findRenderOperationForSource(
  sourcePath: string,
  source: ApplicationRenderSnapshot = snapshot,
  settings?: Pick<RenderOperationView, "displayMode" | "buildMode">,
): RenderOperationView | null {
  const normalized = windowsPathKey(sourcePath);
  return [...source.operations]
    .reverse()
    .find((operation) => windowsPathKey(operation.source.path) === normalized
      && (!settings || (operation.displayMode === settings.displayMode && operation.buildMode === settings.buildMode))) ?? null;
}

export type {
  ApplicationRenderSnapshot,
  RenderOperationView,
} from "../platform/applicationRender";
