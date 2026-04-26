# Litematica-BA Project Map / 项目总地图

本文件给维护者使用，不是产品介绍。目标是让新维护者在几分钟内判断：

- 当前主线在哪里；
- UI、Python、Rust、data、scripts 分别负责什么；
- 哪些文件是源码，哪些是生成物、缓存或二进制；
- 预览、AI plan、viewer-core、blockstate DB 出问题时先查哪里。

This document is for maintainers, not for marketing. It maps the current repository so a maintainer can quickly find the active UI, Rust backend/viewer, data files, generated artifacts, and first debugging entry points.

Legacy cleanup status: `desktop-ui/`, `script/`, and `src/litematicaba/` have been deleted. `desktop-js/` is the only active desktop UI. Historical references to those paths are not current entry points.

Maintainer entry / 维护入口：`MAINTAINERS.md`.

不确定的归属或状态统一标注为：**需要人工确认 / needs human confirmation**。

诊断包导出和阅读说明见：`docs/DIAGNOSTICS.md`。

---

## 1. Current Mainline / 当前主线

### 当前判断 / Current assessment

- **desktop-js 是当前活跃桌面 UI 主线。**  
  React 页面在 `desktop-js/src/routes/`，前端服务封装在 `desktop-js/src/services/`，Tauri 命令在 `desktop-js/src-tauri/src/main.rs`。

- **tools/viewer-core 是当前稳定后端和渲染核心。**  
  `.litematic` 解析、analyze、generate、cache、native viewer、Full Mode V2 主要在 `tools/viewer-core/src/`。构建产物会复制到 `bin/viewer-backend/`。

- **PySide6 代码暂时保留作历史链路和行为参考。**  
  Python 包在 `src/litematicaba/`。根据当前维护确认，它不是真正上线主线，正式上线前会删除；需要旧行为对照时才查它。

- **desktop-ui 存在，但当前角色不清楚。**  
  与 `desktop-js` 的关系和是否仍活跃：**需要人工确认 / needs human confirmation**。

### Practical rule / 实用判断规则

| 要查的问题 / Question | 先看哪里 / Start here |
|---|---|
| 当前桌面 UI 行为 / Current desktop UI behavior | `desktop-js/` |
| `.litematic` 解析、生成、分析、缓存、原生 viewer / Parse, generate, analyze, cache, native viewer | `tools/viewer-core/` |
| 投影库兼容问题 / Projection library compatibility | `desktop-js/src/services/libraryStore.ts` + `src/litematicaba/core/projection_library.py` |
| 旧行为参考 / Old behavior reference | `src/litematicaba/`，但不要默认它是当前主线 / use as reference, not as assumed mainline |

---

## 2. Top-Level Directory Map / 顶层目录地图

### Active source / 活跃源码

| path | role / 作用 | kind / 类型 | safe to edit? / 是否可改 |
|---|---|---|---|
| `desktop-js/` | React + Tauri 桌面应用 / current JS desktop app | source | Yes, with build checks |
| `tools/viewer-core/` | Rust 后端、cache、native viewer、generate/analyze / Rust core backend and viewer | source | Yes, with Rust checks |
| `scripts/` | 维护脚本、assistant index、DB/fixture 生成 / maintenance and generation scripts | source/tooling | Yes |
| `docs/` | 项目维护文档 / project docs | docs | Yes |

### Data and assets / 数据和资源

| path | role / 作用 | kind / 类型 | safe to edit? / 是否可改 |
|---|---|---|---|
| `data/` | 投影库、缓存、模板、AI prompt、BlockState DB / runtime data, templates, generated DBs | mixed | Depends on subdirectory |
| `block/` | 方块图标 PNG 库 / block icon PNG library | data asset | Yes, carefully |
| `item/` | 物品资源 / item assets | data asset | Yes, carefully |
| `lang/` | 语言/资源映射 / language resources | data asset | Yes, carefully |
| `pack-in/` | 资源包输入 / resource-pack inputs | data asset | Yes, carefully |
| `logo/` | app/logo 图标资源 / logo and app icons | data asset | Yes, carefully |
| `third_party/` | 第三方渲染资源 / external render assets | vendor asset | Rarely |

### Legacy, unclear, or auxiliary / 历史、未确认或辅助目录

