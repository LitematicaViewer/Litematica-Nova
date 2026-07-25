import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  createRuntimeProjectionCollection,
  DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES,
  EnumeratorCollection,
  evaluateEnumeratorExpression,
  exportMaterialsArtTable,
  exportMaterialsCsv,
  initI18n,
  loadEnumeratorCollections,
  loadMaterialsScope,
  loadStructureStats,
  loadUserConfigMigratingLocalStorage,
  MaterialItem,
  normalizeMaterialListWindowBehavior,
  normalizeStatisticsEnumeratorInfoRules,
  openMaterialListWindow,
  saveStatisticsEnumeratorInfoRulesConfig,
  StatsData,
} from "../../../../../src/business/facade";
import { BlockIcon } from "../../../../components/BlockIcon";
import { VirtualSpacerCell } from "../../../../components/VirtualSpacerCell";
import { useVirtualWindow } from "../../../../components/useVirtualWindow";
import { EnumeratorDialog, openEnumeratorWithWindowBehavior } from "../../../enumerator";

interface EnumeratorInfoRule {
  title: string;
  expression: string;
  raw: string;
}

interface EnumeratorInfoResult {
  title: string;
  value: string;
  error: string;
  raw: string;
}

function parseEnumeratorInfoRules(rules: string[]): EnumeratorInfoRule[] {
  return normalizeStatisticsEnumeratorInfoRules(rules)
    .map((raw) => {
      const separatorIndex = raw.indexOf("=");
      if (separatorIndex <= 0 || separatorIndex === raw.length - 1) {
        return {
          title: raw.trim() || "未命名项",
          expression: "",
          raw,
        };
      }
      return {
        title: raw.slice(0, separatorIndex).trim() || "未命名项",
        expression: raw.slice(separatorIndex + 1).trim(),
        raw,
      };
    })
    .filter((rule) => rule.title || rule.expression);
}

function parseFunctionCall(expression: string): { name: string; argument: string } | null {
  const trimmed = String(expression || "").trim();
  const openIndex = trimmed.indexOf("(");
  if (openIndex <= 0 || !trimmed.endsWith(")")) return null;
  return {
    name: trimmed.slice(0, openIndex).trim().toLowerCase(),
    argument: trimmed.slice(openIndex + 1, -1).trim(),
  };
}

function uniqValues(values: string[]): string[] {
  return [...new Set(values.map((value) => String(value || "").trim()).filter(Boolean))];
}

function buildBaseUniverse(collections: EnumeratorCollection[]): Set<string> {
  return new Set(
    collections
      .filter((collection) => collection.category === "base")
      .flatMap((collection) => collection.values),
  );
}

function buildVersionUniverse(collections: EnumeratorCollection[]): Set<string> {
  return new Set(
    collections
      .filter((collection) => collection.category === "version")
      .flatMap((collection) => collection.values),
  );
}

function parseVersionToken(collection: EnumeratorCollection): { kind: "older" | "normal"; numbers: number[]; label: string } | null {
  const candidates = [collection.version, collection.name, collection.id];
  for (const candidate of candidates) {
    const normalized = String(candidate || "").trim();
    if (!normalized) continue;
    if (normalized === "v_older") {
      return { kind: "older", numbers: [1, 12, 2], label: "1.12.2之前" };
    }
    if (!normalized.startsWith("v_")) continue;
    const parts = normalized
      .slice(2)
      .split("_")
      .map((part) => Number.parseInt(part, 10));
    if (!parts.length || parts.some((part) => !Number.isFinite(part))) continue;
    return {
      kind: "normal",
      numbers: parts,
      label: parts.join("."),
    };
  }
  return null;
}

