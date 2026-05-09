# 清理策略

本文定义低风险磁盘清理规则。它只处理生成物、缓存和可恢复隔离，不是重构计划。

## 当前 UI 状态

- `desktop-nova/` 是当前桌面 UI 主线和功能基准，清理时不得删除。
- 旧 JS 桌面 UI 已从 Git 跟踪中排除，只保留本地参考，不作为维护入口。
- `src/litematicaba/`、`desktop-ui/`、`script/` 已删除。
- `scripts/` 是当前维护脚本目录。

## 可以删除的内容

这些内容是缓存或构建产物，可在需要释放空间时删除：

- `desktop-nova/node_modules/`
- `desktop-nova/dist/`
- `desktop-nova/src-tauri/target/`
- `tools/viewer-core/target/`
- `.tmp/`
- `.external/`，前提是当前任务不需要 Nova 参考仓库。
- `diagnostics/bundle_*`
- `__pycache__/`、`.pytest_cache/`、`.mypy_cache/`、`.ruff_cache/`、`.cache/`
- `.vite/`、`.turbo/`、`.next/`、`coverage/`、`build/`
- `*.tmp`、`*.temp`
- 本地验证图，例如 `_full_verify_*.png`
- `docs/` 下本地打包出的 exe

保留 `diagnostics/README.md`；只删除生成出的 `bundle_*` 目录。

## 不要删除或移动

清理时不要删除或移动：

- `desktop-nova/`
- `tools/viewer-core/`
- `data/projection-library/`
- `data/ai-projection/`
- `data/minecraft_blockstates/overrides/`
- 当前 BlockState DB 文件，除非任务明确要求重新生成 DB。
- `bin/viewer-backend/`
- `docs/`
- `scripts/`
- `MAINTAINERS.md`
- `README.md`
- package manifest、Cargo manifest、Tauri 配置。
- AI prompt、config、validator、normalizer、apply bridge 源码。
- viewer-core mesh、Full Mode、native viewer、cache 协议源码。

## 只隔离，不直接删

如果某个文件像调试产物或 fixture，但可能仍有回归价值，先移动到仓库外隔离区，不要直接删除。

隔离目录：

```text
C:\Users\27232\CodexBackups\<cleanup_backup>\quarantine\
```

建议隔离对象：

- 根目录 `_full_mode_*`
- 根目录 `_verify_*`
- 根目录包含 `debug`、`trace`、`probe`、`fixture`、`baseline`、`repro`、`test` 的零散产物。
- 根目录 `.png`、`.log`、零散 `.litematic` 样例。
- 无法确认用途的截图、JSON、layout 文件。

隔离前应记录原始路径和判断依据到 `cleanup_manifest.json`。

## 从隔离区恢复

恢复单个文件：

```powershell
Copy-Item "C:\Users\27232\CodexBackups\<cleanup_backup>\quarantine\_verify_example.png" "C:\Users\27232\Documents\Litematica-BA\_verify_example.png"
```

恢复全部文件时，把隔离目录内容按原相对路径复制回仓库根目录。

## 重新安装和构建

删除 `node_modules` 后：

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-nova
npm install
```

重新构建前端：

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-nova
npm run build
```

重新检查 Tauri 和 Rust：

```powershell
cd C:\Users\27232\Documents\Litematica-BA\desktop-nova\src-tauri
cargo check

cd C:\Users\27232\Documents\Litematica-BA\tools\viewer-core
cargo build --release --bin litematica_core
cargo build --release --bin litematica_native_viewer
```

## 重新导出诊断

```powershell
cd C:\Users\27232\Documents\Litematica-BA
python scripts\export_diagnostics.py
```

## 必需清理记录

任何破坏性清理都应创建：

- `cleanup_manifest.json`
- `cleanup_report.md`
- 仓库外备份目录：`C:\Users\27232\CodexBackups\`
- 备份内的 `BACKUP_MANIFEST.json`、备份日志、`cleanup_plan_before.json`

不要把备份或隔离目录放进仓库。

