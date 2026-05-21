import React, {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  buildLayerMetaCache,
  checkCacheExists,
  getBlockColor,
  getLatestRenderCacheState,
  LayerSliceData,
  LayerSliceMeta,
  loadLayerMeta,
  loadLayerSlice,
  loadStructureStats,
  pollCacheBuildTask,
  startCacheBuildTask,
  stateFromLaunch,
  StatsData,
  subscribeRenderCacheStore,
  translateBlockId,
  updateRenderCacheState,
  upsertRenderCacheState,
} from "../../../../../src/business/facade";
import { BlockIcon } from "../../../../components/BlockIcon";
import { MaterialsDialog, openMaterialsWithWindowBehavior } from "../statistics/StatisticsPage";
import { getBlockIconDataUrl } from "../../../../../src/business/facade";

interface LayerCanvasHandle {
  resetView: () => void;
}

interface FlakeHoverBlock {
  x: number;
  y: number;
  z: number;
  id: string;
  name: string;
}

function FlakeBlockTooltip({ x, y, item }: { x: number; y: number; item: FlakeHoverBlock | null }) {
  const popupRef = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState(() => ({ left: x + 14, top: y + 14 }));

  useLayoutEffect(() => {
    if (!item) return;
    const popup = popupRef.current;
    const offset = 14;
    const margin = 4;
    if (!popup) {
      setPosition({ left: x + offset, top: y + offset });
      return;
    }

    const rect = popup.getBoundingClientRect();
    let left = x + offset;
    let top = y + offset;

    if (left + rect.width > window.innerWidth - margin) {
      left = x - rect.width - offset;
    }
    if (top + rect.height > window.innerHeight - margin) {
      top = y - rect.height - offset;
    }

    left = Math.max(margin, Math.min(left, window.innerWidth - rect.width - margin));
    top = Math.max(margin, Math.min(top, window.innerHeight - rect.height - margin));
    setPosition((previous) => previous.left === left && previous.top === top ? previous : { left, top });
  }, [x, y, item]);

  useLayoutEffect(() => {
    const popup = popupRef.current;
    if (!popup) return;
    popup.style.setProperty("--material-list-popup-left", `${position.left}px`);
    popup.style.setProperty("--material-list-popup-top", `${position.top}px`);
  }, [position.left, position.top]);

  if (!item) return null;

  return (
    <div ref={popupRef} className="material-list-hover-popup" role="tooltip">
      <div className="material-list-hover-popup-row material-list-hover-popup-item-row">
        <BlockIcon blockId={item.id} />
        <span className="material-list-hover-popup-name">{item.name}</span>
      </div>
      <div className="material-list-hover-popup-row">方块ID：{item.id}</div>
      <div className="material-list-hover-popup-row">x={item.x} y={item.y} z={item.z}</div>
    </div>
  );
}

function fitView(
  meta: LayerSliceMeta,
  canvas: HTMLCanvasElement | null,
  setScale: (scale: number) => void,
  setOffset: (offset: { x: number; y: number }) => void,
) {
  const width = canvas?.parentElement?.clientWidth || 600;
  const height = canvas?.parentElement?.clientHeight || 600;
  const maxDim = Math.max(meta.size_x, meta.size_z);
  if (maxDim <= 0) return;
  const initialScale = Math.min(10, Math.max(0.5, (Math.min(width, height) * 0.8) / maxDim));
  setScale(initialScale);
  setOffset({
    x: width / 2 - (meta.size_x * initialScale) / 2,
    y: height / 2 - (meta.size_z * initialScale) / 2,
  });
}

const LayerCanvas = forwardRef<
  LayerCanvasHandle,
  {
    meta: LayerSliceMeta | null;
    sliceData: LayerSliceData | null;
    onHoverBlock: (block: FlakeHoverBlock | null, event: React.MouseEvent | null) => void;
  }
