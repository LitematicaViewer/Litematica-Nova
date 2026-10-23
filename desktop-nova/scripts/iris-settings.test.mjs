import assert from 'node:assert/strict';
import { test } from 'node:test';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const fixture = {
  schema: 'nova.iris-settings.v1', root: 'C:/packs', pack: 'example.zip', pack_identity: 'same-pack', packs: [], profiles: [{id:'raw-profile',label:'§6Profile',values:{'raw-key':'§raw-value'}}],
  selected_profile: null, saved_values: {'raw-key':'§raw-value'},
  screens: [{id:'raw-screen',title:'§6体积渲染§r',columns:1,elements:[{kind:'link',screen:'raw-target',label:'§l分类'}]}],
  options: [{id:'raw-key',kind:'string',label:'§f§n按钮悬停',description:'§7A & B; §z remains',default_value:'§raw-value',value:'§raw-value',values:['§raw-value'],value_labels:{'§raw-value':'§aDefault'}}],
};
let sent;
const compiled = ts.transpileModule(fs.readFileSync(new URL('../src/services/irisSettings.ts', import.meta.url), 'utf8'), {compilerOptions:{target:ts.ScriptTarget.ES2020,module:ts.ModuleKind.CommonJS}}).outputText;
const context = {exports:{},require(name) {
  if (name.endsWith('/tauri')) return {invoke:async(_command,args) => {sent=args;return structuredClone(fixture);}};
  if (name.endsWith('/i18n')) return {getUiLocale:()=> 'zh-CN'};
  throw new Error(`Unexpected import ${name}`);
}};
vm.runInNewContext(compiled, context);
const {irisDisplayText,readIrisSettings,saveIrisSettings} = context.exports;
test('display strips supported formatting only, including extended hex sequence', () => {
  assert.equal(irisDisplayText('§f§n按钮悬停 / §6体积渲染§r / §Lbold'), '按钮悬停 / 体积渲染 / bold');
  assert.equal(irisDisplayText('§x§A§0§b§1§C§2color; A & B; §ztext; §'), 'color; A & B; §ztext; §');
});
test('all display surfaces are normalized without altering raw identity or saved values', async() => {
  const view = await readIrisSettings('C:/packs','example.zip');
  assert.equal(view.options[0].label,'按钮悬停');
  assert.equal(view.options[0].description,'A & B; §z remains');
  assert.equal(view.options[0].value_labels['§raw-value'],'Default');
  assert.equal(view.screens[0].title,'体积渲染');
  assert.equal(view.screens[0].elements[0].screen,'raw-target');
  assert.equal(view.options[0].value,'§raw-value');
  assert.equal(view.options[0].default_value,'§raw-value');
  assert.equal(view.profiles[0].values['raw-key'],'§raw-value');
  assert.equal(view.saved_values['raw-key'],'§raw-value');
  await saveIrisSettings('C:/packs','example.zip',{profile:null,values:{'raw-key':'§raw-value'}});
  assert.equal(JSON.parse(sent.args.at(-1)).values['raw-key'],'§raw-value');
});
