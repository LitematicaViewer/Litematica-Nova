import assert from 'node:assert/strict';
import { test } from 'node:test';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import postcss from 'postcss';

// Transpile actual production helpers and closures; never duplicate queue/filter logic.
const uiFile = new URL('../ui/windows/main/pages/statistics/StockpileTaskPanel.tsx', import.meta.url);
const uiSource = fs.readFileSync(uiFile, 'utf8');

test('task workspace passes window width and remaining height to the sole list scroll area', () => {
  const sheet = postcss.parse(fs.readFileSync(new URL('../ui/windows/main/pages/statistics/materials.css', import.meta.url), 'utf8'));
  const rule = selector => {
    const matches = [];
    sheet.walkRules(node => { if (node.selector === selector) matches.push(node); });
    assert.equal(matches.length, 1, `one authoritative ${selector} rule`);
    return Object.fromEntries(matches[0].nodes.filter(node => node.type === 'decl').map(node => [node.prop, node.value]));
  };
  const panel = rule('.stockpile-task'), controls = rule('.stockpile-task-controls');
  const list = rule('.stockpile-table-scroll');
  assert.equal(panel.width, '100%'); assert.equal(panel.flex, '1 0 0');
  assert.equal(panel['min-height'], 'min-content', 'short windows scroll controls rather than overlap the footer');
  assert.equal(panel['min-width'], '0'); assert.equal(panel['max-width'], undefined);
  assert.equal(controls.flex, '0 0 auto', 'toolbars use natural height, not stretched rows');
  assert.equal(list.width, '100%'); assert.equal(list.flex, '1 0 10rem');
  assert.equal(list.height, '0'); assert.equal(list['min-height'], '10rem');
  assert.equal(list['max-height'], undefined);
  assert.equal(rule('.material-list-task-content > .subwindow-body').overflow, 'auto');
  assert.equal(rule('.material-list-task-content > .subwindow-body > .material-export-footer').flex, '0 0 auto');
  assert.equal(rule('.material-list-content .stockpile-table').width, '100%');
  assert.equal(rule('.stockpile-table .beta-material-name > span').flex, '1 1 0');
  assert(!uiSource.includes('--stockpile-content-height'), 'row count must not set viewport height');
  assert.match(uiSource, /className="stockpile-task-controls"/);
  assert.match(uiSource, /className="material-table-scroll stockpile-table-scroll" ref=\{scrollRef\}/);
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
  const render = () => { cursor = 0; return context.exports.useVirtualWindow(items, ref, 50, 8); };
  const flush = () => { for (const [id, callback] of frames) { frames.delete(id); callback(); } return render(); };
  render(); cleanup = effect();
  const initial = render(); assert.equal(initial.visibleItems.length, 12);
  container.clientHeight = 700; observer.callback();
  const maximized = flush(); assert.equal(maximized.visibleItems.length, 22);
  container.scrollTop = 840; listeners.get('scroll')(); const scrolled = flush();
  container.clientHeight = 150; observer.callback(); const narrow = flush();
  assert.equal(narrow.startIndex, scrolled.startIndex); assert(narrow.visibleItems.length < scrolled.visibleItems.length);
  assert.equal(container.scrollTop, 840, 'resize never resets the current task list scroll offset');
  container.clientHeight = 500; headerHeight = 84; observer.callback(); const scaled = flush();
  assert.equal(scaled.startIndex, Math.max(0, Math.floor((840 - 84) / 50) - 8));
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
const helperNames = ['stockpileUnitParts', 'itemState', 'displayLanguage', 'materialName', 'filterStockpileRows',
  'resetConfirmationMatches', 'mergeStockpileMutation', 'recoverableStockpileCommands', 'pendingDraft', 'draftKey'];
const transpile = source => ts.transpileModule(source, { fileName: 'replay.tsx', compilerOptions: {
  target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX,
} }).outputText;
const helperSource = helperNames.map(name => { assert(functions.has(name), `Production ${name} must exist`); return functions.get(name); }).join('\n');
const helperCode = transpile(`${helperSource}\nglobalThis.helpers={${helperNames.join(',')}};`);
const queueNames = ['capturePending', 'persistPending', 'drain', 'enqueue', 'mutate', 'changeQuantity', 'retry'];
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
  const context={exports:{},epoch:{current:1},sequence:{current:0},queue:{current:work},currentTask:{current:work.task},recovery:null,
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

test('web unit hierarchy uses each material stack size and retains exact raw integers',()=>{
  const {stockpileUnitParts}=helpers();
  for(const stack of [1,16,64,99])assert.deepEqual(Array.from(stockpileUnitParts(stack*27*27+stack*27+stack+(stack-1),stack)),[1,1,1,stack-1]);
  assert.deepEqual(Array.from(stockpileUnitParts(0,16)),[0,0,0,0]);
  assert.deepEqual(Array.from(stockpileUnitParts(15,16)),[0,0,0,15]);
  assert.deepEqual(Array.from(stockpileUnitParts(27*64,64)),[0,1,0,0]);
});

test('web single-user filters/search preserve NBT identity and language/base-ID fallback',()=>{
  const {filterStockpileRows,materialName}=helpers();
  const items=[row('potion#A',{doneCount:3,preparedCount:3,status:'done',remainingCount:7}),
    row('potion#B',{preparingCount:8,status:'preparing',quantity:8,publicNote:'bring',storageLocation:'A1'}),
    row('stone',{doneCount:12,preparedCount:12,quantity:12,overfilledCount:2,complete:true,remainingCount:0})];
  const metadata=new Map([['potion#A',{item_icon_key:'minecraft:potion',display_name:'Healing',namespace_id:'potion#A',category:'brew',recipe_status:'available',display_names:{'en-US':'Healing Potion'}}],
    ['potion#B',{item_icon_key:'minecraft:potion',display_name:'Speed',namespace_id:'potion#B',category:'brew',recipe_status:'unresolved',display_names:{'en-US':'Speed Potion'}}],
    ['stone',{item_icon_key:'minecraft:stone',display_name:'Stone',category:'blocks',recipe_status:'missing',display_names:{}}]]);
  const filtered=preferences=>Array.from(filterStockpileRows(items,metadata,{...basePreferences,...preferences}),item=>item.materialId);
  assert.deepEqual(filtered({filter:'preparing'}),['potion#A','potion#B']);
  assert.deepEqual(filtered({filter:'noted'}),['potion#B']);assert.deepEqual(filtered({filter:'stored'}),['potion#B']);
  assert.deepEqual(filtered({filter:'overfilled'}),['stone']);assert.deepEqual(filtered({filter:'unresolved'}),['potion#B']);
  assert.deepEqual(filtered({search:'healing potion'}),['potion#A']);assert.deepEqual(filtered({search:'potion#B'}),['potion#B']);
  assert.equal(materialName({item_icon_key:'minecraft:potion',namespace_id:'potion#NBT',display_name:'Potion',display_names:{}},'auto','potion#NBT'),'minecraft:potion:Potion');
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
  const labels=[...uiSource.matchAll(/t\("(stockpile\.[^"]+)"/g)].map(match=>match[1]);
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
  const context = { exports: {}, performance, console: { info() {} }, require(name) {
    if (name.endsWith('/dialogs')) return {};
    if (name.endsWith('/applicationOperations')) return { getProjectStats };
    if (name.endsWith('/backend')) return {};
    if (name.endsWith('/filePathIdentity')) return { windowsPathKey: value => value.toLowerCase().replaceAll('\\', '/') };
    if (name.endsWith('/i18n')) return { t: key => key, getUiLocale: () => 'en-US', subscribeI18nChanges: () => () => {},
      resolveBlockDisplayName: (id, supplied) => `${id}:${supplied || 'localized'}`, translateBuildingType: value => value };
    throw Error(`Unexpected dependency: ${name}`);
  } };
  vm.runInNewContext(compiled, context);
  return context.exports;
}
const analysis = () => ({
  source_revision: 'source-A', material_semantics: 'minecraft-inventory-v9:test', structural_block_counts: [],
  material_items: [
    { block_id: 'minecraft:potion', item_identity: 'minecraft:potion#nbt-A', material_status: 'item', display_name: 'Healing', total_count: 3, icon_hint: 'minecraft:splash_potion' },
    { block_id: 'minecraft:potion', item_identity: 'minecraft:potion#nbt-B', material_status: 'item', display_name: 'Swiftness', total_count: 5 },
  ],
});

test('stats mapper preserves distinct NBT identities while resolving names and icons from the base item', () => {
  const stats = statsModule();
  const mapped = stats.mapStructureStats(analysis());
  assert.deepEqual(Array.from(mapped.materials, item => item.id), ['minecraft:potion#nbt-A', 'minecraft:potion#nbt-B']);
  assert(mapped.materials.every(item => item.itemId === 'minecraft:potion'));
  assert.equal(mapped.materials[0].name, 'minecraft:potion:Healing');
  assert.equal(mapped.materials[1].name, 'minecraft:potion:Swiftness');
  assert.equal(mapped.materials[0].iconHint, 'minecraft:splash_potion');
  assert.equal(mapped.materials[1].iconHint, 'minecraft:potion');
  const refreshed = stats.refreshStatisticsNames(mapped);
  assert.equal(stats.materialDisplayName(refreshed.materials[1]), 'minecraft:potion:Swiftness');
  assert.equal(refreshed.materials[1].id, 'minecraft:potion#nbt-B');
});

test('inventory analysis accepts v9 only with source revision and cache reuse requires Host revision confirmation', async () => {
  const requests = [];
  const responses = [analysis(), { cache_unchanged: true, source_revision: 'source-A' },
    { ...analysis(), source_revision: 'source-B', material_items: [{ ...analysis().material_items[0], total_count: 9 }] },
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
  assert.equal(changed.raw.source_revision, 'source-B');
  await assert.rejects(stats.loadStructureStats('C:/schematics/A.litematic'), /cacheChanged/);
});