| path | role / 作用 | kind / 类型 | safe to edit? / 是否可改 |
|---|---|---|---|
| `src/` | Python `litematicaba` 包，包含 PySide6 UI 和旧工具 / Python package with PySide6 UI and utilities | legacy reference | Reference only; production removal planned |
| `desktop-ui/` | 旧或备选 UI 区域 / older or alternate UI area | legacy/experimental | Needs confirmation |
| `script/` | 旧脚本目录 / older script directory | legacy | Needs confirmation |

### Generated, cache, binary, local artifacts / 生成物、缓存、二进制、本地产物

| path | role / 作用 | kind / 类型 | safe to edit? / 是否可改 |
|---|---|---|---|
| `.tmp/` | 临时输出、assistant index、预览 / temporary outputs and local index | cache/generated | Can clean/debug |
| `bin/` | 运行时 exe，来自 Rust release build / runtime executables copied from Rust builds | binary | Do not hand-edit |
| `.git/` | Git 元数据 / Git metadata | repository internals | No |
| `.obsidian/` | 本地笔记配置 / local notes workspace | docs/cache | Usually no |
| root `*.log`, `*.png`, `_verify_*`, `_full_mode_*` | 本地验证和调试产物 / local verification artifacts | mixed | Usually no |

### Root config and docs / 根目录配置和文档

| path | role / 作用 |
|---|---|
| `README.md`, `README_EN.md` | 项目说明和交接信息 / project notes and handoff context |
| `开发文档.md`, `投影格式文档.md` | 旧中文开发/格式文档 / older Chinese development and format docs |
| `pyproject.toml`, `requirements.txt` | Python 包和依赖 / Python packaging and dependencies |
| `install.bat`, `launch_ba_ui.bat`, `kill-git-loop.ps1` | 本地安装、启动、维护脚本 / local helper scripts |

---

## 3. Core Subsystems / 核心子系统

### Projection Library / 投影库

- **Purpose / 目的：** 管理导入投影、投影库索引、备份、预览和当前记录。  
  Tracks imported projections, library index records, backups, previews, and current selection.
- **Entry files / 入口文件：** `desktop-js/src/services/libraryStore.ts`, `desktop-js/src/routes/LibraryPage.tsx`, `src/litematicaba/core/projection_library.py`
- **Input / 输入：** `.litematic`, library JSON
- **Output / 输出：** `data/projection-library/*.json`, backups, previews
- **Depends on / 依赖：** Tauri 文件命令、旧 Python 投影库逻辑
- **Debug first / 调试入口：** 先查 `data/projection-library/` 和 `libraryStore.ts`；旧 UI 兼容再查 Python path。

### Property / Metadata Editing / 属性和元数据

- **Purpose / 目的：** 读取和展示投影 metadata、材料、统计、属性页动作。  
  Reads and displays metadata, material/stat views, and property actions.
- **Entry files / 入口文件：** `desktop-js/src/routes/PropertiesPage.tsx`, `desktop-js/src/services/statsService.ts`, Python `properties_page.py`
- **Input / 输入：** 当前 `.litematic`
- **Output / 输出：** metadata、材料、统计视图
- **Depends on / 依赖：** `litematica_core analyze`, library state
- **Debug first / 调试入口：** 先跑 `bin/viewer-backend/litematica_core.exe analyze <file>` 对比 CLI 和 UI。

### Preview Generation / 预览生成

- **Purpose / 目的：** 生成静态 PNG 预览、弹窗 viewer、嵌入式 viewer。  
  Produces static PNG previews, popup viewer, or embedded native viewer.
- **Entry files / 入口文件：** `desktop-js/src/services/renderService.ts`, `desktop-js/src/services/backend.ts`, `PropertiesPage.tsx`, `RenderPage.tsx`, `tools/viewer-core/src/native_viewer.rs`
- **Input / 输入：** `.litematic`, display mode, preview path
- **Output / 输出：** PNG preview 或 viewer process/window
- **Depends on / 依赖：** Tauri commands, native viewer
- **Debug first / 调试入口：** 用 native viewer `--preview-output` 单独复现；嵌入问题查 `[LBA_EMBED_VIEWER]` 日志。

### 3D Cache Build / Read / 3D 缓存构建和读取

- **Purpose / 目的：** 为渲染页和分层页预构建 cache、manifest、layer index。  
  Prebuilds cache, manifests, and layer index for render/layer consumers.
