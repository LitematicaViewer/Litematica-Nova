# 游戏资源管理器：flake 版 vs 现状

> 对比对象：`dev/flake`（`63270f47`，与合并前 main 的 `gameResources.ts` **逐字节一致**）与当前 `HEAD`。
>
> 结论先行：**现状不是"简化版"，而是该功能整块被移除**——公开 Beta 线没有这个管理器，
> 它把资源读取改成了硬编码路径 + 构建期静态 import。两侧不是同一套东西的不同版本。

## 一、总体架构差异

| | flake / 合并前 main | 当前 HEAD |
| --- | --- | --- |
| 资源管理 | **运行时注册表**（5 类资源 × 7 种来源 × 6 个激活槽位） | 无注册表 |
| 资源路径 | 动态解析，可切换、可多版本共存 | **硬编码常量** |
| 元数据索引 | 每类资源一份 `installed.json` | 无 |
| 多版本 | 支持，可登记多个版本并按槽位分别激活 | 不支持（单一内建版本） |
| 下载/抓取 | CCVault + Minecraft Wiki（中英）+ GitHub + 本地导入 | 无（仅随包内建） |
| 相关文件 | `gameResources.ts` 921 行、`AssetManagerContent.tsx` 959 行、`AssetManagerWindow.tsx` 62 行、`wiki_block_icons.rs` 682 行、`asset_manager.html` | **全部不存在** |

现状的实际读取方式（全部硬编码）：

```ts
// services/blockIconResolver.ts
import catalogData from "../../../pack-in/materials/minecraft-26.3.json";
import iconManifest from "../../../pack-in/materials/item-icons-26.3.json";
const paths = ["pack-in/item-icons-26.3"];          // 唯一图标根，无变体、无回退链

// services/blockstateDb.ts
readWorkspaceFile("data/minecraft_blockstates/26.1.json")        // 版本写死在文件名
readWorkspaceFile("data/minecraft_blockstates/26.1.zh_cn.json")

// services/i18n.ts
readWorkspaceFile("pack-in/lang/zh_cn.json")                     // 唯一语言包
```

## 二、可管理的配置（flake 版）

### 2.1 六个激活槽位

「多版本」的本质是**同一类资源可以登记多份，并按槽位分别激活**。这是现状完全没有的能力。

| 槽位 | 字段 | 用途 |
| --- | --- | --- |
| 语言 | `active` | 界面/方块/物品名翻译 |
| 材料图标 | `active_material_list` | 材料列表等界面的方块图标 |
| 分层方块图标 | `active_layering` | 分层画布（Flake）的方块图标 |
| 分层物品图标 | `active_layering`（`item_icon` 类） | 分层画布的物品图标 |
| 游戏数据 | `active` | BlockState 数据库 + 属性翻译 |
| 枚举全集 | `active` | 枚举器基础集合 |

关键设计：`block_icon` 与 `item_icon` **用 `active_layering` 共享一个槽位**，即分层画布的
方块与物品图标是统一选定的；而 `block_icon` 另有独立的 `active_material_list` 槽，
两类界面的图标包可以不同。

### 2.2 五类资源 / 七种来源 / 各自可用来源

| 资源类（`GameResourceKind`） | 落盘目录 | 可用来源 |
| --- | --- | --- |
| `language`（语言） | `minecraft-assets/language` | 内建、本地 JSON、GitHub/InventivetalentDev |
| `block_icon`（方块图标） | `minecraft-assets/block_2d` | 内建、本地目录、CCVault、Minecraft Wiki、中文 Minecraft 维基 |
| `item_icon`（物品图标） | `minecraft-assets/item` | 内建、本地目录、CCVault、Minecraft Wiki |
| `game_data`（游戏数据） | `minecraft-assets/game-data` | 内建、本地 JSON |
| `enum_catalog`（枚举全集） | `enumerator/base` | 内建、Minecraft Wiki |

来源语义（`GameResourceSource`）：`builtin` 内建 / `imported` 导入 / `external` 外部目录登记 /
`github` / `vault`（CCVault）/ `wiki`（英文维基）/ `wiki_zh`（中文维基）。

### 2.3 方块图标的 2D/3D 变体与槽位约束

