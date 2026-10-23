# 开发与构建

Nova 使用 Tauri、React、TypeScript 和 Rust。公开源码默认构建可独立使用的 **Community** 版；保留仓库锁文件和配套合法资源。

## 环境与入口

- Node.js 22、npm、Rust 本机工具链及 [Tauri 平台依赖](https://v2.tauri.app/start/prerequisites/)。
- Windows：PowerShell 7、MSVC C++、Windows SDK、WebView2，在已配置 C++ 环境的 PowerShell 中运行。没有专用 NovaCI 路径依赖；需要自行激活工具链时可显式设置 `NOVA_TOOLCHAIN_ACTIVATE`。
- Linux：GTK 3、WebKitGTK 4.1、OpenSSL、DBus 和系统编译工具；当前原生运行要求以实际平台实现为准，Wayland 桌面需要 XWayland。
- macOS：Xcode Command Line Tools。Community 的 Fast 使用 Metal，不要求 MoltenVK；完整发行路径仍按实际依赖准备。缺少 Apple 签名、公证条件时不能称已公证。

从公开仓库根目录执行：

```powershell
npm ci --prefix desktop-nova
# Windows 首选
pwsh -NoProfile -File ./bin/dev-win.ps1
# Linux / macOS，也可用于 Windows
npm run tauri:dev --prefix desktop-nova
# 本机 Community 安装/分发包
npm run tauri:build --prefix desktop-nova
# 只编译，不打包、不启动 GUI
npm run tauri:build --prefix desktop-nova -- --compile-only
```

`pwsh ./bin/dev-win.ps1 --check` 只检查必要输入；`--prepare` 生成真实公开组件但不启动界面；`--skip-prepare` 仅在已生成组件的输入身份匹配时复用，否则明确失败。可以在功能分支、fork 及未提交的正常开发修改上工作，不要求切回 main 或维护新增 crate/feature 名单。

## 可选 Standard / Full

Community 提供真实公开功能及 Fast；Standard / Full 核心属于可选私有能力。缺席不妨碍编译、启动和公开功能，主动选择未包含能力时明确提示“此构建未包含该功能”。

已有兼容签名组件的开发环境可显式运行：

```powershell
pwsh -NoProfile -File ./bin/dev-win.ps1 --runtime-source prebuilt
```

组织成员拥有私有源码时可直接构建/复用本地开发 Runtime，不需要生产签名材料：

```powershell
pwsh -NoProfile -File ./bin/dev-win.ps1 --runtime-source local --private-source <私有仓库目录>
```

本地开发 Runtime 不用于正式打包。`AppVersion` 与 `RuntimeABI`、`RuntimeBuildId` 分离；UI、I18N、业务及普通产品版本变化不强制重建兼容核心。真实 ABI/能力不兼容、必需模块缺失、签名错误仍明确失败。

## 检查、CI 与正式发行

按变更选择 `npm run build --prefix desktop-nova`、相关前端/脚本/Rust 定向检查，不用完整场景重跑替代局部验证。公共 PR/fork PR 使用 GitHub-hosted Runner，无组织权限与私有组件也能编译公开 Core、Fast、Stockpile 和 Host，并运行小型解析 CLI；不会进入持密钥私有任务或高权限本地 Runner。

正式完整版本由组织私有仓库的可信工作流选择确定的公开/私有提交，沿用既有多平台构建、安装、缩略图、保护、签名与更新流程。公开 CI 不生产私有核心，也不签官方发行包。新增普通公开模块不需要管理员扩充文件名单。

## 人工验收

编译和 CLI 通过不等同于 GPU/桌面实机验收。集中检查原生预览、切页、DPI、焦点、遮挡、主题、文件关联和安装更新。Windows 文件管理器缩略图读取共享缓存，无缓存不启动渲染器。日志可通过“选项 → 导出日志 ZIP…”保存，分享前注意个人路径。

[中文贡献说明](../CONTRIBUTING.md) · [English contribution guide](CONTRIBUTING.en.md) · [更新维护](UPDATER.md) · [第三方组件](THIRD_PARTY_UI.md) · [UI 架构](UI_ARCHITECTURE.md)
