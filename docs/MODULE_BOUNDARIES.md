# Litematica-BA Module Boundaries / 模块边界与风险区

本文件回答维护问题：看到一个 bug 或需求时，哪些地方可以改，哪些地方不能乱改，改完必须验证什么。

This document is for maintenance decisions. It defines module ownership, allowed changes, forbidden changes, regression checks, and debugging entry points. It is not an architecture pitch.

Legacy cleanup status: `desktop-ui/`, `script/`, and `src/litematicaba/` have been deleted. Restore them from the cleanup backup only for historical comparison.

Maintainer entry / 维护入口：`MAINTAINERS.md`.

当前主线继承自 `docs/PROJECT_MAP.md`、`docs/FEATURE_ENTRYPOINTS.md`、`docs/DATA_FLOW.md`：`desktop-js` 是活跃 UI 主线，`tools/viewer-core` 是 Rust 后端/viewer 核心。PySide6 仅暂时保留作参考，正式上线前计划删除。

---

## Quick Boundary Table / 快速总表

| Area | Status | Safe to edit? | Needs regression? | First doc to read |
|---|---|---|---|---|
| viewer-core mesh / full mode / block model pipeline | stable core | Only with explicit rendering task | Yes: cargo + focused viewer fixtures | `docs/PROJECT_MAP.md`, `docs/DATA_FLOW.md` |
| native viewer | stable core | High risk | Yes: popup, preview-output, embedded if touched | `docs/FEATURE_ENTRYPOINTS.md` |
| 3D cache protocol / manifest / layer sidecar | stable core | High risk | Yes: RenderPage + FlakePage + cache CLI | `docs/DATA_FLOW.md` |
| projection library index schema | stable core / user data | Carefully | Yes: existing library records | `docs/DATA_FLOW.md` |
| `.litematic` parse/write/generate | stable core | Carefully | Yes: analyze + generate dry-run/apply + load verify | `docs/FEATURE_ENTRYPOINTS.md` |
| BlockState DB generator / overrides | generated data pipeline | Source files yes; generated files no | Yes: regenerate DB/i18n + UI validation | `docs/DATA_FLOW.md` |
| AI plan schema / validation / normalization | product/service boundary | Carefully | Yes: parse/normalize/warnings/dry-run | `docs/FEATURE_ENTRYPOINTS.md` |
| desktop-js pages | product layer | Usually yes | Yes: affected user path + build | `docs/FEATURE_ENTRYPOINTS.md` |
| Tauri command bridge | bridge | Carefully | Yes: command args + error paths | `docs/DATA_FLOW.md` |
| data/projection-library | user/project data | Avoid schema churn | Yes if schema changes | `docs/DATA_FLOW.md` |
| `.tmp/desktop-js` previews/cache/logs | cache/generated | Safe to delete; do not depend on permanence | Rebuild cache/preview | `docs/DATA_FLOW.md` |
| `bin/viewer-backend/*.exe` | binary output | Do not hand-edit | Rebuild/copy from Rust release | `docs/PROJECT_MAP.md` |
| lighting / MC Light / shadows | experimental | Only explicit experiment task | Yes: viewer visual regression | `docs/DATA_FLOW.md` |
| PySide6 / `src/litematicaba` | removed legacy | No | Restore from backup only if explicitly needed | `docs/CLEANUP_POLICY.md` |
| `desktop-ui`, `script/` | removed legacy | No | Restore from backup only if explicitly needed | `docs/CLEANUP_POLICY.md` |

---

## Stable Core / 高风险稳定核心

### viewer-core mesh / full mode / block model pipeline

- **Status:** stable core
- **Purpose:** Convert `.litematic` region/palette/blockstate data into renderable mesh/model/material output. Full Mode V2 handles state-aware blockstate -> model -> quads, material atlas, special families, fluids/templates, and culling behavior.
- **Key files:**
  - `tools/viewer-core/src/mesh.rs`
  - `tools/viewer-core/src/full_mode.rs`
  - `tools/viewer-core/src/full_mode_v2.rs`
  - `tools/viewer-core/src/model.rs`
  - `tools/viewer-core/src/visual.rs`
- **Inputs:** parsed `.litematic`, block states, palettes, chunk size, display mode, vanilla blockstate/model/texture resources, render assets.
- **Outputs:** mesh chunks, material slots, visual/layer structures, viewer runtime geometry.
- **Allowed changes:**
  - Fix a confirmed block-family rendering bug with a narrow fixture.
  - Add a specific template/special-case path when the task explicitly requires it.
  - Add targeted debug logs behind env flags.
- **Forbidden changes:**
  - Do not change culling/model/material semantics to fix a UI lifecycle bug.
  - Do not change vanilla blockstate/model resource source without explicit task.
  - Do not mix MC Light/shadow experiments into mesh/full-mode baseline.
  - Do not broad-refactor block-family logic while fixing one family.
- **Required regression checks:**
  - `cargo fmt`
  - `cargo check`
  - relevant `cargo test`
  - focused native viewer preview command for the affected fixture.
  - If cache/layer output can change, also verify `cache-layer-meta` and `cache-layer`.
