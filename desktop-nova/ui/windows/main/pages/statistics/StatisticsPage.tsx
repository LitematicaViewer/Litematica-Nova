import { t, getUiLocale, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayFilePath } from "../../../../../src/business/facade";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Textarea } from "../../../../components/ui/textarea";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Dialog } from "../../../../components/Dialog";
import { ChevronDown, X } from "lucide-react";
import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  exportMaterialListWithOptions,
  fetchMaterialRecipeCache,
  hasInventoryMaterialSemantics,
  getI18nRevision,
  subscribeI18nChanges,
  loadStructureStats,
  DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES,
  saveStatisticsEnumeratorInfoRulesConfig,
  recordStatisticsCommit,
  findRenderOperationForSource,
  getApplicationRenderState,
  isApplicationRenderPending,
  subscribeApplicationRender,
  StatsData,
  queryStatisticsRules, getDataOperationStatus, type StatisticsRuleView,
} from "../../../../../src/business/facade";
import { StockpileTaskPanel } from "./StockpileTaskPanel";
import { MaterialCatalogDialog } from "./MaterialCatalogDialog";
import { useSyncExternalStore } from "react";
import {
  loadUserConfigMigratingLocalStorage,
  openMaterialListWindow,
} from "../../../../../src/business/facade";
import { Dropdown } from "../../../../components/Dropdown";
import "./materials.css";
import "../../../../styles/statistics-enumerator.css";
import { EnumeratorDialog, openEnumeratorWithWindowBehavior } from "../../../enumerator";

function useStatisticsRules(sourcePath: string, rules: string[], sourceRevision?: string) {
  const { locale } = useI18n();
  const [view, setView] = useState<StatisticsRuleView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    if (!sourcePath) { setView(null); setLoading(false); return; }
    const operationId = crypto.randomUUID();
    setLoading(true); setError("");
    const timer = window.setTimeout(() => {
      void queryStatisticsRules({ sourcePath, rules, uiLocale: locale }, operationId)
        .then(result => { if (active) setView(result); })
        .catch(failure => { if (active) setError(String(failure)); })
        .finally(() => { if (active) setLoading(false); });
    }, 150);
    return () => { active = false; window.clearTimeout(timer); void getDataOperationStatus(operationId, true).catch(() => undefined); };
  }, [sourcePath, rules, locale, sourceRevision]);
  return { view, loading, error };
}

function EnumeratorInfoEditorDialog({
  rules,
  sourcePath,
  onClose,
  onSave,
}: {
  rules: string[];
  sourcePath: string;
  onClose: () => void;
  onSave: (rules: string[]) => Promise<void>;
}) {
  const { locale } = useI18n();
  const [draft, setDraft] = useState(rules.join("\n"));
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<UiMessage>("");

  useEffect(() => {
    setDraft(rules.join("\n"));
  }, [rules]);

  const draftRules = useMemo(() => draft.split(/\r?\n/), [draft]);
  const preview = useStatisticsRules(sourcePath, draftRules);
  const previewResults = preview.view?.results || [];
  const hasErrors = preview.view?.hasErrors || !!preview.error;

  const handleSave = async () => {
    setIsSaving(true);
    setSaveError("");
    try {
      if (!preview.view || preview.loading) return;
      await onSave(preview.view.rules);
      onClose();
    } catch (error: any) {
      setSaveError(error?.message || error ? String(error?.message || error) : {key:"editor.saveFailed"});
    } finally {
      setIsSaving(false);
    }
  };

  return <Dialog title={t("statistics.customTitle")} subtitle={t("statistics.customSubtitle")} onClose={onClose} busy={isSaving} width="lg" footer={<>
    <Button variant="secondary" disabled={isSaving} onClick={() => setDraft(DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES.join("\n"))}>{t("statistics.restoreDefaults")}</Button>
    <Button variant="secondary" disabled={isSaving} onClick={onClose}>{t("editor.cancel")}</Button>
    <Button disabled={isSaving || preview.loading || !preview.view || hasErrors} onClick={handleSave}>{isSaving ? t("editor.saving") : t("editor.save")}</Button>
  </>}>
    <div className="statistics-enumerator-editor-columns">
      <div className="statistics-enumerator-editor-column">
        <label className="statistics-enumerator-editor-heading" htmlFor="statistics-rules">{t("statistics.rules")}</label>
        <Textarea id="statistics-rules" className="statistics-enumerator-editor-textarea" value={draft} onChange={event => setDraft(event.target.value)} spellCheck={false} disabled={isSaving} />
        <div className="statistics-enumerator-editor-hint">{t("statistics.rulesHint")}</div>
      </div>
      <div className="statistics-enumerator-editor-column"><h3 className="statistics-enumerator-editor-heading">{t("statistics.livePreview")}</h3>
        {preview.loading && <p role="status">{t("statistics.loadingRules")}</p>}
        {preview.error && <p role="alert">{preview.error}</p>}
        {previewResults.map((item,index) => <div key={index} className="statistics-enumerator-preview-item">
          <div className="statistics-enumerator-preview-main"><span>{item.title}</span><strong>{item.error ? t("editor.error") : item.value}</strong></div>
          {item.error && <p className="statistics-enumerator-preview-error" role="alert">{localizeMessage(item.error)}</p>}
        </div>)}
      </div>
    </div>
    {saveError && <pre className="subwindow-error" role="alert">{localizeMessage(saveError)}</pre>}
    {hasErrors && <p className="statistics-enumerator-editor-error">{t("statistics.fixRules")}</p>}
  </Dialog>;
}

