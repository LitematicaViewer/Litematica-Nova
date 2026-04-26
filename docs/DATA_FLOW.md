# Litematica-BA Data Flow Map / 数据流地图

本文件按数据生命周期说明项目，不按目录说明项目。目标是让维护者能判断一个数据对象从哪里来、经过谁、落在哪里、被谁消费、出问题先查哪一段。

This document describes data lifecycles rather than directory ownership. It inherits the current mainline from `docs/PROJECT_MAP.md` and feature entrypoints from `docs/FEATURE_ENTRYPOINTS.md`.

Legacy cleanup status: `desktop-ui/`, `script/`, and `src/litematicaba/` have been deleted. Treat older references to those paths as historical notes only.

已确认：`desktop-js` 是当前桌面 UI 主线，`tools/viewer-core` 是当前 Rust 后端和 viewer 核心；PySide6 仅暂时保留作参考，正式上线前计划删除。无法确认的部分标注为 `needs confirmation`。

---

### Projection Import Flow / 投影导入数据流

- **Status:**
  - active
- **Trigger:**
  - 用户在属性页选择 `.litematic`。
  - 用户在投影库页面选择或设为当前投影。
  - GeneratePage 生成完成后把输出加入投影库。
- **Input:**
  - 用户选择的 `.litematic` 绝对路径。
  - 当前 `data/projection-library/js_library.json`。
- **Main steps:**
  1. `PropertiesPage` 或 `LibraryPage` 通过 Tauri dialog 得到 `.litematic` 路径。
  2. UI 调用 `setCurrentFile(path)`，把当前文件放入 React app state。
  3. `addOrUpdateRecord(state, path)` 检查文件是否存在。
  4. `libraryStore.analyzeFile()` 调用 `executeBackend("litematica_core.exe", ["analyze", path])`。
  5. Tauri `execute_backend` 启动 `bin/viewer-backend/litematica_core.exe analyze <file>`。
  6. Rust `analyze_litematic()` 解析 NBT/metadata/derived stats。
  7. JS 把结果写入 `data/projection-library/js_library.json`。
- **Output:**
  - React `currentFile`。
  - 投影库 record：path、fileName、displayName、author、description、totalBlocks、totalVolume、regionCount、minecraftDataVersion、status、lastAnalyzedAt。
- **Files / directories touched:**
  - Source/user data: selected `.litematic` file.
  - User/runtime data: `data/projection-library/js_library.json`.
  - Backend binary: `bin/viewer-backend/litematica_core.exe`.
- **Consumers:**
  - `PropertiesPage`, `RenderPage`, `FlakePage`, `GeneratePage`, `LibraryPage`。
- **Invalidation / regeneration:**
  - Re-run import/refresh when source `.litematic` changes.
  - Current JS record stores path/status/analysis but file size/mtime are currently written as `0`; robust stale detection needs confirmation.
  - Import flow does not confirm copying the original file into the library; current JS path stores the original path directly.
- **Debug starting point:**
  - `desktop-js/src/services/libraryStore.ts`
  - Manual CLI: `bin\viewer-backend\litematica_core.exe analyze "<file.litematic>"`
- **Risk notes:**
  - Do not assume `js_library.json` and old Python `index.json` are interchangeable.
  - PySide6 projection library code is reference only and planned for removal.

---

### Projection Metadata Edit Flow / 投影属性和 metadata 数据流

- **Status:**
  - active read/display flow; edit/write-back needs confirmation
- **Trigger:**
  - 用户打开属性页。
  - 用户选择当前 `.litematic`。
- **Input:**
  - React `currentFile`。
  - `.litematic` metadata and region data.
  - latest render cache state for preview display.
- **Main steps:**
  1. `PropertiesPage` detects `currentFile`.
  2. It calls `executeBackend("litematica_core.exe", ["analyze", currentFile])`.
  3. Rust analyze returns JSON.
  4. Properties page renders metadata fields, derived stats, version info, region summary.
  5. It also reads latest cache state from `renderCacheStore` to decide preview/static/embedded state.
- **Output:**
  - Read-only metadata UI in current `PropertiesPage`.
  - Preview state derived from cache store.