- **Common failure modes:**
  - Missing/fallback textures.
  - Wrong culling/occlusion.
  - Block state variant fails to select model refs.
  - Water/fluid/transparent blocks disappearing.
  - UV/rotation wrong for special families.
- **Debug starting point:** `tools/viewer-core/src/full_mode_v2.rs`, then `mesh.rs`; use env-specific debug logs only for the affected family.
- **Notes / unknowns:** Some historical debug paths remain; whether every env flag is still current needs confirmation.

### native viewer

- **Status:** stable core
- **Purpose:** Runs popup viewer, embedded viewer child process, preview-output renderer, and cache prebuild process.
- **Key files:**
  - `tools/viewer-core/src/native_viewer.rs`
  - `tools/viewer-core/src/bin/litematica_native_viewer.rs`
  - `desktop-js/src-tauri/src/main.rs`
- **Inputs:** `.litematic`, display mode, CLI flags, env vars, optional HWND, optional preview/cache/progress paths.
- **Outputs:** native window, embedded child window, preview PNG, cache manifest/chunks, progress JSON, stdout/stderr logs.
- **Allowed changes:**
  - Add or fix launch argument parsing when required.
  - Fix viewer-only lifecycle/resize/render bugs with focused reproduction.
  - Add diagnostics that preserve existing behavior.
- **Forbidden changes:**
  - Do not alter rendering semantics for JS page layout bugs.
  - Do not change popup viewer behavior while fixing embedded viewer unless required.
  - Do not add MC Light or experimental shadows into default path.
- **Required regression checks:**
  - popup viewer launch.
  - `--preview-output` command.
  - cache prebuild command if cache code touched.
  - embedded viewer on Windows if HWND path touched.
- **Common failure modes:**
  - Black screen after resize.
  - window appears outside target rect.
  - preview PNG missing or blank.
  - GPU/canvas surface lost.
  - process exits with stderr.
- **Debug starting point:** `[NATIVE_VIEWER]`, `[VIEWER_CACHE]`, `[PREBUILD_TAIL]`, `[TRACE_SUMMARY]`, plus `[LBA_EMBED_VIEWER]` from Tauri.
- **Notes / unknowns:** Non-Windows embedded support is intentionally absent.

### 3D cache protocol / manifest / geometry cache / visual metadata

- **Status:** stable core
- **Purpose:** Persist prebuilt viewer geometry/cache artifacts and layer sidecar data for UI consumers.
- **Key files:**
  - `tools/viewer-core/src/native_viewer.rs`
  - `tools/viewer-core/src/cache_layer.rs`
  - `tools/viewer-core/src/build_mode.rs`
  - `desktop-js/src/services/renderCacheStore.ts`
  - `desktop-js/src/services/layerService.ts`
- **Inputs:** `.litematic`, display mode, chunk size, cache/progress paths.
- **Outputs:** cache manifest JSON, chunk cache directory, `layer_index_file` sidecar, progress JSON, localStorage render cache state.
- **Allowed changes:**
  - Add backwards-compatible metadata fields.
  - Improve error messages for missing/corrupt cache.
  - Fix layer sidecar generation/reading with explicit version handling.
- **Forbidden changes:**
  - Do not rename/remove manifest fields consumed by `cache_layer.rs` or JS without migration.
  - Do not rely on `.tmp` cache permanence.
  - Do not hide corrupt-cache errors as empty layers.
- **Required regression checks:**
  - RenderPage cache build.
  - `poll_cache_build_task` progress.
  - `litematica_core.exe cache-layer-meta <cache>`.
  - `litematica_core.exe cache-layer <cache> --y=<Y>`.
  - FlakePage layer display.
- **Common failure modes:**
  - manifest missing `layer_index_file`.
  - chunk directory missing.
  - stale localStorage points to deleted `.tmp` file.
  - cache built for different display mode/file.
- **Debug starting point:** `.tmp/desktop-js/render/*progress*.json`, `cache_layer.rs`, `renderCacheStore.ts`.
- **Notes / unknowns:** Long-term cache invalidation beyond file + display mode needs confirmation.

### projection library index schema

- **Status:** stable core / user data
- **Purpose:** Store imported/generated projection records and current library metadata for the JS UI.
- **Key files:**
  - `desktop-js/src/services/libraryStore.ts`
  - `desktop-js/src/routes/LibraryPage.tsx`
  - legacy reference: `src/litematicaba/core/projection_library.py`
- **Inputs:** `.litematic` paths, analyze output, existing `js_library.json`.
- **Outputs:** projection records in `data/projection-library/js_library.json`.
- **Allowed changes:**
  - Add optional fields with fallback defaults.
  - Improve refresh/validation behavior.
  - Add migration only when old records are preserved.
- **Forbidden changes:**
  - Do not break existing `js_library.json`.
  - Do not assume Python library schema is active mainline.
  - Do not delete or rewrite user library records without explicit migration.
- **Required regression checks:**
  - Import existing `.litematic`.
  - Refresh/reanalyze.
  - Set current projection.
  - GeneratePage output auto-adds to library.
- **Common failure modes:**
  - parse error stored as record status.
  - original file missing.
  - schema mismatch with older data.
