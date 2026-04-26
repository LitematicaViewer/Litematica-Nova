import React, { useState, useEffect, useRef } from 'react';
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { executeBackend, startNativeViewer } from '../services/backend';
import { addOrUpdateRecord, loadLibrary } from '../services/libraryStore';
import { getLatestRenderCacheState, subscribeRenderCacheStore } from '../services/renderCacheStore';
import {
  elementToPhysicalRect,
  elementToCssRect,
  getEmbeddedViewerStatus,
  hideEmbeddedViewer,
  isUsableEmbeddedRect,
  showEmbeddedViewer,
  startEmbeddedViewer,
  updateEmbeddedViewerBounds,
} from '../services/embeddedViewer';

function formatDate(timestamp: number): string {
  if (!timestamp) return "-";
  const d = new Date(timestamp);
  const pad = (n: number) => n.toString().padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function PropertiesPage({ currentFile, setCurrentFile, setRoute }: any) {
  const [data, setData] = useState<any>(null);
  const [error, setError] = useState("");
  const [cacheExists, setCacheExists] = useState(false);
  const [previewDataUrl, setPreviewDataUrl] = useState("");
  const [cacheStage, setCacheStage] = useState("");
  const [previewMode, setPreviewMode] = useState("normal");
  const [embeddedRunning, setEmbeddedRunning] = useState(false);
  const [embeddedError, setEmbeddedError] = useState("");
  const previewHostRef = useRef<HTMLDivElement | null>(null);

  const syncPreviewState = () => {
    if (!currentFile) {
      setCacheExists(false);
      setPreviewDataUrl("");
      setCacheStage("");
      return;
    }
    const cacheState = getLatestRenderCacheState(currentFile);
    setCacheExists(cacheState?.status === "ready");
    setPreviewDataUrl(cacheState?.previewPath || "");
    setCacheStage(cacheState?.stage || "");
    setPreviewMode(cacheState?.displayMode || "normal");
  };

  useEffect(() => {
    if (!currentFile) return;
    executeBackend("litematica_core.exe", ["analyze", currentFile])
      .then(res => {
        try { 
          setData(JSON.parse(res)); 
          setError(""); 
        }
        catch(e) { setError(res); }
      })
      .catch(e => setError(e));
    syncPreviewState();
  }, [currentFile]);

  useEffect(() => subscribeRenderCacheStore(syncPreviewState), [currentFile]);

  useEffect(() => {
    let cancelled = false;
    let resizeObserver: ResizeObserver | null = null;
    let updateTimer: number | null = null;
    let boundsInterval: number | null = null;
    let statusInterval: number | null = null;

    const clearUpdateTimer = () => {
      if (updateTimer !== null) {
        window.clearTimeout(updateTimer);
        updateTimer = null;
      }
    };

    const updateBounds = async () => {
      const element = previewHostRef.current;
      if (!element) {
        console.log("[LBA_EMBED_VIEWER]", {
          source: "properties_bounds_sync",
          status: "frontend_bounds_skip",
          skipped_reason: "missing_preview_host",
          visible: false,
        });
        await hideEmbeddedViewer().catch(() => {});
        return;
      }
      const rectCss = elementToCssRect(element);
      const rectPhysical = elementToPhysicalRect(element);
      if (!isUsableEmbeddedRect(rectCss) || !isUsableEmbeddedRect(rectPhysical)) {
        console.log("[LBA_EMBED_VIEWER]", {
          source: "properties_bounds_sync",
          status: "frontend_bounds_skip",
          skipped_reason: "invalid_or_hidden_rect",
          rect_css: rectCss,
          rect_physical: rectPhysical,
          visible: false,
        });
        await hideEmbeddedViewer().catch(() => {});
        return;
      }
      try {
        console.log("[LBA_EMBED_VIEWER]", {
          source: "properties_bounds_sync",
          status: "frontend_bounds_update",
          rect_css: rectCss,
          rect_physical: rectPhysical,
          visible: true,
        });
        await showEmbeddedViewer().catch(() => {});
        await updateEmbeddedViewerBounds(rectPhysical);
      } catch {
        // Best-effort bounds sync.
      }
    };

    const scheduleUpdateBounds = () => {
      clearUpdateTimer();
      updateTimer = window.setTimeout(updateBounds, 35);
    };

    const start = async () => {
      await new Promise((resolve) => requestAnimationFrame(() => window.setTimeout(resolve, 120)));
      if (cancelled) return;
      const element = previewHostRef.current;
      if (!currentFile || !cacheExists || !element) {
        await hideEmbeddedViewer().catch(() => {});
        setEmbeddedRunning(false);
        return;
      }
      try {
        const rectCss = elementToCssRect(element);
        const rectPhysical = elementToPhysicalRect(element);
        if (!isUsableEmbeddedRect(rectCss) || !isUsableEmbeddedRect(rectPhysical)) {
          console.log("[LBA_EMBED_VIEWER]", {
            source: "properties_bounds_sync",
            status: "frontend_start_skip",
            skipped_reason: "dom_not_stable",
            file: currentFile,
            mode: previewMode,
            rect_css: rectCss,
            rect_physical: rectPhysical,
            visible: false,
          });
          await hideEmbeddedViewer().catch(() => {});
          setEmbeddedRunning(false);
          return;
        }
        console.log("[LBA_EMBED_VIEWER]", {
          source: "properties_bounds_sync",
          status: "frontend_start",
          file: currentFile,
          mode: previewMode,
          rect_css: rectCss,
          rect_physical: rectPhysical,
          visible: true,
        });
        const status = await startEmbeddedViewer(currentFile, previewMode, rectPhysical, "properties_preview");
        if (cancelled) return;
        setEmbeddedRunning(status.running);
        setEmbeddedError(status.error || "");
        resizeObserver = new ResizeObserver(scheduleUpdateBounds);
        resizeObserver.observe(element);
        window.addEventListener('resize', scheduleUpdateBounds);
        window.addEventListener('scroll', scheduleUpdateBounds, true);
        boundsInterval = window.setInterval(scheduleUpdateBounds, 250);
        statusInterval = window.setInterval(async () => {
          const next = await getEmbeddedViewerStatus().catch(() => null);
          if (next) console.log("[LBA_EMBED_VIEWER]", { ...next, frontend_status: "poll" });
        }, 1000);
      } catch (err: any) {
        if (cancelled) return;
        setEmbeddedRunning(false);
        setEmbeddedError(String(err));
      }
    };

    start();

    return () => {
      cancelled = true;
      clearUpdateTimer();
      if (boundsInterval !== null) window.clearInterval(boundsInterval);
      if (statusInterval !== null) window.clearInterval(statusInterval);
      resizeObserver?.disconnect();
      window.removeEventListener('resize', scheduleUpdateBounds);
      window.removeEventListener('scroll', scheduleUpdateBounds, true);
      hideEmbeddedViewer().catch(() => {});
      setEmbeddedRunning(false);
    };
  }, [currentFile, cacheExists, previewMode]);

  const handleSelectFile = async () => {
    const res = await openDialog({ filters: [{ name: "Litematic", extensions: ["litematic"] }] });
    if (res && typeof res === "string") {
      setCurrentFile(res);
      const state = await loadLibrary();
      await addOrUpdateRecord(state, res);
    }
  };

  const handleSelectInLibrary = () => {
    setRoute("library");
  };

  const handleOpenPopup = async () => {
    if (!currentFile) return;
    await startNativeViewer(currentFile, previewMode);
  };

  const metadata = data?.metadata || {};
  const derived = data?.derived || {};

  const fileName = currentFile ? currentFile.split(/[\\/]/).pop() : "";
  const internalName = metadata.name || "Unnamed";
  const author = metadata.author || "Unknown";
  const desc = metadata.description || "";
  const tCreated = formatDate(metadata.time_created || 0);
  const tModified = formatDate(metadata.time_modified || 0);
  
  const sizeX = metadata.enclosing_size?.x || 0;
  const sizeY = metadata.enclosing_size?.y || 0;
  const sizeZ = metadata.enclosing_size?.z || 0;
  
  const totalBlocks = metadata.total_blocks || 0;
  const totalVolume = metadata.total_volume || 0;
  const density = derived.building?.density || 0;

  // Placeholder region list based on enclosing size if region details are missing
  const regions = [];
  if (metadata.region_count > 0) {
    regions.push({ name: "Unnamed", sx: sizeX, sy: sizeY, sz: sizeZ, px: 0, py: 0, pz: 0 });
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 12 }}>
      {/* 顶部文件选择行 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <button className="btn" onClick={handleSelectFile}>选择文件...</button>
        <button className="btn" onClick={handleSelectInLibrary}>在库中选择...</button>
        <div style={{ flex: 1, color: 'var(--fg)', fontSize: '0.9em', opacity: 0.8, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={currentFile}>
          {currentFile || "未选择投影文件"}
        </div>
      </div>

      {!currentFile ? (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', opacity: 0.5 }}>
          <p>请先选择一个 .litematic 文件。</p>
        </div>
      ) : (
        <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 12, paddingRight: 4 }}>
          {error && <pre style={{ color: '#ff6666', background: 'rgba(255,0,0,0.1)', padding: 8, border: '1px solid #ff6666' }}>{error}</pre>}
          
          <div className="group-box">
            <div className="group-box-title">文件</div>
            <div className="form-row" style={{ marginBottom: 0 }}>
              <div className="form-label" style={{ width: 100 }}>文件名称:</div>
              <div className="form-field">
                <input className="input" style={{ width: '100%' }} value={fileName} readOnly />
              </div>
            </div>
          </div>

          <div style={{ display: 'flex', gap: 12 }}>
            <div className="group-box" style={{ flex: 1, marginTop: 10 }}>
              <div className="group-box-title">SNBT 元数据</div>
              <div className="form-row">
                <div className="form-label" style={{ width: 100 }}>内部名称:</div>
                <div className="form-field"><input className="input" style={{ width: '100%' }} value={internalName} readOnly /></div>
              </div>
              <div className="form-row">
                <div className="form-label" style={{ width: 100 }}>作者:</div>
                <div className="form-field"><input className="input" style={{ width: '100%' }} value={author} readOnly /></div>
              </div>
              <div className="form-row">
                <div className="form-label" style={{ width: 100 }}>描述:</div>
                <div className="form-field"><input className="input" style={{ width: '100%' }} value={desc} readOnly /></div>
              </div>
              <div className="form-row">
                <div className="form-label" style={{ width: 100 }}>创建时间:</div>
                <div className="form-field"><input className="input" style={{ width: '100%' }} value={tCreated} readOnly /></div>
              </div>
              <div className="form-row">
                <div className="form-label" style={{ width: 100 }}>修改时间:</div>
                <div className="form-field"><input className="input" style={{ width: '100%' }} value={tModified} readOnly /></div>
              </div>
              
              <div className="form-row" style={{ alignItems: 'center' }}>
                <div className="form-label" style={{ width: 100 }}>尺寸:</div>
                <div className="form-field" style={{ gap: 12 }}>
                  <span style={{ color: 'var(--fg)', opacity: 0.8 }}>x</span><input className="input" style={{ width: 60 }} value={sizeX} readOnly />
                  <span style={{ color: 'var(--fg)', opacity: 0.8 }}>y</span><input className="input" style={{ width: 60 }} value={sizeY} readOnly />
                  <span style={{ color: 'var(--fg)', opacity: 0.8 }}>z</span><input className="input" style={{ width: 60 }} value={sizeZ} readOnly />
                </div>
              </div>

              <div className="form-row" style={{ alignItems: 'center' }}>
                <div className="form-label" style={{ width: 100 }}>体积:</div>
                <div className="form-field" style={{ gap: 12 }}>
                  <span style={{ color: 'var(--fg)', opacity: 0.8 }}>方块</span><input className="input" style={{ width: 80 }} value={totalBlocks} readOnly />
                  <span style={{ color: 'var(--fg)', opacity: 0.8 }}>总计</span><input className="input" style={{ width: 80 }} value={totalVolume} readOnly />
                  <span style={{ color: 'var(--fg)', opacity: 0.8 }}>密度</span><input className="input" style={{ width: 80 }} value={(density * 100).toFixed(2) + "%"} readOnly />
                </div>
              </div>
            </div>

            <div className="group-box" style={{ width: 340, marginTop: 10, display: 'flex', flexDirection: 'column' }}>
              <div className="group-box-title">3D 静态预览</div>
              <div ref={previewHostRef} style={{ flex: 1, backgroundColor: '#10151a', border: '1px solid #000', display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 0, minHeight: 220, overflow: 'hidden', position: 'relative' }}>
                {embeddedRunning ? (
                  <div style={{ position: 'absolute', left: 8, bottom: 8, background: 'rgba(0,0,0,0.65)', color: '#ccc', border: '1px solid #444', padding: '4px 8px', fontSize: '0.85em', zIndex: 1 }}>
                    嵌入式 Viewer 运行中
                  </div>
                ) : previewDataUrl ? (
                  <div style={{ width: '100%', height: '100%', position: 'relative' }}>
                    <img src={previewDataUrl} alt="static 3D preview" style={{ width: '100%', height: '100%', objectFit: 'cover', display: 'block', imageRendering: 'auto' }} />
                    <div style={{ position: 'absolute', left: 8, bottom: 8, background: 'rgba(0,0,0,0.65)', color: '#ccc', border: '1px solid #444', padding: '4px 8px', fontSize: '0.85em' }}>
                      静态预览；交互请打开弹窗 Viewer
                    </div>
                    {embeddedError && (
                      <button className="btn" style={{ position: 'absolute', right: 8, bottom: 8 }} onClick={handleOpenPopup}>
                        打开弹窗 Viewer
                      </button>
                    )}
                  </div>
                ) : cacheExists ? (
                  <div style={{ color: '#888', textAlign: 'center', fontSize: '0.9em', lineHeight: 1.5 }}>
                    3D cache ready.<br />
                    {cacheStage || 'Preview image has not been generated yet.'}
                    {embeddedError && <><br />嵌入失败：{embeddedError}</>}
                  </div>
                ) : (
                  <div style={{ color: '#777', textAlign: 'center', fontSize: '0.9em', lineHeight: 1.5 }}>
                    请点击渲染页构建 3D cache，<br />
                    构建完成后这里会显示<br />固定摄像机的静态 3D 预览。
                  </div>
                )}
              </div>
            </div>
          </div>

          <div className="group-box">
            <div className="group-box-title">版本信息</div>
            <div className="form-row">
              <div className="form-label" style={{ width: 140 }}>投影文件版本:</div>
              <div className="form-field"><input className="input" style={{ width: '100%' }} value={metadata.litematic_version || "-"} readOnly /></div>
            </div>
            <div className="form-row">
              <div className="form-label" style={{ width: 140 }}>Minecraft 数据版本:</div>
              <div className="form-field"><input className="input" style={{ width: '100%' }} value={metadata.minecraft_data_version || "-"} readOnly /></div>
            </div>
            <div style={{ paddingLeft: 140, marginTop: -4, color: '#777', fontSize: '0.85em' }}>
              只读：来自当前 .litematic 文件；这里不是版本转换入口。
            </div>
          </div>

          <div className="group-box">
            <div className="group-box-title">区域列表</div>
            <div style={{ backgroundColor: '#000', border: '1px solid #555', overflowX: 'auto', minHeight: 120 }}>
              <table style={{ width: '100%', borderCollapse: 'collapse', color: '#ccc', fontSize: '0.95em' }}>
                <thead>
                  <tr style={{ backgroundColor: '#222', borderBottom: '1px solid #555' }}>
                    <th style={{ padding: '6px 8px', textAlign: 'left', borderRight: '1px solid #555' }}>区域名称（双击修改）</th>
                    <th style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #555' }}>尺寸 x</th>
                    <th style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #555' }}>尺寸 y</th>
                    <th style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #555' }}>尺寸 z</th>
                    <th style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #555' }}>位置 x</th>
                    <th style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #555' }}>位置 y</th>
                    <th style={{ padding: '6px 8px', textAlign: 'right' }}>位置 z</th>
                  </tr>
                </thead>
                <tbody>
                  {regions.map((r, i) => (
                    <tr key={i} style={{ borderBottom: '1px solid #333' }}>
                      <td style={{ padding: '6px 8px', borderRight: '1px solid #444' }}>{r.name}</td>
                      <td style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #444' }}>{r.sx}</td>
                      <td style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #444' }}>{r.sy}</td>
                      <td style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #444' }}>{r.sz}</td>
                      <td style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #444' }}>{r.px}</td>
                      <td style={{ padding: '6px 8px', textAlign: 'right', borderRight: '1px solid #444' }}>{r.py}</td>
                      <td style={{ padding: '6px 8px', textAlign: 'right' }}>{r.pz}</td>
                    </tr>
                  ))}
                  {regions.length === 0 && (
                    <tr>
                      <td colSpan={7} style={{ padding: '16px', textAlign: 'center', color: '#555' }}>暂无区域数据</td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      )}

      {currentFile && (
        <div style={{ display: 'flex', gap: 12, justifyContent: 'flex-end', marginTop: 8 }}>
          <button className="btn" disabled>保存</button>
          <button className="btn" disabled>另存为</button>
          <button className="btn" disabled>恢复默认值</button>
          <button className="btn" disabled>转换格式</button>
        </div>
      )}
    </div>
  );
}
