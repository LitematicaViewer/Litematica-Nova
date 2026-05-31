import React, {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  buildLayerMetaCache,
  checkCacheExists,
  getBlockColor,
  getBlockIconDataUrl,
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

// 同级函数
import { fitView, FlakeBlockTooltip, resolveLayerBlockStates } from "./function";
import { CreativeInventoryDialog } from "./creativeInventoryDialog";

interface LayerCanvasHandle {
  resetView: () => void;
}

export interface FlakeHoverBlock {
  x: number;
  y: number;
  z: number;
  id: string;
  name: string;
  states: string;
}

const LayerCanvas = forwardRef<
  LayerCanvasHandle,
  {
    meta: LayerSliceMeta | null;
    sliceData: LayerSliceData | null;
    onHoverBlock: (block: FlakeHoverBlock | null, event: React.MouseEvent | null) => void;
  }
>(({ meta, sliceData, onHoverBlock }, ref) => {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const [iconImages, setIconImages] = useState<Map<number, string>>(new Map());

  const colorMap = useMemo(() => {
    const next = new Map<number, string>();
    if (!meta) return next;
    meta.palette.forEach((entry, index) => {
      let color = getBlockColor(entry.block_id);
      if (entry.block_id.includes("stone")) color = "#888";
      else if (entry.block_id.includes("dirt")) color = "#754";
      else if (entry.block_id.includes("grass")) color = "#583";
      else if (entry.block_id.includes("quartz")) color = "#eee";
      else if (entry.block_id.includes("glass")) color = "rgba(200,200,255,0.5)";
      else if (entry.block_id.includes("air")) color = "transparent";
      next.set(index, color);
    });
    return next;
  }, [meta]);

  const slicePaletteIds = useMemo(() => {
    if (!meta || !sliceData) return [] as number[];
    const seen = new Set<number>();
    const paletteIds: number[] = [];
    for (const block of sliceData.blocks) {
      const paletteId = block.palette_id;
      if (seen.has(paletteId)) continue;
      seen.add(paletteId);
      const entry = meta.palette[paletteId];
      if (!entry?.block_id || entry.block_id.includes("air")) continue;
      paletteIds.push(paletteId);
    }
    return paletteIds;
  }, [meta, sliceData]);

  const sliceBlockIndex = useMemo(() => {
    const index = new Map<number, LayerSliceData["blocks"][number]>();
    if (!meta || !sliceData) return index;
    for (const block of sliceData.blocks) {
      index.set(block.z * meta.size_x + block.x, block);
    }
    return index;
  }, [meta, sliceData]);

  const visibleBlocks = useMemo(() => {
    if (!meta || !sliceData) return [] as Array<{
      key: string;
      left: number;
      top: number;
      width: number;
      height: number;
      color: string;
      iconUrl: string;
    }>;

    const blocks: Array<{
      key: string;
      left: number;
      top: number;
      width: number;
      height: number;
      color: string;
      iconUrl: string;
    }> = [];

    for (const block of sliceData.blocks) {
      const color = colorMap.get(block.palette_id) || "#f0f";
      if (color === "transparent") continue;
      const left = Math.round(offset.x + block.x * scale);
      const top = Math.round(offset.y + block.z * scale);
      const width = Math.max(1, Math.ceil(scale));
      const height = Math.max(1, Math.ceil(scale));
      blocks.push({
        key: `${block.x}:${block.z}:${block.palette_id}`,
        left,
        top,
        width,
        height,
        color,
        iconUrl: iconImages.get(block.palette_id) || "",
      });
    }

    return blocks;
  }, [colorMap, iconImages, meta, offset.x, offset.y, scale, sliceData]);

  const resetView = () => {
    if (meta) fitView(meta, viewportRef.current, setScale, setOffset);
  };

  useImperativeHandle(ref, () => ({ resetView }), [meta]);

  useEffect(() => {
    if (meta) resetView();
  }, [meta]);

  useEffect(() => {
    let cancelled = false;
    setIconImages(new Map());

    if (!meta || slicePaletteIds.length === 0) {
      // console.log("[LBA_FLAKE] icon_batch:disabled", { has_meta: !!meta, slice_palette_ids: slicePaletteIds.length });
      return () => {
        cancelled = true;
      };
    }

    void (async () => {
      const next = new Map<number, string>();
      for (const paletteId of slicePaletteIds) {
        const entry = meta.palette[paletteId];
        if (!entry?.block_id) continue;
        const dataUrl = await getBlockIconDataUrl(entry.block_id, "layering");
        if (cancelled || !dataUrl) continue;
        next.set(paletteId, dataUrl);
      }
      if (!cancelled) {
        setIconImages(next);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [meta, slicePaletteIds]);

  const handleWheel = (event: React.WheelEvent) => {
    event.preventDefault();
    if (!viewportRef.current) return;
    const direction = event.deltaY < 0 ? 1 : -1;
    const nextScale = Math.max(0.1, Math.min(50, scale * Math.pow(1.1, direction)));
    const rect = viewportRef.current.getBoundingClientRect();
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

    if (!meta || !sliceData || !viewportRef.current) {
      onHoverBlock(null, event);
      return;
    }

    const rect = viewportRef.current.getBoundingClientRect();
    const bx = Math.floor((event.clientX - rect.left - offset.x) / scale);
    const bz = Math.floor((event.clientY - rect.top - offset.y) / scale);
    if (bx >= 0 && bx < meta.size_x && bz >= 0 && bz < meta.size_z) {
      const block = sliceBlockIndex.get(bz * meta.size_x + bx);
      if (block) {
        const paletteEntry = meta.palette[block.palette_id];
        onHoverBlock(
          {
            x: bx,
            y: sliceData.y,
            z: bz,
            id: paletteEntry.block_id,
            name: translateBlockId(paletteEntry.block_id),
            states: resolveLayerBlockStates(paletteEntry, meta.property_pool || []),
          },
          event,
        );
        return;
      }
    }

    onHoverBlock(null, event);
  };

  return (
    <div
      ref={viewportRef}
      className={isDragging ? "flake-canvas is-dragging" : "flake-canvas"}
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
    >
      <div
        className="flake-layer-border"
        style={{
          left: `${Math.round(offset.x)}px`,
          top: `${Math.round(offset.y)}px`,
          width: `${Math.max(1, Math.round(meta ? meta.size_x * scale : 0))}px`,
          height: `${Math.max(1, Math.round(meta ? meta.size_z * scale : 0))}px`,
        }}
      />
      {visibleBlocks.map((block) => (
        <div
          key={block.key}
          className="flake-layer-block"
          style={{
            left: `${block.left}px`,
            top: `${block.top}px`,
            width: `${block.width}px`,
            height: `${block.height}px`,
            background: block.iconUrl ? "transparent" : block.color,
          }}
        >
          {block.iconUrl ? <img className="flake-layer-block-icon" src={block.iconUrl} alt="" draggable={false} /> : null}
        </div>
      ))}
    </div>
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
  const [quickbarSlots, setQuickbarSlots] = useState<string[]>(() => Array.from({ length: 9 }, () => ""));
  const [toolPanelWidth, setToolPanelWidth] = useState(320);
  const [isQuickBuilding, setIsQuickBuilding] = useState(false);
  const [quickBuildStatus, setQuickBuildStatus] = useState("");
  const [quickBuildError, setQuickBuildError] = useState("");
  const pageRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<LayerCanvasHandle>(null);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const resizeStateRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const pollIntervalRef = useRef<number | null>(null);
  const syncRequestIdRef = useRef(0);
  const inFlightMetaKeyRef = useRef("");
  const loadedMetaKeyRef = useRef("");
  const inFlightSliceKeyRef = useRef("");
  const loadedSliceKeyRef = useRef("");

  const stopQuickBuildPolling = () => {
    if (pollIntervalRef.current !== null) {
      window.clearInterval(pollIntervalRef.current);
      pollIntervalRef.current = null;
    }
  };

  const syncCacheState = async () => {
    if (!currentFile) return;
    const requestId = ++syncRequestIdRef.current;
    // console.log("[LBA_FLAKE] syncCacheState:start", { currentFile });
    const stored = getLatestRenderCacheState(currentFile);
    /*
    console.log("[LBA_FLAKE] syncCacheState:stored", {
      currentFile,
      status: stored?.status || "idle",
      cacheFile: stored?.cacheFile || "",
      stage: stored?.stage || "",
    });
    */
    setCacheStatus(stored?.status || "idle");
    setCacheFile(stored?.cacheFile || "");
    setQuickBuildStatus(stored?.stage || "");
    setQuickBuildError(stored?.status === "error" ? stored.stage || "" : "");
    setIsQuickBuilding(stored?.status === "building");
    setMeta(null);
    setSliceData(null);
    setCacheExists(false);
    loadedMetaKeyRef.current = "";
    loadedSliceKeyRef.current = "";

    if (!stored?.cacheFile) {
      inFlightMetaKeyRef.current = "";
      inFlightSliceKeyRef.current = "";
      // console.log("[LBA_FLAKE] syncCacheState:no_cache", { currentFile });
      return;
    }
    const exists = await checkCacheExists(stored.cacheFile);
    if (requestId !== syncRequestIdRef.current) return;
    /*
    console.log("[LBA_FLAKE] syncCacheState:cache_exists", {
      currentFile,
      cacheFile: stored.cacheFile,
      exists,
      status: stored.status,
    });
    */
    setCacheExists(exists);
    if (exists && stored.status === "ready") {
      const metaKey = stored.cacheFile;
      if (inFlightMetaKeyRef.current === metaKey || loadedMetaKeyRef.current === metaKey) {
        return;
      }
      inFlightMetaKeyRef.current = metaKey;
      /*
      console.log("[LBA_FLAKE] syncCacheState:load_meta:start", {
        currentFile,
        cacheFile: stored.cacheFile,
      });
      */
      try {
        const loadedMeta = await loadLayerMeta(stored.cacheFile);
        if (requestId !== syncRequestIdRef.current) return;
        /*
        console.log("[LBA_FLAKE] syncCacheState:load_meta:end", {
          currentFile,
          cacheFile: stored.cacheFile,
          size_x: loadedMeta?.size_x || 0,
          size_y: loadedMeta?.size_y || 0,
          size_z: loadedMeta?.size_z || 0,
          palette_len: loadedMeta?.palette?.length || 0,
          property_pool_len: loadedMeta?.property_pool?.length || 0,
        });
        */
        loadedMetaKeyRef.current = metaKey;
        loadedSliceKeyRef.current = "";
        setMeta(loadedMeta);
        setLayerY(0);
      } finally {
        if (inFlightMetaKeyRef.current === metaKey) {
          inFlightMetaKeyRef.current = "";
        }
      }
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

  const ensureStatsData = async () => {
    if (!currentFile) return null;
    if (statsData) return statsData;
    const loaded = await loadStructureStats(currentFile);
    setStatsData(loaded);
    return loaded;
  };

  useEffect(() => {
    if (!currentFile) return;
    // console.log("[LBA_FLAKE] effect:currentFile", { currentFile });
    setStatsData(null);
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
    if (!cacheExists || !meta || !cacheFile) return;
    const sliceKey = `${cacheFile}::${layerY}`;
    if (inFlightSliceKeyRef.current === sliceKey || loadedSliceKeyRef.current === sliceKey) {
      return;
    }
    inFlightSliceKeyRef.current = sliceKey;
    /*
    console.log("[LBA_FLAKE] effect:loadLayerSlice:start", {
      cacheFile,
      layerY,
      size_x: meta.size_x,
      size_y: meta.size_y,
      size_z: meta.size_z,
    });
    */
    loadLayerSlice(cacheFile, layerY)
      .then((nextSlice) => {
        /*
        console.log("[LBA_FLAKE] effect:loadLayerSlice:end", {
          cacheFile,
          layerY,
          block_count: nextSlice?.blocks?.length || 0,
        });
        */
        loadedSliceKeyRef.current = sliceKey;
        setSliceData(nextSlice);
      })
      .catch((error) => {
        /*
        console.log("[LBA_FLAKE] effect:loadLayerSlice:error", {
          cacheFile,
          layerY,
          error: String(error),
        });
        */
        if (loadedSliceKeyRef.current === sliceKey) {
          loadedSliceKeyRef.current = "";
        }
        setSliceData(null);
      })
      .finally(() => {
        if (inFlightSliceKeyRef.current === sliceKey) {
          inFlightSliceKeyRef.current = "";
        }
      });
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

  const regionName = statsData?.regions?.[0]?.name || currentFile.split(/[\\/]/).pop() || "Unnamed";
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
        <button className="btn flake-page__materials-button" onClick={async () => {
          try {
            const loaded = await ensureStatsData();
            if (!loaded) return;
            openMaterialsWithWindowBehavior(currentFile, () => setShowMaterials(true));
          } catch {
            setStatsData(null);
          }
        }}>
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
            {quickbarSlots.map((blockId, index) => {
              const isSelected = selectedQuickbarSlot === index;
              return (
                <button
                  key={`slot-${index + 1}`}
                  className={isSelected ? "flake-page__quickbar-slot is-selected" : "flake-page__quickbar-slot"}
                  type="button"
                  aria-label={blockId ? `${translateBlockId(blockId)}（快捷栏槽位 ${index + 1}）` : `快捷栏槽位 ${index + 1}`}
                  aria-pressed={isSelected}
                  onClick={() => setSelectedQuickbarSlot(index)}
                >
                  {blockId ? <BlockIcon blockId={blockId} /> : <span className="flake-page__quickbar-slot-icon" aria-hidden />}
                  <span className="flake-page__quickbar-slot-label">{blockId || "空槽"}</span>
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
        <CreativeInventoryDialog
          onClose={() => setShowInventoryDialog(false)}
          quickbarSlots={quickbarSlots}
          onChangeQuickbarSlots={(updater) => setQuickbarSlots((current) => updater([...current]))}
        />
      ) : null}

      {showMaterials && statsData && <MaterialsDialog data={statsData} onClose={() => setShowMaterials(false)} currentFile={currentFile} />}
    </div>
  );
}


