import assert from 'node:assert/strict';
import { test } from 'node:test';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import postcss from 'postcss';

// Transpile actual production helpers and closures; never duplicate queue/filter logic.
const uiFile = new URL('../ui/windows/main/pages/statistics/StockpileTaskPanel.tsx', import.meta.url);
const uiSource = fs.readFileSync(uiFile, 'utf8');

test('statistics-first workbench shares seven columns and gives the actual list remaining height', () => {
  const sheet = postcss.parse(fs.readFileSync(new URL('../ui/windows/main/pages/statistics/materials.css', import.meta.url), 'utf8'));
  const rule = selector => {
    const matches = [];
    sheet.walkRules(node => { if (node.selectors.includes(selector) && (node.parent.type !== 'atrule' || node.parent.name === 'layer')) matches.push(node); });
    assert(matches.length, `production ${selector} rule exists`);
    return Object.fromEntries(matches.flatMap(node => node.nodes.filter(node => node.type === 'decl').map(node => [node.prop, node.value])));
  };
  const panel = rule('.stockpile-task'), controls = rule('.stockpile-task-controls'), workspace = rule('.stockpile-workspace');
  const list = rule('.stockpile-table-scroll');
  assert.equal(panel.width, '100%'); assert.match(panel.flex, /^1 1 0/);
  assert.equal(panel['min-height'], '0');
  assert.equal(panel['min-width'], '0'); assert.equal(panel['max-width'], undefined);
  assert.match(controls.flex, /^0 [01] auto$/, 'toolbars use natural height, not stretched rows');
  assert.equal(workspace.display, 'grid'); assert.equal(workspace['min-height'], '0'); assert.equal(workspace['grid-template-columns'], 'minmax(0, 1fr)');
  assert.equal(list.width, '100%'); assert.match(list.flex, /^1 1 0/); assert.equal(list['min-height'], '0');
  assert.equal(list.overflow, 'auto');
  assert.equal(list['max-height'], undefined);
  assert.equal(rule('.material-list-task-content > .subwindow-body').overflow, 'hidden');
  assert.equal(rule('.material-list-task-content > .subwindow-body > .material-export-footer').flex, '0 0 auto');
  assert.equal(rule('.stockpile-task .stockpile-table').width, '100%');
  assert.equal(rule('.stockpile-task .stockpile-table')['min-width'], '945px');
  assert.equal(rule('.stockpile-table .stockpile-row').height, '44px');
  assert.equal(rule('.stockpile-material-detail-dialog.dialog-surface').width, 'min(760px, calc(100vw - 48px))');
  const compact = value => value.replace(/\s+/g, '');
  assert.equal(compact(panel['--stockpile-row-grid']), '40pxminmax(280px,2.2fr)minmax(145px,1fr)minmax(145px,.9fr)minmax(145px,1fr)90px100px');
  let sharedGrid = false, wideGrid = false;
  sheet.walkRules(node => {
    if (node.selector.includes('.stockpile-table-header') && node.selector.includes('.stockpile-row')) {
      node.walkDecls('grid-template-columns', declaration => { sharedGrid ||= declaration.value === 'var(--stockpile-row-grid)'; });
    }
    if (node.selector === '.stockpile-workspace' && node.parent.name === 'media' && node.parent.params.includes('1360px')) {
      node.walkDecls('grid-template-columns', declaration => { wideGrid ||= compact(declaration.value) === 'minmax(0,1fr)340px'; });
    }
  });
  assert(sharedGrid, 'header and virtual rows use the same grid, not separate column estimates');
  assert(!wideGrid, 'main workspace never reserves a permanent 340px detail column');
  assert.doesNotMatch(uiSource, /className="stockpile-detail-panel"|matchMedia/, 'details open on explicit demand, not a competing pane');
  assert(!uiSource.includes('--stockpile-content-height'), 'row count must not set viewport height');
  assert.match(uiSource, /className="stockpile-task-controls"/);
  assert.match(uiSource, /stockpile-table-scroll/);
  assert.match(uiSource, /className="stockpile-workspace"/);
  assert.match(uiSource, /colSpan=\{7\}/);
  assert.doesNotMatch(uiSource, /<Checkbox checked=\{item\.complete\}/, 'bulk selection is distinct from marking complete');
  const base = fs.readFileSync(new URL('../ui/styles/base.css', import.meta.url), 'utf8');
  assert.match(base, /html,\s*body,\s*#root\s*\{[^}]*width:\s*100%;[^}]*height:\s*100%/);
  const window = fs.readFileSync(new URL('../ui/shell/widget/subwindow.css', import.meta.url), 'utf8');
  assert.match(window, /\.material-list-window\s*\{[^}]*width:\s*100%;[^}]*height:\s*100%/);
});

