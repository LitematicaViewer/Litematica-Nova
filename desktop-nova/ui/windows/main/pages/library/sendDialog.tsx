import React from "react";

import { Dialog } from "../../../../components/Dialog";
import { LocalLibraryFolder, ProjectionRecord } from "../../../../../src/business/facade";

export function ensureLitematicFileName(fileName: string): string {
  const trimmed = fileName.trim();
  if (!trimmed) return "";
  return /\.litematic$/i.test(trimmed) ? trimmed : `${trimmed}.litematic`;
}

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
  const normalizedFileName = ensureLitematicFileName(targetFileName);
  const targetPathPreview = targetDirectory && normalizedFileName
    ? `${targetDirectory.replace(/[\\/]+$/, "")}/${normalizedFileName}`
    : "-";

  return (
    <Dialog
      title="发送投影到本地库文件夹"
      subtitle="这是标准对话框，不会以独立子窗口形式打开。"
      width="md"
      onClose={onClose}
      footer={(
        <>
          <button className="btn" type="button" onClick={onClose} disabled={isSending}>取消</button>
          <button className="btn" type="button" onClick={onSubmit} disabled={isSending || !targetDirectory || !normalizedFileName || folders.length === 0}>
            {isSending ? "发送中..." : "发送"}
          </button>
        </>
      )}
    >
      <div className="dialog-form-grid">
        <span className="dialog-label">源文件</span>
        <div className="dialog-path-preview">{record.path}</div>
        <span className="dialog-label">目标文件夹</span>
        <div className="dialog-field-stack">
          <select className="input" value={targetDirectory} onChange={(event) => onTargetDirectoryChange(event.target.value)} disabled={isSending || folders.length === 0}>
            {folders.length === 0 ? (
              <option value="">请先配置本地库文件夹</option>
            ) : folders.map((folder) => (
              <option key={folder.path} value={folder.path}>{folder.path}{folder.enabled ? "" : "（未启用）"}</option>
            ))}
          </select>
          <span className="muted">目标是已配置的本地库文件夹根目录；发送会复制当前 `.litematic` 文件。</span>
        </div>
        <span className="dialog-label">目标文件名</span>
        <div className="dialog-field-stack">
          <input className="input" value={targetFileName} onChange={(event) => onTargetFileNameChange(event.target.value)} disabled={isSending} />
          <span className="muted">未填写 `.litematic` 后缀时会自动补上。</span>
        </div>
        <span className="dialog-label">覆盖策略</span>
        <label className="setting-row"><input type="checkbox" checked={overwrite} onChange={(event) => onOverwriteChange(event.target.checked)} disabled={isSending} /> 允许覆盖同名文件</label>
        <span className="dialog-label">目标路径</span>
        <div className="dialog-path-preview">{targetPathPreview}</div>
      </div>
      {error ? <div className="dialog-message dialog-message-error">{error}</div> : null}
    </Dialog>
  );
}