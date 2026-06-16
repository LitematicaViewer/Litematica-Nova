import React, {
  forwardRef,
  useCallback,
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
import { resolveFlakeLayerBlockImage } from "../../../../../src/services/flakeStateHintResolver";
import { BlockIcon } from "../../../../components/BlockIcon";
import { MaterialsDialog, openMaterialsWithWindowBehavior } from "../statistics/StatisticsPage";

// 同级函数
import { fitView, FlakeBlockTooltip, resolveLayerBlockStates } from "./function";
import { CreativeInventoryDialog } from "./creativeInventoryDialog";
import { ContainerDialog } from "./containerDialog";
import { loadContainerData, isContainerBlock, getContainerType, type ContainerItem } from "../../../../../src/business/facade";

// 全局图标缓存，避免重复处理相同方块
const globalIconCache = new Map<string, Promise<string | null>>();

function getCachedIconImage(
  blockId: string,
  paletteEntry: any,
  propertyPool: any[],
  enabled: boolean
): Promise<string | null> {
  const cacheKey = `${blockId}::${JSON.stringify(paletteEntry)}::${enabled}`;
  
  if (!globalIconCache.has(cacheKey)) {
    globalIconCache.set(
      cacheKey,
      resolveFlakeLayerBlockImage({
        blockId,
        paletteEntry,
        propertyPool,
        enabled,
      })
    );
  }
  
  return globalIconCache.get(cacheKey)!;
}

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

interface FlakeRenderSlice {
  depth: number;
  opacity: number;
  sliceData: LayerSliceData;
}

interface VisibleLayerBlock {
  block: LayerSliceData["blocks"][number];
  y: number;
  opacity: number;
}

const LayerCanvas = forwardRef<
  LayerCanvasHandle,
  {
    meta: LayerSliceMeta | null;
    renderSlices: FlakeRenderSlice[];
    showStateHints: boolean;
    onHoverBlock: (block: FlakeHoverBlock | null, event: React.MouseEvent | null) => void;
    onBlockRightClick: (block: FlakeHoverBlock, event: React.MouseEvent) => void;
  }
>(({ meta, renderSlices, showStateHints, onHoverBlock, onBlockRightClick }, ref) => {
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
    if (!meta || renderSlices.length === 0) return [] as number[];
    const seen = new Set<number>();
    const paletteIds: number[] = [];
    for (const renderSlice of renderSlices) {
      for (const block of renderSlice.sliceData.blocks) {
        const paletteId = block.palette_id;
        if (seen.has(paletteId)) continue;
        seen.add(paletteId);
        const entry = meta.palette[paletteId];
        if (!entry?.block_id || entry.block_id.includes("air")) continue;
        paletteIds.push(paletteId);
      }
    }
    return paletteIds;
  }, [meta, renderSlices]);

  const visibleBlockIndex = useMemo(() => {
    const index = new Map<number, VisibleLayerBlock>();
    if (!meta || renderSlices.length === 0) return index;
    for (const renderSlice of renderSlices) {
      for (const block of renderSlice.sliceData.blocks) {
        const positionKey = block.z * meta.size_x + block.x;
        if (index.has(positionKey)) continue;
        index.set(positionKey, {
          block,
          y: renderSlice.sliceData.y,
          opacity: renderSlice.opacity,
        });
      }
    }
    return index;
  }, [meta, renderSlices]);

  const visibleBlocks = useMemo(() => {
    if (!meta || visibleBlockIndex.size === 0) return [] as Array<{
      key: string;
      left: number;
      top: number;
      width: number;
      height: number;
      color: string;
      iconUrl: string;
      opacity: number;
    }>;

    const blocks: Array<{
      key: string;
      left: number;
      top: number;
      width: number;
      height: number;
      color: string;
      iconUrl: string;
      opacity: number;
    }> = [];

    for (const visibleBlock of visibleBlockIndex.values()) {
      const color = colorMap.get(visibleBlock.block.palette_id) || "#f0f";
      if (color === "transparent") continue;
      const left = Math.round(offset.x + visibleBlock.block.x * scale);
      const top = Math.round(offset.y + visibleBlock.block.z * scale);
      const width = Math.max(1, Math.ceil(scale));
      const height = Math.max(1, Math.ceil(scale));
      blocks.push({
        key: `${visibleBlock.block.x}:${visibleBlock.block.z}:${visibleBlock.y}:${visibleBlock.block.palette_id}`,
        left,
        top,
        width,
        height,
        color,
        iconUrl: iconImages.get(visibleBlock.block.palette_id) || "",
        opacity: visibleBlock.opacity,
      });
    }

    return blocks;
  }, [colorMap, iconImages, meta, offset.x, offset.y, scale, visibleBlockIndex]);

  const resetView = () => {
    if (meta) fitView(meta, viewportRef.current, setScale, setOffset);
  };

  useImperativeHandle(ref, () => ({ resetView }), [meta]);

  useEffect(() => {
    if (meta) resetView();
  }, [meta]);

  useEffect(() => {
    let cancelled = false;
    
    if (!meta || slicePaletteIds.length === 0) {
      setIconImages(new Map());
      return () => {
        cancelled = true;
      };
    }

    void (async () => {
      // 并行处理所有图标，避免串行阻塞
      const tasks = slicePaletteIds.map(async (paletteId) => {
        const entry = meta.palette[paletteId];
        if (!entry?.block_id) return null;
        
        try {
          // 使用缓存，避免重复处理
          const dataUrl = await getCachedIconImage(
            entry.block_id,
            entry,
            meta.property_pool || [],
            showStateHints
          );
          return { paletteId, dataUrl };
        } catch (error) {
          console.error(`Failed to resolve icon for palette ${paletteId}:`, error);
          return null;
        }
      });

      // 等待所有任务完成（并行执行）
      const results = await Promise.all(tasks);
      
      if (cancelled) return;

      // 批量更新状态
      const next = new Map<number, string>();
      for (const result of results) {
        if (result && result.dataUrl) {
          next.set(result.paletteId, result.dataUrl);
        }
      }
      
      setIconImages(next);
    })();

    return () => {
      cancelled = true;
    };
  }, [meta, slicePaletteIds, showStateHints]);

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

    if (!meta || visibleBlockIndex.size === 0 || !viewportRef.current) {
      onHoverBlock(null, event);
      return;
    }

    const rect = viewportRef.current.getBoundingClientRect();
    const bx = Math.floor((event.clientX - rect.left - offset.x) / scale);
    const bz = Math.floor((event.clientY - rect.top - offset.y) / scale);
    if (bx >= 0 && bx < meta.size_x && bz >= 0 && bz < meta.size_z) {
      const visibleBlock = visibleBlockIndex.get(bz * meta.size_x + bx);
      if (visibleBlock) {
        const paletteEntry = meta.palette[visibleBlock.block.palette_id];
        onHoverBlock(
          {
            x: bx,
            y: visibleBlock.y,
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

  const handleContextMenu = (event: React.MouseEvent) => {
    console.log("handleContextMenu 被调用");
    event.preventDefault();
    
    if (!meta || visibleBlockIndex.size === 0 || !viewportRef.current) {
      console.log("无法处理右键: meta=", !!meta, "visibleBlockIndex.size=", visibleBlockIndex.size, "viewportRef=", !!viewportRef.current);
      return;
    }

    const rect = viewportRef.current.getBoundingClientRect();
    const bx = Math.floor((event.clientX - rect.left - offset.x) / scale);
    const bz = Math.floor((event.clientY - rect.top - offset.y) / scale);
    console.log("计算的方块坐标:", bx, bz, "范围:", meta.size_x, meta.size_z);
    
    if (bx >= 0 && bx < meta.size_x && bz >= 0 && bz < meta.size_z) {
      const visibleBlock = visibleBlockIndex.get(bz * meta.size_x + bx);
      console.log("找到的 visibleBlock:", visibleBlock);
      
      if (visibleBlock) {
        const paletteEntry = meta.palette[visibleBlock.block.palette_id];
        const block: FlakeHoverBlock = {
          x: bx,
          y: visibleBlock.y,
          z: bz,
          id: paletteEntry.block_id,
          name: translateBlockId(paletteEntry.block_id),
          states: resolveLayerBlockStates(paletteEntry, meta.property_pool || []),
        };
        console.log("准备调用 onBlockRightClick, block=", block);
        onBlockRightClick(block, event);
      } else {
        console.log("该位置没有方块");
      }
    } else {
      console.log("坐标超出范围");
    }
  };

  return (
    <div
      ref={viewportRef}
      className={isDragging ? "flake-canvas is-dragging" : "flake-canvas"}
      onWheel={handleWheel}
      onMouseDown={(event) => {
        if (event.button === 0) { // 只响应左键拖拽
          setIsDragging(true);
          setDragStart({ x: event.clientX - offset.x, y: event.clientY - offset.y });
        }
      }}
      onMouseMove={handleMouseMove}
      onMouseUp={() => setIsDragging(false)}
      onMouseLeave={() => {
        setIsDragging(false);
        onHoverBlock(null, null);
      }}
      onContextMenu={handleContextMenu}
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
            opacity: block.opacity,
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
  const [sliceDataByY, setSliceDataByY] = useState<Record<number, LayerSliceData | null>>({});
  const [statsData, setStatsData] = useState<StatsData | null>(null);
  const [layerY, setLayerY] = useState(0);
  const [onionSkinDepth, setOnionSkinDepth] = useState(0);
  const [showStateHints, setShowStateHints] = useState(true);
  const [hoverBlock, setHoverBlock] = useState<FlakeHoverBlock | null>(null);
  const [hoverPos, setHoverPos] = useState({ x: 0, y: 0 });
  const [showMaterials, setShowMaterials] = useState(false);
  const [showContainerDialog, setShowContainerDialog] = useState(false);
  const [containerData, setContainerData] = useState<{ type: "chest" | "shulker_box" | "barrel"; items: ContainerItem[]; position: { x: number; y: number; z: number } } | null>(null);
  const [isLoadingContainer, setIsLoadingContainer] = useState(false);
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
  const inFlightSliceKeysRef = useRef(new Set<string>());
  const loadedSliceKeysRef = useRef(new Set<string>());

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
    setSliceDataByY({});
    setCacheExists(false);
    loadedMetaKeyRef.current = "";
    loadedSliceKeysRef.current.clear();

    if (!stored?.cacheFile) {
      inFlightMetaKeyRef.current = "";
      inFlightSliceKeysRef.current.clear();
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
        loadedSliceKeysRef.current.clear();
        inFlightSliceKeysRef.current.clear();
        setSliceDataByY({});
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
    setSliceDataByY({});
    loadedSliceKeysRef.current.clear();
    inFlightSliceKeysRef.current.clear();

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
    const requiredYs: number[] = [];
    const maxDepth = Math.min(5, onionSkinDepth, layerY);
    for (let depth = 0; depth <= maxDepth; depth += 1) {
      requiredYs.push(layerY - depth);
    }

    requiredYs.forEach((targetY) => {
      const sliceKey = `${cacheFile}::${targetY}`;
      if (inFlightSliceKeysRef.current.has(sliceKey) || loadedSliceKeysRef.current.has(sliceKey)) {
        return;
      }
      inFlightSliceKeysRef.current.add(sliceKey);
      // #region agent log
      const layerLoadStart = Date.now();
      fetch('http://127.0.0.1:7337/ingest/eabd7817-9767-4d77-8cb1-446d598a0056',{method:'POST',headers:{'Content-Type':'application/json','X-Debug-Session-Id':'eabd7817-9767-4d77-8cb1-446d598a0056'},body:JSON.stringify({sessionId:'eabd7817-9767-4d77-8cb1-446d598a0056',location:'FlakePage.tsx:656',message:'loadLayerSlice start',data:{cacheFile,targetY},timestamp:Date.now(),hypothesisId:'D'})}).catch(()=>{});
      // #endregion
      
      /*
      console.log("[LBA_FLAKE] effect:loadLayerSlice:start", {
        cacheFile,
        layerY: targetY,
        size_x: meta.size_x,
        size_y: meta.size_y,
        size_z: meta.size_z,
      });
      */
      loadLayerSlice(cacheFile, targetY)
        .then((nextSlice) => {
          // #region agent log
          fetch('http://127.0.0.1:7337/ingest/eabd7817-9767-4d77-8cb1-446d598a0056',{method:'POST',headers:{'Content-Type':'application/json','X-Debug-Session-Id':'eabd7817-9767-4d77-8cb1-446d598a0056'},body:JSON.stringify({sessionId:'eabd7817-9767-4d77-8cb1-446d598a0056',location:'FlakePage.tsx:671',message:'loadLayerSlice success',data:{duration:Date.now()-layerLoadStart,blockCount:nextSlice?.blocks?.length||0},timestamp:Date.now(),hypothesisId:'D'})}).catch(()=>{});
          // #endregion
          
          /*
          console.log("[LBA_FLAKE] effect:loadLayerSlice:end", {
            cacheFile,
            layerY: targetY,
            block_count: nextSlice?.blocks?.length || 0,
          });
          */
          loadedSliceKeysRef.current.add(sliceKey);
          setSliceDataByY((current) => ({
            ...current,
            [targetY]: nextSlice,
          }));
        })
        .catch(() => {
          /*
          console.log("[LBA_FLAKE] effect:loadLayerSlice:error", {
            cacheFile,
            layerY: targetY,
            error: String(error),
          });
          */
          loadedSliceKeysRef.current.delete(sliceKey);
          setSliceDataByY((current) => ({
            ...current,
            [targetY]: null,
          }));
        })
        .finally(() => {
          inFlightSliceKeysRef.current.delete(sliceKey);
        });
    });
  }, [layerY, onionSkinDepth, cacheExists, meta, cacheFile]);

  const sliceData = sliceDataByY[layerY] ?? null;
  const renderSlices = useMemo(() => {
    if (!Object.prototype.hasOwnProperty.call(sliceDataByY, layerY) || sliceDataByY[layerY] === null) return [] as FlakeRenderSlice[];
    const effectiveDepth = Math.min(5, onionSkinDepth, layerY);
    const next: FlakeRenderSlice[] = [];
    for (let depth = 0; depth <= effectiveDepth; depth += 1) {
      const targetY = layerY - depth;
      const slice = sliceDataByY[targetY];
      if (!slice) continue;
      next.push({
        depth,
        opacity: depth === 0 ? 1 : (onionSkinDepth + 1 - depth) / (onionSkinDepth + 1),
        sliceData: slice,
      });
    }
    return next;
  }, [layerY, onionSkinDepth, sliceDataByY]);

  const handleHoverBlock = useCallback((block: FlakeHoverBlock | null, event: React.MouseEvent | null) => {
    setHoverBlock(block);
    if (event) setHoverPos({ x: event.clientX, y: event.clientY });
  }, []);

  const handleBlockRightClick = useCallback(async (block: FlakeHoverBlock, event: React.MouseEvent) => {
    event.preventDefault();
    
    // 防止重复加载
    if (isLoadingContainer) {
      return;
    }
    
    console.log("右键点击方块:", block.id, "坐标:", block.x, block.y, block.z);
    
    // 检查是否为容器方块
    if (!isContainerBlock(block.id)) {
      console.log("不是容器方块");
      return;
    }
    
    console.log("检测到容器方块，开始加载数据...");
    
    // 设置加载状态
    setIsLoadingContainer(true);
    
    try {
      // 计算 regionName（避免闭包依赖问题）
      const region = statsData?.regions?.[0]?.name || currentFile.split(/[\\/]/).pop() || "Unnamed";
      
      // 使用 Promise 包装，避免阻塞主线程
      const data = await Promise.race([
        loadContainerData(currentFile, region, block.x, block.y, block.z),
        new Promise<null>((resolve) => setTimeout(() => resolve(null), 5000)) // 5秒超时
      ]);
      
      console.log("加载到的容器数据:", data);
      
      if (!data) {
        console.log("未找到容器数据或容器为空");
        return;
      }
      
      const containerType = getContainerType(data.block_id);
      
      console.log("容器类型:", containerType);
      
      // 只支持箱子、潜影盒和木桶
      if (!containerType) {
        console.log("不支持的容器类型，当前仅支持：箱子、潜影盒、木桶");
        return;
      }
      
      console.log("准备显示容器对话框");
      
      setContainerData({
        type: containerType,
        items: data.items,
        position: data.position,
      });
      setShowContainerDialog(true);
    } catch (error) {
      console.error("加载容器数据失败:", error);
    } finally {
      setIsLoadingContainer(false);
    }
  }, [currentFile, statsData, isLoadingContainer]);

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
    : `Y=${layerY}; 当前层 ${sliceData?.blocks?.length || 0} 个非空气方块；洋葱皮 ${onionSkinDepth} 层。滚轮缩放，拖拽平移。`);

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

        <div className="flake-page__layer-row flake-page__onion-row">
          <span className="nova-muted flake-page__layer-label">洋葱皮</span>
          <input
            className="flake-page__layer-range"
            type="range"
            min={0}
            max={5}
            step={1}
            value={onionSkinDepth}
            onChange={(event) => setOnionSkinDepth(Math.min(5, Math.max(0, Number(event.target.value) || 0)))}
            disabled={!ready}
          />
          <input
            className="input flake-page__onion-input"
            type="number"
            min={0}
            max={5}
            step={1}
            value={onionSkinDepth}
            onChange={(event) => setOnionSkinDepth(Math.min(5, Math.max(0, Number(event.target.value) || 0)))}
            disabled={!ready}
          />
        </div>
        <div className="nova-muted nova-small flake-page__onion-hint">
          0 表示关闭；只显示当前位置最上面那一层可见方块，下方层按厚度比例半透明补显。
        </div>
        <label className="subwindow-check-row flake-page__edit-toggle">
          <input type="checkbox" checked={showStateHints} onChange={(event) => setShowStateHints(event.target.checked)} disabled={!ready} />
          状态提示图片
        </label>

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
              <LayerCanvas ref={canvasRef} meta={meta} renderSlices={renderSlices} showStateHints={showStateHints} onHoverBlock={handleHoverBlock} onBlockRightClick={handleBlockRightClick} />
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
      
      {showContainerDialog && containerData && (
        <ContainerDialog
          onClose={() => setShowContainerDialog(false)}
          containerType={containerData.type}
          items={containerData.items}
          position={containerData.position}
        />
      )}
    </div>
  );
}


