# 架构说明

本文说明 Litematica Nova 的代码结构、分层边界和迁移方向。当前可运行应用仍在 `desktop-nova/`，长期目标是把 Nova 主代码收束到 `src/litematicanova/`。如果本文与旧文档或旧目录习惯存在冲突，以本文约定为准。

## 核心结论

`rpr/ba` 分支中的 `src/litematicanova` 更适合作为长期目录结构：它把 Nova 相关的核心、桥接、平台和 UI 放进同一个命名空间，项目边界更直观。

当前分支的 `desktop-nova/src` 更适合作为现阶段开发结构：它已经有 `ui`、`business`、`platform` 的实际分层和 `check:layers` 检查，边界规则更可执行。

因此，近期继续在 `desktop-nova/` 开发；等业务边界、兼容入口和构建配置都成熟后，再整体迁移到 `src/litematicanova/`。

## 推荐根目录

```text
env/                         本地虚拟环境和环境文件，不进入 Git
data/                        配置、缓存、内置数据和运行资源
dist/                        最终构建成果
build/                       构建中间产物
pack-in/                     需要打包并在首次运行时解包的非必要资源
bin/                         构建、打包、启动脚本和运行时后端产物
docs/                        项目文档、约定、架构说明和迁移记录
scripts/                     仓库级维护脚本、数据生成脚本
src/litematicanova/          Litematica Nova 长期主代码根
tools/                       独立工具、原生后端源码、第三方工具补丁
third_party/                 第三方源码、资源或上游快照
```

根目录只放跨模块共享入口和仓库级文件。业务代码、UI 代码、平台桥接代码最终都应进入 `src/litematicanova/`；当前阶段不要新增新的平行应用根目录。

## 长期主代码结构

```text
src/litematicanova/
  core/                      与 UI 和具体桌面平台无关的核心业务
    litematic/               .litematic 解析、读取、写入、元数据
    material/                材料统计、材料列表、图标映射所需纯业务
    render/                  渲染缓存、预览数据、viewer 业务模型
    model/                   领域模型和值对象
    services/                核心业务服务编排
    types/                   核心层共享类型
    validation/              plan、metadata、尺寸和协议校验
    export/                  CSV、图片、投影导出逻辑
  bridge/                    前端、后端、平台之间的数据契约和转换
    dto/                     Tauri/Rust/前端传输 DTO
    events/                  跨层事件名称和 payload 类型
    mappers/                 DTO 与 core model 的互转
    tauri/                   Tauri command 的前端侧适配
  platform/                  平台相关实现
    tauri/                   Tauri/Rust 项目与 command 实现
    fs/                      文件系统、AppData、路径安全
    process/                 后端进程调用
    network/                 HTTP、下载代理、外部服务访问
    secure-storage/          API Key 等敏感信息存取
  ui/                        React/Vite UI
    app/                     应用装配、路由、窗口入口
    shell/                   AppShell、导航、主题运行时
    windows/                 多窗口入口与窗口级页面
      main/                  主窗口相关内容
        pages/               各个页面组件
      material-list/         材料列表相关内容
    components/              可复用 UI 组件
    themes/                  主题包、主题资源、控件样式
    styles/                  全局样式入口
    assets/                  UI 专用静态资源
```

## 当前阶段落位

当前分支仍以 `desktop-nova/` 作为可运行应用根目录。近期开发应继续修改 `desktop-nova`，不要把新功能直接写入 `src/litematicanova`。`src/litematicanova` 只作为成熟后的整体迁移目标，不作为当前运行入口。

当前 `desktop-nova` 与长期结构的对应关系：

```text
desktop-nova/src/ui/                 -> src/litematicanova/ui/
desktop-nova/src/business/           -> src/litematicanova/core/services/ + core/<domain>/ + bridge/
desktop-nova/src/platform/           -> src/litematicanova/platform/
desktop-nova/src-tauri/              -> src/litematicanova/platform/tauri/
desktop-nova/src/routes/             -> 迁移期兼容入口，最终并入 ui/pages 或 ui/app/routes
desktop-nova/src/components/         -> 迁移期兼容入口，最终并入 ui/components
desktop-nova/src/services/           -> 迁移期旧服务入口，后续按职责拆入 business/platform/core
desktop-nova/ui/shell/               -> 旧 shell/theme 资源入口，最终并入 ui/shell
desktop-nova/ui/themes/              -> 主题资源入口，最终并入 ui/themes
```

当前 `desktop-nova/src/business` 已覆盖大部分核心业务域：

