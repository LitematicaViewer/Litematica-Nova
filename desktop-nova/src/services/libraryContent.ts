import { windowsPathKey } from "./filePathIdentity";
import type {
  EntryIdRedirect,
  EntryStatus,
  LibraryEntry,
  LibraryState,
  Replica,
} from "./libraryStore";

/**
 * Identity & content-change kernel (C.3, design 2.1.4(2)).
 *
 * Everything here is a **pure, deterministic** transformation over `LibraryState`:
 * content sync/diverge, automatic merge, retirement/revival, the ID redirect
 * chain and dirty-replica extraction. Physical file work (copy / write / hash)
 * is orchestrated by `libraryStore.ts`; this module only reshapes library data,
 * so it stays testable without the IPC host and cannot form a runtime cycle with
 * `libraryStore` (only type-only imports are erased).
 */

// Local copies of the tiny path helpers so this module keeps zero runtime
// dependency on `libraryStore` (which imports this module at runtime).
function fileNameFromPath(filePath: string): string {
  return filePath.split(/[\\/]/).pop() || "";
}

function parentDirectory(path: string): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const lastSeparatorIndex = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  if (lastSeparatorIndex < 0) return "";
  const parent = normalized.slice(0, lastSeparatorIndex);
  return /^(?:\\\\\?\\)?[a-z]:$/i.test(parent) ? `${parent}${normalized[lastSeparatorIndex]}` : parent;
}

function cloneEntries(entries: LibraryEntry[]): LibraryEntry[] {
  return entries.map((entry) => ({ ...entry, categoryIds: [...entry.categoryIds], tags: [...entry.tags], preview: { ...entry.preview }, stats: { ...entry.stats, regionNames: entry.stats.regionNames ? [...entry.stats.regionNames] : undefined } }));
}

function cloneReplicas(replicas: Replica[]): Replica[] {
  return replicas.map((replica) => ({ ...replica }));
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

/** Active-entry index over content fingerprints: covers `retired` (for revival), excludes `merged`. */
export function buildChecksumIndex(state: LibraryState): Map<string, string> {
  const index = new Map<string, string>();
  const better = (candidate: LibraryEntry, incumbent: LibraryEntry): boolean => {
    if (candidate.status !== incumbent.status) return candidate.status === "active";
    if (candidate.createdAt !== incumbent.createdAt) return candidate.createdAt < incumbent.createdAt;
    return candidate.id < incumbent.id;
  };
  const byId = new Map(state.entries.map((entry) => [entry.id, entry]));
  for (const entry of state.entries) {
    if (entry.status === "merged" || !entry.checksum) continue;
    const incumbentId = index.get(entry.checksum);
    if (!incumbentId) { index.set(entry.checksum, entry.id); continue; }
    const incumbent = byId.get(incumbentId);
    if (incumbent && better(entry, incumbent)) index.set(entry.checksum, entry.id);
  }
  return index;
}

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

function hasActiveReplica(state: LibraryState, entryId: string): boolean {
  return state.replicas.some((replica) => replica.entryId === entryId && replica.status === "active");
}

function retireOrphans(entries: LibraryEntry[], replicas: Replica[], candidates: Set<string>): void {
  const byId = new Map(entries.map((entry) => [entry.id, entry]));
  for (const id of candidates) {
    const entry = byId.get(id);
    if (entry && entry.status === "active" && !replicas.some((replica) => replica.entryId === id && replica.status === "active")) {
      entry.status = "retired" as EntryStatus;
    }
  }
}

/**
 * Finds (or creates) the entry that should own a replica whose current checksum
 * differs from its entry. Mirrors the "diverge" path (5.4.1.4) and the dirty-data
 * rule (裁决 9): migrate the replica to the existing entry for that checksum, or
 * create a fresh deterministic one.
 */
function relocateReplicaToChecksumEntry(
  state: LibraryState,
  entries: LibraryEntry[],
  index: Map<string, string>,
  replica: Replica,
): string | null {
  const checksum = replica.checksum;
  if (!checksum) return null;
  let targetId = index.get(checksum);
  if (!targetId) {
    const source = entries.find((entry) => entry.id === replica.entryId);
    let candidate = `entry:content:${checksum.slice(0, 16)}`;
    let suffix = 0;
    while (entries.some((entry) => entry.id === candidate)) { suffix += 1; candidate = `entry:content:${checksum.slice(0, 16)}:${suffix}`; }
    targetId = candidate;
    const created: LibraryEntry = {
      id: targetId,
      createdAt: source ? source.createdAt : 0,
      checksum,
      status: "active",
      categoryIds: [],
      displayName: replica.fileName || fileNameFromPath(replica.path),
      internalName: undefined,
      author: "",
      description: "",
      tags: [],
      preview: {},
      stats: { totalBlocks: 0, totalVolume: 0, regionCount: 0, enclosingSize: { x: 0, y: 0, z: 0 }, minecraftDataVersion: 0 },
      sortIndex: entries.length,
      lastAnalyzedAt: replica.lastAnalyzedAt,
      lastError: replica.lastError,
    };
    entries.push(created);
    index.set(checksum, targetId);
  } else {
    const target = entries.find((entry) => entry.id === targetId);
    if (target && target.status === "retired") target.status = "active";
  }
  return targetId;
}

/**
 * Dirty-data extraction (裁决 9): a replica whose active checksum differs from
 * its entry's checksum is migrated out to the entry that owns its checksum
 * (existing or newly created); the emptied source entry is retired.
 */
export function reconcileDirtyReplicas(state: LibraryState): { state: LibraryState; moved: DirtyReplicaMove[] } {
  const entries = cloneEntries(state.entries);
  const replicas = cloneReplicas(state.replicas);
  const index = buildChecksumIndex({ ...state, entries, replicas });
  const moved: DirtyReplicaMove[] = [];
  const orphaned = new Set<string>();
  for (const replica of replicas) {
    if (replica.status !== "active" || !replica.checksum) continue;
    const entry = entries.find((item) => item.id === replica.entryId);
    if (!entry || !entry.checksum || entry.checksum === replica.checksum) continue;
    const targetId = relocateReplicaToChecksumEntry({ ...state, entries, replicas }, entries, index, replica);
    if (!targetId || targetId === entry.id) continue;
    moved.push({ entryId: entry.id, replicaId: replica.id, fromChecksum: entry.checksum, toChecksum: replica.checksum, toEntryId: targetId });
    replica.entryId = targetId;
    orphaned.add(entry.id);
  }
  retireOrphans(entries, replicas, orphaned);
  return { state: { ...state, entries, replicas }, moved };
}

/** Automatic merge of `discardId` into `keepId` (5.3.1.7): metadata follows keep, categories union, replicas reassigned. */
export function mergeEntries(state: LibraryState, keepId: string, discardId: string, at: number): LibraryState {
  if (keepId === discardId) return state;
  const entries = cloneEntries(state.entries);
  const replicas = cloneReplicas(state.replicas);
  const keep = entries.find((entry) => entry.id === keepId);
  const discard = entries.find((entry) => entry.id === discardId);
  if (!keep || !discard || keep.status === "merged" || discard.status === "merged") return state;
  keep.categoryIds = [...new Set([...keep.categoryIds, ...discard.categoryIds])];
  for (const replica of replicas) if (replica.entryId === discardId) replica.entryId = keepId;
  discard.status = "merged";
  const redirects: EntryIdRedirect[] = [...state.redirects, { fromId: discardId, toId: keepId, at }];
  return { ...state, entries, replicas, redirects };
}

/** Deterministic keeper pick: earlier `createdAt` wins; ties broken by smaller id (裁决 8). */
function pickKeeper(a: LibraryEntry, b: LibraryEntry): LibraryEntry {
  if (a.createdAt !== b.createdAt) return a.createdAt < b.createdAt ? a : b;
  return a.id < b.id ? a : b;
}

/** Rebuilds the checksum → entry index after a merge so it points at the keeper. */
export function autoMergeByChecksum(state: LibraryState, at: number): { state: LibraryState; merges: EntryMergeRecord[] } {
  const groups = new Map<string, LibraryEntry[]>();
  for (const entry of state.entries) {
    if (entry.status === "merged" || !entry.checksum) continue;
    const list = groups.get(entry.checksum) || [];
    list.push(entry);
    groups.set(entry.checksum, list);
  }
  let next = state;
  const merges: EntryMergeRecord[] = [];
  for (const group of groups.values()) {
    if (group.length < 2) continue;
    const [keeper] = [...group].sort((left, right) => (left.createdAt - right.createdAt) || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0));
    for (const other of group) {
      if (other.id === keeper.id) continue;
      // pickKeeper is the canonical tie rule; keep the earlier/lower id as keeper.
      const selected = pickKeeper(keeper, other);
      const discard = selected.id === keeper.id ? other : keeper;
      next = mergeEntries(next, selected.id, discard.id, at);
      merges.push({ fromId: discard.id, toId: selected.id });
    }
  }
  return { state: next, merges };
}

