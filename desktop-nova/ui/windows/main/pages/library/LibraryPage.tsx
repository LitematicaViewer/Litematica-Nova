import React, { useEffect, useMemo, useRef, useState } from "react";
import { Box, Download, FolderOpen, FolderPlus, ImageOff, LayoutGrid, Plus, RefreshCw, Search, Tag, Trash2, X } from "lucide-react";
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
  loadLibrary,
  LocalLibraryFolder,
  ProjectionRecord,
  reorderLibraryRecords,
  saveLibrary,
  searchLocalLibraryFolderRecords,
  setRecordPreview,
} from "../../../../../src/business/facade";
import { SendProjectionDialog } from "./sendDialog";
import { Dialog } from "../../../../components/Dialog";
import "./library.css";
import { ensureLitematicFileName } from "./function";
import { checkFileExists, generatePreviewImage, openFileParentDir, readImageBase64, readProjectionPreviewImage, selectLitematicFile } from "../../../../../src/business/facade";
import { LocalLibraryFoldersDialog, openLocalLibraryFoldersWithWindowBehavior } from "../../../local_library_folders";
import { RedenLibraryDialog, openRedenLibraryWithWindowBehavior } from "../../../reden_library";
import { loadUserConfigMigratingLocalStorage } from "../../../../../src/business/facade";
import { DisplayMode, normalizeDisplayMode } from "../../../../../src/business/facade";
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
  return lastSeparatorIndex >= 0 ? normalized.slice(0, lastSeparatorIndex) : "";
}