- **Debug starting point:** `data/projection-library/js_library.json`, `libraryStore.ts`.
- **Notes / unknowns:** Relationship between `index.json` and `js_library.json` needs confirmation.

### `.litematic` parse / write / apply core

- **Status:** stable core
- **Purpose:** Read, analyze, generate, verify, and write `.litematic` files.
- **Key files:**
  - `tools/viewer-core/src/nbt.rs`
  - `tools/viewer-core/src/analyze.rs`
  - `tools/viewer-core/src/generate_projection.rs`
  - `tools/viewer-core/src/storage.rs`
  - `tools/viewer-core/src/main.rs`
  - `tools/viewer-core/src/cli.rs`
- **Inputs:** `.litematic`, projection plan JSON, output path, replace rules if used.
- **Outputs:** analyze/stats JSON, generated `.litematic`, dry-run summary, verify-load result.
- **Allowed changes:**
  - Add a new plan operation with tests.
  - Improve validation error messages.
  - Fix NBT parse/write bug with fixture coverage.
- **Forbidden changes:**
  - Do not change `.litematic` output semantics to satisfy frontend display-only issues.
  - Do not default-overwrite user output files.
  - Do not reorder palette/air assumptions without tests.
- **Required regression checks:**
  - `cargo test`
  - `litematica_core.exe analyze <file>`
  - `generate --dry-run --json`
  - `generate --output <out.litematic>`
  - analyze generated output.
- **Common failure modes:**
  - invalid region size.
  - operation out of bounds.
  - unsupported operation.
  - invalid material/block shape.
  - output path exists.
- **Debug starting point:** temp plan from `[LBA_JS_GENERATE_TRACE]`, then `generate_projection.rs`.
- **Notes / unknowns:** Some older Python parse/write utilities may exist only as reference.

### blockstate DB generator / override / generated DB

- **Status:** generated data pipeline
- **Purpose:** Build frontend block/property/value database and Chinese display mapping from Minecraft assets plus overrides.
- **Key files:**
  - `scripts/generate_minecraft_blockstate_db.py`
  - `scripts/generate_minecraft_blockstate_i18n.py`
  - `data/minecraft_blockstates/overrides/26.1.json`
  - generated: `data/minecraft_blockstates/26.1.json`
  - generated: `data/minecraft_blockstates/26.1.zh_cn.json`
- **Inputs:** Minecraft version JAR/assets, override JSON, translation maps.
- **Outputs:** generated DB, generated zh_cn DB, missing report.
- **Allowed changes:**
  - Update overrides with confirmed legal Minecraft states.
  - Improve generator merge behavior.
  - Improve i18n maps.
- **Forbidden changes:**
  - Do not hand-edit generated DB as the only fix.
  - Do not mark every unknown property/value as valid.
  - Do not put Chinese into plan JSON block IDs/properties.
- **Required regression checks:**
  - regenerate DB.
  - regenerate i18n.
  - JSON load checks.
  - GeneratePage/AI validation: known state no warning, unknown state still warning.
- **Common failure modes:**
  - valid runtime state absent from vanilla blockstate JSON.
  - override not merged.
  - stale generated DB loaded by frontend.
- **Debug starting point:** `overrides/26.1.json`, then generator scripts, then `blockstateDb.ts`.
- **Notes / unknowns:** Upstream language-file integration completeness needs confirmation.

### AI projection plan schema / validation / normalization / apply bridge

- **Status:** stable contract with experimental AI producers
- **Purpose:** Keep `projection_plan.json` as local contract, validate AI/manual plans, normalize safe defaults, and map plans to GeneratePage form/backend generate.
- **Key files:**
  - `docs/projection_plan_schema.md`
  - `desktop-js/src/services/aiProjection.ts`
  - `desktop-js/src/services/generateService.ts`
  - `desktop-js/src/routes/GeneratePage.tsx`
  - `tools/viewer-core/src/generate_projection.rs`
- **Inputs:** pasted AI JSON, API response text, current form plan, BlockState DB.
- **Outputs:** errors/warnings, normalized plan preview, GeneratePage form state, dry-run/apply input.
- **Allowed changes:**
  - Add validation warning for a confirmed risky pattern.
  - Add normalization for missing safe default properties.
  - Add operation support only if backend schema supports it.
- **Forbidden changes:**
  - Do not allow AI to auto-generate `.litematic`.
  - Do not put API keys in prompts/messages/logs.
  - Do not convert all warnings into hard errors or disable warnings globally.
  - Do not accept Chinese block IDs/properties.
- **Required regression checks:**
  - pure JSON parse.
  - fenced JSON parse.
  - embedded JSON extraction.
  - invalid JSON error.
  - structural error blocks apply.
  - warning-only plan can apply.
  - dry-run after apply.
- **Common failure modes:**
  - checkerboard material shape misread.
  - unknown blockstate warning.
  - sphere/ring out of bounds.
  - API returns natural language around JSON.
- **Debug starting point:** `aiProjection.ts`, then `generateService.ts`.
- **Notes / unknowns:** Provider-specific behavior beyond OpenAI-compatible and Mock needs confirmation.

---

## Product / UI Layer

### desktop-js app startup