```ts
export type BlockIconVariant = "2d" | "3d";
export type BlockIconSlot = "material_list" | "layering";

resolveBlockIconVariant(entry)      // 从 block_icon_variant / source / root_relpath / active_layering 推断
supportsBlockIconLayering(entry)    // 是否可用于分层槽
```

- **3D 方块图标不能用于分层槽**，激活时会抛错：
  「3D 方块图标不能用于分层槽。请改用 2D 方块图标资源。」
- 界面据此做槽位锁定（`lockedBlockIconSlotForSource`）与来源提示（`blockIconSourceHint`）——
  不同来源只产出特定变体，选择来源时会锁定对应槽位。

### 2.4 资源条目的字段（`GameResourceEntry`）

| 字段 | 含义 |
| --- | --- |
| `id` / `kind` / `label` | 标识、类别、显示名 |
| `source` | 来源 |
| `installed_at` | 安装时间（界面「日期」列） |
| `language` / `branch` / `version` | 语言代码、GitHub 分支、版本号 |
| `file_relpath` | 单文件资源的相对路径（语言包） |
| `root_relpath` | 目录型资源的根（图标包、枚举全集） |
| `manifest_relpath` / `root_path` | 清单路径 / 外部绝对根 |
| `data_relpath` / `i18n_relpath` | 游戏数据的 DB 与翻译两个文件 |
| `block_icon_variant` | `2d` / `3d` |
| `active` | 语言/游戏数据/枚举全集的激活标记 |
| `active_material_list` / `active_layering` | 图标的两槽激活标记 |
| `builtin` | 是否内建（内建资源作为离线回退） |

### 2.5 注册表持久化

每类资源一份索引，写在 **app_data**（用户配置目录）下：

```
minecraft-assets/language/installed.json     → { schema_version: 1, kind: "language",  entries: [...] }
minecraft-assets/block_icon/installed.json
minecraft-assets/item/installed.json
minecraft-assets/game-data/installed.json
enumerator/base/installed.json
```

四类索引落在 `minecraft-assets/` 下，都属**本机用户数据、不入库**
（`data/minecraft-assets/` 已在 `.gitignore`）。唯一例外是枚举全集的
`data/enumerator/base/installed.json`：它在 `pack-in/enumerator` 的公开资源范围内，
已随包入库。

### 2.6 内建资源（离线回退基线，不可删除）

| 内建项 | id | 指向 |
| --- | --- | --- |
| 内建 zh_cn | `builtin:language:zh_cn` | `minecraft-assets/language/initial/zh_cn.json`（随包为 `pack-in/lang/zh_cn.json`） |
| 内建 Nova 方块图标 | `builtin:block_icon:nova` | `minecraft-assets/block_2d/initial`，变体 `2d`，两槽均激活 |
| 内建 Nova 物品图标 | `builtin:item_icon:nova` | `minecraft-assets/item/initial` |
| 内建 BlockState 26.1 | `builtin:game_data:26.1` | `data/minecraft_blockstates/26.1.json` + `26.1.zh_cn.json` |
| 内建枚举全集 | `builtin:enum_catalog:base` | `enumerator/base/builtin` |

激活内建资源时会把该类的其它条目全部置为非激活（`selectingBuiltin` 分支），
但**内建条目本身始终保留**，作为离线回退。

#### ⚠️ 与"不分发 Mojang 资产"的冲突（恢复时必须处理）

flake / 合并前 main **把这批内建图标提交入库了**：

| 分支 | `data/minecraft-assets` 入库文件数 |
| --- | --- |
| `dev/flake` | **942**（941 个 `block_2d/initial/*.png` + 1 个 `language/initial/zh_cn.json`） |
| 合并前 `main` | 942 |
| 公开线 | 0 |
| 当前 `HEAD` | **0** |

这些是方块贴图，属 Mojang 资产。**恢复资源管理器时不要把它们重新入库**——
按此前约定，Mojang 资产只留在本地并走 `.gitignore`；可分发的是
`pack-in/game-icons/block.zip` 与 `pack-in/item-icons-26.3`（见
`docs/THIRD_PARTY_NOTICES.txt` 的「公开资源范围」）。
内建图标应改为**首次运行时从 `block.zip` 按需解包**
（flake 版已有 `ensure_builtin_block_icons_extracted`，正好用于此）。

