# 旧主题尺寸与字号保真规格

> 用途：把合并时保留的三套旧主题（`bootstrap5` / `metro10` / `minecraft`）融入新架构（Tailwind 4 + shadcn）时的**尺寸与字号基准**。
> 原则：三套主题的控件高度、内边距、圆角、字号均来自各自上游的原始确定值，融入时**不得重新臆造**，否则会失去原有的视觉风味。
> 取值来源：`git show refs/merge-safety-pre-resolution:desktop-nova/ui/themes/<theme>/widget/*.css`（合并前的旧架构实现）。

## 上游基准

| 主题 | 上游参照 | 判据 |
| --- | --- | --- |
| `bootstrap5` | Bootstrap 5 官方默认值 | 尺寸用 `rem`、1rem 基准 16px；按钮 `padding: 6px 12px` + `line-height: 24px` 得 38px；圆角 `.375rem`；主色 `#0d6efd` |
| `metro10` | Windows 10 (Metro/Modern) 度量 | 按钮 31px（含 47×31 窗口关闭键）、输入 32px、直角（`border-radius: 0`）、强调色 `#0078d7`、字体 `Microsoft YaHei` 10.5pt |
| `minecraft` | Minecraft Java 版 GUI 度量 | 控件 44px、导航按钮 48px、9-slice 边框 4px、`image-rendering: pixelated`、Unifont 16px |

## 一、bootstrap5（Bootstrap 5 原始值）

### 控件高度与内边距

| 变量 | 值 | 备注 |
| --- | --- | --- |
| `--ln-bootstrap5-button-height` | `38px` | 6px 上下 padding + 24px line-height + 2px 边框 |
| `--ln-bootstrap5-input-height` | `38px` | 同上，与按钮同高 |
| `--ln-bootstrap5-input-padding-y` | `0.375rem` | = 6px |
| `--ln-bootstrap5-input-padding-x` | `0.75rem` | = 12px |
| `--ln-bootstrap5-input-border-radius` | `0.375rem` | = 6px |
| `--ln-bootstrap5-input-font-size` | `1rem` | = 16px |
| `--ln-bootstrap5-input-font-weight` | `400` | |
| `--ln-bootstrap5-input-line-height` | `1.5` | = 24px |
| `--ln-bootstrap5-range-height` | `1.5rem` | = 24px |
| `--ln-bootstrap5-range-track-height` | `0.5rem` | = 8px |
| `--ln-bootstrap5-table-row-min-height` | `2.25rem` | = 36px |
| `--ln-bootstrap5-table-cell-padding-y` | `0.5rem` | = 8px |
| `--ln-bootstrap5-table-cell-padding-x` | `0.5rem` | = 8px |
| `--ln-bootstrap5-table-cell-padding-y-sm` | `0.25rem` | = 4px |
| `--ln-bootstrap5-table-cell-padding-x-sm` | `0.25rem` | = 4px |
| `--ln-check-size` | `1em` | 跟随字号 |
| `--ln-check-label-min-height` | `1.5rem` | = 24px |
| `--ln-bootstrap5-check-border-radius` | `0.25em` | |
| `--ln-bootstrap5-check-radio-border-radius` | `50%` | |

### 字号与颜色

| 变量 | 值 |
| --- | --- |
| `--ln-body-font-size` | `16px` |
| `--ln-body-text-color` | `#000000` |
| `--ln-bootstrap5-input-color` | `#212529` |
| `--ln-bootstrap5-input-background` | `#ffffff` |
| `--ln-bootstrap5-input-border-color` | `#dee2e6` |
| `--ln-bootstrap5-input-focus-border-color` | `#86b7fe` |
| `--ln-bootstrap5-input-focus-shadow` | `0 0 0 0.25rem rgb(13 110 253 / 25%)` |
| `--ln-bootstrap5-input-placeholder-color` | `rgb(33 37 41 / 75%)` |
| `--ln-bootstrap5-input-disabled-background` | `#e9ecef` |
| `--ln-bootstrap5-input-disabled-color` | `rgb(33 37 41 / 75%)` |
| `--ln-bootstrap5-button-color` | `#0d6efd` |
| `--ln-bootstrap5-button-active-color` | `#ffffff` |
| `--ln-bootstrap5-table-color` | `#212529` |
| `--ln-bootstrap5-table-border-color` | `#dee2e6` |
| `--ln-bootstrap5-table-caption-color` | `#6c757d` |

