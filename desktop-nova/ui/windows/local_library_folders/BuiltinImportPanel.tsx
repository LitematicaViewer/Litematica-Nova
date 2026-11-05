import { t } from "../../../src/services/i18n";
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
    onBusyChange("builtin-import");
    try {
      const picked = await pickLitematicFile();
      if (!picked) return;
      const outcome = await importFilesToBuiltinFolder(state, [picked]);
      onStateChange(outcome.state);
      onStatus(outcome.failures.length
        ? t("library.ux.builtinImportPartial", { value0: outcome.failures.length })
        : t("library.ux.builtinImported", { value0: displayFilePath(picked) }));
    } catch (error: any) {
      onStatus(t("library.ux.builtinImportFailed", { value0: String(error) }));
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
      onStatus(result.errors.length ? t("library.ux.builtinSyncPartial", { value0: result.errors.length }) : t("library.ux.builtinSynced"));
    } catch (error: any) {
      onStatus(t("library.ux.builtinSyncFailed", { value0: String(error) }));
    } finally {
      onBusyChange("");
    }
  };

  const handleToggleEnabled = async (folderValue: LocalLibraryFolder, enabled: boolean) => {
    if (busy) return;
    onBusyChange("builtin-toggle");
    try {
      onStateChange(await updateLocalLibraryFolder(state, folderValue.path, { enabled }));
      onStatus(enabled ? t("library.ux.builtinEnabled") : t("library.ux.builtinDisabled"));
    } catch (error: any) {
      onStatus(t("library.ux.builtinUpdateFailed", { value0: String(error) }));
    } finally {
      onBusyChange("");
    }
  };

  return (
    <section className="library-builtin-import" data-folder-type="builtin_import" aria-label={t("library.ux.builtinTitle")}>
      <div className="library-builtin-import-heading">
        <h4 className="library-builtin-import-title">{t("library.ux.builtinTitle")}</h4>
        <span className="library-builtin-import-hint">{t("library.ux.builtinProtected")}</span>
      </div>
      {folder ? (
        <>
          <p className="library-builtin-import-path" title={displayFilePath(folder.path)}>{compactFolderPath(folder.path, 3)}</p>
          <div className="library-builtin-import-meta">
            <span>{t("library.lastSync")}{formatEntryDate(folder.lastSyncedAt)}</span>
            <span>{t("library.lastScan")}{folder.lastScanCount} {t("library.ux.builtinFilesUnit")}</span>
          </div>
          {folder.lastError ? <div className="subwindow-status-text-error">{folder.lastError}</div> : null}
          <label className="setting-row subwindow-check-row">
            <Checkbox
              checked={folder.enabled}
              disabled={busy}
              onCheckedChange={(checked) => handleToggleEnabled(folder, checked === true)}
            />
            {t("library.enableFolder")}
          </label>
          <div className="button-row subwindow-wrap-row">
            <Button type="button" disabled={busy} onClick={() => void handleImportFiles()}>
              <ThemeIcon name="import" size={16} fallback={FolderInput} />{t("library.ux.builtinImport")}
            </Button>
            <Button type="button" disabled={busy} onClick={() => void handleSync()} aria-busy={busy}>
              <ThemeIcon name="refresh" size={16} fallback={RefreshCw} />{t("library.syncNow")}
            </Button>
            <Button type="button" disabled={busy} onClick={() => void openWorkspacePath(folder.path).catch((error) => onStatus(t("library.ux.openDirectoryFailed", { value0: String(error) })))}>
              <ThemeIcon name="explorer" size={16} fallback={FolderOpen} />{t("library.openDirectory")}
            </Button>
          </div>
        </>
      ) : (
        <p className="muted">{t("library.ux.builtinCreateHint")}</p>
      )}
      <p className="muted library-builtin-import-note">
        {t("library.ux.builtinCopyHint")}
      </p>
    </section>
  );
}