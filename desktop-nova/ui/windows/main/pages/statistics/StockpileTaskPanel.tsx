import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Copy, Download, FilePlus, Info, Pencil, RotateCcw, Settings, Upload } from "lucide-react";
import {
  commandStockpileTask, getStockpileTask, displayFilePath, openDialog, saveDialog, writeTextFileAbsolute,
  resolveBlockDisplayName, getUiLocale, t, type ApplicationMaterialScope,
} from "../../../../../src/business/facade";
import { listenEvent } from "../../../../../src/platform/events";
import { useI18n } from "../../../../i18n";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Input } from "../../../../components/ui/input";
import { Textarea } from "../../../../components/ui/textarea";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../../components/ui/table";
import { Dropdown } from "../../../../components/Dropdown";
import { Dialog } from "../../../../components/Dialog";
import { VirtualSpacerCell } from "../../../../components/VirtualSpacerCell";
import { useVirtualWindow } from "../../../../components/useVirtualWindow";
import { readUpdateDraft, saveUpdateDraft, useUpdateDraft } from "../../updateRecovery";
import { getApplicationWindowLabel } from "../../../../../src/platform/nativeWindowGeometry";

type Workspace = Awaited<ReturnType<typeof getStockpileTask>>;
type Task = NonNullable<Workspace["task"]>;
type TaskItem = Task["items"][number];
type Command = Parameters<typeof commandStockpileTask>[0];
type Mutation = NonNullable<Command["mutation"]>;
type Preferences = Task["preferences"];
type Material = Task["materials"]["materials"][number];
type RecipeNode = NonNullable<Task["recipeTrees"]>[string];
type WriteQueue = { epoch: number; task: Task; pending: Map<string, Command>; running: boolean; refresh: boolean };
type Confirmation = { taskId: string; sourceRevision: string; name: string; scope: string };
type PendingDraft = { taskId: string; projectId: string; sourceRevision: string; commands: Array<[string, Command]> };

const defaultPreferences: Preferences = {
  search: "", category: "all", filter: "all", sort: "grouped", view: "compact", individualCounts: false,
  showAdvancedRecipeTree: true, showUnresolvedRecipes: true, showIconFallbackBadge: true, language: "auto",
};

/** The existing single-user web hierarchy: 27 stacks per box, 27 boxes per chest. */
export function stockpileUnitParts(value: number, stackSize: number): [number, number, number, number] {
  let remaining = Math.max(0, Math.trunc(value));
  const stack = Math.max(1, Math.trunc(stackSize));
  const box = stack * 27, chest = box * 27;
  const chests = Math.floor(remaining / chest); remaining %= chest;
  const boxes = Math.floor(remaining / box); remaining %= box;
  const stacks = Math.floor(remaining / stack); remaining %= stack;
  return [chests, boxes, stacks, remaining];
}

function quantityText(value: number, stackSize: number, individual: boolean): string {
  if (individual) return value.toLocaleString();
  const parts = stockpileUnitParts(value, stackSize);
  const keys = ["stockpile.unitChest", "stockpile.unitBox", "stockpile.unitStack", "stockpile.unitEach"];
  return parts.map((count, index) => count || index === 3 && !parts.some(Boolean) ? t(keys[index], { count }) : "").filter(Boolean).join(" ");
}

function itemState(item: TaskItem): string {
  if (item.overfilledCount > 0) return "overfilled";
  if (item.complete) return "done";
  if (item.doneCount > 0) return "partial_done";
  return item.status === "preparing" ? "preparing" : "not_started";
}

function displayLanguage(language: string): string { return language === "zh-CN" || language === "en-US" ? language : getUiLocale(); }

function materialName(material: Material | undefined, language: string, id: string): string {
  return material?.display_names?.[displayLanguage(language)] || resolveBlockDisplayName(material?.item_icon_key || id.split("#")[0], material?.display_name || id);
}

function snapshotName(task: Task, id: string, language: string, fallback?: string): string {
  return task.itemNames?.[id]?.[displayLanguage(language)] || resolveBlockDisplayName(id, fallback || id);
}

function StockpileIcon({ task, iconKey }: { task: Task; iconKey: string }) {
  const src = task.iconImages?.[iconKey];
  return src ? <img className="block-icon-image" src={src} alt="" /> : <BlockIcon blockId={iconKey} lookupMode="item_first" />;
}

export function filterStockpileRows(items: TaskItem[], metadata: Map<string, Material>, preferences: Preferences): TaskItem[] {
  const query = preferences.search.trim().toLocaleLowerCase();
  const rows = items.filter(item => {
    const material = metadata.get(item.materialId);
    if (preferences.category !== "all" && material?.category !== preferences.category) return false;
    const state = itemState(item);
    switch (preferences.filter) {
      case "not_started": if (state !== "not_started") return false; break;
      case "preparing": if (state !== "preparing" && state !== "partial_done") return false; break;
      case "done": if (!item.complete) return false; break;
      case "remaining": if (item.remainingCount <= 0) return false; break;
      case "overfilled": if (item.overfilledCount <= 0) return false; break;
      case "stalled": if (item.preparingCount <= 0 || item.complete) return false; break;
      case "noted": if (!item.publicNote?.trim()) return false; break;
      case "stored": if (!item.storageLocation?.trim()) return false; break;
      case "available": case "unresolved": case "missing": if (material?.recipe_status !== preferences.filter) return false; break;
    }
    return !query || [materialName(material, preferences.language, item.materialId), material?.display_name, material?.namespace_id,
      material?.category, material?.item_icon_key, item.materialId].join(" ").toLocaleLowerCase().includes(query);
  });
  const byName = (left: TaskItem, right: TaskItem) => materialName(metadata.get(left.materialId), preferences.language, left.materialId)
    .localeCompare(materialName(metadata.get(right.materialId), preferences.language, right.materialId), displayLanguage(preferences.language));
  return rows.sort((left, right) => {
    switch (preferences.sort) {
      case "count_asc": return left.requiredCount - right.requiredCount || byName(left, right);
      case "count_desc": return right.requiredCount - left.requiredCount || byName(left, right);
      case "remaining_desc": return right.remainingCount - left.remainingCount || byName(left, right);
      case "status": return itemState(left).localeCompare(itemState(right)) || byName(left, right);
      case "grouped": return (metadata.get(left.materialId)?.category || "").localeCompare(metadata.get(right.materialId)?.category || "")
        || itemState(left).localeCompare(itemState(right)) || right.requiredCount - left.requiredCount || byName(left, right);
      default: return byName(left, right);
    }
  });
}

