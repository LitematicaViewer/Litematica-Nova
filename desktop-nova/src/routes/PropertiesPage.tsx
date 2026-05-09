import React, { useEffect, useMemo, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  executeBackend,
  getUserConfigFilePath,
  startNativeViewer,
  writeUserConfigFile,
} from "../services/backend";
import { addOrUpdateRecord, loadLibrary } from "../services/libraryStore";
import { getLatestRenderCacheState, subscribeRenderCacheStore } from "../services/renderCacheStore";
import { hideEmbeddedViewer } from "../services/embeddedViewer";

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
    const res = await executeBackend("litematica_core.exe", ["analyze", filePath]);
    const parsed = JSON.parse(res);
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
    const relative = `render/metadata_patch_${Date.now()}.json`;
    const patchPath = await getUserConfigFilePath(relative);
    await writeUserConfigFile(relative, JSON.stringify(buildPatch(form), null, 2));
    const args = ["edit-metadata", "--input", currentFile, "--patch", patchPath];
    if (outputPath) args.push("--output", outputPath);
    const result = await executeBackend("litematica_core.exe", args);
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
    const selected = await saveDialog({
      defaultPath: currentFile?.replace(/\.litematic$/i, ".edited.litematic") || "edited.litematic",
      filters: [{ name: "Litematic", extensions: ["litematic"] }],
    });
    if (selected && typeof selected === "string") {
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
    const res = await openDialog({ filters: [{ name: "Litematic", extensions: ["litematic"] }] });
    if (res && typeof res === "string") {
      setCurrentFile(res);
      const state = await loadLibrary();
      await addOrUpdateRecord(state, res);
    }
  };

  if (!currentFile) {
    return (
      <div style={{ display: "flex", flexDirection: "column", height: "100%", gap: 12 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <button className="btn" onClick={handleSelectFile}>选择文件...</button>
          <button className="btn" onClick={() => setRoute("library")}>在库中选择...</button>
        </div>
        <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", opacity: 0.55 }}>
          请先选择一个 .litematic 文件。
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <button className="btn" onClick={handleSelectFile}>选择文件...</button>
        <button className="btn" onClick={() => setRoute("library")}>在库中选择...</button>
        <div style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", opacity: 0.8 }} title={currentFile}>
          {currentFile}
        </div>
      </div>

      {error && <pre style={{ whiteSpace: "pre-wrap", color: "#ffb3b3", background: "#1a0808", border: "1px solid #884444", padding: 8 }}>{error}</pre>}
      {status && <pre style={{ whiteSpace: "pre-wrap", color: "#cfcfcf", background: "#111", border: "1px solid #444", padding: 8, maxHeight: 140, overflow: "auto" }}>{status}</pre>}

      {form && (
        <div style={{ flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: 12, paddingRight: 4 }}>
          <div className="group-box">
            <div className="group-box-title">文件</div>
            <div className="form-row">
              <div className="form-label" style={{ width: 130 }}>文件名</div>
              <input className="input" style={{ flex: 1 }} value={fileName} readOnly />
            </div>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "minmax(520px, 1fr) 340px", gap: 12 }}>
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
                <div className="form-label" style={{ width: 130 }}>尺寸</div>
                <div className="form-field" style={{ gap: 8 }}>
                  <input className="input" style={{ width: 70 }} value={metadata.enclosing_size?.x || 0} readOnly />
                  <input className="input" style={{ width: 70 }} value={metadata.enclosing_size?.y || 0} readOnly />
                  <input className="input" style={{ width: 70 }} value={metadata.enclosing_size?.z || 0} readOnly />
                </div>
              </div>
              <div className="form-row">
                <div className="form-label" style={{ width: 130 }}>统计</div>
                <div className="form-field" style={{ gap: 8 }}>
                  <input className="input" style={{ width: 100 }} value={metadata.total_blocks || 0} readOnly />
                  <input className="input" style={{ width: 100 }} value={metadata.total_volume || 0} readOnly />
                  <input className="input" style={{ width: 100 }} value={`${(((derived.building?.density || 0) * 100)).toFixed(2)}%`} readOnly />
                </div>
              </div>
            </div>

            <div className="group-box" style={{ display: "flex", flexDirection: "column" }}>
              <div className="group-box-title">3D 静态预览</div>
              <div style={{ minHeight: 220, background: "#10151a", border: "1px solid #000", display: "flex", alignItems: "center", justifyContent: "center" }}>
                {previewDataUrl ? <img src={previewDataUrl} alt="" style={{ width: "100%", height: "100%", objectFit: "cover" }} /> : <span style={{ opacity: 0.6 }}>请先在渲染页构建 3D cache</span>}
              </div>
              <button className="btn" style={{ marginTop: 8 }} onClick={() => startNativeViewer(currentFile, previewMode)}>打开弹窗 Viewer</button>
            </div>
          </div>

          <div className="group-box">
            <div className="group-box-title">区域列表</div>
            {regionError && <div style={{ color: "#ffb3b3", marginBottom: 8 }}>{regionError}</div>}
            <table style={{ width: "100%", borderCollapse: "collapse", color: "#ccc", fontSize: "0.95em" }}>
              <thead>
                <tr style={{ background: "#222" }}>
                  <th style={th}>区域名称（双击/直接编辑）</th>
                  <th style={th}>尺寸 x</th>
                  <th style={th}>尺寸 y</th>
                  <th style={th}>尺寸 z</th>
                  <th style={th}>位置 x</th>
                  <th style={th}>位置 y</th>
                  <th style={th}>位置 z</th>
                </tr>
              </thead>
              <tbody>
                {form.regions.map((region, index) => (
                  <tr key={region.originalName} style={{ borderBottom: "1px solid #333" }}>
                    <td style={td}>
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
                    <td style={tdRight}>{region.size.x}</td>
                    <td style={tdRight}>{region.size.y}</td>
                    <td style={tdRight}>{region.size.z}</td>
                    <td style={tdRight}>{region.position.x}</td>
                    <td style={tdRight}>{region.position.y}</td>
                    <td style={tdRight}>{region.position.z}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      <div style={{ display: "flex", gap: 12, justifyContent: "flex-end" }}>
        <button className="btn" onClick={handleSave} disabled={!form || !!regionError}>保存</button>
        <button className="btn" onClick={handleSaveAs} disabled={!form || !!regionError}>另存为</button>
        <button className="btn" onClick={handleRestore} disabled={!form}>恢复默认值</button>
        <button className="btn" onClick={() => setConvertOpen(true)}>转换格式</button>
      </div>

      {convertOpen && (
        <div style={modalBackdrop} onClick={() => setConvertOpen(false)}>
          <div style={modalPanel} onClick={(event) => event.stopPropagation()}>
            <h3 style={{ marginTop: 0 }}>转换格式</h3>
            <div>当前格式：litematic</div>
            <div style={{ marginTop: 12 }}>
              <button className="btn" onClick={handleSaveAs}>.litematic 另存/重写 metadata</button>
            </div>
            <div style={{ opacity: 0.72, marginTop: 12 }}>.schem / .nbt / .schematic：后端暂未实现稳定 exporter，已禁用。</div>
            <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 16 }}>
              <button className="btn" onClick={() => setConvertOpen(false)}>关闭</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

const th: React.CSSProperties = { padding: "6px 8px", textAlign: "left", border: "1px solid #444" };
const td: React.CSSProperties = { padding: "6px 8px", border: "1px solid #333" };
const tdRight: React.CSSProperties = { ...td, textAlign: "right" };
const modalBackdrop: React.CSSProperties = { position: "fixed", inset: 0, background: "rgba(0,0,0,0.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 1000 };
const modalPanel: React.CSSProperties = { width: 460, background: "var(--surface)", border: "1px solid var(--border)", padding: 16, boxShadow: "0 10px 30px rgba(0,0,0,0.45)" };

function TextRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <div className="form-row">
      <div className="form-label" style={{ width: 130 }}>{label}</div>
      <input className="input" style={{ flex: 1 }} value={value} onChange={(event) => onChange(event.target.value)} />
    </div>
  );
}

function DateRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <div className="form-row">
      <div className="form-label" style={{ width: 130 }}>{label}</div>
      <input className="input" style={{ width: 220 }} type="datetime-local" value={value} onChange={(event) => onChange(event.target.value)} />
    </div>
  );
}

function NumberRow({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return (
    <div className="form-row">
      <div className="form-label" style={{ width: 130 }}>{label}</div>
      <input className="input" style={{ width: 140 }} type="number" value={value} onChange={(event) => onChange(Number(event.target.value))} />
    </div>
  );
}
