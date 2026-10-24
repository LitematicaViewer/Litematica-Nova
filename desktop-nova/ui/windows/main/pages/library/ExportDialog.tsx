import { useState } from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Dialog } from "../../../../components/Dialog";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { displayFilePath, saveDialog } from "../../../../../src/business/facade";

/** Export selection produced by the dialog and executed by the container (E.6). */
export interface ExportOptions {
  includePreview: boolean;
  includeMaterialList: boolean;
  includeManifest: boolean;
  outputPath: string;
}

function defaultExportName(): string {
  const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
  return `nova_export_${stamp}.zip`;
}

/**
 * Packaged-export dialog (E.6, 5.9.1.5 / 5.9.1.7 / 裁决 16).
 *
 * The projection body is always included and cannot be unchecked, so the "empty
 * set" submit is impossible; preview / material list / manifest are free. The user
 * picks the destination `.zip` path. The material list is exported as `txt` by
 * default (裁决 4).
 */
export function ExportDialog({
  entryCount,
  busy,
  error,
  onClose,
  onSubmit,
}: {
  entryCount: number;
  busy: boolean;
  error: string;
  onClose: () => void;
  onSubmit: (options: ExportOptions) => void;
}) {
  const [includePreview, setIncludePreview] = useState(true);
  const [includeMaterialList, setIncludeMaterialList] = useState(true);
  const [includeManifest, setIncludeManifest] = useState(true);
  const [outputPath, setOutputPath] = useState("");

  const handlePickPath = async () => {
    const picked = await saveDialog({
      defaultPath: defaultExportName(),
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    }).catch(() => null);
    if (picked) setOutputPath(picked);
  };

  const canSubmit = !busy && !!outputPath.trim() && entryCount > 0;

  return (
    <Dialog
      title="打包导出"
      subtitle={`将 ${entryCount} 个投影打包为扁平 zip；投影文件本体始终包含。`}
      width="md"
      busy={busy}
      onClose={onClose}
      footer={(
        <>
          <Button type="button" onClick={onClose} disabled={busy}>取消</Button>
          <Button type="button" variant="default" disabled={!canSubmit} aria-busy={busy} onClick={() => onSubmit({ includePreview, includeMaterialList, includeManifest, outputPath: outputPath.trim() })}>
            {busy ? "正在导出…" : "开始导出"}
          </Button>
        </>
      )}
    >
      <div className="library-export-dialog">
        <fieldset className="library-export-content">
          <legend>导出内容</legend>
          <label className="setting-row library-export-option">
            <Checkbox checked disabled aria-label="投影文件本体（必选）" /> 投影文件本体（必选）
          </label>
          <label className="setting-row library-export-option">
            <Checkbox checked={includePreview} disabled={busy} aria-label="预览图" onCheckedChange={(value) => setIncludePreview(value === true)} /> 预览图
          </label>
          <label className="setting-row library-export-option">
            <Checkbox checked={includeMaterialList} disabled={busy} aria-label="材料清单" onCheckedChange={(value) => setIncludeMaterialList(value === true)} /> 材料清单（默认 txt）
          </label>
          <label className="setting-row library-export-option">
            <Checkbox checked={includeManifest} disabled={busy} aria-label="manifest.json" onCheckedChange={(value) => setIncludeManifest(value === true)} /> manifest.json
          </label>
        </fieldset>
        <div className="library-export-target">
          <span className="dialog-label">导出目标</span>
          <div className="library-export-target-row">
            <Button type="button" disabled={busy} onClick={() => void handlePickPath()}>
              <ThemeIcon name="explorer" size={16} fallback={FolderOpen} />选择目标…
            </Button>
            <span className="library-export-path" title={outputPath ? displayFilePath(outputPath) : ""}>{outputPath ? displayFilePath(outputPath) : "未选择导出目标"}</span>
          </div>
        </div>
        <p className="muted library-export-note">zip 采用扁平结构，按条目名组织，不保留原文件夹层级。</p>
      </div>
      {error && <div className="dialog-message dialog-message-error" role="alert">{error}</div>}
    </Dialog>
  );
}