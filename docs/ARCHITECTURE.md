# 架构说明

本文说明 Litematica-BA 当前主线 `desktop-nova` 的架构边界。目标是让 UI、业务和平台能力可维护地分离，同时保持 viewer-core 渲染语义稳定。

## 总体结构

```text
desktop-nova/src/
  ui/          UI 层：页面、组件、AppShell、样式
  business/    业务层：投影库、统计、生成、替换、AI、Reden、render cache
  platform/    平台层：Tauri invoke、文件、AppData、后端进程、viewer、HTTP、key storage
  shared/      共享类型、常量、纯工具
```

依赖方向固定为：

```text
ui -> business -> platform
shared 可被各层引用
```

禁止反向依赖：

- `platform` 不允许 import `business` 或 `ui`。
- `business` 不允许 import `ui`。
- `ui` 不允许直接 import `@tauri-apps/api`。
- `ui` 不允许直接 `invoke(...)`。
- `ui` 不允许直接写 `litematica_core.exe` 或 `litematica_native_viewer.exe`。
- 页面文件不直接读写 AppData，不直接拼后端命令参数。

边界检查：

```powershell
cd desktop-nova
npm run check:layers
```

## UI 层

路径：`desktop-nova/src/ui/`

职责：

- 渲染 AppShell、导航、页面、弹窗和组件。
- 保持 Nova 视觉、主题和交互布局。
- 接收用户输入并调用 business facade/actions。
- 不关心 Tauri command 名称、后端 exe 名称、AppData 路径或网络实现。

兼容入口：

- `desktop-nova/src/App.tsx` re-export `ui/AppShell`。
- `desktop-nova/src/routes/*` re-export `ui/pages/*`。
- `desktop-nova/src/components/*` re-export `ui/components/*`。

这些薄入口用于降低迁移风险，后续可继续收窄。

## 业务层

路径：`desktop-nova/src/business/`

职责：

- 投影库：导入、入库、搜索、预览状态。
- 属性：analyze、metadata patch、保存/另存。
- 统计：材料、容器物品统计、CSV 导出。
- 分层：cache layer meta 和 slice 数据。
- 渲染：render cache 状态、构建流程、viewer 动作。
- 替换：replace-blocks dry-run/apply。
- 生成：模板、projection plan、dry-run/apply。
- AI：prompt_config、plan normalize/validate、API chat。
- Reden：搜索、详情、下载、尺寸校验、导入。
- Settings：主题、后端健康检查、BlockState DB、图标库、AI 配置。

业务层可以调用 platform，但不能 import React 页面或组件。当前 `business/facade.ts` 仍保留部分兼容 re-export，作为分层迁移期的低风险入口。

## 平台层

路径：`desktop-nova/src/platform/`

职责：

- `tauri.ts`：唯一底层 Tauri invoke 封装。
- `dialogs.ts`：打开/保存/消息确认对话框。
- `files.ts`：工作区文件、AppData 文件、路径检查、打开目录、图片读取。
- `appData.ts`：用户配置目录、普通配置读写。
- `backendProcess.ts`：`litematica_core` / `litematica_native_viewer` 后端进程调用。
- `nativeViewer.ts`：popup viewer、材料列表窗口、系统打开路径。
- `embeddedViewer.ts`：嵌入式 native viewer 生命周期和 bounds。
- `keyStorage.ts`：AI key/config 命令。API Key 不进入 localStorage、普通 config、日志或 prompt。
- `http.ts`：HTTP/fetch 和 Reden 后端命令封装。
- `events.ts`：Tauri event 封装。

平台层只暴露底层能力，不包含业务判断，不 import UI 或 business。

## Tauri 与 Rust 后端桥接

`desktop-nova/src-tauri/src/main.rs` 暴露 Tauri command，负责：

- 文件读写、路径检查、打开目录。
- AppData 用户配置目录选择、迁移、恢复默认。
- 后端 exe 调用。
- RedenMC 下载代理，避免前端直接处理跨域和下载文件。
- render cache 构建任务、progress 轮询和 watchdog。
- popup viewer 与 embedded viewer 启动、隐藏、bounds 更新。
- AI 配置和 key storage。

前端通过 platform 调用 Tauri command，页面不直接 `invoke`。

## Rust 后端关系

运行时目录：`bin/viewer-backend/`

- `litematica_core.exe`：CLI 后端，负责 analyze、stats/materials、replace-blocks、generate、metadata edit、cache layer 等。
- `litematica_native_viewer.exe`：原生 viewer，负责 popup/embedded viewer、preview output、render cache prebuild。

源码目录：`tools/viewer-core/`

修改后端源码后必须重新构建 release，并同步 exe 到 `bin/viewer-backend/`。

## AppData 用户数据

默认用户态目录：

```text
%AppData%\Litematica-BA\desktop-nova
```

用户态数据包括：

- `projection-library/js_library.json`
- `previews/`
- `render/`
- `reden/downloads/`
- `generation-templates/custom/`
- 普通 config
- AI 普通配置

不搬到 AppData 的运行资源：

- `data/minecraft_blockstates/`
- `data/generation-templates/` 内置模板
- `data/ai-projection/prompt_config.md`
- block 图标库
- render assets

API Key 不写入 localStorage、普通 config、日志、prompt 或 plan。

## native viewer 生命周期

- popup viewer 与 embedded viewer 是不同入口。
- 页面切换时应 hide embedded viewer，不应 stop，除非 file/mode/purpose 变化。
- 只有属性页/渲染页 active 且容器 rect 有效时才 show/update bounds。
- rect 无效或页面 inactive 时 hide，不写 0 bounds。
- popup viewer 不受页面 active route 控制。

## 禁区

以下内容不作为 UI 或架构重构的顺手修改对象：

- 不改 `desktop-js`。
- 不改 viewer-core 渲染语义。
- 不改 `native_viewer.rs` 来解决 UI 问题。
- 不碰 MC Light。
- 不碰 Full Mode V2 模型/贴图语义。
- 不用假数据冒充真实后端。
- 不清 unrelated warning。
- 不把中文写入 plan/blockstate JSON 的内部字段、命令参数或后端协议。
- 不把 API Key 写入 localStorage、普通 config、日志、prompt 或文档。
