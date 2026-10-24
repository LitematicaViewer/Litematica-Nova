import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import React from 'react';

const source = fs.readFileSync(new URL('../ui/windows/main/pages/statistics/StockpileSourcesDialog.tsx', import.meta.url), 'utf8');
const code = ts.transpileModule(source, { reportDiagnostics: true, compilerOptions: {
  target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.React, esModuleInterop: true,
} });
assert.deepEqual(code.diagnostics.filter(value => value.category === ts.DiagnosticCategory.Error), []);
const tick = () => new Promise(setImmediate);
const input = (sourcePath, copies = 1) => ({ sourcePath, copies, scope: { kind: 'all', value: null }, includeContainerItems: false });

function fixture(overrides = {}) {
  const slots = [], effects = [];
  let cursor = 0, serial = 0, closed = 0, libraryReads = 0;
  const submitted = [];
  const hooks = { ...React,
    useState(initial) { const index = cursor++; if (!(index in slots)) slots[index] = typeof initial === 'function' ? initial() : initial;
      return [slots[index], next => { slots[index] = typeof next === 'function' ? next(slots[index]) : next; }]; },
    useRef(initial) { const index = cursor++; return slots[index] ??= { current: initial }; },
    useEffect(effect) { const index = cursor++; if (!(index in slots)) { slots[index] = true; effects.push(effect); } },
  };
  const module = { exports: {} };
  const records = [{ id: 'one', path: 'C:/one.litematic', status: 'ok', fileName: 'one.litematic', displayName: 'One' },
    { id: 'two', path: 'C:/two.litematic', status: 'ok', fileName: 'two.litematic', displayName: 'Two' }];
  const dependencies = {
    react: hooks,
    '../../../../../src/platform/dialogs': { openDialog: async options => { assert.equal(options.multiple, true); return ['C:/local-a.litematic', 'C:/local-b.litematic']; } },
    '../../../../../src/services/libraryStore': { loadLibrary: async () => { libraryReads++; return records; }, libraryRecordViews: value => value },
    '../../../../../src/services/i18n': { t: key => key },
    '../../../../i18n': { useI18n: () => ({ locale: 'en-US' }) },
    '../../../../components/Dialog': { Dialog: 'dialog' },
    '../../../../components/ui/button': { Button: 'button' },
    '../../../../components/ui/checkbox': { Checkbox: 'checkbox' },
    '../../../../components/ui/input': { Input: 'input' },
    '../../../../components/Dropdown': { Dropdown: 'dropdown' },
    '../../../../components/ui/table': Object.fromEntries(['Table', 'TableBody', 'TableCell', 'TableHead', 'TableHeader', 'TableRow'].map(name => [name, name])),
    'lucide-react': { FolderOpen: 'icon', Plus: 'icon', Trash2: 'icon' },
    './materials.css': {},
  };
  vm.runInNewContext(code.outputText, { module, exports: module.exports, crypto: { randomUUID: () => `source-request-${++serial}` }, require: name => {
    assert.ok(name in dependencies, `Unexpected dependency ${name}`); return dependencies[name];
  } });
  let props = { mode: 'create', defaultName: 'Town', initialSources: [input('C:/current.litematic')], onClose: () => { closed++; },
    onSubmit: async value => { submitted.push(JSON.parse(JSON.stringify(value))); }, ...overrides };
  const render = () => { cursor = 0; return module.exports.StockpileSourcesDialog(props); };
  const all = tree => !React.isValidElement(tree) ? [] : [tree, ...React.Children.toArray(tree.props.children).flatMap(all), ...React.Children.toArray(tree.props.footer).flatMap(all)];
  const elements = () => all(render());
  const button = label => elements().find(element => element.type === 'button' && element.props.children === label);
  const confirm = () => button(props.mode === 'create' ? 'stockpile.sources.createConfirm' : 'stockpile.sources.saveConfirm').props.onClick();
  const start = async () => { render(); for (const effect of effects.splice(0)) effect(); await tick(); };
  return { start, render, elements, button, confirm, submitted, records, closed: () => closed, libraryReads: () => libraryReads,
    copies: () => elements().filter(element => element.type === 'input' && element.props['aria-label'] === 'statistics.copies'),
    replaceSubmit: callback => { props = { ...props, onSubmit: async value => { submitted.push(JSON.parse(JSON.stringify(value))); return callback(value); } }; },
  };
}

test('opening, library loading and cancel never submit; native picker adds multiple inputs only', async () => {
  const f = fixture(); await f.start();
  assert.equal(f.libraryReads(), 1); assert.equal(f.submitted.length, 0);
  // The icon-bearing native-picker button has an array of children.
  f.elements().find(element => element.type === 'button' && React.Children.toArray(element.props.children).includes('stockpile.sources.localFiles')).props.onClick();
  await tick(); assert.equal(f.copies().length, 3); assert.equal(f.submitted.length, 0);
  f.button('editor.cancel').props.onClick(); assert.equal(f.closed(), 1); assert.equal(f.submitted.length, 0);
});

