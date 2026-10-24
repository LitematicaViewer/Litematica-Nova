import {
  libraryRecordViews,
  type LibraryRecordView,
  type LibraryState,
} from "../../../../../src/business/facade";
import { minecraftVersionLabel } from "../../../../../src/services/minecraftVersions";

/**
 * Per-entry aggregation of the per-replica `LibraryRecordView` read model (D.3–D.11).
 *
 * The store still exposes one view per replica; every D-phase view shows *entries*
 * (one card / row / detail per logical projection) with a replica (重数) badge, so
 * this module folds replicas back onto their entry once and every component shares
 * the result. It is a pure projection over the A-phase transitional read model.
 */

export type EntryViewStatus = "ok" | "missing" | "parse_error";

export interface LibraryEntryView {
  entryId: string;
  displayName: string;
  fileName: string;
  author: string;
  description: string;
  tags: string[];
  categoryIds: string[];
  status: EntryViewStatus;
  totalBlocks: number;
  totalVolume: number;
  regionCount: number;
  enclosingSize: { x: number; y: number; z: number };
  minecraftDataVersion: number;
  versionLabel: string;
  fileSize: number;
  mtime: number;
  checksum: string | null;
  sortIndex: number;
  lastAnalyzedAt: number;
  lastError: string;
  previewUpdatedAt: number;
  /** A representative replica used for preview loading and activation. */
  primary: LibraryRecordView;
  /** Every non-trashed replica belonging to this entry. */
  replicas: LibraryRecordView[];
  /** Non-trashed replica count (the 重数). */
  replicaCount: number;
  /** Replicas whose file is currently present. */
  activeReplicaCount: number;
}

function replicaOrder(view: LibraryRecordView): number {
  return view.status === "ok" ? 0 : view.status === "missing" ? 1 : 2;
}

export function buildEntryViews(state: LibraryState): LibraryEntryView[] {
  const byEntry = new Map<string, LibraryRecordView[]>();
  for (const view of libraryRecordViews(state)) {
    const list = byEntry.get(view.entryId);
    if (list) list.push(view);
    else byEntry.set(view.entryId, [view]);
  }
  const views: LibraryEntryView[] = [];
  for (const [entryId, replicas] of byEntry) {
    const ordered = [...replicas].sort((left, right) => replicaOrder(left) - replicaOrder(right) || left.fileName.localeCompare(right.fileName));
    const primary = ordered[0];
    views.push({
      entryId,
      displayName: primary.displayName,
      fileName: primary.fileName,
      author: primary.author,
      description: primary.description,
      tags: [...primary.tags],
      categoryIds: [...primary.categoryIds],
      status: primary.status,
      totalBlocks: primary.totalBlocks,
      totalVolume: primary.totalVolume,
      regionCount: primary.regionCount,
      enclosingSize: { ...primary.enclosingSize },
      minecraftDataVersion: primary.minecraftDataVersion,
      versionLabel: primary.minecraftDataVersion > 0 ? minecraftVersionLabel(primary.minecraftDataVersion) : "",
      fileSize: primary.fileSize,
      mtime: primary.mtime,
      checksum: primary.checksum,
      sortIndex: primary.sort_index,
      lastAnalyzedAt: primary.lastAnalyzedAt,
      lastError: primary.lastError,
      previewUpdatedAt: primary.previewUpdatedAt ?? 0,
      primary,
      replicas: ordered,
      replicaCount: ordered.length,
      activeReplicaCount: ordered.filter((replica) => replica.status === "ok").length,
    });
  }
  return views;
}

// ---------------------------------------------------------------------------
// Grouping / stacking (D.9, design 2.4.2)
// ---------------------------------------------------------------------------

export type GroupingDimension = "none" | "author" | "version" | "size";
export type GroupingMode = "group" | "stack";

export interface LibraryGroup {
  key: string;
  label: string;
  entries: LibraryEntryView[];
}

export function groupingField(entry: LibraryEntryView, dimension: GroupingDimension): { key: string; label: string } {
  if (dimension === "author") {
    const author = entry.author.trim();
    return author ? { key: `author:${author.toLowerCase()}`, label: author } : { key: "author:__none", label: "未填写作者" };
  }
  if (dimension === "version") {
    const version = entry.versionLabel;
    return version ? { key: `version:${version}`, label: version } : { key: "version:__none", label: "未知版本" };
  }
  if (dimension === "size") {
    const size = `${entry.enclosingSize.x}×${entry.enclosingSize.y}×${entry.enclosingSize.z}`;
    return { key: `size:${size}`, label: size };
  }
  return { key: "all", label: "全部" };
}

export function groupEntryViews(entries: LibraryEntryView[], dimension: GroupingDimension): LibraryGroup[] {
  if (dimension === "none") return [{ key: "all", label: "全部", entries }];
  const groups = new Map<string, LibraryGroup>();
  for (const entry of entries) {
    const { key, label } = groupingField(entry, dimension);
    const group = groups.get(key) ?? { key, label, entries: [] };
    group.entries.push(entry);
    groups.set(key, group);
  }
  return [...groups.values()].sort((left, right) => left.label.localeCompare(right.label, "zh-Hans-CN"));
}

// ---------------------------------------------------------------------------
// Configurable detail columns (D.6 / D.7, spec 5.8.1.3)
// ---------------------------------------------------------------------------

export interface LibraryColumn {
  id: string;
  label: string;
  value: (entry: LibraryEntryView) => string;
}

export function formatSize(numBytes: number): string {
  let value = Math.max(0, numBytes);
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return index === 0 ? `${Math.floor(value)} ${units[index]}` : `${value.toFixed(1)} ${units[index]}`;
}

export function formatEntryDate(timestamp: number): string {
  if (!timestamp) return "—";
  const date = new Date(timestamp);
  const pad = (value: number) => value.toString().padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function entryStatusLabel(entry: LibraryEntryView): string {
  if (entry.status === "missing") return "文件丢失";
  if (entry.status === "parse_error") return "解析失败";
  return "正常";
}

export const LIBRARY_COLUMNS: LibraryColumn[] = [
  { id: "author", label: "作者", value: (entry) => entry.author || "—" },
  { id: "version", label: "版本", value: (entry) => entry.versionLabel || "—" },
  { id: "blocks", label: "方块数", value: (entry) => entry.totalBlocks.toLocaleString() },
  { id: "size", label: "尺寸", value: (entry) => `${entry.enclosingSize.x}×${entry.enclosingSize.y}×${entry.enclosingSize.z}` },
  { id: "replicas", label: "重数", value: (entry) => String(entry.replicaCount) },
  { id: "status", label: "状态", value: (entry) => entryStatusLabel(entry) },
  { id: "analyzed", label: "最近分析", value: (entry) => formatEntryDate(entry.lastAnalyzedAt) },
  { id: "fileName", label: "文件名", value: (entry) => entry.fileName },
  { id: "path", label: "文件路径", value: (entry) => entry.primary.path },
];

export const DEFAULT_LIBRARY_COLUMNS = ["author", "version", "blocks", "replicas", "status"];