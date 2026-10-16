# Nova UI 架构：主题化组件系统

| 文档版本 | 修改日期   | 修改人 | 修改说明 |
| -------- | ---------- | ------ | -------- |
| V1.0     | 2026-10-13 | Codex  | 现状基线与目标架构：分层模型、组件契约、令牌契约、主题契约、迁移路径、回归验证 |

本文件规定 Nova 前端的样式分层、组件契约与主题契约。组件来源与许可证见 [THIRD_PARTY_UI.md](THIRD_PARTY_UI.md)；开发与构建流程见 [DEVELOPMENT.md](DEVELOPMENT.md)；界面功能需求见 [design.md](design.md) 的 2.0 节。

**目标**：三主题（shadcn / 8bitcn / Minecraft）共享同一棵组件树与同一套操作逻辑，切换主题只改外观，不改结构、不改交互、不复制页面。任何控件在任意主题下都只有一种外观来源。

---

## 1. 现状基线

本节全部数据来自对当前工作区（`desktop-nova/`）的实测统计，不是估计值。

### 1.1 样式技术栈：三层叠加

| 层 | 内容 | 载体 |
| -- | ---- | ---- |
| ① 组件库层 | Tailwind CSS v4 + shadcn/ui（CVA + Radix） | `ui/components/ui/*.tsx` |
| ② 手写共享层 | 原生 CSS，BEM 风格类名 + CSS 变量 | `ui/styles/`、`ui/shell/widget/` |
| ③ 主题覆写层 | 三主题各自的令牌与组件覆写 | `ui/themes/*/` |

体量：**48 个 CSS 文件 / 7158 行**，**53 个 TSX 文件 / 12741 行**。

### 1.2 组件库清单

| 类别 | 内容 | 数量 |
| ---- | ---- | ---- |
| 原语 | Radix UI（`radix-ui@1.4.3`，headless） | 已安装 60 个 `@radix-ui/*` 子包，实际 import **9** 个：`Dialog` `Select` `Checkbox` `RadioGroup` `Slider` `ScrollArea` `Popover` `ContextMenu` `Slot` |
| 组件 | shadcn/ui 源码自有副本（source-owned，非 npm 依赖） | `ui/components/ui/` 下 **10** 个：`button` `checkbox` `dialog` `input` `radio-group` `scroll-area` `select` `slider` `table` `textarea` |
| 主题变体 | 8bitcn/ui 适配 | 折进 `button.tsx` 的 `ButtonDecorations`、`checkbox.tsx` 的 `nova-checkbox-pixel-edge` |
| 基础设施 | CVA、`clsx` + `tailwind-merge`（`cn()`）、`lucide-react`、`tailwindcss@4` + `@tailwindcss/vite` | — |
| 组合层 | Nova 自研，不在任何 registry 中 | `Dialog` `Dropdown` `BlockSearchSelect` `BlockPickerDialog` `BlockIcon` `VirtualSpacerCell` `useVirtualWindow` |

**组合层现状不齐**：`Dialog.tsx` 正确包 `ui/dialog.tsx`、`Dropdown.tsx` 正确包 `ui/select.tsx`；而 `BlockSearchSelect.tsx` 用 Radix `Popover` 自建列表、`BlockPickerDialog.tsx` 自建对话框选择器并含裸 `<select>`。

### 1.3 Tailwind 工具类的实际消费面

对 53 个 TSX 文件做全量扫描，含 Tailwind 工具类的行共 **36 行**，分布如下：

| 文件 | 行数 |
| ---- | ---- |
| `ui/components/ui/button.tsx` | 14 |
| `ui/components/ui/select.tsx` | 13 |
| `ui/components/ui/dialog.tsx` | 4 |
| `ui/components/ui/table.tsx` | 4 |
| `ui/components/ui/input.tsx` | 1 |

`ui/windows/**`（全部页面）、`ui/shell/AppShell.tsx`、`ui/components/Dropdown.tsx`、`ui/components/Dialog.tsx` 中 Tailwind 工具类为 **0**。`cn()`（`lib/utils`）也只被 `ui/components/ui/` 下的 10 个文件引用。

### 1.4 层叠现状：项目 CSS 全部位于无层区

构建产物（`dist/assets/`）实测。文件名哈希每次构建都会变，故按角色归类：

| 产物 | 大小 | `@layer` 出现次数 |
| ---- | ---- | ----------------- |
| `base-*.css`（base.css + shell/widget + widgets + controls） | 42.8 KB | **0** |
| `main-*.css` 与各页面分块（`asset-manager` `enumerator` `libraryEvents` `StatisticsPage`） | 100.6 KB | **0** |
| `theme-*.css` 中的 Minecraft（不加载 Tailwind） | 41.7 KB | **0** |
| `theme-*.css` 中的 shadcn 与 8bitcn（各自加载 Tailwind） | 29.2 / 28.2 KB | 3（含 1 个 `@layer utilities`） |

即**项目自己的约 183 KB CSS 产物全部位于无层区**，没有任何一条规则受层序保护。

按 CSS 层叠规则，**无层样式永远优于任何 `@layer` 内的样式**，与选择器和源码顺序无关。因此 `ui/components/ui/` 中 36 行 Tailwind 工具类被项目自身的无层规则系统性压制，项目只能靠在 `controls.css`、`themes/modern.css` 中用 `[data-slot]` / `[data-variant]` 属性选择器**重新声明一遍**才能让颜色生效。

页面 CSS 中大量出现的 `border-image: none; box-shadow: none; transform: none`（例如 `library.css:31,56,365`）是同一原因造成的逐个补丁。

### 1.5 组件契约现状

组件已通过 `data-*` 属性暴露语义，这是现有代码中**最有价值、应当保留并加强**的部分。

- TSX 中**发出** 48 个不同 `data-slot` 值。
- CSS 中**消费** 36 个（134 处引用，分布在 7 个文件）：`button`(20) `radio-group-item`(22) `checkbox`(19) `scroll-area-scrollbar`(13) `scroll-area-viewport`(8) `radio-group`(3) `table-row`(3) `slider`(2) `slider-thumb`(2) `select-content`(2) `select-item`(2) `table-body`(2) `table-head`(2) `button-decorations`(1) `block-search-content`(1) `dialog-overlay`(1) `dialog-pixel-x`(1) `dialog-pixel-y`(1) `input`(1) `radio-choice-mark`(1) `scroll-area-corner`(1) `scroll-area-thumb`(5) `radio-group-indicator`(4) `checkbox-indicator`(5) `checkbox-check`(1) `checkbox-mixed`(1) `select-item-indicator`(1) `select-scroll-up-button`(1) `select-scroll-down-button`(1) `select-trigger`(1) `select-viewport`(1) `slider-range`(1) `slider-track`(1) `table`(1) `table-cell`(1) `textarea`(1)。
- **12 个已发出但无人使用**：`dialog-content` `dialog-description` `dialog-title` `scroll-area` `select` `select-group` `select-label` `select-separator` `select-value` `table-caption` `table-footer` `table-header`。