## 三、下载与抓取能力

### 3.1 后端命令（Tauri）

| 命令 | 来源 | 产出 |
| --- | --- | --- |
| `download_vault_block_icons` | `https://ccvaults.com/` | 方块图标（2D/3D） |
| `download_vault_item_icons` | CCVault | 物品图标 |
| `ensure_builtin_block_icons_extracted` | 内建 `pack-in/game-icons/block.zip` | 解包到图标根 |
| `ensure_builtin_item_icons_extracted` | 内建 `pack-in/game-icons/item.zip` | 解包到物品图标根 |
| `download_minecraft_wiki_enum_catalogs` | `minecraft.wiki` 的 Java Edition data values 四个页面 | 枚举全集 |

`wiki_block_icons.rs`（682 行）的抓取源：

- 英文：`https://minecraft.wiki/api.php?...page=Template:BlockSprite/doc`、
  `Template:BlockLink`
- 中文：`https://zh.minecraft.wiki/api.php?...`、
  `Java版数据值/方块ID`
- 并发：`WIKI_ICON_DOWNLOAD_CONCURRENCY = 6`

### 3.2 语言资源

| 能力 | 实现 |
| --- | --- |
| 列分支 | 抓取 `github.com/InventivetalentDev/minecraft-assets/branches` 页面解析 `/tree/<branch>` |
| 列语言 | 抓取分支目录页面解析 `([a-z0-9_]+)\.json` |
| 下载 | `raw.githubusercontent.com/.../<branch>/<language>.json`，写回 app_data |
| 过滤 | 只保留 `block.` / `effect.` / `enchantment.` / `entity.` / `item.` 前缀的键 |
| 分支排序 | `languageBranchSortKey` 做版本化排序 |

### 3.3 本地导入

| 能力 | 命令/函数 |
| --- | --- |
| 导入语言 JSON | `importLanguageResource(sourcePath, language?)` |
| 导入游戏数据 | `importGameDataResource(dataPath, i18nPath?, version?)` |
| 登记外部图标目录 | `registerExternalIconDirectory(...)`（登记为 `external` 来源，记绝对根 `root_path`） |

## 四、运行期消费与健康检查

flake 版把"资源"与"消费方"解耦，消费方都走注册表：

```ts
readActiveLanguageResource()                                   // ← i18n
readActiveGameDataResource() → { dbText, i18nText, entry }      // ← BlockState 数据库
getActiveIconSearchRoots(slot) → IconSearchRoot[]              // ← 图标解析，按槽位给多个搜索根
readIconFromRoot(root, relativePath)                           // 逐根尝试，形成回退链
getActiveResourceFingerprint()                                 // 缓存失效指纹
checkGameResourceHealth() → GameResourceHealthRow[]            // 界面「健康检查」
```

配套的缓存失效接口：`invalidateBlockIconCache`、`invalidateI18nCache`、
`invalidateBlockstateDbCache`——切换资源后立即生效。

**图标回退链**是现状完全没有的：`getActiveIconSearchRoots` 按槽位返回多个搜索根
（内建 → 外部登记 → 已下载），`readIconFromRoot` 依次尝试，任何一层缺失都能继续回退。

## 五、差异总表