>(({ meta, sliceData, onHoverBlock }, ref) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const [iconImages, setIconImages] = useState<Map<number, HTMLImageElement>>(new Map());

  const resetView = () => {
    if (meta) fitView(meta, canvasRef.current, setScale, setOffset);
  };

  useImperativeHandle(ref, () => ({ resetView }), [meta]);

  useEffect(() => {
    if (meta) resetView();
  }, [meta]);

  useEffect(() => {
    drawCanvas();
  }, [meta, sliceData, scale, offset, iconImages]);

  useEffect(() => {
    let active = true;
    const loadPaletteIcons = async () => {
      const next = new Map<number, HTMLImageElement>();
      if (!meta) {
        setIconImages(next);
        return;
      }
      await Promise.all(
        meta.palette.map(async (entry, index) => {
          if (!entry?.block_id || entry.block_id.includes("air")) return;
          const dataUrl = await getBlockIconDataUrl(entry.block_id, "layering");
          if (!dataUrl) return;
          await new Promise<void>((resolve) => {
            const img = new Image();
            img.onload = () => {
              next.set(index, img);
              resolve();
            };
            img.onerror = () => resolve();
            img.src = dataUrl;
          });
        }),
      );
      if (active) setIconImages(next);
    };
    loadPaletteIcons();
    return () => {
      active = false;
    };
  }, [meta]);

  const drawCanvas = () => {
    const canvas = canvasRef.current;
    if (!canvas || !meta || !sliceData) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const parent = canvas.parentElement;
    const viewportWidth = parent?.clientWidth || canvas.clientWidth || 0;
    const viewportHeight = parent?.clientHeight || canvas.clientHeight || 0;
    const dpr = window.devicePixelRatio || 1;
    const physicalWidth = Math.max(1, Math.round(viewportWidth * dpr));
    const physicalHeight = Math.max(1, Math.round(viewportHeight * dpr));

    if (canvas.width !== physicalWidth || canvas.height !== physicalHeight) {
      canvas.width = physicalWidth;
      canvas.height = physicalHeight;
      canvas.style.width = `${viewportWidth}px`;
      canvas.style.height = `${viewportHeight}px`;
    }

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, viewportWidth, viewportHeight);
    ctx.strokeStyle = "#555";
    ctx.lineWidth = 1;
    ctx.strokeRect(offset.x, offset.y, meta.size_x * scale, meta.size_z * scale);

    const colorMap = new Map<number, string>();
    meta.palette.forEach((p, i) => {
      let color = getBlockColor(p.block_id);
      if (p.block_id.includes("stone")) color = "#888";
      else if (p.block_id.includes("dirt")) color = "#754";
      else if (p.block_id.includes("grass")) color = "#583";
      else if (p.block_id.includes("quartz")) color = "#eee";
      else if (p.block_id.includes("glass")) color = "rgba(200,200,255,0.5)";
      else if (p.block_id.includes("air")) color = "transparent";
      colorMap.set(i, color);
    });

    for (const block of sliceData.blocks) {
      const px = offset.x + block.x * scale;
      const pz = offset.y + block.z * scale;
      if (px + scale < 0 || pz + scale < 0 || px > viewportWidth || pz > viewportHeight) continue;

      const color = colorMap.get(block.palette_id) || "#f0f";
      if (color === "transparent") continue;
      const icon = iconImages.get(block.palette_id);
      if (icon) {
        ctx.drawImage(icon, px, pz, Math.ceil(scale), Math.ceil(scale));
      } else {
        ctx.fillStyle = color;
        ctx.fillRect(px, pz, Math.ceil(scale), Math.ceil(scale));
      }
      if (scale > 8) {
        ctx.strokeStyle = "rgba(0,0,0,0.3)";
        ctx.strokeRect(px, pz, Math.ceil(scale), Math.ceil(scale));
      }
    }
  };

  const handleWheel = (event: React.WheelEvent) => {
    event.preventDefault();
    if (!canvasRef.current) return;
    const direction = event.deltaY < 0 ? 1 : -1;
    const nextScale = Math.max(0.1, Math.min(50, scale * Math.pow(1.1, direction)));
    const rect = canvasRef.current.getBoundingClientRect();
    const mouseX = event.clientX - rect.left;
    const mouseY = event.clientY - rect.top;
    setOffset({
      x: mouseX - (mouseX - offset.x) * (nextScale / scale),
      y: mouseY - (mouseY - offset.y) * (nextScale / scale),
    });
    setScale(nextScale);
  };

  const handleMouseMove = (event: React.MouseEvent) => {
    if (isDragging) {
      setOffset({ x: event.clientX - dragStart.x, y: event.clientY - dragStart.y });
      return;
    }

    if (!meta || !sliceData || !canvasRef.current) {
      onHoverBlock(null, event);
      return;
    }
    const rect = canvasRef.current.getBoundingClientRect();
    const bx = Math.floor((event.clientX - rect.left - offset.x) / scale);
    const bz = Math.floor((event.clientY - rect.top - offset.y) / scale);
    if (bx >= 0 && bx < meta.size_x && bz >= 0 && bz < meta.size_z) {
      const block = sliceData.blocks.find((candidate) => candidate.x === bx && candidate.z === bz);
      if (block) {
        const paletteEntry = meta.palette[block.palette_id];
        onHoverBlock(
          { x: bx, y: sliceData.y, z: bz, id: paletteEntry.block_id, name: translateBlockId(paletteEntry.block_id) },
          event,
        );
        return;
      }
    }
    onHoverBlock(null, event);
  };

  useEffect(() => {
    const handleResize = () => drawCanvas();
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, [meta, sliceData, scale, offset]);

  return (
    <canvas
      ref={canvasRef}
      className={isDragging ? "flake-canvas is-dragging" : "flake-canvas"}
      style={{ imageRendering: "pixelated" }}
      onWheel={handleWheel}
      onMouseDown={(event) => {
        setIsDragging(true);
        setDragStart({ x: event.clientX - offset.x, y: event.clientY - offset.y });
      }}
      onMouseMove={handleMouseMove}
      onMouseUp={() => setIsDragging(false)}
      onMouseLeave={() => {
        setIsDragging(false);
        onHoverBlock(null, null);
      }}
    />
  );
});

