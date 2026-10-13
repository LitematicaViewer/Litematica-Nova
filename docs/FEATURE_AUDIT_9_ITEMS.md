# 功能核查报告：用户列出的 9 项功能在 HEAD 中的存续情况

> 核查对象：用户列出的 9 项功能。
> 参照线：`dev/flake`（`63270f47`）与 `dev/replace`（`71429bde`）。
>
> **拓扑前提**：`dev/replace` 已通过 PR #14 进入 main（`c9060f96`），`dev/flake` 又在其后走了 11 个提交。两条线的代码都在合并前 main 中，**不属于那次合并的冲突范围**。因此问题不是"合并冲突时被覆盖"，而是**合并前 main 里的实现没能整体带入合并结果**。

## 一、核查结论总表

| # | 功能 | 判定 | 关键证据 |
| --- | --- | --- | --- |
| 1 | **枚举器** | ❌ **大部分丢失** | 服务 602 行 → 380 行；5 个核心函数消失；独立窗口与入口页消失 |
| 2 | **自定义统计接口**（依赖枚举器） | ❌ **丢失** | 统计页 805 行 → 392 行，枚举器对接整体消失 |
| 3 | **最低版本判断**（依赖枚举器） | ❌ **丢失** | `parseVersionToken` / `buildVersionUniverse` / `hasHigherVersion` 全无 |
| 4 | **替换组规则系统** | ❌ **大部分丢失** | units 体系、预设、会话、加权随机全无；仅保留旧 rules 格式 |
| 5 | **投影内嵌预览图浏览** | ⚠️ **部分丢失** | 渲染/提取保留，投影库侧的导入/清除/选择全无 |
| 6 | **游戏资源管理**（Asset Manager） | ❌ **整体丢失** | 40 个导出消失；`asset_manager.html` 与窗口移除 |
| 7 | **编辑工具** | ✅ **保留** | `EDIT_TOOL_PLACEHOLDERS` 等，在 `FlakePage.tsx` |
| 8 | **方块状态提示** | ✅ **保留** | `stateDefault.ts` / `stateWaterlogged.ts` / `blockRedstone.ts` / `flakeStateHintResolver.ts` |
| 9 | **创造模式物品栏** | ✅ **保留** | `creativeInventoryDialog.tsx` / `creativeInventory` / `CreativeInventory` |
| 附 | **按地图颜色的缩略预览**（依赖枚举器） | ✅ **保留** | `buildMapColorLookup` + `LayerTileRenderMode` 的 `"map-color"` 分支 |

## 二、丢失项明细

### 1. 枚举器（最严重）

`desktop-nova/src/services/enumeratorService.ts`：**602 行 → 380 行**，丢失 5 个导出（其中 `evaluateEnumeratorExpression` 与 `buildEnumeratorValueRows` 是枚举器表达式求值的核心）：

| 丢失导出 | 作用 |
| --- | --- |
| `evaluateEnumeratorExpression(expression, collections)` | 求值枚举器表达式，返回 `values` / `dependencies` / `error` |
| `buildEnumeratorValueRows(values, collections)` | 把值展开为可渲染行 |
| `saveEnumeratorCollection(input)` | 保存自定义集合 |
| `loadEnumeratorCustomCollections()` | 载入自定义集合 |
| `createRuntimeProjectionCollection(filePath, materials)` | 由投影材质生成运行时集合 |

连带丢失的 UI 与入口：

- `desktop-nova/enumerator.html`（入口页）
- `desktop-nova/ui/windows/enumerator/**`（`EnumeratorWindow.tsx` / `index.ts` / `main.tsx`）
  导出 `EnumeratorWindow`、`EnumeratorDialog`、`openEnumeratorWithWindowBehavior`
- `desktop-nova/src/platform/nativeViewer.ts` 的 `openEnumeratorWindow`
- 侧栏入口与 `capabilities/default.json` 中的相关窗口

> `pack-in/enumerator/**` 的**数据**（base / version / creative / custom / system_enum，37 个 JSON）保留完好，丢的是**服务与界面**。

### 2. 自定义统计接口

`ui/windows/main/pages/statistics/StatisticsPage.tsx`：**805 行 → 392 行**。丢失的导入与结构：

```
DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES
normalizeStatisticsEnumeratorInfoRules
saveStatisticsEnumeratorInfoRulesConfig
import { EnumeratorDialog, openEnumeratorWithWindowBehavior } from "../../../enumerator";
interface EnumeratorInfoRule
interface EnumeratorInfoResult
function parseEnumeratorInfoRules(rules: string[]): EnumeratorInfoRule[]
```

配套的 `services/userConfig.ts` 也丢失 `normalizeStatisticsEnumeratorInfoRules` 与 `saveStatisticsEnumeratorInfoRulesConfig`。

