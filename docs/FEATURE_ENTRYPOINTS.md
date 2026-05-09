# 功能入口地图

本文按功能列出用户入口、主要实现文件、输入输出和调试入口。

## 应用启动

- 用户入口：`desktop-nova` Tauri 应用。
- 主要文件：
  - `desktop-nova/src/main.tsx`
  - `desktop-nova/src/App.tsx`
  - `desktop-nova/src-tauri/src/main.rs`
  - `desktop-nova/src/styles/base.css`
- 调试入口：先看 `App.tsx` 的 route/currentFile/theme 状态。

## 投影库

- 用户入口：投影库页面、本地导入、RedenMC 在线投影库。
- 主要文件：
  - `desktop-nova/src/routes/LibraryPage.tsx`
  - `desktop-nova/src/services/libraryStore.ts`
  - `desktop-nova/src/services/redenLibrary.ts`
  - `desktop-nova/src/services/backend.ts`
  - `tools/viewer-core/src/analyze.rs`
- 输入：本地 `.litematic`、RedenMC API 结果。
- 输出：AppData 投影库记录、当前文件、预览状态。
- 调试入口：先跑 `bin/viewer-backend/litematica_core.exe analyze <file>`。

## 属性页

- 用户入口：属性页。
- 主要文件：
  - `desktop-nova/src/routes/PropertiesPage.tsx`
  - `tools/viewer-core/src/metadata_edit.rs`
  - `tools/viewer-core/src/analyze.rs`
- 输入：当前 `.litematic`。
- 输出：metadata 表单、保存或另存结果。
- 调试入口：对比 UI 表单和 `analyze` JSON。

## 统计和材料

- 用户入口：统计页、材料列表弹窗、导出材料列表。
- 主要文件：
  - `desktop-nova/src/routes/StatisticsPage.tsx`
  - `desktop-nova/src/services/statsService.ts`
  - `tools/viewer-core/src/stats_api.rs`
- 输入：当前 `.litematic`、范围、是否统计容器内物品、倍数。
- 输出：材料列表、容器扫描摘要、CSV。
- 调试入口：运行 `litematica_core.exe materials --input <file> --scope all --include-container-items --json`。

## 渲染页和 3D cache

- 用户入口：渲染页、构建 3D cache、嵌入 viewer、弹窗 viewer。
- 主要文件：
  - `desktop-nova/src/routes/RenderPage.tsx`
  - `desktop-nova/src/services/renderCacheStore.ts`
  - `desktop-nova/src/services/embeddedViewer.ts`
  - `desktop-nova/src-tauri/src/main.rs`
  - `tools/viewer-core/src/native_viewer.rs`
- 输入：当前 `.litematic`、display mode、cache 文件。
- 输出：progress JSON、cache manifest、预览图、native viewer 进程。
- 调试入口：查看 `[LBA_EMBED_VIEWER]`、progress JSON、native stdout/stderr。

## 分层页

- 用户入口：分层页。
- 主要文件：
  - `desktop-nova/src/routes/FlakePage.tsx`
  - `desktop-nova/src/services/layerService.ts`
  - `tools/viewer-core/src/cache_layer.rs`
- 输入：cache manifest、Y 层。
- 输出：层级切片数据和画布。
- 调试入口：运行 `litematica_core.exe cache-layer-meta <cache>` 和 `cache-layer <cache> --y=<Y>`。

## 替换页

- 用户入口：替换页 dry-run/apply。
- 主要文件：
  - `desktop-nova/src/routes/ReplacePage.tsx`
  - `tools/viewer-core/src/replace_blocks.rs`
- 输入：From/To 方块和状态。
- 输出：dry-run summary 或输出 `.litematic`。
- 调试入口：查看临时 rules JSON 和 backend stdout。

## 生成页

- 用户入口：生成页模板、AI plan、dry-run/apply。
- 主要文件：
  - `desktop-nova/src/routes/GeneratePage.tsx`
  - `desktop-nova/src/services/generateService.ts`
  - `desktop-nova/src/services/aiProjection.ts`
  - `tools/viewer-core/src/generate_projection.rs`
  - `docs/projection_plan_schema.md`
- 输入：plan JSON、模板、AI 输出。
- 输出：dry-run summary、生成的 `.litematic`。
- 调试入口：先验证 plan，再手动运行 `litematica_core.exe generate --plan <plan> --dry-run`。

## 选项页

- 用户入口：选项页。
- 主要文件：
  - `desktop-nova/src/routes/SettingsPage.tsx`
  - `desktop-nova/src/services/userConfig.ts`
  - `desktop-nova/src-tauri/src/main.rs`
- 输入：主题、AppData 配置目录、AI 配置。
- 输出：用户配置、检查结果。
- 调试入口：查看 AppData 下 `Litematica-BA\desktop-nova`。

