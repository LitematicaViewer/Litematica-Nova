# Litematica-BA Feature Entrypoints / 功能入口地图

本文件按“功能”组织入口，不按目录组织。它用于回答：用户点击某个按钮、运行某条命令或生成某份数据后，主要调用链是什么，输入输出是什么，出问题先查哪里。

This document maps features to their user-facing entry points, implementation files, call chains, inputs, outputs, touched data, and debugging entry points.

Legacy cleanup status: `desktop-ui/`, `script/`, and `src/litematicaba/` have been deleted. Current UI entry points are under `desktop-js/`; current maintenance scripts are under `scripts/`.

已继承 `docs/PROJECT_MAP.md` 的主线判断，并结合用户确认补充：**PySide6 仅暂时保留作行为参考，真正上线会删除。** 因此本文把 PySide6 相关路径标记为 legacy reference，不作为当前上线主线。`desktop-ui` 的当前角色仍为 **needs confirmation**。

---

## App Startup / 应用启动

- **Status:** active
- **User-facing entry:** `desktop-js` dev/build/Tauri 启动；侧栏进入各页面。
- **Main implementation files:**
  - `desktop-js/src/main.tsx`
  - `desktop-js/src/App.tsx`
  - `desktop-js/src-tauri/src/main.rs`
  - `desktop-js/src/styles/base.css`
- **Call chain:**
  - React mount -> `App`
  - `App` 初始化 theme、currentFile、route、sidebar
  - `loadDatabases()` 读取 BlockState DB
  - `initI18n()` 初始化 UI/资源翻译
  - 用户点侧栏 -> 渲染对应 route component
- **Input:** localStorage theme, BlockState DB files, route state, currentFile state.
- **Output:** 桌面 UI、当前路由页面、主题 class。
- **Data touched:** localStorage `theme`; reads `data/minecraft_blockstates/26.1.json`, `26.1.zh_cn.json`; reads icons from `src/litematicaba/ui/resources/icon/*.svg`.
- **Debug starting point:** `desktop-js/src/App.tsx`
- **Risk notes:** `App.tsx` 是全局 currentFile/route/theme 汇合点；改这里会影响所有页面。
- **Unknowns:** `desktop-ui` 是否还有独立启动链路：needs confirmation.

---

## Projection Library Import / 投影库导入

- **Status:** active, with legacy reference
- **User-facing entry:** 投影库页面选择 `.litematic`；卡片“设为当前”“刷新校验”“打开文件夹”等动作。
- **Main implementation files:**
  - `desktop-js/src/routes/LibraryPage.tsx`
  - `desktop-js/src/services/libraryStore.ts`
  - `desktop-js/src/services/backend.ts`
  - `desktop-js/src-tauri/src/main.rs`
  - `tools/viewer-core/src/analyze.rs`
  - legacy reference: `src/litematicaba/core/projection_library.py`
- **Call chain:**
  - `LibraryPage` opens Tauri file dialog
  - selected path -> `setCurrentFile`
  - `addOrUpdateRecord()` -> `analyzeFile()`
  - `executeBackend("litematica_core.exe", ["analyze", file])`
  - Tauri `execute_backend` -> Rust `litematica_core analyze`
  - save JS library JSON
- **Input:** selected `.litematic` path.
- **Output:** currentFile, projection record, analysis metadata in library JSON.
- **Data touched:** `data/projection-library/js_library.json`; selected source `.litematic`; optional preview paths.
- **Debug starting point:** `desktop-js/src/services/libraryStore.ts`, then CLI `bin/viewer-backend/litematica_core.exe analyze <file>`.
- **Risk notes:** library schema compatibility affects existing user records; PySide6 path is reference only and expected to be removed before real release.
- **Unknowns:** old `data/projection-library/index.json` vs `js_library.json` ownership needs confirmation.

---

## Projection Metadata / 属性编辑

- **Status:** active
- **User-facing entry:** 属性页 / Properties page.
- **Main implementation files:**
  - `desktop-js/src/routes/PropertiesPage.tsx`
  - `desktop-js/src/services/statsService.ts`
  - `desktop-js/src/services/libraryStore.ts`
  - `desktop-js/src/services/backend.ts`
  - `tools/viewer-core/src/analyze.rs`
  - `tools/viewer-core/src/stats_api.rs`