/** Material lookup always belongs to the existing native top-level window. */
export async function openMaterialsWithWindowBehavior(currentFile: string, _showOverlay: () => void, viewMode: "details" | "export" = "details"): Promise<void> {
  if (viewMode === "export") { _showOverlay(); return; }
  await openMaterialListWindow(currentFile, "details");
}

export function MaterialListContent({ data, onClose, currentFile, standalone = false, embedded = false, viewMode = "details", onViewModeChange, exportOnly = false }: {
  data: StatsData | null; onClose?: () => void; currentFile: string; standalone?: boolean; embedded?: boolean; viewMode?:"details"|"export"; onViewModeChange?:(mode:"details"|"export")=>void; exportOnly?: boolean;
}) {
  const { locale } = useI18n();
  const languageRevision = useSyncExternalStore(subscribeI18nChanges,getI18nRevision,getI18nRevision);
  const [selectedMode,setContentMode] = useState(viewMode);
  const contentMode = exportOnly ? "export" : selectedMode;
  useEffect(() => setContentMode(viewMode),[viewMode]);
  const [multiplier, setMultiplier] = useState(1);
  const [asItems, setAsItems] = useState(false);
  const [taskPending, setTaskPending] = useState(false);
  const [includeContainerItems, setIncludeContainerItems] = useState(data?.containerScan.enabled || false);
  const [exportFormat, setExportFormat] = useState<"svg" | "txt" | "html" | "csv">("svg");
  const [terminalRawMaterials, setTerminalRawMaterials] = useState(false);
  const [workbook, setWorkbook] = useState("scope:all");
  useEffect(() => {
    setWorkbook("scope:all"); setMultiplier(1);
    setIncludeContainerItems(data?.containerScan.enabled || false);
  }, [currentFile]);
  const [webMode, setWebMode] = useState<"single" | "multi">("single");
  const [accessPassword, setAccessPassword] = useState("");
  const [adminPassword, setAdminPassword] = useState("");
  const [whitelist, setWhitelist] = useState("");
  const [whitelistEnabled, setWhitelistEnabled] = useState(false);
  const [allowGuestReadonly, setAllowGuestReadonly] = useState(false);
  const [adminPageEnabled, setAdminPageEnabled] = useState(true);
  const [webTargets, setWebTargets] = useState<Array<"windows-x64" | "linux-x64" | "macos-x64" | "macos-arm64">>(["windows-x64"]);
  const [isExporting, setIsExporting] = useState(false);
  const [isPreparingRecipes, setIsPreparingRecipes] = useState(false);
  const [recipeStatus, setRecipeStatus] = useState<UiMessage>("");
  const [exportStatus, setExportStatus] = useState<UiMessage>("");
  const [error, setError] = useState<UiMessage>("");
  const exportInFlight = useRef(false);
  const prepareRecipesInFlight = useRef(false);
  const recipeOperation = useRef<string | null>(null);
  useEffect(() => {
    setIsPreparingRecipes(false); setRecipeStatus("");
    return () => {
      const operationId = recipeOperation.current; recipeOperation.current = null; prepareRecipesInFlight.current = false;
      if (operationId) void getDataOperationStatus(operationId, true).catch(() => undefined);
    };
  }, [currentFile]);
  const busy = isExporting || isPreparingRecipes;
  const selectedScope = useMemo(() => workbook.startsWith("region:") ? { kind:"region" as const, value:workbook.slice(7) }
    : workbook.startsWith("layer:") ? { kind:"layer" as const, value:workbook.slice(6) } : { kind:"all" as const, value:null },[workbook]);
  const contentClassName = [standalone ? "material-list-window" : embedded ? "material-list-content-embedded" : "dialog-content",
    embedded ? "" : "subwindow-frame", "material-list-content", exportOnly ? "material-export-only" : "", contentMode === "details" ? "material-list-task-content" : "", standalone ? "subwindow-frame-full material-list-content-standalone" : ""].filter(Boolean).join(" ");
  const options = useMemo(() => [{ label: t("statistics.all"), value: "scope:all" },
    ...(data?.regions || []).map(region => ({ label: t("statistics.regionScope", {name:region.name}), value: `region:${region.name}` })),
    ...(data?.layers || []).map(layer => ({ label: t("statistics.layerScope", {y:layer.world_y, layer:layer.layer}), value: `layer:${layer.layer}` }))], [data?.regions, data?.layers, locale]);
  const handleExport = async () => {
    if (exportInFlight.current) return;
    exportInFlight.current = true; setIsExporting(true); setExportStatus(""); setError("");
    try {
      const scope = workbook.startsWith("region:") ? { kind: "region" as const, value: workbook.slice(7) }
        : workbook.startsWith("layer:") ? { kind: "layer" as const, value: workbook.slice(6) } : { kind: "all" as const, value: null };
      const ok = await exportMaterialListWithOptions(currentFile, {
        scope, includeContainerItems, multiplier, minecraftVersion: "26.3", terminalRawMaterials, format: exportFormat,
        webOptions: exportFormat === "html" ? { mode: exportOnly ? "single" : webMode,
          accessPassword: !exportOnly && webMode === "multi" ? accessPassword : undefined,
          adminPassword: !exportOnly && webMode === "multi" ? adminPassword : undefined,
          whitelist: exportOnly ? [] : whitelist.split(/[\s,，;；]+/).filter(Boolean), whitelistEnabled: !exportOnly && whitelistEnabled, allowGuestReadonly: !exportOnly && allowGuestReadonly, adminPageEnabled: !exportOnly && adminPageEnabled, targets: webTargets } : undefined,
      });
      if (ok) { setExportStatus({key:"statistics.exported"}); setAccessPassword(""); setAdminPassword(""); }
    } catch (error: any) { setError({ key: "statistics.exportFailed", params: {error:String(error)} }); }
    finally { exportInFlight.current = false; setIsExporting(false); }
  };
  const handlePrepareRecipes = async () => {
    if (prepareRecipesInFlight.current) return;
    const operationId = crypto.randomUUID(); recipeOperation.current = operationId;
    prepareRecipesInFlight.current = true; setIsPreparingRecipes(true); setRecipeStatus(""); setError("");
    try {
      const result = await fetchMaterialRecipeCache(currentFile, includeContainerItems, operationId);
      if (recipeOperation.current !== operationId) return;
      setRecipeStatus(result ? { key: "statistics.recipesBatchReady", params: {
        total: result.summary.totalMaterials.toLocaleString(locale), withRecipes: result.summary.materialsWithRecipes.toLocaleString(locale),
        withoutRecipes: result.summary.materialsWithoutRecipes.toLocaleString(locale), unresolved: result.summary.unresolvedMaterials.toLocaleString(locale),
        excluded: result.summary.excludedMaterials.toLocaleString(locale),
      } } : { key: "statistics.recipesReady" });
    } catch (error: unknown) { if (recipeOperation.current === operationId) setError({ key: "statistics.recipesFailed", params: {error:String(error)} }); }
    finally { if (recipeOperation.current === operationId) { recipeOperation.current = null; prepareRecipesInFlight.current = false; setIsPreparingRecipes(false); } }
  };
  return <div className={contentClassName}>
    {!embedded && contentMode !== "details" && <div className="subwindow-title-bar"><h3 className="subwindow-title">{t("editor.exportMaterials")}</h3>
      {onClose && <Button variant="ghost" size="icon" className="subwindow-close-button" aria-label={t("editor.closeWindow")} onClick={onClose}><X size={18} /></Button>}
    </div>}
    <div className="subwindow-body">
      {contentMode === "details" ? <MaterialCatalogDialog currentFile={currentFile} /> : <ScrollArea className="material-export-settings">
        <>
        <fieldset className="material-export-section" disabled={busy}>
          <legend>{t("statistics.materialFormat")}</legend>
          <div className="material-export-grid">
            <label className="material-export-field"><span>{t("statistics.scope")}</span><Dropdown value={workbook} options={options} onChange={setWorkbook} disabled={busy} ariaLabel={t("statistics.exportScope")} /></label>
            <label className="material-export-field"><span>{t("statistics.copies")}</span><Input type="number" min={1} max={1000000} aria-label={t("statistics.copies")} value={multiplier}
              onChange={event => setMultiplier(Math.min(1000000, Math.max(1, Math.floor(Number(event.target.value) || 1))))} /></label>
            <label className="material-export-field"><span>{t("statistics.exportFormat")}</span><Dropdown value={exportFormat} options={[{ label: t("statistics.svgArt"), value: "svg" }, { label: "CSV", value: "csv" }, { label: "TXT", value: "txt" }, { label: t("statistics.web"), value: "html" }]}
              onChange={value => setExportFormat(value as typeof exportFormat)} disabled={busy} ariaLabel={t("statistics.exportFormatLabel")} /></label>
          </div>
          <div className="beta-material-options">
            <label className="beta-material-check"><Checkbox checked={includeContainerItems} disabled={busy} onCheckedChange={checked => setIncludeContainerItems(checked === true)} />{t("statistics.includeContainers")}</label>
            <label className="beta-material-check"><Checkbox checked={terminalRawMaterials} disabled={busy} onCheckedChange={checked => setTerminalRawMaterials(checked === true)} />{t("statistics.terminalExport")}</label>
          </div>
          <p className="beta-statistics-status">{t("statistics.scopeHint")}</p>
        </fieldset>
        {terminalRawMaterials && <section className="material-export-section">
          <h4>{t("statistics.terminalRecipes")}</h4><p className="beta-statistics-status">{t("statistics.recipesHint")}</p>
          <Button title={t("statistics.prepareRecipesHint")} onClick={handlePrepareRecipes} disabled={busy}>{isPreparingRecipes ? t("statistics.preparingRecipes") : t("statistics.prepareRecipes")}</Button>
          {recipeStatus && <p role="status">{localizeMessage(recipeStatus)}</p>}
        </section>}
        {exportFormat === "html" && !exportOnly && <fieldset className="material-export-section" disabled={busy}>
          <legend>{t("statistics.webOptions")}</legend>
          <label className="material-export-field"><span>{t("statistics.webMode")}</span><Dropdown value={webMode} options={[{ label: t("statistics.webSingle"), value: "single" }, { label: t("statistics.webMulti"), value: "multi" }]}
            onChange={value => setWebMode(value as typeof webMode)} disabled={busy} ariaLabel={t("statistics.webUsage")} /></label>
          {webMode === "multi" && <>
            <div className="material-export-grid">
              <label className="material-export-field"><span>{t("statistics.accessPassword")}</span><Input type="password" autoComplete="new-password" aria-label={t("statistics.webAccessPassword")} value={accessPassword} onChange={event => setAccessPassword(event.target.value)} /></label>
              <label className="material-export-field"><span>{t("statistics.adminPassword")}</span><Input type="password" autoComplete="new-password" aria-label={t("statistics.webAdminPassword")} value={adminPassword} onChange={event => setAdminPassword(event.target.value)} /></label>
              <label className="material-export-field material-export-field-wide"><span>{t("statistics.whitelist")}</span><Input aria-label={t("statistics.whitelist")} placeholder={t("statistics.whitelistHint")} value={whitelist} onChange={event => setWhitelist(event.target.value)} /></label>
            </div>
            <div className="beta-material-options">
              <label className="beta-material-check"><Checkbox checked={whitelistEnabled} disabled={busy} onCheckedChange={checked => setWhitelistEnabled(checked === true)} />{t("statistics.enableWhitelist")}</label>
              <label className="beta-material-check"><Checkbox checked={allowGuestReadonly} disabled={busy} onCheckedChange={checked => setAllowGuestReadonly(checked === true)} />{t("statistics.guestReadonly")}</label>
              <label className="beta-material-check"><Checkbox checked={adminPageEnabled} disabled={busy} onCheckedChange={checked => setAdminPageEnabled(checked === true)} />{t("statistics.enableAdmin")}</label>
            </div>
            <fieldset className="material-export-targets"><legend>{t("statistics.targets")}</legend>
              {([["windows-x64", "Windows x64"], ["linux-x64", "Linux x64"], ["macos-x64", "macOS x64"], ["macos-arm64", "macOS ARM64"]] as const).map(([value, label]) =>
                <label className="beta-material-check" key={value}><Checkbox checked={webTargets.includes(value)} disabled={busy}
                  onCheckedChange={checked => setWebTargets(previous => checked === true ? [...previous.filter(target => target !== value), value] : previous.filter(target => target !== value))} />{label}</label>)}
            </fieldset>
          </>}
        </fieldset>}
        </>
      </ScrollArea>}
      <div className="material-export-footer">
        {!exportOnly && <Button variant="secondary" disabled={busy || taskPending} onClick={() => { const mode = contentMode === "details" ? "export" : "details"; setContentMode(mode); onViewModeChange?.(mode); }}>{contentMode === "details" ? t("statistics.openExport") : t("statistics.returnDetails")}</Button>}
        {contentMode === "export" && <Button onClick={handleExport} disabled={busy || !currentFile}>{isExporting ? t("statistics.exporting") : t("editor.exportMaterials")}</Button>}
        {exportStatus && <span role="status">{localizeMessage(exportStatus)}</span>}
      {error && <pre className="subwindow-error" role="alert">{localizeMessage(error)}</pre>}
      </div>
    </div>
  </div>;
}