| 能力 | flake / 合并前 main | 现状 |
| --- | --- | --- |
| 资源注册表 | ✅ 5 类 × 每类多条目 | ❌ |
| 多版本共存 | ✅ 同类型多份，独立激活 | ❌ 单版本 |
| 六个激活槽位 | ✅ | ❌ |
| 2D/3D 图标变体 | ✅ 且槽位约束 | ❌ 仅 2D 等效 |
| 图标回退链 | ✅ 多搜索根依次回退 | ❌ 单一路径 |
| CCVault 下载（方块/物品图标） | ✅ | ❌ |
| Minecraft Wiki 抓取（图标，中英） | ✅ 682 行实现 | ❌ |
| Minecraft Wiki 抓取（枚举全集） | ✅ 4 个数据值页面 | ❌ |
| GitHub 语言包（分支/语言/下载） | ✅ | ❌ |
| 本地导入（语言/游戏数据/外部图标目录） | ✅ | ❌ |
| 内建图标解包 | ✅ `block.zip` / `item.zip` 按需解包 | ❌ 直接读 `pack-in/item-icons-26.3` |
| 资源健康检查 | ✅ | ❌ |
| 缓存失效接口 | ✅ 3 个 | ❌ |
| 独立管理窗口 | ✅ `asset_manager.html` + 窗口 + 侧栏入口 | ❌ 入口与窗口都已移除 |
| 资源落盘 | `minecraft-assets/**`（942 个文件曾入库，现应为本地 + gitignore） | 仅读 `pack-in/**` 与 `data/minecraft_blockstates/**` |

## 六、恢复的判断与建议

**这不是"把界面加回来"就能解决的**。flake 版的资源读取是注册表驱动，
而现状的消费方（`blockIconResolver`、`blockstateDb`、`i18n`、`enumeratorService`）
全部是硬编码路径——恢复管理器必然要同步改造这四个消费方，否则界面切了资源也不生效。

建议分三层推进，每层可独立验收：

| 层 | 内容 | 说明 |
| --- | --- | --- |
| L1 数据层 | 回移 `gameResources.ts`（921 行）+ 其依赖的 `platform/files.ts` 下载通道 | 纯数据能力，不动界面；可先用脚本验证注册表读写正确 |
| L2 消费方改造 | 让 `blockIconResolver` / `blockstateDb` / `i18n` / `enumeratorService` 改走注册表 | **关键一步**；改完切换资源才会真正生效 |
| L3 界面层 | 回移 `AssetManagerContent.tsx`（959 行）+ 窗口 + 入口 + 侧栏 + `capabilities`/vite | 依赖 L1、L2 |

**风险与取舍**：

1. `AssetManagerContent.tsx` 依赖 `NavIcon` 组件与一批 `asset-manager-*` 样式类，
   这些在新架构中同样缺失，需一并回移或改用 shadcn 组件重写。
2. 界面基于旧自研 CSS 主题，新架构是 shadcn/Tailwind——**建议 L3 用新组件重写界面**，
   只保留 L1/L2 的逻辑层（与枚举器恢复时的策略一致：逻辑回移、界面适配）。
3. 下载能力依赖外网（CCVault、minecraft.wiki、GitHub），且 `wiki_block_icons.rs`
   含页面解析逻辑，长期看有失效风险；恢复后建议保留"内建资源始终作为离线回退"这一设计。
4. **Mojang 资产不要重新入库**：flake 版把 942 个 `data/minecraft-assets` 文件提交了，
   恢复时内建图标必须改为从 `pack-in/game-icons/block.zip` 按需解包
   （`ensure_builtin_block_icons_extracted` 已具备该能力）。详见 2.6 节。
5. 消费方改造会影响**现有图标/语言行为**，需要回归验证：材料列表图标、分层画布图标、
   BlockState 数据库、i18n 文案、枚举器集合——这五条链路都依赖被改造的服务。

**建议顺序**：先做 L1 + L2（能力可用但无界面，可用现有设置页或命令行验证切换效果），
确认消费方改造无回归后再决定 L3 是重写界面还是回移。

## 七、合并丢弃功能的整体恢复状态

本次合并（`main` ← `111a0c50`）把我方 26 个既有代码文件整块丢弃。已按"回归修复"逐项恢复，
而非作为新特性开发。