变体值使用统计：`variant="…"` 共出现 120 次 —— `ghost` 66、`secondary` 38、`default` 10、`destructive` 6。CVA 中定义的 `outline`、`link` 两个变体**从未被业务代码使用**，但 `modern.css:84`、`8bitcn/theme.css:59`、`minecraft/appearance.css:91,94` 都在为它们写样式。

### 1.6 按钮的调用方式：组件已统一，样式未统一

对全部 53 个 TSX 文件做大小写敏感的标签级统计：

| 项 | 数量 |
| -- | ---- |
| `<Button>`（大写，库组件） | **267**，分布在 22 个文件 |
| `<button>`（小写，裸元素） | **17**，仅 3 个文件：`AssetManagerContent`(8) `EnumeratorWindow`(8) `BlockPickerDialog`(1) |

`<Button>` 的 267 次调用按样式写法拆分：

| 写法 | 数量 |
| ---- | ---- |
| 带 `variant=` | 126 |
| 带任意 `className=` | 101 |
| 其中 `className` 含遗留 `btn` 类 | **75** |
| 带 `size=` | 33 |
| 完全裸用（无 `className` / `variant` / `size`，依赖 CVA 默认） | **45** |
| 带遗留尺寸类 `.btn-xs` / `.btn-sm` / `.btn-md` / `.btn-action` / `.btn-primary` | 12（`ReplacePage` 10、`EnumeratorBlockPickerDialog` 2） |

**结论修正**：组件化程度已达 94%（267 : 17），"两种调用写法"不是主要矛盾。真正的问题是**同一个 `<Button>` 有四种样式写法**（裸用 / 用 `variant` / 同时挂遗留 `btn` 类 / 再挂遗留尺寸类），而它们最终产出什么外观由 1.10 节的六层 CSS 决定。因此本文件的重点在层叠与契约，而不在调用替换。

### 1.7 令牌现状：双轨制

| 体系 | 定义处 | 引用 |
| ---- | ------ | ---- |
| shadcn 语义令牌（`--background` 等） | 各主题文件 | 278 处 |
| `--ln-*` 令牌 | 65 个已定义 | 82 个不同名称被引用，共 561 处 `var()` |
| 硬编码色值 | — | 215 处 |

**`--ln-*` 内部已腐烂**：82 个被引用的名称中只有 65 个有定义，**18 个被引用但从未定义**，共 72 处引用永远走 fallback：

| 令牌 | 引用数 | 令牌 | 引用数 |
| ---- | ------ | ---- | ------ |
| `--ln-ui-edge-color` | 17 | `--ln-primary` | 3 |
| `--ln-ui-control-background-color` | 12 | `--ln-fieldset-border-color` | 2 |
| `--ln-border-color` | 11 | `--ln-card-border-width-active` | 1 |
| `--ln-inset-background` | 6 | `--ln-card-radius` | 1 |
| `--ln-accent-color` | 5 | `--ln-disabled-background` | 1 |
| `--ln-divider-color` | 4 | `--ln-disabled-text` | 1 |
| `--ln-surface-background-color` | 4 | `--ln-primary-button-background` | 1 |
| | | `--ln-primary-button-text` | 1 |
| | | `--ln-tooltip-background` | 1 |
| | | `--ln-ui-hover-background-color` | 1 |
| | | `--ln-ui-text-color` | 1 |

这些引用只能落到 `ThreeDShadow`、`Canvas`、`CanvasText`、`Highlight`、`ButtonFace`、`GrayText` 一类 CSS 系统色上——这正是 `base.css`、`replace-units.css`、`materials.css`、`settings.css` 中大量出现 `ThreeDShadow` / `ButtonBorder` 的原因。反向也有 1 个只定义不被引用的死令牌：`--ln-form-max`。

**关键实测结果**：14 个核心语义令牌（`--background` `--foreground` `--card` `--popover` `--primary` `--primary-foreground` `--secondary` `--muted` `--muted-foreground` `--accent` `--destructive` `--border` `--ring` `--radius`）**三个主题已全部定义**，包括 Minecraft（`themes/minecraft/appearance.css:4-16` 与 `:43-55`）。

缺口仅 5 个：

| 令牌 | shadcn | 8bitcn | Minecraft |
| ---- | ------ | ------ | --------- |
| `--edge-width` | 定义 | 定义 | **缺** |
| `--control-shadow` | 定义 | 定义 | **缺** |
| `--panel-shadow` | 定义 | 定义 | **缺** |
| `--success` | **缺** | **缺** | **缺**（仅 `--ln-success-color`） |
| `--warning` | **缺** | **缺** | **缺**（仅 `--ln-warning-color`） |

`themes/modern.css` 是桥接层，把 `--ln-*` 指向 shadcn 令牌（`:33-48`），说明新体系是贴在旧体系之上，而非替换。

### 1.8 僵尸层清单

Minecraft 主题的控件规则以裸元素选择器为主，`[data-slot]` 引用仅 21 处（`widget/scroll.css` 3、`appearance.css` 18）。而 Radix 组件渲染的元素类型与裸元素选择器的假设不符。逐文件核对渲染 DOM 后：

| 文件 | 行数 | 目标选择器 | 实际匹配元素数 | 判定 |
| ---- | ---- | ---------- | -------------- | ---- |
| `themes/minecraft/widget/range.css` | 129 | `input[type="range"]` + `::-webkit-slider-*` | **0** | **完全失效**（Radix Slider 渲染 `<span>`） |
| `themes/minecraft/widget/check.css` | 79 | `input[type="checkbox"／"radio"]` | **1** | 近乎全失效（全项目仅存 `EnumeratorWindow.tsx:543` 一处原生 checkbox；原生 radio、range 各 0 处） |
| `themes/minecraft/widget/button.css` | 163 | `button` | 多处 | 有效（`<Button>` 与裸 `<button>` 均为 button 元素） |
| `themes/minecraft/widget/input.css` | 103 | `input` / `select` / `textarea` | 多处 | 有效 |
| `themes/minecraft/widget/scroll.css` | 41 | `[data-slot="scroll-area-*"]` | 有效 | **已是目标写法** |
| `themes/minecraft/appearance.css` | 202 | 混用（18 处 `data-slot`、11 处 `data-variant`） | 有效 | **已是目标写法** |

**由此产生一个可见缺陷**：整个 `ui/themes/minecraft/` 中不存在任何 `[data-slot="slider"]` 规则，`slider` 字样只出现在 `range.css` 那 4 条失效规则里。因此 **Minecraft 主题下的滑块没有原版贴图外观**，落回 `styles/controls.css:85-90` 的通用方形样式。

`appearance.css:105-146` 已用 `button[data-slot="checkbox"]` / `[data-slot="radio-group-item"]` 挂贴图，并同时保留 `check.css` 的旧写法——**目标写法与旧写法在同一主题里并存**，这是本文件要收敛的对象。

### 1.9 下拉框的四条路线

