import { displayPathText } from "../../../src/business/facade";
import { displayFilePath } from "../../../src/business/facade";
import { X, Folder, FileText, File } from "lucide-react";
import { Button } from "../../components/ui/button";
import { ScrollArea } from "../../components/ui/scroll-area";
import { Dialog } from "../../components/Dialog";
import { Dropdown } from "../../components/Dropdown";
import { Checkbox } from "../../components/ui/checkbox";
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from "../../components/ui/table";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import {
  activateProjectionRecord,
  addLocalLibraryFolder,
  chooseUserConfigDir,
  copyFileToDirectory,
  DirectoryEntryInfo,
  listDirectoryEntries,
  loadLibrary,
  loadUserConfigMigratingLocalStorage,
  LocalLibraryFolder,
  LibraryState,
  LibraryFolderType,
  LibraryRecordView,
  normalizeLibraryState,
  normalizeLocalLibraryTailPathCount,
  normalizeMaterialListWindowBehavior,
  openFileParentDir,
  openLocalLibraryFoldersWindow,
  openWorkspacePath,
  readProjectionPreviewImage,
  removeLocalLibraryFolder,
  scanLibraryFolders,
  syncAllLocalLibraryFolders,
  syncLocalLibraryFolder,
  updateLocalLibraryFolder,
} from "../../../src/business/facade";
import { emitEvent } from "../../../src/platform/events";
import { projectionLibraryImportedEvent, projectionLibraryStateChangedEvent } from "../libraryEvents";
import { SendFileToFolderDialog } from "../main/pages/library/sendDialog";
import { ensureLitematicFileName } from "../main/pages/library/function";
import { compactFolderPath } from "../main/pages/library/function";
import { BuiltinImportPanel } from "./BuiltinImportPanel";
import { TrashPanel } from "./TrashPanel";
import "../main/pages/library/library.css";

type HoverPreviewEntry = { entry: DirectoryEntryInfo; x: number; y: number };

function formatDate(timestamp: number): string {
  if (!timestamp) return "-";
  const date = new Date(timestamp);
  const pad = (value: number) => value.toString().padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function formatSize(numBytes: number): string {
  if (!numBytes) return "-";
  let value = Math.max(0, numBytes);
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index++;
  }
  return index === 0 ? `${Math.floor(value)} ${units[index]}` : `${value.toFixed(1)} ${units[index]}`;
}

function isLitematicEntry(entry: DirectoryEntryInfo): boolean {
  return entry.is_file && /\.litematic$/i.test(entry.name || entry.path);
}

function parentDirectory(path: string): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const lastSeparatorIndex = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  return lastSeparatorIndex >= 0 ? normalized.slice(0, lastSeparatorIndex) : "";
}

function systemFileManagerLabel(): string {
  return navigator.platform.toLowerCase().includes("mac") ? "在访达打开" : "在文件资源管理器打开";
}

function recordFromEntry(entry: DirectoryEntryInfo): LibraryRecordView {
  return {
    id: entry.path,
    entryId: "",
    path: entry.path,
    fileName: entry.name,
    displayName: entry.name,
    author: "",
    description: "",
    totalBlocks: 0,
    totalVolume: 0,
    regionCount: 0,
    enclosingSize: { x: 0, y: 0, z: 0 },
    minecraftDataVersion: 0,
    fileSize: entry.file_size,
    mtime: entry.mtime_ms,
    tags: [],
    categoryIds: [],
    checksum: null,
    status: "ok",
    lastAnalyzedAt: entry.mtime_ms,
    lastError: "",
    sort_index: 0,
  };
}

function pickDefaultSendFolder(record: LibraryRecordView, folders: LocalLibraryFolder[]): string {
  const sourceParent = parentDirectory(record.path).toLowerCase();
  return folders.find((folder) => folder.path.toLowerCase() !== sourceParent)?.path || folders[0]?.path || "";
}

