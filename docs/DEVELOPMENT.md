# 开发与构建

Nova 使用 Tauri、React、TypeScript 和 Rust。请保留仓库内的锁文件及配套资源。

## 开发环境

- Node.js 22、npm，以及 Rust 对应平台工具链。
- Windows：MSVC、Windows SDK、PowerShell 7 和 WebView2。
- Linux：glibc 2.39 或更新版本、GTK 3、WebKitGTK 4.1、OpenSSL、DBus 及系统编译工具；Wayland 桌面需有 XWayland。
- macOS：Xcode Command Line Tools、对应架构的 Rust 工具链和 MoltenVK。构建钩子编译 Quick Look 扩展并将图形运行库放入应用包；缺少 Apple 签名及公证条件时不得标记为已公证。

## Windows 日常开发

从仓库根目录执行：

```powershell
bin\dev-win.cmd --check
bin\dev-win.cmd
```

`--check` 只检查环境、资源和组件，不构建、不启动界面。普通功能分支可以开发，不要求切换到 `main`。

已有匹配组件时，可减少重复准备：

```powershell
bin\dev-win.cmd --skip-prepare
```

该选项仍会检查组件的完整性和版本匹配；提示不匹配时，重新运行不带该选项的入口。

## 必要检查

```powershell
cd desktop-nova
npm ci
npm run build
npm run check:layers
npm run check:application-boundaries
```

脚本状态测试可从仓库根目录运行：

```powershell
node --test desktop-nova/scripts/public-development-launcher.test.mjs
```

按改动运行对应的 Rust 或前端定向测试，不用完整界面验收替代状态测试，也不用编译成功替代图形验证。

## CI 与发行构建

- Windows 和 Linux 使用本地构建环境；Linux 在 WSL 中以普通用户运行。
- macOS 使用云端 macOS 构建机，区分 Intel 和 Apple Silicon。
- 正式构建使用 `main` 的确定提交；日常开发分支不受该发布规则限制。
- 本地 CI 只运行可信的手动工作流，不接收外部 PR 自动执行，不自动清理或切换已有工作区。
- 安装包必须包含匹配的运行组件、材料数据、语言、图标及平台资源。
- 当前各平台的实际产物以成功的工作流和发布页为准；未通过的平台不标为已支持。

## 人工验收

集中检查原生预览、页面切换、DPI、焦点、遮挡、主题、文件关联和安装更新。构建及 CLI 通过不代表这些项目已实机通过。

Windows 文件管理器缩略图读取共享缓存；没有缓存时不启动渲染器。日志可在「选项 → 导出日志 ZIP…」保存，分享前注意个人路径和其它诊断信息。

更新日志见 [Beta 0.2](releases/BETA_0_2.md)，更新维护见 [UPDATER.md](UPDATER.md)，组件来源见 [THIRD_PARTY_UI.md](THIRD_PARTY_UI.md)。
