import { PreviewDraftDialog } from "../../../../components/PreviewDraftDialog";
import { readVisiblePreviews, cancelPreviewRead, generatePreviewImages, cancelPreviewGeneration, discardGeneratedPreview, type ProductPreviewImage } from "../../../../../src/platform/previewImage";
import { libraryContentCommand } from "../../../../../src/platform/files";
import { queryComposition } from "../../../../../src/platform/composition";
import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayPathText } from "../../../../../src/business/facade";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Box, Plus, Trash2, X } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Input } from "../../../../components/ui/input";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Dropdown } from "../../../../components/Dropdown";
import { Dialog } from "../../../../components/Dialog";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import {
  activateNonArchivedPath,
  activateProjectionRecord,
  addFavorite,
  addLocalLibraryFolder,
  addOrUpdateRecord,
  applyReplicaRenames,
  categoryDescendants,
  categoryPath,
  copyFileToDirectory,
  commandStockpileTask,
  getStockpileTask,
  getMinecraftTranslations,
  createLibraryCategory,
  deleteLibraryCategory,
  exportLibraryZip,
  fileNameFromPath,
  getFavorites,
  getPathInfo,
  getRecent,
  importFilesToBuiltinFolder,
  LibraryCategory,
  LibraryCategoryIcon,
  LibraryState,
  listLitematicFileEntriesInDirectory,
  loadLibrary,
  moveLibraryRecordsToCategory,
  moveReplicasToTrash,
  normalizeLibraryState,
  openFileParentDir,
  parentDirectory,
  queryLibraryIndex,
  refreshLibraryIndex,
  removeFavorite,
  reorderLibraryRecords,
  repositionReplica,
  saveLibrary,
  scanLibraryFolders,
  selectLitematicFile,
  subscribeIndexStatus,
  updateLibraryCategory,
  windowsPathKey,
  type RepositionOutcome,
} from "../../../../../src/business/facade";
import { submitAndAwait } from "../../../../../src/services/libraryTasks";
import { emitEvent, listenEvent } from "../../../../../src/platform/events";
import { projectionLibraryImportedEvent, projectionLibraryStateChangedEvent } from "../../../libraryEvents";
import { CategoryIconDialog } from "./categoryIcon";
import { LocalLibraryFoldersDialog, openLocalLibraryFoldersWithWindowBehavior } from "../../../local_library_folders";
import { RedenLibraryDialog, openRedenLibraryWithWindowBehavior } from "../../../reden_library";
import { LibraryToolbar, type LibraryView } from "./LibraryToolbar";
import { LibraryCategoryTree, type LibraryRecentItem } from "./LibraryCategoryTree";
import { LibraryGrid } from "./LibraryGrid";
import { LibraryList } from "./LibraryList";
import { LibraryDetail } from "./LibraryDetail";
import { LibraryGrouping } from "./LibraryGrouping";
import { LibraryColumnPicker } from "./LibraryColumnPicker";
import { LibraryTaskCenter } from "./LibraryTaskCenter";
import { SendProjectionDialog, type DispatchPlan } from "./sendDialog";
import { DropImportDialog, type DroppedItem, type DropImportPlan } from "./DropImportDialog";
import { ExportDialog, type ExportOptions } from "./ExportDialog";
import { StockpileSourcesDialog, type StockpileSourcesSubmission } from "../statistics/StockpileSourcesDialog";
import type { StockpileSourceInput, StockpileTaskView } from "../../../../../src/platform/applicationOperations";
import { TrashPanel } from "../../../local_library_folders/TrashPanel";
import { ReplicaNameEditor } from "./ReplicaNameEditor";
import { ensureLitematicFileName } from "./function";
import type { LibraryCardBinding } from "./EntryCard";
import {
  DEFAULT_LIBRARY_COLUMNS,
  buildEntryViews,
  type GroupingDimension,
  type GroupingMode,
  type LibraryEntryView,
} from "./libraryEntryView";
import "./library.css";

type LibraryStockpileDraft = {
  sources: StockpileSourceInput[];
  phase: "target" | "sources";
  targets: Awaited<ReturnType<typeof getStockpileTask>>["tasks"];
  targetTaskId: string;
  task?: StockpileTaskView;
  loading: boolean;
  error: string;
};

const samePath = (left: string, right: string) => windowsPathKey(left) === windowsPathKey(right);
const joinPath = (folder: string, name: string) => `${folder.replace(/[\\/]+$/, "")}/${name}`;
const uniquePaths = (paths: string[]) => [...new Map(paths.map((path) => [windowsPathKey(path), path])).values()];

/**
 * Library page orchestration container (D.3).
 *
 * Owns data loading, routing, event subscription and task submission only; the three
 * views, the toolbar, the left navigation, grouping and the task centre are separate
 * components. It keeps the two legacy library events (`projection-library-state-changed`
 * / `-record-imported`) and refreshes the background index on every library change.
 */