- **Status:** product layer
- **Purpose:** Own global route/currentFile/theme state and load frontend DB/i18n.
- **Key files:** `desktop-js/src/main.tsx`, `desktop-js/src/App.tsx`, `desktop-js/src/styles/base.css`
- **Inputs:** localStorage theme, BlockState DB, i18n resources.
- **Outputs:** rendered page, sidebar route, app theme, shared props.
- **Allowed changes:** navigation labels, page wiring, state handoff, theme persistence bug fixes.
- **Forbidden changes:** do not put backend business rules or viewer/render semantics into `App.tsx`.
- **Required regression checks:** app starts, theme persists, route switching works, currentFile survives route navigation.
- **Common failure modes:** route renders wrong page, currentFile lost, DB not loaded before selectors.
- **Debug starting point:** `desktop-js/src/App.tsx`
- **Notes / unknowns:** Some UI text appears mojibake in terminal; inspect in editor if needed.

### GeneratePage

- **Status:** product layer
- **Purpose:** Let user edit plan form, load templates, run AI import/chat, dry-run, apply, and open generated output.
- **Key files:** `desktop-js/src/routes/GeneratePage.tsx`, `generateService.ts`, `aiProjection.ts`, `generationTemplates.ts`
- **Inputs:** form fields, templates, AI text, output path, currentFile.
- **Outputs:** plan JSON preview, dry-run summary, generated `.litematic`, library record.
- **Allowed changes:** form UI, template UX, warnings display, user confirmation flow.
- **Forbidden changes:** do not implement backend operation semantics in UI; do not bypass backend dry-run/apply.
- **Required regression checks:** build plan, parse AI plan, apply to form, dry-run, generate output, add to library.
- **Common failure modes:** form-state mismatch with plan schema, stale dry-run summary, output path blocked, warning/error confusion.
- **Debug starting point:** `GeneratePage.tsx`, then `generateService.ts`.
- **Notes / unknowns:** Large page; prefer moving reusable logic to services instead of adding more inline business logic.

### API chat modal

- **Status:** product layer / experimental
- **Purpose:** UI wrapper for saved AI provider chat; parse returned plan and optionally fill GeneratePage.
- **Key files:** `GeneratePage.tsx`, `aiProjection.ts`, `backend.ts`, Tauri `main.rs`
- **Inputs:** messages, current plan, prompt_config wrapper mode, saved provider config.
- **Outputs:** chat history, parsed plan, warnings/errors, form update.
- **Allowed changes:** modal layout, send/apply UX, error display.
- **Forbidden changes:** do not expose saved API key to frontend; do not auto dry-run/apply.
- **Required regression checks:** Mock provider, wrapped prompt mode, raw mode, parse fenced JSON, error plan not applied.
- **Common failure modes:** API error hidden, key leaked in UI/log, AI text not parseable.
- **Debug starting point:** `handleAiChatSend()` and Tauri `ai_chat_completion`.
- **Notes / unknowns:** Gemini-compatible is reserved/not implemented.

### projection library page

- **Status:** product layer
- **Purpose:** Display, filter, sort, refresh, remove, and select projection records.
- **Key files:** `LibraryPage.tsx`, `libraryStore.ts`
- **Inputs:** `js_library.json`, currentFile, file existence checks.
- **Outputs:** library cards, selected currentFile, refreshed records.
- **Allowed changes:** filters, sort display, card UI, validation feedback.
- **Forbidden changes:** do not silently rewrite schema or delete user records.
- **Required regression checks:** load existing library, set current, refresh validate, remove record.
- **Common failure modes:** missing original file, parse_error status, hidden import button behavior.
- **Debug starting point:** `LibraryPage.tsx`.
- **Notes / unknowns:** import UX may be partly hidden to match current design; needs confirmation before redesign.

### property / metadata UI

- **Status:** product layer
- **Purpose:** Display metadata/stats and preview area for current projection.
- **Key files:** `PropertiesPage.tsx`, `statsService.ts`, `libraryStore.ts`
- **Inputs:** currentFile, analyze output, render cache state.
- **Outputs:** metadata UI, region/version display, preview fallback/embedded state.
- **Allowed changes:** read-only display, error clarity, preview status messages.
- **Forbidden changes:** do not claim edit/write-back support unless implemented; do not make metadata display depend on embedded viewer.
- **Required regression checks:** select file, analyze display, static fallback, popup button, route away hides embedded.
- **Common failure modes:** analyze parse error, stale cache preview, embedded overlay drift.
- **Debug starting point:** `PropertiesPage.tsx`.
- **Notes / unknowns:** metadata write-back path in desktop-js not confirmed.

### preview UI

- **Status:** product layer
- **Purpose:** Display static preview or embedded viewer state in RenderPage/PropertiesPage.
- **Key files:** `RenderPage.tsx`, `PropertiesPage.tsx`, `renderService.ts`, `backend.ts`
- **Inputs:** preview data URL, embedded status, cache-ready state.
- **Outputs:** preview panel UI, fallback message, popup viewer button.
- **Allowed changes:** panel sizing, fallback labels, status clarity.
- **Forbidden changes:** do not fake interactivity when showing PNG; do not change native viewer render flags for pure UI layout.
- **Required regression checks:** cache ready -> preview appears, embedded failed -> static fallback, popup unaffected.
- **Common failure modes:** stretched/cropped preview, stale data URL, hidden embedded window.
- **Debug starting point:** `RenderPage.tsx` / `PropertiesPage.tsx`.
- **Notes / unknowns:** preview camera defaults are native viewer owned.

