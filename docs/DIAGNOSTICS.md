# Litematica-BA Diagnostics / 诊断包导出

本文件说明如何导出和阅读最小诊断包。诊断包用于把维护者最常需要的上下文集中到一个目录里，避免靠截图和手动复制零散日志排查问题。

This is a low-risk diagnostics workflow. It does not change business behavior, render output, cache protocol, AI prompt/config, or generated data.

---

## Export Command / 导出命令

在项目根目录运行：

```powershell
python scripts\export_diagnostics.py
```

输出目录格式：

```text
diagnostics/bundle_YYYYMMDD_HHMMSS/
```

可以指定输出根目录：

```powershell
python scripts\export_diagnostics.py --output-dir C:\Temp\lba-diagnostics
```

脚本只读取现有上下文，不复制用户 `.litematic` 文件，不复制大型 cache chunk，不修改投影库，不修改 cache，不修改生成物。

---

## Bundle Files / 诊断包文件

### `README.txt`

- 说明诊断包用途。
- 记录生成时间和项目根路径。
- 说明哪些内容可能缺失。
- 明确安全边界：不复制 `.litematic`，不复制大型 cache，不导出 API key/token/cookie。

### `environment.txt`

包含：

- OS / Python / Node / npm / cargo 版本。
- 当前工作目录。
- 项目根目录。
- `bin/viewer-backend/litematica_core.exe` 是否存在。
- `bin/viewer-backend/litematica_native_viewer.exe` 是否存在。

如果某个工具拿不到版本，会写 `unavailable`，不会中断导出。

### `project_paths.json`

列出关键路径和 `exists` 状态：

- project root
- `desktop-js`
- `tools/viewer-core`
- `data`
- `bin/viewer-backend`
- `data/projection-library`
- `.tmp/desktop-js/render`
- `docs`
- `diagnostics`
- assistant index

### `debug_flags.txt`

收集当前环境变量中以 `LBA_` 开头的项。

安全限制：

- 变量名包含 `api_key`、`token`、`authorization`、`cookie`、`secret`、`password` 时会 redacted。
- 没有 `LBA_` 环境变量时写 `none`。

### `projection_library_index_excerpt.json`

导出投影库摘要，不复制完整大型用户数据。

包含：

- 总记录数。
- 最近最多 10 条记录。
- 安全字段：id/name/status、Minecraft data version、totalBlocks、regionCount、路径摘要。

路径只做摘要，包括 basename、parent_name、hash，不复制 `.litematic` 文件内容。

### `recent_cache_manifest.json`

从 `.tmp/desktop-js/render/` 查找最近的 `lba_native_cache*.json`。

导出摘要：

- manifest path
- format / color_chain / chunk_size
- scene size
- chunk_data_dir
- layer_index_file
- chunks_count
- 前 5 个 chunk 条目

不会复制 chunk payload 目录。找不到时写 `not available yet` 和原因。

### `recent_viewer_command.txt`

当前项目没有确认的“最近 viewer 命令持久化记录”。本轮不侵入启动链路，所以通常会写：

```text
not available yet
```

后续建议：在 Tauri bridge 里低风险持久化最近一次 native viewer / viewer-core command args，注意不要记录 API key。

### `recent_ai_plan_trace.json`

当前 AI plan trace 主要在 GeneratePage 运行态里，不确认有持久化 trace。本轮不重构 AI pipeline，所以通常会写：

```json
{
  "status": "not available yet",
  "reason": "..."
}
```

后续建议：在 AI import/apply 前后持久化安全摘要，例如 parse status、warning/error count、operation count，不保存 API key。

### `recent_logs.txt`

收集最近日志尾部摘要：

- `.tmp/desktop-js/render/*.log`
- repo root `*.log`

只写最近若干文件的 tail，不复制完整大型日志。常见敏感字段会 redacted。

---

## What To Check First / 按问题类型查看

### AI plan 错误

优先看：

1. `recent_ai_plan_trace.json`
2. `recent_logs.txt`
3. `debug_flags.txt`
4. `project_paths.json`

如果 `recent_ai_plan_trace.json` 是 `not available yet`，回到 UI 的 AI parse/validate 报错文本，并检查 `desktop-js/src/services/aiProjection.ts`。

### preview 错位或缺失

优先看：

1. `recent_logs.txt`
2. `recent_cache_manifest.json`
3. `environment.txt`
4. `debug_flags.txt`

重点找 `render_preview_image`、`preview-output`、`[NATIVE_VIEWER]`、`[LBA_EMBED_VIEWER]`。

### native viewer 黑屏或崩溃

优先看：

1. `recent_logs.txt`
2. `environment.txt`
3. `debug_flags.txt`
4. `recent_cache_manifest.json`

重点找：

- `[NATIVE_VIEWER] startup`
- `[VIEWER_CACHE]`
- `[PREBUILD_TAIL]`
- `[TRACE_SUMMARY]`
- `[LBA_EMBED_VIEWER]`

### cache 缺失或损坏

优先看：

1. `recent_cache_manifest.json`
2. `project_paths.json`
3. `recent_logs.txt`

如果 cache manifest 缺失、没有 `layer_index_file`、chunk dir 不存在，通常应重新构建 3D cache。

### blockstate 非法或误报 warning

优先看：

1. `project_paths.json`
2. `environment.txt`
3. `recent_logs.txt`

然后检查：

- `data/minecraft_blockstates/26.1.json`
- `data/minecraft_blockstates/26.1.zh_cn.json`
- `data/minecraft_blockstates/overrides/26.1.json`
- `scripts/generate_minecraft_blockstate_db.py`
- `scripts/generate_minecraft_blockstate_i18n.py`

生成物不要只手工改；应改 override/generator 后重新生成。

### 投影库索引异常

优先看：

1. `projection_library_index_excerpt.json`
2. `project_paths.json`
3. `environment.txt`

重点确认：

- `data/projection-library/js_library.json` 是否存在。
- records 总数是否异常。
- 最近记录 status 是否为 `parse_error` 或 `missing`。
- path 摘要是否指向预期文件。

---

## Security Notes / 安全边界

- 不复制用户 `.litematic`。
- 不复制大型 cache chunk。
- 不导出 API key、token、Authorization header、Cookie、password、secret。
- 对路径只在投影库摘要里导出 basename/parent/hash。
- 拿不到的信息写 `not available yet`，不为了诊断包侵入主流程。

---

## Related Docs / 相关文档

- `docs/PROJECT_MAP.md`
- `docs/FEATURE_ENTRYPOINTS.md`
- `docs/DATA_FLOW.md`
- `docs/MODULE_BOUNDARIES.md`