| # | 能力 | 状态 | 提交 |
| --- | --- | --- | --- |
| 1 | 游戏资源注册表（L1，921 行） | 已恢复 | `f78c44e6` |
| 2 | 四个消费方改走注册表（L2） | 已恢复 | `963be12a` |
| 3 | CCVault 下载 + 运行时自动获取 Key | 已恢复（无硬编码凭据） | `d6cdb441` |
| 4 | 规则文件驱动的图标候选（`itemlike_block.json`、`_slab`/`potted_*`） | 已恢复 | `ec8ddab9` |
| 5 | Wiki 方块图标抓取（中/英） | 已恢复 | `4edf90d5` |
| 6 | 资源管理器界面与入口（L3） | 已恢复 | `f3b48bac` |
| 7 | 替换组规则 TS 层 + `execute_backend` | 已恢复 | `3c2cb355` |
| 8 | 渲染进度/缓存构建链路 | 已恢复 | `e3b19a7d` |
| 9 | Wiki 枚举全集抓取 | 已恢复 | `eb7fc83c` |
| 10 | 投影内嵌预览图导入/清除 | 已恢复 | `eb7fc83c` |
| 11 | 替换组规则 v2 units（含加权随机） | 已恢复 | `cf9ade1d` |
| 12 | units 样式表接线 | 已恢复 | （见 git log） |

### 有意未恢复的项（均无功能影响）

| 文件 | 原因 |
| --- | --- |
| `ui/windows/main/pages/properties/function.tsx` | 合并前后**都无人引用**，且 `PropertiesPage` 已不再导出其依赖的 `MetadataForm` / `RegionEdit`；属死代码 |
| `ui/shell/NavIcon.tsx` | 现役 `AppShell.tsx` 已内置同名 `NavIcon`（lucide 版）；恢复会重复导出 |
| `src/platform/embeddedViewer.ts`、`src/services/embeddedViewer.ts` | 现役为 `ui/windows/main/useEmbeddedViewer.ts` + `embeddedViewerGeometry` + viewer-launch-contract；旧文件无人引用 |
| `ui/styles/replace.css` | 归档版已以 `replace-units.css` 恢复并接入 `base.css`；现役页面另有自己的 `replace.css`（36 行简化版），两者并存无冲突 |
| `performance-test.html`、`run-tauri-with-rust-path.mjs`、`gen/schemas/macOS-schema.json` | 开发工具/平台专用产物，无任何引用，且 `performance-test.html` 未注册到 vite input（恢复后不可达） |
| `src-tauri/icons/icon.png` | `tauri.conf.json` 的 `bundle.icon` 只列 `icons/icon.ico`（已存在） |

### 两处架构差异（已适配，非字面回移）

1. **`execute_backend` 的路径解析**：归档版走 `bin/viewer-backend`（已废弃），
   现改用 `required_backend_exe_path` 的严格白名单 + `bin/public-beta` 布局。
2. **`layerService` 的转出**：其 slice/palette 表面已被 `flakeService` 取代
   （5 个同名类型），文件已恢复但只让现役实现参与 facade 转出。

### 验证基线

- `npm run build`：通过（1648 模块，产出 `dist/asset_manager.html`）
- `tsc --noEmit`：通过
- `cargo check`（desktop-nova）：通过
- `cargo test --workspace`（viewer-core）：**232 项通过，0 失败**
  （含 `replace_blocks` v2 units 5 项：多输入、单输出、加权多输出、子集匹配、v1 兼容）

## 八、逐项恢复进展

### L1 数据层 —— 已完成（本次）

| 项 | 状态 |
| --- | --- |
| `src/services/gameResources.ts`（921 行） | 已回移，与合并前逐字节一致；`tsc --noEmit` 零错误 |
| `src/platform/files.ts` | 补回 3 个接口（`BuiltinIconExtractOutput` / `VaultBlockIconDownloadOutput` / `WikiEnumCatalogDownloadOutput`）与 8 个函数（`pickLitematicFile`、`deleteAppDataFile`、`ensureBuiltinBlockIconsExtracted`、`ensureBuiltinItemIconsExtracted`、`downloadVaultBlockIconsFromVault`、`downloadVaultItemIconsFromVault`、`downloadWikiBlockIconsFromMinecraftWiki`、`downloadWikiEnumCatalogsFromMinecraftWiki`） |
| `src/services/backend.ts` | 按注册表期望的别名转出（`getUserConfigFilePath` / `readUserConfigFile` / `writeUserConfigFile` / `deleteUserConfigFile` / `ensureBuiltin*ExtractedNative`） |
| `src-tauri/src/assets.rs`（新建） | 目录辅助、`extract_builtin_icon_archive`、内建图标解包两个命令、`delete_user_config_file`、`pick_litematic_file` |
| Rust 依赖 | 新增 `flate2 = "1.1"`、`scraper = "0.27.0"`、`zip = "2.2"`（`zip` 原仅在 build-dependencies） |

