import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

function fixture() {
  const values = new Map(), cleanups = [], exports = {};
  let failWrite = false;
  const localStorage = {
    getItem: (key) => values.get(key) || null,
    setItem: (key, value) => { if (failWrite) throw new Error("quota exceeded"); values.set(key, value); },
  };
  const code = ts.transpileModule(fs.readFileSync(new URL("../ui/windows/main/updateRecovery.ts", import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(code, { exports, localStorage, require: (name) => {
    assert.equal(name, "react"); return { useRef: (current) => ({ current }), useEffect: (fn) => cleanups.push(fn()) };
  }});
  return { api: exports, values, cleanups, fail: () => { failWrite = true; } };
}

test("更新草稿：多个页面、当前文件与路由保存后逐个恢复，不恢复其他项目草稿", () => {
  const f = fixture();
  f.api.useUpdateDraft("shell", { route: "properties", currentFile: "one.litematic" });
  f.api.useUpdateDraft("properties:one.litematic", { form: { name: "未保存" } });
  f.api.preserveUpdateDrafts();
  assert.equal(f.api.readUpdateDraft("shell").route, "properties");
  assert.equal(f.api.takeUpdateDraft("properties:one.litematic").form.name, "未保存");
  assert.equal(f.api.takeUpdateDraft("properties:one.litematic"), undefined);
  assert.equal(f.api.readUpdateDraft("properties:two.litematic"), undefined);
  assert.equal(f.api.readUpdateDraft("shell").currentFile, "one.litematic");
});

test("更新草稿：存储失败明确阻止继续安装，卸载页面不捕获陈旧快照", () => {
  const f = fixture(); f.api.useUpdateDraft("generate", { text: "draft" });
  f.cleanups[0](); f.api.preserveUpdateDrafts();
  assert.equal(f.api.readUpdateDraft("generate"), undefined);
  f.fail(); assert.throws(() => f.api.preserveUpdateDrafts(), /quota/);
});

test("取消更新后清理当次恢复快照，后续正常保存不会复活旧修改", () => {
  const f = fixture(); f.api.useUpdateDraft("properties:one", { name: "old draft" });
  f.api.preserveUpdateDrafts();
  assert.equal(f.api.readUpdateDraft("properties:one").name, "old draft");
  f.api.clearCurrentUpdateDrafts();
  assert.equal(f.api.takeUpdateDraft("properties:one"), undefined);
});

test("更新说明：统一六段格式并兼容旧标题，实际Beta说明包含下版平台计划", () => {
  const f = fixture();
  const sections = f.api.releaseNoteSections("版本：0.2.0\n日期：2026-10-12\n## 新增\n- A\n## 修复\n- B");
  assert.equal(sections.map((s) => s.label).join(","), "版本与发布日期,新增功能,功能优化,问题修复,已知问题,后续计划");
  assert.equal(sections[1].lines[0], "A"); assert.equal(sections[3].lines[0], "B");
  assert.equal(f.api.releaseNoteSections("legacy text")[4].lines[0], "legacy text");
  const release = f.api.releaseNoteSections(fs.readFileSync(new URL("../../docs/releases/BETA_0_1.md", import.meta.url), "utf8"));
  assert.ok(release.every((s) => s.lines[0] !== "未提供"));
  assert.ok(release[5].lines.includes("macOS 和 Linux 将在下个版本支持。"));
});

test("更新接口：前端仅走应用命令；官方验证与HTTPS不被旁路", () => {
  const source = fs.readFileSync(new URL("../src-tauri/src/application/updater.rs", import.meta.url), "utf8");
  const config = JSON.parse(fs.readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.ok(config.plugins.updater.pubkey.length > 50);
  assert.equal(config.plugins.updater.endpoints.length, 2);
  assert.ok(config.plugins.updater.endpoints.every((url) => new URL(url).protocol === "https:"));
  assert.match(source, /update\.download\(/);
  assert.match(source, /pending\.update\.install\(/);
  assert.match(source, /https_only\(true\)/);
  assert.match(source, /same_release\(&original\.version, &original\.signature/);
  assert.match(source, /pause_admitting\(\)/);
  const quiesce = source.indexOf("supervisor.stop_admitting();");
  const shutdown = source.indexOf(".close(&view.preview_id)");
  assert.ok(quiesce > 0 && shutdown > quiesce);
  assert.doesNotMatch(source.slice(shutdown, source.indexOf("pending.update.install")), /cancel.changed/);
  assert.doesNotMatch(source, /danger_accept_invalid|verify_signature\s*=\s*false/);
});