### 布局

| 项 | 值 |
| --- | --- |
| 侧边栏展开宽度 | `220px` |
| 侧边栏折叠宽度 | `48px` |
| 导航项最小高度 | `48px`，左右内边距 `16px` |
| 导航图标 | `16px × 16px` |
| 文件图标 `--ln-file-icon-size` | `24px` |
| 主题背景 `--ln-ui-background-color` | `#ffffff` |
| 导航背景 `--ln-navigation-background-color` | `#722ef9` |
| 响应式断点 | `@media (max-width: 760px)` 折叠 |

## 二、metro10（Windows 10 原始度量）

### 控件尺寸

| 变量 | 值 | 备注 |
| --- | --- | --- |
| `--ln-metro10-button-height` | `31px` | Win10 标准按钮高度 |
| `--ln-metro10-window-close-width` | `47px` | Win10 标题栏关闭键 |
| `--ln-metro10-window-close-height` | `31px` | 与标题栏同高 |
| `--ln-metro10-input-height` | `32px` | Win10 文本框高度 |
| `--ln-metro10-input-padding-y` | `3px` | |
| `--ln-metro10-input-padding-x` | `8px` | |
| `--ln-metro10-range-height` | `24px` | |
| `--ln-metro10-range-track-height` | `2px` | 细轨道 |
| `--ln-metro10-range-thumb-width` | `8px` | 细条状滑块 |
| `--ln-metro10-range-thumb-height` | `24px` | |
| `--ln-metro10-table-row-min-height` | `60px` | |
| `--ln-check-size` | `20px` | |
| `--ln-check-label-min-height` | `24px` | |
| 按钮左右内边距 | `0 12px` | |
| 输入行高 | `1.4` | |
| 所有圆角 | `0` | Metro 直角语言（按钮/输入/对话框/子窗口） |
| 输入边框宽度 | `2px` | |

### 字号与颜色

| 变量 | 值 |
| --- | --- |
| `--ln-body-font-family` | `"Microsoft YaHei", "微软雅黑", sans-serif` |
| `--ln-body-font-size` | `10.5pt`（= 14px @96dpi，Windows 10 中文字体基准） |
| `--ln-body-text-color` | `#000000` |
| `--ln-muted-text-color` | `#666666` |
| `--ln-metro10-button-background` | `#cccccc` |
| `--ln-metro10-button-background-active` | `#999999` |
| `--ln-metro10-button-border-hover` | `#7a7a7a` |
| `--ln-metro10-button-text-disabled` | `#7a7a7a` |
| `--ln-metro10-input-background` | `#ffffff` |
| `--ln-metro10-input-border` | `#999999` |
| `--ln-metro10-input-border-hover` | `#666666` |
| `--ln-metro10-input-border-focus` | `#0078d7`（Win10 强调色） |
| `--ln-metro10-input-disabled-color` | `#7a7a7a` |
| `--ln-metro10-input-disabled-background` | `#f2f2f2` |
| `--ln-metro10-table-hover-background` | `#e6e6e6` |
| `--ln-table-row-selected-background` | `#f2f2f2` |

### 布局

| 项 | 值 |
| --- | --- |
| 侧边栏展开宽度 | `220px` |
| 侧边栏折叠宽度 | `48px` |
| 导航项最小/最大高度 | `48px` |
| 导航图标 | `16px × 16px` |
| 导航选中指示条 | `4px × 24px` |
| 文件图标 `--ln-file-icon-size` | `32px` |
| 主题背景 `--ln-ui-background-color` | `#ffffff` |
| 导航背景 `--ln-navigation-background-color` | `#f7f7f7` |

## 三、minecraft（Minecraft Java 版原始度量）

### 控件尺寸

| 变量 | 值 | 备注 |
| --- | --- | --- |
| `--ln-minecraft-button-height` | `44px` | 与游戏内按钮比例一致 |
| `--ln-control-height` | `44px` | |
| `--ln-minecraft-input-height` | `44px` | |
| `--ln-minecraft-input-padding-y` | `4px` | |
| `--ln-minecraft-input-padding-x` | `8px` | |
| `--ln-minecraft-button-border` | `4px` | 9-slice 边框宽度 |
| `--ln-minecraft-nav-button-height` | `48px` | |
| `--ln-minecraft-sidebar-collapsed-width` | `49px` | 原始值非整数，勿"修正"为 48 |
| `--ln-minecraft-range-height` | `44px` | |
| `--ln-minecraft-range-thumb-width` | `8px` | |
| `--ln-minecraft-table-row-min-height` | `44px` | |
| `--ln-check-size` | `20px` | |
| `--ln-check-label-min-height` | `24px` | |
| 所有圆角 | `0` | 像素风格无圆角 |
| 边框宽度 | `2px`（对话框/子窗口/卡片） | |
| `image-rendering` | `pixelated` | 复选框、输入、弹窗、滑块、子窗口 |

