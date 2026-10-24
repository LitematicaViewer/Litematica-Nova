import { LayerCanvas, type LayerCanvasHandle } from "./LayerCanvas";
import { openLayerScene, releaseLayerScene, type LayerScene, type LayerHit, type LayerViewImage } from "../../../../../src/platform/layerView";
import { t, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayPathText } from "../../../../../src/business/facade";
import { creativeItemId } from "../../../../../src/business/facade";
import { displayFilePath } from "../../../../../src/business/facade";
import { Hand, Wand2, Ruler, MapPin, BoxSelect, Pencil, Eraser, Pipette, Square, Circle, PaintBucket, Type, Plus, Minus, Maximize2, SlidersHorizontal } from "lucide-react";
import { Slider } from "../../../../components/ui/slider";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Dropdown } from "../../../../components/Dropdown";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Checkbox } from "../../../../components/ui/checkbox";
import { takeUpdateDraft, useUpdateDraft } from "../../updateRecovery";
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  loadProjectionMarkers, loadStructureStats,
  type ProjectionMarkerPoint, removeProjectionOrigin, selectLitematicSavePath,
  setProjectionOrigin, type StatsData,
  translateBlockId, translateKey, translateValue,
} from "../../../../../src/business/facade";
import "../../../../styles/flake.css";
import { loadStructuralBlocks } from "../../../../../src/business/facade";
import { BlockIcon } from "../../../../components/BlockIcon";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { Dialog } from "../../../../components/Dialog";
import { MaterialsDialog, openMaterialsWithWindowBehavior } from "../statistics/StatisticsPage";
import { Layer3DView } from "./Layer3DView";

// 同级函数
import { FlakeBlockTooltip, formatStateRecord } from "./function";
import { CreativeInventoryDialog } from "./creativeInventoryDialog";
import { ContainerDialog } from "./containerDialog";
import { loadContainerData, isContainerBlock, getContainerType, type ContainerItem, type ContainerType } from "../../../../../src/business/facade";
import { listenEvent } from "../../../../../src/platform/events";
import noteCsvText from "../../../../../note.csv?raw";

// 缩放范围：scale 表示每个方块占用的像素数（像素/格）。
const MIN_SCALE = 1;
const MAX_SCALE = 64;
const NOTE_BLOCK_ID = "minecraft:note_block";
const NOTE_BLOCK_NOTE_VALUES = Array.from({ length: 25 }, (_unused, index) => String(index));
const NOTE_SCALE_LABEL_KEYS = ["flake.note.scale.values", "flake.note.scale.westernMajor", "flake.note.scale.maqamAjam", "flake.note.scale.raagBhairav", "flake.note.scale.maqamHijaz", "flake.note.scale.dorian", "flake.note.scale.gong", "flake.note.scale.miyakoBushi", "flake.note.scale.degung", "flake.note.scale.pyongjo"];
const NOTE_BLOCK_KEY_ROWS = [
  { id: "high", labelKey: "flake.note.high", values: Array.from({ length: 7 }, (_unused, index) => String(index + 18)) },
  { id: "middle", labelKey: "flake.note.middle", values: Array.from({ length: 12 }, (_unused, index) => String(index + 6)) },
  { id: "low", labelKey: "flake.note.low", values: Array.from({ length: 6 }, (_unused, index) => String(index)) },
] as const;
const EDIT_TOOL_PLACEHOLDERS = [
  { id: "hand", icon: Hand, themeIcon: "hand", nameKey: "flake.tool.hand", descriptionKey: "flake.tool.handHint" },
  { id: "debug-stick", icon: Wand2, themeIcon: "stick", nameKey: "flake.tool.debugStick", descriptionKey: "flake.tool.debugStickHint" },
  { id: "measure", icon: Ruler, themeIcon: "ruler", nameKey: "flake.tool.measure", descriptionKey: "flake.tool.measureHint" },
  { id: "marker", icon: MapPin, themeIcon: "pin", nameKey: "flake.tool.marker", descriptionKey: "flake.tool.markerHint" },
  { id: "select", icon: BoxSelect, themeIcon: "select", nameKey: "flake.tool.select", descriptionKey: "flake.tool.selectHint" },
  { id: "brush", icon: Pencil, themeIcon: "pencil", nameKey: "flake.tool.brush", descriptionKey: "flake.tool.brushHint" },
  { id: "eraser", icon: Eraser, themeIcon: "eraser", nameKey: "flake.tool.eraser", descriptionKey: "flake.tool.eraserHint" },
  { id: "picker", icon: Pipette, themeIcon: "pick", nameKey: "flake.tool.picker", descriptionKey: "flake.tool.pickerHint" },
  { id: "rectangle", icon: Square, themeIcon: "rectangle", nameKey: "flake.tool.rectangle", descriptionKey: "flake.tool.rectangleHint" },
  { id: "circle", icon: Circle, themeIcon: "circle", nameKey: "flake.tool.circle", descriptionKey: "flake.tool.circleHint" },
  { id: "bucket", icon: PaintBucket, themeIcon: "fill", nameKey: "flake.tool.bucket", descriptionKey: "flake.tool.bucketHint" },
  { id: "text", icon: Type, themeIcon: "text", nameKey: "flake.tool.text", descriptionKey: "flake.tool.textHint" },
] as const;

