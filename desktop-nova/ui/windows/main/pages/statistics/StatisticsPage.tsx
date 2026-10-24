import { t, getUiLocale, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayFilePath } from "../../../../../src/business/facade";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Textarea } from "../../../../components/ui/textarea";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Dialog } from "../../../../components/Dialog";
import { ChevronDown } from "lucide-react";
import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  executeConfiguredMaterialExport,
  getMinecraftTranslations,
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
import { StockpileTaskPanel, type StockpileExportContext } from "./StockpileTaskPanel";
import { commandMaterialExport, type MaterialExportView, type MaterialExportField, type MaterialExportInput } from "../../../../../src/platform/applicationOperations";
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

/** A field renders only the Rust schema; it never decides export semantics. */
function MaterialExportControl({ field, value, busy, onChange }: {
  field: MaterialExportField; value: unknown; busy: boolean; onChange: (value: unknown) => void;
}) {
  const disabled = busy || !field.enabled;
  const label = t(field.labelKey);
  const id = `material-export-${field.key}`;
  const options = (field.options || []).map(option => ({ value: option.value, label: option.labelKey ? t(option.labelKey) : option.label || option.value }));
  const helpId = `${id}-help`;
  const help = field.reasonKey ? t(field.reasonKey) : field.descriptionKey ? t(field.descriptionKey) : "";
  const control = field.type === "boolean" ? <label className="beta-material-check" htmlFor={id}><Checkbox id={id} checked={value === true} disabled={disabled} aria-describedby={help ? helpId : undefined} onCheckedChange={checked => onChange(checked === true)} />{label}</label>
    : field.type === "select" ? <><span>{label}</span><Dropdown id={id} ariaLabel={label} value={String(value ?? "")} options={options} disabled={disabled} onChange={onChange} /></>
    : field.type === "multiselect" ? <fieldset className="material-export-targets" disabled={disabled} aria-describedby={help ? helpId : undefined}><legend>{label}</legend>{options.map(option => <label key={option.value} className="beta-material-check"><Checkbox checked={Array.isArray(value) && value.includes(option.value)} disabled={disabled}
      onCheckedChange={checked => onChange(checked === true ? [...(Array.isArray(value) ? value : []).filter(entry => entry !== option.value), option.value] : (Array.isArray(value) ? value : []).filter(entry => entry !== option.value))} />{option.label}</label>)}</fieldset>
    : field.type === "multiline" ? <><span>{label}</span><Textarea id={id} rows={3} aria-label={label} aria-describedby={help ? helpId : undefined} value={String(value ?? "")} disabled={disabled} onChange={event => onChange(event.target.value)} /></>
    : <><span>{label}</span><Input id={id} aria-label={label} aria-describedby={help ? helpId : undefined} type={field.type === "password" ? "password" : field.type === "number" ? "number" : "text"}
      {...(field.type === "password" ? { autoComplete: "new-password" } : {})} min={field.min} max={field.max} step={field.step} value={String(value ?? "")} disabled={disabled}
      onChange={event => onChange(field.type === "number" && event.target.value !== "" ? Number(event.target.value) : event.target.value)} /></>;
  return <div className={`material-export-field${field.type === "multiselect" || field.type === "multiline" ? " material-export-field-wide" : ""}`}>
    {field.type === "boolean" || field.type === "multiselect" ? control : <label htmlFor={id}>{control}</label>}
    {help && <p id={helpId} className="material-export-help">{help}</p>}
  </div>;
}

