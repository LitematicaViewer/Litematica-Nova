import { displayPathText } from "../../../src/business/facade";
import { displayFilePath } from "../../../src/business/facade";
import { X } from "lucide-react";
import { Button } from "../../components/ui/button";
import { ScrollArea } from "../../components/ui/scroll-area";
import { Dialog } from "../../components/Dialog";
import { Input } from "../../components/ui/input";
import { useEffect, useLayoutEffect, useRef, useState } from "react";

import {
  getRedenSizeRules,
  importDownloadedLitematic,
  notifyRedenImport,
  redenExternalDriveLink,
  openExternalUrl,
  loadLibrary,
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  openRedenLibraryWindow as openRedenLibraryDesktopWindow,
  RedenMachine,
  RedenSizes,
  RedenDownloadOutput,
  validateRedenSizes,
} from "../../../src/business/facade";
import { ONLINE_PROJECTION_PROVIDERS, searchOnlineSource, fetchOnlineSourceDetail, downloadOnlineSource, cancelOnlineRequest, onlineRequestId } from "../../../src/business/reden";
import { emitEvent } from "../../../src/platform/events";
import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";
import { projectionLibraryImportedEvent } from "../libraryEvents";
import "../main/pages/library/library.css";

function formatSize(numBytes: number): string {
  let value = Math.max(0, numBytes);
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  while (value >= 1024.0 && index < units.length - 1) {
    value /= 1024.0;
    index++;
  }
  return index === 0 ? `${Math.floor(value)} ${units[index]}` : `${value.toFixed(1)} ${units[index]}`;
}

