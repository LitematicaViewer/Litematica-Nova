import React, { useEffect, useRef, useState } from "react";
import { t } from "../../../../../src/services/i18n";
import { updateSceneView, type SceneViewControls } from "../../../../../src/platform/sceneView";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../../../components/ui/select";
import { Button } from "../../../../components/ui/button";

type CameraMode = "orbit" | "free";
type CameraInputMode = "drag" | "pointer_lock";
type RememberedView = { mode: CameraMode; inputMode: CameraInputMode; speed: number };
const defaultView: RememberedView = { mode: "orbit", inputMode: "drag", speed: 12 };
type DisplayMode = "fast_experimental" | "standard" | "full";
const rememberedViews = new Map<string, RememberedView>();

export function FreeCameraControls({ sourcePath, displayMode, ready }: {
  sourcePath: string; displayMode: DisplayMode; ready: boolean;
}) {
  const identity = `${displayMode}\n${sourcePath}`;
  const [view, setView] = useState(() => rememberedViews.get(identity) ?? defaultView);
  const currentIdentity = useRef(identity); currentIdentity.current = identity;
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    setView(rememberedViews.get(identity) ?? defaultView);
    setError("");
  }, [identity]);
  useEffect(() => {
    if (!ready) return;
    const saved = rememberedViews.get(identity);
    if (!saved) return;
    let current = true;
    void updateSceneView({ sourcePath, displayMode, controls: { cameraMode: saved.mode, cameraInputMode: saved.inputMode, cameraSpeed: saved.speed } })
      .catch(failure => { if (current) setError(String(failure)); });
    return () => { current = false; };
  }, [identity, ready]);
  const apply = async (controls: SceneViewControls, next = view) => {
    setPending(true); setError("");
    try {
      await updateSceneView({ sourcePath, displayMode, controls });
      rememberedViews.set(identity, next); if (currentIdentity.current === identity) setView(next);
    } catch (failure) { if (currentIdentity.current === identity) setError(String(failure)); }
    finally { setPending(false); }
  };
  return <div className="free-camera-group">
    <div className="render-page-actions free-camera-controls" role="group" aria-label={t("freeCamera.title")}>
      <Button variant={view.mode === "orbit" ? "default" : "ghost"} aria-pressed={view.mode === "orbit"}
        disabled={!ready || pending} onClick={() => void apply({ cameraMode: "orbit" }, { ...view, mode: "orbit" })}>{t("freeCamera.orbit")}</Button>
      <Button variant={view.mode === "free" ? "default" : "ghost"} aria-pressed={view.mode === "free"}
        disabled={!ready || pending} onClick={() => void apply({ cameraMode: "free", cameraInputMode: view.inputMode, cameraSpeed: view.speed }, { ...view, mode: "free" })}>{t("freeCamera.free")}</Button>
      {view.mode === "free" && <Select value={view.inputMode} disabled={!ready || pending}
        onValueChange={value => { const inputMode = value as CameraInputMode; void apply({ cameraInputMode: inputMode }, { ...view, inputMode }); }}>
        <SelectTrigger size="sm" aria-label={t("freeCamera.inputMode")}><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="drag">{t("freeCamera.drag")}</SelectItem>
          <SelectItem value="pointer_lock">{t("freeCamera.pointerLock")}</SelectItem>
        </SelectContent>
      </Select>}
      <label>{t("freeCamera.speed")} <select className="nova-control" value={view.speed} disabled={!ready || pending}
        onChange={event => { const speed = Number(event.target.value); void apply({ cameraSpeed: speed }, { ...view, speed }); }}>
        {[1, 4, 12, 32, 96, 256].map(speed => <option key={speed} value={speed}>{speed} {t("freeCamera.speedUnit")}</option>)}
      </select></label>
      <Button variant="ghost" disabled={!ready || pending} onClick={() => void apply({ resetCamera: true, cameraMode: "orbit" }, { ...view, mode: "orbit" })}>{t("freeCamera.reset")}</Button>
      {view.mode === "free" && <span className="render-preview-help">{t(view.inputMode === "pointer_lock" ? "freeCamera.lockHint" : "freeCamera.dragHint")}</span>}
    </div>
    {error && <div className="render-error" role="alert">{error}</div>}
  </div>;
}
