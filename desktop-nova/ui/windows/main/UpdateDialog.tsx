import React, { useEffect, useRef, useState } from "react";
import { Download } from "lucide-react";
import { Button } from "../../components/ui/button";
import { Dialog } from "../../components/Dialog";
import { cancelUpdate, checkForUpdate, downloadUpdate, installUpdate, onUpdateState, readUpdateState, restartAfterUpdateFailure, skipUpdate, type UpdateView } from "../../../src/platform/updater";
import { clearCurrentUpdateDrafts, preserveUpdateDrafts, releaseNoteSections } from "./updateRecovery";
import "../../styles/updater.css";

let startupCheck: Promise<UpdateView> | undefined;

export function UpdateDialog() {
  const [view, setView] = useState<UpdateView | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState("");
  const action = useRef(false);
  const [running, setRunning] = useState(false);
  useEffect(() => {
    let live = true;
    const accept = (next: UpdateView) => {
      if (!live) return;
      setView(next);
      if (next.phase === "available") setOpen(true);
    };
    const listening = onUpdateState(accept);
    // Subscribe first: the automatic check is asynchronous and shared across StrictMode mounts.
    void listening.then(async () => {
      accept(await readUpdateState());
      startupCheck ??= checkForUpdate(false);
      accept(await startupCheck);
    }).catch(() => undefined);
    return () => { live = false; void listening.then((stop) => stop()).catch(() => undefined); };
  }, []);

  const manualCheck = async () => {
    if (action.current) return;
    action.current = true; setRunning(true); setError(""); setOpen(true);
    try { setView(await checkForUpdate(true)); } catch (e) { setError(String(e)); }
    finally { action.current = false; setRunning(false); }
  };
  const apply = async () => {
    if (action.current) return;
    action.current = true; setRunning(true); setError("");
    try {
      preserveUpdateDrafts();
      const next = await downloadUpdate(); setView(next);
      if (next.phase !== "downloaded") return;
      preserveUpdateDrafts();
      await installUpdate();
    } catch (e) { setError(String(e)); }
    finally {
      // A cancelled/failed update must not later resurrect old edits during normal Save.
      try { if ((await readUpdateState()).phase !== "restartRequired") clearCurrentUpdateDrafts(); } catch (e) { setError(String(e)); }
      action.current = false; setRunning(false);
    }
  };
  const cancel = async () => {
    if (view?.phase === "installing" || view?.phase === "restartRequired") return;
    try { setView(await cancelUpdate()); setOpen(false); } catch (e) { setError(String(e)); }
  };
  const skip = async () => {
    try { setView(await skipUpdate()); setOpen(false); } catch (e) { setError(String(e)); }
  };
  const installing = view?.phase === "installing" || view?.phase === "restartRequired";
  const canUpdate = !!view?.version && !running && ["available", "error", "cancelled", "downloaded"].includes(view.phase);
  const phaseText = { idle: "尚未检查", disabled: "更新尚未配置", checking: "正在检查更新…", available: "发现新版本", current: "当前已是最新可用版本", skipped: "已跳过当前版本", downloading: "正在下载并校验签名…", downloaded: "签名校验完成", waiting: "等待任务安全退出…", installing: "正在安全退出并安装…", cancelled: "已取消更新", error: "更新未完成", restartRequired: "草稿已保留，需要重启恢复后台服务" };
  return <>
    <Button variant="ghost" className="sidebar-item nav-item updater-check" onClick={manualCheck} disabled={running} title="检查更新" aria-label="检查更新">
      <Download className="nav-icon" aria-hidden="true" /><span className="nav-label">检查更新</span>
    </Button>
    {open && <Dialog title={view?.version ? `更新至 ${view.version}` : "软件更新"} subtitle="使用官方 Tauri Updater；安装前验证签名，保留当前草稿并等待任务退出。" onClose={() => void cancel()} busy={installing} width="md"
      footer={<>
        <Button variant="ghost" onClick={() => void skip()} disabled={!view?.version || running || installing}>跳过当前版本</Button>
        <Button variant="secondary" onClick={() => void cancel()} disabled={installing}>取消</Button>
        {view?.phase === "restartRequired"
          ? <Button onClick={() => void restartAfterUpdateFailure().catch((e) => setError(String(e)))}>重启恢复</Button>
          : <Button onClick={() => void apply()} disabled={!canUpdate}>更新并重启</Button>}
      </>}>
      <p role="status" aria-live="polite">{view ? phaseText[view.phase] : "正在检查更新…"}</p>
      <dl className="update-version"><dt>当前版本</dt><dd>{view?.currentVersion || "—"}</dd><dt>版本</dt><dd>{view?.version || "—"}</dd><dt>日期</dt><dd>{view?.date || "未提供"}</dd></dl>
      {view?.source && <p className="muted">更新源：{view.source}</p>}
      {view?.phase === "downloading" && <div className="update-download"><progress aria-label="更新下载进度" max={view.total || undefined} value={view.total ? Math.min(view.downloaded, view.total) : undefined} /><span>{(view.downloaded / 1048576).toFixed(1)} MB{view.total ? ` / ${(view.total / 1048576).toFixed(1)} MB` : ""}</span></div>}
      {view?.version && releaseNoteSections(view.notes).map(({ label, lines }) => <section className="update-notes" key={label}><h4>{label}</h4><ul>{lines.map((line, i) => <li key={i}>{line}</li>)}</ul></section>)}
      {(error || view?.message) && <p role="alert">{error || view?.message}</p>}
    </Dialog>}
  </>;
}
