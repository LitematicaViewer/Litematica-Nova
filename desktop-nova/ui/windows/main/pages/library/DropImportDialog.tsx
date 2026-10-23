import { useEffect, useState } from "react";
import { Button } from "../../../../components/ui/button";
import { Dropdown } from "../../../../components/Dropdown";
import { Dialog } from "../../../../components/Dialog";
import {
  displayFilePath,
  fileNameFromPath,
  LocalLibraryFolder,
} from "../../../../../src/business/facade";
import { compactFolderPath } from "./function";

/** One OS-dropped item, already resolved to a local path. */
export interface DroppedItem {
  path: string;
  isDirectory: boolean;
}

/** The user's chosen plan for a drop, executed by the library container. */
export interface DropImportPlan {
  /** Files routed into the always-tracked built-in import folder (5.9.1.1). */
  importToBuiltin: string[];
  /** Files copied into an explicit mounted tracked folder (5.9.1.1). */
  copyToFolder: { sourcePath: string; folderPath: string }[];
  /** Folders mounted as tracked folders (5.9.1.2). */
  mountFolders: string[];
  /** Folders whose `.litematic` contents are copied into the built-in folder (5.9.1.2). */
  importFolderContents: string[];
}

function trackedTargets(folders: LocalLibraryFolder[]): LocalLibraryFolder[] {
  return folders.filter((folder) => folder.type === "tracked" || folder.type === "builtin_import");
}

function isLitematicPath(path: string): boolean {
  return /\.litematic$/i.test(path);
}

/**
 * OS drag-import dialog (E.3, 5.9.1.1 / 5.9.1.2 / 5.9.3.1).
 *
 * Dropped files offer {导入内置文件夹, 复制到已挂载文件夹}; dropped folders offer
 * {挂载为文件夹, 导入其中的文件}. Unsupported dropped content is listed and ignored.
 * Library-internal reorder never reaches this dialog (the container routes it by
 * the `application/x-nova-library-entries` drag type).
 */
export function DropImportDialog({
  items,
  folders,
  busy,
  error,
  onClose,
  onSubmit,
}: {
  items: DroppedItem[];
  folders: LocalLibraryFolder[];
  busy: boolean;
  error: string;
  onClose: () => void;
  onSubmit: (plan: DropImportPlan) => void;
}) {
  const [fileMode, setFileMode] = useState<"builtin" | "copy">("builtin");
  const [folderMode, setFolderMode] = useState<"mount" | "contents">("mount");
  const targets = trackedTargets(folders);
  const [copyTarget, setCopyTarget] = useState(targets[0]?.path || "");

  useEffect(() => { if (!copyTarget && targets[0]) setCopyTarget(targets[0].path); }, [copyTarget, targets]);

  const files = items.filter((item) => !item.isDirectory && isLitematicPath(item.path));
  const directories = items.filter((item) => item.isDirectory);
  const unsupported = items.filter((item) => !item.isDirectory && !isLitematicPath(item.path));

  const canSubmit = !busy
    && (files.length === 0 || fileMode === "builtin" || !!copyTarget)
    && (files.length + directories.length) > 0;

  const handleSubmit = () => {
    if (!canSubmit) return;
    const plan: DropImportPlan = { importToBuiltin: [], copyToFolder: [], mountFolders: [], importFolderContents: [] };
    if (files.length) {
      if (fileMode === "builtin") plan.importToBuiltin = files.map((item) => item.path);
      else plan.copyToFolder = files.map((item) => ({ sourcePath: item.path, folderPath: copyTarget }));
    }
    if (directories.length) {
      if (folderMode === "mount") plan.mountFolders = directories.map((item) => item.path);
      else plan.importFolderContents = directories.map((item) => item.path);
    }
    onSubmit(plan);
  };

  return (
    <Dialog
      title="导入拖入的内容"
      subtitle="选择如何处理拖入的文件或文件夹。"
      width="md"
      busy={busy}
      onClose={onClose}
      footer={(
        <>
          <Button type="button" onClick={onClose} disabled={busy}>取消</Button>
          <Button type="button" variant="default" disabled={!canSubmit} aria-busy={busy} onClick={handleSubmit}>{busy ? "正在导入…" : "导入"}</Button>
        </>
      )}
    >
      <div className="library-drop-dialog">
        {files.length > 0 && (
          <div className="library-drop-group" data-drop-kind="file">
            <div className="library-drop-heading">
              <span>拖入的文件（{files.length}）</span>
              <Dropdown
                ariaLabel="文件导入方式"
                value={fileMode}
                onChange={(value) => setFileMode(value as "builtin" | "copy")}
                options={[{ value: "builtin", label: "导入内置文件夹" }, { value: "copy", label: "复制到已挂载文件夹" }]}
              />
            </div>
            <ul className="library-drop-list">
              {files.slice(0, 8).map((item) => <li key={item.path} title={displayFilePath(item.path)}>{fileNameFromPath(item.path)}</li>)}
              {files.length > 8 && <li className="muted">以及另外 {files.length - 8} 个文件</li>}
            </ul>
            {fileMode === "copy" && (
              targets.length
                ? <Dropdown ariaLabel="复制目标文件夹" value={copyTarget} onChange={setCopyTarget} options={targets.map((folder) => ({ value: folder.path, label: compactFolderPath(folder.path, 3) }))} />
                : <p className="muted" role="alert">没有可作为目标的已追踪文件夹。</p>
            )}
          </div>
        )}

        {directories.length > 0 && (
          <div className="library-drop-group" data-drop-kind="folder">
            <div className="library-drop-heading">
              <span>拖入的文件夹（{directories.length}）</span>
              <Dropdown
                ariaLabel="文件夹处理方式"
                value={folderMode}
                onChange={(value) => setFolderMode(value as "mount" | "contents")}
                options={[{ value: "mount", label: "挂载为文件夹" }, { value: "contents", label: "导入其中的文件" }]}
              />
            </div>
            <ul className="library-drop-list">
              {directories.slice(0, 8).map((item) => <li key={item.path} title={displayFilePath(item.path)}>{compactFolderPath(item.path, 3)}</li>)}
              {directories.length > 8 && <li className="muted">以及另外 {directories.length - 8} 个文件夹</li>}
            </ul>
          </div>
        )}

        {unsupported.length > 0 && (
          <div className="library-drop-group library-drop-unsupported" role="status">
            <span>不支持的类型（{unsupported.length}），将被忽略：</span>
            <ul className="library-drop-list">
              {unsupported.slice(0, 8).map((item) => <li key={item.path}>{fileNameFromPath(item.path)}</li>)}
            </ul>
          </div>
        )}

        {files.length === 0 && directories.length === 0 && (
          <p className="muted">没有可导入的投影文件或文件夹。</p>
        )}
      </div>
      {error && <div className="dialog-message dialog-message-error" role="alert">{error}</div>}
    </Dialog>
  );
}