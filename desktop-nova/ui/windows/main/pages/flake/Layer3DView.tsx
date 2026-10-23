import React, { useEffect, useRef, useState } from "react";
import { inspectProject } from "../../../../../src/platform/applicationOperations";
import { updateSceneView } from "../../../../../src/platform/sceneView";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Slider } from "../../../../components/ui/slider";
import { useQuickPreview } from "../../useQuickPreview";
import "./layer3D.css";

export function Layer3DView({ sourcePath, blocked = false }: { sourcePath: string; blocked?: boolean }) {
  useI18n();
  const preview = useQuickPreview(sourcePath, blocked);
  const [bounds, setBounds] = useState<[number, number] | null>(null);
  const [range, setRange] = useState<[number, number]>([0, 0]);
  const [selection, setSelection] = useState<"single" | "range" | "all">("single");
  const [error, setError] = useState("");
  const operationId = preview.operation?.previewId;
  const ready = preview.operation?.state === "ready";
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
      setBounds([min, max]); setRange([min, min]); setSelection("single");
    }).catch(reason => { if (current) setError(String(reason)); });
    void preview.start();
    return () => { current = false; };
  }, [sourcePath]);
  useEffect(() => {
    if (!ready || !bounds) return;
    const expected = ++revision.current;
    void updateSceneView({ sourcePath, controls: {
      layerMinY: selection === "all" ? null : range[0],
      layerMaxY: selection === "all" ? null : range[1],
    } }).then(() => { if (expected === revision.current) setError(""); })
      .catch(reason => { if (expected === revision.current) setError(String(reason)); });
  }, [sourcePath, ready, operationId, bounds, range, selection]);
  useEffect(() => () => {
    ++revision.current;
    void updateSceneView({ sourcePath, controls: { layerMinY: null, layerMaxY: null } }).catch(() => undefined);
  }, [sourcePath]);
  const change = (minimum: number, maximum: number, mode = selection) => {
    if (!bounds || !Number.isFinite(minimum) || !Number.isFinite(maximum)) return;
    const low = Math.max(bounds[0], Math.min(bounds[1], Math.round(minimum)));
    const high = Math.max(low, Math.min(bounds[1], Math.round(maximum)));
    const nextMode = mode === "all" ? "single" : mode;
    setRange([low, nextMode === "single" ? low : high]);
    setSelection(nextMode);
  };
  const step = (delta: number) => {
    if (!bounds) return;
    const movement = Math.max(bounds[0] - range[0], Math.min(bounds[1] - range[1], delta));
    change(range[0] + movement, range[1] + movement, selection === "all" ? "single" : selection);
  };
  return <section className="layer3d-view">
    <div className="layer3d-controls">
      <Button aria-pressed={selection === "single"} variant={selection === "single" ? "default" : "outline"} onClick={() => change(range[0], range[0], "single")}>{t("layerView.single")}</Button>
      <Button aria-pressed={selection === "range"} variant={selection === "range" ? "default" : "outline"} onClick={() => { setSelection("range"); }}>{t("layerView.range")}</Button>
      <Button aria-pressed={selection === "all"} variant={selection === "all" ? "default" : "outline"} onClick={() => setSelection("all")}>{t("layerView.all")}</Button>
      <Button disabled={!bounds || range[0] <= bounds[0]} onClick={() => step(-1)}>{t("flake.previousLayer")}</Button>
      <Button disabled={!bounds || range[1] >= bounds[1]} onClick={() => step(1)}>{t("flake.nextLayer")}</Button>
      <label className="layer3d-number">Y <Input type="number" min={bounds?.[0]} max={bounds?.[1]} value={range[0]} disabled={!bounds} onChange={event => change(Number(event.target.value), range[1])} /></label>
      {selection === "range" && <label className="layer3d-number">– <Input aria-label={t("layerView.maximum")} type="number" min={range[0]} max={bounds?.[1]} value={range[1]} disabled={!bounds} onChange={event => change(range[0], Number(event.target.value))} /></label>}
      <Button disabled={preview.pending} onClick={() => void preview.start()}>{t("layerView.show")}</Button>
    </div>
    <Slider className="layer3d-slider" aria-label={t("layerView.layers")} min={bounds?.[0] ?? 0} max={bounds ? Math.max(bounds[0] + 1, bounds[1]) : 1}
      value={selection === "range" ? range : [range[0]]} disabled={!bounds || bounds[0] === bounds[1]}
      onValueChange={values => change(values[0], values[1] ?? values[0], selection === "range" ? "range" : "single")} />
    {(error || preview.error) && <div role="alert" className="nova-error">{error || preview.error}</div>}
    <div ref={preview.hostRef} className="layer3d-viewport" aria-label={t("layerView.viewport")}>
      {!ready && <div className="nova-empty-state">{preview.operation?.state === "starting" ? t("render.preparingFast") : t("render.notLoaded")}</div>}
    </div>
  </section>;
}
