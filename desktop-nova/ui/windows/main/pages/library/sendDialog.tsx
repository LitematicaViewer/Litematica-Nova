import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayFilePath } from "../../../../../src/business/facade";
import { useEffect, useMemo, useState } from "react";

import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../../../components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../../components/ui/table";
import { Dialog } from "../../../../components/Dialog";
import {
  loadUserConfigMigratingLocalStorage,
  LocalLibraryFolder,
  LibraryRecordView,
  normalizeLocalLibraryTailPathCount,
  windowsPathKey,
} from "../../../../../src/business/facade";
import { ReplicaNameEditor } from "./ReplicaNameEditor";
import type { LibraryEntryView } from "./libraryEntryView";

// 同级函数
import { compactFolderPath, ensureLitematicFileName, DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT } from "./function";

export { DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT } from "./function";

/** One dispatch plan produced by the send dialog and executed by the container. */
export interface DispatchPlan {
  additions: { folderPath: string; fileName: string; sourcePath: string }[];
  renames: { replicaId: string; sourcePath: string; fileName: string }[];
  trash: { replicaId: string; sourcePath: string; fileName: string }[];
}

interface DispatchRow {
  key: string;
  folderPath: string;
  replicaId: string | null;
  sequence: number;
  originalName: string;
  name: string;
  included: boolean;
}

/** A target folder is any tracked folder (built-in import counts as tracked); non-archived is never a target. */
function dispatchTargets(folders: LocalLibraryFolder[]): LocalLibraryFolder[] {
  return folders.filter((folder) => folder.type === "tracked" || folder.type === "builtin_import");
}

function replicasUnder(entry: LibraryEntryView, folderPath: string): LibraryRecordView[] {
  const folderKey = windowsPathKey(folderPath);
  return entry.replicas
    .filter((replica) => windowsPathKey(replica.path).startsWith(folderKey))
    .sort((left, right) => left.fileName.localeCompare(right.fileName));
}

function buildRows(entry: LibraryEntryView, folders: LocalLibraryFolder[]): DispatchRow[] {
  const rows: DispatchRow[] = [];
  for (const folder of dispatchTargets(folders)) {
    const replicas = replicasUnder(entry, folder.path);
    replicas.forEach((replica, index) => rows.push({
      key: replica.id,
      folderPath: folder.path,
      replicaId: replica.id,
      sequence: index + 1,
      originalName: replica.fileName,
      name: replica.fileName,
      included: true,
    }));
    // Every target folder offers at least one "序号 + 1" empty slot (spec 5.5.1.3).
    rows.push({
      key: `${windowsPathKey(folder.path)}::slot`,
      folderPath: folder.path,
      replicaId: null,
      sequence: replicas.length + 1,
      originalName: "",
      name: "",
      included: false,
    });
  }
  return rows;
}

/**
 * Dispatch window (D.11): single-entry, multi-row table.
 *
 * Four columns (folder path / editable file name / 重数序号 / checkbox). Checking an
 * empty slot with a name adds a replica; unchecking an existing replica moves it into
 * the trash (never a physical delete); editing an existing row's name renames it. The
 * footer reports the pending 新增 / 重命名 / 入回收区 counts before submission, and an
 * included-but-unnamed empty slot blocks the submit. The actual file work is executed by
 * the container through the background task queue.
 */
