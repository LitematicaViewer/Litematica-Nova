import { t, getUiLocale, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { zhCN as editorZhCN } from "../../../../../src/services/locales/editor";
import { useI18n } from "../../../../i18n";
import { displayPathText } from "../../../../../src/business/facade";
import { displayFilePath } from "../../../../../src/business/facade";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Button } from "../../../../components/ui/button";
import { Textarea } from "../../../../components/ui/textarea";
import { Input } from "../../../../components/ui/input";
import { Checkbox } from "../../../../components/ui/checkbox";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { takeUpdateDraft, useUpdateDraft } from "../../updateRecovery";
import { BlockSearchSelect } from "../../../../components/BlockSearchSelect";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Dialog } from "../../../../components/Dialog";
import { Dropdown } from "../../../../components/Dropdown";
import {
  getAllBlocks,
  getBlockProperties,
  getDefaultProperties,
  loadDatabases,
  translateKey,
  translateValue,
  translateBlockId,
} from "../../../../../src/business/facade";
import {
  getDefaultProjectionOutputPath,
  openExternalPath,
  selectJsonSavePath,
  selectLitematicSavePath,
  writeWorkspaceFile,
} from "../../../../../src/business/facade";
import {
  addOrUpdateRecord,
  addOrUpdateRecordFromAnalysis,
  loadLibrary,
  projectionRecordAnalysis,
} from "../../../../../src/business/facade";
import {
  GenerationTemplate,
  exportTemplate,
  loadGenerationTemplates,
  templateToForm,
} from "../../../../../src/business/facade";
import {
  OperationFormState,
  OperationType,
  GenerateFormState,
  GenerateSummary,
  MaterialEntrySpec,
  MaterialSpec,
  applyGenerate,
  buildProjectionPlan,
  createDefaultOperation,
  dryRunGenerate,
  ensureLitematicExtension,
  formatGeneratePathLog,
  sanitizeFileName,
  validateOutputPath,
  validatePlanClientSide,
} from "../../../../../src/business/facade";
import {
  AiPlanMessage,
  aiGeneratePlan,
  applyAiPlanToGenerateForm,
  buildWrappedPrompt,
  buildWebPrompt,
  extractPlanFromAiText,
  getAiPlanWarnings,
  normalizeAiPlanForImport,
  sendAiChatMessage,
  validateAiPlanBasic,
} from "../../../../../src/business/facade";

import "./generate.css";

type Status = "idle" | "previewed" | "generating" | "generated" | "failed";

const operationLabels: Array<{ labelKey: string; value: OperationType }> = [
  { labelKey: "generate.fill_box", value: "fill_box" },
  { labelKey: "generate.hollow_box", value: "hollow_box" },
  { labelKey: "generate.floor", value: "floor" },
  { labelKey: "generate.wall", value: "wall" },
  { labelKey: "generate.pillar", value: "pillar" },
  { labelKey: "generate.cylinder", value: "cylinder" },
  { labelKey: "generate.sphere", value: "sphere" },
  { labelKey: "generate.outline_box", value: "outline_box" },
  { labelKey: "generate.checkerboard_floor", value: "checkerboard_floor" },
  { labelKey: "generate.roof_gable", value: "roof_gable" },
  { labelKey: "generate.ring", value: "ring" },
];

function templateText(template: GenerationTemplate, field: "name" | "description"): string {
  const raw = field === "name" ? template.name_zh : template.description_zh;
  const suppliedEnglish = field === "name" ? template.name_en : template.description_en;
  if (getUiLocale() === "en-US" && suppliedEnglish && !template.id.startsWith("custom-") && !template.tags.includes("custom")) return suppliedEnglish;
  const key = `generate.template.${template.id}.${field}`;
  return editorZhCN[key] === raw ? t(key) : raw;
}
function templateTagText(tag: string): string {
  const key = Object.keys(editorZhCN).find(key => key.startsWith("generate.templateTag.") && editorZhCN[key] === tag);
  return key ? t(key) : tag;
}