export function MaterialExportDialog({ sourcePath, taskId, projectId, sourceRevision, query, onClose, beforeExport }: StockpileExportContext & { sourcePath: string; onClose: () => void }) {
  const { locale } = useI18n();
  const [view, setView] = useState<MaterialExportView | null>(null);
  const [draft, setDraft] = useState<Record<string, unknown>>({});
  const draftRef = useRef<Record<string, unknown>>({});
  const [configuring, setConfiguring] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [recipeBusy, setRecipeBusy] = useState(false);
  const recipeOperation = useRef<string | null>(null);
  const sequence = useRef(0);
  const mounted = useRef(true);
  const inFlight = useRef(false);
  const context = useRef<MaterialExportInput>({ action: "configure" });
  context.current = { action: "configure", sourcePath: sourcePath || undefined, taskId, projectId, sourceRevision, query, uiLocale: locale, languageMap: getMinecraftTranslations() };
  const configure = async (settings?: Record<string, unknown>, reset = false): Promise<MaterialExportView | null> => {
    const expected = ++sequence.current;
    setConfiguring(true); setError("");
    try {
      const next = await commandMaterialExport({ ...context.current, action: reset ? "reset" : "configure", settings });
      if (!mounted.current || expected !== sequence.current) return null;
      draftRef.current = next.settings; setDraft(next.settings); setView(next);
      return next;
    } catch (failure) { if (mounted.current && expected === sequence.current) setError(String(failure)); return null; }
    finally { if (mounted.current && expected === sequence.current) setConfiguring(false); }
  };
  useEffect(() => {
    mounted.current = true; draftRef.current = {}; setDraft({}); setView(null); setStatus("");
    void configure();
    return () => {
      mounted.current = false; sequence.current += 1;
      if (recipeOperation.current) void getDataOperationStatus(recipeOperation.current, true).catch(() => undefined);
      recipeOperation.current = null;
    };
  }, [sourcePath, taskId, locale]);
  const change = (key: string, value: unknown) => {
    const next = { ...draftRef.current, [key]: value }; draftRef.current = next; setDraft(next); setStatus("");
    void configure(next);
  };
  const confirm = async () => {
    if (inFlight.current || configuring || !view?.valid || error) return;
    const captured = { ...context.current };
    const settings = { ...draftRef.current };
    const sameSource = () => mounted.current && captured.sourcePath === context.current.sourcePath && captured.taskId === context.current.taskId && captured.sourceRevision === context.current.sourceRevision;
    inFlight.current = true; setBusy(true); setError(""); setStatus("");
    try {
      await beforeExport?.();
      if (!sameSource()) return;
      const current = await configure(settings);
      if (!current?.valid || !sameSource()) return;
      const result = await executeConfiguredMaterialExport(captured, current);
      if (result && mounted.current) {
        setStatus(t(current.copy ? "stockpile.copied" : "statistics.exported"));
        await configure({ ...draftRef.current, accessPassword: "", adminPassword: "" });
      }
    } catch (failure) { if (mounted.current) setError(String(failure)); }
    finally { inFlight.current = false; if (mounted.current) setBusy(false); }
  };
  const prepareRecipes = async () => {
    if (recipeOperation.current || !sourcePath) return;
    const operationId = crypto.randomUUID(); recipeOperation.current = operationId; setRecipeBusy(true); setStatus(""); setError("");
    try {
      const result = await fetchMaterialRecipeCache(sourcePath, draftRef.current.includeContainerItems === true, operationId);
      if (!mounted.current || recipeOperation.current !== operationId) return;
      setStatus(result ? t("statistics.recipesBatchReady", {
        total: result.summary.totalMaterials.toLocaleString(locale), withRecipes: result.summary.materialsWithRecipes.toLocaleString(locale),
        withoutRecipes: result.summary.materialsWithoutRecipes.toLocaleString(locale), unresolved: result.summary.unresolvedMaterials.toLocaleString(locale), excluded: result.summary.excludedMaterials.toLocaleString(locale),
      }) : t("stats.export.recipesReady"));
      await configure(draftRef.current);
    } catch (failure) { if (mounted.current) setError(String(failure)); }
    finally { if (recipeOperation.current === operationId) { recipeOperation.current = null; if (mounted.current) setRecipeBusy(false); } }
  };
  const visibleFields = view?.fields.filter(field => field.visible) || [];
  const groups = [...new Set(visibleFields.map(field => field.group))];
  return <Dialog title={t("stockpile.openExport")} onClose={onClose} width="lg" className="stockpile-export-dialog" busy={busy || recipeBusy}
    footer={<><Button variant="ghost" disabled={busy || recipeBusy || configuring} onClick={() => { setStatus(""); void configure(undefined, true); }}>{t("stats.export.reset")}</Button>
      <span className="material-export-footer-spacer" />
      <Button variant="ghost" disabled={busy || recipeBusy} onClick={onClose}>{t("editor.cancel")}</Button>
      <Button disabled={busy || recipeBusy || configuring || !view?.valid || !!error} onClick={() => void confirm()}>{busy ? t("statistics.exporting") : t("stockpile.confirmExport")}</Button></>}>
    {groups.map(group => <fieldset key={group} className={`material-export-section material-export-group-${group}`} disabled={busy || recipeBusy}>
      <legend>{t(`stats.export.group.${group}`)}</legend><div className="material-export-grid">{visibleFields.filter(field => field.group === group).map(field =>
        <MaterialExportControl key={field.key} field={field} value={Object.prototype.hasOwnProperty.call(draft, field.key) ? draft[field.key] : field.value} busy={busy || recipeBusy} onChange={value => change(field.key, value)} />)}</div>
      {visibleFields.some(field => field.group === group && field.key === "terminalRawMaterials") && <section className="material-export-recipes"><h4>{t("statistics.terminalRecipes")}</h4><p className="material-export-help">{t("stats.export.recipesHint")}</p>
        <Button variant="secondary" title={t("statistics.prepareRecipesHint")} disabled={busy || recipeBusy || !sourcePath} onClick={() => void prepareRecipes()}>{recipeBusy ? t("statistics.preparingRecipes") : t("stats.export.prepareRecipes")}</Button>
      </section>}
    </fieldset>)}
    {configuring && <p className="material-export-help" role="status">{t("stats.export.configuring")}</p>}
    {!!view?.errors.length && <ul className="material-export-errors" role="alert">{view.errors.map((message, index) => <li key={index}>{localizeMessage(message)}</li>)}</ul>}
    {error && <p className="subwindow-error" role="alert">{localizeMessage(error)}</p>}
    {status && <p role="status">{status}</p>}
  </Dialog>;
}

