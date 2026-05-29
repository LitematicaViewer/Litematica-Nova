import { useEffect, useRef, useState } from "react";
import {
  buildLayerMetaCache,
  openProjectionViewer,
  pollCacheBuildTask,
  renderPreviewImage,
  startCacheBuildTask,
} from "../../../../../src/business/facade";
import { RenderProgress } from "../../../../../src/business/facade";
import {
  getRenderCacheState,
  stateFromLaunch,
  subscribeRenderCacheStore,
  updateRenderCacheState,
  upsertRenderCacheState,
} from "../../../../../src/business/facade";
import { DISPLAY_MODE_OPTIONS, loadDisplayMode, saveDisplayMode } from "../../../../../src/business/facade";
import { loadUserConfigMigratingLocalStorage, saveRenderDisplayModeConfig } from "../../../../../src/business/facade";
import { loadStructureStats, StatsData } from "../../../../../src/business/facade";
import { MaterialsDialog, openMaterialsWithWindowBehavior } from "../statistics/StatisticsPage";
import {
  getEmbeddedViewerStatus,
  hideEmbeddedViewer,
  showEmbeddedViewer,
  startEmbeddedViewer,
  updateEmbeddedViewerBounds,
} from "../../../../../src/business/facade";
import { elementToCssRect, elementToPhysicalRect, isUsableEmbeddedRect } from "../../embeddedViewerGeometry";