export function MaterialsDialog({ data, onClose, currentFile, viewMode = "details" }: { data: StatsData | null; onClose: () => void; currentFile: string; viewMode?:"details"|"export" }) {
  const { locale } = useI18n();
  const [mode,setMode] = useState(viewMode);
  return (
    <Dialog title={mode === "details" ? t("editor.materialDetails") : t("editor.exportMaterials")} onClose={onClose} width="lg" className="materials-dialog">
      <MaterialListContent data={data} currentFile={currentFile} embedded viewMode={mode} onViewModeChange={setMode} />
    </Dialog>
  );
}

export function StatisticsPage({ currentFile, theme, debugMode = false }: any) {
  const { locale } = useI18n();
  const languageRevision = useSyncExternalStore(subscribeI18nChanges,getI18nRevision,getI18nRevision);
  const applicationRender = useSyncExternalStore(
    subscribeApplicationRender,
    getApplicationRenderState,
    getApplicationRenderState,
  );
  const [data, setData] = useState<StatsData | null>(null);
  const [error, setError] = useState<UiMessage>("");

  const includeContainerItems = false;
  const [isLoading, setIsLoading] = useState(false);
  const statsGeneration = useRef(0);
  const statsOperation = useRef<string | null>(null);
  const [statsProgress, setStatsProgress] = useState<{ current: number; total: number } | null>(null);
  const [statsStatus, setStatsStatus] = useState("");
  const [showEnumerator, setShowEnumerator] = useState(false);
  useEffect(() => { if (!debugMode) setShowEnumerator(false); }, [debugMode]);
  const [showEnumeratorInfoEditor, setShowEnumeratorInfoEditor] = useState(false);
  const [enumeratorError, setEnumeratorError] = useState<UiMessage>("");
  const [enumeratorRules, setEnumeratorRules] = useState<string[]>([...DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES]);
  const [configLoading, setConfigLoading] = useState(true);
  const runtimeCollection = null;
  useEffect(() => {
    let active = true;
    loadUserConfigMigratingLocalStorage().then(info => { if(active) setEnumeratorRules(info.config.statistics_enumerator_info_rules); })
      .catch(error => { if(active) setEnumeratorError({ key: "statistics.rulesFailed", params: {error:String(error)} }); })
      .finally(() => { if(active) setConfigLoading(false); });
    return () => { active = false; };
  },[]);
  const statisticsRules = useStatisticsRules(currentFile, enumeratorRules, data?.sourceRevision);
  const enumeratorResults = statisticsRules.view?.results || [];
  const enumeratorLoading = statisticsRules.loading;
  const saveEnumeratorRules = async (rules:string[]) => {
    const info = await saveStatisticsEnumeratorInfoRulesConfig(rules);
    setEnumeratorRules(info.config.statistics_enumerator_info_rules);
  };

  const renderOperation = useMemo(
    () => findRenderOperationForSource(currentFile, applicationRender),
    [applicationRender.operations, currentFile],
  );
  const applicationAnalysis = hasInventoryMaterialSemantics(renderOperation?.analysis) && Array.isArray(renderOperation?.analysis?.structural_block_counts) ? renderOperation!.analysis : null;
  const applicationAnalysisKey = applicationAnalysis ? renderOperation?.operationId ?? "analysis" : "";
  const renderPending = isApplicationRenderPending(renderOperation);
  useLayoutEffect(() => {
    if (!data?.timing) return;
    console.info(`[NOVA_STATS_TIMING] request_to_list_commit_ms=${(performance.now() - data.timing.requestStarted).toFixed(3)} map_to_commit_ms=${(performance.now() - data.timing.mappedAt).toFixed(3)} cache_reused=${data.timing.cacheReused}`);
  }, [data, isLoading]);
  const loadStats = async () => {
    if (!currentFile) return;
    const generation = ++statsGeneration.current;
    if (statsOperation.current) void getDataOperationStatus(statsOperation.current, true).catch(() => undefined);
    const operationId = crypto.randomUUID(); statsOperation.current = operationId; setStatsProgress(null); setStatsStatus("");
    const source = "data-worker";
    console.info(`[NOVA_STATS_REQUEST] source=${source} path=${currentFile} request started`);
    setError("");
    setIsLoading(true);
    try {
      const next = await loadStructureStats(currentFile, includeContainerItems, operationId);
      if (generation !== statsGeneration.current) return;
      setData(next);
      console.info(`[NOVA_STATS_REQUEST] source=${source} path=${currentFile} request succeeded`);
    } catch (err: any) {
      if (generation === statsGeneration.current) {
        setData(null);
        setError(String(err));
        console.error(`[NOVA_STATS_REQUEST] source=${source} path=${currentFile} request failed: ${String(err)}`);
      }
    } finally {
      if (generation === statsGeneration.current) { setIsLoading(false); statsOperation.current = null; }
    }
  };

  useEffect(() => {
    if (!currentFile) {
      statsGeneration.current += 1;
      setData(null);
      setError("");
      setIsLoading(false);
      return;
    }
    setData(null);
    void loadStats();
    return () => { statsGeneration.current += 1; if (statsOperation.current) { void getDataOperationStatus(statsOperation.current, true).catch(() => undefined); statsOperation.current = null; } };
  }, [currentFile, includeContainerItems, applicationAnalysisKey, languageRevision]);

  useEffect(() => {
    if (!isLoading) return;
    let active = true;
    const timer = window.setInterval(() => {
      const operationId = statsOperation.current; if (!operationId) return;
      void getDataOperationStatus(operationId).then(operation => {
        if (!active || operationId !== statsOperation.current) return;
        const progress = operation.progress;
        if (progress?.total !== undefined) setStatsProgress({ current: progress.completed ?? progress.current ?? 0, total: progress.total });
      }).catch(() => undefined);
    }, 300);
    return () => { active = false; window.clearInterval(timer); };
  }, [isLoading]);
  const cancelStats = () => {
    const operationId = statsOperation.current; statsGeneration.current += 1; statsOperation.current = null;
    if (operationId) void getDataOperationStatus(operationId, true).catch(failure => setError(String(failure)));
    setIsLoading(false); setStatsStatus(t("statistics.analysisCancelled"));
  };
  const scan = data?.containerScan;
  const percent = (value: number | null) => value === null ? t("statistics.notProvided") : `${(value * 100).toLocaleString(locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}%`;
  const metrics = data ? [
    { label: t("statistics.nonAir"), value: data.totalNonAirBlocks.toLocaleString(locale) },
    { label: t("statistics.bounds"), value: `${data.enclosingSize.x} × ${data.enclosingSize.y} × ${data.enclosingSize.z}` },
    { label: t("statistics.density"), value: percent(data.density) },
    { label: t("statistics.materialKindsOverview"), value: data.materialCount.toLocaleString(locale) },
    { label: t("statistics.entities"), value: data.entityCount.toLocaleString(locale) },
  ] : [];
  return <React.Profiler id="statistics" onRender={recordStatisticsCommit}><div className="beta-statistics-page statistics-page">
    <section className="statistics-overview" aria-label={t("statistics.structure")} aria-busy={isLoading}>
      <header className="statistics-overview-header">
        <div className="statistics-overview-title">
          <h2 className="statistics-overview-source" title={currentFile ? displayFilePath(currentFile) : undefined}>{currentFile ? currentFile.split(/[\\/]/).pop() : t("statistics.title")}</h2>
          <p>{t("statistics.workbenchContext")}</p>
        </div>
        <Button size="sm" variant="secondary" onClick={() => isLoading ? cancelStats() : void loadStats()} disabled={!currentFile}>{isLoading ? t("statistics.cancelAnalysis") : t("statistics.reanalyze")}</Button>
      </header>
      {data ? <>
        <dl className="statistics-overview-kpis statistics-kpis">{metrics.map(metric => <div className="statistics-overview-kpi statistics-kpi" key={metric.label}><dt>{metric.label}</dt><dd>{metric.value}</dd></div>)}</dl>
        <details className="statistics-overview-details statistics-details">
          <summary>
            <span className="statistics-overview-secondary statistics-secondary">
              <span>{localizeMessage(data.buildingType)}</span>
              <span>{t("statistics.redstoneRatio")} {percent(data.redstoneRatio)}</span>
              <span>{t("statistics.fluidRatio")} {percent(data.fluidRatio)}</span>
              <span>{t("statistics.regions")} {data.regionCount.toLocaleString(locale)}</span>
            </span>
            <span className="statistics-overview-toggle">{t("statistics.structureDetails")} <ChevronDown size={14} aria-hidden="true" /></span>
          </summary>
          <dl className="statistics-overview-facts statistics-facts beta-statistics-facts">
            <ReadOnlyRow label={t("statistics.nonAir")} value={data.totalNonAirBlocks} />
            <ReadOnlyRow label={t("statistics.regions")} value={data.regionCount} />
            <ReadOnlyRow label={t("statistics.bounds")} value={`${data.enclosingSize.x} × ${data.enclosingSize.y} × ${data.enclosingSize.z}`} />
            <ReadOnlyRow label={t("statistics.density")} value={percent(data.density)} />
            <ReadOnlyRow label={t("statistics.buildingType")} value={localizeMessage(data.buildingType)} />
            <ReadOnlyRow label={t("statistics.redstoneRatio")} value={percent(data.redstoneRatio)} />
            <ReadOnlyRow label={t("statistics.fluidRatio")} value={percent(data.fluidRatio)} />
            <ReadOnlyRow label={t("statistics.entities")} value={data.entityCount} />
          </dl>
          <section className="statistics-enumerator-panel" aria-busy={enumeratorLoading || configLoading}>
            <header className="material-summary-header"><h3>{t("statistics.customTitle")}</h3>
              <Button size="sm" variant="secondary" disabled={configLoading || enumeratorLoading || !data || !!enumeratorError} onClick={() => setShowEnumeratorInfoEditor(true)}>{t("statistics.editRules")}</Button>
              {debugMode && <Button size="sm" variant="ghost" onClick={() => openEnumeratorWithWindowBehavior(currentFile, () => setShowEnumerator(true))}>{t("statistics.openEnumerator")}</Button>}
            </header>
            {enumeratorLoading || configLoading ? <p role="status">{t("statistics.loadingRules")}</p> : enumeratorError || statisticsRules.error ? <pre role="alert">{localizeMessage(enumeratorError || statisticsRules.error)}</pre>
              : enumeratorResults.map((item, index) => <div key={index} className="statistics-enumerator-row"><div className="statistics-enumerator-row-main"><span>{item.title}</span><strong>{item.error ? t("editor.error") : item.value}</strong></div>{item.error && <p className="statistics-enumerator-row-error">{localizeMessage(item.error)}</p>}</div>)}
          </section>
          {scan?.enabled && <p className="beta-statistics-status">{t("statistics.scanned", { containers: scan.containers_scanned.toLocaleString(locale), stacks: scan.item_stacks_scanned.toLocaleString(locale) })}</p>}
          {!!scan?.warnings?.length && <div className="beta-statistics-warnings"><p>{t("statistics.containerWarnings", { count: scan.warnings.length.toLocaleString(locale) })}</p><pre>{scan.warnings.slice(0, 40).map(localizeMessage).join("\n")}</pre></div>}
        </details>
      </> : <p className="beta-statistics-status" role="status">{!currentFile ? t("statistics.noProjectionOverview") : renderPending ? t("statistics.reading") : isLoading ? t("statistics.analyzing") : t("statistics.noData")}</p>}
      {isLoading && statsProgress && <p role="status">{t("statistics.analysisProgress", { current: statsProgress.current, total: statsProgress.total })}</p>}
      {statsStatus && <p role="status">{statsStatus}</p>}
      {error && <pre className="subwindow-error" role="alert">{localizeMessage(error)}</pre>}
    </section>
    <section className="statistics-stockpile-workspace" aria-label={t("editor.materialDetails")}>
      <StockpileTaskPanel sourcePath={currentFile || ""} onOpenMaterials={() => { void openMaterialListWindow(currentFile).catch(failure => setError(String(failure))); }}
        materialExportContent={data && currentFile ? <MaterialListContent data={data} currentFile={currentFile} embedded viewMode="export" exportOnly /> : undefined} />
    </section>
    {showEnumerator && <EnumeratorDialog currentFile={currentFile} theme={theme} runtimeCollection={runtimeCollection} onClose={() => setShowEnumerator(false)} />}
    {showEnumeratorInfoEditor && <EnumeratorInfoEditorDialog rules={enumeratorRules} sourcePath={currentFile} onClose={() => setShowEnumeratorInfoEditor(false)} onSave={saveEnumeratorRules} />}
  </div></React.Profiler>;
}

function ReadOnlyRow({ label, value }: { label: string; value: string | number }) {
  return <div><dt>{label}</dt><dd>{typeof value === "number" ? value.toLocaleString(getUiLocale()) : value}</dd></div>;
}
