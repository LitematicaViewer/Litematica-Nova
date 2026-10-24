import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
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
  return timestamp ? new Intl.DateTimeFormat(getUiLocale(), { dateStyle: "short", timeStyle: "short" }).format(timestamp) : "-";
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
  return index === 0 ? `${new Intl.NumberFormat(getUiLocale()).format(Math.floor(value))} ${units[index]}` : `${new Intl.NumberFormat(getUiLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(value)} ${units[index]}`;
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
  return navigator.platform.toLowerCase().includes("mac") ? t("library.openFinder") : t("library.openExplorer");
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
  useI18n();
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
          <span>{isLoading ? t("library.loading") : t("library.noEmbeddedPreview")}</span>
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
useI18n();
  const [state, setState] = useState<LibraryState>(() => normalizeLibraryState(null));
  const [busyKey, setBusyKey] = useState("");
  const [status, setStatus] = useState<UiMessage>("");
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
  const [sendError, setSendError] = useState<UiMessage>("");
  const [isSending, setIsSending] = useState(false);
  const [hoverPreview, setHoverPreview] = useState<HoverPreviewEntry | null>(null);
  const [previewDataUrls, setPreviewDataUrls] = useState<Record<string, string | null>>({});
  const [previewLoadingPaths, setPreviewLoadingPaths] = useState<Record<string, boolean>>({});
  const [selectedBrowserEntryPath, setSelectedBrowserEntryPath] = useState("");

  useEffect(() => {
    loadLibrary().then(setState).catch((error) => setStatus(message("library.localConfigReadFailed", { error: String(error) })));
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
          setStatus(message("library.folderReadFailed", { error: String(error) }));
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
      setStatus(message("library.folderDialogFailed", { error: String(error) }));
      return null;
    });
    if (!folderPath) return;
    setBusyKey("add");
    try {
      let nextState = await addLocalLibraryFolder(state, folderPath);
      setState(nextState);
      setStatus(message("library.localFolderAdded", { path: displayFilePath(folderPath) }));
      await emitLibraryStateChanged();
      // B.1: mounting a folder auto-imports its new files when auto-activate is on.
      const scanned = await scanLibraryFolders(nextState, { onlyFolderPath: folderPath });
      nextState = scanned.state;
      setState(nextState);
      if (scanned.failures.length) setStatus(`已添加，但有 ${scanned.failures.length} 个文件未能扫描。`);
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(message("library.localFolderAddFailed", { error: String(error) }));
    } finally {
      setBusyKey("");
    }
  };

  const handleToggleFolder = async (folder: LocalLibraryFolder, patch: Partial<Pick<LocalLibraryFolder, "enabled" | "recursive" | "autoActivate">>) => {
    setBusyKey(`toggle:${folder.path}`);
    try {
      let nextState = await updateLocalLibraryFolder(state, folder.path, patch);
      setState(nextState);
setStatus(message("library.folderSettingsUpdated", { path: displayFilePath(folder.path) }));
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
      setStatus(message("library.folderSettingsFailed", { error: String(error) }));
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
      setStatus(message("library.localFolderRemoved", { path: displayFilePath(folder.path) }));
      setRemoveFolder(null);
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(message("library.localFolderRemoveFailed", { error: String(error) }));
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
          ? message("library.folderSyncErrors", { count: result.errors.length })
          : message("library.foldersScanned", { folders: message("library.folderCount", { count: result.scannedFolderCount }), files: message("library.litematicCount", { count: result.importedCount }) }),
      );
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(message("library.folderSyncFailed", { error: String(error) }));
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
          ? message("library.batchSyncErrors", { count: result.errors.length })
          : message("library.foldersScanned", { folders: message("library.folderCount", { count: result.scannedFolderCount }), files: message("library.litematicCount", { count: result.importedCount }) }),
      );
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(message("library.batchSyncFailed", { error: String(error) }));
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
      setStatus(message("library.activatedPath", { path: displayFilePath(entry.path) }));
      await emitEvent(projectionLibraryImportedEvent, { path: entry.path });
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(message("library.activationFailed", { error: String(error) }));
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
      setSendError(message("library.selectTargetFolder"));
      return;
    }
    if (!normalizedFileName) {
      setSendError(message("library.enterTargetFileName"));
      return;
    }
    setIsSending(true);
    setSendError("");
    try {
      const output = await copyFileToDirectory(sendDialogRecord.path, sendTargetDirectory, normalizedFileName, sendOverwrite);
      setStatus(message("library.sentPath", { path: displayFilePath(output.target_path) }));
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
    (typeof status === "string" ? /失败|错误|failed|error/i.test(status) : /Failed|Errors/.test(status.key)) ? "subwindow-status-text-error" : "",
  ].filter(Boolean).join(" ");

  return (
    <section className="subwindow-panel-shell library-aux-panel library-folders-panel">
      <div className="subwindow-title-bar">
          <h3 className="subwindow-title">{t("library.localFoldersTitle")}</h3>
          {onClose && <Button className="subwindow-close-button" type="button" aria-label={t("library.closeWindow")} onClick={onClose}><X size={18} aria-hidden="true" /></Button>}
      </div>
      <p className="muted subwindow-subtitle">{t("library.localFoldersDescription")}</p>

      <fieldset>
        <legend>{t("library.actions")}</legend>
        <div className="button-row subwindow-wrap-row subwindow-toolbar-group">
          <Button type="button" disabled={!!busyKey} onClick={handleAddFolder}>{t("library.addFolder")}</Button>
          <Button
           
            type="button"
            disabled={!!busyKey || enabledCount === 0}
            onClick={handleSyncAll}
          >
            {busyKey === "sync-all" ? t("library.syncing") : t("library.syncAllFolders")}
          </Button>
          <Button type="button" disabled={!!busyKey} onClick={() => void handleRescanAll(false)}>
            {busyKey === "rescan" ? "重新扫描中..." : "重新扫描全部"}
          </Button>
          <Button type="button" disabled={!!busyKey} title="忽略文件大小与修改时间缓存，强制重新计算校验值" onClick={() => void handleRescanAll(true)}>
            {busyKey === "rescan-force" ? "强制校验中..." : "强制重新校验"}
          </Button>
        </div>
        <div className="muted subwindow-status-text">{t("library.mountedFolders", { count: state.folders.length, enabled: enabledCount })}</div>
        {status ? <div className={statusClassName}>{displayPathText(localizeMessage(status))}</div> : null}
      </fieldset>

      <BuiltinImportPanel
        state={state}
        busy={!!busyKey}
        onBusyChange={(key) => setBusyKey(key)}
        onStateChange={(next) => setState(next)}
        onStatus={setStatus}
      />

      <fieldset className="subwindow-list-fieldset">
        <legend>{browserRootFolder ? t("library.fileBrowser") : t("library.folderList")}</legend>
        <ScrollArea className="library-folder-scroll"><div className="subwindow-list">
          {browserRootFolder ? (
            <>
              <div className="local-browser-toolbar">
                <Button type="button" onClick={handleCloseBrowser}>{t("library.backToFolders")}</Button>
                <Button type="button" disabled={!canGoBrowserUp} onClick={handleGoBrowserUp}>{t("library.parentFolder")}</Button>
                <div className="local-browser-path" title={displayFilePath(browserPath)}>{displayFilePath(browserPath)}</div>
              </div>
              <Table className="local-browser-table">
                <TableHeader>
                  <TableRow>
                    <TableHead className="local-browser-icon-col">{t("library.icon")}</TableHead>
                    <TableHead>{t("library.fileName")}</TableHead>
                    <TableHead className="local-browser-size-col">{t("library.size")}</TableHead>
                    <TableHead className="local-browser-actions-col">{t("library.actions")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {isBrowserLoading ? (
                    <TableRow><TableCell colSpan={4}>{t("library.loading")}</TableCell></TableRow>
                  ) : browserEntries.length === 0 ? (
                    <TableRow><TableCell colSpan={4}>{t("library.folderEmpty")}</TableCell></TableRow>
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
                            handleActivateEntry(entry).catch((error) => setStatus(message("library.activationFailed", { error: String(error) })));
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
                                  .catch((error) => setStatus(message("library.directoryOpenFailed", { error: String(error) })));
                              }}
                            >
                              <span className="local-browser-action-label">{t("library.openDirectory")}</span>
                            </Button>
                            <Button
                              type="button"
                              disabled={!canActivate}
                              onClick={(event) => {
                                event.stopPropagation();
                                handleActivateEntry(entry).catch((error) => setStatus(message("library.activationFailed", { error: String(error) })));
                              }}
                            >
                              <span className="local-browser-action-label">{busyKey === `activate:${entry.path}` ? t("library.activating") : t("library.activate")}</span>
                            </Button>
                            <Button
                              type="button"
                              disabled={!canActivate}
                              onClick={(event) => {
                                event.stopPropagation();
                                handleOpenSendDialog(entry);
                              }}
                            >
                              <span className="local-browser-action-label">{t("library.sendMenu")}</span>
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
            <div className="subwindow-empty-state">{t("library.noMountedFolders")}</div>
          ) : state.folders.map((folder) => (
            <div key={folder.path} className="library-card subwindow-card-stretch" onDoubleClick={() => handleOpenBrowser(folder)}>
              <div className="lib-card-left subwindow-card-stack">
                <div className="lib-card-line-main">
                  <div className="lib-card-heading">
                    <span className="lib-card-title" title={displayFilePath(folder.path)}>{compactFolderPath(folder.path, localLibraryTailPathCount)}</span>
                  </div>
                </div>
<div className="subwindow-status-text">{t("library.lastSync")}{formatDate(folder.lastSyncedAt)}</div>
                <div className="subwindow-status-text">{t("library.lastScannedCount", { count: folder.lastScanCount })}</div>
                {folder.lastError ? <div className="subwindow-status-text-error">{localizeMessage(folder.lastError)}</div> : null}
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
                    {t("library.enableFolder")}</label>
                  <label className="setting-row subwindow-check-row">
                    <Checkbox
                      checked={folder.recursive}
                      disabled={!!busyKey}
                      onCheckedChange={(checked) => handleToggleFolder(folder, { recursive: checked === true })}
                    />
{t("library.recursiveScan")}</label>
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
                  <Button type="button" disabled={!!busyKey} onClick={() => handleOpenBrowser(folder)}>{t("library.browseFolder")}</Button>
                  <Button type="button" disabled={!!busyKey} onClick={() => openWorkspacePath(folder.path)}>{fileManagerLabel}</Button>
                  <Button
                   
                    type="button"
                    disabled={!!busyKey}
                    onClick={() => handleSyncFolder(folder)}
                  >
                    {busyKey === `sync:${folder.path}` ? t("library.syncing") : t("library.syncNow")}
                  </Button>
<Button type="button" disabled={!!busyKey || folder.type === "builtin_import"} title={folder.type === "builtin_import" ? "内置导入文件夹不可移除" : undefined} onClick={() => { setStatus(""); setRemoveFolder(folder); }}>{t("library.remove")}</Button>
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
      {removeFolder && <Dialog title={t("library.removeLocalFolderConfirm")} subtitle={t("library.removeLocalFolderHint")}
        width="sm" busy={!!busyKey} onClose={() => setRemoveFolder(null)}
        footer={<><Button disabled={!!busyKey} onClick={() => setRemoveFolder(null)}>{t("library.keep")}</Button><Button variant="destructive" disabled={!!busyKey} onClick={() => handleRemoveFolder(removeFolder)}>{busyKey ? t("library.removing") : t("library.removeMount")}</Button></>}>
        <div className="dialog-path-preview">{displayFilePath(removeFolder.path)}</div>
        {status && <div className="dialog-message dialog-message-error" role="alert">{displayPathText(localizeMessage(status))}</div>}
      </Dialog>}
      {sendDialogRecord ? (
        <SendFileToFolderDialog
          record={sendDialogRecord}
          folders={state.folders}
          targetDirectory={sendTargetDirectory}
          targetFileName={sendTargetFileName}
          overwrite={sendOverwrite}
          isSending={isSending}
          error={localizeMessage(sendError)}
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
  useI18n();
  return (
    <Dialog title={t("library.localFoldersTitle")} width="lg" className="library-window-dialog" onClose={onClose}>
      <LocalLibraryFoldersPanel />
    </Dialog>
  );
}
