import { FolderOpen, FolderInput, RefreshCw } from "lucide-react";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { ThemeIcon } from "../../components/ThemeIcon";
import {
  displayFilePath,
  importFilesToBuiltinFolder,
  LibraryState,
  LocalLibraryFolder,
  openWorkspacePath,
  pickLitematicFile,
  syncLocalLibraryFolder,
  updateLocalLibraryFolder,
} from "../../../src/business/facade";
import { formatEntryDate } from "../main/pages/library/libraryEntryView";
import { compactFolderPath } from "../main/pages/library/function";

/**
 * Built-in import folder panel (E.1, 5.1.1.7 / 5.1.1.8).
 *
 * The built-in import folder (`projection-library/import`) is always mounted, is
 * tracked (participates in merge + 重数), can be disabled but never removed, and
 * can never be set to non-archived. Imports copy files into it (the copy primitive
 * rebuilds a deleted directory, 5.1.3.2).
 */
export function BuiltinImportPanel({
  state,
  busy,
  onBusyChange,
  onStateChange,
  onStatus,
}: {
  state: LibraryState;
  busy: boolean;
  onBusyChange: (key: string) => void;
  onStateChange: (state: LibraryState) => void;
  onStatus: (message: string) => void;
}) {
  const folder: LocalLibraryFolder | undefined = state.folders.find((item) => item.type === "builtin_import");

  const handleImportFiles = async () => {
    if (busy) return;
    const picked = await pickLitematicFile().catch(() => null);
    if (!picked) return;
    onBusyChange("builtin-import");
    try {
      const outcome = await importFilesToBuiltinFolder(state, [picked]);
      onStateChange(outcome.state);
      onStatus(outcome.failures.length
        ? `导入内置文件夹存在 ${outcome.failures.length} 项失败。`
        : `已导入内置文件夹：${displayFilePath(picked)}`);
    } catch (error: any) {
      onStatus(`导入内置文件夹失败：${String(error)}`);
    } finally {
      onBusyChange("");
    }
  };

  const handleSync = async () => {
    if (busy || !folder) return;
    onBusyChange("builtin-sync");
    try {
      const result = await syncLocalLibraryFolder(state, folder.path);
      onStateChange(result.state);
      onStatus(result.errors.length ? `内置文件夹同步存在 ${result.errors.length} 个错误。` : "内置导入文件夹已同步。");
    } catch (error: any) {
      onStatus(`同步内置导入文件夹失败：${String(error)}`);
    } finally {
      onBusyChange("");
    }
  };

  const handleToggleEnabled = async (folderValue: LocalLibraryFolder, enabled: boolean) => {
    if (busy) return;
    onBusyChange("builtin-toggle");
    try {
      onStateChange(await updateLocalLibraryFolder(state, folderValue.path, { enabled }));
      onStatus(enabled ? "已启用内置导入文件夹。" : "已停用内置导入文件夹。");
    } catch (error: any) {
      onStatus(`更新内置导入文件夹失败：${String(error)}`);
    } finally {
      onBusyChange("");
    }
  };

  return (
    <section className="library-builtin-import" data-folder-type="builtin_import" aria-label="内置导入文件夹">
      <div className="library-builtin-import-heading">
        <h4 className="library-builtin-import-title">内置导入文件夹</h4>
        <span className="library-builtin-import-hint">不可移除 · 禁止设为非归档 · 属已追踪</span>
      </div>
      {folder ? (
        <>
          <p className="library-builtin-import-path" title={displayFilePath(folder.path)}>{compactFolderPath(folder.path, 3)}</p>
          <div className="library-builtin-import-meta">
            <span>最近同步：{formatEntryDate(folder.lastSyncedAt)}</span>
            <span>最近扫描：{folder.lastScanCount} 个 .litematic 文件</span>
          </div>
          {folder.lastError ? <div className="subwindow-status-text-error">{folder.lastError}</div> : null}
          <label className="setting-row subwindow-check-row">
            <Checkbox
              checked={folder.enabled}
              disabled={busy}
              onCheckedChange={(checked) => handleToggleEnabled(folder, checked === true)}
            />
            启用此文件夹
          </label>
          <div className="button-row subwindow-wrap-row">
            <Button type="button" disabled={busy} onClick={() => void handleImportFiles()}>
              <ThemeIcon name="import" size={16} fallback={FolderInput} />导入文件…
            </Button>
            <Button type="button" disabled={busy} onClick={() => void handleSync()} aria-busy={busy}>
              <ThemeIcon name="refresh" size={16} fallback={RefreshCw} />立即同步
            </Button>
            <Button type="button" disabled={busy} onClick={() => void openWorkspacePath(folder.path).catch((error) => onStatus(`打开目录失败：${String(error)}`))}>
              <ThemeIcon name="explorer" size={16} fallback={FolderOpen} />打开目录
            </Button>
          </div>
        </>
      ) : (
        <p className="muted">内置导入文件夹将在下次读取投影库时自动创建。</p>
      )}
      <p className="muted library-builtin-import-note">
        导入的投影会被复制到此文件夹，再参与内容归并与重数统计。
      </p>
    </section>
  );
}