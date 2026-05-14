import React, { useEffect, useMemo, useState } from "react";
import {
  addOrUpdateRecord,
  analyzeFile,
  copyFileToDirectory,
  LibraryState,
  loadLibrary,
  LocalLibraryFolder,
  ProjectionRecord,
  reorderLibraryRecords,
  saveLibrary,
  setRecordPreview,
} from "../../../../../src/business/facade";
import { SendProjectionDialog, ensureLitematicFileName } from "./sendDialog";
import { checkFileExists, generatePreviewImage, openFileParentDir, readImageBase64, readProjectionPreviewImage, selectLitematicFile } from "../../../../../src/business/facade";
import { LocalLibraryFoldersDialog, openLocalLibraryFoldersWithWindowBehavior } from "../../../local_library_folders";
import { RedenLibraryDialog, openRedenLibraryWithWindowBehavior } from "../../../reden_library";
import { loadUserConfigMigratingLocalStorage } from "../../../../../src/business/facade";
import { DisplayMode, normalizeDisplayMode } from "../../../../../src/business/facade";
import { listenEvent } from "../../../../../src/platform/events";
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

function ProjectionCard({
  record,
  isCurrent,
  previewDataUrl,
  projectionPreviewDataUrl,
  isLoadingProjectionPreview,
  previewMode,
  isGenerating,
  onSetCurrent,
  onEditProperties,
  onOpenFolder,
  onSend,
  onRemove,
  onGeneratePreview,
  onEnsureProjectionPreview,
  onDragStart,
  onDragOver,
  onDrop,
}: {
  record: ProjectionRecord;
  isCurrent: boolean;
  previewDataUrl: string;
  projectionPreviewDataUrl: string;
  isLoadingProjectionPreview: boolean;
  previewMode: DisplayMode;
  isGenerating: boolean;
  onSetCurrent: () => void;
  onEditProperties: () => void;
  onOpenFolder: () => void;
  onSend: () => void;
  onRemove: () => void;
  onGeneratePreview: () => void;
  onEnsureProjectionPreview: () => void;
  onDragStart: () => void;
  onDragOver: (event: React.DragEvent) => void;
  onDrop: () => void;
}) {
  const [imageKind, setImageKind] = useState<"render" | "preview">("preview");
  const tags = record.tags?.length ? record.tags.join(" / ") : "无标签";
  const author = record.author || "Unknown";
  const statusLabel = recordStatusLabel(record);
  const cardClassName = ["library-card", isCurrent ? "library-card-current" : ""].filter(Boolean).join(" ");
  const statusClassName = [
    "lib-card-status",
    record.status === "missing" ? "lib-card-status-missing" : "lib-card-status-error",
  ].join(" ");

  useEffect(() => {
    if (imageKind === "preview") onEnsureProjectionPreview();
  }, [imageKind, onEnsureProjectionPreview]);

  return (
    <div
      className={cardClassName}
      draggable
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDrop={onDrop}
      onClick={(event) => {
        event.stopPropagation();
      }}
      onDoubleClick={(event) => {
        event.stopPropagation();
        onSetCurrent();
      }}
    >
      <div className="lib-card-left">
        <div className="lib-card-line-main">
          <div className="lib-card-heading">
            <span className="lib-card-title">{record.displayName}</span>
            {statusLabel && <span className={statusClassName}>{statusLabel}</span>}
          </div>
          <div className="lib-card-size">{formatSize(record.fileSize)}</div>
        </div>
        <div className="lib-card-text">标签：{tags}</div>
        <div className="lib-card-text">内部名称：{record.displayName || "Unnamed"}</div>
        <div className="lib-card-text">作者：{author}</div>
        <div className="lib-card-highlight">文件名：{record.fileName}</div>
        <div className="lib-card-path" title={record.path}>{record.path}</div>
        <div className="lib-card-text">时间：{formatDate(record.lastAnalyzedAt)}</div>
        {record.status === "parse_error" && record.lastError && <div className="lib-card-error-detail">{record.lastError}</div>}

        <div className="lib-card-spacer" />
        <div className="lib-card-actions">
          <button className="btn" onClick={(event) => { event.stopPropagation(); onSetCurrent(); }}>激活</button>
          <button className="btn" onClick={(event) => { event.stopPropagation(); onEditProperties(); }}>属性</button>
          <button className="btn" onClick={(event) => { event.stopPropagation(); onOpenFolder(); }}>打开目录</button>
          <button className="btn" onClick={(event) => { event.stopPropagation(); onSend(); }}>发送...</button>
          <button className="btn" onClick={(event) => { event.stopPropagation(); onRemove(); }}>删除记录</button>
        </div>
      </div>

      <div className="lib-card-right">
        <div className="lib-card-image-label">{imageKind === "render" ? "渲染图" : "预览图"}</div>
        <div className="lib-card-preview-box">
          {imageKind === "render"
            ? (previewDataUrl ? <img className="lib-card-preview-image" src={previewDataUrl} alt="" /> : "暂无渲染图")
            : (isLoadingProjectionPreview
              ? "加载中..."
              : projectionPreviewDataUrl
                ? <img className="lib-card-preview-image" src={projectionPreviewDataUrl} alt="" />
                : "无内嵌预览图")}
        </div>
        <button className="btn wide-button" onClick={(event) => {
          event.stopPropagation();
          const nextKind = imageKind === "render" ? "preview" : "render";
          setImageKind(nextKind);
          if (nextKind === "preview") onEnsureProjectionPreview();
        }}>
          {imageKind === "render" ? "切换到预览图" : "切换到渲染图"}
        </button>
        <button
          className="btn wide-button"
          disabled={isGenerating || record.status !== "ok"}
          onClick={(event) => { event.stopPropagation(); onGeneratePreview(); }}
        >
          {isGenerating ? "生成中..." : `生成渲染 (${previewMode})`}
        </button>
      </div>
    </div>
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
  const [previewMode, setPreviewMode] = useState<DisplayMode>("normal");
  const [generatingPreviewPath, setGeneratingPreviewPath] = useState("");
  const [previewError, setPreviewError] = useState("");
  const [previewDataUrls, setPreviewDataUrls] = useState<Record<string, string>>({});
  const [projectionPreviewDataUrls, setProjectionPreviewDataUrls] = useState<Record<string, string>>({});
  const [projectionPreviewLoadingPaths, setProjectionPreviewLoadingPaths] = useState<Record<string, boolean>>({});
  const [projectionPreviewLoadedPaths, setProjectionPreviewLoadedPaths] = useState<Record<string, boolean>>({});
  const [showLocalLibraryFoldersOverlay, setShowLocalLibraryFoldersOverlay] = useState(false);
  const [showRedenLibraryOverlay, setShowRedenLibraryOverlay] = useState(false);
  const [dragPath, setDragPath] = useState("");
  const [sendDialogRecord, setSendDialogRecord] = useState<ProjectionRecord | null>(null);
  const [sendTargetDirectory, setSendTargetDirectory] = useState("");
  const [sendTargetFileName, setSendTargetFileName] = useState("");
  const [sendOverwrite, setSendOverwrite] = useState(false);
  const [sendError, setSendError] = useState("");
  const [isSending, setIsSending] = useState(false);

  useEffect(() => {
    loadLibrary().then(setState);
    loadUserConfigMigratingLocalStorage()
      .then((info) => setPreviewMode(normalizeDisplayMode(info.config.preview_mode)))
      .catch(() => setPreviewMode("normal"));
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

  useEffect(() => {
    let cancelled = false;
    const loadPreviews = async () => {
      const entries = await Promise.all(state.records.map(async (record) => {
        const imagePath = record.preview_image_path || record.previewPath;
        if (!imagePath) return [record.path, ""] as const;
        try {
          return [record.path, await readImageBase64(imagePath)] as const;
        } catch {
          return [record.path, ""] as const;
        }
      }));
      if (!cancelled) setPreviewDataUrls(Object.fromEntries(entries.filter(([, value]) => value)));
    };
    loadPreviews();
    return () => {
      cancelled = true;
    };
  }, [state.records]);

  const handleSelect = async () => {
    const res = await selectLitematicFile();
    if (res) {
      setCurrentFile(res);
      setState(await addOrUpdateRecord(state, res));
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
    if (!sendDialogRecord) return;
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

  const handleRefresh = async () => {
    setIsRefreshing(true);
    const newState = { ...state, records: [...state.records] };
    for (const rec of newState.records) {
      const exists = await checkFileExists(rec.path);
      if (!exists) {
        rec.status = "missing";
      } else {
        const analysis = await analyzeFile(rec.path);
        Object.assign(rec, analysis);
        rec.lastAnalyzedAt = Date.now();
      }
    }
    await saveLibrary(newState);
    setState(newState);
    setIsRefreshing(false);
  };

  const handleRemove = async (path: string) => {
    const newState = { ...state, records: state.records.filter((r) => r.path !== path) };
    await saveLibrary(newState);
    setState(newState);
    if (currentFile === path) setCurrentFile("");
  };

  const handleGeneratePreview = async (record: ProjectionRecord) => {
    setPreviewError("");
    setGeneratingPreviewPath(record.path);
    try {
      const output = await generatePreviewImage(record.path, previewMode);
      const newState = await setRecordPreview(state, record.path, output.preview_path);
      setState(newState);
      setPreviewDataUrls((current) => ({ ...current, [record.path]: output.data_url }));
    } catch (err: any) {
      setPreviewError(String(err));
    } finally {
      setGeneratingPreviewPath("");
    }
  };

  const ensureProjectionPreview = async (record: ProjectionRecord) => {
    if (projectionPreviewLoadedPaths[record.path] || projectionPreviewLoadingPaths[record.path]) return;
    setProjectionPreviewLoadingPaths((current) => ({ ...current, [record.path]: true }));
    try {
      const output = await readProjectionPreviewImage(record.path);
      if (output?.data_url) {
        setProjectionPreviewDataUrls((current) => ({ ...current, [record.path]: output.data_url }));
      }
    } catch {
      setProjectionPreviewDataUrls((current) => {
        const next = { ...current };
        delete next[record.path];
        return next;
      });
    } finally {
      setProjectionPreviewLoadedPaths((current) => ({ ...current, [record.path]: true }));
      setProjectionPreviewLoadingPaths((current) => {
        const next = { ...current };
        delete next[record.path];
        return next;
      });
    }
  };

  const handleDrop = async (targetPath: string) => {
    if (!dragPath || dragPath === targetPath || sortMode !== "manual") return;
    const ordered = [...visibleRecords];
    const from = ordered.findIndex((record) => record.path === dragPath);
    const to = ordered.findIndex((record) => record.path === targetPath);
    if (from < 0 || to < 0) return;
    const [moved] = ordered.splice(from, 1);
    ordered.splice(to, 0, moved);
    const visiblePaths = ordered.map((record) => record.path);
    const untouched = state.records.filter((record) => !visiblePaths.includes(record.path)).map((record) => record.path);
    setState(await reorderLibraryRecords(state, [...visiblePaths, ...untouched]));
    setDragPath("");
  };

  const tagOptions = useMemo(() => {
    return Array.from(new Set(state.records.flatMap((record) => record.tags || []))).sort((a, b) => a.localeCompare(b));
  }, [state.records]);

  const filteredRecords = useMemo(() => {
    let records = state.records.filter((r) => {
      if (search) {
        const q = search.toLowerCase();
        if (!`${r.displayName} ${r.fileName} ${r.path} ${r.author} ${r.description}`.toLowerCase().includes(q)) return false;
      }
      if (tagFilter && !(r.tags || []).includes(tagFilter)) return false;
      if (statusFilter && r.status !== statusFilter) return false;
      return true;
    });
    if (sortMode === "manual") {
      records = [...records].sort((a, b) => (a.sort_index ?? 0) - (b.sort_index ?? 0));
    } else if (sortMode === "recent_mod" || sortMode === "recent_import") {
      records = [...records].sort((a, b) => b.lastAnalyzedAt - a.lastAnalyzedAt);
    } else if (sortMode === "blocks") {
      records = [...records].sort((a, b) => b.totalBlocks - a.totalBlocks);
    } else if (sortMode === "name") {
      records = [...records].sort((a, b) => a.displayName.localeCompare(b.displayName));
    }
    return records;
  }, [state.records, search, tagFilter, statusFilter, sortMode]);

  const pageCount = pageSize === 0 ? 1 : Math.max(1, Math.ceil(filteredRecords.length / pageSize));
  const currentPage = Math.min(page, pageCount);
  const visibleRecords = useMemo(() => {
    if (pageSize === 0) return filteredRecords;
    const start = (currentPage - 1) * pageSize;
    return filteredRecords.slice(start, start + pageSize);
  }, [filteredRecords, currentPage, pageSize]);
  const issueCount = filteredRecords.filter((r) => r.status === "missing" || r.status === "parse_error").length;

  useEffect(() => {
    setPage(1);
  }, [search, tagFilter, statusFilter, sortMode, pageSize]);

  useEffect(() => {
    if (page > pageCount) setPage(pageCount);
  }, [page, pageCount]);

  return (
    <div className="nova-page library-page" onClick={() => setCurrentFile("")}>
      <div className="library-title-row">
        <h2 className="library-page-title">投影库</h2>
      </div>

      <div className="library-action-row" onClick={(event) => event.stopPropagation()}>
        <button className="btn" onClick={handleSelect}>添加本地记录...</button>
        <button className="btn" onClick={() => handleOpenRedenLibrary().catch((err) => alert(`打开在线投影库失败\n${String(err)}`))}>从在线投影库获取...</button>
        <button className="btn" onClick={() => handleOpenLocalLibraryFolders().catch((err) => alert(`打开本地库文件夹失败\n${String(err)}`))}>管理本地库文件夹...</button>
        <button className="btn" onClick={handleRefresh} disabled={isRefreshing}>{isRefreshing ? "刷新中" : "刷新"}</button>
      </div>

      <div className="library-filter-row" onClick={(event) => event.stopPropagation()}>
        <input className="input nova-input-flex" placeholder="搜索投影名、原文件名或原地址" value={search} onChange={(event) => setSearch(event.target.value)} />
        <select className="input library-filter-select" value={tagFilter} onChange={(event) => setTagFilter(event.target.value)}>
          <option value="">全部标签</option>
          {tagOptions.map((tag) => <option key={tag} value={tag}>{tag}</option>)}
        </select>
        <select className="input library-filter-select" value={statusFilter} onChange={(event) => setStatusFilter(event.target.value)}>
          <option value="">全部状态</option>
          <option value="ok">正常</option>
          <option value="missing">文件丢失</option>
          <option value="parse_error">解析失败</option>
        </select>
        <select className="input library-filter-select" value={sortMode} onChange={(event) => setSortMode(event.target.value)}>
          <option value="manual">手动排序</option>
          <option value="recent_import">最近导入</option>
          <option value="recent_mod">最近修改</option>
          <option value="blocks">方块数高到低</option>
          <option value="name">名称 A-Z</option>
        </select>
      </div>

      {previewError && <pre className="nova-error nova-pre-medium library-preview-error">{previewError}</pre>}

      {state.records.length === 0 ? (
        <div className="library-empty-state">
          <p className="library-empty-text">投影库还没有内容。请先导入一个 .litematic 文件。</p>
          <button className="btn library-empty-action" onClick={(event) => { event.stopPropagation(); handleSelect(); }}>选择 .litematic...</button>
        </div>
      ) : (
        <>
          <div className="library-content-title">{search ? "搜索结果" : "最近内容"}</div>
          <div className="library-record-list">
            {visibleRecords.length > 0 ? visibleRecords.map((r) => (
              <ProjectionCard
                key={r.path}
                record={r}
                isCurrent={currentFile === r.path}
                onSetCurrent={() => setCurrentFile(r.path)}
                onEditProperties={() => { setCurrentFile(r.path); setRoute("properties"); }}
                onOpenFolder={() => openFileParentDir(r.path).catch((err) => alert(`打开失败\n${String(err)}`))}
                onSend={() => handleOpenSendDialog(r)}
                onRemove={() => handleRemove(r.path)}
                onGeneratePreview={() => handleGeneratePreview(r)}
                onEnsureProjectionPreview={() => ensureProjectionPreview(r)}
                previewMode={previewMode}
                isGenerating={generatingPreviewPath === r.path}
                isLoadingProjectionPreview={!!projectionPreviewLoadingPaths[r.path]}
                previewDataUrl={previewDataUrls[r.path] || ""}
                projectionPreviewDataUrl={projectionPreviewDataUrls[r.path] || ""}
                onDragStart={() => setDragPath(r.path)}
                onDragOver={(event) => { if (sortMode === "manual") event.preventDefault(); }}
                onDrop={() => handleDrop(r.path)}
              />
            )) : (
              <div className="library-empty-state library-empty-state-compact">没有符合条件的记录。</div>
            )}
          </div>

          <div className="library-footer" onClick={(event) => event.stopPropagation()}>
            <div className="nova-muted nova-small"> {filteredRecords.length} 个项目，{issueCount} 条需关注</div>
            <label className="library-page-size-control">
              <span>每页条目</span>
              <select className="input library-limit-select" value={pageSize} onChange={(event) => setPageSize(Number(event.target.value))}>
                <option value="20">20</option>
                <option value="50">50</option>
                <option value="100">100</option>
                <option value="200">200</option>
                <option value="0">全部</option>
              </select>
            </label>
            <div className="library-pager">
              <button className="btn" disabled={currentPage <= 1 || pageSize === 0} onClick={() => setPage(Math.max(1, currentPage - 1))}>上一页</button>
              <span>第</span>
              <input className="input library-page-input" type="number" min={1} max={pageCount} value={currentPage} disabled={pageSize === 0} onChange={(event) => setPage(Math.min(pageCount, Math.max(1, Number(event.target.value) || 1)))} />
              <span>页，共 {pageCount} 页</span>
              <button className="btn" disabled={currentPage >= pageCount || pageSize === 0} onClick={() => setPage(Math.min(pageCount, currentPage + 1))}>下一页</button>
            </div>
          </div>
        </>
      )}
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