| # | 实现 | 引用处 | 说明 |
| - | ---- | ------ | ---- |
| 1 | `ui/components/Dropdown.tsx` → `ui/components/ui/select.tsx`（Radix Select） | 8 个页面 | 名义主推路线 |
| 2 | 裸 `<select>` | 6 处：`AssetManagerContent`(3) `BlockPickerDialog`(1) `EnumeratorWindow`(1) `VersionConversionDialog`(1) | 完全绕开组件库 |
| 3 | `ui/components/BlockSearchSelect.tsx`（Radix Popover 自建） | 3 处 | 不是 Select 语义 |
| 4 | `ui/components/BlockPickerDialog.tsx`（自建对话框选择器） | — | 第四条路线 |

且路线 1 自身也在叠样式：`ui/components/ui/select.tsx:40` 同时挂载 `nova-control btn dropdown-trigger` 三个自定义类与一长串 Tailwind 类；`styles/widgets.css:181-192` 用 `.dropdown-trigger.btn` 覆写，`themes/modern.css:66` 再覆写 `button, .btn, .dropdown-trigger`，Minecraft 用 `widget/input.css` 覆写 `select`。

### 1.10 按钮的六层覆写

| 序 | 位置 | 内容 |
| -- | ---- | ---- |
| 1 | `ui/components/ui/button.tsx:8-33` | CVA：`bg-primary text-primary-foreground hover:bg-primary/90` 等 |
| 2 | `ui/styles/base.css:48-50` | `button { min-height: 28px }` |
| 3 | `ui/styles/widgets.css:121-135` | `.btn { padding: 4px 10px }`、`--ln-control-height: 32px` |
| 4 | `ui/styles/replace-units.css:439-501` | `.btn-xs` / `.btn-sm` / `.btn-md` / `.btn-action` / `.btn-primary` |
| 5 | `ui/themes/modern.css:66-84` | `button, .btn, .dropdown-trigger { … }`、`.primary`、`.danger`、`.btn-primary`、`[data-variant]`、`aria-pressed` |
| 6 | `ui/themes/minecraft/widget/button.css:14-38` | `border-image` 贴图、`height: 44px`，推翻第 5 层 |

第 1 层与第 5 层同时命中同一个元素，而第 1 层的 Tailwind 类因处于 `@layer utilities`（见 1.4）必然输给第 5 层的无层规则。这六层全部在无层区中按源码顺序与特异性争夺优先级，因此**同一个 `<Button>` 只要换一个页面、换一个主题，就会落到不同层的规则上**（见 1.6 的四种写法分布：裸用 45、带 `variant` 126、同时挂遗留 `btn` 类 75、再挂遗留尺寸类 12）。

---

## 2. 根因

表象是"按钮和下拉框有多种样式"，根因是五重并存，且缺少层叠约束：

1. **3 层全局覆写**：手写共享层 → 主题层 → 页面层，且全部在无层区，顺序不可预测。仅按钮一个控件就有 6 层（见 1.10）。
2. **4 种样式写法并存**：同一个 `<Button>` 被裸用（45 处）、用 `variant`（126 处）、同时挂遗留 `btn` 类（75 处）、再挂遗留尺寸类（12 处）。组件化本身已达 94%（267 : 17），写法不统一源于**没有单一外观来源**。
3. **4 种下拉实现**、**2 种复选框实现**（Radix 与原生并存）。
4. **2 套设计令牌**加 215 处硬编码色值。
5. **0 个层叠层**：项目自己的约 183 KB CSS 产物 @layer 出现次数为 0（见 1.4），Tailwind 工具类事实上失效。

第 5 条是使前 4 条无法通过"统一写法"解决的结构性原因：在没有层约束的前提下，任何新增样式都必须考虑"是否会压过别人"，于是只能不断追加覆盖规则。**因此迁移必须先修层叠，再统一写法；顺序颠倒会得到更多覆盖规则，而不是更少。**

---

## 3. 目标架构

### 3.1 分层模型

单一层序，五层：

```
@layer theme, tokens, base, components, utilities;
```

| 层 | 职责 | 允许内容 | 禁止内容 |
| -- | ---- | -------- | -------- |
| `theme` | Tailwind 自带设计令牌 | `tailwindcss/theme.css` | — |
| `tokens` | 三主题的变量定义 | 仅自定义属性声明 | 任何选择器规则 |
| `base` | 重置与元素默认值 | `box-sizing`、`body`、`font`、`img`、`table` 折叠 | 控件外观、品牌色 |
| `components` | 控件外观，**按主题分文件** | `[data-slot]` / `[data-variant]` / `[data-size]` / `[data-state]` 组合选择器 | 裸元素选择器、`!important` |
| `utilities` | 布局与一次性微调 | Tailwind 工具类、页面布局类（`.library-*`、`.flake-page__*` 等） | 颜色、边框、圆角、阴影等控件外观属性 |

**规则**：任何 CSS 文件不得有无层规则。层声明写在 `ui/styles/base.css` 顶部，并在每个 `ui/themes/*/theme.css` 顶部重复同一行——两处声明内容完全一致时，先后顺序不影响结果，因此不依赖加载时序。

> 为什么不用页面层：页面 CSS 收进 `utilities` 层后，它仍然可以设置 `display`、`grid-template-columns`、`gap`，但设置 `background` 会被 `components` 层压过。这恰好是想要的语义：**页面负责排布，主题负责外观。**

### 3.2 令牌契约

三层令牌，命名与归属明确。

**Tier 1 语义令牌** —— 三主题**必须全量定义**，组件层只允许引用这一层。

| 令牌 | 用途 | 现状 |
| ---- | ---- | ---- |
| `--background` `--foreground` | 画布底色与正文 | 三主题已有 |
| `--card` | 卡片、面板、输入框表面 | 三主题已有 |
| `--popover` | 浮层表面（下拉、对话框、工具提示） | 三主题已有 |
| `--primary` `--primary-foreground` | 主操作、选中态 | 三主题已有 |
| `--secondary` | 次级操作底色 | 三主题已有 |
| `--muted` `--muted-foreground` | 弱化底色与次要文字 | 三主题已有 |
| `--accent` | 悬停、高亮 | 三主题已有 |
| `--destructive` | 危险操作、错误 | 三主题已有 |
| `--border` | 描边 | 三主题已有 |
| `--ring` | 焦点环 | 三主题已有 |
| `--radius` | 圆角基数 | 三主题已有 |
| `--edge-width` | 描边宽度（shadcn 1px / 8bitcn 2px / Minecraft 0） | **Minecraft 缺，需补 `0`** |
| `--control-shadow` | 控件阴影（8bitcn 像素投影） | **Minecraft 缺，需补 `none`** |
| `--panel-shadow` | 浮层阴影 | **Minecraft 缺，需补 `none`** |
| `--success` `--warning` | 状态色 | **三主题均缺，需新增** |
| `--control-height` | 控件基准高度（32 / 36 / 44px） | 现为 `--ln-control-height`，**升级为语义令牌** |
| `--option-active-background` `--option-active-color` | 列表项激活态 | 现为 `--ln-option-*`，**升级**（Minecraft 需 `#4d4d4d` / `#ffff00`，与 `--accent` 语义不同，必须独立） |

