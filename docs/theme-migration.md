# 主题迁移说明（LitematicaBA）

本文档用于说明如何将外部项目（如 `RdmPickture`）的主题系统迁移到当前项目。

## 一、适用范围

- 主题 Python 文件：`src/litematicaba/ui/themes/*.py`
- 主题资源文件：`src/litematicaba/ui/resources/theme/<theme_id>/...`
- 主题字体文件（如 Minecraft）：`src/litematicaba/ui/resources/font/<theme_id>/...`

## 二、标准迁移步骤

1. 复制主题文件
   - 将外部项目的主题文件复制到 `src/litematicaba/ui/themes/`
   - 将 import 命名空间从外部包名改为 `litematicaba`

2. 复制主题资源
   - 将外部资源目录复制到 `src/litematicaba/ui/resources/theme/<theme_id>/`
   - 若主题依赖图标，确认 `src/litematicaba/ui/resources/icon/` 目录完整

3. 复制字体资源（如需要）
   - 将字体复制到 `src/litematicaba/ui/resources/font/<theme_id>/`
   - 当前 `ui/theme.py` 已内置 Minecraft 字体加载逻辑（`unifont*.ttf|otf`）

4. 注册主题
   - 编辑 `src/litematicaba/ui/themes/__init__.py`
   - 将主题对象（如 `THEME_METRO10`）加入 `THEMES` 列表

5. 开放主题选项
   - 编辑 `src/litematicaba/core/settings.py`
   - 将对应 `theme_id` 加入 `VALID_THEMES`

6. 快速验证
   - 运行：`python -m compileall src`
   - 启动：`bin/dev-win-qt.bat`
   - 在“选项”页切换主题，观察 UI 测试页控件样式是否正确

## 三、接口约束（必须满足）

### 1) ThemeDef 结构

`src/litematicaba/ui/themes/base.py` 中 `ThemeDef` 约定：

- `theme_id: str`
- `build_qss: Callable[[], str]`
- `widget_support: set[str]`（可选，默认空集合）

如果外部主题构造函数传入了 `widget_support`，本项目 `ThemeDef` 必须保留该字段。

### 2) 资源路径约定

- 主题资源目录通过 `resource_dir(theme_id)` 解析：
  - `src/litematicaba/ui/resources/theme/<theme_id>/`
- 图标目录通过 `icon_dir()` 解析：
  - `src/litematicaba/ui/resources/icon/`

请确保主题代码中的资源文件名与实际文件名完全一致。

### 3) f-string QSS 注意事项

若主题使用 `return f"""..."""` 拼接 QSS：

- 普通 QSS 花括号必须写成 `{{` 和 `}}`
- 仅变量插值位置保留 `{expr}`

否则可能出现运行时错误，如：

- `NameError: name 'background' is not defined`

## 四、常见报错与处理

1. `TypeError: ThemeDef.__init__() got an unexpected keyword argument 'widget_support'`
   - 原因：`ThemeDef` 定义与主题文件不一致
   - 处理：在 `themes/base.py` 增加/恢复 `widget_support` 字段

2. `ImportError: cannot import name 'icon_dir'`
   - 原因：`themes/base.py` 缺少 `icon_dir()`
   - 处理：补充 `icon_dir()`，返回 `ui/resources/icon`

3. `NameError` 出现在主题 QSS 文件中
   - 原因：f-string QSS 花括号未转义
   - 处理：将静态 `{` `}` 改为 `{{` `}}`，或直接采用源项目已验证版本

4. 切换主题后无变化
   - 检查：
     - 是否已在 `themes/__init__.py` 注册
     - 是否在 `VALID_THEMES` 中
     - 是否资源路径/文件名匹配

## 五、建议的更新策略

- 优先“整文件替换”已验证主题（如 `metro10.py`、`minecraft.py`）
- 再执行命名空间替换（仅包名前缀）
- 最后统一做两步验证：
  - `python -m compileall src`
  - 实际启动并切换主题

按上述流程，后续主题设计变更可以通过“迁移主题文件 + 迁移资源文件 + 同步注册点”稳定更新。