- **Files / directories touched:**
  - Source/user data: current `.litematic`.
  - Runtime cache state: localStorage `lba.renderCacheState.v1`.
  - Optional preview data URL from render cache store.
- **Consumers:**
  - User-facing properties UI.
  - Embedded/static preview area.
- **Invalidation / regeneration:**
  - If `.litematic` changes, rerun analyze and rebuild cache/preview.
  - Current UI fields inspected are read-only; confirmed metadata write-back path in `desktop-js` was not found.
  - Whether old PySide6 had metadata write-back behavior is reference-only and needs confirmation if required.
- **Debug starting point:**
  - `desktop-js/src/routes/PropertiesPage.tsx`
  - `tools/viewer-core/src/analyze.rs`
- **Risk notes:**
  - Do not claim metadata edits modify original `.litematic` unless a write-back path is confirmed.
  - Metadata display should not depend on embedded viewer success.

---

### Preview Image Generation Flow / 预览图生成数据流

- **Status:**
  - active fallback/static preview flow
- **Trigger:**
  - `RenderPage` finishes 3D cache build and calls `buildPreview()`.
  - Static preview is shown when embedded viewer is not running or fails.
- **Input:**
  - current `.litematic` path.
  - display mode: `normal`, `fast_experimental`, or `full`.
- **Main steps:**
  1. `RenderPage` reaches cache-ready state.
  2. `buildPreview(file, mode)` calls `renderPreviewImage(file, mode)`.
  3. Tauri `render_preview_image` allocates `.tmp/desktop-js/render/lba_native_preview_*.png`.
  4. Tauri starts `litematica_native_viewer.exe` with `--preview-output=<png> --auto-exit-seconds=4 --basic-lighting --basic-shadows`.
  5. Native viewer renders offscreen/preview output and exits.
  6. Tauri reads the PNG and returns a data URL.
  7. `RenderPage` stores preview data URL in render cache state.
- **Output:**
  - PNG file under `.tmp/desktop-js/render/`.
  - Data URL in JS render cache state.
  - Static preview UI in RenderPage/PropertiesPage.
- **Files / directories touched:**
  - Cache/generated: `.tmp/desktop-js/render/lba_native_preview_*.png`.
  - Runtime state: localStorage `lba.renderCacheState.v1`.
  - Backend: `bin/viewer-backend/litematica_native_viewer.exe`.
- **Consumers:**
  - `RenderPage` preview panel.
  - `PropertiesPage` preview panel.
- **Invalidation / regeneration:**
  - Regenerate when current file or display mode changes, or when user rebuilds cache.
  - Current confirmed keying is by file + display mode in `renderCacheStore`; source file mtime/hash invalidation is needs confirmation.
  - Camera/screenshot parameters are native viewer defaults for preview mode/output; exact camera rules need confirmation in `native_viewer.rs`.
- **Debug starting point:**
  - `desktop-js/src-tauri/src/main.rs` command `render_preview_image`.
  - Manual command: `litematica_native_viewer.exe <file> --display-mode=<mode> --preview-output=<png> --auto-exit-seconds=4`.
- **Risk notes:**
  - Static preview is not interactive and must not be presented as embedded interactive viewer.
  - Preview image generation calls native viewer directly; changing viewer startup flags affects preview output.

---

### 3D Cache Build Flow / 3D 缓存构建数据流

- **Status:**
  - active
- **Trigger:**
  - 用户在 RenderPage 点击“构建 3D cache”。
  - Optional: “同时预生成分层” triggers a layer index check after cache ready.
- **Input:**
  - current `.litematic`.
  - display/build mode.
  - Optional build mode env flags set by Tauri.
