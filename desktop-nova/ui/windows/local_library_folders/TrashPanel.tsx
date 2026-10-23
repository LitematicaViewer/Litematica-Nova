import { useState } from "react";
import { RotateCcw, Trash2 } from "lucide-react";
import { Button } from "../../components/ui/button";
import { ThemeIcon } from "../../components/ThemeIcon";
import {
  displayFilePath,
  LibraryState,
  LibraryTrashEntry,
  purgeLibraryTrashAsync,
  restoreLibraryTrashAsync,
} from "../../../src/business/facade";
import { ReplicaNameEditor, isReplicaNameValid } from "../main/pages/library/ReplicaNameEditor";
import { formatEntryDate } from "../main/pages/library/libraryEntryView";

/**
 * Trash panel (E.7, 5.6).
 *
 * Lists the app-managed recycle bin (`projection-library/trash`). Restore moves a
 * file back to its original folder; an occupied target offers a rename through the
 * shared `ReplicaNameEditor` (裁决 17). `legacy` records have no physical file and
 * can only be cleared, never restored (裁决 6). Clear = physical delete; items that
 * cannot be deleted are kept and reported (5.6.3.2). Trash replicas never count as
 * entry members or 重数 (5.6.1.4).
 */
export function TrashPanel({
  state,
  onStateChange,
  onStatus,
  busy,
  onBusyChange,
}: {
  state: LibraryState;
  onStateChange: (state: LibraryState) => void;
  onStatus: (message: string) => void;
  busy: boolean;
  onBusyChange: (key: string) => void;
}) {
  const [conflictId, setConflictId] = useState("");
  const [renameDraft, setRenameDraft] = useState("");

  const entries = state.trash || [];

  const handleRestore = async (entry: LibraryTrashEntry, renameTo?: string) => {
    if (busy) return;
    onBusyChange(`restore:${entry.id}`);
    try {
      const result = await restoreLibraryTrashAsync(state, entry.id, renameTo);
      onStateChange(result.state);
      if (result.ok) {
        setConflictId("");
        setRenameDraft("");
        onStatus(`已还原：${entry.originalFileName || entry.name}`);
      } else if (result.reason === "occupied") {
        setConflictId(entry.id);
        setRenameDraft(entry.originalFileName || entry.name);
        onStatus("原位置已存在同名文件，请输入新的文件名后重试。");
      } else if (result.reason === "legacy") {
        onStatus("历史记录不可还原，只能清理。");
      } else {
        onStatus(`还原失败：${result.reason || "未知原因"}`);
      }
    } catch (error: any) {
      onStatus(`还原失败：${String(error)}`);
    } finally {
      onBusyChange("");
    }
  };

  const handlePurge = async (entry: LibraryTrashEntry) => {
    if (busy) return;
    onBusyChange(`purge:${entry.id}`);
    try {
      const result = await purgeLibraryTrashAsync(state, [entry.id]);
      onStateChange(result.state);
      onStatus(result.failures.length
        ? `未删除：${result.failures.map((failure) => failure.trashPath).join("、")}`
        : `已清理：${entry.name || entry.originalFileName}`);
    } catch (error: any) {
      onStatus(`清理失败：${String(error)}`);
    } finally {
      onBusyChange("");
    }
  };

  const handlePurgeAll = async () => {
    if (busy || !entries.length) return;
    onBusyChange("purge-all");
    try {
      const result = await purgeLibraryTrashAsync(state);
      onStateChange(result.state);
      onStatus(result.failures.length
        ? `清空完成，但有 ${result.failures.length} 项未能删除并已保留。`
        : "已清空回收区。");
    } catch (error: any) {
      onStatus(`清空回收区失败：${String(error)}`);
    } finally {
      onBusyChange("");
    }
  };

  return (
    <section className="library-trash-panel" aria-label="回收区">
      <div className="library-trash-panel-heading">
        <h4 className="library-trash-panel-title">回收区</h4>
        <span className="muted library-trash-panel-count">共 {entries.length} 项</span>
      </div>
      {entries.length === 0 ? (
        <p className="muted">回收区为空。</p>
      ) : (
        <ul className="library-trash-list">
          {entries.map((entry) => {
            const isLegacy = entry.status === "legacy";
            const isConflict = conflictId === entry.id;
            return (
              <li key={entry.id} data-trash-status={entry.status}>
                <div>
                  <span className="library-trash-name">{entry.name || entry.originalFileName || "未命名"}</span>
                  <small className="muted">
                    {isLegacy ? "历史记录（不可还原）" : `${entry.status === "in_trash" ? "在回收区" : entry.status} · ${formatEntryDate(entry.movedAt)}`}
                  </small>
                  {!isLegacy && entry.originalPath ? <small className="muted" title={displayFilePath(entry.originalPath)}>{displayFilePath(entry.originalPath)}</small> : null}
                  {isConflict && (
                    <div className="library-trash-conflict">
                      <ReplicaNameEditor
                        value={renameDraft}
                        disabled={busy}
                        autoFocus
                        ariaLabel="还原冲突的新文件名"
                        onChange={setRenameDraft}
                      />
                      <Button
                        size="sm"
                        disabled={busy || !isReplicaNameValid(renameDraft)}
                        onClick={() => void handleRestore(entry, renameDraft)}
                      >
                        改名还原
                      </Button>
                    </div>
                  )}
                </div>
                <div className="library-trash-actions">
                  {!isLegacy && (
                    <Button
                      size="sm"
                      disabled={busy}
                      onClick={() => void handleRestore(entry)}
                    >
                      <ThemeIcon name="refresh" size={14} fallback={RotateCcw} />还原
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => void handlePurge(entry)}
                  >
                    <ThemeIcon name="delete" size={14} fallback={Trash2} />{isLegacy ? "清理记录" : "删除"}
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
      {entries.length > 0 && (
        <div className="library-trash-footer">
          <span className="muted">清空将物理删除回收区中的文件，且不可恢复。</span>
          <Button variant="destructive" size="sm" disabled={busy} onClick={() => void handlePurgeAll()} aria-busy={busy}>
            清空回收区
          </Button>
        </div>
      )}
    </section>
  );
}