**Tier 2 结构令牌** —— 跨主题稳定，随控件尺寸而非随主题变化。

`--ln-scroll-size` `--ln-scroll-radius` `--ln-scroll-track` `--ln-scroll-track-hover` `--ln-scroll-thumb` `--ln-scroll-hover` `--ln-scroll-active` `--ln-scroll-edge` `--ln-scroll-shadow` `--ln-file-icon-size` `--ln-body-font-family` `--ln-body-font-size` `--ln-check-size` `--ln-check-label-gap` `--ln-check-label-min-height`

**Tier 3 主题私有令牌** —— 仅允许被所属主题目录内的文件引用，跨主题引用视为错误。

`--ln-minecraft-button-image` `--ln-minecraft-button-height` `--ln-minecraft-button-border` `--ln-minecraft-input-*` `--ln-minecraft-table-row-*` `--ln-minecraft-nav-*` `--ln-minecraft-range-*` `--ln-minecraft-card-border-color`（拟由 `--ln-card-border-color` 改名） `--ln-table-row-selected-background` `--ln-card-border-width` `--ln-card-background-color-active` `--ln-card-border-color-active`

**纯桥接、应删除**（值为另一令牌的直接别名，无独立语义）：
`--ln-ui-background-color` `--ln-navigation-background-color` `--ln-body-text-color` `--ln-muted-text-color` `--ln-control-radius` `--ln-focus-color` `--ln-popup-shadow` `--ln-error-color`

> `--ln-card-border-color` 一名两用：`minecraft/widget/content.css:3` 定义为 `#a0a0a0`，属 Tier 3；`modern.css:46` 与 `appearance.css:22` 定义为 `var(--border)`，属桥接。处理方式：删除后两处定义，其消费者改用 `--border`；保留 Minecraft 的定义并重命名为 `--ln-minecraft-card-border-color` 以显式标明归属。

**保留的兼容令牌**：`--ln-space-1..6` `--ln-form-max` `--ln-field-max` `--ln-window-content-padding` 属于布局尺寸，不参与主题切换，可留在 `base` 层。

> 完整清单见附录 A。

### 3.3 组件契约

组件是**语义容器**，不携带外观。

**必须发出**（`data-*` 属性），这是主题唯一的挂钩点：

| 属性 | 取值来源 | 说明 |
| ---- | -------- | ---- |
| `data-slot` | 组件内部固定 | 标识控件槽位，如 `button`、`checkbox`、`slider-thumb` |
| `data-variant` | CVA 变体名 | `default` `secondary` `outline` `ghost` `destructive` `link` |
| `data-size` | CVA 尺寸名 | `default` `xs` `sm` `lg` `icon` `icon-xs` `icon-sm` `icon-lg` |
| `data-state` | Radix 状态 | `checked` `unchecked` `indeterminate` `open` `closed` |

**CVA 中禁止出现外观类。** 现状 `button.tsx:9` 的 `bg-primary text-primary-foreground hover:bg-primary/90`、`input.tsx:11` 的 `h-9 rounded-md border border-input bg-transparent px-3`、`select.tsx:40` 的 `border border-input px-3 py-2 shadow-xs` 全部迁入 `components` 层。CVA 只保留结构类（`inline-flex` `items-center` `gap-2` `shrink-0` `whitespace-nowrap`）与可访问性类（`outline-none` `disabled:pointer-events-none` `aria-invalid:*`）。

**理由**：这些外观类在三个主题里都会被覆写（Minecraft 甚至不加载 Tailwind），保留它们只会制造"看起来生效但实际被压过"的负债。剥离后组件的行为契约由 `data-*` 表达，外观契约由主题表达，职责单一。

**组合层 API 统一**：`BlockSearchSelect`、`BlockPickerDialog` 改为基于 `ui/select.tsx` 或至少复用 `ui/dialog.tsx` + `ui/input.tsx`，消除第 3、4 条下拉路线。

### 3.4 主题契约

每个主题 = **一份 Tier 1 令牌 + 一份 `components` 层规则**。三个主题地位完全平等，不存在"谁压过谁"。

```
ui/themes/
  shadcn/theme.css      @layer tokens     { 语义令牌（zinc，radius 6px，edge 1px） }
                        @layer components { [data-slot] 规则（如无特有规则则不写） }
  8bitcn/theme.css      @layer tokens     { 语义令牌（像素，radius 0，edge 2px，像素投影） }
                        @layer components { [data-slot] 像素补丁 }
  minecraft/theme.css   @layer tokens     { 语义令牌（含新增的 3 个缺口令牌） }
                        @layer components { [data-slot] 贴图规则 }
```

**Minecraft 主题必须加载 Tailwind 层。** 现状 `themes/minecraft/theme.css` 不 import `modern.css`，导致它依赖"无层规则压过一切"才能工作。改为加载后，它与其他两个主题走同一条路径；同时须补齐 `--edge-width: 0`、`--control-shadow: none`、`--panel-shadow: none`。

**共享结构放 `base` 层，共享行为放各主题 `components` 层。** 现状 `styles/controls.css` 与 `styles/widgets.css` 的正确部分（焦点环、图标尺寸、滚动条命中区、下拉浮层定位、`table-row-selected` 语义）应进入 `base`；与颜色/描边相关的部分进入各主题 `components`。

### 3.5 Minecraft 主题的接入方式

**结论：完全可以并入，且已并入一半。** `appearance.css:105-146` 是现成样板：

```css
@layer components {
    html[data-theme="minecraft"] [data-slot="checkbox"] {
        width: 20px;
        height: 20px;
        padding: 0;
        border: 0;
        border-image: none;
        border-radius: 0;
        background: transparent url("./resource/form/checkbox_default.png") center / 20px 20px no-repeat;
        image-rendering: pixelated;
        box-shadow: none;
    }
    html[data-theme="minecraft"] [data-slot="checkbox"]:hover:not(:disabled) {
        background-image: url("./resource/form/checkbox_hover.png");
    }
    html[data-theme="minecraft"] [data-slot="checkbox"][data-state="checked"] {
        background-image: url("./resource/form/checkbox_checked.png");
    }
}
```

Minecraft 的独有外观全部可由"组件槽位 + 贴图"表达，无需保留裸元素选择器：

