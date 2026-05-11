import React, { useEffect, useMemo, useState } from "react";
import {
  addOrUpdateRecord,
  analyzeFile,
  LibraryState,
  loadLibrary,
  ProjectionRecord,
  reorderLibraryRecords,
  saveLibrary,
  setRecordPreview,
} from "../../business/facade";
import { checkFileExists, generatePreviewImage, openFileParentDir, readImageBase64, selectLitematicFile } from "../../business/facade";
import { loadUserConfigMigratingLocalStorage } from "../../business/facade";
import { DisplayMode, normalizeDisplayMode } from "../../business/facade";
import {
  downloadRedenAttachment,
  downloadRedenParametric,
  fetchRedenMachineDetail,
  getRedenSizeRules,
  importDownloadedLitematic,
  RedenMachine,
  RedenSizes,
  searchRedenLitematica,
  validateRedenSizes,
} from "../../business/facade";

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

function ProjectionCard({
  record,
  isCurrent,
  previewDataUrl,
  previewMode,
  isGenerating,
  onSetCurrent,
  onEditProperties,
  onOpenFolder,
  onReanalyze,
  onRemove,
  onGeneratePreview,
  onDragStart,
  onDragOver,
  onDrop,
}: {
  record: ProjectionRecord;
  isCurrent: boolean;
  previewDataUrl: string;
  previewMode: DisplayMode;
  isGenerating: boolean;
  onSetCurrent: () => void;
  onEditProperties: () => void;
  onOpenFolder: () => void;
  onReanalyze: () => void;
  onRemove: () => void;
  onGeneratePreview: () => void;
  onDragStart: () => void;
  onDragOver: (event: React.DragEvent) => void;
  onDrop: () => void;
}) {
  const tags = record.tags?.length ? record.tags.join(" / ") : "无标签";
  const author = record.author || "Unknown";
  return (
    <div
      className={`library-card ${isCurrent ? "library-card-current" : ""}`}
      draggable
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDrop={onDrop}
      onClick={(event) => {
        event.stopPropagation();
        onSetCurrent();
      }}
    >
      <div className="lib-card-left">
        <div className="lib-card-title">{record.displayName}</div>
        <div className="lib-card-text">标签：{tags}</div>
        <div className="lib-card-text">内部名称：{record.displayName || "Unnamed"} &nbsp;&nbsp;&nbsp;&nbsp; 作者：{author}</div>
        <div className="lib-card-highlight">原文件：{record.fileName}</div>
        <div className="lib-card-path" title={record.path}>{record.path}</div>
        <div className="lib-card-text">导入：{formatDate(record.lastAnalyzedAt)} &nbsp;&nbsp;&nbsp;&nbsp; 最近使用：{formatDate(record.lastAnalyzedAt)}</div>

        {record.status !== "ok" && (
          <div className={`library-card-status ${record.status === "missing" ? "is-missing" : "is-warning"}`}>
            {record.status === "missing" ? "文件已丢失" : `解析失败: ${record.lastError}`}
          </div>
        )}

        <div className="nova-input-flex" />
        <div className="lib-card-actions">
          <button className="btn" onClick={(event) => { event.stopPropagation(); onSetCurrent(); }}>打开</button>
          <button className="btn" onClick={(event) => { event.stopPropagation(); onEditProperties(); }}>更改属性</button>
          <button className="btn" onClick={(event) => { event.stopPropagation(); onReanalyze(); }}>重新分析</button>
          <button className="btn" onClick={(event) => { event.stopPropagation(); onOpenFolder(); }}>打开目录</button>
          <button className="btn" onClick={(event) => { event.stopPropagation(); onRemove(); }}>删除</button>
        </div>
      </div>

      <div className="lib-card-right">
        <div className="lib-card-size">{formatSize(record.fileSize)}</div>
        <div className="lib-card-preview-box">
          {previewDataUrl ? <img src={previewDataUrl} alt="" className="library-preview-image" /> : "暂无预览图"}
        </div>
        <button
          className="btn nova-input-full"
          disabled={isGenerating || record.status !== "ok"}
          onClick={(event) => { event.stopPropagation(); onGeneratePreview(); }}
        >
          {isGenerating ? "生成中..." : `生成预览 (${previewMode})`}
        </button>
      </div>
    </div>
  );
}