### embedded 3D preview UI

- **Status:** product layer plus bridge-sensitive Windows integration
- **Purpose:** Host native viewer child window in DOM preview bounds.
- **Key files:** `embeddedViewer.ts`, `PropertiesPage.tsx`, `RenderPage.tsx`, Tauri `main.rs`
- **Inputs:** DOM rect, devicePixelRatio, currentFile, displayMode, purpose.
- **Outputs:** bounds updates, show/hide/start/stop commands.
- **Allowed changes:** rect measurement, debouncing, visibility handling, UI fallback.
- **Forbidden changes:** do not stop viewer on every transient ref loss; do not write 0x0 bounds; do not change render semantics.
- **Required regression checks:** resize, scroll, route switch, file/mode change, Windows embedded visible, popup unaffected.
- **Common failure modes:** first-second black screen, child window floats over other pages, rect DPI mismatch.
- **Debug starting point:** `[LBA_EMBED_VIEWER]`.
- **Notes / unknowns:** Windows-only by design.

### layer viewer UI

- **Status:** product layer
- **Purpose:** Show Y-slice of cache layer sidecar on canvas.
- **Key files:** `FlakePage.tsx`, `layerService.ts`, `BlockIcon.tsx`, `blockIconResolver.ts`
- **Inputs:** cacheFile, layer metadata, layerY, palette/icon data.
- **Outputs:** canvas layer view, hover block info.
- **Allowed changes:** controls, labels, canvas UX, icon fallback.
- **Forbidden changes:** do not make layer +/- buttons control zoom; do not read `.litematic` directly if cache layer path is expected.
- **Required regression checks:** build cache, load meta, slider changes layer, +/- sync, zoom/pan still mouse-driven.
- **Common failure modes:** cache missing, sidecar missing, wrong Y clamp, blank canvas.
- **Debug starting point:** `FlakePage.tsx`, then `layerService.ts`.
- **Notes / unknowns:** page name FlakePage vs LayerPage naming needs confirmation.

---

## Bridge / Service Layer

### Tauri command bridge

- **Status:** bridge
- **Purpose:** Cross boundary between React and local filesystem/process/native OS APIs.
- **Key files:** `desktop-js/src-tauri/src/main.rs`, `desktop-js/src/services/backend.ts`
- **Inputs:** command args from frontend.
- **Outputs:** command results, errors, traces, process handles.
- **Allowed changes:** argument normalization, path validation, command trace detail, clear error propagation.
- **Forbidden changes:** do not bury complex business rules here; do not drop stdout/stderr context; do not log API keys.
- **Required regression checks:** each affected command from UI and direct invoke path if possible.
- **Common failure modes:** wrong working directory, missing exe, path normalization mismatch, lost stderr.
- **Debug starting point:** Tauri command function and matching `backend.ts` wrapper.
- **Notes / unknowns:** Some command names are broad (`execute_backend`); callers must log enough args to reproduce.

### AI projection service

- **Status:** bridge/service
- **Purpose:** Build prompts, parse AI text, normalize plan states/geometry, validate and map to form.
- **Key files:** `desktop-js/src/services/aiProjection.ts`, `generateService.ts`, `blockstateDb.ts`
- **Inputs:** prompt, AI response text, current plan, BlockState DB.
- **Outputs:** normalized plan, errors, warnings, form state.
- **Allowed changes:** parser robustness, additional safe normalizers, warning precision.
- **Forbidden changes:** do not call real API by default outside explicit API chat; do not write files; do not auto apply/generate.
- **Required regression checks:** JSON parse variants, warnings vs errors, dry-run after apply.
- **Common failure modes:** malformed AI output, unsupported material shape, false hard error.
- **Debug starting point:** `extractPlanFromAiText()`, `normalizeAiPlanForImport()`.
- **Notes / unknowns:** Provider-specific prompt behavior needs confirmation.

### native backend bridge

- **Status:** bridge
- **Purpose:** Wrap calls to `litematica_core.exe` and `litematica_native_viewer.exe`.
- **Key files:** `backend.ts`, `main.rs`, `renderService.ts`, `renderCacheStore.ts`
- **Inputs:** backend binary name, args, file paths, display mode.
- **Outputs:** stdout/stderr, JSON results, process state.
- **Allowed changes:** preserve command args, improve trace formatting, normalize display mode.
- **Forbidden changes:** do not parse/modify backend semantic output beyond wrapper needs; do not swallow nonzero exit context.
- **Required regression checks:** analyze, generate dry-run, preview-output, native viewer launch.
- **Common failure modes:** exe not synced, cwd wrong, args mismatch, stdout not JSON.
- **Debug starting point:** `[LBA_JS_GENERATE_TRACE]`, `execute_backend_trace`.
- **Notes / unknowns:** Old Python `native_backend_bridge.py` is legacy reference only.

### projection preview bridge