- **Entry files / 入口文件：** `RenderPage.tsx`, `renderCacheStore.ts`, `backend.ts`, `tools/viewer-core/src/cache_layer.rs`, `build_mode.rs`
- **Input / 输入：** `.litematic`, display mode
- **Output / 输出：** cache file, ready/progress files, layer index
- **Depends on / 依赖：** Tauri cache commands, Rust cache output
- **Debug first / 调试入口：** 查 progress/ready/cache 文件，再查 `poll_cache_build_task`。

### Native Viewer / 原生 Viewer

- **Purpose / 目的：** 弹窗或 Windows HWND 嵌入式 3D viewer。  
  Native 3D viewer for popup mode or embedded Windows child window mode.
- **Entry files / 入口文件：** `tools/viewer-core/src/native_viewer.rs`, `desktop-js/src-tauri/src/main.rs`, `desktop-js/src/services/embeddedViewer.ts`
- **Input / 输入：** `.litematic`, display mode, viewer flags, HWND
- **Output / 输出：** native window, embedded child host, PNG preview
- **Depends on / 依赖：** wgpu/winit, Tauri process/window management
- **Debug first / 调试入口：** 先验证 popup viewer；嵌入问题看 `[LBA_EMBED_VIEWER]` 的 bounds、pid、visible、rect 日志。

### Layer Viewer / 分层查看

- **Purpose / 目的：** 显示指定 Y 层切片。  
  Shows Y-layer slices.
- **Entry files / 入口文件：** `desktop-js/src/routes/FlakePage.tsx`, `desktop-js/src/services/layerService.ts`, `tools/viewer-core/src/cache_layer.rs`
- **Input / 输入：** 当前文件或 cache manifest
- **Output / 输出：** canvas layer view
- **Depends on / 依赖：** cache state, block icon library
- **Debug first / 调试入口：** 确认 cache 对应当前文件和 display mode；再查 layer index/canvas draw。

### AI Projection Generator / AI 投影生成

- **Purpose / 目的：** 构建 prompt、API/网页端对话、解析/修复/校验 plan，并填入 GeneratePage 表单。  
  Builds prompts, handles chat/web workflows, parses/normalizes/validates plans, and fills GeneratePage.
- **Entry files / 入口文件：** `GeneratePage.tsx`, `aiProjection.ts`, `generateService.ts`, Tauri `main.rs`, `data/ai-projection/prompt_config.md`
- **Input / 输入：** user prompt, current plan/context, AI config
- **Output / 输出：** form中的 `projection_plan.json`；不会自动生成 `.litematic`
- **Depends on / 依赖：** Tauri AI config/key, BlockState DB, schema docs
- **Debug first / 调试入口：** 按 parse -> normalize -> validate -> preview -> dry-run 顺序查。

### Minecraft BlockState DB / 方块状态数据库

- **Purpose / 目的：** 提供方块、property、value 候选和 warning 判断。  
  Provides block/property/value candidates and validation warnings.
- **Entry files / 入口文件：** `scripts/generate_minecraft_blockstate_db.py`, `scripts/generate_minecraft_blockstate_i18n.py`, `data/minecraft_blockstates/overrides/26.1.json`, `desktop-js/src/services/blockstateDb.ts`
- **Input / 输入：** Minecraft version assets/JAR, override JSON
- **Output / 输出：** `26.1.json`, `26.1.zh_cn.json`, missing report
- **Depends on / 依赖：** local Minecraft assets, override files
- **Debug first / 调试入口：** 合法状态误报 warning 时，改 override 后重新生成 DB/i18n；不要只手改生成物。

### Lighting / Shader Experiments / 光照和 Shader 实验

- **Purpose / 目的：** viewer 侧光照、历史 MC Light、点阴影、shadow map 实验。  
  Viewer-side lighting and historical/experimental lighting paths.
- **Entry files / 入口文件：** `native_viewer.rs`, `full_mode_v2.rs`, README handoff notes
- **Input / 输入：** viewer flags, env vars
- **Output / 输出：** shaded viewer output
- **Depends on / 依赖：** wgpu shader, Full Mode materials
- **Debug first / 调试入口：** 高风险；README 显示 MC Light/block-light voxel propagation 已从正常运行路径移除或废弃。

