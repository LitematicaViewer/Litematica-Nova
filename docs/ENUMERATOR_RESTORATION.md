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

## 二、下一阶段（未完成）

统计页的枚举器对接仍未恢复，原因是它依赖一条**在新架构中完全缺失的配置链**。

### 2.1 阻塞项：`statistics_enumerator_info_rules` 配置字段

该字段在两侧都不存在，需先恢复整条链：

| 层 | 合并前实现位置 | 需恢复内容 |
| --- | --- | --- |
| Rust 配置结构 | `src-tauri/src/main.rs`：L189-190、L201、L494、L510-524、L575-576、L1735-1736 | `statistics_enumerator_info_rules: Vec<String>` 字段、`Option<Vec<String>>` 输入、`default_statistics_enumerator_info_rules()`、`normalize_statistics_enumerator_info_rules()` |
| TS 类型 | `src/platform/appData.ts:10` | `statistics_enumerator_info_rules: string[]` |
| 服务 | `src/services/userConfig.ts` | `DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES`（默认两条规则）、`normalizeStatisticsEnumeratorInfoRules()`、`saveStatisticsEnumeratorInfoRulesConfig()` |

### 2.2 统计页待回移内容

`ui/windows/main/pages/statistics/StatisticsPage.tsx`：HEAD 392 行 / 合并前 805 行。
需回移（合并前 L26–L195、L488–L584、L589–L701、L783–L856）：

| 回移项 | 说明 |
| --- | --- |
| `interface EnumeratorInfoRule` / `EnumeratorInfoResult` | 规则与结果结构 |
| `parseEnumeratorInfoRules()` / `parseFunctionCall()` / `uniqValues()` | 规则解析 |
| `buildBaseUniverse()` / `buildVersionUniverse()` / `parseVersionToken()` / `compareVersionCollections()` / `formatVersionResult()` | **最低版本判断**（依赖枚举器 `version` 类别） |
| `evaluateEnumeratorInfoRule()` / `buildEnumeratorInfoResults()` | 规则求值，支持 `version()` / `any()` / `ratio()` 三个函数 |
| `EnumeratorInfoEditorDialog` | 规则编辑对话框（97 行） |
| state/effects | `enumeratorCollections`、`enumeratorInfoRules`、`showEnumeratorInfoEditor` 等，含加载与保存 |
| JSX | 「打开枚举器…」按钮、信息结果列表、「编辑规则」按钮、`<EnumeratorDialog>` 渲染 |

注意：合并前统计页的 `StatisticsPage({ currentFile, theme })` 签名与新架构的
`StatisticsPage({ currentFile })` 不同，且旧页基于自研 CSS、新页基于 shadcn/Tailwind，
因此**不能整文件覆盖**，只能按上述段落逐段回移并适配样式。

### 2.3 后续可选

- `gameResources`（40 个导出）与 Asset Manager：枚举器的 `loadBaseCollections`
  原本可读游戏资源注册表，新架构已改为读 `pack-in/enumerator`，故非阻塞。
- 替换组规则系统（v2 units / 预设 / 会话持久化）。
- 投影内嵌预览图导入/清除。

## 三、回退方式

本阶段所有改动均在 `main` 的普通提交中，未改写历史。
恢复前的基线为 `refs/merge-safety-pre-resolution`（`c9060f96`）。