- **Status:** bridge
- **Purpose:** Trigger preview PNG or embedded viewer from UI.
- **Key files:** `backend.ts`, `embeddedViewer.ts`, Tauri `main.rs`
- **Inputs:** file, display mode, rect, purpose.
- **Outputs:** preview PNG data URL or embedded viewer status.
- **Allowed changes:** path/rect validation, fallback status, stdout/stderr tail exposure.
- **Forbidden changes:** do not conflate static PNG and interactive embedded viewer.
- **Required regression checks:** preview PNG, embedded fail fallback, popup viewer.
- **Common failure modes:** invalid rect, zero-size host, native process exit.
- **Debug starting point:** `render_preview_image`, `start_embedded_viewer`.
- **Notes / unknowns:** exact preview camera parameters are native viewer owned.

### cache build/read bridge

- **Status:** bridge
- **Purpose:** Start/poll/kill cache build and read cache layer data.
- **Key files:** `backend.ts`, `renderCacheStore.ts`, `layerService.ts`, Tauri `main.rs`, `cache_layer.rs`
- **Inputs:** file, build mode, cache path, layer Y.
- **Outputs:** launch info, snapshots, layer meta/slice JSON.
- **Allowed changes:** better stale-cache detection, clearer error context, polling UX.
- **Forbidden changes:** do not change manifest protocol in bridge; do not treat missing cache as successful empty data.
- **Required regression checks:** cache build, cache read, layer read, missing cache.
- **Common failure modes:** stale localStorage, missing sidecar, partial cache.
- **Debug starting point:** `poll_cache_build_task`, `cache-layer-meta`.
- **Notes / unknowns:** permanent cache policy needs confirmation.

### script invocation bridge

- **Status:** bridge / needs confirmation
- **Purpose:** Manual scripts and helper scripts invoke backend/viewer/generators outside UI.
- **Key files:** `scripts/open_generated_projection.ps1`, `scripts/setup_python_env.ps1`, package/npm scripts, generator scripts.
- **Inputs:** file paths, version dir, CLI args.
- **Outputs:** opened viewer, generated DBs, fixtures, assistant index.
- **Allowed changes:** script usability, argument validation, clear output.
- **Forbidden changes:** do not make helper scripts silently modify source/generated data unless documented.
- **Required regression checks:** run the script with a known input.
- **Common failure modes:** path quoting, cwd assumptions, missing local Minecraft version.
- **Debug starting point:** script stdout/stderr.
- **Notes / unknowns:** `script/` vs `scripts/` ownership needs confirmation; prefer `scripts/` for active maintenance.

---

## Data / Generated / Cache Areas

### `data/projection-library`

- **Status:** user/project data
- **Purpose:** Store projection library records, previews, backups if used.
- **Key files:** `data/projection-library/js_library.json`, `data/projection-library/index.json`, `data/projection-library/previews/`, `data/projection-library/backups/`
- **Inputs:** imported/generated `.litematic`, analyze output.
- **Outputs:** library records and related data.
- **Allowed changes:** careful schema extension with migration/fallback.
- **Forbidden changes:** do not bulk rewrite/delete user records without explicit request.
- **Required regression checks:** existing library loads, import, refresh, current selection.
- **Common failure modes:** missing original file, schema drift.
- **Debug starting point:** `libraryStore.ts`.
- **Notes / unknowns:** `index.json` role vs `js_library.json` needs confirmation.

### generated preview images

- **Status:** cache/generated
- **Purpose:** Static viewer output for preview panels.
- **Key files:** `.tmp/desktop-js/render/lba_native_preview_*.png`
- **Inputs:** `.litematic`, display mode, native viewer preview-output.
- **Outputs:** PNG and data URL.
- **Allowed changes:** delete/regenerate.
- **Forbidden changes:** do not treat as source data; do not call it interactive.
- **Required regression checks:** regenerate preview after cache ready.
- **Common failure modes:** stale/missing PNG, wrong display mode.
- **Debug starting point:** `render_preview_image`.
- **Notes / unknowns:** none.

### 3D cache

- **Status:** cache/generated
- **Purpose:** Store prebuilt native preview cache and layer sidecar.
- **Key files:** `.tmp/desktop-js/render/lba_native_cache_*.json`, `*.chunks/`, layer sidecar.
- **Inputs:** `.litematic`, display mode, chunk size.
- **Outputs:** cache manifest/chunks/layer index.
- **Allowed changes:** delete and rebuild; add backwards-compatible metadata.
- **Forbidden changes:** do not hand-edit cache artifacts as source fixes.
- **Required regression checks:** rebuild cache, read layer meta/slice.
- **Common failure modes:** deleted temp files, corrupt JSON, missing sidecar.
- **Debug starting point:** `cache_layer.rs`, progress logs.
- **Notes / unknowns:** stable cache persistence policy needs confirmation.

### generated blockstate DB / i18n DB

- **Status:** generated
- **Purpose:** Frontend block/property candidates and Chinese display names.
- **Key files:** `data/minecraft_blockstates/26.1.json`, `26.1.zh_cn.json`, `26.1.zh_cn.missing.json`
- **Inputs:** generator scripts and overrides.
- **Outputs:** generated JSON DBs.
- **Allowed changes:** regenerate from source scripts/overrides.
- **Forbidden changes:** do not hand-edit generated DB only.
- **Required regression checks:** JSON load, UI block selector, AI warning behavior.
- **Common failure modes:** stale DB, missing legal runtime state.
- **Debug starting point:** `generate_minecraft_blockstate_db.py`.
- **Notes / unknowns:** none beyond language-source completeness.