/**
 * "Diverged" content change (5.4.1.4): a single replica with a new checksum is
 * moved to the entry owning that checksum (existing or new); the source entry
 * keeps its other replicas and is retired if none remain active.
 */
export function applyContentDiverge(state: LibraryState, replicaId: string, at: number): RelocateResult | null {
  const entries = cloneEntries(state.entries);
  const replicas = cloneReplicas(state.replicas);
  const index = buildChecksumIndex({ ...state, entries, replicas });
  const replica = replicas.find((item) => item.id === replicaId);
  if (!replica || !replica.checksum) return null;
  const source = entries.find((entry) => entry.id === replica.entryId);
  if (!source || source.checksum === replica.checksum) return null;
  const targetId = relocateReplicaToChecksumEntry({ ...state, entries, replicas }, entries, index, replica);
  if (!targetId) return null;
  replica.entryId = targetId;
  const orphaned = new Set<string>([source.id]);
  retireOrphans(entries, replicas, orphaned);
  return { state: { ...state, entries, replicas }, toEntryId: targetId, sourceEntryRetired: entries.find((entry) => entry.id === source.id)?.status === "retired" };
}

/**
 * Applies a completed content sync (5.4.1.3): every active replica of the entry
 * now carries the source bytes, so all their fingerprints and the entry
 * fingerprint collapse onto `newChecksum` — the entry ID is untouched.
 */
export function applyContentSync(state: LibraryState, entryId: string, newChecksum: string): LibraryState {
  const entries = cloneEntries(state.entries);
  const replicas = cloneReplicas(state.replicas);
  const entry = entries.find((item) => item.id === entryId);
  if (!entry) return state;
  entry.checksum = newChecksum;
  if (entry.status === "retired") entry.status = "active";
  for (const replica of replicas) {
    if (replica.entryId === entryId && replica.status === "active") replica.checksum = newChecksum;
  }
  return { ...state, entries, replicas };
}

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