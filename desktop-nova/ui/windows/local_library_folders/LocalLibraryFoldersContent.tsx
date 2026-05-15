import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { SyntheticEvent } from "react";

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
  normalizeMaterialListWindowBehavior,
  openFileParentDir,
  openLocalLibraryFoldersWindow,
  openWorkspacePath,
  ProjectionRecord,
  readProjectionPreviewImage,
  removeLocalLibraryFolder,
  syncAllLocalLibraryFolders,
  syncLocalLibraryFolder,
  updateLocalLibraryFolder,
} from "../../../src/business/facade";
import { confirmDialog } from "../../../src/platform/dialogs";
import { emitEvent } from "../../../src/platform/events";
import { currentThemeId, subscribeToThemeChanges, themeResourceKey } from "../../shell/themeRuntime";
import { projectionLibraryImportedEvent, projectionLibraryStateChangedEvent } from "../libraryEvents";
import { SendProjectionDialog, ensureLitematicFileName } from "../main/pages/library/sendDialog";

type FileIconUrls = Record<string, string>;
type HoverPreviewEntry = { entry: DirectoryEntryInfo; x: number; y: number };

const iconNameFromPath = (path: string) => path.split("/").pop()?.replace(/\.[^.]+$/, "") || "";

const shellFileIconUrls = Object.entries(
  import.meta.glob<string>("../../shell/resource/icon/file/*.{svg,png}", {
    eager: true,
    import: "default",
    query: "?url",
  }),
).reduce<FileIconUrls>((icons, [path, url]) => {
  const iconName = iconNameFromPath(path);
  if (iconName) icons[iconName] = url;
  return icons;
}, {});

