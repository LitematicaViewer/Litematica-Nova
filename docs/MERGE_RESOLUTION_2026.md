# 2026 合并解决记录：main ← 公开 Beta 线

## 背景

本次合并把公开 Beta 线（`111a0c50`）并入内部开发线 `main`（`c9060f96`）。

- 合并基点：`b2b434a1`（2026-04-27）
- 分叉后提交数：`main` 独有 124，公开线独有 145
- 未合并文件：271，冲突标记 882 个（分布 114 个文件）
- 分叉的实质：两条线各自独立重写了同一批模块，196 个文件为「双方各自新增」

## 取舍决策

> 依据：原有文件版本较旧，**前端 UI 迁移到新架构**；**保留原有主题文件与资源**以便后续融入新技术栈；**渲染部分无条件迁移到新架构**。

| 层面 | 决策 |
| --- | --- |
| 前端技术栈与页面/服务 | 采用公开线：Tailwind 4 + shadcn/radix + `src/{business,platform,services}` 分层 |
| 旧主题与主题资源 | **保留在仓库中**（`bootstrap5` / `metro10` 及图标资源），暂不参与构建 |
| Tauri 宿主 | 采用公开线：`src-tauri/{application,platform}` 分层 + `viewer-launch-contract` |
| Rust 渲染管线 | 无条件采用 `render-ir` / `canonical_ir` / `viewer-launch-contract`；删除 `full_mode` / `full_mode_v2` 与旧纹理网格管线 |
| 品牌 / 版本 | 统一为 `Litematica-Nova` / `0.1.1` |
| 资源管线 | 采用 `pack-in/item-icons-26.3`（有 1643 条 manifest 与测试依赖） |

### 批量处理结果

| 处理 | 数量 |
| --- | --- |
| 取公开线（stage 3） | 256 |
| 取我方（stage 2） | 13 |
| 双方均已删除 | 2 |

### 从公开线中特意保留的我方内容

- `data/ai-projection/prompt_config.md` 与 `prompt_config.json`：公开线删除了 `prompt_config.md`，但 `desktop-nova/src/services/aiProjection.ts:59` 仍在读取它，删除会造成运行时失败，故两者均保留。
- `docs/design.md`、`scripts/{build_assistant_index,query_assistant_index,export_diagnostics}.py`、`data/blockColorCache.json`。

## 保留的旧主题与资源（待融入新技术栈）

以下 113 个文件是从我方保留、公开线不存在的资产，**不得被新架构代码 import**：

| 路径 | 规模 |
| --- | --- |
| `desktop-nova/ui/themes/bootstrap5/**` | 15 文件 |
| `desktop-nova/ui/themes/metro10/**` | 51 文件 |
| `desktop-nova/ui/shell/nav-icons.css` | 1 文件 |
| `desktop-nova/ui/themes/minecraft/nav-icons.css` | 1 文件 |
| `desktop-nova/ui/themes/minecraft/resource/button/more.png` | 1 文件 |
| `desktop-nova/ui/themes/minecraft/resource/icon/nav/**` | 5 文件 |
| `desktop-nova/ui/shell/resource/icon/nav/**` | 5 文件 |
| `desktop-nova/ui/shell/resource/button/openInventory_*.png` | 2 文件 |

融入方向：把旧主题的 CSS 变量与资源映射到 Tailwind 4 的 `@theme` 图层与 shadcn 组件变量，使其作为可切换主题与新主题（`shadcn` / `8bitcn` / `Minecraft` / `modern`）并存。

## 本次移除并登记待迁的旧架构代码

统一从回滚点 `refs/merge-safety-pre-resolution`（= `c9060f96`）可恢复。

### 旧架构独有页面（依赖已删除的旧服务层）

- `desktop-nova/ui/windows/asset_manager/**`（4 文件）与 `asset_manager.html`
- `desktop-nova/ui/windows/enumerator/**`（3 文件）与 `enumerator.html`
- `desktop-nova/ui/windows/main/pages/replace/EnumeratorBlockPickerDialog.tsx`
- `desktop-nova/ui/windows/main/pages/properties/function.tsx`
- `desktop-nova/performance-test.html`

