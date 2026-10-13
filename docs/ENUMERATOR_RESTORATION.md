# 枚举器恢复迁移说明

> 目标：把 `dev/flake` / `dev/replace` 线上被合并丢失的枚举器功能迁回新架构
> （Tauri + public-beta + Tailwind 4/shadcn）。
>
> 恢复来源：`refs/merge-safety-pre-resolution`（= 合并前 main `c9060f96`）。

## 一、本阶段已完成（构建通过）

### 1. 枚举器服务：回移 5 个核心函数

`desktop-nova/src/services/enumeratorService.ts`：**417 行 → 620 行**（合并前 659 行）。

| 回移的函数 | 作用 |
| --- | --- |
| `evaluateEnumeratorExpression(expression, collections)` | 求值枚举器表达式：集合名/通配符/`∪ \| ∩ & -` 运算、括号、RPN 求值 |
| `buildEnumeratorValueRows(values, collections)` | 把值展开为可渲染行（含 `iconLookupMode`、类型判定、所属自定义集合） |
| `saveEnumeratorCollection(input)` | 保存自定义集合到 appdata |
| `loadEnumeratorCustomCollections()` | 载入自定义集合 |
| `createRuntimeProjectionCollection(filePath, materials)` | 由统计结果生成运行时集合 |

同时回移的私有 helper（共 17 个声明）：`targetCustomRelativePath`、`resolveCollectionToken`、
`validateWildcardToken`、`isWildcardCollectionToken`、`resolveWildcardCollections`、
`tokenizeExpression`、`validateWildcardParentheses`、`precedence`、`toRpn`、`applyOperator`、
`detectValueType`、`isItemLikeBlockCollection`。

**保留了新架构新增的内容**（未回退）：

- `EnumeratorCollectionCategory` 的 `"map"` 类别
- `buildMapColorLookup()`（按地图颜色的缩略预览依赖它）
- 新架构的数据源重接：`loadActiveLanguageMap` 读 `pack-in/lang/zh_cn.json`；
  `loadActiveBlocks` 用 `loadDatabases()` + `getAllBlocks()`（合并前版本依赖已删除的
  `gameResources.ts`，不能整文件覆盖）

配套新增 import：`writeAppDataFile`、`type MaterialItem`（来自 `statsService`，无循环依赖）。

### 2. 枚举器窗口与入口

| 文件 | 状态 |
| --- | --- |
| `desktop-nova/enumerator.html` | 从合并前恢复（12 行） |
| `desktop-nova/ui/windows/enumerator/EnumeratorWindow.tsx` | 恢复（718 行） |
| `desktop-nova/ui/windows/enumerator/index.ts` | 恢复，导出 `EnumeratorDialog` / `EnumeratorWindow` / `openEnumeratorWithWindowBehavior` |
| `desktop-nova/ui/windows/enumerator/main.tsx` | 恢复并改造：接入 `initializeWindowTheme(getUserConfig)` 与 `styles/enumerator.css` |
| `desktop-nova/ui/components/useVirtualWindow.ts` | 回移（72 行，`EnumeratorWindow` 依赖） |
| `desktop-nova/ui/components/VirtualSpacerCell.tsx` | 回移（19 行） |

### 3. 样式

新建 `desktop-nova/ui/styles/enumerator.css`（350 行）：自合并前
`ui/styles/base.css` 的 enumerator 块（原 L2624–L2956，333 行）原样恢复，
并在末尾补上该块共用、而新架构缺失的通用类 `.generate-json-preview`
（`.visually-hidden` 新架构已存在）。选择独立文件而非塞回 `base.css`，
因为新架构的 `base.css` 已缩到 1097 行并改用 Tailwind 分层。

### 4. 接线

| 接线点 | 改动 |
| --- | --- |
| `src/business/facade.ts` | 由「只导出 3 个符号」改为 `export * from "../services/enumeratorService"`，并转出 `openEnumeratorWindow` |
| `src/platform/nativeViewer.ts` | 新增 `openEnumeratorWindow(activeFile)`，invoke `open_enumerator_window` |
| `src-tauri/src/main.rs` | 新增 `open_enumerator_window` 命令（label `enumerator`、事件 `enumerator-open-file`、窗口 1180×760、最小 900×560）并注册 |
| `src-tauri/capabilities/default.json` | 窗口清单加回 `"enumerator"` |
| `vite.config.ts` | rollup input 加回 `enumerator: enumerator.html` |
| `ui/components/BlockIcon.tsx` | 接受 `lookupMode?: "default" \| "item_first"` 以兼容窗口调用点（新架构的 `iconCandidates` 已内置 block→`item_id` 映射，故无需独立分支） |

### 5. 验收

| 检查 | 结果 |
| --- | --- |
| `npx tsc --noEmit` | 通过 |
| `npm run build` | 通过，1634 模块；产出 `dist/enumerator.html`、`enumerator-*.css`、`enumerator-*.js`、`enumeratorService-*.js` |
| `cargo check`（src-tauri） | 通过 |

## 二、已完成（第二阶段）

### 2.1 `statistics_enumerator_info_rules` 配置链（前置阻塞项已解除）

