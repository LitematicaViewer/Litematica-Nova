import { useRef, useState } from "react";
import { Dialog } from "../../components/Dialog";
import { getUiLocale, t } from "../../../src/services/i18n";
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
  const [confirmation, setConfirmation] = useState<{ ids: string[]; names: string[] } | null>(null);
  const [confirmationError, setConfirmationError] = useState("");
  const purgeRunning = useRef(false);
  const english = getUiLocale() === "en-US";

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
        onStatus(t("library.ux.restored", { value0: entry.originalFileName || entry.name }));
      } else if (result.reason === "occupied") {
        setConflictId(entry.id);
        setRenameDraft(entry.originalFileName || entry.name);
        onStatus(t("library.ux.restoreConflict"));
      } else if (result.reason === "legacy") {
        onStatus(t("library.ux.restoreHistory"));
      } else {
        onStatus(t("library.ux.restoreFailed", { value0: result.reason || t("library.ux.unknownReason") }));
      }
    } catch (error: any) {
      onStatus(t("library.ux.restoreFailed", { value0: String(error) }));
    } finally {
      onBusyChange("");
    }
  };

  const requestPurge = (selected: LibraryTrashEntry[]) => {
    if (busy || purgeRunning.current || !selected.length) return;
    setConfirmationError("");
    setConfirmation({ ids: selected.map(entry => entry.id), names: selected.map(entry => entry.name || entry.originalFileName) });
  };
  const confirmPurge = async () => {
    if (!confirmation || busy || purgeRunning.current) return;
    purgeRunning.current = true;
    onBusyChange("purge-confirmed"); setConfirmationError("");
    try {
      // Keep the explicitly confirmed identities; later additions are never deleted.
      const result = await purgeLibraryTrashAsync(state, confirmation.ids);
      onStateChange(result.state); setConfirmation(null);
      onStatus(result.failures.length
        ? (english ? `${result.failures.length} items could not be deleted and were retained.` : t("library.ux.purgePartial", { value0: result.failures.length }))
        : (english ? "Selected trash items were deleted." : t("library.ux.purged")));
    } catch (error) { setConfirmationError(String(error)); }
    finally { purgeRunning.current = false; onBusyChange(""); }
  };

  return (
    <section className="library-trash-panel" aria-label={t("library.ux.recycle")}>
      <div className="library-trash-panel-heading">
        <span className="muted library-trash-panel-count">{t("library.ux.total")} {entries.length} {t("library.items")}</span>
      </div>
      {entries.length === 0 ? (
        <p className="muted">{t("library.ux.trashEmpty")}</p>
      ) : (
        <ul className="library-trash-list">
          {entries.map((entry) => {
            const isLegacy = entry.status === "legacy";
            const isConflict = conflictId === entry.id;
            return (
              <li key={entry.id} data-trash-status={entry.status}>
                <div>
                  <span className="library-trash-name">{entry.name || entry.originalFileName || t("library.ux.unnamed")}</span>
                  <small className="muted">
                    {isLegacy ? t("library.ux.historyUnrestorable") : `${entry.status === "in_trash" ? t("library.ux.inTrash") : entry.status} · ${formatEntryDate(entry.movedAt)}`}
                  </small>
                  {!isLegacy && entry.originalPath ? <small className="muted" title={displayFilePath(entry.originalPath)}>{displayFilePath(entry.originalPath)}</small> : null}
                  {isConflict && (
                    <div className="library-trash-conflict">
                      <ReplicaNameEditor
                        value={renameDraft}
                        disabled={busy}
                        autoFocus
                        ariaLabel={t("library.ux.restoreFileName")}
                        onChange={setRenameDraft}
                      />
                      <Button
                        size="sm"
                        disabled={busy || !isReplicaNameValid(renameDraft)}
                        onClick={() => void handleRestore(entry, renameDraft)}
                      >
                        {t("library.ux.renameRestore")}
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
                      <ThemeIcon name="refresh" size={14} fallback={RotateCcw} />{t("library.ux.restoreAction")}
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => requestPurge([entry])}
                  >
                    <ThemeIcon name="delete" size={14} fallback={Trash2} />{isLegacy ? t("library.ux.purgeRecord") : t("editor.delete")}
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
      {entries.length > 0 && (
        <div className="library-trash-footer">
          <span className="muted">{t("library.ux.purgeHint")}</span>
          <Button variant="destructive" size="sm" disabled={busy} onClick={() => requestPurge(entries)} aria-busy={busy}>
            {t("library.ux.emptyTrash")}
          </Button>
        </div>
      )}
      {confirmation && <Dialog title={english ? "Permanently delete trash items?" : t("library.ux.purgeTitle")}
        subtitle={english ? "The selected trash files will be deleted permanently. Other files and library items are unchanged." : t("library.ux.purgeConfirmHint")}
        width="sm" busy={busy} onClose={() => setConfirmation(null)} footer={<>
          <Button variant="ghost" disabled={busy} onClick={() => setConfirmation(null)}>{t("editor.cancel")}</Button>
          <Button variant="destructive" disabled={busy} onClick={() => void confirmPurge()}>{english ? "Delete permanently" : t("library.ux.purgeAction")}</Button>
        </>}>
        <ul>{confirmation.names.map((name, index) => <li key={confirmation.ids[index]}>{name}</li>)}</ul>
        {confirmationError && <p className="dialog-message dialog-message-error" role="alert">{confirmationError}</p>}
      </Dialog>}
    </section>
  );
}
