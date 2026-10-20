import { displayPathText } from "../../../../../src/business/facade";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { takeUpdateDraft, useUpdateDraft } from "../../updateRecovery";
import {
  analyzeProjectionFile,
  saveProjectionMetadataPatch,
  selectLitematicFile,
  selectLitematicSavePath,
  readProjectionPreviewImage,
  selectPreviewImageFile,
  importProjectionPreviewImage,
  clearProjectionPreviewImage,
  renderPreviewImage,
} from "../../../../../src/business/facade";
import { emitEvent, listenEvent } from "../../../../../src/platform/events";
import { projectionLibraryStateChangedEvent } from "../../../libraryEvents";
import {
  addOrUpdateRecordFromAnalysis,
  loadLibrary,
  projectionRecordAnalysis,
} from "../../../../../src/business/facade";
import {
  findRenderOperationForSource,
  getApplicationRenderState,
  subscribeApplicationRender,
} from "../../../../../src/business/facade";

import { Box, FolderOpen, Library, Play, RotateCcw, Save, ExternalLink, ImagePlus, ImageOff } from "lucide-react";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Textarea } from "../../../../components/ui/textarea";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from "../../../../components/ui/table";
import { useQuickPreview } from "../../useQuickPreview";
import { useFullPreview } from "../../useFullPreview";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Dialog } from "../../../../components/Dialog";
import "./properties.css";
import { displayFilePath, minecraftVersionLabel, windowsPathKey } from "../../../../../src/business/projectionLibrary";
import { VersionConversionDialog } from "./VersionConversionDialog";
import { formatResourceBytes } from "../../../../../src/business/resourcePreflightUnits";

type RegionEdit = {
  originalName: string;
  name: string;
  position: { x: number; y: number; z: number };
  size: { x: number; y: number; z: number };
};

type MetadataForm = {
  name: string;
  author: string;
  description: string;
  time_created: string;
  time_modified: string;
  litematic_version: number;
  litematic_subversion: number;
  minecraft_data_version: number;
  regions: RegionEdit[];
};

