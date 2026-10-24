import React, { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { cancelLayerView, hitTestLayer, nextLayerRequestId, requestLayerView, type LayerScene, type LayerViewImage } from "../../../../../src/platform/layerView";
import type { ProjectionMarkerPoint } from "../../../../../src/services/flakeService";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import type { FlakeHoverBlock } from "./FlakePage";

export interface LayerCanvasHandle { resetView: () => void; zoomTo: (scale: number) => void }
interface Point { x: number; z: number }
interface Measurement { id: string; startX: number; startZ: number; endX: number; endZ: number }
interface Props {
  scene: LayerScene; layerY: number; onionDepth: number; showStateHints: boolean; interactionTool: string;
  editedBlockStates: Record<string, Record<string, string>>; originMarker: ProjectionMarkerPoint | null;
  measurements: Measurement[];
  onHoverBlock: (block: FlakeHoverBlock | null, event: React.MouseEvent | null) => void;
  onBlockClick: (block: FlakeHoverBlock, event: React.MouseEvent) => void;
  onBlockRightClick: (block: FlakeHoverBlock, event: React.MouseEvent) => void;
  onMarkerContextMenu: (point: ProjectionMarkerPoint, isOrigin: boolean, event: React.MouseEvent) => void;
  onOriginChange: (point: ProjectionMarkerPoint) => void;
  onAddMeasurement: (rect: Omit<Measurement, "id">) => void;
  onDeleteMeasurementAt: (point: Point) => void;
  onScaleChange: (scale: number) => void;
  onViewState: (image: LayerViewImage | null, error: string) => void;
}
type Variables = React.CSSProperties & Record<`--${string}`, string | number>;

/** Layout/input coordinates only. All source, palette, model and pixel work is Core-owned. */
export const LayerCanvas = forwardRef<LayerCanvasHandle, Props>((props, ref) => {
  const { locale } = useI18n();
  const { scene, layerY, onionDepth, showStateHints, editedBlockStates, interactionTool, originMarker, measurements } = props;
  const viewport = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [image, setImage] = useState<LayerViewImage | null>(null);
  const [error, setError] = useState("");
  const [draft, setDraft] = useState<Measurement | null>(null);
  const [originDraft, setOriginDraft] = useState<ProjectionMarkerPoint | null>(null);
  const dragging = useRef<{ kind: "pan" | "measure" | "origin"; x: number; y: number; ox: number; oy: number } | null>(null);
  const current = useRef(props); current.current = props;
  const transform = useRef({ scale, offset }); transform.current = { scale, offset };
  const request = useRef(0);
  const hoverSequence = useRef(0);
  const hoverPoint = useRef("");
  const hoverFrame = useRef<number | null>(null);
  const latestHover = useRef<React.MouseEvent | null>(null);
  const coordinates = (clientX: number, clientY: number) => {
    const element = viewport.current!; const rect = element.getBoundingClientRect();
    return { x: (clientX - rect.left) * element.clientWidth / Math.max(1, rect.width), y: (clientY - rect.top) * element.clientHeight / Math.max(1, rect.height) };
  };

  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const measure = () => setSize({ width: element.clientWidth, height: element.clientHeight });
    measure(); const observer = new ResizeObserver(measure); observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const reset = () => {
    const next = Math.max(1, Math.min(64, Math.min(size.width / Math.max(1, scene.meta.size_x), size.height / Math.max(1, scene.meta.size_z)) * .9));
    setScale(next); setOffset({ x: (size.width - scene.meta.size_x * next) / 2, y: (size.height - scene.meta.size_z * next) / 2 });
  };
  const initialized = useRef("");
  useEffect(() => { if (size.width > 0 && initialized.current !== scene.sceneHandle) { initialized.current = scene.sceneHandle; reset(); } }, [scene.sceneHandle, size.width, size.height]);
  const zoom = (value: number, x = size.width / 2, y = size.height / 2) => {
    const next = Math.max(1, Math.min(64, value)); const old = transform.current;
    setOffset({ x: x - (x - old.offset.x) * next / old.scale, y: y - (y - old.offset.y) * next / old.scale }); setScale(next);
  };
  useImperativeHandle(ref, () => ({ resetView: reset, zoomTo: zoom }), [scene, size, scale, offset]);
  useEffect(() => { current.current.onScaleChange(scale); }, [scale]);

  useEffect(() => {
    if (!size.width || !size.height) return;
    const requestId = nextLayerRequestId(); request.current = requestId;
    hoverSequence.current += 1; hoverPoint.current = ""; current.current.onHoverBlock(null, null);
    let active = true;
    setImage(previous => previous?.y === layerY ? previous : null); setError(""); current.current.onViewState(null, "");
    // WebView coordinates choose the viewport, not the blocks in that viewport.
    const x = Math.max(0, Math.min(scene.meta.size_x - 1, Math.floor(-offset.x / scale)));
    const z = Math.max(0, Math.min(scene.meta.size_z - 1, Math.floor(-offset.y / scale)));
    const width = Math.max(1, Math.min(4096, scene.meta.size_x - x, Math.ceil(size.width / scale) + 2));
    const height = Math.max(1, Math.min(4096, scene.meta.size_z - z, Math.ceil(size.height / scale) + 2));
    // A frame coalesces UI movement without delaying the actual layer choice.
    const frame = requestAnimationFrame(() => {
      void requestLayerView(scene, { requestId, y: layerY, onionDepth, viewRect: { x, z, width, height }, scale, stateHints: showStateHints, overrides: editedBlockStates }).then(result => {
        if (!active || request.current !== result.requestId || result.sceneHandle !== scene.sceneHandle || result.generation !== scene.generation || result.y !== layerY) return;
        setImage(result); current.current.onViewState(result, "");
      }).catch(reason => {
        if (!active) return;
        const message = String(reason); setError(message); current.current.onViewState(null, message);
      });
    });
    return () => { active = false; cancelAnimationFrame(frame); void cancelLayerView(scene, requestId).catch(() => undefined); };
  }, [scene, layerY, onionDepth, showStateHints, editedBlockStates, size, scale, offset]);
  useEffect(() => () => { if (hoverFrame.current !== null) cancelAnimationFrame(hoverFrame.current); ++hoverSequence.current; }, []);
  useEffect(() => {
    const cancel = () => { dragging.current = null; setDraft(null); setOriginDraft(null); ++hoverSequence.current; current.current.onHoverBlock(null, null); };
    window.addEventListener("blur", cancel); return () => window.removeEventListener("blur", cancel);
  }, []);

  const point = (event: React.MouseEvent): Point | null => {
    if (!viewport.current) return null;
    const local = coordinates(event.clientX, event.clientY);
    // Undo the actual CSS view transform once; never multiply devicePixelRatio.
    const x = Math.floor((local.x - offset.x) / scale), z = Math.floor((local.y - offset.y) / scale);
    return x < 0 || z < 0 || x >= scene.meta.size_x || z >= scene.meta.size_z ? null : { x, z };
  };
  const hit = async (event: React.MouseEvent, kind: "hover" | "click" | "context") => {
    const p = point(event); if (!p || !image) { if (kind === "hover") current.current.onHoverBlock(null, null); return; }
    const sequence = ++hoverSequence.current; const requestId = request.current;
    try {
      const result = await hitTestLayer(scene, { requestId, y: layerY, onionDepth, overrides: editedBlockStates, uiLocale: locale }, p.x, p.z);
      if (sequence !== hoverSequence.current || requestId !== request.current || result.sceneHandle !== scene.sceneHandle || result.generation !== scene.generation) return;
      const block = result.hit;
      if (kind === "hover") current.current.onHoverBlock(block, event);
      else if (block && kind === "click") current.current.onBlockClick(block, event);
      else if (block) current.current.onBlockRightClick(block, event);
    } catch (reason) {
      if (requestId === request.current && sequence === hoverSequence.current) { setError(String(reason)); current.current.onViewState(image, String(reason)); }
    }
  };
  const move = (event: React.MouseEvent) => {
    const drag = dragging.current;
    if (drag?.kind === "pan") { const local = coordinates(event.clientX, event.clientY); setOffset({ x: drag.ox + local.x - drag.x, y: drag.oy + local.y - drag.y }); return; }
    const p = point(event);
    if (drag?.kind === "measure" && p) { setDraft(old => old && ({ ...old, endX: p.x, endZ: p.z })); return; }
    if (drag?.kind === "origin" && p && originMarker) { setOriginDraft({ ...originMarker, ...p }); return; }
    if (scale < 8) return;
    const key = p ? `${p.x}:${p.z}` : "";
    if (key === hoverPoint.current) return;
    hoverPoint.current = key; latestHover.current = event;
    if (hoverFrame.current === null) hoverFrame.current = requestAnimationFrame(() => {
      hoverFrame.current = null; if (latestHover.current) void hit(latestHover.current, "hover");
    });
  };
  const finish = () => {
    if (dragging.current?.kind === "measure" && draft) current.current.onAddMeasurement({ startX: draft.startX, startZ: draft.startZ, endX: draft.endX, endZ: draft.endZ });
    if (dragging.current?.kind === "origin" && originDraft) current.current.onOriginChange(originDraft);
    dragging.current = null; setDraft(null); setOriginDraft(null);
  };
  const marker = originDraft || originMarker;
  const worldStyle = { transform: `translate(${offset.x}px, ${offset.y}px)` };
  const imageGeometry: Variables | undefined = image ? { "--layer-image-x": `${image.viewRect.x * scale}px`, "--layer-image-z": `${image.viewRect.z * scale}px`, "--layer-image-width": `${image.viewRect.width * scale}px`, "--layer-image-height": `${image.viewRect.height * scale}px` } : undefined;
  const borderGeometry: Variables = { "--flake-layer-border-width": `${scene.meta.size_x * scale}px`, "--flake-layer-border-height": `${scene.meta.size_z * scale}px` };
  const markerGeometry: Variables | undefined = marker ? { "--flake-origin-marker-left": `${marker.x * scale}px`, "--flake-origin-marker-top": `${marker.z * scale}px`, "--flake-origin-marker-width": `${scale}px`, "--flake-origin-marker-height": `${scale}px` } : undefined;
  return <div ref={viewport} className={`flake-canvas ${interactionTool === "measure" ? "is-measure-tool" : interactionTool === "marker" ? "is-marker-tool" : ""}`}
    onWheel={event => { event.preventDefault(); const local = coordinates(event.clientX, event.clientY); zoom(scale * (event.deltaY < 0 ? 2 : .5), local.x, local.y); }}
    onPointerDown={event => {
      if (event.button !== 0) return; const p = point(event);
      event.currentTarget.closest<HTMLElement>(".flake-page__viewport")?.focus({ preventScroll: true });
      if (interactionTool === "measure" && p) setDraft({ id: "draft", startX: p.x, startZ: p.z, endX: p.x, endZ: p.z });
      const originDrag = interactionTool === "marker" && p && marker?.y === layerY && p.x === marker.x && p.z === marker.z;
      if (interactionTool === "hand" || interactionTool === "measure" || originDrag) {
        event.currentTarget.setPointerCapture(event.pointerId);
        const local = coordinates(event.clientX, event.clientY);
        dragging.current = { kind: originDrag ? "origin" : interactionTool === "measure" ? "measure" : "pan", x: local.x, y: local.y, ox: offset.x, oy: offset.y };
      }
    }} onPointerMove={move} onPointerUp={finish} onPointerCancel={() => { dragging.current = null; setDraft(null); setOriginDraft(null); }}
    onMouseLeave={() => { ++hoverSequence.current; hoverPoint.current = ""; current.current.onHoverBlock(null, null); }}
    onClick={event => { if (!["hand", "measure", "marker"].includes(interactionTool)) void hit(event, "click"); }}
    onContextMenu={event => { event.preventDefault(); const p = point(event); if (!p) return;
      if (interactionTool === "measure") current.current.onDeleteMeasurementAt(p);
      else if (interactionTool === "marker") current.current.onMarkerContextMenu({ ...p, y: layerY }, !!marker && marker.y === layerY && p.x === marker.x && p.z === marker.z, event);
      else void hit(event, "context");
    }}>
    {error && <div className="flake-visual-status" role="alert">{error}</div>}
    {image && image.missingVisuals > 0 && !error && <div className="flake-visual-status" role="status">{t("flake.missingVisuals", { count: image.missingVisuals })}</div>}
    <div className="flake-layer-world" style={worldStyle}>
      {image && <img className="flake-layer-image" src={image.imageUrl} alt="" draggable={false} style={imageGeometry} />}
      <div className="flake-layer-border" style={borderGeometry} />
      {[...measurements, ...(draft ? [draft] : [])].map(rect => {
        const width = Math.abs(rect.endX - rect.startX) + 1, height = Math.abs(rect.endZ - rect.startZ) + 1;
        const widthPx = width * scale, heightPx = height * scale;
        const minX = Math.min(rect.startX, rect.endX), minZ = Math.min(rect.startZ, rect.endZ);
        const lines: string[] = [];
        if (scale >= 8) {
          for (let x = Math.max(minX + 1, Math.ceil(-offset.x / scale)); x <= Math.min(minX + width - 1, Math.ceil((size.width - offset.x) / scale)); x++) lines.push(`M ${(x - minX) * scale} 0 V ${heightPx}`);
          for (let z = Math.max(minZ + 1, Math.ceil(-offset.y / scale)); z <= Math.min(minZ + height - 1, Math.ceil((size.height - offset.y) / scale)); z++) lines.push(`M 0 ${(z - minZ) * scale} H ${widthPx}`);
        }
        const geometry: Variables = { "--flake-measurement-left": `${Math.min(rect.startX, rect.endX) * scale}px`, "--flake-measurement-top": `${Math.min(rect.startZ, rect.endZ) * scale}px`, "--flake-measurement-width": `${width * scale}px`, "--flake-measurement-height": `${height * scale}px` };
        return <div key={rect.id} className={`flake-measurement-rect${rect.id === "draft" ? " is-draft" : ""}`} style={geometry}>
          <svg className="flake-measurement-svg" viewBox={`0 0 ${widthPx} ${heightPx}`} preserveAspectRatio="none" aria-hidden="true">
            {lines.length > 0 && <path className="flake-measurement-grid-line" d={lines.join(" ")} strokeDasharray="4 4" />}
            <line className="flake-measurement-diagonal" x1={rect.startX <= rect.endX ? 0 : widthPx} y1={rect.startZ <= rect.endZ ? 0 : heightPx} x2={rect.startX <= rect.endX ? widthPx : 0} y2={rect.startZ <= rect.endZ ? heightPx : 0} />
          </svg>
          <span className="flake-measurement-label flake-measurement-label--x">x {width}</span>
          <span className="flake-measurement-label flake-measurement-label--z">z {height}</span>
          <span className="flake-measurement-label flake-measurement-label--manhattan">{t("flake.manhattan", { distance: (width + height).toLocaleString(locale) })}</span>
          {widthPx >= 28 && heightPx >= 18 && <span className="flake-measurement-label flake-measurement-label--inside">{t("flake.euclidean", { distance: Math.hypot(width, height).toLocaleString(locale, { maximumFractionDigits: 2 }) })}</span>}
        </div>;
      })}
      {marker?.y === layerY && <div className="flake-origin-marker" style={markerGeometry} />}
    </div>
  </div>;
});