function compareVersionCollections(left: EnumeratorCollection, right: EnumeratorCollection): number {
  const leftToken = parseVersionToken(left);
  const rightToken = parseVersionToken(right);
  if (!leftToken && !rightToken) return left.name.localeCompare(right.name, "zh-CN");
  if (!leftToken) return -1;
  if (!rightToken) return 1;
  if (leftToken.kind !== rightToken.kind) {
    return leftToken.kind === "older" ? -1 : 1;
  }
  const size = Math.max(leftToken.numbers.length, rightToken.numbers.length, 3);
  for (let index = 0; index < size; index += 1) {
    const diff = (leftToken.numbers[index] ?? 0) - (rightToken.numbers[index] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

function formatVersionResult(values: string[], collections: EnumeratorCollection[]): string {
  if (values.length === 0) return "--";
  const targetSet = new Set(values);
  const baseUniverse = buildBaseUniverse(collections);
  const versionCollections = collections.filter((collection) => collection.category === "version");
  const validVersionCollections = versionCollections.filter((collection) => Boolean(parseVersionToken(collection)));
  const versionUniverse = buildVersionUniverse(collections);
  const matchedVersions = versionCollections.filter((collection) => collection.values.some((value) => targetSet.has(value)));
  const hasHigherVersion = [...targetSet].some((value) => baseUniverse.has(value) && !versionUniverse.has(value));
  const sortedMatchedVersions = matchedVersions.sort(compareVersionCollections);
  const latestVersion = sortedMatchedVersions.length > 0 ? sortedMatchedVersions[sortedMatchedVersions.length - 1] : null;
  if (!latestVersion) {
    if (validVersionCollections.length === 0) {
      return "未配置版本集合";
    }
    return hasHigherVersion ? "更高版本" : "--";
  }
  const versionToken = parseVersionToken(latestVersion);
  const label = versionToken?.label || latestVersion.name;
  return hasHigherVersion ? `${label} / 更高版本` : label;
}

function evaluateEnumeratorInfoRule(rule: EnumeratorInfoRule, data: StatsData, collections: EnumeratorCollection[]): EnumeratorInfoResult {
  if (!rule.expression) {
    return { title: rule.title, value: "", error: "缺少表达式。", raw: rule.raw };
  }
  const call = parseFunctionCall(rule.expression);
  if (!call) {
    return { title: rule.title, value: "", error: "表达式必须为 函数(集合表达式) 形式。", raw: rule.raw };
  }
  const evaluated = evaluateEnumeratorExpression(call.argument, collections);
  if (evaluated.error) {
    return { title: rule.title, value: "", error: evaluated.error, raw: rule.raw };
  }
  const values = uniqValues(evaluated.values);
  switch (call.name) {
    case "version":
      return { title: rule.title, value: formatVersionResult(values, collections), error: "", raw: rule.raw };
    case "any":
      return { title: rule.title, value: values.length > 0 ? "True" : "False", error: "", raw: rule.raw };
    case "ratio": {
      if (data.totalNonAirBlocks <= 0) {
        return { title: rule.title, value: "--", error: "", raw: rule.raw };
      }
      const countById = new Map(data.materials.map((material) => [material.id, material.totalCount]));
      const numerator = values.reduce((sum, value) => sum + (countById.get(value) || 0), 0);
      return {
        title: rule.title,
        value: `${((numerator / data.totalNonAirBlocks) * 100).toFixed(1)}%`,
        error: "",
        raw: rule.raw,
      };
    }
    default:
      return { title: rule.title, value: "", error: `不支持的函数：${call.name}`, raw: rule.raw };
  }
}

function buildEnumeratorInfoResults(rules: string[], data: StatsData | null, collections: EnumeratorCollection[]): EnumeratorInfoResult[] {
  const parsedRules = parseEnumeratorInfoRules(rules);
  if (!data) {
    return parsedRules.map((rule) => ({ title: rule.title, value: "等待统计结果", error: "", raw: rule.raw }));
  }
  return parsedRules.map((rule) => evaluateEnumeratorInfoRule(rule, data, collections));
}

function MaterialTooltip({ x, y, item, multiplier }: { x: number; y: number; item: MaterialItem | null; multiplier: number }) {
  const popupRef = useRef<HTMLDivElement | null>(null);
  const total = item ? Math.max(0, Math.floor(item.totalCount * multiplier)) : 0;
  const stacks = Math.floor(total / 64);
  const remainder = total % 64;
  const shulkerBoxes = (total / 1728).toFixed(2);
  const [position, setPosition] = useState(() => ({ left: x + 14, top: y + 14 }));

  useLayoutEffect(() => {
    if (!item) return;
    const popup = popupRef.current;
    const offset = 14;
    const margin = 4;
    if (!popup) {
      setPosition({ left: x + offset, top: y + offset });
      return;
    }

    const rect = popup.getBoundingClientRect();
    let left = x + offset;
    let top = y + offset;

    if (left + rect.width > window.innerWidth - margin) {
      left = x - rect.width - offset;
    }
    if (top + rect.height > window.innerHeight - margin) {
      top = y - rect.height - offset;
    }

    left = Math.max(margin, Math.min(left, window.innerWidth - rect.width - margin));
    top = Math.max(margin, Math.min(top, window.innerHeight - rect.height - margin));
    setPosition((previous) => previous.left === left && previous.top === top ? previous : { left, top });
  }, [x, y, item, total]);

  useLayoutEffect(() => {
    const popup = popupRef.current;
    if (!popup) return;
    popup.style.setProperty("--material-list-popup-left", `${position.left}px`);
    popup.style.setProperty("--material-list-popup-top", `${position.top}px`);
  }, [position.left, position.top]);

  if (!item) return null;

  return (
    <div
      ref={popupRef}
      className="material-list-hover-popup"
      role="tooltip"
    >
      <div className="material-list-hover-popup-row material-list-hover-popup-item-row">
        <span className="material-list-hover-popup-label">项目：</span>
        <BlockIcon blockId={item.iconHint} />
        <span className="material-list-hover-popup-name">{item.name}</span>
      </div>
      <div className="material-list-hover-popup-row">ID：{item.id}</div>
      <div className="material-list-hover-popup-row">
        总计： <strong>&nbsp;{total}&nbsp;</strong> = <strong>&nbsp;{stacks}&nbsp;</strong> × 64 + <strong>&nbsp;{remainder}&nbsp;</strong> = <strong>&nbsp;{shulkerBoxes}&nbsp;</strong> 潜影盒
      </div>
    </div>
  );
}

/**
 * Opens the material list according to the user's configured window behavior.
 */
export async function openMaterialsWithWindowBehavior(currentFile: string, showOverlay: () => void): Promise<void> {
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
  const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
  if (behavior === "independent_window") {
    try {
      await openMaterialListWindow(currentFile);
      return;
    } catch {
      // Browser preview cannot create a desktop window, so keep the in-window dialog as fallback.
    }
  }
  await initI18n(true);
  showOverlay();
}

/**
 * Shared material-list body used by both the modal dialog and the desktop child window.
 */
export function MaterialListContent({
  data,
  onClose,
  currentFile,
  standalone = false,
}: {
  data: StatsData;
  onClose?: () => void;
  currentFile: string;
  standalone?: boolean;
}) {
  const [multiplier, setMultiplier] = useState(1);
  const [includeContainerItems, setIncludeContainerItems] = useState(false);
  const [workbook, setWorkbook] = useState("scope:all");
  const [materials, setMaterials] = useState<MaterialItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState("");
  const [hoverItem, setHoverItem] = useState<MaterialItem | null>(null);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });
  const tableWrapRef = useRef<HTMLDivElement | null>(null);
  const contentClassName = [
    standalone ? "material-list-window" : "dialog-content",
    "subwindow-frame",
    "material-list-content",
    standalone ? "subwindow-frame-full material-list-content-standalone" : "",
  ].filter(Boolean).join(" ");

  const options = useMemo(() => {
    const next = [{ label: "整个投影", value: "scope:all" }];
    for (const region of data.regions || []) {
      next.push({ label: `选定区域：${region.name}`, value: `region:${region.name}` });
    }
    for (const layer of data.layers || []) {
      next.push({ label: `选定层级：y=${layer.world_y}（层 ${layer.layer}）`, value: `layer:${layer.layer}` });
    }
    return next;
  }, [data.regions, data.layers]);

  const scopeArgs = (value: string): string[] => {
    if (value.startsWith("scope:")) return ["--scope", value.split(":")[1]];
    if (value.startsWith("region:")) return ["--region", value.slice("region:".length)];
    if (value.startsWith("layer:")) return ["--layer", value.split(":")[1]];
    return ["--scope", "all"];
  };

  const loadMats = async (value: string, includeContainers = includeContainerItems) => {
    setIsLoading(true);
    setError("");
    try {
      await initI18n();
      const next = await loadMaterialsScope(currentFile, scopeArgs(value), includeContainers);
      setMaterials(next.sort((a, b) => b.totalCount - a.totalCount));
    } catch (err: any) {
      setError(`获取材料失败：${err}`);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadMats(workbook);
  }, [workbook, includeContainerItems]);

  const {
    visibleItems: visibleMaterials,
    startIndex,
    topSpacerHeight,
    bottomSpacerHeight,
  } = useVirtualWindow(materials, tableWrapRef, 40, 10);

  const handleExportArtTable = async () => {
    try {
      const ok = await exportMaterialsArtTable(currentFile, materials, multiplier);
      if (ok) alert("写入艺术表成功");
    } catch (err: any) {
      setError(`写入艺术表失败：${err}`);
    }
  };

  const handleExportCsv = async () => {
    try {
      const ok = await exportMaterialsCsv(currentFile, materials, multiplier);
      if (ok) alert("写入 CSV 成功");
    } catch (err: any) {
      setError(`写入 CSV 失败：${err}`);
    }
  };

  return (
      <div
        className={contentClassName}
        onMouseMove={(event) => setMousePos({ x: event.clientX, y: event.clientY })}
      >
        <div className="subwindow-title-bar">
          <h3 className="subwindow-title">材料列表</h3>
          {onClose && <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}>×</button>}
        </div>

        <div className="subwindow-body">
          <div className="material-list-control-stack">
            <div className="material-list-control-row material-list-control-row-workbook">
              <span className="material-list-control-label">工作簿</span>
              <select
                className="input material-list-workbook-select"
                value={workbook}
                onChange={(event) => setWorkbook(event.target.value)}
              >
                {options.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </select>
            </div>

            <div className="material-list-control-row material-list-control-row-actions">
              <button className="btn" onClick={() => loadMats(workbook)} disabled={isLoading}>重新加载</button>
              <button className="btn" onClick={handleExportArtTable} disabled={isLoading || materials.length === 0}>写入艺术表</button>
              <button className="btn" onClick={handleExportCsv} disabled={isLoading || materials.length === 0}>写入CSV</button>
              <label className="material-list-multiplier-control">
                <span>倍数</span>
                <input
                  type="number"
                  className="input material-list-count-input"
                  value={multiplier}
                  onChange={(event) => setMultiplier(Math.max(1, parseInt(event.target.value, 10) || 1))}
                />
              </label>
            </div>

            <div className="material-list-control-row material-list-control-row-toggles">
              <label className="subwindow-check-row">
                <input
                  type="checkbox"
                  checked={includeContainerItems}
                  onChange={(event) => setIncludeContainerItems(event.target.checked)}
                />
                统计容器
              </label>
              <label className="subwindow-check-row material-list-disabled-toggle" title="统计实体尚未实现">
                <input type="checkbox" disabled />
                统计实体
              </label>
            </div>
          </div>

          <div className="subwindow-status-text">
            {isLoading ? "正在分析..." : includeContainerItems ? "已包含容器 BlockEntity/TileEntity 内物品。" : "当前仅统计投影方块。"}
          </div>
          {error && <pre className="subwindow-error">{error}</pre>}

          <div className="material-list-table-wrap" ref={tableWrapRef}>
            <table className="material-list-table">
              <thead>
                <tr>
                  <th className="material-list-icon-col">图标</th>
                  <th>名称</th>
                  <th className="material-list-number">方块</th>
                  <th className="material-list-number">容器物品</th>
                  <th className="material-list-total-col">合计</th>
                </tr>
              </thead>
              <tbody>
                {materials.length === 0 && !isLoading ? (
                  <tr>
                    <td colSpan={5} className="material-list-empty-cell">暂无材料数据</td>
                  </tr>
                ) : (
                  <>
                    {topSpacerHeight > 0 ? (
                      <tr className="material-list-virtual-spacer" aria-hidden>
                        <VirtualSpacerCell colSpan={5} height={topSpacerHeight} />
                      </tr>
                    ) : null}
                    {visibleMaterials.map((material, index) => (
                      <tr
                        key={material.id}
                        className={(startIndex + index) % 2 === 0 ? "material-list-row-even" : "material-list-row-odd"}
                        onMouseEnter={() => setHoverItem(material)}
                        onMouseLeave={() => setHoverItem(null)}
                      >
                        <td className="material-list-icon-cell"><BlockIcon blockId={material.iconHint} /></td>
                        <td>{material.name}</td>
                        <td className="material-list-number">{material.blockCount * multiplier}</td>
                        <td className="material-list-number">{material.containerItemCount * multiplier}</td>
                        <td className="material-list-number">{material.totalCount * multiplier}</td>
                      </tr>
                    ))}
                    {bottomSpacerHeight > 0 ? (
                      <tr className="material-list-virtual-spacer" aria-hidden>
                        <VirtualSpacerCell colSpan={5} height={bottomSpacerHeight} />
                      </tr>
                    ) : null}
                  </>
                )}
              </tbody>
            </table>
          </div>
        </div>
        <MaterialTooltip x={mousePos.x} y={mousePos.y} item={hoverItem} multiplier={multiplier} />
      </div>
  );
}

export function MaterialsDialog({ data, onClose, currentFile }: { data: StatsData; onClose: () => void; currentFile: string }) {
  return (
    <div className="dialog-overlay">
      <MaterialListContent data={data} onClose={onClose} currentFile={currentFile} />
    </div>
  );
}

function EnumeratorInfoEditorDialog({
  rules,
  data,
  collections,
  onClose,
  onSave,
}: {
  rules: string[];
  data: StatsData | null;
  collections: EnumeratorCollection[];
  onClose: () => void;
  onSave: (rules: string[]) => Promise<void>;
}) {
  const [draft, setDraft] = useState(rules.join("\n"));
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState("");

  useEffect(() => {
    setDraft(rules.join("\n"));
  }, [rules]);

  const parsedDraftRules = useMemo(
    () => normalizeStatisticsEnumeratorInfoRules(draft.split(/\r?\n/)),
    [draft],
  );

  const previewResults = useMemo(
    () => buildEnumeratorInfoResults(parsedDraftRules, data, collections),
    [collections, data, parsedDraftRules],
  );

  const hasErrors = previewResults.some((item) => Boolean(item.error));

  const handleSave = async () => {
    setIsSaving(true);
    setSaveError("");
    try {
      await onSave(parsedDraftRules);
      onClose();
    } catch (error: any) {
      setSaveError(String(error?.message || error || "保存失败"));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <section className="dialog-content subwindow-frame subwindow-frame-wide statistics-enumerator-editor" onClick={(event) => event.stopPropagation()}>
        <div className="subwindow-title-bar">
          <h3 className="subwindow-title">枚举统计编辑器</h3>
          <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}>×</button>
        </div>
        <div className="subwindow-body statistics-enumerator-editor-body">
          <div className="statistics-enumerator-editor-columns">
            <div className="statistics-enumerator-editor-column">
              <div className="statistics-enumerator-editor-heading">多行编辑框</div>
              <textarea
                className="input statistics-enumerator-editor-textarea"
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                spellCheck={false}
              />
              <div className="statistics-enumerator-editor-hint">
                <div>一行一个显示项，格式：标题 = 函数(集合表达式)</div>
                <div>推荐运算符：<code>|</code>、<code>&</code>、<code>-</code></div>
                <div>可用函数：<code>version(expr)</code>、<code>any(expr)</code>、<code>ratio(expr)</code></div>
              </div>
            </div>
            <div className="statistics-enumerator-editor-column">
              <div className="statistics-enumerator-editor-heading">实时预览</div>
              <div className="statistics-enumerator-preview-list">
                {previewResults.map((item) => (
                  <div key={item.raw} className="statistics-enumerator-preview-item">
                    <div className="statistics-enumerator-preview-main">
                      <span className="statistics-enumerator-preview-title">{item.title}</span>
                      <span className="statistics-enumerator-preview-value">{item.error ? "错误" : item.value}</span>
                    </div>
                    {item.error ? <div className="statistics-enumerator-preview-error">{item.error}</div> : null}
                  </div>
                ))}
              </div>
              {saveError ? <pre className="statistics-error">{saveError}</pre> : null}
              {hasErrors ? <div className="statistics-enumerator-editor-error">存在语法或集合错误，修复后才能保存。</div> : null}
            </div>
          </div>
          <div className="statistics-enumerator-editor-footer">
            <button className="btn" type="button" onClick={() => setDraft(DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES.join("\n"))} disabled={isSaving}>恢复默认</button>
            <div className="statistics-enumerator-editor-footer-actions">
              <button className="btn" type="button" onClick={onClose} disabled={isSaving}>取消</button>
              <button className="btn" type="button" onClick={handleSave} disabled={isSaving || hasErrors}>保存</button>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

export function StatisticsPage({ currentFile, theme }: any) {
  const [data, setData] = useState<StatsData | null>(null);
  const [error, setError] = useState("");
  const [showMaterials, setShowMaterials] = useState(false);
  const [showEnumerator, setShowEnumerator] = useState(false);
  const [showEnumeratorInfoEditor, setShowEnumeratorInfoEditor] = useState(false);
  const [includeContainerItems, setIncludeContainerItems] = useState(false);
  const [enumeratorCollections, setEnumeratorCollections] = useState<EnumeratorCollection[]>([]);
  const [enumeratorCollectionsError, setEnumeratorCollectionsError] = useState("");
  const [isLoadingEnumeratorCollections, setIsLoadingEnumeratorCollections] = useState(false);
  const [enumeratorInfoRules, setEnumeratorInfoRules] = useState<string[]>([...DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES]);
  const [isLoadingEnumeratorInfoConfig, setIsLoadingEnumeratorInfoConfig] = useState(true);

  const loadStats = async () => {
    if (!currentFile) return;
    setError("");
    try {
      const next = await loadStructureStats(currentFile, includeContainerItems);
      setData(next);
    } catch (err: any) {
      setError(String(err));
    }
  };

  useEffect(() => {
    loadStats();
  }, [currentFile, includeContainerItems]);

  useEffect(() => {
    let cancelled = false;
    setIsLoadingEnumeratorInfoConfig(true);
    loadUserConfigMigratingLocalStorage()
      .then((info) => {
        if (cancelled) return;
        setEnumeratorInfoRules(normalizeStatisticsEnumeratorInfoRules(info.config.statistics_enumerator_info_rules));
      })
      .catch(() => {
        if (cancelled) return;
        setEnumeratorInfoRules([...DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES]);
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoadingEnumeratorInfoConfig(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const runtimeCollection = useMemo(
    () => (data ? createRuntimeProjectionCollection(currentFile, data.materials) : null),
    [currentFile, data],
  );

  useEffect(() => {
    if (!runtimeCollection) {
      setEnumeratorCollections([]);
      setEnumeratorCollectionsError("");
      setIsLoadingEnumeratorCollections(false);
      return;
    }
    let cancelled = false;
    setIsLoadingEnumeratorCollections(true);
    setEnumeratorCollectionsError("");
    loadEnumeratorCollections([runtimeCollection])
      .then((collections) => {
        if (cancelled) return;
        setEnumeratorCollections(collections);
      })
      .catch((err: any) => {
        if (cancelled) return;
        setEnumeratorCollections([]);
        setEnumeratorCollectionsError(String(err?.message || err || "加载枚举集合失败"));
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoadingEnumeratorCollections(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [runtimeCollection]);


  const topMaterials = data ? [...data.materials].sort((a, b) => b.totalCount - a.totalCount) : [];
  const featuredMaterials = topMaterials.slice(0, 5);
  const totalMaterialCount = topMaterials.reduce((sum, material) => sum + material.totalCount, 0);
  const scan = data?.containerScan;
  const enumeratorInfoResults = data && enumeratorCollections.length > 0
    ? buildEnumeratorInfoResults(enumeratorInfoRules, data, enumeratorCollections)
    : [];

  const handleSaveEnumeratorInfoRules = async (rules: string[]) => {
    const info = await saveStatisticsEnumeratorInfoRulesConfig(rules);
    setEnumeratorInfoRules(normalizeStatisticsEnumeratorInfoRules(info.config.statistics_enumerator_info_rules));
  };

  return (
    <div className="statistics-page">
      <div className="statistics-toolbar">
        <div className="statistics-action-row">
          <button className="btn" onClick={() => openMaterialsWithWindowBehavior(currentFile, () => setShowMaterials(true))}>材料列表</button>
          <button className="btn" onClick={loadStats} disabled={!currentFile}>重新统计</button>
          <button className="btn" type="button" onClick={() => openEnumeratorWithWindowBehavior(currentFile, () => setShowEnumerator(true))}>打开枚举器...</button>
        </div>

        <div className="statistics-toggle-row">
          <label className="statistics-container-toggle">
            <input
              type="checkbox"
              checked={includeContainerItems}
              onChange={(event) => setIncludeContainerItems(event.target.checked)}
            />
            统计容器
          </label>
          <label className="statistics-container-toggle statistics-toggle-disabled">
            <input type="checkbox" disabled />
            统计实体
          </label>
        </div>
      </div>

      {error && <pre className="statistics-error">{error}</pre>}

      {scan?.warnings?.length ? (
        <details className="statistics-warnings">
          <summary className="statistics-warnings-summary">容器扫描 warning（{scan.warnings.length}）</summary>
          <pre className="statistics-warnings-body">
            {scan.warnings.slice(0, 40).join("\n")}
          </pre>
        </details>
      ) : null}

      <div className="statistics-layout">
        <div className="group-box statistics-panel-scroll">
          <div className="group-box-title">统计学信息</div>
          {!currentFile ? (
            <div className="statistics-loading">请先选择一个 .litematic 文件</div>
          ) : data ? (
            <div className="statistics-metrics">
              <ReadOnlyRow label="网格数" value={data.enclosingSize.x * data.enclosingSize.y * data.enclosingSize.z} />
              <ReadOnlyRow label="方块数" value={data.totalNonAirBlocks} />
              <ReadOnlyRow label="密度" value={`${(data.density * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="区域数量" value={data.regionCount} />
              <ReadOnlyRow label="包围尺寸" value={`${data.enclosingSize.x}x${data.enclosingSize.y}x${data.enclosingSize.z}`} />
              <ReadOnlyRow label="结构类型" value={data.buildingType} />
              <ReadOnlyRow label="红石偏度" value={`${(data.redstoneRatio * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="液体偏度" value={`${(data.fluidRatio * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="实体种类" value={data.entityCount} />
            </div>
          ) : (
            <div className="statistics-loading">加载中...</div>
          )}
        </div>

        <div className="statistics-side-stack">
          <div className="group-box statistics-materials-panel">
            <div className="group-box-title">主要材料</div>
            <div className="statistics-materials-body">
              {!currentFile ? (
                <div className="statistics-loading">请先选择一个 .litematic 文件</div>
              ) : data ? (
                featuredMaterials.map((material) => {
                  const ratio = totalMaterialCount > 0 ? (material.totalCount / totalMaterialCount) * 100 : 0;
                  return (
                    <div key={material.id} className="statistics-material-row">
                      <div className="statistics-material-main">
                        <span className="statistics-material-name">{material.name}</span>
                        <span className="statistics-material-count">{material.totalCount}</span>
                      </div>
                      <div className="statistics-material-meta">
                        <span className="statistics-material-ratio">占比 {ratio.toFixed(2)}%</span>
                        {includeContainerItems && material.containerItemCount > 0 && (
                          <span className="statistics-material-detail">
                            方块 {material.blockCount} / 容器 {material.containerItemCount}
                          </span>
                        )}
                      </div>
                    </div>
                  );
                })
              ) : (
                <div className="statistics-loading">加载中...</div>
              )}
            </div>
          </div>

          <div className="group-box statistics-enumerator-panel">
            <div className="group-box-title">枚举器信息</div>
            <div className="statistics-enumerator-body">
              <div className="statistics-enumerator-list">
                {isLoadingEnumeratorInfoConfig || isLoadingEnumeratorCollections ? (
                  <div className="statistics-loading">加载中...</div>
                ) : enumeratorCollectionsError ? (
                  <pre className="statistics-error">{enumeratorCollectionsError}</pre>
                ) : enumeratorInfoResults.length > 0 ? (
                  enumeratorInfoResults.map((item) => (
                    <div key={item.raw} className="statistics-enumerator-row">
                      <div className="statistics-enumerator-row-main">
                        <span className="statistics-enumerator-row-title">{item.title}</span>
                        <span className="statistics-enumerator-row-value">{item.error ? "错误" : item.value}</span>
                      </div>
                      {item.error ? <div className="statistics-enumerator-row-error">{item.error}</div> : null}
                    </div>
                  ))
                ) : (
                  <div className="statistics-panel-note">暂无可显示的枚举器信息。</div>
                )}
              </div>
              <div className="statistics-panel-note">
                {runtimeCollection
                  ? `当前统计结果可作为枚举器运行时集合使用，共 ${runtimeCollection.values.length} 项。`
                  : "枚举器会基于当前统计结果生成运行时集合。"}
              </div>
              <div className="statistics-enumerator-actions">
                <button
                  className="btn"
                  type="button"
                  onClick={() => setShowEnumeratorInfoEditor(true)}
                  disabled={!data || isLoadingEnumeratorInfoConfig || isLoadingEnumeratorCollections || !!enumeratorCollectionsError || enumeratorCollections.length === 0}
                >
                  编辑信息框
                </button>
                <button className="btn" type="button" onClick={() => openEnumeratorWithWindowBehavior(currentFile, () => setShowEnumerator(true))} disabled={!data}>打开枚举器...</button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="statistics-footer">
        <div className="statistics-summary">
          {includeContainerItems && scan
            ? `容器扫描：${scan.containers_scanned} 个容器，${scan.item_stacks_scanned} 个物品堆。`
            : "当前仅统计投影方块。"}
        </div>
        <div className="statistics-file-path" title={currentFile}>
          {currentFile}
        </div>
      </div>

      {showMaterials && data && (
        <MaterialsDialog data={data} onClose={() => setShowMaterials(false)} currentFile={currentFile} />
      )}
      {showEnumerator && (
        <EnumeratorDialog
          currentFile={currentFile}
          runtimeCollection={runtimeCollection}
          theme={theme || "WebDefault"}
          onClose={() => setShowEnumerator(false)}
        />
      )}
      {showEnumeratorInfoEditor ? (
        <EnumeratorInfoEditorDialog
          rules={enumeratorInfoRules}
          data={data}
          collections={enumeratorCollections}
          onClose={() => setShowEnumeratorInfoEditor(false)}
          onSave={handleSaveEnumeratorInfoRules}
        />
      ) : null}
    </div>
  );
}

function ReadOnlyRow({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="form-row statistics-readonly-row">
      <div className="form-label statistics-readonly-label">{label}</div>
      <div className="form-field statistics-readonly-field">
        <input className="input statistics-readonly-input" value={value} readOnly />
      </div>
    </div>
  );
}