type EditToolId = typeof EDIT_TOOL_PLACEHOLDERS[number]["id"];

interface NoteToneLabel {
  value: string;
  raw: string;
  base: string;
  sharpMarks: string;
}

interface NoteScaleOption {
  name: string;
  labelKey: string;
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

  return rows.slice(1).map((row,index) => {
    const labelsByValue: Record<string, NoteToneLabel> = {};
    noteValues.forEach((value, index) => {
      labelsByValue[value] = parseNoteToneLabel(value, row[index + 1]);
    });
    NOTE_BLOCK_NOTE_VALUES.forEach((value) => {
      labelsByValue[value] = labelsByValue[value] || parseNoteToneLabel(value, value);
    });
    return {
      name: row[0] || "__default__",
      labelKey: NOTE_SCALE_LABEL_KEYS[index] || "flake.defaultScale",
      labelsByValue,
    };
  });
}

const NOTE_SCALE_OPTIONS = parseNoteScaleOptions(noteCsvText);
const DEFAULT_NOTE_SCALE_NAME = NOTE_SCALE_OPTIONS[0]?.name || "__default__";

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
  return formatStateRecord(stateRecord) || t("editor.none");
}

function isTextEntryTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return !!target.closest("input, textarea, select");
}

export type FlakeHoverBlock = LayerHit;
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

