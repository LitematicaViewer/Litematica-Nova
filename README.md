# Litematica-BA

Litematica-BA 是面向 Minecraft `.litematic` 投影文件的桌面工具。当前主线是 `desktop-nova`，使用 React + Tauri 提供 UI，调用 Rust 后端完成解析、统计、生成、替换、cache 和原生 viewer。

## 当前主线

- `desktop-nova/`：当前桌面端主线，包含 UI、Tauri 后端桥接、Nova 主题和页面入口。
- `tools/viewer-core/`：Rust 后端源码，提供 `litematica_core` 和 `litematica_native_viewer`。
- `bin/viewer-backend/`：桌面应用运行时读取的后端 exe 目录，文件应由 `tools/viewer-core` release build 同步而来。
- `data/`：运行资源和用户态数据，包括生成模板、AI prompt、BlockState DB、中文翻译、投影库、cache、stockpile、Reden 下载和导出物。
- `scripts/`：维护脚本、诊断导出、索引生成和数据生成脚本。

`desktop-js` 已从 Git 主线排除。若本地工作区仍存在 `desktop-js/`，它只作为历史参考，不上传、不作为功能规格、不参与当前验证。

## 已接入能力

- 选择本地 `.litematic` 并 analyze。
- `data/` 投影库。
- RedenMC 在线投影库搜索、详情、普通下载、参数化下载、入库。
- 属性页 metadata 编辑、保存、另存、恢复。
- 统计页和材料列表，支持可选统计容器内物品。
- 材料 CSV 导出，UTF-8 BOM，列为 `名称,数字,统计数据`。
- Stockpile 材料数据导出、合成表缓存 preflight、离线网页 ZIP 导出和本地 serve/SQLite 协作。
- 渲染页 3D cache 构建、进度轮询、静态预览、嵌入 viewer、弹窗 viewer。
- 分层页读取真实 cache。
- 替换页 dry-run/apply。
- 生成页模板、AI plan、API 对话、dry-run/apply。
- 选项页主题、用户数据配置、后端检查、AI 设置。

## 怎么运行 desktop-nova

安装依赖：

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-nova
npm install
```

开发运行：

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-nova
npm run tauri dev
```

构建前端：

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-nova
npm run build
```

检查层边界：

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-nova
npm run check:layers
```

检查 Tauri 后端：

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-nova\src-tauri
cargo check
```

## 关键目录

| 路径 | 用途 |
| --- | --- |
| `desktop-nova/src/ui/` | UI 层：AppShell、页面、组件、样式。 |
| `desktop-nova/src/business/` | 业务层：投影库、属性、统计、替换、生成、AI、Reden、渲染 cache 等 facade/actions。 |
| `desktop-nova/src/platform/` | 平台层：Tauri、文件、data 用户目录、后端进程、viewer、HTTP、key storage。 |
| `desktop-nova/src/services/` | 兼容旧 import 的薄服务入口，逐步收敛到 business/platform。 |
| `desktop-nova/src-tauri/` | Tauri Rust 端命令和窗口桥接。 |
| `tools/viewer-core/` | Rust 核心库和两个运行时二进制源码。 |
| `bin/viewer-backend/` | desktop-nova 调用的运行时 exe。 |
| `data/ai-projection/` | AI prompt 和 plan 相关运行配置，不能当普通文档删除。 |
| `data/minecraft_blockstates/` | BlockState DB 和 overrides，运行资源。 |
| `data/generation-templates/` | 内置生成模板，运行资源。 |

## 常用命令

```powershell
# 生成维护索引
python scripts\build_assistant_index.py

# 导出诊断包
python scripts\export_diagnostics.py

# 检查 viewer-core
cd tools\viewer-core
cargo check

# stockpile 合成表缓存状态/抓取
cd ..\..
bin\viewer-backend\litematica_core.exe stockpile recipe-status --minecraft-version 1.21.10
bin\viewer-backend\litematica_core.exe stockpile recipe-fetch --minecraft-version 1.21.10
bin\viewer-backend\litematica_core.exe stockpile export-zip --input tools\viewer-core\tests\fixtures\stats_water_fixture.litematic --output data\stockpile\exports\test.stockpile.zip --minecraft-version 1.21.10
bin\viewer-backend\litematica_core.exe stockpile serve --zip data\stockpile\exports\test.stockpile.zip --bind 127.0.0.1:8787
bin\viewer-backend\litematica_core.exe stockpile session-info --zip data\stockpile\exports\test.stockpile.zip
bin\viewer-backend\litematica_core.exe stockpile session-export --zip data\stockpile\exports\test.stockpile.zip --output data\stockpile\sessions\test.state.json
bin\viewer-backend\litematica_core.exe stockpile session-reset --zip data\stockpile\exports\test.stockpile.zip --yes
bin\viewer-backend\litematica_core.exe stockpile session-import --zip data\stockpile\exports\test.stockpile.zip --input data\stockpile\sessions\test.state.json --replace

# 构建并同步 litematica_core
cd tools\viewer-core
cargo build --release --bin litematica_core
Copy-Item target\release\litematica_core.exe ..\..\bin\viewer-backend\litematica_core.exe -Force

# 构建并同步 native viewer
cd tools\viewer-core
cargo build --release --bin litematica_native_viewer
Copy-Item target\release\litematica_native_viewer.exe ..\..\bin\viewer-backend\litematica_native_viewer.exe -Force
```

## 维护文档

主文档只保留三份：

- `README.md`：项目入口和运行方式。
- `docs/ARCHITECTURE.md`：架构、分层、后端桥接和禁区。
- `docs/DEVELOPMENT.md`：开发、验证、schema、Reden、BlockState、清理和排障。
