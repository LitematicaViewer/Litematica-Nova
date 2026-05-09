# 主题迁移说明

当前 UI 主线是 `desktop-nova`。

Nova 视觉来源：

```text
.external/Litematica-Nova-rpr-ba
```

该目录只作为本地视觉参考，不上传。

## 迁移规则

- 页面结构、功能流程、按钮位置以 `desktop-nova` 当前实现为准。
- 视觉 token、主题名、颜色、字体、圆角、阴影、控件质感应来自 Nova 原仓库。
- 不再迁移旧 PySide6 主题。
- 不再使用历史桌面入口作为主题来源。
- 不自创“看起来像 Nova”的假主题。

## 修改主题后检查

```powershell
cd desktop-nova
npm run build

cd src-tauri
cargo check
```

需要人工检查：

- 主题下拉是否显示真实 Nova 主题。
- 切换主题是否即时生效。
- 重启后是否保持上次主题。
- 页面是否仍符合 Nova 视觉，而不是旧 inline 样式。

