# 项目地图

本文给维护者使用，用于快速判断当前主线、目录职责和调试入口。

## 当前主线

- `desktop-nova/`：当前桌面 UI 主线，React + Tauri。
- `tools/viewer-core/`：Rust 后端、`.litematic` 解析、生成、替换、cache、原生 viewer。
- `bin/viewer-backend/`：运行时 exe，来自 `tools/viewer-core` release build。
- `data/`：内置模板、AI prompt、BlockState DB、资源数据。
- `scripts/`：维护脚本和数据生成脚本。
- `docs/`：维护文档。

旧 JS 桌面 UI 已从 Git 跟踪中排除，只保留本地参考，不再作为维护入口。

## 顶层目录

| 路径 | 职责 | 是否可改 |
|---|---|---|
| `desktop-nova/` | 当前桌面 UI 主线 | 可以，需跑前端和 Tauri 检查 |
| `tools/viewer-core/` | Rust 后端和原生 viewer | 可以，需跑 Rust 检查和相关测试 |
| `bin/viewer-backend/` | 运行时 exe | 不手改，只从 release build 复制 |
| `data/` | 模板、AI、BlockState、资源 | 视子目录而定 |
| `scripts/` | 维护和生成脚本 | 可以 |
| `docs/` | 文档 | 可以 |
| `.tmp/` | 临时输出和 cache | 可清理 |
| `.external/` | 本地参考仓库 | 不上传，可按需删除 |

## 常见问题先看哪里

| 问题 | 先看 |
|---|---|
| UI 行为 | `desktop-nova/src/routes/` |
| 前端后端桥接 | `desktop-nova/src/services/`、`desktop-nova/src-tauri/src/main.rs` |
| `.litematic` analyze/stats/materials | `tools/viewer-core/src/analyze.rs`、`tools/viewer-core/src/stats_api.rs` |
| 生成/替换 | `tools/viewer-core/src/generate_projection.rs`、`tools/viewer-core/src/replace_blocks.rs` |
| 3D cache | `desktop-nova/src/routes/RenderPage.tsx`、`tools/viewer-core/src/cache_layer.rs` |
| 原生 viewer | `tools/viewer-core/src/native_viewer.rs` |
| Full Mode 材质/模型 | `tools/viewer-core/src/full_mode_v2.rs` |
| BlockState DB | `data/minecraft_blockstates/`、相关生成脚本 |
| AI plan | `desktop-nova/src/services/aiProjection.ts`、`data/ai-projection/prompt_config.md` |

## 高风险区域

- `tools/viewer-core/src/native_viewer.rs`
- `tools/viewer-core/src/full_mode_v2.rs`
- `tools/viewer-core/src/mesh.rs`
- `tools/viewer-core/src/cache_layer.rs`
- `desktop-nova/src-tauri/src/main.rs`
- `data/minecraft_blockstates/*.json`

这些区域会影响渲染、cache 协议、进程管理、AI Key、BlockState 校验。修改后必须做针对性验证。

## 生成物和缓存

可重新生成：

- `desktop-nova/dist/`
- `desktop-nova/node_modules/`
- `desktop-nova/src-tauri/target/`
- `tools/viewer-core/target/`
- `.tmp/`
- `diagnostics/bundle_*`
- 本地验证图和日志

不要手改：

- `bin/viewer-backend/*.exe`
- 生成出的 BlockState DB
- cache manifest 和 chunk payload，除非任务明确要求维护 cache。

