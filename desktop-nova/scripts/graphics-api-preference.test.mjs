import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

function configService(store) {
  const exports = {}, calls = [];
  const source = readFileSync(new URL("../src/services/userConfig.ts", import.meta.url), "utf8");
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(code, { exports, require(name) {
    if (name === "./renderMode") return { normalizeDisplayMode: value => value };
    if (name === "./backend") return {
      getUserConfig: async () => ({ config: store.value }),
      saveUserConfig: async patch => { calls.push(JSON.parse(JSON.stringify(patch))); store.value = { ...store.value, ...patch }; return { config: store.value }; },
    };
    throw new Error("Unexpected configuration dependency: " + name);
  }, localStorage: { getItem: () => "1" } });
  return { ...exports, calls };
}

test("图形API缺省auto，DX12与Vulkan独立于旧光影后端值", () => {
  const service = configService({ value: {} });
  assert.equal(service.normalizeGraphicsBackend(undefined), "auto");
  for (const value of ["auto", "dx12", "vulkan"]) assert.equal(service.normalizeGraphicsBackend(value), value);
  for (const value of ["iris-gl46", "full", "fast_experimental", "unsupported"]) assert.equal(service.normalizeGraphicsBackend(value), "auto");
});

test("图形API仅保存自己的原生配置字段，重新读取仍保留且不改光影设置", async () => {
  const oldCompatibility = { backend: "iris-gl46", shaderpack_name: "existing.zip" };
  const store = { value: { v5_compatibility: oldCompatibility } };
  const first = configService(store);
  await first.saveGraphicsBackendConfig("dx12");
  assert.deepEqual(first.calls, [{ graphics_backend: "dx12" }]);
  assert.equal(store.value.v5_compatibility, oldCompatibility);
  const restarted = configService(store);
  const info = await restarted.loadUserConfigMigratingLocalStorage();
  assert.equal(info.config.graphics_backend, "dx12");
  assert.equal(info.config.v5_compatibility.backend, "iris-gl46");
});
