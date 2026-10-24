import React, { useEffect, useRef, useState } from "react";
import { Plus, Copy, Trash2, Undo2, Redo2, Save, Download, Play, X, Scissors, Eye, Lock } from "lucide-react";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { confirmDialog, openDialog, saveDialog } from "../../../../../src/platform/dialogs";
import { getUiLocale } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { RenderPage } from "../render/RenderPage";
import { queryComposition, cancelComposition, type CompositionIntent, type CompositionProject, type CompositionResult, type CompositionReport, type CompositionTransform, type Vec3, type SceneBounds } from "../../../../../src/platform/composition";
import "./composition.css";

export function CompositionPage({ currentFile = "", presentationBlocked = false, initialSources = [], initialProjectId = "" }: { currentFile?: string; presentationBlocked?: boolean; initialSources?: string[]; initialProjectId?: string }) {
  useI18n();
  const l = (zh: string, en: string) => getUiLocale().startsWith("zh") ? zh : en;
  const [compactInspector, setCompactInspector] = useState(() => window.matchMedia("(max-width: 1439px)").matches);
  const [compactParts, setCompactParts] = useState(() => window.matchMedia("(max-width: 959px)").matches);
  const [panel, setPanel] = useState<"parts" | "properties" | null>(null);
  useEffect(() => {
    // WebView zoom changes CSS-pixel media queries, so these are effective UI-space breakpoints.
    const inspector = window.matchMedia("(max-width: 1439px)");
    const parts = window.matchMedia("(max-width: 959px)");
    const update = () => { setCompactInspector(inspector.matches); setCompactParts(parts.matches); };
    inspector.addEventListener("change", update); parts.addEventListener("change", update);
    return () => { inspector.removeEventListener("change", update); parts.removeEventListener("change", update); };
  }, []);
  const drawer = panel === "parts" ? compactParts : panel === "properties" && compactInspector;
  const [view, setView] = useState<CompositionResult>({});
  const [projects, setProjects] = useState<CompositionProject[]>([]);
  const [selected, setSelected] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [report, setReport] = useState<CompositionReport | null>(null);
  const [migration, setMigration] = useState<CompositionResult | null>(null);
  const [previewPath, setPreviewPath] = useState("");
  const [previewGeneration, setPreviewGeneration] = useState<number | null>(null);
  const [placement, setPlacement] = useState<"original" | "beside">("original");
  const [format, setFormat] = useState("litematic");
  const [splitMode, setSplitMode] = useState<"regions" | "boxes" | "plane">("plane");
  const [axis, setAxis] = useState<"x" | "y" | "z">("y");
  const [plane, setPlane] = useState("0");
  const [splitOutput, setSplitOutput] = useState<"files" | "project">("files");
  const [boxes, setBoxes] = useState<{ name: string; bounds: SceneBounds }[]>([]);
  const [splitView, setSplitView] = useState<CompositionResult["outputs"]>();
  const active = useRef<string | null>(null);
  const alive = useRef(true);
  const initial = useRef(initialSources);
  const project = view.project;
  const part = project?.state.parts.find(p => p.id === selected);
  const detail = view.parts?.find(p => p.part.id === selected);
  useEffect(() => { alive.current = true; void queryComposition({ action: "list" }, crypto.randomUUID()).then(r => { if (alive.current) setProjects(r.projects ?? []); }).catch(e => { if (alive.current) setError(String(e)); }); return () => { alive.current = false; if (active.current) void cancelComposition(active.current); }; }, []);
  const run = async (intent: CompositionIntent) => {
    if (active.current) return;
    const operation = crypto.randomUUID(); active.current = operation; setPending(true); setError(""); setNotice("");
    try {
      const existing = project && !["create", "open", "list"].includes(intent.action) ? { projectId: project.id, generation: project.generation } : {};
      const r = await queryComposition({ ...existing, ...intent }, operation);
      if (!alive.current || active.current !== operation) return;
      if (r.project) { if (r.project.id !== project?.id) { setPreviewPath(""); setPreviewGeneration(null); setMigration(null); setSplitView(undefined); } setView(r); setReport(null); setSelected(previous => r.project!.state.parts.some(p => p.id === previous) ? previous : r.project!.state.parts[0]?.id ?? ""); }
      if (r.report) setReport(r.report);
      if (r.migrationReports) setMigration(r);
      if (intent.action === "preview" && r.outputPath) { setPreviewPath(r.outputPath); setPreviewGeneration(r.generation ?? null); }
      if (r.outputs) setSplitView(r.outputs);
      if (intent.action === "export" || intent.action === "split") setNotice(intent.action === "split" && intent.splitOutput === "project" ? l("已创建拆分组合，源文件未修改。", "Split composition created. Source files are unchanged.") : l("导出完成，源文件未修改。", "Export complete. Source files are unchanged."));
      if (intent.action === "create" || intent.action === "save" || intent.action === "split") { const list = await queryComposition({ action: "list" }, crypto.randomUUID()); if (alive.current) setProjects(list.projects ?? []); }
    } catch (e) { if (alive.current) setError(String(e)); }
    finally { if (active.current === operation) { active.current = null; if (alive.current) setPending(false); } }
  };
  useEffect(() => { if (initialProjectId) void run({ action: "open", projectId: initialProjectId }); }, [initialProjectId]);
  useEffect(() => { setSplitView(undefined); }, [splitMode, axis, plane, boxes, project?.generation]);
  const add = async () => {
    const chosen = await openDialog({ multiple: true, filters: [{ name: l("投影文件", "Schematic files"), extensions: ["litematic", "nbt", "json", "bg", "bgcp"] }] });
    if (!chosen) return;
    await run({ action: project ? "add" : "create", sourcePaths: typeof chosen === "string" ? [chosen] : chosen, placement });
  };
  const update = (patch: CompositionIntent["patch"]) => part && run({ action: "update", partId: part.id, patch });
  const updateAxis = (group: "offset" | "origin", index: number, value: string) => {
    if (!part || value === "") return;
    const tuple = [...part.transform[group]] as Vec3; tuple[index] = Number(value);
    void update({ transform: { ...part.transform, [group]: tuple } });
  };
  const transform = (patch: Partial<CompositionTransform>) => part && update({ transform: { ...part.transform, ...patch } });
  const exportFile = async () => { const path = await saveDialog({ defaultPath: `${project?.state.name ?? "composition"}.${format === "litematic" ? "litematic" : format.startsWith("building") ? "json" : "nbt"}` }); if (path) await run({ action: "export", format, outputPath: path }); };
  const splitInput = { splitMode, axis, at: Number(plane), boxes };
  const exportSplit = async () => { if (splitOutput === "project") { await run({ action: "split", ...splitInput, splitOutput }); return; } const directory = await openDialog({ directory: true }); if (typeof directory === "string") await run({ action: "split", ...splitInput, splitOutput, outputDirectory: directory }); };
  const removePart = async (partId: string) => {
    if (await confirmDialog(l("从组合中移除此实例？源投影文件不会删除。", "Remove this instance from the composition? The source schematic will not be deleted."), { title: l("移除部件", "Remove part"), kind: "warning" })) await run({ action: "remove", partId });
  };
  const partsPanel = <div className="composition-parts" aria-label={l("部件列表", "Parts")}><h3>{l("部件", "Parts")}</h3>{project?.state.parts.map(p => <div key={p.id} className={`composition-part${selected === p.id ? " is-selected" : ""}`}><button className="composition-part-name" aria-pressed={selected === p.id} onClick={() => setSelected(p.id)}>{p.locked && <Lock size={13} />}{p.name}</button><div className="composition-part-actions"><label><Checkbox disabled={pending || p.locked} checked={p.visible} onCheckedChange={v => void run({ action: "update", partId: p.id, patch: { visible: v === true } })} /><Eye size={13} />{l("显示", "Show")}</label><button disabled={pending || p.locked} aria-label={l("复制实例", "Duplicate instance")} onClick={() => void run({ action: "duplicate", partId: p.id })}><Copy size={14} /></button><button disabled={pending || p.locked} aria-label={l("删除实例", "Remove instance")} onClick={() => void removePart(p.id)}><Trash2 size={14} /></button></div></div>)}{!project && <p className="composition-muted">{l("添加投影开始组合，或打开已保存项目。", "Add schematics to begin, or open a saved project.")}</p>}</div>;
  const propertiesPanel = <div className="composition-inspector"><h3>{l("部件属性", "Part properties")}</h3>{part ? <><label>{l("名称", "Name")}<input key={`${part.id}-name-${project?.generation}`} defaultValue={part.name} disabled={pending || part.locked} onBlur={e => { if (e.target.value !== part.name) void update({ name: e.target.value }); }} /></label><label><Checkbox checked={part.locked} disabled={pending} onCheckedChange={v => void update({ locked: v === true })} />{l("锁定实例", "Lock instance")}</label>{(["offset", "origin"] as const).map(group => <fieldset key={group} disabled={pending || part.locked}><legend>{group === "offset" ? l("位移（整数方块）", "Offset (integer blocks)") : l("局部原点", "Local origin")}</legend><div className="composition-vector">{["X", "Y", "Z"].map((a, i) => <label key={a}>{a}<input type="number" step="1" key={`${part.id}-${group}-${i}-${project?.generation}`} defaultValue={part.transform[group][i]} onBlur={e => { if (Number(e.target.value) !== part.transform[group][i]) updateAxis(group, i, e.target.value); }} /></label>)}</div></fieldset>)}<label>{l("绕 Y 旋转", "Y rotation")}<select value={part.transform.rotation} disabled={pending || part.locked} onChange={e => void transform({ rotation: Number(e.target.value) as CompositionTransform["rotation"] })}>{[0, 90, 180, 270].map(v => <option key={v} value={v}>{v}°</option>)}</select></label><div className="composition-checks"><label><Checkbox checked={part.transform.mirrorX} disabled={pending || part.locked} onCheckedChange={v => void transform({ mirrorX: v === true })} />{l("镜像 X", "Mirror X")}</label><label><Checkbox checked={part.transform.mirrorZ} disabled={pending || part.locked} onCheckedChange={v => void transform({ mirrorZ: v === true })} />{l("镜像 Z", "Mirror Z")}</label></div><label><Checkbox checked={part.clearAir} disabled={pending || part.locked} onCheckedChange={v => void update({ clearAir: v === true })} />{l("显式清空所覆盖区域的空气", "Explicitly clear air in covered selections")}</label><dl><dt>{l("尺寸", "Size")}</dt><dd>{detail?.size?.join(" × ") ?? "—"}</dd><dt>{l("边界", "Bounds")}</dt><dd>{detail?.bounds ? `${detail.bounds.min.join(", ")} → ${detail.bounds.max.join(", ")}` : "—"}</dd><dt>DataVersion</dt><dd>{detail?.dataVersion ?? "—"}</dd></dl><fieldset disabled={pending || part.locked}><legend>{l("源选区（空选表示全部）", "Source regions (none selects all)")}</legend>{detail?.regions.map(name => <label key={name}><Checkbox checked={part.regions.includes(name)} onCheckedChange={v => void update({ regions: v === true ? [...part.regions, name] : part.regions.filter(r => r !== name) })} />{name}</label>)}</fieldset></> : <p className="composition-muted">{l("选择左侧部件编辑。", "Select a part to edit.")}</p>}</div>;
  return <section className="composition-page">
    <header className="composition-header"><div><h2>{l("组合 / 拆分", "Compose / split")}</h2>{project && <input aria-label={l("组合名称", "Composition name")} key={`${project.id}-title-${project.generation}`} defaultValue={project.state.name} disabled={pending} onBlur={e => { if (e.target.value !== project.state.name) void run({ action: "save", name: e.target.value }); }} />}<p>{l("源文件保持只读；每个部件都是独立实例。", "Source files stay read-only. Every part is an independent instance.")}</p></div><select aria-label={l("打开组合", "Open composition")} value={project?.id ?? ""} disabled={pending} onChange={e => void run({ action: "open", projectId: e.target.value })}><option value="">{l("选择已保存组合…", "Choose a saved composition…")}</option>{projects.map(p => <option key={p.id} value={p.id}>{p.state.name}</option>)}</select></header>
    <div className="composition-toolbar">
      {compactParts && <Button variant="outline" aria-expanded={panel === "parts"} onClick={() => setPanel("parts")}>{l("部件列表", "Parts")}</Button>}
      {compactInspector && <Button variant="outline" aria-expanded={panel === "properties"} onClick={() => setPanel("properties")}>{l("部件属性", "Part properties")}</Button>}
      <Button disabled={pending} onClick={() => void add()}><Plus size={16} />{l("添加部件", "Add parts")}</Button>
      <select value={placement} onChange={e => setPlacement(e.target.value as typeof placement)} aria-label={l("添加位置", "Placement")}><option value="original">{l("按原位置", "Original positions")}</option><option value="beside">{l("放在组合旁", "Place beside composition")}</option></select>
      {!project && (currentFile || initial.current.length > 0) && <Button variant="outline" disabled={pending} onClick={() => void run({ action: "create", sourcePaths: initial.current.length ? initial.current : [currentFile], placement })}>{l("加入当前投影", "Add current schematic")}</Button>}
      <Button variant="ghost" disabled={pending || !view.canUndo} onClick={() => void run({ action: "undo" })}><Undo2 size={16} />{l("撤销", "Undo")}</Button><Button variant="ghost" disabled={pending || !view.canRedo} onClick={() => void run({ action: "redo" })}><Redo2 size={16} />{l("重做", "Redo")}</Button>
      <Button variant="outline" disabled={pending || !project} onClick={() => void run({ action: "save" })}><Save size={16} />{l("保存组合", "Save")}</Button>
      <select aria-label={l("导出格式", "Export format")} value={format} onChange={e => setFormat(e.target.value)}><option value="litematic">Litematic</option><option value="structure">Minecraft Structure</option><option value="create">Create</option><option value="anvilCraft">AnvilCraft</option><option value="buildingGadgetsLegacy">Building Gadgets</option><option value="buildingGadgets2">Building Gadgets 2</option></select>
      <Button variant="outline" disabled={pending || !project} onClick={() => void exportFile()}><Download size={16} />{l("导出合并投影…", "Export merged…")}</Button>
      {pending && <Button variant="ghost" onClick={() => { if (active.current) void cancelComposition(active.current); }}><X size={16} />{l("取消", "Cancel")}</Button>}
    </div>
    {error && <p className="composition-error" role="alert">{error}</p>}{notice && <p role="status">{notice}</p>}
    <div className={`composition-workspace${compactInspector ? " is-compact-inspector" : ""}${compactParts ? " is-compact-parts" : ""}`}>
      {!compactParts && partsPanel}

      <main className="composition-canvas"><div className="composition-canvas-toolbar"><Button disabled={pending || !project} onClick={() => void run({ action: "preview" })}><Play size={16} />{l("准备 / 更新组合场景", "Prepare / update composition scene")}</Button><span>{previewGeneration !== null && previewGeneration !== project?.generation ? l("编辑已保存，更新场景后再使用已有渲染入口查看。", "Edits saved. Update the scene, then use the render controls to view it.") : ""}</span></div>{previewPath ? <RenderPage currentFile={previewPath} presentationBlocked={presentationBlocked || !!drawer} /> : <div className="composition-empty"><h3>{l("组合场景", "Composition scene")}</h3><p>{l("添加部件后点击“准备 / 更新组合场景”，使用原有快速、标准或完整模式查看真实组合。移动、切页和保存不会自动重建。", "Add parts and prepare the composition scene, then use the existing Fast, Standard or Full renderer. Editing, navigation and saving never trigger automatic rebuilding.")}</p></div>}</main>
      {!compactInspector && propertiesPanel}

    </div>
    {project && <section className="composition-bottom"><div className="composition-report-controls"><Button variant="outline" disabled={pending} onClick={() => void run({ action: "migrationReport" })}>{l("检查版本兼容", "Check version compatibility")}</Button>{migration?.writable && <Button variant="outline" disabled={pending} onClick={() => void run({ action: "migrate", targetVersion: migration.targetVersion })}>{l("转换到组合版本", "Convert to composition version")}</Button>}<Button variant="outline" disabled={pending} onClick={() => void run({ action: "report" })}>{l("检查合并与冲突", "Check merge and conflicts")}</Button><label>{l("冲突决策", "Conflict policy")}<select value={project.state.conflictPolicy} disabled={pending} onChange={e => void run({ action: "save", conflictPolicy: e.target.value as CompositionProject["state"]["conflictPolicy"] })}><option value="reject">{l("冲突时阻止导出", "Block export on conflicts")}</option><option value="keepFirst">{l("明确保留列表前项", "Keep earlier listed part")}</option><option value="keepLast">{l("明确保留列表后项", "Keep later listed part")}</option></select></label>{report && <span role="status">{l("方块", "Blocks")} {report.blockCount} · {l("冲突", "Conflicts")} {report.collisionCount} · {l("未解决", "Unresolved")} {report.issues.length}</span>}</div>{migration?.migrationReports && <details><summary>{l("版本兼容报告", "Version compatibility report")}</summary><ul>{migration.migrationReports.map(r => <li key={r.partId}>{r.name}: {r.conversion.sourceVersion} → {r.conversion.targetVersion} — {r.conversion.writable ? l("可无损转换", "Lossless conversion available") : l("存在不兼容内容", "Incompatible content") }<ul>{r.conversion.blockedPayloads.map(v => <li key={v}>{v}</li>)}</ul></li>)}</ul></details>}{report && <><ul className="composition-issues">{report.issues.map(v => <li key={v}>{v}</li>)}</ul>{report.collisions.length > 0 && <details><summary>{l("冲突坐标（前200项）", "Conflict coordinates (first 200)")}</summary><ul>{report.collisions.map((v, i) => <li key={i}>{v.position.join(", ")} — {v.keptInstance} / {v.incomingInstance}</li>)}</ul></details>}</>}<details className="composition-split"><summary><Scissors size={15} />{l("拆分投影", "Split schematic")}</summary><div className="composition-split-controls"><select aria-label={l("拆分方式", "Split mode")} value={splitMode} onChange={e => { setSplitMode(e.target.value as typeof splitMode); setSplitView(undefined); }}><option value="regions">{l("按原选区", "Original regions")}</option><option value="plane">{l("沿平面切成两份", "Cut along a plane")}</option><option value="boxes">{l("多个XYZ范围", "Multiple XYZ bounds")}</option></select>{splitMode === "plane" && <><select value={axis} onChange={e => setAxis(e.target.value as typeof axis)} aria-label={l("切割轴", "Cut axis")}><option>x</option><option>y</option><option>z</option></select><input type="number" step="1" value={plane} onChange={e => setPlane(e.target.value)} aria-label={l("切割位置", "Cut position")} /></>}{splitMode === "boxes" && <Button variant="outline" onClick={() => setBoxes(previous => [...previous, { name: `part_${previous.length + 1}`, bounds: { min: [0, 0, 0], max: [15, 15, 15] } }])}>{l("添加范围", "Add bounds")}</Button>}<Button variant="outline" disabled={pending} onClick={() => void run({ action: "splitReport", ...splitInput })}>{l("预览拆分清单", "Review split outputs")}</Button><select aria-label={l("拆分输出", "Split output")} value={splitOutput} onChange={e => setSplitOutput(e.target.value as typeof splitOutput)}><option value="files">{l("逐份文件", "Separate files")}</option><option value="project">{l("创建组合项目", "Create composition project")}</option></select><Button disabled={pending || !splitView} onClick={() => void exportSplit()}>{splitOutput === "project" ? l("创建拆分组合", "Create split composition") : l("逐份导出…", "Export parts…")}</Button></div>{splitMode === "boxes" && boxes.map((box, i) => <div className="composition-box" key={i}><input aria-label={l("输出名称", "Output name")} value={box.name} onChange={e => setBoxes(previous => previous.map((b, n) => n === i ? { ...b, name: e.target.value } : b))} />{(["min", "max"] as const).map(edge => <div className="composition-vector" key={edge}><span>{edge}</span>{box.bounds[edge].map((v, a) => <input key={a} type="number" step="1" aria-label={`${edge} ${"XYZ"[a]}`} value={v} onChange={e => setBoxes(previous => previous.map((b, n) => n === i ? { ...b, bounds: { ...b.bounds, [edge]: b.bounds[edge].map((old, axis) => axis === a ? Number(e.target.value) : old) as Vec3 } } : b))} />)}</div>)}<button aria-label={l("删除范围", "Remove bounds")} onClick={() => setBoxes(previous => previous.filter((_, n) => n !== i))}><Trash2 size={15} /></button></div>)}{splitView && <table><thead><tr><th>{l("文件", "File")}</th><th>{l("范围", "Bounds")}</th><th>{l("方块 / 实体 / 方块实体", "Blocks / entities / block entities")}</th></tr></thead><tbody>{splitView.map(o => <tr key={o.name}><td>{o.name}.litematic</td><td>{o.bounds.min.join(", ")} → {o.bounds.max.join(", ")}</td><td>{o.report.blockCount} / {o.report.entityCount} / {o.report.blockEntityCount}</td></tr>)}</tbody></table>}</details></section>}
    {drawer && <Dialog title={panel === "parts" ? l("部件列表", "Parts") : l("部件属性", "Part properties")} width="sm" className="composition-drawer" onClose={() => setPanel(null)}>{panel === "parts" ? partsPanel : propertiesPanel}</Dialog>}
  </section>;
}
