import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import React from "react";
import ts from "typescript";

const text = name => readFileSync(new URL(name, import.meta.url), "utf8");
function moduleFrom(source, dependencies) {
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.React, esModuleInterop: true } }).outputText;
  const module = { exports: {} };
  vm.runInNewContext(`(function(require,module,exports){${compiled}\n})`, { URL, console, React })((name) => dependencies(name), module, module.exports);
  return module.exports;
}
const service = moduleFrom(text("../src/services/redenLibrary.ts"), () => ({}));
const providers = moduleFrom(text("../src/services/onlineProjectionProviders.ts"), () => ({}));
const source = text("../ui/windows/reden_library/RedenLibraryWindow.tsx");
const label = node => node == null || typeof node === "boolean" ? "" : Array.isArray(node) ? node.map(label).join("") : typeof node === "object" ? label(node.props?.children) : String(node);
const tick = () => new Promise(resolve => setImmediate(resolve));

function harness({ attachment, output, emitError, importError } = {}) {
  const machine = { key: "test", name: "真实附件", attachments: [{ name: "scene.litematic", url: attachment }] };
  const calls = { downloaded: 0, imported: 0, emitted: 0, opened: [] };
  const facade = { ...service,
    searchRedenLitematica: async () => ({ d: [machine] }), fetchRedenMachineDetail: async () => machine,
    loadLibrary: async () => ({ records: [], folders: [] }),
    importDownloadedLitematic: async () => { calls.imported++; if (importError) throw new Error(importError); },
    downloadRedenAttachment: async () => { calls.downloaded++; return output ?? { kind: "downloaded", path: "C:/scene.litematic", file_name: "scene.litematic", bytes: 123 }; },
    openExternalPath: async url => calls.opened.push(url),
  };
  const slots = []; let cursor = 0; let tree;
  const hooks = { ...React,
    useState(initial) { const i = cursor++; if (!(i in slots)) slots[i] = typeof initial === "function" ? initial() : initial; return [slots[i], next => { slots[i] = typeof next === "function" ? next(slots[i]) : next; }]; },
    useEffect() {}, useLayoutEffect() {},
  };
  const exports = moduleFrom(`${source}\nexport { RedenLibraryPanel };`, name => {
    if (name === "react") return hooks;
    if (name.endsWith("src/business/facade")) return facade;
    if (name.endsWith("src/business/reden")) return providers;
    if (name.endsWith("platform/events")) return { emitEvent: async () => { calls.emitted++; if (emitError) throw new Error(emitError); } };
    if (name.endsWith("libraryEvents")) return { projectionLibraryImportedEvent: "library-imported" };
    if (name.endsWith("Dialog")) return { Dialog: "dialog" };
    if (name.endsWith("button")) return { Button: "button" };
    if (name.endsWith("input")) return { Input: "input" };
    if (name.endsWith("scroll-area")) return { ScrollArea: "scroll" };
    return {};
  });
  const render = () => { cursor = 0; tree = exports.RedenLibraryPanel({}); };
  const nodes = node => node == null || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(child => nodes(child)) : [node, ...nodes(node.props?.children), ...nodes(node.props?.footer)];
  const button = name => nodes(tree).find(node => node.type === "button" && label(node) === name);
  const settle = async () => { await tick(); render(); await tick(); render(); };
  const selectAttachment = async () => { await button("搜索").props.onClick(); await settle(); const result = nodes(tree).find(node => node.props?.className?.includes("reden-result-card")); await result.props.onClick(); await settle(); };
  render();
  return { calls, nodes: () => nodes(tree), button, settle, selectAttachment, content: () => label(tree) };
}

test("Reden 子窗口获得事件及系统浏览器权限", () => {
  const capability = JSON.parse(text("../src-tauri/capabilities/default.json"));
  assert(capability.windows.includes("reden-library"));
  for (const permission of ["core:event:allow-emit", "core:event:allow-listen", "shell:default"]) assert(capability.permissions.includes(permission));
});

test("真实网盘域名可识别，伪域名/HTTP/凭据 URL 不当作网盘", () => {
  assert.equal(service.redenExternalDriveLink("https://pan.quark.cn/s/abc").provider, "quark");
  assert.equal(service.redenExternalDriveLink("https://pan.baidu.com/s/abc").provider, "baidu");
  for (const url of ["https://pan.baidu.com.evil.test/s/abc", "http://pan.quark.cn/s/abc", "https://user:pass@pan.baidu.com/s/abc", "javascript:alert(1)", "not a url"]) assert.equal(service.redenExternalDriveLink(url), null);
});

test("下载已入库后广播失败只提示刷新，不伪报下载失败", async () => {
  const h = harness({ emitError: "event denied" }); await h.selectAttachment();
  await h.button("下载并入库").props.onClick(); await h.settle();
  assert.equal(h.calls.downloaded, 1); assert.equal(h.calls.imported, 1); assert.equal(h.calls.emitted, 1);
  assert(h.content().includes("已下载并入库")); assert(h.content().includes("刷新通知失败")); assert(!h.content().includes("RedenMC 下载失败"));
});

test("下载已落盘而入库失败明确保留文件，不伪报网络失败", async () => {
  const h = harness({ importError: "library unavailable" }); await h.selectAttachment();
  await h.button("下载并入库").props.onClick(); await h.settle();
  assert(h.content().includes("已下载 scene.litematic，但入库失败")); assert(h.content().includes("C:/scene.litematic")); assert.equal(h.calls.emitted, 0);
});

test("附件网盘先确认，取消不打开、不下载、不入库", async () => {
  const h = harness({ attachment: "https://pan.quark.cn/s/abc" }); await h.selectAttachment();
  await h.button("下载并入库").props.onClick(); await h.settle();
  assert(h.button("确认并打开系统浏览器")); assert.equal(h.calls.downloaded, 0); assert.equal(h.calls.opened.length, 0);
  h.button("取消").props.onClick(); await h.settle();
  assert.equal(h.calls.opened.length, 0); assert.equal(h.calls.imported, 0);
});

test("后台重定向网盘也先确认，确认后仅系统浏览器打开", async () => {
  const h = harness({ output: { kind: "external_link", provider: "baidu", url: "https://pan.baidu.com/s/abc" } }); await h.selectAttachment();
  await h.button("下载并入库").props.onClick(); await h.settle();
  assert.equal(h.calls.opened.length, 0); await h.button("确认并打开系统浏览器").props.onClick(); await h.settle();
  assert.deepEqual(h.calls.opened, ["https://pan.baidu.com/s/abc"]); assert.equal(h.calls.imported, 0);
});

test("八个来源区分 Reden API 与原站网页，不冒充已接入搜索下载", async () => {
  assert.equal(providers.ONLINE_PROJECTION_PROVIDERS.length, 8);
  assert.equal(providers.ONLINE_PROJECTION_PROVIDERS.filter(item => item.mode === "reden_api").length, 1);
  for (const item of providers.ONLINE_PROJECTION_PROVIDERS) assert.equal(new URL(item.url).protocol, "https:");
  const h = harness(); h.button("Minemev").props.onClick(); await h.settle();
  assert(h.content().includes("仅网页访问")); assert.equal(h.button("搜索"), undefined);
  h.button("在系统浏览器打开原站").props.onClick(); await h.settle();
  assert.equal(h.calls.opened.length, 0);
  await h.button("确认并打开系统浏览器").props.onClick(); await h.settle();
  assert.deepEqual(h.calls.opened, ["https://www.minemev.com/"]);
});
