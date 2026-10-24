import React, { useEffect, useRef, useState } from "react";
import { t } from "../../../../../src/services/i18n";
import { getUserConfig, type UserConfigInfo, type ViewerControlConfig } from "../../../../../src/platform/appData";
import { listenEvent } from "../../../../../src/platform/events";
import { getSceneViewState, releaseCameraInput, updateSceneView, type SceneViewAck, type SceneViewControls } from "../../../../../src/platform/sceneView";

type DisplayMode = "fast_experimental" | "standard" | "full";
type Weather = "auto" | "clear" | "rain" | "thunder" | "snow";
export function EnvironmentControls({ sourcePath, displayMode, ready, environmentSupported }: {
  sourcePath: string; displayMode: DisplayMode; ready: boolean; environmentSupported: boolean;
}) {
  const identity = `${displayMode}\n${sourcePath}`;
  const currentIdentity = useRef(identity); currentIdentity.current = identity;
  const [permissions, setPermissions] = useState<ViewerControlConfig>({ allow_time_modification: false, allow_weather_modification: false });
  const [time, setTime] = useState(6000);
  const [weather, setWeather] = useState<Weather>("auto");
  const [applied, setApplied] = useState<SceneViewAck | null>(null);
  const appliedRef = useRef(applied); appliedRef.current = applied;
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const queue = useRef<{ identity: string; running: boolean; latest: SceneViewControls | null }>({ identity, running: false, latest: null });
  if (queue.current.identity !== identity) queue.current = { identity, running: false, latest: null };
  useEffect(() => {
    let alive = true;
    const receive = (info: UserConfigInfo) => { if (alive) setPermissions(info.config.viewer_controls); };
    void getUserConfig().then(receive).catch(failure => { if (alive) setError(String(failure)); });
    const listener = listenEvent<UserConfigInfo>("user-config-changed", event => receive(event.payload));
    return () => { alive = false; void listener.then(stop => stop()); };
  }, []);
  const receive = (state: SceneViewAck) => {
    if (currentIdentity.current !== identity || state.sourcePath !== sourcePath || state.displayMode !== displayMode) return;
    setApplied(state); appliedRef.current = state;
    if (!queue.current.running) {
      if (typeof state.worldTime === "number") setTime(state.worldTime);
      if (state.weather) setWeather(state.weather as Weather);
    }
    if (state.error) setError(state.error);
  };
  useEffect(() => {
    setPending(false); setApplied(null); setError("");
    if (!ready || !environmentSupported || displayMode === "fast_experimental") return;
    let alive = true;
    const currentQueue = queue.current;
    void getSceneViewState({ sourcePath, displayMode }).then(state => { if (alive) receive(state); })
      .catch(failure => { if (alive) setError(String(failure)); });
    const listener = listenEvent<SceneViewAck>("scene-view-state", event => { if (alive) receive(event.payload); });
    return () => { alive = false; currentQueue.latest = null; void listener.then(stop => stop()); };
  }, [identity, ready, environmentSupported]);
  const enqueue = (controls: SceneViewControls) => {
    const state = queue.current;
    state.latest = { ...state.latest, ...controls };
    if (state.running) return;
    state.running = true; setPending(true); setError("");
    void (async () => {
      while (state.latest && currentIdentity.current === state.identity) {
        const controls = state.latest; state.latest = null;
        try {
          const ack = await updateSceneView({ sourcePath, displayMode, controls });
          if (ack.error) throw new Error(ack.error);
          if (currentIdentity.current === state.identity) receive(ack);
        } catch (failure) {
          if (currentIdentity.current === state.identity) {
            setError(String(failure));
            if (!state.latest) {
              if (typeof appliedRef.current?.worldTime === "number") setTime(appliedRef.current.worldTime);
              if (appliedRef.current?.weather) setWeather(appliedRef.current.weather as Weather);
            }
          }
        }
      }
      state.running = false;
      if (currentIdentity.current === state.identity) setPending(false);
    })();
  };
  const supported = environmentSupported && displayMode !== "fast_experimental";
  const timeDisabled = !ready || !supported || !permissions.allow_time_modification;
  const weatherDisabled = !ready || !supported || !permissions.allow_weather_modification;
  const confirmed = applied && !applied.error && (typeof applied.worldTime === "number" || applied.weather);
  return <details className="render-environment" onToggle={event => {
    if (event.currentTarget.open && ready) void releaseCameraInput({ sourcePath, displayMode }).catch(failure => setError(String(failure)));
  }}>
    <summary>{t("environment.title")}</summary>
    <div className="render-environment-body">
      {!supported && <p className="render-preview-help">{t("environment.unavailable")}</p>}
      <label className="render-environment-time">{t("environment.time")}
        <input type="range" min={0} max={23999} step={1} value={time} disabled={timeDisabled}
          aria-describedby="render-environment-clock" list="render-environment-ticks"
          onChange={event => { const next = Number(event.currentTarget.value); setTime(next); enqueue({ worldTime: next }); }} />
        <output>{time} {t("environment.ticks")}</output>
      </label>
      <datalist id="render-environment-ticks"><option value="0" label="06:00" /><option value="6000" label="12:00" /><option value="12000" label="18:00" /><option value="18000" label="00:00" /></datalist>
      <p id="render-environment-clock" className="render-preview-help">{t("environment.clockHint")}</p>
      <label className="render-environment-weather">{t("environment.weather")}
        <select className="nova-control" value={weather} disabled={weatherDisabled}
          onChange={event => { const next = event.currentTarget.value as Weather; setWeather(next); enqueue({ weather: next }); }}>
          {(["auto", "clear", "rain", "thunder", "snow"] as const).map(value => <option key={value} value={value}>{t(`environment.weather.${value}`)}</option>)}
        </select>
      </label>
      {weather === "auto" && supported && <p className="render-preview-help">{t("environment.autoHint")}</p>}
      {supported && (!permissions.allow_time_modification || !permissions.allow_weather_modification)
        && <p className="render-preview-help">{t("environment.permissionHint")}</p>}
      <output className="render-environment-applied" aria-live="polite">{t(error ? "environment.failed" : pending ? "environment.applying" : confirmed ? "environment.applied" : "environment.waiting")}</output>
      {error && <div className="render-error" role="alert">{error}</div>}
    </div>
  </details>;
}
