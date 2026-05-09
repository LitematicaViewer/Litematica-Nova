# Litematica-BA

## 当前主线

- `desktop-nova/`：当前桌面端主线，React + Tauri。
- `tools/viewer-core/`：当前 Rust 后端、`.litematic` 解析、cache、原生 viewer。
- `bin/viewer-backend/`：应用运行时使用的后端可执行文件。
- `data/`：内置模板、AI prompt、BlockState DB、资源数据。
- `scripts/`：维护脚本、诊断导出、索引生成、数据生成。

## 已移除的旧入口

- `desktop-ui/` 已删除。
- `script/` 已删除。
- `src/litematicaba/` 和旧 PySide6 UI 已删除。
- `python -m litematicaba` 不再是支持入口。

## desktop-nova 状态

`desktop-nova` 是新的主线。后续功能、页面结构、交互流程都以 `desktop-nova` 为准。

约束：

- 不使用假后端数据。
- 不修改 viewer-core 渲染语义来解决 UI 问题。
- 普通用户态数据默认写入 `%AppData%\Litematica-BA\desktop-nova`。
- API Key 不写入 localStorage、普通配置、日志或 prompt。
- 历史桌面入口不再上传，也不再作为功能规格引用。

当前已接入的主要能力：

- 本地 `.litematic` 选择与 analyze。
- AppData 投影库。
- RedenMC 在线投影库搜索、详情、下载、入库。
- 属性页 metadata 基础编辑、保存、另存、恢复。
- 统计页和材料列表，支持可选统计容器内物品。
- 材料 CSV 导出，UTF-8 BOM，列为 `名称,数字,统计数据`。
- 渲染页 3D cache 构建、进度轮询、静态预览、嵌入 viewer、弹窗 viewer。
- 分层页读取真实 cache。
- 替换页 dry-run/apply。
- 生成页模板、AI plan、API 对话、dry-run/apply。
- 选项页主题、AppData 配置、后端检查、AI 设置。

未完全完成的内容见 `docs/DESKTOP_NOVA_STATUS.md`。

## 维护文档

建议阅读顺序：

1. `MAINTAINERS.md`
2. `docs/DESKTOP_NOVA_STATUS.md`
3. `docs/PROJECT_MAP.md`
4. `docs/FEATURE_ENTRYPOINTS.md`
5. `docs/DATA_FLOW.md`
6. `docs/MODULE_BOUNDARIES.md`
7. `docs/DIAGNOSTICS.md`
8. `docs/CLEANUP_POLICY.md`

## 常用命令

生成维护索引：

```powershell
python scripts\build_assistant_index.py
```

导出诊断包：

```powershell
python scripts\export_diagnostics.py
```

构建 desktop-nova：

```powershell
cd desktop-nova
npm install
npm run build
```

检查 desktop-nova Tauri 后端：

```powershell
cd desktop-nova\src-tauri
cargo check
```

检查 viewer-core：

```powershell
cd tools\viewer-core
cargo check
```

直接启动原生 viewer：

```powershell
bin\viewer-backend\litematica_native_viewer.exe "<path-to-file.litematic>" --display-mode=full
```

## 清理规则

构建产物和缓存可以重新生成。删除或移动文件前先看 `docs/CLEANUP_POLICY.md`。

不要删除或移动：

- `desktop-nova/`
- `tools/viewer-core/`
- `bin/viewer-backend/`
- `data/`
- `docs/`
- `scripts/`

