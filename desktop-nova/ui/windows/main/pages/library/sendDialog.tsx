import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayFilePath } from "../../../../../src/business/facade";
import { useEffect, useState } from "react";

import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../../../components/ui/select";
import { Dialog } from "../../../../components/Dialog";
import {
  loadUserConfigMigratingLocalStorage,
  LocalLibraryFolder,
  normalizeLocalLibraryTailPathCount,
  ProjectionRecord,
} from "../../../../../src/business/facade";

// 同级函数
import { compactFolderPath, ensureLitematicFileName } from "./function";

export const DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT = 3;


export function SendProjectionDialog({
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
  record: ProjectionRecord;
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