export function LibraryPage({ currentFile, setCurrentFile, setRoute }: any) {
  const [state, setState] = useState<LibraryState>({ records: [] });
  const [search, setSearch] = useState("");
  const [tagFilter, setTagFilter] = useState("");
  const [sortMode, setSortMode] = useState("manual");
  const [limit, setLimit] = useState(20);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [previewMode, setPreviewMode] = useState<DisplayMode>("normal");
  const [generatingPreviewPath, setGeneratingPreviewPath] = useState("");
  const [previewError, setPreviewError] = useState("");
  const [previewDataUrls, setPreviewDataUrls] = useState<Record<string, string>>({});
  const [dragPath, setDragPath] = useState("");
  const [redenQuery, setRedenQuery] = useState("刷石机");
  const [redenResults, setRedenResults] = useState<RedenMachine[]>([]);
  const [redenDetail, setRedenDetail] = useState<RedenMachine | null>(null);
  const [redenStatus, setRedenStatus] = useState("");
  const [redenBusy, setRedenBusy] = useState("");
  const [redenSizes, setRedenSizes] = useState<RedenSizes>({});

  useEffect(() => {
    loadLibrary().then(setState);
    loadUserConfigMigratingLocalStorage()
      .then((info) => setPreviewMode(normalizeDisplayMode(info.config.preview_mode)))
      .catch(() => setPreviewMode("normal"));
  }, []);

  useEffect(() => {
    let cancelled = false;
    const loadPreviews = async () => {
      const entries = await Promise.all(state.records.map(async (record) => {
        const imagePath = record.preview_image_path || record.previewPath;
        if (!imagePath) return [record.path, ""] as const;
        try {
          return [record.path, await readImageBase64(imagePath)] as const;
        } catch {
          return [record.path, ""] as const;
        }
      }));
      if (!cancelled) setPreviewDataUrls(Object.fromEntries(entries.filter(([, value]) => value)));
    };
    loadPreviews();
    return () => {
      cancelled = true;
    };
  }, [state.records]);

  const handleSelect = async () => {
    const res = await selectLitematicFile();
    if (res) {
      setCurrentFile(res);
      setState(await addOrUpdateRecord(state, res));
      setRoute("properties");
    }
  };

  const handleRefresh = async () => {
    setIsRefreshing(true);
    const newState = { ...state, records: [...state.records] };
    for (const rec of newState.records) {
      const exists = await checkFileExists(rec.path);
      if (!exists) {
        rec.status = "missing";
      } else {
        const analysis = await analyzeFile(rec.path);
        Object.assign(rec, analysis);
        rec.lastAnalyzedAt = Date.now();
      }
    }
    await saveLibrary(newState);
    setState(newState);
    setIsRefreshing(false);
  };

  const handleRemove = async (path: string) => {
    const newState = { ...state, records: state.records.filter((r) => r.path !== path) };
    await saveLibrary(newState);
    setState(newState);
    if (currentFile === path) setCurrentFile("");
  };

  const handleGeneratePreview = async (record: ProjectionRecord) => {
    setPreviewError("");
    setGeneratingPreviewPath(record.path);
    try {
      const output = await generatePreviewImage(record.path, previewMode);
      const newState = await setRecordPreview(state, record.path, output.preview_path);
      setState(newState);
      setPreviewDataUrls((current) => ({ ...current, [record.path]: output.data_url }));
    } catch (err: any) {
      setPreviewError(String(err));
    } finally {
      setGeneratingPreviewPath("");
    }
  };

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
    const nextState = await importDownloadedLitematic(state, path);
    setState(nextState);
    setCurrentFile(path);
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

  const handleDrop = async (targetPath: string) => {
    if (!dragPath || dragPath === targetPath || sortMode !== "manual") return;
    const ordered = [...visibleRecords];
    const from = ordered.findIndex((record) => record.path === dragPath);
    const to = ordered.findIndex((record) => record.path === targetPath);
    if (from < 0 || to < 0) return;
    const [moved] = ordered.splice(from, 1);
    ordered.splice(to, 0, moved);
    const visiblePaths = ordered.map((record) => record.path);
    const untouched = state.records.filter((record) => !visiblePaths.includes(record.path)).map((record) => record.path);
    setState(await reorderLibraryRecords(state, [...visiblePaths, ...untouched]));
    setDragPath("");
  };

  let visibleRecords = useMemo(() => {
    let records = state.records.filter((r) => {
      if (search) {
        const q = search.toLowerCase();
        if (!`${r.displayName} ${r.fileName} ${r.path} ${r.author} ${r.description}`.toLowerCase().includes(q)) return false;
      }
      if (tagFilter === "missing" && r.status !== "missing") return false;
      if (tagFilter === "parse_error" && r.status !== "parse_error") return false;
      if (tagFilter === "untagged" && r.tags.length > 0) return false;
      return true;
    });
    if (sortMode === "manual") {
      records = [...records].sort((a, b) => (a.sort_index ?? 0) - (b.sort_index ?? 0));
    } else if (sortMode === "recent_mod") {
      records = [...records].sort((a, b) => b.lastAnalyzedAt - a.lastAnalyzedAt);
    } else if (sortMode === "blocks") {
      records = [...records].sort((a, b) => b.totalBlocks - a.totalBlocks);
    } else if (sortMode === "name") {
      records = [...records].sort((a, b) => a.displayName.localeCompare(b.displayName));
    }
    return limit === 0 ? records : records.slice(0, limit);
  }, [state.records, search, tagFilter, sortMode, limit]);

  const issueCount = state.records.filter((r) => r.status === "missing" || r.status === "parse_error").length;

  return (
    <div className="nova-page" onClick={() => setCurrentFile("")}>
      <div className="nova-toolbar">
        <div className="nova-page-title-sm">投影库</div>
        <div className="nova-muted nova-small nova-flex-1">{state.records.length} 条记录，{issueCount} 条需关注</div>
        <div className="nova-small">最近保留</div>
        <select className="input nova-select-xs" value={limit} onChange={(event) => setLimit(Number(event.target.value))}>
          <option value="20">20</option>
          <option value="50">50</option>
          <option value="100">100</option>
          <option value="200">200</option>
          <option value="0">全部</option>
        </select>
        <button className="btn" onClick={(event) => { event.stopPropagation(); handleRefresh(); }} disabled={isRefreshing}>{isRefreshing ? "刷新中..." : "刷新校验"}</button>
      </div>

      <div className="library-filter-row" onClick={(event) => event.stopPropagation()}>
        <input className="input nova-input-flex" placeholder="搜索投影名、原文件名或原地址" value={search} onChange={(event) => setSearch(event.target.value)} />
        <select className="input nova-select-sm" value={tagFilter} onChange={(event) => setTagFilter(event.target.value)}>
          <option value="">全部标签</option>
          <option value="untagged">无标签</option>
          <option value="missing">已丢失</option>
          <option value="parse_error">解析失败</option>
        </select>
        <select className="input nova-select-sm" value={sortMode} onChange={(event) => setSortMode(event.target.value)}>
          <option value="manual">手动排序</option>
          <option value="recent_import">最近导入</option>
          <option value="recent_mod">最近修改</option>
          <option value="blocks">方块数高到低</option>
          <option value="name">名称 A-Z</option>
        </select>
      </div>

      <section className="reden-library-section" onClick={(event) => event.stopPropagation()}>
        <div className="nova-row-tight">
          <div className="nova-strong">在线投影库 / RedenMC</div>
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

        {redenStatus && (
          <div className={`nova-small ${redenStatus.includes("失败") || redenStatus.includes("不合法") ? "nova-error-text" : ""}`}>
            {redenStatus}
          </div>
        )}

        {redenResults.length > 0 && (
          <div className="nova-grid-results">
            {redenResults.map((item) => (
              <button
                key={item.key}
                className={`btn reden-result-card ${redenDetail?.key === item.key ? "is-active" : ""}`}
                disabled={redenBusy === `detail:${item.key}`}
                onClick={() => handleRedenDetail(item.key)}
              >
                <div className="nova-strong">{item.name || item.key}</div>
                <div className="nova-muted-copy nova-tiny">作者：{item.author?.username || "-"} · 下载：{item.downloads ?? 0} · 收藏/赞：{item.upVotes ?? 0}</div>
                <div className="nova-muted-copy nova-tiny">标签：{redenTags(item)}</div>
                <div className="nova-subtle nova-tiny">更新：{formatRedenDate(item.updatedAt)}</div>
                {redenSummary(item) && <div className="nova-tiny nova-mt-xs">{redenSummary(item).slice(0, 120)}</div>}
              </button>
            ))}
          </div>
        )}

        {redenDetail && (
          <div className="reden-detail">
            <div>
              <div className="nova-strong">{redenDetail.name || redenDetail.key}</div>
              <div className="nova-muted-copy nova-small">ID：{redenDetail.key} · 类型：{redenDetail.type || "-"} · 作者：{redenDetail.author?.username || "-"}</div>
              {redenSummary(redenDetail) && <div className="nova-small reden-summary">{redenSummary(redenDetail)}</div>}
            </div>

            {(redenDetail.attachments || []).length > 0 && (
              <div className="nova-stack-compact">
                <div className="nova-section-title">普通附件</div>
                {(redenDetail.attachments || []).map((attachment, index) => (
                  <div key={`${attachment.name}-${index}`} className="nova-row-tight">
                    <div className="nova-input-flex">
                      <div>{index + 1}. {attachment.name || `attachment-${index + 1}.litematic`}</div>
                      <div className="nova-muted-copy nova-tiny">{attachment.size ? formatSize(attachment.size) : "-"} {attachment.description || ""}</div>
                    </div>
                    <button className="btn" disabled={redenBusy === `download:${index}`} onClick={() => handleRedenDownloadAttachment(redenDetail, index)}>
                      {redenBusy === `download:${index}` ? "下载中..." : "下载并入库"}
                    </button>
                  </div>
                ))}
              </div>
            )}

            {(redenDetail.hasX || redenDetail.hasY || redenDetail.hasZ) && (
              <div className="nova-stack-tight">
                <div className="nova-section-title">重复结构尺寸</div>
                <div className="nova-row-wrap">
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
                <button className="btn nova-self-start" disabled={redenBusy === "download:parametric"} onClick={() => handleRedenDownloadParametric(redenDetail)}>
                  {redenBusy === "download:parametric" ? "生成下载中..." : "生成下载并入库"}
                </button>
              </div>
            )}
          </div>
        )}
      </section>

      {previewError && <pre className="nova-error nova-pre-medium">{previewError}</pre>}

      {state.records.length === 0 ? (
        <div className="library-empty">
          <p className="library-empty-text">投影库还没有内容。请先导入一个 .litematic 文件。</p>
          <button className="btn nova-mt-md" onClick={(event) => { event.stopPropagation(); handleSelect(); }}>选择 .litematic...</button>
        </div>
      ) : (
        <div className="nova-list-panel">
          {visibleRecords.map((r) => (
            <ProjectionCard
              key={r.path}
              record={r}
              isCurrent={currentFile === r.path}
              onSetCurrent={() => setCurrentFile(r.path)}
              onEditProperties={() => { setCurrentFile(r.path); setRoute("properties"); }}
              onOpenFolder={() => openFileParentDir(r.path).catch((err) => alert(`打开失败\n${String(err)}`))}
              onReanalyze={async () => setState(await addOrUpdateRecord(state, r.path))}
              onRemove={() => handleRemove(r.path)}
              onGeneratePreview={() => handleGeneratePreview(r)}
              previewMode={previewMode}
              isGenerating={generatingPreviewPath === r.path}
              previewDataUrl={previewDataUrls[r.path] || ""}
              onDragStart={() => setDragPath(r.path)}
              onDragOver={(event) => { if (sortMode === "manual") event.preventDefault(); }}
              onDrop={() => handleDrop(r.path)}
            />
          ))}
        </div>
      )}
    </div>
  );
}


