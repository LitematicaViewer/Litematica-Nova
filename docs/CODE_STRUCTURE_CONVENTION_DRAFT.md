# Litematica Nova 代码结构约定草案

本文是整理项目代码结构的草案，参考当前 `dev/cleanup` 分支的实际实现，以及 `rpr/ba` 分支中 `src/litematicanova` 的目录规划。目标是把 Nova 代码收束到稳定入口下，同时保留当前分支已经比较清晰的层边界。

## 结论

如果只比较“目录是否清晰表达项目边界”，`rpr/ba` 的 `src/litematicanova` 更清晰：它把 Nova 相关的核心、桥接、平台和 UI 放进同一个命名空间，读目录就能知道这是 Litematica Nova 的主代码。

如果比较“现有实现的依赖分层是否成熟”，当前分支的 `desktop-nova/src` 更清晰：它已经有 `ui`、`business`、`platform`、`shared` 的架构说明和 `check:layers` 检查，边界规则比 `rpr/ba` 的骨架更可执行。

推荐采用折中方案：以 `rpr/ba` 的 `src/litematicanova` 作为长期目录根，以当前分支的 `ui -> business -> platform` 分层和检查规则作为实现约束。

## 推荐根目录

```text
env/                         本地虚拟环境和环境文件，不进入 Git
data/                        配置、缓存、内置数据和运行资源，不进入 Git
dist/                        最终构建成果，不进入 Git
build/                       构建中间产物，不进入 Git
pack-in/                     需要打包并在首次运行时解包的非必要资源
bin/                         构建、打包、启动脚本和运行时后端产物
docs/                        项目文档、约定、架构说明和迁移记录
scripts/                     仓库级维护脚本、数据生成脚本
src/litematicanova/          Litematica Nova 主代码
tools/                       独立工具、原生后端源码、第三方工具补丁
third_party/                 第三方源码、资源或上游快照
```

根目录只放跨模块共享的入口和仓库级文件。业务代码、UI 代码、平台桥接代码都应进入 `src/litematicanova/`，不要继续新增平行的应用根目录。

## 推荐主代码结构

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
    pages/                   页面组件
    components/              可复用 UI 组件
    themes/                  主题包、主题资源、控件样式
    styles/                  全局样式入口
    assets/                  UI 专用静态资源