function LocalBrowserPreviewPopup({
  hover,
  previewDataUrl,
  isLoading,
}: {
  hover: HoverPreviewEntry | null;
  previewDataUrl: string | null | undefined;
  isLoading: boolean;
}) {
  const popupRef = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState(() => ({ left: (hover?.x || 0) + 14, top: (hover?.y || 0) + 14 }));

  useLayoutEffect(() => {
    if (!hover) return;
    const popup = popupRef.current;
    const offset = 14;
    const margin = 4;
    if (!popup) {
      setPosition({ left: hover.x + offset, top: hover.y + offset });
      return;
    }

    const rect = popup.getBoundingClientRect();
    let left = hover.x + offset;
    let top = hover.y + offset;

    if (left + rect.width > window.innerWidth - margin) {
      left = hover.x - rect.width - offset;
    }
    if (top + rect.height > window.innerHeight - margin) {
      top = hover.y - rect.height - offset;
    }

    left = Math.max(margin, Math.min(left, window.innerWidth - rect.width - margin));
    top = Math.max(margin, Math.min(top, window.innerHeight - rect.height - margin));
    setPosition((previous) => previous.left === left && previous.top === top ? previous : { left, top });
  }, [hover, previewDataUrl, isLoading]);

  useLayoutEffect(() => {
    const popup = popupRef.current;
    if (!popup) return;
    popup.style.left = `${position.left}px`;
    popup.style.top = `${position.top}px`;
  }, [position.left, position.top]);

  if (!hover) return null;

  return (
    <div ref={popupRef} className="material-list-hover-popup local-browser-preview-popup" role="tooltip">
      <div className="local-browser-preview-box">
        {previewDataUrl ? (
          <img className="local-browser-preview-image" src={previewDataUrl} alt="" />
        ) : (
          <span>{isLoading ? "加载中..." : "无内嵌预览图"}</span>
        )}
      </div>
    </div>
  );
}

async function emitLibraryStateChanged(): Promise<void> {
  await emitEvent(projectionLibraryStateChangedEvent);
}

/**
 * Opens the local-library-folder manager according to the user's configured child-window behavior.
 */
export async function openLocalLibraryFoldersWithWindowBehavior(showOverlay: () => void): Promise<void> {
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
  const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
  if (behavior === "independent_window") {
    try {
      await openLocalLibraryFoldersWindow();
      return;
    } catch {
      // Browser preview cannot create a desktop window, so fall back to the in-window dialog.
    }
  }
  showOverlay();
}

/**
 * Shared content used by both the standalone child window and the main-window overlay.
 */