### build output / binary output

- **Status:** binary/generated
- **Purpose:** Runtime executables and build artifacts.
- **Key files:** `bin/viewer-backend/litematica_core.exe`, `bin/viewer-backend/litematica_native_viewer.exe`, `desktop-js/dist/`, `desktop-js/src-tauri/target/`, `tools/viewer-core/target/`
- **Inputs:** Rust/JS builds.
- **Outputs:** runtime binaries and bundled UI.
- **Allowed changes:** rebuild and copy from source builds.
- **Forbidden changes:** do not hand-edit binaries or commit accidental build churn without intent.
- **Required regression checks:** `npm run build`, `cargo check`, relevant `cargo build --release`.
- **Common failure modes:** binary out of sync with source, old exe on PATH.
- **Debug starting point:** SettingsPage backend check and `bin/viewer-backend` timestamps.
- **Notes / unknowns:** release packaging process needs confirmation.

### temporary files

- **Status:** cache/generated
- **Purpose:** transient plans, preview/cache/logs, assistant index.
- **Key files:** `.tmp/desktop-js/`, `.tmp/assistant/lba_assistant_index.sqlite`
- **Inputs:** UI actions, scripts.
- **Outputs:** temp JSON/log/cache/index files.
- **Allowed changes:** safe to clean when not actively running tasks.
- **Forbidden changes:** do not rely on `.tmp` as durable user data.
- **Required regression checks:** rerun the action that regenerates it.
- **Common failure modes:** stale temp path in localStorage.
- **Debug starting point:** `.tmp` path named in UI/logs.
- **Notes / unknowns:** cleanup policy is ad hoc.

---

## Scripts / Tooling

### assistant index scripts

- **Status:** tooling
- **Purpose:** Build/query local code/document index for maintenance.
- **Key files:** `scripts/build_assistant_index.py`, `scripts/query_assistant_index.py`, `.tmp/assistant/lba_assistant_index.sqlite`
- **Inputs:** repo source/docs.
- **Outputs:** SQLite index and query results.
- **Allowed changes:** improve indexing/query robustness.
- **Forbidden changes:** do not let index script modify source files.
- **Required regression checks:** build index and query simple terms.
- **Common failure modes:** FTS syntax error on punctuation such as `-` or `/`.
- **Debug starting point:** rerun query with simpler term.
- **Notes / unknowns:** This is the preferred first tool for AI/Codex after docs.

### blockstate DB generation scripts

- **Status:** tooling / generated data pipeline
- **Purpose:** Generate BlockState DB and zh_cn mapping.
- **Key files:** `scripts/generate_minecraft_blockstate_db.py`, `scripts/generate_minecraft_blockstate_i18n.py`
- **Inputs:** Minecraft version dir/JAR, overrides.
- **Outputs:** generated DB/i18n/missing report.
- **Allowed changes:** controlled generator/override fixes.
- **Forbidden changes:** do not invent legal states without confirmation.
- **Required regression checks:** run both scripts, JSON load outputs.
- **Common failure modes:** local JAR path missing, override not applied.
- **Debug starting point:** script printed summary.
- **Notes / unknowns:** version path is machine-specific.

### projection / fixture / helper scripts

- **Status:** tooling / mixed legacy
- **Purpose:** Generate focused `.litematic` fixtures, open generated projections, set up Python env.
- **Key files:** `scripts/generate_*_fixture.py`, `scripts/open_generated_projection.ps1`, `scripts/setup_python_env.ps1`
- **Inputs:** script-specific fixture parameters or file paths.
- **Outputs:** fixture `.litematic`, opened viewer, local env.
- **Allowed changes:** add a focused fixture for a confirmed rendering bug.
- **Forbidden changes:** do not use fixture scripts to mask production parser/render bugs.
- **Required regression checks:** generated fixture loads/analyzes/viewer opens.
- **Common failure modes:** script path/cwd assumptions, stale generated fixture.
- **Debug starting point:** script stdout/stderr and generated file path.
- **Notes / unknowns:** old `script/` directory role needs confirmation; active maintenance should prefer `scripts/`.

---

## Experimental Areas

### lighting experiments / MC Light / point shadows / shadow map prototype

- **Status:** experimental / partially deprecated
- **Purpose:** Viewer-side lighting experiments and historical MC Light work.
- **Key files:** `tools/viewer-core/src/native_viewer.rs`, `tools/viewer-core/src/full_mode_v2.rs`, README lighting handoff notes.
- **Inputs:** CLI flags, env/debug flags, render assets.
- **Outputs:** altered viewer lighting/shadow/tone output and debug logs.
- **Allowed changes:** only behind explicit env/debug flags or explicit task.
- **Forbidden changes:** do not make experiments affect default cache/render pipeline; do not change mesh/full_mode/cache protocol as a side effect.
- **Required regression checks:** default viewer baseline and explicit experimental flag behavior.
- **Common failure modes:** experimental branch hides water/transparent blocks, lighting changes mistaken for geometry bug.
- **Debug starting point:** README lighting handoff, then `native_viewer.rs`.
- **Notes / unknowns:** docs say MC Light/block-light voxel propagation is removed/deprecated from normal runtime path; any remaining prototype flags need confirmation before use.

