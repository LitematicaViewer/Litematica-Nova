import React, { useEffect, useMemo, useRef, useState } from "react";
import { takeUpdateDraft, useUpdateDraft } from "../../updateRecovery";
import {
  analyzeProjectionFile,
  saveProjectionMetadataPatch,
  selectLitematicFile,
  selectLitematicSavePath,
} from "../../../../../src/business/facade";
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

import { Box, FolderOpen, Library, Play, RotateCcw, Save, ExternalLink } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Textarea } from "../../../../components/ui/textarea";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from "../../../../components/ui/table";
import { useQuickPreview } from "../../useQuickPreview";
import { Dialog } from "../../../../components/Dialog";
import "./properties.css";

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
    litematic_version: Number(form.litematic_version) || 6,
    litematic_subversion: Number(form.litematic_subversion) || 1,
    minecraft_data_version: Number(form.minecraft_data_version) || 0,
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

export function PropertiesPage({ currentFile, setCurrentFile, setRoute }: any) {
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
  const preview = useQuickPreview(currentFile || "", convertOpen);
  const quickOperation = preview.operation;
  const renderPending = quickOperation?.state === "starting" && !quickOperation.error;
  const viewerPrepared = quickOperation?.state === "ready";
  const previewPending = preview.pending;
  const handlePreview = preview.start;
  const handleCancelPreview = preview.close;
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
      if (preferredAnalysis === undefined) {
        void loadLibrary()
          .then((state) => addOrUpdateRecordFromAnalysis(
            state,
            filePath,
            projectionRecordAnalysis(parsed),
          ))
          .catch((error) => {
            console.warn(`[NOVA_METADATA_REQUEST] path=${filePath} library update failed: ${String(error)}`);
          });
      }
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
  const derived = data?.derived || {};
  const density = derived.building?.density
    ?? ((data?.structure_stats?.total_non_air_blocks || 0) / Math.max(1, metadata.total_volume || 0));
  const regionError = useMemo(() => validateRegions(form?.regions || []), [form?.regions]);

  const updateForm = (patch: Partial<MetadataForm>) => {
    setForm((current) => current ? { ...current, ...patch } : current);
  };

  const hasChanges = !!form && JSON.stringify(form) !== JSON.stringify(initialForm);

  const savePatch = async (saveAs = false) => {
    if (savingRef.current || !currentFile || !form) return;
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
      setStatus(outputPath ? `已另存为：${outputPath}` : "已保存，文件中的方块保持不变。");
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
      setStatus("已撤销未保存的修改，恢复为最近一次读取或保存的内容。");
    }
  };

  const handleSelectFile = async () => {
    const res = await selectLitematicFile();
    if (res) {
      setCurrentFile(res);
    }
  };

  const previewFailure = preview.error || quickOperation?.error;
  const previewStatus = !quickOperation
    ? "快速渲染使用独立的简化预览，不开启光影。"
    : viewerPrepared
      ? "拖动旋转 · 滚轮缩放"
      : renderPending
        ? "正在准备快速预览…"
        : quickOperation.state === "closed" ? "快速预览已关闭，可重新加载。" : "快速预览加载失败，可重新加载。";

  return (
    <section className="properties-page" aria-busy={saving}>
      <header className="properties-header">
        <h2 className="properties-title">投影属性</h2>
        <div className="properties-toolbar">
          <Button disabled={saving} onClick={handleSelectFile}><FolderOpen size={16} aria-hidden="true" />选择投影…</Button>
          <Button variant="ghost" disabled={saving} onClick={() => setRoute("library")}><Library size={16} aria-hidden="true" />从投影库选择</Button>
        </div>
      </header>
      <div className="properties-file" title={currentFile}>{fileName || "尚未选择投影"}</div>
      {error && <div className="properties-message properties-error" role="alert">{error}</div>}
      {status && <div className="properties-message" role="status">{status}</div>}
      {!currentFile ? <div className="properties-empty"><Box size={40} aria-hidden="true" /><span>选择投影，编辑信息并查看 3D 预览。</span></div> : (
        <div className="properties-workspace">
          <ScrollArea className="properties-editor-scroll" viewportClassName="properties-editor-viewport">
            {!form ? (!error && <div className="properties-empty" role="status">正在读取投影属性…</div>) : (
              <fieldset className="properties-editor" disabled={saving}>
                <legend className="properties-sr-only">投影属性</legend>
                <section className="properties-section">
                  <h3>基本信息</h3>
                  <TextRow label="投影名称" value={form.name} onChange={(value) => updateForm({ name: value })} />
                  <TextRow label="作者" value={form.author} onChange={(value) => updateForm({ author: value })} />
                  <label className="properties-field">
                    <span>描述</span>
                    <Textarea rows={4} value={form.description} onChange={(event) => updateForm({ description: event.target.value })} />
                  </label>
                  <dl className="properties-facts">
                    <div><dt>尺寸（长 × 高 × 宽）</dt><dd>{metadata.enclosing_size?.x || 0} × {metadata.enclosing_size?.y || 0} × {metadata.enclosing_size?.z || 0}</dd></div>
                    <div><dt>方块数量</dt><dd>{Number(metadata.total_blocks || 0).toLocaleString()}</dd></div>
                    <div><dt>包围体积</dt><dd>{Number(metadata.total_volume || 0).toLocaleString()}</dd></div>
                    <div><dt>空间占用</dt><dd>{(density * 100).toFixed(2)}%</dd></div>
                  </dl>
                </section>
                <details className="properties-details">
                  <summary>区域名称与位置 · {form.regions.length} 个区域</summary>
                  {regionError && <div className="properties-error" role="alert">{regionError}</div>}
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
                </details>
                <details className="properties-details">
                  <summary>高级属性 · 时间与文件版本</summary>
                  <p className="muted">版本号不是格式转换工具；不确定时请保持原值。</p>
                  <DateRow label="创建时间" value={form.time_created} onChange={(value) => updateForm({ time_created: value })} />
                  <DateRow label="修改时间" value={form.time_modified} onChange={(value) => updateForm({ time_modified: value })} />
                  <NumberRow label="投影文件版本" value={form.litematic_version} onChange={(value) => updateForm({ litematic_version: value })} />
                  <NumberRow label="文件子版本" value={form.litematic_subversion} onChange={(value) => updateForm({ litematic_subversion: value })} />
                  <NumberRow label="Minecraft 数据版本" value={form.minecraft_data_version} onChange={(value) => updateForm({ minecraft_data_version: value })} />
                </details>
              </fieldset>
            )}
          </ScrollArea>
          <section className="properties-preview" aria-label="投影快速渲染">
            <header className="properties-preview-toolbar">
              <h3>快速预览</h3>
              <Button size="sm" onClick={() => void handlePreview()} disabled={previewPending || (renderPending && !viewerPrepared)}>
                <Play size={15} aria-hidden="true" />{previewPending ? "正在连接…" : viewerPrepared ? "页内显示" : renderPending ? "正在加载…" : quickOperation ? "重新加载" : "快速渲染"}
              </Button>
              {renderPending && !viewerPrepared && <Button size="sm" variant="ghost" disabled={previewPending} onClick={() => void handleCancelPreview()}>取消</Button>}
              <Button size="sm" variant="ghost" onClick={() => setRoute("render")} title="前往渲染页使用原生页内预览"><ExternalLink size={15} aria-hidden="true" />渲染页</Button>
            </header>
            <div ref={preview.hostRef} className="properties-preview-host" aria-label="快速 Viewer 页内视口">
              <div className="properties-preview-placeholder"><Box size={36} aria-hidden="true" /><span>{previewFailure ? "预览加载失败" : previewStatus}</span></div>
            </div>
            {previewFailure && <div className="properties-preview-error" role="alert">{previewFailure}</div>}
            <div className="properties-preview-status" role="status" aria-live="polite">{previewStatus}</div>
            <div className="properties-full-placeholder"><Button size="sm" disabled>完整模式</Button><span>完整模式未包含在公开版</span></div>
          </section>
        </div>
      )}
      {currentFile && <div className="properties-actions">
        <span className="muted">{saving ? "正在保存…" : !form ? "正在读取属性…" : hasChanges ? "有未保存的修改" : "所有修改已保存"}</span>
        <Button variant="ghost" onClick={handleRestore} disabled={!form || !hasChanges || saving}><RotateCcw size={16} aria-hidden="true" />撤销未保存修改</Button>
        <Button variant="ghost" onClick={() => setConvertOpen(true)} disabled={saving || !form}>其他格式</Button>
        <Button onClick={handleSaveAs} disabled={!form || !!regionError || saving}>另存副本…</Button>
        <Button variant="default" onClick={handleSave} disabled={!form || !hasChanges || !!regionError || saving}><Save size={16} aria-hidden="true" />{saving ? "保存中…" : "保存修改"}</Button>
      </div>}
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