| 层 | 改动 |
| --- | --- |
| Rust `src-tauri/src/main.rs` | `UserConfig` 加 `statistics_enumerator_info_rules: Vec<String>`（`#[serde(default = ...)]`）；`UserConfigInput` 加 `Option<Vec<String>>`；新增 `default_statistics_enumerator_info_rules()`（默认 `理论版本 = version(L)` / `生存不可达 = any(L & survival_impossible)`）与 `normalize_statistics_enumerator_info_rules()`；`read_user_config_data` 载入时归一化；`save_user_config` 接受该字段；`default_user_config()` 补齐字段 |
| TS `src/platform/appData.ts` | `UserConfig` 加 `statistics_enumerator_info_rules: string[]` |
| TS `src/services/userConfig.ts` | 回移 `DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES`、`normalizeStatisticsEnumeratorInfoRules()`、`saveStatisticsEnumeratorInfoRulesConfig()`；经 facade 的 `export * from "../services/userConfig"` 转出 |

### 2.2 统计页：自定义统计接口 + 最低版本判断

`ui/windows/main/pages/statistics/StatisticsPage.tsx`：**392 行 → 805 行**（与合并前同规模）。

回移内容（全部来自合并前 L26–L195、L488–L584、L589–L701、L783–L856）：

| 回移项 | 说明 |
| --- | --- |
| `EnumeratorInfoRule` / `EnumeratorInfoResult` | 规则与结果结构 |
| `parseEnumeratorInfoRules()` / `parseFunctionCall()` / `uniqValues()` | `标题 = 函数(集合表达式)` 解析 |
| `buildBaseUniverse()` / `buildVersionUniverse()` / `parseVersionToken()` / `compareVersionCollections()` / `formatVersionResult()` | **最低版本判断**：从枚举器 `version` 类别解析版本号（含 `v_older` → 1.12.2 之前），比对目标投影引用版本，输出 `x.y.z` / `x.y.z / 更高版本` / `更高版本` / `未配置版本集合` |
| `evaluateEnumeratorInfoRule()` / `buildEnumeratorInfoResults()` | 规则求值，支持 `version()` / `any()` / `ratio()` |
| `EnumeratorInfoEditorDialog` | 规则编辑器（多行编辑 + 实时预览 + 恢复默认 + 保存） |
| state / effects | `enumeratorInfoRules` 自用户配置载入；`runtimeCollection` 由统计结果生成并驱动 `loadEnumeratorCollections` |
| JSX | 「打开枚举器…」按钮、「枚举器信息」结果区（含「编辑规则」）、`<EnumeratorDialog>`、`<EnumeratorInfoEditorDialog>` |

**适配点**（与原版不同之处）：

- `StatisticsPage` 签名保持新架构的 `({ currentFile }: any)`（旧签名是 `({ currentFile, theme })`），
  旧页基于自研 CSS、新页基于 shadcn/Tailwind，故**没有整文件覆盖**，而是按段落回移。
- `<EnumeratorDialog>` 需要 `theme` prop，改用 `currentThemeId()`（来自 `ui/shell/themeRuntime`）提供。
- 新增 CSS 文件 `ui/styles/statistics-enumerator.css`（205 行 + 新增区块样式）：
  自合并前 `base.css` 的统计枚举器块（原 L2358–L2557，200 行）恢复，
  并补上本次新标记 `.beta-statistics-enumerator-info` 的布局。

### 2.3 第二阶段验收

| 检查 | 结果 |
| --- | --- |
| `tsc --noEmit` | 通过 |
| `npm run build` | 通过，1635 模块；`StatisticsPage` chunk 由 392 行版增至 21.32 kB |
| `cargo check`（src-tauri） | 通过 |
| 前端测试套件（18 个 `*.test.mjs`） | 17 通过；`themes.test.mjs` 失败 3 例，**为既有失败**（基线同一测试同样退出 1：`MODULE_NOT_FOUND` 于 `AppShell.tsx:57`，另含 `settings.tsx` 的 `<Textarea` 断言），与本次改动无关 |

## 三、剩余（下一阶段）

| 优先级 | 工作 | 来源 |
| --- | --- | --- |
| 1 | 替换组规则系统：v2 units、预设管理、会话持久化、`EnumerablePickerDialog`、`ui/styles/replace.css` | `refs/merge-safety-pre-resolution`：`src/business/replace/**`、`src/services/replaceSessionStore.ts`、`tools/viewer-core/src/replace_blocks.rs` |
| 2 | 游戏资源管理（`gameResources.ts` 40 个导出）+ Asset Manager 窗口 | `refs/merge-safety-pre-resolution`：`src/services/gameResources.ts`、`ui/windows/asset_manager/**` |
| 3 | 投影内嵌预览图导入/清除（`clearProjectionPreviewImage` / `importProjectionPreviewImage` / `selectPreviewImageFile`） | `refs/merge-safety-pre-resolution`：`src/business/actions.ts` |
| 4 | 渲染进度与缓存构建机制（`renderCacheStore` 9 导出、`coreReadCache` 3 导出、`RenderProgress`） | 建议按 `render-ir`/`viewer_session` 事件流重建，而非回退旧实现 |

## 四、回退方式

本阶段所有改动均在 `main` 的普通提交中，未改写历史。
恢复前的基线为 `refs/merge-safety-pre-resolution`（`c9060f96`）。