- **Main steps:**
  1. `RenderPage.handleBuild()` clears previous UI state.
  2. `startCacheBuildTask(currentFile, buildMode)` calls Tauri.
  3. Tauri creates progress/cache/stdout/stderr paths under `.tmp/desktop-js/render/`.
  4. Tauri starts `litematica_native_viewer.exe` with `--chunk-size=32 --prebuild-before-show --prebuild-only --ready-file=<progress> --cache-file=<cache> --display-mode=<mode>`.
  5. Native viewer parses `.litematic`, builds scene chunks/meshes, writes progress JSON, writes cache manifest and chunk files.
  6. Native viewer writes a layer index sidecar and stores its file name in cache manifest as `layer_index_file`.
  7. `RenderPage` polls `pollCacheBuildTask()` and updates `renderCacheStore`.
  8. When ready, RenderPage optionally calls `litematica_core cache-layer-meta <cache>` to verify layer data.
- **Output:**
  - Progress JSON.
  - Cache manifest JSON.
  - Cache chunk directory.
  - Layer index sidecar.
  - stdout/stderr logs.
  - JS render cache state.
- **Files / directories touched:**
  - `.tmp/desktop-js/render/lba_native_progress_*.json`
  - `.tmp/desktop-js/render/lba_native_cache_*.json`
  - `.tmp/desktop-js/render/lba_native_cache_*.json.chunks/`
  - layer index sidecar next to cache manifest
  - `.tmp/desktop-js/render/lba_native_stdout_*.log`
  - `.tmp/desktop-js/render/lba_native_stderr_*.log`
  - localStorage `lba.renderCacheState.v1`
- **Consumers:**
  - `RenderPage` progress and ready state.
  - `FlakePage` / layer viewer via `cache-layer-meta` and `cache-layer`.
  - Properties page uses cache-ready state and preview data.
- **Invalidation / regeneration:**
  - Rebuild when `.litematic` changes, display mode changes, or cache files are missing/corrupt.
  - Current store is keyed by file + display mode; confirmed source-file hash invalidation was not found.
  - Temp cache under `.tmp` may be cleaned; UI must tolerate missing cache.
- **Debug starting point:**
  - `RenderPage.tsx` polling state.
  - `.tmp/desktop-js/render/*progress*.json`.
  - Native viewer stdout/stderr tails returned by `pollCacheBuildTask()`.
- **Risk notes:**
  - Cache protocol is shared between native viewer writer, `cache_layer.rs`, and JS consumers.
  - Do not change cache manifest fields or layer sidecar format without regression on RenderPage and FlakePage.

---

### 3D Cache Read / Render Flow / 3D 缓存读取和渲染数据流

- **Status:**
  - active for layer/cache metadata; native viewer cache-input path exists but JS popup/embedded reuse needs confirmation
- **Trigger:**
  - RenderPage restores cache state.
  - FlakePage opens and reads layer data.
  - Native viewer can parse `--cache-input=<path>` according to CLI parser; current JS launch path does not pass it.
- **Input:**
  - cache manifest path.
  - chunk directory referenced by manifest.
  - layer index sidecar referenced by `layer_index_file`.
  - current file + display mode for JS cache-state lookup.
- **Main steps:**
  1. `renderCacheStore.hydrateRenderCacheStore()` loads localStorage `lba.renderCacheState.v1`.
  2. RenderPage selects state by current file + display mode.
  3. Layer page gets latest cache state for current file.
  4. `layerService.loadLayerMeta(cacheFile)` calls `litematica_core cache-layer-meta <cache>`.
  5. `cache_layer.rs` opens cache manifest, locates `layer_index_file`, reads layer sidecar.
  6. `loadLayerSlice(cacheFile, y)` calls `litematica_core cache-layer <cache> --y=<Y>`.
- **Output:**
  - Cache-ready UI state.
  - Layer metadata.
  - Layer slice block list.
- **Files / directories touched:**
  - localStorage `lba.renderCacheState.v1`
  - `.tmp/desktop-js/render/lba_native_cache_*.json`
  - cache chunk directory
  - layer index sidecar
- **Consumers:**
  - `RenderPage`
  - `PropertiesPage`
  - `FlakePage`
  - `litematica_core cache-layer*` commands
- **Invalidation / regeneration:**
  - Missing/corrupt cache or missing `layer_index_file` requires rebuilding 3D cache.
  - Stale cache detection beyond file + display mode is needs confirmation.
  - Embedded preview and popup native viewer are gated by cache-ready in some UI paths, but current confirmed Tauri launch commands pass the original `.litematic`, not `--cache-input`.