| 能力 | 目标写法 |
| ---- | -------- |
| 贴图按钮边框 | `[data-slot="button"]` + `border-image-source`，状态换图用变量（`widget/button.css:18,44,52,57,62,70` 已验证该机制与选择器无关） |
| 像素复选框 / 单选 | `[data-slot="checkbox"]` / `[data-slot="radio-group-item"]` —— **已完成** |
| 滑块贴图 | `[data-slot="slider-track"]` / `[data-slot="slider-thumb"]` 的 `background-image` —— **待重写** |
| 滚动条贴图 | `[data-slot="scroll-area-thumb"]` —— **已完成**（`widget/scroll.css:15-29`） |
| 输入框与原生下拉箭头 | `[data-slot="input"]`、`[data-slot="select-trigger"]`（Radix 触发器是 `<button>`，不是 `<select>`，需新增规则） |
| 表格斑马纹与选中态 | `[data-slot="table-row"]`（现用 `table > tbody > tr`，元素选择器仍可生效，但为一致性应改为 `data-slot`） |
| 导航按钮贴图 | 已是 `.nav-item` 类选择器，改为 `[data-slot="button"].nav-item` 以纳入契约 |

**4 项必须解决的技术点**：

1. **伪元素无法重命名（唯一真难点）**：`::-webkit-slider-thumb` / `::-moz-range-thumb` 在 Radix Slider 上不存在。须改为在 `[data-slot="slider-thumb"]` 上设 `background-image`，并保留 `image-rendering: pixelated`。技术方案现成，照抄 `widget/scroll.css:15-29` 处理 12×2 贴图的做法（固定尺寸、pinned 到边缘、禁用 `box-shadow`）即可，约 40 行。
2. **状态贴图换图**：已解决，见 `widget/button.css` 的 `--ln-minecraft-button-image` 变量切换模式。
3. **Radix Select 触发器元素类型**：需从 `select { background-image: url(spin_down.png) }` 改为针对 `[data-slot="select-trigger"]`，否则 Minecraft 主题下的 Radix 下拉拿到的是 44px 按钮贴图而非"输入框 + 下拉箭头"。
4. **补齐 3 个缺口令牌**：`--edge-width: 0`、`--control-shadow: none`、`--panel-shadow: none`。

**必须放弃的**：继续让裸元素自动继承主题。具体为 6 处裸 `<select>` 与 1 处裸 checkbox，均应改为库组件。

### 3.6 禁止清单

以下写法在迁移完成后不得新增，代码评审按此检查：

1. 新增无层 CSS 规则。
2. 在 `base` / `components` / `utilities` 层内使用 `!important`。
3. 在 `components` 层使用裸元素选择器（`button`、`input`、`select`、`table`、`th`、`td`）。
4. 在业务代码中写 `className="bg-primary …"` 一类外观工具类。
5. 新增第二套按钮类名（`.btn-*` 之外的变体前缀）；尺寸一律用 `size` 变体。
6. 新增下拉实现；一律走 `ui/select.tsx` 或 `Dropdown`。
7. 在业务代码中直接写 `var(--ln-minecraft-*)`。
8. 跨主题引用 Tier 3 令牌。
9. 引用未定义的 `--ln-*` 令牌（依赖 fallback 生效）。

---

## 4. 迁移路径

各阶段可独立验证、独立回滚。**每阶段结束都必须通过第 5 节的回归矩阵。**

### Phase 0 — 建立护栏（不改视觉）✅ 已完成

**产物**

| 文件 | 作用 |
| ---- | ---- |
| `desktop-nova/scripts/check-ui-styles.mjs` | 护栏脚本，六条检测规则 + 基线比较；可 `import` 复用 |
| `desktop-nova/scripts/ui-styles-baseline.json` | 基线快照 |
| `desktop-nova/scripts/check-ui-styles.test.mjs` | 8 个用例，覆盖检测、基线、校验与仓库一致性 |
| `desktop-nova/scripts/virtual-spacer.test.mjs` | 3 个用例，锁定虚拟列表占位高度的组件/样式表契约 |

**六条检测规则**（对应 3.6 禁止清单）

| 规则名 | 检测 | 基线计数 |
| ------ | ---- | -------- |
| `css-unlayered-rule` | 不在任何 `@layer` 块内的样式规则 | 1658 |
| `css-important` | `!important`（`@media (prefers-reduced-motion: reduce)` 内除外） | 13 |
| `css-components-bare-element-selector` | `@layer components` 内以裸元素名为主题的规则 | 0 |
| `tsx-appearance-utility-class` | `className` 中的 Tailwind 外观工具类（变体前缀先剥离） | 102 |
| `token-undefined-reference` | `var(--ln-*)` 引用但全项目无定义 | 73 |
| `css-legacy-button-variant` / `tsx-legacy-button-variant` | `.btn-*` 遗留变体类的定义与使用 | 8 / 12 |

**基线机制：按 (规则, 文件) 计数，而非精确行号。** 理由：Phase 1 会把每条规则搬进层里、Phase 3 会重写组件类名，精确行号记录会在每次提交时整批翻转成"已解决 + 新增"，噪声淹没信号。计数式表达的是迁移期间真正要守住的性质：**任一文件的任一规则不得增加，且所有计数必须归零**。

**命令**

```
npm run check:ui-styles                 # 基线比较（CI 与日常使用）
npm run check:ui-styles:strict          # 逐条列出全部位置
npm run check:ui-styles:report          # 现状总量报告
npm run check:ui-styles:update-baseline # 有意调整后刷新基线
npm run test:ui-styles                  # 8 个用例
```

`--print-baseline` 把基线 JSON 输出到 stdout，便于在无法写工作区的环境里手工落盘。

**本阶段对文档的两处收严**（实现时发现更合适的边界）

1. `!important` 的禁令扩大到**全部项目 CSS**，不再限于层内——`!important` 与是否分层无关，一刀切更简单也更强。唯一豁免是 `@media (prefers-reduced-motion: reduce)` 块：那里用 `!important` 是压制动画的标准做法。
2. `outline-none` / `outline-hidden` 不计为外观工具类：它们是 3.3 节明确要求 CVA 保留的焦点环重置；只有 `outline-<宽度|颜色>` 才算外观。

**退出验证**

| 检查 | 结果 |
| ---- | ---- |
| `npm run check:ui-styles` | ✅ 1866 项已知，0 回归，0 新增 |
| `npm run test:ui-styles` | ✅ 8/8 通过 |
| `npm run build` | ✅ 通过 |
| `npm run check:application-boundaries` | ✅ 0 违规 |
| `npm run check:layers` | ✅ 0 违规。此前 3 项 `ui-no-fixed-inline-style` 已一并修复，见下 |

**顺带修复：虚拟列表占位高度不再内联声明**

`check:layers` 在本阶段开始时是红的，3 项违规全部是同一个东西——虚拟列表占位元素用内联 `height` 预留滚动高度（`VirtualSpacerCell.tsx:41`、`StatisticsPage.tsx:381,389`）。这属于"上游传入导致的"，与层化无关，但它正好示范了本文件 3.3 节要推广的模式，所以一并修掉：

