import React, { useEffect, useRef, useState } from "react";
import { Box, EyeOff, Play, X, Image } from "lucide-react";
import { loadLibrary, setRecordPreview, renderPreviewImage, loadStructureStats, type StatsData } from "../../../../../src/business/facade";
import { emitEvent } from "../../../../../src/platform/events";
import { projectionLibraryStateChangedEvent } from "../../../libraryEvents";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { useQuickPreview } from "../../useQuickPreview";
import { useFullPreview } from "../../useFullPreview";
import { QuickResourceDialog } from "../../QuickResourceDialog";
import { formatResourceBytes } from "../../../../../src/business/resourcePreflightUnits";
import { MaterialsDialog, openMaterialsWithWindowBehavior } from "../statistics/StatisticsPage";
import "./render.css";

/** Separate historical Fast and real private Full service paths; no algorithm substitution. */
export function RenderPage({ currentFile, presentationBlocked = false }: { currentFile: string; presentationBlocked?: boolean }) {
  const [showMaterials, setShowMaterials] = useState(false);
  const [materialsData, setMaterialsData] = useState<StatsData | null>(null);
  const [materialsBusy, setMaterialsBusy] = useState(false);
  const [materialsError, setMaterialsError] = useState("");
  const materialsGeneration = useRef(0);
  useEffect(() => {
    materialsGeneration.current++; setShowMaterials(false); setMaterialsData(null); setMaterialsBusy(false); setMaterialsError("");
    return () => { materialsGeneration.current++; };
  }, [currentFile]);
  const openMaterials = async () => {
    setMaterialsError("");
    try {
      await openMaterialsWithWindowBehavior(currentFile, () => {
        const generation = ++materialsGeneration.current;
        setMaterialsBusy(true);
        void loadStructureStats(currentFile).then((data) => {
          if (materialsGeneration.current !== generation) return;
          setMaterialsData(data); setShowMaterials(true);
        }).catch((error) => {
          if (materialsGeneration.current === generation) setMaterialsError(String(error));
        }).finally(() => {
          if (materialsGeneration.current === generation) setMaterialsBusy(false);
        });
      });
    } catch (error) { setMaterialsError(String(error)); }
  };
  const [mode, setMode] = useState<"fast" | "standard" | "full">("fast");
  const [preloads, setPreloads] = useState({ full: false, standard: false, fast: false });
  const exactMode = mode === "standard" ? "standard" : "full";
  const preload = preloads[mode];
  const preview = useQuickPreview(currentFile || "", mode !== "fast" || showMaterials || presentationBlocked, preload);
  const full = useFullPreview(currentFile || "", preload, mode === "fast" || showMaterials || presentationBlocked, exactMode);
  const [capturing, setCapturing] = useState(false);
  const [captureMessage, setCaptureMessage] = useState("");
  const capture = async () => {
    setCapturing(true); setCaptureMessage("");
    try {
      const image = await renderPreviewImage(currentFile, "fast_experimental");
      await setRecordPreview(await loadLibrary(), currentFile, image.preview_path);
      await emitEvent(projectionLibraryStateChangedEvent, {}).catch(() => undefined);
      setCaptureMessage("缩略图已保存。");
    } catch (error) { setCaptureMessage(String(error)); }
    finally { setCapturing(false); }
  };
  const fullProgress = full.progress;
  const fullProgressBounded = fullProgress?.completed != null && fullProgress?.total != null
    && Number.isFinite(fullProgress.completed) && Number.isFinite(fullProgress.total) && fullProgress.total > 0;
  const operation = preview.operation;
  const ready = operation?.state === "ready";
  const loading = operation?.state === "starting" && !operation.error;
  const error = preview.error || operation?.error;
  const status = !currentFile ? "请先打开一个投影。"
    : loading ? "正在准备快速预览…"
      : ready ? "拖动旋转 · 滚轮缩放"
        : operation?.state === "failed" ? "快速预览加载失败，请重新加载。"
          : operation?.state === "closed" ? "快速预览已关闭，可重新加载。" : "尚未加载预览";
  const name = currentFile?.split(/[\\/]/).pop() || "尚未选择投影";
  return <section className="render-page">
    <header className="render-page-header">
      <h2 className="render-page-title">{mode === "full" ? "完整渲染" : mode === "standard" ? "标准渲染" : "快速渲染"}</h2>
      <div className="render-page-source" title={currentFile}>{name}</div>
    </header>
    <div className="render-page-actions" aria-label="渲染模式">
      <Button variant="secondary" disabled={!currentFile || materialsBusy} onClick={() => void openMaterials()}>{materialsBusy ? "正在读取材料…" : "材料列表"}</Button>
      <Button variant={mode === "fast" ? "default" : "ghost"} aria-pressed={mode === "fast"} onClick={() => setMode("fast")}>快速模式</Button>
      <Button variant={mode === "standard" ? "default" : "ghost"} aria-pressed={mode === "standard"} onClick={() => setMode("standard")}>标准模式</Button>
      <Button variant={mode === "full" ? "default" : "ghost"} aria-pressed={mode === "full"} onClick={() => setMode("full")}>完整模式</Button>
    </div>
    {materialsError && <pre className="render-error" role="alert">{materialsError}</pre>}
    {mode !== "fast" ? <>
      <div className="render-page-actions">
        <label><Checkbox checked={preload} disabled={full.pending || full.loading} onCheckedChange={checked => setPreloads(previous => ({ ...previous, [exactMode]: checked === true }))} />预加载</label>
        <Button disabled={!currentFile || full.pending || full.loading} onClick={() => void full.start()}><Play size={16} aria-hidden="true" />{mode === "standard" ? "标准" : "完整"}渲染</Button>
        <Button variant="ghost" disabled={!full.operation || full.pending} onClick={() => void full.hide()}><EyeOff size={16} aria-hidden="true" />隐藏</Button>
        <Button variant="ghost" disabled={!full.operation && !full.pending && !full.preflight} onClick={() => void full.close()}><X size={16} aria-hidden="true" />关闭 / 取消</Button>
      </div>
      {full.preflight && <section className="render-preview-help" role="status">
        <p>显存需求：{full.preflight.conservativeVramBytes == null ? "未知" : formatResourceBytes(full.preflight.conservativeVramBytes)}</p>
        {full.preflight.severity === "warning" && <p>需求达到当前可用预算的 80%，确认后才继续预加载。</p>}
        {full.preflight.severity === "hard" && <p>需求达到当前可用预算的 95%，预加载不可用，可选择实时加载。</p>}
        {full.preflight.severity === "unmeasured" && <p>需求或当前预算尚未确认，预加载不可用，可选择实时加载。</p>}
        <div className="render-page-actions">
          <Button disabled={full.pending || !full.preflight.allowedActions.includes("continue_prebuilt")} onClick={() => void full.choose("prebuilt")}>确认预构建</Button>
          <Button disabled={full.pending || (full.preflight.severity !== "normal" && !full.preflight.allowedActions.includes("switch_realtime"))} onClick={() => void full.choose("realtime")}>直接打开</Button>
          <Button variant="ghost" disabled={full.pending || !full.preflight.allowedActions.includes("use_basic_vulkan")} onClick={() => void full.choose("basic_vulkan")}>降低画质</Button>
          <Button variant="ghost" onClick={() => void full.close()}>取消</Button>
        </div>
      </section>}
      {full.error && <pre className="render-error" role="alert">{full.error}</pre>}
      {fullProgress && !full.visible && !full.error && <div className="render-preview-help" role="status" aria-live="polite">
        {full.progressLabel}
        {fullProgressBounded && <progress aria-label="渲染进度" max={fullProgress.total!} value={fullProgress.completed!} />}
      </div>}
      <div className="render-preview-group"><div ref={full.hostRef} className="render-preview-host" aria-label={`${mode === "standard" ? "标准" : "完整"}预览视口`}>
        {!full.visible && <div className="render-preview-placeholder"><Box size={40} aria-hidden="true" /><strong>{name}</strong><span>{full.error ? "预览加载失败，请重新加载。" : full.progressLabel}</span></div>}
      </div></div>
    </> : <>
    <div className="render-page-actions">
      <label><Checkbox checked={preload} disabled={preview.pending || loading} onCheckedChange={(checked) => setPreloads((current) => ({ ...current, fast: checked === true }))} />预加载</label>
      <Button variant="default" disabled={!currentFile || preview.pending || loading} onClick={() => void preview.start()}>
        <Play size={16} aria-hidden="true" />{preview.pending ? "正在连接…" : loading ? "正在加载…" : ready ? "页内显示" : operation ? "重新加载" : "快速渲染"}
      </Button>
      <Button variant="ghost" disabled={!operation?.presentation.visible || preview.pending} onClick={() => void preview.hide()}><EyeOff size={16} aria-hidden="true" />隐藏预览</Button>
      <Button variant="ghost" disabled={!operation || !["starting", "ready"].includes(operation.state) || preview.pending} onClick={() => void preview.close()}><X size={16} aria-hidden="true" />{loading ? "取消加载" : "关闭预览"}</Button>
      <Button variant="ghost" disabled={!ready || capturing} onClick={() => void capture()}><Image size={16} aria-hidden="true" />{capturing ? "正在读取真实帧…" : "设为缩略图"}</Button>
    </div>
    {error && <pre className="render-error" role="alert">{error}</pre>}
    {captureMessage && <div role="status">{captureMessage}</div>}
    <div className="render-preview-group">
      <div ref={preview.hostRef} className="render-preview-host" aria-label="快速 Viewer 页内视口">
        {!ready && <div className="render-preview-placeholder"><Box size={40} aria-hidden="true" /><strong>{name}</strong><span>{status}</span></div>}
      </div>
      <div className="render-preview-help" role="status" aria-live="polite">{status}</div>
    </div>
    </>}
    {mode === "fast" && operation?.resource?.decisionRequired && <QuickResourceDialog report={operation.resource} busy={preview.pending} onChoose={async (action) => {
      if (action !== "cancel") setPreloads((current) => ({ ...current, fast: action === "preload" }));
      await preview.chooseResources(action);
    }} />}
    {showMaterials && materialsData && <MaterialsDialog data={materialsData} currentFile={currentFile} onClose={() => setShowMaterials(false)} />}
  </section>;
}