- **Debug starting point:**
  - `desktop-js/src/services/renderCacheStore.ts`
  - `tools/viewer-core/src/cache_layer.rs`
  - Manual: `litematica_core.exe cache-layer-meta "<cache_file>"`
- **Risk notes:**
  - Do not assume all viewer modes read the prebuilt cache. Confirm command args first.
  - Cache missing should be handled as rebuild-needed, not as projection corruption.

---

### Native Viewer Launch Flow / 原生 viewer 启动数据流

- **Status:**
  - active
- **Trigger:**
  - “打开弹窗 Viewer”。
  - GeneratePage generated output “直接打开 3D 预览”。
  - Embedded preview internally starts native viewer with embed args.
- **Input:**
  - `.litematic` path.
  - display mode.
  - launch flags and env vars.
  - Optional HWND for embedded launch.
- **Main steps:**
  1. UI calls `startNativeViewer(file, displayMode)` or `startEmbeddedViewer(...)`.
  2. Tauri normalizes display mode.
  3. Popup path starts `litematica_native_viewer.exe <file> --display-mode=<mode> --basic-lighting --basic-shadows`.
  4. Embedded path creates child host HWND and starts native viewer with `--embed-parent-hwnd=<child_hwnd>`.
  5. Properties embedded purpose adds `--preview-mode --preview-spin`.
  6. Render embedded purpose omits preview spin for interactive behavior.
  7. Native viewer parses args, loads `.litematic`, builds runtime scene, opens popup or embedded viewport.
- **Output:**
  - Native viewer process/window.
  - Embedded child window.
  - Startup and diagnostic logs.
- **Files / directories touched:**
  - Popup: no confirmed persistent file by default.
  - Embedded: `.tmp/desktop-js/render/lba_embedded_stdout_*.log`, `lba_embedded_stderr_*.log`.
  - Source/user data: current `.litematic`.
- **Consumers:**
  - User interactive viewer.
  - RenderPage/PropertiesPage embedded preview hosts.
- **Invalidation / regeneration:**
  - Restart when file, display mode, or embedded purpose changes.
  - If rect becomes invalid, embedded viewer should hide, not write zero bounds.
- **Debug starting point:**
  - Popup: run the printed/manual native viewer command.
  - Embedded: `[LBA_EMBED_VIEWER]` logs and embedded stdout/stderr tail.
  - Black screen/model missing: `native_viewer.rs`, `full_mode_v2.rs`, `mesh.rs`.
- **Risk notes:**
  - Viewer startup flags affect rendering and interactivity.
  - Do not modify `native_viewer.rs` rendering semantics for a UI lifecycle issue.
  - API and UI code should not assume embedded is available outside Windows.

---

### Layer Viewer Flow / 分层查看数据流

- **Status:**
  - active
- **Trigger:**
  - 用户打开 FlakePage / 分层页.
  - 用户移动 slider or clicks `+`, `++`, `-`, `--`.
- **Input:**
  - latest render cache state for current file.
  - cache manifest path.
  - target layer Y.
  - block icon library.
- **Main steps:**
  1. `FlakePage` subscribes to render cache store.
  2. It finds latest cache for current file and checks cache file exists.
  3. `loadLayerMeta(cacheFile)` calls `litematica_core cache-layer-meta <cache>`.
  4. `cache_layer.rs` loads manifest and layer sidecar.
  5. Layer controls update `layerY`.
  6. `loadLayerSlice(cacheFile, layerY)` calls `litematica_core cache-layer <cache> --y=<layerY>`.
  7. Canvas draws non-air blocks from returned layer data.
- **Output:**
  - Layer metadata: size, palette, property pool.
  - Current layer block list.
  - Canvas image and hover info.
- **Files / directories touched:**
  - cache manifest and layer sidecar under `.tmp/desktop-js/render/`.
  - `block/*.png` for icons/texture-like display where consumed by UI components.
- **Consumers:**
  - `FlakePage` canvas.