### 字号与颜色

| 变量 | 值 |
| --- | --- |
| `--ln-body-font-family` | `"Unifont", monospace` |
| `--ln-body-font-size` | `16px` |
| `--ln-body-text-color` | `#ffffff` |
| `--ln-muted-text-color` | `#666666` |
| `--ln-minecraft-input-background` | `#000000` |
| `--ln-minecraft-input-color` | `#ffffff` |
| `--ln-minecraft-input-border` | `#a0a0a0` |
| `--ln-minecraft-input-border-focus` | `#ffffff` |
| `--ln-minecraft-input-disabled-color` | `#808080` |
| `--ln-minecraft-table-row-background-odd` | `#000000` |
| `--ln-minecraft-table-row-background-even` | `#1a1a1a` |
| `--ln-minecraft-table-row-background-hover` | `#2b2b2b` |
| `--ln-table-row-selected-background` | `#4d4d4d` |
| 弹窗字号 | `16px` |

### 字体资源（许可见 `docs/THIRD_PARTY_NOTICES.txt`）

| 文件 | 许可 |
| --- | --- |
| `ui/themes/minecraft/resource/font/unifont-16.0.04.ttf` | SIL OFL 1.1（双许可择一） |
| `ui/themes/minecraft/resource/font/UnifontExMono.woff2` | 内嵌 OFL 1.1，保留字体名 `UnifontExMono` |

### 布局与资源

| 项 | 值 |
| --- | --- |
| 侧边栏展开宽度 | `220px` |
| 侧边栏折叠宽度 | `49px`（见上） |
| 导航项左右内边距 | `0 16px` |
| 文件图标 `--ln-file-icon-size` | `32px` |
| 主题背景 `--ln-ui-background-color` | `#3c3c3c` |
| 导航背景 `--ln-navigation-background-color` | `#3c3c3c` |
| Flake 叠加图 | `hotbar` / `hotbar_selection` / `creativeInventory` / `chest27` / `hopper` / `editTool_{normal,hover,active}` / `openInventory_{normal,hover}` |

## 四、三主题对照速查

| 项 | bootstrap5 | metro10 | minecraft |
| --- | --- | --- | --- |
| 按钮高度 | 38px | 31px | 44px |
| 输入高度 | 38px | 32px | 44px |
| 正文字号 | 16px | 10.5pt | 16px |
| 正文字族 | 系统默认 | Microsoft YaHei | Unifont |
| 圆角 | 6px / .375rem | 0 | 0 |
| 复选框 | 1em | 20px | 20px |
| 表格行高 | 2.25rem | 60px | 44px |
| 侧栏折叠宽 | 48px | 48px | 49px |
| 文件图标 | 24px | 32px | 32px |
| 强调色 | `#0d6efd` | `#0078d7` | 无（9-slice 贴图） |

## 五、融入 Tailwind 4 的约束

1. **不要用 Tailwind 的间距刻度替换原始像素值**。原值来自上游规格，`p-1.5` 之类的近似会造成 1–2px 偏差并在控件对齐上累积。
2. **每套主题的控件高度必须成组保留**：按钮高度与输入高度在 bootstrap5 与 minecraft 中是相同的（38/38、44/44），metro10 则不同（31/32），融入时不要强制统一。
3. **圆角与直角是主题识别特征**：metro10 与 minecraft 的 `border-radius: 0` 必须保留；bootstrap5 的 `6px`/`.375rem` 亦然。
4. **像素风主题需要 `image-rendering: pixelated`** 与 9-slice `border-image`，这两项在 Tailwind 工具类里没有等价物，需保留手写 CSS 图层。
5. **metro10 的 `10.5pt` 不要换算成整数 px**：`10.5pt` 在高 DPI 下与 `14px` 不等价，Windows 度量要求保留 `pt` 单位。
6. **minecraft 的 `49px` 折叠宽度是原始值**，不要"取整"成 48px 以与其它主题对齐。
