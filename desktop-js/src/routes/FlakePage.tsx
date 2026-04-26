import React, {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import {
  checkCacheExists,
  getBlockColor,
  LayerSliceData,
  LayerSliceMeta,
  loadLayerMeta,
  loadLayerSlice,
} from "../services/layerService";
import { getLatestRenderCacheState, subscribeRenderCacheStore } from "../services/renderCacheStore";
import { loadStructureStats, StatsData } from "../services/statsService";
import { translateBlockId } from "../services/i18n";
import { MaterialsDialog } from "./StatisticsPage";
import { getBlockIconDataUrl } from "../services/blockIconResolver";

interface LayerCanvasHandle {
  resetView: () => void;
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
    onHoverBlock: (block: any, event: React.MouseEvent | null) => void;
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
          const dataUrl = await getBlockIconDataUrl(entry.block_id);
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
    ctx.imageSmoothingEnabled = false;

    const parent = canvas.parentElement;
    if (parent) {
      canvas.width = parent.clientWidth;
      canvas.height = parent.clientHeight;
    }

    ctx.clearRect(0, 0, canvas.width, canvas.height);
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
      if (px + scale < 0 || pz + scale < 0 || px > canvas.width || pz > canvas.height) continue;

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
      style={{ width: "100%", height: "100%", display: "block", cursor: isDragging ? "grabbing" : "grab" }}
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

export function FlakePage({ currentFile }: any) {
  const [cacheFile, setCacheFile] = useState("");
  const [cacheStatus, setCacheStatus] = useState("idle");
  const [cacheExists, setCacheExists] = useState(false);
  const [meta, setMeta] = useState<LayerSliceMeta | null>(null);
  const [sliceData, setSliceData] = useState<LayerSliceData | null>(null);
  const [statsData, setStatsData] = useState<StatsData | null>(null);
  const [layerY, setLayerY] = useState(0);
  const [hoverBlock, setHoverBlock] = useState<any>(null);
  const [hoverPos, setHoverPos] = useState({ x: 0, y: 0 });
  const [showMaterials, setShowMaterials] = useState(false);
  const canvasRef = useRef<LayerCanvasHandle>(null);

  const syncCacheState = async () => {
    if (!currentFile) return;
    const stored = getLatestRenderCacheState(currentFile);
    setCacheStatus(stored?.status || "idle");
    setCacheFile(stored?.cacheFile || "");
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

  useEffect(() => {
    if (!currentFile) return;
    loadStructureStats(currentFile).then(setStatsData).catch(() => setStatsData(null));
    syncCacheState();
  }, [currentFile]);

  useEffect(() => subscribeRenderCacheStore(syncCacheState), [currentFile]);

  useEffect(() => {
    if (cacheExists && meta && cacheFile) {
      loadLayerSlice(cacheFile, layerY).then(setSliceData);
    }
  }, [layerY, cacheExists, meta, cacheFile]);

  const handleHoverBlock = (block: any, event: React.MouseEvent | null) => {
    setHoverBlock(block);
    if (event) setHoverPos({ x: event.clientX, y: event.clientY });
  };

  const regionName = statsData?.regions?.[0]?.name || "Unnamed";
  const maxY = meta ? Math.max(0, meta.size_y - 1) : 0;
  const ready = cacheExists && cacheStatus === "ready" && !!meta;
  const building = cacheStatus === "building";

  if (!currentFile) {
    return (
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", height: "100%", opacity: 0.7 }}>
        <h2>Layers</h2>
        <p>Select a .litematic file first.</p>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", gap: 12 }}>
      <div style={{ padding: "6px 12px", backgroundColor: "#222", border: "1px solid #444", color: "#ccc", fontSize: "0.9em", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {currentFile}
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ color: "#aaa" }}>Region</span>
        <input className="input" style={{ width: 250 }} value={regionName} readOnly />
      </div>

      <div className="group-box" style={{ padding: "16px 12px", marginTop: 16 }}>
        <div className="group-box-title">Layer control</div>
        <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
          <span style={{ color: "#aaa", minWidth: 60 }}>Layer</span>
          <input type="range" min={0} max={maxY} value={layerY} onChange={(event) => setLayerY(parseInt(event.target.value, 10))} style={{ flex: 1 }} disabled={!ready} />
          <button className="btn" disabled={!ready} onClick={() => canvasRef.current?.resetView()}>Reset view</button>
        </div>

        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-end", marginTop: 8 }}>
          <div style={{ color: "#e0e0e0", fontSize: "1.1em", fontWeight: "bold" }}>Y = {layerY}</div>
          <div style={{ display: "flex", gap: 4 }}>
            <button className="btn" style={{ minWidth: 32 }} disabled={!ready} onClick={() => setLayerY(Math.max(0, layerY - 5))}>--</button>
            <button className="btn" style={{ minWidth: 32 }} disabled={!ready} onClick={() => setLayerY(Math.max(0, layerY - 1))}>-</button>
            <button className="btn" style={{ minWidth: 32 }} disabled={!ready} onClick={() => setLayerY(Math.min(maxY, layerY + 1))}>+</button>
            <button className="btn" style={{ minWidth: 32 }} disabled={!ready} onClick={() => setLayerY(Math.min(maxY, layerY + 5))}>++</button>
          </div>
        </div>
      </div>

      <div style={{ flex: 1, backgroundColor: "#222", border: "1px solid #444", position: "relative", overflow: "hidden" }}>
        {!ready ? (
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", height: "100%", color: "#888" }}>
            <div style={{ fontSize: "1.2em", marginBottom: 8 }}>{building ? "3D cache is building." : "Layer view needs a finished 3D cache."}</div>
            <div>{building ? "Open the Render page to watch the live build progress." : "Build 3D cache on the Render page first."}</div>
          </div>
        ) : (
          <LayerCanvas ref={canvasRef} meta={meta} sliceData={sliceData} onHoverBlock={handleHoverBlock} />
        )}

        {hoverBlock && (
          <div style={{ position: "absolute", left: hoverPos.x + 15, top: hoverPos.y + 15, backgroundColor: "#1a1a1a", border: "1px solid #555", color: "#fff", padding: "8px 12px", zIndex: 10, pointerEvents: "none", boxShadow: "2px 2px 5px rgba(0,0,0,0.5)", display: "flex", flexDirection: "column", gap: 4 }}>
            <div style={{ fontWeight: "bold", fontSize: "1.1em" }}>{hoverBlock.name}</div>
            <div style={{ color: "#aaa" }}>x={hoverBlock.x} y={hoverBlock.y} z={hoverBlock.z}</div>
            <div style={{ color: "#888", fontSize: "0.9em", marginTop: 4 }}>Block ID:</div>
            <div style={{ color: "#ccc", fontSize: "0.95em" }}>{hoverBlock.id}</div>
          </div>
        )}
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <div style={{ flex: 1, color: "#aaa", fontSize: "0.9em" }}>
          {!ready ? (building ? "Cache is building; layer data will become available when the ready file is written." : "Layers unavailable: build 3D cache on the Render page first.") : `Y=${layerY}; ${sliceData?.blocks?.length || 0} non-air blocks. Wheel zooms, drag pans.`}
        </div>
        <button className="btn" style={{ minWidth: 150, padding: "8px 16px" }} onClick={() => setShowMaterials(true)} disabled={!statsData}>
          Materials
        </button>
      </div>

      {showMaterials && statsData && <MaterialsDialog data={statsData} onClose={() => setShowMaterials(false)} currentFile={currentFile} />}
    </div>
  );
}