test('existing virtual list resizes its real viewport without remounting or writing task state', () => {
  const hookSource = fs.readFileSync(new URL('../ui/components/useVirtualWindow.ts', import.meta.url), 'utf8');
  const code = ts.transpileModule(hookSource, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS } }).outputText;
  const states = [], frames = new Map(), listeners = new Map();
  let cursor = 0, effect, cleanup, mounted = false, observer, observerCount = 0, frameId = 0, headerHeight = 42;
  const container = { clientHeight: 200, scrollTop: 0,
    querySelector: () => ({ getBoundingClientRect: () => ({ height: headerHeight }) }),
    addEventListener: (name, callback) => listeners.set(name, callback),
    removeEventListener: (name, callback) => { assert.equal(listeners.get(name), callback); listeners.delete(name); },
  };
  const ref = { current: container };
  const items = Array.from({ length: 400 }, (_, id) => ({ id }));
  const react = {
    useState(initial) { const index = cursor++; if (!(index in states)) states[index] = initial; return [states[index], value => { states[index] = value; }]; },
    useMemo: compute => compute(),
    useEffect(callback) { if (!mounted) { mounted = true; effect = callback; } },
  };
  const context = { exports: {}, require: name => { assert.equal(name, 'react'); return react; },
    window: { requestAnimationFrame: callback => { frames.set(++frameId, callback); return frameId; }, cancelAnimationFrame: id => frames.delete(id) },
    ResizeObserver: class { constructor(callback) { observer = this; this.callback = callback; observerCount++; }
      observe(target) { assert.equal(target, container); } disconnect() { this.disconnected = true; } },
  };
  vm.runInNewContext(code, context);
  const render = () => { cursor = 0; return context.exports.useVirtualWindow(items, ref, 44, 8); };
  const flush = () => { for (const [id, callback] of frames) { frames.delete(id); callback(); } return render(); };
  render(); cleanup = effect();
  const initial = render(); assert.equal(initial.visibleItems.length, 13);
  container.clientHeight = 700; observer.callback();
  const maximized = flush(); assert.equal(maximized.visibleItems.length, 24);
  container.scrollTop = 840; listeners.get('scroll')(); const scrolled = flush();
  container.clientHeight = 150; observer.callback(); const narrow = flush();
  assert.equal(narrow.startIndex, scrolled.startIndex); assert(narrow.visibleItems.length < scrolled.visibleItems.length);
  assert.equal(container.scrollTop, 840, 'resize never resets the current task list scroll offset');
  container.clientHeight = 500; headerHeight = 84; observer.callback(); const scaled = flush();
  assert.equal(scaled.startIndex, Math.max(0, Math.floor((840 - 84) / 44) - 8));
  assert.equal(scaled.visibleItems[0], items[scaled.startIndex]);
  assert.equal(observerCount, 1, 'reuse the existing observer, not another resize subscription');
  cleanup(); assert.equal(observer.disconnected, true); assert.equal(listeners.size, 0);
});

const uiAst = ts.createSourceFile(uiFile.pathname, uiSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const declarations = new Map(), functions = new Map(), arrows = [];
(function visit(node) {
  if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name)) declarations.set(node.name.text, node.getText(uiAst));
  if (ts.isFunctionDeclaration(node) && node.name) functions.set(node.name.text, node.getText(uiAst));
  if (ts.isArrowFunction(node)) arrows.push(node);
  ts.forEachChild(node, visit);
})(uiAst);
const helperNames = ['resetConfirmationMatches','mergeStockpileMutation','stockpileDetailSelection','recoverableStockpileCommands','pendingDraft','draftKey'];
const transpile = source => ts.transpileModule(source, { fileName: 'replay.tsx', compilerOptions: {
  target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX,
} }).outputText;
const helperSource = helperNames.map(name => { assert(functions.has(name), `Production ${name} must exist`); return functions.get(name); }).join('\n');
const helperCode = transpile(`${helperSource}\nglobalThis.helpers={${helperNames.join(',')}};`);
const queueNames = ['capturePending', 'persistPending', 'drain', 'enqueue', 'mutate', 'changeQuantity',
  'clearQuantityDraft', 'latestQuantity', 'adjustQuantity', 'completeItem', 'rememberQuantity', 'applySelected', 'retry'];
