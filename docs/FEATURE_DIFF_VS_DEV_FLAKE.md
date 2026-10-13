# 功能差异表：当前 HEAD ↔ dev/flake

> 对比对象：`dev/flake`（`63270f47`，我方在合并前的开发线末端）与当前 `HEAD`。
>
> 拓扑事实：`dev/flake` 从合并前 main 的末端 `c9060f96` 分出，**另有 11 个提交：**
>
> ```
> 63270f47 略微优化注释和外观        ac8f58d1 feat：保存、另存为功能
> fb3860d9 fix：优化流畅度           5da9e796 feat：测量工具
> 3d725c9b feat：编辑功能框架与调试棒工具
> 6e22f75d feat：漏斗容器内容读取    ca80b217 feat：空负载保留UI可用
> 0f05d03f feat：加入对拉杆的支持    1b67b106 fix：纠正拼写错误并加入对雕纹书架的支持
> c1e8018c feat：地图基色            b25b5d61 Merge pull request #15 from LitematicaViewer/main
> ```
>
> 改动 60 个文件 / +2692 行。**这 11 个提交从未进入 `main`**——它们是平行的后续开发，而不是被"合并时丢弃"。

## 一、结论

**功能没有被整体忽略。** dev/flake 的 11 项功能里，**功能实现基本都在 HEAD 中保留**（FlakePage 含编辑工具、测量、调试状态、投影标记等，2214 行 vs dev/flake 2380 行）。真正的差异集中在两类：

1. **渲染进度/缓存构建机制整体丢失**（实质性功能回归，非渲染画面本身）；
2. **支撑层服务被新架构另一套实现替换**（`layerService` → `flakeService` 等）。

## 二、逐功能差异表

| # | dev/flake 的功能 | HEAD 状态 | 证据 |
| --- | --- | --- | --- |
| 1 | 地图基色（`buildMapColorLookup`、枚举器 `map` 类别） | **保留** | `business/facade.ts`、`services/enumeratorService.ts`、`FlakePage.tsx` |
| 2 | 雕纹书架支持（`CHISELED_BOOKSHELF_EMPTY_BLOCK_ID`、`SLOT_0..5_OCCUPIED_TRUE_RELPATH`） | **保留** | `services/flake/stateHintRules/stateDefault.ts` |
| 3 | 拉杆支持（`LEVER_GORUND_RELPATH` + 6 个 lever 资源） | **保留** | `stateDefault.ts`、`pack-in/flake/redstone_display/lever_*` |
| 4 | 去皮竹块（`STRIPPED_BAMBOO_BLOCK_TOP_BLOCK_ID`） | **保留** | `stateDefault.ts` |
| 5 | 朝向叠加图（`FACING_{NORTH,SOUTH,WEST,EAST}_OVERLAY_RELPATH` 及 `_2`） | **保留** | `stateDefault.ts`、`pack-in/flake/state_overlay/` |
| 6 | 空负载保留 UI 可用 | **保留** | `FlakePage.tsx` 空态分支 |
| 7 | 漏斗容器内容读取 | **保留** | `tools/viewer-core/src/block_entity.rs` 的 `read_layer_container`、`FlakePage` 容器对话框 |
| 8 | 编辑功能框架与调试棒工具（`EDIT_TOOL_PLACEHOLDERS`、`selectedEditTool`、`editedBlockStates`、`FlakeDebugStateDialog`、`handleApplyDebugState`） | **保留** | `FlakePage.tsx` |
| 9 | 测量工具（`FlakeMeasurementRect`、`handleAddMeasurement`、`formatMeasurementDistance`、`normalizeMeasurementRect`） | **保留** | `FlakePage.tsx` |
| 10 | 保存 / 另存为（`markerSaveAsDefaultPath`、`ensureLitematicExtension`） | **保留** | `FlakePage.tsx`、`services/generateService.ts` |
| 11 | 投影标记与原点编辑（Rust `projection_marker.rs`、`read_projection_markers`、`set_projection_origin`、`remove_projection_origin`；TS `loadProjectionMarkers` 等） | **保留** | `tools/viewer-core/src/projection_marker.rs`（HEAD 存在）、`services/flakeService.ts` |
| 12 | 音符盒音阶（`parseNoteScaleOptions`、`parseCsvLine`、`NoteToneLabel`） | **保留** | `FlakePage.tsx` |
| 13 | 图层瓦片渲染（`buildLayerTile`、`LayerTileRenderMode`、`getMaxLayerTileCacheSize`） | **保留** | `FlakePage.tsx` |