function formatDate(timestamp: number): string {
  if (!timestamp) return "-";
  const d = new Date(timestamp);
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function formatRedenDate(timestamp?: number): string {
  return timestamp ? formatDate(timestamp) : "-";
}

function redenTags(item: RedenMachine): string {
  const tags = [
    item.categoryTag?.name || item.categoryTag?.tag,
    ...(item.featureTags || []).map((tag) => tag.name || tag.tag),
    ...(item.versions || []),
  ].filter(Boolean);
  return tags.length ? tags.join(" / ") : "-";
}

function redenSummary(item: RedenMachine): string {
  return (item.summary || item.description || "").replace(/\s+/g, " ").trim();
}

/**
 * Opens the online projection library according to the user's configured child-window behavior.
 */
export async function openRedenLibraryWithWindowBehavior(showOverlay: () => void): Promise<void> {
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
  const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
  if (behavior === "independent_window") {
    try {
      await openRedenLibraryDesktopWindow();
      return;
    } catch {
      // Browser preview cannot create a desktop window, so fall back to the in-window dialog.
    }
  }
  showOverlay();
}

function RedenLibraryPanel({ onClose }: { onClose?: () => void }) {
  const [redenQuery, setRedenQuery] = useState("刷石机");
  const [redenResults, setRedenResults] = useState<RedenMachine[]>([]);
  const [redenDetail, setRedenDetail] = useState<RedenMachine | null>(null);
  const [redenStatus, setRedenStatus] = useState("");
  const [redenBusy, setRedenBusy] = useState("");
  const [redenSizes, setRedenSizes] = useState<RedenSizes>({});
  const [providerId, setProviderId] = useState("reden");
  const [externalLink, setExternalLink] = useState<{ url: string; label: string; drive: boolean } | null>(null);
  const [searchDirty, setSearchDirty] = useState(false);
  const request = useRef<string | null>(null);
  const provider = ONLINE_PROJECTION_PROVIDERS.find((item) => item.id === providerId) || ONLINE_PROJECTION_PROVIDERS[0];
  const beginRequest = () => {
    if (request.current) void cancelOnlineRequest(request.current).catch(() => undefined);
    const id = onlineRequestId(); request.current = id; return id;
  };
  const cancelRequest = () => {
    const id = request.current; request.current = null; setRedenBusy(""); setSearchDirty(false);
    if (id) void cancelOnlineRequest(id).catch(() => undefined);
    setRedenStatus("已取消等待任务；不再新增该任务的网站请求。");
  };
  useEffect(() => () => { if (request.current) void cancelOnlineRequest(request.current).catch(() => undefined); }, []);
  useEffect(() => {
    if (!searchDirty || provider.mode !== "native") return;
    const timer = setTimeout(() => void handleRedenSearch(), 500);
    return () => clearTimeout(timer);
  }, [redenQuery, searchDirty, providerId]);

  const handleRedenSearch = async () => {
    const query = redenQuery.trim();
    if (!query) {
      setRedenStatus("请输入搜索关键词。");
      return;
    }
    const id = beginRequest(); setSearchDirty(false); setRedenBusy("search");
    setRedenStatus("");
    setRedenDetail(null);
    try {
      const response = await searchOnlineSource(providerId, query, id);
      if (request.current !== id) return;
      const results = response.d || [];
      setRedenResults(results);
      setRedenStatus(results.length ? `${provider.name}：本页 ${results.length} 个结果。` : `${provider.name}：没有结果。`);
    } catch (err: any) {
      if (request.current === id) setRedenStatus(`${provider.name} 查询失败：${String(err)}`);
    } finally {
      if (request.current === id) { request.current = null; setRedenBusy(""); }
    }
  };

  const handleRedenDetail = async (machineId: string, downloadFiles = false) => {
    const id = beginRequest();
    setRedenBusy(`detail:${machineId}`);
    setRedenStatus("");
    try {
      const detail = await fetchOnlineSourceDetail(providerId, machineId, id, downloadFiles);
      if (request.current !== id) return;
      setRedenDetail(detail);
      const rules = getRedenSizeRules(detail);
      setRedenSizes({
        xSize: detail.hasX ? rules.x.min : undefined,
        ySize: detail.hasY ? rules.y.min : undefined,
        zSize: detail.hasZ ? rules.z.min : undefined,
      });
      setRedenStatus(`${provider.name}：已加载详情 ${detail.name || detail.key}。`);
    } catch (err: any) {
      if (request.current === id) setRedenStatus(`${provider.name} 详情失败：${String(err)}`);
    } finally {
      if (request.current === id) { request.current = null; setRedenBusy(""); }
    }
  };

  const importRedenPath = async (path: string) => {
    const libraryState = await loadLibrary();
    await importDownloadedLitematic(libraryState, path);
    return await notifyRedenImport(() => emitEvent(projectionLibraryImportedEvent, { path }));
  };

  const finishRedenDownload = async (output: RedenDownloadOutput, generated = false, requestId?: string) => {
    const isCurrent = () => !requestId || request.current === requestId;
    if (!isCurrent()) return;
    if (output.kind === "external_link") {
      const drive = ["quark", "baidu"].includes(output.provider);
      const label = output.provider === "quark" ? "夸克网盘" : output.provider === "baidu" ? "百度网盘" : ONLINE_PROJECTION_PROVIDERS.find(item => item.id === output.provider)?.name || "原站";
      setExternalLink({ url: output.url, label, drive });
      setRedenStatus("此附件需在原站下载，是否打开浏览器？");
      return;
    }
    try {
      const warning = await importRedenPath(output.path);
      if (isCurrent()) setRedenStatus(`${provider.name}：已${generated ? "生成、" : ""}下载并入库 ${output.file_name} (${formatSize(output.bytes)})。${warning || ""}`);
    } catch (error) {
      if (isCurrent()) setRedenStatus(`${provider.name}：已下载 ${output.file_name}，但入库失败：${String(error)}。文件保留在 ${displayFilePath(output.path)}`);
    }
  };

  const confirmExternalLink = async () => {
    if (!externalLink) return;
    setRedenBusy("external:open");
    try {
      await openExternalUrl(externalLink.url);
      setRedenStatus(`已在系统浏览器打开 ${externalLink.label}；下载后请从本地导入。`);
      setExternalLink(null);
    } catch (error) { setRedenStatus(`系统浏览器打开失败：${String(error)}`); }
    finally { setRedenBusy(""); }
  };

  const handleRedenDownloadAttachment = async (detail: RedenMachine, index: number) => {
    const driveLink = redenExternalDriveLink(detail.attachments?.[index]?.url);
    if (driveLink) {
      await finishRedenDownload(driveLink);
      return;
    }
    setRedenBusy(`download:${index}`);
    const id = beginRequest();
    setRedenStatus("");
    try {
      const output = await downloadOnlineSource(providerId, detail.key, index, id);
      if (request.current !== id) return;
      await finishRedenDownload(output, false, id);
    } catch (err: any) {
      if (request.current === id) setRedenStatus(`${provider.name} 下载失败：${String(err)}`);
    } finally {
      if (request.current === id) { request.current = null; setRedenBusy(""); }
    }
  };

  const handleRedenDownloadParametric = async (detail: RedenMachine) => {
    const errors = validateRedenSizes(detail, redenSizes);
    if (errors.length) {
      setRedenStatus(`RedenMC 尺寸不合法：${errors.join("；")}`);
      return;
    }
    setRedenBusy("download:parametric");
    const id = beginRequest();
    setRedenStatus("");
    try {
      const output = await downloadOnlineSource(providerId, detail.key, 0, id, redenSizes);
      if (request.current !== id) return;
      await finishRedenDownload(output, true, id);
    } catch (err: any) {
      if (request.current === id) setRedenStatus(`${provider.name} 重复结构下载失败：${String(err)}`);
    } finally {
      if (request.current === id) { request.current = null; setRedenBusy(""); }
    }
  };

  const statusClassName = [
    "subwindow-status-text",
    redenStatus.includes("失败") || redenStatus.includes("不合法") ? "subwindow-status-text-error" : "",
  ].filter(Boolean).join(" ");

  return (
    <section className="subwindow-panel-shell library-aux-panel library-reden-panel">
      <div className="subwindow-title-bar">
        <div className="subwindow-title-stack">
          <h3 className="subwindow-title">在线投影资源库</h3>
        </div>
        {onClose ? <Button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}><X size={18} aria-hidden="true" /></Button> : null}
      </div>

      <div className="subwindow-body">
        <div className="subwindow-toolbar subwindow-wrap-row" aria-label="在线资源来源">
          {ONLINE_PROJECTION_PROVIDERS.map((item) => <Button key={item.id} variant={item.id === providerId ? "default" : "ghost"}
            aria-pressed={item.id === providerId} onClick={() => { cancelRequest(); setProviderId(item.id); setRedenResults([]); setRedenDetail(null); setExternalLink(null); setRedenStatus(""); }}>{item.name}</Button>)}
        </div>
        {provider.mode === "website" ? <section className="reden-detail-stack">
          <h4>{provider.name} · 仅网页访问</h4>
          <p>{provider.url}</p>
          <Button disabled={!!redenBusy} onClick={() => setExternalLink({ url: provider.url, label: provider.name, drive: false })}>在系统浏览器打开原站</Button>
        </section> : <>
        <div className="subwindow-toolbar subwindow-wrap-row">
          <Input
            className="input nova-input-flex"
            value={redenQuery}
            aria-label="搜索在线投影"
            placeholder="搜索机器，例如：刷石机、世界吞噬者"
            onChange={(event) => { setRedenQuery(event.target.value); setSearchDirty(true); }}
            onKeyDown={(event) => { if (event.key === "Enter") handleRedenSearch(); }}
          />
          <Button className="btn" disabled={!!redenBusy} onClick={handleRedenSearch}>
            {redenBusy === "search" ? "搜索中..." : "搜索"}
          </Button>
          {redenBusy && <Button variant="ghost" onClick={cancelRequest}>取消等待任务</Button>}
          <Button variant="ghost" onClick={() => setExternalLink({ url: provider.url, label: provider.name, drive: false })}>原站入口</Button>
        </div>

        <div className="reden-content-grid">
          <div className="reden-results-pane">
            <div className="reden-pane-title">搜索结果</div>
            <ScrollArea className="library-online-results-scroll"><div className="reden-result-list">
              {redenResults.length > 0 ? redenResults.map((item) => {
                const resultClassName = [
                  "library-card",
                  "reden-result-card",
                  redenDetail?.key === item.key ? "library-card-current" : "",
                ].filter(Boolean).join(" ");
                return (
                  <Button
                    key={item.key}
                    className={resultClassName}
                    variant="ghost"
                    disabled={!!redenBusy}
                    aria-pressed={redenDetail?.key === item.key}
                    onClick={() => {
                      if (redenBusy === `detail:${item.key}`) return;
                      handleRedenDetail(item.key);
                    }}

                  >
                    <div className="lib-card-left reden-result-card-body">
                      <div className="lib-card-line-main">
                        <div className="lib-card-heading">
                          <span className="lib-card-title">{item.name || item.key}</span>
                        </div>
                      </div>
                      <div className="lib-card-text">来源：{provider.name} · 下载遵循原站门禁</div>
                      <div className="lib-card-text">标签：{redenTags(item)}</div>
                      <div className="reden-result-row">
                        <div className="reden-result-meta">作者：{item.author?.username || "-"}</div>
                        <div className="reden-result-meta reden-result-stats">下载：{item.downloads ?? "-"} / 收藏：{item.upVotes ?? "-"}</div>
                      </div>
                      <div className="reden-result-date">更新：{formatRedenDate(item.updatedAt)}</div>
                      {redenSummary(item) && <div className="reden-result-summary">{redenSummary(item).slice(0, 120)}</div>}
                    </div>
                  </Button>
                );
              }) : (
                <div className="subwindow-empty-state">
                  暂无结果。请输入关键词后开始搜索。
                </div>
              )}
            </div></ScrollArea>
          </div>

          <ScrollArea className="reden-detail-pane">
            {redenDetail ? (
              <div className="reden-detail-stack">
                <div>
                  <div className="reden-detail-title">{redenDetail.name || redenDetail.key}</div>
                  <div className="reden-detail-meta">ID：{redenDetail.key} · 类型：{redenDetail.type || "-"} · 作者：{redenDetail.author?.username || "-"}</div>
                  {redenSummary(redenDetail) && <div className="reden-detail-summary">{redenSummary(redenDetail)}</div>}
                  {redenDetail.pageUrl && <Button variant="ghost" onClick={() => setExternalLink({ url: redenDetail.pageUrl!, label: provider.name, drive: false })}>原始详情页</Button>}
                  {providerId === "minemev" && <Button onClick={() => void finishRedenDownload({ kind: "external_link", provider: "minemev", url: redenDetail.pageUrl || provider.url })}>在原页下载</Button>}
                  {redenDetail.downloadListAvailable && <Button disabled={!!redenBusy} onClick={() => void handleRedenDetail(redenDetail.key, true)}>获取下载附件</Button>}
                </div>

                {(redenDetail.attachments || []).length > 0 && (
                  <div className="reden-section-stack">
                    <div className="reden-section-title">普通附件</div>
                    {(redenDetail.attachments || []).map((attachment, index) => (
                      <div key={`${attachment.name}-${index}`} className="reden-attachment-row">
                        <div className="nova-flex-1">
                          <div>{index + 1}. {attachment.name || `attachment-${index + 1}.litematic`}</div>
                          <div className="reden-attachment-meta">{attachment.size ? formatSize(attachment.size) : "-"} {attachment.description || ""}</div>
                        </div>
                        <Button className="btn" disabled={!!redenBusy} onClick={() => handleRedenDownloadAttachment(redenDetail, index)}>
                          {redenBusy === `download:${index}` ? "下载中..." : "下载并入库"}
                        </Button>
                      </div>
                    ))}
                  </div>
                )}

                {(redenDetail.hasX || redenDetail.hasY || redenDetail.hasZ) && (
                  <div className="reden-section-stack">
                    <div className="reden-section-title">重复结构尺寸</div>
                    <div className="reden-size-grid">
                      {(["x", "y", "z"] as const).map((axis) => {
                        const enabled = axis === "x" ? redenDetail.hasX : axis === "y" ? redenDetail.hasY : redenDetail.hasZ;
                        if (!enabled) return null;
                        const field = `${axis}Size` as keyof RedenSizes;
                        const rules = getRedenSizeRules(redenDetail)[axis];
                        const hint = [
                          rules.min !== undefined ? `min ${rules.min}` : "",
                          rules.max !== undefined ? `max ${rules.max}` : "",
                          rules.mod ? `mod ${rules.mod.step},${rules.mod.offset}` : "",
                        ].filter(Boolean).join(" / ");
                        return (
                          <label key={axis} className="reden-size-field">
                            <span>{axis.toUpperCase()} ({hint || "no rule"})</span>
                            <Input
                              className="input"
                              type="number"
                              value={redenSizes[field] ?? ""}
                              onChange={(event) => setRedenSizes((current) => ({ ...current, [field]: Number(event.target.value) }))}
                            />
                          </label>
                        );
                      })}
                    </div>
                    <Button className="btn reden-parametric-button" disabled={!!redenBusy} onClick={() => handleRedenDownloadParametric(redenDetail)}>
                      {redenBusy === "download:parametric" ? "生成下载中..." : "生成下载并入库"}
                    </Button>
                  </div>
                )}
              </div>
            ) : (
              <div className="subwindow-status-text reden-empty-detail">请选择左侧搜索结果查看详情。</div>
            )}
          </ScrollArea>
        </div>
        </>}

        {redenStatus && (
          <footer className="reden-footer">
            <div className={statusClassName}>
              {redenStatus}
            </div>
          </footer>
        )}
        {externalLink && <Dialog title={`打开 ${externalLink.label}`} onClose={() => setExternalLink(null)} busy={redenBusy === "external:open"}
          footer={<><Button variant="ghost" disabled={redenBusy === "external:open"} onClick={() => setExternalLink(null)}>取消</Button>
            <Button disabled={redenBusy === "external:open"} onClick={() => void confirmExternalLink()}>确认并打开系统浏览器</Button></>}>
          <p>{externalLink.drive ? "这是外部网盘网页，不是投影直链。应用不会绕过登录，也不会把网页下载成投影。" : "将在系统浏览器打开真实原站，登录、付费与验证仍由原站处理。"}</p>
          <p>{externalLink.url}</p>
        </Dialog>}
      </div>
    </section>
  );
}

/**
 * Overlay dialog form of the online projection library.
 */
export function RedenLibraryDialog({ onClose }: { onClose: () => void }) {
  return (
    <Dialog title="下载投影" width="lg" className="library-window-dialog" onClose={onClose}>
      <RedenLibraryPanel />
    </Dialog>
  );
}

/**
 * Renders the standalone desktop window used for the online projection library.
 */
export function RedenLibraryWindow() {
  const [themeId, setThemeId] = useState(currentThemeId);

  useLayoutEffect(() => {
    applyThemeStylesheet(themeId);
  }, [themeId]);

  useEffect(() => {
    setThemeId(currentThemeId());
    return subscribeToThemeChanges(setThemeId);
  }, []);

  return (
    <main className={["subwindow-standalone-page", themeClassName(themeId)].filter(Boolean).join(" ")}>
      <RedenLibraryPanel />
    </main>
  );
}
