# 设计说明

当前设计以 `desktop-nova` 为主线。

## 产品定位

Litematica-BA 是面向 Minecraft `.litematic` 投影文件的本地管理、分析、生成和预览工具。

核心目标：

- 不启动游戏即可管理投影文件。
- 快速查看 metadata、区域、统计和材料。
- 构建 3D cache，用于渲染和分层查看。
- 支持方块替换和简单几何投影生成。
- 支持 AI 辅助生成 plan，但不自动执行写文件操作。
- 接入 RedenMC 在线投影库。

## 主线 UI

主线 UI 是：

```text
desktop-nova/
```

页面：

- 首页
- 投影库
- 属性
- 统计
- 分层
- 渲染
- 替换
- 生成
- 选项

旧 JS 桌面 UI 不再作为功能规格引用。

## 后端

后端由 `tools/viewer-core` 提供，运行时 exe 位于：

```text
bin/viewer-backend/
```

主要命令：

- `analyze`
- `stats`
- `materials`
- `replace-blocks`
- `generate`
- `cache-layer-meta`
- `cache-layer`

## 数据

用户态数据默认写入：

```text
%AppData%\Litematica-BA\desktop-nova
```

项目内资源保留在：

- `data/minecraft_blockstates/`
- `data/generation-templates/`
- `data/ai-projection/`
- `block/`
- `item/`

## 视觉

Nova 视觉参考来自：

```text
.external/Litematica-Nova-rpr-ba
```

该目录不上传。主题、颜色、字体、控件质感应来自 Nova 原仓库。