export function FlakePage({ currentFile, setRoute }: any) {
  const [cacheFile, setCacheFile] = useState("");
  const [cacheStatus, setCacheStatus] = useState("idle");
  const [cacheExists, setCacheExists] = useState(false);
  const [meta, setMeta] = useState<LayerSliceMeta | null>(null);
  const [sliceData, setSliceData] = useState<LayerSliceData | null>(null);
  const [statsData, setStatsData] = useState<StatsData | null>(null);
  const [layerY, setLayerY] = useState(0);
  const [hoverBlock, setHoverBlock] = useState<FlakeHoverBlock | null>(null);
  const [hoverPos, setHoverPos] = useState({ x: 0, y: 0 });
  const [showMaterials, setShowMaterials] = useState(false);
  const [editMode, setEditMode] = useState(false);
  const [showInventoryDialog, setShowInventoryDialog] = useState(false);
  const [selectedQuickbarSlot, setSelectedQuickbarSlot] = useState(0);
  const [toolPanelWidth, setToolPanelWidth] = useState(320);
  const [isQuickBuilding, setIsQuickBuilding] = useState(false);
  const [quickBuildStatus, setQuickBuildStatus] = useState("");
  const [quickBuildError, setQuickBuildError] = useState("");
  const pageRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<LayerCanvasHandle>(null);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const resizeStateRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const pollIntervalRef = useRef<number | null>(null);

  const stopQuickBuildPolling = () => {
    if (pollIntervalRef.current !== null) {
      window.clearInterval(pollIntervalRef.current);
      pollIntervalRef.current = null;
    }
  };

  const syncCacheState = async () => {
    if (!currentFile) return;
    const stored = getLatestRenderCacheState(currentFile);
    setCacheStatus(stored?.status || "idle");
    setCacheFile(stored?.cacheFile || "");
    setQuickBuildStatus(stored?.stage || "");
    setQuickBuildError(stored?.status === "error" ? stored.stage || "" : "");
    setIsQuickBuilding(stored?.status === "building");
    setMeta(null);
    setSliceData(null);
    setCacheExists(false);

    if (!stored?.cacheFile) return;
    const exists = await checkCacheExists(stored.cacheFile);
    setCacheExists(exists);
    if (exists && stored.status === "ready") {
      const loadedMeta = await loadLayerMeta(stored.cacheFile);
      setMeta(loadedMeta);
      setLayerY(0);
    }
  };

  const handleQuickBuild = async () => {
    if (!currentFile || isQuickBuilding) return;
    stopQuickBuildPolling();
    setQuickBuildError("");
    setQuickBuildStatus("已启动标准模式 3D cache 构建。");
    setIsQuickBuilding(true);
    setCacheExists(false);
    setMeta(null);
    setSliceData(null);

    try {
      const launch = await startCacheBuildTask(currentFile, "normal");
      setCacheFile(launch.cache_file);
      upsertRenderCacheState(stateFromLaunch(currentFile, "normal", launch));

      pollIntervalRef.current = window.setInterval(async () => {
        const snapshot = await pollCacheBuildTask();
        const stage = snapshot.progress_json ? "标准模式 3D cache 构建中。" : "构建进程已启动，等待进度输出。";
        setQuickBuildStatus(stage);
        updateRenderCacheState(currentFile, "normal", {
          stage,
          status: snapshot.running ? "building" : "error",
          cacheFile: snapshot.cache_file || launch.cache_file,
        });

        if (snapshot.running) return;

        stopQuickBuildPolling();
        setIsQuickBuilding(false);
        const readyCache = snapshot.cache_file || launch.cache_file;
        if (readyCache && snapshot.exit_code === 0) {
          try {
            await buildLayerMetaCache(readyCache);
            updateRenderCacheState(currentFile, "normal", {
              status: "ready",
              cacheFile: readyCache,
              stage: "标准模式 3D cache 已完成，分层索引可用。",
            });
            setQuickBuildStatus("标准模式 3D cache 已完成，分层索引可用。");
            await syncCacheState();
          } catch (error: any) {
            const message = `3D cache 已完成，但分层索引初始化失败：${error}`;
            setQuickBuildError(message);
            setQuickBuildStatus(message);
            updateRenderCacheState(currentFile, "normal", {
              status: "error",
              cacheFile: readyCache,
              stage: message,
            });
          }
          return;
        }

        const detail = snapshot.stderr_tail || snapshot.stdout_tail || `exit_code=${snapshot.exit_code}`;
        const message = `标准模式 3D cache 构建失败。\n${detail}`;
        setQuickBuildError(message);
        setQuickBuildStatus(message);
        updateRenderCacheState(currentFile, "normal", { status: "error", stage: message });
      }, 500);
    } catch (error: any) {
      const message = `标准模式 3D cache 启动失败：${error}`;
      setIsQuickBuilding(false);
      setQuickBuildError(message);
      setQuickBuildStatus(message);
    }
  };

  const handleToolSplitterPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    resizeStateRef.current = { startX: event.clientX, startWidth: toolPanelWidth };
    window.addEventListener("pointermove", handleToolSplitterPointerMove);
    window.addEventListener("pointerup", handleToolSplitterPointerUp);
  };

  const handleToolSplitterPointerMove = (event: PointerEvent) => {
    const resizeState = resizeStateRef.current;
    if (!resizeState) return;
    const nextWidth = Math.max(240, Math.min(460, resizeState.startWidth - (event.clientX - resizeState.startX)));
    setToolPanelWidth(nextWidth);
  };

  const handleToolSplitterPointerUp = () => {
    resizeStateRef.current = null;
    window.removeEventListener("pointermove", handleToolSplitterPointerMove);
    window.removeEventListener("pointerup", handleToolSplitterPointerUp);
  };

  useEffect(() => {
    if (!currentFile) return;
    loadStructureStats(currentFile).then(setStatsData).catch(() => setStatsData(null));
    syncCacheState();
  }, [currentFile]);

  useEffect(() => subscribeRenderCacheStore(syncCacheState), [currentFile]);

  useEffect(() => () => {
    stopQuickBuildPolling();
    handleToolSplitterPointerUp();
  }, []);

  useEffect(() => {
    if (!editMode) {
      setShowInventoryDialog(false);
    }
  }, [editMode]);

  useEffect(() => {
    if (cacheExists && meta && cacheFile) {
      loadLayerSlice(cacheFile, layerY).then(setSliceData);
    }
  }, [layerY, cacheExists, meta, cacheFile]);

  const handleHoverBlock = (block: FlakeHoverBlock | null, event: React.MouseEvent | null) => {
    setHoverBlock(block);
    if (event) setHoverPos({ x: event.clientX, y: event.clientY });
  };

  useLayoutEffect(() => {
    const page = pageRef.current;
    if (!page) return;
    page.style.setProperty("--flake-tool-panel-width", `${toolPanelWidth}px`);
  }, [toolPanelWidth]);

  const regionName = statsData?.regions?.[0]?.name || "Unnamed";
  const maxY = meta ? Math.max(0, meta.size_y - 1) : 0;
  const ready = cacheExists && cacheStatus === "ready" && !!meta;
  const building = cacheStatus === "building" || isQuickBuilding;
  const statusText = quickBuildError || quickBuildStatus || (!ready
    ? (building ? "Cache is building; layer data will become available when the ready file is written." : "Layers unavailable: build 3D cache first.")
    : `Y=${layerY}; ${sliceData?.blocks?.length || 0} non-air blocks. Wheel zooms, drag pans.`);

  if (!currentFile) {
    return (
      <div className="nova-empty-state flake-page__empty-state">
        <h2 className="flake-page__empty-title">Layers</h2>
        <p>Select a .litematic file first.</p>
      </div>
    );
  }

  return (
    <div ref={pageRef} className={editMode ? "nova-page flake-page flake-page--edit-mode" : "nova-page flake-page"}>
      <div className="subwindow-toolbar flake-page__topbar">
        <button className="btn flake-page__materials-button" onClick={() => openMaterialsWithWindowBehavior(currentFile, () => setShowMaterials(true))} disabled={!statsData}>
          材料列表
        </button>
        <button className="btn flake-page__build-button" onClick={handleQuickBuild} disabled={!currentFile || isQuickBuilding}>
          {isQuickBuilding ? "生成中..." : "生成3DCache"}
        </button>
        <label className="subwindow-check-row flake-page__edit-toggle">
          <input type="checkbox" checked={editMode} onChange={(event) => setEditMode(event.target.checked)} />
          编辑模式
        </label>
        <div className="subwindow-toolbar-spacer" />
        <div className="flake-page__file-path" title={currentFile}>{currentFile}</div>
      </div>

      <div className="group-box flake-page__control-box">
        <div className="group-box-title">层级控制</div>
        <div className="flake-page__layer-row">
          <span className="nova-muted flake-page__layer-label">层级</span>
          <input className="flake-page__layer-range" type="range" min={0} max={maxY} value={layerY} onChange={(event) => setLayerY(parseInt(event.target.value, 10))} disabled={!ready} />
        </div>

        <div className="flake-page__layer-footer">
          <div className="flake-page__layer-value">Y = {layerY}</div>
          <div className="flake-page__layer-actions">
            <button className="btn flake-page__step-button" disabled={!ready} onClick={() => setLayerY(Math.max(0, layerY - 5))}>-5</button>
            <button className="btn flake-page__step-button" disabled={!ready} onClick={() => setLayerY(Math.max(0, layerY - 1))}>-</button>
            <button className="btn flake-page__step-button" disabled={!ready} onClick={() => setLayerY(Math.min(maxY, layerY + 1))}>+</button>
            <button className="btn flake-page__step-button" disabled={!ready} onClick={() => setLayerY(Math.min(maxY, layerY + 5))}>+5</button>
            <button className="btn" disabled={!ready} onClick={() => canvasRef.current?.resetView()}>重置视图</button>
          </div>
        </div>
      </div>

      <div className="flake-page__workspace">
        <section className="flake-page__main-panel">
          <div ref={viewportRef} className="flake-page__viewport">
            {!ready ? (
              <div className="nova-empty-state flake-page__viewport-empty">
                <div className="flake-page__viewport-title">{building ? "3D cache is building." : "Layer view needs a finished 3D cache."}</div>
                <div>{building ? "标准模式 3D cache 正在构建。" : "请先生成标准模式 3D cache。"}</div>
              </div>
            ) : (
              <LayerCanvas ref={canvasRef} meta={meta} sliceData={sliceData} onHoverBlock={handleHoverBlock} />
            )}

            <FlakeBlockTooltip x={hoverPos.x} y={hoverPos.y} item={hoverBlock} />
          </div>
        </section>

        {editMode ? (
          <>
            <div
              className="flake-page__splitter"
              role="separator"
              aria-orientation="vertical"
              aria-label="调整编辑工具宽度"
              onPointerDown={handleToolSplitterPointerDown}
            />

            <aside className="group-box flake-page__tool-panel">
              <div className="group-box-title">编辑工具</div>
              <div className="flake-page__tool-panel-body">
                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">当前上下文</div>
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">区域</span>
                    <span>{regionName}</span>
                  </div>
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">层级</span>
                    <span>Y = {layerY}</span>
                  </div>
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">缓存</span>
                    <span>{ready ? "已就绪" : building ? "构建中" : "未就绪"}</span>
                  </div>
                </div>

                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">状态</div>
                  <div className="nova-muted nova-small flake-page__status-text">{statusText}</div>
                  {quickBuildError ? <p className="nova-error flake-page__tool-panel-error">{quickBuildError}</p> : null}
                </div>

                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">编辑工具占位</div>
                  <div className="nova-muted nova-small flake-page__tool-panel-placeholder">
                    编辑逻辑尚未实现。当前先保留右侧工具区、可拖拽宽度、底部快捷栏与物品栏入口，用于验证整体交互布局。
                  </div>
                </div>

                <div className="flake-page__tool-panel-actions">
                  <button className="btn" type="button" onClick={() => setShowInventoryDialog(true)} disabled={!editMode}>
                    打开创造模式物品栏
                  </button>
                  <button className="btn" type="button" onClick={() => setRoute?.("render")}>
                    打开渲染页
                  </button>
                </div>
              </div>
            </aside>
          </>
        ) : null}
      </div>

      {editMode ? (
        <div className="flake-page__quickbar" aria-label="编辑快捷栏">
          <div className="flake-page__quickbar-body" role="toolbar" aria-label="快捷栏本体">
            {Array.from({ length: 9 }, (_, index) => {
              const isSelected = selectedQuickbarSlot === index;
              return (
                <button
                  key={`slot-${index + 1}`}
                  className={isSelected ? "flake-page__quickbar-slot is-selected" : "flake-page__quickbar-slot"}
                  type="button"
                  aria-label={`快捷栏槽位 ${index + 1}`}
                  aria-pressed={isSelected}
                  onClick={() => setSelectedQuickbarSlot(index)}
                >
                  <span className="flake-page__quickbar-slot-icon" aria-hidden />
                  <span className="flake-page__quickbar-slot-label">空槽</span>
                </button>
              );
            })}
          </div>
          <button className="btn flake-page__quickbar-open-inventory" type="button" onClick={() => setShowInventoryDialog(true)}>
            <span className="flake-page__quickbar-open-inventory-text">打开物品栏</span>
          </button>
        </div>
      ) : null}

      {showInventoryDialog ? (
        <div className="dialog-overlay" onClick={() => setShowInventoryDialog(false)}>
          <section className="dialog-content flake-page__inventory-dialog" onClick={(event) => event.stopPropagation()}>
            <div className="subwindow-title-row">
              <div>
                <h3 className="subwindow-title">创造模式物品栏</h3>
                <p className="subwindow-subtitle nova-muted">当前仅提供对话框占位，用于验证分层编辑模式的底部快捷栏与入口行为。</p>
              </div>
              <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={() => setShowInventoryDialog(false)}>×</button>
            </div>
            <div className="flake-page__inventory-dialog-body">
              <div className="flake-page__inventory-grid">
                {Array.from({ length: 27 }, (_, index) => (
                  <button key={`inventory-${index + 1}`} className="btn flake-page__inventory-slot" type="button">
                    槽位 {index + 1}
                  </button>
                ))}
              </div>
            </div>
          </section>
        </div>
      ) : null}

      {showMaterials && statsData && <MaterialsDialog data={statsData} onClose={() => setShowMaterials(false)} currentFile={currentFile} />}
    </div>
  );
}