**内建图标改为本地解包**：`BUILTIN_BLOCK_ARCHIVE_RELPATH` / `BUILTIN_ITEM_ARCHIVE_RELPATH`
指向 `pack-in/game-icons/{block,item}.zip`（合并前指向 `pack-in/arr-private/`），
目标仍是 app-data 下的 `minecraft-assets/block_2d/initial` 与 `minecraft-assets/item/initial`。
这样 942 个 Mojang PNG **无需入库**即可让"内建图标"这一离线回退源可用。

**Minecraft Wiki 方块图标抓取已接入**（这是必需能力，不是可选增强）：
`wiki_block_icons.rs`（682 行）现在参与编译，其父模块辅助
（`fetch_wiki_html` / `element_text` / `extract_code_values_from_cell` /
`normalize_html_text` / `normalize_resource_location` / `wiki_client` /
`WIKI_ENUM_BLOCKS_URL`）已补入 `assets.rs` 并在 `main.rs` re-export。
命令 `download_minecraft_wiki_block_icons(slot, source)` 已注册，
支持英文 `minecraft.wiki` 与中文 `zh.minecraft.wiki`。

**为什么它是必需**：新版本 Minecraft 新增的方块在任何随包图标集里都没有图标，
没有 Wiki 抓取就会"直接不显示"。

**仍未接入**：`download_minecraft_wiki_enum_catalogs`（Wiki 枚举全集的表格解析，
约 150 行）。它影响的是枚举器的 `base:dv-*` 集合，不影响方块图标显示。

### L3 界面接线清单（供界面适配用）

当前**没有管理器界面与入口**，需要新建并接回以下五处（与枚举器恢复时同样的模式）：

| 位置 | 需要做什么 | 参考 |
| --- | --- | --- |
| `desktop-nova/asset_manager.html` | 新建入口页（12 行的标准样板） | `enumerator.html` |
| `desktop-nova/ui/windows/asset_manager/` | 新建窗口与内容组件 | 逻辑 API 全部已从 `business/facade` 转出 |
| `src-tauri/capabilities/default.json` | `windows` 数组加 `"asset-manager"` | 现有 `"enumerator"` 条目 |
| `desktop-nova/vite.config.ts` | `rollupOptions.input` 加 `assetManager: resolve(__dirname, "asset_manager.html")` | 现有 `enumerator` 条目 |
| `src-tauri/src/main.rs` | 新增 `open_asset_manager_window` 命令并注册 | `open_enumerator_window` |
| `src/platform/nativeViewer.ts` | 新增 `openAssetManagerWindow()` | `openEnumeratorWindow` |
| 主界面入口 | 从设置页或工具菜单加一个「游戏资源管理」按钮 | 任意现有 `open*Window` 调用点 |

打开窗口的调用形态（与其它子窗口一致）：

```ts
import { openAssetManagerWindow } from "../../../src/platform/nativeViewer";
await openAssetManagerWindow();   // invoke("open_asset_manager_window")
```

界面可绑定的注册表 API 与失效接口见上文表格与第 7 节。

**CCVault 已接入，且不再存任何凭据**：合并前源码里带有两个硬编码 API key
（`VAULT_API_KEY` / `VAULT_LEGACY_API_KEY`，由当时的 Agent 填入）。现在改为
**运行时自动获取**——`discover_vault_api_key()` 先从站点首页 HTML 定位
`static/js/index.*.js`，再从中提取 key（先找 `/api/token` POST 前绑定的 `const e="…"`，
退化为找字面量 `x-api-key` 头值），然后用它调 `/api/token` 换 token。
仓库内已无任何 key 字符串。若站点 JS 结构变化导致发现失败，会返回
「未能从 ccvaults.com 自动获取 API Key（站点 JS 结构可能已变化）」而不是静默失败。

### L2 消费方改造 —— 已完成（本次）

四个硬编码读点已改走注册表，切换资源现在会真正生效：

