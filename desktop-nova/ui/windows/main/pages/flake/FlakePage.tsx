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
  getBlockProperties,
  translateBlockId,
  translateKey,
  translateValue,
  updateRenderCacheState,
  upsertRenderCacheState,
} from "../../../../../src/business/facade";
import { loadMaterialsScope } from "../../../../../src/services/statsService";
import { buildMapColorLookup, loadEnumeratorCollections } from "../../../../../src/services/enumeratorService";
import { resolveFlakeLayerBlockImage, extractLayerPaletteStates } from "../../../../../src/services/flakeStateHintResolver";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Dialog } from "../../../../components/Dialog";
import { MaterialsDialog, openMaterialsWithWindowBehavior } from "../statistics/StatisticsPage";

// 同级函数
import { fitView, FlakeBlockTooltip, formatStateRecord } from "./function";
import { CreativeInventoryDialog } from "./creativeInventoryDialog";
import { ContainerDialog } from "./containerDialog";
import { loadContainerData, isContainerBlock, getContainerType, type ContainerItem, type ContainerType } from "../../../../../src/business/facade";
import noteCsvText from "../../../../../note.csv?raw";

// 缩放范围：scale 表示每个方块占用的像素数（像素/格）。
const MIN_SCALE = 1;
const MAX_SCALE = 64;
const ICON_NATIVE_SIZE = 16;
const LOW_ZOOM_TILE_SIZE = 16;
const HOVER_TOOLTIP_MIN_SCALE = 8;
const RENDER_OVERSCAN_PIXELS = 256;
const LOW_ZOOM_OVERSCAN_PIXELS = 64;
const MAX_LOW_ZOOM_TILE_CACHE_SIZE = 2048;
const NOTE_BLOCK_ID = "minecraft:note_block";
const NOTE_BLOCK_NOTE_VALUES = Array.from({ length: 25 }, (_unused, index) => String(index));
const NOTE_BLOCK_KEY_ROWS = [
  { id: "high", label: "高位音高", values: Array.from({ length: 7 }, (_unused, index) => String(index + 18)) },
  { id: "middle", label: "本位音高", values: Array.from({ length: 12 }, (_unused, index) => String(index + 6)) },
  { id: "low", label: "低位音高", values: Array.from({ length: 6 }, (_unused, index) => String(index)) },
] as const;
const EDIT_TOOL_PLACEHOLDERS = [
  { id: "hand", glyph: "手", name: "手型", description: "拖拽和查看" },
  { id: "debug-stick", glyph: "调", name: "调试棒", description: "修改方块状态" },
  { id: "measure", glyph: "量", name: "测量工具", description: "测量欧几里得距离与曼哈顿距离，右键删除矩形" },
  { id: "select", glyph: "选", name: "框选工具", description: "框选区域" },
  { id: "brush", glyph: "笔", name: "画笔", description: "绘制方块" },
  { id: "eraser", glyph: "擦", name: "橡皮擦", description: "擦除方块" },
  { id: "picker", glyph: "吸", name: "取色器", description: "拾取方块" },
  { id: "rectangle", glyph: "矩", name: "矩形工具", description: "绘制矩形" },
  { id: "circle", glyph: "圆", name: "圆形工具", description: "绘制圆形" },
  { id: "bucket", glyph: "桶", name: "填色工具", description: "填充区域" },
  { id: "text", glyph: "字", name: "文字工具", description: "仅支持 unifont 字体" },
] as const;

type EditToolId = typeof EDIT_TOOL_PLACEHOLDERS[number]["id"];
type CssVariableStyle<T extends string> = React.CSSProperties & Record<T, string | number>;

interface NoteToneLabel {
  value: string;
  raw: string;
  base: string;
  sharpMarks: string;
}

interface NoteScaleOption {
  name: string;
  labelsByValue: Record<string, NoteToneLabel>;
}

function parseCsvLine(line: string): string[] {
  const cells: string[] = [];
  let cell = "";
  let inQuotes = false;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (char === "\"") {
      if (inQuotes && line[index + 1] === "\"") {
        cell += "\"";
        index += 1;
      } else {
        inQuotes = !inQuotes;
      }
      continue;
    }
    if (char === "," && !inQuotes) {
      cells.push(cell.trim());
      cell = "";
      continue;
    }
    cell += char;
  }
  cells.push(cell.trim());
  return cells;
}

function parseNoteToneLabel(value: string, rawLabel: string | undefined): NoteToneLabel {
  const raw = rawLabel?.trim() || value;
  const sharpMatch = raw.match(/#+$/);
  const sharpMarks = sharpMatch?.[0] || "";
  const base = sharpMarks ? raw.slice(0, -sharpMarks.length) || raw : raw;
  return { value, raw, base, sharpMarks };
}

function parseNoteScaleOptions(csvText: string): NoteScaleOption[] {
  const rows = csvText
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map(parseCsvLine);
  const header = rows[0] || [];
  const noteValues = header.slice(1).filter((value) => NOTE_BLOCK_NOTE_VALUES.includes(value));

  return rows.slice(1).map((row) => {
    const labelsByValue: Record<string, NoteToneLabel> = {};
    noteValues.forEach((value, index) => {
      labelsByValue[value] = parseNoteToneLabel(value, row[index + 1]);
    });
    NOTE_BLOCK_NOTE_VALUES.forEach((value) => {
      labelsByValue[value] = labelsByValue[value] || parseNoteToneLabel(value, value);
    });
    return {
      name: row[0] || "默认调式",
      labelsByValue,
    };
  });
}

const NOTE_SCALE_OPTIONS = parseNoteScaleOptions(noteCsvText);
const DEFAULT_NOTE_SCALE_NAME = NOTE_SCALE_OPTIONS[0]?.name || "默认调式";

function flakeBlockPositionKey(x: number, y: number, z: number): string {
  return `${x}:${y}:${z}`;
}

function normalizeLayerStateRecord(value: Record<string, unknown> | null | undefined): Record<string, string> {
  const next: Record<string, string> = {};
  if (!value) return next;
  Object.entries(value).forEach(([key, raw]) => {
    if (!key || raw === null || raw === undefined) return;
    next[key] = String(raw);
  });
  return next;
}

function layerStateRecordText(stateRecord: Record<string, string>): string {
  return formatStateRecord(stateRecord) || "无";
}

function isTextEntryTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return !!target.closest("input, textarea, select");
}

// 全局图标缓存，避免重复处理相同方块
const globalIconCache = new Map<string, Promise<string | null>>();

function getCachedIconImage(
  blockId: string,
  paletteEntry: any,
  propertyPool: any[],
  enabled: boolean
): Promise<string | null> {
  // 缓存键必须基于解析后的真实状态，而非 paletteEntry.property_id 下标。
  // property_id 只是指向当前投影 propertyPool 的索引，不同投影的 propertyPool
  // 各自独立——同一个 property_id 在投影 A 可能是 level=0，在投影 B 却是 level=3。
  // 若只用 property_id 作键，跨投影会命中错误的旧图标（如水的 level 遮罩错乱）。
  const resolvedStates = extractLayerPaletteStates(paletteEntry, propertyPool);
  const cacheKey = `${blockId}::${JSON.stringify(resolvedStates)}::${enabled}`;

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
  // 将缩放设置到指定的像素/格值，围绕视口中心缩放。
  zoomTo: (nextScale: number) => void;
}