export function LocalLibraryFoldersPanel({ onClose }: { onClose?: () => void }) {
  const [state, setState] = useState<LibraryState>(() => normalizeLibraryState(null));
  const [busyKey, setBusyKey] = useState("");
  const [status, setStatus] = useState("");
  const [removeFolder, setRemoveFolder] = useState<LocalLibraryFolder | null>(null);
  const [localLibraryTailPathCount, setLocalLibraryTailPathCount] = useState(3);
  const [browserRootFolder, setBrowserRootFolder] = useState<LocalLibraryFolder | null>(null);
  const [browserPath, setBrowserPath] = useState("");
  const [browserEntries, setBrowserEntries] = useState<DirectoryEntryInfo[]>([]);
  const [isBrowserLoading, setIsBrowserLoading] = useState(false);
  const [sendDialogRecord, setSendDialogRecord] = useState<LibraryRecordView | null>(null);
  const [sendTargetDirectory, setSendTargetDirectory] = useState("");
  const [sendTargetFileName, setSendTargetFileName] = useState("");
  const [sendOverwrite, setSendOverwrite] = useState(false);
  const [sendError, setSendError] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [hoverPreview, setHoverPreview] = useState<HoverPreviewEntry | null>(null);
  const [previewDataUrls, setPreviewDataUrls] = useState<Record<string, string | null>>({});
  const [previewLoadingPaths, setPreviewLoadingPaths] = useState<Record<string, boolean>>({});
  const [selectedBrowserEntryPath, setSelectedBrowserEntryPath] = useState("");

  useEffect(() => {
    loadLibrary().then(setState).catch((error) => setStatus(`读取本地库配置失败：${String(error)}`));
  }, []);

  useEffect(() => {
    loadUserConfigMigratingLocalStorage()
      .then((info) => setLocalLibraryTailPathCount(normalizeLocalLibraryTailPathCount(info.config.local_library_tail_path_count)))
      .catch(() => undefined);
  }, []);


  useEffect(() => {
    if (!browserPath) {
      setBrowserEntries([]);
      setSelectedBrowserEntryPath("");
      return;
    }
    let cancelled = false;
    setSelectedBrowserEntryPath("");
    setIsBrowserLoading(true);
    listDirectoryEntries(browserPath)
      .then((entries) => {
        if (!cancelled) setBrowserEntries(entries);
      })
      .catch((error) => {
        if (!cancelled) {
          setBrowserEntries([]);
          setStatus(`读取文件夹失败：${String(error)}`);
        }
      })
      .finally(() => {
        if (!cancelled) setIsBrowserLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [browserPath]);

  const browserRootKey = useMemo(() => browserRootFolder ? browserRootFolder.path.toLowerCase() : "", [browserRootFolder]);
  const canGoBrowserUp = !!browserRootFolder && browserPath.toLowerCase() !== browserRootKey;

  const handleAddFolder = async () => {
    const folderPath = await chooseUserConfigDir().catch((error) => {
      setStatus(`打开文件夹选择器失败：${String(error)}`);
      return null;
    });
    if (!folderPath) return;
    setBusyKey("add");
    try {
      let nextState = await addLocalLibraryFolder(state, folderPath);
      setState(nextState);
      setStatus(`已添加本地库文件夹：${displayFilePath(folderPath)}`);
      await emitLibraryStateChanged();
      // B.1: mounting a folder auto-imports its new files when auto-activate is on.
      const scanned = await scanLibraryFolders(nextState, { onlyFolderPath: folderPath });
      nextState = scanned.state;
      setState(nextState);
      if (scanned.failures.length) setStatus(`已添加，但有 ${scanned.failures.length} 个文件未能扫描。`);
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`添加本地库文件夹失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleToggleFolder = async (folder: LocalLibraryFolder, patch: Partial<Pick<LocalLibraryFolder, "enabled" | "recursive" | "autoActivate">>) => {
    setBusyKey(`toggle:${folder.path}`);
    try {
      let nextState = await updateLocalLibraryFolder(state, folder.path, patch);
      setState(nextState);
      setStatus(`已更新文件夹设置：${displayFilePath(folder.path)}`);
      // B.1: enabling / re-configuring a tracked folder re-scans it (new files are
      // merged in only when auto-activate is on; the kernel enforces that).
      const updated = nextState.folders.find((item) => item.path === folder.path);
      if (updated && updated.enabled && updated.type !== "non_archived") {
        const scanned = await scanLibraryFolders(nextState, { onlyFolderPath: folder.path });
        nextState = scanned.state;
        setState(nextState);
      }
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`更新文件夹设置失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleChangeFolderType = async (folder: LocalLibraryFolder, type: LibraryFolderType) => {
    if (type === folder.type) return;
    setBusyKey(`type:${folder.path}`);
    try {
      let nextState = await updateLocalLibraryFolder(state, folder.path, { type });
      setState(nextState);
      setStatus(type === "non_archived" ? `已设为非归档：${displayFilePath(folder.path)}` : `已设为已追踪：${displayFilePath(folder.path)}`);
      if (type !== "non_archived") {
        const scanned = await scanLibraryFolders(nextState, { onlyFolderPath: folder.path });
        nextState = scanned.state;
        setState(nextState);
      }
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`更新文件夹类型失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleRemoveFolder = async (folder: LocalLibraryFolder) => {
    if (busyKey) return;
    setBusyKey(`remove:${folder.path}`);
    try {
      const nextState = await removeLocalLibraryFolder(state, folder.path);
      setState(nextState);
      setStatus(`已移除本地库文件夹：${displayFilePath(folder.path)}`);
      setRemoveFolder(null);
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`移除本地库文件夹失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleSyncFolder = async (folder: LocalLibraryFolder) => {
    setBusyKey(`sync:${folder.path}`);
    try {
      const result = await syncLocalLibraryFolder(state, folder.path);
      setState(result.state);
      setStatus(
        result.errors.length
          ? `同步完成，但存在 ${result.errors.length} 个错误。`
          : `已扫描 ${result.scannedFolderCount} 个文件夹，共发现 ${result.importedCount} 个 .litematic 文件。`,
      );
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`同步文件夹失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleSyncAll = async () => {
    setBusyKey("sync-all");
    try {
      const result = await syncAllLocalLibraryFolders(state);
      setState(result.state);
      setStatus(
        result.errors.length
          ? `批量同步完成，但存在 ${result.errors.length} 个错误。`
          : `已扫描 ${result.scannedFolderCount} 个文件夹，共发现 ${result.importedCount} 个 .litematic 文件。`,
      );
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`批量同步失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  /** Explicit "重新扫描全部" entry (B.2): scope = enabled tracked / built-in folders. */
  const handleRescanAll = async (forceRecheck: boolean) => {
    setBusyKey(forceRecheck ? "rescan-force" : "rescan");
    try {
      const outcome = await scanLibraryFolders(state, { useCache: !forceRecheck, forceRecheck });
      setState(outcome.state);
      setStatus(outcome.failures.length
        ? `${forceRecheck ? "强制重新校验" : "重新扫描"}完成，但有 ${outcome.failures.length} 个文件失败。`
        : `${forceRecheck ? "已强制重新校验" : "已重新扫描"} ${outcome.scannedCount} 个文件。`);
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`重新扫描失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleOpenBrowser = (folder: LocalLibraryFolder) => {
    setBrowserRootFolder(folder);
    setBrowserPath(folder.path);
    setStatus("");
  };

  const handleCloseBrowser = () => {
    setBrowserRootFolder(null);
    setBrowserPath("");
    setBrowserEntries([]);
    setSelectedBrowserEntryPath("");
  };

  const handleGoBrowserUp = () => {
    if (!canGoBrowserUp) return;
    const parent = parentDirectory(browserPath);
    if (!parent || parent.toLowerCase().length < browserRootKey.length) return;
    setBrowserPath(parent);
  };

  const handleActivateEntry = async (entry: DirectoryEntryInfo) => {
    if (!isLitematicEntry(entry)) return;
    setBusyKey(`activate:${entry.path}`);
    try {
      const nextState = await activateProjectionRecord(await loadLibrary(), entry.path);
      setState(nextState);
      setStatus(`已激活：${displayFilePath(entry.path)}`);
      await emitEvent(projectionLibraryImportedEvent, { path: entry.path });
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`激活失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleOpenSendDialog = (entry: DirectoryEntryInfo) => {
    if (!isLitematicEntry(entry)) return;
    const record = recordFromEntry(entry);
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
      setStatus(`已发送到：${displayFilePath(output.target_path)}`);
      handleCloseSendDialog();
    } catch (error: any) {
      setSendError(String(error));
    } finally {
      setIsSending(false);
    }
  };

  const ensureHoverPreview = async (entry: DirectoryEntryInfo) => {
    if (!isLitematicEntry(entry) || previewDataUrls[entry.path] !== undefined || previewLoadingPaths[entry.path]) return;
    setPreviewLoadingPaths((current) => ({ ...current, [entry.path]: true }));
    try {
      const output = await readProjectionPreviewImage(entry.path);
      setPreviewDataUrls((current) => ({ ...current, [entry.path]: output?.data_url || null }));
    } catch {
      setPreviewDataUrls((current) => ({ ...current, [entry.path]: null }));
    } finally {
      setPreviewLoadingPaths((current) => {
        const next = { ...current };
        delete next[entry.path];
        return next;
      });
    }
  };

  const enabledCount = state.folders.filter((folder) => folder.enabled).length;
  const fileManagerLabel = systemFileManagerLabel();
  const statusClassName = [
    "subwindow-status-text",
    status.includes("失败") || status.includes("错误") ? "subwindow-status-text-error" : "",
  ].filter(Boolean).join(" ");

  return (
    <section className="subwindow-panel-shell library-aux-panel library-folders-panel">
      <div className="subwindow-title-bar">
          <h3 className="subwindow-title">本地库管理</h3>
          {onClose && <Button className="subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}><X size={18} aria-hidden="true" /></Button>}
      </div>
      <p className="muted subwindow-subtitle">管理挂载目录，并把目录中的 .litematic 文件同步到投影库记录。</p>

      <fieldset>
        <legend>操作</legend>
        <div className="button-row subwindow-wrap-row subwindow-toolbar-group">
          <Button type="button" disabled={!!busyKey} onClick={handleAddFolder}>添加文件夹...</Button>
          <Button
           
            type="button"
            disabled={!!busyKey || enabledCount === 0}
            onClick={handleSyncAll}
          >
            {busyKey === "sync-all" ? "同步中..." : "同步全部已启用文件夹"}
          </Button>
          <Button type="button" disabled={!!busyKey} onClick={() => void handleRescanAll(false)}>
            {busyKey === "rescan" ? "重新扫描中..." : "重新扫描全部"}
          </Button>
          <Button type="button" disabled={!!busyKey} title="忽略文件大小与修改时间缓存，强制重新计算校验值" onClick={() => void handleRescanAll(true)}>
            {busyKey === "rescan-force" ? "强制校验中..." : "强制重新校验"}
          </Button>
        </div>
        <div className="muted subwindow-status-text">当前已挂载 {state.folders.length} 个文件夹，其中 {enabledCount} 个已启用。</div>
        {status ? <div className={statusClassName}>{displayPathText(status)}</div> : null}
      </fieldset>

      <BuiltinImportPanel
        state={state}
        busy={!!busyKey}
        onBusyChange={(key) => setBusyKey(key)}
        onStateChange={(next) => setState(next)}
        onStatus={setStatus}
      />

      <fieldset className="subwindow-list-fieldset">
        <legend>{browserRootFolder ? "文件浏览器" : "文件夹列表"}</legend>
        <ScrollArea className="library-folder-scroll"><div className="subwindow-list">
          {browserRootFolder ? (
            <>
              <div className="local-browser-toolbar">
                <Button type="button" onClick={handleCloseBrowser}>返回文件夹列表</Button>
                <Button type="button" disabled={!canGoBrowserUp} onClick={handleGoBrowserUp}>上一级</Button>
                <div className="local-browser-path" title={displayFilePath(browserPath)}>{displayFilePath(browserPath)}</div>
              </div>
              <Table className="local-browser-table">
                <TableHeader>
                  <TableRow>
                    <TableHead className="local-browser-icon-col">图标</TableHead>
                    <TableHead>文件名</TableHead>
                    <TableHead className="local-browser-size-col">大小</TableHead>
                    <TableHead className="local-browser-actions-col">操作</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {isBrowserLoading ? (
                    <TableRow><TableCell colSpan={4}>加载中...</TableCell></TableRow>
                  ) : browserEntries.length === 0 ? (
                    <TableRow><TableCell colSpan={4}>这个文件夹里没有内容。</TableCell></TableRow>
                  ) : browserEntries.map((entry) => {
                    const FileIcon = entry.is_dir ? Folder : isLitematicEntry(entry) ? FileText : File;
                    const canActivate = isLitematicEntry(entry) && !busyKey;
                    const isSelected = selectedBrowserEntryPath === entry.path;
                    return (
                      <TableRow
                        key={entry.path}
                        className={isSelected ? "table-row-selected" : ""}
                        aria-selected={isSelected}
                        onClick={() => setSelectedBrowserEntryPath(entry.path)}
                        onMouseEnter={(event) => {
                          if (!isLitematicEntry(entry)) return;
                          setHoverPreview({ entry, x: event.clientX, y: event.clientY });
                          ensureHoverPreview(entry).catch(() => undefined);
                        }}
                        onMouseMove={(event) => {
                          if (!isLitematicEntry(entry)) return;
                          setHoverPreview((current) => current?.entry.path === entry.path
                            ? { entry, x: event.clientX, y: event.clientY }
                            : current);
                        }}
                        onMouseLeave={() => {
                          if (isLitematicEntry(entry)) setHoverPreview(null);
                        }}
                        onDoubleClick={() => {
                          if (entry.is_dir) {
                            setBrowserPath(entry.path);
                          } else {
                            handleActivateEntry(entry).catch((error) => setStatus(`激活失败：${String(error)}`));
                          }
                        }}
                      >
                        <TableCell className="local-browser-icon-cell">
                          <FileIcon className="local-browser-file-icon" aria-hidden="true" focusable="false" />
                        </TableCell>
                        <TableCell className="local-browser-name-cell" title={displayFilePath(entry.path)}>{entry.name}</TableCell>
                        <TableCell className="local-browser-size-cell">{entry.is_dir ? "-" : formatSize(entry.file_size)}</TableCell>
                        <TableCell className="local-browser-actions-cell">
                          {/* One group, one line: see `.local-browser-action-row` in subwindow.css. */}
                          <div className="button-row local-browser-action-row">
                            <Button
                              type="button"
                              onClick={(event) => {
                                event.stopPropagation();
                                (entry.is_dir ? openWorkspacePath(entry.path) : openFileParentDir(entry.path))
                                  .catch((error) => setStatus(`打开目录失败：${String(error)}`));
                              }}
                            >
                              <span className="local-browser-action-label">打开目录</span>
                            </Button>
                            <Button
                              type="button"
                              disabled={!canActivate}
                              onClick={(event) => {
                                event.stopPropagation();
                                handleActivateEntry(entry).catch((error) => setStatus(`激活失败：${String(error)}`));
                              }}
                            >
                              <span className="local-browser-action-label">{busyKey === `activate:${entry.path}` ? "激活中..." : "激活"}</span>
                            </Button>
                            <Button
                              type="button"
                              disabled={!canActivate}
                              onClick={(event) => {
                                event.stopPropagation();
                                handleOpenSendDialog(entry);
                              }}
                            >
                              <span className="local-browser-action-label">发送...</span>
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </>
          ) : state.folders.length === 0 ? (
            <div className="subwindow-empty-state">还没有挂载任何本地库文件夹。</div>
          ) : state.folders.map((folder) => (
            <div key={folder.path} className="library-card subwindow-card-stretch" onDoubleClick={() => handleOpenBrowser(folder)}>
              <div className="lib-card-left subwindow-card-stack">
                <div className="lib-card-line-main">
                  <div className="lib-card-heading">
                    <span className="lib-card-title" title={displayFilePath(folder.path)}>{compactFolderPath(folder.path, localLibraryTailPathCount)}</span>
                  </div>
                </div>
                <div className="subwindow-status-text">最近同步：{formatDate(folder.lastSyncedAt)}</div>
                <div className="subwindow-status-text">最近扫描：{folder.lastScanCount} 个 .litematic 文件</div>
                {folder.lastError ? <div className="subwindow-status-text-error">{folder.lastError}</div> : null}
                <div className="setting-row subwindow-check-row library-folder-type-row">
                  <span className="library-folder-type-label">类型</span>
                  {folder.type === "builtin_import"
                    ? <span className="library-folder-type-locked" data-folder-type="builtin_import">内置导入（不可改 · 不可移除）</span>
                    : <Dropdown
                        ariaLabel={`${compactFolderPath(folder.path, localLibraryTailPathCount)} 文件夹类型`}
                        value={folder.type}
                        disabled={!!busyKey}
                        onChange={(value) => void handleChangeFolderType(folder, value as LibraryFolderType)}
                        options={[{ value: "tracked", label: "已追踪（扫描入库）" }, { value: "non_archived", label: "非归档（不扫描）" }]}
                      />}
                </div>
                <div className="button-row subwindow-wrap-row">
                  <label className="setting-row subwindow-check-row">
                    <Checkbox
                      checked={folder.enabled}
                      disabled={!!busyKey}
                      onCheckedChange={(checked) => handleToggleFolder(folder, { enabled: checked === true })}
                    />
                    启用此文件夹
                  </label>
                  <label className="setting-row subwindow-check-row">
                    <Checkbox
                      checked={folder.recursive}
                      disabled={!!busyKey}
                      onCheckedChange={(checked) => handleToggleFolder(folder, { recursive: checked === true })}
                    />
                    递归扫描子目录
                  </label>
                  {folder.type !== "builtin_import" && (
                    <label className="setting-row subwindow-check-row" data-folder-auto-activate={folder.autoActivate || undefined}>
                      <Checkbox
                        checked={folder.autoActivate}
                        disabled={!!busyKey}
                        onCheckedChange={(checked) => handleToggleFolder(folder, { autoActivate: checked === true })}
                      />
                      自动激活（新文件自动入库）
                    </label>
                  )}
                </div>
                <div className="button-row subwindow-wrap-row">
                  <Button type="button" disabled={!!busyKey} onClick={() => handleOpenBrowser(folder)}>浏览目录</Button>
                  <Button type="button" disabled={!!busyKey} onClick={() => openWorkspacePath(folder.path)}>{fileManagerLabel}</Button>
                  <Button
                   
                    type="button"
                    disabled={!!busyKey}
                    onClick={() => handleSyncFolder(folder)}
                  >
                    {busyKey === `sync:${folder.path}` ? "同步中..." : "立即同步"}
                  </Button>
                  <Button type="button" disabled={!!busyKey || folder.type === "builtin_import"} title={folder.type === "builtin_import" ? "内置导入文件夹不可移除" : undefined} onClick={() => { setStatus(""); setRemoveFolder(folder); }}>移除</Button>
                </div>
              </div>
            </div>
          ))}
        </div></ScrollArea>
      </fieldset>
      <fieldset className="subwindow-list-fieldset">
        <legend>回收区</legend>
        <TrashPanel
          state={state}
          busy={!!busyKey}
          onBusyChange={(key) => setBusyKey(key)}
          onStateChange={(next) => setState(next)}
          onStatus={setStatus}
        />
      </fieldset>
      {removeFolder && <Dialog title="移除本地库文件夹？" subtitle="只取消这个文件夹的挂载，已有投影记录与电脑上的文件都保持不变。"
        width="sm" busy={!!busyKey} onClose={() => setRemoveFolder(null)}
        footer={<><Button disabled={!!busyKey} onClick={() => setRemoveFolder(null)}>保留</Button><Button variant="destructive" disabled={!!busyKey} onClick={() => handleRemoveFolder(removeFolder)}>{busyKey ? "正在移除…" : "移除挂载"}</Button></>}>
        <div className="dialog-path-preview">{displayFilePath(removeFolder.path)}</div>
        {status && <div className="dialog-message dialog-message-error" role="alert">{displayPathText(status)}</div>}
      </Dialog>}
      {sendDialogRecord ? (
        <SendFileToFolderDialog
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
      <LocalBrowserPreviewPopup
        hover={hoverPreview}
        previewDataUrl={hoverPreview ? previewDataUrls[hoverPreview.entry.path] : undefined}
        isLoading={hoverPreview ? !!previewLoadingPaths[hoverPreview.entry.path] : false}
      />
    </section>
  );
}

/**
 * Overlay dialog form of the local-library-folder manager.
 */
export function LocalLibraryFoldersDialog({ onClose }: { onClose: () => void }) {
  return (
    <Dialog title="本地库管理" width="lg" className="library-window-dialog" onClose={onClose}>
      <LocalLibraryFoldersPanel />
    </Dialog>
  );
}