| 消费方 | 改动 |
| --- | --- |
| `services/i18n.ts` | `readWorkspaceFile("pack-in/lang/zh_cn.json")` → `readActiveLanguageResource()`；失败时回退 `readFallbackLanguageResource()`；新增 `invalidateI18nCache()` |
| `services/blockstateDb.ts` | 两个硬编码 `26.1*.json` → `readActiveGameDataResource()`（返回 `dbText` / `i18nText` / `entry`）；新增 `invalidateBlockstateDbCache()` |
| `services/blockIconResolver.ts` | 硬编码 `pack-in/item-icons-26.3` → **注册表搜索根优先**（`getActiveIconSearchRoots(slot)` + `readIconFromRoot` 逐根尝试），打包目录作为最后兜底；新增 `getBlockIconDataUrl(blockId, slot)` 的槽位参数、`invalidateBlockIconCache()`、`getActiveIconFingerprint()` |
| `services/enumeratorService.ts` | 语言映射改走 `readActiveLanguageResource()`；方块全集改走 `readActiveGameDataResource()`（顺带让内建集合的 `version` 从写死的 `"bundled"` 变为真实版本号） |
| `business/facade.ts` | 新增 `export * from "../services/gameResources"`，供 L3 界面绑定 |

路径解析已核对：`readEntryFile` 对 `source === "builtin"` 的条目走**工作区**读取
（`data/minecraft_blockstates/26.1.json` 等），其余走 app-data；语言包在首次运行由
`ensureInitialLanguageSeeded()` 从 `pack-in/lang/zh_cn.json` 播种到 app-data。

**验证现状**：`tsc --noEmit` 与 `npm run build` 通过（1636 模块）。
因为 L3 界面尚未适配，暂时没有界面入口触发资源切换；L3 完成后可用
"切换图标/语言/游戏数据 → 调对应的 invalidate → 重新打开页面"来验证四条链路。

### L2 之后的已知缺口

1. **图标包目录布局**：已补上候选路径的目录层
   （`block_itemLike` / `item` / `block_2d` / `block_icon` + 平铺兜底）。
   与合并前的差异：合并前是**解析**搜索根得到精确的 `{folder}/{pack}` 组合，
   当前是按上述固定次序逐个尝试。功能上等价（多试几次路径），
   代价是文件系统调用增多。若图标包命名规则有例外，再改为解析式。
2. **图标候选规则 —— 已按你的说明补齐并纠正来源**：
   - `item_like` 分支由**枚举器规则文件**驱动，即
     **`pack-in/enumerator/custom/itemlike_block.json`**（集合 id `custom:itemlike_block`，
     随包已含 `survival_impossible` / `functional_blocks` / `tools_n_Utilities` 等 9 个集合），
     不是写死的业务规则；`loadItemLikeBlocks()` 读它，并据此把 item 风格目录排在前面。
   - `_slab` → `_planks`、`potted_*` → `flower_pot` 属 `iconCandidates` 内的**字面规则**，
     已恢复（且 `_planks` / `flower_pot` 只作为**回退**候选，不覆盖方块自身的贴图）。
   - 新增 `invalidateItemLikeRuleCache()`：切换枚举器集合后与
     `invalidateBlockIconCache()` 一起调用即可生效。

   合并前的实现顺序是「先按 `iconCandidates` 的通用候选探路径，再用
   `registryCandidates` 的 `item_id` 别名兜底」，当前实现采用同样次序
   （`[...literal, ...alias]` 去重）。若 L3 中发现个别方块缺图标，参考
   `refs/merge-safety-pre-resolution:desktop-nova/src/services/blockIconResolver.ts`。

### 仍需回移的 L1 远端部分

| 单元 | 规模 | 说明 |
| --- | --- | --- |
| Vault 闭包 | 约 250 行 | `vault_*` 系列（URL 构造、client、token 获取与 key 发现、分类条目收集、单图标下载）+ 两个 sync 下载函数 |
| Wiki 图标抓取 | 682 行（已放入仓库） | `wiki_block_icons.rs` + 5 个父模块辅助 + `WIKI_ENUM_*` 常量 |
| Wiki 枚举全集抓取 | 约 150 行 | `download_minecraft_wiki_enum_catalogs_sync` + 表格解析 |