export interface FlakeHoverBlock {
  x: number;
  y: number;
  z: number;
  paletteId: number;
  id: string;
  name: string;
  states: string;
  stateRecord: Record<string, string>;
  originalStateRecord: Record<string, string>;
  hasStates: boolean;
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

interface VisibleLayerIndex {
  byPosition: Map<number, VisibleLayerBlock>;
  rows: Map<number, VisibleLayerBlock[]>;
  tiles: Map<string, VisibleLayerBlock[]>;
}

interface VisibleBlockWindow {
  minX: number;
  maxX: number;
  minZ: number;
  maxZ: number;
}

interface FlakeMeasurementRect {
  id: string;
  startX: number;
  startZ: number;
  endX: number;
  endZ: number;
}

interface NormalizedMeasurementRect {
  minX: number;
  maxX: number;
  minZ: number;
  maxZ: number;
  widthBlocks: number;
  heightBlocks: number;
}

interface FlakeMeasurementPoint {
  x: number;
  z: number;
}

interface LowZoomTileCache {
  meta: LayerSliceMeta | null;
  scale: number;
  devicePixelRatio: number;
  colorMap: Map<number, string> | null;
  tileSource: Map<string, VisibleLayerBlock[]> | null;
  tiles: Map<string, HTMLCanvasElement>;
}

function findFirstBlockAtOrAfterX(blocks: VisibleLayerBlock[], minX: number): number {
  let low = 0;
  let high = blocks.length;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (blocks[mid].block.x < minX) low = mid + 1;
    else high = mid;
  }
  return low;
}

function normalizeFlakeBlockId(blockId: string): string {
  const normalized = String(blockId || "").trim().toLowerCase();
  if (!normalized) return "";
  return normalized.includes(":") ? normalized : `minecraft:${normalized}`;
}

function lowZoomTileKey(tileX: number, tileZ: number): string {
  return `${tileX}:${tileZ}`;
}

function normalizeMeasurementRect(rect: FlakeMeasurementRect): NormalizedMeasurementRect {
  const minX = Math.min(rect.startX, rect.endX);
  const maxX = Math.max(rect.startX, rect.endX);
  const minZ = Math.min(rect.startZ, rect.endZ);
  const maxZ = Math.max(rect.startZ, rect.endZ);
  return {
    minX,
    maxX,
    minZ,
    maxZ,
    widthBlocks: maxX - minX + 1,
    heightBlocks: maxZ - minZ + 1,
  };
}

function measurementRectContainsPoint(rect: FlakeMeasurementRect, point: FlakeMeasurementPoint): boolean {
  const normalized = normalizeMeasurementRect(rect);
  return point.x >= normalized.minX
    && point.x <= normalized.maxX
    && point.z >= normalized.minZ
    && point.z <= normalized.maxZ;
}

function formatMeasurementDistance(value: number): string {
  if (Number.isInteger(value)) return String(value);
  return value < 10 ? value.toFixed(2) : value.toFixed(1);
}

function buildLowZoomTile(
  tileBlocks: VisibleLayerBlock[],
  tileX: number,
  tileZ: number,
  meta: LayerSliceMeta,
  scale: number,
  devicePixelRatio: number,
  colorMap: Map<number, string>,
): { canvas: HTMLCanvasElement; width: number; height: number } {
  const originX = tileX * LOW_ZOOM_TILE_SIZE;
  const originZ = tileZ * LOW_ZOOM_TILE_SIZE;
  const width = Math.max(1, Math.ceil(Math.min(LOW_ZOOM_TILE_SIZE, meta.size_x - originX) * scale));
  const height = Math.max(1, Math.ceil(Math.min(LOW_ZOOM_TILE_SIZE, meta.size_z - originZ) * scale));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.ceil(width * devicePixelRatio));
  canvas.height = Math.max(1, Math.ceil(height * devicePixelRatio));

  const context = canvas.getContext("2d");
  if (!context) return { canvas, width, height };

  context.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
  context.imageSmoothingEnabled = false;
  for (const visibleBlock of tileBlocks) {
    const color = colorMap.get(visibleBlock.block.palette_id) || "#f0f";
    if (color === "transparent") continue;

    const localX = visibleBlock.block.x - originX;
    const localZ = visibleBlock.block.z - originZ;
    const left = Math.floor(localX * scale);
    const top = Math.floor(localZ * scale);
    const right = Math.max(left + 1, Math.ceil((localX + 1) * scale));
    const bottom = Math.max(top + 1, Math.ceil((localZ + 1) * scale));
    context.globalAlpha = visibleBlock.opacity;
    context.fillStyle = color;
    context.fillRect(left, top, right - left, bottom - top);
  }
  context.globalAlpha = 1;

  return { canvas, width, height };
}

const LayerCanvas = forwardRef<
  LayerCanvasHandle,
  {
    meta: LayerSliceMeta | null;
    renderSlices: FlakeRenderSlice[];
    showStateHints: boolean;
    interactionTool: EditToolId;
    editedBlockStates: Record<string, Record<string, string>>;
    measurements: FlakeMeasurementRect[];
    onHoverBlock: (block: FlakeHoverBlock | null, event: React.MouseEvent | null) => void;
    onBlockClick: (block: FlakeHoverBlock, event: React.MouseEvent) => void;
    onBlockRightClick: (block: FlakeHoverBlock, event: React.MouseEvent) => void;
    onAddMeasurement: (rect: Omit<FlakeMeasurementRect, "id">) => void;
    onDeleteMeasurementAt: (point: FlakeMeasurementPoint) => void;
    onScaleChange?: (scale: number) => void;
  }
