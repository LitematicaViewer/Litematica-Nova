import React, { useEffect, useRef, useState } from "react";
import { t } from "../../../../../src/services/i18n";
import { getUserConfig, saveUserConfig, type UserConfigInfo } from "../../../../../src/platform/appData";
import { listenEvent } from "../../../../../src/platform/events";
import { getSceneViewState, releaseCameraInput, updateSceneView, type SceneViewAck, type SceneViewControls } from "../../../../../src/platform/sceneView";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../../../components/ui/select";
import { Button } from "../../../../components/ui/button";

type CameraInputMode = "drag" | "pointer_lock";
type CameraSettings = { inputMode: CameraInputMode; speed: number; sensitivity: number };
type DisplayMode = "fast_experimental" | "standard" | "full";
const defaults: CameraSettings = { inputMode: "drag", speed: 12, sensitivity: 1 };
const fromConfig = ({ config }: UserConfigInfo): CameraSettings => ({
  inputMode: config.camera_input_mode, speed: config.camera_speed, sensitivity: config.camera_sensitivity,
});
const controlsFor = (settings: CameraSettings): SceneViewControls => ({
  cameraInputMode: settings.inputMode, cameraSpeed: settings.speed, cameraSensitivity: settings.sensitivity,
});

export function FreeCameraControls({ sourcePath, displayMode, ready }: {
  sourcePath: string; displayMode: DisplayMode; ready: boolean;
}) {
  const identity = `${displayMode}\n${sourcePath}`;
  const currentIdentity = useRef(identity); currentIdentity.current = identity;
  const [view, setView] = useState(defaults);
  const [configured, setConfigured] = useState(false);
  const [configurationRevision, setConfigurationRevision] = useState(0);
  const [pending, setPending] = useState(false);
  const [locked, setLocked] = useState(false);
  const [error, setError] = useState("");
  const [nativeError, setNativeError] = useState("");
  const viewRef = useRef(view); viewRef.current = view;
  useEffect(() => {
    let alive = true;
    const receive = (info: UserConfigInfo) => {
      if (alive) {
        const value = fromConfig(info); const previous = viewRef.current;
        if (previous.inputMode !== value.inputMode || previous.speed !== value.speed || previous.sensitivity !== value.sensitivity) {
          setConfigurationRevision(revision => revision + 1);
        }
        viewRef.current = value; setView(value); setConfigured(true);
      }
    };
    void getUserConfig().then(receive).catch(failure => { if (alive) setError(String(failure)); });
    const listener = listenEvent<UserConfigInfo>("user-config-changed", event => receive(event.payload));
    return () => { alive = false; void listener.then(stop => stop()); };
  }, []);
  useEffect(() => {
    if (!ready || !configured) { setLocked(false); return; }
    let alive = true;
    const receive = (state: SceneViewAck) => {
      if (!alive || currentIdentity.current !== identity) return;
      setLocked(state.cameraLocked);
      setNativeError(state.error ?? "");
    };
    void updateSceneView({ sourcePath, displayMode, controls: controlsFor(viewRef.current) })
      .then(receive).catch(failure => { if (alive) setError(String(failure)); });
    let querying = false;
    const query = () => {
      if (document.hidden || querying) return;
      querying = true;
      void getSceneViewState({ sourcePath, displayMode }).then(receive)
        .catch(failure => { if (alive) setError(String(failure)); }).finally(() => { querying = false; });
    };
    // Only the native owner writes receipts, on actual state changes.
    const timer = window.setInterval(query, 500);
    const release = () => { void releaseCameraInput({ sourcePath, displayMode }).then(receive)
      .catch(failure => { if (alive) setError(String(failure)); }); };
    const hide = () => { if (document.hidden) release(); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") release(); };
    const modalFocus = (event: FocusEvent) => {
      if (event.target instanceof Element && event.target.closest('[role="dialog"],[role="alertdialog"],[role="menu"],[role="listbox"]')) release();
    };
    document.addEventListener("visibilitychange", hide);
    document.addEventListener("keydown", key);
    document.addEventListener("focusin", modalFocus);
    return () => {
      alive = false; window.clearInterval(timer);
      document.removeEventListener("visibilitychange", hide);
      document.removeEventListener("keydown", key);
      document.removeEventListener("focusin", modalFocus);
      void releaseCameraInput({ sourcePath, displayMode }).catch(console.warn);
    };
  }, [identity, ready, configured, configurationRevision]);
  const apply = async (controls: SceneViewControls, next = view) => {
    setPending(true); setError("");
    try {
      const state = await updateSceneView({ sourcePath, displayMode, controls });
      if (state.error) throw new Error(state.error);
      if (currentIdentity.current !== identity) return;
      setLocked(state.cameraLocked); setView(next);
      const info = await saveUserConfig({ camera_input_mode: next.inputMode, camera_speed: next.speed, camera_sensitivity: next.sensitivity });
      if (currentIdentity.current === identity) setView(fromConfig(info));
    } catch (failure) { if (currentIdentity.current === identity) setError(String(failure)); }
    finally { if (currentIdentity.current === identity) setPending(false); }
  };
  const disabled = !ready || !configured || pending;
  return <div className="free-camera-group">
    <div className="render-page-actions free-camera-controls" role="group" aria-label={t("freeCamera.title")}>
      <Select value={view.inputMode} disabled={disabled}
        onValueChange={value => { const inputMode = value as CameraInputMode; void apply({ cameraInputMode: inputMode, releaseCameraInput: true }, { ...view, inputMode }); }}>
        <SelectTrigger size="sm" aria-label={t("freeCamera.inputMode")}><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="drag">{t("freeCamera.drag")}</SelectItem>
          <SelectItem value="pointer_lock">{t("freeCamera.pointerLock")}</SelectItem>
        </SelectContent>
      </Select>
      <label>{t("freeCamera.speed")} <select className="nova-control" value={view.speed} disabled={disabled}
        onChange={event => { const speed = Number(event.target.value); void apply({ cameraSpeed: speed }, { ...view, speed }); }}>
        {[1, 4, 12, 32, 96, 256].map(speed => <option key={speed} value={speed}>{speed} {t("freeCamera.speedUnit")}</option>)}
      </select></label>
      <label>{t("freeCamera.sensitivity")} <select className="nova-control" value={view.sensitivity} disabled={disabled}
        onChange={event => { const sensitivity = Number(event.target.value); void apply({ cameraSensitivity: sensitivity }, { ...view, sensitivity }); }}>
        {[0.25, 0.5, 1, 1.5, 2, 4].map(value => <option key={value} value={value}>{value}×</option>)}
      </select></label>
      <Button variant="ghost" disabled={disabled} onClick={() => void apply({ resetCamera: true, releaseCameraInput: true })}>{t("freeCamera.reset")}</Button>
      {view.inputMode === "pointer_lock" && <output className="free-camera-status" aria-live="polite">{t(locked ? "freeCamera.locked" : "freeCamera.unlocked")}</output>}
    </div>
    <p className="render-preview-help">{t(view.inputMode === "pointer_lock" ? "freeCamera.lockHint" : "freeCamera.dragHint")}</p>
    {(error || nativeError) && <div className="render-error" role="alert">{error || nativeError}</div>}
  </div>;
}