const themedFileIconUrls = Object.entries(
  import.meta.glob<string>("../../themes/*/resource/icon/file/*.{svg,png}", {
    eager: true,
    import: "default",
    query: "?url",
  }),
).reduce<Record<string, FileIconUrls>>((themes, [path, url]) => {
  const themeKey = path.match(/\.\.\/\.\.\/themes\/([^/]+)\//)?.[1];
  const iconName = iconNameFromPath(path);
  if (themeKey && iconName) {
    themes[themeKey] = themes[themeKey] || {};
    themes[themeKey][iconName] = url;
  }
  return themes;
}, {});

function fallbackFileIconUrl(iconName: string): string {
  return shellFileIconUrls[iconName] || shellFileIconUrls.unknown || shellFileIconUrls.undefined || "";
}

function fileIconUrl(themeId: string, iconName: string): string {
  const themeKey = themeResourceKey(themeId);
  return themedFileIconUrls[themeKey]?.[iconName]
    || themedFileIconUrls[themeKey]?.unknown
    || themedFileIconUrls[themeKey]?.undefined
    || fallbackFileIconUrl(iconName);
}

function handleFileIconError(event: SyntheticEvent<HTMLImageElement>, iconName: string) {
  const fallbackUrl = fallbackFileIconUrl(iconName);
  if (fallbackUrl && event.currentTarget.src !== fallbackUrl) {
    event.currentTarget.src = fallbackUrl;
  }
}

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

function recordFromEntry(entry: DirectoryEntryInfo): ProjectionRecord {
  return {
    id: entry.path,
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
    status: "ok",
    lastAnalyzedAt: entry.mtime_ms,
    lastError: "",
    sort_index: 0,
  };
}

function pickDefaultSendFolder(record: ProjectionRecord, folders: LocalLibraryFolder[]): string {
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
  const [state, setState] = useState<LibraryState>({ records: [], folders: [] });
  const [busyKey, setBusyKey] = useState("");
  const [status, setStatus] = useState("");
  const [themeId, setThemeId] = useState(() => currentThemeId());
  const [browserRootFolder, setBrowserRootFolder] = useState<LocalLibraryFolder | null>(null);
  const [browserPath, setBrowserPath] = useState("");
  const [browserEntries, setBrowserEntries] = useState<DirectoryEntryInfo[]>([]);
  const [isBrowserLoading, setIsBrowserLoading] = useState(false);
  const [sendDialogRecord, setSendDialogRecord] = useState<ProjectionRecord | null>(null);
  const [sendTargetDirectory, setSendTargetDirectory] = useState("");
  const [sendTargetFileName, setSendTargetFileName] = useState("");
  const [sendOverwrite, setSendOverwrite] = useState(false);
  const [sendError, setSendError] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [hoverPreview, setHoverPreview] = useState<HoverPreviewEntry | null>(null);
  const [previewDataUrls, setPreviewDataUrls] = useState<Record<string, string | null>>({});
  const [previewLoadingPaths, setPreviewLoadingPaths] = useState<Record<string, boolean>>({});

  useEffect(() => {
    loadLibrary().then(setState).catch((error) => setStatus(`读取本地库配置失败：${String(error)}`));
  }, []);

  useEffect(() => subscribeToThemeChanges(setThemeId), []);

  useEffect(() => {
    if (!browserPath) {
      setBrowserEntries([]);
      return;
    }
    let cancelled = false;
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
      const nextState = await addLocalLibraryFolder(state, folderPath);
      setState(nextState);
      setStatus(`已添加本地库文件夹：${folderPath}`);
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`添加本地库文件夹失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleToggleFolder = async (folder: LocalLibraryFolder, patch: Partial<Pick<LocalLibraryFolder, "enabled" | "recursive">>) => {
    setBusyKey(`toggle:${folder.path}`);
    try {
      const nextState = await updateLocalLibraryFolder(state, folder.path, patch);
      setState(nextState);
      setStatus(`已更新文件夹设置：${folder.path}`);
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`更新文件夹设置失败：${String(error)}`);
    } finally {
      setBusyKey("");
    }
  };

  const handleRemoveFolder = async (folder: LocalLibraryFolder) => {
    const ok = await confirmDialog(`确定移除本地库文件夹？\n${folder.path}`, {
      title: "移除本地库文件夹",
      kind: "warning",
    });
    if (!ok) return;
    setBusyKey(`remove:${folder.path}`);
    try {
      const nextState = await removeLocalLibraryFolder(state, folder.path);
      setState(nextState);
      setStatus(`已移除本地库文件夹：${folder.path}`);
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

  const handleOpenBrowser = (folder: LocalLibraryFolder) => {
    setBrowserRootFolder(folder);
    setBrowserPath(folder.path);
    setStatus("");
  };

  const handleCloseBrowser = () => {
    setBrowserRootFolder(null);
    setBrowserPath("");
    setBrowserEntries([]);
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
      setStatus(`已激活：${entry.path}`);
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
      setStatus(`已发送到：${output.target_path}`);
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
  const statusClassName = [
    "subwindow-status-text",
    status.includes("失败") || status.includes("错误") ? "subwindow-status-text-error" : "",
  ].filter(Boolean).join(" ");

  return (
    <section className="subwindow-panel-shell">
      <div className="subwindow-title-bar">
          <h3 className="subwindow-title">本地库管理</h3>
          {onClose && <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}>×</button>}
      </div>
      <p className="muted subwindow-subtitle">管理挂载目录，并把目录中的 .litematic 文件同步到投影库记录。</p>

      <fieldset>
        <legend>操作</legend>
        <div className="button-row subwindow-wrap-row subwindow-toolbar-group">
          <button className="btn" type="button" disabled={!!busyKey} onClick={handleAddFolder}>添加文件夹...</button>
          <button
            className="btn"
            type="button"
            disabled={!!busyKey || enabledCount === 0}
            onClick={handleSyncAll}
          >
            {busyKey === "sync-all" ? "同步中..." : "同步全部已启用文件夹"}
          </button>
        </div>
        <div className="muted subwindow-status-text">当前已挂载 {state.folders.length} 个文件夹，其中 {enabledCount} 个已启用。</div>
        {status ? <div className={statusClassName}>{status}</div> : null}
      </fieldset>

      <fieldset className="subwindow-list-fieldset">
        <legend>{browserRootFolder ? "文件浏览器" : "文件夹列表"}</legend>
        <div className="subwindow-list">
          {browserRootFolder ? (
            <>
              <div className="local-browser-toolbar">
                <button className="btn" type="button" onClick={handleCloseBrowser}>返回文件夹列表</button>
                <button className="btn" type="button" disabled={!canGoBrowserUp} onClick={handleGoBrowserUp}>上一级</button>
                <div className="local-browser-path" title={browserPath}>{browserPath}</div>
              </div>
              <table className="local-browser-table">
                <thead>
                  <tr>
                    <th className="local-browser-icon-col">图标</th>
                    <th>文件名</th>
                    <th className="local-browser-size-col">大小</th>
                    <th className="local-browser-actions-col">操作</th>
                  </tr>
                </thead>
                <tbody>
                  {isBrowserLoading ? (
                    <tr><td colSpan={4}>加载中...</td></tr>
                  ) : browserEntries.length === 0 ? (
                    <tr><td colSpan={4}>这个文件夹里没有内容。</td></tr>
                  ) : browserEntries.map((entry) => {
                    const iconName = entry.is_dir ? "folder" : isLitematicEntry(entry) ? "litematic" : "unknown";
                    const canActivate = isLitematicEntry(entry) && !busyKey;
                    return (
                      <tr
                        key={entry.path}
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
                        <td className="local-browser-icon-cell">
                          <img
                            className="local-browser-file-icon"
                            src={fileIconUrl(themeId, iconName)}
                            alt=""
                            aria-hidden="true"
                            onError={(event) => handleFileIconError(event, iconName)}
                          />
                        </td>
                        <td className="local-browser-name-cell" title={entry.path}>{entry.name}</td>
                        <td className="local-browser-size-cell">{entry.is_dir ? "-" : formatSize(entry.file_size)}</td>
                        <td>
                          <div className="button-row subwindow-wrap-row">
                            <button
                              className="btn"
                              type="button"
                              onClick={(event) => {
                                event.stopPropagation();
                                (entry.is_dir ? openWorkspacePath(entry.path) : openFileParentDir(entry.path))
                                  .catch((error) => setStatus(`打开目录失败：${String(error)}`));
                              }}
                            >
                              打开目录
                            </button>
                            <button
                              className="btn"
                              type="button"
                              disabled={!canActivate}
                              onClick={(event) => {
                                event.stopPropagation();
                                handleActivateEntry(entry).catch((error) => setStatus(`激活失败：${String(error)}`));
                              }}
                            >
                              {busyKey === `activate:${entry.path}` ? "激活中..." : "激活"}
                            </button>
                            <button
                              className="btn"
                              type="button"
                              disabled={!canActivate}
                              onClick={(event) => {
                                event.stopPropagation();
                                handleOpenSendDialog(entry);
                              }}
                            >
                              发送...
                            </button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </>
          ) : state.folders.length === 0 ? (
            <div className="subwindow-empty-state">还没有挂载任何本地库文件夹。</div>
          ) : state.folders.map((folder) => (
            <div key={folder.path} className="library-card subwindow-card-stretch" onDoubleClick={() => handleOpenBrowser(folder)}>
              <div className="lib-card-left subwindow-card-stack">
                <div className="lib-card-line-main">
                  <div className="lib-card-heading">
                    <span className="lib-card-title">{folder.path}</span>
                  </div>
                </div>
                <div className="subwindow-status-text">最近同步：{formatDate(folder.lastSyncedAt)}</div>
                <div className="subwindow-status-text">最近扫描：{folder.lastScanCount} 个 .litematic 文件</div>
                {folder.lastError ? <div className="subwindow-status-text-error">{folder.lastError}</div> : null}
                <div className="button-row subwindow-wrap-row">
                  <label className="setting-row subwindow-check-row">
                    <input
                      type="checkbox"
                      checked={folder.enabled}
                      disabled={!!busyKey}
                      onChange={(event) => handleToggleFolder(folder, { enabled: event.target.checked })}
                    />
                    启用此文件夹
                  </label>
                  <label className="setting-row subwindow-check-row">
                    <input
                      type="checkbox"
                      checked={folder.recursive}
                      disabled={!!busyKey}
                      onChange={(event) => handleToggleFolder(folder, { recursive: event.target.checked })}
                    />
                    递归扫描子目录
                  </label>
                </div>
                <div className="button-row subwindow-wrap-row">
                  <button className="btn" type="button" disabled={!!busyKey} onClick={() => openWorkspacePath(folder.path)}>打开文件夹</button>
                  <button
                    className="btn"
                    type="button"
                    disabled={!!busyKey}
                    onClick={() => handleSyncFolder(folder)}
                  >
                    {busyKey === `sync:${folder.path}` ? "同步中..." : "仅同步此文件夹"}
                  </button>
                  <button className="btn" type="button" disabled={!!busyKey} onClick={() => handleRemoveFolder(folder)}>移除</button>
                </div>
              </div>
            </div>
          ))}
        </div>
      </fieldset>
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
    <div className="dialog-overlay" onClick={onClose}>
      <div className="dialog-content subwindow-frame subwindow-frame-wide" onClick={(event) => event.stopPropagation()}>
        <LocalLibraryFoldersPanel onClose={onClose} />
      </div>
    </div>
  );
}
