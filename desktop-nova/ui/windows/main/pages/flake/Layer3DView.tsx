import React, { useEffect, useRef, useState } from "react";
import { inspectProject } from "../../../../../src/platform/applicationOperations";
import { updateSceneView } from "../../../../../src/platform/sceneView";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Slider } from "../../../../components/ui/slider";
import { useFullPreview } from "../../useFullPreview";
import { listenEvent } from "../../../../../src/platform/events";
import { FreeCameraControls } from "../render/FreeCameraControls";
import { shiftLayerRange } from "./layer3DSelection";
import "./layer3D.css";

type LayerSelection = "single" | "range" | "all";
const rememberedLayers = new Map<string, { range: [number, number]; selection: LayerSelection; previousRange: [number, number] }>();

export function Layer3DView({ sourcePath, blocked = false }: { sourcePath: string; blocked?: boolean }) {
  useI18n();
  const preview = useFullPreview(sourcePath, false, blocked, "standard");
  const [bounds, setBounds] = useState<[number, number] | null>(null);
  const [range, setRange] = useState<[number, number]>([0, 0]);
  const [selection, setSelection] = useState<"single" | "range" | "all">("single");
  const [error, setError] = useState("");
  const [occupied, setOccupied] = useState<number[]>([]);
  const previousRange = useRef<[number, number]>([0, 0]);
  const operationId = preview.operation?.operationId;
  const ready = preview.ready;
  const startRef = useRef(preview.start); startRef.current = preview.start;
  const started = useRef<string | null>(null);
  const latestStep = useRef<(delta: number) => void>(() => undefined);
  const revision = useRef(0);
  useEffect(() => {
    let current = true;
    setBounds(null); setError("");
    void inspectProject({ sourcePath }).then(analysis => {
      if (!current) return;
      const intervals = analysis.regions.filter(region => region.size.y !== 0).map(region => [
        region.position.y + Math.min(0, region.size.y + 1),
        region.position.y + Math.max(0, region.size.y - 1),
      ]);
      if (!intervals.length) throw new Error(t("layerView.noLayers"));
      const min = Math.min(...intervals.map(value => value[0]));
      const max = Math.max(...intervals.map(value => value[1]));
      const remembered = rememberedLayers.get(sourcePath);
      const clamp = (value: number) => Math.max(min, Math.min(max, value));
      const restored: [number, number] = remembered ? [clamp(remembered.range[0]), clamp(remembered.range[1])] : [min, min];
      setBounds([min, max]); setRange(restored); setSelection(remembered?.selection ?? "single");
      previousRange.current = remembered ? [clamp(remembered.previousRange[0]), clamp(remembered.previousRange[1])] : [min, min];
      setOccupied(analysis.layers_summary.filter(layer => layer.total_non_air_blocks > 0).map(layer => layer.world_y));
    }).catch(reason => { if (current) setError(String(reason)); });
    started.current = null;
    return () => { current = false; };
  }, [sourcePath]);
  // Entering 3D prepares the existing real Standard scene once; choosing a
  // layer never starts another task or introduces a second confirmation.
  useEffect(() => {
    if (!bounds || blocked || started.current === sourcePath) return;
    const frame = requestAnimationFrame(() => {
      started.current = sourcePath;
      void startRef.current();
    });
    return () => cancelAnimationFrame(frame);
  }, [sourcePath, bounds, blocked]);
  const queued = useRef<{ sourcePath: string; low: number | null; high: number | null; revision: number } | null>(null);
  const applying = useRef(false);
  useEffect(() => {
    const version = ++revision.current;
    if (!ready || !bounds || blocked) { queued.current = null; return; }
    queued.current = { sourcePath, low: selection === "all" ? null : range[0], high: selection === "all" ? null : range[1], revision: version };
    if (applying.current) return;
    applying.current = true;
    void (async () => {
      try {
        while (queued.current) {
          const next = queued.current; queued.current = null;
          try {
            await updateSceneView({ sourcePath: next.sourcePath, displayMode: "standard", controls: { layerMinY: next.low, layerMaxY: next.high } });
            if (revision.current === next.revision) setError("");
          } catch (reason) { if (revision.current === next.revision) setError(String(reason)); }
        }
      } finally { applying.current = false; }
    })();
  }, [sourcePath, ready, operationId, bounds, range, selection, blocked]);
  useEffect(() => {
    let disposed = false;
    const unsubscribe = listenEvent<{ sourcePath: string; operationId: string; delta: number }>("application_scene_layer_step", event => {
      if (!disposed && !blocked && event.payload.operationId === operationId && event.payload.sourcePath === sourcePath) latestStep.current(event.payload.delta);
    });
    return () => { disposed = true; void unsubscribe.then(unlisten => unlisten()); };
  }, [sourcePath, operationId, blocked]);
  useEffect(() => () => { ++revision.current; queued.current = null; }, [sourcePath]);
  const change = (minimum: number, maximum: number, mode = selection) => {
    if (!bounds || !Number.isFinite(minimum) || !Number.isFinite(maximum)) return;
    const low = Math.max(bounds[0], Math.min(bounds[1], Math.round(minimum)));
    const high = Math.max(low, Math.min(bounds[1], Math.round(maximum)));
    const nextMode = mode === "all" ? "single" : mode;
    if (nextMode === "range") previousRange.current = [low, high];
    setRange([low, nextMode === "single" ? low : high]);
    setSelection(nextMode);
  };
  const step = (delta: number) => {
    if (!bounds) return;
    const [low, high] = shiftLayerRange(range, bounds, delta);
    change(low, high, selection === "all" ? "single" : selection);
  };
  latestStep.current = step;
  useEffect(() => {
    if (bounds) rememberedLayers.set(sourcePath, { range, selection, previousRange: previousRange.current });
  }, [sourcePath, bounds, range, selection]);
  const noBlocks = selection !== "all" && !occupied.some(y => y >= range[0] && y <= range[1]);
  return <section className="layer3d-view" onKeyDown={event => {
    if (event.target instanceof HTMLElement && event.target.closest("input,textarea,select,[contenteditable=true]")) return;
    if (event.key === "PageUp" || event.key === "PageDown") { event.preventDefault(); step(event.key === "PageUp" ? 1 : -1); }
  }}>
    <div className="layer3d-controls">
      <Button aria-pressed={selection === "single"} variant={selection === "single" ? "default" : "outline"} onClick={() => change(range[0], range[0], "single")}>{t("layerView.single")}</Button>
      <Button aria-pressed={selection === "range"} variant={selection === "range" ? "default" : "outline"} onClick={() => change(previousRange.current[0], previousRange.current[1], "range")}>{t("layerView.range")}</Button>
      <Button aria-pressed={selection === "all"} variant={selection === "all" ? "default" : "outline"} onClick={() => setSelection("all")}>{t("layerView.all")}</Button>
      <Button disabled={!bounds || range[0] <= bounds[0]} onClick={() => step(-1)}>{t("flake.previousLayer")}</Button>
      <Button disabled={!bounds || range[1] >= bounds[1]} onClick={() => step(1)}>{t("flake.nextLayer")}</Button>
      <label className="layer3d-number">Y <Input type="number" min={bounds?.[0]} max={bounds?.[1]} value={range[0]} disabled={!bounds} onChange={event => change(Number(event.target.value), range[1])} /></label>
      {selection === "range" && <label className="layer3d-number">– <Input aria-label={t("layerView.maximum")} type="number" min={range[0]} max={bounds?.[1]} value={range[1]} disabled={!bounds} onChange={event => change(range[0], Number(event.target.value))} /></label>}

    </div>
    <FreeCameraControls sourcePath={sourcePath} displayMode="standard" ready={ready && preview.visible} />
    <Slider className="layer3d-slider" aria-label={t("layerView.layers")} min={bounds?.[0] ?? 0} max={bounds ? Math.max(bounds[0] + 1, bounds[1]) : 1}
      value={selection === "range" ? range : [range[0]]} disabled={!bounds || bounds[0] === bounds[1]}
      onValueChange={values => change(values[0], values[1] ?? values[0], selection === "range" ? "range" : "single")} />
    {ready && noBlocks && <div role="status">{t("layerView.noBlocks")}</div>}
    {preview.error && <Button disabled={preview.pending} onClick={() => void preview.start()}>{t("render.reload")}</Button>}
    {(error || preview.error) && <div role="alert" className="nova-error">{error || preview.error}</div>}
    <div ref={preview.hostRef} className="layer3d-viewport" aria-label={t("layerView.viewport")}>
      {!preview.visible && <div className="nova-empty-state">{preview.progressLabel}</div>}
    </div>
  </section>;
}
