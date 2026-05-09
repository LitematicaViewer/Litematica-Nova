# 数据流

本文说明主要数据从哪里来、写到哪里、哪些是生成物。

## 用户态目录

默认用户态目录：

```text
%AppData%\Litematica-BA\desktop-nova
```

当前应写入这里的数据：

- 投影库：`projection-library/js_library.json`
- RedenMC 下载：`reden/downloads/`
- 预览图：`previews/`
- 渲染 cache：`render/`
- 自定义生成模板：`generation-templates/custom/`
- 普通配置和 AI 普通配置

API Key 不得进入 localStorage、普通配置、日志或 prompt。

## 本地投影导入

流程：

1. UI 选择 `.litematic`。
2. `desktop-nova` 设置 currentFile。
3. 调用 `litematica_core.exe analyze`。
4. 写入 AppData 投影库。
5. UI 根据分析结果刷新属性、统计、渲染入口。

## RedenMC 在线投影

流程：

1. 搜索接口返回列表。
2. 详情接口返回附件和参数化限制。
3. 普通附件或参数化附件下载到 AppData。
4. 检查文件是否为 `.litematic`。
5. 下载成功后 analyze。
6. 加入投影库并设置 currentFile。

外部网盘或 HTML 响应必须按真实错误处理，不得假装成功。

## 统计和材料

流程：

1. UI 调用 `litematica_core.exe stats` 或 `materials`。
2. 不勾选容器物品时只统计方块本体。
3. 勾选容器物品时增加 `--include-container-items`。
4. 后端输出：
   - `block_count`
   - `container_item_count`
   - `total_count`
   - `container_scan`
5. UI 以 `total_count` 展示和导出。

CSV 导出：

- 由用户选择路径。
- UTF-8 BOM。
- 三列：`名称,数字,统计数据`。
- 单位：`箱盒 / 盒 / 组 / 个`。

## 3D cache

流程：

1. 渲染页启动 cache 构建。
2. Tauri 启动 native viewer prebuild。
3. native viewer 写 progress JSON 和 cache manifest。
4. UI 轮询 progress。
5. ready 后可生成静态预览、启动嵌入 viewer 或弹窗 viewer。

cache 文件是生成物，不要手改。

## 嵌入式 viewer

流程：

1. 渲染页有可见容器时计算物理像素 rect。
2. Tauri 创建 Windows child host。
3. native viewer 使用 `--embed-parent-hwnd` 启动。
4. resize/scroll 时更新 bounds。
5. 切到非渲染页时 hide，不 stop。

调试看 `[LBA_EMBED_VIEWER]` 日志。

## 生成和替换

生成：

1. UI 生成 plan。
2. dry-run 调用 `litematica_core.exe generate --dry-run`。
3. apply 调用 `generate --output`。
4. 输出文件 analyze 后入库。

替换：

1. UI 生成 rules。
2. dry-run 调用 `replace-blocks --dry-run`。
3. apply 输出新 `.litematic`。

## 生成物

可删除后重建：

- `desktop-nova/dist/`
- `desktop-nova/node_modules/`
- `desktop-nova/src-tauri/target/`
- `tools/viewer-core/target/`
- `.tmp/`
- `diagnostics/bundle_*`
- 本地验证 PNG 和日志