- 参与方只有两半：组件通过 `style.setProperty` 发布 `--material-list-virtual-spacer-height`，**样式表拥有 `height` 声明**。先前的内联 `height` 是冗余的第二来源，也是 `enumerator.css:353` 不得不用 `!important` 压过它的原因。
- `StatisticsPage` 的摘要列表分支（非表格）此前没享受到组件化，直接写内联 `<li>`；新增 `VirtualSpacerItem` 作为 `VirtualSpacerCell` 的列表项孪生体，两处都改用组件。
- `materials.css` 的 `.material-summary-list > .material-virtual-spacer` 补上 `height: var(--material-list-virtual-spacer-height, 0px)`；两处 `var()` 统一带 `0px` 兜底。
- 该耦合曾经出过一次线上回归（`enumerator.css:329-339` 记录了滚动条在两偏移间来回跳的现象），所以新增 `scripts/virtual-spacer.test.mjs` 把"组件发布的属性名"与"样式表消费的属性名"钉在一起：任一侧改名都会失败。

这也是 3.6 禁止清单第 3、4 条之外的一条通用结论：**动态几何值走自定义属性，视觉属性留在样式表**；组件只传值，不声明样式。

### Phase 1 — 层化（不改视觉）

**做**：
1. 在 `ui/styles/base.css` 与 3 个 `ui/themes/*/theme.css`（`shadcn` `8bitcn` `minecraft`）顶部加同一行 `@layer theme, tokens, base, components, utilities;`。
2. 把 `ui/styles/`、`ui/shell/widget/`、`ui/themes/`、页面 CSS 的现有规则**原样**包进对应层。此阶段只搬位置，不改内容。
3. 把现有 `[data-slot]` 规则与主题覆写放入 `components`；令牌声明放入 `tokens`；页面布局类放入 `utilities`。

**退出**：`dist/assets/*.css` 中无层规则数为 0（除 `theme` 层的 Tailwind 声明）；三主题切换截图与迁移前逐像素一致。

**风险**：高。层化会改变现有覆盖关系（原本无层互压，现在按层序）。**必须**在本阶段保留"最后写入者胜"的等效结果——必要时用选择器特异性补偿，而不是重新引入无层规则。建议本阶段单独一个提交，便于二分定位。

### Phase 2 — 令牌收敛

**做**：
1. 补齐 `--edge-width`、`--control-shadow`、`--panel-shadow` 到 Minecraft；新增 `--success`、`--warning` 到三主题。
2. 处理 18 个"被引用但未定义"的令牌（见 1.7）：逐个判定是"应存在的语义令牌"还是"早期遗留的名称"。前者（如 `--ln-ui-edge-color`、`--ln-ui-control-background-color`、`--ln-inset-background`、`--ln-divider-color`）归入 Tier 1/Tier 2 并补齐三主题定义；后者（如 `--ln-primary`、`--ln-card-radius`）直接改为对应语义令牌，删除引用。**此步骤必须在 Phase 1 之后做**，因为无层区下无法判断某个 fallback 是否真的在生效。
3. 把 `--ln-control-height`、`--ln-option-active-*` 升级为 Tier 1 语义令牌，旧名保留为别名一个版本。
4. 删除纯桥接令牌，把引用直接指向语义令牌。
5. 把 215 处硬编码色值改为令牌引用；确属主题特有的（如 `--ln-minecraft-*` 贴图路径）保留，但登记为 Tier 3。
6. 删除只定义不引用的死令牌（`--ln-form-max`）。

**退出**：Tier 1 令牌在三主题中覆盖率为 100%；"被引用但未定义"的令牌数为 0；`var(--ln-*)` 引用数从 561 降至仅 Tier 2 / Tier 3 的量；硬编码色值降至仅 Tier 3 定义处。

### Phase 3 — 组件外观类剥离

**做**：从 `ui/components/ui/` 的 5 个含 Tailwind 外观类的文件（`button` `select` `dialog` `table` `input`）中移除颜色/描边/圆角/阴影类，把这些外观写入 `ui/themes/shadcn/theme.css` 的 `components` 层。`cn()` 保留用于合并业务传入的结构类。

**退出**：`ui/components/ui/` 中外观工具类为 0；三主题下控件外观与迁移前一致（该主题本来就是靠覆写生效的，因此应有极小差异，需逐项确认）。

**风险**：中。8bitcn 的 `ButtonDecorations` 依赖 Tailwind 定位类（`absolute` `-top-0.5` `h-0.5` `w-1/2`），属于结构类，保留。

### Phase 4 — 控件统一

按依赖顺序，每个控件独立一个提交：

| 步骤 | 内容 | 前置 |
| ---- | ---- | ---- |
| 4.1 | Slider：为 Minecraft 新增 `[data-slot="slider-*"]` 贴图规则；删除 `widget/range.css` | Phase 1 |
| 4.2 | Checkbox：把 `EnumeratorWindow.tsx:543` 的裸 checkbox 换成 `ui/checkbox.tsx`；删除 `widget/check.css` | 4.1 |
| 4.3 | Select：把 6 处裸 `<select>` 换成 `Dropdown`；删除 Minecraft 的裸 `select` 规则，改为 `[data-slot="select-trigger"]` | 4.2 |
| 4.4 | 按钮尺寸：用 `size` 变体替换遗留尺寸类（`ReplacePage` 10 处、`EnumeratorBlockPickerDialog` 2 处）；删除 `.btn-xs` / `.btn-sm` / `.btn-md` / `.btn-action` / `.btn-primary` 定义 | 4.3 |
| 4.5 | 按钮写法：从 75 处 `<Button className="btn …">` 中移除遗留 `btn` 类，改为按语义指定 `variant`；补齐 45 处裸用 `<Button>` 的语义变体 | 4.4 |
| 4.6 | 按钮残留：把 3 个文件共 17 处裸 `<button className="btn …">` 换成 `<Button>` | 4.5 |
| 4.7 | 下拉收敛：`BlockSearchSelect` / `BlockPickerDialog` 改为复用库组件 | 4.3 |
| 4.8 | 变体清理：确认 `outline` / `link` 无业务使用后，删除其主题规则，或在 `UiTestPage` 中补上用例 | 4.5 |

**退出**：`className` 中出现遗留 `btn` 类的 `<Button>` 为 0；裸 `<button>` 为 0（组件实现自身除外）；裸 `<select>`、裸 checkbox 为 0；下拉实现收敛为 1 条路线（`BlockSearchSelect` 的搜索语义可保留为 Select 的上层封装）。

### Phase 5 — 清理僵尸层

**做**：删除 `widget/range.css`、`widget/check.css`；把 `widget/button.css`、`widget/input.css`、`widget/table.css`、`widget/nav.css`、`widget/subwindow.css` 的裸元素选择器改写为 `[data-slot]`；`widget/popup.css`、`widget/dialog.css`、`widget/content.css`、`widget/text.css` 的类选择器并入 `components`。

**退出**：`ui/themes/minecraft/` 中裸元素选择器为 0；文件数从 15 降至不超过 6。

### Phase 6 — 页面 CSS 收编

**做**：把 `ui/windows/**/*.css`（8 个文件，913 行）收进 `utilities` 层；删除其中的外观属性（`background`、`border`、`box-shadow`、`transform`），保留布局属性。`library.css:31,56,365` 的手动清零规则在本阶段删除——层序已保证 `components` 优先。