const queueCode = transpile(`${helperSource}\n` + queueNames.map(name => {
  assert(declarations.has(name), `Production ${name} closure must exist`); return `const ${declarations.get(name)};`;
}).join('\n') + `\nglobalThis.queueApi={${queueNames.join(',')}};`);
const plain = value => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(setImmediate);
const basePreferences = { search:'',category:'all',filter:'all',sort:'grouped',view:'compact',language:'auto',individualCounts:false,
  showAdvancedRecipeTree:true,showUnresolvedRecipes:true,showIconFallbackBadge:true };
function row(materialId='stone', overrides={}) { return {materialId, requiredCount:10, preparedCount:0,doneCount:0,preparingCount:0,
  remainingCount:10,overfilledCount:0,complete:false,quantity:0,status:'not_started',stackSize:64,...overrides}; }
function task(sourceRevision='revision-A', taskId='task-A') { return {taskId,projectId:'project-A',sourceRevision,
  preferences:{...basePreferences}, items:[row(),row('wood')], requiredCount:20,preparedCount:0,remainingCount:20,completedItems:0,totalItems:2}; }
function setupQueue(write) {
  const states={views:[],errors:[],pending:[],recoveries:[]}, saved=new Map();
  const work={epoch:1,task:task(),pending:new Map(),running:false,refresh:false};
  const context={exports:{},pageOffset:0,mutationSequence:{current:0},crypto:{randomUUID:()=>`test-${Math.random()}`},getMinecraftTranslations:()=>({}),epoch:{current:1},sequence:{current:0},queue:{current:work},currentTask:{current:work.task},recovery:null,
    quantityDrafts:{current:new Map()},selectedItems:[],setEditingQuantities(){},
    preferencesRef:{current:{...basePreferences}},searchRef:{current:''},
    setPending:value=>states.pending.push(value),setError:value=>states.errors.push(value),setRecoveryOpen(){},setRecovery:value=>states.recoveries.push(value),
    accept:value=>states.views.push(value),loadRef:{current(){}},commandStockpileTask:write,
    saveUpdateDraft:(key,value)=>value===undefined?saved.delete(key):saved.set(key,plain(value)),
    getApplicationWindowLabel:()=>'material-list', getUiLocale:()=>'en-US',t:key=>key,
    resolveBlockDisplayName:(id,fallback)=>`${id}:${fallback}`,
  };
  vm.runInNewContext(queueCode,context);
  return {context,states,work,saved,...context.queueApi};
}
function helpers() { const context={exports:{},getUiLocale:()=>'en-US',resolveBlockDisplayName:(id,fallback)=>`${id}:${fallback}`};
  vm.runInNewContext(helperCode,context); return context.helpers; }

test('UI queue coalesces actual item commands, preserves overfill and serializes latest quantity', async()=>{
  const requests=[],waiters=[];
  const state=setupQueue(input=>{requests.push(input);return new Promise(resolve=>waiters.push(resolve));});
  state.changeQuantity('stone',1); state.changeQuantity('stone',17); state.changeQuantity('wood',4);
  assert.equal(requests.length,1); assert.equal(state.saved.size,1,'unacknowledged changes persist before dispatch');
  waiters.shift()({task:{...task(),preparedCount:1}});await tick();
  assert.equal(requests[1].mutation.quantity,17,'overfill is not clamped to requirement');
  waiters.shift()({task:{...task(),preparedCount:17}});await tick();
  assert.equal(requests[2].mutation.materialId,'wood');
  waiters.shift()({task:{...task(),preparedCount:21}});await tick();
  assert.deepEqual(requests.map(input=>input.mutation.quantity),[1,17,4]);
  assert(requests.every(input=>input.taskId==='task-A'&&input.sourceRevision==='revision-A'));
  assert.equal(state.work.pending.size,0); assert.equal(state.saved.size,0,'ACK clears recovery draft');
  assert.equal(state.states.views.at(-1).task.preparedCount,21);
});

