import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import React from 'react';
import { JSDOM } from 'jsdom';

const source = fs.readFileSync(new URL('../ui/windows/main/pages/composition/CompositionPage.tsx', import.meta.url), 'utf8');
const clone = value => structuredClone(value);

test('组合编辑UI：恢复既有资源、Host权威回执、数值/选择/工具/拆分联动，不重建或重复提交', async () => {
  const dom = new JSDOM('<div id="root"></div>');
  dom.window.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  const names = ['window', 'document', 'HTMLElement', 'Element', 'Node', 'Event', 'CustomEvent', 'getComputedStyle', 'IS_REACT_ACT_ENVIRONMENT'];
  const previous = Object.fromEntries(names.map(name => [name, globalThis[name]]));
  Object.assign(globalThis, { window: dom.window, document: dom.window.document, getComputedStyle: dom.window.getComputedStyle, IS_REACT_ACT_ENVIRONMENT: true });
  for (const name of ['HTMLElement', 'Element', 'Node', 'Event', 'CustomEvent']) globalThis[name] = dom.window[name];
  const { createRoot } = await import('react-dom/client');
  const root = createRoot(document.getElementById('root'));
  const transform = { offset: [0,0,0], origin: [0,0,0], rotation: 0, mirrorX: false, mirrorZ: false };
  let project = { schema: 1, id: 'project', generation: 1, state: { name: 'Saved', conflictPolicy: 'reject', targetDataVersion: 3955, splitBoxes: [], parts: ['a','b'].map(id => ({ id, name: `Part ${id}`, sourcePath: `${id}.litematic`, snapshotId: id, selection: null, regions: [], visible: true, locked: false, clearAir: false, transform: clone(transform) })) } };
  let selected = 'a', tool = 'translate', starts = 0, nextEvent = null;
  const calls = [], controls = [], hookInputs = [];
  const view = () => ({ project: clone(project), canUndo: project.generation > 1, canRedo: false, parts: project.state.parts.map(part => ({ part: clone(part), bounds: { min: [0,0,0], max: [1,1,1] }, size: [2,2,2], blockCount: 8, dataVersion: 3955, regions: [] })) });
  const editorScene = () => ({ schema: 'nova.composition.editor-scene.v1', projectId: project.id, generation: project.generation, sceneKey: 'same-geometry', resources: [], instances: [], selectedInstanceId: selected, tool, splitBoxes: project.state.splitBoxes.map((box,index) => ({ id: `split-${index}`, ...clone(box) })), eventPath: 'C:/session/events.json', acknowledgedEventSequence: 0 });
  const query = async input => {
    calls.push(clone(input));
    if (input.action === 'list') return { projects: [clone(project)] };
    if (input.action === 'open') return view();
    if (input.action === 'editorState') {
      if ('selectedInstanceId' in input) selected = input.selectedInstanceId;
      if ('tool' in input) tool = input.tool;
      return { outputPath: 'C:/session/packed.litematic', editorScenePath: 'C:/session/editor.json', editorScene: editorScene() };
    }
    if (input.action === 'update') {
      assert.equal(input.generation, project.generation);
      const part = project.state.parts.find(item => item.id === input.partId);
      Object.assign(part, clone(input.patch)); project.generation++;
      return view();
    }
    if (input.action === 'setSplitBoxes') { project.state.splitBoxes = clone(input.boxes); project.generation++; return view(); }
    throw new Error(`Unexpected action ${input.action}`);
  };
  const modules = {
    react: { ...React, default: React },
    'lucide-react': new Proxy({}, { get: () => () => null }),
    '../../../../components/Dialog': { Dialog: ({ children }) => React.createElement('div', null, children) },
    '../../../../components/ui/button': { Button: ({ children, variant, size, ...props }) => React.createElement('button', props, children) },
    '../../../../components/ui/checkbox': { Checkbox: ({ checked, onCheckedChange, ...props }) => React.createElement('input', { ...props, type: 'checkbox', checked, onChange: e => onCheckedChange(e.target.checked) }) },
    '../../../../../src/platform/dialogs': { confirmDialog: async () => true, openDialog: async () => null, saveDialog: async () => null },
    '../../../../../src/services/i18n': { getUiLocale: () => 'zh-CN' },
    '../../../../i18n': { useI18n: () => ({ locale: 'zh-CN' }) },
    '../render/RenderPage': { RenderPage: () => React.createElement('div', { 'data-render': true }) },
    '../../useQuickPreview': { useQuickPreview: (...args) => { hookInputs.push(args); return { operation: args[0] ? { previewId: 'same-viewer', state: 'ready' } : null, pending: false, error: '', hostRef: React.useRef(null), start: async () => { starts++; }, hide: async () => {}, close: async () => assert.fail('Closing the Viewer is not a UI edit') }; } },
    '../../../../../src/platform/quickPreview': { readQuickPreviewEditorEvent: async (_id, after) => {
      if (!nextEvent || nextEvent.sequence <= after) return null;
      const event = nextEvent; nextEvent = null;
      // Simulate the real Host boundary: commit and acknowledge before return.
      if (event.committed && !event.cancelled) { Object.assign(project.state.parts.find(part => part.id === event.instanceId), { transform: clone(event.transform) }); project.generation++; }
      return clone(event);
    } },
    '../../../../../src/platform/sceneView': { updateSceneView: async input => { controls.push(clone(input)); }, releaseCameraInput: async () => {} },
    '../../../../../src/platform/schematicFormats': { querySchematicFormat: async () => ({ sourceVersions: [{ version: '1.21', dataVersion: 3955 }] }) },
    '../../../../../src/platform/composition': { queryComposition: query, cancelComposition: async () => {} },
    './composition.css': {},
  };
  const exports = {};
  const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React } }).outputText;
  vm.runInNewContext(output, { exports, require: name => { assert.ok(name in modules, name); return modules[name]; }, window: dom.window, crypto, setTimeout, clearTimeout });
  const Page = exports.CompositionPage;
  const settle = async (ms = 5) => { await React.act(async () => { await new Promise(resolve => setTimeout(resolve, ms)); }); };
  const button = name => [...document.querySelectorAll('button')].find(node => node.textContent === name);
  const click = async name => { const element = button(name); assert.ok(element, name); assert.equal(element.disabled, false, `${name} enabled`); await React.act(async () => { element.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true })); }); await settle(); };
  const offsetX = () => [...document.querySelectorAll('fieldset')].find(node => node.querySelector('legend')?.textContent.includes('位移')).querySelector('input');
  try {
    await React.act(async () => { root.render(React.createElement(Page, { initialProjectId: 'project' })); }); await settle(20);
    assert.ok(document.body.textContent.includes('快速编辑视图'));
    assert.equal(starts, 0); assert.ok(!calls.some(call => ['preview', 'previewEditor'].includes(call.action)));
    assert.equal(hookInputs.at(-1)[3], 'C:/session/editor.json');
    assert.ok(document.querySelector('option[value="3955"]'));
    await click('显示编辑视图'); assert.equal(starts, 1);
    await React.act(async () => { root.render(React.createElement(Page, { initialProjectId: 'project', presentationBlocked: true })); }); await settle();
    assert.equal(hookInputs.at(-1)[1], true); assert.equal(hookInputs.at(-1)[3], 'C:/session/editor.json');
    await React.act(async () => { root.render(React.createElement(Page, { initialProjectId: 'project' })); }); await settle();
    assert.equal(starts, 1);
    await click('Part b'); assert.equal(controls.at(-1).controls.compositionEditor.selectedInstanceId, 'b');
    await click('旋转'); assert.equal(controls.at(-1).controls.compositionEditor.tool, 'rotate');
    nextEvent = { schema: 'nova.composition.editor-event.v1', projectId: 'project', sceneKey: 'same-geometry', generation: project.generation, sequence: 1, selectedInstanceId: 'b', instanceId: 'b', committed: true, cancelled: false, transform: { ...clone(transform), offset: [8,9,10] } };
    // Blur is queued before UI poll: only X is replaced after the Host flush.
    await React.act(async () => {
      const input = offsetX(); Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, 'value').set.call(input, '33');
      input.dispatchEvent(new dom.window.FocusEvent('focusout', { bubbles: true }));
    }); await settle(20);
    const update = calls.findLast(call => call.action === 'update');
    assert.deepEqual(update.patch.transform.offset, [33,9,10]);
    assert.equal(offsetX().value, '33');
    assert.ok(!calls.some(call => call.action === 'commitEditor'));
    assert.ok(calls.filter(call => call.action === 'editorState').every(call => !('acknowledgeSequence' in call)));
    const generation = project.generation;
    nextEvent = { schema: 'nova.composition.editor-event.v1', projectId: 'project', sceneKey: 'same-geometry', generation, sequence: 2, selectedInstanceId: 'b', committed: false, cancelled: true };
    await settle(170); assert.equal(project.generation, generation); assert.ok(document.body.textContent.includes('当前手势已取消'));
    const select = document.querySelector('select[aria-label="拆分方式"]');
    await React.act(async () => { select.value = 'boxes'; select.dispatchEvent(new dom.window.Event('change', { bubbles: true })); });
    await click('添加范围'); assert.equal(button('应用范围').disabled, false); await click('应用范围');
    assert.equal(project.state.splitBoxes.length, 1); assert.equal(controls.at(-1).controls.compositionEditor.tool, 'split');
    assert.equal(controls.at(-1).controls.compositionEditor.splitBoxes.length, 1);
    assert.equal(starts, 1); assert.ok(!calls.some(call => ['preview', 'previewEditor'].includes(call.action)));
    assert.ok(!document.querySelector('[role="alert"]'), document.querySelector('[role="alert"]')?.textContent);
  } finally {
    await React.act(async () => { root.unmount(); }); dom.window.close();
    for (const name of names) if (previous[name] === undefined) delete globalThis[name]; else globalThis[name] = previous[name];
  }
});

test('组合页切页只隐藏，保留同一组件、数字草稿与Viewer；最终结果使用真实RenderPage', () => {
  const shell = fs.readFileSync(new URL('../ui/shell/AppShell.tsx', import.meta.url), 'utf8');
  assert.ok(shell.includes('(compositionVisited || route === "composition")'));
  assert.ok(shell.includes('className="workspace-composition-page" hidden={route !== "composition"}'));
  assert.ok(shell.includes('formatImportOpen || route !== "composition"'));
  assert.ok(shell.includes('route === "composition" ? null : requiresProjection(route)'));
  assert.ok(source.includes('useQuickPreview(editorSourcePath,'));
  assert.ok(source.includes('false, editorScenePath || undefined)'));
  assert.ok(source.includes('<RenderPage currentFile={previewPath}'));
  assert.ok(!source.includes('addEventListener("blur"')); // Native child focus is not app focus loss.
});