**退出**：页面 CSS 中外观属性为 0；无 `border-image: none` / `box-shadow: none` / `transform: none` 一类补丁。

---

## 5. 回归矩阵

每阶段结束执行。构建与 CLI 检查见 [DEVELOPMENT.md](DEVELOPMENT.md)。

| 维度 | 检查项 | 方式 |
| ---- | ------ | ---- |
| 主题 | 三主题 × 浅色/深色/跟随系统，共 9 组 | 人工逐组截图 |
| 控件 | `UiTestPage`（`ui/windows/main/pages/ui-test/UiTestPage.tsx`）全部控件，在 9 组下外观正确 | 人工 |
| 控件 | 复选框 checked / unchecked / indeterminate；单选 checked / unchecked；滑块拖动、禁用、键盘；下拉打开、键盘选择、溢出滚动 | 人工 |
| 结构 | `ui/windows/main/pages/` 下 10 个页面 + `ui/windows/` 下 6 个子窗口，在 9 组下无错位 | 人工 |
| 层叠 | `dist/assets/*.css` 无层规则数为 0；`components` 层无裸元素选择器 | `npm run check:ui-styles` |
| 约束 | 无新增无层规则；无新增 `.btn-*`；业务代码外观工具类为 0；无未定义令牌引用 | `npm run check:ui-styles` |
| 护栏 | 检测规则与基线机制本身未被改坏；仓库基线未陈旧 | `npm run test:ui-styles` |
| 既有 | `npm run build`、`check:layers`、`check:application-boundaries`、`test:layer-check`、`test:application-boundaries` | CLI |
| 焦点 | Tab 顺序、焦点环可见性、Radix 焦点陷阱、`prefers-reduced-motion` | 人工 |

> 说明：仓库现有文档已明确"构建及 CLI 通过不代表实机通过"（`DEVELOPMENT.md:60`）。主题视觉属于必须人工验收的项目，不得以静态检查替代。

---

## 6. 风险与放弃项

**风险**

| 风险 | 等级 | 应对 |
| ---- | ---- | ---- |
| Phase 1 层化改变既有覆盖关系，产生大面积视觉回归 | 高 | 单独提交；逐主题逐页对照；必要时用特异性补偿而非重新引入无层规则 |
| Phase 4.5 / 4.6 改写按钮写法时语义误判（如把导航项、关闭按钮当成动作按钮） | 中 | 导航项、关闭按钮、物品栏槽位、快捷栏已有专属类，属于"结构容器"而非"动作按钮"：保留类名但纳入 `[data-slot]` 契约。改动 75+45+17 处，建议按页面分批提交 |
| Minecraft 主题加载 Tailwind 后首屏样式闪烁 | 中 | 层声明在 `base.css` 与 `theme.css` 双写，不依赖加载顺序；主题样式表已由 `themeRuntime.ts` 预载（`media="not all"`），切换不需重新请求 |
| `--ln-minecraft-button-image` 变量在 `:root` 定义、在元素上覆写，层化后继承链变化 | 中 | Phase 1 后专项验证按钮四态 + 禁用态 + `aria-pressed` |

**放弃项**

1. **裸元素自动继承主题**：不再支持"写一个 `<select>` 就自动变成 Minecraft 风格"。控件必须走组件库。
2. **`outline` / `link` 变体的主题规则**（若无业务使用）：删除，避免维护无人使用的分支；如需保留，须在 `UiTestPage` 建立用例。
3. **`.btn-xs` / `.btn-sm` / `.btn-md` / `.btn-action` / `.btn-primary` 类名**：由 `size` / `variant` 变体取代。
4. **`--ln-*` 桥接令牌**：删除，引用直接指向语义令牌。

**不做的事**

- 不升级 React 18（`THIRD_PARTY_UI.md:12` 已约束）。
- 不复制三套页面（`THIRD_PARTY_UI.md:13` 已约束）。
- 不引入新的 UI 组件库；Radix + shadcn 源码自有副本已足够。
- 不清理 `@radix-ui/*` 中未使用的 51 个子包——属于独立议题，先确认 `radix-ui` 聚合包的 tree-shaking 行为。

---

## 附录 A：令牌对照表

`--ln-*` 当前定义处与目标归属。定义处为文件短名。

