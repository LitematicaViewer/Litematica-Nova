import React, { useEffect, useRef, useState } from "react";
import { Dialog } from "../../components/Dialog";
import { Button } from "../../components/ui/button";
import { querySchematicFormat, type FormatResult } from "../../../src/platform/schematicFormats";
import { useI18n } from "../../i18n";

/** One shared content-driven import path for open/library/compose consumers. */
export function FormatImportDialog({ sourcePath = "", clipboardMode = false, onReady, onCancel, onVisibilityChange }: {
  sourcePath?: string; clipboardMode?: boolean; onReady: (path: string) => void; onCancel: () => void;
  onVisibilityChange?: (visible: boolean) => void;
}) {
  const { locale } = useI18n();
  const en = locale === "en-US";
  const [visible, setVisible] = useState(false);
  const [result, setResult] = useState<FormatResult>();
  const [versions, setVersions] = useState<NonNullable<FormatResult["sourceVersions"]>>([]);
  const [version, setVersion] = useState("");
  const [clipboardText, setClipboardText] = useState("");
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const generation = useRef(0);
  const ready = useRef(onReady); ready.current = onReady;
  useEffect(() => { onVisibilityChange?.(visible); return () => onVisibilityChange?.(false); }, [visible, onVisibilityChange]);
  useEffect(() => {
    const current = ++generation.current;
    setVisible(false); setResult(undefined); setError(""); setVersion("");
    if (clipboardMode) { setVisible(true); return; }
    if (!sourcePath) return;
    void querySchematicFormat({ action: "runtimeInput", input: sourcePath }).then(view => {
      if (current !== generation.current) return;
      if (view.nativePassthrough) return;
      setResult(view); setVisible(true);
    }).catch(reason => {
      if (current === generation.current) { setError(String(reason)); setVisible(true); }
    });
    return () => { ++generation.current; };
  }, [sourcePath, clipboardMode]);
  useEffect(() => {
    if (!visible) return;
    let active = true;
    void querySchematicFormat({ action: "registry" }).then(view => { if (active) setVersions(view.sourceVersions ?? []); }).catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [visible]);
  if (!visible) return null;
  const load = async () => {
    const current = generation.current;
    setPending(true); setError("");
    try {
      const view = result?.snapshotPath ? result : await querySchematicFormat({ action: "decodeToSnapshot", ...(clipboardMode ? { clipboardText } : { input: sourcePath }),
        ...(version ? { sourceDataVersion: Number(version) } : {}) });
      if (generation.current !== current) return;
      if (!view.snapshotPath) throw new Error(en ? "Import did not return a scene." : "导入未返回场景快照。");
      setVisible(false); ready.current(view.snapshotPath);
    } catch (reason) { if (current === generation.current) setError(String(reason)); }
    finally { if (current === generation.current) setPending(false); }
  };
  return <Dialog title={en ? "Import schematic" : "导入投影"} busy={pending} onClose={onCancel} footer={<>
    <Button variant="ghost" onClick={onCancel} disabled={pending}>{en ? "Cancel" : "取消"}</Button>
    <Button onClick={() => void load()} disabled={pending}>{pending ? (en ? "Preparing…" : "正在准备…") : (en ? "Open in workbench" : "在工作台中打开")}</Button>
  </>}>
    <p>{en ? "The source file stays unchanged. All workbench tools use the same normalized scene." : "源文件保持不变，工作台全部功能使用同一份规范化场景。"}</p>
    {clipboardMode && <label>{en ? "Template / clipboard text" : "模板 / 剪贴板文本"}<textarea className="input format-import-text" value={clipboardText} onChange={event => setClipboardText(event.target.value)} spellCheck={false} /></label>}
    {result?.report && <dl className="format-import-report"><dt>{en ? "Format" : "格式"}</dt><dd>{result.report.variant}</dd><dt>{en ? "Size" : "尺寸"}</dt><dd>{result.report.size.join(" × ")}</dd><dt>{en ? "Blocks / entities / block entities" : "方块 / 实体 / 方块实体"}</dt><dd>{result.report.blocks} / {result.report.entities} / {result.report.blockEntities}</dd></dl>}
    {!result?.snapshotPath && <label>{en ? "Source Minecraft version (never guessed)" : "源 Minecraft 版本（不会自动猜测）"}<select className="input" value={version} onChange={event => setVersion(event.target.value)}>
      <option value="">{en ? "Use declared version / select missing version" : "使用文件声明 / 为缺失版本选择"}</option>
      {versions.map(profile => <option key={profile.version} value={profile.dataVersion ?? ""} disabled={profile.dataVersion == null}>{profile.version}</option>)}
    </select></label>}
    {error && <p role="alert">{error}</p>}
  </Dialog>;
}