/** Kept for existing callers; lookup stays read-only and export uses one schema. */
export function MaterialListContent({ onClose, currentFile, standalone = false, viewMode = "details", exportOnly = false }: {
  data: StatsData | null; onClose?: () => void; currentFile: string; standalone?: boolean; embedded?: boolean; viewMode?: "details" | "export"; onViewModeChange?: (mode: "details" | "export") => void; exportOnly?: boolean;
}) {
  if (viewMode === "export" || exportOnly) return <MaterialExportDialog sourcePath={currentFile} onClose={onClose || (() => undefined)} />;
  return <div className={standalone ? "material-list-window" : "material-catalog-content"}><MaterialCatalogDialog currentFile={currentFile} /></div>;
}

export function MaterialsDialog({ currentFile, onClose, viewMode = "details" }: { data: StatsData | null; onClose: () => void; currentFile: string; viewMode?: "details" | "export" }) {
  if (viewMode === "export") return <MaterialExportDialog sourcePath={currentFile} onClose={onClose} />;
  return <Dialog title={t("editor.materialDetails")} onClose={onClose} width="lg" className="materials-dialog"><MaterialCatalogDialog currentFile={currentFile} /></Dialog>;
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
        renderExportDialog={(context, onClose) => <MaterialExportDialog {...context} sourcePath={currentFile || ""} onClose={onClose} />} />
    </section>
    {showEnumerator && <EnumeratorDialog currentFile={currentFile} theme={theme} runtimeCollection={runtimeCollection} onClose={() => setShowEnumerator(false)} />}
    {showEnumeratorInfoEditor && <EnumeratorInfoEditorDialog rules={enumeratorRules} sourcePath={currentFile} onClose={() => setShowEnumeratorInfoEditor(false)} onSave={saveEnumeratorRules} />}
  </div></React.Profiler>;
}

function ReadOnlyRow({ label, value }: { label: string; value: string | number }) {
  return <div><dt>{label}</dt><dd>{typeof value === "number" ? value.toLocaleString(getUiLocale()) : value}</dd></div>;
}
