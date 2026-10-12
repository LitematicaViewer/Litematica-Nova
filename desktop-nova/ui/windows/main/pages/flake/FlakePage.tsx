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
  loadProjectionMarkers,
  loadStructureStats,
  pollCacheBuildTask,
  type ProjectionMarkerPoint,
  removeProjectionOrigin,
  selectLitematicSavePath,
  setProjectionOrigin,
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
const LAYER_TILE_SIZE = 16;
const HOVER_TOOLTIP_MIN_SCALE = 8;
const RENDER_OVERSCAN_PIXELS = 256;
const LOW_ZOOM_OVERSCAN_PIXELS = 64;
const MAX_MAP_COLOR_TILE_CACHE_SIZE = 2048;
const MAX_TEXTURE_TILE_CACHE_SIZE = 192;
const TEXTURE_LOAD_IDLE_DELAY_MS = 160;
const TEXTURE_LOAD_CONCURRENCY = 3;
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
  { id: "marker", glyph: "标", name: "标记点工具", description: "在任意方格设定投影标记点" },
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

interface FlakeMarkerMenuState {
  clientX: number;
  clientY: number;
  point: ProjectionMarkerPoint;
  isOrigin: boolean;
}

type LayerTileRenderMode = "map-color" | "texture";

interface TileIconImageCacheEntry {
  image: HTMLImageElement;
  ready: boolean;
  failed: boolean;
}