test('uncommitted valid quantity drafts persist without Host writes and survive another row ACK', async () => {
  const requests=[],waiters=[];
  const state=setupQueue(input=>{requests.push(input);return new Promise(resolve=>waiters.push(resolve));});
  state.rememberQuantity('task-A','revision-A','stone',123);
  assert.equal(requests.length,0,'typing is local until Enter or blur');
  assert.equal(state.saved.values().next().value.commands[0][1].mutation.quantity,123);
  state.changeQuantity('wood',4);
  waiters.shift()({task:{...task(),items:[row('stone'),row('wood',{quantity:4,status:'preparing'})]}});await tick();
  assert.equal(state.saved.values().next().value.commands[0][1].mutation.materialId,'stone');
  assert.equal(state.saved.values().next().value.commands[0][1].mutation.quantity,123);
  state.rememberQuantity('other-task','revision-A','wood',777);
  assert.equal(state.context.quantityDrafts.current.size,1,'late edits cannot enter another task');
  state.rememberQuantity('task-A','revision-A','stone',undefined);
  state.changeQuantity('stone',123);
  assert.equal(requests.at(-1).mutation.quantity,123);
  waiters.shift()({task:{...task(),items:[row('stone',{quantity:123,status:'preparing'}),row('wood',{quantity:4,status:'preparing'})]}});await tick();
  assert.equal(state.saved.size,0,'committed draft clears only after ACK');
});

test('batch completion targets only selected stable material identities through the same writer', async () => {
  const requests=[];
  const state=setupQueue(async input=>{requests.push(input);return {task:task()};});
  state.context.selectedItems=[row('wood')];
  state.applySelected(false);await tick();
  assert.equal(requests.length,1);assert.equal(requests[0].mutation.materialId,'wood');
  assert.equal(requests[0].mutation.quantity,0);assert.equal(requests[0].mutation.complete,true);assert.equal(requests[0].mutation.status,'done');
  assert.equal(requests[0].taskId,'task-A');assert.equal(requests[0].sourceRevision,'revision-A');
});

test('a remounted quantity editor commits a recovered draft, while unchanged values never rewrite the database', async () => {
  const requests=[];
  const state=setupQueue(async input=>{requests.push(input);return {task:{...task(),items:[row('stone',{quantity:123,status:'preparing'}),row('wood')]}};});
  state.changeQuantity('stone',0);assert.equal(requests.length,0);
  state.rememberQuantity('task-A','revision-A','stone',123);
  const context={exports:{},draft:'123',submitted:{current:123},setDraft(){},
    onQuantity:quantity=>state.changeQuantity('stone',quantity),
    onDraft:quantity=>state.rememberQuantity('task-A','revision-A','stone',quantity)};
  vm.runInNewContext(transpile(`const ${declarations.get('commit')};globalThis.commit=commit;`),context);
  context.commit();await tick();
  assert.equal(requests.length,1);assert.equal(requests[0].mutation.quantity,123);
  assert.equal(state.saved.size,0);assert.equal(state.context.quantityDrafts.current.size,0);
  state.changeQuantity('stone',123);assert.equal(requests.length,1,'unchanged Enter/blur produces no second SQLite mutation');
});

test('failed writes retain exact intent and retry without marking unsaved values as saved', async()=>{
  let offline=true; const requests=[];
  const state=setupQueue(async input=>{requests.push(input);if(input.action==='select')return {task:task()};if(offline)throw Error('storage offline');return {task:task()};});
  state.changeQuantity('stone',19);await tick();
  assert.equal(state.work.pending.get('item:stone').mutation.quantity,19);
  assert.match(state.states.errors.at(-1),/storage offline/);assert.equal(state.states.views.length,0);assert.equal(state.saved.size,1);
  offline=false; await state.retry();await tick();
  assert.equal(requests.at(-1).mutation.quantity,19);assert.equal(state.work.pending.size,0);assert.equal(state.saved.size,0);
});

test('retry detects changed snapshot and asks before rebasing; it never repeats stale-revision edits',async()=>{
  const requests=[];const state=setupQueue(async input=>{requests.push(input);if(input.action==='select')return{task:task('revision-B')};throw Error('source revision changed');});
  state.changeQuantity('stone',19);await tick();await state.retry();
  assert.equal(requests.filter(input=>input.action==='mutate').length,1);
  assert.equal(state.states.recoveries.at(-1).sourceRevision,'revision-A');
  assert.equal(state.states.recoveries.at(-1).commands[0][1].mutation.quantity,19);
  assert.equal(state.work.task.sourceRevision,'revision-B');assert.equal(state.work.pending.size,0);
});

