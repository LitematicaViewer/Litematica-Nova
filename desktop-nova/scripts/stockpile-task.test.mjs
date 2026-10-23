import assert from 'node:assert/strict';
import { test } from 'node:test';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

// Replay the production closures, not a second implementation of the queue.
const uiFile = new URL('../ui/windows/main/pages/statistics/StatisticsPage.tsx', import.meta.url);
const uiSource = fs.readFileSync(uiFile, 'utf8');
const uiAst = ts.createSourceFile(uiFile.pathname, uiSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const declarations = new Map();
(function visit(node) {
  if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name)) declarations.set(node.name.text, node.getText(uiAst));
  ts.forEachChild(node, visit);
})(uiAst);
const queueCode = ts.transpileModule(
  ['drain', 'changeQuantity', 'retry'].map(name => {
    assert(declarations.has(name), `Production ${name} closure must exist`);
    return `const ${declarations.get(name)};`;
  }).join('\n') + '\nglobalThis.queueApi = {drain, changeQuantity, retry};',
  { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS } },
).outputText;

function task(sourceRevision = 'revision-A') {
  return { taskId: 'task-A', sourceRevision, items: [{ materialId: 'stone', requiredCount: 10, preparedCount: 0, remainingCount: 10, complete: false }],
    requiredCount: 10, preparedCount: 0, remainingCount: 10, completedItems: 0, totalItems: 1 };
}
function setupQueue(write, read = async () => task()) {
  const states = { views: [], errors: [], pending: [] };
  const work = { epoch: 1, view: task(), pending: new Map(), running: false, reset: false, refresh: false };
  const context = {
    epoch: { current: 1 }, requestSequence: { current: 0 }, queue: { current: work },
    inputRef: { current: { sourcePath: 'C:/schematics/A.litematic', scope: { kind: 'all', value: null }, multiplier: 1, includeContainerItems: false } },
    setView: value => states.views.push(value), setPending: value => states.pending.push(value), setError: value => states.errors.push(value),
    loadRef: { current: () => {} }, setStockpileTaskQuantity: write, getStockpileTask: read,
    resetStockpileTask: async () => task(),
  };
  vm.runInNewContext(queueCode, context);
  return { context, states, work, ...context.queueApi };
}

test('UI queue serializes writes and coalesces the latest quantity without changing task identity', async () => {
  const requests = [], waiters = [];
  const state = setupQueue(input => { requests.push(input); return new Promise(resolve => waiters.push(resolve)); });
  state.work.pending.set('stone', 1);
  const draining = state.drain(state.work);
  state.work.pending.set('stone', 7);
  state.work.pending.set('wood', 4);
  assert.equal(requests.length, 1, 'only one write may be in flight');
  waiters.shift()({ ...task(), preparedCount: 1 });
  await new Promise(setImmediate);
  assert.equal(requests[1].quantity, 7);
  waiters.shift()({ ...task(), preparedCount: 7 });
  await new Promise(setImmediate);
  assert.equal(requests[2].materialId, 'wood');
  waiters.shift()({ ...task(), preparedCount: 11 });
  await draining;
  assert.deepEqual(requests.map(input => input.quantity), [1, 7, 4]);
  assert(requests.every(input => input.taskId === 'task-A' && input.sourceRevision === 'revision-A'));
  assert.equal(state.work.pending.size, 0);
  assert.equal(state.work.running, false);
  assert.equal(state.states.views.at(-1).preparedCount, 11);
});

test('UI failed writes stay unsaved; retry rebases pending quantities onto the real latest source revision', async () => {
  let offline = true;
  const requests = [];
  const updated = { ...task('revision-B'), items: [{ ...task().items[0], requiredCount: 4 }], requiredCount: 4 };
  const state = setupQueue(async input => {
    requests.push(input);
    if (offline) throw Error('source revision changed');
    return { ...updated, preparedCount: input.quantity };
  }, async () => updated);
  state.work.pending.set('stone', 9);
  state.work.pending.set('removed-variant', 3);
  await state.drain(state.work);
  assert.equal(state.work.pending.get('stone'), 9);
  assert.match(state.states.errors.at(-1), /source revision changed/);
  assert.equal(state.states.views.length, 0, 'a failed save must not become an acknowledged view');
  offline = false;
  await state.retry();
  assert.equal(requests.at(-1).sourceRevision, 'revision-B');
  assert.equal(requests.at(-1).quantity, 4, 'rebase clamps to the new authoritative requirement');
  assert.equal(state.work.pending.size, 0);
  assert.equal(state.states.views.at(-1).preparedCount, 4);
});

test('closing an overlay or switching source does not drop queued writes or apply old responses to the new view', async () => {
  const requests = [], waiters = [];
  const state = setupQueue(input => { requests.push(input); return new Promise(resolve => waiters.push(resolve)); });
  state.work.pending.set('stone', 2);
  const draining = state.drain(state.work);
  state.work.pending.set('wood', 3);
  state.context.epoch.current = 2;
  state.context.queue.current = null;
  waiters.shift()(task());
  await new Promise(setImmediate);
  assert.equal(requests.length, 2, 'the detached queue continues its remaining write');
  waiters.shift()(task());
  await draining;
  assert.equal(state.work.pending.size, 0);
  assert.equal(state.states.views.length, 0, 'old acknowledgements cannot update the new source');
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
