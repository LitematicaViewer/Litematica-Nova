# 诊断说明

本文说明如何导出和阅读诊断信息。

## 导出诊断包

```powershell
cd C:\Users\27232\Documents\Litematica-BA
python scripts\export_diagnostics.py
```

诊断包输出到：

```text
diagnostics/bundle_<时间戳>/
```

诊断脚本只读取已有文件和环境信息，不复制用户 `.litematic` 文件，不复制大型 cache payload，并会遮蔽常见密钥字段。

## 常见日志入口

### 嵌入式 viewer

看 Tauri 控制台里的：

```text
[LBA_EMBED_VIEWER]
```

重点字段：

- `parent_hwnd`
- `child_hwnd`
- `viewer_pid`
- `visible`
- `bounds`
- `purpose`
- `display_mode`
- `command_args`
- `stdout_tail`
- `stderr_tail`

切页问题优先看是否有：

```text
route_active=<route> action=hide/show reason=route_change
```

### 3D cache

看：

- progress JSON
- cache manifest
- native stdout/stderr tail
- UI 中的 cache path

常见问题：

- progress 一直 0：看 native 进程是否仍在运行，stdout/stderr 是否卡在某个阶段。
- cache ready 但 viewer 不显示：确认传入的是 cache manifest，不是 progress/preview/layer meta。
- cache 旧版本：应判定不兼容并要求重建。

### 材料统计

命令：

```powershell
bin\viewer-backend\litematica_core.exe materials --input "<file.litematic>" --scope all --include-container-items --json
```

重点字段：

- `block_count`
- `container_item_count`
- `total_count`
- `container_scan.enabled`
- `container_scan.containers_scanned`
- `container_scan.item_stacks_scanned`
- `container_scan.warnings`

### RedenMC

先确认接口返回 JSON，再确认下载内容类型。

如果下载结果是 HTML 或外部网盘页面，必须按真实错误处理，不允许导入为 `.litematic`。

API 文档见：

```text
docs/reden_litematica_api.md
```

### AI

检查顺序：

1. prompt 是否构建正确。
2. API Key 是否只在后端安全存储。
3. 返回文本是否能提取 plan JSON。
4. plan 是否通过前端结构校验。
5. dry-run 是否通过后端校验。

AI plan 导入只填表，不自动 dry-run/apply。

## 不应进入诊断包的内容

- API Key。
- Cookie。
- Authorization header。
- 用户 `.litematic` 文件本体。
- 大型 cache chunk payload。
- AppData 下载物本体。

