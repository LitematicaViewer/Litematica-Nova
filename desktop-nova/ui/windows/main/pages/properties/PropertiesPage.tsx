import { FormatExportDialog } from "../../FormatExportDialog";
import { t, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayPathText } from "../../../../../src/business/facade";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { takeUpdateDraft, useUpdateDraft } from "../../updateRecovery";
import {
  analyzeProjectionFile,
  saveProjectionMetadataPatch,
  selectLitematicFile,
  selectLitematicSavePath,
  readProjectionPreviewImage,
  selectPreviewImageFile,
  importProjectionPreviewImage,
  clearProjectionPreviewImage,
  renderPreviewImage,
} from "../../../../../src/business/facade";
import { emitEvent, listenEvent } from "../../../../../src/platform/events";
import { projectionLibraryStateChangedEvent } from "../../../libraryEvents";
import {
  addOrUpdateRecordFromAnalysis,
  loadLibrary,
  projectionRecordAnalysis,
} from "../../../../../src/business/facade";
import {
  findRenderOperationForSource,
  getApplicationRenderState,
  subscribeApplicationRender,
} from "../../../../../src/business/facade";

import { Box, Download, FolderOpen, Library, Play, RotateCcw, Save, ExternalLink, ImagePlus, ImageOff } from "lucide-react";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Textarea } from "../../../../components/ui/textarea";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from "../../../../components/ui/table";
import { useQuickPreview } from "../../useQuickPreview";
import { useFullPreview } from "../../useFullPreview";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Dialog } from "../../../../components/Dialog";
import { PreviewDraftDialog } from "../../../../components/PreviewDraftDialog";
import type { ProductPreviewImage } from "../../../../../src/platform/previewImage";
import "./properties.css";
import { displayFilePath, minecraftVersionLabel, windowsPathKey } from "../../../../../src/business/projectionLibrary";
import { VersionConversionDialog } from "./VersionConversionDialog";
import { WorldExportDialog } from "./WorldExportDialog";
import { formatResourceBytes } from "../../../../../src/business/resourcePreflightUnits";

type RegionEdit = {
  originalName: string;
  name: string;
  position: { x: number; y: number; z: number };
  size: { x: number; y: number; z: number };
};

type MetadataForm = {
  name: string;
  author: string;
  description: string;
  time_created: string;
  time_modified: string;
  litematic_version: number;
  litematic_subversion: number;
  minecraft_data_version: number;
  regions: RegionEdit[];
};