function makeId(): string {
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function cloneBlock(block: { name: string; properties?: Record<string, string> }) {
  return { name: block.name, properties: { ...(block.properties || {}) } };
}

function cloneMaterial(material: MaterialSpec | undefined): MaterialSpec | undefined {
  if (!material) return undefined;
  if (material.type === "single") return { type: "single", block: cloneBlock(material.block) };
  return {
    type: "weighted_random",
    seed: material.seed,
    entries: material.entries.map((entry) => ({
      id: makeId(),
      weight: entry.weight,
      block: cloneBlock(entry.block),
    })),
  };
}

function defaultFormState(): GenerateFormState {
  return {
    metadata: {
      name: t("generate.defaultName"),
      author: "Litematica-Nova",
      description: t("generate.defaultDescription"),
      minecraftDataVersion: 3953,
    },
    region: {
      name: "main",
      origin: [0, 0, 0],
      size: [32, 16, 32],
    },
    operations: [
      { ...createDefaultOperation("floor"), from: [0, 0, 0], to: [31, 0, 31] },
      createDefaultOperation("hollow_box"),
    ],
  };
}

export function GeneratePage({ currentFile, setCurrentFile, setRoute }: any) {
  const { locale } = useI18n();
  const [form, setForm] = useState<GenerateFormState>(() => defaultFormState());
  const [status, setStatus] = useState<Status>("idle");
  const [clientErrors, setClientErrors] = useState<string[]>([]);
  const [dryRunSummary, setDryRunSummary] = useState<GenerateSummary | null>(null);
  const [rawOutput, setRawOutput] = useState("");
  const [lastError, setLastError] = useState<UiMessage>("");
  const [outputPath, setOutputPath] = useState("");
  const [generatedPath, setGeneratedPath] = useState("");
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [rawOpen, setRawOpen] = useState(false);
  const [aiPrompt, setAiPrompt] = useState("");
  const [aiPlanText, setAiPlanText] = useState("");
  const [aiImportedPlan, setAiImportedPlan] = useState<any | null>(null);
  const [aiPlanErrors, setAiPlanErrors] = useState<UiMessage[]>([]);
  const [aiPlanWarnings, setAiPlanWarnings] = useState<string[]>([]);
  const [aiWebPromptCopied, setAiWebPromptCopied] = useState(false);
  const [aiChatOpen, setAiChatOpen] = useState(false);
  const [aiDirectMode, setAiDirectMode] = useState(false);
  const [aiChatMessages, setAiChatMessages] = useState<AiPlanMessage[]>([]);
  const [aiChatInput, setAiChatInput] = useState("");
  const [aiChatSending, setAiChatSending] = useState(false);
  const [aiChatError, setAiChatError] = useState<UiMessage>("");
  const [aiChatPlan, setAiChatPlan] = useState<any | null>(null);
  const [allBlocks, setAllBlocks] = useState<string[]>(() => getAllBlocks().sort());
  const [templates, setTemplates] = useState<GenerationTemplate[]>([]);
  const [templatesLoading, setTemplatesLoading] = useState(true);
  const [templatesError, setTemplatesError] = useState("");
  const [templateLoadAttempt, setTemplateLoadAttempt] = useState(0);
  const [templateQuery, setTemplateQuery] = useState("");
  const [templateTag, setTemplateTag] = useState("");
  const [formDirty, setFormDirty] = useState(false);
  const [busy, setBusy] = useState<"preview" | "apply" | "choose" | null>(null);
  const busyRef = useRef(false);
  const previewPlan = useRef<string | null>(null);
  const updateDraft = { form, formDirty, outputPath, aiPrompt, aiPlanText, aiImportedPlan, aiChatMessages, aiChatInput, aiChatPlan };
  useUpdateDraft("generate", updateDraft);
  useEffect(() => {
    const saved = takeUpdateDraft<typeof updateDraft>("generate");
    if (!saved) return;
    setForm(saved.form); setFormDirty(saved.formDirty); setOutputPath(saved.outputPath);
    setAiPrompt(saved.aiPrompt); setAiPlanText(saved.aiPlanText); setAiImportedPlan(saved.aiImportedPlan);
    setAiChatMessages(saved.aiChatMessages); setAiChatInput(saved.aiChatInput); setAiChatPlan(saved.aiChatPlan);
    // Preview authorization is intentionally not restored; a new dry-run is required.
  }, []);

  const plan = useMemo(() => buildProjectionPlan(form), [form]);
  const planJson = useMemo(() => JSON.stringify(plan, null, 2), [plan]);
  const latestPlan = useRef(planJson);
  latestPlan.current = planJson;
  const currentValidation = useMemo(() => validatePlanClientSide(plan), [plan]);

  useEffect(() => {
    let active = true;
    loadDatabases().then(() => {
      if (active) setAllBlocks(getAllBlocks().sort());
    });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    let active = true;
    setTemplatesLoading(true);
    setTemplatesError("");
    loadGenerationTemplates()
      .then((items) => {
        if (active) setTemplates(items);
      })
      .catch((error) => {
        if (active) setTemplatesError(String(error));
      })
      .finally(() => { if (active) setTemplatesLoading(false); });
    return () => { active = false; };
  }, [templateLoadAttempt]);

  const templateTags = useMemo(() => Array.from(new Set(templates.flatMap((template) => template.tags || []))).sort((a, b) => a.localeCompare(b, getUiLocale())), [templates]);
  const filteredTemplates = useMemo(() => {
    const needle = templateQuery.trim().toLowerCase();
    return templates.filter((template) => {
      const tagOk = !templateTag || template.tags.includes(templateTag);
      const haystack = `${template.id} ${templateText(template,"name")} ${templateText(template,"description")} ${(template.tags || []).join(" ")}`.toLowerCase();
      return tagOk && (!needle || haystack.includes(needle));
    });
  }, [templates, templateQuery, templateTag, locale]);

  const clearDryRun = () => {
    previewPlan.current = null;
    setDryRunSummary(null);
    setRawOutput("");
    setLastError("");
    if (status !== "idle") setStatus("idle");
  };

  const updateMetadata = (key: keyof GenerateFormState["metadata"], value: string | number) => {
    setForm((prev) => ({ ...prev, metadata: { ...prev.metadata, [key]: value } }));
    setFormDirty(true);
    clearDryRun();
  };

  const updateRegion = (key: keyof GenerateFormState["region"], value: string | [number, number, number]) => {
    setForm((prev) => ({ ...prev, region: { ...prev.region, [key]: value } }));
    setFormDirty(true);
    clearDryRun();
  };

  const updateOperation = (index: number, operation: OperationFormState) => {
    setForm((prev) => {
      const operations = [...prev.operations];
      operations[index] = operation;
      return { ...prev, operations };
    });
    setFormDirty(true);
    clearDryRun();
  };

  const handleLoadTemplate = (template: GenerationTemplate) => {
    if (formDirty && !window.confirm(t("generate.confirmTemplate"))) return;
    setForm(templateToForm(template));
    setFormDirty(false);
    setGeneratedPath("");
    clearDryRun();
  };

  const handleExportTemplate = async () => {
    const selected = await selectJsonSavePath(`${sanitizeFileName(form.metadata.name || "custom_template")}.json`);
    if (selected) await exportTemplate(selected, form, plan);
  };

  const handleDryRun = async () => {
    if (busyRef.current) return;
    const errors = validatePlanClientSide(plan);
    setClientErrors(errors);
    setLastError("");
    if (errors.length) { setStatus("failed"); return; }
    const requestedPlan = planJson;
    busyRef.current = true;
    setBusy("preview");
    previewPlan.current = null;
    setDryRunSummary(null);
    try {
      const result = await dryRunGenerate(plan);
      const defaultPath = outputPath || await getDefaultProjectionOutputPath(currentFile, form.metadata.name);
      if (latestPlan.current !== requestedPlan) return;
      previewPlan.current = requestedPlan;
      setDryRunSummary(result.summary);
      setRawOutput(result.diagnostics);
      setOutputPath(defaultPath);
      setStatus("previewed");
    } catch (error) {
      setLastError(String(error));
      setStatus("failed");
    } finally { busyRef.current = false; setBusy(null); }
  };

  const chooseOutput = async () => {
    if (busyRef.current) return;
    busyRef.current = true; setBusy("choose");
    try {
      const defaultPath = outputPath || await getDefaultProjectionOutputPath(currentFile, form.metadata.name);
      const selected = await selectLitematicSavePath(defaultPath);
      if (selected) { setOutputPath(ensureLitematicExtension(selected)); setLastError(""); }
    } catch (error) { setLastError(String(error)); }
    finally { busyRef.current = false; setBusy(null); }
  };

  const handleApply = async () => {
    if (busyRef.current) return;
    const errors = validatePlanClientSide(plan);
    setClientErrors(errors);
    setLastError("");
    if (errors.length) { setStatus("failed"); return; }
    if (!dryRunSummary || previewPlan.current !== planJson) {
      setLastError({key:"generate.planChanged"});
      return;
    }
    if (!outputPath) { await chooseOutput(); return; }
    busyRef.current = true; setBusy("apply");
    try {
      const outputValidation = await validateOutputPath(outputPath);
      const preflightLog = formatGeneratePathLog({
        selected_output_raw: outputPath,
        normalized_output: outputValidation.normalizedPath,
        parent_dir: outputValidation.parentDir,
        parent_exists: outputValidation.parentExists,
        output_has_litematic_ext: outputValidation.outputHasLitematicExt,
        output_is_absolute: outputValidation.outputIsAbsolute,
      });
      console.log(preflightLog);
      setRawOutput(preflightLog);
      if (!outputValidation.ok) { setLastError(outputValidation.error || {key:"generate.choosePathAgain"}); return; }
      setOutputPath(outputValidation.normalizedPath);
      setStatus("generating");
      const result = await applyGenerate(plan, outputValidation.normalizedPath);
      setDryRunSummary(result.summary);
      setRawOutput(result.diagnostics);
      const finalPath = result.normalizedOutputPath || outputValidation.normalizedPath;
      const library = await loadLibrary();
      if (result.analysis) {
        await addOrUpdateRecordFromAnalysis(library, finalPath, projectionRecordAnalysis(result.analysis));
      } else {
        await addOrUpdateRecord(library, finalPath);
      }
      setCurrentFile(finalPath);
      setGeneratedPath(finalPath);
      setStatus("generated");
    } catch (error) {
      setLastError(String(error));
      setStatus("failed");
    } finally { busyRef.current = false; setBusy(null); }
  };

  const savePlan = async () => {
    const selected = await selectJsonSavePath("projection_plan.json");
    if (selected) await writeWorkspaceFile(selected, planJson);
  };

  const openFolder = async () => {
    if (!generatedPath) return;
    const index = Math.max(generatedPath.lastIndexOf("\\"), generatedPath.lastIndexOf("/"));
    if (index > 0) await openExternalPath(generatedPath.slice(0, index));
  };

  const previewImportedPlan = (text: string) => {
    const importedPlan = extractPlanFromAiText(text);
    const normalized = normalizeAiPlanForImport(importedPlan);
    const errors = [...validateAiPlanBasic(normalized.plan), ...normalized.errors];
    const warnings = [...normalized.warnings, ...getAiPlanWarnings(normalized.plan)];
    setAiImportedPlan(errors.length ? null : normalized.plan);
    setAiPlanErrors(errors);
    setAiPlanWarnings(warnings);
    setAiPlanText(JSON.stringify(normalized.plan, null, 2));
    return { importedPlan: normalized.plan, errors, warnings };
  };

  const applyMockPlan = async () => {
    const generated = await aiGeneratePlan({
      provider: "mock",
      prompt: aiPrompt,
      currentPlan: plan,
      context: {
        defaultSize: form.region.size,
        minecraftDataVersion: form.metadata.minecraftDataVersion,
      },
    });
    const text = JSON.stringify(generated, null, 2);
    setAiPlanText(text);
    previewImportedPlan(text);
  };

  const copyWebPrompt = async () => {
    const fullPrompt = await buildWebPrompt(aiPrompt, plan);
    await navigator.clipboard.writeText(fullPrompt);
    setAiWebPromptCopied(true);
    window.setTimeout(() => setAiWebPromptCopied(false), 1600);
  };

  const importAiPlan = () => {
    try {
      previewImportedPlan(aiPlanText);
    } catch (error: any) {
      setAiImportedPlan(null);
      setAiPlanWarnings([]);
      setAiPlanErrors([{key:"generate.invalidJson", params:{error:String(error.message || error)}}]);
    }
  };

  const applyImportedPlan = (importedPlan: any) => {
    if (busyRef.current) return;
    const normalized = normalizeAiPlanForImport(importedPlan);
    const errors = [...validateAiPlanBasic(normalized.plan), ...normalized.errors];
    setAiPlanErrors(errors);
    setAiPlanWarnings([...normalized.warnings, ...getAiPlanWarnings(normalized.plan)]);
    if (errors.length) return;
    if (formDirty && !window.confirm(t("generate.confirmAi"))) return;
    setForm(applyAiPlanToGenerateForm(normalized.plan));
    setAiImportedPlan(normalized.plan);
    setAiPlanText(JSON.stringify(normalized.plan, null, 2));
    setFormDirty(true);
    setGeneratedPath("");
    clearDryRun();
  };

  const handleAiChatSend = async () => {
    const userText = aiChatInput.trim();
    if (!userText || aiChatSending) return;
    setAiChatSending(true);
    setAiChatError("");
    try {
      const content = aiDirectMode ? userText : await buildWrappedPrompt(userText, plan);
      const userMessage: AiPlanMessage = {
        role: "user",
        content,
        timestamp: Date.now(),
        planSnapshot: aiDirectMode ? undefined : plan,
      };
      const nextMessages = [...aiChatMessages, userMessage];
      setAiChatMessages(nextMessages);
      setAiChatInput("");

      const assistantMessage = await sendAiChatMessage({ messages: nextMessages });
      const finalMessages = [...nextMessages, assistantMessage];
      setAiChatMessages(finalMessages);

      try {
        const extracted = extractPlanFromAiText(assistantMessage.content);
        const normalized = normalizeAiPlanForImport(extracted);
        const errors = [...validateAiPlanBasic(normalized.plan), ...normalized.errors];
        const warnings = [...normalized.warnings, ...getAiPlanWarnings(normalized.plan)];
        setAiPlanErrors(errors);
        setAiPlanWarnings(warnings);
        setAiChatPlan(errors.length ? null : normalized.plan);
        setAiImportedPlan(errors.length ? null : normalized.plan);
        setAiPlanText(JSON.stringify(normalized.plan, null, 2));
        if (!errors.length) {
          applyImportedPlan(normalized.plan);
        }
      } catch (parseError: any) {
        setAiChatPlan(null);
        setAiImportedPlan(null);
        setAiPlanErrors([{key:"generate.invalidAiPlan", params:{error:String(parseError.message || parseError)}}]);
        setAiPlanWarnings([]);
      }
    } catch (error: any) {
      setAiChatError(String(error?.message || error));
    } finally {
      setAiChatSending(false);
    }
  };

  const clearAiChat = () => {
    setAiChatMessages([]);
    setAiChatInput("");
    setAiChatError("");
    setAiChatPlan(null);
  };

  return (
    <div className="generate-page" aria-busy={!!busy}>
      <div className="generate-header">
        <div className="generate-heading">
          <h2 className="generate-title">{t("generate.title")}</h2>
          <p className="generate-subtitle">{t("generate.subtitle")}</p>
        </div>
        <div className="form-field generate-status">
          <span>{t("generate.currentStatus")}</span>
          <strong role="status">{busy === "preview" ? t("generate.checking") : t(`generate.status.${status}`)}</strong>
        </div>
      </div>

      <div className="generate-primary-toolbar">
        <Button variant="secondary" onClick={handleDryRun} disabled={!!busy}>{busy === "preview" ? t("generate.previewing") : t("generate.preview")}</Button>
        <Button variant="ghost" onClick={chooseOutput} disabled={!!busy}>{t("editor.changeSavePath")}</Button>
        <Button variant="default" onClick={handleApply} disabled={!dryRunSummary || !!busy}>{busy === "apply" ? t("generate.generating") : t("generate.generateFile")}</Button>
      </div>

      <ScrollArea className="generate-scroll-area" aria-label={t("generate.editArea")}>
      <fieldset className="generate-workspace" disabled={!!busy}>
        <div className="generate-form-column">
          <TemplateLibrary
            loading={templatesLoading}
            loadError={templatesError}
            onRetry={() => setTemplateLoadAttempt((attempt) => attempt + 1)}
            templates={filteredTemplates}
            allTags={templateTags}
            query={templateQuery}
            selectedTag={templateTag}
            onQueryChange={setTemplateQuery}
            onTagChange={setTemplateTag}
            onLoad={handleLoadTemplate}
            onExportCurrent={handleExportTemplate}
          />
          <MetadataGroup form={form} updateMetadata={updateMetadata} />
          <details className="generate-advanced">
            <summary>{t("generate.advancedRegion")}</summary>
            <RegionGroup form={form} updateRegion={updateRegion} />
          </details>

          <div className="group-box">
            <div className="group-box-title">{t("generate.operations")}</div>
            <div className="generate-operation-toolbar">
              <Button variant="secondary" onClick={() => { setForm((prev) => ({ ...prev, operations: [...prev.operations, createDefaultOperation("fill_box")] })); setFormDirty(true); clearDryRun(); }}>{t("generate.addOperation")}</Button>
              <Button variant="secondary" onClick={() => { setForm((prev) => ({ ...prev, operations: [] })); setFormDirty(true); clearDryRun(); }}>{t("editor.clear")}</Button>
              <div className="generate-spacer" />
              <span className="generate-muted">{t("generate.overlapHint")}</span>
            </div>
            {form.operations.map((operation, index) => (
              <OperationEditor
                key={operation.id}
                operation={operation}
                index={index}
                allBlocks={allBlocks}
                onChange={(next) => updateOperation(index, next)}
                onRemove={() => {
                  setForm((prev) => ({ ...prev, operations: prev.operations.filter((_, i) => i !== index) }));
                  setFormDirty(true);
                  clearDryRun();
                }}
                onDuplicate={() => {
                  setForm((prev) => {
                    const copy = {
                      ...operation,
                      id: makeId(),
                      block: cloneBlock(operation.block),
                      material: cloneMaterial(operation.material),
                    };
                    const operations = [...prev.operations];
                    operations.splice(index + 1, 0, copy);
                    return { ...prev, operations };
                  });
                  setFormDirty(true);
                  clearDryRun();
                }}
              />
            ))}
          </div>

          <details className="group-box generate-advanced">
            <summary>{t("generate.optionalAi")}</summary>
            <div className="form-row">
              <div className="form-label">{t("generate.naturalLanguage")}</div>
              <Textarea className="input generate-ai-prompt" value={aiPrompt} onChange={(e) => setAiPrompt(e.target.value)} placeholder={t("generate.promptHint")} />
            </div>
            <div className="generate-action-row">
              <Button variant="secondary" onClick={copyWebPrompt}>{t("generate.copyWebPrompt")}</Button>
              <Button variant="secondary" onClick={applyMockPlan}>{t("generate.mock")}</Button>
              <Button variant="secondary" onClick={() => setAiChatOpen((open) => !open)}>{t("generate.apiChat")}</Button>
              {aiWebPromptCopied && <span className="generate-muted">{t("generate.copied")}</span>}
            </div>
            <div className="form-row">
              <div className="form-label">{t("generate.importAi")}</div>
              <Textarea
                className="input generate-ai-plan-input"
                value={aiPlanText}
                onChange={(e) => {
                  setAiPlanText(e.target.value);
                  setAiImportedPlan(null);
                  setAiPlanErrors([]);
                  setAiPlanWarnings([]);
                }}
                placeholder={t("generate.aiJsonHint")}
              />
            </div>
            <div className="generate-action-row">
              <Button variant="secondary" onClick={importAiPlan}>{t("generate.parsePreview")}</Button>
              <Button variant="secondary" onClick={() => aiImportedPlan && applyImportedPlan(aiImportedPlan)} disabled={!aiImportedPlan}>{t("generate.applyPlan")}</Button>
            </div>
            {aiPlanErrors.length > 0 && <ErrorList errors={aiPlanErrors} />}
            {aiPlanWarnings.length > 0 && (
              <div className="generate-ai-warnings">
                {aiPlanWarnings.map((warning,index) => <div key={index}>{t("generate.warning", {message:localizeMessage(warning)})}</div>)}
              </div>
            )}
            {aiImportedPlan && (
              <details open className="generate-ai-plan-preview">
                <summary className="generate-summary-toggle">{t("generate.aiPreview")}</summary>
                <pre className="generate-json-preview generate-ai-plan-json">{JSON.stringify(aiImportedPlan, null, 2)}</pre>
              </details>
            )}
            <div className="generate-help">
              {t("generate.aiHelp")}
            </div>
          </details>
        </div>

        <div className="generate-output-column">
          <div className="group-box">
            <div className="group-box-title">{t("generate.generate")}</div>
            {currentValidation.length > 0 && <ErrorList errors={currentValidation} />}
            {clientErrors.length > 0 && <ErrorList errors={clientErrors} />}

            <div className="form-row">
              <div className="form-label">{t("generate.savePath")}</div>
              <Input className="input generate-grow" value={displayFilePath(outputPath)} readOnly aria-label={t("generate.savePath")} placeholder={t("generate.autoSavePath")} />
            </div>
            {lastError && <Textarea readOnly className="input generate-output-error" value={localizeMessage(lastError)} />}
            {dryRunSummary && <SummaryView summary={dryRunSummary} />}
            {rawOutput && (
              <div className="generate-log-section">
                <Button variant="secondary" onClick={() => setRawOpen(!rawOpen)}>{rawOpen ? t("generate.hideLog") : t("generate.showLog")}</Button>
                {rawOpen && <Textarea readOnly className="input generate-output-log" value={rawOutput} />}
              </div>
            )}
            {generatedPath && (
              <div className="generate-result-actions">
                <Button variant="secondary" onClick={() => setRoute("properties")}>{t("generate.openProperties")}</Button>
                <Button variant="secondary" onClick={() => setRoute("library")}>{t("generate.openLibrary")}</Button>
                <Button variant="secondary" onClick={() => { setCurrentFile(generatedPath); setRoute("render"); }}>{t("generate.goRender")}</Button>
                <Button variant="secondary" onClick={openFolder}>{t("generate.openFolder")}</Button>
              </div>
            )}
          </div>
          <div className="group-box generate-plan-panel">
            <div className="group-box-title">{t("generate.advancedJson")}</div>
            <div className="generate-action-row">
              <Button variant="secondary" onClick={() => setDetailsOpen(!detailsOpen)}>{detailsOpen ? t("generate.collapse") : t("generate.expand")}</Button>
              <Button variant="secondary" onClick={() => navigator.clipboard.writeText(planJson)}>{t("generate.copyJson")}</Button>
              <Button variant="secondary" onClick={savePlan}>{t("generate.exportJson")}</Button>
            </div>
            {detailsOpen && <pre className="generate-json-preview">{planJson}</pre>}
          </div>

        </div>
      </fieldset>
      </ScrollArea>
      {aiChatOpen && (
        <Dialog title={t("generate.apiTitle")} width="lg" className="generate-ai-dialog" busy={aiChatSending || !!busy} onClose={() => setAiChatOpen(false)}>
            <div className="generate-chat-actions">
              <Button variant="secondary" onClick={() => setAiDirectMode((value) => !value)}>
                {t("generate.changeLabel")}{aiDirectMode ? t("generate.directOn") : t("generate.directOff")}
              </Button>
              <Button variant="ghost" onClick={clearAiChat}>{t("generate.clearChat")}</Button>
            </div>
            <div className="generate-chat-history">
              {aiChatMessages.length === 0 ? (
                <div className="generate-muted">{t("generate.noMessages")}</div>
              ) : (
                aiChatMessages.map((message, index) => (
                  <div key={`${message.role}-${message.timestamp || index}`} className="generate-chat-message">
                    <div className="generate-chat-role">{message.role === "assistant" ? "AI" : message.role === "system" ? "System" : "User"}</div>
                    <pre className="generate-chat-content">
                      {message.content.length > 5000 ? `${message.content.slice(0, 5000)}\n...` : message.content}
                    </pre>
                  </div>
                ))
              )}
            </div>
            <Textarea
              className="input generate-chat-input"
              value={aiChatInput}
              onChange={(event) => setAiChatInput(event.target.value)}
              placeholder={aiDirectMode ? t("generate.directHint") : t("generate.wrappedHint")}
            />
            <div className="generate-chat-actions">
              <Button variant="default" onClick={handleAiChatSend} disabled={aiChatSending || !aiChatInput.trim()}>
                {aiChatSending ? t("generate.sending") : t("generate.send")}
              </Button>
              <Button variant="secondary" onClick={() => aiChatPlan && applyImportedPlan(aiChatPlan)} disabled={!aiChatPlan}>{t("generate.applyAi")}</Button>
              <span className="generate-muted">{aiDirectMode ? t("generate.directStatus") : t("generate.wrappedStatus")}</span>
            </div>
            {aiChatError && <div className="generate-chat-error" role="alert">{localizeMessage(aiChatError)}</div>}
        </Dialog>
      )}
    </div>
  );
}

function TemplateLibrary({
  loading,
  loadError,
  onRetry,
  templates,
  allTags,
  query,
  selectedTag,
  onQueryChange,
  onTagChange,
  onLoad,
  onExportCurrent,
}: {
  loading: boolean;
  loadError: string;
  onRetry: () => void;
  templates: GenerationTemplate[];
  allTags: string[];
  query: string;
  selectedTag: string;
  onQueryChange: (value: string) => void;
  onTagChange: (value: string) => void;
  onLoad: (template: GenerationTemplate) => void;
  onExportCurrent: () => void;
}) {
  const { locale } = useI18n();
  return (
    <div className="group-box">
      <div className="group-box-title">{t("generate.templates")}</div>
      <div className="generate-editor-toolbar">
        <Input className="input generate-grow" value={query} onChange={(event) => onQueryChange(event.target.value)} aria-label={t("generate.searchTemplates")} placeholder={t("generate.templateSearchHint")} />
        <Dropdown value={selectedTag} onChange={onTagChange} ariaLabel={t("generate.templateTag")}
          options={[{ value: "", label: t("generate.allTags") }, ...allTags.map((tag) => ({ value: tag, label: templateTagText(tag) }))]} />
        <Button variant="secondary" onClick={onExportCurrent}>{t("generate.saveTemplate")}</Button>
      </div>
      <div className="template-library-list" aria-busy={loading}>
        {loading && <p className="muted" role="status">{t("generate.loadingTemplates")}</p>}
        {loadError && <div role="alert">
          <p>{t("generate.templatesFailed")}</p>
          <Button variant="secondary" onClick={onRetry} disabled={loading}>{t("generate.reloadTemplates")}</Button>
          <details><summary>{t("generate.viewError")}</summary><p>{localizeMessage(loadError)}</p></details>
        </div>}
        {templates.map((template) => (
          <div key={template.id} className="template-library-row">
            <div className="generate-template-info">
              <div className="generate-template-name">{templateText(template,"name")}</div>
              <div className="generate-template-description">{templateText(template,"description")}</div>
              <div className="generate-template-tags">{template.tags.map(templateTagText).join(" / ")}</div>
            </div>
            <Button variant="secondary" onClick={() => onLoad(template)}>{t("generate.useTemplate")}</Button>
          </div>
        ))}
        {!loading && !loadError && templates.length === 0 && <div className="generate-template-empty">{t("generate.noTemplates")}</div>}
      </div>
    </div>
  );
}

function OperationEditor({ operation, index, allBlocks, onChange, onRemove, onDuplicate }: {
  operation: OperationFormState;
  index: number;
  allBlocks: string[];
  onChange: (operation: OperationFormState) => void;
  onRemove: () => void;
  onDuplicate: () => void;
}) {
  const { locale } = useI18n();
  const update = (patch: Partial<OperationFormState>) => onChange({ ...operation, ...patch });
  return (
    <div className="generate-operation">
      <div className="generate-editor-toolbar">
        <strong>#{index + 1}</strong>
        <Dropdown value={operation.type} options={operationLabels.map(({labelKey,value}) => ({label:t(labelKey),value}))} onChange={(value) => update({ type: value as OperationType })} />
        <div className="generate-spacer" />
        <Button variant="secondary" onClick={onDuplicate}>{t("editor.copy")}</Button>
        <Button variant="ghost" onClick={onRemove}>{t("editor.delete")}</Button>
      </div>

      {["fill_box", "hollow_box", "floor", "wall", "outline_box", "checkerboard_floor", "roof_gable"].includes(operation.type) && (
        <>
          <Vec3Row label={t("generate.start")} value={operation.from} onChange={(value) => update({ from: value })} />
          <Vec3Row label={t("generate.end")} value={operation.to} onChange={(value) => update({ to: value })} />
        </>
      )}
      {["hollow_box", "wall"].includes(operation.type) && <NumberRow label={t("generate.thickness")} value={operation.thickness} min={1} onChange={(value) => update({ thickness: value })} />}
      {operation.type === "pillar" && (
        <>
          <Vec3Row label={t("generate.base")} value={operation.base} onChange={(value) => update({ base: value })} />
          <NumberRow label={t("generate.height")} value={operation.height} min={1} onChange={(value) => update({ height: value })} />
        </>
      )}
      {operation.type === "cylinder" && (
        <>
          <Vec3Row label={t("generate.center")} value={operation.center} onChange={(value) => update({ center: value })} />
          <NumberRow label={t("generate.radius")} value={operation.radius} min={0} onChange={(value) => update({ radius: value })} />
          <NumberRow label={t("generate.height")} value={operation.height} min={1} onChange={(value) => update({ height: value })} />
          <CheckRow label={t("generate.filled")} value={operation.filled} onChange={(value) => update({ filled: value })} />
        </>
      )}
      {operation.type === "sphere" && (
        <>
          <Vec3Row label={t("generate.center")} value={operation.center} onChange={(value) => update({ center: value })} />
          <NumberRow label={t("generate.radius")} value={operation.radius} min={0} onChange={(value) => update({ radius: value })} />
          <CheckRow label={t("generate.filled")} value={operation.filled} onChange={(value) => update({ filled: value })} />
        </>
      )}
      {operation.type === "roof_gable" && (
        <>
          <div className="form-row">
            <div className="form-label">{t("generate.direction")}</div>
            <Dropdown value={operation.axis} options={[{ label: "x", value: "x" }, { label: "z", value: "z" }]} onChange={(value) => update({ axis: value as "x" | "z" })} />
          </div>
          <NumberRow label={t("generate.overhang")} value={operation.overhang} min={0} onChange={(value) => update({ overhang: value })} />
        </>
      )}
      {operation.type === "ring" && (
        <>
          <Vec3Row label={t("generate.center")} value={operation.center} onChange={(value) => update({ center: value })} />
          <NumberRow label={t("generate.radius")} value={operation.radius} min={0} onChange={(value) => update({ radius: value })} />
          <NumberRow label={t("generate.height")} value={operation.height} min={1} onChange={(value) => update({ height: value })} />
          <NumberRow label={t("generate.thickness")} value={operation.thickness} min={1} onChange={(value) => update({ thickness: value })} />
        </>
      )}
      {operation.type === "checkerboard_floor" ? (
        <CheckerboardMaterialEditor
          materialA={operation.materialA}
          materialB={operation.materialB}
          onChangeA={(materialA) => update({ materialA })}
          onChangeB={(materialB) => update({ materialB })}
        />
      ) : (
        <MaterialEditor
          block={operation.block}
          material={operation.material}
          allBlocks={allBlocks}
          onBlockChange={(block) => update({ block, material: undefined })}
          onMaterialChange={(material) => update({ material })}
        />
      )}
    </div>
  );
}

function MetadataGroup({ form, updateMetadata }: { form: GenerateFormState; updateMetadata: (key: keyof GenerateFormState["metadata"], value: string | number) => void }) {
  const { locale } = useI18n();
  return (
    <div className="group-box">
      <div className="group-box-title">{t("generate.metadata")}</div>
      <TextRow label={t("properties.schematicName")} value={form.metadata.name} onChange={(value) => updateMetadata("name", value)} />
      <TextRow label={t("properties.author")} value={form.metadata.author} onChange={(value) => updateMetadata("author", value)} />
      <TextRow label={t("properties.description")} value={form.metadata.description} onChange={(value) => updateMetadata("description", value)} />
      <details className="generate-advanced"><summary>{t("generate.advancedVersion")}</summary><NumberRow label={t("generate.dataVersion")} value={form.metadata.minecraftDataVersion} onChange={(value) => updateMetadata("minecraftDataVersion", value)} /></details>
    </div>
  );
}

function RegionGroup({ form, updateRegion }: { form: GenerateFormState; updateRegion: (key: keyof GenerateFormState["region"], value: string | [number, number, number]) => void }) {
  const { locale } = useI18n();
  return (
    <div className="group-box">
      <div className="group-box-title">{t("generate.region")}</div>
      <TextRow label={t("generate.regionName")} value={form.region.name} onChange={(value) => updateRegion("name", value)} />
      <Vec3Row label={t("generate.start")} value={form.region.origin} onChange={(value) => updateRegion("origin", value)} />
      <Vec3Row label={t("generate.dimensions")} value={form.region.size} min={1} onChange={(value) => updateRegion("size", value)} />
    </div>
  );
}

function MaterialEditor({
  block,
  material,
  allBlocks,
  onBlockChange,
  onMaterialChange,
}: {
  block: { name: string; properties?: Record<string, string> };
  material?: MaterialSpec;
  allBlocks: string[];
  onBlockChange: (block: { name: string; properties: Record<string, string> }) => void;
  onMaterialChange: (material: MaterialSpec | undefined) => void;
}) {
  const { locale } = useI18n();
  const mode = material?.type || "legacy_block";
  const setMode = (nextMode: string) => {
    if (nextMode === "weighted_random") {
      onMaterialChange({
        type: "weighted_random",
        seed: 12345,
        entries: [
          { id: makeId(), weight: 70, block: cloneBlock(block) },
          { id: makeId(), weight: 30, block: { name: "minecraft:cracked_stone_bricks", properties: {} } },
        ],
      });
    } else if (nextMode === "single") {
      onMaterialChange({ type: "single", block: cloneBlock(block) });
    } else {
      onMaterialChange(undefined);
    }
  };
  const singleBlock = material?.type === "single" ? material.block : block;

  return (
    <div className="generate-block-selector">
      <div className="form-row">
        <div className="form-label">{t("generate.materialMix")}</div>
        <Dropdown
          value={mode}
          options={[
            { label: t("generate.simpleBlock"), value: "legacy_block" },
            { label: t("generate.singleMaterial"), value: "single" },
            { label: t("generate.weighted"), value: "weighted_random" },
          ]}
          onChange={setMode}
        />
        {allBlocks.length === 0 && <span className="generate-muted">{t("generate.missingDatabase")}</span>}
      </div>
      {mode === "weighted_random" && material?.type === "weighted_random" ? (
        <WeightedRandomEditor material={material} onChange={onMaterialChange} />
      ) : (
        <BlockSelector
          block={singleBlock}
          onChange={(nextBlock) => {
            if (material?.type === "single") onMaterialChange({ type: "single", block: nextBlock });
            else onBlockChange(nextBlock);
          }}
        />
      )}
    </div>
  );
}

function CheckerboardMaterialEditor({
  materialA,
  materialB,
  onChangeA,
  onChangeB,
}: {
  materialA?: MaterialSpec;
  materialB?: MaterialSpec;
  onChangeA: (material: MaterialSpec) => void;
  onChangeB: (material: MaterialSpec) => void;
}) {
  const { locale } = useI18n();
  const blockA = materialA?.type === "single" ? materialA.block : { name: "minecraft:white_concrete", properties: {} };
  const blockB = materialB?.type === "single" ? materialB.block : { name: "minecraft:black_concrete", properties: {} };
  return (
    <div className="generate-block-selector">
      <div className="generate-material-entry">
        <div className="generate-material-heading">{t("generate.checkerA")}</div>
        <BlockSelector block={blockA} onChange={(block) => onChangeA({ type: "single", block })} />
      </div>
      <div className="generate-material-entry">
        <div className="generate-material-heading">{t("generate.checkerB")}</div>
        <BlockSelector block={blockB} onChange={(block) => onChangeB({ type: "single", block })} />
      </div>
    </div>
  );
}

function WeightedRandomEditor({ material, onChange }: { material: Extract<MaterialSpec, { type: "weighted_random" }>; onChange: (material: MaterialSpec) => void }) {
  const { locale } = useI18n();
  const updateEntry = (index: number, patch: Partial<MaterialEntrySpec>) => {
    onChange({ ...material, entries: material.entries.map((entry, i) => i === index ? { ...entry, ...patch } : entry) });
  };
  const removeEntry = (index: number) => {
    onChange({ ...material, entries: material.entries.filter((_, i) => i !== index) });
  };
  return (
    <div>
      <div className="form-row">
        <div className="form-label">{t("generate.randomSeed")}</div>
        <Input className="input generate-seed-input" type="number" value={material.seed} onChange={(e) => onChange({ ...material, seed: Number(e.target.value) })} />
        <Button variant="secondary" onClick={() => onChange({ ...material, entries: [...material.entries, { id: makeId(), weight: 10, block: { name: "minecraft:stone_bricks", properties: {} } }] })}>{t("generate.addMaterial")}</Button>
      </div>
      {material.entries.map((entry, index) => (
        <div key={entry.id} className="generate-material-entry">
          <div className="form-row">
            <div className="form-label">{t("generate.weight")}</div>
            <Input className="input generate-number-input" type="number" min={1} value={entry.weight} onChange={(e) => updateEntry(index, { weight: Number(e.target.value) })} />
            <Button variant="secondary" onClick={() => removeEntry(index)} disabled={material.entries.length <= 1}>{t("editor.delete")}</Button>
          </div>
          <BlockSelector block={entry.block} onChange={(nextBlock) => updateEntry(index, { block: nextBlock })} />
        </div>
      ))}
    </div>
  );
}

function BlockSelector({ block, onChange }: { block: { name: string; properties?: Record<string, string> }; onChange: (block: { name: string; properties: Record<string, string> }) => void }) {
  const { locale } = useI18n();
  const properties = getBlockProperties(block.name);
  const defaultProperties = getDefaultProperties(block.name);
  const propertyKeys = Object.keys(properties).sort();
  const setProperty = (key: string, value: string) => {
    const next = { ...(block.properties || {}) };
    if (!value) delete next[key];
    else next[key] = value;
    onChange({ name: block.name, properties: next });
  };

  return (
    <div>
      <BlockSearchSelect value={block.name} onChange={(blockId) => onChange({ name: blockId, properties: {} })} />
      <details className="generate-advanced">
        <summary>{t("generate.advancedStates")}</summary>
        <Input className="input" aria-label={t("generate.blockId")} value={block.name} onChange={(event) => onChange({ name: event.target.value, properties: {} })} placeholder="minecraft:stone_bricks" />
        <div className="generate-block-state-actions">
          <Button variant="secondary" onClick={() => onChange({ name: block.name, properties: { ...defaultProperties } })}>{t("generate.defaultStates")}</Button>
          <Button variant="secondary" onClick={() => onChange({ name: block.name, properties: {} })}>{t("generate.clearStates")}</Button>
        </div>
        {propertyKeys.length === 0 ? <p className="muted">{t("generate.noStates")}</p> : (
          <div className="generate-block-states">
            {propertyKeys.map((key) => (
              <label key={key}>
                <span>{translateKey(key)}</span>
                <Dropdown value={(block.properties || {})[key] || ""} onChange={(value) => setProperty(key, value)} ariaLabel={translateKey(key)}
                  options={[{ value: "", label: t("generate.unspecified") }, ...properties[key].map((value) => ({ value, label: `${translateValue(key, value)} (${value})` }))]} />
              </label>
            ))}
          </div>
        )}
      </details>
    </div>
  );
}

function SummaryView({ summary }: { summary: GenerateSummary }) {
  const { locale } = useI18n();
  return (
    <div className="generate-summary">
      <div>{t("generate.regionCount")} {(summary.regions_count).toLocaleString(locale)}</div>
      <div>{t("generate.shapeCount")} {typeof (summary.operation_count ?? "-") === "number" ? Number(summary.operation_count ?? "-").toLocaleString(locale) : "-"}</div>
      <div>{t("generate.volume")} {(summary.total_volume).toLocaleString(locale)}</div>
      <div>{t("generate.nonAir")} {(summary.estimated_non_air_blocks).toLocaleString(locale)}</div>
      <div>{t("generate.paletteCount")} {typeof (summary.estimated_palette_count ?? "-") === "number" ? Number(summary.estimated_palette_count ?? "-").toLocaleString(locale) : "-"}</div>
      <div>{t("generate.regionPalette")} {summary.palette_entries_per_region.map((entry) => `${entry.region}=${entry.palette_entries.toLocaleString(locale)}`).join(", ")}</div>
      <div className="generate-summary-heading">{t("generate.materialPreview")}</div>
      {summary.material_summary.map((entry) => (
        <div className="generate-material-summary-row" key={`${entry.block_id}-${JSON.stringify(entry.properties)}`} title={`${entry.block_id} ${JSON.stringify(entry.properties)}`}><BlockIcon blockId={entry.block_id} /><span>{translateBlockId(entry.block_id)}</span><strong>{entry.count.toLocaleString(locale)}</strong></div>
      ))}
      {summary.operations && summary.operations.length > 0 && (
        <>
          <div className="generate-summary-heading">{t("generate.shapeImpact")}</div>
          {summary.operations.map((operation) => (
            <div key={`${operation.region}-${operation.index}`}>#{operation.index + 1} {operation.name || t(`generate.${operation.operation_type}`)}: {operation.affected_block_count.toLocaleString(locale)}</div>
          ))}
        </>
      )}
      {summary.warnings.length > 0 && <div className="generate-summary-warning">{t("generate.hint")} {summary.warnings.map(localizeMessage).join("; ")}</div>}
    </div>
  );
}

function ErrorList({ errors }: { errors: UiMessage[] }) {
  useI18n();
  return <div className="generate-error-list">{errors.map((error,index) => <div key={index}>{displayPathText(localizeMessage(error))}</div>)}</div>;
}

function TextRow({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  const { locale } = useI18n();
  return (
    <div className="form-row">
      <div className="form-label">{label}</div>
      <Input className="input generate-grow" aria-label={label} value={value} onChange={(e) => onChange(e.target.value)} />
    </div>
  );
}

function NumberRow({ label, value, onChange, min }: { label: string; value: number; onChange: (value: number) => void; min?: number }) {
  const { locale } = useI18n();
  return (
    <div className="form-row">
      <div className="form-label">{label}</div>
      <Input className="input generate-number-input" type="number" aria-label={label} min={min} value={value} onChange={(e) => onChange(Number(e.target.value))} />
    </div>
  );
}

function CheckRow({ label, value, onChange }: { label: string; value: boolean; onChange: (value: boolean) => void }) {
  const { locale } = useI18n();
  return (
    <div className="form-row">
      <div className="form-label">{label}</div>
      <label className="generate-check-label">
        <Checkbox aria-label={label} checked={value} onCheckedChange={(checked) => onChange((checked === true))} />
        {t("generate.enabled")}
      </label>
    </div>
  );
}

function Vec3Row({ label, value, onChange, min }: { label: string; value: [number, number, number]; onChange: (value: [number, number, number]) => void; min?: number }) {
  const { locale } = useI18n();
  const setAxis = (axis: 0 | 1 | 2, next: number) => {
    const vec: [number, number, number] = [...value] as [number, number, number];
    vec[axis] = next;
    onChange(vec);
  };
  return (
    <div className="form-row">
      <div className="form-label">{label}</div>
      {[0, 1, 2].map((axis) => (
        <Input key={axis} className="input generate-axis-input" aria-label={`${label} ${["x", "y", "z"][axis]}`} type="number" min={min} value={value[axis]} onChange={(e) => setAxis(axis as 0 | 1 | 2, Number(e.target.value))} />
      ))}
    </div>
  );
}