### 3. 最低版本判断

原实现位于 `StatisticsPage.tsx`（属上述 805 行内），是一整套基于枚举器 `version` 类别的比对逻辑：

| 丢失函数 | 作用 |
| --- | --- |
| `buildVersionUniverse(collections)` | 汇总全部版本集合的取值全集 |
| `parseVersionToken(collection)` | 解析版本号标记（`kind: "older" \| "normal"`、`numbers`、`label`） |
| `compareVersionCollections(left, right)` | 版本排序 |
| `formatVersionResult(values, collections)` | 输出判断结论 |
| `hasHigherVersion` 判定 | 目标投影引用了比当前更高的版本 → 报"版本过低" |

`pack-in/enumerator/version/**`（20 个版本 JSON）数据仍在，判断逻辑没有承接者。

### 4. 替换组规则系统

| 类别 | 丢失内容 |
| --- | --- |
| Rust | `replace_blocks_v2`、`ReplaceUnitsFile`、`OutputDistEntry`、`weighted_random_pick`、`validate_units_file`、`properties_subset_match`（`replace_blocks.rs` 1174 行 → 604 行，仅保留旧 `ReplaceRulesFile`/`ReplaceRule`/`RuleSummary` 格式） |
| TS 类型 | `src/business/replace/types.ts` 全部 10 个导出：`ReplaceUnit`、`ReplaceItem`、`ReplaceEntry`、`ReplaceOutputEntry`、`OutputDistEntry`、`ReplacePreset`、`ReplacePreviewSummary`、`UnitPreviewSummary`、`SerialSeparator`、`isSeparatorItem` |
| TS 动作 | `src/business/replace/actions.ts` 全部 7 个导出：`applyReplaceUnits`、`dryRunReplaceUnits`、`listReplacePresets`、`loadReplacePreset`、`saveReplacePreset`、`deleteReplacePreset`、`openReplacePresetFolder` |
| 会话持久化 | `src/services/replaceSessionStore.ts` 全部 3 个导出：`saveReplaceSession`、`loadReplaceSession`、`clearReplaceSession` |
| 选择器 | `ui/windows/main/pages/replace/EnumeratorBlockPickerDialog.tsx` |
| 样式 | `ui/styles/replace.css` |

> 注：这是我在上一轮按你"渲染无条件迁移"指示执行的结果——当时为匹配新架构前端 `ReplacePage.tsx` 的 `ReplaceRulesFile`/`per_rule` 契约而取了公开线版本，并把加权随机列为"暂搁置到下一阶段"。但 **v2 units 体系、预设管理、会话持久化这三项并未被单独登记为搁置项**，属遗漏，特此更正。

### 5. 投影内嵌预览图浏览

| 保留 | 丢失 |
| --- | --- |
| `platform/previewImage.ts`：`renderPreviewImage`、`generatePreviewImage`、接口 `RenderPreviewOutput` | `clearProjectionPreviewImage(currentFile)`、`importProjectionPreviewImage(currentFile, imagePath)`、`selectPreviewImageFile()`（均在原 `business/actions.ts`） |
| `viewer-core/src/projection_preview.rs`：`extract_projection_preview` | 投影库侧的预览图导入/清除交互 |
| `services/backend.ts`、`platform/index.ts` 的 `previewImage` 通道 | `MetadataNbt` 的 `preview_image_data` / `preview_image_path` 补丁写入 |

即：**能渲染预览图，但投影库里无法导入/更换/清除预览图**，内嵌预览浏览的使用链路不完整。

### 6. 游戏资源管理（Asset Manager）

`src/services/gameResources.ts` 的 **40 个导出全部消失**，包括：

- 资源库注册与激活：`registerGameResource`、`activateGameResource`、`GameResourceEntry`、`GameResourceKind`、`GameResourceSnapshot`、`GameResourceSource`、`GameResourceIndex`、`listGameResourceRegistry`、`checkGameResourceHealth`、`GameResourceHealthRow`
- 图标下载与提取：`downloadWikiBlockIcons`、`downloadWikiEnumCatalog`、`downloadVaultBlockIcons`、`downloadVaultItemIcons`、`ensureBuiltinBlockIconsExtracted`、`ensureBuiltinItemIconsExtracted`、`WikiBlockIconSource`、`VaultBlockIconInstallResult`、`WikiEnumCatalogInstallResult`
- 图标解析：`resolveBlockIconVariant`、`BlockIconVariant`、`BlockIconSlot`、`supportsBlockIconLayering`、`getActiveIconSearchRoots`、`IconSearchRoot`、`readIconFromRoot`、`registerExternalIconDirectory`
- 语言资源：`readActiveLanguageResource`、`readFallbackLanguageResource`、`readActiveGameDataResource`、`downloadRemoteMinecraftLanguage`、`listRemoteMinecraftLanguages`、`listRemoteMinecraftLanguageBranches`、`RemoteLanguageSource`
- 导入：`importGameDataResource`、`importLanguageResource`、`deleteGameResource`、`getActiveResourceFingerprint`