- **Call chain:**
  - currentFile changes -> Properties page reads preview/cache state
  - stats/material requests -> `statsService` or `executeBackend`
  - Tauri bridge -> `litematica_core analyze`, `stats`, or `materials`
  - UI renders metadata/stat/material result
- **Input:** current `.litematic`; cache state when preview is available.
- **Output:** metadata, stats, material views, preview fallback state.
- **Data touched:** reads projection file; may read `data/projection-library/js_library.json`; uses `.tmp/desktop-js/render/*` preview/cache outputs indirectly.
- **Debug starting point:** run `bin/viewer-backend/litematica_core.exe analyze <file>` and compare to `PropertiesPage.tsx`.
- **Risk notes:** do not couple metadata display to embedded viewer success; popup/static fallback should remain separate.
- **Unknowns:** exact legacy PySide6 property editing parity is reference-only and not release-blocking unless explicitly requested.

---

## Preview Image Generation / 预览图生成

- **Status:** active fallback path
- **User-facing entry:** 渲染页构建 cache 后自动生成静态预览；属性页/渲染页嵌入失败时显示静态 preview。
- **Main implementation files:**
  - `desktop-js/src/routes/RenderPage.tsx`
  - `desktop-js/src/services/backend.ts`
  - `desktop-js/src-tauri/src/main.rs`
  - `tools/viewer-core/src/native_viewer.rs`
- **Call chain:**
  - `RenderPage` cache ready -> `buildPreview()`
  - `renderPreviewImage(file, mode)`
  - Tauri `render_preview_image`
  - spawn `litematica_native_viewer.exe <file> --display-mode=<mode> --preview-output=<png> --auto-exit-seconds=4 --basic-lighting --basic-shadows`
  - Tauri reads PNG and returns data URL
- **Input:** currentFile, displayMode.
- **Output:** PNG preview path and data URL.
- **Data touched:** `.tmp/desktop-js/render/lba_native_preview_*.png`; stdout/stderr returned in command result.
- **Debug starting point:** manually run native viewer with `--preview-output`.
- **Risk notes:** static preview must not be described as interactive embedded preview.
- **Unknowns:** none known.

---

## Embedded 3D Preview / 嵌入式 3D 预览

- **Status:** active on Windows, experimental/sensitive lifecycle
- **User-facing entry:** 属性页右侧预览框；渲染页预览框 after cache ready.
- **Main implementation files:**
  - `desktop-js/src/routes/PropertiesPage.tsx`
  - `desktop-js/src/routes/RenderPage.tsx`
  - `desktop-js/src/services/embeddedViewer.ts`
  - `desktop-js/src-tauri/src/main.rs`
  - `tools/viewer-core/src/native_viewer.rs`
- **Call chain:**
  - page container ref -> DOM rect
  - `elementToPhysicalRect()` converts CSS px to physical px
  - `startEmbeddedViewer(file, mode, rect, purpose)`
  - Tauri creates Windows child host HWND
  - spawn native viewer with `--embed-parent-hwnd=<child_hwnd>`
  - page ResizeObserver/scroll/resize -> `updateEmbeddedViewerBounds`
- **Input:** currentFile, displayMode, DOM rect, purpose `properties_preview` or `render_interactive`.
- **Output:** embedded native viewer child window; fallback static preview if failed.
- **Data touched:** `.tmp/desktop-js/render/lba_embedded_stdout_*.log`, `lba_embedded_stderr_*.log`; embedded state in Tauri memory.
- **Debug starting point:** `[LBA_EMBED_VIEWER]` logs: parent_hwnd, child_hwnd, rect, pid, visible, purpose, command_args.
- **Risk notes:** bounds/lifecycle changes can cause black screen, floating child window, or page overlay leaks. Keep popup viewer unaffected.
- **Unknowns:** non-Windows embedded behavior is intentionally unsupported.

---

## Native Viewer Launch / 原生 viewer 启动

- **Status:** active
- **User-facing entry:** “打开弹窗 Viewer”“直接打开 3D 预览”等按钮。
- **Main implementation files:**
  - `desktop-js/src/services/backend.ts`
  - `desktop-js/src-tauri/src/main.rs`
  - `tools/viewer-core/src/bin/litematica_native_viewer.rs`
  - `tools/viewer-core/src/native_viewer.rs`