function pickDefaultSendFolder(record: ProjectionRecord, folders: LocalLibraryFolder[]): string {
  const sourceParent = parentDirectory(record.path).toLowerCase();
  return folders.find((folder) => folder.path.toLowerCase() !== sourceParent)?.path || folders[0]?.path || "";
}

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
  const [tagFilter, setTagFilter] = useState("");
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
  const [previewMode, setPreviewMode] = useState<DisplayMode>("fast_experimental");
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
  const [tagDialogPaths, setTagDialogPaths] = useState<string[]>([]);
  const [tagDraft, setTagDraft] = useState("");
  const [isTagging, setIsTagging] = useState(false);
  const [tagError, setTagError] = useState("");
  const [dropTarget, setDropTarget] = useState("");
  const [detailImageKind, setDetailImageKind] = useState<"auto" | "preview">("auto");
  const [failedDetailImage, setFailedDetailImage] = useState("");
  const previewRequests = useRef(new Set<string>());
  const [sendDialogRecord, setSendDialogRecord] = useState<ProjectionRecord | null>(null);
  const [sendTargetDirectory, setSendTargetDirectory] = useState("");
  const [sendTargetFileName, setSendTargetFileName] = useState("");
  const [sendOverwrite, setSendOverwrite] = useState(false);
  const [sendError, setSendError] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [folderSearchRecords, setFolderSearchRecords] = useState<ProjectionRecord[]>([]);
  const [isFolderSearchLoading, setIsFolderSearchLoading] = useState(false);
  const [folderSearchError, setFolderSearchError] = useState("");
  const libraryBusy = isRefreshing || isOpening || isRemoving || isSending || isTagging;

  useEffect(() => {
    loadLibrary().then(setState).catch((error) => setPreviewError(`读取投影库失败，请点击刷新重试：${String(error)}`));
    loadUserConfigMigratingLocalStorage()
      .then((info) => setPreviewMode(normalizeDisplayMode(info.config.preview_mode)))
      .catch(() => setPreviewMode("fast_experimental"));
  }, []);

  useEffect(() => {
    const unlistenPromise = listenEvent<{ path?: string }>(projectionLibraryImportedEvent, async (event) => {
      setState(await loadLibrary());
      if (event.payload?.path) {
        setCurrentFile(event.payload.path);
      }
    }).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, [setCurrentFile]);

  useEffect(() => {
    const unlistenPromise = listenEvent(projectionLibraryStateChangedEvent, async () => {
      setState(await loadLibrary());
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
      const paths = new Set(removeRecords.map((record) => record.path));
      const newState = { ...latest, records: latest.records.filter((record) => !paths.has(record.path)) };
      await saveLibrary(newState);
      setState(newState);
      setSelectedPaths((current) => current.filter((path) => !paths.has(path)));
      setFolderSearchRecords((current) => current.filter((record) => !paths.has(record.path)));
      if (paths.has(currentFile)) setCurrentFile("");
      if (paths.has(inspectedPath)) setInspectedPath("");
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
      const output = await generatePreviewImage(record.path, previewMode);
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
    previewRequests.current.add(record.path);
    setProjectionPreviewLoadingPaths((current) => ({ ...current, [record.path]: true }));
    try {
      const output = await readProjectionPreviewImage(record.path);
      setProjectionPreviewDataUrls((current) => ({ ...current, [record.path]: output?.data_url || "" }));
    } catch {
      setProjectionPreviewDataUrls((current) => ({ ...current, [record.path]: "" }));
    } finally {
      setProjectionPreviewLoadedPaths((current) => ({ ...current, [record.path]: true }));
      setProjectionPreviewLoadingPaths((current) => ({ ...current, [record.path]: false }));
    }
  };

  const handleDrop = async (targetPath: string) => {
    if (libraryBusy || !dragPaths.length || dragPaths.includes(targetPath) || sortMode !== "manual") return;
    const moving = new Set(dragPaths);
    const latest = await loadLibrary();
    const ordered = [...latest.records].sort((a, b) => (a.sort_index ?? 0) - (b.sort_index ?? 0));
    const moved = ordered.filter((record) => moving.has(record.path));
    const remaining = ordered.filter((record) => !moving.has(record.path));
    const to = remaining.findIndex((record) => record.path === targetPath);
    if (!moved.length || to < 0) return;
    remaining.splice(to, 0, ...moved);
    setState(await reorderLibraryRecords(latest, remaining.map((record) => record.path)));
    setDragPaths([]);
  };

  const applyTag = async (paths: string[], tag: string, remove = false) => {
    if (!paths.length || libraryBusy) return;
    const value = tag.trim();
    if (!value && !remove) return;
    setIsTagging(true);
    setTagError("");
    setPreviewError("");
    try {
      const latest = await loadLibrary();
      const byPath = new Map(latest.records.map((record) => [record.path, record]));
      // Search may find a file in a mounted folder that is not yet a library record.
      for (const record of folderSearchRecords) if (paths.includes(record.path) && !byPath.has(record.path)) byPath.set(record.path, record);
      const targets = new Set(paths);
      const next = { ...latest, records: [...byPath.values()].map((record) => {
        if (!targets.has(record.path)) return record;
        const tags = record.tags || [];
        return { ...record, tags: remove ? (value ? tags.filter((entry) => entry !== value) : []) : [...new Set([...tags, value])] };
      }) };
      await saveLibrary(next);
      setState(next);
      setTagDialogPaths([]);
      setTagDraft("");
      void emitEvent(projectionLibraryStateChangedEvent).catch(() => undefined);
    } catch (error) {
      const message = `保存标签失败，请重试：${String(error)}`;
      setTagError(message);
      setPreviewError(message);
    } finally {
      setIsTagging(false);
      setDragPaths([]);
      setDropTarget("");
    }
  };

  const openTagDialog = (paths: string[]) => {
    setTagDialogPaths(paths); setTagDraft(""); setTagError("");
  };
  const toggleSelected = (path: string, checked: boolean) => {
    setSelectedPaths((current) => checked ? [...new Set([...current, path])] : current.filter((entry) => entry !== path));
    if (checked) setInspectedPath(path);
  };
  const tagDropProps = (tag: string) => ({
    onDragOver: (event: React.DragEvent) => {
      if (!dragPaths.length || libraryBusy) return;
      event.preventDefault(); event.dataTransfer.dropEffect = "link"; setDropTarget(tag || "untagged");
    },
    onDragLeave: () => setDropTarget(""),
    onDrop: (event: React.DragEvent) => {
      event.preventDefault(); event.stopPropagation();
      void applyTag(dragPaths, tag, !tag);
    },
  });

  const tagOptions = useMemo(() => {
    return Array.from(new Set(state.records.flatMap((record) => record.tags || []))).sort((a, b) => a.localeCompare(b));
  }, [state.records]);

  const filteredRecords = useMemo(() => {
    const recordsByPath = new Map<string, ProjectionRecord>();
    for (const record of state.records) recordsByPath.set(record.path, record);
    if (search) {
      for (const record of folderSearchRecords) {
        recordsByPath.set(record.path, recordsByPath.get(record.path) || record);
      }
    }
    let records = [...recordsByPath.values()].filter((r) => {
      if (search) {
        const q = search.toLowerCase();
        if (!`${r.displayName} ${r.fileName} ${r.path} ${r.author} ${r.description} ${(r.tags || []).join(" ")}`.toLowerCase().includes(q)) return false;
      }
      if (tagFilter && !(r.tags || []).includes(tagFilter)) return false;
      if (untaggedOnly && r.tags?.length) return false;
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
  }, [state.records, folderSearchRecords, search, tagFilter, untaggedOnly, statusFilter, sortMode]);

  const pageCount = pageSize === 0 ? 1 : Math.max(1, Math.ceil(filteredRecords.length / pageSize));
  const currentPage = Math.min(page, pageCount);
  const visibleRecords = useMemo(() => {
    if (pageSize === 0) return filteredRecords;
    const start = (currentPage - 1) * pageSize;
    return filteredRecords.slice(start, start + pageSize);
  }, [filteredRecords, currentPage, pageSize]);
  const issueCount = state.records.filter((record) => record.status !== "ok").length;
  const availableRecords = useMemo(() => {
    const entries = new Map(folderSearchRecords.map((record) => [record.path, record]));
    for (const record of state.records) entries.set(record.path, record);
    return [...entries.values()];
  }, [state.records, folderSearchRecords]);
  const selectedRecords = availableRecords.filter((record) => selectedPaths.includes(record.path));
  const inspectedRecord = availableRecords.find((record) => record.path === inspectedPath) || null;
  const allPageSelected = visibleRecords.length > 0 && visibleRecords.every((record) => selectedPaths.includes(record.path));
  const somePageSelected = visibleRecords.some((record) => selectedPaths.includes(record.path));
  const activeCategory = untaggedOnly ? "未分类" : tagFilter || "全部投影";
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
  }, [search, tagFilter, untaggedOnly, statusFilter, sortMode, pageSize]);

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
        <div className="library-search"><Search size={16} aria-hidden="true" /><Input type="search" aria-label="搜索投影" placeholder="搜索名称、作者、标签…" value={search} onChange={(event) => setSearch(event.target.value)} /></div>
        <Button variant="ghost" onClick={() => handleOpenLocalLibraryFolders().catch((error) => setPreviewError(`打开本地库文件夹失败：${String(error)}`))}><FolderOpen size={16} />管理文件夹</Button>
        <Button variant="ghost" size="icon" aria-label={isRefreshing ? "正在刷新投影库" : "刷新投影库"} title="刷新投影库" disabled={libraryBusy} aria-busy={isRefreshing} onClick={handleRefresh}><RefreshCw size={16} /></Button>
      </header>
      {previewError && <div className="library-message" role="alert">{previewError}<Button variant="ghost" size="icon-sm" aria-label="关闭错误提示" onClick={() => setPreviewError("")}><X size={14} /></Button></div>}
      {folderSearchError && <div className="library-message" role="alert">文件夹搜索失败：{folderSearchError}</div>}
      {isRefreshing && <div className="library-progress" role="status">刷新中 {refreshProgress} / {state.records.length}</div>}

      <div className="library-workspace">
        <aside className="library-categories" aria-label="投影分类">
          <div className="library-pane-heading"><h3>我的投影</h3><span>{state.records.length}</span></div>
          <ScrollArea className="library-category-scroll" horizontal>
            <nav className="library-category-list" aria-label="标签筛选">
              <Button variant="ghost" className="library-category" aria-pressed={!tagFilter && !untaggedOnly} onClick={() => { setTagFilter(""); setUntaggedOnly(false); }}><LayoutGrid size={16} /><span>全部投影</span><small>{state.records.length}</small></Button>
              <Button variant="ghost" className="library-category" aria-pressed={untaggedOnly} data-drop-target={dropTarget === "untagged" || undefined} onClick={() => { setTagFilter(""); setUntaggedOnly(true); }} {...tagDropProps("")}><Box size={16} /><span>未分类</span><small>{state.records.filter((record) => !record.tags?.length).length}</small></Button>
              <div className="library-tag-heading"><span>标签</span><Button size="icon-sm" variant="ghost" aria-label="为选中投影添加标签" title={selectedRecords.length ? "为选中投影添加标签" : "先勾选投影，再添加标签"} disabled={!selectedRecords.length || libraryBusy} onClick={() => openTagDialog(selectedRecords.map((record) => record.path))}><Plus size={15} /></Button></div>
              {tagOptions.map((tag) => <Button key={tag} variant="ghost" className="library-category" aria-pressed={tagFilter === tag} data-drop-target={dropTarget === tag || undefined} onClick={() => { setTagFilter(tag); setUntaggedOnly(false); }} {...tagDropProps(tag)}><Tag size={15} /><span title={tag}>{tag}</span><small>{state.records.filter((record) => record.tags?.includes(tag)).length}</small></Button>)}
              {!tagOptions.length && <p className="library-category-hint">选中投影后添加标签。</p>}
            </nav>
          </ScrollArea>
          <div className="library-category-footnote">拖到标签即可归类，文件保持原位。</div>
        </aside>

        <section className="library-collection" aria-label="投影卡片">
          <div className="library-collection-toolbar">
            <label className="library-select-all"><Checkbox checked={allPageSelected ? true : somePageSelected ? "indeterminate" : false} disabled={!visibleRecords.length || libraryBusy} aria-label="选择本页全部投影" onCheckedChange={(checked) => setSelectedPaths((current) => checked === true ? [...new Set([...current, ...visibleRecords.map((record) => record.path)])] : current.filter((path) => !visibleRecords.some((record) => record.path === path)))} /><span>{selectedRecords.length ? `已选 ${selectedRecords.length}` : activeCategory}</span></label>
            <span className="library-result-count" role="status">{filteredRecords.length} 项{isFolderSearchLoading ? " · 搜索中…" : ""}</span>
            <div className="library-filter-control"><Dropdown ariaLabel="筛选文件状态" value={statusFilter} onChange={setStatusFilter} options={[{ value: "", label: "全部状态" }, { value: "ok", label: "正常" }, { value: "missing", label: "文件丢失" }, { value: "parse_error", label: "解析失败" }]} /></div>
            <div className="library-filter-control"><Dropdown ariaLabel="投影排序" value={sortMode} onChange={setSortMode} options={[{ value: "manual", label: "手动排序" }, { value: "recent_analyzed", label: "最近分析" }, { value: "blocks", label: "方块数量" }, { value: "name", label: "名称 A–Z" }]} /></div>
          </div>
          {selectedRecords.length > 0 && <div className="library-batch-toolbar"><Button size="sm" onClick={() => openTagDialog(selectedRecords.map((record) => record.path))} disabled={libraryBusy}><Tag size={14} />添加标签</Button><Button size="sm" variant="ghost" disabled={libraryBusy} onClick={() => { setRemoveError(""); setRemoveRecords(selectedRecords); }}><Trash2 size={14} />从列表移除</Button><Button size="sm" variant="ghost" onClick={() => setSelectedPaths([])}>取消选择</Button></div>}
          <ScrollArea className="library-grid-scroll">
            {visibleRecords.length > 0 ? <div className="library-card-grid">{visibleRecords.map((record) => <ProjectionCard key={record.path} record={record} isCurrent={currentFile === record.path} isInspected={inspectedRecord?.path === record.path} checked={selectedPaths.includes(record.path)} busy={libraryBusy}
              previewUrl={previewDataUrls[record.path] || projectionPreviewDataUrls[record.path] || ""} loading={!!projectionPreviewLoadingPaths[record.path]}
              onInspect={() => { setInspectedPath(record.path); setDetailImageKind("auto"); }} onToggle={(checked) => toggleSelected(record.path, checked)} onActivate={() => handleActivateRecord(record.path)}
              onDragStart={(event) => { const paths = selectedPaths.includes(record.path) ? selectedRecords.map((entry) => entry.path) : [record.path]; setDragPaths(paths); event.dataTransfer.effectAllowed = "all"; event.dataTransfer.setData("application/x-nova-library-records", JSON.stringify(paths)); }}
              onDragEnd={() => { setDragPaths([]); setDropTarget(""); }} onDrop={(event) => { event.preventDefault(); event.stopPropagation(); handleDrop(record.path).catch((error) => setPreviewError(`排序失败：${String(error)}`)); }}
            />)}</div> : <div className="library-empty-state"><Box size={40} aria-hidden="true" /><h3>{state.records.length || search ? "没有找到投影" : "把你的第一份投影放进来"}</h3><p>{state.records.length || search ? "换个关键词或清除筛选，再试一次。" : "导入本地 .litematic，或从在线投影库下载。"}</p><div className="library-empty-actions">{state.records.length || search ? <Button onClick={() => { setSearch(""); setTagFilter(""); setUntaggedOnly(false); setStatusFilter(""); }}>清除筛选</Button> : <Button variant="default" onClick={handleSelect} disabled={libraryBusy}><Plus size={16} />导入投影</Button>}</div></div>}
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
              <div className="library-detail-actions"><Button variant="default" disabled={libraryBusy || inspectedRecord.status !== "ok" || currentFile === inspectedRecord.path} onClick={() => handleActivateRecord(inspectedRecord.path)}>{currentFile === inspectedRecord.path ? "当前投影" : "设为当前投影"}</Button><Button disabled={libraryBusy || inspectedRecord.status === "missing"} onClick={() => { setCurrentFile(inspectedRecord.path); setRoute("properties"); }}>查看属性</Button></div>
              <dl className="library-detail-facts"><div><dt>方块数量</dt><dd>{inspectedRecord.totalBlocks.toLocaleString()}</dd></div><div><dt>尺寸</dt><dd>{inspectedRecord.enclosingSize.x} × {inspectedRecord.enclosingSize.y} × {inspectedRecord.enclosingSize.z}</dd></div><div><dt>区域</dt><dd>{inspectedRecord.regionCount}</dd></div><div><dt>文件大小</dt><dd>{formatSize(inspectedRecord.fileSize)}</dd></div><div><dt>最近分析</dt><dd>{formatDate(inspectedRecord.lastAnalyzedAt)}</dd></div></dl>
              {inspectedRecord.description && <p className="library-detail-description">{inspectedRecord.description}</p>}
              <div className="library-detail-section"><div className="library-detail-section-heading"><h4>标签</h4><Button size="icon-sm" variant="ghost" aria-label="添加标签" disabled={libraryBusy} onClick={() => openTagDialog([inspectedRecord.path])}><Plus size={15} /></Button></div><div className="library-detail-tags">{inspectedRecord.tags?.length ? inspectedRecord.tags.map((tag) => <span className="library-tag-chip" key={tag}>{tag}<Button size="icon-sm" variant="ghost" aria-label={`移除标签 ${tag}`} disabled={libraryBusy} onClick={() => applyTag([inspectedRecord.path], tag, true)}><X size={12} /></Button></span>) : <span className="muted">未分类</span>}</div></div>
              <div className="library-detail-section"><h4>预览图</h4><div className="library-detail-actions">{previewDataUrls[inspectedRecord.path] && projectionPreviewDataUrls[inspectedRecord.path] && <Button size="sm" onClick={() => setDetailImageKind(detailImageKind === "auto" ? "preview" : "auto")}>{detailImageKind === "auto" ? "查看内嵌图" : "查看渲染图"}</Button>}<Button size="sm" disabled={libraryBusy || !!generatingPreviewPath || inspectedRecord.status !== "ok"} aria-busy={generatingPreviewPath === inspectedRecord.path} onClick={() => handleGeneratePreview(inspectedRecord)}>{generatingPreviewPath === inspectedRecord.path ? "正在生成…" : "生成预览图"}</Button></div></div>
              <div className="library-detail-section"><h4>文件</h4><p className="library-detail-path" title={inspectedRecord.path}>{inspectedRecord.path}</p><div className="library-detail-actions"><Button size="sm" onClick={() => openFileParentDir(inspectedRecord.path).catch((error) => setPreviewError(`打开文件夹失败：${String(error)}`))}>打开文件夹</Button><Button size="sm" disabled={libraryBusy || inspectedRecord.status === "missing"} onClick={() => handleOpenSendDialog(inspectedRecord)}>复制到…</Button><Button size="sm" variant="ghost" disabled={libraryBusy} onClick={() => { setRemoveError(""); setRemoveRecords([inspectedRecord]); }}><Trash2 size={14} />从列表移除</Button></div></div>
            </div> : <div className="library-inspector-empty"><Box size={32} /><p>点击卡片查看详情</p></div>}
          </ScrollArea>
        </aside>
      </div>
      {removeRecords.length > 0 && <Dialog title={`从投影库移除${removeRecords.length > 1 ? ` ${removeRecords.length} 项` : "此投影"}？`} subtitle="只移除列表记录，不会删除电脑上的投影文件。" width="sm" busy={isRemoving} onClose={() => setRemoveRecords([])} footer={<><Button onClick={() => setRemoveRecords([])} disabled={isRemoving}>保留</Button><Button variant="destructive" onClick={handleRemove} disabled={isRemoving} aria-busy={isRemoving}>{isRemoving ? "正在移除…" : "从列表移除"}</Button></>}><ul className="library-remove-list">{removeRecords.slice(0, 8).map((record) => <li key={record.path}>{record.displayName || record.fileName}</li>)}</ul>{removeRecords.length > 8 && <p>以及另外 {removeRecords.length - 8} 项</p>}{removeError && <div className="dialog-message dialog-message-error" role="alert">{removeError}</div>}</Dialog>}
      {tagDialogPaths.length > 0 && <Dialog title="添加标签" subtitle={`为 ${tagDialogPaths.length} 个投影归类，已有标签会保留。`} width="sm" busy={isTagging} onClose={() => setTagDialogPaths([])} footer={<><Button disabled={isTagging} onClick={() => setTagDialogPaths([])}>取消</Button><Button variant="default" disabled={isTagging || !tagDraft.trim()} onClick={() => applyTag(tagDialogPaths, tagDraft)}>{isTagging ? "保存中…" : "添加"}</Button></>}><form className="library-tag-form" onSubmit={(event) => { event.preventDefault(); void applyTag(tagDialogPaths, tagDraft); }}><label htmlFor="library-tag-name">标签名称</label><Input id="library-tag-name" autoFocus value={tagDraft} disabled={isTagging} onChange={(event) => setTagDraft(event.target.value)} placeholder="例如：红石机器" />{tagOptions.length > 0 && <Dropdown ariaLabel="选择已有标签" value={tagOptions.includes(tagDraft) ? tagDraft : ""} onChange={setTagDraft} options={[{ value: "", label: "或选择已有标签" }, ...tagOptions.map((tag) => ({ value: tag, label: tag }))]} />}</form>{tagError && <div className="dialog-message dialog-message-error" role="alert">{tagError}</div>}</Dialog>}
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


