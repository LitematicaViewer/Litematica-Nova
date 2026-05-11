import React, { useEffect, useMemo, useState } from "react";
import {
  analyzeProjectionFile,
  openProjectionViewer,
  saveProjectionMetadataPatch,
  selectLitematicFile,
  selectLitematicSavePath,
} from "../../business/facade";
import { addOrUpdateRecord, loadLibrary } from "../../business/facade";
import { getLatestRenderCacheState, subscribeRenderCacheStore } from "../../business/facade";
import { hideEmbeddedViewer } from "../../business/facade";

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
  const [data, setData] = useState<any>(null);
  const [initialForm, setInitialForm] = useState<MetadataForm | null>(null);
  const [form, setForm] = useState<MetadataForm | null>(null);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [previewDataUrl, setPreviewDataUrl] = useState("");
  const [previewMode, setPreviewMode] = useState("normal");
  const [convertOpen, setConvertOpen] = useState(false);

  useEffect(() => {
    console.log("[LBA_EMBED_VIEWER] route_active=properties action=hide reason=no_embedded_container");
    hideEmbeddedViewer().catch(() => undefined);
  }, []);

  const refreshAnalysis = async (filePath = currentFile) => {
    if (!filePath) return;
    const parsed = await analyzeProjectionFile(filePath);
    setData(parsed);
    const nextForm = metadataToForm(parsed);
    setInitialForm(nextForm);
    setForm(nextForm);
    setError("");
    const state = await loadLibrary();
    await addOrUpdateRecord(state, filePath);
  };

  useEffect(() => {
    if (!currentFile) return;
    refreshAnalysis().catch((err) => setError(String(err)));
    const cacheState = getLatestRenderCacheState(currentFile);
    setPreviewDataUrl(cacheState?.previewPath || "");
    setPreviewMode(cacheState?.displayMode || "normal");
  }, [currentFile]);

  useEffect(() => subscribeRenderCacheStore(() => {
    if (!currentFile) return;
    const cacheState = getLatestRenderCacheState(currentFile);
    setPreviewDataUrl(cacheState?.previewPath || "");
    setPreviewMode(cacheState?.displayMode || "normal");
  }), [currentFile]);

  const fileName = currentFile ? currentFile.split(/[\\/]/).pop() : "";
  const metadata = data?.metadata || {};
  const derived = data?.derived || {};
  const regionError = useMemo(() => validateRegions(form?.regions || []), [form?.regions]);

  const updateForm = (patch: Partial<MetadataForm>) => {
    setForm((current) => current ? { ...current, ...patch } : current);
  };

  const savePatch = async (outputPath?: string) => {
    if (!currentFile || !form) return;
    if (regionError) {
      setError(regionError);
      return;
    }
    const result = await saveProjectionMetadataPatch({ currentFile, patch: buildPatch(form), outputPath });
    setStatus(result);
    const nextFile = outputPath || currentFile;
    if (outputPath) setCurrentFile(outputPath);
    await refreshAnalysis(nextFile);
  };

  const handleSave = async () => {
    try {
      await savePatch();
    } catch (err: any) {
      setError(String(err));
    }
  };

  const handleSaveAs = async () => {
    const selected = await selectLitematicSavePath(currentFile?.replace(/\.litematic$/i, ".edited.litematic") || "edited.litematic");
    if (selected) {
      try {
        await savePatch(selected);
      } catch (err: any) {
        setError(String(err));
      }
    }
  };

  const handleRestore = () => {
    if (initialForm) {
      setForm(JSON.parse(JSON.stringify(initialForm)));
      setError("");
      setStatus("已恢复为打开文件时的 metadata。");
    }
  };

  const handleSelectFile = async () => {
    const res = await selectLitematicFile();
    if (res) {
      setCurrentFile(res);
      const state = await loadLibrary();
      await addOrUpdateRecord(state, res);
    }
  };

  if (!currentFile) {
    return (
      <div className="nova-page">
        <div className="nova-toolbar">
          <button className="btn" onClick={handleSelectFile}>选择文件...</button>
          <button className="btn" onClick={() => setRoute("library")}>在库中选择...</button>
        </div>
        <div className="nova-empty-state">
          请先选择一个 .litematic 文件。
        </div>
      </div>
    );
  }

  return (
    <div className="nova-page">
      <div className="nova-toolbar">
        <button className="btn" onClick={handleSelectFile}>选择文件...</button>
        <button className="btn" onClick={() => setRoute("library")}>在库中选择...</button>
        <div className="properties-current-file" title={currentFile}>
          {currentFile}
        </div>
      </div>

      {error && <pre className="nova-error">{error}</pre>}
      {status && <pre className="nova-pre properties-status">{status}</pre>}

      {form && (
        <div className="nova-list-panel properties-form-scroll">
          <div className="group-box">
            <div className="group-box-title">文件</div>
            <div className="form-row">
              <div className="form-label properties-form-label">文件名</div>
              <input className="input nova-input-flex" value={fileName} readOnly />
            </div>
          </div>

          <div className="properties-grid">
            <div className="group-box">
              <div className="group-box-title">元数据</div>
              <TextRow label="内部名称" value={form.name} onChange={(value) => updateForm({ name: value })} />
              <TextRow label="作者" value={form.author} onChange={(value) => updateForm({ author: value })} />
              <TextRow label="描述" value={form.description} onChange={(value) => updateForm({ description: value })} />
              <DateRow label="创建时间" value={form.time_created} onChange={(value) => updateForm({ time_created: value })} />
              <DateRow label="修改时间" value={form.time_modified} onChange={(value) => updateForm({ time_modified: value })} />
              <NumberRow label="投影文件版本" value={form.litematic_version} onChange={(value) => updateForm({ litematic_version: value })} />
              <NumberRow label="SubVersion" value={form.litematic_subversion} onChange={(value) => updateForm({ litematic_subversion: value })} />
              <NumberRow label="Minecraft 数据版本" value={form.minecraft_data_version} onChange={(value) => updateForm({ minecraft_data_version: value })} />
              <div className="form-row">
                <div className="form-label properties-form-label">尺寸</div>
                <div className="form-field">
                  <input className="input properties-number-xs" value={metadata.enclosing_size?.x || 0} readOnly />
                  <input className="input properties-number-xs" value={metadata.enclosing_size?.y || 0} readOnly />
                  <input className="input properties-number-xs" value={metadata.enclosing_size?.z || 0} readOnly />
                </div>
              </div>
              <div className="form-row">
                <div className="form-label properties-form-label">统计</div>
                <div className="form-field">
                  <input className="input properties-number-sm" value={metadata.total_blocks || 0} readOnly />
                  <input className="input properties-number-sm" value={metadata.total_volume || 0} readOnly />
                  <input className="input properties-number-sm" value={`${(((derived.building?.density || 0) * 100)).toFixed(2)}%`} readOnly />
                </div>
              </div>
            </div>

            <div className="group-box nova-stack-tight">
              <div className="group-box-title">3D 静态预览</div>
              <div className="properties-preview">
                {previewDataUrl ? <img src={previewDataUrl} alt="" className="render-preview-image" /> : <span className="nova-subtle">请先在渲染页构建 3D cache</span>}
              </div>
              <button className="btn nova-mt-sm" onClick={() => openProjectionViewer(currentFile, previewMode)}>打开弹窗 Viewer</button>
            </div>
          </div>

          <div className="group-box">
            <div className="group-box-title">区域列表</div>
            {regionError && <div className="properties-region-error">{regionError}</div>}
            <table className="nova-table properties-region-table">
              <thead>
                <tr>
                  <th>区域名称（双击/直接编辑）</th>
                  <th>尺寸 x</th>
                  <th>尺寸 y</th>
                  <th>尺寸 z</th>
                  <th>位置 x</th>
                  <th>位置 y</th>
                  <th>位置 z</th>
                </tr>
              </thead>
              <tbody>
                {form.regions.map((region, index) => (
                  <tr key={region.originalName}>
                    <td>
                      <input
                        className="input"
                        value={region.name}
                        onChange={(event) => {
                          const regions = [...form.regions];
                          regions[index] = { ...region, name: event.target.value };
                          updateForm({ regions });
                        }}
                      />
                    </td>
                    <td className="nova-table-number">{region.size.x}</td>
                    <td className="nova-table-number">{region.size.y}</td>
                    <td className="nova-table-number">{region.size.z}</td>
                    <td className="nova-table-number">{region.position.x}</td>
                    <td className="nova-table-number">{region.position.y}</td>
                    <td className="nova-table-number">{region.position.z}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      <div className="properties-actions">
        <button className="btn" onClick={handleSave} disabled={!form || !!regionError}>保存</button>
        <button className="btn" onClick={handleSaveAs} disabled={!form || !!regionError}>另存为</button>
        <button className="btn" onClick={handleRestore} disabled={!form}>恢复默认值</button>
        <button className="btn" onClick={() => setConvertOpen(true)}>转换格式</button>
      </div>

      {convertOpen && (
        <div className="nova-dialog-backdrop" onClick={() => setConvertOpen(false)}>
          <div className="dialog-content properties-convert-dialog" onClick={(event) => event.stopPropagation()}>
            <h3>转换格式</h3>
            <div>当前格式：litematic</div>
            <div className="nova-mt-md">
              <button className="btn" onClick={handleSaveAs}>.litematic 另存/重写 metadata</button>
            </div>
            <div className="nova-muted-copy nova-mt-md">.schem / .nbt / .schematic：后端暂未实现稳定 exporter，已禁用。</div>
            <div className="properties-dialog-actions">
              <button className="btn" onClick={() => setConvertOpen(false)}>关闭</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function TextRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <div className="form-row">
      <div className="form-label properties-form-label">{label}</div>
      <input className="input nova-input-flex" value={value} onChange={(event) => onChange(event.target.value)} />
    </div>
  );
}

function DateRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <div className="form-row">
      <div className="form-label properties-form-label">{label}</div>
      <input className="input properties-date-input" type="datetime-local" value={value} onChange={(event) => onChange(event.target.value)} />
    </div>
  );
}

function NumberRow({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return (
    <div className="form-row">
      <div className="form-label properties-form-label">{label}</div>
      <input className="input nova-select-sm" type="number" value={value} onChange={(event) => onChange(Number(event.target.value))} />
    </div>
  );
}


