import { useEffect, useState } from "react";

import {
  addLocalLibraryFolder,
  chooseUserConfigDir,
  loadLibrary,
  loadUserConfigMigratingLocalStorage,
  LocalLibraryFolder,
  LibraryState,
  normalizeMaterialListWindowBehavior,
  openLocalLibraryFoldersWindow,
  openWorkspacePath,
  removeLocalLibraryFolder,
  syncAllLocalLibraryFolders,
  syncLocalLibraryFolder,
  updateLocalLibraryFolder,
} from "../../../src/business/facade";
import { confirmDialog } from "../../../src/platform/dialogs";
import { emitEvent } from "../../../src/platform/events";
import { projectionLibraryStateChangedEvent } from "../libraryEvents";

function formatDate(timestamp: number): string {
  if (!timestamp) return "-";
  const date = new Date(timestamp);
  const pad = (value: number) => value.toString().padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
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

  useEffect(() => {
    loadLibrary().then(setState).catch((error) => setStatus(`读取本地库配置失败：${String(error)}`));
  }, []);

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
          : `已同步 ${result.scannedFolderCount} 个文件夹，共处理 ${result.importedCount} 个 .litematic 文件。`,
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
          : `已同步 ${result.scannedFolderCount} 个文件夹，共处理 ${result.importedCount} 个 .litematic 文件。`,
      );
      await emitLibraryStateChanged();
    } catch (error: any) {
      setStatus(`批量同步失败：${String(error)}`);
    } finally {
      setBusyKey("");
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
        <legend>文件夹列表</legend>
        <div className="subwindow-list">
          {state.folders.length === 0 ? (
            <div className="subwindow-empty-state">还没有挂载任何本地库文件夹。</div>
          ) : state.folders.map((folder) => (
            <div key={folder.path} className="library-card subwindow-card-stretch">
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