test('library multi-selection and per-source edits are sent only after confirmation', async () => {
  const f = fixture({ initialSources: [] }); await f.start();
  const choices = f.elements().filter(element => element.type === 'checkbox');
  choices[0].props.onCheckedChange(true); choices[1].props.onCheckedChange(true);
  f.elements().find(element => element.type === 'button' && React.Children.toArray(element.props.children).includes('stockpile.sources.addSelected')).props.onClick();
  assert.equal(f.copies().length, 2);
  f.copies()[0].props.onChange({ target: { value: '3' } });
  f.copies()[1].props.onChange({ target: { value: '2' } });
  assert.equal(f.submitted.length, 0); f.confirm(); await tick();
  assert.equal(f.submitted.length, 1);
  assert.deepEqual(f.submitted[0].sources.map(source => [source.sourcePath, source.copies]), [['C:/one.litematic', 3], ['C:/two.litematic', 2]]);
  assert.equal(f.closed(), 1);
});

test('invalid copies do not write and backend failure keeps form plus stable retry identity', async () => {
  const f = fixture(); await f.start();
  f.copies()[0].props.onChange({ target: { value: '0' } }); f.confirm(); await tick();
  assert.equal(f.submitted.length, 0);
  assert.ok(f.elements().some(element => element.props.role === 'alert' && element.props.children === 'stockpile.sources.invalidCopies'));
  f.copies()[0].props.onChange({ target: { value: '5' } });
  f.replaceSubmit(async () => { throw Error('SQLite revision conflict'); });
  f.confirm(); await tick(); assert.equal(f.closed(), 0);
  assert.ok(f.elements().some(element => element.props.role === 'alert' && String(element.props.children).includes('SQLite revision conflict')));
  f.confirm(); await tick(); assert.equal(f.submitted[0].requestId, f.submitted[1].requestId);
  f.copies()[0].props.onChange({ target: { value: '6' } }); f.confirm(); await tick();
  assert.notEqual(f.submitted[1].requestId, f.submitted[2].requestId);
});

test('double confirmation is single-flight and existing source metadata is not sent as input', async () => {
  let finish;
  const f = fixture({ mode: 'manage', task: { name: 'Existing' }, initialSources: [{ ...input('C:/saved.litematic', 4), sourceId: 'stable-source', projectId: 'project', status: 'missing', context: { resourceIdentity: 'private' } }] });
  await f.start(); f.replaceSubmit(() => new Promise(resolve => { finish = resolve; }));
  const confirm = f.button('stockpile.sources.saveConfirm').props.onClick;
  confirm(); confirm(); await tick(); assert.equal(f.submitted.length, 1);
  assert.equal(f.submitted[0].sources[0].sourceId, 'stable-source');
  assert.equal('status' in f.submitted[0].sources[0], false); assert.equal('context' in f.submitted[0].sources[0], false);
  finish(); await tick(); assert.equal(f.closed(), 1);
});

test('scope, container and removal edits stay local and preserve the remaining stable source', async () => {
  const f = fixture({ mode: 'manage', task: { name: 'Existing' }, initialSources: [
    { ...input('C:/same.litematic', 2), sourceId: 'first' }, { ...input('C:/same.litematic', 3), sourceId: 'second' },
  ] });
  await f.start();
  f.elements().filter(element => element.type === 'dropdown').at(-1).props.onChange('region');
  f.elements().find(element => element.type === 'input' && element.props['aria-label'] === 'stockpile.region').props.onChange({ target: { value: 'Hall' } });
  f.elements().filter(element => element.type === 'checkbox').at(-1).props.onCheckedChange(true);
  f.elements().find(element => element.type === 'button' && element.props['aria-label'] === 'stockpile.sources.remove').props.onClick();
  assert.equal(f.submitted.length, 0); assert.equal(f.copies().length, 1);
  f.confirm(); await tick();
  assert.equal(f.submitted[0].sources[0].sourceId, 'second');
  assert.deepEqual(f.submitted[0].sources[0].scope, { kind: 'region', value: 'Hall' });
  assert.equal(f.submitted[0].sources[0].includeContainerItems, true);
  assert.equal(f.submitted[0].sources[0].copies, 3);
});

test('manage may confirm removal of the last source, while create and add require a source', async () => {
  for (const mode of ['manage', 'create', 'add']) {
    const f = fixture({ mode, task: { name: 'Existing' }, initialSources: [] });
    await f.start();
    const button = f.button(mode === 'create' ? 'stockpile.sources.createConfirm' : 'stockpile.sources.saveConfirm');
    assert.equal(button.props.disabled, mode !== 'manage');
    f.confirm(); await tick();
    assert.equal(f.submitted.length, mode === 'manage' ? 1 : 0);
    if (mode === 'manage') assert.deepEqual(f.submitted[0].sources, []);
  }
});