export function resetConfirmationMatches(confirmation: Confirmation, task: Pick<Task, "taskId" | "sourceRevision"> | undefined): boolean {
  return task?.taskId === confirmation.taskId && task.sourceRevision === confirmation.sourceRevision;
}

export function mergeStockpileMutation(previous: Mutation | undefined, next: Mutation): Mutation {
  if (next.clear || next.reset) return next;
  const { clear: _clear, reset: _reset, ...retained } = previous || {};
  return { ...retained, ...next };
}

function taskScopeLabel(task: Task): string {
  const scope = task.context.scope;
  const scopeName = scope?.kind === "region" ? t("statistics.regionScope", { name: scope.value })
    : scope?.kind === "layer" ? t("stockpile.layerScope", { layer: scope.value }) : t("statistics.all");
  return `${scopeName} · ${t("stockpile.copiesValue", { count: task.multiplier })}${task.context.includeContainerItems ? ` · ${t("statistics.includeContainers")}` : ""}`;
}

export function recoverableStockpileCommands(commands: Array<[string, Command]>): Array<[string, Command]> {
  return commands.filter(([, command]) => command.action === "settings" || command.action === "mutate" && !command.mutation?.reset && !command.mutation?.clear);
}

function pendingDraft(work: WriteQueue): PendingDraft | undefined {
  const commands = recoverableStockpileCommands([...work.pending]);
  return commands.length ? { taskId: work.task.taskId, projectId: work.task.projectId, sourceRevision: work.task.sourceRevision, commands } : undefined;
}

function draftKey(taskId: string): string { return `stockpile.pending:${getApplicationWindowLabel()}:${taskId}`; }

