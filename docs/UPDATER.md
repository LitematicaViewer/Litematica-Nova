# 自动更新与签名

Nova 使用 **Tauri 官方 Updater 2.10.0**，与锁定的 Tauri 2.11.1 运行栈配合。更新编排在 `desktop-nova/src-tauri/src/application/updater.rs`，WebView 仅调用应用命令，不持有安装包字节、签名私钥或直接安装权限。

## 当前配置与行为

1. 启动后异步检查 `https://raw.githubusercontent.com/LitematicaViewer/Litematica-Nova/main/updates/latest.json`。
2. 网络、HTTP、解析失败自动检查 `https://45-62-112-121.sslip.io:8448/nova-updates/latest.json`。成功的 `204` 表示没有更新，不当作失败。
3. 浮窗显示版本、日期、更新说明、来源及下载进度，提供更新、取消和跳过当前版本。手动「检查更新」可重新查看已跳过版本。
4. 确认后先保存当前文件/页面，以及属性、替换、生成和分层草稿。签名校验完成后再保存最新草稿，暂停新任务，等待写入/构建任务自然结束，关闭预览并等待后台退出，随后官方插件安装并重启。
5. 取消会取消检查/下载 future；等待任务阶段也可以取消，不强杀正在写文件的任务。开始最终停服阶段后禁用取消与新任务；若停服/安装失败，保留草稿并提供「重启恢复」，不会恢复已失效服务的业务入口。草稿存储失败则不开始安装。
6. 下载失败仅对配置中的其他源各重试一次；镜像必须返回**同版本、同安装形式、同签名**，并重新由官方插件下载、强制校验。HTTPS 限制同时覆盖接口、安装包和重定向；不接受无效证书，不使用明文降级。

更新前的确认不会跨重启恢复，恢复替换或生成草稿后须重新预览。分层方块状态与测量恢复为会话草稿，不伪称写入投影。

Beta 0.2 的 Windows x64 使用单文件 NSIS Setup.exe，Linux x64 使用 AppImage 或 Debian 安装包，macOS Intel/Apple Silicon 分别使用对应应用更新归档；安装与更新载荷包含匹配的运行组件和全部映射资源。CI 构建审计后由发布者在本机使用官方 Tauri signer 签名，安装包字节不变，签名放入更新清单。GitHub Pre-release 不会出现在 `/releases/latest`，因此主更新接口使用公开 `main/updates/latest.json`。真实安装、重启和主题显示未进行 GUI 验收；CLI 签名与安装包静态检查不等于安装端到端验收。

## 发布与密钥边界

更新签名私钥与密码仅由发布者在仓库外管理，不进入源码、服务器、构建产物或 CI 日志。客户端只内置公钥。CI 当前只生成审计用构建产物，不自动创建正式 GitHub Release，不配置签名私钥。

Windows 更新载荷为包含全部资源的签名 NSIS Setup.exe，不提供便携 EXE、ZIP 或 MSI；Linux 更新载荷分别为 AppImage 和 `.deb`，由官方插件按当前安装形式选择，macOS 为官方更新格式的 `.app.tar.gz`（DMG 用于初次安装）。签名、安装包 SHA256、版本一致性及双 HTTPS 源通过 CLI 检查后发布清单；无清单时镜像返回 204。VPS 每个版本只保存安装包、平台更新载荷、更新说明和必要元数据，不存源码或构建缓存。

更新简介使用对应版本的更新日志；Beta 0.2 见 [更新日志](releases/BETA_0_2.md)。

### VPS 安装包保留规则

VPS 永久只保留当前线上最新版的各平台安装包、更新载荷、说明及必需元数据，不保留历史安装包。先上传并验明新版载荷，再切换正式清单；清单切换后由 `prune-vps-updates.py` 清理旧版本。新版载荷不完整时不清理；尚在上传的更高版本目录不删除。该规则仅针对 VPS 更新镜像，GitHub 历史发行版、核心组件、密文及构建登记不受影响。

## 唯一更新说明格式

平台键为 `windows-x86_64`、`linux-x86_64-appimage`、`linux-x86_64-deb`、`darwin-x86_64` 和 `darwin-aarch64`。`latest.json` 遵循官方静态清单：`version`、RFC3339 `pub_date`、`notes`、`platforms.<平台-架构>.url` 与 `.signature`。后者是 `.sig` 内容，不是路径。GitHub 与镜像使用完全相同的已签名包和版本，URL 可不同。以下仅为说明模板，不代表已发布版本：

```text
# <版本> 更新日志
版本：<SemVer>。发布日期：<YYYY-MM-DD>。

## 更新内容
- <实际交付的新功能、优化及已验证修复>

## 已知 Bug
- <已确认存在且尚未修复的问题；无则写“无”>
```

## CLI 验证

从 `desktop-nova` 执行 `node --test scripts/updater.test.mjs`、`npx tsc --noEmit`；从仓库根执行 `cargo test --manifest-path desktop-nova/src-tauri/Cargo.toml --bin desktop-nova updater_`。公开测试夹具仅是无执行能力的文本和签名，覆盖签名正确、内容篡改与无效签名；不包含私钥。

参考：[Tauri 官方 Updater](https://v2.tauri.app/plugin/updater/)、[锁定版本 Rust API](https://docs.rs/tauri-plugin-updater/2.10.0/tauri_plugin_updater/)。
