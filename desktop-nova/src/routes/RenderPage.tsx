import React, { useEffect, useRef, useState } from "react";
import {
  executeBackend,
  pollCacheBuildTask,
  renderPreviewImage,
  startCacheBuildTask,
  startNativeViewer,
} from "../services/backend";
import { RenderProgress } from "../services/renderService";
import {
  getRenderCacheState,
  stateFromLaunch,
  subscribeRenderCacheStore,
  updateRenderCacheState,
  upsertRenderCacheState,
} from "../services/renderCacheStore";
import { DISPLAY_MODE_OPTIONS, loadDisplayMode, saveDisplayMode } from "../services/renderMode";
import { loadUserConfigMigratingLocalStorage, saveRenderDisplayModeConfig } from "../services/userConfig";
import { loadStructureStats, StatsData } from "../services/statsService";
import { Dropdown } from "../components/Dropdown";
import { MaterialsDialog } from "./StatisticsPage";
import {
  elementToPhysicalRect,
  elementToCssRect,
  getEmbeddedViewerStatus,
  hideEmbeddedViewer,
  isUsableEmbeddedRect,
  showEmbeddedViewer,
  startEmbeddedViewer,
  updateEmbeddedViewerBounds,
} from "../services/embeddedViewer";

function parseProgress(raw: string | null): RenderProgress | null {
  if (!raw) return null;
  try {
    const json = JSON.parse(raw);
    return {
      ready: !!json.ready,
      total_chunks: Number(json.total_chunks || 0),
      built_chunks: Number(json.built_chunks || 0),
      percent: Number(json.percent || 0),
      phase: String(json.phase || ""),
      error: json.error ? String(json.error) : "",
    };
  } catch {
    return null;
  }
}