function toLocalDateTime(value: unknown): string {
  const n = Number(value || 0);
  if (!Number.isFinite(n) || n <= 0) return "";
  const d = new Date(n);
  const pad = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function fromLocalDateTime(value: string): number | undefined {
  if (!value) return undefined;
  const ms = new Date(value).getTime();
  return Number.isFinite(ms) ? ms : undefined;
}

function metadataToForm(data: any): MetadataForm {
  const meta = data?.metadata || {};
  const regions = (data?.regions || []).map((region: any): RegionEdit => ({
    originalName: region.name || "Unnamed",
    name: region.name || "Unnamed",
    position: region.position || { x: 0, y: 0, z: 0 },
    size: region.size || { x: 0, y: 0, z: 0 },
  }));
  return {
    name: meta.name || "",
    author: meta.author || "",
    description: meta.description || "",
    time_created: toLocalDateTime(meta.time_created),
    time_modified: toLocalDateTime(meta.time_modified),
    litematic_version: Number(meta.litematic_version || 6),
    litematic_subversion: Number(meta.litematic_subversion || 1),
    minecraft_data_version: Number(meta.minecraft_data_version || 0),
    regions,
  };
}

function buildPatch(form: MetadataForm) {
  return {
    name: form.name,
    author: form.author,
    description: form.description,
    time_created: fromLocalDateTime(form.time_created),
    time_modified: fromLocalDateTime(form.time_modified),
    regions: form.regions
      .filter((region) => region.name !== region.originalName)
      .map((region) => ({ old_name: region.originalName, new_name: region.name })),
  };
}

function validateRegions(regions: RegionEdit[]): UiMessage {
  const names = new Set<string>();
  for (const region of regions) {
    const name = region.name.trim();
    if (!name) return {key: "properties.emptyRegion"};
    if (names.has(name)) return { key: "properties.duplicateRegion", params: {name} };
    names.add(name);
  }
  return "";
}

export function PropertiesPage({ currentFile, setCurrentFile, setRoute, onDirtyChange, presentationBlocked = false }: any) {
  const { locale } = useI18n();
  const applicationRender = React.useSyncExternalStore(
    subscribeApplicationRender,
    getApplicationRenderState,
    getApplicationRenderState,
  );
  const [data, setData] = useState<any>(null);
  const [initialForm, setInitialForm] = useState<MetadataForm | null>(null);
  const [form, setForm] = useState<MetadataForm | null>(null);
  const [undoDrafts, setUndoDrafts] = useState<MetadataForm[]>([]);
  const [redoDrafts, setRedoDrafts] = useState<MetadataForm[]>([]);
  useUpdateDraft(`properties:${currentFile}`, { form, initialForm });
  const [error, setError] = useState<UiMessage>("");
  const [status, setStatus] = useState<UiMessage>("");
  const [convertOpen, setConvertOpen] = useState(false);
  const [worldExportOpen, setWorldExportOpen] = useState(false);
  const [versionConvertOpen, setVersionConvertOpen] = useState(false);
  const [renderMode, setRenderMode] = useState<"fast" | "standard" | "full">("fast");
  const [preload, setPreload] = useState(false);
  const [filePreview, setFilePreview] = useState("");
  const [filePreviewBusy, setFilePreviewBusy] = useState(false);
  const [capturingPreview, setCapturingPreview] = useState(false);
  const [previewDraft, setPreviewDraft] = useState<{ source: string; image: ProductPreviewImage } | null>(null);
  const filePreviewWrite = useRef(false);
  const filePreviewRevision = useRef(0);
  const [clearPreviewConfirm, setClearPreviewConfirm] = useState(false);
  const analysisGeneration = useRef(0);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const sourceRef = useRef(currentFile);
  sourceRef.current = currentFile;

  const renderOperation = useMemo(
    () => findRenderOperationForSource(currentFile, applicationRender),
    [applicationRender.operations, currentFile],
  );
  const applicationAnalysis = renderOperation?.analysis ?? null;
  const preview = useQuickPreview(currentFile || "", presentationBlocked || convertOpen || worldExportOpen || versionConvertOpen || clearPreviewConfirm || filePreviewBusy || renderMode !== "fast", preload);
  const fullPreview = useFullPreview(currentFile || "", preload, presentationBlocked || convertOpen || worldExportOpen || versionConvertOpen || clearPreviewConfirm || filePreviewBusy || renderMode === "fast", renderMode === "standard" ? "standard" : "full");
  const quickOperation = preview.operation;
  const renderPending = renderMode === "fast" ? quickOperation?.state === "starting" && !quickOperation.error : fullPreview.loading;
  const viewerPrepared = renderMode === "fast" ? quickOperation?.state === "ready" : fullPreview.ready;
  const previewVisible = renderMode === "fast" ? quickOperation?.presentation.visible === true : fullPreview.visible;
  const previewPending = renderMode === "fast" ? preview.pending : fullPreview.pending;
  const handlePreview = renderMode === "fast" ? preview.start : fullPreview.start;
  const handleCancelPreview = renderMode === "fast" ? preview.close : fullPreview.close;
  useEffect(() => {
    const revision = ++filePreviewRevision.current;
    setFilePreview(""); setClearPreviewConfirm(false);
    if (currentFile) void readProjectionPreviewImage(currentFile).then((image) => {
      if (filePreviewRevision.current === revision) setFilePreview(image?.data_url || "");
    }).catch((reason) => { if (filePreviewRevision.current === revision) setError({ key: "properties.previewReadFailed", params: {error: String(reason)} }); });
    return () => { ++filePreviewRevision.current; };
  }, [currentFile]);
  useEffect(() => {
    const listening = listenEvent<{ reason?: string; path?: string }>(projectionLibraryStateChangedEvent, event => {
      if (event.payload?.reason !== "embedded-preview" || !event.payload.path
        || windowsPathKey(event.payload.path) !== windowsPathKey(currentFile || "")) return;
      const revision = ++filePreviewRevision.current;
      void readProjectionPreviewImage(currentFile).then(image => {
        if (filePreviewRevision.current === revision) setFilePreview(image?.data_url || "");
      }).catch(reason => { if (filePreviewRevision.current === revision) setError({ key: "properties.previewReadFailed", params: {error: String(reason)} }); });
    }).catch(() => undefined);
    return () => { void listening.then(off => off?.()); };
  }, [currentFile]);
  const captureFilePreview = async () => {
    if (!currentFile || filePreviewWrite.current || savingRef.current || !viewerPrepared) return;
    const source = currentFile;
    filePreviewWrite.current = true; setCapturingPreview(true); setError("");
    try {
      const image = await renderPreviewImage(source, renderMode === "fast" ? "fast_experimental" : renderMode, undefined,
        renderMode === "fast" ? quickOperation?.previewId : fullPreview.operation?.operationId);
      if (sourceRef.current === source) setPreviewDraft({ source, image });
    } catch (reason) { if (sourceRef.current === source) setError(String(reason)); }
    finally { filePreviewWrite.current = false; setCapturingPreview(false); }
  };
  const updateFilePreview = async (clear: boolean) => {
    if (!currentFile || filePreviewWrite.current || savingRef.current) return;
    const source = currentFile;
    let revision = filePreviewRevision.current;
    filePreviewWrite.current = true; setFilePreviewBusy(true); setError("");
    try {
      const imagePath = clear ? null : await selectPreviewImageFile();
      if ((!clear && !imagePath) || filePreviewRevision.current !== revision) return;
      revision = ++filePreviewRevision.current;
      if (clear) await clearProjectionPreviewImage(source);
      else await importProjectionPreviewImage(source, imagePath!);
      const image = await readProjectionPreviewImage(source);
      if (filePreviewRevision.current === revision) {
        setFilePreview(image?.data_url || ""); setClearPreviewConfirm(false);
        setStatus({key:clear ? "properties.previewCleared" : "properties.previewImported"});
      }
      // Only the preview field changes; the user's unsaved form stays intact.
      await emitEvent(projectionLibraryStateChangedEvent, { reason: "embedded-preview", path: source });
    } catch (reason) { if (filePreviewRevision.current === revision) setError(String(reason)); }
    finally { filePreviewWrite.current = false; setFilePreviewBusy(false); }
  };
  const refreshAnalysis = async (filePath = currentFile, preferredAnalysis?: unknown) => {
    if (!filePath) return;
    const generation = ++analysisGeneration.current;
    const source = preferredAnalysis === undefined ? "source" : "application";
    console.info(`[NOVA_METADATA_REQUEST] source=${source} path=${filePath} request started`);
    try {
      const parsed = preferredAnalysis ?? await analyzeProjectionFile(filePath);
      if (generation !== analysisGeneration.current) return;
      setData(parsed);
      const nextForm = metadataToForm(parsed);
      const recovered = takeUpdateDraft<{ form: MetadataForm | null; initialForm: MetadataForm | null }>(`properties:${filePath}`);
      setInitialForm(recovered?.initialForm || nextForm);
      setForm(recovered?.form || nextForm);
      setUndoDrafts([]); setRedoDrafts([]);
      setError("");
      console.info(`[NOVA_METADATA_REQUEST] source=${source} path=${filePath} request succeeded`);
      void loadLibrary()
        .then((state) => addOrUpdateRecordFromAnalysis(
          state,
          filePath,
          projectionRecordAnalysis(parsed),
        ))
        .catch((error) => {
          console.warn(`[NOVA_METADATA_REQUEST] path=${filePath} library update failed: ${String(error)}`);
        });
    } catch (error) {
      if (generation === analysisGeneration.current) {
        console.error(`[NOVA_METADATA_REQUEST] source=${source} path=${filePath} request failed: ${String(error)}`);
      }
      throw error;
    }
  };

  useEffect(() => {
    if (!currentFile) {
      analysisGeneration.current += 1;
      setData(null);
      setInitialForm(null);
      setForm(null);
      return;
    }
    setStatus("");
    setError("");
    setForm(null);
    const file = currentFile;
    refreshAnalysis(file, applicationAnalysis || undefined).catch((err) => {
      if (sourceRef.current === file) setError(String(err));
    });
    return () => { analysisGeneration.current += 1; };
    // A render analysis arriving for this same file is not a metadata reload:
    // keep the user's unsaved edits. Explicit Save still refreshes from disk.
  }, [currentFile]);

  const fileName = currentFile ? currentFile.split(/[\\/]/).pop() : "";
  const metadata = data?.metadata || {};
  const regionNames: string[] | undefined = Array.isArray(data?.regions) && data.regions.every((region: any) => typeof region?.name === "string") ? data.regions.map((region: any) => region.name) : undefined;
  const derived = data?.derived || {};
  const density = derived.building?.density
    ?? ((data?.structure_stats?.total_non_air_blocks || 0) / Math.max(1, metadata.total_volume || 0));
  const regionError = useMemo(() => validateRegions(form?.regions || []), [form?.regions]);

  const updateForm = (patch: Partial<MetadataForm>) => {
    if (!form) return;
    setUndoDrafts(history => [...history.slice(-63), form]); setRedoDrafts([]);
    setForm({ ...form, ...patch });
  };

  const undoDraft = () => {
    if (!form || !undoDrafts.length) return;
    setRedoDrafts(history => [...history, form]); setForm(undoDrafts[undoDrafts.length - 1]); setUndoDrafts(history => history.slice(0, -1));
  };
  const redoDraft = () => {
    if (!form || !redoDrafts.length) return;
    setUndoDrafts(history => [...history, form]); setForm(redoDrafts[redoDrafts.length - 1]); setRedoDrafts(history => history.slice(0, -1));
  };
  const hasChanges = !!form && JSON.stringify(form) !== JSON.stringify(initialForm);
  useEffect(() => { onDirtyChange?.(hasChanges); }, [hasChanges, onDirtyChange]);
  const worldExportUnavailable = !form || hasChanges || saving || filePreviewBusy || capturingPreview;
  const openWorldExport = () => {
    if (worldExportUnavailable) return;
    setConvertOpen(false);
    setWorldExportOpen(true);
  };

  const savePatch = async (saveAs = false) => {
    if (savingRef.current || filePreviewWrite.current || !currentFile || !form) return;
    if (regionError) { setError(regionError); return; }
    const source = currentFile;
    const patch = buildPatch(form);
    savingRef.current = true;
    setSaving(true);
    setError("");
    setStatus("");
    try {
      const outputPath = saveAs
        ? await selectLitematicSavePath(source.replace(/\.litematic$/i, ".edited.litematic"))
        : undefined;
      if (saveAs && !outputPath) return;
      await saveProjectionMetadataPatch({ currentFile: source, patch, outputPath: outputPath || undefined });
      if (sourceRef.current !== source) return;
      const nextFile = outputPath || source;
      if (outputPath && outputPath !== source) {
        setCurrentFile(outputPath);
      } else {
        await refreshAnalysis(nextFile);
      }
      setStatus(outputPath ? { key: "properties.savedAs", params: {path: displayFilePath(outputPath)} } : {key:"properties.saved"});
      setConvertOpen(false);
    } catch (err) {
      if (sourceRef.current === source) setError(String(err));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const handleSave = () => savePatch();
  const handleSaveAs = () => savePatch(true);

  const handleRestore = () => {
    if (initialForm && !savingRef.current) {
      setForm(JSON.parse(JSON.stringify(initialForm)));
      setError("");
      setStatus({key:"properties.restored"});
    }
  };

  const handleSelectFile = async () => {
    const res = await selectLitematicFile();
    if (res) {
      setCurrentFile(res);
    }
  };

  const previewFailure = renderMode === "fast" ? preview.error || quickOperation?.error : fullPreview.error;
  const modeLabel = renderMode === "fast" ? t("properties.fast") : renderMode === "standard" ? t("properties.standard") : t("properties.full");
  const previewStatus = !currentFile ? t("properties.selectPrompt") : previewFailure ? t("properties.previewFailed")
    : previewPending || renderPending || (viewerPrepared && !previewVisible) ? (renderMode === "fast" ? t("editor.loading") : fullPreview.progressLabel)
    : previewVisible ? "" : t("properties.previewEmpty");

  return (
    <section className="properties-page" aria-busy={saving}>
      <PreviewDraftDialog draft={previewDraft?.image ?? null} sourcePath={previewDraft?.source ?? currentFile}
        onClose={() => setPreviewDraft(null)} onSaved={image => {
          const source = previewDraft?.source; setPreviewDraft(null);
          if (!source) return;
          if (sourceRef.current === source) { setFilePreview(image.data_url); setStatus({key:"properties.thumbnailSaved"}); }
          void emitEvent(projectionLibraryStateChangedEvent, { reason: "embedded-preview", path: source }).catch(reason => setError(String(reason)));
        }} />
      <header className="properties-header">
        <h2 className="properties-title">{t("properties.title")}</h2>
        <div className="properties-toolbar">
          <Button disabled={saving} onClick={handleSelectFile}><ThemeIcon name="import" size={16} fallback={FolderOpen} />{t("properties.select")}</Button>
          <Button variant="ghost" disabled={saving} onClick={() => setRoute("library")}><ThemeIcon name="gallery" size={16} fallback={Library} />{t("properties.selectLibrary")}</Button>
          {currentFile && <Button disabled={worldExportUnavailable} aria-describedby={hasChanges ? "world-export-save-first" : undefined} onClick={openWorldExport}><ThemeIcon name="download" size={16} fallback={Download} />{t("worldExport.title")}</Button>}
        </div>
      </header>
      {currentFile && <div className="properties-actions">
        <span className="muted">{saving ? t("editor.saving") : !form ? t("properties.readingShort") : hasChanges ? t("properties.unsaved") : t("properties.savedAll")}</span>
        <Button variant="ghost" onClick={undoDraft} disabled={!undoDrafts.length || saving}>{locale === "en-US" ? "Undo" : "撤销"}</Button>
        <Button variant="ghost" onClick={redoDraft} disabled={!redoDrafts.length || saving}>{locale === "en-US" ? "Redo" : "重做"}</Button>
        <Button variant="ghost" onClick={handleRestore} disabled={!form || !hasChanges || saving || filePreviewBusy || capturingPreview}><ThemeIcon name="undo" size={16} fallback={RotateCcw} />{t("properties.undoUnsaved")}</Button>
        <Button variant="ghost" onClick={() => setConvertOpen(true)} disabled={saving || filePreviewBusy || !form}>{t("properties.otherFormats")}</Button>
        <Button onClick={handleSaveAs} disabled={!form || !!regionError || saving || filePreviewBusy || capturingPreview}>{t("properties.saveCopy")}</Button>
        <Button variant="default" onClick={handleSave} disabled={!form || !hasChanges || !!regionError || saving || filePreviewBusy || capturingPreview}><ThemeIcon name="save" size={16} fallback={Save} />{saving ? t("editor.saveBusy") : t("properties.saveChanges")}</Button>
      </div>}
      {currentFile && hasChanges && <div className="properties-toolbar">
        <span className="muted" id="world-export-save-first">{t("worldExport.saveFirst")}</span>
        <Button size="sm" variant="link" onClick={handleSave} disabled={!!regionError || saving || filePreviewBusy || capturingPreview}>{saving ? t("editor.saveBusy") : t("properties.saveChanges")}</Button>
      </div>}
      <div className="properties-file muted" title={displayFilePath(currentFile || "")}>{fileName || t("properties.notSelected")}</div>
      {error && <div className="properties-message properties-error nova-text-danger" role="alert">{displayPathText(localizeMessage(error))}</div>}
      {status && <div className="properties-message" role="status">{displayPathText(localizeMessage(status))}</div>}
      {!currentFile ? <div className="properties-empty muted"><Box size={40} aria-hidden="true" /><span>{t("properties.empty")}</span></div> : (
        <div className="properties-workspace">
          <ScrollArea className="properties-editor-scroll" viewportClassName="properties-editor-viewport">
            {!form ? (!error && <div className="properties-empty muted" role="status">{t("properties.reading")}</div>) : (
            <fieldset className="properties-editor nova-box-plain" disabled={saving || filePreviewBusy || capturingPreview}>
                <legend className="properties-sr-only">{t("properties.title")}</legend>
                  <h3 className="properties-section-title">{locale === "en-US" ? "Basic information" : "基本信息"}</h3>
                  <TextRow label={t("properties.internalName")} value={form.name} onChange={(value) => updateForm({ name: value })} />
                  <TextRow label={t("properties.author")} value={form.author} onChange={(value) => updateForm({ author: value })} />
                  <label className="properties-field">
                    <span>{t("properties.description")}</span>
                    <Textarea rows={4} value={form.description} onChange={(event) => updateForm({ description: event.target.value })} />
                  </label>
                  <dl className="properties-facts nova-box-plain">
                    <div><dt>{t("properties.schematicName")}</dt><dd>{fileName}</dd></div>
                    <div><dt>{t("properties.minecraftVersion")}</dt><dd>{minecraftVersionLabel(form.minecraft_data_version)}</dd></div>
                    <div><dt>{t("properties.regionNames")}</dt><dd>{regionNames === undefined ? t("properties.notRead") : regionNames.length ? regionNames.map((name, index) => <div key={index}>{name || t("properties.emptyName")}</div>) : t("properties.noRegions")}</dd></div>
                    <div><dt>{t("properties.dimensions")}</dt><dd>{metadata.enclosing_size?.x || 0} × {metadata.enclosing_size?.y || 0} × {metadata.enclosing_size?.z || 0}</dd></div>
                    <div><dt>{t("properties.blocks")}</dt><dd>{Number(metadata.total_blocks || 0).toLocaleString(locale)}</dd></div>
                    <div><dt>{t("properties.volume")}</dt><dd>{Number(metadata.total_volume || 0).toLocaleString(locale)}</dd></div>
                    <div><dt>{t("properties.density")}</dt><dd>{(density * 100).toLocaleString(locale, {minimumFractionDigits:2, maximumFractionDigits:2})}%</dd></div>
                  </dl>
                  <h3 className="properties-section-title">{locale === "en-US" ? "Regions" : "选区"}</h3>
                  {regionError && <div className="properties-error nova-text-danger" role="alert">{localizeMessage(regionError)}</div>}
                  <ScrollArea className="properties-table-scroll" horizontal>
                    <Table>
                      <TableHeader><TableRow><TableHead>{t("properties.editRegion")}</TableHead><TableHead>{t("properties.regionDimensions")}</TableHead><TableHead>{t("properties.regionPosition")}</TableHead></TableRow></TableHeader>
                      <TableBody>{form.regions.map((region, index) => (
                        <TableRow key={region.originalName}>
                          <TableCell><Input aria-label={t("properties.regionAria", {index: index+1})} value={region.name} onChange={(event) => {
                            const regions = [...form.regions];
                            regions[index] = { ...region, name: event.target.value };
                            updateForm({ regions });
                          }} /></TableCell>
                          <TableCell>{region.size.x} / {region.size.y} / {region.size.z}</TableCell>
                          <TableCell>{region.position.x} / {region.position.y} / {region.position.z}</TableCell>
                        </TableRow>
                      ))}</TableBody>
                    </Table>
                  </ScrollArea>
                  <details className="properties-advanced"><summary>{locale === "en-US" ? "File, version and advanced fields" : "文件、版本与高级字段"}</summary>
                  <DateRow label={t("properties.created")} value={form.time_created} onChange={(value) => updateForm({ time_created: value })} />
                  <DateRow label={t("properties.modified")} value={form.time_modified} onChange={(value) => updateForm({ time_modified: value })} />
                  <dl className="properties-facts nova-box-plain"><div className="properties-format-row"><dt>{t("properties.format")}</dt><dd>Litematica v{form.litematic_version}.{form.litematic_subversion}<Button size="sm" variant="ghost" disabled={saving || hasChanges} onClick={() => setVersionConvertOpen(true)}>{t("properties.convertVersion")}</Button></dd></div></dl>
                  </details>
                <div className="properties-file-preview">
                  <div className="properties-preview-toolbar"><h3>{t("properties.filePreview")}</h3>
                    <Button size="sm" variant="ghost" onClick={() => void updateFilePreview(false)}><ThemeIcon name="picture_import" size={16} fallback={ImagePlus} />{t("properties.importImage")}</Button>
                    <Button size="sm" variant="ghost" disabled={!filePreview} onClick={() => setClearPreviewConfirm(true)}><ThemeIcon name="picture_error" size={16} fallback={ImageOff} />{t("editor.clear")}</Button>
                  </div>
                  <div className="properties-file-preview-canvas">{filePreview ? <img src={filePreview} alt={t("properties.embeddedPreview")} /> : <span>{t("properties.noPreview")}</span>}</div>
                </div>
              </fieldset>
            )}
          </ScrollArea>
          <section className="properties-preview" aria-label={t("properties.render")}>
            <header className="properties-preview-toolbar">
              <h3>{t("properties.modePreview", {mode:modeLabel})}</h3>
              {(["fast", "standard", "full"] as const).map((mode) => <Button key={mode} size="sm" variant={renderMode === mode ? "default" : "ghost"} aria-pressed={renderMode === mode}
                onClick={() => setRenderMode(mode)}>{mode === "fast" ? t("properties.fastMode") : mode === "standard" ? t("properties.standardMode") : t("properties.fullMode")}</Button>)}
              <label><Checkbox checked={preload} disabled={previewPending || renderPending} onCheckedChange={(checked) => setPreload(checked === true)} />{t("properties.preload")}</label>
              <Button size="sm" onClick={() => void handlePreview()} disabled={previewPending || (renderPending && !viewerPrepared)}>
                <ThemeIcon name={renderMode === "full" ? "build" : "run"} size={15} fallback={renderMode === "full" ? Box : Play} />{previewPending ? t("properties.connecting") : renderPending ? t("editor.loading") : viewerPrepared ? t("properties.showEmbedded") : quickOperation ? t("editor.reload") : t("properties.modeRender", {mode:modeLabel})}
              </Button>
              {(renderPending || (renderMode !== "fast" && previewPending)) && !viewerPrepared && <Button size="sm" variant="ghost" onClick={() => void handleCancelPreview()}>{t("editor.cancel")}</Button>}
              <Button size="sm" variant="ghost" disabled={!viewerPrepared || previewPending || saving || filePreviewBusy || capturingPreview} onClick={() => void captureFilePreview()}><ThemeIcon name="picture_trans" size={15} fallback={ImagePlus} />{capturingPreview ? t("editor.saving") : t("properties.thumbnail")}</Button>
              <Button size="sm" variant="ghost" onClick={() => setRoute("render")} ><ThemeIcon name="outlink" size={15} fallback={ExternalLink} />{t("properties.renderPage")}</Button>
            </header>
            <div ref={renderMode === "fast" ? preview.hostRef : fullPreview.hostRef} className="properties-preview-host" aria-label={t("properties.viewport", {mode:modeLabel})}>
              {!previewVisible && <div className="properties-preview-placeholder muted"><ThemeIcon name="empty" size={36} fallback={Box} /><span>{previewFailure ? t("properties.previewFailedShort") : previewStatus}</span></div>}
            </div>
            {renderMode !== "fast" && fullPreview.preflight && <div role="status" className="properties-preview-status muted">
              <p>{t("properties.vram", {requirement:fullPreview.preflight.conservativeVramBytes == null ? t("editor.unknown") : formatResourceBytes(fullPreview.preflight.conservativeVramBytes)})}</p>
              {fullPreview.preflight.severity === "warning" && <p>{t("properties.vramWarning")}</p>}
              {fullPreview.preflight.severity === "hard" && <p>{t("properties.vramHard")}</p>}
              {fullPreview.preflight.severity === "unmeasured" && <p role="alert">{fullPreview.preflight.invalidReason ? displayPathText(localizeMessage(fullPreview.preflight.invalidReason)) : t("properties.vramUnknown")}</p>}
              <div className="properties-preview-toolbar">
                <Button size="sm" disabled={previewPending || !fullPreview.preflight.allowedActions.includes("continue_prebuilt")} onClick={() => void fullPreview.choose("prebuilt")}>{t("properties.confirmPrebuild")}</Button>
                <Button size="sm" disabled={previewPending || (fullPreview.preflight.severity !== "normal" && !fullPreview.preflight.allowedActions.includes("switch_realtime"))} onClick={() => void fullPreview.choose("realtime")}>{t("properties.openDirectly")}</Button>
                {renderMode === "full" && <Button size="sm" variant="ghost" disabled={previewPending || !fullPreview.preflight.allowedActions.includes("use_basic_vulkan")} onClick={() => void fullPreview.choose("basic_vulkan")}>{t("properties.lowerQuality")}</Button>}
                <Button size="sm" variant="ghost" onClick={() => void fullPreview.close()}>{t("editor.cancel")}</Button>
              </div>
            </div>}
            {previewFailure && <div className="properties-preview-error nova-text-danger" role="alert">{displayPathText(localizeMessage(previewFailure))}</div>}
            <div className="properties-preview-status muted" role="status" aria-live="polite">{previewStatus}</div>
          </section>
        </div>
      )}
      {clearPreviewConfirm && <Dialog title={t("properties.clearPreviewTitle")} busy={filePreviewBusy} onClose={() => setClearPreviewConfirm(false)} footer={<><Button variant="ghost" disabled={filePreviewBusy} onClick={() => setClearPreviewConfirm(false)}>{t("editor.cancel")}</Button><Button variant="destructive" disabled={filePreviewBusy} onClick={() => void updateFilePreview(true)}>{t("properties.confirmClear")}</Button></>}><p>{t("properties.clearPreviewPrompt")}</p></Dialog>}

      {versionConvertOpen && <VersionConversionDialog sourcePath={currentFile} onClose={() => setVersionConvertOpen(false)} onSaved={(path) => { setStatus({ key: "properties.convertedSaved", params: {path: displayFilePath(path)} }); setVersionConvertOpen(false); }} />}
      {worldExportOpen && <WorldExportDialog sourcePath={currentFile} name={form?.name || ""} onClose={() => setWorldExportOpen(false)} />}
      {convertOpen && <FormatExportDialog sourcePath={currentFile} onClose={() => setConvertOpen(false)} onWorldExport={() => { setConvertOpen(false); openWorldExport(); }} />}
    </section>
  );
}

function TextRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return <label className="properties-field"><span>{label}</span><Input value={value} onChange={(event) => onChange(event.target.value)} /></label>;
}

function DateRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return <label className="properties-field"><span>{label}</span><Input type="datetime-local" value={value} onChange={(event) => onChange(event.target.value)} /></label>;
}

function NumberRow({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return <label className="properties-field"><span>{label}</span><Input type="number" value={value} onChange={(event) => onChange(Number(event.target.value))} /></label>;
}