| 令牌 | 定义处 | 目标层级 |
| ---- | ------ | -------- |
| `--ln-space-1..6` | base.css:6-10 | 保留（布局，不入主题） |
| `--ln-form-max` `--ln-field-max` | base.css:11-12 | 保留（布局） |
| `--ln-window-content-padding` | shell/theme.css:11 | 保留（布局） |
| `--ln-ui-background-color` | shell/theme.css, modern.css:33, appearance.css:18, minecraft/theme.css:17 | **删除**（→ `--background`） |
| `--ln-navigation-background-color` | shell/theme.css, modern.css:34, appearance.css:19, minecraft/theme.css:18 | **删除**（→ `--card` / `--background`） |
| `--ln-body-text-color` | shell/widget/text.css:3, modern.css:35, appearance.css:20, minecraft/widget/text.css:12 | **删除**（→ `--foreground`） |
| `--ln-body-font-family` | shadcn:21, 8bitcn:45, minecraft/widget/text.css:10 | Tier 2 |
| `--ln-body-font-size` | shell/widget/text.css:2, shadcn:22, 8bitcn:46, minecraft/widget/text.css:11 | Tier 2 |
| `--ln-muted-text-color` | modern.css:36, appearance.css:21 | **删除**（→ `--muted-foreground`） |
| `--ln-control-height` | widgets.css:111 (32px), modern.css:37 (36px), minecraft/widget/button.css:2 (44px) | **Tier 1** |
| `--ln-control-radius` | widgets.css:112 (3px), modern.css:38 | **删除**（→ `--radius`） |
| `--ln-focus-color` | widgets.css:114, modern.css:39, minecraft/widget/button.css:4 | **删除**（→ `--ring`） |
| `--ln-popup-shadow` | widgets.css:113, modern.css:42 | **删除**（→ `--panel-shadow`） |
| `--ln-error-color` | widgets.css:117, modern.css:43, minecraft/widget/button.css:8, appearance.css:27 | **删除**（→ `--destructive`） |
| `--ln-warning-color` | widgets.css:118, modern.css:44,53, minecraft/widget/button.css:9, appearance.css:29,57 | **Tier 1**（→ `--warning`） |
| `--ln-success-color` | modern.css:45,54, appearance.css:28,56 | **Tier 1**（→ `--success`） |
| `--ln-option-active-background` | widgets.css:115, modern.css:40, minecraft/widget/button.css:5, appearance.css:24 | **Tier 1** |
| `--ln-option-active-color` | widgets.css:116, modern.css:41, minecraft/widget/button.css:6, appearance.css:25 | **Tier 1** |
| `--ln-card-border-width` | minecraft/widget/content.css:2 | Tier 3 |
| `--ln-card-border-color` | minecraft/widget/content.css:3, appearance.css:22, modern.css:46 | Tier 3（值等于 `--border` 处删除） |
| `--ln-card-border-color-active` | minecraft/widget/content.css:4 | Tier 3 |
| `--ln-card-background-color-active` | minecraft/widget/content.css:5, modern.css:47, appearance.css:23 | Tier 3 |
| `--ln-scroll-size` | widgets.css:58,78, minecraft/widget/scroll.css:7 | Tier 2 |
| `--ln-scroll-radius` | widgets.css:59,69,79, appearance.css:156 | Tier 2 |
| `--ln-scroll-track` | widgets.css:60,70,80, modern.css:57, appearance.css:34,64,157 | Tier 2 |
| `--ln-scroll-track-hover` | widgets.css:61,71,81, modern.css:58, appearance.css:35,65,158 | Tier 2 |
| `--ln-scroll-thumb` | widgets.css:62,72,82, modern.css:59, appearance.css:36,66 | Tier 2 |
| `--ln-scroll-hover` | widgets.css:63,73,83, modern.css:60, appearance.css:37,67 | Tier 2 |
| `--ln-scroll-active` | widgets.css:64,74,84, modern.css:61, appearance.css:38,68 | Tier 2 |
| `--ln-scroll-edge` | widgets.css:65 | Tier 2 |
| `--ln-scroll-shadow` | widgets.css:66,75,85, appearance.css:69 | Tier 2 |
| `--ln-file-icon-size` | shell/theme.css:14, modern.css:48, minecraft/theme.css:19 | Tier 2 |
| `--ln-table-row-selected-background` | shell/widget/table.css:2, minecraft/widget/table.css:6, appearance.css:63 | Tier 3 |
| `--ln-check-size` | shell/widget/check.css:2, minecraft/widget/check.css:2 | Tier 2 |
| `--ln-check-label-gap` | shell/widget/check.css:3, minecraft/widget/check.css:3 | Tier 2 |
| `--ln-check-label-min-height` | shell/widget/check.css:4, minecraft/widget/check.css:4 | Tier 2 |
| `--ln-minecraft-button-image` | minecraft/widget/button.css:18,44,52,57,62,70,101,108,120,128,146; nav.css:91,97; subwindow.css:32,40; flake.css:460,488,498 | Tier 3 |
| `--ln-minecraft-button-border` | minecraft/widget/button.css:10 | Tier 3 |
| `--ln-minecraft-button-height` | minecraft/widget/button.css:11 | Tier 3 |
| `--ln-minecraft-input-*`（8 个） | minecraft/widget/input.css:2-9, appearance.css:30-33,58,59 | Tier 3 |
| `--ln-minecraft-nav-button-height` | minecraft/widget/nav.css:2 | Tier 3 |
| `--ln-minecraft-sidebar-collapsed-width` | minecraft/widget/nav.css:3 | Tier 3 |
| `--ln-minecraft-range-*`（7 个） | minecraft/widget/range.css:2-8 | Tier 3（**该文件整体待删除**） |
| `--ln-minecraft-table-row-min-height` | minecraft/widget/table.css:2 | Tier 3 |
| `--ln-minecraft-table-row-background-odd/even/hover` | minecraft/widget/table.css:3,4,5, appearance.css:60,61,62 | Tier 3 |
| **未定义的 18 个**：`--ln-accent-color` `--ln-border-color` `--ln-card-border-width-active` `--ln-card-radius` `--ln-disabled-background` `--ln-disabled-text` `--ln-divider-color` `--ln-fieldset-border-color` `--ln-inset-background` `--ln-primary` `--ln-primary-button-background` `--ln-primary-button-text` `--ln-surface-background-color` `--ln-tooltip-background` `--ln-ui-control-background-color` `--ln-ui-edge-color` `--ln-ui-hover-background-color` `--ln-ui-text-color` | 无 | Phase 2 逐项判定：归入 Tier 1/2 并补齐定义，或改为对应语义令牌 |
| **只定义不引用的 1 个**：`--ln-form-max` | base.css:11 | 删除 |

## 附录 B：`data-slot` 契约表

| 组件 | 槽位 | 状态 |
| ---- | ---- | ---- |
| `ui/button.tsx` | `button`, `button-decorations` | 已消费 |
| `ui/checkbox.tsx` | `checkbox`, `checkbox-indicator`, `checkbox-check`, `checkbox-mixed` | 已消费 |
| `ui/dialog.tsx` | `dialog-content`✗, `dialog-description`✗, `dialog-title`✗, `dialog-overlay`, `dialog-pixel-x`, `dialog-pixel-y` | 3 个未使用 |
| `ui/input.tsx` | `input` | 已消费 |
| `ui/radio-group.tsx` | `radio-group`, `radio-group-item`, `radio-group-indicator` | 已消费 |
| `ui/scroll-area.tsx` | `scroll-area`✗, `scroll-area-viewport`, `scroll-area-scrollbar`, `scroll-area-thumb`, `scroll-area-corner` | 1 个未使用 |
| `ui/select.tsx` | `select`✗, `select-trigger`, `select-value`✗, `select-content`, `select-viewport`, `select-item`, `select-item-indicator`, `select-label`✗, `select-group`✗, `select-separator`✗, `select-scroll-up-button`, `select-scroll-down-button` | 5 个未使用 |
| `ui/slider.tsx` | `slider`, `slider-track`, `slider-range`, `slider-thumb` | 已消费（Minecraft 缺贴图规则） |
| `ui/table.tsx` | `table`, `table-header`✗, `table-body`, `table-footer`✗, `table-row`, `table-head`, `table-cell`, `table-caption`✗ | 3 个未使用 |
| `ui/textarea.tsx` | `textarea` | 已消费 |
| `ui/components/BlockSearchSelect.tsx` | `block-search-content` | 已消费（待并入 Select 契约） |
| `ui/windows/main/pages/settings/SettingsPage.tsx` | `radio-choice-mark` | 已消费 |

✗ = 已发出但 CSS 未使用，共 12 个（发出 48，消费 36）。这些槽位保留（主题可能需要），但应在 Phase 3 前确认是否需要补规则或从组件中移除。

## 附录 C：待删除文件清单

| 文件 | 行数 | 删除条件 |
| ---- | ---- | -------- |
| `ui/themes/minecraft/widget/range.css` | 129 | Phase 4.1 完成（`[data-slot="slider-*"]` 贴图规则就位） |
| `ui/themes/minecraft/widget/check.css` | 79 | Phase 4.2 完成（最后一处原生 checkbox 替换） |
| `ui/shell/widget/button.css` | 3 | 内容为占位类 `.iJustWantToHideWarnInConsole`，无人引用，Phase 1 时确认后删除 |
| `ui/shell/widget/input.css` | 3 | 同上，占位 |
| `ui/shell/widget/range.css` | 3 | 同上，占位 |
| `ui/shell/widget/content.css` | 3 | 同上，占位 |
| `--ln-*` 桥接令牌声明 | ~20 行 | Phase 2 完成并保留一个版本的别名后 |