- `ai/`：AI prompt、provider 调用、plan normalize/validate 的业务编排。
- `blockstates/`：BlockState DB、方块属性和本地化资源读取。
- `generation/`：projection plan 生成、dry-run/apply 编排。
- `layers/`：分层、cache layer 和 slice 数据业务。
- `materials/`：材料统计和材料列表业务。
- `projectionLibrary/`：本地投影库、导入、搜索和预览状态。
- `projectionMetadata/`：属性读取、metadata patch、保存/另存。
- `reden/`：RedenMC 搜索、详情、下载、导入和尺寸校验。
- `renderCache/`：渲染缓存状态、构建流程和 viewer 动作。
- `replace/`：replace-blocks dry-run/apply。
- `settings/`：主题、后端健康检查、图标库、AI 配置和普通配置。

需要继续收敛的部分：

- `business` 里既有纯领域逻辑，也有前端业务编排；迁移时拆成 `core/<domain>` 和 `core/services`。
- 跨 Tauri/Rust/前端的数据结构目前散在 `business`、`platform`、`services` 中；迁移时集中到 `bridge/dto`、`bridge/events`、`bridge/mappers`。
- `routes`、`components`、`services` 是兼容目录；新代码优先写入 `ui`、`business`、`platform`，只在需要保持旧 import 稳定时使用兼容入口。
- `desktop-nova/src-tauri` 暂时保持不动；等前端目录迁移稳定后，再整体迁入 `platform/tauri` 并调整配置路径。

## 分层职责

`core` 只表达领域规则，不依赖 React、Tauri、浏览器 API 或具体文件系统实现。核心层可以被 UI、平台命令、CLI 或测试复用。

`bridge` 负责稳定协议。所有跨进程、跨语言、跨窗口的数据结构都先落在这里，避免页面直接理解 Rust command 返回值，也避免后端协议散落在 UI 组件中。

`platform` 负责环境能力，例如 Tauri invoke、文件、AppData、进程、HTTP、系统对话框和安全存储。它暴露能力，不写页面交互逻辑。

`ui` 负责界面、交互状态和视图组合。页面可以调用 business facade 或 bridge API，但不直接拼后端命令参数，不直接读写 AppData，不直接调用底层 Tauri invoke。

当前迁移期的实际职责：

- `desktop-nova/ui/windows/main/pages/`：主窗口页面级组件。页面可以编排业务动作，但不放可复用控件实现。
- `desktop-nova/ui/windows/`：多窗口入口，例如主窗口和材料列表窗口。
- `desktop-nova/ui/components/`：跨页面、跨窗口复用组件。
- `desktop-nova/ui/shell/`：AppShell、导航、主题切换和窗口框架。
- `desktop-nova/ui/styles/`：全局样式入口。
- `desktop-nova/src/business/`：投影库、统计、生成、替换、AI、Reden、render cache、settings 等业务编排。
- `desktop-nova/src/platform/`：Tauri invoke、文件、AppData、后端进程、viewer、HTTP、key storage。

## 依赖方向

长期推荐依赖方向：

```text
ui -> bridge -> core
ui -> platform
bridge -> core
platform -> bridge
platform -> core
```

当前迁移期依赖方向：

```text
ui -> business -> platform
```

禁止方向：

- `core` 不 import `ui`、`platform` 或 Tauri API。
- `bridge` 不 import React 组件和页面。
- `platform` 不 import React 页面。
- 当前 `platform` 不允许 import `business` 或 `ui`。
- 当前 `business` 不允许 import `ui`。
- `ui` 不直接 import `@tauri-apps/api`。
- `ui` 不直接 `invoke(...)`。
- `ui` 不直接写 `litematica_core.exe` 或 `litematica_native_viewer.exe`。
- 页面文件不直接读写 AppData，不直接拼后端命令参数。

边界检查：

```powershell
cd desktop-nova
npm run check:layers
```

## UI 约定

UI 采用“应用装配、窗口、页面、组件、主题”分层。

- `ui/app/` 放路由、全局 provider、应用启动和窗口注册。
- `ui/windows/` 放多窗口入口，例如主窗口和材料列表窗口。
- `ui/windows/main/pages/` 放主窗口页面级组件。页面可以编排业务动作，但不放可复用控件实现。
- `ui/windows/<window-name>/` 放特定窗口的入口、窗口组件和窗口私有页面。
- `ui/pages/` 是迁移期兼容入口，不再新增实际页面实现。
- `ui/components/` 放跨页面、跨窗口复用组件。
- `ui/shell/` 放 AppShell、导航、主题切换和窗口框架。
- `ui/themes/<theme-name>/` 放主题 CSS、主题资源和控件样式。
- `ui/assets/` 放 UI 私有静态资源。可被多个运行时使用的大型资源应放 `data/` 或 `pack-in/`。

当前兼容入口：

