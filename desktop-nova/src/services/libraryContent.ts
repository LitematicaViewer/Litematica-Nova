import { libraryContentCommand } from "../platform/files";
import { windowsPathKey } from "./filePathIdentity";
import type {
  LibraryState,
  Replica,
} from "./libraryStore";

/** Content ownership is decided by Rust; selectors below only present acknowledged state. */
export function normalizeLibraryContent(state: LibraryState): Promise<LibraryState> { return libraryContentCommand({ action: "normalize", state, at: Date.now() }); }

function parentDirectory(path: string): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const lastSeparatorIndex = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  if (lastSeparatorIndex < 0) return "";
  const parent = normalized.slice(0, lastSeparatorIndex);
  return /^(?:\\\\\?\\)?[a-z]:$/i.test(parent) ? `${parent}${normalized[lastSeparatorIndex]}` : parent;
}

export interface DirtyReplicaMove {
  entryId: string;
  replicaId: string;
  fromChecksum: string | null;
  toChecksum: string;
  toEntryId: string;
}

export interface EntryMergeRecord {
  fromId: string;
  toId: string;
}

export interface RelocateResult {
  state: LibraryState;
  toEntryId: string;
  sourceEntryRetired: boolean;
}
export async function buildChecksumIndex(state: LibraryState): Promise<Map<string, string>> { return new Map(Object.entries(await libraryContentCommand<Record<string, string>>({ action: "index", state }))); }

/** Follows the append-only ID redirect chain to the surviving entry id (5.3.1.8 / 6.7), with cycle guard. */
export function resolveEntryId(state: LibraryState, id: string): string {
  const redirects = state.redirects || [];
  const seen = new Set<string>();
  let current = id;
  while (!seen.has(current)) {
    seen.add(current);
    const redirect = redirects.find((item) => item.fromId === current);
    if (!redirect) break;
    current = redirect.toId;
  }
  return current;
}

export function entryActiveReplicas(state: LibraryState, entryId: string): Replica[] {
  return state.replicas.filter((replica) => replica.entryId === entryId && replica.status === "active");
}

export function reconcileDirtyReplicas(state: LibraryState): Promise<{ state: LibraryState; moved: DirtyReplicaMove[] }> { return libraryContentCommand({ action: "reconcile", state }); }

export function mergeEntries(state: LibraryState, keepId: string, discardId: string, at: number): Promise<LibraryState> { return libraryContentCommand({ action: "merge", state, keepId, discardId, at }); }

export function autoMergeByChecksum(state: LibraryState, at: number): Promise<{ state: LibraryState; merges: EntryMergeRecord[] }> { return libraryContentCommand({ action: "mergeAll", state, at }); }

export function applyContentDiverge(state: LibraryState, replicaId: string, at: number): Promise<RelocateResult | null> { return libraryContentCommand({ action: "diverge", state, replicaId, at }); }

export function applyContentSync(state: LibraryState, entryId: string, checksum: string, replicaIds: string[]): Promise<LibraryState> { return libraryContentCommand({ action: "sync", state, entryId, checksum, replicaIds }); }

export interface ReplicaFolderDistribution {
  folderPath: string;
  count: number;
  replicaIds: string[];
}

export interface EntryRedundancyReport {
  entryId: string;
  displayName: string;
  totalActiveReplicas: number;
  folders: ReplicaFolderDistribution[];
  duplicateFolders: number;
}

export interface RedundancyReport {
  totalEntries: number;
  totalActiveReplicas: number;
  entries: EntryRedundancyReport[];
}

/**
 * Read-only redundancy audit (C.10, design 2.1.4(3)): per entry, group active
 * replicas by folder to expose distribution / duplicate counts. It never mutates
 * the library and is fully decoupled from any dispatch action.
 */
export function auditRedundancy(state: LibraryState): RedundancyReport {
  const byEntry = new Map<string, Replica[]>();
  for (const replica of state.replicas) {
    if (replica.status !== "active") continue;
    const list = byEntry.get(replica.entryId) || [];
    list.push(replica);
    byEntry.set(replica.entryId, list);
  }
  const entries: EntryRedundancyReport[] = [];
  for (const entry of state.entries) {
    if (entry.status === "merged") continue;
    const replicas = byEntry.get(entry.id) || [];
    const folderMap = new Map<string, ReplicaFolderDistribution>();
    for (const replica of replicas) {
      const folderPath = replica.folderPath || parentDirectory(replica.path);
      const key = windowsPathKey(folderPath);
      const bucket = folderMap.get(key) || { folderPath, count: 0, replicaIds: [] };
      bucket.count += 1;
      bucket.replicaIds.push(replica.id);
      folderMap.set(key, bucket);
    }
    const folders = [...folderMap.values()];
    entries.push({
      entryId: entry.id,
      displayName: entry.displayName,
      totalActiveReplicas: replicas.length,
      folders,
      duplicateFolders: folders.filter((folder) => folder.count > 1).length,
    });
  }
  return {
    totalEntries: entries.length,
    totalActiveReplicas: state.replicas.filter((replica) => replica.status === "active").length,
    entries,
  };
}