- **Call chain:**
  - UI button -> `startNativeViewer(filePath, displayMode)`
  - Tauri `start_native_viewer`
  - spawn `bin/viewer-backend/litematica_native_viewer.exe`
  - args: file, `--display-mode=<mode>`, `--basic-lighting`, `--basic-shadows`
  - native viewer creates window and handles interaction
- **Input:** `.litematic` path, displayMode.
- **Output:** independent native viewer process/window.
- **Data touched:** no persistent JS data by default; native viewer may log to stdout/stderr.
- **Debug starting point:** run the same exe command manually from repo root.
- **Risk notes:** do not change native viewer rendering semantics for UI launch bugs.
- **Unknowns:** none known.

---

## 3D Cache Build / 3D 缓存构建

- **Status:** active
- **User-facing entry:** 渲染页“构建 3D cache”；可选“同时预生成分层”。
- **Main implementation files:**
  - `desktop-js/src/routes/RenderPage.tsx`
  - `desktop-js/src/services/renderCacheStore.ts`
  - `desktop-js/src/services/backend.ts`
  - `desktop-js/src-tauri/src/main.rs`
  - `tools/viewer-core/src/native_viewer.rs`
  - `tools/viewer-core/src/build_mode.rs`
- **Call chain:**
  - button -> `handleBuild()`
  - `startCacheBuildTask(currentFile, buildMode)`
  - Tauri spawns native viewer with `--prebuild-before-show --prebuild-only --ready-file=<progress> --cache-file=<cache>`
  - UI polls `pollCacheBuildTask()`
  - progress JSON updates `renderCacheStore`
  - ready -> optional preview image generation
- **Input:** currentFile, displayMode/buildMode, optional fast-mode env flags.
- **Output:** cache manifest, chunk cache directory, progress/ready JSON, stdout/stderr logs, render cache store state.
- **Data touched:** `.tmp/desktop-js/render/lba_native_progress_*.json`, `lba_native_cache_*.json`, cache chunk directory, stdout/stderr logs; localStorage `lba.renderCacheState.v1`.
- **Debug starting point:** `RenderPage.tsx` polling state, then `.tmp/desktop-js/render/*progress*.json` and native viewer stderr tail.
- **Risk notes:** cache protocol affects render page, layer page, and native cache reads. Verify all consumers after changes.
- **Unknowns:** long-term cache file location/persistence policy may need confirmation.

---

## 3D Cache Read / 3D 缓存读取

- **Status:** active
- **User-facing entry:** 渲染页 restores previous cache state; 分层页 reads cache layer index.
- **Main implementation files:**
  - `desktop-js/src/services/renderCacheStore.ts`
  - `desktop-js/src/routes/RenderPage.tsx`
  - `desktop-js/src/routes/FlakePage.tsx`
  - `desktop-js/src/services/layerService.ts`
  - `tools/viewer-core/src/cache_layer.rs`
  - `tools/viewer-core/src/native_viewer.rs`
- **Call chain:**
  - `hydrateRenderCacheStore()` reads localStorage state
  - Render page applies state by current file + display mode
  - Layer page gets latest cache state for file
  - `loadLayerMeta()` / `loadLayerSlice()` call `litematica_core cache-layer-meta/cache-layer`
  - Rust reads cache manifest and layer sidecar index
- **Input:** cache manifest path, current file, display mode, target Y.
- **Output:** UI cache status; layer metadata; layer block list.
- **Data touched:** localStorage `lba.renderCacheState.v1`; `.tmp/desktop-js/render/*cache*.json`; chunk cache directory; layer index sidecar.
- **Debug starting point:** verify cache file exists, then run `litematica_core.exe cache-layer-meta <cache_file>`.
- **Risk notes:** stale localStorage can point at missing temp cache files; UI must handle missing cache.
- **Unknowns:** whether cache should survive app restart as a supported feature needs confirmation.

---

## Layer Viewer / 分层查看

- **Status:** active
- **User-facing entry:** 分层页 / FlakePage.
- **Main implementation files:**
  - `desktop-js/src/routes/FlakePage.tsx`
  - `desktop-js/src/services/layerService.ts`
  - `desktop-js/src/components/BlockIcon.tsx`
  - `desktop-js/src/services/blockIconResolver.ts`
  - `tools/viewer-core/src/cache_layer.rs`