```

## 分层职责

`core` 只表达领域规则，不依赖 React、Tauri、浏览器 API 或具体文件系统实现。核心层可以被 UI、平台命令、CLI 或测试复用。

`bridge` 负责稳定协议。所有跨进程、跨语言、跨窗口的数据结构都先落在这里，避免页面直接理解 Rust command 返回值，也避免后端协议散落在 UI 组件中。

`platform` 负责环境能力，例如 Tauri invoke、文件、AppData、进程、HTTP、系统对话框和安全存储。它暴露能力，不写页面交互逻辑。

`ui` 负责界面、交互状态和视图组合。页面可以调用 business facade 或 bridge API，但不直接拼后端命令参数，不直接读写 AppData，不直接调用底层 Tauri invoke。

## 依赖方向

推荐长期依赖方向：

```text
ui -> bridge -> core
ui -> platform
bridge -> core
platform -> bridge
platform -> core
```

禁止方向：

- `core` 不 import `ui`、`platform` 或 Tauri API。
- `bridge` 不 import React 组件和页面。
- `platform` 不 import React 页面。
- `ui` 不直接 import `@tauri-apps/api`。
- `ui` 不直接 `invoke(...)`。
- 页面不直接写后端 exe 名称、AppData 路径、下载缓存路径或后端命令行参数。

当前分支的 `ui -> business -> platform` 可以作为迁移期依赖方向。迁移完成后，`business` 的职责应逐步拆入 `core/services`、`bridge` 和 `platform`。

## 入口约定

每个一级模块保留稳定的 `index.ts` 或语言对应入口，外部优先从模块入口导入。

```text
src/litematicanova/core/index.ts
src/litematicanova/bridge/index.ts
src/litematicanova/platform/index.ts
src/litematicanova/ui/app/main.tsx
```

兼容入口允许短期存在，例如从旧路径 re-export 新路径，但必须标注迁移目的，并避免在兼容入口中新增业务逻辑。

## UI 目录约定

UI 采用“应用装配、窗口、页面、组件、主题”分层。

- `ui/app/` 放路由、全局 provider、应用启动和窗口注册。
- `ui/windows/` 放多窗口入口，例如主窗口和材料列表窗口。
- `ui/pages/` 放页面级组件。页面可以编排业务动作，但不放可复用控件实现。
- `ui/components/` 放跨页面复用组件。
- `ui/shell/` 放 AppShell、导航、主题切换和窗口框架。
- `ui/themes/<theme-name>/` 放主题 CSS、主题资源和控件样式。
- `ui/assets/` 放 UI 私有静态资源。可被多个运行时使用的大型资源应放 `data/` 或 `pack-in/`。

导出的 TypeScript 函数、组件工具函数和公共 API 必须写 JSDoc。纯页面组件如果作为默认内部组件使用，可按项目 ESLint/文档规则收敛。

## 后端与工具约定

`tools/viewer-core/` 保持为独立 Rust 后端源码目录，除非后续决定把它正式纳入 `src/litematicanova/platform/native/`。在未完成迁移前，不建议把 viewer-core 和 Tauri 平台壳混放。

运行时 exe 和启动脚本放在 `bin/`。构建中间产物进入 `build/`，最终产物进入 `dist/`。Cargo `target/`、Vite `dist/`、`node_modules/`、`__pycache__/` 等生成物不得进入源码目录或 Git。

## 数据与资源约定

- `data/` 放运行时配置、缓存、内置数据、BlockState DB、AI prompt config 等。
- `pack-in/` 放随应用打包并在首次运行时解包的非必要资源。
- UI 主题私有资源放 `src/litematicanova/ui/themes/<theme>/resource/`。
- 可被后端和前端共同消费的大型 Minecraft 资源放 `data/` 或 `third_party/`，不要复制进多个 UI 主题目录。
- 用户下载、RedenMC 缓存、预览缓存等写入 AppData，不写入项目源码目录。

## 命名约定

- 目录名使用小写 kebab-case 或既有生态惯例；Python 包目录使用小写无连字符。
- TypeScript React 组件文件使用 PascalCase，例如 `MaterialListWindow.tsx`。
- TypeScript 普通模块使用 camelCase 或领域名目录下的 `index.ts`。
- Rust 模块使用 snake_case。
- Python 模块使用 snake_case。
- 领域名称优先稳定统一，例如 `material`、`litematic`、`render`、`projection`，避免同一概念同时出现 `stats/materials/material_list` 三套入口。

## 与两个分支的对比

当前分支优点：

- `desktop-nova/src` 已经有 `ui`、`business`、`platform` 的实际分层。
- `docs/ARCHITECTURE.md` 已明确依赖方向和禁区。
- `scripts/check-layer-boundaries.mjs` 让架构边界可检查。
- 业务模块按能力拆分，覆盖 AI、BlockState、生成、统计、渲染缓存、替换、Reden、设置等实际功能。

当前分支问题：

- Nova 主应用在 `desktop-nova/`，没有收束到 `src/litematicanova/`。
- `src/`、`desktop-nova/`、`tools/` 的项目边界不够直观。
- `routes/`、`services/`、`components/` 与 `ui/`、`business/`、`platform/` 并存，迁移期兼容入口容易让新代码选错位置。
- Tauri 项目位于 `desktop-nova/src-tauri`，和长期 `platform` 边界不完全一致。

`rpr/ba` 优点：

- `src/litematicanova` 作为统一命名空间，项目身份清楚。
- `core`、`bridge`、`platform`、`ui` 的一级分区更适合长期维护。
- Tauri 位于 `src/litematicanova/platform/tauri`，平台归属更明确。
- UI 下有 `shell`、`themes`、`windows`，窗口和主题边界比当前兼容目录更集中。

`rpr/ba` 问题：

- `core` 和 `bridge` 多数还是 `.gitkeep` 骨架，尚未承载真实业务。
- 缺少当前分支已有的 `business` 能力拆分和层检查脚本。
- UI 主题资源较完整，但业务、平台和协议之间的可执行约束还不够。
- `ui/shell`、`ui/windows`、`ui/themes` 清晰，但未来仍需要补上 `pages`、`components`、`app` 等统一约定。

因此，目录清晰度建议判定为：`rpr/ba` 的目标结构更清晰；当前分支的实现分层更成熟。整理代码时优先以 `rpr/ba` 的目录骨架作为目的地，以当前分支的分层规则作为迁移标准。

## 迁移建议

1. 先在文档中确认 `src/litematicanova` 为长期主代码根。
2. 把当前 `desktop-nova/src/ui` 迁入 `src/litematicanova/ui`，保留短期 re-export 入口。
3. 把当前 `desktop-nova/src/business` 拆入 `core/services`、`core/<domain>` 和 `bridge`。
4. 把当前 `desktop-nova/src/platform` 迁入 `src/litematicanova/platform`。
5. 把 `desktop-nova/src-tauri` 迁入 `src/litematicanova/platform/tauri`，同步调整 Vite/Tauri 配置路径。
6. 扩展 `check-layer-boundaries`，让它检查新目录，而不是只检查 `desktop-nova/src`。
7. 清理迁移期兼容入口，确保新代码只从新目录新增。

迁移过程中不建议同时重构 viewer-core 渲染语义、Reden 下载逻辑、AI plan schema 或 BlockState 数据格式。目录迁移和行为变更应拆开处理。
