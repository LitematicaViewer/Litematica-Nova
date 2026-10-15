import React, { useEffect, useMemo, useRef, useState } from "react";
import { Box, Download, FolderOpen, FolderPlus, ImageOff, LayoutGrid, Plus, RefreshCw, Search, ChevronDown, ChevronRight, Folder, Trash2, X } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Checkbox } from "../../../../components/ui/checkbox";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Dropdown } from "../../../../components/Dropdown";
import {
  addOrUpdateRecord,
  analyzeFile,
  activateProjectionRecord,
  copyFileToDirectory,
  LibraryState,
  LibraryCategory,
  LibraryCategoryIcon,
  categoryDescendants,
  categoryPath,
  createLibraryCategory,
  deleteLibraryCategory,
  trashLibraryRecords,
  restoreLibraryTrash,
  discardLibraryTrash,
  moveLibraryRecordsToCategory,
  normalizeLibraryState,
  updateLibraryCategory,
  loadLibrary,
  LocalLibraryFolder,
  ProjectionRecord,
  reorderLibraryRecords,
  saveLibrary,
  searchLocalLibraryFolderRecords,
  setRecordPreview,
} from "../../../../../src/business/facade";
import { ContextMenu } from "radix-ui";
import { CategoryIcon, CategoryIconDialog } from "./categoryIcon";
import { displayFilePath, windowsPathKey } from "../../../../../src/business/projectionLibrary";
import { SendProjectionDialog } from "./sendDialog";
import { Dialog } from "../../../../components/Dialog";
import "./library.css";
import { ensureLitematicFileName } from "./function";
import { checkFileExists, generatePreviewImage, openFileParentDir, readImageBase64, readProjectionPreviewImage, selectLitematicFile } from "../../../../../src/business/facade";
import { LocalLibraryFoldersDialog, openLocalLibraryFoldersWithWindowBehavior } from "../../../local_library_folders";
import { RedenLibraryDialog, openRedenLibraryWithWindowBehavior } from "../../../reden_library";
import { emitEvent, listenEvent } from "../../../../../src/platform/events";
import { projectionLibraryImportedEvent, projectionLibraryStateChangedEvent } from "../../../libraryEvents";

function formatSize(numBytes: number): string {
  let value = Math.max(0, numBytes);
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  while (value >= 1024.0 && index < units.length - 1) {
    value /= 1024.0;
    index++;
  }
  return index === 0 ? `${Math.floor(value)} ${units[index]}` : `${value.toFixed(1)} ${units[index]}`;
}

function formatDate(timestamp: number): string {
  if (!timestamp) return "-";
  const d = new Date(timestamp);
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function recordStatusLabel(record: ProjectionRecord): string {
  if (record.status === "missing") return "文件丢失";
  if (record.status === "parse_error") return "解析失败";
  return "";
}

function parentDirectory(path: string): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const lastSeparatorIndex = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  if (lastSeparatorIndex < 0) return "";
  const parent = normalized.slice(0, lastSeparatorIndex);
  return /^(?:\\\\\?\\)?[a-z]:$/i.test(parent) ? `${parent}${normalized[lastSeparatorIndex]}` : parent;
}

function pickDefaultSendFolder(record: ProjectionRecord, folders: LocalLibraryFolder[]): string {
  const sourceParent = windowsPathKey(parentDirectory(record.path));
  return folders.find((folder) => windowsPathKey(folder.path) !== sourceParent)?.path || folders[0]?.path || "";
}

const samePath = (left: string, right: string) => windowsPathKey(left) === windowsPathKey(right);
const hasPath = (paths: string[], path: string) => paths.some((candidate) => samePath(candidate, path));
const uniquePaths = (paths: string[]) => [...new Map(paths.map((path) => [windowsPathKey(path), path])).values()];

function ProjectionCard({ record, isCurrent, isInspected, checked, previewUrl, loading, busy,
  onInspect, onToggle, onActivate, onDragStart, onDragEnd, onDrop,
}: {
  record: ProjectionRecord; isCurrent: boolean; isInspected: boolean; checked: boolean;
  previewUrl: string; loading: boolean; busy: boolean;
  onInspect: () => void; onToggle: (checked: boolean) => void; onActivate: () => void;
  onDragStart: (event: React.DragEvent) => void; onDragEnd: () => void; onDrop: (event: React.DragEvent) => void;
}) {
  const [failedImage, setFailedImage] = useState("");
  const hasImage = !!previewUrl && failedImage !== previewUrl;
  return (
    <article className="library-tile" data-inspected={isInspected || undefined} data-checked={checked || undefined}
      draggable={!busy} onDragStart={onDragStart} onDragEnd={onDragEnd}
      onDragOver={(event) => { if (!busy) event.preventDefault(); }} onDrop={onDrop}>
      <div className="library-tile-selection">
        <Checkbox checked={checked} disabled={busy} aria-label={`选择 ${record.displayName || record.fileName}`}
          onCheckedChange={(value) => onToggle(value === true)} />
      </div>
      {isCurrent && <span className="library-current-marker">当前</span>}
      <Button className="library-tile-open" variant="ghost" aria-pressed={isInspected}
        aria-label={`查看 ${record.displayName || record.fileName} 的详情`} onClick={onInspect}
        onDoubleClick={() => { if (!busy && record.status === "ok") onActivate(); }}>
        <span className="library-tile-image">
          {hasImage ? <img src={previewUrl} alt="" loading="lazy" draggable={false} onError={() => setFailedImage(previewUrl)} />
            : <span className="library-preview-placeholder"><ImageOff size={30} aria-hidden="true" /><span>{loading ? "预览加载中…" : "暂无预览图"}</span></span>}
        </span>
        <span className="library-tile-name" title={record.displayName || record.fileName}>{record.displayName || record.fileName}</span>
        <span className="library-tile-meta">{recordStatusLabel(record) || `${record.totalBlocks.toLocaleString()} 方块`}</span>
      </Button>
    </article>
  );
}

/**
 * Renders the local projection library page.
 */