配套丢失：`platform/files.ts` 与 `business/actions.ts` 的 **16 个下载/提取通道**，以及入口与窗口：

- `desktop-nova/asset_manager.html`
- `desktop-nova/ui/windows/asset_manager/**`（`AssetManagerContent.tsx` / `AssetManagerWindow.tsx` / `index.ts` / `main.tsx`）
- `capabilities/default.json` 中的 `asset-manager` 窗口
- Tauri 侧 `wiki_block_icons.rs`
- 缓存失效接口：`invalidateBlockIconCache`、`invalidateI18nCache`、`invalidateBlockstateDbCache`

> 这是资产（icon/lang）管理能力的整块消失，与"Mojang 资产不入库"无关——入库的是**下载与管理这些资产的代码**。

## 三、保留项明细（无需处理）

| 功能 | 位置 |
| --- | --- |
| 编辑工具 | `ui/windows/main/pages/flake/FlakePage.tsx`：`EDIT_TOOL_PLACEHOLDERS`、`EditToolId`、`selectedEditTool`、`editedBlockStates`、`FlakeDebugStateDialog`、`debugStateBlock`、`handleApplyDebugState`；主题 `--flake-edit-tool-*-image` 变量与 `resource/button/editTool_*.png` 均存在 |
| 方块状态提示 | `src/services/flake/stateHintRules/{stateDefault,stateWaterlogged,blockRedstone}.ts`、`src/services/flakeStateHintResolver.ts`；含雕纹书架、拉杆、去皮竹块、朝向叠加图 |
| 创造模式物品栏 | `ui/windows/main/pages/flake/creativeInventoryDialog.tsx`、`FlakePage` 内 `creativeInventory` 引用；主题变量 `--flake-creative-inventory-image` |
| 按地图颜色的缩略预览 | `buildMapColorLookup`（`facade.ts` / `enumeratorService.ts` / `FlakePage.tsx`）+ `LayerTileRenderMode = "map-color" \| "texture"` + `buildLayerTile` |
| 编辑测量工具 | `FlakeMeasurementRect` / `handleAddMeasurement` / `formatMeasurementDistance` |

## 四、总量指标

| 指标 | 值 |
| --- | --- |
| 合并前 main 的前端导出总数 | 526 |
| HEAD 的前端导出总数 | 505 |
| **HEAD 已无的我方导出** | **166** |
| 其中 `gameResources.ts` | 40 |
| 其中 `platform/backendProcess.ts` | 17（含缓存任务三件套） |
| 其中 `business/replace/types.ts` | 10 |
| 其中 `services/renderCacheStore.ts` | 9 |
| 其中 `platform/embeddedViewer.ts` | 9 |
| 其中 `platform/files.ts` | 9 |
| 其中 `business/replace/actions.ts` | 7 |
| 其中 `business/actions.ts` | 7 |
| 其中 `services/enumeratorService.ts` | 5 |
| 其它 | 其余 43 项分散于 20 个文件 |

## 五、恢复建议

按"承接难度 × 用户价值"排序：

| 优先级 | 工作 | 恢复来源 |
| --- | --- | --- |
| 1 | 枚举器服务 5 函数 + 枚举器窗口（自定义统计与最低版本判断都依赖它） | `refs/heads/dev/flake:desktop-nova/src/services/enumeratorService.ts`、`ui/windows/enumerator/**`、`enumerator.html` |
| 2 | 统计页枚举器对接 + 最低版本判断（805 行版） | `refs/merge-safety-pre-resolution:desktop-nova/ui/windows/main/pages/statistics/StatisticsPage.tsx` |
| 3 | 替换组规则系统（types / actions / 会话 / Rust v2 units） | `refs/merge-safety-pre-resolution:desktop-nova/src/business/replace/**`、`src/services/replaceSessionStore.ts`、`tools/viewer-core/src/replace_blocks.rs` |
| 4 | 游戏资源管理（40 导出 + Asset Manager 窗口） | `refs/merge-safety-pre-resolution:desktop-nova/src/services/gameResources.ts`、`ui/windows/asset_manager/**` |
| 5 | 投影内嵌预览图导入/清除 | `refs/merge-safety-pre-resolution:desktop-nova/src/business/actions.ts` |

> 恢复时需按新架构改造：窗口入口改走 `capabilities/default.json` + `vite.config.ts` 的 rollup input；服务接入 `business/facade.ts` 的新导出面；Tauri 命令注册改到 `src-tauri/src/application/operations.rs`。