- **Invalidation / regeneration:**
  - Rebuild 3D cache if manifest has no `layer_index_file`, sidecar is missing, or source file/display mode changed.
  - Neighbor/background preloading strategy beyond direct current-layer load is needs confirmation; current inspected path loads requested layer on `layerY` change.
- **Debug starting point:**
  - `desktop-js/src/routes/FlakePage.tsx`
  - `desktop-js/src/services/layerService.ts`
  - `tools/viewer-core/src/cache_layer.rs`
- **Risk notes:**
  - Layer buttons change Y layer, not zoom.
  - Layer data comes from cache layer sidecar, not directly from `.litematic` in the current UI path.

---

### AI Projection Plan Flow / AI 投影 plan 数据流

- **Status:**
  - experimental active
- **Trigger:**
  - GeneratePage “复制给网页端 AI 的完整提示词”。
  - GeneratePage “Mock 生成示例方案”。
  - GeneratePage API 对话弹窗 send.
  - User pastes AI response and clicks parse/preview.
- **Input:**
  - user natural language prompt.
  - `data/ai-projection/prompt_config.md`.
  - current `projection_plan.json` from GeneratePage form.
  - optional saved AI config/key in Tauri app config.
  - AI raw response text.
- **Main steps:**
  1. Wrapped prompt path calls `buildWebPrompt()` / `buildWrappedPrompt()`.
  2. Prompt includes prompt_config, current context, current plan, user request, operation/material lists.
  3. Web workflow copies prompt; API workflow sends messages through Tauri `ai_chat_completion`.
  4. Raw AI response returns as text.
  5. `extractPlanFromAiText()` accepts pure JSON, fenced ```json blocks, or first embedded JSON object.
  6. `normalizeAiPlanForImport()` calls blockstate default completion and geometry normalization.
  7. `validateAiPlanBasic()` produces structural errors.
  8. `getAiPlanWarnings()` produces warnings for DB unknowns, size/operation limits, etc.
  9. If no structural errors, `applyAiPlanToGenerateForm()` fills GeneratePage form.
  10. User must manually run dry-run and apply.
- **Output:**
  - normalized plan preview.
  - errors/warnings.
  - GeneratePage form state.
  - no `.litematic` until user runs apply.
- **Files / directories touched:**
  - Source config: `data/ai-projection/prompt_config.md`.
  - Reads generated DB: `data/minecraft_blockstates/26.1.json`.
  - API key storage: Tauri app config path, not localStorage.
  - No plan/template/prompt file should contain API key.
- **Consumers:**
  - GeneratePage form.
  - `dryRunGenerate()` and `applyGenerate()` after user action.
- **Invalidation / regeneration:**
  - Rebuild prompt whenever current plan/context/user prompt changes.
  - Re-parse and re-normalize whenever pasted AI text changes.
  - No automatic dry-run or apply; this is intentional.
- **Debug starting point:**
  - `desktop-js/src/services/aiProjection.ts`
  - `desktop-js/src/routes/GeneratePage.tsx`
  - Tauri `ai_chat_completion` for API provider issues.
- **Risk notes:**
  - API key must not enter frontend logs, localStorage, prompts, messages copied to web AI, plans, or templates.
  - Chinese block IDs/properties are structural errors.
  - Unknown `minecraft:*` blocks/properties are warnings unless structurally invalid.

---

### Projection Plan Apply Flow / plan 生成 `.litematic` 数据流

- **Status:**
  - active
- **Trigger:**
  - GeneratePage “Dry-run 预览”.
  - GeneratePage “生成 .litematic”.
- **Input:**
  - validated/normalized plan from GeneratePage.
  - selected output path.
  - operation/material definitions inside plan.
- **Main steps:**
  1. `buildProjectionPlan(form)` converts form state to plan JSON.
  2. `validatePlanClientSide(plan)` checks basic structure and bounds.
  3. `dryRunGenerate(plan)` writes temp plan under `.tmp/desktop-js/generate/`.
  4. Tauri `execute_backend_trace` runs `litematica_core.exe generate --plan <temp> --dry-run`.
  5. Rust `generate_projection::generate_projection()` parses and validates plan.
  6. Dry-run returns JSON summary.
  7. Apply validates output path and runs `generate --plan <temp> --output <out.litematic>`.
  8. Backend writes `.litematic` and verifies load.
  9. UI calls analyze and adds output to projection library.
- **Output:**
  - Dry-run JSON summary.
  - `.litematic` output file.
  - Analyze output.
  - Projection library record for generated file.
  - Raw trace shown in UI.
- **Files / directories touched:**
  - `.tmp/desktop-js/generate/projection_plan_*.json`
  - user-selected output `.litematic`
  - `data/projection-library/js_library.json`
- **Consumers:**
  - Projection library.
  - Properties page.
  - Render page/cache builder/native viewer.
- **Invalidation / regeneration:**
  - Regenerate output when plan changes.
  - Current UI blocks default overwrite of existing output path.
  - Dry-run summary becomes stale when form changes; GeneratePage clears it on edits.
- **Debug starting point:**
  - `[LBA_JS_GENERATE_TRACE]` in UI raw output.
  - temp plan path logged by `generateService`.
  - `tools/viewer-core/src/generate_projection.rs`.
- **Risk notes:**
  - Common failures: invalid block/material shape, unsupported operation type, out-of-bounds coordinates, radius/height outside region, output path exists, backend parse errors.
  - Do not skip backend dry-run; frontend validation is not the final authority.

---

### Minecraft BlockState DB Flow / Minecraft 方块状态 DB 数据流

- **Status:**
  - generated data pipeline
- **Trigger:**
  - Maintainer runs generation scripts.
  - Frontend app startup reads generated DB.
- **Input:**
  - Vanilla Minecraft version directory/JAR, for example `D:\.minecraft\versions\26.1`.
  - `assets/minecraft/blockstates/*.json` from the JAR.
  - Override source: `data/minecraft_blockstates/overrides/26.1.json`.
  - i18n maps embedded in `scripts/generate_minecraft_blockstate_i18n.py`.
- **Main steps:**
  1. `generate_minecraft_blockstate_db.py` locates the version JAR.
  2. It extracts blockstate JSON files.
  3. It infers property keys/values and default properties.
  4. It merges overrides for runtime/special states not present in blockstate JSON.
  5. It writes `data/minecraft_blockstates/26.1.json`.
  6. `generate_minecraft_blockstate_i18n.py` reads the DB.
  7. It writes `26.1.zh_cn.json` and `26.1.zh_cn.missing.json`.
  8. `App` calls `loadDatabases()` and frontend services consume the generated DB.
- **Output:**
  - Generated BlockState DB.
  - Generated Chinese display DB.
  - Missing translation report.
- **Files / directories touched:**
  - Source input: `data/minecraft_blockstates/overrides/26.1.json`.
  - Generated: `data/minecraft_blockstates/26.1.json`.
  - Generated: `data/minecraft_blockstates/26.1.zh_cn.json`.
  - Generated report: `data/minecraft_blockstates/26.1.zh_cn.missing.json`.
- **Consumers:**
  - `desktop-js/src/services/blockstateDb.ts`
  - `GeneratePage` block/material selectors.
  - `ReplacePage` and block selectors.
  - AI plan validation/warnings and default state normalization.
- **Invalidation / regeneration:**
  - After changing overrides or generator script, regenerate both DB and i18n output.
  - Do not manually patch only generated `26.1.json` or `26.1.zh_cn.json` as the final fix.
- **Debug starting point:**
  - `scripts/generate_minecraft_blockstate_db.py`
  - `scripts/generate_minecraft_blockstate_i18n.py`
  - `desktop-js/src/services/blockstateDb.ts`
- **Risk notes:**
  - Unknown-state warnings should not be globally disabled.
  - Overrides should only add confirmed legal Minecraft states.

---

### Lighting / Shader Experiment Data Flow / 光照和 Shader 实验数据流

- **Status:**
  - experimental / partially deprecated
- **Trigger:**
  - native viewer launch flags.
  - environment variables.
  - internal viewer defaults.
- **Input:**
  - `--basic-lighting`, `--basic-shadows`, `--shadow-debug`, `--shadow-debug-view`.
  - Env flags such as `LBA_VIEWER_BUILD_MODE`, `LBA_ENABLE_COMPACT_CACHE_V2`, `LBA_VIEWER_BASIC_LIGHTING`.
  - Runtime assets under `third_party/render-assets/`.
- **Main steps:**
  1. Tauri or manual command starts native viewer with flags.
  2. `native_viewer.rs` parses CLI args and env.
  3. Viewer builds scene/materials through normal render path.
  4. Lighting/shadow/tone config affects shader uniforms/rendering.
  5. Debug helpers may print startup diagnostics.
- **Output:**
  - Viewer-side shaded render.
  - Debug stdout logs such as lighting/tone/shadow diagnostics.
- **Files / directories touched:**
  - Usually no persistent files.
  - Reads `third_party/render-assets/`.
  - May write stdout/stderr logs when launched by Tauri cache/embedded paths.
- **Consumers:**
  - Native viewer popup.
  - Embedded viewer.
  - Preview output.
- **Invalidation / regeneration:**
  - Not a generated data pipeline by default.
  - Changing render assets or flags requires viewer preview regression.
  - Whether any remaining MC Light prototype data participates in normal cache/render pipeline: `needs confirmation`; docs indicate MC Light/block-light voxel propagation is removed/deprecated from the normal path.
- **Debug starting point:**
  - `tools/viewer-core/src/native_viewer.rs`
  - README viewer lighting handoff notes.
- **Risk notes:**
  - Treat MC Light, point shadows, and shadow map prototypes as experimental unless explicitly revalidated.
  - Do not mix experimental lighting paths into stable cache/render pipeline without an explicit task.

---

### Diagnostics / Logs / Trace Flow / 诊断、日志和 trace 数据流

- **Status:**
  - active ad hoc diagnostics
- **Trigger:**
  - UI actions.
  - Tauri command execution.
  - native viewer startup/cache/embedded runs.
  - generation scripts.
- **Input:**
  - action parameters, file paths, env vars, backend args.
- **Main steps:**
  1. UI services log frontend prefixes such as `[LBA_RENDER_MODE]` and `[LBA_JS_GENERATE_TRACE]`.
  2. Tauri bridge logs embedded viewer lifecycle with `[LBA_EMBED_VIEWER]`.
  3. Tauri captures backend stdout/stderr for trace commands and cache tasks.
  4. Native viewer writes startup/progress/cache/render diagnostics.
  5. Scripts print generation summaries to terminal.
- **Output:**
  - Browser/dev console logs.
  - Tauri stdout/stderr.
  - `.tmp/desktop-js/render/*.log`.
  - progress JSON.
  - script terminal output.
- **Files / directories touched:**
  - `.tmp/desktop-js/render/lba_native_stdout_*.log`
  - `.tmp/desktop-js/render/lba_native_stderr_*.log`
  - `.tmp/desktop-js/render/lba_embedded_stdout_*.log`
  - `.tmp/desktop-js/render/lba_embedded_stderr_*.log`
  - `.tmp/desktop-js/render/lba_native_progress_*.json`
  - `.tmp/assistant/lba_assistant_index.sqlite`
- **Consumers:**
  - Developers/maintainers.
  - RenderPage polling UI.
  - Assistant index query workflow.
- **Invalidation / regeneration:**
  - Logs under `.tmp` are disposable.
  - Rebuild assistant index after meaningful docs/code changes.
  - Centralized log retention/cleanup policy: `needs confirmation`.
- **Debug starting point:**
  - Use prefix to select path:
    - `[LBA_EMBED_VIEWER]` for embedded bounds/process/lifecycle.
    - `[LBA_RENDER_MODE]` for display mode persistence.
    - `[LBA_JS_GENERATE_TRACE]` for GeneratePage backend command/temp plan.
    - `[NATIVE_VIEWER]`, `[VIEWER_CACHE]`, `[PREBUILD_TAIL]`, `[TRACE_SUMMARY]` for native viewer/cache.
- **Risk notes:**
  - Do not remove diagnostic logs casually; many are current troubleshooting entry points.
  - API keys must never appear in logs, prompt copies, localStorage, plan JSON, template JSON, stdout, or stderr.
