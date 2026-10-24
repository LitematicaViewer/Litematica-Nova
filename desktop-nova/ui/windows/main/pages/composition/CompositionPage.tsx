import React, { useEffect, useRef, useState } from "react";
import { Plus, Copy, Trash2, Undo2, Redo2, Save, Download, Play, X, Scissors, Eye, Lock, MousePointer, Move, RotateCw, FlipHorizontal, FlipVertical } from "lucide-react";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { confirmDialog, openDialog, saveDialog } from "../../../../../src/platform/dialogs";
import { getUiLocale } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { RenderPage } from "../render/RenderPage";
import { useQuickPreview } from "../../useQuickPreview";
import { readQuickPreviewEditorEvent } from "../../../../../src/platform/quickPreview";
import { updateSceneView, releaseCameraInput } from "../../../../../src/platform/sceneView";
import { querySchematicFormat } from "../../../../../src/platform/schematicFormats";
import { queryComposition, cancelComposition, type CompositionIntent, type CompositionProject, type CompositionResult, type CompositionReport, type CompositionTransform, type Vec3, type SceneBounds, type CompositionEditorScene, type CompositionEditorTool, type CompositionEditorEvent, type CompositionSplitBox } from "../../../../../src/platform/composition";
import "./composition.css";

export function CompositionPage({ currentFile = "", presentationBlocked = false, initialSources = [], initialProjectId = "", setCompositionId }: {
  currentFile?: string; presentationBlocked?: boolean; initialSources?: string[]; initialProjectId?: string; setCompositionId?: (id: string) => void;
}) {
  useI18n();
  const l = (zh: string, en: string) => getUiLocale().startsWith("zh") ? zh : en;
  const [compactInspector, setCompactInspector] = useState(() => window.matchMedia("(max-width: 1439px)").matches);
  const [compactParts, setCompactParts] = useState(() => window.matchMedia("(max-width: 959px)").matches);
  const [panel, setPanel] = useState<"parts" | "properties" | null>(null);
  useEffect(() => {
    const inspector = window.matchMedia("(max-width: 1439px)"), parts = window.matchMedia("(max-width: 959px)");
    const update = () => { setCompactInspector(inspector.matches); setCompactParts(parts.matches); };
    inspector.addEventListener("change", update); parts.addEventListener("change", update);
    return () => { inspector.removeEventListener("change", update); parts.removeEventListener("change", update); };
  }, []);
  const drawer = panel === "parts" ? compactParts : panel === "properties" && compactInspector;
  const [view, setView] = useState<CompositionResult>({});
  const [projects, setProjects] = useState<CompositionProject[]>([]);
  const [selected, setSelected] = useState("");
  const [pending, setPending] = useState(false);
  const [activeOperation, setActiveOperation] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [report, setReport] = useState<CompositionReport | null>(null);
  const [migration, setMigration] = useState<CompositionResult | null>(null);
  const [previewPath, setPreviewPath] = useState("");
  const [previewGeneration, setPreviewGeneration] = useState<number | null>(null);
  const [pane, setPane] = useState<"editor" | "result">("editor");
  const [tool, setTool] = useState<CompositionEditorTool>("translate");
  const [editorSourcePath, setEditorSourcePath] = useState("");
  const [editorScenePath, setEditorScenePath] = useState("");
  const [editorScene, setEditorScene] = useState<CompositionEditorScene | null>(null);
  const [requiresPreparation, setRequiresPreparation] = useState(false);
  const [editorPollRevision, setEditorPollRevision] = useState(0);
  const [startRevision, setStartRevision] = useState(0);
  const [placement, setPlacement] = useState<"original" | "beside">("original");
  const [sourceVersion, setSourceVersion] = useState("");
  const [sourceVersions, setSourceVersions] = useState<{ version: string; dataVersion: number | null }[]>([]);
  const [format, setFormat] = useState("litematic");
  const [splitMode, setSplitMode] = useState<"regions" | "boxes" | "plane">("plane");
  const [axis, setAxis] = useState<"x" | "y" | "z">("y");
  const [plane, setPlane] = useState("0");
  const [splitOutput, setSplitOutput] = useState<"files" | "project">("files");
  const [boxes, setBoxes] = useState<CompositionSplitBox[]>([]);
  const [boxesDirty, setBoxesDirty] = useState(false);
  const [splitView, setSplitView] = useState<CompositionResult["outputs"]>();
  const active = useRef<string | null>(null), busyRef = useRef(false), alive = useRef(true);
  const initial = useRef(initialSources), startRequested = useRef(false);
  const projectRef = useRef<CompositionProject | undefined>();
  const selectedRef = useRef(selected), toolRef = useRef(tool), boxesDirtyRef = useRef(boxesDirty);
  const editorSceneRef = useRef(editorScene), editorPathRef = useRef(editorScenePath), editorSourceRef = useRef(editorSourcePath);
  const editorSyncRevision = useRef(0);
  const eventCursor = useRef({ session: "", sequence: 0 });
  const busy = pending;
  selectedRef.current = selected; toolRef.current = tool; boxesDirtyRef.current = boxesDirty;
  editorSceneRef.current = editorScene; editorPathRef.current = editorScenePath; editorSourceRef.current = editorSourcePath;
  const project = view.project;
  const editor = useQuickPreview(editorSourcePath, presentationBlocked || !!drawer || pane !== "editor" || requiresPreparation || !project, false, editorScenePath || undefined);
  const editorRef = useRef(editor); editorRef.current = editor;
  const part = project?.state.parts.find(p => p.id === selected), detail = view.parts?.find(p => p.part.id === selected);
  const setSelection = (id: string) => { selectedRef.current = id; setSelected(id); };
  const setEditorTool = (next: CompositionEditorTool) => { toolRef.current = next; setTool(next); };
  const adoptEditor = (result: CompositionResult) => {
    if (result.outputPath) { editorSourceRef.current = result.outputPath; setEditorSourcePath(result.outputPath); }
    if (result.editorScenePath) { editorPathRef.current = result.editorScenePath; setEditorScenePath(result.editorScenePath); }
    if (result.editorScene) { editorSceneRef.current = result.editorScene; setEditorScene(result.editorScene); }
    if (result.requiresGeometryUpdate || result.requiresPreparation) setRequiresPreparation(true);
    else if (result.editorScene) setRequiresPreparation(false);
  };
  const applyResult = (result: CompositionResult, action: CompositionIntent["action"]) => {
    if (result.project) {
      const changedProject = result.project.id !== projectRef.current?.id;
      if (changedProject) {
        ++editorSyncRevision.current;
        editorSourceRef.current = ""; editorPathRef.current = ""; editorSceneRef.current = null;
        setEditorSourcePath(""); setEditorScenePath(""); setEditorScene(null); setRequiresPreparation(false);
        setPreviewPath(""); setPreviewGeneration(null); setMigration(null); setSplitView(undefined); setPane("editor");
        eventCursor.current = { session: "", sequence: 0 };
        setSelection(result.project.state.parts[0]?.id ?? "");
      } else if (selectedRef.current && !result.project.state.parts.some(p => p.id === selectedRef.current)) setSelection(result.project.state.parts[0]?.id ?? "");
      projectRef.current = result.project; setView(result); setReport(null); setCompositionId?.(result.project.id);
      if (changedProject || !boxesDirtyRef.current || action === "setSplitBoxes" || action === "commitEditor" || action === "undo" || action === "redo") {
        setBoxes(result.project.state.splitBoxes ?? []); boxesDirtyRef.current = false; setBoxesDirty(false);
      }
    }
    if (result.report) setReport(result.report);
    if (result.migrationReports) setMigration(result);
    if (result.outputs) setSplitView(result.outputs);
    if (action === "preview" && result.outputPath) { setPreviewPath(result.outputPath); setPreviewGeneration(result.generation ?? null); setPane("result"); }
    if (action === "previewEditor") adoptEditor(result);
  };
  async function perform(intent: CompositionIntent) {
    const operation = crypto.randomUUID(); active.current = operation; setActiveOperation(operation);
    try {
      const current = projectRef.current;
      const identity = current && !["create", "open", "list"].includes(intent.action) ? { projectId: current.id, generation: current.generation } : {};
      const result = await queryComposition({ ...identity, ...intent }, operation);
      if (!alive.current) return;
      applyResult(result, intent.action);
      return result;
    } finally { if (active.current === operation) { active.current = null; if (alive.current) setActiveOperation(null); } }
  }
  async function syncEditor(restore = false) {
    const current = projectRef.current;
    if (!current) return false;
    const ticket = ++editorSyncRevision.current;
    const result = await queryComposition({ action: "editorState", projectId: current.id, generation: current.generation,
      ...(editorPathRef.current ? { editorScenePath: editorPathRef.current } : {}),
      ...(restore ? {} : { selectedInstanceId: selectedRef.current || null, tool: toolRef.current }) }, crypto.randomUUID());
    if (!alive.current || ticket !== editorSyncRevision.current || projectRef.current?.id !== current.id || projectRef.current.generation !== current.generation) return false;
    adoptEditor(result);
    if (!result.editorScene) return false;
    if (restore) { setSelection(result.editorScene.selectedInstanceId ?? ""); setEditorTool(result.editorScene.tool); }
    if (editorRef.current.operation?.state === "ready" && editorSourceRef.current) {
      await updateSceneView({ sourcePath: editorSourceRef.current, displayMode: "fast_experimental", controls: { compositionEditor: result.editorScene } });
    }
    return true;
  }
  async function consumeEditorEvent(event: CompositionEditorEvent) {
    const current = projectRef.current, scene = editorSceneRef.current;
    if (!current || !scene || event.projectId !== current.id || event.sceneKey !== scene.sceneKey || event.sequence <= eventCursor.current.sequence) return;
    // The Application Host CAS-commits native gestures before returning them.
    // React reads authoritative state rather than persisting a second copy.
    if (event.committed && !event.cancelled || event.generation !== current.generation) {
      await perform({ action: "open", projectId: current.id });
    }
    setSelection(event.selectedInstanceId ?? "");
    if (!await syncEditor()) throw new Error(l("编辑回执尚未完成同步，请使用“同步编辑状态”。", "The editor receipt is not synchronized. Use Sync editor state."));
    eventCursor.current.sequence = event.sequence;
    if (event.cancelled) setNotice(l("当前手势已取消，项目没有修改。", "Gesture cancelled. The project is unchanged."));
  }
  async function readPendingEditorEvent() {
    const operation = editorRef.current.operation, scene = editorSceneRef.current;
    if (operation?.state !== "ready" || !scene) return null;
    const session = `${operation.previewId}\n${scene.eventPath}`;
    if (eventCursor.current.session !== session) { eventCursor.current = { session, sequence: 0 }; }
    return readQuickPreviewEditorEvent(operation.previewId, eventCursor.current.sequence);
  }
  const run = async (intent: CompositionIntent, afterFlush?: () => CompositionIntent) => {
    if (busyRef.current) return;
    busyRef.current = true; setPending(true); setError(""); setNotice("");
    try {
      // Flush a completed native gesture before exports/undo/numeric commands so
      // a toolbar click cannot observe the preceding, not-yet-acknowledged pose.
      if (!["list", "open", "create"].includes(intent.action)) { const event = await readPendingEditorEvent(); if (event) await consumeEditorEvent(event); }
      if (!alive.current) return;
      const result = await perform(afterFlush ? afterFlush() : intent);
      if (!result) return;
      if (intent.action === "export" || intent.action === "split") setNotice(intent.action === "split" && intent.splitOutput === "project" ? l("已创建拆分组合，源文件未修改。", "Split composition created. Sources are unchanged.") : l("导出完成，源文件未修改。", "Export complete. Sources are unchanged."));
      if (["create", "save", "split"].includes(intent.action)) { const list = await queryComposition({ action: "list" }, crypto.randomUUID()); if (alive.current) setProjects(list.projects ?? []); }
      return result;
    } catch (reason) { if (alive.current) setError(String(reason)); }
    finally { busyRef.current = false; if (alive.current) setPending(false); }
  };
  useEffect(() => {
    alive.current = true;
    void queryComposition({ action: "list" }, crypto.randomUUID()).then(result => { if (alive.current) setProjects(result.projects ?? []); }).catch(reason => { if (alive.current) setError(String(reason)); });
    void querySchematicFormat({ action: "registry" }).then(result => { if (alive.current) setSourceVersions(result.sourceVersions ?? []); }).catch(reason => { if (alive.current) setError(String(reason)); });
    return () => { alive.current = false; ++editorSyncRevision.current; if (active.current) void cancelComposition(active.current); };
  }, []);
  useEffect(() => { if (initialProjectId && initialProjectId !== projectRef.current?.id) void run({ action: "open", projectId: initialProjectId }); }, [initialProjectId]);
  useEffect(() => {
    if (!project || pending) return;
    let activeEffect = true;
    void syncEditor(!editorPathRef.current).catch(reason => { if (activeEffect) setError(String(reason)); });
    return () => { activeEffect = false; ++editorSyncRevision.current; };
  }, [project?.id, project?.generation, selected, tool, pending, editor.operation?.previewId, editor.operation?.state]);
  useEffect(() => {
    if (!project || !editorScene || editor.operation?.state !== "ready" || pane !== "editor" || presentationBlocked || drawer || requiresPreparation) return;
    let stopped = false, timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        if (!busyRef.current) {
          const event = await readPendingEditorEvent();
          if (event && !stopped && !busyRef.current) {
            busyRef.current = true; setPending(true);
            try { await consumeEditorEvent(event); }
            finally { busyRef.current = false; if (alive.current) setPending(false); }
          }
        }
        if (!stopped) timer = setTimeout(() => void poll(), 120);
      } catch (reason) { if (!stopped) setError(String(reason)); }
    };
    void poll();
    return () => { stopped = true; clearTimeout(timer); };
  }, [project?.id, editorScene?.eventPath, editor.operation?.previewId, editor.operation?.state, pane, presentationBlocked, drawer, requiresPreparation, editorPollRevision]);
  useEffect(() => {
    if (!startRequested.current || !editorSourcePath || !editorScenePath) return;
    startRequested.current = false;
    void editor.start();
  }, [editorSourcePath, editorScenePath, startRevision]);
  useEffect(() => {
    if (!(presentationBlocked || drawer || pane !== "editor") || editor.operation?.state !== "ready") return;
    void releaseCameraInput({ sourcePath: editorSourcePath, displayMode: "fast_experimental" }).catch(reason => { if (alive.current) setError(String(reason)); });
  }, [presentationBlocked, drawer, pane, editor.operation?.state, editorSourcePath]);
  useEffect(() => { setSplitView(undefined); }, [splitMode, axis, plane, boxes, project?.generation]);
  const openEditing = async () => {
    setPane("editor"); setPanel(null);
    if (!editorSourceRef.current || requiresPreparation) {
      const result = await run({ action: "previewEditor", selectedInstanceId: selectedRef.current || undefined, tool: toolRef.current });
      if (!result?.editorScenePath || !result.outputPath) return;
    }
    startRequested.current = true; setStartRevision(value => value + 1);
  };
  const add = async () => {
    const chosen = await openDialog({ multiple: true, filters: [{ name: l("投影文件", "Schematic files"), extensions: ["litematic", "nbt", "json", "bg", "bgcp"] }] });
    if (chosen) await run({ action: project ? "add" : "create", sourcePaths: typeof chosen === "string" ? [chosen] : chosen, placement, ...(sourceVersion ? { sourceDataVersion: Number(sourceVersion) } : {}) });
  };
  const update = (patch: CompositionIntent["patch"]) => part && run({ action: "update", partId: part.id, patch });
  const updateAxis = (group: "offset" | "origin", index: number, value: string) => {
    if (!part || value === "") return;
    const partId = part.id;
    void run({ action: "update", partId }, () => {
      const latest = projectRef.current?.state.parts.find(item => item.id === partId);
      if (!latest) throw new Error(l("部件已移除，请重新选择。", "The part was removed. Select it again."));
      const tuple = [...latest.transform[group]] as Vec3; tuple[index] = Number(value);
      return { action: "update", partId, patch: { transform: { ...latest.transform, [group]: tuple } } };
    });
  };
  const transform = (patch: Partial<CompositionTransform>) => {
    if (!part) return;
    const partId = part.id;
    return run({ action: "update", partId }, () => {
      const latest = projectRef.current?.state.parts.find(item => item.id === partId);
      if (!latest) throw new Error(l("部件已移除，请重新选择。", "The part was removed. Select it again."));
      return { action: "update", partId, patch: { transform: { ...latest.transform, ...patch } } };
    });
  };
  const exportFile = async () => { const path = await saveDialog({ defaultPath: `${project?.state.name ?? "composition"}.${format === "litematic" ? "litematic" : format.startsWith("building") ? "json" : "nbt"}` }); if (path) await run({ action: "export", format, outputPath: path }); };
  const splitInput = { splitMode, axis, at: Number(plane), boxes: project?.state.splitBoxes ?? [] };
  const exportSplit = async () => { if (splitOutput === "project") { await run({ action: "split", ...splitInput, splitOutput }); return; } const directory = await openDialog({ directory: true }); if (typeof directory === "string") await run({ action: "split", ...splitInput, splitOutput, outputDirectory: directory }); };
  const editBoxes = (next: CompositionSplitBox[]) => { boxesDirtyRef.current = true; setBoxesDirty(true); setBoxes(next); if (toolRef.current === "split") setEditorTool("select"); };
  const applyBoxes = async (next = boxes) => {
    const result = await run({ action: "setSplitBoxes", boxes: next });
    if (result?.project) { setSplitMode("boxes"); setEditorTool("split"); setPane("editor"); }
  };
  const removePart = async (partId: string) => {
    if (await confirmDialog(l("从组合中移除此实例？源投影文件不会删除。", "Remove this instance from the composition? The source schematic will not be deleted."), { title: l("移除部件", "Remove part"), kind: "warning" })) await run({ action: "remove", partId });
  };
  const partsPanel = <div className="composition-parts" aria-label={l("部件列表", "Parts")}><h3>{l("部件", "Parts")}</h3>{project?.state.parts.map(p => <div key={p.id} className={`composition-part${selected === p.id ? " is-selected" : ""}`}><button className="composition-part-name" aria-pressed={selected === p.id} onClick={() => setSelection(p.id)}>{p.locked && <Lock size={13} />}{p.name}</button><div className="composition-part-actions"><label><Checkbox disabled={busy || p.locked} checked={p.visible} onCheckedChange={v => void run({ action: "update", partId: p.id, patch: { visible: v === true } })} /><Eye size={13} />{l("显示", "Show")}</label><button disabled={busy || p.locked} aria-label={l("复制实例", "Duplicate instance")} onClick={() => void run({ action: "duplicate", partId: p.id })}><Copy size={14} /></button><button disabled={busy || p.locked} aria-label={l("删除实例", "Remove instance")} onClick={() => void removePart(p.id)}><Trash2 size={14} /></button></div></div>)}{!project && <p className="composition-muted">{l("添加投影开始组合，或打开已保存项目。", "Add schematics to begin, or open a saved project.")}</p>}</div>;
  const propertiesPanel = <div className="composition-inspector"><h3>{l("部件属性", "Part properties")}</h3>{part ? <><label>{l("名称", "Name")}<input key={`${part.id}-name-${project?.generation}`} defaultValue={part.name} disabled={busy || part.locked} onBlur={e => { if (e.target.value !== part.name) void update({ name: e.target.value }); }} /></label><label><Checkbox checked={part.locked} disabled={busy} onCheckedChange={v => void update({ locked: v === true })} />{l("锁定实例", "Lock instance")}</label>{(["offset", "origin"] as const).map(group => <fieldset key={group} disabled={busy || part.locked}><legend>{group === "offset" ? l("位移（整数方块）", "Offset (integer blocks)") : l("局部原点", "Local origin")}</legend><div className="composition-vector">{["X", "Y", "Z"].map((a, i) => <label key={a}>{a}<input type="number" step="1" key={`${part.id}-${group}-${i}-${project?.generation}`} defaultValue={part.transform[group][i]} onBlur={e => { if (Number(e.target.value) !== part.transform[group][i]) updateAxis(group, i, e.target.value); }} /></label>)}</div></fieldset>)}<label>{l("绕 Y 旋转", "Y rotation")}<select value={part.transform.rotation} disabled={busy || part.locked} onChange={e => void transform({ rotation: Number(e.target.value) as CompositionTransform["rotation"] })}>{[0, 90, 180, 270].map(v => <option key={v} value={v}>{v}°</option>)}</select></label><div className="composition-checks"><label><Checkbox checked={part.transform.mirrorX} disabled={busy || part.locked} onCheckedChange={v => void transform({ mirrorX: v === true })} />{l("镜像 X", "Mirror X")}</label><label><Checkbox checked={part.transform.mirrorZ} disabled={busy || part.locked} onCheckedChange={v => void transform({ mirrorZ: v === true })} />{l("镜像 Z", "Mirror Z")}</label></div><label><Checkbox checked={part.clearAir} disabled={busy || part.locked} onCheckedChange={v => void update({ clearAir: v === true })} />{l("显式清空所覆盖区域的空气", "Explicitly clear air in covered selections")}</label><dl><dt>{l("尺寸", "Size")}</dt><dd>{detail?.size?.join(" × ") ?? "—"}</dd><dt>{l("边界", "Bounds")}</dt><dd>{detail?.bounds ? `${detail.bounds.min.join(", ")} → ${detail.bounds.max.join(", ")}` : "—"}</dd><dt>DataVersion</dt><dd>{detail?.dataVersion ?? "—"}</dd></dl><fieldset disabled={busy || part.locked}><legend>{l("源选区（空选表示全部）", "Source regions (none selects all)")}</legend>{detail?.regions.map(name => <label key={name}><Checkbox checked={part.regions.includes(name)} onCheckedChange={v => void update({ regions: v === true ? [...part.regions, name] : part.regions.filter(r => r !== name) })} />{name}</label>)}</fieldset></> : <p className="composition-muted">{l("选择左侧部件编辑。", "Select a part to edit.")}</p>}</div>;
  return <section className="composition-page">
    <header className="composition-header"><div><h2>{l("组合 / 拆分", "Compose / split")}</h2>{project && <input aria-label={l("组合名称", "Composition name")} key={`${project.id}-title-${project.generation}`} defaultValue={project.state.name} disabled={busy} onBlur={e => { if (e.target.value !== project.state.name) void run({ action: "save", name: e.target.value }); }} />}<p>{l("源文件保持只读；每个部件都是独立实例。", "Source files stay read-only. Every part is an independent instance.")}</p></div><select aria-label={l("打开组合", "Open composition")} value={project?.id ?? ""} disabled={busy} onChange={e => void run({ action: "open", projectId: e.target.value })}><option value="">{l("选择已保存组合…", "Choose a saved composition…")}</option>{projects.map(p => <option key={p.id} value={p.id}>{p.state.name}</option>)}</select></header>
    <div className="composition-toolbar">
      {compactParts && <Button variant="outline" aria-expanded={panel === "parts"} onClick={() => setPanel("parts")}>{l("部件列表", "Parts")}</Button>}
      {compactInspector && <Button variant="outline" aria-expanded={panel === "properties"} onClick={() => setPanel("properties")}>{l("部件属性", "Part properties")}</Button>}
      <Button disabled={busy} onClick={() => void add()}><Plus size={16} />{l("添加部件", "Add parts")}</Button>
      <select value={placement} onChange={e => setPlacement(e.target.value as typeof placement)} aria-label={l("添加位置", "Placement")}><option value="original">{l("按原位置", "Original positions")}</option><option value="beside">{l("放在组合旁", "Place beside composition")}</option></select>
      <label className="composition-source-version">{l("源版本缺失时", "Missing source version")}<select value={sourceVersion} onChange={event => setSourceVersion(event.target.value)}><option value="">{l("使用文件声明", "Use file declaration")}</option>{sourceVersions.map(version => <option key={version.version} disabled={version.dataVersion === null} value={version.dataVersion ?? ""}>{version.version}</option>)}</select></label>
      {!project && (currentFile || initial.current.length > 0) && <Button variant="outline" disabled={busy} onClick={() => void run({ action: "create", sourcePaths: initial.current.length ? initial.current : [currentFile], placement, ...(sourceVersion ? { sourceDataVersion: Number(sourceVersion) } : {}) })}>{l("加入当前投影", "Add current schematic")}</Button>}
      <Button variant="ghost" disabled={busy || !view.canUndo} onClick={() => void run({ action: "undo" })}><Undo2 size={16} />{l("撤销", "Undo")}</Button><Button variant="ghost" disabled={busy || !view.canRedo} onClick={() => void run({ action: "redo" })}><Redo2 size={16} />{l("重做", "Redo")}</Button>
      <Button variant="outline" disabled={busy || !project} onClick={() => void run({ action: "save" })}><Save size={16} />{l("保存组合", "Save")}</Button>
      <select aria-label={l("导出格式", "Export format")} value={format} onChange={e => setFormat(e.target.value)}><option value="litematic">Litematic</option><option value="structure">Minecraft Structure</option><option value="create">Create</option><option value="anvilCraft">AnvilCraft</option><option value="buildingGadgetsLegacy">Building Gadgets</option><option value="buildingGadgets2">Building Gadgets 2</option></select>
      <Button variant="outline" disabled={busy || !project} onClick={() => void exportFile()}><Download size={16} />{l("导出合并投影…", "Export merged…")}</Button>
      {activeOperation && <Button variant="ghost" onClick={() => { if (active.current) void cancelComposition(active.current); }}><X size={16} />{l("取消", "Cancel")}</Button>}
    </div>
    {error && <p className="composition-error" role="alert">{error}</p>}{notice && <p role="status">{notice}</p>}
    <div className={`composition-workspace${compactInspector ? " is-compact-inspector" : ""}${compactParts ? " is-compact-parts" : ""}`}>
      {!compactParts && partsPanel}

      <main className="composition-canvas">
        <div className="composition-canvas-toolbar">
          <div className="composition-pane-switch" role="group" aria-label={l("视图", "View")}>
            <Button variant={pane === "editor" ? "secondary" : "ghost"} aria-pressed={pane === "editor"} onClick={() => setPane("editor")}>{l("快速编辑视图", "Fast editing view")}</Button>
            <Button variant={pane === "result" ? "secondary" : "ghost"} aria-pressed={pane === "result"} onClick={() => setPane("result")}>{l("合并结果预览", "Merged result preview")}</Button>
          </div>
          {pane === "editor" ? <>
            <Button disabled={busy || !project || editor.pending} onClick={() => void openEditing()}><Play size={15} />{requiresPreparation || !editorSourcePath ? l("准备编辑视图", "Prepare editing view") : l("显示编辑视图", "Show editing view")}</Button>
            <Button variant="ghost" disabled={!editor.operation || editor.pending} onClick={() => void editor.hide()}>{l("隐藏", "Hide")}</Button>
            <Button variant="ghost" disabled={!project || busy} onClick={() => { setError(""); void syncEditor(!editorPathRef.current).then(ok => { if (ok) setEditorPollRevision(value => value + 1); }).catch(reason => setError(String(reason))); }}>{l("同步编辑状态", "Sync editor state")}</Button>
          </> : <Button disabled={busy || !project} onClick={() => void run({ action: "preview" })}><Play size={15} />{l("生成 / 更新合并快照", "Generate / update merged snapshot")}</Button>}
        </div>
        <div className="composition-editor-panel" hidden={pane !== "editor"}>
          <div className="composition-editor-tools" role="group" aria-label={l("原生编辑工具", "Native editing tools")}>
            {([
              ["select", l("选择", "Select"), MousePointer], ["translate", l("平移", "Move"), Move], ["rotate", l("旋转", "Rotate"), RotateCw],
              ["mirror_x", l("镜像 X", "Mirror X"), FlipHorizontal], ["mirror_z", l("镜像 Z", "Mirror Z"), FlipVertical], ["split", l("拆分框", "Split bounds"), Scissors],
            ] as const).map(([key, label, Icon]) => <Button key={key} size="sm" variant={tool === key ? "secondary" : "ghost"} aria-pressed={tool === key}
              disabled={busy || !editorScene || requiresPreparation || key === "split" && (!project?.state.splitBoxes.length || boxesDirty)} onClick={() => setEditorTool(key)}><Icon size={14} />{label}</Button>)}
          </div>
          <div ref={editor.hostRef} className="composition-native-viewport" aria-label={l("组合原生三维编辑视口", "Native 3D composition editor")}>
            {(editor.operation?.state !== "ready" || requiresPreparation) && <div className="composition-empty"><h3>{l("快速编辑视图", "Fast editing view")}</h3><p>{requiresPreparation ? l("部件资源已变化，请准备编辑视图。普通位置、旋转、镜像与拆分框修改不重建几何。", "Part resources changed. Prepare the editor. Position, rotation, mirrors and split bounds reuse the existing geometry.") : editor.pending || editor.operation?.state === "starting" ? l("正在准备原生编辑视图…", "Preparing the native editing view…") : l("添加部件后准备编辑视图；同一源资源仅构建一次，拖拽由原生 Viewer 处理。", "Add parts and prepare the editor. Each unique source resource is built once. Gestures are handled by the native Viewer.")}</p></div>}
          </div>
          <p className="composition-editor-help">{l("左键选择 / 拖动手柄；Ctrl+左键循环重叠实例；右键环绕、中键平移、滚轮缩放；Esc 取消。平移按方块、旋转按 90° 吸附。", "Left-click to select / drag handles; Ctrl+click cycles overlapping instances. Right-drag orbits, middle-drag pans, wheel zooms. Esc cancels. Movement snaps to blocks and rotation to 90°.")}</p>
          {editor.error && <p className="composition-error" role="alert">{editor.error}</p>}
        </div>
        <div className="composition-result-panel" hidden={pane !== "result"}>
          {previewGeneration !== null && previewGeneration !== project?.generation && <p role="status" className="composition-editor-help">{l("合并快照早于当前编辑，请明确更新后查看最终结果。", "The merged snapshot predates the current edits. Update it to inspect the final result.")}</p>}
          {previewPath ? <RenderPage currentFile={previewPath} presentationBlocked={presentationBlocked || !!drawer || pane !== "result"} /> : <div className="composition-empty"><h3>{l("合并结果预览", "Merged result preview")}</h3><p>{l("明确生成合并快照后，使用真实快速、标准或完整模式查看最终结果。编辑视图不会触发标准 / 完整构建或领钥。", "Generate a merged snapshot explicitly, then use the real Fast, Standard or Full renderer. Editing never triggers a Standard/Full build or key request.")}</p></div>}
        </div>
      </main>
      {!compactInspector && propertiesPanel}

    </div>
    {project && <section className="composition-bottom">
      <div className="composition-report-controls">
        <Button variant="outline" disabled={busy} onClick={() => void run({ action: "migrationReport" })}>{l("检查版本兼容", "Check version compatibility")}</Button>
        {migration?.writable && <Button variant="outline" disabled={busy} onClick={() => void run({ action: "migrate", targetVersion: migration.targetVersion })}>{l("转换到组合版本", "Convert to composition version")}</Button>}
        <Button variant="outline" disabled={busy} onClick={() => void run({ action: "report" })}>{l("检查合并与冲突", "Check merge and conflicts")}</Button>
        <label>{l("冲突决策", "Conflict policy")}<select value={project.state.conflictPolicy} disabled={busy} onChange={event => void run({ action: "save", conflictPolicy: event.target.value as CompositionProject["state"]["conflictPolicy"] })}><option value="reject">{l("冲突时阻止导出", "Block export on conflicts")}</option><option value="keepFirst">{l("明确保留列表前项", "Keep earlier listed part")}</option><option value="keepLast">{l("明确保留列表后项", "Keep later listed part")}</option></select></label>
        {report && <span role="status">{l("方块", "Blocks")} {report.blockCount} · {l("冲突", "Conflicts")} {report.collisionCount} · {l("未解决", "Unresolved")} {report.issues.length}</span>}
      </div>
      {migration?.migrationReports && <details><summary>{l("版本兼容报告", "Version compatibility report")}</summary><ul>{migration.migrationReports.map(item => <li key={item.partId}>{item.name}: {item.conversion.sourceVersion} → {item.conversion.targetVersion} — {item.conversion.writable ? l("可无损转换", "Lossless conversion available") : l("存在不兼容内容", "Incompatible content")}<ul>{item.conversion.blockedPayloads.map(value => <li key={value}>{value}</li>)}</ul></li>)}</ul></details>}
      {report && <><ul className="composition-issues">{report.issues.map(value => <li key={value}>{value}</li>)}</ul>{report.collisions.length > 0 && <details><summary>{l("冲突坐标（前200项）", "Conflict coordinates (first 200)")}</summary><ul>{report.collisions.map((value, index) => <li key={index}>{value.position.join(", ")} — {value.keptInstance} / {value.incomingInstance}</li>)}</ul></details>}</>}
      <details className="composition-split">
        <summary><Scissors size={15} />{l("拆分投影", "Split schematic")}</summary>
        <div className="composition-split-controls">
          <select aria-label={l("拆分方式", "Split mode")} value={splitMode} onChange={event => { setSplitMode(event.target.value as typeof splitMode); setSplitView(undefined); }}><option value="regions">{l("按原选区", "Original regions")}</option><option value="plane">{l("沿平面切成两份", "Cut along a plane")}</option><option value="boxes">{l("多个XYZ范围", "Multiple XYZ bounds")}</option></select>
          {splitMode === "plane" && <><select value={axis} onChange={event => setAxis(event.target.value as typeof axis)} aria-label={l("切割轴", "Cut axis")}><option>x</option><option>y</option><option>z</option></select><input type="number" step="1" value={plane} onChange={event => setPlane(event.target.value)} aria-label={l("切割位置", "Cut position")} /></>}
          {splitMode === "boxes" && <><Button variant="outline" disabled={busy} onClick={() => editBoxes([...boxes, { name: `part_${boxes.length + 1}`, bounds: { min: [0, 0, 0], max: [15, 15, 15] } }])}>{l("添加范围", "Add bounds")}</Button><Button disabled={busy || !boxesDirty} onClick={() => void applyBoxes()}>{l("应用范围", "Apply bounds")}</Button>{boxesDirty && <Button variant="ghost" onClick={() => { setBoxes(project.state.splitBoxes ?? []); boxesDirtyRef.current = false; setBoxesDirty(false); }}>{l("放弃范围草稿", "Discard bounds draft")}</Button>}</>}
          <Button variant="outline" disabled={busy || splitMode === "boxes" && boxesDirty} onClick={() => void run({ action: "splitReport", ...splitInput })}>{l("预览拆分清单", "Review split outputs")}</Button>
          {splitView && <Button variant="outline" disabled={busy} onClick={() => void applyBoxes(splitView.map(output => ({ name: output.name, bounds: output.bounds })))}>{l("在三维中编辑这些范围", "Edit these bounds in 3D")}</Button>}
          <select aria-label={l("拆分输出", "Split output")} value={splitOutput} onChange={event => setSplitOutput(event.target.value as typeof splitOutput)}><option value="files">{l("逐份文件", "Separate files")}</option><option value="project">{l("创建组合项目", "Create composition project")}</option></select>
          <Button disabled={busy || !splitView || boxesDirty} onClick={() => void exportSplit()}>{splitOutput === "project" ? l("创建拆分组合", "Create split composition") : l("逐份导出…", "Export parts…")}</Button>
        </div>
        {boxesDirty && <p className="composition-editor-help" role="status">{l("范围是未应用的输入草稿。应用后才更新原生框、拆分报告及导出；不会修改源投影。", "These bounds are an unapplied input draft. Apply them to update native handles, the split report and export. Sources remain unchanged.")}</p>}
        {splitMode === "boxes" && boxes.map((box, index) => <div className="composition-box" key={index}>
          <input aria-label={l("输出名称", "Output name")} disabled={busy} value={box.name} onChange={event => editBoxes(boxes.map((old, row) => row === index ? { ...old, name: event.target.value } : old))} />
          {(["min", "max"] as const).map(edge => <div className="composition-vector" key={edge}><span>{edge}</span>{box.bounds[edge].map((value, coordinate) => <input key={coordinate} type="number" step="1" disabled={busy} aria-label={`${edge} ${"XYZ"[coordinate]}`} value={value} onChange={event => editBoxes(boxes.map((old, row) => row === index ? { ...old, bounds: { ...old.bounds, [edge]: old.bounds[edge].map((number, axisIndex) => axisIndex === coordinate ? Number(event.target.value) : number) as Vec3 } } : old))} />)}</div>)}
          <button disabled={busy} aria-label={l("删除范围", "Remove bounds")} onClick={() => editBoxes(boxes.filter((_, row) => row !== index))}><Trash2 size={15} /></button>
        </div>)}
        {splitView && <table><thead><tr><th>{l("文件", "File")}</th><th>{l("范围", "Bounds")}</th><th>{l("方块 / 实体 / 方块实体", "Blocks / entities / block entities")}</th></tr></thead><tbody>{splitView.map(output => <tr key={output.name}><td>{output.name}.litematic</td><td>{output.bounds.min.join(", ")} → {output.bounds.max.join(", ")}</td><td>{output.report.blockCount} / {output.report.entityCount} / {output.report.blockEntityCount}</td></tr>)}</tbody></table>}
      </details>
    </section>}
    {drawer && <Dialog title={panel === "parts" ? l("部件列表", "Parts") : l("部件属性", "Part properties")} width="sm" className="composition-drawer" onClose={() => setPanel(null)}>{panel === "parts" ? partsPanel : propertiesPanel}</Dialog>}
  </section>;
}
