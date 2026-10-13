# 公开版开发说明

Beta 0.2 当前本地验收仍仅提供 Windows x64。macOS/Linux 为后续计划；当前 Application Host 的进程监督依赖 Windows Job Objects，非 Windows 启动会明确报错，不以编译成功声称支持。

## 环境

Windows、Node.js 22、Rust stable/MSVC、WebView2，以及 Tauri 官方 Windows 构建依赖。公开快速模式不需要 Java、Iris 或完整模式适配产物。仓库包含获准再分发的游戏图标与统计资源。

## Windows NSIS 安装包

从仓库根目录执行 PowerShell：

```powershell
$ErrorActionPreference = "Stop"
$env:LBA_BUILD_GIT_SHA = (git rev-parse HEAD).Trim()
& .\tools\windows-thumbnail-provider\build.ps1 -Smoke
cargo build --locked --release --manifest-path tools/viewer-core/Cargo.toml --target-dir tools/viewer-core/target/public-beta --no-default-features --features public-beta --bin litematica_core --bin litematica_native_viewer --bin stockpile_server
if ($LASTEXITCODE -ne 0) { throw "快速后端构建失败" }
.\desktop-nova\scripts\prepare-backend-deployment.ps1 -RepoRoot $PWD.Path -Profile release -GitSha $env:LBA_BUILD_GIT_SHA
$env:CARGO_TARGET_DIR = Join-Path $PWD.Path 'desktop-nova/src-tauri/target/public-beta'
Set-Location desktop-nova
npm ci
if ($LASTEXITCODE -ne 0) { throw "前端依赖安装失败" }
npm run tauri -- build --bundles nsis
if ($LASTEXITCODE -ne 0) { throw "桌面构建失败" }
```

输出为 `desktop-nova/src-tauri/target/public-beta/release/bundle/nsis/*-setup.exe`。NSIS 使用 perMachine 安装，将显式映射的资源与快速后端包含在同一个 Setup.exe 内；不发布 ZIP/MSI。签名构建须从仓库外提供 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，或在 CI 构建后用官方 `tauri signer sign --app-version 0.2.0` 对原始安装包签名。运行 `Litematica-Nova.exe --verify-public-resources` 核对后端身份、哈希及游戏资源；此 CLI 不启动界面。

## 验证

- 前端：`npm run build`、`npm run check:layers`、`npm run check:application-boundaries`；相关业务测试使用 `node --test`。
- 后端：分别运行 viewer-core lib、viewer-launch-contract 和 Tauri Application Host 的 CLI 单测。
- 公开构建门禁：`node desktop-nova/scripts/check-public-build.mjs`，产物检查参数见该脚本。
- 原生预览、DPI、焦点、遮挡、主题视觉与实际安装更新交给人工验收，不以编译、CLI 单测替代。

## 公开边界与发布

- 公开版物理只保留原有 Fast/wgpu 渲染路径，以及投影读写、材料统计、生成和 Stockpile。Full/V5/Iris 源码、Cargo 依赖、服务程序与专用资源不进入当前公开树和产物。
- 缺失完整模式明确显示“完整模式未包含在公开版”。快速模式不冒充 Full，不复制另一套渲染算法。
- 私有开发历史不合并进公开 `main`；仅选择性导入已审计文件。允许保留的旧版公开历史不代表当前商业实现可公开。
- Beta Tag 指向公开 `main` 的审计提交；GitHub Actions 只构建 Windows 审计产物，不自动创建正式 Release。其他平台本版不构建、不宣称可运行。
- 更新维护见 [UPDATER.md](UPDATER.md)，组件来源见 [THIRD_PARTY_UI.md](THIRD_PARTY_UI.md)。

## Beta 0.2 本地验收

文件关联由 NSIS 安装/卸载 hook 管理；官方 single-instance 2.4.0 与 Tauri 2.11.1 配合，外部文件请求排队后交已有主窗口。Native Thumbnail Provider 仅读取按投影内容 SHA256 命名的 PNG 缓存，不启动渲染器，不解析完整投影。共享缩略图目录为 %LOCALAPPDATA%\Litematica-Nova\thumbnails。

启动先绘制缓存主题，配置异步加载；分层资源按需初始化。退出停止任务接纳并在后台等待所属子进程，生命周期日志分别标记资源身份、配置迁移和子进程退出。网络更新原本已异步，未改变签名要求。

统计分阶段日志由 NOVA_STATS_TIMING 开启，Host 共享同文件同参数的请求及结果；React Profiler 只报告实际挂载的提交时间。CLI 的 helper transport 不等于真实 WebView IPC，后两项留待人工读取运行日志。

本轮只本地构建；不推送、不创建 Tag、不修改线上更新清单。通过 docs/releases/BETA_0_2.md 的人工清单后再发布。