## 三、真正的丢失项

### 3.1 渲染进度与缓存构建机制（功能性回归）

dev/flake 的这套机制在 HEAD 中**完全不存在**：

| 丢失符号 | 来源 |
| --- | --- |
| `startCacheBuildTask` / `pollCacheBuildTask` / `killCacheBuildTask` | `services/renderCacheStore.ts` + `services/backend.ts` |
| `CacheBuildLaunch` / `CacheBuildSnapshot` | 同上 |
| `RenderProgress` / `readProgressFile` | `services/renderService.ts` |
| `hydrateRenderCacheStore` / `subscribeRenderCacheStore` / `upsertRenderCacheState` / `updateRenderCacheState` / `getRenderCacheState` / `getLatestRenderCacheState` / `stateFromLaunch` | `services/renderCacheStore.ts`（9 个导出全部缺失） |
| `getCachedCoreOutput` / `getAnalyzeOutput` / `invalidateCoreReadCache` | `services/coreReadCache.ts` |

HEAD 的替代实现是**空壳**：

```ts
// desktop-nova/src/services/renderService.ts
export function ensureApplicationRenderSubscription(): Promise<void> { return Promise.resolve(); }
export function subscribeApplicationRender(_listener: () => void): () => void { return () => {}; }
export function isApplicationRenderStateReady(): boolean { return true; }
export function findRenderOperationForSource(...): RenderOperationView | null { return null; }
```

`RenderPage.tsx` 规模：**dev/flake 474 行 → HEAD 40 行**。

> 判定：渲染的"画面管线"按你的指示迁到了新架构（`render-ir`），但 dev/flake 里的**渲染缓存构建进度跟踪与状态订阅**没有对应承接者，属于真实能力损失。

### 3.2 被替换的支撑层服务

| dev/flake 服务 | HEAD 替代 | 等价性 |
| --- | --- | --- |
| `services/layerService.ts`（`LayerPaletteEntry`/`LayerSliceMeta`/`LayerSliceData`/`checkCacheExists`/`loadLayerMeta`/`loadLayerSlice`/`loadAllLayerSlices`/`getBlockColor`） | `services/flakeService.ts`（同名类型 + `readLayerMeta`/`readLayer` 再导出） | **类型与标记 API 等价**；`loadLayerSlice`/`loadAllLayerSlices` 未见对应导出 |
| `services/renderCacheStore.ts` | `platform/applicationRender.ts`（仅类型 + 空壳函数） | **不等价**（见 3.1） |
| `services/coreReadCache.ts` | 无 | **丢失** |
| `services/gameResources.ts` | `pack-in/` + `src-tauri/application/operations.rs` | 数据源迁移 |
| `platform/embeddedViewer.ts` / `services/embeddedViewer.ts` | `ui/windows/main/useEmbeddedViewer.ts` + `viewer-launch-contract` | 架构替换 |
| `platform/backendProcess.ts` | `platform/applicationOperations.ts` | 架构替换 |
| `src/business/layers/index.ts`、`renderCache/index.ts`、`replace/{actions,types}.ts` | `business/{flake?}/index.ts`、`business/replace/index.ts` | 目录收敛 |

### 3.3 未随合并保留的资源与页面

