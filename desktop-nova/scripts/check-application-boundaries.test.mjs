import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const checkerPath = path.join(scriptDir, "check-application-boundaries.mjs");
const fixtureBase = fs.mkdtempSync(path.join(os.tmpdir(), "nova-application-boundaries-"));

after(() => {
  fs.rmSync(fixtureBase, { recursive: true, force: true });
});

function createFixture(name) {
  const root = path.join(fixtureBase, name);
  fs.mkdirSync(root, { recursive: true });
  return root;
}

function writeFixture(root, relativePath, contents) {
  const filePath = path.join(root, ...relativePath.split("/"));
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, contents, "utf8");
}

function runChecker(root) {
  return spawnSync(process.execPath, [checkerPath, `--root-dir=${root}`], {
    encoding: "utf8",
  });
}

function outputOf(result) {
  return `${result.stdout}\n${result.stderr}`;
}

test("accepts semantic frontend, pure application model, and supervised render service", () => {
  const root = createFixture("clean");
  writeFixture(
    root,
    "desktop-nova/ui/RenderPage.tsx",
    'import type { RenderOperationView } from "../../src/platform/application";\n// Legacy note: import from "../../src/platform/renderService" is forbidden.\nexport const RenderPage = ({ operation }: { operation: RenderOperationView }) => <div>{operation.phase}</div>;\n',
  );
  writeFixture(
    root,
    "desktop-nova/src-tauri/src/application/model.rs",
    "pub enum IrisFallbackPreference { Fail, Vanilla }\npub struct RenderOperationView { pub phase: String }\n",
  );
  writeFixture(
    root,
    "tools/viewer-core/src/render_service.rs",
    "fn request_viewer_launch(host: &HostSupervisor) { host.spawn_viewer(); }\n",
  );
  writeFixture(
    root,
    "tools/viewer-core/src/viewer.rs",
    'const NOTE: &str = "SetParent(viewer, webview) is forbidden";\n// SetParent(viewer, webview);\nfn keep_top_level_window() {}\n',
  );

  const result = runChecker(root);
  assert.equal(result.status, 0, outputOf(result));
  assert.match(result.stdout, /passed with zero violations/);
});

test("rejects frontend render wire imports and renderer runtime details", () => {
  const root = createFixture("frontend-runtime");
  writeFixture(
    root,
    "desktop-nova/ui/RenderPage.tsx",
    'import type { ViewerSnapshot } from "../../../src/platform/renderService";\nexport const RenderPage = ({ viewer }: { viewer: ViewerSnapshot }) => <div>PID {viewer.pid}: {viewer.sectionsUploaded}/{viewer.sectionsGpuAccepted}</div>;\n',
  );

  const result = runChecker(root);
  assert.equal(result.status, 1, outputOf(result));
  assert.match(outputOf(result), /\[frontend-no-render-wire-import\]/);
  assert.match(outputOf(result), /\[frontend-no-render-runtime-contract\]/);
  assert.match(outputOf(result), /ViewerSnapshot/);
  assert.match(outputOf(result), /sectionsUploaded/);
  assert.match(outputOf(result), /sectionsGpuAccepted/);
});

test("rejects framework and renderer dependencies in the application semantic model", () => {
  const root = createFixture("application-model");
  writeFixture(
    root,
    "desktop-nova/src-tauri/src/application/model.rs",
    "use nova_render_protocol::BuildSnapshot;\nuse tauri::AppHandle;\nuse windows::Win32::Foundation::HANDLE;\npub struct State { slot: ResidentSlot }\n",
  );

  const result = runChecker(root);
  assert.equal(result.status, 1, outputOf(result));
  assert.match(outputOf(result), /render protocol dependency/);
  assert.match(outputOf(result), /Tauri dependency/);
  assert.match(outputOf(result), /Windows platform dependency/);
  assert.match(outputOf(result), /renderer runtime type/);
});

test("rejects SetParent and OS child ownership in render service code", () => {
  const root = createFixture("process-ownership");
  writeFixture(
    root,
    "tools/viewer-core/src/render_service.rs",
    'use std::process::{Child, Command};\nfn spawn_and_stop(child: &mut Child) { let _ = Command::new("viewer"); let _ = child.kill(); }\n',
  );
  writeFixture(
    root,
    "tools/viewer-core/src/viewer.rs",
    "fn embed(viewer: HWND, webview: HWND) { unsafe { SetParent(viewer, webview); } }\n",
  );

  const result = runChecker(root);
  assert.equal(result.status, 1, outputOf(result));
  assert.match(outputOf(result), /\[render-service-no-os-process-ownership\]/);
  assert.match(outputOf(result), /child process handles/);
  assert.match(outputOf(result), /request process launch/);
  assert.match(outputOf(result), /request termination/);
  assert.match(outputOf(result), /\[no-cross-process-setparent\]/);
});

test("rejects generic helper and Viewer runtime ownership in frontend code", () => {
  const root = createFixture("frontend-helper-ownership");
  writeFixture(
    root,
    "desktop-nova/src/platform/backend.ts",
    'export const run = () => invoke("execute_backend", { binaryName: "litematica_core.exe" });\nexport const release = () => invoke("application_render_release");\n',
  );

  const result = runChecker(root);
  assert.equal(result.status, 1, outputOf(result));
  assert.match(outputOf(result), /\[frontend-no-helper-or-runtime-ownership\]/);
  assert.match(outputOf(result), /generic helper execution contract/);
  assert.match(outputOf(result), /core helper executable identity/);
  assert.match(outputOf(result), /render-runtime lifecycle command/);
});

test("rejects scene-scale payloads in the host supervision control plane", () => {
  const root = createFixture("host-supervision-data-plane");
  writeFixture(
    root,
    "tools/viewer-core/crates/nova-render-protocol/src/host_supervision.rs",
    "pub struct Spawn { scene: SceneContent, artifact: ArtifactSnapshot, bytes: Vec<u8> }\n",
  );

  const result = runChecker(root);
  assert.equal(result.status, 1, outputOf(result));
  assert.match(outputOf(result), /\[host-supervision-control-plane-only\]/);
  assert.match(outputOf(result), /scene or artifact data-plane type/);
  assert.match(outputOf(result), /opaque or byte-scale payload/);
});

test("rejects render wire DTOs in Tauri product command handlers", () => {
  const root = createFixture("tauri-entrypoint-wire");
  writeFixture(
    root,
    "desktop-nova/src-tauri/src/main.rs",
    "use nova_render_protocol::{RenderPreviewOutput, UiCommand};\nfn preview() -> RenderPreviewOutput { todo!() }\n",
  );

  const result = runChecker(root);
  assert.equal(result.status, 1, outputOf(result));
  assert.match(outputOf(result), /\[tauri-entrypoint-uses-application-api\]/);
  assert.match(outputOf(result), /render wire type/);
});
