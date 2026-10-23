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
import { compactFolderPath, ensureLitematicFileName } from "./function";

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

  useEffect(() => { setRows(buildRows(entry, folders)); }, [entry, folders]);

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
      title="分发投影"
      subtitle={`${entry.displayName || entry.fileName} · 勾选空槽位新增副本，取消勾选既有副本移入回收区，行内改名即批量重命名。`}
      width="lg"
      busy={busy}
      onClose={onClose}
      footer={(
        <>
          <span className="library-dispatch-summary" role="status">新增 {additions.length} · 重命名 {renames.length} · 入回收区 {trash.length}</span>
          <Button type="button" onClick={onClose} disabled={busy}>取消</Button>
          <Button type="button" variant="default" onClick={handleSubmit} aria-busy={busy} disabled={!canSubmit}>{busy ? "正在分发…" : "确认分发"}</Button>
        </>
      )}
    >
      <div className="dialog-path-preview" title={displayFilePath(entry.primary.path)}>源文件：{displayFilePath(entry.primary.path)}</div>
      <Table className="library-dispatch-table" data-entry-id={entry.entryId}>
        <TableHeader>
          <TableRow>
            <TableHead>文件夹路径</TableHead>
            <TableHead>文件名</TableHead>
            <TableHead className="library-dispatch-seq-col">重数序号</TableHead>
            <TableHead className="library-dispatch-select-col">选择</TableHead>
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
                    ariaLabel={`${compactFolderPath(row.folderPath, localLibraryTailPathCount)} 第 ${row.sequence} 份文件名`}
                    onChange={(value) => update(row.key, { name: value })}
                  />
                  {nameError && <small className="library-dispatch-hint" role="alert">请填写文件名</small>}
                </TableCell>
                <TableCell className="library-dispatch-seq-col">{row.sequence}</TableCell>
                <TableCell className="library-dispatch-select-col">
                  <Checkbox
                    checked={row.included}
                    disabled={busy}
                    aria-label={`${row.replicaId ? `第 ${row.sequence} 份副本` : "新增副本"}`}
                    onCheckedChange={(value) => update(row.key, { included: value === true })}
                  />
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
      {dispatchTargets(folders).length === 0 && <p className="muted">没有可作为分发目标的已追踪文件夹。</p>}
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
  error: string;
  onTargetDirectoryChange: (value: string) => void;
  onTargetFileNameChange: (value: string) => void;
  onOverwriteChange: (value: boolean) => void;
  onClose: () => void;
  onSubmit: () => void;
}) {
  const [localLibraryTailPathCount, setLocalLibraryTailPathCount] = useState(3);

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
          <Button type="button" onClick={onClose} disabled={isSending}>取消</Button>
          <Button type="button" onClick={onSubmit} aria-busy={isSending} disabled={isSending || !targetDirectory || !normalizedFileName || folders.length === 0}>
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