export function RenderPage({ currentFile, activeRoute }: any) {
  const [buildMode, setBuildModeState] = useState(() => loadDisplayMode());
  const [precacheLayers, setPrecacheLayers] = useState(false);
  const [isBuilding, setIsBuilding] = useState(false);
  const [progress, setProgress] = useState<RenderProgress | null>(null);
  const [stageText, setStageText] = useState("请先打开 .litematic 文件。");
  const [error, setError] = useState("");
  const [cacheReady, setCacheReady] = useState(false);
  const [cacheFile, setCacheFile] = useState("");
  const [previewDataUrl, setPreviewDataUrl] = useState("");
  const [stdoutTail, setStdoutTail] = useState("");
  const [stderrTail, setStderrTail] = useState("");
  const [showMaterials, setShowMaterials] = useState(false);
  const [statsData, setStatsData] = useState<StatsData | null>(null);
  const [embeddedRunning, setEmbeddedRunning] = useState(false);
  const [embeddedError, setEmbeddedError] = useState("");
  const pollInterval = useRef<number | null>(null);
  const previewHostRef = useRef<HTMLDivElement | null>(null);

  const stopPolling = () => {
    if (pollInterval.current !== null) {
      window.clearInterval(pollInterval.current);
      pollInterval.current = null;
    }
  };

  const applyStoredState = () => {
    if (!currentFile) return;
    const stored = getRenderCacheState(currentFile, buildMode);
    if (!stored) {
      setProgress(null);
      setCacheReady(false);
      setCacheFile("");
      setPreviewDataUrl("");
      setStageText("已选择文件，尚未构建 3D cache。");
      return;
    }
    setProgress(stored.progress);
    setCacheFile(stored.cacheFile);
    setPreviewDataUrl(stored.previewPath || "");
    setStageText(stored.stage || "");
    setError(stored.status === "error" ? stored.stage : "");
    setCacheReady(stored.status === "ready");
    setIsBuilding(stored.status === "building");
  };

  useEffect(() => {
    loadUserConfigMigratingLocalStorage()
      .then((info) => setBuildModeState(info.config.render_display_mode as any))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    stopPolling();
    setError("");
    setStdoutTail("");
    setStderrTail("");
    if (currentFile) {
      loadStructureStats(currentFile).then(setStatsData).catch(() => setStatsData(null));
      applyStoredState();
    } else {
      setStatsData(null);
      setProgress(null);
      setCacheReady(false);
      setCacheFile("");
      setPreviewDataUrl("");
      setStageText("请先打开 .litematic 文件。");
    }
  }, [currentFile, buildMode]);

  useEffect(() => subscribeRenderCacheStore(applyStoredState), [currentFile, buildMode]);
  useEffect(() => () => stopPolling(), []);

  useEffect(() => {
    let cancelled = false;
    let resizeObserver: ResizeObserver | null = null;
    let updateTimer: number | null = null;
    let statusTimer: number | null = null;

    const clearUpdateTimer = () => {
      if (updateTimer !== null) {
        window.clearTimeout(updateTimer);
        updateTimer = null;
      }
    };

    const updateBounds = async () => {
      const element = previewHostRef.current;
      if (activeRoute !== "render" || !element) {
        console.log("[LBA_EMBED_VIEWER] route_active=render action=hide reason=route_inactive_or_missing_container");
        await hideEmbeddedViewer().catch(() => undefined);
        return;
      }
      const rectCss = elementToCssRect(element);
      const rectPhysical = elementToPhysicalRect(element);
      if (!isUsableEmbeddedRect(rectCss) || !isUsableEmbeddedRect(rectPhysical)) {
        console.log("[LBA_EMBED_VIEWER]", {
          status: "frontend_bounds_skip",
          reason: "invalid_or_hidden_rect",
          rect_css: rectCss,
            rect_physical: rectPhysical,
        });
        await hideEmbeddedViewer().catch(() => undefined);
        return;
      }
      try {
        console.log("[LBA_EMBED_VIEWER]", {
          status: "frontend_bounds_update",
          rect_css: rectCss,
          rect_physical: rectPhysical,
        });
        await updateEmbeddedViewerBounds(rectPhysical);
      } catch {
        // Bounds updates are best-effort; the next start will recreate the host if needed.
      }
    };

    const scheduleUpdateBounds = () => {
      clearUpdateTimer();
      updateTimer = window.setTimeout(updateBounds, 80);
    };

    const start = async () => {
      await new Promise((resolve) => requestAnimationFrame(() => window.setTimeout(resolve, 120)));
      if (cancelled) return;
      const element = previewHostRef.current;
      if (activeRoute !== "render" || !currentFile || !cacheReady || !element) {
        console.log("[LBA_EMBED_VIEWER] route_active=render action=hide reason=inactive_or_not_ready");
        await hideEmbeddedViewer().catch(() => undefined);
        setEmbeddedRunning(false);
        return;
      }
      try {
        const rectCss = elementToCssRect(element);
        const rectPhysical = elementToPhysicalRect(element);
        if (!isUsableEmbeddedRect(rectCss) || !isUsableEmbeddedRect(rectPhysical)) {
          console.log("[LBA_EMBED_VIEWER]", {
            status: "frontend_start_skip",
            reason: "dom_not_stable",
            file: currentFile,
            mode: buildMode,
            rect_css: rectCss,
            rect_physical: rectPhysical,
          });
          await hideEmbeddedViewer().catch(() => undefined);
          setEmbeddedRunning(false);
          return;
        }
        console.log("[LBA_EMBED_VIEWER] route_active=render action=show reason=visible_container");
        await showEmbeddedViewer().catch(() => undefined);
        console.log("[LBA_EMBED_VIEWER]", {
          status: "frontend_start",
          file: currentFile,
          mode: buildMode,
          rect_css: rectCss,
          rect_physical: rectPhysical,
        });
        const status = await startEmbeddedViewer(currentFile, buildMode, rectPhysical, "render_interactive", cacheFile || undefined);
        if (cancelled) return;
        setEmbeddedRunning(status.running);
        setEmbeddedError(status.error || "");
        resizeObserver = new ResizeObserver(scheduleUpdateBounds);
        resizeObserver.observe(element);
        window.addEventListener("resize", scheduleUpdateBounds);
        window.addEventListener("scroll", scheduleUpdateBounds, true);
        statusTimer = window.setInterval(async () => {
          const next = await getEmbeddedViewerStatus().catch(() => null);
          if (next) console.log("[LBA_EMBED_VIEWER]", { ...next, frontend_status: "poll" });
          if (next?.stdout_tail) setStdoutTail(next.stdout_tail);
          if (next?.stderr_tail) setStderrTail(next.stderr_tail);
          if (next && !next.running) {
            setEmbeddedRunning(false);
            setEmbeddedError(next.error || next.stderr_tail || next.stdout_tail || next.status || "embedded viewer exited");
          }
        }, 1000);
      } catch (err: any) {
        if (cancelled) return;
        setEmbeddedRunning(false);
        setEmbeddedError(String(err));
      }
    };

    start();

    return () => {
      cancelled = true;
      clearUpdateTimer();
      if (statusTimer !== null) window.clearInterval(statusTimer);
      resizeObserver?.disconnect();
      window.removeEventListener("resize", scheduleUpdateBounds);
      window.removeEventListener("scroll", scheduleUpdateBounds, true);
      hideEmbeddedViewer().catch(() => undefined);
      setEmbeddedRunning(false);
    };
  }, [currentFile, buildMode, cacheReady, cacheFile, activeRoute]);

  const buildPreview = async (file: string, mode: string) => {
    try {
      const preview = await renderPreviewImage(file, mode);
      setPreviewDataUrl(preview.data_url);
      updateRenderCacheState(file, mode, { previewPath: preview.data_url, stage: "3D cache 已完成，预览图已生成。" });
    } catch (err: any) {
      setStageText(`3D cache 已完成，但 preview png 生成失败：${err}`);
    }
  };

  const handleBuild = async () => {
    if (!currentFile || isBuilding) return;
    stopPolling();
    setError("");
    setProgress(null);
    setCacheReady(false);
    setCacheFile("");
    setPreviewDataUrl("");
    setStdoutTail("");
    setStderrTail("");
    setIsBuilding(true);
    setStageText("已启动 native viewer cache 构建，等待真实 progress JSON。");

    try {
      const launch = await startCacheBuildTask(currentFile, buildMode);
      setCacheFile(launch.cache_file);
      upsertRenderCacheState(stateFromLaunch(currentFile, buildMode, launch));

      pollInterval.current = window.setInterval(async () => {
        const snapshot = await pollCacheBuildTask();
        setStdoutTail(snapshot.stdout_tail || "");
        setStderrTail(snapshot.stderr_tail || "");
        const nextProgress = parseProgress(snapshot.progress_json);

        if (nextProgress) {
          const stage = `${nextProgress.phase || "building"} ${nextProgress.percent.toFixed(1)}% (${nextProgress.built_chunks}/${nextProgress.total_chunks || "?"})`;
          setProgress(nextProgress);
          setStageText(stage);
          updateRenderCacheState(currentFile, buildMode, {
            progress: nextProgress,
            stage,
            status: nextProgress.ready ? "ready" : "building",
            cacheFile: snapshot.cache_file || launch.cache_file,
          });

          if (nextProgress.error) {
            stopPolling();
            setIsBuilding(false);
            setError(nextProgress.error);
            updateRenderCacheState(currentFile, buildMode, { status: "error", stage: nextProgress.error });
            return;
          }

          if (nextProgress.ready) {
            stopPolling();
            setIsBuilding(false);
            setCacheReady(true);
            const readyCache = snapshot.cache_file || launch.cache_file;
            setCacheFile(readyCache);
            let finalStage = "3D cache 已完成。";
            if (precacheLayers) {
              try {
                await executeBackend("litematica_core.exe", ["cache-layer-meta", readyCache]);
                finalStage = "3D cache 已完成，分层索引可用。";
              } catch (err: any) {
                finalStage = `3D cache 已完成，但分层索引检查失败：${err}`;
              }
            }
            setStageText(finalStage);
            updateRenderCacheState(currentFile, buildMode, { status: "ready", cacheFile: readyCache, stage: finalStage });
            await buildPreview(currentFile, buildMode);
            return;
          }
        } else if (snapshot.running) {
          setStageText("native viewer 正在运行，尚未写出可解析的 progress JSON。");
        }

        if (!snapshot.running) {
          stopPolling();
          setIsBuilding(false);
          if (!nextProgress?.ready) {
            const detail = snapshot.stderr_tail || snapshot.stdout_tail || `exit_code=${snapshot.exit_code}`;
            const message = `构建进程已结束，但没有生成 ready cache。\n${detail}`;
            setError(message);
            setStageText("3D cache 构建失败。");
            updateRenderCacheState(currentFile, buildMode, { status: "error", stage: message });
          }
        }
      }, 500);
    } catch (err: any) {
      setIsBuilding(false);
      setError(String(err));
      setStageText("3D cache 启动失败。");
    }
  };

  const handleOpenPopup = async () => {
    if (!currentFile) {
      setError("请先打开 .litematic 文件。");
      return;
    }
    try {
      setError("");
      setStageText("正在启动弹窗 Viewer。");
      await startNativeViewer(currentFile, buildMode, cacheFile || undefined);
      setStageText("已发送弹窗 Viewer 启动命令。");
    } catch (err: any) {
      const message = `弹窗 Viewer 启动失败：${err}`;
      setError(message);
      setStageText(message);
    }
  };

  const progressWidth = progress ? Math.max(0, Math.min(100, progress.percent)) : 0;
  const fileName = currentFile ? currentFile.split(/[\\/]/).pop() : "未选择文件";

  const setBuildMode = (value: string) => {
    const mode = saveDisplayMode(value);
    setBuildModeState(mode);
    saveRenderDisplayModeConfig(mode).catch(() => undefined);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
        <div style={{ color: "#aaa", fontSize: "0.9em", maxWidth: "30%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={currentFile}>
          {currentFile || "请先在属性页打开 .litematic。"}
        </div>
        <div style={{ fontSize: "1.3em", fontWeight: "bold", textAlign: "center", flex: 1 }}>Render Bridge Page / 渲染页</div>
        <button className="btn" onClick={() => setShowMaterials(true)} disabled={!currentFile || !statsData}>材料列表</button>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <span style={{ color: "#ccc" }}>模式</span>
        <Dropdown value={buildMode} options={DISPLAY_MODE_OPTIONS} onChange={setBuildMode} />
        <label style={{ display: "flex", alignItems: "center", gap: 4, color: "#ccc", marginLeft: 8 }}>
          <input type="checkbox" checked={precacheLayers} onChange={(event) => setPrecacheLayers(event.target.checked)} />
          同时预生成分层
        </label>
        <button className="btn" onClick={handleBuild} disabled={!currentFile || isBuilding}>构建 3D cache</button>
        <button className="btn" onClick={handleOpenPopup} disabled={!currentFile}>打开弹窗 Viewer</button>
        <button className="btn" onClick={() => setStageText("弹窗 Viewer 负责视角控制，打开后可在 Viewer 窗口内按 R 重置视角。")} disabled={!currentFile}>重置视角</button>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 4 }}>
        <div style={{ flex: 1, height: 16, backgroundColor: "#111", border: "2px solid #555", position: "relative" }}>
          <div style={{ height: "100%", width: `${progressWidth}%`, backgroundColor: "#0078d4", transition: "width 0.2s" }} />
        </div>
        <div style={{ color: "#ccc", width: 360, fontSize: "0.9em" }}>
          {isBuilding ? (progress ? `${progress.phase} ${progress.percent.toFixed(1)}% (${progress.built_chunks}/${progress.total_chunks})` : "等待真实进度...") : cacheReady ? "cache 已完成：下方是静态预览，交互请打开弹窗 Viewer。" : stageText}
        </div>
      </div>

      <div style={{ color: "#888", fontSize: "0.85em", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {cacheFile ? `cache=${cacheFile}` : ""}
      </div>

      {error && <pre style={{ color: "#ff6666", background: "rgba(255,0,0,0.1)", padding: 8, border: "1px solid #ff6666", margin: 0, whiteSpace: "pre-wrap" }}>{error}</pre>}
      {embeddedError && <pre style={{ color: "#ffcc66", background: "rgba(255,180,0,0.1)", padding: 8, border: "1px solid #8a6a22", margin: 0, whiteSpace: "pre-wrap" }}>嵌入式 viewer 启动失败，已回退静态预览 / 弹窗 Viewer：{embeddedError}</pre>}
      {(stdoutTail || stderrTail) && (
        <details>
          <summary style={{ color: "#aaa", cursor: "pointer" }}>native viewer stdout/stderr</summary>
          <pre style={{ whiteSpace: "pre-wrap", maxHeight: 180, overflow: "auto", background: "#111", border: "1px solid #444", padding: 8, marginTop: 6 }}>{stderrTail || stdoutTail}</pre>
        </details>
      )}

      <div className="group-box" style={{ flex: 1, display: "flex", flexDirection: "column", marginTop: 4 }}>
        <div className="group-box-title">渲染页</div>
        <div ref={previewHostRef} style={{ flex: 1, backgroundColor: "#222", border: "2px solid #111", display: "flex", alignItems: "center", justifyContent: "center", margin: "16px 8px 8px 8px", overflow: "hidden", position: "relative" }}>
          {embeddedRunning ? (
            <div style={{ position: "absolute", left: 8, bottom: 8, background: "rgba(0,0,0,0.65)", color: "#ccc", border: "1px solid #444", padding: "4px 8px", fontSize: "0.85em", zIndex: 1 }}>
              嵌入式 Viewer 运行中
            </div>
          ) : previewDataUrl ? (
            <div style={{ width: "100%", height: "100%", position: "relative" }}>
              <img src={previewDataUrl} alt="static 3D preview" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block", imageRendering: "auto" }} />
              <div style={{ position: "absolute", left: 8, bottom: 8, background: "rgba(0,0,0,0.65)", color: "#ccc", border: "1px solid #444", padding: "4px 8px", fontSize: "0.85em" }}>
                静态预览；交互请打开弹窗 Viewer
              </div>
            </div>
          ) : !cacheReady && !isBuilding ? (
            <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 16, backgroundColor: "#333", padding: 32, border: "2px solid #555" }}>
              <div style={{ fontSize: "0.9em", color: "#888" }}>RENDER MODE ACTIVE</div>
              <div style={{ fontSize: "1.4em", fontWeight: "bold", textAlign: "center", lineHeight: 1.4 }}>RENDER BRIDGE PAGE<br />渲染页</div>
              <div style={{ fontSize: "1.1em" }}>当前文件：{fileName}</div>
              <div style={{ fontSize: "1.1em", fontWeight: "bold" }}>cache 状态：{error ? "失败" : "未开始"}</div>
              <div style={{ color: "#aaa", fontSize: "0.95em", textAlign: "center" }}>{currentFile ? stageText : "请先在属性页打开 .litematic 文件。"}</div>
              <button className="btn" style={{ padding: "8px 24px", fontSize: "1.1em" }} onClick={handleBuild} disabled={!currentFile}>构建 3D cache</button>
            </div>
          ) : (
            <div style={{ fontSize: "1.2em", color: "#ccc" }}>{stageText}</div>
          )}
        </div>
      </div>

      {showMaterials && statsData && <MaterialsDialog data={statsData} onClose={() => setShowMaterials(false)} currentFile={currentFile} />}
    </div>
  );
}