test('detached queues finish old task writes without updating a switched source', async()=>{
  const requests=[],waiters=[];const state=setupQueue(input=>{requests.push(input);return new Promise(resolve=>waiters.push(resolve));});
  state.changeQuantity('stone',2);state.changeQuantity('wood',3);
  state.context.epoch.current=2;state.context.queue.current=null;state.context.currentTask.current=task('revision-B','task-B');
  waiters.shift()({task:task()});await tick();assert.equal(requests.length,2);
  waiters.shift()({task:task()});await tick();
  assert.equal(state.work.pending.size,0);assert.equal(state.states.views.length,0);
  assert(requests.every(input=>input.taskId==='task-A'));
});

test('pending search survives another item ACK and contains only idempotent patches',async()=>{
  let resolve;const state=setupQueue(()=>new Promise(done=>{resolve=done;}));
  state.changeQuantity('stone',2);state.context.searchRef.current='potion';
  resolve({task:task()});await tick();
  const draft=[...state.saved.values()][0];assert.equal(draft.commands[0][1].preferences.search,'potion');
  const {recoverableStockpileCommands,mergeStockpileMutation}=helpers();
  const commands=[['reset',{action:'mutate',mutation:{reset:true}}],['clear',{action:'mutate',mutation:{clear:true}}],
    ['sync',{action:'sync'}],['import',{action:'import'}],['note',{action:'mutate',mutation:{publicNote:'x'}}],['prefs',{action:'settings',preferences:{search:'x'}}]];
  assert.deepEqual(Array.from(recoverableStockpileCommands(commands),entry=>entry[0]),['note','prefs']);
  assert.deepEqual(plain(mergeStockpileMutation({clear:true},{quantity:4,status:'preparing'})),{quantity:4,status:'preparing'});
});

test('detail selection follows the visible stable identity, never an old filtered or other variant row', () => {
  const {stockpileDetailSelection} = helpers();
  const visible = [row('potion#A'),row('potion#B')];
  assert.equal(stockpileDetailSelection(visible,null),'potion#A');
  assert.equal(stockpileDetailSelection(visible,'potion#B'),'potion#B');
  assert.equal(stockpileDetailSelection([visible[0]],'potion#B'),'potion#A');
  assert.equal(stockpileDetailSelection([],'potion#B'),null);
});

test('unified export opens with zero I/O and only confirmation dispatches the chosen real export', async () => {
  const writes = [], state = {};
  const context = { exports: {}, preferences: { individualCounts: true }, task: task(), busy: false,
    exportBusy: false, exportSource: 'materials', exportFormat: 'copy', exportScope: 'remaining', materialExportContent: undefined,
    setExportSource: value => { state.source = value; }, setExportFormat: value => { state.format = value; },
    setExportScope: value => { state.scope = value; }, setExportIndividualCounts: value => { state.individual = value; },
    setStatus() {}, setExportOpen: value => { state.open = value; }, setExportBusy() {}, setError() {},
    exportView: (...input) => writes.push(['view', ...input]),
    backup: restore => writes.push(['backup', restore]),
  };
  vm.runInNewContext(transpile(`const ${declarations.get('beginExport')}; const ${declarations.get('confirmExport')}; globalThis.api={beginExport,confirmExport};`), context);
  context.api.beginExport();
  assert.deepEqual(state, { source: 'materials', format: 'copy', scope: 'all', individual: true, open: true });
  assert.equal(writes.length, 0, 'opening configuration must not copy, save or mutate a task');
  await context.api.confirmExport();
  assert.deepEqual(writes.pop(), ['view', 'copy']);
  context.exportSource = 'task'; context.exportFormat = 'csv';
  await context.api.confirmExport(); assert.deepEqual(writes.pop(), ['view', 'csv']);
  context.exportFormat = 'json'; await context.api.confirmExport(); assert.deepEqual(writes.pop(), ['backup', false]);
  context.exportSource = 'materials'; context.materialExportContent = {};
  await context.api.confirmExport(); assert.equal(writes.length, 0, 'static export keeps its own explicit final confirmation');
});

