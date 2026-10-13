import React, { useState } from "react";
import { Box, EyeOff, Play, X, Image } from "lucide-react";
import { loadLibrary, setRecordPreview, renderPreviewImage } from "../../../../../src/business/facade";
import { emitEvent } from "../../../../../src/platform/events";
import { projectionLibraryStateChangedEvent } from "../../../libraryEvents";
import { Button } from "../../../../components/ui/button";
import { useQuickPreview } from "../../useQuickPreview";
import "./render.css";

/** Beta exposes the historical fast preview only. The public binary contains only the existing fast renderer. */
export function RenderPage({ currentFile }: { currentFile: string }) {
  const preview = useQuickPreview(currentFile || "", false);
  const [capturing, setCapturing] = useState(false);
  const [captureMessage, setCaptureMessage] = useState("");
  const capture = async () => {
    setCapturing(true); setCaptureMessage("");
    try {
      const image = await renderPreviewImage(currentFile, "fast_experimental");
      await setRecordPreview(await loadLibrary(), currentFile, image.preview_path);
      await emitEvent(projectionLibraryStateChangedEvent, {}).catch(() => undefined);
      setCaptureMessage("已保存当前真实渲染帧，投影库与Explorer共用此缩略图。");
    } catch (error) { setCaptureMessage(String(error)); }
    finally { setCapturing(false); }
  };
  const operation = preview.operation;
  const ready = operation?.state === "ready";
  const loading = operation?.state === "starting" && !operation.error;
  const error = preview.error || operation?.error;
  const status = !currentFile ? "请先打开一个投影。"
    : loading ? "正在准备快速预览…"
      : ready ? "拖动旋转 · 滚轮缩放"
        : operation?.state === "failed" ? "快速预览加载失败，请重新加载。"
          : operation?.state === "closed" ? "快速预览已关闭，可重新加载。" : "快速预览使用独立的简化渲染，不加载光影包。";
  const name = currentFile?.split(/[\\/]/).pop() || "尚未选择投影";
  return <section className="render-page">
    <header className="render-page-header">
      <h2 className="render-page-title">快速渲染</h2>
      <div className="render-page-source" title={currentFile}>{name}</div>
    </header>
    <div className="render-page-actions">
      <Button variant="default" disabled={!currentFile || preview.pending || loading} onClick={() => void preview.start()}>
        <Play size={16} aria-hidden="true" />{preview.pending ? "正在连接…" : loading ? "正在加载…" : ready ? "页内显示" : operation ? "重新加载" : "快速渲染"}
      </Button>
      <Button variant="ghost" disabled={!operation?.presentation.visible || preview.pending} onClick={() => void preview.hide()}><EyeOff size={16} aria-hidden="true" />隐藏预览</Button>
      <Button variant="ghost" disabled={!operation || !["starting", "ready"].includes(operation.state) || preview.pending} onClick={() => void preview.close()}><X size={16} aria-hidden="true" />{loading ? "取消加载" : "关闭预览"}</Button>
      <div className="render-full-placeholder"><Button disabled>完整模式</Button><span>完整模式未包含在公开版</span></div>
      <Button variant="ghost" disabled={!ready || capturing} onClick={() => void capture()}><Image size={16} aria-hidden="true" />{capturing ? "正在读取真实帧…" : "设为缩略图"}</Button>
    </div>
    {error && <pre className="render-error" role="alert">{error}</pre>}
    {captureMessage && <div role="status">{captureMessage}</div>}
    <div className="render-preview-group">
      <div ref={preview.hostRef} className="render-preview-host" aria-label="快速 Viewer 页内视口">
        <div className="render-preview-placeholder"><Box size={40} aria-hidden="true" /><strong>{name}</strong><span>{status}</span></div>
      </div>
      <div className="render-preview-help" role="status" aria-live="polite">{status}</div>
    </div>
  </section>;
}
