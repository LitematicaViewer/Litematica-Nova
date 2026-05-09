# 模块边界

本文说明哪些层负责什么，以及修改后应做什么验证。

## UI 层

路径：

- `desktop-nova/src/routes/`
- `desktop-nova/src/components/`
- `desktop-nova/src/styles/`

职责：

- 页面结构。
- 表单和按钮交互。
- 弹窗。
- 展示后端返回结果。

不应做：

- 解析二进制 NBT。
- 伪造后端数据。
- 修改 viewer 渲染语义。

验证：

```powershell
cd desktop-nova
npm run build
```

## 前端 service 层

路径：

- `desktop-nova/src/services/`

职责：

- 封装 Tauri 调用。
- 封装 AppData 文件读写。
- 封装 RedenMC、AI、投影库、统计、生成等业务调用。

不应做：

- 绕过 adapter 到处散落命令参数。
- 把 API Key 写入 localStorage 或日志。

验证：

- `npm run build`
- 对应功能的 CLI 或 UI 手测。

## Tauri 层

路径：

- `desktop-nova/src-tauri/src/main.rs`

职责：

- 文件系统访问。
- AppData 路径。
- 后端进程启动。
- native viewer 嵌入和隐藏。
- AI Key 安全保存和调用。

不应做：

- 改 viewer-core 渲染逻辑。
- 打印 API Key。
- 静默覆盖用户选择的输出文件。

验证：

```powershell
cd desktop-nova\src-tauri
cargo check
```

## viewer-core 层

路径：

- `tools/viewer-core/src/`

职责：

- `.litematic` 解析。
- analyze/stats/materials。
- replace/generate。
- cache 构建和读取。
- native viewer。

高风险文件：

- `native_viewer.rs`
- `full_mode_v2.rs`
- `mesh.rs`
- `cache_layer.rs`
- `nbt.rs`

验证：

```powershell
cd tools\viewer-core
cargo fmt
cargo check
cargo test
```

如果构建了新 exe，要复制到：

```text
bin/viewer-backend/
```

## 数据层

路径：

- `data/`
- `%AppData%\Litematica-BA\desktop-nova`

规则：

- 内置模板、prompt、BlockState DB 属于项目资源。
- 用户配置、投影库、下载缓存、预览和 render cache 属于 AppData。
- API Key 不进入普通 config、日志、prompt、localStorage。

## 文档层

路径：

- `README.md`
- `MAINTAINERS.md`
- `docs/`

规则：

- 文档使用中文。
- 不再引用旧 JS 桌面 UI 作为主线或功能规格。
- 不把本地私有路径、API Key、下载物内容写入文档。

