import {
  libraryIndexQuery,
  libraryIndexStatus,
  libraryTaskSubmit,
  type LibraryIndexEntry,
  type LibraryIndexField,
  type LibraryIndexQuery,
  type LibraryIndexQueryResult,
  type LibraryIndexStatus,
} from "../platform/files";
import { subscribeLibraryTask } from "./libraryTasks";

/**
 * Thin index client (D.2, design 2.2.2(C) / 2.4.3).
 *
 * Wraps the read-only `library_index_query` / `library_index_status` commands and
 * exposes the readiness subscription the views use to fill in the
 * "index building → ready" transition (5.8.3.1). The index itself is built and
 * persisted by the Rust `library_index` module through the single serial queue;
 * this client never derives or caches index contents.
 *
 * Named `libraryIndexClient.ts` to mirror the Rust `library_index.rs` carrier.
 */

export type {
  LibraryIndexEntry,
  LibraryIndexField,
  LibraryIndexQuery,
  LibraryIndexQueryResult,
  LibraryIndexStatus,
};

export type UnsubscribeIndexStatus = () => void;

/** Read-only index lookup; returns the matching entry ids plus readiness. */
export function queryIndex(query: LibraryIndexQuery): Promise<LibraryIndexQueryResult> {
  return libraryIndexQuery(query);
}

/** Current index readiness snapshot. */
export function indexStatus(): Promise<LibraryIndexStatus> {
  return libraryIndexStatus();
}

/**
 * Submits an incremental `index` build from the current indexable entry set. The
 * Rust side diffs it by `entryId` against the persisted index, so passing the
 * whole set is safe and never forces a full rebuild.
 *
 * @returns The submitted task id; readiness is refreshed on its terminal event.
 */
export async function submitIndexBuild(entries: LibraryIndexEntry[]): Promise<string> {
  const { taskId } = await libraryTaskSubmit("index", { entries });
  return taskId;
}

/**
 * Subscribes to index readiness: emits the current status immediately, then
 * re-queries whenever an `index` task finishes so the caller can fill in the
 * already-indexed partial results. Returns an unsubscribe function.
 */
export async function subscribeIndexStatus(
  listener: (status: LibraryIndexStatus) => void,
): Promise<UnsubscribeIndexStatus> {
  listener(await indexStatus());
  return subscribeLibraryTask({
    onFinished: (payload) => {
      if (payload.kind === "index") void indexStatus().then(listener);
    },
  });
}