function FlakeDebugStateDialog({
  block,
  onApply,
  onClose,
}: {
  block: FlakeHoverBlock;
  onApply: (stateRecord: Record<string, string>) => void;
  onClose: () => void;
}) {
  useI18n();
  const [draftState, setDraftState] = useState<Record<string, string>>(() => ({ ...block.stateRecord }));
  const [selectedNoteScaleName, setSelectedNoteScaleName] = useState(DEFAULT_NOTE_SCALE_NAME);
  const propertyKeys = useMemo(() => Object.keys(block.stateRecord).sort(), [block.stateRecord]);
  const knownProperties = block.propertyOptions;
  const selectedNoteScale = NOTE_SCALE_OPTIONS.find((option) => option.name === selectedNoteScaleName) || NOTE_SCALE_OPTIONS[0];

  useEffect(() => {
    setDraftState({ ...block.stateRecord });

  }, [block]);

  const setProperty = (key: string, value: string) => {
    setDraftState((current) => ({ ...current, [key]: value }));
  };

  return (
    <Dialog
      title={t("flake.tool.debugStick")}
      subtitle={`${translateBlockId(block.id)} (${block.id})`}
      width="lg"
      className="flake-page__state-dialog"
      onClose={onClose}
      footer={(
        <>
          <Button type="button" onClick={onClose}>{t("editor.cancel")}</Button>
          <Button type="button" onClick={() => onApply(normalizeLayerStateRecord(draftState))}>{t("editor.apply")}</Button>
        </>
      )}
    >
      <div className="dialog-form-grid flake-page__state-dialog-grid">
        <span className="dialog-label">{t("editor.position")}</span>
        <span>x={block.x} y={block.y} z={block.z}</span>
        <span className="dialog-label">{t("editor.blockId")}</span>
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
                  <Dropdown
                    className="input flake-page__note-scale-select"
                    value={selectedNoteScaleName}
                    ariaLabel={t("flake.note.scale")}
                    onChange={setSelectedNoteScaleName}
                    options={NOTE_SCALE_OPTIONS.map((option) => ({value: option.name, label: t(option.labelKey)}))}
                  />
                  <div className="flake-page__note-keyboard" role="group" aria-label={t("flake.note.pitch")}>
                    {NOTE_BLOCK_KEY_ROWS.map((row) => (
                      <div
                        key={row.id}
                        className={`flake-page__note-key-row flake-page__note-key-row--${row.id}`}
                        role="group"
                        aria-label={t(row.labelKey)}
                      >
                        {row.values.map((value) => {
                          const noteLabel = selectedNoteScale?.labelsByValue[value] || parseNoteToneLabel(value, value);
                          return (
                            <span key={value} className="flake-page__note-key-slot">
                              <Button
                                className={noteLabel.sharpMarks ? "flake-page__note-key-button is-sharp" : "flake-page__note-key-button"}
                                type="button"
                                aria-label={t("flake.note.aria", {note:noteLabel.raw,value})}
                                aria-pressed={currentValue === value}
                                title={`${noteLabel.raw} (${value})`}
                                onClick={() => setProperty(key, value)}
                              >
                                <span className="flake-page__note-key-label">{noteLabel.base}</span>
                                {noteLabel.sharpMarks ? <span className="flake-page__note-key-sharp" aria-hidden="true">{noteLabel.sharpMarks}</span> : null}
                              </Button>
                            </span>
                          );
                        })}
                      </div>
                    ))}
                  </div>
                </div>
              ) : optionValues.length > 0 ? (
                <Dropdown
                  id={`flake-state-${key}`}
                  className="input"
                  value={currentValue}
                  ariaLabel={translateKey(key)}
                  onChange={(value) => setProperty(key, value)}
                  options={optionValues.map((value) => ({value, label: `${translateValue(key, value)} (${value})`}))}
                />
              ) : (
                <Input
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
  const { locale } = useI18n();
  const [viewMode, setViewMode] = useState<"2d" | "3d">("2d");
  const [scene, setScene] = useState<LayerScene | null>(null);
  const meta = scene?.meta ?? null;
  const sceneRef = useRef<LayerScene | null>(null);
  const [layerImage, setLayerImage] = useState<LayerViewImage | null>(null);
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
  const [markerStatus, setMarkerStatus] = useState<UiMessage>("");
  const [debugStateBlock, setDebugStateBlock] = useState<FlakeHoverBlock | null>(null);
  const [showInventoryDialog, setShowInventoryDialog] = useState(false);
  const [materialBlocks, setMaterialBlocks] = useState<string[]>([]);
  const [selectedQuickbarSlot, setSelectedQuickbarSlot] = useState(0);
  const [quickbarSlots, setQuickbarSlots] = useState<string[]>(() => Array.from({ length: 9 }, () => ""));
  const [toolPanelWidth, setToolPanelWidth] = useState(320);
  const [isLoadingLayers, setIsLoadingLayers] = useState(false);
  const [layerError, setLayerError] = useState<UiMessage>("");
  const pageRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<LayerCanvasHandle>(null);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const markerMenuRef = useRef<HTMLDivElement | null>(null);
  const resizeStateRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const syncRequestIdRef = useRef(0);
  const measurementIdRef = useRef(0);
  const markerRequestIdRef = useRef(0);
  const markerSourceRef = useRef("");
  const updateDraft = { editedBlockStates, measurementRects, draftOriginMarker, hasDraftOriginMarker, quickbarSlots, selectedQuickbarSlot };
  useUpdateDraft(`flake:${currentFile}`, updateDraft);

  const refreshProjectionMarkers = useCallback(async (initialOrigin?: ProjectionMarkerPoint | null) => {
    if (!currentFile) {
      setOriginMarker(null);
      setDraftOriginMarker(null);
      setHasDraftOriginMarker(false);
      return;
    }
    const requestId = ++markerRequestIdRef.current;
    const markers = initialOrigin === undefined ? await loadProjectionMarkers(currentFile) : { origin: initialOrigin };
    if (requestId !== markerRequestIdRef.current) return;
    setOriginMarker(markers.origin);
    const recovered = takeUpdateDraft<typeof updateDraft>(`flake:${currentFile}`);
    setDraftOriginMarker(recovered?.draftOriginMarker || null);
    setHasDraftOriginMarker(recovered?.hasDraftOriginMarker || false);
    if (recovered) {
      setEditedBlockStates(recovered.editedBlockStates); setMeasurementRects(recovered.measurementRects);
      setQuickbarSlots(recovered.quickbarSlots); setSelectedQuickbarSlot(recovered.selectedQuickbarSlot);
      measurementIdRef.current = Math.max(0, ...recovered.measurementRects.map((rect) => Number(rect.id.replace("measure-", "")) || 0));
    }
  }, [currentFile]);

  const loadProjectionLayers = async () => {
    const requestId = ++syncRequestIdRef.current;
    const previous = sceneRef.current; sceneRef.current = null;
    setScene(null); setLayerImage(null); setLayerError(""); setIsLoadingLayers(!!currentFile);
    if (previous) void releaseLayerScene(previous).catch(() => undefined);
    if (!currentFile) return;
    try {
      const opened = await openLayerScene(currentFile);
      if (requestId !== syncRequestIdRef.current) { void releaseLayerScene(opened).catch(() => undefined); return; }
      sceneRef.current = opened; setScene(opened); setLayerY(value => Math.max(0, Math.min(value, opened.meta.size_y - 1)));
      if (markerSourceRef.current !== currentFile) { markerSourceRef.current = currentFile; await refreshProjectionMarkers(opened.origin); }
      else setOriginMarker(opened.origin);
    } catch (error) {
      if (requestId === syncRequestIdRef.current) setLayerError({ key: "flake.layersFailed", params: { error: String(error) } });
    } finally {
      if (requestId === syncRequestIdRef.current) setIsLoadingLayers(false);
    }
  };
  const handleViewState = useCallback((image: LayerViewImage | null, error: string) => {
    setLayerImage(image); setLayerError(error);
  }, []);

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
    const requestId = syncRequestIdRef.current;
    const loaded = await loadStructureStats(currentFile);
    if (requestId !== syncRequestIdRef.current) return null;
    setStatsData(loaded);
    return loaded;
  };

  useEffect(() => {

    markerSourceRef.current = ""; setLayerY(0);
    setStatsData(null);
    setMaterialBlocks([]);
    setShowContainerDialog(false);
    setIsLoadingContainer(false);
    setIsSavingEdits(false);
    setContainerData(null);
    setShowMaterials(false);
    setEditedBlockStates({});
    setMeasurementRects([]);
    setOriginMarker(null);
    setDraftOriginMarker(null);
    setHasDraftOriginMarker(false);
    setMarkerMenu(null);
    setMarkerStatus("");
    measurementIdRef.current = 0;
    setDebugStateBlock(null);
    if (viewMode === "2d") void loadProjectionLayers();
    else { setScene(null); setLayerImage(null); setIsLoadingLayers(false); }
    return () => { ++syncRequestIdRef.current; ++markerRequestIdRef.current; const opened = sceneRef.current; sceneRef.current = null; if (opened) void releaseLayerScene(opened).catch(() => undefined); };
  }, [currentFile]);

  useEffect(() => {
    const subscription = listenEvent("game-resources-changed", () => {
      if (viewMode === "2d") void loadProjectionLayers();
      else { const opened = sceneRef.current; sceneRef.current = null; setScene(null); if (opened) void releaseLayerScene(opened).catch(() => undefined); }
    });
    return () => { void subscription.then(unlisten => unlisten()).catch(() => undefined); };
  }, [currentFile, viewMode]);

  // Selecting 3D for a new source must not start the unrelated all-layer 2D catalog.
  // Returning to 2D loads it only if this source has not already been prepared.
  const previousViewMode = useRef(viewMode);
  useEffect(() => {
    const previous = previousViewMode.current;
    previousViewMode.current = viewMode;
    if (previous === "3d" && viewMode === "2d" && currentFile && !meta && !isLoadingLayers) void loadProjectionLayers();
  }, [viewMode]);

  useEffect(() => () => {
    handleToolSplitterPointerUp();
  }, []);

  // 打开创造模式物品栏时懒加载当前投影的材料方块列表
  useEffect(() => {
    if (!showInventoryDialog || !currentFile || materialBlocks.length > 0) return;
    let active = true;
    loadStructuralBlocks(currentFile)
      .then((mats) => { if (active) setMaterialBlocks(mats.map((m) => m.id).filter(Boolean)); })
      .catch((error) => { if (active) setLayerError({ key: "flake.inventoryFailed", params: {error:String(error)} }); });
    return () => { active = false; };
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
    if (!currentFile || isSavingEdits) return;
    ++markerRequestIdRef.current;
    if (!markerPointsChanged(originMarker, point)) {
      setDraftOriginMarker(null);
      setHasDraftOriginMarker(false);
      setMarkerStatus({key:"flake.originUnchanged"});
      return;
    }
    setDraftOriginMarker(point);
    setHasDraftOriginMarker(true);
    setMarkerStatus({ key: "flake.originDraft", params: {x:point.x,y:point.y,z:point.z} });
  }, [currentFile, originMarker, isSavingEdits]);

  const handleRemoveOriginMarker = useCallback(() => {
    setMarkerMenu(null);
    if (!currentFile || isSavingEdits) return;
    ++markerRequestIdRef.current;
    if (!markerPointsChanged(originMarker, null)) {
      setDraftOriginMarker(null);
      setHasDraftOriginMarker(false);
      setMarkerStatus({key:"flake.noOrigin"});
      return;
    }
    setDraftOriginMarker(null);
    setHasDraftOriginMarker(true);
    setMarkerStatus({key:"flake.originRemovalDraft"});
  }, [currentFile, originMarker, isSavingEdits]);

  const handleShiftOriginMarker = useCallback((deltaY: number) => {
    if (isSavingEdits || !effectiveOriginMarker || !meta) return;
    const nextY = effectiveOriginMarker.y + deltaY;
    if (nextY < 0 || nextY >= meta.size_y) {
      setMarkerMenu(null);
      setMarkerStatus({key:"flake.originBoundary"});
      return;
    }
    handleStageOriginMarker({ ...effectiveOriginMarker, y: nextY });
  }, [effectiveOriginMarker, handleStageOriginMarker, meta, isSavingEdits]);

  const handleSaveMarkerEdits = useCallback(async () => {
    if (!currentFile || !hasUnsavedMarkerEdits || isSavingEdits) return;
    const requestId = syncRequestIdRef.current;
    setMarkerMenu(null);
    setIsSavingEdits(true);
    setMarkerStatus({key:"flake.savingMarker"});
    try {
      const summary = draftOriginMarker
        ? await setProjectionOrigin(currentFile, draftOriginMarker)
        : await removeProjectionOrigin(currentFile);
      if (requestId !== syncRequestIdRef.current) return;
      setOriginMarker(summary.origin);
      setDraftOriginMarker(null);
      setHasDraftOriginMarker(false);
      setStatsData(null);
      setMarkerStatus(summary.origin
        ? { key: "flake.originSaved", params: {x:summary.origin.x,y:summary.origin.y,z:summary.origin.z} }
        : {key:"flake.originRemoved"});
      setIsSavingEdits(false); void loadProjectionLayers();
    } catch (error) {
      if (requestId === syncRequestIdRef.current) setMarkerStatus({ key: "flake.originSaveFailed", params: {error:String(error)} });
    } finally {
      if (requestId === syncRequestIdRef.current) setIsSavingEdits(false);
    }
  }, [currentFile, draftOriginMarker, hasUnsavedMarkerEdits, isSavingEdits]);

  const handleSaveMarkerEditsAs = useCallback(async () => {
    if (!currentFile || !hasUnsavedMarkerEdits || isSavingEdits) return;
    const requestId = syncRequestIdRef.current;
    setMarkerMenu(null);
    setIsSavingEdits(true);
    try {
      const selected = await selectLitematicSavePath(markerSaveAsDefaultPath(currentFile));
      if (requestId !== syncRequestIdRef.current || !selected) return;
      const outputPath = ensureLitematicExtension(selected);
      if (!outputPath) return;
      setMarkerStatus({key:"flake.savingOriginCopy"});
      const summary = draftOriginMarker
        ? await setProjectionOrigin(currentFile, draftOriginMarker, outputPath)
        : await removeProjectionOrigin(currentFile, outputPath);
      if (requestId !== syncRequestIdRef.current) return;
      const savedPath = summary.output || outputPath;
      if (pathsLookSame(savedPath, currentFile)) {
        setOriginMarker(summary.origin);
        setDraftOriginMarker(null);
        setHasDraftOriginMarker(false);
        setStatsData(null);
        setMarkerStatus(summary.origin
          ? { key: "flake.originSaved", params: {x:summary.origin.x,y:summary.origin.y,z:summary.origin.z} }
          : {key:"flake.originRemoved"});
        setIsSavingEdits(false); void loadProjectionLayers();
      } else {
        setMarkerStatus({ key: "flake.originSavedAs", params: {path:displayFilePath(savedPath)} });
      }
    } catch (error) {
      if (requestId === syncRequestIdRef.current) setMarkerStatus({ key: "flake.originSaveAsFailed", params: {error:String(error)} });
    } finally {
      if (requestId === syncRequestIdRef.current) setIsSavingEdits(false);
    }
  }, [currentFile, draftOriginMarker, hasUnsavedMarkerEdits, isSavingEdits]);

  const handleRestoreMarkerEdits = useCallback(async () => {
    if (!currentFile || isSavingEdits) return;
    const requestId = syncRequestIdRef.current;
    setMarkerMenu(null);
    setIsSavingEdits(true);
    setMarkerStatus({key:"flake.restoringMarker"});
    try {
      await refreshProjectionMarkers();
      if (requestId === syncRequestIdRef.current) setMarkerStatus({key:"flake.markerRestored"});
    } catch (error) {
      if (requestId === syncRequestIdRef.current) setMarkerStatus({ key: "flake.originRestoreFailed", params: {error:String(error)} });
    } finally {
      if (requestId === syncRequestIdRef.current) setIsSavingEdits(false);
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
    const requestId = syncRequestIdRef.current;
    try {
      const data = await loadContainerData(currentFile, block.x, block.y, block.z);

      if (requestId !== syncRequestIdRef.current) return;
      if (!data) {
        setLayerError({key:"flake.noContainer"});
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
      if (requestId === syncRequestIdRef.current) setLayerError({ key: "flake.containerFailed", params: {error:String(error)} });
    } finally {
      if (requestId === syncRequestIdRef.current) setIsLoadingContainer(false);
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

  const regionName = hoverBlock?.region || meta?.name || currentFile?.split(/[\\/]/).pop() || "Unnamed";
  const maxY = meta ? Math.max(0, meta.size_y - 1) : 0;
  const ready = !!meta;
  const building = isLoadingLayers;
  const statusText = localizeMessage(layerError) || (!ready
    ? (building ? t("flake.readingLayers") : t("flake.layersNotReady"))
    : !layerImage ? t("flake.readingLayer", {layer:layerY + (meta?.min_y || 0)}) : t("flake.layerStatus", {layer:layerImage.worldY,blocks:layerImage.blockCount.toLocaleString(locale),depth:onionSkinDepth}));

  if (!currentFile) {
    return (
      <div className="nova-page flake-page">
        <div className="subwindow-toolbar flake-page__topbar">
          <Button className="flake-page__materials-button" disabled>
            {t("editor.materials")}
          </Button>
          <Button className="flake-page__build-button" disabled>
            {t("flake.reloadLayers")}
          </Button>
          <label className="subwindow-check-row flake-page__edit-toggle">
            <Checkbox  disabled />
            {t("flake.editMode")}
          </label>
          <div className="subwindow-toolbar-spacer" />
        </div>
        <div className="properties-empty-state">{t("flake.chooseFile")}</div>
      </div>
    );
  }

  return (
    <div ref={pageRef} className={editMode ? "nova-page flake-page flake-page--edit-mode" : "nova-page flake-page"} onKeyDown={event => {
      if (viewMode !== "2d" || !ready || showMaterials || showContainerDialog || debugStateBlock || showInventoryDialog || isTextEntryTarget(event.target)) return;
      if (event.key === "PageUp" || event.key === "PageDown") {
        event.preventDefault();
        setLayerY(value => Math.max(0, Math.min(maxY, value + (event.key === "PageUp" ? 1 : -1))));
      }
    }}>
      <div className="subwindow-toolbar flake-page__topbar">
        <div className="layer3d-view-tabs">
          <Button aria-pressed={viewMode === "2d"} variant={viewMode === "2d" ? "default" : "outline"} onClick={() => setViewMode("2d")}>{t("layerView.2d")}</Button>
          <Button aria-pressed={viewMode === "3d"} variant={viewMode === "3d" ? "default" : "outline"} onClick={() => { setEditMode(false); setViewMode("3d"); }}>{t("layerView.3d")}</Button>
        </div>
        <Button className="flake-page__materials-button" onClick={async () => {
          try {
            const loaded = await ensureStatsData();
            if (!loaded) return;
            openMaterialsWithWindowBehavior(currentFile, () => setShowMaterials(true));
          } catch (error) {
            setStatsData(null);
            setLayerError({ key: "flake.materialsFailed", params: {error:String(error)} });
          }
        }}>
          {t("editor.materials")}
        </Button>
        <label className="subwindow-check-row flake-page__edit-toggle">
          <Checkbox

            checked={editMode}
            disabled={viewMode !== "2d"}
            onCheckedChange={(checked) => {
              const enabled = (checked === true);
              if (enabled) setSelectedEditTool("hand");
              setEditMode(enabled);
            }}
          />
          {t("flake.editMode")}
        </label>
        <div className="subwindow-toolbar-spacer" />
        <div className="flake-page__file-path" title={displayFilePath(currentFile)}>{displayFilePath(currentFile)}</div>
      </div>

      {viewMode === "3d" ? <Layer3DView key={currentFile} sourcePath={currentFile} blocked={showMaterials || showContainerDialog || !!debugStateBlock || showInventoryDialog} /> : <>
      <div className="flake-page__control-box">
        <div className="flake-page__primary-controls" role="toolbar" aria-label={t("flake.layerControls")}>
          <label className="flake-page__y-field">Y <Input type="number" min={meta?.min_y ?? 0} max={maxY + (meta?.min_y ?? 0)} value={layerY + (meta?.min_y ?? 0)} disabled={!ready} aria-label={t("flake.currentLayer")} onChange={event => {
            const next = Number(event.target.value);
            if (Number.isFinite(next)) setLayerY(Math.max(0, Math.min(maxY, Math.round(next) - (meta?.min_y ?? 0))));
          }} /></label>
          <Button className="flake-page__step-button" disabled={!ready || layerY === 0} title={t("flake.previousLayer")} aria-label={t("flake.previousLayer")} onClick={() => setLayerY(Math.max(0, layerY - 1))}><Minus aria-hidden="true" /></Button>
          <Button className="flake-page__step-button" disabled={!ready || layerY === maxY} title={t("flake.nextLayer")} aria-label={t("flake.nextLayer")} onClick={() => setLayerY(Math.min(maxY, layerY + 1))}><Plus aria-hidden="true" /></Button>
          <Slider className="flake-page__layer-range" aria-label={t("flake.currentLayer")} min={0} max={Math.max(1, maxY)} value={[layerY]} onValueChange={([value]) => setLayerY(Math.min(value, maxY))} disabled={!ready || maxY === 0} />
          <span className="flake-page__key-hint"><kbd>PgUp</kbd> / <kbd>PgDn</kbd></span>
          <Button disabled={!ready} onClick={() => canvasRef.current?.resetView()}><Maximize2 aria-hidden="true" />{t("flake.resetView")}</Button>
        </div>
        <details className="flake-page__advanced-controls">
          <summary><SlidersHorizontal aria-hidden="true" />{t("flake.advanced")}</summary>
          <div className="flake-page__advanced-content">
            <div className="flake-page__layer-row flake-page__onion-row">
              <span className="nova-muted flake-page__layer-label">{t("flake.onionSkin")}</span>
              <Slider className="flake-page__layer-range" aria-label={t("flake.onionSkinDepth")} min={0} max={5} step={1} value={[onionSkinDepth]} onValueChange={([value]) => setOnionSkinDepth(Math.min(5, Math.max(0, value || 0)))} disabled={!ready} />
              <Input className="input flake-page__onion-input" type="number" min={0} max={5} step={1} value={onionSkinDepth} aria-label={t("flake.onionSkinDepth")} onChange={event => setOnionSkinDepth(Math.min(5, Math.max(0, Number(event.target.value) || 0)))} disabled={!ready} />
            </div>
            <div className="flake-page__layer-row flake-page__zoom-row">
              <span className="nova-muted flake-page__layer-label">{t("flake.zoom")}</span>
              <Slider className="flake-page__layer-range" aria-label={t("flake.layerZoom")} min={MIN_SCALE} max={MAX_SCALE} step={0.1} value={[viewScale]} onValueChange={([value]) => canvasRef.current?.zoomTo(value)} disabled={!ready} />
              <Input className="input flake-page__zoom-input" type="number" min={MIN_SCALE} max={MAX_SCALE} step={0.1} value={Math.round(viewScale * 10) / 10} aria-label={t("flake.layerZoom")} onChange={event => {
                const next = Number(event.target.value);
                if (Number.isFinite(next)) canvasRef.current?.zoomTo(next);
              }} disabled={!ready} />
              <span className="nova-muted nova-small flake-page__zoom-unit">{t("flake.pixelsPerBlock")}</span>
            </div>
            <div className="flake-page__advanced-actions">
              <label className="subwindow-check-row flake-page__edit-toggle"><Checkbox checked={showStateHints} onCheckedChange={checked => setShowStateHints(checked === true)} disabled={!ready} />{t("flake.stateImages")}</label>
              <Button className="flake-page__step-button" disabled={!ready} onClick={() => setLayerY(Math.max(0, layerY - 5))}>−5</Button>
              <Button className="flake-page__step-button" disabled={!ready} onClick={() => setLayerY(Math.min(maxY, layerY + 5))}>+5</Button>
              <Button className="flake-page__build-button" onClick={loadProjectionLayers} disabled={!currentFile || isLoadingLayers || isSavingEdits}>{isLoadingLayers ? t("flake.readingShort") : t("flake.reloadLayers")}</Button>
            </div>
          </div>
        </details>
      </div>

      <div className={layerError ? "nova-error flake-page__status-text" : "nova-muted nova-small flake-page__status-text"} role="status">{statusText}</div>

      <div className="flake-page__workspace">
        <section className="flake-page__main-panel">
          <div ref={viewportRef} className="flake-page__viewport" tabIndex={0} aria-label={t("flake.layerControls")}>
            {!ready ? (
              <div className="nova-empty-state flake-page__viewport-empty">
                <div className="flake-page__viewport-title">{building ? t("flake.reading2d") : t("flake.dataNotReady")}</div>
                <div>{statusText}</div>
              </div>
            ) : (
              <LayerCanvas
                ref={canvasRef}
                scene={scene!}
                layerY={layerY}
                onionDepth={onionSkinDepth}
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
                onViewState={handleViewState}
              />
            )}

            {ready && layerImage && layerImage.blockCount === 0 && <div className="flake-page__empty-layer" role="status">Y = {layerY + (meta?.min_y ?? 0)} · {t("flake.emptyViewport")}</div>}
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
              aria-label={t("flake.resizeTools")}
              onPointerDown={handleToolSplitterPointerDown}
            />

            <aside className="group-box flake-page__tool-panel">
              <div className="group-box-title">{t("flake.tools")}</div>
              <ScrollArea className="flake-page__tool-panel-body">
                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">{t("flake.context")}</div>
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">{t("flake.region")}</span>
                    <span>{regionName}</span>
                  </div>
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">{t("flake.layer")}</span>
                    <span>Y = {layerY + (meta?.min_y ?? 0)}</span>
                  </div>
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">{t("flake.data2d")}</span>
                    <span>{ready ? t("flake.ready") : building ? t("flake.reading") : t("flake.notReady")}</span>
                  </div>
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">{t("flake.measurement")}</span>
                    <span>{t("flake.rectangleCount", {count:measurementRects.length.toLocaleString(locale)})}</span>
                  </div>
                  <div className="flake-page__tool-panel-kv">
                    <span className="nova-muted">{t("flake.marker")}</span>
                    <span>
                      {effectiveOriginMarker
                        ? t("flake.originInfo", {x:effectiveOriginMarker.x,y:effectiveOriginMarker.y,z:effectiveOriginMarker.z,unsaved:hasUnsavedMarkerEdits ? t("flake.unsavedSuffix") : ""})
                        : hasUnsavedMarkerEdits ? t("flake.unsavedNone") : t("editor.none")}
                    </span>
                  </div>
                </div>

                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">{t("flake.status")}</div>
                  <div className="nova-muted nova-small flake-page__status-text">{statusText}</div>
                  {markerStatus ? <div className="nova-muted nova-small flake-page__status-text">{displayPathText(localizeMessage(markerStatus))}</div> : null}
                  {layerError ? <p className="nova-error flake-page__tool-panel-error">{localizeMessage(layerError)}</p> : null}
                </div>

                <div className="flake-page__tool-panel-section">
                  <div className="flake-page__tool-panel-heading">{t("flake.tools")}</div>
                  <div className="flake-page__tool-palette" role="toolbar" aria-label={t("flake.tools")}>
                    {EDIT_TOOL_PLACEHOLDERS.map((tool) => (
                      <Button
                        key={tool.id}
                        className="flake-page__edit-tool-button"
                        type="button"
                        onClick={() => setSelectedEditTool(tool.id)}
                        title={t("flake.toolHelp", {name:t(tool.nameKey), description:t(tool.descriptionKey)})}
                        aria-label={t(tool.nameKey)}
                        disabled={!["hand", "debug-stick", "measure", "marker"].includes(tool.id)}
                        aria-pressed={effectiveEditTool === tool.id}
                      >
                        <ThemeIcon set="tool" name={tool.themeIcon} size={32} className="flake-page__edit-tool-glyph" fallback={tool.icon} />
                      </Button>
                    ))}
                  </div>
                </div>

                <div className="flake-page__tool-panel-actions">
                  <Button type="button" onClick={handleSaveMarkerEdits} disabled={!editMode || !hasUnsavedMarkerEdits || isSavingEdits}>
                    {isSavingEdits ? t("flake.saveBusy") : t("flake.saveOrigin")}
                  </Button>
                  <Button type="button" onClick={handleSaveMarkerEditsAs} disabled={!editMode || !hasUnsavedMarkerEdits || isSavingEdits}>
                    {t("flake.saveOriginAs")}
                  </Button>
                  <Button type="button" onClick={handleRestoreMarkerEdits} disabled={!editMode || !hasUnsavedMarkerEdits || isSavingEdits}>
                    {t("flake.restoreOrigin")}
                  </Button>
                  <Button type="button" onClick={() => setShowInventoryDialog(true)} disabled={!editMode}>
                    {t("flake.openCreative")}
                  </Button>
                  <Button type="button" onClick={() => setRoute?.("render")}>
                    {t("flake.openRender")}
                  </Button>
                </div>
              </ScrollArea>
            </aside>
          </>
        ) : null}
      </div>

      {editMode ? (
        <div className="flake-page__quickbar" aria-label={t("flake.quickbar")}>
          <div className="flake-page__quickbar-body" role="toolbar" aria-label={t("flake.quickbarBody")}>
            {quickbarSlots.map((blockId, index) => {
              const isSelected = selectedQuickbarSlot === index;
              return (
                <Button
                  key={`slot-${index + 1}`}
                  className={isSelected ? "flake-page__quickbar-slot is-selected" : "flake-page__quickbar-slot"}
                  type="button"
                  aria-label={blockId ? t("flake.hotbarItemAria", {item:translateBlockId(creativeItemId(blockId) || blockId),index:index+1}) : t("flake.hotbarSlotAria", {index:index+1})}
                  aria-pressed={isSelected}
                  onClick={() => setSelectedQuickbarSlot(index)}
                >
                  {blockId ? <BlockIcon blockId={creativeItemId(blockId) || blockId} lookupMode="item_first" /> : <span className="flake-page__quickbar-slot-icon" aria-hidden />}
                  <span className="flake-page__quickbar-slot-label">{blockId ? translateBlockId(creativeItemId(blockId) || blockId) : t("flake.emptySlot")}</span>
                </Button>
              );
            })}
          </div>
          <Button className="flake-page__quickbar-open-inventory" type="button" onClick={() => setShowInventoryDialog(true)}>
            <span className="flake-page__quickbar-open-inventory-text">{t("flake.openInventory")}</span>
          </Button>
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
              <div className="dropdown-item flake-marker-menu-heading">{t("flake.edit")}</div>
              <Button className="dropdown-item flake-marker-menu-item" type="button" onClick={handleRemoveOriginMarker} disabled={isSavingEdits}>
                {t("flake.removeMarker")}
              </Button>
              <Button
                className="dropdown-item flake-marker-menu-item"
                type="button"
                disabled={isSavingEdits || !effectiveOriginMarker || !meta || effectiveOriginMarker.y >= meta.size_y - 1}
                onClick={() => handleShiftOriginMarker(1)}
              >
                {t("flake.moveUp")}
              </Button>
              <Button
                className="dropdown-item flake-marker-menu-item"
                type="button"
                disabled={isSavingEdits || !effectiveOriginMarker || effectiveOriginMarker.y <= 0}
                onClick={() => handleShiftOriginMarker(-1)}
              >
                {t("flake.moveDown")}
              </Button>
              <div className="dropdown-item flake-marker-menu-heading">{t("flake.createChange")}</div>
              <div className="dropdown-item flake-marker-menu-empty">{t("flake.noOptions")}</div>
            </>
          ) : (
            <>
              <div className="dropdown-item flake-marker-menu-heading">{t("flake.createHint")}</div>
              <Button className="dropdown-item flake-marker-menu-item" type="button" onClick={() => handleStageOriginMarker(markerMenu.point)}>
                {t("flake.origin")}
              </Button>
            </>
          )}
        </div>
      ) : null}

      </>}
      {showInventoryDialog ? (
        <CreativeInventoryDialog
          onClose={() => setShowInventoryDialog(false)}
          quickbarSlots={quickbarSlots}
          onChangeQuickbarSlots={(updater) => setQuickbarSlots((current) => updater([...current]))}
          extraCollections={
            materialBlocks.length > 0
              ? [{ id: '__current__', name: t("flake.currentBlocks"), values: materialBlocks }]
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