### Scripts / Assistant Index / 脚本和 Assistant Index

- **Purpose / 目的：** 本地搜索索引、fixture、数据生成。  
  Local search index plus fixture/data generation.
- **Entry files / 入口文件：** `scripts/build_assistant_index.py`, `scripts/query_assistant_index.py`, `scripts/generate_*`
- **Input / 输入：** repo files, Minecraft version assets
- **Output / 输出：** SQLite assistant index, generated DBs, fixtures
- **Depends on / 依赖：** Python stdlib/local files
- **Debug first / 调试入口：** 先 build index；FTS 遇到 `-`、`/`、标点报错时，改用更简单的查询词。

---

## 4. Data and Generated Files / 数据和生成物

### User/runtime data / 用户或运行时数据

- `data/projection-library/index.json`
- `data/projection-library/js_library.json`
- `data/projection-library/backups/`
- `data/projection-library/previews/`
- `data/settings.json`

### Cache/generated data / 缓存或生成物

- `.tmp/`
- `.tmp/assistant/lba_assistant_index.sqlite`
- `data/cache/material_*.json`
- `desktop-js/dist/`
- `desktop-js/node_modules/`
- `desktop-js/src-tauri/target/`
- `tools/viewer-core/target/`
- `tools/viewer-core/.tmp/`
- root `_verify_*.png`, `_*.log`, preview/debug images

### Generated BlockState DB / 生成的方块状态数据库

这些文件是生成物，不应该只手工修改它们作为最终修复：

- `data/minecraft_blockstates/26.1.json`
- `data/minecraft_blockstates/26.1.zh_cn.json`
- `data/minecraft_blockstates/26.1.zh_cn.missing.json`

Regenerate these files instead of patching only the generated outputs.

### Regeneration inputs / 重新生成入口

- `data/minecraft_blockstates/overrides/26.1.json`
- `scripts/generate_minecraft_blockstate_db.py`
- `scripts/generate_minecraft_blockstate_i18n.py`

### Source/config inputs / 源配置输入

- `data/ai-projection/prompt_config.md`
- `data/ai-projection/prompt_config.json`
- `data/generation-templates/*.json`
- `data/generation-templates/index.json`
- `third_party/render-assets/`
- `block/`, `item/`, `lang/`, `pack-in/`, `logo/`

### Binary outputs / 二进制输出

- `bin/viewer-backend/litematica_core.exe`
- `bin/viewer-backend/litematica_native_viewer.exe`

这些 exe 应从 `tools/viewer-core/target/release/` 复制，不应手工编辑。  
These executables should be copied from Rust release builds, not edited directly.

---

## 5. Stable Core / Risk Zones / 稳定核心和风险区

### High-risk files / 高风险文件

| area / 区域 | files / 文件 | why risky / 风险原因 |
|---|---|---|
| Viewer mesh/rendering | `tools/viewer-core/src/mesh.rs`, `full_mode.rs`, `full_mode_v2.rs`, `native_viewer.rs` | 容易影响所有 3D 渲染、材质、窗口行为 |
| Cache protocol | `tools/viewer-core/src/cache_layer.rs`, `desktop-js/src/services/renderCacheStore.ts` | UI 和 Rust 需要同一份协议理解 |
| NBT / litematic parse | `tools/viewer-core/src/nbt.rs`, `analyze.rs` | 解析错误会影响导入、统计、生成和 viewer |
| Tauri bridge | `desktop-js/src-tauri/src/main.rs` | 前端能力、文件系统、进程管理、AI key 都从这里进出 |
| Projection library schema | `libraryStore.ts`, `src/litematicaba/core/projection_library.py` | JS/Python 兼容和旧数据迁移风险 |
| BlockState DB generation | `scripts/generate_minecraft_blockstate_db.py`, `overrides/26.1.json` | 影响方块选择、AI 校验、状态 warning |
| AI schema/normalizer/validator | `aiProjection.ts`, `generateService.ts`, `docs/projection_plan_schema.md`, `prompt_config.md` | 影响 AI plan 导入、修复、dry-run |

### Regression expectations / 回归验证要求