### 旧架构服务与平台层（新架构无引用）

`src/business/{layers,renderCache,replace}`、`src/services/{coreReadCache,embeddedViewer,gameResources,layerService,renderCacheStore,replaceSessionStore}.ts`、`src/platform/{backendProcess,embeddedViewer}.ts`、`ui/components/{BlockPickerDialog,VirtualSpacerCell}.tsx`、`ui/components/useVirtualWindow.ts`、`ui/shell/NavIcon.tsx`、`ui/styles/replace.css`

### 随渲染迁移淘汰

`tools/viewer-core/src/full_mode_v2.rs`、`tools/viewer-core/BUILD_MODES.md`、`scripts/generate_preserve_neighbor_fixture.py`

### 本次合并带入的构建垃圾（已清理）

`tmp_preview_check.png`、`.vscode/launch.json`、`tools/launcher/bin/Release/**`（5 个 .NET 构建产物）

## 渲染迁移的已知能力损失（待在新架构重建）

1. `mesh.rs` 纹理化网格路径与 `stripped_birch` 叠加（我方新增约 2295 行，`preserve_neighbor` 92 处引用）。
2. `full_mode_v2.rs` 场景装配逻辑（我方改动 4757 行）——需确认 `canonical_ir` 是否等价覆盖。
3. `native_viewer.rs` 的 prepared cache v5（`NATIVE_CACHE_CHUNK_BINARY_MAGIC_REVISION5`）与 `parse_block_color_cache` 材质色回退。
4. `metadata_edit.rs` 的 `load_preview_image_data()`（140×140 Lanczos 裁切）。
5. `replace_blocks.rs` 的 v2 units 格式与加权随机多输出（`ReplaceUnitsFile` / `replace_blocks_v2` / `weighted_random_pick` / `OutputDistEntry` / `validate_units_file` / `properties_subset_match`，我方 1174 行 vs 公开线 604 行）。取公开线是为与前端 `ReplacePage.tsx` 的 `ReplaceRulesFile` / `per_rule` 契约保持一致。

## 数据模型连带修复

公开线把 `nbt.rs` 改为值语义 + `BigEndianLongArray`，并移除预览图补丁字段，连带修正：

| 文件 | 修复 |
| --- | --- |
| `tools/viewer-core/src/visual.rs` | `(*root).clone()` → `root`（`load_litematic_root` 不再返回 `Arc`，且 `LitematicRoot` 未实现 `Clone`） |
| `tools/viewer-core/src/generate_projection.rs` | 删除 `preview_image_data: None`（`MetadataNbt` 已无该字段） |
| `desktop-nova/src/services/blockstateDb.ts` | 双方各自新增同名文件被合并成混合体，改为公开线版本（`readWorkspaceFile`） |
| `desktop-nova/reden_library.html` | 内容整体错位（内部为 `asset_manager.html`），改为公开线版本 |
| `pack-in/**`、`desktop-nova/ui/themes/**` 的 97 个文件 | 我方遗留的 100755 可执行位规范为 100644 |

## 工具链要求

- **Node ≥ 20.19**：公开线使用 vite 7.3.7 / react-router 7.18.4 / jsdom 27，系统 Node v18 不满足。已验证使用 Node v24.21.0。
- Rust：`cargo build --features public-beta`（`lib.rs` 有 `compile_error!` feature 门）。已验证 rustc 1.95.0 通过。

## 验收结果

| 检查 | 结果 |
| --- | --- |
| 未合并条目 | 0 |
| 残留冲突标记 | 0（全仓扫描） |
| `npm run build`（`tsc && vite build`） | 通过 |
| `cargo build --features public-beta` | 通过 |
| 旧主题与资源在仓库中可见 | 113 个文件 |