const RENDERER_OPTIONS = [
  { value: "rsbin", label: "RSBIN" },
] as const;

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
  const [renderer, setRenderer] = useState<string>(RENDERER_OPTIONS[0].value);
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
                await buildLayerMetaCache(readyCache);
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
      await openProjectionViewer(currentFile, buildMode, cacheFile || undefined);
      setStageText("已发送弹窗 Viewer 启动命令。");
    } catch (err: any) {
      const message = `弹窗 Viewer 启动失败：${err}`;
      setError(message);
      setStageText(message);
    }
  };

  const setBuildMode = (value: string) => {
    const mode = saveDisplayMode(value);
    setBuildModeState(mode);
    saveRenderDisplayModeConfig(mode).catch(() => undefined);
  };

  const progressValue = progress ? Math.max(0, Math.min(100, progress.percent)) : 0;
  const fileName = currentFile ? currentFile.split(/[\\/]/).pop() : "未选择文件";
  const renderStatusText = isBuilding
    ? (progress ? `${progress.phase} ${progress.percent.toFixed(1)}% (${progress.built_chunks}/${progress.total_chunks})` : "等待真实进度...")
    : cacheReady
      ? "cache 已完成：左侧为嵌入式查看器或静态预览。"
      : stageText;
  const cacheStatusLabel = error ? "失败" : cacheReady ? "已完成" : isBuilding ? "构建中" : "未开始";

  return (
    <div className="nova-page render-page">
      <div className="render-page__toolbar">
        <button className="btn render-page__toolbar-button" onClick={() => openMaterialsWithWindowBehavior(currentFile, () => setShowMaterials(true))} disabled={!currentFile || !statsData}>
          材料列表
        </button>
        <label className="render-page__toolbar-field">
          {/* <span className="nova-muted">渲染器</span> */}
          <select className="input render-page__select" value={renderer} onChange={(event) => setRenderer(event.target.value)}>
            {RENDERER_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>{option.label}</option>
            ))}
          </select>
        </label>
      </div>

      {error ? <pre className="nova-error render-page__message">{error}</pre> : null}
      {embeddedError ? <pre className="nova-warning render-page__message">嵌入式 viewer 启动失败，已回退静态预览 / 弹窗 Viewer：{embeddedError}</pre> : null}
      {(stdoutTail || stderrTail) ? (
        <details className="render-page__logs">
          <summary className="nova-summary">native viewer stdout/stderr</summary>
          <pre className="nova-pre nova-pre-medium render-page__log-output">{stderrTail || stdoutTail}</pre>
        </details>
      ) : null}

      <div className="group-box render-page__shell">
        <div className="group-box-title">渲染器主界面</div>
        <div className="render-page__shell-body">
          <section className="group-box render-page__build-panel">
            <div className="group-box-title">构建</div>
            <div className="render-page__build-body">
              <div className="render-page__build-grid">
                <label className="render-page__field">
                  <span className="nova-muted">构建模式</span>
                  <select className="input render-page__select" value={buildMode} onChange={(event) => setBuildMode(event.target.value)}>
                    {DISPLAY_MODE_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>{option.label}</option>
                    ))}
                  </select>
                </label>

                <label className="render-page__check-row render-page__build-check">
                  <input type="checkbox" checked={precacheLayers} onChange={(event) => setPrecacheLayers(event.target.checked)} />
                  <span>预生成分层</span>
                </label>
              </div>

              <div className="render-page__action-row">
                <button className="btn" onClick={handleBuild} disabled={!currentFile || isBuilding}>
                  {isBuilding ? "构建中..." : "开始构建"}
                </button>
                <button className="btn" onClick={handleOpenPopup} disabled={!currentFile}>
                  打开浏览弹窗
                </button>
              </div>

              <div className="render-page__status-card">
                <div className="render-page__status-heading">构建进度</div>
                <progress className="render-page__progress-bar" max={100} value={progressValue} />
                <div className="render-page__status-text">{renderStatusText}</div>
              </div>
            </div>
          </section>

          <div className="render-page__workspace">
            <section className="group-box render-page__viewer-panel">
              <div className="group-box-title">嵌入式查看器</div>
              <div ref={previewHostRef} className="render-page__viewer-host">
                {embeddedRunning ? (
                  <div className="render-page__viewer-badge">嵌入式 Viewer 运行中</div>
                ) : previewDataUrl ? (
                  <div className="render-page__preview-shell">
                    <img src={previewDataUrl} alt="static 3D preview" className="render-page__preview-image" />
                    <div className="render-page__viewer-badge">静态预览；交互请打开弹窗 Viewer</div>
                  </div>
                ) : !cacheReady && !isBuilding ? (
                  <div className="render-page__empty-state">
                    <div className="render-page__empty-kicker">RENDER MODE ACTIVE</div>
                    <div className="render-page__empty-title">RENDER BRIDGE PAGE / 渲染页</div>
                    <div className="render-page__empty-meta">当前文件：{fileName}</div>
                    <div className="render-page__empty-meta">cache 状态：{cacheStatusLabel}</div>
                    <div className="render-page__empty-text">{currentFile ? stageText : "请先在属性页打开 .litematic 文件。"}</div>
                    <button className="btn nova-button-wide render-page__empty-action" onClick={handleBuild} disabled={!currentFile}>
                      开始构建
                    </button>
                  </div>
                ) : (
                  <div className="render-page__viewer-text">{stageText}</div>
                )}
              </div>
            </section>

            <aside className="group-box render-page__control-panel">
              <div className="group-box-title">控制面板</div>
              <div className="render-page__control-body">
                <div className="render-page__action-row">
                  <button className="btn" onClick={() => setStageText("弹窗 Viewer 负责视角控制，打开后可在 Viewer 窗口内按 R 重置视角。")} disabled={!currentFile}>
                    重置视角
                  </button>
                </div>

                <div className="render-page__info-list">
                  <div className="render-page__info-row">
                    <span className="nova-muted">当前文件</span>
                    <span className="render-page__info-value" title={currentFile || ""}>{currentFile || "请先在属性页打开 .litematic。"}</span>
                  </div>
                  <div className="render-page__info-row">
                    <span className="nova-muted">cache 状态</span>
                    <span className="render-page__info-value">{cacheStatusLabel}</span>
                  </div>
                  <div className="render-page__info-row">
                    <span className="nova-muted">cache 文件</span>
                    <span className="render-page__info-value" title={cacheFile || ""}>{cacheFile || "尚未生成"}</span>
                  </div>
                </div>
              </div>
            </aside>
          </div>
        </div>
      </div>

      {showMaterials && statsData ? <MaterialsDialog data={statsData} onClose={() => setShowMaterials(false)} currentFile={currentFile} /> : null}
    </div>
  );
}