interface LayerTileCache {
  mode: LayerTileRenderMode | null;
  meta: LayerSliceMeta | null;
  scale: number;
  devicePixelRatio: number;
  colorMap: Map<number, string> | null;
  tileSource: Map<string, VisibleLayerBlock[]> | null;
  iconImages: Map<number, string> | null;
  tileImageRevision: number;
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

function layerTileKey(tileX: number, tileZ: number): string {
  return `${tileX}:${tileZ}`;
}

function getMaxLayerTileCacheSize(mode: LayerTileRenderMode, scale: number): number {
  if (mode === "map-color") return MAX_MAP_COLOR_TILE_CACHE_SIZE;
  if (scale >= 48) return 48;
  if (scale >= 32) return 96;
  return MAX_TEXTURE_TILE_CACHE_SIZE;
}

function resolveReadyTileIconImage(
  iconUrl: string,
  imageCache: Map<string, TileIconImageCacheEntry>,
  onReady: () => void,
): HTMLImageElement | null {
  let cacheEntry = imageCache.get(iconUrl);
  if (!cacheEntry) {
    const image = new Image();
    cacheEntry = {
      image,
      ready: false,
      failed: false,
    };
    image.onload = () => {
      cacheEntry!.ready = true;
      onReady();
    };
    image.onerror = () => {
      cacheEntry!.failed = true;
      onReady();
    };
    image.src = iconUrl;
    imageCache.set(iconUrl, cacheEntry);
  }
  return cacheEntry.ready && !cacheEntry.failed ? cacheEntry.image : null;
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

function markerPointsEqual(left: ProjectionMarkerPoint | null | undefined, right: ProjectionMarkerPoint | null | undefined): boolean {
  return !!left
    && !!right
    && left.x === right.x
    && left.y === right.y
    && left.z === right.z;
}

function markerPointsChanged(left: ProjectionMarkerPoint | null | undefined, right: ProjectionMarkerPoint | null | undefined): boolean {
  if (!left && !right) return false;
  if (!left || !right) return true;
  return !markerPointsEqual(left, right);
}

function ensureLitematicExtension(path: string): string {
  const trimmed = path.trim();
  if (!trimmed) return trimmed;
  return /\.litematic$/i.test(trimmed) ? trimmed : `${trimmed}.litematic`;
}

function markerSaveAsDefaultPath(currentFile: string): string {
  if (!currentFile) return "edited.litematic";
  const replaced = currentFile.replace(/\.litematic$/i, ".edited.litematic");
  return replaced === currentFile ? `${currentFile}.edited.litematic` : replaced;
}

function pathsLookSame(left: string, right: string): boolean {
  return left.replace(/\//g, "\\").toLocaleLowerCase() === right.replace(/\//g, "\\").toLocaleLowerCase();
}

function formatMeasurementDistance(value: number): string {
  if (Number.isInteger(value)) return String(value);
  return value < 10 ? value.toFixed(2) : value.toFixed(1);
}

function buildLayerTile(
  tileBlocks: VisibleLayerBlock[],
  tileX: number,
  tileZ: number,
  meta: LayerSliceMeta,
  scale: number,
  devicePixelRatio: number,
  colorMap: Map<number, string>,
  iconImages: Map<number, string> | null,
  imageCache: Map<string, TileIconImageCacheEntry>,
  onIconReady: () => void,
): { canvas: HTMLCanvasElement; width: number; height: number } {
  const originX = tileX * LAYER_TILE_SIZE;
  const originZ = tileZ * LAYER_TILE_SIZE;
  const width = Math.max(1, Math.ceil(Math.min(LAYER_TILE_SIZE, meta.size_x - originX) * scale));
  const height = Math.max(1, Math.ceil(Math.min(LAYER_TILE_SIZE, meta.size_z - originZ) * scale));
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
    const iconUrl = iconImages?.get(visibleBlock.block.palette_id);
    const iconImage = iconUrl ? resolveReadyTileIconImage(iconUrl, imageCache, onIconReady) : null;
    context.globalAlpha = visibleBlock.opacity;
    if (iconImage) {
      context.drawImage(iconImage, left, top, right - left, bottom - top);
    } else {
      context.fillStyle = color;
      context.fillRect(left, top, right - left, bottom - top);
    }
  }
  context.globalAlpha = 1;

  return { canvas, width, height };
}

const LayerCanvas = forwardRef<
  LayerCanvasHandle,
  {
    meta: LayerSliceMeta | null;
    layerY: number;
    renderSlices: FlakeRenderSlice[];
    showStateHints: boolean;
    interactionTool: EditToolId;
    editedBlockStates: Record<string, Record<string, string>>;
    originMarker: ProjectionMarkerPoint | null;
    measurements: FlakeMeasurementRect[];
    onHoverBlock: (block: FlakeHoverBlock | null, event: React.MouseEvent | null) => void;
    onBlockClick: (block: FlakeHoverBlock, event: React.MouseEvent) => void;
    onBlockRightClick: (block: FlakeHoverBlock, event: React.MouseEvent) => void;
    onMarkerContextMenu: (point: ProjectionMarkerPoint, isOrigin: boolean, event: React.MouseEvent) => void;
    onOriginChange: (point: ProjectionMarkerPoint) => void;
    onAddMeasurement: (rect: Omit<FlakeMeasurementRect, "id">) => void;
    onDeleteMeasurementAt: (point: FlakeMeasurementPoint) => void;
    onScaleChange?: (scale: number) => void;
  }
>(({ meta, layerY, renderSlices, showStateHints, interactionTool, editedBlockStates, originMarker, measurements, onHoverBlock, onBlockClick, onBlockRightClick, onMarkerContextMenu, onOriginChange, onAddMeasurement, onDeleteMeasurementAt, onScaleChange }, ref) => {
  const viewportRef = useRef<HTMLDivElement>(null);
  const worldRef = useRef<HTMLDivElement>(null);
  const layerTileCanvasRef = useRef<HTMLCanvasElement>(null);
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const [isMeasuring, setIsMeasuring] = useState(false);
  const [measurementDraft, setMeasurementDraft] = useState<FlakeMeasurementRect | null>(null);
  const [originDragDraft, setOriginDragDraft] = useState<ProjectionMarkerPoint | null>(null);
  const [isDraggingOrigin, setIsDraggingOrigin] = useState(false);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const [iconImages, setIconImages] = useState<Map<number, string>>(new Map());
  const [tileImageRevision, setTileImageRevision] = useState(0);
  const [isTextureInteractionActive, setIsTextureInteractionActive] = useState(false);
  const [mapColorByBlockId, setMapColorByBlockId] = useState<Map<string, string>>(new Map());
  const [viewportSize, setViewportSize] = useState({ width: 0, height: 0 });
  const dragFrameRef = useRef<number | null>(null);
  const pendingOffsetRef = useRef(offset);
  const measurementDraftRef = useRef<FlakeMeasurementRect | null>(null);
  const originDragDraftRef = useRef<ProjectionMarkerPoint | null>(null);
  const originMarkerRef = useRef<ProjectionMarkerPoint | null>(originMarker);
  const lastHoverBlockRef = useRef<FlakeHoverBlock | null>(null);
  const iconImagesRef = useRef(iconImages);
  const isTextureInteractionActiveRef = useRef(isTextureInteractionActive);
  const textureInteractionTimerRef = useRef<number | null>(null);
  const textureJobVersionRef = useRef(0);
  const tileIconImageCacheRef = useRef(new Map<string, TileIconImageCacheEntry>());
  const tileImageFrameRef = useRef<number | null>(null);
  const layerTileCacheRef = useRef<LayerTileCache>({
    mode: null,
    meta: null,
    scale: 0,
    devicePixelRatio: 0,
    colorMap: null,
    tileSource: null,
    iconImages: null,
    tileImageRevision: 0,
    tiles: new Map(),
  });

  // 用 ref 镜像最新的 scale / offset，供事件处理和命令式缩放共用，避免闭包读到旧值。
  const scaleRef = useRef(scale);
  const offsetRef = useRef(offset);
  scaleRef.current = scale;
  offsetRef.current = offset;
  originMarkerRef.current = originMarker;

  const scheduleOffset = useCallback((nextOffset: { x: number; y: number }) => {
    pendingOffsetRef.current = nextOffset;
    if (dragFrameRef.current !== null) return;
    dragFrameRef.current = window.requestAnimationFrame(() => {
      dragFrameRef.current = null;
      setOffset(pendingOffsetRef.current);
    });
  }, []);

  const scheduleTileImageRevision = useCallback(() => {
    if (tileImageFrameRef.current !== null) return;
    tileImageFrameRef.current = window.requestAnimationFrame(() => {
      tileImageFrameRef.current = null;
      setTileImageRevision((current) => current + 1);
    });
  }, []);

  // 通知父组件当前缩放，用于同步滑块和输入框。用 ref 保存回调避免因引用变化触发多余 effect。
  const onScaleChangeRef = useRef(onScaleChange);
  onScaleChangeRef.current = onScaleChange;
  useEffect(() => {
    onScaleChangeRef.current?.(scale);
  }, [scale]);

  useEffect(() => {
    iconImagesRef.current = iconImages;
  }, [iconImages]);

  const setMeasurementDraftValue = useCallback((nextDraft: FlakeMeasurementRect | null) => {
    measurementDraftRef.current = nextDraft;
    setMeasurementDraft(nextDraft);
  }, []);

  const updateMeasurementDraftValue = useCallback((updater: (current: FlakeMeasurementRect | null) => FlakeMeasurementRect | null) => {
    setMeasurementDraftValue(updater(measurementDraftRef.current));
  }, [setMeasurementDraftValue]);

  const setOriginDragDraftValue = useCallback((nextDraft: ProjectionMarkerPoint | null) => {
    originDragDraftRef.current = nextDraft;
    setOriginDragDraft(nextDraft);
  }, []);

  const markTextureInteraction = useCallback(() => {
    if (!isTextureInteractionActiveRef.current) {
      isTextureInteractionActiveRef.current = true;
      setIsTextureInteractionActive(true);
    }
    if (textureInteractionTimerRef.current !== null) {
      window.clearTimeout(textureInteractionTimerRef.current);
    }
    textureInteractionTimerRef.current = window.setTimeout(() => {
      textureInteractionTimerRef.current = null;
      isTextureInteractionActiveRef.current = false;
      setIsTextureInteractionActive(false);
    }, TEXTURE_LOAD_IDLE_DELAY_MS);
  }, []);

  const emitHoverBlock = useCallback((block: FlakeHoverBlock | null, event: React.MouseEvent | null) => {
    lastHoverBlockRef.current = block;
    onHoverBlock(block, event);
  }, [onHoverBlock]);

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

  const finishOriginDrag = useCallback(() => {
    const draft = originDragDraftRef.current;
    const original = originMarkerRef.current;
    setIsDraggingOrigin(false);
    setOriginDragDraftValue(null);
    if (!draft || !original || markerPointsEqual(draft, original)) return;
    onOriginChange(draft);
  }, [onOriginChange, setOriginDragDraftValue]);

  useEffect(() => {
    if (interactionTool === "measure" && meta) return;
    setIsMeasuring(false);
    setMeasurementDraftValue(null);
  }, [interactionTool, meta, setMeasurementDraftValue]);

  useEffect(() => {
    if (interactionTool === "marker" && meta) return;
    setIsDraggingOrigin(false);
    setOriginDragDraftValue(null);
  }, [interactionTool, meta, setOriginDragDraftValue]);

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
    if (!isDraggingOrigin) return;

    const handleMouseMove = (event: MouseEvent) => {
      const original = originMarkerRef.current;
      if (!original) return;
      const point = resolveMeasurementPointAtClient(event.clientX, event.clientY, true);
      if (!point) return;
      setOriginDragDraftValue({
        x: point.x,
        y: original.y,
        z: point.z,
      });
    };
    const handleMouseUp = (event: MouseEvent) => {
      if (event.button !== 0) return;
      event.preventDefault();
      finishOriginDrag();
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [finishOriginDrag, isDraggingOrigin, resolveMeasurementPointAtClient, setOriginDragDraftValue]);

  useEffect(() => {
    if (scale < HOVER_TOOLTIP_MIN_SCALE) {
      emitHoverBlock(null, null);
    }
  }, [emitHoverBlock, scale]);

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
    if (tileImageFrameRef.current !== null) {
      window.cancelAnimationFrame(tileImageFrameRef.current);
    }
    if (textureInteractionTimerRef.current !== null) {
      window.clearTimeout(textureInteractionTimerRef.current);
    }
    textureJobVersionRef.current += 1;
  }, []);

  // 围绕锚点 (anchorX, anchorY，均相对视口左上角) 将缩放设置为 nextScale。
  const applyZoom = (rawScale: number, anchorX: number, anchorY: number) => {
    const nextScale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, rawScale));
    const currentScale = scaleRef.current;
    if (nextScale === currentScale) return;
    markTextureInteraction();
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

