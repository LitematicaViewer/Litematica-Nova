# 参与贡献 / Contributing

Nova 的公开仓库可以独立构建 **Community** 版本，不需要组织成员身份、私有仓库、运行组件下载凭据或生产签名材料。

## 中文

### 环境

安装 Node.js 22、Rust 对应平台工具链和 [Tauri 平台依赖](https://v2.tauri.app/start/prerequisites/)。Windows 使用 PowerShell 7、MSVC C++ 工具及 Windows SDK（从已配置 C++ 环境的 PowerShell 运行）；Linux 使用 GTK 3 / WebKitGTK 4.1 等系统库；macOS 使用 Xcode Command Line Tools。无需 `D:\NovaCI` 或专用 Runner。

### 开发与构建

在公开仓库根目录执行：

```powershell
npm ci --prefix desktop-nova
pwsh -NoProfile -File ./bin/dev-win.ps1
```

Windows 首选上述 PowerShell 入口；Linux/macOS 使用同一公开构建入口：

```powershell
npm run tauri:dev --prefix desktop-nova
```

构建本机 Community 安装/分发包：

```powershell
npm run tauri:build --prefix desktop-nova
```

仅编译、不打安装包、不启动 GUI：

```powershell
npm run tauri:build --prefix desktop-nova -- --compile-only
```

默认使用真实公开 Core、Fast Viewer 和应用业务：投影解析、属性、投影库、材料、分层及已公开编辑功能。Standard / Full 渲染核心不包含在 Community 中；主动使用时明确提示“此构建未包含该功能”，不影响公开功能及应用启动。Community 包不需要生成官方更新签名，不代表官方正式发行。

功能分支、fork、合法新增 crate / feature / 文件均可贡献。修改后按受影响模块做必要检查并提交 PR；公开 PR CI 在 GitHub-hosted Runner 上构建真实 Community，不接触私有源码和密钥。请勿提交生成产物、个人数据、运行模块或凭据。

组织成员的完整开发和正式签名入口见私有仓库说明。已有兼容签名组件可显式使用 `--runtime-source prebuilt`；拥有私有源码的成员可用 `--runtime-source local --private-source <私有仓库目录>`。这两个选项都不是公开贡献的前置条件。

## English

See [CONTRIBUTING.en.md](docs/CONTRIBUTING.en.md) for the same instructions in English.
