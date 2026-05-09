# 清理策略

本文定义仓库清理规则，目标是避免把本地构建产物、调试输出、下载缓存和临时样例提交到 GitHub。

## 当前主线

- `desktop-nova/` 是当前桌面端主线。
- `tools/viewer-core/` 是 `.litematic` 解析、统计、cache 和原生 viewer 后端。
- `bin/viewer-backend/` 是应用运行时读取的后端目录，里面的 exe 应来自 release build。
- `data/` 保存内置模板、AI prompt、BlockState DB 和资源数据。
- `scripts/` 保存维护脚本和可重复生成数据的脚本。

## 不提交的内容

这些内容属于本地状态或可重新生成内容，不应提交：

- `desktop-nova/node_modules/`
- `desktop-nova/dist/`
- `desktop-nova/src-tauri/target/`
- `tools/viewer-core/target/`
- `.tmp/`
- `.external/`
- `diagnostics/bundle_*`
- `__pycache__/`、`.pytest_cache/`、`.mypy_cache/`、`.ruff_cache/`
- `*.tmp`、`*.temp`
- `_full_verify_*.png`
- `_full_mode_*`
- `*_trace.log`
- `*_debug.log`
- `*_probe.png`
- `*_preview.png`
- AppData 下载物和 RedenMC 下载出来的 `.litematic`
- 本地打包出的 exe，例如 `docs/*.exe`

## 可以删除的本地内容

需要释放磁盘空间时，可以删除：

- Node 依赖目录和前端构建目录。
- Rust `target/`。
- 诊断包目录 `diagnostics/bundle_*`。
- 根目录临时验证图、临时 fixture、probe 输出和 debug log。
- `.external/`，前提是当前任务不需要对照 Nova 原仓库。

## 不要删除或移动

除非任务明确要求，不要删除或移动：

- `desktop-nova/`
- `tools/viewer-core/`
- `data/`
- `docs/`
- `scripts/`
- `bin/viewer-backend/`
- package manifest、Cargo manifest、Tauri 配置。
- AI prompt、配置模板、validator、normalizer 和 apply bridge 源码。
- viewer-core mesh、Full Mode、native viewer、cache 协议源码。

## 测试 fixture

测试需要的 fixture 应放在对应模块的 `tests/fixtures/` 下，不再放在仓库根目录。

当前保留：

- `tools/viewer-core/tests/fixtures/stats_water_fixture.litematic`

## 重新构建

安装前端依赖：

```powershell
cd desktop-nova
npm install
```

构建前端：

```powershell
cd desktop-nova
npm run build
```

检查 Tauri 后端：

```powershell
cd desktop-nova\src-tauri
cargo check
```

检查 viewer-core：

```powershell
cd tools\viewer-core
cargo check
```