test('statistics default renders the existing task workspace even without a source, and retains genuine overview fields', () => {
  const source = fs.readFileSync(new URL('../ui/windows/main/pages/statistics/StatisticsPage.tsx', import.meta.url), 'utf8');
  const page = source.slice(source.indexOf('export function StatisticsPage'));
  assert.match(page, /<StockpileTaskPanel sourcePath=\{currentFile \|\| ""\}/);
  assert.doesNotMatch(page, /openMaterialsWithWindowBehavior|<MaterialTable|<MaterialsDialog|beta-statistics-panes/);
  assert.doesNotMatch(page, /<MaterialCatalogDialog/); assert.match(page, /openMaterialListWindow\(currentFile\)/);
  assert.match(page, /materialExportContent=\{data && currentFile \? <MaterialListContent[^>]+exportOnly/);
  for (const field of ['totalNonAirBlocks', 'enclosingSize.x', 'density', 'materialCount', 'entityCount', 'regionCount', 'redstoneRatio', 'fluidRatio']) {
    assert(page.includes(`data.${field}`), `Genuine statistics field ${field} remains visible`);
  }
  assert.match(page, /enumeratorResults\.map/); assert.match(page, /EnumeratorInfoEditorDialog/);
  assert.doesNotMatch(page, /if \(!currentFile\)\s*\{\s*return/);
});

test('actual reset cancellation writes nothing and confirmation cannot target a different task or revision',()=>{
  const confirm=arrows.find(node=>node.body.getText(uiAst).includes('const captured = confirmation;'));
  assert(confirm,'Production reset confirmation handler must exist');
  let resetOpening;
  (function find(node){if(ts.isJsxOpeningElement(node)&&node.tagName.getText(uiAst)==='Dialog'&&node.attributes.getText(uiAst).includes('title={t("stockpile.reset")}'))resetOpening=node;ts.forEachChild(node,find);})(uiAst);
  const cancel=resetOpening.attributes.properties.find(node=>ts.isJsxAttribute(node)&&node.name.getText(uiAst)==='onClose').initializer.expression;
  const captured={taskId:'task-A',sourceRevision:'revision-A',name:'A',scope:'all'},writes=[];
  const context={exports:{},confirmation:captured,currentTask:{current:task()},setConfirmation(){},enqueue:(...args)=>writes.push(args)};
  vm.runInNewContext(transpile(`${functions.get('resetConfirmationMatches')}\nglobalThis.confirm=${confirm.getText(uiAst)};globalThis.cancel=${cancel.getText(uiAst)};`),context);
  context.cancel();assert.equal(writes.length,0);
  context.currentTask.current=task('revision-A','task-B');context.confirm();assert.equal(writes.length,0);
  context.currentTask.current=task('revision-B');context.confirm();assert.equal(writes.length,0);
  context.currentTask.current=task();context.confirm();assert.equal(writes.length,1);assert.equal(writes[0][1].mutation.reset,true);
});

test('existing recovery store captures the newest getter without stale-render overwrite and clears acknowledged keys',()=>{
  const source=fs.readFileSync(new URL('../ui/windows/main/updateRecovery.ts',import.meta.url),'utf8');
  const storage=new Map(),refs=[],effects=[];let cursor=0;
  const hooks={useRef(value){const index=cursor++;return refs[index]||(refs[index]={current:value});},useState(){return[null,()=>{}];},
    useEffect(effect,deps){const index=cursor++;if(!effects[index]||deps.some((value,i)=>effects[index].deps[i]!==value)){effects[index]?.cleanup?.();effects[index]={deps,cleanup:effect()};}}};
  const context={exports:{},console,localStorage:{getItem:key=>storage.get(key)||null,setItem:(key,value)=>storage.set(key,value)},require:name=>name==='react'?hooks:{}};
  vm.runInNewContext(transpile(source),context);
  const api=context.exports;
  let latest={quantity:1};api.useUpdateDraft('stockpile:A',()=>latest);
  latest={quantity:7};cursor=0;api.useUpdateDraft('stockpile:A',()=>latest);
  api.preserveUpdateDrafts();assert.equal(api.readUpdateDraft('stockpile:A').quantity,7);
  api.saveUpdateDraft('stockpile:B',{quantity:3});api.saveUpdateDraft('stockpile:A',undefined);
  assert.equal(api.readUpdateDraft('stockpile:A'),undefined);assert.equal(api.readUpdateDraft('stockpile:B').quantity,3);
  latest=undefined;api.preserveUpdateDrafts();assert.equal(api.readUpdateDraft('stockpile:A'),undefined);
});

test('new stockpile labels exist in both locales without duplicate keys',()=>{
  const file=new URL('../src/services/locales/editor.ts',import.meta.url),source=fs.readFileSync(file,'utf8');
  const context={exports:{}};vm.runInNewContext(transpile(source),context);
  const {zhCN,enUS}=context.exports;
  const surfaces = ['StatisticsPage.tsx', 'MaterialCatalogDialog.tsx', 'RecipeTreeView.tsx']
    .map(name => fs.readFileSync(new URL(`../ui/windows/main/pages/statistics/${name}`, import.meta.url), 'utf8'));
  const labels=[...[uiSource, ...surfaces].join('\n').matchAll(/t\("((?:stockpile|statistics|editor)\.[^"]+)"/g)].map(match=>match[1]);
  for(const label of labels){assert(zhCN[label],`Missing Chinese label ${label}`);assert(enUS[label],`Missing English label ${label}`);}
  const ast=ts.createSourceFile(file.pathname,source,ts.ScriptTarget.Latest,true,ts.ScriptKind.TS);
  for(const statement of ast.statements){if(!ts.isVariableStatement(statement))continue;for(const declaration of statement.declarationList.declarations){
    if(!declaration.initializer||!ts.isObjectLiteralExpression(declaration.initializer))continue;
    const seen=new Set();for(const property of declaration.initializer.properties){if(!ts.isPropertyAssignment(property))continue;const name=property.name.text;
      if(!name.startsWith('stockpile.'))continue;assert(!seen.has(name),`Duplicate ${name}`);seen.add(name);}
  }}
});

function statsModule(getProjectStats = async () => { throw Error('Unexpected project read'); }) {
  const source = fs.readFileSync(new URL('../src/services/statsService.ts', import.meta.url), 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS } }).outputText;
  const context = { exports: {}, crypto:{randomUUID:()=>"test-stats-operation"}, performance, console: { info() {} }, require(name) {
    if (name.endsWith('/dialogs')) return {};
    if (name.endsWith('/applicationOperations')) return { getProjectStats };
    if (name.endsWith('/backend')) return {};
    if (name.endsWith('/filePathIdentity')) return { windowsPathKey: value => value.toLowerCase().replaceAll('\\', '/') };
    if (name.endsWith('/i18n')) return { t: key => key, getUiLocale: () => 'en-US', getI18nRevision:()=>1, getMinecraftTranslations:()=>({}), initI18n:async()=>{}, subscribeI18nChanges: () => () => {},
      resolveBlockDisplayName: (id, supplied) => `${id}:${supplied || 'localized'}`, translateBuildingType: value => value };
    throw Error(`Unexpected dependency: ${name}`);
  } };
  vm.runInNewContext(compiled, context);
  return context.exports;
}
const analysis = () => ({
  statisticsView:{totalNonAirBlocks:8,materialCount:2,materials:[{id:'minecraft:potion#nbt-A',name:'Healing',totalCount:3},{id:'minecraft:potion#nbt-B',name:'Swiftness',totalCount:5}]},source_revision: 'source-A', material_semantics: 'minecraft-inventory-v9:test', structural_block_counts: [],
  material_items: [
    { block_id: 'minecraft:potion', item_identity: 'minecraft:potion#nbt-A', material_status: 'item', display_name: 'Healing', total_count: 3, icon_hint: 'minecraft:splash_potion' },
    { block_id: 'minecraft:potion', item_identity: 'minecraft:potion#nbt-B', material_status: 'item', display_name: 'Swiftness', total_count: 5 },
  ],
});

test('inventory analysis accepts v9 only with source revision and cache reuse requires Host revision confirmation', async () => {
  const requests = [];
  const responses = [analysis(), { cache_unchanged: true, source_revision: 'source-A' },
    { ...analysis(), source_revision: 'source-B', statisticsView:{...analysis().statisticsView,materials:[{id:'minecraft:potion#nbt-A',name:'Healing',totalCount:9}]} },
    { cache_unchanged: true, source_revision: 'wrong-source' }];
  const stats = statsModule(async input => { requests.push(input); return responses.shift(); });
  assert.equal(stats.hasInventoryMaterialSemantics(analysis()), true);
  assert.equal(stats.hasInventoryMaterialSemantics({ ...analysis(), material_semantics: 'minecraft-inventory-v8:test' }), false);
  assert.equal(stats.hasInventoryMaterialSemantics({ material_semantics: 'minecraft-inventory-v9:test' }), false);
  const first = await stats.loadStructureStats('C:/schematics/A.litematic');
  const second = await stats.loadStructureStats('C:/schematics/A.litematic');
  assert.equal(second, first);
  assert.equal(requests[1].knownRevision, 'source-A');
  const changed = await stats.loadStructureStats('C:/schematics/A.litematic');
  assert.notEqual(changed, first);
  assert.equal(changed.materials[0].totalCount, 9);
  assert.equal(changed.sourceRevision, 'source-B');
  await assert.rejects(stats.loadStructureStats('C:/schematics/A.litematic'), /cacheChanged/);
});


test('increment buttons send independent Rust adjustments and stable retry identities, not UI arithmetic', async () => {
 const requests=[], waiters=[];
 const state=setupQueue(input=>{requests.push(input);return new Promise(resolve=>waiters.push(resolve));});
 state.adjustQuantity(row(),1); state.adjustQuantity(row(),1);
 assert.equal(requests.length,1);assert.equal(requests[0].mutation.adjustment.steps,1);assert.equal(requests[0].mutation.quantity,undefined);
 const receipt=requests[0].mutation.requestId;assert(receipt);
 waiters.shift()({task:task()});await tick();
 assert.equal(requests.length,2);assert.notEqual(requests[1].mutation.requestId,receipt);
 waiters.shift()({task:task()});await tick();assert.equal(state.work.pending.size,0);
 assert.doesNotMatch(uiSource,/stockpileUnitParts|filterStockpileRows|quantityText\(|Math\.(floor|ceil)\(detailItem/);
});

test('catalog is native read-only paginated Rust data, with cancellable stale query isolation', () => {
 const catalog=fs.readFileSync(new URL('../ui/windows/main/pages/statistics/MaterialCatalogDialog.tsx',import.meta.url),'utf8');
 const native=fs.readFileSync(new URL('../ui/windows/material_list/MaterialListWindow.tsx',import.meta.url),'utf8');
 assert.match(catalog,/queryMaterials\(/);assert.match(catalog,/getDataOperationStatus\(operationId, true\)/);
 assert.match(catalog,/generation === queryGeneration.current/);assert.match(catalog,/limit: 200/);
 assert.doesNotMatch(catalog,/filterCatalogMaterials|commandStockpileTask|getStockpileTask|loadStructureStats|DialogPrimitive|\.sort\(/);
 assert.match(native,/<MaterialCatalogDialog/);assert.doesNotMatch(native,/StockpileTaskPanel|loadStructureStats|mapStructureStats/);
 assert.match(catalog,/<MaterialSources/);assert.match(catalog,/group.layers.map/);assert.match(catalog,/sourceSummary/);
});

test('statistics mapping renders only the exact Rust view and never repairs missing business output', () => {
 const stats=statsModule(),raw=analysis();
 assert.equal(stats.mapStructureStats(raw),raw.statisticsView);
 assert.equal(stats.materialDisplayName({name:'Backend localized name',id:'minecraft:any'}),'Backend localized name');
 assert.throws(()=>stats.mapStructureStats({material_items:[]}),/displayDataMissing/);
 const source=fs.readFileSync(new URL('../ui/windows/main/pages/statistics/StatisticsPage.tsx',import.meta.url),'utf8');
 assert.match(source,/queryStatisticsRules\(/);assert.doesNotMatch(source,/evaluateEnumeratorInfoRule|buildBaseUniverse|evaluateEnumeratorExpression/);
});


test('version conversion sends selection/property edits to Rust and cancels superseded operations', () => {
 const source=fs.readFileSync(new URL('../ui/windows/main/pages/properties/VersionConversionDialog.tsx',import.meta.url),'utf8');
 assert.doesNotMatch(source,/function parseState|function stateString|function targetState|versions \|\| \[\]\)\.filter/);
 assert.match(source,/setReplacements\(result.effectiveReplacements\)/);
 assert.match(source,/selection, propertyEdit/);assert.match(source,/getDataOperationStatus\(operation.current, true\)/);
 assert.match(source,/operationId/);assert.match(source,/progress.completed \?\? progress.current/);
 assert.match(source,/entry.targetProperties/);assert.match(source,/entry.sourceBlockId/);
});