export function LibraryPage({ currentFile, setCurrentFile, setRoute, setCompositionId, onImportClipboard }: any) {
  useI18n();
  const [state, setState] = useState<LibraryState>(() => normalizeLibraryState(null));
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const [sortMode, setSortMode] = useState("manual");
  const [view, setView] = useState<LibraryView>("grid");
  const [groupDimension, setGroupDimension] = useState<GroupingDimension>("none");
  const [groupMode, setGroupMode] = useState<GroupingMode>("group");
  const [categoryFilter, setCategoryFilter] = useState("");
  const [untaggedOnly, setUntaggedOnly] = useState(false);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [recentOnly, setRecentOnly] = useState(false);
  const [selectedEntryIds, setSelectedEntryIds] = useState<string[]>([]);
  const [inspectedEntryId, setInspectedEntryId] = useState("");
  const [categoriesOpen, setCategoriesOpen] = useState(false);
  const [expandedCategories, setExpandedCategories] = useState<string[]>([]);
  const [columns, setColumns] = useState<string[]>(DEFAULT_LIBRARY_COLUMNS);
  const [indexMatchIds, setIndexMatchIds] = useState<Set<string> | null>(null);
  const [indexReady, setIndexReady] = useState(false);
  const [indexMatchCount, setIndexMatchCount] = useState(0);
  const [searchLoading, setSearchLoading] = useState(false);
  const [indexVersion, setIndexVersion] = useState(0);

  const [message, setMessage] = useState("");
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isOpening, setIsOpening] = useState(false);
  const [refreshProgress, setRefreshProgress] = useState(0);
  const [refreshTotal, setRefreshTotal] = useState(0);
  const [isCategorizing, setIsCategorizing] = useState(false);
  const [isDispatching, setIsDispatching] = useState(false);
  const [stockpileDraft, setStockpileDraft] = useState<LibraryStockpileDraft | null>(null);
  const [stockpileSaved, setStockpileSaved] = useState<StockpileTaskView | null>(null);
  const [stockpileWriting, setStockpileWriting] = useState(false);
  const stockpileEpoch = useRef(0);
  const stockpileReadPending = useRef(false);
  const stockpileWritePending = useRef(false);
  useEffect(() => () => { stockpileEpoch.current += 1; }, []);

  const [generatingPreviewId, setGeneratingPreviewId] = useState("");
  const [visiblePreviewIds, setVisiblePreviewIds] = useState<Set<string>>(new Set());
  const [previewErrors, setPreviewErrors] = useState<Record<string, string>>({});
  const [previewDrafts, setPreviewDrafts] = useState<Array<{ path: string; image: ProductPreviewImage }>>([]);
  const previewDraftQueue = useRef(previewDrafts);
  const updatePreviewDrafts = (next: typeof previewDrafts) => {
    previewDraftQueue.current = next; setPreviewDrafts(next);
  };
  const [previewProgress, setPreviewProgress] = useState({ completed: 0, total: 0 });
  const [previewPaused, setPreviewPaused] = useState(false);
  const generationRequest = useRef("");
  const libraryAlive = useRef(true);
  const previewReadSequence = useRef(0);
  const previewReadIdentity = useRef(new Map<string, string>());
  const handlePreviewVisibility = useCallback((id: string, visible: boolean) => setVisiblePreviewIds(current => {
    if (current.has(id) === visible) return current; const next = new Set(current); if (visible) next.add(id); else next.delete(id); return next;
  }), []);
  const [previewDataUrls, setPreviewDataUrls] = useState<Record<string, string>>({});
  const [previewLoadingPaths, setPreviewLoadingPaths] = useState<Record<string, boolean>>({});




  const [dragPaths, setDragPaths] = useState<string[]>([]);
  const [dropTarget, setDropTarget] = useState("");

  const [showLocalLibraryFoldersOverlay, setShowLocalLibraryFoldersOverlay] = useState(false);
  const [showRedenLibraryOverlay, setShowRedenLibraryOverlay] = useState(false);
  const [taskCenterOpen, setTaskCenterOpen] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  const [trashError, setTrashError] = useState("");
  const [trashBusy, setTrashBusy] = useState(false);

  const [dropItems, setDropItems] = useState<DroppedItem[] | null>(null);
  const [dropError, setDropError] = useState("");
  const [isDropBusy, setIsDropBusy] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportError, setExportError] = useState("");
  const [isExporting, setIsExporting] = useState(false);
  const [repositionConflict, setRepositionConflict] = useState<{ replicaId: string; occupiedByEntryId: string; newPath: string } | null>(null);
  const [repositionRename, setRepositionRename] = useState("");
  const [repositionError, setRepositionError] = useState("");
  const [isRepositioning, setIsRepositioning] = useState(false);
  const repositionPending = useRef(false);

  const [moveEntryIds, setMoveEntryIds] = useState<string[]>([]);
  const [moveCategoryId, setMoveCategoryId] = useState("");
  const [categoryError, setCategoryError] = useState("");
  const [categoryDialog, setCategoryDialog] = useState<{ kind: "create" | "rename" | "move" | "delete"; id: string; parentId: string | null } | null>(null);
  const [categoryNameDraft, setCategoryNameDraft] = useState("");
  const [categoryParentDraft, setCategoryParentDraft] = useState("");
  const [iconCategoryId, setIconCategoryId] = useState("");

  const [removeEntryIds, setRemoveEntryIds] = useState<string[]>([]);
  const [dispatchEntryId, setDispatchEntryId] = useState("");
  const [dispatchError, setDispatchError] = useState("");

  const libraryBusy = stockpileWriting || isRefreshing || isOpening || isCategorizing || isDispatching || trashBusy || isDropBusy || isExporting || isRepositioning;

  // --- data loading + event subscription ----------------------------------------
  const applyState = (next: LibraryState) => {
    setState(next);
    void refreshLibraryIndex(next).catch(() => undefined);
  };

  useEffect(() => {
    loadLibrary()
      .then((library) => applyState(normalizeLibraryState(library)))
      .catch((error) => setMessage(t("library.ux.readLibraryFailed", { value0: String(error) })));
  }, []);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let cancelled = false;
    subscribeIndexStatus((status) => {
      setIndexReady(status.ready);
      setIndexVersion((value) => value + 1);
    }).then((handle) => { if (cancelled) handle(); else unsubscribe = handle; }).catch(() => undefined);
    return () => { cancelled = true; unsubscribe?.(); };
  }, []);

  useEffect(() => {
    const unlistenPromise = listenEvent<{ path?: string }>(projectionLibraryImportedEvent, async (event) => {
      applyState(normalizeLibraryState(await loadLibrary()));
      if (event.payload?.path) setCurrentFile(event.payload.path);
    }).catch(() => undefined);
    return () => { unlistenPromise.then((unlisten) => unlisten?.()); };
  }, [setCurrentFile]);

  useEffect(() => {
    const unlistenPromise = listenEvent<{ path?: string; reason?: string }>(projectionLibraryStateChangedEvent, async (event) => {
      if (event.payload?.reason === "embedded-preview" && event.payload.path) {
        const path = event.payload.path;
        previewReadIdentity.current.delete(path);

        const forget = <T,>(values: Record<string, T>) => Object.fromEntries(Object.entries(values).filter(([candidate]) => !samePath(candidate, path)));
        setPreviewDataUrls(forget); setPreviewErrors(forget); setPreviewLoadingPaths(forget);
      }
      applyState(normalizeLibraryState(await loadLibrary()));
    }).catch(() => undefined);
    return () => { unlistenPromise.then((unlisten) => unlisten?.()); };
  }, []);

  // --- derived data --------------------------------------------------------------
  const entryViews = useMemo(() => buildEntryViews(state), [state]);
  const entryById = useMemo(() => new Map(entryViews.map((entry) => [entry.entryId, entry])), [entryViews]);
  const favoriteIds = useMemo(() => new Set(getFavorites(state)), [state]);
  const recentRefs = useMemo(() => getRecent(state), [state]);
  const recentEntryIds = useMemo(() => new Set(recentRefs.filter((ref) => ref.kind === "entry" && ref.entryId).map((ref) => ref.entryId as string)), [recentRefs]);
  const selectedIds = useMemo(() => new Set(selectedEntryIds), [selectedEntryIds]);
  const [queryView, setQueryView] = useState<{ entryIds: string[]; groups: { key: string; label: string; entryIds: string[] }[]; categoryCounts: Record<string, number>; untaggedCount: number; issueCount: number } | null>(null);
  const [queryBusy, setQueryBusy] = useState(false);
  useEffect(() => {
    let current = true;
    setQueryBusy(true);
    void libraryContentCommand<NonNullable<typeof queryView>>({ action: "query", state, query: {
      category: categoryFilter, untaggedOnly, favoritesOnly, recentOnly, status: statusFilter, sort: sortMode,
      matchIds: search.trim() && indexMatchIds ? [...indexMatchIds] : null, group: groupDimension,
      versionLabels: Object.fromEntries(entryViews.map(entry => [entry.minecraftDataVersion, entry.versionLabel])),
    } }).then(result => { if (current) setQueryView(result); })
      .catch(error => { if (current) setMessage(t("library.ux.readListFailed", { value0: String(error) })); })
      .finally(() => { if (current) setQueryBusy(false); });
    return () => { current = false; };
  }, [state, entryViews, categoryFilter, untaggedOnly, favoritesOnly, recentOnly, statusFilter, sortMode, search, indexMatchIds, groupDimension]);
  const categoryCounts = useMemo(() => new Map(Object.entries(queryView?.categoryCounts || {})), [queryView]);

  const favorites = useMemo(() => entryViews.filter((entry) => favoriteIds.has(entry.entryId)), [entryViews, favoriteIds]);
  const recentItems = useMemo<LibraryRecentItem[]>(() => recentRefs
    .map((ref): LibraryRecentItem | null => {
      if (ref.kind === "entry" && ref.entryId) {
        const entry = entryById.get(ref.entryId);
        if (entry) return { key: `entry:${ref.entryId}`, label: entry.displayName || entry.fileName, entryId: ref.entryId };
        return { key: `entry:${ref.entryId}`, label: ref.entryId, entryId: ref.entryId };
      }
      if (ref.kind === "path" && ref.path) return { key: `path:${windowsPathKey(ref.path)}`, label: fileNameFromPath(ref.path), path: ref.path };
      return null;
    })
    .filter((item): item is LibraryRecentItem => !!item), [recentRefs, entryById]);

  const filteredEntries = useMemo(() => (queryView?.entryIds || []).map(id => entryById.get(id)).filter((entry): entry is LibraryEntryView => !!entry), [queryView, entryById]);
  const grouped = groupDimension !== "none";
  const groups = useMemo(() => (queryView?.groups || []).map(group => ({ ...group,
    label: group.label || (groupDimension === "author" ? t("library.noAuthor") : groupDimension === "version" ? t("library.ux.unknownVersion") : t("stats.export.option.all")),
    entries: group.entryIds.map(id => entryById.get(id)).filter((entry): entry is LibraryEntryView => !!entry),
  })), [queryView, entryById, groupDimension]);

  const pageEntries = filteredEntries;

  const inspectedEntry = inspectedEntryId ? entryById.get(inspectedEntryId) || null : null;
  const selectedEntries = entryViews.filter((entry) => selectedIds.has(entry.entryId));
  const allPageSelected = pageEntries.length > 0 && pageEntries.every((entry) => selectedIds.has(entry.entryId));
  const somePageSelected = pageEntries.some((entry) => selectedIds.has(entry.entryId));
  const issueCount = queryView?.issueCount || 0;
  const dispatchEntry = dispatchEntryId ? entryById.get(dispatchEntryId) || null : null;
  const trashEntries = state.trash || [];
  const activeFilterLabel = untaggedOnly ? t("library.uncategorized") : favoritesOnly ? t("library.ux.favorites") : recentOnly ? t("library.ux.recents") : categoryFilter ? categoryPath(state, categoryFilter) || t("library.ux.allLibrary") : t("library.ux.allLibrary");

  // --- search through the background index ---------------------------------------
  useEffect(() => {
    const text = search.trim();
    if (!text) {
      setIndexMatchIds(null);
      setIndexMatchCount(0);
      setSearchLoading(false);
      return;
    }
    setSearchLoading(true);
    let cancelled = false;
    const handle = setTimeout(() => {
      queryLibraryIndex({ text })
        .then((result) => {
          if (cancelled) return;
          setIndexMatchIds(new Set(result.entryIds));
          setIndexMatchCount(result.entryIds.length);
          setIndexReady(result.ready);
        })
        .catch(() => { if (!cancelled) { setIndexMatchIds(new Set()); setIndexMatchCount(0); } })
        .finally(() => { if (!cancelled) setSearchLoading(false); });
    }, 200);
    return () => { cancelled = true; clearTimeout(handle); };
  }, [search, state.entries.length, indexVersion]);

  // DOM visibility is the only UI-side selection: Core owns identity, decode and caching.
  const previewTargets = useMemo(() => {
    const visible = pageEntries.filter(entry => visiblePreviewIds.has(entry.entryId) && entry.status !== "missing");
    if (inspectedEntry && inspectedEntry.status !== "missing" && !visible.includes(inspectedEntry)) visible.unshift(inspectedEntry);
    return visible;
  }, [pageEntries, inspectedEntry, visiblePreviewIds]);
  const previewTargetIdentity = JSON.stringify(previewTargets.map(entry => [entry.primary.path, entry.checksum, entry.fileSize, entry.mtime, entry.previewUpdatedAt]));
  useEffect(() => {
    let active = true;
    const identities = new Map(previewTargets.map(entry => [entry.primary.path, JSON.stringify([entry.checksum, entry.fileSize, entry.mtime, entry.previewUpdatedAt])]));
    const keep = new Set(identities.keys());
    const paths = [...keep].filter(path => previewReadIdentity.current.get(path) !== identities.get(path));
    for (const path of previewReadIdentity.current.keys()) if (!keep.has(path)) previewReadIdentity.current.delete(path);
    setPreviewDataUrls(current => Object.fromEntries(Object.entries(current).filter(([path]) => keep.has(path))));
    setPreviewErrors(current => Object.fromEntries(Object.entries(current).filter(([path]) => keep.has(path))));
    setPreviewLoadingPaths(Object.fromEntries(paths.map(path => [path, true])));
    const operationIds: string[] = [];
    for (let start = 0; start < paths.length; start += 32) {
      const targets = paths.slice(start, start + 32);
      const operationId = `visible-preview-${Date.now()}-${++previewReadSequence.current}`;
      operationIds.push(operationId);
      void readVisiblePreviews(targets, operationId).then(rows => {
        if (!active) return;
        for (const row of rows) if (!row.error && identities.has(row.sourcePath)) previewReadIdentity.current.set(row.sourcePath, identities.get(row.sourcePath)!);
        setPreviewDataUrls(current => ({ ...current, ...Object.fromEntries(rows.map(row => [row.sourcePath, row.image?.data_url || ""])) }));
        setPreviewErrors(current => ({ ...current, ...Object.fromEntries(rows.map(row => [row.sourcePath, row.error || ""])) }));
      }).catch(error => { if (active) setPreviewErrors(current => ({ ...current, ...Object.fromEntries(targets.map(path => [path, String(error)])) })); })
        .finally(() => { if (active) setPreviewLoadingPaths(current => ({ ...current, ...Object.fromEntries(targets.map(path => [path, false])) })); });
    }
    return () => { active = false; for (const operationId of operationIds) void cancelPreviewRead(operationId).catch(() => undefined); };
  }, [previewTargetIdentity]);
  useEffect(() => {
    libraryAlive.current = true;
    const events = listenEvent<{ requestId: string; completed: number; total: number }>("nova://thumbnail-progress", event => {
      if (event.payload.requestId === generationRequest.current) setPreviewProgress({ completed: event.payload.completed, total: event.payload.total });
    });
    return () => {
      libraryAlive.current = false;
      if (generationRequest.current) void cancelPreviewGeneration(generationRequest.current).catch(() => undefined);
      generationRequest.current = "";
      // The displayed first item belongs to PreviewDraftDialog (including any
      // pending save). This page owns only the drafts still waiting in the queue.
      queueMicrotask(() => {
        if (libraryAlive.current) return;
        for (const { image } of previewDraftQueue.current.slice(1)) if (image.draft_id) {
          void discardGeneratedPreview(image.draft_id).catch(reason => console.warn("[NOVA_PREVIEW] discard queued draft failed", reason));
        }
      });
      void events.then(unlisten => unlisten()).catch(() => undefined);
    };
  }, []);

  // --- actions -------------------------------------------------------------------
  const handleSelect = async () => {
    if (libraryBusy) return;
    setIsOpening(true); setMessage("");
    try {
      const res = await selectLitematicFile();
      if (res) {
        setCurrentFile(res);
        setInspectedEntryId("");
        applyState(await addOrUpdateRecord(await loadLibrary(), res));
      }
    } catch (error) {
      setMessage(t("library.ux.openFailed", { value0: String(error) }));
    } finally { setIsOpening(false); }
  };

  const handleRefresh = async () => {
    if (libraryBusy) return;
    setIsRefreshing(true); setRefreshProgress(0); setRefreshTotal(0); setMessage("");
    try {
      const outcome = await scanLibraryFolders(normalizeLibraryState(await loadLibrary()), {
        useCache: true,
        forceRecheck: false,
        onProgress: (progress) => { setRefreshProgress(progress.processed); setRefreshTotal(progress.total); },
      });
      applyState(outcome.state);
      if (outcome.status !== "completed") setMessage(t("library.ux.refreshIncomplete"));
      else if (outcome.failures.length) setMessage(t("library.ux.scanPartial", { value0: outcome.failures.length }));
    } catch (error) {
      setMessage(t("library.ux.refreshFailed", { value0: String(error) }));
    } finally { setIsRefreshing(false); }
  };

  /** Explicit "重新扫描全部" entry (B.2): defaults to the size+mtime cache; `forceRecheck` bypasses it. */
  const handleRescanAll = async (forceRecheck: boolean) => {
    if (libraryBusy) return;
    setIsRefreshing(true); setRefreshProgress(0); setRefreshTotal(0); setMessage("");
    try {
      const outcome = await scanLibraryFolders(normalizeLibraryState(await loadLibrary()), {
        useCache: !forceRecheck,
        forceRecheck,
        onProgress: (progress) => { setRefreshProgress(progress.processed); setRefreshTotal(progress.total); },
      });
      applyState(outcome.state);
      if (outcome.status !== "completed") setMessage(t("library.ux.rescanIncomplete"));
      else setMessage(t("library.ux.filesDone", { value0: forceRecheck ? t("library.ux.rechecked") : t("library.ux.rescanned"), value1: outcome.scannedCount }));
    } catch (error) {
      setMessage(t("library.ux.rescanFailed", { value0: String(error) }));
    } finally { setIsRefreshing(false); }
  };

  const handleActivate = async (entry: LibraryEntryView) => {
    console.info(`[NOVA_PROJECTION_ACTIVATION] path=${entry.primary.path} request started`);
    setCurrentFile(entry.primary.path);
    console.info(`[NOVA_PROJECTION_ACTIVATION] path=${entry.primary.path} request succeeded`);
    void loadLibrary()
      .then((library) => activateProjectionRecord(library, entry.primary.path))
      .then((next) => applyState(next))
      .catch((error) => setMessage(t("library.ux.recentFailed", { value0: String(error) })));
  };

  const handleActivatePath = async (path: string) => {
    setCurrentFile(path);
    try {
      applyState(await activateNonArchivedPath(await loadLibrary(), path));
    } catch (error) {
      setMessage(t("library.ux.activateFailed", { value0: String(error) }));
    }
  };

  const handleGeneratePreviews = async (entries: LibraryEntryView[]) => {
    if (generationRequest.current || !entries.length) return;
    const requestId = `library-generate-${Date.now()}-${++previewReadSequence.current}`;
    generationRequest.current = requestId; setMessage(""); setGeneratingPreviewId(entries[0].entryId); setPreviewPaused(false); setPreviewProgress({ completed: 0, total: entries.length });
    try {
      // Rust reads the persisted preview_mode, limits work and owns this batch.
      const output = await generatePreviewImages(entries.map(entry => entry.primary.path), requestId);
      if (!libraryAlive.current || generationRequest.current !== requestId) { for (const item of output.items) if (item.image?.draft_id) void discardGeneratedPreview(item.image.draft_id).catch(reason => console.warn("[NOVA_PREVIEW] discard late batch failed", reason)); return; }
      updatePreviewDrafts([...previewDraftQueue.current, ...output.items.filter(item => !!item.image).map(item => ({ path: item.file_path, image: item.image! }))]);
      const failures = output.items.filter(item => item.error).map(item => item.error).join("\n");
      if (failures) setMessage(failures); else if (output.cancelled) setMessage(t("library.previewCancelled"));
    } catch (error) { if (libraryAlive.current && generationRequest.current === requestId) setMessage(String(error)); }
    finally { if (generationRequest.current === requestId) { generationRequest.current = ""; if (libraryAlive.current) { setGeneratingPreviewId(""); setPreviewPaused(false); } } }
  };
  const handleGeneratePreview = (entry: LibraryEntryView) => handleGeneratePreviews([entry]);
  const handlePreviewControl = async (action: "pause" | "resume" | "cancel") => {
    if (!generationRequest.current) return;
    try { await cancelPreviewGeneration(generationRequest.current, action); setPreviewPaused(action === "pause"); }
    catch (error) { setMessage(String(error)); }
  };
  const handlePreviewSaved = (path: string, image: ProductPreviewImage) => {
    setPreviewDataUrls(current => ({ ...current, [path]: image.data_url })); setMessage(t("library.previewSaved"));
    void emitEvent(projectionLibraryStateChangedEvent, { reason: "embedded-preview", path }).catch(error => setMessage(String(error)));
  };

  const commitCategories = async (operation: (latest: LibraryState) => LibraryState) => {
    if (libraryBusy) return;
    setIsCategorizing(true); setCategoryError("");
    try {
      const next = operation(normalizeLibraryState(await loadLibrary()));
      await saveLibrary(next);
      applyState(next);
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error) { setCategoryError(String(error)); throw error; }
    finally { setIsCategorizing(false); setDragPaths([]); setDropTarget(""); }
  };

  const openCategoryDialog = (kind: "create" | "rename" | "move" | "delete", category?: LibraryCategory) => {
    setCategoryDialog({ kind, id: category?.id || "", parentId: kind === "create" ? category?.id || null : category?.parentId || null });
    setCategoryNameDraft(kind === "create" ? "" : category?.name || "");
    setCategoryParentDraft(kind === "create" ? category?.id || "" : category?.parentId || ""); setCategoryError("");
  };

  const confirmCategory = async () => {
    if (!categoryDialog) return;
    const dialog = categoryDialog;
    await commitCategories((latest) => dialog.kind === "create" ? createLibraryCategory(latest, categoryNameDraft, categoryParentDraft || null)
      : dialog.kind === "delete" ? deleteLibraryCategory(latest, dialog.id)
      : updateLibraryCategory(latest, dialog.id, dialog.kind === "rename" ? { name: categoryNameDraft } : { parentId: categoryParentDraft || null }));
    if (dialog.kind === "delete" && categoryFilter === dialog.id) setCategoryFilter(dialog.parentId || "");
    if (dialog.kind === "create" && categoryParentDraft) setExpandedCategories((current) => [...new Set([...current, categoryParentDraft])]);
    setCategoryDialog(null);
  };

  const moveEntries = async (entryIds: string[], id: string) => {
    if (!entryIds.length || libraryBusy) return;
    setIsCategorizing(true); setCategoryError("");
    try {
      const paths = uniquePaths(entryIds.flatMap((entryId) => entryById.get(entryId)?.replicas.map((replica) => replica.path) || []));
      const next = moveLibraryRecordsToCategory(normalizeLibraryState(await loadLibrary()), paths, id || null);
      await saveLibrary(next);
      applyState(next);
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error) { setCategoryError(String(error)); }
    finally { setIsCategorizing(false); setMoveEntryIds([]); setDragPaths([]); setDropTarget(""); }
  };

  const handleTrash = async (entryIds: string[]) => {
    if (!entryIds.length || libraryBusy) return;
    setTrashBusy(true); setMessage("");
    try {
      const replicaIds = entryIds.flatMap((entryId) => entryById.get(entryId)?.replicas.map((replica) => replica.id) || []);
      const result = await moveReplicasToTrash(normalizeLibraryState(await loadLibrary()), replicaIds);
      applyState(result.state);
      setSelectedEntryIds((current) => current.filter((id) => !entryIds.includes(id)));
      if (entryIds.includes(inspectedEntryId)) setInspectedEntryId("");
      if (result.failures.length) setMessage(t("library.ux.trashPartial", { value0: result.failures.length }));
      else if (result.removedRecords.length) setMessage(t("library.ux.removedMissing", { value0: result.removedRecords.length }));
      setRemoveEntryIds([]);
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error) {
      setMessage(t("library.ux.trashFailed", { value0: String(error) }));
    } finally { setTrashBusy(false); }
  };

  const toggleFavorite = async (entryId: string) => {
    if (libraryBusy) return;
    try {
      const latest = normalizeLibraryState(await loadLibrary());
      const next = favoriteIds.has(entryId) ? removeFavorite(latest, entryId) : addFavorite(latest, entryId);
      await saveLibrary(next);
      applyState(next);
    } catch (error) { setMessage(t("library.ux.favoriteFailed", { value0: String(error) })); }
  };

  const toggleSelected = (entryId: string, checked: boolean) => {
    setSelectedEntryIds((current) => checked ? (current.includes(entryId) ? current : [...current, entryId]) : current.filter((id) => id !== entryId));
  };

  const handleDragStart = (entry: LibraryEntryView, event: React.DragEvent) => {
    const paths = selectedIds.has(entry.entryId) ? selectedEntries.map((item) => item.primary.path) : [entry.primary.path];
    setDragPaths(paths);
    event.dataTransfer.effectAllowed = "all";
    event.dataTransfer.setData("application/x-nova-library-entries", JSON.stringify(paths));
  };

  const handleDropOnEntry = async (targetEntryId: string) => {
    if (libraryBusy || !dragPaths.length || sortMode !== "manual" || grouped) return;
    const target = entryById.get(targetEntryId);
    if (!target || dragPaths.some((path) => samePath(path, target.primary.path))) return;
    const moving = new Set(dragPaths.map(windowsPathKey));
    const order = [...entryViews].sort((a, b) => a.sortIndex - b.sortIndex);
    const moved = order.filter((entry) => moving.has(windowsPathKey(entry.primary.path)));
    const remaining = order.filter((entry) => !moving.has(windowsPathKey(entry.primary.path)));
    const to = remaining.findIndex((entry) => entry.entryId === targetEntryId);
    if (!moved.length || to < 0) return;
    remaining.splice(to, 0, ...moved);
    applyState(await reorderLibraryRecords(await loadLibrary(), remaining.map((entry) => entry.primary.path)));
    setDragPaths([]);
  };

  const handleDispatch = async (plan: DispatchPlan) => {
    if (isDispatching) return;
    setIsDispatching(true); setDispatchError("");
    const failures: string[] = [];
    try {
      if (plan.additions.length) {
        const result = await submitAndAwait("batch-copy", { items: plan.additions.map((item) => ({ sourcePath: item.sourcePath, targetDirectory: item.folderPath, targetFileName: item.fileName, overwrite: false })) });
        const failed = new Set(result.failures.map((failure) => failure.path));
        for (const item of plan.additions) if (failed.has(item.sourcePath)) failures.push(t("library.ux.copyFailed", { value0: item.fileName }));
        // A.2: merge the newly created copies back through the scan kernel
        // (recompute checksum + checksum merge) instead of `addOrUpdateRecord`.
        const copiedFolders = uniquePaths(plan.additions.filter((item) => !failed.has(item.sourcePath)).map((item) => item.folderPath));
        let working = await loadLibrary();
        for (const folderPath of copiedFolders) {
          const scanned = await scanLibraryFolders(working, { onlyFolderPath: folderPath });
          working = scanned.state;
          for (const failure of scanned.failures) failures.push(`${fileNameFromPath(failure.path)}：${failure.reason}`);
        }
      }
      if (plan.renames.length) {
        const result = await submitAndAwait("batch-rename", { items: plan.renames.map((item) => ({ sourcePath: item.sourcePath, targetFileName: item.fileName })) });
        const failed = new Set(result.failures.map((failure) => failure.path));
        const succeeded = plan.renames.filter((item) => !failed.has(item.sourcePath));
        for (const item of plan.renames) if (failed.has(item.sourcePath)) failures.push(t("library.ux.renameFailedItem", { value0: item.fileName }));
        if (succeeded.length) {
          const working = await loadLibrary();
          await saveLibrary(applyReplicaRenames(working, succeeded.map((item) => ({ replicaId: item.replicaId, fileName: item.fileName }))));
        }
      }
      if (plan.trash.length) {
        const working = await loadLibrary();
        const result = await moveReplicasToTrash(working, plan.trash.map((item) => item.replicaId));
        for (const failure of result.failures) failures.push(t("library.ux.trashFailedItem", { value0: failure.path }));
      }
      applyState(await loadLibrary());
      setDispatchEntryId("");
      if (failures.length) setMessage(t("library.ux.dispatchPartial", { value0: failures.length, value1: failures.slice(0, 3).join("；") }));
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error) {
      setDispatchError(String(error));
    } finally { setIsDispatching(false); }
  };

  // --- OS drag import (E.3) ------------------------------------------------------
  const isInternalLibraryDrag = (event: React.DragEvent) => Array.from(event.dataTransfer.types).includes("application/x-nova-library-entries");

  const handlePageDragOver = (event: React.DragEvent) => {
    if (isInternalLibraryDrag(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  };

  const handlePageDrop = async (event: React.DragEvent) => {
    if (isInternalLibraryDrag(event)) return;
    event.preventDefault();
    const files = Array.from(event.dataTransfer.files || []);
    if (!files.length) return;
    const items: DroppedItem[] = [];
    for (const file of files) {
      const path = (file as File & { path?: string }).path;
      if (!path) continue;
      const info = await getPathInfo(path).catch(() => null);
      items.push({ path, isDirectory: !!info?.is_dir });
    }
    if (!items.length) return;
    setDropError("");
    setDropItems(items);
  };

  const handleDropSubmit = async (plan: DropImportPlan) => {
    if (isDropBusy) return;
    setIsDropBusy(true); setDropError("");
    const failures: string[] = [];
    try {
      let working = normalizeLibraryState(await loadLibrary());
      for (const folderPath of plan.mountFolders) {
        try { working = await addLocalLibraryFolder(working, folderPath); }
        catch (error: any) { failures.push(`${folderPath}：${String(error)}`); }
      }
      const scanFolders = new Set<string>();
      for (const item of plan.copyToFolder) {
        try {
          await copyFileToDirectory(item.sourcePath, item.folderPath, fileNameFromPath(item.sourcePath), false);
          scanFolders.add(item.folderPath);
        } catch (error: any) { failures.push(`${fileNameFromPath(item.sourcePath)}：${String(error)}`); }
      }
      const contentPaths: string[] = [];
      for (const directory of plan.importFolderContents) {
        try {
          const entries = await listLitematicFileEntriesInDirectory(directory, true);
          for (const entry of entries) contentPaths.push(entry.path);
        } catch (error: any) { failures.push(`${directory}：${String(error)}`); }
      }
      const toBuiltin = [...plan.importToBuiltin, ...contentPaths];
      if (toBuiltin.length) {
        const outcome = await importFilesToBuiltinFolder(working, toBuiltin);
        working = outcome.state;
        for (const failure of outcome.failures) failures.push(`${fileNameFromPath(failure.path)}：${failure.reason}`);
      }
      for (const folderPath of scanFolders) {
        const scanned = await scanLibraryFolders(working, { onlyFolderPath: folderPath });
        working = scanned.state;
        for (const failure of scanned.failures) failures.push(`${fileNameFromPath(failure.path)}：${failure.reason}`);
      }
      applyState(working);
      setDropItems(null);
      if (failures.length) setMessage(t("library.ux.importPartial", { value0: failures.length, value1: failures.slice(0, 3).join("；") }));
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error: any) {
      setDropError(t("library.ux.importFailed", { value0: String(error) }));
    } finally { setIsDropBusy(false); }
  };

  // --- packaged export (E.6) ------------------------------------------------------
  const exportTargetIds = (): string[] => {
    if (selectedEntries.length) return selectedEntries.map((entry) => entry.entryId);
    if (inspectedEntry) return [inspectedEntry.entryId];
    return filteredEntries.map((entry) => entry.entryId);
  };

  const handleExport = async (options: ExportOptions) => {
    if (isExporting) return;
    const entryIds = exportTargetIds();
    if (!entryIds.length) { setExportError(t("library.ux.noExportEntries")); return; }
    setIsExporting(true); setExportError("");
    try {
      const result = await exportLibraryZip(normalizeLibraryState(await loadLibrary()), entryIds, {
        includePreview: options.includePreview,
        includeMaterialList: options.includeMaterialList,
        includeManifest: options.includeManifest,
        outputPath: options.outputPath,
      });
      if (result.status === "completed") {
        setExportOpen(false);
        setMessage(t("library.ux.exported", { value0: result.entryCount, value1: result.failures.length ? t("library.ux.notIncluded", { value0: result.failures.length }) : "" }));
      } else {
        setExportError(result.errors[0] || (result.status === "cancelled" ? t("library.ux.exportCancelled") : t("library.ux.exportFailed")));
      }
    } catch (error: any) {
      setExportError(String(error));
    } finally { setIsExporting(false); }
  };

  // --- reposition a missing replica (E.4) ----------------------------------------
  const applyRepositionOutcome = (outcome: RepositionOutcome, replicaId: string, newPath: string) => {
    if (outcome.kind === "repositioned") { applyState(outcome.state); setMessage(t("library.ux.repositioned")); return; }
    if (outcome.kind === "merged") { applyState(outcome.state); setMessage(t("library.ux.merged")); return; }
    if (outcome.kind === "invalid") { setMessage(t("library.ux.noRepositionFile")); return; }
    setRepositionConflict({ replicaId, occupiedByEntryId: outcome.occupiedByEntryId, newPath });
    setRepositionRename(fileNameFromPath(newPath));
    setRepositionError(t("library.ux.repositionChoice"));
  };

  const handleRepositionReplica = async (replicaId: string) => {
    if (repositionPending.current) return;
    repositionPending.current = true;
    setIsRepositioning(true); setMessage(""); setRepositionError("");
    try {
      const picked = await selectLitematicFile();
      if (!picked) return;
      const outcome = await repositionReplica(normalizeLibraryState(await loadLibrary()), replicaId, picked, {});
      applyRepositionOutcome(outcome, replicaId, picked);
    } catch (error: any) {
      setMessage(t("library.ux.locateFailed", { value0: String(error) }));
    } finally { repositionPending.current = false; setIsRepositioning(false); }
  };

  const resolveRepositionConflict = async (mode: "reject" | "merge" | "rename") => {
    if (!repositionConflict || isRepositioning) return;
    const { replicaId, newPath } = repositionConflict;
    setIsRepositioning(true); setRepositionError("");
    try {
      let targetPath = newPath;
      if (mode === "rename") {
        const desired = ensureLitematicFileName(repositionRename);
        if (!desired) { setRepositionError(t("library.ux.validName")); return; }
        const renamed = await submitAndAwait("batch-rename", { items: [{ sourcePath: newPath, targetFileName: desired }] });
        if (renamed.failures.length) { setRepositionError(t("library.ux.renameFailed", { value0: renamed.failures[0].reason })); return; }
        targetPath = joinPath(parentDirectory(newPath), desired);
      }
      const outcome = await repositionReplica(normalizeLibraryState(await loadLibrary()), replicaId, targetPath, mode === "merge" ? { onConflict: "relocate" } : {});
      setRepositionConflict(null);
      applyRepositionOutcome(outcome, replicaId, targetPath);
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error: any) {
      setRepositionError(t("library.ux.locateFailed", { value0: String(error) }));
    } finally { setIsRepositioning(false); }
  };

  // --- card binding --------------------------------------------------------------
  const binding: LibraryCardBinding = {
    isCurrent: (entry) => samePath(currentFile, entry.primary.path),
    isInspected: (entry) => entry.entryId === inspectedEntryId,
    isChecked: (entry) => selectedIds.has(entry.entryId),
    previewUrl: (entry) => previewDataUrls[entry.primary.path] || "",
    previewLoading: (entry) => !!previewLoadingPaths[entry.primary.path],
    previewError: (entry) => previewErrors[entry.primary.path] || "",
    onVisibility: handlePreviewVisibility,
    busy: libraryBusy,
    onInspect: (entry) => setInspectedEntryId(current => current === entry.entryId ? "" : entry.entryId),
    onToggle: (entry, checked) => toggleSelected(entry.entryId, checked),
    onActivate: (entry) => void handleActivate(entry),
    onDragStart: handleDragStart,
    onDragEnd: () => { setDragPaths([]); setDropTarget(""); },
    onDrop: (entry, event) => { if (!isInternalLibraryDrag(event)) return; event.preventDefault(); event.stopPropagation(); void handleDropOnEntry(entry.entryId); },
  };

  const handleCategoryDragOver = (target: string, event: React.DragEvent) => {
    if (!dragPaths.length || libraryBusy) return;
    event.preventDefault(); event.dataTransfer.dropEffect = "move"; setDropTarget(target || "untagged");
  };
  const handleCategoryDragLeave = () => setDropTarget("");
  const handleCategoryDrop = (target: string, event: React.DragEvent) => {
    event.preventDefault(); event.stopPropagation();
    const draggedEntries = entryViews.filter((entry) => dragPaths.some((path) => entry.replicas.some((replica) => samePath(replica.path, path))));
    void moveEntries(draggedEntries.map((entry) => entry.entryId), target).catch((error) => setMessage(t("library.ux.categoryMoveFailed", { value0: String(error) })));
  };

  const toggleColumn = (id: string, checked: boolean) => setColumns((current) => checked ? (current.includes(id) ? current : [...current, id]) : current.filter((columnId) => columnId !== id));

  const activeView = view === "grid"
    ? <LibraryGrid entries={pageEntries} binding={binding} />
    : view === "list"
      ? <LibraryList entries={pageEntries} binding={binding} columns={columns} allSelected={allPageSelected} someSelected={somePageSelected} onToggleAll={(checked) => setSelectedEntryIds((current) => checked ? [...new Set([...current, ...pageEntries.map((entry) => entry.entryId)])] : current.filter((id) => !pageEntries.some((entry) => entry.entryId === id)))} />
      : <LibraryDetail entry={inspectedEntry} variant="full" columns={columns} onToggleColumn={toggleColumn} previewUrl={inspectedEntry ? previewDataUrls[inspectedEntry.primary.path] || "" : ""} previewLoading={inspectedEntry ? !!previewLoadingPaths[inspectedEntry.primary.path] : false} previewError={inspectedEntry ? previewErrors[inspectedEntry.primary.path] || "" : ""} busy={libraryBusy} favorite={!!inspectedEntry && favoriteIds.has(inspectedEntry.entryId)} generatingPreview={!!generatingPreviewId}
        onActivate={() => inspectedEntry && void handleActivate(inspectedEntry)} onOpenProperties={() => { if (inspectedEntry) { setCurrentFile(inspectedEntry.primary.path); setRoute("properties"); } }} onOpenFolder={() => inspectedEntry && void openFileParentDir(inspectedEntry.primary.path).catch((error) => setMessage(t("library.ux.openFolderFailed", { value0: String(error) })))} onCopy={() => inspectedEntry && (setDispatchError(""), setDispatchEntryId(inspectedEntry.entryId))} onMoveCategory={() => inspectedEntry && (setMoveCategoryId(""), setCategoryError(""), setMoveEntryIds([inspectedEntry.entryId]))} onGeneratePreview={() => inspectedEntry && void handleGeneratePreview(inspectedEntry)} onTrash={() => setRemoveEntryIds(inspectedEntry ? [inspectedEntry.entryId] : [])} onToggleFavorite={() => inspectedEntry && void toggleFavorite(inspectedEntry.entryId)} categoryLabel={(id) => categoryPath(state, id) || id} onRepositionReplica={(replicaId: string) => void handleRepositionReplica(replicaId)} repositioning={isRepositioning} />;

  const boardContent = grouped
    ? <LibraryGrouping groups={groups} mode={groupMode} renderEntries={(entries) => <LibraryGrid entries={entries} binding={binding} />} renderStack={(group) => <LibraryGrid entries={group.entries.slice(0, 1)} binding={binding} />} />
    : activeView;

  const dispatchEntryView = dispatchEntry;
  const addSelectedToComposition = async () => {
    try {
      const result = await queryComposition({ action: "create", sourcePaths: selectedEntries.map(entry => entry.primary.path) }, crypto.randomUUID());
      if (!result.project) throw new Error(t("library.ux.compositionMissing"));
      setCompositionId(result.project.id);
      setRoute("composition");
    } catch (error) { setMessage(String(error)); }
  };

  const stockpileCurrent = (epoch: number) => libraryAlive.current && stockpileEpoch.current === epoch;
  const closeStockpile = () => {
    if (stockpileWritePending.current) return;
    stockpileEpoch.current += 1;
    stockpileReadPending.current = false;
    setStockpileDraft(null);
  };
  const loadStockpileTargets = async (epoch: number) => {
    if (stockpileReadPending.current) return;
    stockpileReadPending.current = true;
    setStockpileDraft(current => current && ({ ...current, loading: true, error: "" }));
    try {
      // Read only: opening this chooser never creates or changes a task.
      const workspace = await getStockpileTask({ uiLocale: getUiLocale(), languageMap: getMinecraftTranslations() });
      if (stockpileCurrent(epoch)) setStockpileDraft(current => current && ({ ...current, targets: workspace.tasks }));
    } catch (error) {
      if (stockpileCurrent(epoch)) setStockpileDraft(current => current && ({ ...current, error: String(error) }));
    } finally {
      if (stockpileCurrent(epoch)) {
        stockpileReadPending.current = false;
        setStockpileDraft(current => current && ({ ...current, loading: false }));
      }
    }
  };
  const beginStockpile = () => {
    if (libraryBusy || stockpileDraft || stockpileReadPending.current || !selectedEntries.length) return;
    const epoch = ++stockpileEpoch.current;
    // Preserve exact primary identities and every selection; Rust merges matching sources.
    const sources: StockpileSourceInput[] = selectedEntries.map(entry => ({ sourcePath: entry.primary.path,
      scope: { kind: "all", value: null }, includeContainerItems: false, copies: 1 }));
    setStockpileSaved(null);
    setStockpileDraft({ sources, phase: "target", targets: [], targetTaskId: "", loading: true, error: "" });
    void loadStockpileTargets(epoch);
  };
  const editStockpileSources = async () => {
    if (!stockpileDraft || stockpileReadPending.current || stockpileWritePending.current) return;
    if (!stockpileDraft.targetTaskId) {
      setStockpileDraft(current => current && ({ ...current, phase: "sources", task: undefined, error: "" }));
      return;
    }
    const epoch = stockpileEpoch.current;
    const taskId = stockpileDraft.targetTaskId;
    stockpileReadPending.current = true;
    setStockpileDraft(current => current && ({ ...current, loading: true, error: "" }));
    try {
      const workspace = await getStockpileTask({ taskId, uiLocale: getUiLocale(), languageMap: getMinecraftTranslations() });
      if (!stockpileCurrent(epoch)) return;
      if (!workspace.task || workspace.task.taskId !== taskId) throw new Error(t("stockpile.sources.taskMissing"));
      setStockpileDraft(current => current && ({ ...current, phase: "sources", task: workspace.task! }));
    } catch (error) {
      if (stockpileCurrent(epoch)) setStockpileDraft(current => current && ({ ...current, error: String(error) }));
    } finally {
      if (stockpileCurrent(epoch)) {
        stockpileReadPending.current = false;
        setStockpileDraft(current => current && ({ ...current, loading: false }));
      }
    }
  };
  const submitStockpileSources = async ({ name, sources, requestId }: StockpileSourcesSubmission) => {
    if (!stockpileDraft || stockpileDraft.phase !== "sources") throw new Error(t("stockpile.sources.taskMissing"));
    if (stockpileWritePending.current) throw new Error(t("editor.saving"));
    const epoch = stockpileEpoch.current;
    const target = stockpileDraft.task;
    stockpileWritePending.current = true;
    setStockpileWriting(true);
    try {
      // The shared editor keeps requestId stable on unchanged retries. No optimistic counts or material merging.
      const workspace = await commandStockpileTask({
        uiLocale: getUiLocale(), languageMap: getMinecraftTranslations(), requestId, sources,
        ...(target ? { action: "addSources", taskId: target.taskId, expectedRevision: target.sourceRevision }
          : { action: "create", name }),
      });
      if (!stockpileCurrent(epoch)) return;
      if (!workspace.task || (target && workspace.task.taskId !== target.taskId)) throw new Error(t("stockpile.sources.resultMissing"));
      setStockpileSaved(workspace.task);
      setStockpileDraft(null);
    } finally {
      stockpileWritePending.current = false;
      if (stockpileCurrent(epoch)) setStockpileWriting(false);
    }
  };

  return (
    <div className="nova-page library-page" onDragOver={handlePageDragOver} onDrop={(event) => void handlePageDrop(event)} onKeyDown={event => {
      if (event.key !== "Escape" || event.defaultPrevented || event.nativeEvent.isComposing) return;
      if ((event.target as Element).closest('[role="dialog"], [role="menu"], [role="listbox"]')) return;
      if (categoriesOpen) setCategoriesOpen(false);
      else if (inspectedEntryId) setInspectedEntryId("");
      else if (selectedEntryIds.length) setSelectedEntryIds([]);
      else return;
      event.preventDefault(); event.stopPropagation();
    }}>
      <LibraryToolbar
        busy={libraryBusy} isRefreshing={isRefreshing} isOpening={isOpening}
        search={search} onSearchChange={setSearch} searching={searchLoading} searchMatches={indexMatchCount} indexReady={indexReady}
        statusFilter={statusFilter} onStatusFilterChange={setStatusFilter} sortMode={sortMode} onSortModeChange={setSortMode}
        view={view} onViewChange={setView} groupDimension={groupDimension} groupMode={groupMode} onGroupDimensionChange={setGroupDimension} onGroupModeChange={setGroupMode}
        onImport={() => void handleSelect()} onDownload={() => void openRedenLibraryWithWindowBehavior(() => setShowRedenLibraryOverlay(true)).catch((error) => setMessage(t("library.ux.onlineOpenFailed", { value0: String(error) })))}
        onManageFolders={() => void openLocalLibraryFoldersWithWindowBehavior(() => setShowLocalLibraryFoldersOverlay(true)).catch((error) => setMessage(t("library.ux.foldersOpenFailed", { value0: String(error) })))}
        onRefresh={() => void handleRefresh()} onRescanAll={(forceRecheck) => void handleRescanAll(forceRecheck)} onOpenTaskCenter={() => setTaskCenterOpen(true)}
        onNewComposition={() => setRoute("composition")} onImportClipboard={onImportClipboard}
        onExport={() => { setExportError(""); setExportOpen(true); }}
      />
      <div className="library-status-slot">
      {message && <div className="library-message" role="alert">{displayPathText(message)}<Button variant="ghost" size="icon-sm" aria-label={t("library.ux.dismiss")} onClick={() => setMessage("")}><ThemeIcon name="close" size={14} fallback={X} /></Button></div>}
      {stockpileSaved && <div className="library-message" role="status">
        <span>{t("stockpile.sources.saved", { name: stockpileSaved.name })}</span>
        <Button size="sm" onClick={() => setRoute("statistics")}>{t("stockpile.sources.viewTask")}</Button>
        <Button variant="ghost" size="icon-sm" aria-label={t("library.closeDialog")} onClick={() => setStockpileSaved(null)}><ThemeIcon name="close" size={14} fallback={X} /></Button>
      </div>}
      {generatingPreviewId && <div className="library-progress" role="status"><span>{t("library.previewProgress", previewProgress)}</span><Button size="sm" onClick={() => void handlePreviewControl(previewPaused ? "resume" : "pause")}>{t(previewPaused ? "library.previewResume" : "library.previewPause")}</Button><Button size="sm" onClick={() => void handlePreviewControl("cancel")}>{t("library.previewCancel")}</Button></div>}
      {isRefreshing && <div className="library-progress" role="status">{t("library.refreshing")} {refreshProgress} / {refreshTotal}</div>}
      </div>
      <PreviewDraftDialog draft={previewDrafts[0]?.image ?? null} sourcePath={previewDrafts[0]?.path ?? ""} onSaved={image => { if (previewDrafts[0]) handlePreviewSaved(previewDrafts[0].path, image); }} onClose={() => {
        if (previewDraftQueue.current[0]?.image.draft_id === previewDrafts[0]?.image.draft_id) updatePreviewDrafts(previewDraftQueue.current.slice(1));
      }} />


      <div className="library-workspace" data-categories-open={categoriesOpen || undefined}>
        <LibraryCategoryTree
          state={state} totalCount={entryViews.length} untaggedCount={queryView?.untaggedCount || 0} busy={libraryBusy}
          categoryFilter={categoryFilter} untaggedOnly={untaggedOnly} favoritesOnly={favoritesOnly} recentOnly={recentOnly}
          expandedCategories={expandedCategories} dropTarget={dropTarget} hasDrag={dragPaths.length > 0}
          favorites={favorites} recent={recentItems} categoryEntryCount={(categoryId) => categoryCounts.get(categoryId) || 0}
          onToggleExpanded={(id) => setExpandedCategories((current) => current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id])}
          onSelectAll={() => { setCategoryFilter(""); setUntaggedOnly(false); setFavoritesOnly(false); setRecentOnly(false); }}
          onSelectUntagged={() => { setCategoryFilter(""); setUntaggedOnly(true); setFavoritesOnly(false); setRecentOnly(false); }}
          onSelectCategory={(id) => { setCategoryFilter(id); setUntaggedOnly(false); setFavoritesOnly(false); setRecentOnly(false); }}
          onSelectFavorites={() => { setCategoryFilter(""); setUntaggedOnly(false); setFavoritesOnly(true); setRecentOnly(false); }}
          onSelectRecent={() => { setCategoryFilter(""); setUntaggedOnly(false); setFavoritesOnly(false); setRecentOnly(true); }}
          onSelectRecentEntry={(entryId) => { setFavoritesOnly(false); setUntaggedOnly(false); setRecentOnly(true); setInspectedEntryId(entryId); }}
          onSelectRecentPath={(path) => void handleActivatePath(path)}
          onInspect={(entry) => setInspectedEntryId(entry.entryId)}
          onDragOverCategory={handleCategoryDragOver}
          onDragLeaveCategory={handleCategoryDragLeave}
          onDropCategory={handleCategoryDrop}
          onOpenCategoryDialog={openCategoryDialog} onOpenIconDialog={(id) => { setIconCategoryId(id); setCategoryError(""); }}
          onOpenTrash={() => { setTrashError(""); setTrashOpen(true); }} trashCount={trashEntries.length}
          onClose={() => setCategoriesOpen(false)}
        />

        <section className="library-collection" aria-label={t("library.ux.entries")}>
          <div className="library-collection-toolbar">
            <Button className="library-categories-toggle" variant="ghost" size="sm" aria-expanded={categoriesOpen} aria-controls="library-categories" onClick={() => setCategoriesOpen(value => !value)}>{t("editor.category")}</Button>
            <label className="library-select-all">
              <Checkbox checked={allPageSelected ? true : somePageSelected ? "indeterminate" : false} disabled={!pageEntries.length || libraryBusy} aria-label={t("library.selectCurrentPage")} onCheckedChange={(checked) => setSelectedEntryIds((current) => checked === true ? [...new Set([...current, ...pageEntries.map((entry) => entry.entryId)])] : current.filter((id) => !pageEntries.some((entry) => entry.entryId === id)))} />
              <span>{selectedEntries.length ? t("library.ux.selectedCount", { value0: selectedEntries.length }) : activeFilterLabel}</span>
            </label>
            {!selectedEntries.length && <span className="library-result-count" role="status">{filteredEntries.length} {t("library.items")}{searchLoading ? t("library.searchingInline") : ""}{grouped ? t("library.ux.groupCount", { value0: groups.length }) : ""}</span>}
            {view === "list" && <LibraryColumnPicker selected={columns} onToggle={toggleColumn} disabled={libraryBusy} />}
          {selectedEntries.length >= 1 && (
            <div className="library-batch-toolbar">
              <span>{t("library.ux.selected")} {selectedEntries.length}</span>
              <Button size="sm" disabled={libraryBusy || !!generatingPreviewId} onClick={() => void handleGeneratePreviews(selectedEntries)}>{t("library.previewGenerateSelected")}</Button>
              <Button size="sm" disabled={libraryBusy || !!stockpileDraft} onClick={beginStockpile}>{t("stockpile.sources.chooseTask")}</Button>
              <Button size="sm" disabled={libraryBusy} onClick={() => void addSelectedToComposition()}>{t("library.ux.addComposition")}</Button>
              <Button size="sm" disabled={libraryBusy} onClick={() => { setExportError(""); setExportOpen(true); }}>{t("library.ux.batchExport")}</Button>
              <Button size="sm" onClick={() => { setMoveCategoryId(""); setCategoryError(""); setMoveEntryIds(selectedEntries.map((entry) => entry.entryId)); }} disabled={libraryBusy}>{t("library.moveToCategoryMenu")}</Button>
              <Button size="sm" variant="ghost" disabled={libraryBusy} onClick={() => setRemoveEntryIds(selectedEntries.map((entry) => entry.entryId))}><ThemeIcon name="delete" size={14} fallback={Trash2} />{t("library.ux.trash")}</Button>
              <Button size="sm" variant="ghost" onClick={() => setSelectedEntryIds([])}>{t("stockpile.clearSelection")}</Button>
            </div>
          )}
          </div>
          <ScrollArea className="library-grid-scroll" onClick={event => {
            const target = event.target as Element;
            if (!target.closest('[data-entry-id], button, input, a, [role="button"], .library-detail-content')) setInspectedEntryId("");
          }}>
            {filteredEntries.length === 0 && queryBusy ? <p className="library-empty-state" role="status">{t("library.loading")}</p> : filteredEntries.length === 0
              ? <div className="library-empty-state"><Box size={40} aria-hidden="true" /><h3>{entryViews.length || search ? t("library.noSchematicsFound") : t("library.firstSchematic")}</h3><p>{entryViews.length || search ? t("library.changeFilterHint") : t("library.importFirstHint")}</p><div className="library-empty-actions">{entryViews.length || search ? <Button onClick={() => { setSearch(""); setCategoryFilter(""); setUntaggedOnly(false); setFavoritesOnly(false); setRecentOnly(false); setStatusFilter(""); }}>{t("library.clearFilters")}</Button> : <Button variant="default" onClick={() => void handleSelect()} disabled={libraryBusy}><ThemeIcon name="import" size={16} fallback={Plus} />{t("library.importSchematic")}</Button>}</div></div>
              : boardContent}
          </ScrollArea>
          <footer className="library-collection-footer">
            <span>{issueCount ? t("library.ux.attentionCount", { value0: issueCount }) : t("library.activateHint")}</span>
          </footer>
        </section>

        {view !== "detail" && inspectedEntry && (
          <aside className="library-inspector" aria-label={t("library.schematicDetails")}>
            <div className="library-pane-heading"><h3>{t("library.details")}</h3>{inspectedEntry && <Button variant="ghost" size="icon-sm" aria-label={t("library.closeDetails")} onClick={() => setInspectedEntryId("")}><ThemeIcon name="close" size={15} fallback={X} /></Button>}</div>
            <ScrollArea className="library-inspector-scroll">
              <LibraryDetail
                entry={inspectedEntry} variant="inspector" columns={columns} onToggleColumn={toggleColumn}
                previewUrl={inspectedEntry ? previewDataUrls[inspectedEntry.primary.path] || "" : ""} previewLoading={inspectedEntry ? !!previewLoadingPaths[inspectedEntry.primary.path] : false} previewError={inspectedEntry ? previewErrors[inspectedEntry.primary.path] || "" : ""}
                busy={libraryBusy} favorite={!!inspectedEntry && favoriteIds.has(inspectedEntry.entryId)} generatingPreview={!!generatingPreviewId}
                onActivate={() => inspectedEntry && void handleActivate(inspectedEntry)} onOpenProperties={() => { if (inspectedEntry) { setCurrentFile(inspectedEntry.primary.path); setRoute("properties"); } }}
                onOpenFolder={() => inspectedEntry && void openFileParentDir(inspectedEntry.primary.path).catch((error) => setMessage(t("library.ux.openFolderFailed", { value0: String(error) })))}
                onCopy={() => inspectedEntry && (setDispatchError(""), setDispatchEntryId(inspectedEntry.entryId))}
                onMoveCategory={() => inspectedEntry && (setMoveCategoryId(""), setCategoryError(""), setMoveEntryIds([inspectedEntry.entryId]))}
                onGeneratePreview={() => inspectedEntry && void handleGeneratePreview(inspectedEntry)}
                onTrash={() => setRemoveEntryIds(inspectedEntry ? [inspectedEntry.entryId] : [])}
                onToggleFavorite={() => inspectedEntry && void toggleFavorite(inspectedEntry.entryId)}
                categoryLabel={(id) => categoryPath(state, id) || id} onRepositionReplica={(replicaId: string) => void handleRepositionReplica(replicaId)} repositioning={isRepositioning}
              />
            </ScrollArea>
          </aside>
        )}
      </div>

      {stockpileDraft?.phase === "target" && <Dialog title={t("stockpile.sources.chooseTask")} subtitle={t("stockpile.sources.chooseTaskHint")} width="sm" onClose={closeStockpile}
        footer={<><Button variant="ghost" onClick={closeStockpile}>{t("editor.cancel")}</Button>
          <Button disabled={stockpileDraft.loading} onClick={() => void editStockpileSources()}>{stockpileDraft.loading ? t("editor.loading") : t("stockpile.sources.next")}</Button></>}>
        <div className="library-category-form">
          <label id="library-stockpile-target-label">{t("stockpile.sources.targetTask")}</label>
          <Dropdown ariaLabel={t("stockpile.sources.targetTask")} value={stockpileDraft.targetTaskId} disabled={stockpileDraft.loading}
            onChange={targetTaskId => setStockpileDraft(current => current && ({ ...current, targetTaskId, error: "" }))}
            options={[{ value: "", label: t("stockpile.sources.newTask") }, ...stockpileDraft.targets.map(task => ({ value: task.taskId, label: task.name }))]} />
          {stockpileDraft.loading && <p role="status">{t("editor.loading")}</p>}
          {!stockpileDraft.loading && !stockpileDraft.error && !stockpileDraft.targets.length && <p>{t("stockpile.sources.noTasks")}</p>}
          {stockpileDraft.error && <div role="alert" className="dialog-message dialog-message-error"><p>{stockpileDraft.error}</p>
            <Button size="sm" onClick={() => void loadStockpileTargets(stockpileEpoch.current)}>{t("stockpile.retry")}</Button></div>}
        </div>
      </Dialog>}
      {stockpileDraft?.phase === "sources" && <StockpileSourcesDialog mode={stockpileDraft.task ? "add" : "create"}
        initialSources={stockpileDraft.sources} task={stockpileDraft.task} onClose={closeStockpile} onSubmit={submitStockpileSources} />}

      {removeEntryIds.length > 0 && <Dialog title={t("library.ux.trashConfirm", { value0: removeEntryIds.length > 1 ? t("library.ux.itemCount", { value0: removeEntryIds.length }) : t("library.ux.thisSchematic") })} subtitle={t("library.ux.trashHint")} width="sm" busy={trashBusy} onClose={() => setRemoveEntryIds([])} footer={<><Button onClick={() => setRemoveEntryIds([])} disabled={trashBusy}>{t("library.keep")}</Button><Button variant="destructive" onClick={() => void handleTrash(removeEntryIds)} disabled={trashBusy} aria-busy={trashBusy}>{trashBusy ? t("library.ux.trashing") : t("library.ux.trash")}</Button></>}><ul className="library-remove-list">{removeEntryIds.slice(0, 8).map((id) => { const entry = entryById.get(id); return <li key={id}>{entry ? entry.displayName || entry.fileName : id}</li>; })}</ul>{removeEntryIds.length > 8 && <p>{t("library.additionalItems")} {removeEntryIds.length - 8} {t("library.items")}</p>}</Dialog>}

      {moveEntryIds.length > 0 && <Dialog title={t("library.moveToCategory")} subtitle={t("library.ux.moveHint", { value0: moveEntryIds.length })} width="sm" busy={isCategorizing} onClose={() => setMoveEntryIds([])} footer={<><Button disabled={isCategorizing} onClick={() => setMoveEntryIds([])}>{t("common.cancel")}</Button><Button variant="default" disabled={isCategorizing} onClick={() => void moveEntries(moveEntryIds, moveCategoryId).catch(() => undefined)}>{t("library.move")}</Button></>}><Dropdown ariaLabel={t("library.targetCategory")} value={moveCategoryId} onChange={setMoveCategoryId} options={[{ value: "", label: t("library.uncategorized") }, ...state.categories.map((category) => ({ value: category.id, label: categoryPath(state, category.id) }))]} />{categoryError && <p className="dialog-message dialog-message-error" role="alert">{categoryError}</p>}</Dialog>}

      {categoryDialog && <Dialog title={categoryDialog.kind === "create" ? t("library.createCategory") : categoryDialog.kind === "rename" ? t("library.renameCategory") : categoryDialog.kind === "move" ? t("library.moveCategory") : t("library.deleteCategoryConfirm")} subtitle={categoryDialog.kind === "delete" ? t("library.ux.categoryTrashHint") : undefined} width="sm" busy={isCategorizing} onClose={() => setCategoryDialog(null)} footer={<><Button disabled={isCategorizing} onClick={() => setCategoryDialog(null)}>{t("common.cancel")}</Button><Button variant={categoryDialog.kind === "delete" ? "destructive" : "default"} disabled={isCategorizing || (["create", "rename"].includes(categoryDialog.kind) && !categoryNameDraft.trim())} onClick={() => void confirmCategory().catch(() => undefined)}>{categoryDialog.kind === "delete" ? t("library.deleteCategory") : t("common.save")}</Button></>}>
        {["create", "rename"].includes(categoryDialog.kind) && <div className="library-category-form"><label htmlFor="library-category-name">{t("library.categoryName")}</label><Input id="library-category-name" autoFocus maxLength={80} disabled={isCategorizing} value={categoryNameDraft} onChange={(event) => setCategoryNameDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.nativeEvent.isComposing && categoryNameDraft.trim()) { event.preventDefault(); void confirmCategory().catch(() => undefined); } }} /></div>}
        {["create", "move"].includes(categoryDialog.kind) && <Dropdown ariaLabel={t("library.parentCategory")} value={categoryParentDraft} onChange={setCategoryParentDraft} options={[{ value: "", label: t("library.categoryRoot") }, ...state.categories.filter((category) => categoryDialog.kind !== "move" || !categoryDescendants(state, categoryDialog.id).has(category.id)).map((category) => ({ value: category.id, label: categoryPath(state, category.id) }))]} />}
        {categoryError && <p className="dialog-message dialog-message-error" role="alert">{categoryError}</p>}
      </Dialog>}

      {trashOpen && <Dialog title={t("library.ux.recycle")} width="lg" busy={trashBusy} onClose={() => setTrashOpen(false)} footer={<Button disabled={trashBusy} onClick={() => setTrashOpen(false)}>{t("common.close")}</Button>}>
        {trashError && <p role="alert" className="dialog-message dialog-message-error">{trashError}</p>}
        <TrashPanel
          state={state}
          busy={trashBusy}
          onBusyChange={(key) => setTrashBusy(!!key)}
          onStateChange={(next) => applyState(next)}
          onStatus={setTrashError}
        />
      </Dialog>}

      {dropItems && <DropImportDialog
        items={dropItems}
        folders={state.folders}
        busy={isDropBusy}
        error={dropError}
        onClose={isDropBusy ? () => undefined : () => { setDropItems(null); setDropError(""); }}
        onSubmit={(plan) => void handleDropSubmit(plan)}
      />}

      {exportOpen && <ExportDialog
        entryCount={exportTargetIds().length}
        busy={isExporting}
        error={exportError}
        onClose={isExporting ? () => undefined : () => { setExportOpen(false); setExportError(""); }}
        onSubmit={(options) => void handleExport(options)}
      />}

      {repositionConflict && <Dialog
        title={t("library.ux.repositionConflict")}
        subtitle={t("library.ux.pathOccupied")}
        width="sm"
        busy={isRepositioning}
        onClose={isRepositioning ? () => undefined : () => { setRepositionConflict(null); setRepositionError(""); }}
        footer={<>
          <Button disabled={isRepositioning} onClick={() => { setRepositionConflict(null); setRepositionError(""); setMessage(t("library.ux.repositionRejected")); }}>{t("library.ux.reject")}</Button>
          <Button disabled={isRepositioning} onClick={() => void resolveRepositionConflict("rename")}>{t("library.ux.renameLocate")}</Button>
          <Button variant="default" disabled={isRepositioning} onClick={() => void resolveRepositionConflict("merge")}>{t("library.ux.mergeEntry")}</Button>
        </>}
      >
        <div className="library-reposition-conflict">
          <p className="dialog-path-preview" title={displayPathText(repositionConflict.newPath)}>{displayPathText(repositionConflict.newPath)}</p>
          <label className="dialog-label" htmlFor="library-reposition-name">{t("library.ux.newLocateName")}</label>
          <ReplicaNameEditor
            value={repositionRename}
            disabled={isRepositioning}
            ariaLabel={t("library.ux.locateFileName")}
            onChange={setRepositionRename}
          />
        </div>
        {repositionError && <p className="dialog-message dialog-message-error" role="alert">{repositionError}</p>}
      </Dialog>}

      {iconCategoryId && state.categories.find((category) => category.id === iconCategoryId) && <CategoryIconDialog name={state.categories.find((category) => category.id === iconCategoryId)!.name} icon={state.categories.find((category) => category.id === iconCategoryId)!.icon} busy={isCategorizing} onClose={() => setIconCategoryId("")} onSubmit={async (icon: LibraryCategoryIcon) => { await commitCategories((latest) => updateLibraryCategory(latest, iconCategoryId, { icon })); setIconCategoryId(""); }} />}

      {dispatchEntryView && <SendProjectionDialog key={dispatchEntryView.entryId} entry={dispatchEntryView} folders={state.folders} busy={isDispatching} error={dispatchError} onClose={isDispatching ? () => undefined : () => { setDispatchEntryId(""); setDispatchError(""); }} onSubmit={(plan) => void handleDispatch(plan)} />}

      <LibraryTaskCenter open={taskCenterOpen} onClose={() => setTaskCenterOpen(false)} onFinished={() => { void loadLibrary().then((library) => applyState(normalizeLibraryState(library))).catch(() => undefined); }} />

      {showLocalLibraryFoldersOverlay ? <LocalLibraryFoldersDialog onClose={() => setShowLocalLibraryFoldersOverlay(false)} /> : null}
      {showRedenLibraryOverlay ? <RedenLibraryDialog onClose={() => setShowRedenLibraryOverlay(false)} /> : null}
    </div>
  );
}
