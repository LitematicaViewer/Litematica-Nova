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
  error: string;
  onTargetDirectoryChange: (value: string) => void;
  onTargetFileNameChange: (value: string) => void;
  onOverwriteChange: (value: boolean) => void;
  onClose: () => void;
  onSubmit: () => void;
}) {
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
      title="复制投影到文件夹"
      subtitle="原文件保持不变。复制完成后，新文件会显示在投影库中。"
      width="md"
      busy={isSending}
      onClose={onClose}
      footer={(
        <>
          <Button className="btn" type="button" onClick={onClose} disabled={isSending}>取消</Button>
          <Button className="btn" type="button" onClick={onSubmit} aria-busy={isSending} disabled={isSending || !targetDirectory || !normalizedFileName || folders.length === 0}>
            {isSending ? "正在复制…" : "复制"}
          </Button>
        </>
      )}
    >
      <div className="dialog-form-grid">
        <span className="dialog-label">源文件</span>
        <div className="dialog-path-preview">{displayFilePath(record.path)}</div>
        <label className="dialog-label" htmlFor="library-copy-folder">复制到</label>
        <div className="dialog-field-stack">
          <Select value={targetDirectory || undefined} onValueChange={onTargetDirectoryChange} disabled={isSending || folders.length === 0}>
            <SelectTrigger id="library-copy-folder"><SelectValue placeholder={folders.length ? "选择文件夹" : "请先添加文件夹"} /></SelectTrigger>
            <SelectContent>{folders.map((folder) => <SelectItem key={folder.path} value={folder.path}>{compactFolderPath(folder.path, localLibraryTailPathCount)}{folder.enabled ? "" : "（未启用）"}</SelectItem>)}</SelectContent>
          </Select>
          {folders.length === 0 && <span className="muted">关闭此窗口，点击投影库上方的「管理文件夹」即可添加。</span>}
        </div>
        <label className="dialog-label" htmlFor="library-copy-name">新文件名</label>
        <div className="dialog-field-stack">
          <Input id="library-copy-name" className="input" value={targetFileName} onChange={(event) => onTargetFileNameChange(event.target.value)} disabled={isSending} />
          <span className="muted">无需输入 .litematic 后缀，软件会自动补全。</span>
        </div>
        <span className="dialog-label">同名文件</span>
        <label className="setting-row"><Checkbox checked={overwrite} onCheckedChange={(checked) => onOverwriteChange(checked === true)} disabled={isSending} /> 允许覆盖同名文件</label>
        <span className="dialog-label">目标路径</span>
        <div className="dialog-path-preview">{targetPathPreview}</div>
      </div>
      {error ? <div className="dialog-message dialog-message-error" role="alert">{error}</div> : null}
    </Dialog>
  );
}
