import React, { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import { cancelLayerView, hitTestLayer, invalidateLayerVisuals, nextLayerRequestId, requestLayerView, type LayerScene, type LayerViewImage } from "../../../../../src/platform/layerView";
import type { ProjectionMarkerPoint } from "../../../../../src/services/flakeService";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import type { FlakeHoverBlock } from "./FlakePage";
import { Button } from "../../../../components/ui/button";

export interface LayerCanvasView { scale: number; offset: { x: number; y: number } }
export type LayerCanvasStatus = "loading" | "ready" | "partial" | "failed";
export interface LayerCanvasHandle { resetView: () => void; zoomTo: (scale: number) => void; viewState: () => LayerCanvasView }
interface Point { x: number; z: number }
interface Measurement { id: string; startX: number; startZ: number; endX: number; endZ: number }
interface Props {
  scene: LayerScene; layerY: number; onionDepth: number; showStateHints: boolean; interactionTool: string;
  suspended?: boolean; initialView?: LayerCanvasView | null; onResourceChanged: () => void;
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
  onViewState: (image: LayerViewImage | null, error: string, status: LayerCanvasStatus) => void;
}
type Variables = React.CSSProperties & Record<`--${string}`, string | number>;

/** Layout/input coordinates only. All source, palette, model and pixel work is Core-owned. */
export const LayerCanvas = forwardRef<LayerCanvasHandle, Props>((props, ref) => {
  const { locale } = useI18n();
  const { scene, layerY, onionDepth, showStateHints, editedBlockStates, interactionTool, originMarker, measurements } = props;
  const viewport = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [scale, setScale] = useState(props.initialView?.scale ?? 1);
  const [offset, setOffset] = useState(props.initialView?.offset ?? { x: 0, y: 0 });
  const [image, setImage] = useState<LayerViewImage | null>(null);
  const [pendingImage, setPendingImage] = useState<LayerViewImage | null>(null);
  const pendingElement = useRef<HTMLImageElement>(null);
  const [loading, setLoading] = useState(true);
  const [retrying, setRetrying] = useState(false);
  const retryInFlight = useRef(false);
  const failedRetryScene = useRef<LayerScene | null>(null);
  const [retryEpoch, setRetryEpoch] = useState(0);
  const [error, setError] = useState("");
  const [draft, setDraft] = useState<Measurement | null>(null);
  const [originDraft, setOriginDraft] = useState<ProjectionMarkerPoint | null>(null);
  const dragging = useRef<{ kind: "pan" | "measure" | "origin"; x: number; y: number; ox: number; oy: number } | null>(null);
  const current = useRef(props); current.current = props;
  const transform = useRef({ scale, offset }); transform.current = { scale, offset };
  const visualEpoch = useRef({ sceneHandle: scene.sceneHandle, generation: scene.generation, resourceIdentity: scene.resourceIdentity, minimum: scene.visualGeneration });
  if (visualEpoch.current.sceneHandle !== scene.sceneHandle || visualEpoch.current.generation !== scene.generation || visualEpoch.current.resourceIdentity !== scene.resourceIdentity) {
    visualEpoch.current = { sceneHandle: scene.sceneHandle, generation: scene.generation, resourceIdentity: scene.resourceIdentity, minimum: scene.visualGeneration };
  } else visualEpoch.current.minimum = Math.max(visualEpoch.current.minimum, scene.visualGeneration);
  const queryIdentity = useMemo(() => ({}), [scene.sceneHandle, scene.generation, scene.resourceIdentity, layerY, onionDepth,
    scene.visualGeneration, showStateHints, editedBlockStates, size.width, size.height, scale, offset.x, offset.y, retryEpoch, retrying, props.suspended]);
  const currentQuery = useRef(queryIdentity); currentQuery.current = queryIdentity;
  const pendingQuery = useRef<object | null>(null);
  const mounted = useRef(true);
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
    const nextOffset = { x: (size.width - scene.meta.size_x * next) / 2, y: (size.height - scene.meta.size_z * next) / 2 };
    transform.current = { scale: next, offset: nextOffset }; setScale(next); setOffset(nextOffset);
  };
  const initialized = useRef(props.initialView ? scene.sourceIdentity : "");
  useEffect(() => { if (size.width > 0 && initialized.current !== scene.sourceIdentity) { initialized.current = scene.sourceIdentity; reset(); } }, [scene.sourceIdentity, size.width, size.height]);
  const zoom = (value: number, x = size.width / 2, y = size.height / 2) => {
    const next = Math.max(1, Math.min(64, value)); const old = transform.current;
    const nextOffset = { x: x - (x - old.offset.x) * next / old.scale, y: y - (y - old.offset.y) * next / old.scale };
    transform.current = { scale: next, offset: nextOffset }; setOffset(nextOffset); setScale(next);
  };
  useImperativeHandle(ref, () => ({ resetView: reset, zoomTo: zoom, viewState: () => transform.current }), [scene, size, scale, offset]);
  useEffect(() => { current.current.onScaleChange(scale); }, [scale]);

  useEffect(() => {
    if (!size.width || !size.height || props.suspended || retrying) return;
    if (failedRetryScene.current) {
      const failed = failedRetryScene.current; failedRetryScene.current = null;
      if (failed.sceneHandle === scene.sceneHandle && failed.generation === scene.generation && failed.resourceIdentity === scene.resourceIdentity) return;
    }
    const requestId = nextLayerRequestId(); request.current = requestId;
    hoverSequence.current += 1; hoverPoint.current = ""; current.current.onHoverBlock(null, null);
    if (hoverFrame.current !== null) { cancelAnimationFrame(hoverFrame.current); hoverFrame.current = null; }
    let active = true;
    const relevant = (value: LayerViewImage) => value.sceneHandle === scene.sceneHandle && value.generation === scene.generation
      && value.resourceIdentity === scene.resourceIdentity && value.visualGeneration >= visualEpoch.current.minimum && value.y === layerY;
    setImage(previous => previous && relevant(previous) ? previous : null);
    setPendingImage(null); pendingQuery.current = null; setLoading(true); setError(""); current.current.onViewState(null, "", "loading");
    // WebView coordinates choose the viewport, not the blocks in that viewport.
    const x = Math.max(0, Math.min(scene.meta.size_x - 1, Math.floor(-offset.x / scale)));
    const z = Math.max(0, Math.min(scene.meta.size_z - 1, Math.floor(-offset.y / scale)));
    const width = Math.max(1, Math.min(4096, scene.meta.size_x - x, Math.ceil(size.width / scale) + 2));
    const height = Math.max(1, Math.min(4096, scene.meta.size_z - z, Math.ceil(size.height / scale) + 2));
    // A frame coalesces UI movement without delaying the actual layer choice.
    const frame = requestAnimationFrame(() => {
      void requestLayerView(scene, { requestId, y: layerY, onionDepth, viewRect: { x, z, width, height }, scale, stateHints: showStateHints, overrides: editedBlockStates }).then(result => {
        if (!active || currentQuery.current !== queryIdentity) return;
        if (requestId !== result.requestId || !relevant(result)) throw new Error(t("flake.imageIdentityFailed"));
        if (result.pixelWidth < 1 || result.pixelHeight < 1 || result.viewRect.width < 1 || result.viewRect.height < 1) throw new Error(t("flake.imageDimensionsFailed"));
        pendingQuery.current = queryIdentity; setPendingImage(result);
      }).catch(reason => {
        if (!active || currentQuery.current !== queryIdentity) return;
        const message = String(reason);
        setLoading(false); setError(message); current.current.onViewState(null, message, "failed");
        if (message.includes("LayerResourceChanged:") || message.includes("LayerSceneExpired:")) current.current.onResourceChanged();
      });
    });
    return () => { active = false; pendingQuery.current = null; cancelAnimationFrame(frame); void cancelLayerView(scene, requestId).catch(reason => console.warn("Cancel obsolete layer request:", reason)); };
  }, [queryIdentity]);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; if (hoverFrame.current !== null) cancelAnimationFrame(hoverFrame.current); ++hoverSequence.current; }; }, []);

  const matchesCurrent = (value: LayerViewImage) => value.sceneHandle === current.current.scene.sceneHandle
    && value.generation === current.current.scene.generation && value.resourceIdentity === current.current.scene.resourceIdentity
    && value.visualGeneration >= visualEpoch.current.minimum && value.y === current.current.layerY;
  const commitImage = (value: LayerViewImage, element: HTMLImageElement) => {
    if (!mounted.current || pendingQuery.current !== currentQuery.current || request.current !== value.requestId || !matchesCurrent(value)) return;
    if (element.naturalWidth !== value.pixelWidth || element.naturalHeight !== value.pixelHeight) { failImage(value, t("flake.imageDimensionsFailed")); return; }
    visualEpoch.current.minimum = value.visualGeneration;
    setImage(value); setPendingImage(null); pendingQuery.current = null; setLoading(false); setError("");
    current.current.onViewState(value, "", value.missingVisuals > 0 ? "partial" : "ready");
  };
  const failImage = (value: LayerViewImage, message = t("flake.imageDecodeFailed")) => {
    if (!mounted.current || pendingQuery.current !== currentQuery.current || request.current !== value.requestId || !matchesCurrent(value)) return;
    pendingQuery.current = null; setPendingImage(null); setLoading(false); setError(message); current.current.onViewState(null, message, "failed");
  };
  useLayoutEffect(() => {
    const element = pendingElement.current;
    // A data/resource URL may already be decoded before an event listener is
    // attached. Cached completion takes exactly the same guarded commit path.
    if (pendingImage && element?.complete) {
      if (element.naturalWidth > 0) commitImage(pendingImage, element); else failImage(pendingImage);
    }
  }, [pendingImage]);
  const retryVisuals = async () => {
    if (retryInFlight.current || props.suspended) return;
    const target = scene;
    const sameScene = () => mounted.current && current.current.scene.sceneHandle === target.sceneHandle
      && current.current.scene.generation === target.generation && current.current.scene.resourceIdentity === target.resourceIdentity;
    retryInFlight.current = true; pendingQuery.current = null; setPendingImage(null);
    setRetrying(true); setLoading(true); setError(""); current.current.onViewState(null, "", "loading");
    try {
      const invalidated = await invalidateLayerVisuals(target);
      if (sameScene()) {
        if (invalidated.sceneHandle !== target.sceneHandle || invalidated.generation !== target.generation || invalidated.resourceIdentity !== target.resourceIdentity) throw new Error(t("flake.imageIdentityFailed"));
        visualEpoch.current.minimum = Math.max(visualEpoch.current.minimum, invalidated.visualGeneration);
        setRetryEpoch(value => value + 1);
      }
    }
    catch (reason) {
      if (sameScene()) { const message = String(reason); failedRetryScene.current = target; setError(message); setLoading(false); current.current.onViewState(null, message, "failed"); if (message.includes("LayerResourceChanged:") || message.includes("LayerSceneExpired:")) current.current.onResourceChanged(); }
    } finally { retryInFlight.current = false; if (mounted.current) setRetrying(false); }
  };
  useEffect(() => {
    const cancel = () => { dragging.current = null; setDraft(null); setOriginDraft(null); ++hoverSequence.current; current.current.onHoverBlock(null, null); };
    window.addEventListener("blur", cancel); return () => window.removeEventListener("blur", cancel);
  }, []);

  const point = (event: React.MouseEvent): Point | null => {
    if (!viewport.current) return null;
    const local = coordinates(event.clientX, event.clientY);
    // Undo the actual CSS view transform once; never multiply devicePixelRatio.
    const view = transform.current;
    const x = Math.floor((local.x - view.offset.x) / view.scale), z = Math.floor((local.y - view.offset.y) / view.scale);
    return x < 0 || z < 0 || x >= scene.meta.size_x || z >= scene.meta.size_z ? null : { x, z };
  };
  const hit = async (event: React.MouseEvent, kind: "hover" | "click" | "context") => {
    if (queryIdentity !== currentQuery.current || props.suspended) return;
    const p = point(event); if (!p || !image || !matchesCurrent(image)) { if (kind === "hover") current.current.onHoverBlock(null, null); return; }
    const sequence = ++hoverSequence.current; const requestId = request.current;
    try {
      const result = await hitTestLayer(scene, { requestId, y: layerY, onionDepth, overrides: editedBlockStates, uiLocale: locale }, p.x, p.z);
      if (currentQuery.current !== queryIdentity || sequence !== hoverSequence.current || requestId !== request.current || result.requestId !== requestId || result.sceneHandle !== scene.sceneHandle || result.generation !== scene.generation) return;
      const block = result.hit;
      if (kind === "hover") current.current.onHoverBlock(block, event);
      else if (block && kind === "click") current.current.onBlockClick(block, event);
      else if (block) current.current.onBlockRightClick(block, event);
    } catch (reason) {
      if (currentQuery.current === queryIdentity && requestId === request.current && sequence === hoverSequence.current) { setError(String(reason)); current.current.onViewState(image, String(reason), "failed"); }
    }
  };
  const move = (event: React.MouseEvent) => {
    const drag = dragging.current;
    if (drag?.kind === "pan") { const local = coordinates(event.clientX, event.clientY); const next = { x: drag.ox + local.x - drag.x, y: drag.oy + local.y - drag.y }; transform.current = { ...transform.current, offset: next }; setOffset(next); return; }
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
  const visibleImage = image && matchesCurrent(image) ? image : null;
  const worldStyle = { transform: `translate(${offset.x}px, ${offset.y}px)` };
  const imageGeometry = (value: LayerViewImage): Variables => ({ "--layer-image-x": `${value.viewRect.x * scale}px`, "--layer-image-z": `${value.viewRect.z * scale}px`, "--layer-image-width": `${value.viewRect.width * scale}px`, "--layer-image-height": `${value.viewRect.height * scale}px` });
  const borderGeometry: Variables = { "--flake-layer-border-width": `${scene.meta.size_x * scale}px`, "--flake-layer-border-height": `${scene.meta.size_z * scale}px` };
  const markerGeometry: Variables | undefined = marker ? { "--flake-origin-marker-left": `${marker.x * scale}px`, "--flake-origin-marker-top": `${marker.z * scale}px`, "--flake-origin-marker-width": `${scale}px`, "--flake-origin-marker-height": `${scale}px` } : undefined;
  return <div ref={viewport} className={`flake-canvas ${interactionTool === "measure" ? "is-measure-tool" : interactionTool === "marker" ? "is-marker-tool" : ""}`}
    aria-busy={loading || props.suspended || retrying}
    onWheel={event => { event.preventDefault(); const local = coordinates(event.clientX, event.clientY); zoom(transform.current.scale * (event.deltaY < 0 ? 2 : .5), local.x, local.y); }}
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
    {(error || loading || props.suspended || retrying || (visibleImage?.missingVisuals ?? 0) > 0) && <div className="flake-visual-status" data-state={error ? "failed" : loading || props.suspended || retrying ? "loading" : "partial"}
      onPointerDown={event => event.stopPropagation()} onPointerMove={event => event.stopPropagation()} onWheel={event => event.stopPropagation()} onClick={event => event.stopPropagation()} onContextMenu={event => event.stopPropagation()}>
      <div className="flake-visual-status__heading" role={error ? "alert" : "status"}>
        <span>{error || (props.suspended ? t("flake.refreshingResources") : loading || retrying ? t("flake.loadingImage") : t("flake.missingVisuals", { count: visibleImage?.missingVisuals ?? 0 }))}</span>
        {(error || (visibleImage?.missingVisuals ?? 0) > 0) && <Button size="sm" disabled={retrying || loading || props.suspended} onClick={() => void retryVisuals()}>{t("flake.retryVisuals")}</Button>}
      </div>
      {visibleImage?.visualDiagnostics && visibleImage.missingVisuals > 0 && <details className="flake-visual-diagnostics">
        <summary>{t("flake.visualDetails")}</summary>
        <p>{t("flake.visualSummary", { ready: visibleImage.visualDiagnostics.readyStates, transparent: visibleImage.visualDiagnostics.transparentStates,
          missing: visibleImage.visualDiagnostics.missingStates, failed: visibleImage.visualDiagnostics.failedStates, retryable: visibleImage.visualDiagnostics.retryableStates })}</p>
        <ul>{visibleImage.visualDiagnostics.issues.map(issue => <li key={`${issue.state}:${issue.kind}:${issue.resourceKey ?? ""}`}>
          <strong>{issue.state}</strong><span>{issue.kind} · {t("flake.visualBlocks", { count: issue.blockCount })}</span>
          {issue.resourceKey && <code>{issue.resourceKey}</code>}<span>{issue.message}</span>
          <span>{t(issue.retryable ? "flake.visualRetryable" : "flake.visualNeedsResources")}</span>
        </li>)}</ul>
      </details>}
    </div>}
    <div className="flake-layer-world" style={worldStyle}>
      {visibleImage && <img className="flake-layer-image" src={visibleImage.imageUrl} alt="" draggable={false} style={imageGeometry(visibleImage)} />}
      {pendingImage && matchesCurrent(pendingImage) && <img key={`${pendingImage.sceneHandle}:${pendingImage.generation}:${pendingImage.requestId}`} ref={pendingElement}
        className="flake-layer-image flake-layer-image--pending" src={pendingImage.imageUrl} alt="" aria-hidden="true" draggable={false} style={imageGeometry(pendingImage)}
        onLoad={event => commitImage(pendingImage, event.currentTarget)} onError={() => failImage(pendingImage)} />}
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