>(({ meta, renderSlices, showStateHints, interactionTool, editedBlockStates, measurements, onHoverBlock, onBlockClick, onBlockRightClick, onAddMeasurement, onDeleteMeasurementAt, onScaleChange }, ref) => {
  const viewportRef = useRef<HTMLDivElement>(null);
  const worldRef = useRef<HTMLDivElement>(null);
  const lowZoomCanvasRef = useRef<HTMLCanvasElement>(null);
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const [isMeasuring, setIsMeasuring] = useState(false);
  const [measurementDraft, setMeasurementDraft] = useState<FlakeMeasurementRect | null>(null);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const [iconImages, setIconImages] = useState<Map<number, string>>(new Map());
  const [mapColorByBlockId, setMapColorByBlockId] = useState<Map<string, string>>(new Map());
  const [viewportSize, setViewportSize] = useState({ width: 0, height: 0 });
  const dragFrameRef = useRef<number | null>(null);
  const pendingOffsetRef = useRef(offset);
  const measurementDraftRef = useRef<FlakeMeasurementRect | null>(null);
  const lowZoomTileCacheRef = useRef<LowZoomTileCache>({
    meta: null,
    scale: 0,
    devicePixelRatio: 0,
    colorMap: null,
    tileSource: null,
    tiles: new Map(),
  });

  // 用 ref 镜像最新的 scale / offset，供事件处理和命令式缩放共用，避免闭包读到旧值。
  const scaleRef = useRef(scale);
  const offsetRef = useRef(offset);
  scaleRef.current = scale;
  offsetRef.current = offset;

  const scheduleOffset = useCallback((nextOffset: { x: number; y: number }) => {
    pendingOffsetRef.current = nextOffset;
    if (dragFrameRef.current !== null) return;
    dragFrameRef.current = window.requestAnimationFrame(() => {
      dragFrameRef.current = null;
      setOffset(pendingOffsetRef.current);
    });
  }, []);

  // 通知父组件当前缩放，用于同步滑块和输入框。用 ref 保存回调避免因引用变化触发多余 effect。
  const onScaleChangeRef = useRef(onScaleChange);
  onScaleChangeRef.current = onScaleChange;
  useEffect(() => {
    onScaleChangeRef.current?.(scale);
  }, [scale]);

  const setMeasurementDraftValue = useCallback((nextDraft: FlakeMeasurementRect | null) => {
    measurementDraftRef.current = nextDraft;
    setMeasurementDraft(nextDraft);
  }, []);

  const updateMeasurementDraftValue = useCallback((updater: (current: FlakeMeasurementRect | null) => FlakeMeasurementRect | null) => {
    setMeasurementDraftValue(updater(measurementDraftRef.current));
  }, [setMeasurementDraftValue]);

  const resolveMeasurementPointAtClient = useCallback((clientX: number, clientY: number, clampToBounds = false): FlakeMeasurementPoint | null => {
    if (!meta || !viewportRef.current) return null;

    const rect = viewportRef.current.getBoundingClientRect();
    const rawX = Math.floor((clientX - rect.left - offsetRef.current.x) / scaleRef.current);
    const rawZ = Math.floor((clientY - rect.top - offsetRef.current.y) / scaleRef.current);
    if (clampToBounds) {
      return {
        x: Math.max(0, Math.min(meta.size_x - 1, rawX)),
        z: Math.max(0, Math.min(meta.size_z - 1, rawZ)),
      };
    }
    if (rawX < 0 || rawX >= meta.size_x || rawZ < 0 || rawZ >= meta.size_z) return null;
    return { x: rawX, z: rawZ };
  }, [meta]);

  const finishMeasurementDraft = useCallback(() => {
    const draft = measurementDraftRef.current;
    setIsMeasuring(false);
    setMeasurementDraftValue(null);
    if (!draft) return;
    onAddMeasurement({
      startX: draft.startX,
      startZ: draft.startZ,
      endX: draft.endX,
      endZ: draft.endZ,
    });
  }, [onAddMeasurement, setMeasurementDraftValue]);

  useEffect(() => {
    if (interactionTool === "measure" && meta) return;
    setIsMeasuring(false);
    setMeasurementDraftValue(null);
  }, [interactionTool, meta, setMeasurementDraftValue]);

  useEffect(() => {
    if (!isMeasuring) return;

    const handleMouseMove = (event: MouseEvent) => {
      const point = resolveMeasurementPointAtClient(event.clientX, event.clientY, true);
      if (!point) return;
      updateMeasurementDraftValue((current) => current ? { ...current, endX: point.x, endZ: point.z } : current);
    };
    const handleMouseUp = (event: MouseEvent) => {
      if (event.button !== 0) return;
      event.preventDefault();
      finishMeasurementDraft();
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [finishMeasurementDraft, isMeasuring, resolveMeasurementPointAtClient, updateMeasurementDraftValue]);

  useEffect(() => {
    if (scale < HOVER_TOOLTIP_MIN_SCALE) {
      onHoverBlock(null, null);
    }
  }, [onHoverBlock, scale]);

  useLayoutEffect(() => {
    const world = worldRef.current;
    if (!world) return;
    world.style.transform = `translate(${Math.round(offset.x)}px, ${Math.round(offset.y)}px)`;
  }, [offset.x, offset.y]);

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;

    const updateViewportSize = () => {
      const rect = viewport.getBoundingClientRect();
      const width = Math.round(rect.width);
      const height = Math.round(rect.height);
      setViewportSize((current) => (
        current.width === width && current.height === height ? current : { width, height }
      ));
    };

    updateViewportSize();
    const observer = new ResizeObserver(updateViewportSize);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, []);

  useEffect(() => () => {
    if (dragFrameRef.current !== null) {
      window.cancelAnimationFrame(dragFrameRef.current);
    }
  }, []);

  // 围绕锚点 (anchorX, anchorY，均相对视口左上角) 将缩放设置为 nextScale。
  const applyZoom = (rawScale: number, anchorX: number, anchorY: number) => {
    const nextScale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, rawScale));
    const currentScale = scaleRef.current;
    if (nextScale === currentScale) return;
    const currentOffset = offsetRef.current;
    const ratio = nextScale / currentScale;
    setOffset({
      x: anchorX - (anchorX - currentOffset.x) * ratio,
      y: anchorY - (anchorY - currentOffset.y) * ratio,
    });
    setScale(nextScale);
  };

  useEffect(() => {
    let cancelled = false;
    loadEnumeratorCollections()
      .then((collections) => {
        if (!cancelled) setMapColorByBlockId(buildMapColorLookup(collections));
      })
      .catch((error) => {
        if (cancelled) return;
        console.warn("Failed to load map base colors:", error);
        setMapColorByBlockId(new Map());
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const colorMap = useMemo(() => {
    const next = new Map<number, string>();
    if (!meta) return next;
    meta.palette.forEach((entry, index) => {
      const normalizedBlockId = normalizeFlakeBlockId(entry.block_id);
      const mapColor = mapColorByBlockId.get(normalizedBlockId);
      let color = mapColor || getBlockColor(entry.block_id);
      if (!mapColor) {
        if (normalizedBlockId.includes("stone")) color = "#888";
        else if (normalizedBlockId.includes("dirt")) color = "#754";
        else if (normalizedBlockId.includes("grass")) color = "#583";
        else if (normalizedBlockId.includes("quartz")) color = "#eee";
        else if (normalizedBlockId.includes("glass")) color = "rgba(200,200,255,0.5)";
      }
      if (normalizedBlockId.includes("air")) color = "transparent";
      next.set(index, color);
    });
    return next;
  }, [mapColorByBlockId, meta]);

  const visibleLayerIndex = useMemo<VisibleLayerIndex>(() => {
    const byPosition = new Map<number, VisibleLayerBlock>();
    const rowMaps = new Map<number, Map<number, VisibleLayerBlock>>();
    const tileMaps = new Map<string, VisibleLayerBlock[]>();
    if (!meta || renderSlices.length === 0) {
      return { byPosition, rows: new Map(), tiles: tileMaps };
    }

    for (const renderSlice of renderSlices) {
      for (const block of renderSlice.sliceData.blocks) {
        const positionKey = block.z * meta.size_x + block.x;
        if (byPosition.has(positionKey)) continue;
        const visibleBlock = {
          block,
          y: renderSlice.sliceData.y,
          opacity: renderSlice.opacity,
        };
        byPosition.set(positionKey, visibleBlock);

        const tileKey = lowZoomTileKey(
          Math.floor(block.x / LOW_ZOOM_TILE_SIZE),
          Math.floor(block.z / LOW_ZOOM_TILE_SIZE),
        );
        const tile = tileMaps.get(tileKey);
        if (tile) {
          tile.push(visibleBlock);
        } else {
          tileMaps.set(tileKey, [visibleBlock]);
        }

        let row = rowMaps.get(block.z);
        if (!row) {
          row = new Map<number, VisibleLayerBlock>();
          rowMaps.set(block.z, row);
        }
        row.set(block.x, visibleBlock);
      }
    }

    const rows = new Map<number, VisibleLayerBlock[]>();
    for (const [z, row] of rowMaps.entries()) {
      rows.set(z, Array.from(row.values()).sort((a, b) => a.block.x - b.block.x));
    }
    return { byPosition, rows, tiles: tileMaps };
  }, [meta, renderSlices]);

  const visibleBlockWindow = useMemo<VisibleBlockWindow | null>(() => {
    if (!meta) return null;
    const width = viewportSize.width || 600;
    const height = viewportSize.height || 600;
    const safeScale = Math.max(MIN_SCALE, scale);
    const overscanPixels = scale < ICON_NATIVE_SIZE ? LOW_ZOOM_OVERSCAN_PIXELS : RENDER_OVERSCAN_PIXELS;
    const overscanBlocks = Math.max(2, Math.ceil(overscanPixels / safeScale));
    const minX = Math.max(0, Math.floor((-offset.x) / safeScale) - overscanBlocks);
    const maxX = Math.min(meta.size_x - 1, Math.ceil((width - offset.x) / safeScale) + overscanBlocks);
    const minZ = Math.max(0, Math.floor((-offset.y) / safeScale) - overscanBlocks);
    const maxZ = Math.min(meta.size_z - 1, Math.ceil((height - offset.y) / safeScale) + overscanBlocks);
    if (maxX < minX || maxZ < minZ) return null;
    return { minX, maxX, minZ, maxZ };
  }, [meta, offset.x, offset.y, scale, viewportSize.height, viewportSize.width]);

  const slicePaletteIds = useMemo(() => {
    if (
      !meta
      || scale < ICON_NATIVE_SIZE
      || visibleLayerIndex.rows.size === 0
      || !visibleBlockWindow
    ) {
      return [] as number[];
    }

    const seen = new Set<number>();
    const paletteIds: number[] = [];
    for (let z = visibleBlockWindow.minZ; z <= visibleBlockWindow.maxZ; z += 1) {
      const row = visibleLayerIndex.rows.get(z);
      if (!row) continue;

      for (let index = findFirstBlockAtOrAfterX(row, visibleBlockWindow.minX); index < row.length; index += 1) {
        const visibleBlock = row[index];
        if (visibleBlock.block.x > visibleBlockWindow.maxX) break;

        const paletteId = visibleBlock.block.palette_id;
        if (seen.has(paletteId)) continue;
        seen.add(paletteId);
        const entry = meta.palette[paletteId];
        if (!entry?.block_id || normalizeFlakeBlockId(entry.block_id).includes("air")) continue;
        paletteIds.push(paletteId);
      }
    }
    return paletteIds;
  }, [meta, scale, visibleBlockWindow, visibleLayerIndex]);

  useLayoutEffect(() => {
    const tileCache = lowZoomTileCacheRef.current;
    if (scale >= ICON_NATIVE_SIZE || !meta || !visibleBlockWindow) {
      tileCache.tiles.clear();
      tileCache.meta = null;
      tileCache.scale = 0;
      tileCache.devicePixelRatio = 0;
      tileCache.colorMap = null;
      tileCache.tileSource = null;
      return;
    }

    const canvas = lowZoomCanvasRef.current;
    const viewport = viewportRef.current;
    if (!canvas || !viewport) return;

    const width = Math.max(1, viewportSize.width || viewport.clientWidth || 600);
    const height = Math.max(1, viewportSize.height || viewport.clientHeight || 600);
    const devicePixelRatio = Math.max(1, window.devicePixelRatio || 1);
    if (
      tileCache.meta !== meta
      || tileCache.scale !== scale
      || tileCache.devicePixelRatio !== devicePixelRatio
      || tileCache.colorMap !== colorMap
      || tileCache.tileSource !== visibleLayerIndex.tiles
    ) {
      tileCache.tiles.clear();
      tileCache.meta = meta;
      tileCache.scale = scale;
      tileCache.devicePixelRatio = devicePixelRatio;
      tileCache.colorMap = colorMap;
      tileCache.tileSource = visibleLayerIndex.tiles;
    }

    const canvasWidth = Math.max(1, Math.round(width * devicePixelRatio));
    const canvasHeight = Math.max(1, Math.round(height * devicePixelRatio));
    if (canvas.width !== canvasWidth || canvas.height !== canvasHeight) {
      canvas.width = canvasWidth;
      canvas.height = canvasHeight;
    }

    const context = canvas.getContext("2d");
    if (!context) return;
    context.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
    context.imageSmoothingEnabled = false;
    context.clearRect(0, 0, width, height);

    const minTileX = Math.floor(visibleBlockWindow.minX / LOW_ZOOM_TILE_SIZE);
    const maxTileX = Math.floor(visibleBlockWindow.maxX / LOW_ZOOM_TILE_SIZE);
    const minTileZ = Math.floor(visibleBlockWindow.minZ / LOW_ZOOM_TILE_SIZE);
    const maxTileZ = Math.floor(visibleBlockWindow.maxZ / LOW_ZOOM_TILE_SIZE);
    for (let tileZ = minTileZ; tileZ <= maxTileZ; tileZ += 1) {
      for (let tileX = minTileX; tileX <= maxTileX; tileX += 1) {
        const tileKey = lowZoomTileKey(tileX, tileZ);
        const tileBlocks = visibleLayerIndex.tiles.get(tileKey);
        if (!tileBlocks) continue;

        let tile = tileCache.tiles.get(tileKey);
        if (!tile) {
          const built = buildLowZoomTile(
            tileBlocks,
            tileX,
            tileZ,
            meta,
            scale,
            devicePixelRatio,
            colorMap,
          );
          tile = built.canvas;
          tileCache.tiles.set(tileKey, tile);
          if (tileCache.tiles.size > MAX_LOW_ZOOM_TILE_CACHE_SIZE) {
            const oldestKey = tileCache.tiles.keys().next().value;
            if (oldestKey) tileCache.tiles.delete(oldestKey);
          }
        }

        const originX = tileX * LOW_ZOOM_TILE_SIZE;
        const originZ = tileZ * LOW_ZOOM_TILE_SIZE;
        const tileWidth = Math.max(1, Math.ceil(Math.min(LOW_ZOOM_TILE_SIZE, meta.size_x - originX) * scale));
        const tileHeight = Math.max(1, Math.ceil(Math.min(LOW_ZOOM_TILE_SIZE, meta.size_z - originZ) * scale));
        context.drawImage(
          tile,
          Math.round(originX * scale + offset.x),
          Math.round(originZ * scale + offset.y),
          tileWidth,
          tileHeight,
        );
      }
    }
  }, [
    colorMap,
    meta,
    offset.x,
    offset.y,
    scale,
    visibleBlockWindow,
    visibleLayerIndex.tiles,
    viewportSize.height,
    viewportSize.width,
  ]);

  const visibleBlocks = useMemo(() => {
    if (
      !meta
      || scale < ICON_NATIVE_SIZE
      || visibleLayerIndex.byPosition.size === 0
      || !visibleBlockWindow
    ) return [] as Array<{
      key: string;
      iconUrl: string;
      shouldRenderIcon: boolean;
      style: CssVariableStyle<
        | "--flake-layer-block-left"
        | "--flake-layer-block-top"
        | "--flake-layer-block-width"
        | "--flake-layer-block-height"
        | "--flake-layer-block-opacity"
        | "--flake-layer-block-background"
      >;
    }>;

    const shouldRenderIcons = scale >= ICON_NATIVE_SIZE;
    const blocks: Array<{
      key: string;
      iconUrl: string;
      shouldRenderIcon: boolean;
      style: CssVariableStyle<
        | "--flake-layer-block-left"
        | "--flake-layer-block-top"
        | "--flake-layer-block-width"
        | "--flake-layer-block-height"
        | "--flake-layer-block-opacity"
        | "--flake-layer-block-background"
      >;
    }> = [];

    for (let z = visibleBlockWindow.minZ; z <= visibleBlockWindow.maxZ; z += 1) {
      const row = visibleLayerIndex.rows.get(z);
      if (!row) continue;

      for (let index = findFirstBlockAtOrAfterX(row, visibleBlockWindow.minX); index < row.length; index += 1) {
        const visibleBlock = row[index];
        if (visibleBlock.block.x > visibleBlockWindow.maxX) break;

        const color = colorMap.get(visibleBlock.block.palette_id) || "#f0f";
        if (color === "transparent") continue;
        const iconUrl = iconImages.get(visibleBlock.block.palette_id) || "";
        const shouldRenderIcon = shouldRenderIcons && !!iconUrl;
        blocks.push({
          key: `${visibleBlock.block.x}:${visibleBlock.block.z}:${visibleBlock.y}:${visibleBlock.block.palette_id}`,
          iconUrl,
          shouldRenderIcon,
          style: {
            "--flake-layer-block-left": `${Math.round(visibleBlock.block.x * scale)}px`,
            "--flake-layer-block-top": `${Math.round(visibleBlock.block.z * scale)}px`,
            "--flake-layer-block-width": `${Math.max(1, Math.ceil(scale))}px`,
            "--flake-layer-block-height": `${Math.max(1, Math.ceil(scale))}px`,
            "--flake-layer-block-opacity": visibleBlock.opacity,
            "--flake-layer-block-background": shouldRenderIcon ? "transparent" : color,
          },
        });
      }
    }

    return blocks;
  }, [colorMap, iconImages, meta, scale, visibleBlockWindow, visibleLayerIndex]);

  const borderStyle: CssVariableStyle<"--flake-layer-border-width" | "--flake-layer-border-height"> = useMemo(() => ({
    "--flake-layer-border-width": `${Math.max(1, Math.round(meta ? meta.size_x * scale : 0))}px`,
    "--flake-layer-border-height": `${Math.max(1, Math.round(meta ? meta.size_z * scale : 0))}px`,
  }), [meta, scale]);

  const measurementOverlays = useMemo(() => {
    const allMeasurements = measurementDraft ? [...measurements, measurementDraft] : measurements;
    return allMeasurements.map((rect) => {
      const normalized = normalizeMeasurementRect(rect);
      const leftPx = Math.round(normalized.minX * scale);
      const topPx = Math.round(normalized.minZ * scale);
      const widthPx = Math.max(1, Math.round((normalized.maxX + 1) * scale) - leftPx);
      const heightPx = Math.max(1, Math.round((normalized.maxZ + 1) * scale) - topPx);
      const diagonalStartX = rect.startX <= rect.endX ? 0 : widthPx;
      const diagonalStartY = rect.startZ <= rect.endZ ? 0 : heightPx;
      const diagonalEndX = rect.startX <= rect.endX ? widthPx : 0;
      const diagonalEndY = rect.startZ <= rect.endZ ? heightPx : 0;
      const euclidean = Math.sqrt(
        normalized.widthBlocks * normalized.widthBlocks
        + normalized.heightBlocks * normalized.heightBlocks
      );
      const manhattan = normalized.widthBlocks + normalized.heightBlocks;
      const gridPathParts: string[] = [];
      if (scale >= 8) {
        for (let x = normalized.minX + 1; x <= normalized.maxX; x += 1) {
          const lineX = Math.round(x * scale) - leftPx;
          gridPathParts.push(`M ${lineX} 0 V ${heightPx}`);
        }
        for (let z = normalized.minZ + 1; z <= normalized.maxZ; z += 1) {
          const lineZ = Math.round(z * scale) - topPx;
          gridPathParts.push(`M 0 ${lineZ} H ${widthPx}`);
        }
      }
      return {
        id: rect.id,
        isDraft: rect.id === "__draft__",
        gridPath: gridPathParts.join(" "),
        showGrid: gridPathParts.length > 0,
        widthPx,
        heightPx,
        gridDash: scale >= 8 ? "4 4" : "2 2",
        xLabel: `x ${normalized.widthBlocks}`,
        zLabel: `z ${normalized.heightBlocks}`,
        manhattanLabel: `曼哈顿 ${manhattan}`,
        insideLabel: widthPx >= 64 && heightPx >= 28
          ? `欧氏 ${formatMeasurementDistance(euclidean)}`
          : `E ${formatMeasurementDistance(euclidean)}`,
        showInsideLabel: widthPx >= 28 && heightPx >= 18,
        diagonalStartX,
        diagonalStartY,
        diagonalEndX,
        diagonalEndY,
        style: {
          "--flake-measurement-left": `${leftPx}px`,
          "--flake-measurement-top": `${topPx}px`,
          "--flake-measurement-width": `${widthPx}px`,
          "--flake-measurement-height": `${heightPx}px`,
        } as CssVariableStyle<
          | "--flake-measurement-left"
          | "--flake-measurement-top"
          | "--flake-measurement-width"
          | "--flake-measurement-height"
        >,
      };
    });
  }, [measurementDraft, measurements, scale]);

  const resetView = () => {
    if (meta) fitView(meta, viewportRef.current, setScale, setOffset);
  };

  // 命令式缩放：围绕视口中心缩放，供层级控制区的滑块 / 输入框调用。
  const zoomTo = (nextScale: number) => {
    const viewport = viewportRef.current;
    const rect = viewport?.getBoundingClientRect();
    const anchorX = rect ? rect.width / 2 : 0;
    const anchorY = rect ? rect.height / 2 : 0;
    applyZoom(nextScale, anchorX, anchorY);
  };

  useImperativeHandle(ref, () => ({ resetView, zoomTo }), [meta]);

  useEffect(() => {
    if (meta) resetView();
  }, [meta]);

  useEffect(() => {
    let cancelled = false;
    
    if (!meta || scale < ICON_NATIVE_SIZE || slicePaletteIds.length === 0) {
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
      const nextImages = new Map<number, string>();
      for (const result of results) {
        if (result && result.dataUrl) {
          nextImages.set(result.paletteId, result.dataUrl);
        }
      }
      
      setIconImages(nextImages);
    })();

    return () => {
      cancelled = true;
    };
  }, [meta, scale, slicePaletteIds, showStateHints]);

  // 滚轮事件
  const handleWheel = (event: React.WheelEvent) => {
    event.preventDefault();
    if (!viewportRef.current) return;
    const direction = event.deltaY < 0 ? 1 : -1;
    const rect = viewportRef.current.getBoundingClientRect();
    // 缩放乘数
    applyZoom(scaleRef.current * Math.pow(2, direction), event.clientX - rect.left, event.clientY - rect.top);
  };

  const resolveBlockAtEvent = (event: React.MouseEvent): FlakeHoverBlock | null => {
    if (!meta || visibleLayerIndex.byPosition.size === 0) return null;

    const point = resolveMeasurementPointAtClient(event.clientX, event.clientY);
    if (!point) return null;
    const bx = point.x;
    const bz = point.z;
    const visibleBlock = visibleLayerIndex.byPosition.get(bz * meta.size_x + bx);
    if (!visibleBlock) return null;

    const paletteId = visibleBlock.block.palette_id;
    const paletteEntry = meta.palette[paletteId];
    if (!paletteEntry) return null;

    const originalStateRecord = normalizeLayerStateRecord(extractLayerPaletteStates(paletteEntry, meta.property_pool || []));
    const positionKey = flakeBlockPositionKey(bx, visibleBlock.y, bz);
    const stateRecord = editedBlockStates[positionKey] || originalStateRecord;

    return {
      x: bx,
      y: visibleBlock.y,
      z: bz,
      paletteId,
      id: paletteEntry.block_id,
      name: translateBlockId(paletteEntry.block_id),
      states: layerStateRecordText(stateRecord),
      stateRecord,
      originalStateRecord,
      hasStates: Object.keys(originalStateRecord).length > 0,
    };
  };

  const handleMouseMove = (event: React.MouseEvent) => {
    if (isMeasuring) {
      onHoverBlock(null, null);
      return;
    }

    if (isDragging) {
      scheduleOffset({ x: event.clientX - dragStart.x, y: event.clientY - dragStart.y });
      return;
    }

    if (scale < HOVER_TOOLTIP_MIN_SCALE) {
      onHoverBlock(null, null);
      return;
    }

    onHoverBlock(resolveBlockAtEvent(event), event);
  };

  const handleContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    if (interactionTool === "measure") {
      setIsMeasuring(false);
      setMeasurementDraftValue(null);
      const point = resolveMeasurementPointAtClient(event.clientX, event.clientY);
      if (point) onDeleteMeasurementAt(point);
      return;
    }

    const block = resolveBlockAtEvent(event);
    if (block) onBlockRightClick(block, event);
  };

  const handleClick = (event: React.MouseEvent) => {
    if (event.button !== 0 || interactionTool === "hand" || interactionTool === "measure") return;
    const block = resolveBlockAtEvent(event);
    if (block) onBlockClick(block, event);
  };

  const handleMouseDown = (event: React.MouseEvent) => {
    if (event.button === 0 && interactionTool === "hand") {
      setIsDragging(true);
      setDragStart({ x: event.clientX - offsetRef.current.x, y: event.clientY - offsetRef.current.y });
      return;
    }

    if (event.button !== 0 || interactionTool !== "measure") return;
    const point = resolveMeasurementPointAtClient(event.clientX, event.clientY);
    if (!point) return;
    event.preventDefault();
    onHoverBlock(null, null);
    setMeasurementDraftValue({
      id: "__draft__",
      startX: point.x,
      startZ: point.z,
      endX: point.x,
      endZ: point.z,
    });
    setIsMeasuring(true);
  };

  const handleMouseUp = () => {
    setIsDragging(false);
    if (isMeasuring) finishMeasurementDraft();
  };

  return (
    <div
      ref={viewportRef}
      className={[
        "flake-canvas",
        isDragging ? "is-dragging" : "",
        interactionTool === "measure" ? "is-measure-tool" : "",
        isMeasuring ? "is-measuring" : "",
      ].filter(Boolean).join(" ")}
      onWheel={handleWheel}
      onMouseDown={handleMouseDown}
      onClick={handleClick}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      onMouseLeave={() => {
        setIsDragging(false);
        onHoverBlock(null, null);
      }}
      onContextMenu={handleContextMenu}
    >
      {scale < ICON_NATIVE_SIZE ? (
        <canvas
          ref={lowZoomCanvasRef}
          className="flake-layer-low-zoom-canvas"
          aria-hidden="true"
        />
      ) : null}
      <div
        ref={worldRef}
        className="flake-layer-world"
      >
        <div
          className="flake-layer-border"
          style={borderStyle}
        />
        {scale >= ICON_NATIVE_SIZE
          ? visibleBlocks.map((block) => (
            <div
              key={block.key}
              className="flake-layer-block"
              style={block.style}
            >
              {block.shouldRenderIcon ? <img className="flake-layer-block-icon" src={block.iconUrl} alt="" draggable={false} /> : null}
            </div>
          ))
          : null}
        {measurementOverlays.map((measurement) => (
          <div
            key={measurement.id}
            className={measurement.isDraft ? "flake-measurement-rect is-draft" : "flake-measurement-rect"}
            style={measurement.style}
          >
            <svg
              className="flake-measurement-svg"
              viewBox={`0 0 ${measurement.widthPx} ${measurement.heightPx}`}
              preserveAspectRatio="none"
              aria-hidden="true"
            >
              {measurement.showGrid ? (
                <path
                  className="flake-measurement-grid-line"
                  d={measurement.gridPath}
                  strokeDasharray={measurement.gridDash}
                />
              ) : null}
              <line
                className="flake-measurement-diagonal"
                x1={measurement.diagonalStartX}
                y1={measurement.diagonalStartY}
                x2={measurement.diagonalEndX}
                y2={measurement.diagonalEndY}
              />
            </svg>
            <span className="flake-measurement-label flake-measurement-label--x">
              {measurement.xLabel}
            </span>
            <span className="flake-measurement-label flake-measurement-label--z">
              {measurement.zLabel}
            </span>
            <span className="flake-measurement-label flake-measurement-label--manhattan">
              {measurement.manhattanLabel}
            </span>
            {measurement.showInsideLabel ? (
              <span className="flake-measurement-label flake-measurement-label--inside">
                {measurement.insideLabel}
              </span>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  );
});

function FlakeDebugStateDialog({
  block,
  onApply,
  onClose,
}: {
  block: FlakeHoverBlock;
  onApply: (stateRecord: Record<string, string>) => void;
  onClose: () => void;
}) {
  const [draftState, setDraftState] = useState<Record<string, string>>(() => ({ ...block.stateRecord }));
  const [selectedNoteScaleName, setSelectedNoteScaleName] = useState(DEFAULT_NOTE_SCALE_NAME);
  const propertyKeys = useMemo(() => Object.keys(block.stateRecord).sort(), [block.stateRecord]);
  const knownProperties = getBlockProperties(block.id);
  const selectedNoteScale = NOTE_SCALE_OPTIONS.find((option) => option.name === selectedNoteScaleName) || NOTE_SCALE_OPTIONS[0];

  useEffect(() => {
    setDraftState({ ...block.stateRecord });
  }, [block]);

  const setProperty = (key: string, value: string) => {
    setDraftState((current) => ({ ...current, [key]: value }));
  };

  return (
    <Dialog
      title="调试棒"
      subtitle={`${block.name} (${block.id})`}
      width="lg"
      className="flake-page__state-dialog"
      onClose={onClose}
      footer={(
        <>
          <button type="button" onClick={onClose}>取消</button>
          <button type="button" onClick={() => onApply(normalizeLayerStateRecord(draftState))}>应用</button>
        </>
      )}
    >
      <div className="dialog-form-grid flake-page__state-dialog-grid">
        <span className="dialog-label">位置</span>
        <span>x={block.x} y={block.y} z={block.z}</span>
        <span className="dialog-label">方块ID</span>
        <div className="dialog-path-preview">{block.id}</div>
        {propertyKeys.map((key) => {
          const knownValues = knownProperties[key] || [];
          const currentValue = draftState[key] ?? "";
          const isNoteBlockNote = block.id === NOTE_BLOCK_ID && key === "note";
          const optionValues = currentValue && !knownValues.includes(currentValue)
            ? [currentValue, ...knownValues]
            : knownValues;
          return (
            <React.Fragment key={key}>
              <label className="dialog-label" htmlFor={`flake-state-${key}`}>{translateKey(key)}</label>
              {isNoteBlockNote ? (
                <div id={`flake-state-${key}`} className="flake-page__note-control">
                  <select
                    className="input flake-page__note-scale-select"
                    value={selectedNoteScaleName}
                    aria-label="音符盒调式"
                    onChange={(event) => setSelectedNoteScaleName(event.target.value)}
                  >
                    {NOTE_SCALE_OPTIONS.map((option) => (
                      <option key={option.name} value={option.name}>{option.name}</option>
                    ))}
                  </select>
                  <div className="flake-page__note-keyboard" role="group" aria-label="音符盒音高">
                    {NOTE_BLOCK_KEY_ROWS.map((row) => (
                      <div
                        key={row.id}
                        className={`flake-page__note-key-row flake-page__note-key-row--${row.id}`}
                        role="group"
                        aria-label={row.label}
                      >
                        {row.values.map((value) => {
                          const noteLabel = selectedNoteScale?.labelsByValue[value] || parseNoteToneLabel(value, value);
                          return (
                            <span key={value} className="flake-page__note-key-slot">
                              <button
                                className={noteLabel.sharpMarks ? "btn flake-page__note-key-button is-sharp" : "btn flake-page__note-key-button"}
                                type="button"
                                aria-label={`${noteLabel.raw}，音高 ${value}`}
                                aria-pressed={currentValue === value}
                                title={`${noteLabel.raw} (${value})`}
                                onClick={() => setProperty(key, value)}
                              >
                                <span className="flake-page__note-key-label">{noteLabel.base}</span>
                                {noteLabel.sharpMarks ? <span className="flake-page__note-key-sharp" aria-hidden="true">{noteLabel.sharpMarks}</span> : null}
                              </button>
                            </span>
                          );
                        })}
                      </div>
                    ))}
                  </div>
                </div>
              ) : optionValues.length > 0 ? (
                <select
                  id={`flake-state-${key}`}
                  className="input"
                  value={currentValue}
                  onChange={(event) => setProperty(key, event.target.value)}
                >
                  {optionValues.map((value) => (
                    <option key={value} value={value}>{translateValue(key, value)} ({value})</option>
                  ))}
                </select>
              ) : (
                <input
                  id={`flake-state-${key}`}
                  className="input"
                  value={currentValue}
                  onChange={(event) => setProperty(key, event.target.value)}
                />
              )}
            </React.Fragment>
          );
        })}
      </div>
    </Dialog>
  );
}

/**
 * Renders the flake layer viewer and editing controls.
 */
export function FlakePage({ currentFile, setRoute }: any) {
  const [cacheFile, setCacheFile] = useState("");
  const [cacheStatus, setCacheStatus] = useState("idle");
  const [cacheExists, setCacheExists] = useState(false);
  const [meta, setMeta] = useState<LayerSliceMeta | null>(null);
  const [sliceDataByY, setSliceDataByY] = useState<Record<number, LayerSliceData | null>>({});
  const [statsData, setStatsData] = useState<StatsData | null>(null);
  const [layerY, setLayerY] = useState(0);
  const [onionSkinDepth, setOnionSkinDepth] = useState(0);
  const [viewScale, setViewScale] = useState(1);
  const [showStateHints, setShowStateHints] = useState(true);
  const [hoverBlock, setHoverBlock] = useState<FlakeHoverBlock | null>(null);
  const [hoverPos, setHoverPos] = useState({ x: 0, y: 0 });
  const [showMaterials, setShowMaterials] = useState(false);
  const [showContainerDialog, setShowContainerDialog] = useState(false);
  const [containerData, setContainerData] = useState<{ type: ContainerType; items: ContainerItem[]; position: { x: number; y: number; z: number } } | null>(null);
  const [isLoadingContainer, setIsLoadingContainer] = useState(false);
  const [editMode, setEditMode] = useState(false);
  const [selectedEditTool, setSelectedEditTool] = useState<EditToolId>("hand");
  const [isSpaceHandTool, setIsSpaceHandTool] = useState(false);
  const [editedBlockStates, setEditedBlockStates] = useState<Record<string, Record<string, string>>>({});
  const [measurementRects, setMeasurementRects] = useState<FlakeMeasurementRect[]>([]);
  const [debugStateBlock, setDebugStateBlock] = useState<FlakeHoverBlock | null>(null);
  const [showInventoryDialog, setShowInventoryDialog] = useState(false);
  const [materialBlocks, setMaterialBlocks] = useState<string[]>([]);
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
  const measurementIdRef = useRef(0);

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
    setMaterialBlocks([]);
    setEditedBlockStates({});
    setMeasurementRects([]);
    measurementIdRef.current = 0;
    setDebugStateBlock(null);
    syncCacheState();
  }, [currentFile]);

  useEffect(() => subscribeRenderCacheStore(syncCacheState), [currentFile]);

  useEffect(() => () => {
    stopQuickBuildPolling();
    handleToolSplitterPointerUp();
  }, []);

  // 打开创造模式物品栏时懒加载当前投影的材料方块列表
  useEffect(() => {
    if (!showInventoryDialog || !currentFile || materialBlocks.length > 0) return;
    loadMaterialsScope(currentFile, [])
      .then((mats) => setMaterialBlocks(mats.map((m) => m.id).filter(Boolean)))
      .catch(() => setMaterialBlocks([]));
  }, [showInventoryDialog, currentFile]);

  useEffect(() => {
    if (!editMode) {
      setShowInventoryDialog(false);
      setIsSpaceHandTool(false);
      setDebugStateBlock(null);
      return;
    }
    setSelectedEditTool("hand");
  }, [editMode]);

  useEffect(() => {
    if (!editMode || debugStateBlock) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.code !== "Space" || isTextEntryTarget(event.target)) return;
      event.preventDefault();
      setIsSpaceHandTool(true);
    };
    const handleKeyUp = (event: KeyboardEvent) => {
      if (event.code !== "Space") return;
      event.preventDefault();
      setIsSpaceHandTool(false);
    };
    const handleBlur = () => setIsSpaceHandTool(false);

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    window.addEventListener("blur", handleBlur);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      window.removeEventListener("blur", handleBlur);
    };
  }, [debugStateBlock, editMode]);

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

  const effectiveEditTool: EditToolId = editMode && !isSpaceHandTool ? selectedEditTool : "hand";

  const handleHoverBlock = useCallback((block: FlakeHoverBlock | null, event: React.MouseEvent | null) => {
    setHoverBlock(block);
    if (event) setHoverPos({ x: event.clientX, y: event.clientY });
  }, []);

  const handleBlockClick = useCallback((block: FlakeHoverBlock, event: React.MouseEvent) => {
    event.preventDefault();
    if (effectiveEditTool !== "debug-stick" || !block.hasStates) return;
    setDebugStateBlock(block);
  }, [effectiveEditTool]);

  const handleApplyDebugState = useCallback((stateRecord: Record<string, string>) => {
    if (!debugStateBlock) return;
    const normalizedStateRecord = normalizeLayerStateRecord(stateRecord);
    const positionKey = flakeBlockPositionKey(debugStateBlock.x, debugStateBlock.y, debugStateBlock.z);
    setEditedBlockStates((current) => ({
      ...current,
      [positionKey]: normalizedStateRecord,
    }));
    setHoverBlock((current) => {
      if (!current || flakeBlockPositionKey(current.x, current.y, current.z) !== positionKey) return current;
      return {
        ...current,
        stateRecord: normalizedStateRecord,
        states: layerStateRecordText(normalizedStateRecord),
      };
    });
    setDebugStateBlock(null);
  }, [debugStateBlock]);

  const handleAddMeasurement = useCallback((rect: Omit<FlakeMeasurementRect, "id">) => {
    measurementIdRef.current += 1;
    setMeasurementRects((current) => [
      ...current,
      {
        id: `measure-${measurementIdRef.current}`,
        ...rect,
      },
    ]);
  }, []);

  const handleDeleteMeasurementAt = useCallback((point: FlakeMeasurementPoint) => {
    setMeasurementRects((current) => {
      let targetIndex = -1;
      for (let index = current.length - 1; index >= 0; index -= 1) {
        if (!measurementRectContainsPoint(current[index], point)) continue;
        targetIndex = index;
        break;
      }
      if (targetIndex < 0) return current;
      return current.filter((_rect, index) => index !== targetIndex);
    });
  }, []);

  const handleBlockRightClick = useCallback(async (block: FlakeHoverBlock, event: React.MouseEvent) => {
    event.preventDefault();
    
    // 防止重复加载
    if (isLoadingContainer) {
      return;
    }

    // 检查是否为容器方块
    if (!isContainerBlock(block.id)) {
      return;
    }

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

      if (!data) {
        return;
      }
      
      const containerType = getContainerType(data.block_id);

      // 只支持已定义 UI 布局的容器类型
      if (!containerType) {
        return;
      }

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
      <div className="nova-page flake-page">
        <div className="subwindow-toolbar flake-page__topbar">
          <button className="btn flake-page__materials-button" onClick={() => {}}>
            材料列表
          </button>
          <button className="btn flake-page__build-button" disabled>
            生成3DCache
          </button>
          <label className="subwindow-check-row flake-page__edit-toggle">
            <input type="checkbox" disabled />
            编辑模式
          </label>
          <div className="subwindow-toolbar-spacer" />
        </div>
        <div className="properties-empty-state">请先选择一个 .litematic 文件</div>
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
          <input
            type="checkbox"
            checked={editMode}
            onChange={(event) => {
              const enabled = event.target.checked;
              if (enabled) setSelectedEditTool("hand");
              setEditMode(enabled);
            }}
          />
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

        <div className="flake-page__layer-row flake-page__zoom-row">
          <span className="nova-muted flake-page__layer-label">缩放</span>
          <input
            className="flake-page__layer-range"
            type="range"
            min={MIN_SCALE}
            max={MAX_SCALE}
            step={0.1}
            value={viewScale}
            onChange={(event) => canvasRef.current?.zoomTo(Number(event.target.value))}
            disabled={!ready}
          />
          <input
            className="input flake-page__zoom-input"
            type="number"
            min={MIN_SCALE}
            max={MAX_SCALE}
            step={0.1}
            value={Math.round(viewScale * 10) / 10}
            onChange={(event) => {
              const next = Number(event.target.value);
              if (Number.isFinite(next)) canvasRef.current?.zoomTo(next);
            }}
            disabled={!ready}
          />
          <span className="nova-muted nova-small flake-page__zoom-unit">像素/格</span>
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
              <LayerCanvas
                ref={canvasRef}
                meta={meta}
                renderSlices={renderSlices}
                showStateHints={showStateHints}
                interactionTool={effectiveEditTool}
                editedBlockStates={editedBlockStates}
                measurements={measurementRects}
                onHoverBlock={handleHoverBlock}
                onBlockClick={handleBlockClick}
                onBlockRightClick={handleBlockRightClick}
                onAddMeasurement={handleAddMeasurement}
                onDeleteMeasurementAt={handleDeleteMeasurementAt}
                onScaleChange={setViewScale}
              />
            )}

            <FlakeBlockTooltip
              x={hoverPos.x}
              y={hoverPos.y}
              item={hoverBlock}
              highlightEmptyState={effectiveEditTool === "debug-stick"}
            />
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
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">测量</span>
                    <span>{measurementRects.length} 个矩形</span>
                  </div>
                </div>

                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">状态</div>
                  <div className="nova-muted nova-small flake-page__status-text">{statusText}</div>
                  {quickBuildError ? <p className="nova-error flake-page__tool-panel-error">{quickBuildError}</p> : null}
                </div>

                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">编辑工具占位</div>
                  <div className="flake-page__tool-palette" role="toolbar" aria-label="编辑工具占位按钮">
                    {EDIT_TOOL_PLACEHOLDERS.map((tool) => (
                      <button
                        key={tool.id}
                        className="btn flake-page__edit-tool-button"
                        type="button"
                        onClick={() => setSelectedEditTool(tool.id)}
                        title={`${tool.name}：${tool.description}`}
                        aria-label={`${tool.name}，${tool.description}`}
                        aria-pressed={effectiveEditTool === tool.id}
                      >
                        <span className="flake-page__edit-tool-glyph" aria-hidden="true">{tool.glyph}</span>
                      </button>
                    ))}
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
          extraCollections={
            materialBlocks.length > 0
              ? [{ id: '__current__', name: '当前统计结果', values: materialBlocks }]
              : []
          }
        />
      ) : null}

      {showMaterials && statsData && <MaterialsDialog data={statsData} onClose={() => setShowMaterials(false)} currentFile={currentFile} />}
      {debugStateBlock ? (
        <FlakeDebugStateDialog
          key={flakeBlockPositionKey(debugStateBlock.x, debugStateBlock.y, debugStateBlock.z)}
          block={debugStateBlock}
          onApply={handleApplyDebugState}
          onClose={() => setDebugStateBlock(null)}
        />
      ) : null}
      
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