- **Call chain:**
  - page subscribes to render cache store
  - if ready -> `loadLayerMeta(cacheFile)`
  - slider/buttons update `layerY`
  - `loadLayerSlice(cacheFile, layerY)`
  - canvas draws slice using palette/block icon/color data
- **Input:** cacheFile, target layer Y, block icon library.
- **Output:** canvas layer view, hovered block info.
- **Data touched:** cache manifest/layer sidecar; block icon PNGs under `block/`.
- **Debug starting point:** `FlakePage.tsx` ready/cache state, then `litematica_core.exe cache-layer <cache_file> --y=<Y>`.
- **Risk notes:** `+`, `++`, `-`, `--` are layer controls, not zoom controls. Zoom/pan should remain mouse-driven.
- **Unknowns:** exact long-term naming of “FlakePage” vs “LayerPage” needs confirmation.

---

## AI Projection Generator / AI 生成投影

- **Status:** experimental active
- **User-facing entry:** GeneratePage “智能生成（网页端）”；Mock 生成；API 对话弹窗.
- **Main implementation files:**
  - `desktop-js/src/routes/GeneratePage.tsx`
  - `desktop-js/src/services/aiProjection.ts`
  - `desktop-js/src/services/generateService.ts`
  - `data/ai-projection/prompt_config.md`
  - `desktop-js/src-tauri/src/main.rs`
- **Call chain:**
  - user prompt -> `buildWebPrompt()` or API chat wrapping
  - AI text -> `extractPlanFromAiText()`
  - `normalizeAiPlanForImport()`
  - `validateAiPlanBasic()` + warnings
  - apply -> `applyAiPlanToGenerateForm()` -> GeneratePage form
- **Input:** natural language prompt, current plan, prompt_config, optional API provider config.
- **Output:** normalized `projection_plan.json` in UI form; no automatic `.litematic` generation.
- **Data touched:** reads `data/ai-projection/prompt_config.md`; may read BlockState DB; no API key in prompt/localStorage.
- **Debug starting point:** `desktop-js/src/services/aiProjection.ts`, especially parse -> normalize -> validate.
- **Risk notes:** AI must not automatically write `.litematic`; Chinese must not enter block ids/properties; warning-only plans may apply, structural errors must block.
- **Unknowns:** provider behavior beyond Mock/OpenAI-compatible remains experimental; Gemini is reserved/not implemented.

---

## API Chat Modal / API 对话弹窗

- **Status:** experimental active
- **User-facing entry:** GeneratePage “使用 API 对话” button.
- **Main implementation files:**
  - `desktop-js/src/routes/GeneratePage.tsx`
  - `desktop-js/src/services/aiProjection.ts`
  - `desktop-js/src/services/backend.ts`
  - `desktop-js/src-tauri/src/main.rs`
  - `desktop-js/src/routes/SettingsPage.tsx`
- **Call chain:**
  - button opens modal
  - send button builds message: wrapped prompt by default, raw text when “更改” is on
  - `sendAiChatMessage()` -> Tauri `ai_chat_completion`
  - Tauri reads saved AI config/key
  - Mock returns sample plan or OpenAI-compatible calls `/chat/completions`
  - UI parses returned plan and applies if no structural errors
- **Input:** chat history, user message, current plan, saved AI config/key.
- **Output:** chat messages; optional generated plan applied to form; no dry-run/apply automation.
- **Data touched:** AI config at Tauri app config path; GeneratePage state; no localStorage API key.
- **Debug starting point:** `handleAiChatSend()` in `GeneratePage.tsx`, then Tauri `ai_chat_completion`.
- **Risk notes:** API key must not be logged, sent to frontend, inserted into prompt, or saved to localStorage.
- **Unknowns:** exact provider-specific response parsing for non-OpenAI APIs needs confirmation.

---

## Projection Plan Validation / plan 校验

- **Status:** active
- **User-facing entry:** GeneratePage live plan preview, AI import “解析并预览”, Dry-run precheck.
- **Main implementation files:**
  - `desktop-js/src/services/generateService.ts`
  - `desktop-js/src/services/aiProjection.ts`
  - `desktop-js/src/services/blockstateDb.ts`
  - `docs/projection_plan_schema.md`
- **Call chain:**
  - form -> `buildProjectionPlan()`
  - client structural check -> `validatePlanClientSide()`
  - AI import -> `extractPlanFromAiText()`
  - `normalizePlanBlockStates()` and `normalizeAiPlanGeometry()`
  - `validateAiPlanBasic()` and `getAiPlanWarnings()`