- 改 viewer-core mesh/full/native viewer：跑 `cargo check`、相关 `cargo test`、focused viewer preview 命令。
- 改 cache protocol：验证 cache build、progress polling、layer metadata read、UI consumer。
- 改投影库 schema：检查已有投影库记录，并评估 JS/Python 兼容。
- 改 BlockState DB：重新生成 DB/i18n，确认合法状态不再 warning，未知状态仍 warning。
- 改 AI schema/normalizer/validator：验证纯 JSON、代码块 JSON、非法 JSON、非法状态、仅 warning、应用后 dry-run。

---

## 6. Experimental Areas / 实验区域

这些功能存在，但不应在没有当前验证时写成稳定能力。

These areas exist, but should not be presented as fully stable without current validation.

- **Lighting / MC Light / point shadows / shadow map prototypes**  
  README 交接信息显示当前稳定基线是 viewer-side lighting；MC Light/block-light voxel propagation 已从正常路径移除或废弃。

- **Full Mode V2 block-family fidelity work**  
  活跃且重要，但仍是按方块族逐步收口的工作。

- **AI projection generator**  
  能构建 prompt、对话、解析/修复/校验 plan、填入 GeneratePage。仍是实验线，不应自动生成 `.litematic`。

- **desktop-js embedded native viewer**  
  Windows HWND 嵌入已存在；bounds/lifecycle 很敏感。

- **desktop-ui**  
  当前角色不清楚：**需要人工确认 / needs human confirmation**。

---

## 7. Where To Start When Debugging / 调试入口

| symptom / 现象 | start here / 先看 | next / 再看 |
|---|---|---|
| UI 按钮没反应 / UI button does nothing | `desktop-js/src/routes/<Page>.tsx` | `desktop-js/src/services/*.ts`, `desktop-js/src-tauri/src/main.rs` |
| `.litematic` 导入/打开/分析失败 / import/open/analyze fails | `tools/viewer-core/src/nbt.rs`, `analyze.rs`, `cli.rs` | `libraryStore.ts`, Python `projection_library.py` |
| 预览图错位、缺失 / preview wrong or missing | `renderService.ts`, Tauri `render_preview_image` | `native_viewer.rs`, `full_mode_v2.rs`, `--preview-output` |
| 3D viewer 黑屏/崩溃 / black screen or crash | `native_viewer.rs` | `full_mode_v2.rs`, `mesh.rs`, `[LBA_EMBED_VIEWER]` |
| AI plan 报错 / AI generated plan errors | `aiProjection.ts` | `generateService.ts`, `prompt_config.md`, `projection_plan_schema.md` |
| 方块状态误报非法 / valid blockstate flagged illegal | `blockstateDb.ts` | `overrides/26.1.json`, generator scripts |
| cache 读写异常 / cache build/read abnormal | `RenderPage.tsx`, `renderCacheStore.ts` | Tauri cache commands, `cache_layer.rs` |
| 分层页错误 / layer viewer wrong | `FlakePage.tsx`, `layerService.ts` | cache status, `cache_layer.rs` |
| 材料/统计错误 / material or statistics wrong | `analyze.rs`, `stats_api.rs` | `statsService.ts`, `PropertiesPage.tsx`, `StatisticsPage.tsx` |
| GeneratePage dry-run/apply 失败 / dry-run/apply fails | `generateService.ts` | `generate_projection.rs`, schema docs, temp plan |

---

## 8. Assistant Workflow / AI 维护流程

默认维护流程 / Default maintenance workflow:

1. 先读 `docs/PROJECT_MAP.md`。  
   Read this project map first.

2. 运行 assistant index。  
   Run:

   ```powershell
   python scripts/build_assistant_index.py
   ```

3. 用简单关键词查询索引。遇到 `-`、`/`、标点导致 FTS 报错时，简化查询词。  
   Query with simple terms; simplify terms if FTS rejects punctuation.

4. 再读对应功能文档。  
   Then inspect relevant docs:

   - `docs/projection_plan_schema.md`
   - `开发文档.md`
   - `README.md`
   - `投影格式文档.md`

5. 最后查真实源码。  
   Only then inspect and edit actual source files.

6. 不允许全仓库盲改。  
   Do not make blind full-repository edits.

7. 不确定的归属和状态必须标注：**需要人工确认 / needs human confirmation**。  
   Mark uncertain ownership/status explicitly.

8. 修改必须保持范围收敛，并运行匹配的回归验证。  
   Keep changes scoped and run matching regression checks.