### AI projection generator

- **Status:** experimental product/service feature
- **Purpose:** Use prompt/API/web workflow to produce projection plans.
- **Key files:** `GeneratePage.tsx`, `aiProjection.ts`, `prompt_config.md`, Tauri AI commands.
- **Inputs:** prompt, current plan, AI provider config, raw AI response.
- **Outputs:** normalized plan and form state.
- **Allowed changes:** improve UX, parsing, safe warnings/normalization.
- **Forbidden changes:** do not default-call paid APIs; do not auto apply/generate; do not leak API key.
- **Required regression checks:** Mock, OpenAI-compatible failure display, parse/apply, dry-run after apply.
- **Common failure modes:** invalid JSON, unsupported operation, large/out-of-bounds plan, leaked key.
- **Debug starting point:** `aiProjection.ts`.
- **Notes / unknowns:** Gemini-compatible is reserved/not implemented.

### debug-only renderer paths

- **Status:** experimental / debug
- **Purpose:** Family-specific render diagnostics and preview fixtures.
- **Key files:** `full_mode_v2.rs`, `native_viewer.rs`, `scripts/generate_*_fixture.py`
- **Inputs:** env flags and focused fixture files.
- **Outputs:** debug logs, preview images, fixture renders.
- **Allowed changes:** add focused diagnostics for current bug.
- **Forbidden changes:** do not leave debug-only behavior enabled by default.
- **Required regression checks:** default render unchanged; debug flag produces expected logs.
- **Common failure modes:** debug condition accidentally affects production path.
- **Debug starting point:** search for the specific `LBA_*` env flag.
- **Notes / unknowns:** current list of supported debug flags needs confirmation.

---

## Legacy / Needs Confirmation Areas

### PySide6 temporary reference chain

- **Status:** legacy reference
- **Purpose:** Historical behavior reference for old UI flows.
- **Key files:** `src/litematicaba/`, legacy resources under that tree.
- **Inputs:** old Python app paths/data.
- **Outputs:** reference behavior only.
- **Allowed changes:** generally avoid; inspect for behavior comparison.
- **Forbidden changes:** do not build new mainline features here; do not depend on it for release.
- **Required regression checks:** N/A unless explicitly touched.
- **Common failure modes:** outdated behavior mistaken for current mainline.
- **Debug starting point:** only after checking desktop-js and viewer-core.
- **Notes / unknowns:** user confirmed PySide6 is temporary reference and will be deleted before real release.

### desktop-ui

- **Status:** needs confirmation
- **Purpose:** Older/alternate UI area; current role unclear.
- **Key files:** `desktop-ui/`
- **Inputs:** needs confirmation.
- **Outputs:** needs confirmation.
- **Allowed changes:** none without confirming ownership.
- **Forbidden changes:** do not migrate/delete blindly.
- **Required regression checks:** needs confirmation.
- **Common failure modes:** stale code mistaken for active UI.
- **Debug starting point:** compare with `desktop-js` before touching.
- **Notes / unknowns:** needs human confirmation.

### `script/` vs `scripts/`

- **Status:** `scripts/` active tooling; `script/` legacy/needs confirmation
- **Purpose:** `scripts/` holds current assistant index, generators, fixtures, helpers. `script/` appears older.
- **Key files:** `scripts/*.py`, `scripts/*.ps1`, `script/`
- **Inputs:** repo files, version assets, fixture parameters.
- **Outputs:** index, generated DBs, fixtures, helper launches.
- **Allowed changes:** prefer `scripts/` for active maintenance.
- **Forbidden changes:** do not delete or migrate `script/` without human confirmation.
- **Required regression checks:** run modified script with known input.
- **Common failure modes:** duplicated old logic, cwd/path assumptions.
- **Debug starting point:** `docs/FEATURE_ENTRYPOINTS.md`, then `scripts/`.
- **Notes / unknowns:** exact old `script/` consumers need confirmation.

### generated reports, root fixtures, local debug artifacts

- **Status:** mixed generated/cache/fixture; needs confirmation per file
- **Purpose:** Some root `.litematic`, `.png`, `.log`, `_full_mode_*`, `_verify_*` files are useful fixtures; others are local debug artifacts.
- **Key files:** root `*.litematic`, `*.png`, `*.log`, `_full_mode_*`, `_verify_*`
- **Inputs:** test/debug scripts and manual viewer runs.
- **Outputs:** fixture or local artifact.
- **Allowed changes:** add a focused fixture when task requires it.
- **Forbidden changes:** do not bulk delete; do not assume every root artifact is disposable.
- **Required regression checks:** if fixture changes, analyze/viewer-open it.
- **Common failure modes:** useful fixture accidentally removed.
- **Debug starting point:** README handoff notes and git history if available.
- **Notes / unknowns:** per-file ownership needs confirmation.