function QuantityInput({ value, label, onQuantity }: { value: number; label: string; onQuantity: (value: number) => void }) {
  const [draft, setDraft] = useState(String(value));
  const focused = useRef(false);
  const submitted = useRef(value);
  useEffect(() => { if (!focused.current) { setDraft(String(value)); submitted.current = value; } }, [value]);
  return <Input className="stockpile-quantity-input" type="number" min={0} step={1} inputMode="numeric" aria-label={label} value={draft}
    onFocus={() => { focused.current = true; }} onChange={event => {
      const text = event.target.value; setDraft(text);
      if (!/^\d+$/.test(text)) return;
      const quantity = Number(text);
      if (!Number.isSafeInteger(quantity) || quantity < 0) return;
      submitted.current = quantity; onQuantity(quantity);
    }} onBlur={() => { focused.current = false; setDraft(String(submitted.current)); }}
    onKeyDown={event => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") { event.stopPropagation(); event.currentTarget.blur(); } }} />;
}

function RecipeTree({ node, task, preferences, depth = 0 }: { node: RecipeNode; task: Task; preferences: Preferences; depth?: number }) {
  const [open, setOpen] = useState(depth < 2);
  const ingredients = node.ingredients || [];
  const possible = node.possible_items || [];
  return <div className="stockpile-recipe-node">
    <div className="stockpile-recipe-heading"><StockpileIcon task={task} iconKey={node.icon_key || node.item_id} />
      <span><strong>{snapshotName(task, node.item_id, preferences.language, node.display_name)}</strong><small>{node.item_id}</small></span>
      {!!node.children?.length && <Button variant="ghost" size="sm" aria-expanded={open} onClick={() => setOpen(!open)}>{t(open ? "stockpile.collapse" : "stockpile.expand")}</Button>}
    </div>
    <dl className="stockpile-detail-facts">
      <div><dt>{t("stockpile.recipeType")}</dt><dd>{node.recipe_type}</dd></div>
      <div><dt>{t("stockpile.process")}</dt><dd>{t(`stockpile.process.${node.process_type}`) === `stockpile.process.${node.process_type}` ? node.process_type : t(`stockpile.process.${node.process_type}`)}</dd></div>
      <div><dt>{t("stockpile.recipeQuantity")}</dt><dd>{node.needed_count}</dd></div>
      <div><dt>{t("stockpile.recipeOutput")}</dt><dd>{node.output_count}</dd></div>
      <div><dt>{t("stockpile.recipeBatches")}</dt><dd>{node.batch_count}</dd></div>
      <div><dt>{t("stockpile.recipeExtra")}</dt><dd>{node.extra_output}</dd></div>
      <div><dt>{t("stockpile.recipeDepth")}</dt><dd>{node.depth}</dd></div>
    </dl>
    {!!ingredients.length && <ul className="stockpile-recipe-ingredients">{ingredients.map((ingredient, index) => <li key={`${ingredient.item_id}:${index}`}>
      <StockpileIcon task={task} iconKey={ingredient.icon_key || ingredient.item_id} />{snapshotName(task, ingredient.item_id, preferences.language, ingredient.display_name)} × {ingredient.needed_count}
      {ingredient.tag && <span> #{ingredient.tag}</span>}{preferences.showUnresolvedRecipes && ingredient.unresolved && <span> · {ingredient.unresolved_reason}</span>}
      {!!ingredient.possible_items?.length && <span> · {ingredient.possible_items.slice(0, 12).map(id => snapshotName(task, id, preferences.language)).join(", ")}{ingredient.possible_items.length > 12 && ` (+${ingredient.possible_items.length - 12})`}</span>}
    </li>)}</ul>}
    {node.tag && <p>#{node.tag}</p>}
    {!!possible.length && <p className="muted">{possible.slice(0, 12).map(id => snapshotName(task, id, preferences.language)).join(", ")}{possible.length > 12 && ` (+${possible.length - 12})`}</p>}
    {preferences.showUnresolvedRecipes && node.unresolved && <p className="muted">{t("stockpile.unresolvedReason", { reason: node.unresolved_reason || "unresolved" })}</p>}
    {node.requires_fuel && <p>{t("stockpile.fuelRequired")}{node.fuel_estimate != null && ` · ${node.fuel_estimate}`}</p>}
    {node.decorative_smithing && <p>{t("stockpile.decorativeSmithing")}</p>}
    {open && node.children?.map(child => <RecipeTree key={child.node_id} node={child} task={task} preferences={preferences} depth={depth + 1} />)}
  </div>;
}

export function StockpileTaskPanel({ sourcePath, scope = { kind: "all", value: null }, includeContainerItems = false, multiplier = 1, onPendingChange }: {
  sourcePath: string; scope?: ApplicationMaterialScope; includeContainerItems?: boolean; multiplier?: number; onPendingChange?: (pending: boolean) => void;
}) {
  const { locale } = useI18n();
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [preferences, setPreferences] = useState<Preferences>(defaultPreferences);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [pending, setPending] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [nameDialog, setNameDialog] = useState<"create" | "rename" | null>(null);
  const [nameDraft, setNameDraft] = useState("");
  const [copiesDraft, setCopiesDraft] = useState("1");
  const [contextDraft, setContextDraft] = useState("all");
  const [containersDraft, setContainersDraft] = useState(false);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [legacyOpen, setLegacyOpen] = useState(false);
  const [legacyId, setLegacyId] = useState("");
  const [recovery, setRecovery] = useState<PendingDraft | null>(null);
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const [detailId, setDetailId] = useState<string | null>(null);
  const [searchDraft, setSearchDraft] = useState("");
  const composing = useRef(false);
  const preferencesRef = useRef(preferences);
  const searchRef = useRef(searchDraft);
  preferencesRef.current = preferences; searchRef.current = searchDraft;
  const queue = useRef<WriteQueue | null>(null);
  const epoch = useRef(0);
  const sequence = useRef(0);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const currentTask = useRef<Task | undefined>(undefined);
  const currentSource = useRef(sourcePath);
  currentSource.current = sourcePath;
  const task = workspace?.task || undefined;
  currentTask.current = task;
  const capturePending = () => {
    const work = queue.current;
    if (!work) return undefined;
    if (recovery?.taskId === work.task.taskId) return recovery;
    const draft = pendingDraft(work);
    if (searchRef.current === preferencesRef.current.search) return draft;
    const commands = new Map(draft?.commands || []);
    commands.set("settings", { action: "settings", preferences: { ...preferencesRef.current, search: searchRef.current } });
    return { taskId: work.task.taskId, projectId: work.task.projectId, sourceRevision: work.task.sourceRevision, commands: [...commands] };
  };
  const persistPending = (work: WriteQueue) => saveUpdateDraft(draftKey(work.task.taskId), work === queue.current ? capturePending() : pendingDraft(work));
  useUpdateDraft(task ? draftKey(task.taskId) : "stockpile.pending:inactive", capturePending);

  const accept = (next: Workspace, preserveInput = false) => {
    const changedTask = next.task?.taskId !== currentTask.current?.taskId;
    setWorkspace(next); currentTask.current = next.task || undefined;
    if (next.task && (changedTask || !preserveInput)) {
      const nextPreferences = { ...defaultPreferences, ...next.task.preferences,
        view: next.task.preferences.view === "comfortable" ? "comfortable" : "compact" };
      setPreferences(nextPreferences); setSearchDraft(nextPreferences.search);
      preferencesRef.current = nextPreferences; searchRef.current = nextPreferences.search;
      setCopiesDraft(String(next.task.multiplier));
      const context = next.task.context as { scope?: ApplicationMaterialScope; includeContainerItems?: boolean } | undefined;
      setContextDraft(context?.scope?.kind === "region" ? `region:${context.scope.value}` : context?.scope?.kind === "layer" ? `layer:${context.scope.value}` : "all");
      setContainersDraft(context?.includeContainerItems === true);
      if (changedTask) { setDetailId(null); setRecovery(null); }
    }
  };
  const load = async () => {
    const expected = ++epoch.current, request = ++sequence.current;
    setLoading(true); setError(""); setStatus(""); setConfirmation(null);
    try {
      const next = await getStockpileTask({ sourcePath: currentSource.current || undefined, scope, includeContainerItems, multiplier });
      if (expected !== epoch.current || request !== sequence.current) return;
      queue.current = next.task ? { epoch: expected, task: next.task, pending: new Map(), running: false, refresh: false } : null;
      setPending(0); accept(next);
      if (next.task && queue.current) restorePending(queue.current);
    } catch (failure) { if (expected === epoch.current) { if (queue.current) queue.current.epoch = expected; setError(String(failure)); } }
    finally { if (expected === epoch.current) setLoading(false); }
  };
  const loadRef = useRef(load); loadRef.current = load;
  useEffect(() => { void loadRef.current(); return () => { epoch.current += 1; sequence.current += 1; }; }, [sourcePath]);
  const searchPending = searchDraft !== preferences.search;
  useEffect(() => { onPendingChange?.(pending > 0 || searchPending); }, [pending, searchPending, onPendingChange]);

  const drain = async (work: WriteQueue) => {
    if (work.running) return;
    work.running = true;
    if (work.epoch === epoch.current) { setError(""); setPending(work.pending.size); }
    try {
      while (work.pending.size) {
        const [key, command] = work.pending.entries().next().value as [string, Command];
        const next = await commandStockpileTask({ ...command, projectId: work.task.projectId, taskId: work.task.taskId, sourceRevision: work.task.sourceRevision });
        if (!next.task || next.task.taskId !== work.task.taskId) throw new Error(t("stockpile.taskChanged"));
        if (work.pending.get(key) === command) work.pending.delete(key);
        work.task = next.task;
        persistPending(work);
        if (work.epoch === epoch.current) { sequence.current += 1; accept(next, true); setPending(work.pending.size); }
      }
      if (work.refresh && work.epoch === epoch.current) {
        work.refresh = false;
        const next = await commandStockpileTask({ action: "select", projectId: work.task.projectId, taskId: work.task.taskId });
        if (work.epoch === epoch.current) { accept(next, true); if (next.task) work.task = next.task; }
      }
    } catch (failure) { if (work.epoch === epoch.current) setError(String(failure)); }
    finally { work.running = false; if (work.epoch === epoch.current) setPending(work.pending.size); }
  };
  const enqueue = (key: string, command: Command) => {
    const work = queue.current;
    if (!work || work.task.taskId !== currentTask.current?.taskId) return;
    if (recovery?.taskId === work.task.taskId) { setRecoveryOpen(true); return; }
    const previous = work.pending.get(key);
    work.pending.set(key, command.mutation ? { ...command, mutation: mergeStockpileMutation(previous?.mutation, command.mutation) } : command);
    setPending(work.pending.size);
    try { persistPending(work); void drain(work); } catch (failure) { setError(String(failure)); }
  };
  const mutate = (materialId: string, mutation: Mutation) => enqueue(`item:${materialId}`, { action: "mutate", mutation: { ...mutation, materialId } });
  const changeQuantity = (materialId: string, quantity: number) => {
    const work = queue.current;
    const item = work?.task.items.find(entry => entry.materialId === materialId);
    if (!item || !Number.isSafeInteger(quantity) || quantity < 0) return;
    mutate(materialId, { quantity, status: item.status === "done" ? "done" : "preparing" });
  };
  const retry = async () => {
    const work = queue.current;
    if (work?.pending.size) {
      if (work.running) return;
      try {
        const next = await commandStockpileTask({ action: "select", projectId: work.task.projectId, taskId: work.task.taskId });
        if (work !== queue.current || !next.task || next.task.taskId !== work.task.taskId) return;
        if (next.task.sourceRevision !== work.task.sourceRevision) {
          const draft = pendingDraft(work);
          work.pending.clear(); work.task = next.task;
          accept(next); setPending(0); setError("");
          // A changed snapshot needs an explicit user decision before rebasing edits.
          // Destructive resets/clears are never restored across revisions.
          if (draft) { setRecovery(draft); setRecoveryOpen(true); }
          else { saveUpdateDraft(draftKey(work.task.taskId), undefined); setError(t("stockpile.taskChanged")); }
          return;
        }
        persistPending(work); void drain(work);
      } catch (failure) { setError(String(failure)); }
      return;
    }
    void loadRef.current();
  };
  const action = async (command: Command, showLoading = true) => {
    const work = queue.current;
    if (work?.pending.size || work?.running) return;
    const expected = ++epoch.current;
    if (showLoading) setLoading(true);
    setError(""); setStatus("");
    try {
      const next = await commandStockpileTask({ projectId: task?.projectId, taskId: task?.taskId, sourceRevision: task?.sourceRevision, ...command });
      if (expected !== epoch.current) return;
      queue.current = next.task ? { epoch: expected, task: next.task, pending: new Map(), running: false, refresh: false } : null;
      setPending(0); accept(next, command.action === "details" || command.action === "export");
      if (next.task && queue.current) restorePending(queue.current);
    } catch (failure) { if (expected === epoch.current) { if (queue.current) queue.current.epoch = expected; setError(String(failure)); } }
    finally { if (expected === epoch.current) setLoading(false); }
  };
  const restorePending = (work: WriteQueue) => {
    const draft = readUpdateDraft<PendingDraft>(draftKey(work.task.taskId));
    if (!draft?.commands.length || draft.taskId !== work.task.taskId || draft.projectId !== work.task.projectId) return;
    const commands = recoverableStockpileCommands(draft.commands);
    if (draft.sourceRevision !== work.task.sourceRevision) { setRecovery({ ...draft, commands }); return; }
    for (const [key, command] of commands) work.pending.set(key, command);
    const savedPreferences = work.pending.get("settings")?.preferences;
    if (savedPreferences) {
      const next = { ...defaultPreferences, ...work.task.preferences, ...savedPreferences };
      preferencesRef.current = next; searchRef.current = next.search; setPreferences(next); setSearchDraft(next.search);
    }
    setPending(work.pending.size); if (work.pending.size) void drain(work);
  };
  const refreshRef = useRef<() => void>(() => {});
  refreshRef.current = () => { const work = queue.current; if (!work) return; if (work.pending.size || work.running) work.refresh = true;
    else void action({ action: "select", taskId: work.task.taskId, projectId: work.task.projectId }, false); };
  useEffect(() => {
    const unlisten = listenEvent<{ taskId: string }>("stockpile-task-changed", event => {
      if (event.payload.taskId === currentTask.current?.taskId) refreshRef.current();
    }).catch(() => undefined);
    return () => { void unlisten.then(stop => stop?.()); };
  }, []);
  useEffect(() => {
    if (composing.current) return;
    const timer = window.setTimeout(() => {
      if (searchDraft !== preferencesRef.current.search) preference("search", searchDraft);
    }, 180);
    return () => window.clearTimeout(timer);
  }, [searchDraft, preferences.search]);
  const preference = <K extends keyof Preferences>(key: K, value: Preferences[K]) => {
    const next = { ...preferencesRef.current, [key]: value };
    preferencesRef.current = next; setPreferences(next);
    enqueue("settings", { action: "settings", preferences: next });
  };
  const editSearch = (value: string) => {
    searchRef.current = value; setSearchDraft(value);
    const work = queue.current;
    if (work && !recovery) try { saveUpdateDraft(draftKey(work.task.taskId), capturePending()); } catch (failure) { setError(String(failure)); }
  };
  const metadata = useMemo(() => new Map(task?.materials.materials.map(material => [material.id, material]) || []), [task?.materials]);
  const rows = useMemo(() => filterStockpileRows(task?.items || [], metadata, preferences), [task?.items, metadata, preferences]);
  const rowHeight = preferences.view === "comfortable" ? 64 : 50;
  const virtual = useVirtualWindow(rows, scrollRef, rowHeight, 8);
  useLayoutEffect(() => { if (scrollRef.current) scrollRef.current.scrollTop = 0; }, [preferences.search, preferences.category, preferences.filter, preferences.sort, task?.taskId]);
  const categories = useMemo(() => [...new Set(task?.materials.materials.map(material => material.category) || [])].sort(), [task?.materials]);
  const detailItem = task?.items.find(item => item.materialId === detailId);
  const detailMaterial = detailId ? metadata.get(detailId) : undefined;
  const detailTree = detailId ? task?.recipeTrees?.[detailId] || (detailMaterial ? task?.recipeTrees?.[detailMaterial.namespace_id] : undefined) : undefined;
  const busy = loading || pending > 0 || searchPending;
  const sourceLabel = task?.sourcePath.split(/[\\/]/).pop() || sourcePath.split(/[\\/]/).pop() || t("stockpile.noProjection");
  const scopeLabel = task ? taskScopeLabel(task) : "";
  const percent = task && task.requiredCount > 0 ? Math.min(100, task.preparedCount / task.requiredCount * 100) : task ? 100 : 0;
  const displayQuantity = (item: TaskItem, quantity: number) => quantityText(quantity, item.stackSize, preferences.individualCounts);

  const copy = async (kind: "remaining" | "overfilled" | "prepared") => {
    if (!task) return;
    const lines = [`# ${task.name}`, sourceLabel, ""];
    for (const item of task.items) {
      const count = kind === "remaining" ? item.remainingCount : kind === "overfilled" ? item.overfilledCount : item.quantity;
      if (count <= 0) continue;
      const material = metadata.get(item.materialId);
      lines.push(`- ${materialName(material, preferences.language, item.materialId)} (${material?.namespace_id || item.materialId}) ${displayQuantity(item, count)} (${count}) [${t(`stockpile.state.${itemState(item)}`)}]`);
    }
    try { await navigator.clipboard.writeText(lines.join("\n")); setStatus(t("stockpile.copied")); }
    catch (failure) { setError(String(failure)); }
  };
  const exportList = async () => {
    if (!task) return;
    const path = await saveDialog({ defaultPath: `${task.name}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (!path) return;
    const cell = (value: string | number) => `"${String(value).replace(/"/g, '""')}"`;
    const lines = [[t("statistics.material"), "ID", t("stockpile.required"), t("stockpile.prepared"), t("stockpile.remainingQuantity"), t("stockpile.overfilled"), t("stockpile.note"), t("stockpile.storage")].map(cell).join(",")];
    for (const item of rows) lines.push([materialName(metadata.get(item.materialId), preferences.language, item.materialId), item.materialId,
      item.requiredCount, item.doneCount, item.remainingCount, item.overfilledCount, item.publicNote || "", item.storageLocation || ""].map(cell).join(","));
    try { await writeTextFileAbsolute(path, `\uFEFF${lines.join("\r\n")}\r\n`); setStatus(t("statistics.exported")); }
    catch (failure) { setError(String(failure)); }
  };
  const backup = async (restore: boolean) => {
    try {
      if (restore) {
        const path = await openDialog({ multiple: false, filters: [{ name: "JSON", extensions: ["json"] }] });
        if (!path || Array.isArray(path)) return;
        await action({ action: "import", inputPath: path });
      } else if (task) {
        const path = await saveDialog({ defaultPath: `${task.name}.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
        if (path) await action({ action: "export", outputPath: path });
      }
    } catch (failure) { setError(String(failure)); }
  };
  const showDetails = (id: string) => {
    setDetailId(id);
    if (preferences.showAdvancedRecipeTree && !task?.recipeTrees?.[id]) void action({ action: "details", materialId: id }, false);
  };

  return <section className={`stockpile-task stockpile-density-${preferences.view}`} aria-busy={loading}>
    <div className="stockpile-task-toolbar">
      <Dropdown value={task?.taskId || ""} options={(workspace?.tasks || []).map(summary => ({ value: summary.taskId, label: summary.name }))}
        disabled={busy} onChange={taskId => { const summary = workspace?.tasks.find(entry => entry.taskId === taskId); if (summary) void action({ action: "select", taskId, projectId: summary.projectId, sourcePath: currentSource.current || undefined }); }} ariaLabel={t("stockpile.selectTask")} />
      <Button size="sm" disabled={busy || !sourcePath && !task} onClick={() => { setNameDraft(task?.name || sourcePath.split(/[\\/]/).pop()?.replace(/\.litematic$/i, "") || ""); setNameDialog("create"); }}><FilePlus size={16} />{t("stockpile.create")}</Button>
      <Button size="sm" variant="ghost" disabled={busy || !task} onClick={() => { setNameDraft(task?.name || ""); setNameDialog("rename"); }}><Pencil size={16} />{t("stockpile.rename")}</Button>
      <span className="stockpile-source muted" title={displayFilePath(task?.sourcePath || sourcePath)}>{sourceLabel}</span>
      <Button size="sm" variant="ghost" aria-pressed={settingsOpen} disabled={!task} onClick={() => setSettingsOpen(!settingsOpen)}><Settings size={16} />{t("stockpile.settings")}</Button>
      <Button size="sm" variant="outline" disabled={busy || !task || !!recovery || !!error} onClick={() => task && setConfirmation({ taskId: task.taskId, sourceRevision: task.sourceRevision, name: task.name, scope: scopeLabel })}><RotateCcw size={16} />{t("stockpile.reset")}</Button>
    </div>
    <div className="stockpile-summary" aria-live="polite">
      <span>{t("stockpile.quantityProgress", { prepared: task?.preparedCount.toLocaleString(locale) || "0", required: task?.requiredCount.toLocaleString(locale) || "0", remaining: task?.remainingCount.toLocaleString(locale) || "0" })}</span>
      <span>{t("stockpile.itemProgress", { complete: task?.completedItems.toLocaleString(locale) || "0", total: task?.totalItems.toLocaleString(locale) || "0" })}</span>
      <span>{t("stockpile.preparingTotal", { count: task?.preparingCount.toLocaleString(locale) || "0", overfilled: task?.overfilledCount.toLocaleString(locale) || "0" })}</span>
      <progress value={percent} max={100} aria-label={t("stockpile.progress")} />
    </div>
    <div className="stockpile-filters beta-material-options">
      <Input type="search" aria-label={t("statistics.searchDetails")} placeholder={t("editor.nameSearchHint")} value={searchDraft}
        onCompositionStart={() => { composing.current = true; }} onCompositionEnd={event => { composing.current = false; const value = event.currentTarget.value; editSearch(value); preference("search", value); }} onChange={event => editSearch(event.target.value)} />
      <Dropdown value={preferences.category} options={[{ label: t("stockpile.allCategories"), value: "all" }, ...categories.map(category => ({ label: category, value: category }))]} onChange={value => preference("category", value)} ariaLabel={t("stockpile.category")} />
      <Dropdown value={preferences.filter} options={["all", "remaining", "not_started", "preparing", "done", "overfilled", "stalled", "noted", "stored", "available", "unresolved", "missing"].map(value => ({ value, label: t(`stockpile.filter.${value}`) }))} onChange={value => preference("filter", value)} ariaLabel={t("stockpile.filter")} />
      <Dropdown value={preferences.sort} options={["grouped", "count_desc", "count_asc", "remaining_desc", "status", "name"].map(value => ({ value, label: t(`stockpile.sort.${value}`) }))} onChange={value => preference("sort", value)} ariaLabel={t("stockpile.sort")} />
      <label className="beta-material-check"><Checkbox checked={preferences.individualCounts} onCheckedChange={checked => preference("individualCounts", checked === true)} />{t("statistics.individual")}</label>
    </div>
    <div className="stockpile-list-actions">
      <Button size="sm" variant="ghost" disabled={!task} onClick={() => void copy("remaining")}><Copy size={15} />{t("stockpile.copyRemaining")}</Button>
      <Button size="sm" variant="ghost" disabled={!task} onClick={() => void copy("prepared")}>{t("stockpile.copyPrepared")}</Button>
      <Button size="sm" variant="ghost" disabled={!task} onClick={() => void copy("overfilled")}>{t("stockpile.copyOverfilled")}</Button>
      <Button size="sm" variant="ghost" disabled={!task} onClick={() => void exportList()}><Download size={15} />{t("stockpile.exportList")}</Button>
      <span className="stockpile-save-status muted" role="status">{error || recovery ? t("stockpile.notSaved") : pending || searchPending ? t("editor.saving") : loading ? t("editor.loading") : status || (task ? t("stockpile.saved") : "")}</span>
    </div>
    {error && <div className="subwindow-error stockpile-error" role="alert"><span>{t("stockpile.failed", { error })}</span><Button size="sm" onClick={() => void retry()}>{t("stockpile.retry")}</Button></div>}
    {task?.sourceStatus && task.sourceStatus !== "current" && <div className="stockpile-context-notice" role="status">{t(task.sourceStatus === "changed" ? "stockpile.sourceChanged" : "stockpile.sourceMissing")}</div>}
    {recovery && <div className="stockpile-context-notice" role="status"><span>{t("stockpile.pendingRecovery")}</span><Button size="sm" variant="outline" onClick={() => setRecoveryOpen(true)}>{t("stockpile.reviewRecovery")}</Button></div>}
    {!!workspace?.legacyTasks?.length && <div className="stockpile-context-notice"><span>{t("stockpile.legacyRecords", { count: workspace.legacyTasks.length })}</span><Button size="sm" disabled={busy} onClick={() => { setLegacyId(workspace.legacyTasks[0].taskId); setLegacyOpen(true); }}>{t("stockpile.bindLegacy")}</Button></div>}
    {!loading && !task && <div className="stockpile-empty"><p>{t("stockpile.noTasks")}</p><Button disabled={!sourcePath} onClick={() => { setNameDraft(sourceLabel); setNameDialog("create"); }}>{t("stockpile.create")}</Button><Button variant="ghost" onClick={() => void backup(true)}>{t("stockpile.importBackup")}</Button></div>}
    {settingsOpen && task && <section className="stockpile-settings nova-box-plain">
      <div className="stockpile-settings-grid">
        <label>{t("statistics.copies")}<Input type="number" min={1} step={1} value={copiesDraft} onChange={event => {
          const value = event.target.value; setCopiesDraft(value);
          const count = Number(value); if (Number.isSafeInteger(count) && count > 0 && count !== queue.current?.task.multiplier) enqueue("copies", { action: "settings", multiplier: count });
        }} onBlur={() => {
          const count = Number(copiesDraft); if (!Number.isSafeInteger(count) || count <= 0) setCopiesDraft(String(task.multiplier));
        }} /></label>
        <label>{t("statistics.scope")}<Dropdown value={contextDraft.startsWith("region:") ? "region" : contextDraft.startsWith("layer:") ? "layer" : "all"}
          options={[{ label: t("statistics.all"), value: "all" }, { label: t("stockpile.region"), value: "region" }, { label: t("stockpile.layer"), value: "layer" }]}
          onChange={value => setContextDraft(value === "region" ? `region:${task.materials.project.regions[0] || ""}` : value === "layer" ? "layer:0" : "all")} /></label>
        {contextDraft.startsWith("region:") && <label>{t("stockpile.region")}<Dropdown value={contextDraft.slice(7)} options={task.materials.project.regions.map(region => ({ label: region, value: region }))} onChange={value => setContextDraft(`region:${value}`)} /></label>}
        {contextDraft.startsWith("layer:") && <label>{t("stockpile.layer")}<Input type="number" step={1} value={contextDraft.slice(6)} onChange={event => setContextDraft(`layer:${event.target.value}`)} /></label>}
        <label className="beta-material-check"><Checkbox checked={containersDraft} onCheckedChange={checked => setContainersDraft(checked === true)} />{t("statistics.includeContainers")}</label>
        <Button size="sm" disabled={busy || task.sourceStatus === "missing"} onClick={() => {
          if (contextDraft.startsWith("region:") && !contextDraft.slice(7) || contextDraft.startsWith("layer:") && !/^-?\d+$/.test(contextDraft.slice(6))) { setError(t("stockpile.invalidScope")); return; }
          const selectedScope: ApplicationMaterialScope = contextDraft.startsWith("region:") ? { kind: "region", value: contextDraft.slice(7) }
            : contextDraft.startsWith("layer:") ? { kind: "layer", value: contextDraft.slice(6) } : { kind: "all", value: null };
          void action({ action: "sync", sourcePath: task.sourcePath, expectedRevision: task.sourceRevision, scope: selectedScope, includeContainerItems: containersDraft, multiplier: Number(copiesDraft) || task.multiplier });
        }}>{t("stockpile.syncSnapshot")}</Button>
        <label>{t("stockpile.density")}<Dropdown value={preferences.view} options={[{ label: t("stockpile.compact"), value: "compact" }, { label: t("stockpile.comfortable"), value: "comfortable" }]} onChange={value => preference("view", value)} /></label>
        <label>{t("stockpile.nameLanguage")}<Dropdown value={preferences.language} options={[{ label: t("stockpile.followApp"), value: "auto" }, { label: "简体中文", value: "zh-CN" }, { label: "English", value: "en-US" }]} onChange={value => preference("language", value)} /></label>
      </div>
      <div className="beta-material-options">
        <label className="beta-material-check"><Checkbox checked={preferences.showAdvancedRecipeTree} onCheckedChange={checked => preference("showAdvancedRecipeTree", checked === true)} />{t("stockpile.advancedRecipeTree")}</label>
        <label className="beta-material-check"><Checkbox checked={preferences.showUnresolvedRecipes} onCheckedChange={checked => preference("showUnresolvedRecipes", checked === true)} />{t("stockpile.unresolvedRecipes")}</label>
        <label className="beta-material-check"><Checkbox checked={preferences.showIconFallbackBadge} onCheckedChange={checked => preference("showIconFallbackBadge", checked === true)} />{t("stockpile.iconFallbackBadge")}</label>
      </div>
      <div className="stockpile-list-actions"><Button size="sm" disabled={busy} onClick={() => void backup(false)}><Download size={15} />{t("stockpile.exportBackup")}</Button><Button size="sm" disabled={busy} onClick={() => void backup(true)}><Upload size={15} />{t("stockpile.importBackup")}</Button>
        <span className="muted">{t("stockpile.createdAt", { date: new Date(task.createdAt > 1e12 ? task.createdAt : task.createdAt * 1000).toLocaleString(locale) })}</span>
        <span className="muted">{t("stockpile.updatedAt", { date: new Date(task.updatedAt > 1e12 ? task.updatedAt : task.updatedAt * 1000).toLocaleString(locale) })}</span></div>
    </section>}
    <div className="material-table-scroll stockpile-table-scroll" ref={scrollRef} style={{ "--stockpile-content-height": `${Math.max(1, rows.length) * rowHeight + 42}px` } as React.CSSProperties}>
      <Table className="material-list-table beta-material-table stockpile-table">
        <TableHeader><TableRow><TableHead scope="col">{t("stockpile.complete")}</TableHead><TableHead scope="col">{t("statistics.material")}</TableHead><TableHead scope="col">{t("stockpile.required")}</TableHead><TableHead scope="col">{t("stockpile.currentQuantity")}</TableHead><TableHead scope="col">{t("stockpile.remainingQuantity")}</TableHead><TableHead scope="col">{t("stockpile.actions")}</TableHead></TableRow></TableHeader>
        <TableBody>{virtual.topSpacerHeight > 0 && <TableRow aria-hidden><VirtualSpacerCell colSpan={6} height={virtual.topSpacerHeight} /></TableRow>}
          {virtual.visibleItems.map((item, index) => {
            const material = metadata.get(item.materialId), name = materialName(material, preferences.language, item.materialId);
            return <TableRow key={item.materialId} className={(virtual.startIndex + index) % 2 ? "material-list-row-odd" : "material-list-row-even"}>
              <TableCell><Checkbox checked={item.complete} aria-label={t("stockpile.markComplete", { name })} onCheckedChange={checked => mutate(item.materialId, checked === true ? { quantity: item.requiredCount, status: "done" } : { clear: true })} /></TableCell>
              <TableCell><div className="beta-material-name">{task && <StockpileIcon task={task} iconKey={material?.item_icon_key || item.materialId.split("#")[0]} />}<span title={item.materialId}>{name}<small>{material?.category} · {t(`stockpile.state.${itemState(item)}`)}{preferences.showIconFallbackBadge && material?.icon_available === false && !!material.icon_diagnostic ? ` · ${t("stockpile.iconFallbackBadge")}` : ""}</small></span></div></TableCell>
              <TableCell className="beta-material-quantity" title={String(item.requiredCount)}>{displayQuantity(item, item.requiredCount)}</TableCell>
              <TableCell><QuantityInput value={item.quantity} label={t("stockpile.preparedFor", { name })} onQuantity={quantity => changeQuantity(item.materialId, quantity)} /></TableCell>
              <TableCell className="beta-material-quantity" title={String(item.remainingCount)}>{displayQuantity(item, item.remainingCount)}{item.overfilledCount > 0 && <small>{t("stockpile.overfilledValue", { count: item.overfilledCount })}</small>}</TableCell>
              <TableCell><div className="stockpile-row-actions"><Button size="xs" variant="ghost" onClick={() => mutate(item.materialId, { status: "preparing", quantity: (queue.current?.pending.get(`item:${item.materialId}`)?.mutation?.quantity ?? item.quantity) || item.requiredCount })}>{t("stockpile.state.preparing")}</Button><Button size="xs" variant="ghost" onClick={() => mutate(item.materialId, { status: "done", quantity: (queue.current?.pending.get(`item:${item.materialId}`)?.mutation?.quantity ?? item.quantity) || item.requiredCount })}>{t("stockpile.state.done")}</Button><Button size="xs" variant="ghost" onClick={() => mutate(item.materialId, { clear: true })}>{t("stockpile.cancelItem")}</Button><Button size="xs" variant="ghost" aria-label={t("stockpile.detailsFor", { name })} onClick={() => showDetails(item.materialId)}><Info size={15} /></Button></div></TableCell>
            </TableRow>;
          })}
          {virtual.bottomSpacerHeight > 0 && <TableRow aria-hidden><VirtualSpacerCell colSpan={6} height={virtual.bottomSpacerHeight} /></TableRow>}
          {!loading && task && rows.length === 0 && <TableRow><TableCell colSpan={6} className="material-list-empty-cell">{task.totalItems === 0 ? t("stockpile.empty") : t("statistics.noMatches")}</TableCell></TableRow>}
        </TableBody>
      </Table>
    </div>
    {nameDialog && <Dialog title={t(nameDialog === "create" ? "stockpile.create" : "stockpile.rename")} onClose={() => setNameDialog(null)} width="sm" footer={<><Button variant="ghost" onClick={() => setNameDialog(null)}>{t("editor.cancel")}</Button><Button disabled={!nameDraft.trim() || busy} onClick={() => {
      const mode = nameDialog; setNameDialog(null); void action(mode === "create" ? task ? { action: "create", taskId: task.taskId, name: nameDraft.trim() } : { action: "create", name: nameDraft.trim(), sourcePath, multiplier, scope, includeContainerItems } : { action: "rename", name: nameDraft.trim() });
    }}>{t("stockpile.saveTask")}</Button></>}><label className="material-export-field">{t("stockpile.taskName")}<Input autoFocus value={nameDraft} onChange={event => setNameDraft(event.target.value)} /></label></Dialog>}
    {confirmation && <Dialog title={t("stockpile.reset")} onClose={() => setConfirmation(null)} width="sm" footer={<><Button variant="ghost" onClick={() => setConfirmation(null)}>{t("editor.cancel")}</Button><Button variant="destructive" disabled={!resetConfirmationMatches(confirmation, task)} onClick={() => {
      const captured = confirmation; setConfirmation(null);
      if (resetConfirmationMatches(captured, currentTask.current)) enqueue("reset", { action: "mutate", mutation: { reset: true } });
    }}>{t("stockpile.confirmReset")}</Button></>}><p>{t("stockpile.resetNamedPrompt", { name: confirmation.name })}</p><p className="muted">{confirmation.scope}</p></Dialog>}
    {legacyOpen && <Dialog title={t("stockpile.bindLegacy")} onClose={() => setLegacyOpen(false)} width="md" footer={<><Button variant="ghost" onClick={() => setLegacyOpen(false)}>{t("editor.cancel")}</Button><Button disabled={!legacyId || !sourcePath || busy} onClick={() => {
      setLegacyOpen(false); void action({ action: "bindLegacy", legacyTaskId: legacyId, sourcePath, scope, includeContainerItems, multiplier });
    }}>{t("stockpile.confirmBind")}</Button></>}>
      <label className="material-export-field">{t("stockpile.legacyRecord")}<Dropdown value={legacyId} options={(workspace?.legacyTasks || []).map(record => ({ value: record.taskId, label: t("stockpile.legacyRecordSummary", { count: record.preparedCount, items: record.itemCount, date: new Date(record.updatedAt > 1e12 ? record.updatedAt : record.updatedAt * 1000).toLocaleString(locale) }) }))} onChange={setLegacyId} /></label>
      <p>{t("stockpile.legacyBindPrompt", { source: sourcePath ? displayFilePath(sourcePath) : t("stockpile.noProjection"), copies: multiplier })}</p>
      <p className="muted">{scope.kind === "all" ? t("statistics.all") : scope.kind === "region" ? t("statistics.regionScope", { name: scope.value }) : t("stockpile.layerScope", { layer: scope.value })}{includeContainerItems ? ` · ${t("statistics.includeContainers")}` : ""}</p>
    </Dialog>}
    {recoveryOpen && recovery && <Dialog title={t("stockpile.reviewRecovery")} onClose={() => setRecoveryOpen(false)} width="md" footer={<><Button variant="ghost" onClick={() => setRecoveryOpen(false)}>{t("editor.cancel")}</Button><Button variant="destructive" onClick={() => {
      if (currentTask.current?.taskId !== recovery.taskId) return;
      try { saveUpdateDraft(draftKey(recovery.taskId), undefined); setRecovery(null); setRecoveryOpen(false); } catch (failure) { setError(String(failure)); }
    }}>{t("stockpile.discardRecovery")}</Button><Button disabled={task?.taskId !== recovery.taskId || busy} onClick={() => {
      const work = queue.current; if (!work || work.task.taskId !== recovery.taskId) return;
      const validItems = new Set(work.task.items.map(item => item.materialId));
      for (const [key, command] of recoverableStockpileCommands(recovery.commands)) {
        if (command.mutation?.materialId && !validItems.has(command.mutation.materialId)) continue;
        work.pending.set(key, command);
      }
      const savedPreferences = work.pending.get("settings")?.preferences;
      if (savedPreferences) {
        const next = { ...defaultPreferences, ...work.task.preferences, ...savedPreferences };
        preferencesRef.current = next; searchRef.current = next.search; setPreferences(next); setSearchDraft(next.search);
      }
      try { persistPending(work); setRecovery(null); setRecoveryOpen(false); setPending(work.pending.size); void drain(work); } catch (failure) { setError(String(failure)); }
    }}>{t("stockpile.restoreRecovery")}</Button></>}><p>{t("stockpile.recoveryPrompt", { name: task?.name || recovery.taskId })}</p></Dialog>}
    {detailItem && <Dialog title={materialName(detailMaterial, preferences.language, detailItem.materialId)} onClose={() => setDetailId(null)} width="lg" className="stockpile-details-dialog">
      <dl className="stockpile-detail-facts">
        <div><dt>ID</dt><dd>{detailMaterial?.namespace_id || detailItem.materialId}</dd></div>
        <div><dt>{t("stockpile.itemIdentity")}</dt><dd>{detailItem.materialId}</dd></div>
        <div><dt>{t("stockpile.rawCount")}</dt><dd>{detailItem.requiredCount}</dd></div>
        <div><dt>{t("stockpile.required")}</dt><dd>{displayQuantity(detailItem, detailItem.requiredCount)}</dd></div>
        <div><dt>{t("stockpile.stackSize")}</dt><dd>{detailItem.stackSize}</dd></div>
        <div><dt>{t("stockpile.stacksRemainder")}</dt><dd>{Math.floor(detailItem.requiredCount / detailItem.stackSize)} / {detailItem.requiredCount % detailItem.stackSize}</dd></div>
        <div><dt>{t("stockpile.boxesRequired")}</dt><dd>{Math.ceil(detailItem.requiredCount / (detailItem.stackSize * 27))}</dd></div>
        <div><dt>{t("stockpile.sourceRegions")}</dt><dd>{detailMaterial?.source_regions.join(", ") || "—"}</dd></div>
        <div><dt>{t("stockpile.recipeStatus")}</dt><dd>{detailMaterial?.recipe_status}</dd></div>
        {!!detailMaterial?.normalized_from.length && <div><dt>{t("stockpile.normalizedFrom")}</dt><dd>{detailMaterial.normalized_from.join(", ")}</dd></div>}
        {detailMaterial?.icon_diagnostic && <div><dt>{t("stockpile.iconDiagnostic")}</dt><dd>{detailMaterial.icon_diagnostic}</dd></div>}
      </dl>
      <div className="stockpile-settings-grid">
        <label>{t("stockpile.stackSize")}<Input key={`${task?.taskId}:${detailItem.materialId}:stack`} type="number" min={1} max={99} defaultValue={detailItem.stackSize} onChange={event => {
          const size = Number(event.target.value); if (Number.isInteger(size) && size >= 1 && size <= 99) mutate(detailItem.materialId, { stackSize: size });
        }} onBlur={event => {
          const size = Number(event.target.value); if (!Number.isInteger(size) || size < 1 || size > 99) event.currentTarget.value = String(detailItem.stackSize);
        }} /></label>
        <label>{t("stockpile.storage")}<Input key={`${task?.taskId}:${detailItem.materialId}:storage`} defaultValue={detailItem.storageLocation || ""} onChange={event => mutate(detailItem.materialId, { storageLocation: event.target.value })} /></label>
        <label className="stockpile-note-field">{t("stockpile.note")}<Textarea key={`${task?.taskId}:${detailItem.materialId}:note`} rows={2} defaultValue={detailItem.publicNote || ""} onChange={event => mutate(detailItem.materialId, { publicNote: event.target.value })} /></label>
      </div>
      {preferences.showAdvancedRecipeTree && detailTree && task && <RecipeTree node={detailTree} task={task} preferences={preferences} />}
      {preferences.showUnresolvedRecipes && !detailTree && <p className="muted">{t(detailMaterial?.recipe_status === "available" ? "stockpile.recipeCachedNoTree" : detailMaterial?.recipe_status === "unresolved" ? "stockpile.recipeUnresolved" : "stockpile.recipeUnavailable")}</p>}
    </Dialog>}
  </section>;
}
