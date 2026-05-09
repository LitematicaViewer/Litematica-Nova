# Litematica-BA 维护入口

本文是维护入口，不是产品介绍。

## 当前主线

- 活跃桌面 UI：`desktop-nova/`。
- Rust 后端和原生 viewer：`tools/viewer-core/`。
- 应用运行时二进制：`bin/viewer-backend/`。
- 主数据和配置区：`data/`。
- 维护脚本：`scripts/`。
- 旧 JS 桌面 UI 已从 Git 跟踪中排除，只保留为本地历史参考，不上传，不作为维护入口。
- 已删除旧路径：`src/litematicaba/`、`desktop-ui/`、`script/`。

## 先读这些

1. `docs/DESKTOP_NOVA_STATUS.md`：desktop-nova 当前状态和缺口。
2. `docs/PROJECT_MAP.md`：项目目录和主线说明。
3. `docs/FEATURE_ENTRYPOINTS.md`：功能入口和调用链。
4. `docs/DATA_FLOW.md`：数据流、缓存、生成物。
5. `docs/MODULE_BOUNDARIES.md`：模块边界和回归要求。
6. `docs/DIAGNOSTICS.md`：诊断包导出和阅读方法。

## 常用命令

```powershell
python scripts\build_assistant_index.py
python scripts\query_assistant_index.py "desktop-nova"
python scripts\export_diagnostics.py

cd desktop-nova
npm run build

cd src-tauri
cargo check
```

## 不要手改

- `data/minecraft_blockstates/*.json` 这类生成出的 BlockState DB；应修改 override 或生成脚本后重新生成。
- `diagnostics/bundle_*` 诊断包；需要时重新导出。
- `desktop-nova/dist/`、`desktop-nova/node_modules/`、Rust `target/` 等构建产物。
- `bin/viewer-backend/` 里的运行时 exe；必须来自 `tools/viewer-core` 的 release build。
- cache 文件和 manifest，除非任务明确要求维护 cache。

## 清理规则

清理磁盘空间时遵循 `docs/CLEANUP_POLICY.md`。

可再生成的构建产物可以删除；不确定用途的 fixture、截图、根目录零散调试文件应先移动到仓库外隔离区，不要直接永久删除。

## AI/Codex 维护流程

1. 先读本文和上面列出的文档。
2. 大范围搜索前先重建或查询维护索引。
3. 编辑前先定位功能入口和调用链。
4. 保持 UI、service、Tauri、viewer-core 的边界清晰。
5. 修 UI 问题时不要顺手改 viewer-core mesh、Full Mode、cache 协议、AI plan 语义或 BlockState 生成逻辑。
6. 修改共享契约后运行对应 build/check/test。

## 顶层清理说明

- 仓库根目录仍可能有 renderer 调查遗留的 fixture、截图、日志。默认按生成物或调试产物处理，除非文档或测试明确引用。
- 低风险整理时不要移动核心目录：`desktop-nova/`、`tools/viewer-core/`、`data/`、`bin/viewer-backend/`。
- 不确定归属时，先补说明文档，不要大规模移动或删除。