function toLocalDateTime(value: unknown): string {
  const n = Number(value || 0);
  if (!Number.isFinite(n) || n <= 0) return "";
  const d = new Date(n);
  const pad = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function fromLocalDateTime(value: string): number | undefined {
  if (!value) return undefined;
  const ms = new Date(value).getTime();
  return Number.isFinite(ms) ? ms : undefined;
}

function metadataToForm(data: any): MetadataForm {
  const meta = data?.metadata || {};
  const regions = (data?.regions || []).map((region: any): RegionEdit => ({
    originalName: region.name || "Unnamed",
    name: region.name || "Unnamed",
    position: region.position || { x: 0, y: 0, z: 0 },
    size: region.size || { x: 0, y: 0, z: 0 },
  }));
  return {
    name: meta.name || "",
    author: meta.author || "",
    description: meta.description || "",
    time_created: toLocalDateTime(meta.time_created),
    time_modified: toLocalDateTime(meta.time_modified),
    litematic_version: Number(meta.litematic_version || 6),
    litematic_subversion: Number(meta.litematic_subversion || 1),
    minecraft_data_version: Number(meta.minecraft_data_version || 0),
    regions,
  };
}

function buildPatch(form: MetadataForm) {
  return {
    name: form.name,
    author: form.author,
    description: form.description,
    time_created: fromLocalDateTime(form.time_created),
    time_modified: fromLocalDateTime(form.time_modified),
    regions: form.regions
      .filter((region) => region.name !== region.originalName)
      .map((region) => ({ old_name: region.originalName, new_name: region.name })),
  };
}

function validateRegions(regions: RegionEdit[]): string {
  const names = new Set<string>();
  for (const region of regions) {
    const name = region.name.trim();
    if (!name) return "区域名不能为空。";
    if (names.has(name)) return `区域名重复：${name}`;
    names.add(name);
  }
  return "";
}

export function PropertiesPage({ currentFile, setCurrentFile, setRoute, presentationBlocked = false }: any) {
  const applicationRender = React.useSyncExternalStore(
    subscribeApplicationRender,
    getApplicationRenderState,
    getApplicationRenderState,
  );
  const [data, setData] = useState<any>(null);
  const [initialForm, setInitialForm] = useState<MetadataForm | null>(null);
  const [form, setForm] = useState<MetadataForm | null>(null);
  useUpdateDraft(`properties:${currentFile}`, { form, initialForm });
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [convertOpen, setConvertOpen] = useState(false);
  const [versionConvertOpen, setVersionConvertOpen] = useState(false);
  const [renderMode, setRenderMode] = useState<"fast" | "standard" | "full">("fast");
  const [preload, setPreload] = useState(false);
  const [filePreview, setFilePreview] = useState("");
  const [filePreviewBusy, setFilePreviewBusy] = useState(false);
  const [capturingPreview, setCapturingPreview] = useState(false);
  const filePreviewWrite = useRef(false);
  const filePreviewRevision = useRef(0);
  const [clearPreviewConfirm, setClearPreviewConfirm] = useState(false);
  const analysisGeneration = useRef(0);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const sourceRef = useRef(currentFile);
  sourceRef.current = currentFile;

  const renderOperation = useMemo(
    () => findRenderOperationForSource(currentFile, applicationRender),
    [applicationRender.operations, currentFile],
  );
  const applicationAnalysis = renderOperation?.analysis ?? null;
  const preview = useQuickPreview(currentFile || "", presentationBlocked || convertOpen || versionConvertOpen || clearPreviewConfirm || filePreviewBusy || renderMode !== "fast", preload);
  const fullPreview = useFullPreview(currentFile || "", preload, presentationBlocked || convertOpen || versionConvertOpen || clearPreviewConfirm || filePreviewBusy || renderMode === "fast", renderMode === "standard" ? "standard" : "full");
  const quickOperation = preview.operation;
  const renderPending = renderMode === "fast" ? quickOperation?.state === "starting" && !quickOperation.error : fullPreview.loading;
  const viewerPrepared = renderMode === "fast" ? quickOperation?.state === "ready" : fullPreview.ready;
  const previewVisible = renderMode === "fast" ? quickOperation?.presentation.visible === true : fullPreview.visible;
  const previewPending = renderMode === "fast" ? preview.pending : fullPreview.pending;
  const handlePreview = renderMode === "fast" ? preview.start : fullPreview.start;
  const handleCancelPreview = renderMode === "fast" ? preview.close : fullPreview.close;
  useEffect(() => {
    const revision = ++filePreviewRevision.current;
    setFilePreview(""); setClearPreviewConfirm(false);
    if (currentFile) void readProjectionPreviewImage(currentFile).then((image) => {
      if (filePreviewRevision.current === revision) setFilePreview(image?.data_url || "");
    }).catch((reason) => { if (filePreviewRevision.current === revision) setError(`读取预览图失败：${String(reason)}`); });
    return () => { ++filePreviewRevision.current; };
  }, [currentFile]);
  useEffect(() => {
    const listening = listenEvent<{ reason?: string; path?: string }>(projectionLibraryStateChangedEvent, event => {
      if (event.payload?.reason !== "embedded-preview" || !event.payload.path
        || windowsPathKey(event.payload.path) !== windowsPathKey(currentFile || "")) return;
      const revision = ++filePreviewRevision.current;
      void readProjectionPreviewImage(currentFile).then(image => {
        if (filePreviewRevision.current === revision) setFilePreview(image?.data_url || "");
      }).catch(reason => { if (filePreviewRevision.current === revision) setError(`读取预览图失败：${String(reason)}`); });
    }).catch(() => undefined);
    return () => { void listening.then(off => off?.()); };
  }, [currentFile]);
  const captureFilePreview = async () => {
    if (!currentFile || filePreviewWrite.current || savingRef.current || !viewerPrepared) return;
    const source = currentFile;
    filePreviewWrite.current = true; setCapturingPreview(true); setError("");
    try {
      await renderPreviewImage(source, renderMode === "fast" ? "fast_experimental" : renderMode, undefined,
        renderMode === "fast" ? quickOperation?.previewId : fullPreview.operation?.operationId);
      const image = await readProjectionPreviewImage(source);
      if (!image?.data_url) throw new Error("预览图写回后读取失败，请重试。");
      if (sourceRef.current === source) { setFilePreview(image.data_url); setStatus("缩略图已写入投影文件。"); }
      await emitEvent(projectionLibraryStateChangedEvent, { reason: "embedded-preview", path: source });
    } catch (reason) { if (sourceRef.current === source) setError(String(reason)); }
    finally { filePreviewWrite.current = false; setCapturingPreview(false); }
  };
  const updateFilePreview = async (clear: boolean) => {
    if (!currentFile || filePreviewWrite.current || savingRef.current) return;
    const source = currentFile;
    let revision = filePreviewRevision.current;
    filePreviewWrite.current = true; setFilePreviewBusy(true); setError("");
    try {
      const imagePath = clear ? null : await selectPreviewImageFile();
      if ((!clear && !imagePath) || filePreviewRevision.current !== revision) return;
      revision = ++filePreviewRevision.current;
      if (clear) await clearProjectionPreviewImage(source);
      else await importProjectionPreviewImage(source, imagePath!);
      const image = await readProjectionPreviewImage(source);
      if (filePreviewRevision.current === revision) {
        setFilePreview(image?.data_url || ""); setClearPreviewConfirm(false);
        setStatus(clear ? "文件预览图已清空。" : "文件预览图已导入。");
      }
      // Only the preview field changes; the user's unsaved form stays intact.
      await emitEvent(projectionLibraryStateChangedEvent, { reason: "embedded-preview", path: source });
    } catch (reason) { if (filePreviewRevision.current === revision) setError(String(reason)); }
    finally { filePreviewWrite.current = false; setFilePreviewBusy(false); }
  };
  const refreshAnalysis = async (filePath = currentFile, preferredAnalysis?: unknown) => {
    if (!filePath) return;
    const generation = ++analysisGeneration.current;
    const source = preferredAnalysis === undefined ? "source" : "application";
    console.info(`[NOVA_METADATA_REQUEST] source=${source} path=${filePath} request started`);
    try {
      const parsed = preferredAnalysis ?? await analyzeProjectionFile(filePath);
      if (generation !== analysisGeneration.current) return;
      setData(parsed);
      const nextForm = metadataToForm(parsed);
      const recovered = takeUpdateDraft<{ form: MetadataForm | null; initialForm: MetadataForm | null }>(`properties:${filePath}`);
      setInitialForm(recovered?.initialForm || nextForm);
      setForm(recovered?.form || nextForm);
      setError("");
      console.info(`[NOVA_METADATA_REQUEST] source=${source} path=${filePath} request succeeded`);
      void loadLibrary()
        .then((state) => addOrUpdateRecordFromAnalysis(
          state,
          filePath,
          projectionRecordAnalysis(parsed),
        ))
        .catch((error) => {
          console.warn(`[NOVA_METADATA_REQUEST] path=${filePath} library update failed: ${String(error)}`);
        });
    } catch (error) {
      if (generation === analysisGeneration.current) {
        console.error(`[NOVA_METADATA_REQUEST] source=${source} path=${filePath} request failed: ${String(error)}`);
      }
      throw error;
    }
  };

  useEffect(() => {
    if (!currentFile) {
      analysisGeneration.current += 1;
      setData(null);
      setInitialForm(null);
      setForm(null);
      return;
    }
    setStatus("");
    setError("");
    setForm(null);
    const file = currentFile;
    refreshAnalysis(file, applicationAnalysis || undefined).catch((err) => {
      if (sourceRef.current === file) setError(String(err));
    });
    return () => { analysisGeneration.current += 1; };
    // A render analysis arriving for this same file is not a metadata reload:
    // keep the user's unsaved edits. Explicit Save still refreshes from disk.
  }, [currentFile]);

  const fileName = currentFile ? currentFile.split(/[\\/]/).pop() : "";
  const metadata = data?.metadata || {};
  const regionNames: string[] | undefined = Array.isArray(data?.regions) && data.regions.every((region: any) => typeof region?.name === "string") ? data.regions.map((region: any) => region.name) : undefined;
  const derived = data?.derived || {};
  const density = derived.building?.density
    ?? ((data?.structure_stats?.total_non_air_blocks || 0) / Math.max(1, metadata.total_volume || 0));
  const regionError = useMemo(() => validateRegions(form?.regions || []), [form?.regions]);

  const updateForm = (patch: Partial<MetadataForm>) => {
    setForm((current) => current ? { ...current, ...patch } : current);
  };

  const hasChanges = !!form && JSON.stringify(form) !== JSON.stringify(initialForm);

  const savePatch = async (saveAs = false) => {
    if (savingRef.current || filePreviewWrite.current || !currentFile || !form) return;
    if (regionError) { setError(regionError); return; }
    const source = currentFile;
    const patch = buildPatch(form);
    savingRef.current = true;
    setSaving(true);
    setError("");
    setStatus("");
    try {
      const outputPath = saveAs
        ? await selectLitematicSavePath(source.replace(/\.litematic$/i, ".edited.litematic"))
        : undefined;
      if (saveAs && !outputPath) return;
      await saveProjectionMetadataPatch({ currentFile: source, patch, outputPath: outputPath || undefined });
      if (sourceRef.current !== source) return;
      const nextFile = outputPath || source;
      if (outputPath && outputPath !== source) {
        setCurrentFile(outputPath);
      } else {
        await refreshAnalysis(nextFile);
      }
      setStatus(outputPath ? `已另存为：${displayFilePath(outputPath)}` : "已保存，文件中的方块保持不变。");
      setConvertOpen(false);
    } catch (err) {
      if (sourceRef.current === source) setError(String(err));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const handleSave = () => savePatch();
  const handleSaveAs = () => savePatch(true);

  const handleRestore = () => {
    if (initialForm && !savingRef.current) {
      setForm(JSON.parse(JSON.stringify(initialForm)));
      setError("");
      setStatus("已撤销修改。");
    }
  };

  const handleSelectFile = async () => {
    const res = await selectLitematicFile();
    if (res) {
      setCurrentFile(res);
    }
  };

  const previewFailure = renderMode === "fast" ? preview.error || quickOperation?.error : fullPreview.error;
  const modeLabel = renderMode === "fast" ? "快速" : renderMode === "standard" ? "标准" : "完整";
  const previewStatus = !currentFile ? "请选择投影" : previewFailure ? "预览加载失败，请重新加载。"
    : previewPending || renderPending || (viewerPrepared && !previewVisible) ? (renderMode === "fast" ? "正在加载…" : fullPreview.progressLabel)
    : previewVisible ? "" : "尚未加载预览";

  return (
    <section className="properties-page" aria-busy={saving}>
      <header className="properties-header">
        <h2 className="properties-title">投影属性</h2>
        <div className="properties-toolbar">
          <Button disabled={saving} onClick={handleSelectFile}><ThemeIcon name="import" size={16} fallback={FolderOpen} />选择投影…</Button>
          <Button variant="ghost" disabled={saving} onClick={() => setRoute("library")}><ThemeIcon name="gallery" size={16} fallback={Library} />从投影库选择</Button>
        </div>
      </header>
      <div className="properties-file muted" title={displayFilePath(currentFile || "")}>{fileName || "尚未选择投影"}</div>
      {error && <div className="properties-message properties-error nova-text-danger" role="alert">{displayPathText(error)}</div>}
      {status && <div className="properties-message" role="status">{displayPathText(status)}</div>}
      {!currentFile ? <div className="properties-empty muted"><Box size={40} aria-hidden="true" /><span>选择投影，编辑信息并查看 3D 预览。</span></div> : (
        <div className="properties-workspace">
          <ScrollArea className="properties-editor-scroll" viewportClassName="properties-editor-viewport">
            {!form ? (!error && <div className="properties-empty muted" role="status">正在读取投影属性…</div>) : (
            <fieldset className="properties-editor nova-box-plain" disabled={saving || filePreviewBusy || capturingPreview}>
                <legend className="properties-sr-only">投影属性</legend>
                  <TextRow label="内部名称" value={form.name} onChange={(value) => updateForm({ name: value })} />
                  <TextRow label="作者" value={form.author} onChange={(value) => updateForm({ author: value })} />
                  <label className="properties-field">
                    <span>描述</span>
                    <Textarea rows={4} value={form.description} onChange={(event) => updateForm({ description: event.target.value })} />
                  </label>
                  <dl className="properties-facts nova-box-plain">
                    <div><dt>投影名称</dt><dd>{fileName}</dd></div>
                    <div><dt>Minecraft 版本</dt><dd>{minecraftVersionLabel(form.minecraft_data_version)}</dd></div>
                    <div><dt>选区名</dt><dd>{regionNames === undefined ? "未读取" : regionNames.length ? regionNames.map((name, index) => <div key={index}>{name || "（空名称）"}</div>) : "无选区"}</dd></div>
                    <div><dt>尺寸（长 × 高 × 宽）</dt><dd>{metadata.enclosing_size?.x || 0} × {metadata.enclosing_size?.y || 0} × {metadata.enclosing_size?.z || 0}</dd></div>
                    <div><dt>方块数量</dt><dd>{Number(metadata.total_blocks || 0).toLocaleString()}</dd></div>
                    <div><dt>包围体积</dt><dd>{Number(metadata.total_volume || 0).toLocaleString()}</dd></div>
                    <div><dt>空间占用</dt><dd>{(density * 100).toFixed(2)}%</dd></div>
                  </dl>
                  {regionError && <div className="properties-error nova-text-danger" role="alert">{regionError}</div>}
                  <ScrollArea className="properties-table-scroll" horizontal>
                    <Table>
                      <TableHeader><TableRow><TableHead>区域名称（可编辑）</TableHead><TableHead>尺寸 x / y / z</TableHead><TableHead>位置 x / y / z</TableHead></TableRow></TableHeader>
                      <TableBody>{form.regions.map((region, index) => (
                        <TableRow key={region.originalName}>
                          <TableCell><Input aria-label={`区域 ${index + 1} 名称`} value={region.name} onChange={(event) => {
                            const regions = [...form.regions];
                            regions[index] = { ...region, name: event.target.value };
                            updateForm({ regions });
                          }} /></TableCell>
                          <TableCell>{region.size.x} / {region.size.y} / {region.size.z}</TableCell>
                          <TableCell>{region.position.x} / {region.position.y} / {region.position.z}</TableCell>
                        </TableRow>
                      ))}</TableBody>
                    </Table>
                  </ScrollArea>
                  <DateRow label="创建时间" value={form.time_created} onChange={(value) => updateForm({ time_created: value })} />
                  <DateRow label="修改时间" value={form.time_modified} onChange={(value) => updateForm({ time_modified: value })} />
                  <dl className="properties-facts nova-box-plain"><div className="properties-format-row"><dt>投影格式</dt><dd>Litematica v{form.litematic_version}.{form.litematic_subversion}<Button size="sm" variant="ghost" disabled={saving || hasChanges} onClick={() => setVersionConvertOpen(true)}>转换版本</Button></dd></div></dl>
                <div className="properties-file-preview">
                  <div className="properties-preview-toolbar"><h3>文件内预览图</h3>
                    <Button size="sm" variant="ghost" onClick={() => void updateFilePreview(false)}><ThemeIcon name="picture_import" size={16} fallback={ImagePlus} />导入图片</Button>
                    <Button size="sm" variant="ghost" disabled={!filePreview} onClick={() => setClearPreviewConfirm(true)}><ThemeIcon name="picture_error" size={16} fallback={ImageOff} />清空</Button>
                  </div>
                  <div className="properties-file-preview-canvas">{filePreview ? <img src={filePreview} alt="投影文件内嵌预览图" /> : <span>文件内暂无预览图</span>}</div>
                </div>
              </fieldset>
            )}
          </ScrollArea>
          <section className="properties-preview" aria-label="投影渲染">
            <header className="properties-preview-toolbar">
              <h3>{modeLabel}预览</h3>
              {(["fast", "standard", "full"] as const).map((mode) => <Button key={mode} size="sm" variant={renderMode === mode ? "default" : "ghost"} aria-pressed={renderMode === mode}
                onClick={() => setRenderMode(mode)}>{mode === "fast" ? "快速模式" : mode === "standard" ? "标准模式" : "完整模式"}</Button>)}
              <label><Checkbox checked={preload} disabled={previewPending || renderPending} onCheckedChange={(checked) => setPreload(checked === true)} />预加载</label>
              <Button size="sm" onClick={() => void handlePreview()} disabled={previewPending || (renderPending && !viewerPrepared)}>
                <ThemeIcon name={renderMode === "full" ? "build" : "run"} size={15} fallback={renderMode === "full" ? Box : Play} />{previewPending ? "正在连接…" : renderPending ? "正在加载…" : viewerPrepared ? "页内显示" : quickOperation ? "重新加载" : `${modeLabel}渲染`}
              </Button>
              {(renderPending || (renderMode !== "fast" && previewPending)) && !viewerPrepared && <Button size="sm" variant="ghost" onClick={() => void handleCancelPreview()}>取消</Button>}
              <Button size="sm" variant="ghost" disabled={!viewerPrepared || previewPending || saving || filePreviewBusy || capturingPreview} onClick={() => void captureFilePreview()}><ThemeIcon name="picture_trans" size={15} fallback={ImagePlus} />{capturingPreview ? "正在保存…" : "设为缩略图"}</Button>
              <Button size="sm" variant="ghost" onClick={() => setRoute("render")} ><ThemeIcon name="outlink" size={15} fallback={ExternalLink} />渲染页</Button>
            </header>
            <div ref={renderMode === "fast" ? preview.hostRef : fullPreview.hostRef} className="properties-preview-host" aria-label={`${modeLabel}预览视口`}>
              {!previewVisible && <div className="properties-preview-placeholder muted"><ThemeIcon name="empty" size={36} fallback={Box} /><span>{previewFailure ? "预览加载失败" : previewStatus}</span></div>}
            </div>
            {renderMode !== "fast" && fullPreview.preflight && <div role="status" className="properties-preview-status muted">
              <p>显存需求：{fullPreview.preflight.conservativeVramBytes == null ? "未知" : formatResourceBytes(fullPreview.preflight.conservativeVramBytes)}</p>
              {fullPreview.preflight.severity === "warning" && <p>需求达到当前可用预算的 80%，确认后才继续预加载。</p>}
              {fullPreview.preflight.severity === "hard" && <p>需求达到当前可用预算的 95%，预加载不可用，可选择实时加载。</p>}
              {fullPreview.preflight.severity === "unmeasured" && <p role="alert">{fullPreview.preflight.invalidReason ? displayPathText(fullPreview.preflight.invalidReason) : "需求或当前预算尚未确认，预加载不可用，可选择实时加载。"}</p>}
              <div className="properties-preview-toolbar">
                <Button size="sm" disabled={previewPending || !fullPreview.preflight.allowedActions.includes("continue_prebuilt")} onClick={() => void fullPreview.choose("prebuilt")}>确认预构建</Button>
                <Button size="sm" disabled={previewPending || (fullPreview.preflight.severity !== "normal" && !fullPreview.preflight.allowedActions.includes("switch_realtime"))} onClick={() => void fullPreview.choose("realtime")}>直接打开</Button>
                {renderMode === "full" && <Button size="sm" variant="ghost" disabled={previewPending || !fullPreview.preflight.allowedActions.includes("use_basic_vulkan")} onClick={() => void fullPreview.choose("basic_vulkan")}>降低画质</Button>}
                <Button size="sm" variant="ghost" onClick={() => void fullPreview.close()}>取消</Button>
              </div>
            </div>}
            {previewFailure && <div className="properties-preview-error nova-text-danger" role="alert">{displayPathText(previewFailure)}</div>}
            <div className="properties-preview-status muted" role="status" aria-live="polite">{previewStatus}</div>
          </section>
        </div>
      )}
      {clearPreviewConfirm && <Dialog title="清空文件预览图" busy={filePreviewBusy} onClose={() => setClearPreviewConfirm(false)} footer={<><Button variant="ghost" disabled={filePreviewBusy} onClick={() => setClearPreviewConfirm(false)}>取消</Button><Button variant="destructive" disabled={filePreviewBusy} onClick={() => void updateFilePreview(true)}>确认清空</Button></>}><p>清空此投影文件的内嵌预览图？方块和未保存的属性编辑不会改变。</p></Dialog>}
      {currentFile && <div className="properties-actions">
        <span className="muted">{saving ? "正在保存…" : !form ? "正在读取属性…" : hasChanges ? "有未保存的修改" : "所有修改已保存"}</span>
        <Button variant="ghost" onClick={handleRestore} disabled={!form || !hasChanges || saving || filePreviewBusy || capturingPreview}><ThemeIcon name="undo" size={16} fallback={RotateCcw} />撤销未保存修改</Button>
        <Button variant="ghost" onClick={() => setConvertOpen(true)} disabled={saving || filePreviewBusy || !form}>其他格式</Button>
        <Button onClick={handleSaveAs} disabled={!form || !!regionError || saving || filePreviewBusy || capturingPreview}>另存副本…</Button>
        <Button variant="default" onClick={handleSave} disabled={!form || !hasChanges || !!regionError || saving || filePreviewBusy || capturingPreview}><ThemeIcon name="save" size={16} fallback={Save} />{saving ? "保存中…" : "保存修改"}</Button>
      </div>}
      {versionConvertOpen && <VersionConversionDialog sourcePath={currentFile} onClose={() => setVersionConvertOpen(false)} onSaved={(path) => { setStatus(`已转换并另存：${displayFilePath(path)}；原文件保留。`); setVersionConvertOpen(false); }} />}
      {convertOpen && <Dialog title="保存格式" busy={saving} onClose={() => setConvertOpen(false)} width="sm" footer={
        <Button disabled={saving} onClick={handleSaveAs}>另存为 .litematic…</Button>
      }>
        <p>当前支持保存为 .litematic，保留投影中的方块。</p>
        <p className="muted">.schem、.nbt 和 .schematic 导出尚未完成。</p>
      </Dialog>}
    </section>
  );
}

function TextRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return <label className="properties-field"><span>{label}</span><Input value={value} onChange={(event) => onChange(event.target.value)} /></label>;
}

function DateRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return <label className="properties-field"><span>{label}</span><Input type="datetime-local" value={value} onChange={(event) => onChange(event.target.value)} /></label>;
}

function NumberRow({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return <label className="properties-field"><span>{label}</span><Input type="number" value={value} onChange={(event) => onChange(Number(event.target.value))} /></label>;
}