- `desktop-nova/src/App.tsx` re-export `ui/AppShell`。
- `desktop-nova/src/routes/*` re-export `ui/windows/main/pages/*`。
- `desktop-nova/src/components/*` re-export `ui/components/*`。
- `desktop-nova/ui/pages/*` re-export `ui/windows/main/pages/*`，仅保留给迁移期旧 import 使用。

这些薄入口用于降低迁移风险，后续继续收窄。导出的 TypeScript 函数、组件工具函数和公共 API 必须写 JSDoc。

## Tauri 与 Rust 后端桥接

`desktop-nova/src-tauri/src/main.rs` 暴露 Tauri command，负责：

- 文件读写、路径检查、打开目录。
- AppData 用户配置目录选择、迁移、恢复默认。
- 后端 exe 调用。
- RedenMC 下载代理，避免前端直接处理跨域和下载文件。
- render cache 构建任务、progress 轮询和 watchdog。
- popup viewer 与 embedded viewer 启动、隐藏、bounds 更新。
- AI 配置和 key storage。

前端通过 `desktop-nova/src/platform` 调用 Tauri command，页面不直接 `invoke`。长期迁移后，Tauri 项目归入 `src/litematicanova/platform/tauri`。

## Rust 后端关系

运行时目录：`bin/viewer-backend/`

- `litematica_core.exe`：CLI 后端，负责 analyze、stats/materials、replace-blocks、generate、metadata edit、cache layer 等。
- `litematica_native_viewer.exe`：原生 viewer，负责 popup/embedded viewer、preview output、render cache prebuild。

源码目录：`tools/viewer-core/`

`tools/viewer-core/` 保持为独立 Rust 后端源码目录，除非后续决定把它正式纳入 `src/litematicanova/platform/native/`。修改后端源码后必须重新构建 release，并同步 exe 到 `bin/viewer-backend/`。

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

API Key 不写入 localStorage、普通 config、日志、prompt 或 plan。用户下载、RedenMC 缓存、预览缓存等写入 AppData，不写入项目源码目录。

## 数据与资源

- `data/` 放运行时配置、缓存、内置数据、BlockState DB、AI prompt config 等。
- `pack-in/` 放随应用打包并在首次运行时解包的非必要资源。
- UI 主题私有资源最终放 `src/litematicanova/ui/themes/<theme>/resource/`。
- 可被后端和前端共同消费的大型 Minecraft 资源放 `data/` 或 `third_party/`，不要复制进多个 UI 主题目录。
- 运行时 exe 和启动脚本放在 `bin/`。
- 构建中间产物进入 `build/`，最终产物进入 `dist/`。
- Cargo `target/`、Vite `dist/`、`node_modules/`、`__pycache__/` 等生成物不得进入源码目录或 Git。

## native viewer 生命周期

- popup viewer 与 embedded viewer 是不同入口。
- 页面切换时应 hide embedded viewer，不应 stop，除非 file/mode/purpose 变化。
- 只有属性页/渲染页 active 且容器 rect 有效时才 show/update bounds。
- rect 无效或页面 inactive 时 hide，不写 0 bounds。
- popup viewer 不受页面 active route 控制。

## 命名约定

- 目录名使用小写 kebab-case 或既有生态惯例；Python 包目录使用小写无连字符。
- TypeScript React 组件文件使用 PascalCase，例如 `MaterialListWindow.tsx`。
- TypeScript 普通模块使用 camelCase 或领域名目录下的 `index.ts`。
- Rust 模块使用 snake_case。
- Python 模块使用 snake_case。
- 领域名称优先稳定统一，例如 `material`、`litematic`、`render`、`projection`，避免同一概念同时出现 `stats/materials/material_list` 三套入口。

## 迁移建议

1. 继续在 `desktop-nova/` 内收敛边界，保持 `npm run check:layers` 通过。
2. 把当前 `desktop-nova/src/ui` 迁入 `src/litematicanova/ui`，保留短期 re-export 入口。
3. 把当前 `desktop-nova/src/business` 拆入 `core/services`、`core/<domain>` 和 `bridge`。
4. 把当前 `desktop-nova/src/platform` 迁入 `src/litematicanova/platform`。
5. 把 `desktop-nova/src-tauri` 迁入 `src/litematicanova/platform/tauri`，同步调整 Vite/Tauri 配置路径。
6. 扩展 `check-layer-boundaries`，让它检查新目录，而不是只检查 `desktop-nova/src`。
7. 清理迁移期兼容入口，确保新代码只从新目录新增。

迁移过程中不建议同时重构 viewer-core 渲染语义、Reden 下载逻辑、AI plan schema 或 BlockState 数据格式。目录迁移和行为变更应拆开处理。

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