export function SendProjectionDialog({
  entry,
  folders,
  busy,
  error,
  onClose,
  onSubmit,
}: {
  entry: LibraryEntryView;
  folders: LocalLibraryFolder[];
  busy: boolean;
  error: string;
  onClose: () => void;
  onSubmit: (plan: DispatchPlan) => void;
}) {
  const [localLibraryTailPathCount, setLocalLibraryTailPathCount] = useState(3);
  const [rows, setRows] = useState<DispatchRow[]>(() => buildRows(entry, folders));

  useEffect(() => {
    loadUserConfigMigratingLocalStorage()
      .then((info) => setLocalLibraryTailPathCount(normalizeLocalLibraryTailPathCount(info.config.local_library_tail_path_count)))
      .catch(() => undefined);
  }, []);

  // The dialog is keyed by entryId. Background library/preview refreshes must
  // not replace the names and inclusion checkboxes currently being edited.

  const replicaPathById = useMemo(() => new Map(entry.replicas.map((replica) => [replica.id, replica.path])), [entry]);

  const update = (key: string, patch: Partial<DispatchRow>) => setRows((current) => current.map((row) => row.key === key ? { ...row, ...patch } : row));

  const additions = rows.filter((row) => row.replicaId === null && row.included && row.name.trim());
  const renames = rows.filter((row) => row.replicaId !== null && row.included && ensureLitematicFileName(row.name) !== row.originalName);
  const trash = rows.filter((row) => row.replicaId !== null && !row.included);
  const invalidSlot = rows.find((row) => row.replicaId === null && row.included && !row.name.trim());
  const changedCount = additions.length + renames.length + trash.length;
  const canSubmit = !busy && !invalidSlot && changedCount > 0;

  const handleSubmit = () => {
    if (!canSubmit) return;
    onSubmit({
      additions: additions.map((row) => ({ folderPath: row.folderPath, fileName: ensureLitematicFileName(row.name), sourcePath: entry.primary.path })),
      renames: renames.map((row) => ({ replicaId: row.replicaId as string, sourcePath: replicaPathById.get(row.replicaId as string) || "", fileName: ensureLitematicFileName(row.name) })),
      trash: trash.map((row) => ({ replicaId: row.replicaId as string, sourcePath: replicaPathById.get(row.replicaId as string) || "", fileName: row.originalName })),
    });
  };

  return (
    <Dialog
      title={t("library.ux.dispatch")}
      subtitle={t("library.ux.dispatchHint", { value0: entry.displayName || entry.fileName })}
      width="lg"
      busy={busy}
      onClose={onClose}
      footer={(
        <>
          <span className="library-dispatch-summary" role="status">{t("library.ux.added")} {additions.length} {t("library.ux.renamedPart")} {renames.length} {t("library.ux.trashPart")} {trash.length}</span>
          <Button type="button" onClick={onClose} disabled={busy}>{t("common.cancel")}</Button>
          <Button type="button" variant="default" onClick={handleSubmit} aria-busy={busy} disabled={!canSubmit}>{busy ? t("library.ux.dispatching") : t("library.ux.confirmDispatch")}</Button>
        </>
      )}
    >
      <div className="dialog-path-preview" title={displayFilePath(entry.primary.path)}>{t("library.ux.sourceFilePrefix")}{displayFilePath(entry.primary.path)}</div>
      <Table className="library-dispatch-table" data-entry-id={entry.entryId}>
        <TableHeader>
          <TableRow>
            <TableHead>{t("library.ux.folderPath")}</TableHead>
            <TableHead>{t("library.fileName")}</TableHead>
            <TableHead className="library-dispatch-seq-col">{t("library.ux.replicaSequence")}</TableHead>
            <TableHead className="library-dispatch-select-col">{t("library.ux.select")}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => {
            const nameError = row.included && row.replicaId === null && !row.name.trim();
            return (
              <TableRow key={row.key} data-empty-slot={row.replicaId === null || undefined} data-included={row.included || undefined}>
                <TableCell className="library-dispatch-folder" title={displayFilePath(row.folderPath)}>{compactFolderPath(row.folderPath, localLibraryTailPathCount)}</TableCell>
                <TableCell className="library-dispatch-name">
                  <ReplicaNameEditor
                    value={row.name}
                    disabled={busy || (row.replicaId !== null && !row.included)}
                    ariaLabel={t("library.ux.copyFileLabel", { value0: compactFolderPath(row.folderPath, localLibraryTailPathCount), value1: row.sequence })}
                    onChange={(value) => update(row.key, { name: value })}
                  />
                  {nameError && <small className="library-dispatch-hint" role="alert">{t("library.ux.enterName")}</small>}
                </TableCell>
                <TableCell className="library-dispatch-seq-col">{row.sequence}</TableCell>
                <TableCell className="library-dispatch-select-col">
                  <Checkbox
                    checked={row.included}
                    disabled={busy}
                    aria-label={`${row.replicaId ? t("library.ux.copyLabel", { value0: row.sequence }) : t("library.ux.newReplica")}`}
                    onCheckedChange={(value) => update(row.key, { included: value === true })}
                  />
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
      {dispatchTargets(folders).length === 0 && <p className="muted">{t("library.ux.noDispatchTarget")}</p>}
      {error && <div className="dialog-message dialog-message-error" role="alert">{error}</div>}
    </Dialog>
  );
}

/**
 * Single-file copy dialog retained for the local-folder browser's "发送..." action
 * (its record may not be a library entry yet). The entry-level dispatch window above
 * is the library page's replacement for the old single-file form.
 */
export function SendFileToFolderDialog({
  record,
  folders,
  targetDirectory,
  targetFileName,
  overwrite,
  isSending,
  error,
  onTargetDirectoryChange,
  onTargetFileNameChange,
  onOverwriteChange,
  onClose,
  onSubmit,
}: {
  record: LibraryRecordView;
  folders: LocalLibraryFolder[];
  targetDirectory: string;
  targetFileName: string;
  overwrite: boolean;
  isSending: boolean;
  error: UiMessage;
  onTargetDirectoryChange: (value: string) => void;
  onTargetFileNameChange: (value: string) => void;
  onOverwriteChange: (value: boolean) => void;
  onClose: () => void;
  onSubmit: () => void;
}) {
useI18n();
  const [localLibraryTailPathCount, setLocalLibraryTailPathCount] = useState(DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT);

  useEffect(() => {
    loadUserConfigMigratingLocalStorage()
      .then((info) => setLocalLibraryTailPathCount(normalizeLocalLibraryTailPathCount(info.config.local_library_tail_path_count)))
      .catch(() => undefined);
  }, []);

  const normalizedFileName = ensureLitematicFileName(targetFileName);
  const targetPathPreview = targetDirectory && normalizedFileName
    ? `${targetDirectory.replace(/[\\/]+$/, "")}/${normalizedFileName}`
    : "-";

  return (
    <Dialog
      title={t("library.copySchematicTitle")}
      subtitle={t("library.copySchematicHint")}
      width="md"
      busy={isSending}
      onClose={onClose}
      footer={(
        <>
          <Button type="button" onClick={onClose} disabled={isSending}>{t("library.cancel")}</Button>
          <Button type="button" onClick={onSubmit} aria-busy={isSending} disabled={isSending || !targetDirectory || !normalizedFileName || folders.length === 0}>
            {isSending ? t("library.copying") : t("library.copy")}
          </Button>
        </>
      )}
    >
      <div className="dialog-form-grid">
        <span className="dialog-label">{t("library.sourceFile")}</span>
        <div className="dialog-path-preview">{displayFilePath(record.path)}</div>
        <label className="dialog-label" htmlFor="library-copy-folder">{t("library.copyTo")}</label>
        <div className="dialog-field-stack">
          <Select value={targetDirectory || undefined} onValueChange={onTargetDirectoryChange} disabled={isSending || folders.length === 0}>
            <SelectTrigger id="library-copy-folder"><SelectValue placeholder={folders.length ? t("library.chooseFolder") : t("library.addFolderFirst")} /></SelectTrigger>
            <SelectContent>{folders.map((folder) => <SelectItem key={folder.path} value={folder.path}>{compactFolderPath(folder.path, localLibraryTailPathCount)}{folder.enabled ? "" : t("library.disabledSuffix")}</SelectItem>)}</SelectContent>
          </Select>
          {folders.length === 0 && <span className="muted">{t("library.addFolderHint")}</span>}
        </div>
        <label className="dialog-label" htmlFor="library-copy-name">{t("library.newFileName")}</label>
        <div className="dialog-field-stack">
          <Input id="library-copy-name" className="input" value={targetFileName} onChange={(event) => onTargetFileNameChange(event.target.value)} disabled={isSending} />
          <span className="muted">{t("library.extensionHint")}</span>
        </div>
        <span className="dialog-label">{t("library.existingFile")}</span>
        <label className="setting-row"><Checkbox checked={overwrite} onCheckedChange={(checked) => onOverwriteChange(checked === true)} disabled={isSending} /> {t("library.allowOverwrite")}</label>
        <span className="dialog-label">{t("library.targetPath")}</span>
        <div className="dialog-path-preview">{targetPathPreview}</div>
      </div>
      {error ? <div className="dialog-message dialog-message-error" role="alert">{localizeMessage(error)}</div> : null}
    </Dialog>
  );
}
