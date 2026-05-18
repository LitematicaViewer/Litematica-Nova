import { useEffect, useMemo, useState } from "react";
import {
  analyzeProjectionFile,
  clearProjectionPreviewImage,
  importProjectionPreviewImage,
  openProjectionViewer,
  saveProjectionMetadataPatch,
  selectLitematicFile,
  selectLitematicSavePath,
  selectPreviewImageFile,
} from "../../../../../src/business/facade";
import { addOrUpdateRecord, loadLibrary, setRecordPreview } from "../../../../../src/business/facade";
import { generatePreviewImage, readImageBase64, readProjectionPreviewImage } from "../../../../../src/business/facade";
import { getLatestRenderCacheState, subscribeRenderCacheStore } from "../../../../../src/business/facade";
import { hideEmbeddedViewer } from "../../../../../src/business/facade";
// 同级函数
import { buildPatch, metadataToForm, validateRegions } from "./function";

export type RegionEdit = {
  originalName: string;
  name: string;
  position: { x: number; y: number; z: number };
  size: { x: number; y: number; z: number };
};

export type MetadataForm = {
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

/**
 * Main Layout
 */
export function PropertiesPage({ currentFile, setCurrentFile, setRoute }: any) {
  const [data, setData] = useState<any>(null);
  const [initialForm, setInitialForm] = useState<MetadataForm | null>(null);
  const [form, setForm] = useState<MetadataForm | null>(null);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [projectionPreviewDataUrl, setProjectionPreviewDataUrl] = useState("");
  const [previewDataUrl, setPreviewDataUrl] = useState("");
  const [previewMode, setPreviewMode] = useState("normal");
  const [isGeneratingPreview, setIsGeneratingPreview] = useState(false);
  const [isUpdatingProjectionPreview, setIsUpdatingProjectionPreview] = useState(false);
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

  const refreshLibraryRenderPreview = async (filePath = currentFile) => {
    if (!filePath) return;
    const cacheState = getLatestRenderCacheState(filePath);
    if (cacheState?.previewPath) {
      setPreviewDataUrl(cacheState.previewPath);
      setPreviewMode(cacheState.displayMode || "normal");
      return;
    }

    const state = await loadLibrary();
    const record = state.records.find((item) => item.path === filePath);
    const imagePath = record?.preview_image_path || record?.previewPath;
    if (!imagePath) {
      setPreviewDataUrl("");
      return;
    }

    try {
      setPreviewDataUrl(await readImageBase64(imagePath));
    } catch {
      setPreviewDataUrl("");
    }
  };

  const refreshProjectionPreview = async (filePath = currentFile) => {
    if (!filePath) {
      setProjectionPreviewDataUrl("");
      return;
    }
    const output = await readProjectionPreviewImage(filePath);
    setProjectionPreviewDataUrl(output?.data_url || "");
  };

  useEffect(() => {
    if (!currentFile) return;
    refreshAnalysis().catch((err) => setError(String(err)));
    refreshLibraryRenderPreview().catch(() => setPreviewDataUrl(""));
  }, [currentFile]);

  useEffect(() => subscribeRenderCacheStore(() => {
    if (!currentFile) return;
    const cacheState = getLatestRenderCacheState(currentFile);
    if (cacheState?.previewPath) {
      setPreviewDataUrl(cacheState.previewPath);
      setPreviewMode(cacheState.displayMode || "normal");
      return;
    }
    refreshLibraryRenderPreview(currentFile).catch(() => setPreviewDataUrl(""));
  }), [currentFile]);

  useEffect(() => {
    let cancelled = false;
    if (!currentFile) {
      setProjectionPreviewDataUrl("");
      return;
    }
    setProjectionPreviewDataUrl("");
    readProjectionPreviewImage(currentFile)
      .then((output) => {
        if (!cancelled) setProjectionPreviewDataUrl(output?.data_url || "");
      })
      .catch(() => {
        if (!cancelled) setProjectionPreviewDataUrl("");
      });
    return () => {
      cancelled = true;
    };
  }, [currentFile]);

  const handleClearProjectionPreview = async () => {
    if (!currentFile || isUpdatingProjectionPreview) return;
    setIsUpdatingProjectionPreview(true);
    setError("");
    setStatus("正在清空预览图...");
    try {
      await clearProjectionPreviewImage(currentFile);
      await refreshProjectionPreview(currentFile);
      setStatus("预览图已清空。");
    } catch (err: any) {
      setError(String(err));
      setStatus("");
    } finally {
      setIsUpdatingProjectionPreview(false);
    }
  };

  const handleImportProjectionPreview = async () => {
    if (!currentFile || isUpdatingProjectionPreview) return;
    const imagePath = await selectPreviewImageFile();
    if (!imagePath) return;
    setIsUpdatingProjectionPreview(true);
    setError("");
    setStatus("正在导入预览图...");
    try {
      await importProjectionPreviewImage(currentFile, imagePath);
      await refreshProjectionPreview(currentFile);
      setStatus("预览图已导入。");
    } catch (err: any) {
      setError(String(err));
      setStatus("");
    } finally {
      setIsUpdatingProjectionPreview(false);
    }
  };

  const handleGenerateRenderPreview = async () => {
    if (!currentFile || isGeneratingPreview) return;
    setIsGeneratingPreview(true);
    setError("");
    setStatus("正在生成渲染图...");
    try {
      const output = await generatePreviewImage(currentFile, previewMode);
      setPreviewDataUrl(output.data_url);
      const state = await loadLibrary();
      await setRecordPreview(state, currentFile, output.preview_path);
      setStatus("渲染图已生成，并已同步到投影库 preview cache。");
    } catch (err: any) {
      setError(String(err));
      setStatus("");
    } finally {
      setIsGeneratingPreview(false);
    }
  };

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
      <div className="properties-page">
        <div className="properties-toolbar">
          <button className="btn" onClick={handleSelectFile}>选择文件...</button>
          <button className="btn" onClick={() => setRoute("library")}>在库中选择...</button>
        </div>
        <div className="properties-empty-state">
          请先选择一个 .litematic 文件。
        </div>
      </div>
    );
  }

  return (
    <div className="properties-page">
      <div className="properties-toolbar">
        <button className="btn" onClick={handleSelectFile}>选择文件...</button>
        <button className="btn" onClick={() => setRoute("library")}>在库中选择...</button>
        <div className="properties-file-path" title={currentFile}>
          {currentFile}
        </div>
      </div>

      {error && <pre className="properties-message properties-message-error">{error}</pre>}

      {form && (
        <div className="properties-scroll">
          <div className="group-box">
            <div className="group-box-title">文件</div>
            <div className="form-row">
              <div className="form-label properties-label">文件名</div>
              <input className="input properties-input-flex" value={fileName} readOnly />
            </div>
          </div>

          <div className="properties-main-grid">
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
                <div className="form-label properties-label">尺寸</div>
                <div className="form-field properties-axis-field">
                  <input className="input properties-size-input" value={metadata.enclosing_size?.x || 0} readOnly />
                  <input className="input properties-size-input" value={metadata.enclosing_size?.y || 0} readOnly />
                  <input className="input properties-size-input" value={metadata.enclosing_size?.z || 0} readOnly />
                </div>
              </div>
              <div className="form-row">
                <div className="form-label properties-label">统计</div>
                <div className="form-field properties-axis-field">
                  <input className="input properties-stat-input" value={metadata.total_blocks || 0} readOnly />
                  <input className="input properties-stat-input" value={metadata.total_volume || 0} readOnly />
                  <input className="input properties-stat-input" value={`${(((derived.building?.density || 0) * 100)).toFixed(2)}%`} readOnly />
                </div>
              </div>
            </div>

            <div className="group-box properties-preview-card">
              <div className="group-box-title">预览/渲染</div>

              <div className="properties-preview-section">
                <div className="properties-preview-section-title">预览图画布</div>
                <div className="properties-preview-canvas properties-projection-preview-canvas">
                  {projectionPreviewDataUrl ? <img className="properties-preview-img properties-projection-preview-img" src={projectionPreviewDataUrl} alt="" /> : <span className="properties-preview-placeholder">文件内暂无预览图</span>}
                </div>
                <div className="properties-preview-actions-row">
                  <button className="btn" onClick={handleClearProjectionPreview} disabled={!currentFile || isUpdatingProjectionPreview}>清空预览图</button>
                  <button className="btn" onClick={handleImportProjectionPreview} disabled={!currentFile || isUpdatingProjectionPreview}>导入预览图</button>
                </div>
              </div>

              <div className="properties-preview-section">
                <div className="properties-preview-section-title">渲染图画布</div>
                <div className="properties-preview-canvas properties-render-preview-canvas">
                  {previewDataUrl ? <img className="properties-preview-img" src={previewDataUrl} alt="" /> : <span className="properties-preview-placeholder">暂无渲染图 cache</span>}
                </div>
                <div className="properties-preview-actions-row">
                  <button className="btn" onClick={handleGenerateRenderPreview} disabled={!currentFile || isGeneratingPreview}>{isGeneratingPreview ? "生成中..." : "生成渲染"}</button>
                  <button className="btn" onClick={() => openProjectionViewer(currentFile, previewMode)} disabled={!currentFile}>打开弹窗 Viewer</button>
                </div>
              </div>
            </div>
          </div>

          <div className="group-box">
            <div className="group-box-title">区域列表</div>
            {regionError && <div className="properties-region-error">{regionError}</div>}
            <table>
              <thead>
                <tr>
                  <th>区域名称</th>
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
                    <td className="properties-table-number">{region.size.x}</td>
                    <td className="properties-table-number">{region.size.y}</td>
                    <td className="properties-table-number">{region.size.z}</td>
                    <td className="properties-table-number">{region.position.x}</td>
                    <td className="properties-table-number">{region.position.y}</td>
                    <td className="properties-table-number">{region.position.z}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      <div className="properties-footer">
        <div className="properties-footer-status" title={status}>{status}</div>
        <div className="properties-actions">
          <button className="btn" onClick={handleSave} disabled={!form || !!regionError}>保存</button>
          <button className="btn" onClick={handleSaveAs} disabled={!form || !!regionError}>另存为</button>
          <button className="btn" onClick={handleRestore} disabled={!form}>恢复默认值</button>
          <button className="btn" onClick={() => setConvertOpen(true)}>转换格式</button>
        </div>
      </div>

      {convertOpen && (
        <div className="properties-modal-backdrop" onClick={() => setConvertOpen(false)}>
          <div className="properties-modal-panel" onClick={(event) => event.stopPropagation()}>
            <h3 className="properties-modal-title">转换格式</h3>
            <div>当前格式：litematic</div>
            <div className="properties-modal-section">
              <button className="btn" onClick={handleSaveAs}>.litematic 另存/重写 metadata</button>
            </div>
            <div className="properties-modal-note">.schem / .nbt / .schematic：后端暂未实现稳定 exporter，已禁用。</div>
            <div className="properties-modal-footer">
              <button className="btn" onClick={() => setConvertOpen(false)}>关闭</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * 文本输入行控件行为
 * @param label 标签
 * @param value 值
 * @param onChange 值变化回调
 */
function TextRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <div className="form-row">
      <div className="form-label properties-label">{label}</div>
      <input className="input properties-input-flex" value={value} onChange={(event) => onChange(event.target.value)} />
    </div>
  );
}

/**
 * 日期输入行控件行为
 * @param label 标签
 * @param value 值
 * @param onChange 值变化回调
 */
function DateRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <div className="form-row">
      <div className="form-label properties-label">{label}</div>
      <input className="input properties-date-input" type="datetime-local" value={value} onChange={(event) => onChange(event.target.value)} />
    </div>
  );
}

/**
 * 数字输入行控件行为
 * @param label 标签
 * @param value 值
 * @param onChange 值变化回调
 */
function NumberRow({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return (
    <div className="form-row">
      <div className="form-label properties-label">{label}</div>
      <input className="input properties-number-input" type="number" value={value} onChange={(event) => onChange(Number(event.target.value))} />
    </div>
  );
}
