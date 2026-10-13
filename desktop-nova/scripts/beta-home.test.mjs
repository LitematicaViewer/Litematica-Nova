import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { harness } from "./fixtures/beta-page-harness.mjs";

test("首页：打开投影保留文件选择、属性跳转与入库调用链，取消不修改状态", async () => {
  let picked = "C:/fixtures/sample.litematic";
  const calls = [];
  const library = {};
  const h = harness("home/HomePage", "HomePage", {
    selectLitematicFile: async () => picked,
    loadLibrary: async () => library,
    addOrUpdateRecord: async (state, path) => { assert.equal(state, library); calls.push(["record", path]); },
  });
  h.render({ currentFile: "", setCurrentFile: (path) => calls.push(["file", path]), setRoute: (route) => calls.push(["route", route]) });
  await h.button("打开投影文件").props.onClick();
  assert.deepEqual(calls, [["file", picked], ["route", "properties"], ["record", picked]]);
  calls.length = 0; picked = null;
  await h.button("打开投影文件").props.onClick();
  assert.deepEqual(calls, []);
});

test("首页：继续操作只使用真实页面路由，无文件时禁用投影操作", () => {
  const routes = [];
  const h = harness("home/HomePage", "HomePage");
  const props = { currentFile: "", setCurrentFile() {}, setRoute: (route) => routes.push(route) };
  h.render(props);
  for (const label of ["查看投影属性", "查看材料与统计", "打开三维预览"]) {
    assert.equal(h.nodes().find((node) => node.props?.["aria-label"] === label).props.disabled, true);
  }
  h.button("浏览投影库").props.onClick();
  h.render({ ...props, currentFile: "C:\\fixtures\\中文投影.litematic" });
  for (const label of ["查看投影属性", "查看材料与统计", "打开三维预览"]) {
    const button = h.nodes().find((node) => node.props?.["aria-label"] === label);
    assert.equal(button.props.disabled, false);
    button.props.onClick();
  }
  assert.deepEqual(routes, ["library", "properties", "statistics", "render"]);
  assert.ok(h.nodes().some((node) => node.type === "strong" && node.props.children === "中文投影.litematic"));
});

test("首页、生成、替换使用共享控件与单一主滚动区，不留固定内联样式", () => {
  for (const page of ["home/HomePage", "generate/GeneratePage", "replace/ReplacePage"]) {
    const source = readFileSync(new URL(`../ui/windows/main/pages/${page}.tsx`, import.meta.url), "utf8");
    assert.doesNotMatch(source, /style=|<(?:button|input|select|textarea)\b/);
    assert.match(source, /<ScrollArea\b/);
  }
});