| 类别 | 明细 |
| --- | --- |
| 旧架构独有页面 | `asset_manager.html` + `ui/windows/asset_manager/**`（4 文件）、`enumerator.html` + `ui/windows/enumerator/**`（3 文件）、`performance-test.html`、`ui/windows/main/pages/properties/function.tsx`、`ui/windows/main/pages/replace/EnumeratorBlockPickerDialog.tsx` |
| 旧 UI 基础组件 | `ui/components/{BlockPickerDialog,VirtualSpacerCell}.tsx`、`ui/components/useVirtualWindow.ts`、`ui/shell/NavIcon.tsx`、`ui/styles/replace.css` |
| 我方新增的光栅图标 | `desktop-nova/ui/shell/resource/button/editTool_*.png`、`.../content/{campfire,chest54,chiseled_bookshelf,dispenser}.png`（主题目录内的同名文件已保留；`ui/shell/resource/**` 这套因 `ui/shell` 被新架构接管而未保留，且当前无代码引用） |
| 资源 | `pack-in/legacy/`、`data/legacy/`、`logo/*`、`tm_icon.ico`、`icon.ico`、`tools/patches/vscode-nbt-lba-editor.patch` |
| 渲染生成脚本（与新渲染架构自洽删除） | `scripts/generate_{chest,hopper,piston,glass,water,rail,...}_fixture.py` 等 19 个、`viewer-core/scripts/check_render_build_baseline.py`、`viewer-core/baselines/render_build_baseline.json` |
| 旧渲染实现（按你的指示删除） | `viewer-core/src/full_mode.rs`、`full_mode_v2.rs`、`BUILD_MODES.md` |

> 已核对：**主题目录内的图片变量全部可解析**（`--flake-*image` 指向的文件均存在），当前代码没有悬空图片引用。

## 四、HEAD 新增（dev/flake 没有）

| 类别 | 明细 |
| --- | --- |
| 前端技术栈 | Tailwind 4 + shadcn/radix（`ui/components/ui/*` 10 个组件）、`ui/styles/{controls,flake,widgets,updater}.css`、主题 `8bitcn`/`shadcn`/`modern` |
| 平台层 | `platform/{applicationOperations,applicationRender,nativeWindowGeometry,quickPreview,updater}.ts` |
| Tauri 宿主 | `src-tauri/src/{application/*,platform/*,viewer_backend.rs,viewer_launch_contract.rs}`、`installer/{nsis-hooks.nsh,legacy-name-cleanup.wxs}` |
| 打包与 CI | `.github/workflows/beta-public.yml`、`scripts/prepare-backend-deployment.ps1`、`check-public-build.mjs` |
| 测试 | 23 个 `*.test.mjs`（beta-*、themes、widgets、materials、updater、native-child 等）+ `layer-violations-baseline.json` |
| 更新器 | `plugins.updater` 双源签名、`UpdateDialog.tsx`、`updateRecovery.ts` |
| Rust workspace | `crates/{render-ir,nova-render-protocol,viewer-launch-contract}`、`canonical_ir.rs`、`viewer_session.rs`、`build.rs` |
| 快速预览 | `application/quick_preview.rs`、`platform/quickPreview.ts`、`useQuickPreview.ts` |

## 五、恢复建议（如需找回 3.1）

渲染进度机制建议按新架构重建，而不是回退旧文件：

1. 后端：`crates/render-ir` / `viewer_session.rs` 已提供会话与帧事件，可在其上实现"构建进度"事件流，替代原来的 `readProgressFile` 轮询。
2. 前端：在 `platform/applicationRender.ts` 的空壳函数位置填充真实实现（当前 `isApplicationRenderStateReady()` 恒为 `true`、`findRenderOperationForSource` 恒为 `null`）。
3. 参考实现：`refs/heads/dev/flake:desktop-nova/src/services/renderCacheStore.ts`（9 个导出）与 `coreReadCache.ts`（3 个导出）。

旧文件均可从 `refs/heads/dev/flake`（`63270f47`）直接取用。