        const tileKey = layerTileKey(
          Math.floor(block.x / LAYER_TILE_SIZE),
          Math.floor(block.z / LAYER_TILE_SIZE),
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

    const centerX = (visibleBlockWindow.minX + visibleBlockWindow.maxX) / 2;
    const centerZ = (visibleBlockWindow.minZ + visibleBlockWindow.maxZ) / 2;
    const palettePriority = new Map<number, number>();
    for (let z = visibleBlockWindow.minZ; z <= visibleBlockWindow.maxZ; z += 1) {
      const row = visibleLayerIndex.rows.get(z);
      if (!row) continue;

      for (let index = findFirstBlockAtOrAfterX(row, visibleBlockWindow.minX); index < row.length; index += 1) {
        const visibleBlock = row[index];
        if (visibleBlock.block.x > visibleBlockWindow.maxX) break;

        const paletteId = visibleBlock.block.palette_id;
        const entry = meta.palette[paletteId];
        if (!entry?.block_id || normalizeFlakeBlockId(entry.block_id).includes("air")) continue;
        const dx = visibleBlock.block.x - centerX;
        const dz = visibleBlock.block.z - centerZ;
        const priority = dx * dx + dz * dz;
        const currentPriority = palettePriority.get(paletteId);
        if (currentPriority === undefined || priority < currentPriority) {
          palettePriority.set(paletteId, priority);
        }
      }
    }
    return Array.from(palettePriority.entries())
      .sort((left, right) => left[1] - right[1])
      .map(([paletteId]) => paletteId);
  }, [meta, scale, visibleBlockWindow, visibleLayerIndex]);

  useLayoutEffect(() => {
    const tileCache = layerTileCacheRef.current;
    if (!meta || !visibleBlockWindow) {
      tileCache.tiles.clear();
      tileCache.mode = null;
      tileCache.meta = null;
      tileCache.scale = 0;
      tileCache.devicePixelRatio = 0;
      tileCache.colorMap = null;
      tileCache.tileSource = null;
      tileCache.iconImages = null;
      tileCache.tileImageRevision = 0;
      const canvas = layerTileCanvasRef.current;
      const context = canvas?.getContext("2d");
      if (canvas && context) {
        context.setTransform(1, 0, 0, 1, 0, 0);
        context.clearRect(0, 0, canvas.width, canvas.height);
      }
      return;
    }

    const canvas = layerTileCanvasRef.current;
    const viewport = viewportRef.current;
    if (!canvas || !viewport) return;

    const width = Math.max(1, viewportSize.width || viewport.clientWidth || 600);
    const height = Math.max(1, viewportSize.height || viewport.clientHeight || 600);
    const devicePixelRatio = Math.max(1, window.devicePixelRatio || 1);
    const renderMode: LayerTileRenderMode = scale < ICON_NATIVE_SIZE ? "map-color" : "texture";
    const tileIconImages = renderMode === "texture" ? iconImages : null;
    const activeTileImageRevision = renderMode === "texture" ? tileImageRevision : 0;
    if (
      tileCache.mode !== renderMode
      || tileCache.meta !== meta
      || tileCache.scale !== scale
      || tileCache.devicePixelRatio !== devicePixelRatio
      || tileCache.colorMap !== colorMap
      || tileCache.tileSource !== visibleLayerIndex.tiles
      || tileCache.iconImages !== tileIconImages
      || tileCache.tileImageRevision !== activeTileImageRevision
    ) {
      tileCache.tiles.clear();
      tileCache.mode = renderMode;
      tileCache.meta = meta;
      tileCache.scale = scale;
      tileCache.devicePixelRatio = devicePixelRatio;
      tileCache.colorMap = colorMap;
      tileCache.tileSource = visibleLayerIndex.tiles;
      tileCache.iconImages = tileIconImages;
      tileCache.tileImageRevision = activeTileImageRevision;
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

    const minTileX = Math.floor(visibleBlockWindow.minX / LAYER_TILE_SIZE);
    const maxTileX = Math.floor(visibleBlockWindow.maxX / LAYER_TILE_SIZE);
    const minTileZ = Math.floor(visibleBlockWindow.minZ / LAYER_TILE_SIZE);
    const maxTileZ = Math.floor(visibleBlockWindow.maxZ / LAYER_TILE_SIZE);
    const maxCacheSize = getMaxLayerTileCacheSize(renderMode, scale);
    for (let tileZ = minTileZ; tileZ <= maxTileZ; tileZ += 1) {
      for (let tileX = minTileX; tileX <= maxTileX; tileX += 1) {
        const tileKey = layerTileKey(tileX, tileZ);
        const tileBlocks = visibleLayerIndex.tiles.get(tileKey);
        if (!tileBlocks) continue;

        let tile = tileCache.tiles.get(tileKey);
        if (!tile) {
          const built = buildLayerTile(
            tileBlocks,
            tileX,
            tileZ,
            meta,
            scale,
            devicePixelRatio,
            colorMap,
            tileIconImages,
            tileIconImageCacheRef.current,
            scheduleTileImageRevision,
          );
          tile = built.canvas;
          tileCache.tiles.set(tileKey, tile);
          if (tileCache.tiles.size > maxCacheSize) {
            const oldestKey = tileCache.tiles.keys().next().value;
            if (oldestKey) tileCache.tiles.delete(oldestKey);
          }
        }

        const originX = tileX * LAYER_TILE_SIZE;
        const originZ = tileZ * LAYER_TILE_SIZE;
        const tileWidth = Math.max(1, Math.ceil(Math.min(LAYER_TILE_SIZE, meta.size_x - originX) * scale));
        const tileHeight = Math.max(1, Math.ceil(Math.min(LAYER_TILE_SIZE, meta.size_z - originZ) * scale));
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
    iconImages,
    meta,
    offset.x,
    offset.y,
    scale,
    scheduleTileImageRevision,
    tileImageRevision,
    visibleBlockWindow,
    visibleLayerIndex.tiles,
    viewportSize.height,
    viewportSize.width,
  ]);

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

  const originMarkerOverlay = useMemo(() => {
    const marker = originDragDraft || originMarker;
    if (!meta || !marker || marker.y !== layerY) return null;
    if (marker.x < 0 || marker.x >= meta.size_x || marker.z < 0 || marker.z >= meta.size_z) return null;
    const leftPx = Math.round(marker.x * scale);
    const topPx = Math.round(marker.z * scale);
    const rightPx = Math.max(leftPx + 1, Math.round((marker.x + 1) * scale));
    const bottomPx = Math.max(topPx + 1, Math.round((marker.z + 1) * scale));
    return {
      style: {
        "--flake-origin-marker-left": `${leftPx}px`,
        "--flake-origin-marker-top": `${topPx}px`,
        "--flake-origin-marker-width": `${rightPx - leftPx}px`,
        "--flake-origin-marker-height": `${bottomPx - topPx}px`,
      } as CssVariableStyle<
        | "--flake-origin-marker-left"
        | "--flake-origin-marker-top"
        | "--flake-origin-marker-width"
        | "--flake-origin-marker-height"
      >,
    };
  }, [layerY, meta, originDragDraft, originMarker, scale]);

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
    textureJobVersionRef.current += 1;
    const emptyImages = new Map<number, string>();
    iconImagesRef.current = emptyImages;
    tileIconImageCacheRef.current.clear();
    setTileImageRevision((current) => current + 1);
    setIconImages(emptyImages);
  }, [meta, showStateHints]);

  useEffect(() => {
    let cancelled = false;

    if (
      !meta
      || scale < ICON_NATIVE_SIZE
      || isDragging
      || isTextureInteractionActive
      || slicePaletteIds.length === 0
    ) {
      return () => {
        cancelled = true;
      };
    }

    const pendingPaletteIds = slicePaletteIds.filter((paletteId) => !iconImagesRef.current.has(paletteId));
    if (!pendingPaletteIds.length) {
      return () => {
        cancelled = true;
      };
    }

    const jobVersion = ++textureJobVersionRef.current;
    let cursor = 0;
    const workerCount = Math.min(TEXTURE_LOAD_CONCURRENCY, pendingPaletteIds.length);

    const runWorker = async () => {
      while (!cancelled && textureJobVersionRef.current === jobVersion) {
        const paletteId = pendingPaletteIds[cursor];
        cursor += 1;
        if (paletteId === undefined) return;
        if (iconImagesRef.current.has(paletteId)) continue;

        const entry = meta.palette[paletteId];
        if (!entry?.block_id) continue;
        
        try {
          const dataUrl = await getCachedIconImage(
            entry.block_id,
            entry,
            meta.property_pool || [],
            showStateHints
          );
          if (!dataUrl || cancelled || textureJobVersionRef.current !== jobVersion) continue;
          setIconImages((current) => {
            if (cancelled || textureJobVersionRef.current !== jobVersion || current.has(paletteId)) return current;
            const next = new Map(current);
            next.set(paletteId, dataUrl);
            iconImagesRef.current = next;
            return next;
          });
          await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
        } catch (error) {
          console.error(`Failed to resolve icon for palette ${paletteId}:`, error);
        }
      }
    };

    void Promise.all(Array.from({ length: workerCount }, () => runWorker()));

    return () => {
      cancelled = true;
    };
  }, [isDragging, isTextureInteractionActive, meta, scale, slicePaletteIds, showStateHints]);

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
    if (isDraggingOrigin) {
      emitHoverBlock(null, null);
      return;
    }

    if (isMeasuring) {
      emitHoverBlock(null, null);
      return;
    }

    if (isDragging) {
      markTextureInteraction();
      if (scale >= HOVER_TOOLTIP_MIN_SCALE && lastHoverBlockRef.current) {
        emitHoverBlock(lastHoverBlockRef.current, event);
      }
      scheduleOffset({ x: event.clientX - dragStart.x, y: event.clientY - dragStart.y });
      return;
    }

    if (scale < HOVER_TOOLTIP_MIN_SCALE) {
      emitHoverBlock(null, null);
      return;
    }

    emitHoverBlock(resolveBlockAtEvent(event), event);
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

    if (interactionTool === "marker") {
      const gridPoint = resolveMeasurementPointAtClient(event.clientX, event.clientY);
      if (gridPoint) {
        const point = { x: gridPoint.x, y: layerY, z: gridPoint.z };
        onMarkerContextMenu(point, markerPointsEqual(originMarker, point), event);
      }
      return;
    }

    const block = resolveBlockAtEvent(event);
    if (block) onBlockRightClick(block, event);
  };

  const handleClick = (event: React.MouseEvent) => {
    if (event.button !== 0 || interactionTool === "hand" || interactionTool === "measure" || interactionTool === "marker") return;
    const block = resolveBlockAtEvent(event);
    if (block) onBlockClick(block, event);
  };

  const handleMouseDown = (event: React.MouseEvent) => {
    if (event.button === 0 && interactionTool === "marker" && originMarker?.y === layerY) {
      const point = resolveMeasurementPointAtClient(event.clientX, event.clientY);
      if (point && point.x === originMarker.x && point.z === originMarker.z) {
        event.preventDefault();
        emitHoverBlock(null, null);
        setIsDraggingOrigin(true);
        setOriginDragDraftValue({ ...originMarker });
        return;
      }
    }

    if (event.button === 0 && interactionTool === "hand") {
      markTextureInteraction();
      setIsDragging(true);
      setDragStart({ x: event.clientX - offsetRef.current.x, y: event.clientY - offsetRef.current.y });
      return;
    }

    if (event.button !== 0 || interactionTool !== "measure") return;
    const point = resolveMeasurementPointAtClient(event.clientX, event.clientY);
    if (!point) return;
    event.preventDefault();
    emitHoverBlock(null, null);
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
    if (isDraggingOrigin) finishOriginDrag();
  };

  return (
    <div
      ref={viewportRef}
      className={[
        "flake-canvas",
        isDragging ? "is-dragging" : "",
        interactionTool === "measure" ? "is-measure-tool" : "",
        interactionTool === "marker" ? "is-marker-tool" : "",
        isMeasuring ? "is-measuring" : "",
        isDraggingOrigin ? "is-dragging-marker" : "",
      ].filter(Boolean).join(" ")}
      onWheel={handleWheel}
      onMouseDown={handleMouseDown}
      onClick={handleClick}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      onMouseLeave={() => {
        setIsDragging(false);
        emitHoverBlock(null, null);
      }}
      onContextMenu={handleContextMenu}
    >
      <canvas
        ref={layerTileCanvasRef}
        className="flake-layer-tile-canvas"
        aria-hidden="true"
      />
      <div
        ref={worldRef}
        className="flake-layer-world"
      >
        <div
          className="flake-layer-border"
          style={borderStyle}
        />
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
        {originMarkerOverlay ? (
          <div
            className={isDraggingOrigin ? "flake-origin-marker is-dragging" : "flake-origin-marker"}
            style={originMarkerOverlay.style}
            aria-hidden="true"
          />
        ) : null}
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
  const [originMarker, setOriginMarker] = useState<ProjectionMarkerPoint | null>(null);
  const [draftOriginMarker, setDraftOriginMarker] = useState<ProjectionMarkerPoint | null>(null);
  const [hasDraftOriginMarker, setHasDraftOriginMarker] = useState(false);
  const [isSavingEdits, setIsSavingEdits] = useState(false);
  const [markerMenu, setMarkerMenu] = useState<FlakeMarkerMenuState | null>(null);
  const [markerStatus, setMarkerStatus] = useState("");
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
  const markerMenuRef = useRef<HTMLDivElement | null>(null);
  const resizeStateRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const pollIntervalRef = useRef<number | null>(null);
  const syncRequestIdRef = useRef(0);
  const inFlightMetaKeyRef = useRef("");
  const loadedMetaKeyRef = useRef("");
  const inFlightSliceKeysRef = useRef(new Set<string>());
  const loadedSliceKeysRef = useRef(new Set<string>());
  const measurementIdRef = useRef(0);
  const markerRequestIdRef = useRef(0);

  const stopQuickBuildPolling = () => {
    if (pollIntervalRef.current !== null) {
      window.clearInterval(pollIntervalRef.current);
      pollIntervalRef.current = null;
    }
  };

  const refreshProjectionMarkers = useCallback(async () => {
    if (!currentFile) {
      setOriginMarker(null);
      setDraftOriginMarker(null);
      setHasDraftOriginMarker(false);
      return;
    }
    const requestId = ++markerRequestIdRef.current;
    const markers = await loadProjectionMarkers(currentFile);
    if (requestId !== markerRequestIdRef.current) return;
    setOriginMarker(markers.origin);
    setDraftOriginMarker(null);
    setHasDraftOriginMarker(false);
  }, [currentFile]);

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
    setOriginMarker(null);
    setDraftOriginMarker(null);
    setHasDraftOriginMarker(false);
    setMarkerMenu(null);
    setMarkerStatus("");
    measurementIdRef.current = 0;
    setDebugStateBlock(null);
    syncCacheState();
    refreshProjectionMarkers();
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
      setMarkerMenu(null);
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
  const effectiveOriginMarker = hasDraftOriginMarker ? draftOriginMarker : originMarker;
  const hasUnsavedMarkerEdits = hasDraftOriginMarker && markerPointsChanged(originMarker, draftOriginMarker);

  useEffect(() => {
    if (effectiveEditTool === "marker") return;
    setMarkerMenu(null);
  }, [effectiveEditTool]);

  useEffect(() => {
    if (!markerMenu) return;
    const closeMenu = () => setMarkerMenu(null);
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeMenu();
    };
    window.addEventListener("mousedown", closeMenu);
    window.addEventListener("wheel", closeMenu, { passive: true });
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("mousedown", closeMenu);
      window.removeEventListener("wheel", closeMenu);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [markerMenu]);

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

  const handleStageOriginMarker = useCallback((point: ProjectionMarkerPoint) => {
    setMarkerMenu(null);
    if (!currentFile) return;
    if (!markerPointsChanged(originMarker, point)) {
      setDraftOriginMarker(null);
      setHasDraftOriginMarker(false);
      setMarkerStatus("原点未改变。");
      return;
    }
    setDraftOriginMarker(point);
    setHasDraftOriginMarker(true);
    setMarkerStatus(`原点变更已暂存：x=${point.x} y=${point.y} z=${point.z}。点击“保存”后写入文件。`);
  }, [currentFile, originMarker]);

  const handleRemoveOriginMarker = useCallback(() => {
    setMarkerMenu(null);
    if (!currentFile) return;
    if (!markerPointsChanged(originMarker, null)) {
      setDraftOriginMarker(null);
      setHasDraftOriginMarker(false);
      setMarkerStatus("当前文件没有原点标记。");
      return;
    }
    setDraftOriginMarker(null);
    setHasDraftOriginMarker(true);
    setMarkerStatus("移除原点已暂存。点击“保存”后写入文件。");
  }, [currentFile, originMarker]);

  const handleShiftOriginMarker = useCallback((deltaY: number) => {
    if (!effectiveOriginMarker || !meta) return;
    const nextY = effectiveOriginMarker.y + deltaY;
    if (nextY < 0 || nextY >= meta.size_y) {
      setMarkerMenu(null);
      setMarkerStatus("原点已在投影边界层");
      return;
    }
    handleStageOriginMarker({ ...effectiveOriginMarker, y: nextY });
  }, [effectiveOriginMarker, handleStageOriginMarker, meta]);

  const handleSaveMarkerEdits = useCallback(async () => {
    if (!currentFile || !hasUnsavedMarkerEdits || isSavingEdits) return;
    setMarkerMenu(null);
    setIsSavingEdits(true);
    setMarkerStatus("正在保存标记...");
    try {
      const summary = draftOriginMarker
        ? await setProjectionOrigin(currentFile, draftOriginMarker)
        : await removeProjectionOrigin(currentFile);
      setOriginMarker(summary.origin);
      setDraftOriginMarker(null);
      setHasDraftOriginMarker(false);
      setStatsData(null);
      setMarkerStatus(summary.origin
        ? `原点已保存：x=${summary.origin.x} y=${summary.origin.y} z=${summary.origin.z}`
        : "原点移除已保存。");
    } catch (error) {
      setMarkerStatus(`标记保存失败：${String(error)}`);
      await refreshProjectionMarkers();
    } finally {
      setIsSavingEdits(false);
    }
  }, [currentFile, draftOriginMarker, hasUnsavedMarkerEdits, isSavingEdits, refreshProjectionMarkers]);

  const handleSaveMarkerEditsAs = useCallback(async () => {
    if (!currentFile || !hasUnsavedMarkerEdits || isSavingEdits) return;
    const selected = await selectLitematicSavePath(markerSaveAsDefaultPath(currentFile));
    if (!selected) return;
    const outputPath = ensureLitematicExtension(selected);
    if (!outputPath) return;

    setMarkerMenu(null);
    setIsSavingEdits(true);
    setMarkerStatus("正在另存标记...");
    try {
      const summary = draftOriginMarker
        ? await setProjectionOrigin(currentFile, draftOriginMarker, outputPath)
        : await removeProjectionOrigin(currentFile, outputPath);
      const savedPath = summary.output || outputPath;
      if (pathsLookSame(savedPath, currentFile)) {
        setOriginMarker(summary.origin);
        setDraftOriginMarker(null);
        setHasDraftOriginMarker(false);
        setStatsData(null);
        setMarkerStatus(summary.origin
          ? `原点已保存：x=${summary.origin.x} y=${summary.origin.y} z=${summary.origin.z}`
          : "原点移除已保存。");
      } else {
        setMarkerStatus(`已另存为：${savedPath}\n当前文件仍保留未保存标记。`);
      }
    } catch (error) {
      setMarkerStatus(`标记另存失败：${String(error)}`);
    } finally {
      setIsSavingEdits(false);
    }
  }, [currentFile, draftOriginMarker, hasUnsavedMarkerEdits, isSavingEdits]);

  const handleRestoreMarkerEdits = useCallback(async () => {
    if (!currentFile || isSavingEdits) return;
    setMarkerMenu(null);
    setMarkerStatus("正在还原标记...");
    try {
      await refreshProjectionMarkers();
      setMarkerStatus("标记已还原到当前文件状态。");
    } catch (error) {
      setMarkerStatus(`标记还原失败：${String(error)}`);
    }
  }, [currentFile, isSavingEdits, refreshProjectionMarkers]);

  const handleMarkerContextMenu = useCallback((point: ProjectionMarkerPoint, isOrigin: boolean, event: React.MouseEvent) => {
    event.preventDefault();
    const menuWidth = 184;
    const menuHeight = isOrigin ? 196 : 84;
    setMarkerMenu({
      clientX: Math.max(8, Math.min(event.clientX, window.innerWidth - menuWidth - 8)),
      clientY: Math.max(8, Math.min(event.clientY, window.innerHeight - menuHeight - 8)),
      point,
      isOrigin,
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

  useLayoutEffect(() => {
    const menu = markerMenuRef.current;
    if (!menu || !markerMenu) return;
    menu.style.setProperty("--flake-marker-menu-left", `${markerMenu.clientX}px`);
    menu.style.setProperty("--flake-marker-menu-top", `${markerMenu.clientY}px`);
  }, [markerMenu]);

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
                layerY={layerY}
                renderSlices={renderSlices}
                showStateHints={showStateHints}
                interactionTool={effectiveEditTool}
                editedBlockStates={editedBlockStates}
                originMarker={effectiveOriginMarker}
                measurements={measurementRects}
                onHoverBlock={handleHoverBlock}
                onBlockClick={handleBlockClick}
                onBlockRightClick={handleBlockRightClick}
                onMarkerContextMenu={handleMarkerContextMenu}
                onOriginChange={handleStageOriginMarker}
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
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">标记</span>
                    <span>
                      {effectiveOriginMarker
                        ? `原点 x=${effectiveOriginMarker.x} y=${effectiveOriginMarker.y} z=${effectiveOriginMarker.z}${hasUnsavedMarkerEdits ? "（未保存）" : ""}`
                        : hasUnsavedMarkerEdits ? "无（未保存）" : "无"}
                    </span>
                  </div>
                </div>

                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">状态</div>
                  <div className="nova-muted nova-small flake-page__status-text">{statusText}</div>
                  {markerStatus ? <div className="nova-muted nova-small flake-page__status-text">{markerStatus}</div> : null}
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
                  <button className="btn" type="button" onClick={handleSaveMarkerEdits} disabled={!editMode || !hasUnsavedMarkerEdits || isSavingEdits}>
                    {isSavingEdits ? "保存中..." : "保存"}
                  </button>
                  <button className="btn" type="button" onClick={handleSaveMarkerEditsAs} disabled={!editMode || !hasUnsavedMarkerEdits || isSavingEdits}>
                    另存为
                  </button>
                  <button className="btn" type="button" onClick={handleRestoreMarkerEdits} disabled={!editMode || !hasUnsavedMarkerEdits || isSavingEdits}>
                    还原
                  </button>
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

      {markerMenu ? (
        <div
          ref={markerMenuRef}
          className="dropdown-menu flake-marker-context-menu"
          onMouseDown={(event) => event.stopPropagation()}
          onContextMenu={(event) => event.preventDefault()}
        >
          {markerMenu.isOrigin ? (
            <>
              <div className="dropdown-item flake-marker-menu-heading">编辑</div>
              <button className="dropdown-item flake-marker-menu-item" type="button" onClick={handleRemoveOriginMarker}>
                移除标记
              </button>
              <button
                className="dropdown-item flake-marker-menu-item"
                type="button"
                disabled={!effectiveOriginMarker || !meta || effectiveOriginMarker.y >= meta.size_y - 1}
                onClick={() => handleShiftOriginMarker(1)}
              >
                上移一层
              </button>
              <button
                className="dropdown-item flake-marker-menu-item"
                type="button"
                disabled={!effectiveOriginMarker || effectiveOriginMarker.y <= 0}
                onClick={() => handleShiftOriginMarker(-1)}
              >
                下移一层
              </button>
              <div className="dropdown-item flake-marker-menu-heading">创建/更改</div>
              <div className="dropdown-item flake-marker-menu-empty">（无更多选项）</div>
            </>
          ) : (
            <>
              <div className="dropdown-item flake-marker-menu-heading">创建/更改（仅作提示）</div>
              <button className="dropdown-item flake-marker-menu-item" type="button" onClick={() => handleStageOriginMarker(markerMenu.point)}>
                原点
              </button>
            </>
          )}
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


