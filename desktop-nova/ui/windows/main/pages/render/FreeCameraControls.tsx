import React, { useEffect, useRef, useState } from "react";
import { t } from "../../../../../src/services/i18n";
import { releaseCameraInput, updateSceneView, type SceneViewControls } from "../../../../../src/platform/sceneView";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../../../components/ui/select";
import { Button } from "../../../../components/ui/button";

type CameraInputMode = "drag" | "pointer_lock";
type CameraSettings = { inputMode: CameraInputMode; speed: number; sensitivity: number };
type DisplayMode = "fast_experimental" | "standard" | "full";
const storageKey = "nova.camera.input.v1";
const defaultSettings: CameraSettings = { inputMode: "drag", speed: 12, sensitivity: 1 };
function loadSettings(): CameraSettings {
  try {
    const saved = JSON.parse(localStorage.getItem(storageKey) ?? "null");
    return {
      inputMode: saved?.inputMode === "pointer_lock" ? "pointer_lock" : "drag",
      speed: [1, 4, 12, 32, 96, 256].includes(saved?.speed) ? saved.speed : 12,
      sensitivity: [0.25, 0.5, 1, 1.5, 2, 4].includes(saved?.sensitivity) ? saved.sensitivity : 1,
    };
  } catch { return defaultSettings; }
}
function controlsFor(settings: CameraSettings): SceneViewControls {
  return { cameraInputMode: settings.inputMode, cameraSpeed: settings.speed, cameraSensitivity: settings.sensitivity };
}

export function FreeCameraControls({ sourcePath, displayMode, ready }: {
  sourcePath: string; displayMode: DisplayMode; ready: boolean;
}) {
  const identity = `${displayMode}\n${sourcePath}`;
  const [view, setView] = useState(loadSettings);
  const currentIdentity = useRef(identity); currentIdentity.current = identity;
  const viewRef = useRef(view); viewRef.current = view;
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!ready) return;
    let current = true;
    void updateSceneView({ sourcePath, displayMode, controls: controlsFor(viewRef.current) })
      .catch(failure => { if (current) setError(String(failure)); });
    return () => { current = false; };
  }, [identity, ready]);
  useEffect(() => {
    if (!ready) return;
    const release = () => { void releaseCameraInput({ sourcePath, displayMode }).catch(failure => {
      if (currentIdentity.current === identity) setError(String(failure));
    }); };
    const hide = () => { if (document.hidden) release(); };
    // Focusing the native child blurs WebView itself; the native owner handles
    // true window focus loss without treating that normal handoff as an exit.
    document.addEventListener("visibilitychange", hide);
    return () => { document.removeEventListener("visibilitychange", hide); };
  }, [identity, ready]);
  const apply = async (controls: SceneViewControls, next = view) => {
    setPending(true); setError("");
    try {
      await updateSceneView({ sourcePath, displayMode, controls });
      if (currentIdentity.current !== identity) return;
      setView(next);
      localStorage.setItem(storageKey, JSON.stringify(next));
    } catch (failure) { if (currentIdentity.current === identity) setError(String(failure)); }
    finally { if (currentIdentity.current === identity) setPending(false); }
  };
  return <div className="free-camera-group">
    <div className="render-page-actions free-camera-controls" role="group" aria-label={t("freeCamera.title")}>
      <Select value={view.inputMode} disabled={!ready || pending}
        onValueChange={value => { const inputMode = value as CameraInputMode; void apply({ cameraInputMode: inputMode, releaseCameraInput: true }, { ...view, inputMode }); }}>
        <SelectTrigger size="sm" aria-label={t("freeCamera.inputMode")}><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="drag">{t("freeCamera.drag")}</SelectItem>
          <SelectItem value="pointer_lock">{t("freeCamera.pointerLock")}</SelectItem>
        </SelectContent>
      </Select>
      <label>{t("freeCamera.speed")} <select className="nova-control" value={view.speed} disabled={!ready || pending}
        onChange={event => { const speed = Number(event.target.value); void apply({ cameraSpeed: speed }, { ...view, speed }); }}>
        {[1, 4, 12, 32, 96, 256].map(speed => <option key={speed} value={speed}>{speed} {t("freeCamera.speedUnit")}</option>)}
      </select></label>
      <label>{t("freeCamera.sensitivity")} <select className="nova-control" value={view.sensitivity} disabled={!ready || pending}
        onChange={event => { const sensitivity = Number(event.target.value); void apply({ cameraSensitivity: sensitivity }, { ...view, sensitivity }); }}>
        {[0.25, 0.5, 1, 1.5, 2, 4].map(value => <option key={value} value={value}>{value}×</option>)}
      </select></label>
      <Button variant="ghost" disabled={!ready || pending} onClick={() => void apply({ resetCamera: true, releaseCameraInput: true })}>{t("freeCamera.reset")}</Button>
      <span className="render-preview-help">{t(view.inputMode === "pointer_lock" ? "freeCamera.lockHint" : "freeCamera.dragHint")}</span>
    </div>
    {error && <div className="render-error" role="alert">{error}</div>}
  </div>;
}