- **Input:** plan JSON, current form, BlockState DB, AI output text.
- **Output:** error list, warning list, normalized plan preview.
- **Data touched:** reads `data/minecraft_blockstates/26.1.json`; reads i18n DB indirectly through selectors.
- **Debug starting point:** reproduce with pasted plan text in `aiProjection.ts` parse/normalize/validate path.
- **Risk notes:** unknown `minecraft:*` block/state should be warning, not fatal; non-string/empty block names and Chinese in IDs/properties remain structural errors.
- **Unknowns:** backend and frontend validation parity is not fully guaranteed; dry-run remains final authority.

---

## Projection Plan Apply / plan 生成 `.litematic`

- **Status:** active
- **User-facing entry:** GeneratePage “Dry-run 预览” then “生成 .litematic”.
- **Main implementation files:**
  - `desktop-js/src/routes/GeneratePage.tsx`
  - `desktop-js/src/services/generateService.ts`
  - `desktop-js/src/services/libraryStore.ts`
  - `desktop-js/src-tauri/src/main.rs`
  - `tools/viewer-core/src/generate_projection.rs`
  - `tools/viewer-core/src/main.rs`
- **Call chain:**
  - `handleDryRun()` -> `dryRunGenerate(plan)`
  - write temp plan under `.tmp/desktop-js/generate/`
  - `executeBackendTrace("litematica_core.exe", ["generate", "--plan", temp, "--dry-run"])`
  - `handleApply()` validates output path
  - `applyGenerate()` -> `generate --plan temp --output out.litematic`
  - backend verifies load, UI adds output to projection library
- **Input:** normalized plan, output path.
- **Output:** dry-run JSON summary, `.litematic`, analyze output, projection library record.
- **Data touched:** `.tmp/desktop-js/generate/projection_plan_*.json`; output `.litematic`; `data/projection-library/js_library.json`.
- **Debug starting point:** raw `[LBA_JS_GENERATE_TRACE]` and the temp plan path; then run the same `litematica_core.exe generate` command manually.
- **Risk notes:** output overwrite is intentionally blocked unless backend force is explicitly used elsewhere; do not silently overwrite user files.
- **Unknowns:** none known.

---

## Minecraft BlockState DB Generation / 方块状态 DB 生成

- **Status:** generated data pipeline
- **User-facing entry:** maintenance script, not UI.
- **Main implementation files:**
  - `scripts/generate_minecraft_blockstate_db.py`
  - `data/minecraft_blockstates/overrides/26.1.json`
  - `data/minecraft_blockstates/26.1.json`
- **Call chain:**
  - script reads Minecraft version dir/JAR
  - extracts `assets/minecraft/blockstates/*.json`
  - infers properties/defaults
  - merges `overrides/<version>.json`
  - writes generated DB
- **Input:** Minecraft version directory or JAR; override JSON.
- **Output:** `data/minecraft_blockstates/26.1.json` and summary counts.
- **Data touched:** generated DB file; override file as source input.
- **Debug starting point:** run `python scripts/generate_minecraft_blockstate_db.py --version-dir "D:\.minecraft\versions\26.1"`.
- **Risk notes:** do not hand-patch generated `26.1.json` as the only fix; update generator/override and regenerate.
- **Unknowns:** exact installed Minecraft version path is machine-specific.

---

## Blockstate i18n Generation / 方块状态中文名生成

- **Status:** generated data pipeline
- **User-facing entry:** maintenance script, not UI.
- **Main implementation files:**
  - `scripts/generate_minecraft_blockstate_i18n.py`
  - `data/minecraft_blockstates/26.1.json`
  - `data/minecraft_blockstates/26.1.zh_cn.json`
  - `data/minecraft_blockstates/26.1.zh_cn.missing.json`
- **Call chain:**
  - script reads generated BlockState DB
  - applies key/value translation maps
  - writes zh_cn JSON
  - writes missing report
  - GeneratePage/block selectors read DB through `blockstateDb.ts`
- **Input:** `26.1.json`, translation maps in script.
- **Output:** `26.1.zh_cn.json`, `26.1.zh_cn.missing.json`.
- **Data touched:** generated i18n DB and missing report.
- **Debug starting point:** run `python scripts/generate_minecraft_blockstate_i18n.py --db data/minecraft_blockstates/26.1.json --output data/minecraft_blockstates/26.1.zh_cn.json --report data/minecraft_blockstates/26.1.zh_cn.missing.json`.
- **Risk notes:** generated i18n output should be regenerated, not manually patched in isolation.
- **Unknowns:** full upstream language-file integration status needs confirmation.