export function LibraryPage({ currentFile, setCurrentFile, setRoute }: any) {
  const [state, setState] = useState<LibraryState>({ records: [], folders: [] });
  const [search, setSearch] = useState("");
  const [categoryFilter, setCategoryFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const [sortMode, setSortMode] = useState("manual");
  const [pageSize, setPageSize] = useState(20);
  const [page, setPage] = useState(1);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isOpening, setIsOpening] = useState(false);
  const [refreshProgress, setRefreshProgress] = useState(0);
  const [removeRecords, setRemoveRecords] = useState<ProjectionRecord[]>([]);
  const [isRemoving, setIsRemoving] = useState(false);
  const [removeError, setRemoveError] = useState("");
  const [trashOpen, setTrashOpen] = useState(false);
  const [discardTrashId, setDiscardTrashId] = useState<string | null>(null);
  const [generatingPreviewPath, setGeneratingPreviewPath] = useState("");
  const [previewError, setPreviewError] = useState("");
  const [previewDataUrls, setPreviewDataUrls] = useState<Record<string, string>>({});
  const [projectionPreviewDataUrls, setProjectionPreviewDataUrls] = useState<Record<string, string>>({});
  const [projectionPreviewLoadingPaths, setProjectionPreviewLoadingPaths] = useState<Record<string, boolean>>({});
  const [projectionPreviewLoadedPaths, setProjectionPreviewLoadedPaths] = useState<Record<string, boolean>>({});
  const [showLocalLibraryFoldersOverlay, setShowLocalLibraryFoldersOverlay] = useState(false);
  const [showRedenLibraryOverlay, setShowRedenLibraryOverlay] = useState(false);
  const [dragPaths, setDragPaths] = useState<string[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [inspectedPath, setInspectedPath] = useState("");
  const [untaggedOnly, setUntaggedOnly] = useState(false);
  const [moveDialogPaths, setMoveDialogPaths] = useState<string[]>([]);
  const [moveCategoryId, setMoveCategoryId] = useState("");
  const [isCategorizing, setIsCategorizing] = useState(false);
  const [categoryError, setCategoryError] = useState("");
  const [categoryDialog, setCategoryDialog] = useState<{ kind: "create" | "rename" | "move" | "delete"; id: string; parentId: string | null } | null>(null);
  const [categoryNameDraft, setCategoryNameDraft] = useState("");
  const [categoryParentDraft, setCategoryParentDraft] = useState("");
  const [expandedCategories, setExpandedCategories] = useState<string[]>([]);
  const [iconCategoryId, setIconCategoryId] = useState("");
  const [dropTarget, setDropTarget] = useState("");
  const [detailImageKind, setDetailImageKind] = useState<"auto" | "preview">("auto");
  const [failedDetailImage, setFailedDetailImage] = useState("");
  const previewRequests = useRef(new Set<string>());
  const previewRevisions = useRef(new Map<string, number>());
  const [sendDialogRecord, setSendDialogRecord] = useState<ProjectionRecord | null>(null);
  const [sendTargetDirectory, setSendTargetDirectory] = useState("");
  const [sendTargetFileName, setSendTargetFileName] = useState("");
  const [sendOverwrite, setSendOverwrite] = useState(false);
  const [sendError, setSendError] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [folderSearchRecords, setFolderSearchRecords] = useState<ProjectionRecord[]>([]);
  const [isFolderSearchLoading, setIsFolderSearchLoading] = useState(false);
  const [folderSearchError, setFolderSearchError] = useState("");
  const libraryBusy = isRefreshing || isOpening || isRemoving || isSending || isCategorizing;

  useEffect(() => {
    loadLibrary().then((library) => setState(normalizeLibraryState(library))).catch((error) => setPreviewError(`读取投影库失败，请点击刷新重试：${String(error)}`));

  }, []);

  useEffect(() => {
    const unlistenPromise = listenEvent<{ path?: string }>(projectionLibraryImportedEvent, async (event) => {
      setState(normalizeLibraryState(await loadLibrary()));
      if (event.payload?.path) {
        setCurrentFile(event.payload.path);
      }
    }).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, [setCurrentFile]);

  useEffect(() => {
    const unlistenPromise = listenEvent<{ path?: string; reason?: string }>(projectionLibraryStateChangedEvent, async (event) => {
      if (event.payload?.reason === "embedded-preview" && event.payload.path) {
        const path = event.payload.path;
        const key = windowsPathKey(path);
        previewRevisions.current.set(key, (previewRevisions.current.get(key) || 0) + 1);
        for (const requested of previewRequests.current) if (samePath(requested, path)) previewRequests.current.delete(requested);
        const forget = <T,>(values: Record<string, T>) => Object.fromEntries(Object.entries(values).filter(([candidate]) => !samePath(candidate, path)));
        setProjectionPreviewDataUrls(forget); setProjectionPreviewLoadedPaths(forget); setProjectionPreviewLoadingPaths(forget);
      }
      setState(normalizeLibraryState(await loadLibrary()));
    }).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, []);

  const handleSelect = async () => {
    if (libraryBusy) return;
    setIsOpening(true);
    setPreviewError("");
    try {
      const res = await selectLitematicFile();
      if (res) {
        setCurrentFile(res);
        setInspectedPath(res);
        setState(await addOrUpdateRecord(await loadLibrary(), res));
      }
    } catch (error) {
      setPreviewError(`打开投影失败，请重新选择文件：${String(error)}`);
    } finally {
      setIsOpening(false);
    }
  };

  const handleOpenLocalLibraryFolders = async () => {
    await openLocalLibraryFoldersWithWindowBehavior(() => setShowLocalLibraryFoldersOverlay(true));
  };

  const handleOpenRedenLibrary = async () => {
    await openRedenLibraryWithWindowBehavior(() => setShowRedenLibraryOverlay(true));
  };

  const handleOpenSendDialog = (record: ProjectionRecord) => {
    setSendDialogRecord(record);
    setSendTargetDirectory(pickDefaultSendFolder(record, state.folders));
    setSendTargetFileName(record.fileName);
    setSendOverwrite(false);
    setSendError("");
  };

  const handleCloseSendDialog = () => {
    setSendDialogRecord(null);
    setSendTargetDirectory("");
    setSendTargetFileName("");
    setSendOverwrite(false);
    setSendError("");
  };

  const handleConfirmSend = async () => {
    if (!sendDialogRecord || isSending) return;
    const normalizedFileName = ensureLitematicFileName(sendTargetFileName);
    if (!sendTargetDirectory) {
      setSendError("请选择目标本地库文件夹。");
      return;
    }
    if (!normalizedFileName) {
      setSendError("请输入目标文件名。");
      return;
    }
    setIsSending(true);
    setSendError("");
    try {
      const output = await copyFileToDirectory(sendDialogRecord.path, sendTargetDirectory, normalizedFileName, sendOverwrite);
      const nextState = await addOrUpdateRecord(await loadLibrary(), output.target_path);
      setState(nextState);
      setCurrentFile(output.target_path);
      handleCloseSendDialog();
    } catch (error: any) {
      setSendError(String(error));
    } finally {
      setIsSending(false);
    }
  };

  const handleActivateRecord = (path: string) => {
    console.info(`[NOVA_PROJECTION_ACTIVATION] path=${path} request started`);
    setCurrentFile(path);
    console.info(`[NOVA_PROJECTION_ACTIVATION] path=${path} request succeeded`);
    // Selection is an application-level state change. Refreshing/reordering the
    // library record is a best-effort side effect and must never veto it.
    void loadLibrary()
      .then((library) => activateProjectionRecord(library, path))
      .then(setState)
      .catch((error) => {
        console.error(`[NOVA_PROJECTION_ACTIVATION] path=${path} library refresh failed: ${String(error)}`);
        setPreviewError(`投影已激活，但最近记录刷新失败：${String(error)}`);
      });
  };

  const handleRefresh = async () => {
    if (libraryBusy) return;
    setIsRefreshing(true);
    setRefreshProgress(0);
    setPreviewError("");
    try {
      const latest = await loadLibrary();
      const newState = { ...latest, records: latest.records.map((record) => ({ ...record })) };
      for (const [index, rec] of newState.records.entries()) {
        const exists = await checkFileExists(rec.path);
        if (!exists) {
          rec.status = "missing";
        } else {
          const analysis = await analyzeFile(rec.path);
          Object.assign(rec, analysis);
          rec.lastAnalyzedAt = Date.now();
        }
        setRefreshProgress(index + 1);
      }
      await saveLibrary(newState);
      setState(newState);
    } catch (error) {
      setPreviewError(`刷新失败，原有列表已保留。请检查文件后重试：${String(error)}`);
    } finally {
      setIsRefreshing(false);
    }
  };

  const handleRemove = async () => {
    if (!removeRecords.length || isRemoving) return;
    setIsRemoving(true);
    setRemoveError("");
    try {
      const latest = await loadLibrary();
      const paths = new Set(removeRecords.map((record) => windowsPathKey(record.path)));
      const newState = trashLibraryRecords(latest, removeRecords);
      await saveLibrary(newState);
      setState(newState);
      setSelectedPaths((current) => current.filter((path) => !paths.has(windowsPathKey(path))));
      setFolderSearchRecords((current) => current.filter((record) => !paths.has(windowsPathKey(record.path))));
      if (paths.has(windowsPathKey(currentFile))) setCurrentFile("");
      if (paths.has(windowsPathKey(inspectedPath))) setInspectedPath("");
      setRemoveRecords([]);
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error) {
      setRemoveError(`移除失败，请重试：${String(error)}`);
    } finally {
      setIsRemoving(false);
    }
  };

  const handleGeneratePreview = async (record: ProjectionRecord) => {
    setPreviewError("");
    setGeneratingPreviewPath(record.path);
    try {
      const output = await generatePreviewImage(record.path, "fast_experimental");
      const newState = await setRecordPreview(await loadLibrary(), record.path, output.preview_path);
      setState(newState);
      setPreviewDataUrls((current) => ({ ...current, [record.path]: output.data_url }));
    } catch (err: any) {
      setPreviewError(String(err));
    } finally {
      setGeneratingPreviewPath("");
    }
  };

  const ensureProjectionPreview = async (record: ProjectionRecord) => {
    if (record.status === "missing" || previewRequests.current.has(record.path)) return;
    const key = windowsPathKey(record.path);
    const revision = previewRevisions.current.get(key) || 0;
    previewRequests.current.add(record.path);
    setProjectionPreviewLoadingPaths((current) => ({ ...current, [record.path]: true }));
    try {
      const output = await readProjectionPreviewImage(record.path);
      if ((previewRevisions.current.get(key) || 0) !== revision) return;
      setProjectionPreviewDataUrls((current) => ({ ...current, [record.path]: output?.data_url || "" }));
    } catch {
      if ((previewRevisions.current.get(key) || 0) !== revision) return;
      setProjectionPreviewDataUrls((current) => ({ ...current, [record.path]: "" }));
    } finally {
      if ((previewRevisions.current.get(key) || 0) === revision) {
        setProjectionPreviewLoadedPaths((current) => ({ ...current, [record.path]: true }));
        setProjectionPreviewLoadingPaths((current) => ({ ...current, [record.path]: false }));
      }
    }
  };

  const handleDrop = async (targetPath: string) => {
    if (libraryBusy || !dragPaths.length || hasPath(dragPaths, targetPath) || sortMode !== "manual") return;
    const moving = new Set(dragPaths.map(windowsPathKey));
    const latest = await loadLibrary();
    const ordered = [...latest.records].sort((a, b) => (a.sort_index ?? 0) - (b.sort_index ?? 0));
    const moved = ordered.filter((record) => moving.has(windowsPathKey(record.path)));
    const remaining = ordered.filter((record) => !moving.has(windowsPathKey(record.path)));
    const to = remaining.findIndex((record) => samePath(record.path, targetPath));
    if (!moved.length || to < 0) return;
    remaining.splice(to, 0, ...moved);
    setState(await reorderLibraryRecords(latest, remaining.map((record) => record.path)));
    setDragPaths([]);
  };

  const commitCategories = async (operation: (latest: LibraryState) => LibraryState) => {
    if (libraryBusy) return;
    setIsCategorizing(true); setCategoryError("");
    try {
      const next = operation(normalizeLibraryState(await loadLibrary()));
      await saveLibrary(next); setState(next);
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error) { const message = String(error); setCategoryError(message); throw error; }
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
    if (dialog.kind === "delete") {
      const ids = categoryDescendants(state, dialog.id);
      const paths = state.records.filter((record) => record.categoryIds?.some((id) => ids.has(id))).map((record) => record.path);
      setSelectedPaths((current) => current.filter((path) => !hasPath(paths, path)));
      setFolderSearchRecords((current) => current.filter((record) => !hasPath(paths, record.path)));
      if (hasPath(paths, currentFile)) setCurrentFile("");
      if (hasPath(paths, inspectedPath)) setInspectedPath("");
    }
    if (dialog.kind === "create" && categoryParentDraft) setExpandedCategories((current) => [...new Set([...current, categoryParentDraft])]);
    setCategoryDialog(null);
  };
  const moveRecords = async (paths: string[], id: string) => {
    if (!paths.length) return;
    await commitCategories((latest) => {
      const byPath = new Map(latest.records.map((record) => [windowsPathKey(record.path), record]));
      for (const record of folderSearchRecords) if (hasPath(paths, record.path) && !byPath.has(windowsPathKey(record.path))) byPath.set(windowsPathKey(record.path), record);
      return moveLibraryRecordsToCategory({ ...latest, records: [...byPath.values()] }, paths, id || null);
    });
    setMoveDialogPaths([]);
  };
  const openMoveDialog = (paths: string[]) => { setMoveDialogPaths(paths); setMoveCategoryId(""); setCategoryError(""); };
  const toggleSelected = (path: string, checked: boolean) => {
    setSelectedPaths((current) => checked ? uniquePaths([...current, path]) : current.filter((entry) => !samePath(entry, path)));
    if (checked) setInspectedPath(path);
  };
  const categoryDropProps = (id: string) => ({
    onDragOver: (event: React.DragEvent) => { if (!dragPaths.length || libraryBusy) return; event.preventDefault(); event.dataTransfer.dropEffect = "move"; setDropTarget(id || "untagged"); },
    onDragLeave: () => setDropTarget(""),
    onDrop: (event: React.DragEvent) => { event.preventDefault(); event.stopPropagation(); void moveRecords(dragPaths, id).catch((error) => setPreviewError(`移动分类失败：${String(error)}`)); },
  });
  const categories = state.categories || [];
  const categoryOptions = useMemo(() => categories.map((category) => ({ value: category.id, label: categoryPath(state, category.id) })), [state.categories]);
  const selectedCategoryIds = useMemo(() => categoryDescendants(state, categoryFilter), [state.categories, categoryFilter]);
  const toggleExpanded = (id: string) => setExpandedCategories((current) => current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id]);
  const renderCategoryTree = (parentId: string | null, depth = 1): React.ReactNode => (
    <ul className="library-category-tree" role={parentId ? "group" : "tree"} aria-label={parentId ? undefined : "分类树"}>
      {categories.filter((entry) => entry.parentId === parentId).sort((a, b) => a.sort_index - b.sort_index).map((category) => {
        const hasChildren = categories.some((item) => item.parentId === category.id); const expanded = expandedCategories.includes(category.id);
        const descendants = categoryDescendants(state, category.id);
        return <li key={category.id} role="none"><ContextMenu.Root><ContextMenu.Trigger asChild><div className="library-category-row">
          {hasChildren ? <Button className="library-category-expander" variant="ghost" size="icon-sm" disabled={libraryBusy} aria-label={`${expanded ? "收起" : "展开"} ${category.name}`} onClick={() => toggleExpanded(category.id)}>{expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</Button> : <span className="library-category-expander-spacer" />}
          <Button variant="ghost" className="library-category" role="treeitem" aria-level={depth} aria-expanded={hasChildren ? expanded : undefined} aria-selected={categoryFilter === category.id} aria-pressed={categoryFilter === category.id} data-category-tree-node={category.id} disabled={libraryBusy}
            data-drop-target={dropTarget === category.id || undefined} onClick={() => { setCategoryFilter(category.id); setUntaggedOnly(false); }} onKeyDown={(event) => {
              const treeNodes = () => Array.from(event.currentTarget.closest('[role="tree"]')?.querySelectorAll<HTMLButtonElement>('[data-category-tree-node]') || []);
              const focusCategory = (id?: string | null) => treeNodes().find((node) => node.dataset.categoryTreeNode === id)?.focus();
              if (event.key === "ArrowRight" && hasChildren) { event.preventDefault(); if (!expanded) toggleExpanded(category.id); else focusCategory(categories.filter((item) => item.parentId === category.id).sort((a, b) => a.sort_index - b.sort_index)[0]?.id); }
              else if (event.key === "ArrowLeft") { event.preventDefault(); if (hasChildren && expanded) toggleExpanded(category.id); else focusCategory(category.parentId); }
              else if (["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) { event.preventDefault(); const nodes = treeNodes(); const index = nodes.indexOf(event.currentTarget); nodes[event.key === "Home" ? 0 : event.key === "End" ? nodes.length - 1 : Math.max(0, Math.min(nodes.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)))]?.focus(); }
            }} {...categoryDropProps(category.id)}><CategoryIcon icon={category.icon} /><span title={categoryPath(state, category.id)}>{category.name}</span><small>{state.records.filter((record) => record.categoryIds?.some((id) => descendants.has(id))).length}</small></Button>
        </div></ContextMenu.Trigger><ContextMenu.Portal><ContextMenu.Content className="library-category-menu" collisionPadding={8}>
          <ContextMenu.Item className="library-category-menu-item" disabled={libraryBusy} onSelect={() => openCategoryDialog("create", category)}>新建子分类</ContextMenu.Item>
          <ContextMenu.Item className="library-category-menu-item" disabled={libraryBusy} onSelect={() => openCategoryDialog("rename", category)}>重命名</ContextMenu.Item>
          <ContextMenu.Item className="library-category-menu-item" disabled={libraryBusy} onSelect={() => openCategoryDialog("move", category)}>移动分类…</ContextMenu.Item>
          <ContextMenu.Item className="library-category-menu-item" disabled={libraryBusy} onSelect={() => { setIconCategoryId(category.id); setCategoryError(""); }}>更换图标…</ContextMenu.Item>
          <ContextMenu.Separator className="library-category-menu-separator" />
          <ContextMenu.Item className="library-category-menu-item" disabled={libraryBusy} onSelect={() => openCategoryDialog("delete", category)}>删除分类…</ContextMenu.Item>
        </ContextMenu.Content></ContextMenu.Portal></ContextMenu.Root>{hasChildren && expanded && renderCategoryTree(category.id, depth + 1)}</li>;
      })}
    </ul>
  );

  const filteredRecords = useMemo(() => {
    const recordsByPath = new Map<string, ProjectionRecord>();
    for (const record of state.records) recordsByPath.set(windowsPathKey(record.path), record);
    if (search) {
      for (const record of folderSearchRecords) {
        const key = windowsPathKey(record.path);
        recordsByPath.set(key, recordsByPath.get(key) || record);
      }
    }
    let records = [...recordsByPath.values()].filter((r) => {
      if (search) {
        const q = search.toLowerCase();
        if (!`${r.displayName} ${r.fileName} ${r.path} ${r.author} ${r.description} ${(r.categoryIds || []).map((id) => categoryPath(state, id)).join(" ")}`.toLowerCase().includes(q)) return false;
      }
      if (categoryFilter && !(r.categoryIds || []).some((id) => selectedCategoryIds.has(id))) return false;
      if (untaggedOnly && r.categoryIds?.length) return false;
      if (statusFilter && r.status !== statusFilter) return false;
      return true;
    });
    if (sortMode === "manual") {
      records = [...records].sort((a, b) => (a.sort_index ?? 0) - (b.sort_index ?? 0));
    } else if (sortMode === "recent_analyzed") {
      records = [...records].sort((a, b) => b.lastAnalyzedAt - a.lastAnalyzedAt);
    } else if (sortMode === "blocks") {
      records = [...records].sort((a, b) => b.totalBlocks - a.totalBlocks);
    } else if (sortMode === "name") {
      records = [...records].sort((a, b) => a.displayName.localeCompare(b.displayName));
    }
    return records;
  }, [state.records, state.categories, selectedCategoryIds, folderSearchRecords, search, categoryFilter, untaggedOnly, statusFilter, sortMode]);

  const pageCount = pageSize === 0 ? 1 : Math.max(1, Math.ceil(filteredRecords.length / pageSize));
  const currentPage = Math.min(page, pageCount);
  const visibleRecords = useMemo(() => {
    if (pageSize === 0) return filteredRecords;
    const start = (currentPage - 1) * pageSize;
    return filteredRecords.slice(start, start + pageSize);
  }, [filteredRecords, currentPage, pageSize]);
  const issueCount = state.records.filter((record) => record.status !== "ok").length;
  const availableRecords = useMemo(() => {
    const entries = new Map(folderSearchRecords.map((record) => [windowsPathKey(record.path), record]));
    for (const record of state.records) entries.set(windowsPathKey(record.path), record);
    return [...entries.values()];
  }, [state.records, folderSearchRecords]);
  const selectedRecords = availableRecords.filter((record) => hasPath(selectedPaths, record.path));
  const inspectedRecord = availableRecords.find((record) => samePath(record.path, inspectedPath)) || null;
  const allPageSelected = visibleRecords.length > 0 && visibleRecords.every((record) => hasPath(selectedPaths, record.path));
  const somePageSelected = visibleRecords.some((record) => hasPath(selectedPaths, record.path));
  const activeCategory = untaggedOnly ? "未分类" : categoryPath(state, categoryFilter) || "全部投影";
  const detailPreviewUrl = inspectedRecord
    ? (detailImageKind === "auto" ? previewDataUrls[inspectedRecord.path] || projectionPreviewDataUrls[inspectedRecord.path] : projectionPreviewDataUrls[inspectedRecord.path]) || ""
    : "";

  useEffect(() => {
    let cancelled = false;
    const records = inspectedRecord && !visibleRecords.includes(inspectedRecord) ? [...visibleRecords, inspectedRecord] : visibleRecords;
    // Read only the visible page and inspector. Opening the library never renders every projection.
    void (async () => {
      for (const record of records) {
        if (cancelled) break;
        if (!projectionPreviewLoadedPaths[record.path]) await ensureProjectionPreview(record);
        const imagePath = record.preview_image_path || record.previewPath;
        if (imagePath && previewDataUrls[record.path] === undefined) {
          let dataUrl = "";
          try { dataUrl = await readImageBase64(imagePath); } catch { /* Embedded preview remains available. */ }
          if (!cancelled) setPreviewDataUrls((current) => ({ ...current, [record.path]: dataUrl }));
        }
      }
    })();
    return () => { cancelled = true; };
  }, [visibleRecords, inspectedRecord]);

  useEffect(() => {
    setPage(1);
  }, [search, categoryFilter, untaggedOnly, statusFilter, sortMode, pageSize]);

  useEffect(() => {
    const trimmedSearch = search.trim();
    if (!trimmedSearch) {
      setFolderSearchRecords([]);
      setFolderSearchError("");
      setIsFolderSearchLoading(false);
      return;
    }
    let cancelled = false;
    setIsFolderSearchLoading(true);
    setFolderSearchError("");
    searchLocalLibraryFolderRecords(state, trimmedSearch)
      .then((records) => {
        if (!cancelled) setFolderSearchRecords(records);
      })
      .catch((error) => {
        if (!cancelled) {
          setFolderSearchRecords([]);
          setFolderSearchError(String(error));
        }
      })
      .finally(() => {
        if (!cancelled) setIsFolderSearchLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [state.folders, search]);

  useEffect(() => {
    if (page > pageCount) setPage(pageCount);
  }, [page, pageCount]);

  return (
    <div className="nova-page library-page">
      <header className="library-toolbar">
        <h2>投影库</h2>
        <Button variant="default" onClick={handleSelect} disabled={libraryBusy} aria-busy={isOpening}><FolderPlus size={16} />{isOpening ? "正在导入…" : "导入投影"}</Button>
        <Button onClick={() => handleOpenRedenLibrary().catch((error) => setPreviewError(`打开在线投影库失败：${String(error)}`))}><Download size={16} />下载投影</Button>
        <div className="library-search"><Search size={16} aria-hidden="true" /><Input type="search" aria-label="搜索投影" placeholder="搜索名称、作者、分类…" value={search} onChange={(event) => setSearch(event.target.value)} /></div>
        <Button variant="ghost" onClick={() => handleOpenLocalLibraryFolders().catch((error) => setPreviewError(`打开本地库文件夹失败：${String(error)}`))}><FolderOpen size={16} />管理文件夹</Button>
        <Button variant="ghost" size="icon" aria-label={isRefreshing ? "正在刷新投影库" : "刷新投影库"} title="刷新投影库" disabled={libraryBusy} aria-busy={isRefreshing} onClick={handleRefresh}><RefreshCw size={16} /></Button>
      </header>
      {previewError && <div className="library-message" role="alert">{previewError}<Button variant="ghost" size="icon-sm" aria-label="关闭错误提示" onClick={() => setPreviewError("")}><X size={14} /></Button></div>}
      {folderSearchError && <div className="library-message" role="alert">文件夹搜索失败：{folderSearchError}</div>}
      {isRefreshing && <div className="library-progress" role="status">刷新中 {refreshProgress} / {state.records.length}</div>}

      <div className="library-workspace">
        <aside className="library-categories" aria-label="投影分类">
          <div className="library-pane-heading"><h3>我的投影</h3><span>{state.records.length}</span></div>
          <div className="library-category-tools"><Button size="sm" variant="ghost" disabled={libraryBusy} onClick={() => openCategoryDialog("create")}><FolderPlus size={14} />新建分类</Button></div>
          <ScrollArea className="library-category-scroll">
            <nav className="library-category-list" aria-label="分类筛选">
              <Button variant="ghost" className="library-category" aria-pressed={!categoryFilter && !untaggedOnly} onClick={() => { setCategoryFilter(""); setUntaggedOnly(false); }}><LayoutGrid size={16} /><span>全部投影</span><small>{state.records.length}</small></Button>
              <Button variant="ghost" className="library-category" aria-pressed={untaggedOnly} data-drop-target={dropTarget === "untagged" || undefined} onClick={() => { setCategoryFilter(""); setUntaggedOnly(true); }} {...categoryDropProps("")}><Box size={16} /><span>未分类</span><small>{state.records.filter((record) => !record.categoryIds?.length).length}</small></Button>
              {renderCategoryTree(null)}
            </nav>
          </ScrollArea>
          <Button variant="ghost" className="library-category library-trash-button" aria-haspopup="dialog" onClick={() => { setDiscardTrashId(null); setCategoryError(""); setTrashOpen(true); }}><Trash2 size={16} /><span>回收站</span><small>{state.trash?.length || 0}</small></Button>
        </aside>

        <section className="library-collection" aria-label="投影卡片">
          <div className="library-collection-toolbar">
            <label className="library-select-all"><Checkbox checked={allPageSelected ? true : somePageSelected ? "indeterminate" : false} disabled={!visibleRecords.length || libraryBusy} aria-label="选择本页全部投影" onCheckedChange={(checked) => setSelectedPaths((current) => checked === true ? uniquePaths([...current, ...visibleRecords.map((record) => record.path)]) : current.filter((path) => !visibleRecords.some((record) => samePath(record.path, path))))} /><span>{selectedRecords.length ? `已选 ${selectedRecords.length}` : activeCategory}</span></label>
            <span className="library-result-count" role="status">{filteredRecords.length} 项{isFolderSearchLoading ? " · 搜索中…" : ""}</span>
            <div className="library-filter-control"><Dropdown ariaLabel="筛选文件状态" value={statusFilter} onChange={setStatusFilter} options={[{ value: "", label: "全部状态" }, { value: "ok", label: "正常" }, { value: "missing", label: "文件丢失" }, { value: "parse_error", label: "解析失败" }]} /></div>
            <div className="library-filter-control"><Dropdown ariaLabel="投影排序" value={sortMode} onChange={setSortMode} options={[{ value: "manual", label: "手动排序" }, { value: "recent_analyzed", label: "最近分析" }, { value: "blocks", label: "方块数量" }, { value: "name", label: "名称 A–Z" }]} /></div>
          </div>
          {selectedRecords.length > 0 && <div className="library-batch-toolbar"><Button size="sm" onClick={() => openMoveDialog(selectedRecords.map((record) => record.path))} disabled={libraryBusy}><Folder size={14} />移动到分类…</Button><Button size="sm" variant="ghost" disabled={libraryBusy} onClick={() => { setRemoveError(""); setRemoveRecords(selectedRecords); }}><Trash2 size={14} />移入回收站</Button><Button size="sm" variant="ghost" onClick={() => setSelectedPaths([])}>取消选择</Button></div>}
          <ScrollArea className="library-grid-scroll">
            {visibleRecords.length > 0 ? <div className="library-card-grid">{visibleRecords.map((record) => <ProjectionCard key={record.path} record={record} isCurrent={samePath(currentFile, record.path)} isInspected={!!inspectedRecord && samePath(inspectedRecord.path, record.path)} checked={hasPath(selectedPaths, record.path)} busy={libraryBusy}
              previewUrl={previewDataUrls[record.path] || projectionPreviewDataUrls[record.path] || ""} loading={!!projectionPreviewLoadingPaths[record.path]}
              onInspect={() => { setInspectedPath(record.path); setDetailImageKind("auto"); }} onToggle={(checked) => toggleSelected(record.path, checked)} onActivate={() => handleActivateRecord(record.path)}
              onDragStart={(event) => { const paths = hasPath(selectedPaths, record.path) ? selectedRecords.map((entry) => entry.path) : [record.path]; setDragPaths(paths); event.dataTransfer.effectAllowed = "all"; event.dataTransfer.setData("application/x-nova-library-records", JSON.stringify(paths)); }}
              onDragEnd={() => { setDragPaths([]); setDropTarget(""); }} onDrop={(event) => { event.preventDefault(); event.stopPropagation(); handleDrop(record.path).catch((error) => setPreviewError(`排序失败：${String(error)}`)); }}
            />)}</div> : <div className="library-empty-state"><Box size={40} aria-hidden="true" /><h3>{state.records.length || search ? "没有找到投影" : "把你的第一份投影放进来"}</h3><p>{state.records.length || search ? "换个关键词或清除筛选，再试一次。" : "导入本地 .litematic，或从在线投影库下载。"}</p><div className="library-empty-actions">{state.records.length || search ? <Button onClick={() => { setSearch(""); setCategoryFilter(""); setUntaggedOnly(false); setStatusFilter(""); }}>清除筛选</Button> : <Button variant="default" onClick={handleSelect} disabled={libraryBusy}><Plus size={16} />导入投影</Button>}</div></div>}
          </ScrollArea>
          <footer className="library-collection-footer"><span>{issueCount ? `${issueCount} 项需关注` : "双击卡片设为当前投影"}</span><div className="library-page-limit"><Dropdown ariaLabel="每页条目" value={String(pageSize)} onChange={(value) => setPageSize(Number(value))} options={[20, 50, 100, 200, 0].map((count) => ({ value: String(count), label: count ? `${count} 项 / 页` : "全部显示" }))} /></div><Button size="sm" variant="ghost" disabled={currentPage <= 1 || pageSize === 0} onClick={() => setPage(currentPage - 1)}>上一页</Button><span>{currentPage} / {pageCount}</span><Button size="sm" variant="ghost" disabled={currentPage >= pageCount || pageSize === 0} onClick={() => setPage(currentPage + 1)}>下一页</Button></footer>
        </section>

        <aside className="library-inspector" aria-label="投影详情">
          <div className="library-pane-heading"><h3>详情</h3>{inspectedRecord && <Button variant="ghost" size="icon-sm" aria-label="关闭投影详情" onClick={() => setInspectedPath("")}><X size={15} /></Button>}</div>
          <ScrollArea className="library-inspector-scroll">
            {inspectedRecord ? <div className="library-detail-content">
              <div className="library-detail-preview">{detailPreviewUrl && failedDetailImage !== detailPreviewUrl ? <img src={detailPreviewUrl} alt={`${inspectedRecord.displayName} 预览`} onError={() => setFailedDetailImage(detailPreviewUrl)} /> : <span className="library-preview-placeholder"><ImageOff size={36} /><span>{projectionPreviewLoadingPaths[inspectedRecord.path] ? "预览加载中…" : "暂无预览图"}</span></span>}</div>
              <h3 className="library-detail-name">{inspectedRecord.displayName || inspectedRecord.fileName}</h3>
              <p className="library-detail-author">{inspectedRecord.author ? `作者 · ${inspectedRecord.author}` : "未填写作者"}</p>
              {recordStatusLabel(inspectedRecord) && <div className="library-detail-error" role="status">{recordStatusLabel(inspectedRecord)}{inspectedRecord.lastError && <p>{inspectedRecord.lastError}</p>}</div>}
              <div className="library-detail-actions"><Button variant="default" disabled={libraryBusy || inspectedRecord.status !== "ok" || samePath(currentFile, inspectedRecord.path)} onClick={() => handleActivateRecord(inspectedRecord.path)}>{samePath(currentFile, inspectedRecord.path) ? "当前投影" : "设为当前投影"}</Button><Button disabled={libraryBusy || inspectedRecord.status === "missing"} onClick={() => { setCurrentFile(inspectedRecord.path); setRoute("properties"); }}>查看属性</Button></div>
              <dl className="library-detail-facts">
                <div><dt>选区名</dt><dd>{inspectedRecord.regionNames === undefined ? "未读取（刷新或查看属性后补齐）" : inspectedRecord.regionNames.length ? inspectedRecord.regionNames.map((name, index) => <div key={index}>{name || "（空名称）"}</div>) : "无选区"}</dd></div>
                {typeof inspectedRecord.internalName === "string" && <div><dt>内部名称</dt><dd>{inspectedRecord.internalName || "未填写"}</dd></div>}
                <div><dt>方块数量</dt><dd>{inspectedRecord.totalBlocks.toLocaleString()}</dd></div><div><dt>尺寸</dt><dd>{inspectedRecord.enclosingSize.x} × {inspectedRecord.enclosingSize.y} × {inspectedRecord.enclosingSize.z}</dd></div><div><dt>选区数量</dt><dd>{inspectedRecord.regionCount}</dd></div><div><dt>文件大小</dt><dd>{formatSize(inspectedRecord.fileSize)}</dd></div><div><dt>最近分析</dt><dd>{formatDate(inspectedRecord.lastAnalyzedAt)}</dd></div>
              </dl>
              {inspectedRecord.description && <p className="library-detail-description">{inspectedRecord.description}</p>}
              <div className="library-detail-section"><h4>分类</h4><div className="library-detail-categories">{inspectedRecord.categoryIds?.length ? inspectedRecord.categoryIds.map((id) => <span className="library-category-membership" key={id}>{categoryPath(state, id)}</span>) : <span className="muted">未分类</span>}</div><Button size="sm" disabled={libraryBusy} onClick={() => openMoveDialog([inspectedRecord.path])}>移动到分类…</Button></div>
              <div className="library-detail-section"><h4>预览图</h4><div className="library-detail-actions">{previewDataUrls[inspectedRecord.path] && projectionPreviewDataUrls[inspectedRecord.path] && <Button size="sm" onClick={() => setDetailImageKind(detailImageKind === "auto" ? "preview" : "auto")}>{detailImageKind === "auto" ? "查看内嵌图" : "查看渲染图"}</Button>}<Button size="sm" disabled={libraryBusy || !!generatingPreviewPath || inspectedRecord.status !== "ok"} aria-busy={generatingPreviewPath === inspectedRecord.path} onClick={() => handleGeneratePreview(inspectedRecord)}>{generatingPreviewPath === inspectedRecord.path ? "正在生成…" : "生成预览图"}</Button></div></div>
              <div className="library-detail-section"><h4>文件</h4><p className="library-detail-path" title={displayFilePath(inspectedRecord.path)}>{displayFilePath(inspectedRecord.path)}</p><div className="library-detail-actions"><Button size="sm" onClick={() => openFileParentDir(inspectedRecord.path).catch((error) => setPreviewError(`打开文件夹失败：${String(error)}`))}>打开文件夹</Button><Button size="sm" disabled={libraryBusy || inspectedRecord.status === "missing"} onClick={() => handleOpenSendDialog(inspectedRecord)}>复制到…</Button><Button size="sm" variant="ghost" disabled={libraryBusy} onClick={() => { setRemoveError(""); setRemoveRecords([inspectedRecord]); }}><Trash2 size={14} />移入回收站</Button></div></div>
            </div> : <div className="library-inspector-empty"><Box size={32} /><p>点击卡片查看详情</p></div>}
          </ScrollArea>
        </aside>
      </div>
      {removeRecords.length > 0 && <Dialog title={`移入回收站${removeRecords.length > 1 ? ` ${removeRecords.length} 项` : "此投影"}？`} subtitle="可从 Nova 回收站恢复，原文件保持不变。" width="sm" busy={isRemoving} onClose={() => setRemoveRecords([])} footer={<><Button onClick={() => setRemoveRecords([])} disabled={isRemoving}>保留</Button><Button variant="destructive" onClick={handleRemove} disabled={isRemoving} aria-busy={isRemoving}>{isRemoving ? "正在移除…" : "移入回收站"}</Button></>}><ul className="library-remove-list">{removeRecords.slice(0, 8).map((record) => <li key={record.path}>{record.displayName || record.fileName}</li>)}</ul>{removeRecords.length > 8 && <p>以及另外 {removeRecords.length - 8} 项</p>}{removeError && <div className="dialog-message dialog-message-error" role="alert">{removeError}</div>}</Dialog>}
      {moveDialogPaths.length > 0 && <Dialog title="移动到分类" subtitle={`${moveDialogPaths.length} 个投影，原文件保持原位。`} width="sm" busy={isCategorizing} onClose={() => setMoveDialogPaths([])} footer={<><Button disabled={isCategorizing} onClick={() => setMoveDialogPaths([])}>取消</Button><Button variant="default" disabled={isCategorizing} onClick={() => void moveRecords(moveDialogPaths, moveCategoryId).catch(() => undefined)}>移动</Button></>}><Dropdown ariaLabel="目标分类" value={moveCategoryId} onChange={setMoveCategoryId} options={[{ value: "", label: "未分类" }, ...categoryOptions]} />{categoryError && <p className="dialog-message dialog-message-error" role="alert">{categoryError}</p>}</Dialog>}
      {categoryDialog && <Dialog title={categoryDialog.kind === "create" ? "新建分类" : categoryDialog.kind === "rename" ? "重命名分类" : categoryDialog.kind === "move" ? "移动分类" : "删除分类？"} subtitle={categoryDialog.kind === "delete" ? "此分类、子分类和其中的投影移入回收站，原文件保持不变。" : undefined} width="sm" busy={isCategorizing} onClose={() => setCategoryDialog(null)} footer={<><Button disabled={isCategorizing} onClick={() => setCategoryDialog(null)}>取消</Button><Button variant={categoryDialog.kind === "delete" ? "destructive" : "default"} disabled={isCategorizing || (["create", "rename"].includes(categoryDialog.kind) && !categoryNameDraft.trim())} onClick={() => void confirmCategory().catch(() => undefined)}>{categoryDialog.kind === "delete" ? "删除分类" : "保存"}</Button></>}>
        {["create", "rename"].includes(categoryDialog.kind) && <div className="library-category-form"><label htmlFor="library-category-name">分类名称</label><Input id="library-category-name" autoFocus maxLength={80} disabled={isCategorizing} value={categoryNameDraft} onChange={(event) => setCategoryNameDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && categoryNameDraft.trim()) { event.preventDefault(); void confirmCategory().catch(() => undefined); } }} /></div>}
        {["create", "move"].includes(categoryDialog.kind) && <Dropdown ariaLabel="上级分类" value={categoryParentDraft} onChange={setCategoryParentDraft} options={[{ value: "", label: "分类根目录" }, ...categoryOptions.filter((item) => categoryDialog.kind !== "move" || !categoryDescendants(state, categoryDialog.id).has(item.value))]} />}
        {categoryError && <p className="dialog-message dialog-message-error" role="alert">{categoryError}</p>}
      </Dialog>}
      {trashOpen && <Dialog title="回收站" width="lg" busy={isCategorizing} onClose={() => setTrashOpen(false)}
        footer={<><Button variant="destructive" disabled={isCategorizing || !state.trash?.length || discardTrashId !== null} onClick={() => setDiscardTrashId("")}>清空回收站…</Button><Button disabled={isCategorizing} onClick={() => setTrashOpen(false)}>关闭</Button></>}>
        {categoryError && <p role="alert" className="dialog-message dialog-message-error">{categoryError}</p>}
        {discardTrashId !== null ? <div className="library-trash-confirm">
          <p>{discardTrashId ? "永久移除此项 Nova 记录？" : "清空全部 Nova 回收站记录？"}原投影文件不会删除。移除后不能从回收站恢复。</p>
          <Button disabled={isCategorizing} onClick={() => setDiscardTrashId(null)}>取消</Button>
          <Button variant="destructive" disabled={isCategorizing} onClick={() => void commitCategories((latest) => discardLibraryTrash(latest, discardTrashId || undefined)).then(() => setDiscardTrashId(null)).catch(() => undefined)}>永久移除记录</Button>
        </div> : <ul className="library-trash-list">{state.trash?.length ? state.trash.map((entry) => <li key={entry.id}>
          {entry.kind === "category" ? <Folder size={18} /> : <Box size={18} />}
          <div><strong>{entry.name}</strong><span>{entry.records.length} 个投影 · {formatDate(entry.deletedAt)}</span>
          <small>{[...new Set(entry.records.flatMap((record) => record.categoryIds || []).map((id) => categoryPath({ ...state, categories: entry.categories }, id)))].filter(Boolean).join("；")}</small></div>
          <Button size="sm" disabled={isCategorizing} onClick={() => void commitCategories((latest) => restoreLibraryTrash(latest, entry.id)).catch(() => undefined)}>恢复</Button>
          <Button size="sm" variant="ghost" disabled={isCategorizing} onClick={() => setDiscardTrashId(entry.id)}>永久移除…</Button>
        </li>) : <li className="muted">回收站为空</li>}</ul>}
      </Dialog>}
      {iconCategoryId && categories.find((category) => category.id === iconCategoryId) && <CategoryIconDialog name={categories.find((category) => category.id === iconCategoryId)!.name} icon={categories.find((category) => category.id === iconCategoryId)!.icon} busy={isCategorizing} onClose={() => setIconCategoryId("")} onSubmit={async (icon: LibraryCategoryIcon) => { await commitCategories((latest) => updateLibraryCategory(latest, iconCategoryId, { icon })); setIconCategoryId(""); }} />}
      {sendDialogRecord ? (
        <SendProjectionDialog
          record={sendDialogRecord}
          folders={state.folders}
          targetDirectory={sendTargetDirectory}
          targetFileName={sendTargetFileName}
          overwrite={sendOverwrite}
          isSending={isSending}
          error={sendError}
          onTargetDirectoryChange={setSendTargetDirectory}
          onTargetFileNameChange={setSendTargetFileName}
          onOverwriteChange={setSendOverwrite}
          onClose={isSending ? () => undefined : handleCloseSendDialog}
          onSubmit={handleConfirmSend}
        />
      ) : null}
      {showLocalLibraryFoldersOverlay ? <LocalLibraryFoldersDialog onClose={() => setShowLocalLibraryFoldersOverlay(false)} /> : null}
      {showRedenLibraryOverlay ? <RedenLibraryDialog onClose={() => setShowRedenLibraryOverlay(false)} /> : null}
    </div>
  );
}

