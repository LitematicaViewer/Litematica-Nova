import { listenEvent } from "../platform/events";
import {
  libraryTaskCancel,
  libraryTaskStatus,
  libraryTaskSubmit,
  type LibraryTaskKind,
  type TaskDescriptor,
  type TaskQueueSnapshot,
} from "../platform/files";

/**
 * Background-task client (C.1, design 2.1.3 / 2.2.2(B) / 2.2.3).
 *
 * Thin wrapper over `library_task_submit/cancel/status` plus the three
 * `projection-library-task-*` events. All library I/O tasks run in one Rust-side
 * serial queue; this client only submits, observes and cancels them. `submitAndAwait`
 * resolves the terminal event for a single task so the store can commit once.
 */

export const libraryTaskProgressEvent = "projection-library-task-progress";
export const libraryTaskFinishedEvent = "projection-library-task-finished";
export const libraryTaskQueueChangedEvent = "projection-library-task-queue-changed";

export interface TaskFailure {
  path: string;
  reason: string;
}

export interface TaskProgressPayload {
  taskId: string;
  kind: LibraryTaskKind;
  phase: string;
  processed: number;
  total: number;
  currentPath?: string;
  message?: string;
}

export type TaskFinishedStatus = "completed" | "cancelled" | "failed";

export interface TaskFinishedPayload {
  taskId: string;
  kind: LibraryTaskKind;
  status: TaskFinishedStatus;
  /** Per-kind summary (e.g. scan's `files`, batch counts). */
  summary: Record<string, unknown>;
  failures: TaskFailure[];
}

export interface LibraryTaskSubscriptions {
  onProgress?: (payload: TaskProgressPayload) => void;
  onFinished?: (payload: TaskFinishedPayload) => void;
  onQueueChanged?: (snapshot: TaskQueueSnapshot) => void;
}

export type UnsubscribeLibraryTasks = () => void;

// --- module-level event fan-out -------------------------------------------------

interface Waiters {
  progress: Set<(payload: TaskProgressPayload) => void>;
  finished: Set<(payload: TaskFinishedPayload) => void>;
  queue: Set<(snapshot: TaskQueueSnapshot) => void>;
}

const waiters: Waiters = { progress: new Set(), finished: new Set(), queue: new Set() };
// Cache keeps the very last terminal events so `submitAndAwait` cannot lose a
// task that finishes in the (small) window between submit and registration.
const RECENT_FINISHED_CACHE = 32;
const recentFinished = new Map<string, TaskFinishedPayload>();
let listenerHandles: Array<() => void> = [];
let started = false;

function rememberFinished(payload: TaskFinishedPayload): void {
  recentFinished.set(payload.taskId, payload);
  while (recentFinished.size > RECENT_FINISHED_CACHE) {
    const oldest = recentFinished.keys().next().value;
    if (oldest === undefined) break;
    recentFinished.delete(oldest);
  }
}

async function ensureStarted(): Promise<void> {
  if (started) return;
  started = true;
  listenerHandles = [
    await listenEvent<TaskProgressPayload>(libraryTaskProgressEvent, (event) => {
      for (const listener of waiters.progress) listener(event.payload);
    }),
    await listenEvent<TaskFinishedPayload>(libraryTaskFinishedEvent, (event) => {
      rememberFinished(event.payload);
      for (const listener of waiters.finished) listener(event.payload);
    }),
    await listenEvent<TaskQueueSnapshot>(libraryTaskQueueChangedEvent, (event) => {
      for (const listener of waiters.queue) listener(event.payload);
    }),
  ];
}

/** Subscribes to background-task events; returns an unsubscribe function. */
export async function subscribeLibraryTask(subscriptions: LibraryTaskSubscriptions): Promise<UnsubscribeLibraryTasks> {
  await ensureStarted();
  if (subscriptions.onProgress) waiters.progress.add(subscriptions.onProgress);
  if (subscriptions.onFinished) waiters.finished.add(subscriptions.onFinished);
  if (subscriptions.onQueueChanged) waiters.queue.add(subscriptions.onQueueChanged);
  return () => {
    if (subscriptions.onProgress) waiters.progress.delete(subscriptions.onProgress);
    if (subscriptions.onFinished) waiters.finished.delete(subscriptions.onFinished);
    if (subscriptions.onQueueChanged) waiters.queue.delete(subscriptions.onQueueChanged);
  };
}

/** Submits a task and returns its id. */
export async function submitLibraryTask(kind: LibraryTaskKind, params: unknown): Promise<string> {
  await ensureStarted();
  const { taskId } = await libraryTaskSubmit(kind, params);
  return taskId;
}

/** Requests cooperative cancellation of a task. */
export async function cancelLibraryTask(taskId: string): Promise<boolean> {
  const { cancelled } = await libraryTaskCancel(taskId);
  return cancelled;
}

/** Current queue snapshot for the task centre. */
export function queryLibraryTaskQueue(): Promise<TaskQueueSnapshot> {
  return libraryTaskStatus();
}

export interface SubmitAndAwaitOptions {
  onProgress?: (payload: TaskProgressPayload) => void;
}

/**
 * Submits one task and resolves its terminal event. Cancellation/failure are
 * surfaced through `status` and `failures` rather than rejected, so the caller
 * can still apply any completed partial results.
 */
export async function submitAndAwait(
  kind: LibraryTaskKind,
  params: unknown,
  options: SubmitAndAwaitOptions = {},
): Promise<TaskFinishedPayload> {
  await ensureStarted();
  const { taskId } = await libraryTaskSubmit(kind, params);
  const cached = recentFinished.get(taskId);
  if (cached) return cached;

  return new Promise<TaskFinishedPayload>((resolve) => {
    const onProgress = options.onProgress;
    const finish = (payload: TaskFinishedPayload) => {
      if (payload.taskId !== taskId) return;
      waiters.finished.delete(finish);
      if (onProgress) waiters.progress.delete(forwardProgress);
      recentFinished.delete(taskId);
      resolve(payload);
    };
    const forwardProgress = (payload: TaskProgressPayload) => {
      if (payload.taskId === taskId) onProgress?.(payload);
    };
    waiters.finished.add(finish);
    if (onProgress) waiters.progress.add(forwardProgress);
  });
}

/** Test/host reset: clears cached terminal events and listener handles. */
export function resetLibraryTaskClientForTests(): void {
  for (const handle of listenerHandles) handle();
  listenerHandles = [];
  started = false;
  recentFinished.clear();
  waiters.progress.clear();
  waiters.finished.clear();
  waiters.queue.clear();
}

/**
 * Resolves the terminal event for a task id submitted elsewhere (e.g. the typed
 * `library_export_zip` command). Uses the same finished cache as `submitAndAwait`
 * so a task that finishes in the submit/register window is never lost.
 */
export async function awaitLibraryTask(taskId: string): Promise<TaskFinishedPayload> {
  await ensureStarted();
  const cached = recentFinished.get(taskId);
  if (cached) return cached;
  return new Promise<TaskFinishedPayload>((resolve) => {
    const finish = (payload: TaskFinishedPayload) => {
      if (payload.taskId !== taskId) return;
      waiters.finished.delete(finish);
      recentFinished.delete(taskId);
      resolve(payload);
    };
    waiters.finished.add(finish);
  });
}

export type { LibraryTaskKind, TaskDescriptor, TaskQueueSnapshot };