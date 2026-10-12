import assert from "node:assert/strict";
import test from "node:test";
import { harness, text, nextTurn, deferred } from "./fixtures/beta-page-harness.mjs";

const replaceSummary = { block_positions_affected: 64, regions_scanned: 1, warnings: [], per_rule: [] };
const replaceFacade = { getBlockProperties: () => ({}), translateBlockId: (id) => id };

test("替换：预览与应用防重复，存在同名输出时不写入", async () => {
  let previews = 0; let writes = 0; const gate = deferred();
  const h = harness("replace/ReplacePage", "ReplacePage", { ...replaceFacade,
    dryRunReplaceBlocks: async () => { previews++; await gate.promise; return JSON.stringify(replaceSummary); },
    checkFileExists: async () => true, applyReplaceBlocks: async () => { writes++; },
  });
  const props = { currentFile: "C:/fixtures/sample.litematic" };
  h.render(props); h.button("添加规则").props.onClick(); h.render(props);
  const preview = h.button("预览替换影响").props.onClick;
  const first = preview(); await preview(); assert.equal(previews, 1);
  gate.resolve(); await first; h.render(props);
  assert.match(text(h.component("Dialog").props.children), /64 个方块/);
  await h.button("确认替换并另存").props.onClick(); h.render(props);
  assert.equal(writes, 0);
  assert.match(text(h.component("Dialog").props.children), /文件已存在/);
  h.component("Dialog").props.onClose(); h.render(props);
  h.component("RuleRow").props.onRemove(); h.render(props);
  assert.equal(h.button("预览替换影响").props.disabled, true);
});

test("替换：预览固定规则快照，应用等待期间只写一个新文件", async () => {
  let writes = 0; let received; const gate = deferred();
  const h = harness("replace/ReplacePage", "ReplacePage", { ...replaceFacade,
    dryRunReplaceBlocks: async () => JSON.stringify(replaceSummary), checkFileExists: async () => { await gate.promise; return false; },
    applyReplaceBlocks: async (source, output, rules) => { writes++; received = { source, output, rules }; return "ok"; },
  });
  const props = { currentFile: "C:/fixtures/sample.LITEMATIC" };
  h.render(props); h.button("添加规则").props.onClick(); h.render(props);
  await h.button("预览替换影响").props.onClick(); h.render(props);
  const apply = h.button("确认替换并另存").props.onClick;
  const first = apply(); await apply(); gate.resolve(); await first;
  assert.equal(writes, 1); assert.equal(received.output, "C:/fixtures/sample.replaced.litematic");
  assert.equal(received.rules[0].match.name, "minecraft:stone");
});

test("替换：更换文件后丢弃旧预览响应", async () => {
  const gate = deferred();
  const h = harness("replace/ReplacePage", "ReplacePage", { ...replaceFacade, dryRunReplaceBlocks: () => gate.promise });
  const props = { currentFile: "first.litematic" };
  h.render(props); h.button("添加规则").props.onClick(); h.render(props);
  const request = h.button("预览替换影响").props.onClick();
  props.currentFile = "second.litematic"; h.render(props);
  gate.resolve(JSON.stringify(replaceSummary)); await request; h.render(props);
  assert.equal(h.component("Dialog"), undefined);
});