---

## Lighting / MC Light / Shadow Experiments / 光照、MC Light、阴影实验

- **Status:** experimental / partially deprecated
- **User-facing entry:** native viewer flags/env vars; no stable JS UI entry confirmed for MC Light.
- **Main implementation files:**
  - `tools/viewer-core/src/native_viewer.rs`
  - `tools/viewer-core/src/full_mode_v2.rs`
  - `tools/viewer-core/src/build_mode.rs`
  - `README.md`
- **Call chain:**
  - native viewer parses flags/env
  - scene/materials loaded
  - lighting/shadow/tone config applied in viewer render path
  - debug logs print selected lighting/tone/shadow info
- **Input:** viewer flags such as `--basic-lighting`, `--basic-shadows`; env debug flags.
- **Output:** shaded viewer output and debug logs.
- **Data touched:** no normal persistent data; uses render assets from `third_party/render-assets/`.
- **Debug starting point:** `native_viewer.rs` startup logs and README handoff notes.
- **Risk notes:** PROJECT_MAP says MC Light/block-light voxel propagation is removed/deprecated from normal runtime path. Do not re-enable or alter MC Light behavior without explicit task and regression plan.
- **Unknowns:** exact remaining prototype flags are needs confirmation before documenting as supported UI behavior.

---

## Diagnostics / Logs / Debug Flags / 诊断、日志、调试开关

- **Status:** active ad hoc diagnostics
- **User-facing entry:** console logs, native viewer stdout/stderr, Tauri command traces, env vars.
- **Main implementation files:**
  - `desktop-js/src-tauri/src/main.rs`
  - `desktop-js/src/services/generateService.ts`
  - `desktop-js/src/routes/RenderPage.tsx`
  - `desktop-js/src/routes/PropertiesPage.tsx`
  - `tools/viewer-core/src/native_viewer.rs`
  - `tools/viewer-core/src/build_mode.rs`
- **Call chain:**
  - UI/service logs with prefixes such as `[LBA_RENDER_MODE]`, `[LBA_JS_GENERATE_TRACE]`
  - Tauri logs embedded viewer and process status
  - native viewer writes progress/diagnostic logs
  - RenderPage polls stdout/stderr tails for cache build
- **Input:** env vars, file paths, viewer flags, active UI actions.
- **Output:** browser/dev console logs, Tauri stdout, `.tmp/desktop-js/render/*.log`, progress JSON.
- **Data touched:** `.tmp/desktop-js/render/lba_native_stdout_*.log`, `lba_native_stderr_*.log`, embedded stdout/stderr logs, progress JSON.
- **Debug starting point:** identify prefix first: `[LBA_EMBED_VIEWER]`, `[LBA_RENDER_MODE]`, `[LBA_JS_GENERATE_TRACE]`, `[VIEWER_CACHE]`, `[NATIVE_VIEWER]`, `[PREBUILD_TAIL]`.
- **Risk notes:** do not remove noisy-looking logs unless the task is specifically log cleanup; many logs are active diagnostics for viewer/cache lifecycle.
- **Unknowns:** a centralized diagnostics policy does not appear to exist.

---

## Assistant Index Scripts / Assistant 索引脚本

- **Status:** active maintenance tooling
- **User-facing entry:** command line scripts.
- **Main implementation files:**
  - `scripts/build_assistant_index.py`
  - `scripts/query_assistant_index.py`
  - `.tmp/assistant/lba_assistant_index.sqlite`
- **Call chain:**
  - run build script
  - script scans repository files/docs/symbols/passages
  - writes SQLite FTS index
  - query script searches symbols/anchors/passages
  - maintainer uses results before reading code directly
- **Input:** repository files and docs.
- **Output:** `.tmp/assistant/lba_assistant_index.sqlite`; query results.
- **Data touched:** `.tmp/assistant/`.
- **Debug starting point:** rebuild index, then query with simple terms.
- **Risk notes:** FTS can error on punctuation such as `-` or `/`; simplify queries instead of treating it as missing implementation.
- **Unknowns:** none known.
