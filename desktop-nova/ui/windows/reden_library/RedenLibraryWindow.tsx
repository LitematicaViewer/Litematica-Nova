import { useEffect, useState } from "react";

import {
  downloadRedenAttachment,
  downloadRedenParametric,
  fetchRedenMachineDetail,
  getRedenSizeRules,
  importDownloadedLitematic,
  loadLibrary,
  RedenMachine,
  RedenSizes,
  searchRedenLitematica,
  validateRedenSizes,
} from "../../../src/business/facade";
import { emitEvent } from "../../../src/platform/events";
import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";
import { projectionLibraryImportedEvent } from "../libraryEvents";

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
 * Renders the RedenMC online projection library window.
 */
export function RedenLibraryWindow() {
  const [themeId, setThemeId] = useState(currentThemeId());
  const [redenQuery, setRedenQuery] = useState("刷石机");
  const [redenResults, setRedenResults] = useState<RedenMachine[]>([]);
  const [redenDetail, setRedenDetail] = useState<RedenMachine | null>(null);
  const [redenStatus, setRedenStatus] = useState("");
  const [redenBusy, setRedenBusy] = useState("");
  const [redenSizes, setRedenSizes] = useState<RedenSizes>({});

  useEffect(() => {
    applyThemeStylesheet(themeId);
  }, [themeId]);

  useEffect(() => {
    setThemeId(currentThemeId());
    return subscribeToThemeChanges(setThemeId);
  }, []);

  const handleRedenSearch = async () => {
    const query = redenQuery.trim();
    if (!query) {
      setRedenStatus("请输入 RedenMC 搜索关键词。");
      return;
    }
    setRedenBusy("search");
    setRedenStatus("");
    setRedenDetail(null);
    try {
      const response = await searchRedenLitematica(query);
      const results = response.d || [];
      setRedenResults(results);
      setRedenStatus(results.length ? `RedenMC：找到 ${results.length} 个结果。` : "RedenMC：没有结果。");
    } catch (err: any) {
      setRedenStatus(`RedenMC API 失败：${String(err)}`);
    } finally {
      setRedenBusy("");
    }
  };

  const handleRedenDetail = async (machineId: string) => {
    setRedenBusy(`detail:${machineId}`);
    setRedenStatus("");
    try {
      const detail = await fetchRedenMachineDetail(machineId);
      setRedenDetail(detail);
      const rules = getRedenSizeRules(detail);
      setRedenSizes({
        xSize: detail.hasX ? rules.x.min : undefined,
        ySize: detail.hasY ? rules.y.min : undefined,
        zSize: detail.hasZ ? rules.z.min : undefined,
      });
      setRedenStatus(`RedenMC：已加载详情 ${detail.name || detail.key}。`);
    } catch (err: any) {
      setRedenStatus(`RedenMC 详情失败：${String(err)}`);
    } finally {
      setRedenBusy("");
    }
  };

  const importRedenPath = async (path: string) => {
    const libraryState = await loadLibrary();
    await importDownloadedLitematic(libraryState, path);
    await emitEvent(projectionLibraryImportedEvent, { path });
  };

  const handleRedenDownloadAttachment = async (detail: RedenMachine, index: number) => {
    setRedenBusy(`download:${index}`);
    setRedenStatus("");
    try {
      const output = await downloadRedenAttachment(detail.key, index + 1);
      await importRedenPath(output.path);
      setRedenStatus(`RedenMC：已下载并入库 ${output.file_name} (${formatSize(output.bytes)})。`);
    } catch (err: any) {
      setRedenStatus(`RedenMC 下载失败：${String(err)}`);
    } finally {
      setRedenBusy("");
    }
  };

  const handleRedenDownloadParametric = async (detail: RedenMachine) => {
    const errors = validateRedenSizes(detail, redenSizes);
    if (errors.length) {
      setRedenStatus(`RedenMC 尺寸不合法：${errors.join("；")}`);
      return;
    }
    setRedenBusy("download:parametric");
    setRedenStatus("");
    try {
      const output = await downloadRedenParametric(detail.key, redenSizes);
      await importRedenPath(output.path);
      setRedenStatus(`RedenMC：已生成、下载并入库 ${output.file_name} (${formatSize(output.bytes)})。`);
    } catch (err: any) {
      setRedenStatus(`RedenMC 重复结构下载失败：${String(err)}`);
    } finally {
      setRedenBusy("");
    }
  };

  const statusClassName = [
    "reden-status",
    redenStatus.includes("失败") || redenStatus.includes("不合法") ? "reden-status-error" : "",
  ].filter(Boolean).join(" ");

  return (
    <main
      className={["reden-library-window", themeClassName(themeId)].filter(Boolean).join(" ")}
    >
      <div className="reden-library-header">
        <div className="reden-library-title">在线投影库 / RedenMC</div>
        <div className="reden-library-subtitle">搜索、查看详情并直接导入投影库</div>
      </div>

      <section className="reden-library-panel">
        <div className="reden-search-row">
          <input
            className="input nova-input-flex"
            value={redenQuery}
            placeholder="搜索机器，例如：刷石机、世界吞噬者"
            onChange={(event) => setRedenQuery(event.target.value)}
            onKeyDown={(event) => { if (event.key === "Enter") handleRedenSearch(); }}
          />
          <button className="btn" disabled={redenBusy === "search"} onClick={handleRedenSearch}>
            {redenBusy === "search" ? "搜索中..." : "搜索"}
          </button>
        </div>

        <div className="reden-content-grid">
          <div className="reden-results-pane">
            <div className="reden-pane-title">搜索结果</div>
            <div className="reden-result-list">
              {redenResults.length > 0 ? redenResults.map((item) => {
                const resultClassName = [
                  "library-card",
                  "reden-result-card",
                  redenDetail?.key === item.key ? "library-card-current" : "",
                ].filter(Boolean).join(" ");
                return (
                  <div
                    key={item.key}
                    className={resultClassName}
                    role="button"
                    tabIndex={0}
                    aria-disabled={redenBusy === `detail:${item.key}`}
                    onClick={() => {
                      if (redenBusy === `detail:${item.key}`) return;
                      handleRedenDetail(item.key);
                    }}
                    onKeyDown={(event) => {
                      if (redenBusy === `detail:${item.key}`) return;
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        handleRedenDetail(item.key);
                      }
                    }}
                  >
                    <div className="lib-card-left reden-result-card-body">
                      <div className="lib-card-line-main">
                        <div className="lib-card-heading">
                          <span className="lib-card-title">{item.name || item.key}</span>
                        </div>
                      </div>
                      <div className="lib-card-text">状态：在线可导入</div>
                      <div className="lib-card-text">标签：{redenTags(item)}</div>
                      <div className="reden-result-row">
                        <div className="reden-result-meta">作者：{item.author?.username || "-"}</div>
                        <div className="reden-result-meta reden-result-stats">下载：{item.downloads ?? 0} / 收藏：{item.upVotes ?? 0}</div>
                      </div>
                      <div className="reden-result-date">更新：{formatRedenDate(item.updatedAt)}</div>
                      {redenSummary(item) && <div className="reden-result-summary">{redenSummary(item).slice(0, 120)}</div>}
                    </div>
                  </div>
                );
              }) : (
                <div className="reden-empty-results">
                  暂无结果。请输入关键词后开始搜索。
                </div>
              )}
            </div>
          </div>

          <div className="reden-detail-pane">
            {redenDetail ? (
              <div className="reden-detail-stack">
                <div>
                  <div className="reden-detail-title">{redenDetail.name || redenDetail.key}</div>
                  <div className="reden-detail-meta">ID：{redenDetail.key} · 类型：{redenDetail.type || "-"} · 作者：{redenDetail.author?.username || "-"}</div>
                  {redenSummary(redenDetail) && <div className="reden-detail-summary">{redenSummary(redenDetail)}</div>}
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
                        <button className="btn" disabled={redenBusy === `download:${index}`} onClick={() => handleRedenDownloadAttachment(redenDetail, index)}>
                          {redenBusy === `download:${index}` ? "下载中..." : "下载并入库"}
                        </button>
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
                            <input
                              className="input"
                              type="number"
                              value={redenSizes[field] ?? ""}
                              onChange={(event) => setRedenSizes((current) => ({ ...current, [field]: Number(event.target.value) }))}
                            />
                          </label>
                        );
                      })}
                    </div>
                    <button className="btn reden-parametric-button" disabled={redenBusy === "download:parametric"} onClick={() => handleRedenDownloadParametric(redenDetail)}>
                      {redenBusy === "download:parametric" ? "生成下载中..." : "生成下载并入库"}
                    </button>
                  </div>
                )}
              </div>
            ) : (
              <div className="reden-empty-detail">请选择左侧搜索结果查看详情。</div>
            )}
          </div>
        </div>

        {redenStatus && (
          <footer className="reden-footer">
            <div className={statusClassName}>
              {redenStatus}
            </div>
          </footer>
        )}
      </section>
    </main>
  );
}
