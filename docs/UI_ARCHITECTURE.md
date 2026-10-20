# Nova UI 架构：主题化组件系统

| 文档版本 | 修改日期   | 修改人 | 修改说明 |
| -------- | ---------- | ------ | -------- |
| V1.0     | 2026-10-13 | Codex  | 现状基线与目标架构：分层模型、组件契约、令牌契约、主题契约、迁移路径、回归验证 |
| V1.1     | 2026-10-13 | Codex  | 记录 Phase 0 完成（护栏脚本、计数式基线、8+3 用例） |
| V1.2     | 2026-10-13 | Codex  | 记录 Phase 1 完成：层序修正为 `theme, vendor, tokens, base, components, utilities`；实测 2,951 处反转后把页面 CSS 的迁入推迟到 Phase 6；48 个文件完成层化，等价性三重验证通过 |
| V1.3     | 2026-10-13 | Codex  | 记录 Phase 2A 完成：系统色 240 → 4、未定义令牌 73 → 0；护栏扩到全部自定义属性并新增 `css-system-colour` 与 `css-vendor-layer-above-components` 两条规则；记下 `layer(vendor)` 被 `git checkout` 静默还原导致 Tailwind 工具类越层的教训 |
| V1.4     | 2026-10-13 | Codex  | 记录 Phase 2B 完成：14 个桥接令牌退场（191 处引用），`--navigation-background` 经实测改为提升而非删除；状态色与阴影令牌化；新增 `css-hardcoded-colour` 规则并保留 33 处需设计决定的字面值 |
| V1.5     | 2026-10-13 | Codex  | 记录 Phase 3 完成：58 个已失效的外观类直接删除、44 个仍生效的移植进 `components` 层，`tsx-appearance-utility-class` 102 → 0；记下分类工具四次误判（结构类、转义变体名、简写展开、元素选择器作用域）与一次脚本改写 `variant !== "outline"` 的事故 |
| V1.6     | 2026-10-13 | Codex  | 记录 Phase 4.1 / 4.2 完成：`range.css` 整套伪元素规则从未命中过 Radix 滑块（删除并改写为 `[data-slot="slider-*"]`），顺带修掉共享滑块缺少 `position: relative` 的错位缺陷；最后 1 处裸 checkbox 迁移后两份 `check.css` 规则全部成为死代码，`--ln-check-size` 退场；新增 `tsx-native-control` 规则，裸 `<select>` 基数复核为 10 处 |
| V1.7     | 2026-10-13 | Codex  | 修复两个仅运行时可见的 MC 控件缺陷（单选框被裁圆、复选框透出 `--primary`）：根因是共享层用 `background` 简写重置了 `background-image`，且共享层在 `background-color` 上优先级高于主题规则；新增 `minecraft-controls.test.mjs` 级联求解器测试，并记下该测试第一版为假绿的教训 |
| V1.8     | 2026-10-13 | Codex  | 记录 Phase 4.3 完成：10 处裸 `<select>`（原估 6 处）全部换成 `Dropdown`，`tsx-native-control` 归零；MC 的 `select` 样式搬到 `[data-slot="select-trigger"]`，并补齐 hover/active 重述以对抗通用 `button:hover`/`:active`；记下 `themeOf()` 把 `[data-theme]` 判成"别的主题"导致整个共享层被排除、测试再次假绿的教训 |
| V1.9     | 2026-10-13 | Codex  | 记录 Phase 4.4 / 4.6 完成：5 个遗留尺寸/主色类退场（`size`/`variant` 取代）；17 处裸 `<button>` 改为 `<Button>`，新增第 6 条护栏规则 `tsx-raw-button` |
| V1.10    | 2026-10-13 | Codex  | **修正**：Phase 4.4 首版给 `[data-size="xs"]` 加了 `font-size: 11px`，会破坏 Unifont（位图字体只在整数倍字号下清晰）——已删除，并统一所有按钮字号为继承正文尺寸；同步删除 `flake.css` 的按钮字号覆盖，新增第 7 条护栏规则 `css-button-font-size`；另更正"`[data-size]` 此前无 CSS"的判断（`sm` 与 `icon*` 早已存在） |
| V1.11    | 2026-10-13 | Codex  | **修正按钮尺寸标准**：改为按各主题上游定标准——Minecraft 44px 刻意保留、shadcn 回到组件库默认（删掉我们的 `sm` 覆盖）、8bitcn 作为小众 registry 保持现状；推翻 4.4 的 `size="xs"` 决定，replace 10 处恢复标准按钮；`icon*` 宽高钉在 `--control-height` 以保证方正与主题内统一。另修复 flake 音符盒标签全部重叠：根因是 Radix Portal 使 `.flake-page …` 选择器永不匹配，新增第 8 条护栏规则 `css-portal-scope` |
| V1.12    | 2026-10-13 | Codex  | 完成 Phase 4.5 / 4.7 / 4.8：清理 65 处调用点遗留 `btn` 标记（新增第 9 条规则 `tsx-redundant-btn-class`），`BlockPickerDialog` 改用共享 `Dialog` 从而获得 Portal、焦点陷阱与 Esc 关闭，`UiTestPage` 补齐六种按钮变体用例；163 处裸 `<Button>` 的语义变体留待决策 |
| V1.13    | 2026-10-13 | Codex  | Minecraft 按钮变体贴图接线：按 `button_<状态>[_<变体>]_min.png` 命名，`default`→`primary`、`outline` 独立一套、`secondary` 用基础套、`destructive` 暂无；用"变体只提供支撑变量 + 状态规则回落"实现，避免优先级竞争与漏状态静默失败；新增 2 个贴图完整性测试并修掉 `token-undefined-reference` 误读注释的假阳性 |
| V1.14    | 2026-10-13 | Codex  | destructive 贴图接入（4 状态，disabled 刻意回落共享禁用贴图）；primary 常态文字改为深灰 #3f3f3f（悬停/按下仍黄），用 --ln-minecraft-button-normal-color 变量间接避免层叠竞争；UiTestPage 变体按钮改为点击循环 常态→按下→禁用 |
| V1.15    | 2026-10-13 | Codex  | Phase 5 部分完成：13 个 MC 样式文件合并为 widgets.css（15 → 3 个文件，81 条规则序列逐条校验一致），并入时发现并修复 40 条相对贴图路径失效；新增第 10 条护栏规则 css-unresolved-asset；裸元素选择器未能归零——页面中 51 个裸表单/表格元素仍依赖它们，需先做标记迁移 |
| V1.16    | 2026-10-13 | Codex  | Phase 5a 完成：51 个裸表单/表格元素全部迁移到共享 Table/Input/Textarea（枚举器 40、资产管理器 9、统计页 1、虚拟占位 1），	able.tsx 补上 ref 转发；裸元素现已为 0，5b（MC 裸元素选择器收窄）解锁。会改变那 4 个页面的表格/输入框观感，待人工确认 |
| V1.17    | 2026-10-13 | Codex  | **Phase 5 完成**：MC 主题 116 处裸元素选择器收窄到 [data-slot]（已有槽位的删裸名、无槽位的替换；input[type=button/submit/reset] 15 处确认无元素后删除），裸元素选择器 42 → 0、文件数 15 → 3，两条退出标准均达成；副作用是 Radix 按钮不再继承按钮贴图，符合预期 |
| V1.18    | 2026-10-13 | Codex  | 修复音符盒键盘回归：Phase 4.5 清 `btn` 的脚本把空白分隔符一并过滤，`is-sharp` 被粘成 `flake-page__note-key-buttonis-sharp`，两条 note-key 规则同时失效；全仓扫描确认仅此一处受损，已修复。护栏新增第 11 条规则 `tsx-glued-class-token` |
| V1.19    | 2026-10-13 | Codex  | 修复 Minecraft 复选框被裁成圆角：共享层 `controls.css:66` 多带一个 `button` 元素，使其成为 (0,2,2) 而恒定压住主题的 (0,2,1) 覆盖；删除该元素后 8bitcn/Minecraft 的 `border-radius: 0` 正常胜出，shadcn 的 4px 不变。新增第 8 个 MC 用例（先失败后通过） |
| V1.20    | 2026-10-13 | Codex  | **Phase 6 完成（退出标准已修正）**：原标准「页面外观属性为 0」不成立——188 条外观声明中 106 条是页面自有设计（挂在 `<article>`/`<section>` 上，无组件可继承）、15 条是无障碍/拖放轮廓、1 条是定位变换。改为「每条留存声明都须说明为何页面必须自己拥有」，据此删除 39 条无法匹配任何元素的孤儿规则（43 行 / 6 文件）。66 条零化补丁均为合法覆盖，全部保留。记录了把语句型 `@layer` 误判为空块并修复的过程 |
| V1.21    | 2026-10-18 | Codex  | **补齐按钮选中态**：核实 shadcn/ui 与 8bitcn/ui 的 Button 上游本就没有选中态（选中态属 Toggle），而 Nova 把 Button 当开关用（23 处 `aria-pressed`）却从未画出该状态——`button[aria-pressed="true"]` 是 (0,1,1)，被共享层 `secondary` 常态规则 (0,4,1) 恒定压过。改为共享层 (0,5,1) 规则 + `--nova-selected-*` 令牌（声明在 `tokens` 层，否则 8bitcn 的覆写会被层序压掉），新增 `button-selected-state.test.mjs` 8 个用例与级联求解器夹具 |
| V1.22 | 2026-10-18 | Codex  | **主题清单单一来源**：新增 `src/services/themeManifest.ts`，运行期预载、配置规范化、设置页选项与测试全部由它派生；`themeRuntime` 的 body class 清理改为通用 `theme-*`（退役主题不再需要名单）；新增清单自洽测试（目录存在、id/key 唯一、与 `src-tauri` 的 `normalize_theme` 一致）。新增主题从 10 处注册点降为 3 处，且漏改 Rust 会立刻测试失败而不是静默回落 shadcn |
| V1.23 | 2026-10-18 | Codex  | **组件子层与属性归属契约**：`components` 拆出 `components.shared < components.page < components.theme`，主题声明由层序保证生效，不再靠特异性提权；主题文件迁入 `components.theme`（`8bitcn/theme.css`、`minecraft/appearance.css`、`minecraft/widgets.css`）。顺带修掉两处违反“主题不改布局”的声明（MC 下拉触发器 `width: auto`、焦点环硬编码 `#ffffff`）。新增级联用例锁定“主题低特异性仍胜出” |
| V1.24 | 2026-10-18 | Codex  | **结构性容器不再冒充动作按钮**：`<Button>` 新增 `themeSlot`，投影库卡片/分类行/展开按钮改用 `library-tile` / `library-category` / `library-category-expander` 槽位。此前它们在子层下会被 MC 的 `[data-slot="button"]` 强制成 44px 九宫格按钮，`overflow: hidden` 把 140×140 预览图整块裁掉（表现为“不显示图标”）。同时把内嵌预览图固定为 140×140（`--library-preview-size`，px 值，不随主题根字号变化；网格列宽下限也用同一令牌），并删除 MC 主题里对 `.library-tile-image` 的宽度改写（主题不写布局） |
| V1.25 | 2026-10-18 | Codex  | **页面外观迁移第 1 批（home / render）**：home 的卡片改用共享 `.group-box`、次要文字改用共享 `.muted`，页面只留间距与流；render 的预览容器、占位/状态文字、进度条、操作区分隔线移入 `base.css` 并改用令牌（原 `#111/#222/#ccc/#888` 全部退场），页面只留尺寸与流。删除 MC 专用的 `#e0e0e0`（其特异性始终低于主题规则，从未生效）。页面外观声明 336 → 319，硬编码色基线 33 → 28 |
| V1.26 | 2026-10-18 | Codex  | **主题字阶令牌**：`--nova-text-xs/sm/base` 进入三主题 tokens——shadcn `.75/.875/1rem`，8bitcn 与 Minecraft 全部 `1rem`（位图字体只在整数字号下清晰，原 `.86rem` 在它们下是 13.76px）。home / render 的 `.86rem`、`.9rem`、`.9em` 全部改为 `var(--nova-text-sm)`。新增护栏 `css-page-font-size`（页面字号必须用字阶令牌或 `clamp()`，存量 39 处计入基线）与 `css-theme-type-token`（主题入口必须定义字阶，0 基线），并各配一条可证伪用例 |
| V1.27 | 2026-10-18 | Codex  | **页面外观迁移第 2 批（replace / properties）**：新增共享原语 `.nova-code` / `.nova-panel-subtle` / `.nova-box-plain` / `.nova-text-danger`，replace 的错误块、结果面板、规则 fieldset、pre 代码块与 properties 的字段集/错误/弱化文字全部改用它们；`BlockSearchSelect` 的属性 chip（`.replace-property*`）从页面样式改为共享组件样式（此前同一组件在两个宿主页面外观不同）。页面外观声明 319 → 298（replace 13 → 2、properties 28 → 18），页面字号存量 39 → 37，基线 203 → 201 |
| V1.28 | 2026-10-18 | Codex  | **小字号用专属位图字体**：Minecraft 的 `--nova-text-xs/sm` 定为 12px，并新增 `--nova-font-xs/sm` 指向仓库内 `fusion-pixel-12px-monospaced-zh_hans.ttf`（12px 设计字号）；Unifont 继续只用于 ≥16px 的正文。`replace-units.css` 里 34 处硬编码 `11/12/13px` 全部改为 `--nova-text-*` + `--nova-font-*` 配对。新增护栏 `css-text-scale-family`：用了小字号令牌就必须同时声明字族令牌（0 基线）。另把 replace 的「清空规则」「删除方案」改为 `destructive`、「打开方案文件夹」改为 `outline` |
| V1.29 | 2026-10-18 | Codex  | **8bitcn 同步 12px 字阶 + 清空页面字号存量**：8bitcn 也改用 Fusion Pixel 12px（新增同一 `@font-face`），并新增 `--nova-text-lg`（shadcn `1.5rem`；8bitcn / Minecraft `24px`，即 12px 的 2 倍，仍为像素清晰倍数）与配套 `--nova-font-lg`。7 个页面里剩余 **37 处**硬编码字号全部改为 `var(--nova-text-*)` + `var(--nova-font-*)`，`css-page-font-size` 基线清零（164 项总基线中不再有该规则），页面字号由主题统一决定 |
| V1.30 | 2026-10-18 | Codex  | **页面外观迁移第 3 批（generate / materials），改为自动拆分**：新增 `ui/styles/page-appearance.css`（`components.shared`），把两个页面样式表里的 71 条外观声明按原选择器、原 `@container` 上下文整体搬入；页面只剩布局与交互。拆分脚本按 `(选择器, 属性, 值)` 多重集校验前后一致。generate 外观 43 → **0**、materials 44 → **12**（余下为字号/字族/numeric 等排版属性），页面外观总量 298 → 264。顺带把 `.beta-material-table th/td` 收窄为 `[data-slot="table-head"/"table-cell"]`，裸元素基线 125 → 121，总基线 164 → 160 |
| V1.31 | 2026-10-18 | Codex  | **第 4 批（library / settings）+ 悬浮提示框统一**：材料列表的 `.material-list-hover-popup` 把字号/行高收进共享层一处（`var(--nova-text-base)`，即**正文大小**：Minecraft/8bitcn 16px、shadcn 14px，字族随正文），行内 label/name/ID/加粗数字全部继承同一字号；MC 主题不再单独写 `font-size: 16px` 与 `#999/#000/#fff`（改为 `border-width` + 令牌色）。library 外观 131 条、settings 48 条中的 131 条搬入 `page-appearance.css`；`settings-theme-swatch` 因刻意展示其他主题配色而保留在页面（拆分脚本带排除规则）。页面外观总量 264 → **131**，总基线仍为 160 |
| V1.32 | 2026-10-18 | Codex  | **第 5 批（properties）**：页面里最后一批非排版外观（`color`/`background`/`border`/`image-rendering` 等 12 条）搬入 `page-appearance.css`，properties 20 → **8**（余下为字号/字族/numeric）。页面外观总量 131 → **119**；其中非排版项只剩 home 的 1 条图标主色与 settings 的 17 条主题预览色块（刻意保留）。总基线维持 160 |
| V1.33 | 2026-10-19 | Codex  | **令牌化遮罩与中性色，清理可去掉的 `!important`**：新增 `--overlay`（三主题 `rgb(0 0 0 / 42%)`）替代 `dialog-overlay` 与 subwindow 遮罩的字面色；dialog 的 `rgb(127 127 127 / N%)` 改为 `color-mix(... var(--foreground) N% ...)`；MC 复选框内嵌高光同样改为 `color-mix`。删除两处已无竞争者的 `!important`（`.material-list-multiplier`、library 横向布局的 viewport `overflow`）。`css-hardcoded-colour` 27 → **20**（余下全是 flake 画布测量线/原点的数据可视化配色）、`css-important` 12 → **10**（余下均用于覆盖 Radix 内联样式或虚拟占位几何），总基线 160 → **151** |
| V1.34 | 2026-10-19 | Codex  | **裸元素第 1 批（页面表格）**：DOM 侦察确认 `tbody/thead/tr/td/th` 在 TSX 中**只以带 `data-slot` 的形式出现**，`button/input/textarea/select` 仍有不带槽位的用法（不能机械替换）。据此把 library 6 条、properties 5 条、materials 1 条表格选择器改为 `[data-slot="table-body"/"table-header"/"table-row"/"table-cell"/"table-head"]`，裸元素 121 → **110**，总基线 151 → **140** |
| V1.35     | 2026-10-20 | Codex  | **Metro10 接入当前主题契约**：取回的 Metro10 仍是裸元素 `widget/*.css`，注册前会被护栏当作 staged 资源跳过。现改为 `theme.css`（浅色/深色 Tier 1 令牌、字阶、`--flake-*` 资源、私有尺寸）+ `widgets.css`（`[data-slot]`、`components.theme`），删除 12 个 widget 文件与 `nav-icons.css`；同时登记 `THEMES`、Rust `normalize_theme` 与设置页色块，主题从三套变四套。新主题 CSS 在 `check:ui-styles` 下 0 违规，`library-theme-css` 的明暗对比检查扩展到 Metro10 |
| V1.36     | 2026-10-20 | Codex  | **功能图标主题化**：新增 `ui/components/ThemeIcon.tsx`，按活动主题解析 `ui/themes/<theme>/resource/icon/{button,tool}/<name>.{svg,png}`；有素材时覆盖，缺素材时回退到调用点传入的 lucide 图标，因此 Minecraft 等后续补图无需改业务代码。SVG 用 `mask + currentColor` 绘制，跟随按钮文字在浅/深色与各 variant 下变色；PNG 保留原图给像素主题。Home / Library / Properties / Render / Settings / Flake 的按钮与占位图标改用语义名（folder、gallery、import、run、empty…），`folder` 仅替换分类的默认 lucide Folder，保留用户自选图标。`functional-icons.test.mjs` 从“禁止主题位图”改为锁定新契约 |

本文件规定 Nova 前端的样式分层、组件契约与主题契约。组件来源与许可证见 [THIRD_PARTY_UI.md](THIRD_PARTY_UI.md)；开发与构建流程见 [DEVELOPMENT.md](DEVELOPMENT.md)；界面功能需求见 [design.md](design.md) 的 2.0 节。

**目标**：四主题（shadcn / 8bitcn / Minecraft / Metro10）共享同一棵组件树与同一套操作逻辑，切换主题只改外观，不改结构、不改交互、不复制页面。任何控件在任意主题下都只有一种外观来源。

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

即**项目自己的约 183 KB CSS 产物全部位于无层区**，没有任何一条规则受层序保护。该状态已由 Phase 1 消除（无层规则 1658 → 0，见 Phase 1 实施记录）；本节保留为迁移前的基线记录。

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

单一层序，六层；`components` 内部再分三个子层：

```
@layer theme, vendor, tokens, base, components, utilities;
@layer components.shared, components.page, components.theme;
```

| 层 | 职责 | 允许内容 | 禁止内容 |
| -- | ---- | -------- | -------- |
| `theme` | Tailwind 自带设计令牌 | `tailwindcss/theme.css` | — |
| `vendor` | Tailwind 生成的工具类 | `tailwindcss/utilities.css` | 项目自己的规则 |
| `tokens` | 三主题的变量定义 | 仅自定义属性声明 | 任何选择器规则 |
| `base` | 重置与元素默认值 | `box-sizing`、`body`、`font`、`img`、`table` 折叠 | 控件外观、品牌色 |
| `components.shared` | 共享控件外观与默认值、shell 骨架 | `[data-slot]` 组合选择器、`.btn` 等共享类 | 页面专属布局 |
| `components.page` | 页面布局：文档位置、分区、流动 | 页面容器类（`.library-*` 等）的 `display`/`flex`/`grid`/`gap` | 控件外观（颜色、描边、圆角、阴影） |
| `components.theme` | 主题外观，**声明即生效** | `[data-slot]` 贴图与配色、Tier 3 令牌 | 页面位置（`position`/`order`/`float`/`grid-template`） |
| `utilities` | 保留（当前为空） | — | — |

**子层的四条契约（V1.23）**，也是"主题 / 共享 / 全局"三类优先级的判定依据：

1. **主题不改布局位置。** 分区左右、元素顺序、页面级定位属于共享层；主题只声明外观。九宫格贴图已经把圆角、描边、颜色画进图里，主题可以声明 `border: 0` / `border-radius: 0` 来"接管"这些属性，但不能移动元素。
2. **页面布局只规定文档位置，尺寸靠内容流动。** 页面 CSS 不应给控件写死宽高；硬性尺寸只用于确有像素对齐要求的场合（如 140×140 预览图）。
3. **布局之下部件是单调的。** 同一控件在页面任何位置都用同一套尺寸与外观；页面若需要不同尺寸，应换一个 `size`/变体或专用类，而不是就地覆写。
4. **主题声明即生效。** 主题规则位于 `components.theme`，其父层 `components` 里的共享与页面规则一律排在其下；因此主题不需要靠提高特异性来"抢"外观，新增主题也不必研究共享层的选择器权重。

> 由第 4 条，`html[data-theme="x"]` 前缀在主题文件里不再是必需（主题表按 `media` 互斥加载），它的历史作用——压过共享层——已由子层承担。保留的前缀只用于同一主题表内的条件表达。

> **为什么 Tailwind 在 `vendor` 而不是 `utilities`。** 原方案把 Tailwind 工具类与页面布局类放在同一层（`utilities` 最高）。实施时发现这是错的：Tailwind 工具类今天位于 `@layer utilities`，而项目 CSS 全部无层，所以**项目 CSS 一直赢过 Tailwind**。若让它们同层且置于最高，`ui/components/ui/button.tsx` 基础串里的 `bg-primary` 会开始生效，**所有按钮都会变成主色**。把 Tailwind 移到最低的 `vendor` 层，才与"项目 CSS 压过 Tailwind"这一既有事实一致。

> **`utilities` 高于 `components`，因此页面 CSS 会压过主题外观。** 原方案的表述（"设置 `background` 会被 `components` 压过"）与本层序相反。这直接影响 Phase 1 的取舍：把页面 CSS 现在就抬进 `utilities`，会让页面的默认值盖掉主题值（实测 **2,951 处**获胜者反转，例如 Minecraft 主题下 `html[data-theme="minecraft"] .library-page` 的 `--library-*` 令牌会被 `library.css` 的默认值取代）。因此 Phase 1 只做层化、把组件与页面规则暂留同一层，**页面 CSS 的迁入与外观属性清理留给 Phase 6 一并处理**。

**层顺序在每个文件里重复声明。** 这不是冗余，而是必需：层序由文档中**首次**出现的声明确定，而开发模式下 `ui/app/main.tsx` 先 import `AppShell`（它又 import `ui/styles/enumerator.css`）、之后才 import `ui/styles/base.css`，模块求值顺序与打包后 `<link>` 顺序并不一致。每个文件都写同一行，层序就与加载顺序无关。36 个项目 CSS 文件均已写入；使用组件子层的 31 个文件还各自重复了一行子层顺序声明。

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
| `--control-shadow` | 控件阴影（8bitcn 像素投影） | ✅ Phase 2A 已补 Minecraft `none` |
| `--panel-shadow` | 浮层阴影 | ✅ Phase 2A 已补 Minecraft `none` |
| `--success` `--warning` | 状态色 | ✅ Phase 2A 已补三主题 |
| `--control-height` | 控件基准高度（32 / 36 / 44px） | ✅ Phase 2B 已由 `--ln-control-height` 提升 |
| `--option-active-background` `--option-active-color` | 列表项激活态 | ✅ Phase 2B 已由 `--ln-option-*` 提升（与 `--accent` 语义不同，必须独立） |
| `--navigation-background` | 导航/侧栏表面 | ✅ Phase 2B 新增：shadcn/8bitcn 取 `--card`、Minecraft 取 `--background`，是真实的独立概念（见 Phase 2B 记录） |

**Tier 2 结构令牌** —— 跨主题稳定，随控件尺寸而非随主题变化。

`--ln-scroll-size` `--ln-scroll-radius` `--ln-scroll-track` `--ln-scroll-track-hover` `--ln-scroll-thumb` `--ln-scroll-hover` `--ln-scroll-active` `--ln-scroll-edge` `--ln-scroll-shadow` `--ln-file-icon-size` `--ln-body-font-family` `--ln-body-font-size` `--ln-check-size` `--ln-check-label-gap` `--ln-check-label-min-height`

**Tier 3 主题私有令牌** —— 仅允许被所属主题目录内的文件引用，跨主题引用视为错误。

`--ln-minecraft-button-image` `--ln-minecraft-button-height` `--ln-minecraft-button-border` `--ln-minecraft-input-*` `--ln-minecraft-table-row-*` `--ln-minecraft-nav-*` `--ln-minecraft-range-*` `--ln-minecraft-card-border-color`（拟由 `--ln-card-border-color` 改名） `--ln-table-row-selected-background` `--ln-card-border-width` `--ln-card-background-color-active` `--ln-card-border-color-active`

**纯桥接、应删除**（值为另一令牌的直接别名，无独立语义）：
`--ln-body-text-color` `--ln-muted-text-color` `--ln-control-radius` `--ln-focus-color` `--ln-popup-shadow` `--ln-error-color`（均已于 Phase 2B 删除）

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

**层序声明在每个文件里重复，而不是只在 `base.css` 与主题文件里。** 层序由文档中首次出现的声明确定；开发模式下 `main.tsx` 先 import `AppShell`（进而 import `enumerator.css`），之后才 import `base.css`，模块求值顺序与打包后 `<link>` 顺序不一致。若只有 `base.css` 声明，先被求值的文件里的 `@layer components { … }` 会抢先确立层序，把 `components` 定在最低位。48 个项目 CSS 文件全部写入同一行后，层序与加载顺序无关（Phase 1 已落地）。

**Minecraft 主题必须加载 Tailwind 层。** 现状 `themes/minecraft/theme.css` 不 import `modern.css`，导致它依赖"无层规则压过一切"才能工作。改为加载后，它与其他两个主题走同一条路径；同时须补齐 `--edge-width: 0`、`--control-shadow: none`、`--panel-shadow: none`。注意 Phase 1 已把 Tailwind 放在最低的 `vendor` 层，因此这一步不会让 shadcn 组件身上的外观工具类在 Minecraft 主题下突然生效。

**共享结构放 `base` 层，共享行为放各主题 `components` 层。** 现状 `styles/controls.css` 与 `styles/widgets.css` 的正确部分（焦点环、图标尺寸、滚动条命中区、下拉浮层定位、`table-row-selected` 语义）应进入 `base`；与颜色/描边相关的部分进入各主题 `components`。

#### 3.4.1 主题注册契约（V1.22）

一个主题的全部身份信息只存在于 `desktop-nova/src/services/themeManifest.ts` 的 `THEMES` 数组：

| 字段 | 含义 | 消费者 |
| ---- | ---- | ------ |
| `id` | 写进配置文件的规范 id（大小写敏感，`Minecraft` 是历史值） | 配置读写、设置页 `value`、Rust `normalize_theme` |
| `key` | 小写资源键，用于 `data-theme` 与 `theme-<key>` body class | `themeRuntime`、`NavIcon` |
| `label` | 设置页显示名 | `SettingsPage` |
| `directory` | `ui/themes/<directory>/theme.css` 的目录名 | 运行期预载、清单自洽测试 |

**新增一个主题只改三处**：`THEMES` 加一条、建 `ui/themes/<id>/theme.css`、在 `src-tauri/src/main.rs` 的 `normalize_theme` 里加一个返回该 id 的分支。第三处漏改会让 `themes.test.mjs` 的「与 Rust 规范化一致」用例失败——这是刻意的：后端读取配置时会重写主题值，漏改的后果是**静默回落 shadcn**，而不是报错。

**不再允许**在 `themeRuntime`、`userConfig`、`SettingsPage` 里硬编码主题名；默认值统一取清单的 `DEFAULT_THEME_ID`。body class 清理也改为按 `theme-` 前缀通用处理，退役主题（WebDefault / Metro10 等）的残留 class 无需再登记。

#### 3.4.2 页面外观迁移规则（V1.25 起）

页面 CSS 每出现一条外观声明，按以下顺序决定归属：

| 顺序 | 判定 | 做法 |
| ---- | ---- | ---- |
| 1 | 已有共享语义类（`.muted` 弱化文字、`.group-box` 卡片面） | 标记加到元素上，删除页面声明 |
| 2 | 可复用的外观但还没有类 | 在 `components.shared` 定义类或 `[data-slot]` 规则；页面只留布局 |
| 3 | 只属于某个主题（贴图、专属配色、字体） | 写进该主题的 `components.theme` 文件 |
| 4 | 纯几何：`display` / `flex` / `grid` / `gap` / `order` / `position` / 尺寸上限 / 断点 | **留在页面**，这是页面唯一该拥有的东西 |

迁移时**先等价搬移**（值不变、只换层），再单独讨论是否改值；这样视觉回归只会来自“主题本来就想覆盖它”的部分。每批迁移后用 `check:ui-styles` 的硬编码色基线验证，用 `tsc` + 页面测试兜底。

**字号不写在页面，也不手挑数值。** 主题通过 `tokens` 层的 `--nova-text-xs` / `--nova-text-sm` / `--nova-text-base` 给出自己的字阶：

| 主题 | `--nova-text-xs` | `--nova-text-sm` | `--nova-text-base` | `--nova-text-lg` | 说明 |
| ---- | ---------------- | ---------------- | ------------------ | ---------------- | ---- |
| shadcn | `.75rem` | `.875rem` | `1rem` | `1.5rem` | 比例字体，可按 14px 根字号缩放 |
| 8bitcn | `12px` | `12px` | `1rem` | `24px` | 小字与标题用 Fusion Pixel 12px（24px = 2 倍设计字号） |
| Minecraft | `12px` | `12px` | `1rem` | `24px` | 同上；Unifont 只用于 16px 正文 |

页面只写 `font-size: var(--nova-text-sm)` 与 `font-family: var(--nova-font-sm, inherit)`；展示型标题可以用 `clamp()`。三条护栏守住这个约定：`css-page-font-size` 拦截页面里的 `rem/em/px` 字号（**当前 0 处**，全部由主题字阶提供），`css-text-scale-family` 要求小字号令牌必须与字族令牌配对（否则位图主题会退化成缩放 Unifont），`css-theme-type-token` 要求每个主题入口都定义 xs/sm/base/lg 四档（当前 0 违规，新主题漏定义会直接报错）。

已迁移批次：`home`（20 → 12）、`render`（13 → 2）、`replace`（13 → 2）、`properties`（20 → 8）、`generate`（43 → 0）、`materials`（44 → 12）、`library`（131 → 0）、`settings`（48 → 22，其中主题预览色块刻意保留）。第 3 批起改用自动拆分：脚本把 `components.page` 里的外观声明按原选择器与 `@container` 上下文搬进 `ui/styles/page-appearance.css`（`components.shared`），并校验 `(选择器, 属性, 值)` 多重集前后一致；页面只保留布局与交互。共享原语 `.nova-code`、`.nova-panel-subtle`、`.nova-box-plain`、`.nova-text-danger` 与 `.replace-property*` 已就位，后续页面优先组合它们。

#### 3.4.3 主题功能图标（V1.36）

页面不再把 lucide 图标当作最终外观，而是通过 `ui/components/ThemeIcon.tsx` 给主题一个覆盖点：

```
ui/themes/<theme>/resource/icon/button/<name>.{svg,png}   ← 按钮、占位图、设置选项
ui/themes/<theme>/resource/icon/tool/<name>.{svg,png}     ← 分层编辑工具
```

- 有素材时使用主题素材；没有时回退到调用点传入的 `fallback` lucide 图标，**缺图不是错误**。后续补 `save`、`undo` 或整套 Minecraft PNG 不需要改业务代码。
- **SVG 用 `mask + currentColor` 绘制**，因此图标跟随按钮文字颜色：浅/深色、hover、primary/destructive 填充都会同步变化。不要把主题 SVG 当 `<img>` 用，`currentColor` 在独立图片文档里会固定成黑色。
- **PNG 保留原图**，供 Minecraft 等像素主题使用；`theme-icon` 的背景色只作用于 SVG 遮罩，不会给 PNG 透明区染色。
- 语义名按功能区固定（`folder`、`gallery`、`import`、`download`、`delete`、`all`、`unknown`、`picture_error`、`empty`、`run`、`build`、`close`、`hide`、`save`、`undo`、`light`、`dark`、`system`、`hand`…`text`）。分类的 `folder` 只替换默认 lucide `Folder`，用户自选的分类图标保持不变。
- `functional-icons.test.mjs` 锁定“页面使用语义名、回退图标有效、SVG 走遮罩、PNG 不用遮罩”四条契约。

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

### Phase 1 — 层化（不改视觉）✅ 已完成

**做**：
1. 在 `ui/styles/base.css` 与 3 个 `ui/themes/*/theme.css`（`shadcn` `8bitcn` `minecraft`）顶部加同一行 `@layer theme, tokens, base, components, utilities;`。
2. 把 `ui/styles/`、`ui/shell/widget/`、`ui/themes/`、页面 CSS 的现有规则**原样**包进对应层。此阶段只搬位置，不改内容。
3. 把现有 `[data-slot]` 规则与主题覆写放入 `components`；令牌声明放入 `tokens`；页面布局类放入 `utilities`。

**退出**：`dist/assets/*.css` 中无层规则数为 0（除 `theme` 层的 Tailwind 声明）；三主题切换截图与迁移前逐像素一致。

**风险**：高。层化会改变现有覆盖关系（原本无层互压，现在按层序）。**必须**在本阶段保留"最后写入者胜"的等效结果——必要时用选择器特异性补偿，而不是重新引入无层规则。建议本阶段单独一个提交，便于二分定位。

---

#### 实施记录

**先量后改。** 直接按原方案切四层是不可接受的，所以先写了分析工具 `.tmp/analyze-cascade-flips.mjs`：它从 TSX 采集 `className`、`id`、`data-slot` 与元素标签的对应关系，建立元素模型，再对每一对"能匹配同一元素且设置同一属性"的跨层规则，判断今天谁赢、分层后谁赢。要点：

- 三个主题样式表在运行时互斥（`themeRuntime.ts` 切换 `media="not all"`），跨主题冲突不可能发生，必须排除，否则绝大多数"冲突"是假的；
- `themes/modern.css` 只被 shadcn / 8bitcn 引入，Minecraft 从不加载它；
- 没有元素模型时误报率极高（`label:has(> input[type=checkbox])` 会被拿去和 `.asset-manager-header` 比较），加入模型后报告从 4.4 万条降到可信量级。

**量到的结论**：原方案的四层切分会产生 **2,951 处真实获胜者反转**（`components` 今天赢、分层后 `utilities` 赢）。反向层序更糟（20,353 处，且集中在 `display` / `gap` 等布局属性上）。`tokens` 与 `base` 两层则是 **0 反转**，可以安全切出。

**据此调整实施范围**：Phase 1 只切出 `tokens`（23 个令牌块）与 `base`（7 条重置规则），其余 **1,628 条规则全部暂留 `components` 一层**。单层内部特异性与顺序完全不变，因此层化是**可证明的等效变换**；`utilities` 层已声明但为空，等 Phase 6 连同页面外观属性清理一起迁入。

**改动**

| 项 | 内容 |
| -- | ---- |
| 48 个 CSS 文件 | 每个文件加层序声明，并把规则按连续同层片段包进 `@layer` 块；共 71 个层块 |
| `ui/themes/modern.css` | `@import "tailwindcss/utilities.css"` 的层名由 `utilities` 改为 `vendor`，把 Tailwind 工具类压到项目层之下 |
| `desktop-nova/scripts/check-ui-styles.mjs` | 期望层序常量加入 `vendor` |
| `desktop-nova/scripts/ui-styles-baseline.json` | 刷新基线（`css-unlayered-rule` 1658 → 0；`css-components-bare-element-selector` 0 → 176） |

**等价性验证**（三重）

1. **规则集不变**：变换脚本对每个文件重新解析前后两份 CSS，比较"选择器 + 全部声明"的多重集，必须完全一致——48/48 通过。
2. **字节全覆盖**：按原文节点偏移切片重组，断言切片覆盖原始文件全部字节——48/48 通过。
3. **行不丢失**：对每个文件比较改动前后的非空行多重集，新增行只允许是 `@layer` 包装行——48/48 通过（`git diff` 显示的 13 行"删除"是行对齐噪声）。

**层叠验证**：分析工具复测 **0 处获胜者反转**（含/不含元素模型均为 0）。

**产物验证**：`base-*.css` 是 `dist/index.html` 中的第一个样式表，确立了全局层序；实测 `.bg-primary` 落在 `vendor` 层，**没有**升到 `project` 层之上，因此不会出现"所有按钮变主色"。

**退出验证**

| 检查 | 结果 |
| ---- | ---- |
| `check:ui-styles` | ✅ 无层规则 **0**；层序识别为 `theme, vendor, tokens, base, components, utilities`；384 项已知，0 回归 |
| `check:layers` / `check:application-boundaries` | ✅ 0 违规 |
| `npm run build` | ✅ 通过 |
| 测试套件 | ✅ 与 HEAD **完全一致**（`check-ui-styles` 8/0、`virtual-spacer` 3/0、`layer-check` 2/0、`application-boundaries` 7/0、`library-theme-css` 4/0、`choice-controls` 6/0、`beta021-ui` 10/0；`functional-icons` 0/3、`widgets` 7/4、`themes` 12/2、`materials` 0/1、`restored-entrypoints` 4/2 均为**改动前既存**失败，已在 HEAD 逐一核对） |

**待人工验收**：三主题 × 明暗共 9 组的视觉对照。自动化只能证明"计算值层面的获胜者未变"，不能替代像素级确认；分析工具已经把需要人眼关注的范围收敛到 0 处已知反转，但层序本身是全局机制，仍建议按第 5 节矩阵过一遍。

**未做**：页面 CSS 未迁入 `utilities`（见上）。这是相对原方案第 3 条的**有意收窄**，理由与实测数据见 3.1 节与本节实施记录；Phase 6 会连同"删除页面外观属性"一起完成，届时那 2,951 处反转将由逐文件的清理显式处理，而不是靠层序偶然决定。

### Phase 2 — 令牌收敛

拆成 2A（已完成）与 2B（待办）两步：2A 只做"补令牌 + 消灭系统色"，改动集中在 `styles/` 与 `shell/widget/`；2B 删除桥接令牌与硬编码色值，会触及全部 48 个文件，故分开以保持回滚粒度。

**做**：
1. 补齐 `--edge-width`、`--control-shadow`、`--panel-shadow` 到 Minecraft；新增 `--success`、`--warning` 到三主题。
2. 处理 18 个"被引用但未定义"的令牌（见 1.7）：逐个判定是"应存在的语义令牌"还是"早期遗留的名称"。前者（如 `--ln-ui-edge-color`、`--ln-ui-control-background-color`、`--ln-inset-background`、`--ln-divider-color`）归入 Tier 1/Tier 2 并补齐三主题定义；后者（如 `--ln-primary`、`--ln-card-radius`）直接改为对应语义令牌，删除引用。**此步骤必须在 Phase 1 之后做**，因为无层区下无法判断某个 fallback 是否真的在生效。
3. 把 `--ln-control-height`、`--ln-option-active-*` 升级为 Tier 1 语义令牌，旧名保留为别名一个版本。
4. 删除纯桥接令牌，把引用直接指向语义令牌。
5. 把 215 处硬编码色值改为令牌引用；确属主题特有的（如 `--ln-minecraft-*` 贴图路径）保留，但登记为 Tier 3。
6. 删除只定义不引用的死令牌（`--ln-form-max`）。

**退出**：Tier 1 令牌在三主题中覆盖率为 100%；"被引用但未定义"的令牌数为 0；`var(--ln-*)` 引用数从 561 降至仅 Tier 2 / Tier 3 的量；硬编码色值降至仅 Tier 3 定义处。

---

#### Phase 2A 实施记录 ✅ 已完成（第 1、2、5 步）

**范围**：补 5 个缺口令牌 + 消灭全部系统色依赖（含死令牌引用）。**不做**桥接令牌删除与硬编码色值替换（留给 2B）。

**先摸清解析链。** 把每个 `var()` 兜底按链条解开后发现，240 处系统色分三类，处理方式与验收预期都不同：

| 类别 | 数量 | 处理 | 视觉影响 |
| ---- | ---- | ---- | -------- |
| 死令牌，兜底落到真系统色 | ~90 | 换成语义令牌 | **变色**（正是要修的 bug） |
| 死令牌，兜底已解析到已定义令牌 | ~20 | 换成该语义令牌 | 中性 |
| 已定义令牌的系统色兜底（不可达） | 129 | 删掉兜底 | 中性 |

另外 47 处是**裸写**系统色（不经 `var()`），以及 `--flake-*` / `--material-list-virtual-spacer-height` 这类**由 TypeScript 运行时设置**的属性（不是死令牌，是护栏的误报）。

**改动**（20 个文件、241 条声明）

| 项 | 内容 |
| -- | ---- |
| 新增 Tier 1 令牌 | `--success` / `--warning`（5 处）、Minecraft 的 `--edge-width: 0` / `--control-shadow: none` / `--panel-shadow: none`；`--ln-success-color` / `--ln-warning-color` 反转为引用它们 |
| 死令牌映射 | 21 个名字 → 语义令牌。`--ln-ui-edge-color` → `--border`（按要求改名）；`--ln-border-color`/`--ln-divider-color`/`--ln-fieldset-border-color` → `--border`；`--ln-ui-control-background-color`/`--ln-inset-background` → `--background`；`--ln-surface-background-color`/`--surface-elevated` → `--card`；`--ln-accent-color` → `--primary` 等 |
| 系统色映射 | `ThreeDShadow` → `--border`、`GrayText` → `--muted-foreground`、`Highlight`/`HighlightText` → `--primary`/`--primary-foreground`、`ButtonFace` → `--muted`、`Canvas` → `--background` 等 |
| **保留** | `@media (forced-colors: active)` 块（`widgets.css`）—— 那里用系统色才是正确行为，与 `prefers-reduced-motion` 的 `!important` 同理 |
| 顺带删除 | `.flake-page__tooltip*` 4 条死规则（TSX 中 0 引用，真实 tooltip 用 `material-list-hover-popup*`） |

**结果**

| 指标 | 前 | 后 |
| ---- | -- | -- |
| 系统色提及 | 240 | **4**（2 处 forced-colors + 2 处在注释里） |
| `token-undefined-reference` | 73 | **0** |
| `css-system-colour`（新护栏规则，排除 forced-colors） | — | **0** |

**护栏同步加强**（这三处都是 Phase 2A 暴露出来的盲区）

1. `token-undefined-reference` 从只查 `--ln-*` 扩到**所有自定义属性** —— 正是这个盲区让 `--surface-elevated`、`--text`、`--popover-foreground` 三个死名字溜过了 Phase 1 的护栏。同时豁免两类合法缺失：TS 里以字符串字面量出现（运行时设置）、以及 `--radix-*`（Radix 在内容元素上发布）。
2. 新增 `css-system-colour` 规则，排除 `forced-colors` 块。
3. 新增 `css-vendor-layer-above-components` 规则，见下。

用例从 8 个增至 **11 个**。

#### 一个必须记下的教训：`layer(vendor)` 曾被静默还原

Phase 1 期间我把 `modern.css` 的 Tailwind 导入从 `layer(utilities)` 改成 `layer(vendor)` 并验证过。之后为修另一个 bug 执行了 `git checkout -- desktop-nova/ui`，**这次还原把该修改一起回退了**，而我没有在最终状态上重跑"构建产物里 `.bg-primary` 落在哪一层"的检查就报告完成。

后果是真实的：`utilities` 在层序中是第 6 位、`components` 是第 5 位，于是 Tailwind 的外观工具类**压在项目 CSS 之上**——`button.tsx` 基础串里的 `bg-primary` 会让所有按钮变成主色。护栏与层叠分析工具都没发现它，因为**两者都只读项目 CSS**，而 Tailwind 工具类是构建期生成的。

修复后补了两道保险：

- `modern.css` 的注释写明为什么必须是 `vendor`；
- 新增护栏规则 `css-vendor-layer-above-components`：任何 `@import ... layer(X)` 只要 X 的层序位置 ≥ `components` 就报错。已用夹具验证它会失败（`layer(utilities)` 报错、`layer(vendor)` 通过）。

同时把"构建产物中 `.bg-primary` 所在层"补进第 5 节回归矩阵，作为每阶段必跑的检查——这是唯一能看到 vendor CSS 位置的检查。

**Phase 2A 退出验证**

| 检查 | 结果 |
| ---- | ---- |
| `check:ui-styles` | ✅ `token-undefined-reference` 0、`css-system-colour` 0、无层规则 0；0 回归 |
| `check:layers` / `check:application-boundaries` | ✅ 0 违规 |
| `npm run build` | ✅ 通过；产物中 `.bg-primary` 位于 `vendor` 层 |
| 层叠反转 | ✅ 0（改动只涉及声明取值，不涉及选择器与层序） |
| 测试套件 | ✅ 无新增失败（`widgets` 7/4、`themes` 12/2、`materials` 0/1、`functional-icons` 0/3、`restored-entrypoints` 4/2 均与 HEAD 一致） |

**待人工验收**：本次改动**会改变渲染结果**——约 90 处原本落到 Windows 系统色（灰边、系统蓝高亮、系统灰文字）的位置，现在跟随主题令牌。这正是要修的缺陷，但必须按第 5 节矩阵做三主题 × 明暗的视觉对照，重点看：资产管理器与枚举器的面板边框、属性页的预览画布底色、replacement 页的按钮与工具提示、flake 页的标记菜单高亮。

**2B 待办**：删除 10 个桥接令牌（225 处引用）、替换 215 处硬编码色值、删除死令牌 `--ln-form-max`、把 `--ln-success-color`/`--ln-warning-color` 的引用直接指向 `--success`/`--warning`。

---

#### Phase 2B 实施记录 ✅ 已完成（第 3、4、5、6 步）

**先分清两件事。** "215 处硬编码色值"这个数字把两类完全不同的东西混在一起：

| 类别 | 数量 | 处置 |
| ---- | ---- | ---- |
| **调色板定义**（自定义属性 = 字面色值） | 157 | **保留** —— 这里就是主题色的源头，字面值是正确的 |
| 样式声明里的字面色值 | 96 → 清理后 33 | 部分映射到令牌 |

那 96 处里还有两个**必须保留**的：`ui/themes/**` 的 19 处（主题文件本身就是调色板，Tier 3），以及 `settings.css` 的 11 处 —— 那是**主题预览色块**，它展示的是*其他*主题的颜色，一旦解析成当前主题就失去意义了。加上使用 `color-mix(… var(--muted) …)` 这类**已经基于令牌**的写法被我的检测器误报，真正可动的只有 54 处。

**桥接令牌退场**（29 个文件、191 处引用）

按"引用改名、定义按类型处理"的规则执行：值为 `var(目标)` 的别名定义直接删除（28 条），值为字面量的定义改名为目标名保留（10 条，保住主题加载前的兜底）。

| 处置 | 令牌 | 目标 |
| ---- | ---- | ---- |
| **提升为 Tier 1**（含义是新的，不是别名） | `--ln-control-height` → `--control-height`；`--ln-option-active-background/color` → `--option-active-*` | 保留定义，全局改名 |
| **同上，方案修正** | `--ln-navigation-background-color` → `--navigation-background` | 见下 |
| 退场为语义令牌 | `--ln-ui-background-color` → `--background`；`--ln-body-text-color` → `--foreground`；`--ln-muted-text-color` → `--muted-foreground`；`--ln-control-radius` → `--radius`；`--ln-focus-color` → `--ring`；`--ln-popup-shadow` → `--panel-shadow`；`--ln-error-color` → `--destructive`；`--ln-card-border-color` → `--border`；`--ln-success-color` → `--success`；`--ln-warning-color` → `--warning` | 引用改名；别名定义删除 |
| 删除 | `--ln-form-max` | 定义了但无人引用 |

> **方案修正：`--ln-navigation-background-color` 不是纯桥接。** 附录 A 写的是"删除 → `--card` / `--background`"（当时就给了两个目标，说明当时没判定清楚）。实测它按主题取不同的值：shadcn/8bitcn 是 `var(--card)`，Minecraft 是 `var(--background)`。删成任一个都会改变另一侧的外观。它是真实的独立概念（**导航/侧栏表面**），因此**提升为 Tier 1 的 `--navigation-background`**，三主题各自定义。这是 §3.2 "语义不同就必须独立"的又一例。

**状态色与阴影**（6 个文件、21 处）

| 字面值 | 替换为 |
| ------ | ------ |
| `#ff6666`、`#ffb3b3` | `var(--destructive)` |
| `rgb(255 0 0 / 10%\|18%)`、`rgba(255,0,0,.1)` | `color-mix(in srgb, var(--destructive) N%, transparent)` |
| `#8a6a22`、`#ffcc66`、`#d97706` | `var(--warning)` |
| `rgb(255 180 0 / 10%)` | `color-mix(in srgb, var(--warning) 10%, transparent)` |
| 三处弹层 `box-shadow` | `var(--panel-shadow)` |

用 `color-mix` 而非固定 RGB 三元组，是为了让透明度跟着主题色走。Minecraft 的 `--panel-shadow: none`，所以它的弹层不再有阴影——与贴图风格一致。

**剩余 33 处（已记入基线，不再增长）**

| 文件 | 数量 | 性质 |
| ---- | ---- | ---- |
| `styles/flake.css` | 20 | **画布覆盖层配色**：测量线蓝、原点标记橙。属于"图表数据色"性质，是否可主题化是独立决定 |
| `shell/widget/dialog.css` | 5 | 遮罩底色 `rgb(0 0 0 / 42%)` 与中性微调 `rgb(127 127 127 / N%)`；前者缺一个 `--overlay` 之类的语义令牌 |
| `styles/base.css` | 4 | JSON 预览代码块的深色终端配色（`#111`/`#222`/`#ccc`/`#888`） |
| `shell/widget/subwindow.css` | 2 | 遮罩底色 + `rgb(255 255 255 / 2%)` 微高光 |
| `styles/controls.css` | 1 | Minecraft 控件的内嵌斜面高光 |
| `windows/main/pages/home/home.css` | 1 | `#e0e0e0` |

这些都需要"要不要为覆盖层/代码块引入新语义令牌"的设计决定，不属于机械收敛，因此**留作独立议题**，但已由新护栏规则纳入基线，不会继续增长。

**护栏新增第 4 条规则：`css-hardcoded-colour`**

检测样式声明里的字面色值（不含 `var()`），并**在规则内**排除 `ui/themes/**` 与 `settings/settings.css`——两者的字面值都是刻意的，放进基线反而不如直接豁免诚实。用例 12 个。

**顺带修正一处过期断言**：`library-theme-css.test.mjs` 断言的 `var(--ln-option-active-background` / `var(--ln-focus-color` 是旧名，已更新为 `--option-active-background` / `--ring`。这是**测试断言过期**，不是行为回归。

**Phase 2B 退出验证**

| 检查 | 结果 |
| ---- | ---- |
| 桥接令牌 | ✅ 14 个名字全部退场，无残留引用（`--ln-card-border-color-active` 是 Tier 3 的另一个令牌，按设计保留） |
| `check:ui-styles` | ✅ 无层规则 0、未定义令牌 0、系统色 0、vendor 层位正确；0 回归 |
| `check:layers` / `check:application-boundaries` | ✅ 0 违规 |
| `npm run build` | ✅ 通过；产物中 `.bg-primary` 位于 `vendor` 层 |
| 层叠反转 | ✅ 0 |
| 测试套件 | ✅ 与 HEAD 完全一致（仅修正 2 处过期断言） |

**待人工验收**：本次同样**改变渲染结果**。重点：错误/警告态的颜色与边框（原为固定霓虹红 `#ff6666` / 暖黄）、三处弹层阴影改为统一面板阴影、Minecraft 主题下弹层阴影消失。

### Phase 3 — 组件外观类剥离

**做**：从 `ui/components/ui/` 的 5 个含 Tailwind 外观类的文件（`button` `select` `dialog` `table` `input`）中移除颜色/描边/圆角/阴影类，把这些外观写入 `ui/themes/shadcn/theme.css` 的 `components` 层。`cn()` 保留用于合并业务传入的结构类。

**退出**：`ui/components/ui/` 中外观工具类为 0；三主题下控件外观与迁移前一致（该主题本来就是靠覆写生效的，因此应有极小差异，需逐项确认）。

**风险**：中。8bitcn 的 `ButtonDecorations` 依赖 Tailwind 定位类（`absolute` `-top-0.5` `h-0.5` `w-1/2`），属于结构类，保留。

---

#### Phase 3 实施记录 ✅ 已完成

**先判断每个类是否还在生效。** 这一步不能靠"看起来像外观类就删"：Phase 1 把 Tailwind 放进 `vendor` 层（排在 `components` 之下）之后，**绝大多数外观类其实已经失效**，但**少数属性只有 Tailwind 在设**，删掉就会真的改变外观。于是先写了一个分类工具（`.tmp/recon-phase3.mjs`），对每个类回答"项目 CSS 是否已经在设同一个属性"：

| 判定 | 含义 | 处置 |
| ---- | ---- | ---- |
| **DEAD** | `components` 层已有规则设置该属性 | 直接删类，视觉中性 |
| **LIVE** | 没有别的来源，Tailwind 是唯一出处 | 必须写进 `components` 层 |

结果：**58 个 DEAD / 44 个 LIVE**（44 个实例对应约 15 条规则）。

> **工具本身错了四次，每次都被实测纠正**，值得记下来：① 只看 `[data-slot]` 规则，漏了元素自己的结构类（`.input` `.dropdown-trigger`）；② 变体工具类（`disabled:opacity-50`）在产物里是转义过的类名，匹配不上；③ **简写 vs 长写**——项目写 `border: 0`，Tailwind 写 `border-style`/`border-width`，不展开简写就会把一大片误判成 LIVE（78 → 45）；④ 元素选择器要算数（对话框标题是 `<h3>`，`line-height` 来自 `:where(h1,h2,h3,h4)`），但**必须限定"无顶层组合器"**——否则 `.settings-theme-swatch > span` 的背景色会被拿去"覆盖"按钮里的 span（44 → 20，且修掉了一个假阳性）。
>
> 工具仍有它看不到的东西：它不懂 DOM。`<thead>` 里的 `tr` 身上带着 `data-slot="table-row"`，已经被 `[data-slot="table-row"] { border: 0 }` 归零，所以 `[&_tr]:border-b` 其实早就失效——这一条是我读代码确认的，不是工具报的。

**写入 `controls.css` 的移植规则**（放在 `components` 层，而非方案里写的 `themes/shadcn/theme.css`）

放这里是因为这些规则对 shadcn 和 8bitcn 都适用，而共享控件规则本来就集中在这个文件；三主题各自需要的差异由主题文件覆写，顺序上主题在 `base.css` 之后加载，仍然赢。

| 目标 | 规则要点 |
| ---- | -------- |
| 8bitcn 阶梯边框 | `[data-slot="button-decorations"] > span { background-color: var(--foreground) }`，软色变体加 `nova-pixel-fill-soft` 类 → `color-mix(in oklab, var(--foreground) 20%, transparent)` |
| 8bitcn 对话框外框 | `[data-slot="dialog-pixel-x|y"] { border-inline/block: 2px solid var(--foreground) }` |
| 下拉 | 触发器 `box-shadow: var(--control-shadow)`、图标 `opacity: .5`、标签字号/颜色、条目字号、禁用条目 `opacity: .5`、分隔线 `background-color: var(--border)` |
| 表格 | 页脚 `border-top` + `--muted` 半透明底 + `font-weight: 500`；说明行字号与 `--muted-foreground` |
| 文件输入 | `[data-slot="input"]::file-selector-button`（项目 CSS 覆盖不到伪元素；仓库里确有一处 `<Input type="file">`） |

**踩到的坑**：脚本第一版按"字符串字面量里出现外观词就改写"，把 `variant !== "outline"` 改成了 `variant !== ""`——`outline` 正好既是外观类名又是变体名。改成只在类上下文（`className` 属性、`cn`/`cva` 调用）内改写，并加了一条断言确认该比较式未被触碰。

**预期内的差异（需人工确认）**

1. `disabled:opacity-50`：删掉后 Minecraft 主题的禁用按钮少一层 50% 透明（shadcn/8bitcn 由 `button:disabled` 覆盖，无影响）。Minecraft 的禁用观感本来由贴图 + 灰字表达，去掉更符合该主题。
2. `hover:text-accent-foreground`：shadcn 下 hover 文字色由 `--accent-foreground` 变为继承值，而两者都解析到 `--foreground`，**实际无变化**。
3. 焦点环：删掉 `focus-visible:ring-[3px]` + `ring-ring/50`，统一用项目的 `outline: 2px solid var(--ring)`。这正是要消除的"两套焦点样式"。

**Phase 3 退出验证**

| 检查 | 结果 |
| ---- | ---- |
| `tsx-appearance-utility-class` | ✅ **102 → 0**（退出标准达成） |
| `npx tsc --noEmit` | ✅ 0 错误 |
| `check:layers` / `check:application-boundaries` / `check:ui-styles` | ✅ 0 违规 |
| `npm run build` | ✅ 通过；`.bg-primary` 仍位于 `vendor` 层 |
| 层叠反转 | ✅ 0 |
| 测试套件 | ✅ 与 HEAD 完全一致（无新增失败） |

**待人工验收**：三主题 × 明暗下逐个确认——按钮各变体（default/destructive/outline/secondary/ghost）、8bitcn 的按钮阶梯边框与对话框外框、下拉菜单（触发器阴影、标签、条目、分隔线）、表格页脚与说明行、文件选择按钮。

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

**基数已复核**：实测裸 `<select>` 为 **10 处**（原估 6 处已过期），分布在 `BlockPickerDialog` 1、`AssetManagerContent` 7、`EnumeratorWindow` 1、`VersionConversionDialog` 1。

---

#### Phase 4.0 实施记录 ✅ 两个运行时才可见的 Minecraft 控件缺陷

**这两个缺陷不是本轮计划要做的**——4.3～4.8 是 select、按钮、下拉、变体，都不会碰到复选框与单选框。由人工验收发现，故优先修复。

**缺陷一：单选框贴图被裁成圆形。** `styles/controls.css:115` 为 `[data-choice-layout="control"]` 声明了 `border-radius: 50%`（其他主题想要圆形）。Minecraft 的覆盖规则 `appearance.css` 优先级更高（0,3,2 > 0,3,1），但它**只重设了 `border`/`border-image`/`background`，没有重设 `border-radius`**，于是 20×20 的方形 `radio_default.png` 被裁圆。

**缺陷二：复选框贴图中心透出 `--primary`。** `styles/controls.css` 的选中态规则用**简写** `background: var(--primary)` 填充；Minecraft 的 `[data-state="checked"]` 规则**只覆盖 `background-image`**。
关键在于两者的优先级是颠倒的：

| 声明 | 选择器 | 优先级 |
| ---- | ------ | ------ |
| 共享层填充 | `html[data-theme] [data-slot="checkbox"][data-state="checked"]` | **0,3,1** |
| MC 贴图基底 | `html[data-theme="minecraft"] button[data-slot="checkbox"]` | 0,2,2 |

属性数目先于类型数目比较，**所以共享层赢了 `background-color`**，而 MC 的 `[data-state="checked"]`（0,3,2）只赢了 `background-image`。结果就是贴图画在 `--primary` 色块上——正好是观察到的 `#b8d49c`（深）/ `#3d6033`（浅），两者都是 MC 的 `--primary`。

**修复**

1. `controls.css`：把两处 `background:` 简写改为 `background-color:`。共享层只应声明填充色，**不该重置 `background-image`**——正是这个简写让"贴图的主题"和"配色的主题"在各自动一个属性上互相打架，才产生了这个bug。
2. `appearance.css`：MC 单选框规则补 `border-radius: 0`。
3. `appearance.css`：新增 `html[data-theme="minecraft"] button[data-slot="checkbox"][data-state] { background-color: transparent }`。`[data-state]` 是刻意的——正是它让这条规则以 0,3,2 压过共享层的 0,3,1，而不是打平。

**为什么现有检查全都没发现**

五条护栏规则、层叠反转分析器和测试套件读的都是**源码**，而这两个缺陷只存在于**层叠解析结果**里：声明都在、语法都对、层序也对，只是**赢家不是预期的那个**。这是"读源码"和"算结果"之间的盲区。

**新增 `scripts/minecraft-controls.test.mjs`（4 个用例）** —— 一个最小的级联求解器：按"层序 → 优先级 → 文档顺序"算出某个属性的**获胜声明**，再断言它的值。它检查的不是"规则写了什么"，而是"最后谁赢"。

> 这个测试第一版**是假绿的**：它只看选择器是否提到 slot，不看主题约束，于是 8bitcn 的 `[data-theme="8bitcn"] { border-radius: 0 }` 把"MC 单选框是方的"这条断言满足了——删掉修复它照样通过。补上主题过滤后，两条断言才各自可被证伪（分别单独回退修复，各自失败 1 个用例）。**一个无法失败的测试比没有测试更糟**，因为它给出的是虚假的安全感。

**验证**：`check:layers` / `check:application-boundaries` / `check:ui-styles` 全部 0 违规；`build` 通过；测试套件与 HEAD 完全一致并新增 4 个用例。

---

#### Phase 4.1 实施记录 ✅ Slider

**发现一整套死 CSS。** `themes/minecraft/widget/range.css` 全部选择器都是 `input[type="range"]` 及其 `::-webkit-slider-*` / `::-moz-range-*` 伪元素，但**仓库里没有任何原生 range input**——唯一的滑块是 Radix `<Slider>`（`FlakePage` 3 处、`UiTestPage` 1 处），它渲染的是 `[data-slot="slider"]` 的 span 树。**这些规则一条也不可能命中。**

**还发现一个真实布局缺陷。** Radix 给 Thumb 的内联样式是 `position: absolute; left: calc(X% + Npx); transform: translateX(-50%)`，但**从不给根元素 `position: relative`**。而 `controls.css` 也没设，只有 `[data-slot="slider-track"] { position: relative; flex: 1 }`——track 不是 thumb 的祖先，`flex: 1` 也因为没有 flex 容器而无效。所以共享滑块在三个主题下都是错位的。已在 `controls.css` 补上根规则（`position: relative; display: flex; align-items: center`），thumb 的静态位置才会按 flex 居中。

**改动**
- 新增 `themes/minecraft/widget/slider.css`：把贴图从伪元素改写到 `[data-slot="slider-track"]` / `[data-slot="slider-thumb"]`。因为是同一套 DOM，**webkit/moz 两套重复规则合并成一套**。
- 删除 `themes/minecraft/widget/range.css`，更新 `theme.css` 导入。
- 清理 `modern.css` 中两处已死的 `input[type="range"]` 提及（`:not([type="range"])` 与 `accent-color` 列表）。

#### Phase 4.2 实施记录 ✅ Checkbox

`EnumeratorWindow.tsx` 的最后一处裸 checkbox 换成 `<Checkbox>`（`onCheckedChange` 返回 `boolean | "indeterminate"`，故取 `checked === true`）。

**随后两份 `check.css` 的规则全部变成死代码**：`shell/widget/check.css` 与 `themes/minecraft/widget/check.css` 的选择器是 `input[type="checkbox"]` / `input[type="radio"]` / `label:has(> input[...])`，而仓库里**已无任何原生 checkbox 或 radio**（28 处 `<Checkbox>` 都是 Radix，原生 radio 为 0）。MC 那份的复选框贴图早就被 `appearance.css:108-131` 的 `button[data-slot="checkbox"]` 规则取代——是重复实现。

- 删除 `themes/minecraft/widget/check.css`，其两个仍然生效的 token（`--ln-check-label-gap: 8px`、`--ln-check-label-min-height: 24px`）移入 `appearance.css` 的 MC 令牌块。
- `shell/widget/check.css` 只保留 token 定义（`6px` / `20px`）并删除已死的 components 块。
- **删除 `--ln-check-size`**：它的唯一引用就在这两份死规则里，Radix 复选框在 `controls.css` 与 `appearance.css` 中各自固定尺寸，无人再读它。

**护栏新增第 5 条规则：`tsx-native-control`** —— 检测 TSX 中的裸 `<select>` 与 `<input type="checkbox"|"radio"|"range">`（`type="file"` 合法，不报）。用例 13 个。这让 Phase 4 剩余步骤的退出标准变成可测的。

**4.1 / 4.2 退出验证**

| 检查 | 结果 |
| ---- | ---- |
| 原生 checkbox / radio / range | ✅ **0**（4.2 退出标准达成） |
| `tsx-native-control` | 10（均为裸 `<select>`，属 4.3 范围） |
| `npx tsc --noEmit` | ✅ 0 错误 |
| `check:layers` / `check:application-boundaries` / `check:ui-styles` | ✅ 0 违规 |
| `npm run build` | ✅ 通过 |
| 测试套件 | ✅ 与 HEAD 完全一致 |

**待人工验收**：三主题下的滑块（轨道贴图、thumb 贴图、hover、禁用、焦点环；注意共享滑块此前是错位的，**这次是修好而不是保持原样**），以及枚举器"减去右侧"复选框的外观与键盘操作。

---

#### Phase 4.3 实施记录 ✅ Select

**10 处裸 `<select>` 全部换成 `Dropdown`**（方案里写的 6 处已过期）：

| 文件 | 数量 | 说明 |
| ---- | ---- | ---- |
| `AssetManagerContent.tsx` | 7 | 其中 4 处是真选择（激活槽位、版本、语言、来源），3 处是**仅显示文字的禁用占位**（"子选项1/2"、来源说明），它们本来就没有可选项，改用禁用的 `Dropdown` 保持外观一致 |
| `BlockPickerDialog.tsx` | 1 | 与 `creativeInventoryDialog.tsx:207` 的写法对齐 |
| `EnumeratorWindow.tsx` | 1 | 分类筛选，选项表提升为模块常量 |
| `VersionConversionDialog.tsx` | 1 | 目标版本 |

**一个必须处理的连带问题：MC 的 `<button>` 会被贴图化。** `themes/minecraft/widget/button.css` 用 `button` 选择器给**所有**按钮套上 `button_normal_min.png` 贴图，而 Radix 的下拉触发器正是一个 `<button>`。若不处理，转换后的下拉会从"输入框外观"变成"贴图按钮外观"——正好和原生 `<select>` 原有的样子相反。

于是按方案把 MC 的 `select` 规则搬到触发器上（`minecraft/widget/input.css`），删除已死的裸 `select` 选择器：

- `[data-slot="select-trigger"]` 取 MC 输入框外观（2px `#a0a0a0` 边框、黑底、白字、44px），并用 `border-image: none` 顶掉按钮贴图，`> svg { display: none }` 让像素箭头 `spin_down.png` 取代矢量 chevron。
- **hover / active 必须重述配色。** 通用规则 `button:hover:not(:disabled)` 与 `button:active:not(:disabled)` 的优先级 (0,2,1) **高于**基础触发器规则 (0,1,0)，否则悬停时触发器会变成按钮黄 `#ffff00`、点击时被按钮底色覆盖。这是同一类"主题规则输给通用规则"的坑，只是这次在写之前就预料到了。
- 用同样的方式核对了它确实赢：宽度方面给 `.asset-manager-select` / `.enumerator-filter-select` 补 `> [data-slot="select-trigger"] { width: 100% }`（触发器是 `inline-flex`，不会自己撑满容器）。

**顺带调整**：`.properties-conversion-target select` → `[data-slot="select-trigger"]`，保住原有的 `padding: 6px 10px` / `background: var(--background)`。

**退出标准**

| 检查 | 结果 |
| ---- | ---- |
| `tsx-native-control` | ✅ **10 → 0**（裸 `<select>`、裸 checkbox/radio/range 均为 0） |
| `npx tsc --noEmit` | ✅ 0 错误 |
| `check:layers` / `check:application-boundaries` / `check:ui-styles` | ✅ 0 违规 |
| `npm run build` | ✅ 通过 |
| 测试套件 | ✅ 与 HEAD 完全一致 |

**遗留（交给 Phase 5）**：共享层里仍有一批已死的裸 `select` 选择器（`modern.css` 的 `.input, …, select, textarea` 与 `select { cursor: pointer }`、`base.css`、`widgets.css`、`generate.css` 等）。它们现在匹配不到任何元素，清理属第 5 节"僵尸层"范围，本轮不动以免混淆 diff。

#### Phase 4.4 实施记录 ✅ 按钮尺寸

**先量了一下两套尺寸系统，发现它们量的不是同一个维度**：遗留类按**字号**分档（`.btn-xs` 11px / `.btn-sm` 11px / `.btn-md` 12px / `.btn-action` 13px），`size` 变体按**盒子尺寸**分档。所以不是改名，而是**选一套**——按方案保留 `size` 变体这套，并且**尺寸不再参与字号**（见下）。

**一处需要更正我自己的判断**：我起初是从 `button.tsx` 的 CVA 串读出尺寸变体的（`xs: h-6 px-2`、`sm: h-8 px-3`），据此推断"高度被 `min-height` 顶住、变体无效"。实际上 `controls.css` **早就有** `[data-size]` 规则：

```css
html[data-theme] [data-slot="button"][data-size^="icon"] { padding: 7px; width: var(--control-height); flex: 0 0 var(--control-height); }
html[data-theme] [data-slot="button"][data-size="sm"] { min-height: 30px; padding: 5px 10px; }
```

它们在 `components` 层，**压过** Tailwind 的 `h-8`/`px-3`（`vendor` 层），所以那些 Tailwind 尺寸类从来就不生效。真正缺规则的只有 `xs`：`h-6`（24px）被基础按钮的 `min-height: var(--control-height)`（32px）顶住，永远压不下去。于是补上 `xs` 这一档——且只给内距，不给字号：

```css
html[data-theme] [data-slot="button"][data-size="xs"] { padding: 1px 6px; }
```

**字号必须是统一的，这是像素字体的硬约束。** Minecraft 主题用 Unifont 绘制正文，而 Unifont 是位图字体，**只在设计字号的整数倍下才渲染出像素风格**；11px、13px 这类中间值会让字形发虚。我最初为"保住遗留观感"给 `xs` 加了 `font-size: 11px`，恰恰制造了这个问题——已删除。

正确的机制是让按钮**继承**正文尺寸：`base.css` 的 `button, input, select, textarea { font: inherit }`（`components` 层，三主题都加载），正文尺寸由 `--ln-body-font-size` 给出（shadcn 14px / 8bitcn 16px / Minecraft 16px）。因此：

- 尺寸变体**只改盒子**，永不改字号；
- `flake.css` 里 `.flake-page__edit-tool-button` 的 `font-size: 16px` 一并删除。它是画在 44px 贴图上的标签，但 Minecraft 下 16px 恰好等于继承值（删除无变化），而其他主题下它正是"逐个按钮自定义字号"的来源；
- 新增护栏规则 `css-button-font-size`（第 7 条），只检查**最右复合选择器**是按钮的规则，因此 `.toolbar button span { font-size }` 这类后代规则不会被误报；`font: inherit` 简写不受影响。仓库当前为 **0**，没有基线条目。

**处置**

| 遗留类 | 改为 | 依据 |
| ------ | ---- | ---- |
| `.btn-xs`（8 处） | `size="xs"` | 两者都是 32px 高（基础 `min-height`）、`padding: 1px 6px`，完全一致 |
| `.btn-sm`（2 处） | `size="xs"` | 与 `.btn-xs` 只差 1px 内距（`1px 6px` vs `2px 7px`），实为同一档。映射到 `size="sm"` 会变成 30px 高 / `5px 10px` 内距，与遗留不同 |
| `.btn-md`（2 处） | 不传 `size`（默认档） | 它只设 `font-size: 12px`，不改盒子 |
| `.btn-primary`（1 处） | `variant="default"` | 与 `modern.css` 的 `button[data-variant="default"]` 语义相同 |
| `.btn-action`（0 处） | 删除 | 从未被引用 |

删除这 5 个类的定义（`replace-units.css`），并移除 `modern.css` 中已成死选择器的 `button.btn-primary`。

**退出标准**：`css-legacy-button-variant` 8 → **0**，`tsx-legacy-button-variant` 12 → **0**，`css-button-font-size` **0**。

**已知差异**：`.btn-primary` 另设了 `font-weight: 600`，而 `variant="default"` 继承基础按钮的 500。字号差异已按上面的原则全部消除。

#### Phase 4.6 实施记录 ✅ 裸 `<button>` 归零

3 个文件共 **17 处**（`AssetManagerContent` 10、`EnumeratorWindow` 6、`BlockPickerDialog` 1）改为 `<Button>`。

- `className="btn"` 整个删除——`<Button>` 的基础类串里已经带 `btn`，在调用点重复没有意义；`subwindow-close-button` 等其它类保留。
- `type="button"` 保留：它与组件默认值相同，保留可减少 diff 噪音。
- 两个文件补上 `Button` 导入（相对路径由脚本从文件位置算出，`BlockPickerDialog.tsx` 需要 `./ui/button` 而非 `../../components/ui/button`）。

**护栏新增第 6 条规则：`tsx-raw-button`** —— 小写 `<button>` 会绕过 `<Button>` 的 variant/size 体系。仓库当前为 **0**，因此这条规则没有基线条目，属于"从今往后不许再出现"。用例 13 个。

#### 修正：按钮字号必须统一（像素字体约束）

Phase 4.4 的第一版给 `[data-size="xs"]` 加了 `font-size: 11px`，理由是"保住遗留观感"。**这是错的**：Minecraft 主题用 Unifont 绘制正文，它是位图字体，**只在设计字号的整数倍下才显示像素风格**，11px 会让字形发虚。而且"每个尺寸档一个字号"本身就是这套 UI 里字号散乱的来源之一。

已按"所有按钮同一字号"修正：

1. `[data-size="xs"]` 只留内距，删除字号。
2. `flake.css` 的 `.flake-page__edit-tool-button { font-size: 16px }` 删除（Minecraft 下 16px 恰等于继承值，删除无变化；其他主题下它正是逐个按钮自定义字号的来源）。
3. 新增第 7 条护栏规则 `css-button-font-size`，从机制上禁止再出现"某个按钮自己的字号"。

统一字号的机制是**继承**：`base.css` 的 `button, input, select, textarea { font: inherit }` 在 `components` 层、三主题都加载，正文尺寸来自 `--ln-body-font-size`（shadcn 14px / 8bitcn 16px / Minecraft 16px）。护栏只检查 `font-size`，`font: inherit` 简写不受影响。

#### Phase 4.4 / 4.6 退出验证

| 检查 | 结果 |
| ---- | ---- |
| `css-legacy-button-variant` / `tsx-legacy-button-variant` | ✅ 8 → 0 / 12 → 0 |
| `css-button-font-size`（按钮统一字号） | ✅ 0（规则在夹具上验证会触发；后代选择器不误报） |
| 小写 `<button>`（全仓库 TSX AST 扫描） | ✅ **0** |
| `tsx-raw-button` | ✅ 0（规则本身在夹具上已验证会触发） |
| `npx tsc --noEmit` | ✅ 0 错误 |
| `check:layers` / `check:application-boundaries` / `check:ui-styles` | ✅ 0 违规 |
| `npm run build` | ✅ 通过 |
| 测试套件 | ✅ 与 HEAD 完全一致 |

**待人工验收**：replace 页的分隔符与规则卡片工具条（↑ ↓ 复制规则 删除规则）、"下一步替换"计数行、`+ 创造模式物品栏` / `+ 枚举器`、枚举器右侧的移动与保存按钮、枚举器的"新增集合/保存副本/保存集合"、资产管理器的 10 个操作按钮（含关闭按钮）、方块选择对话框的关闭按钮，以及枚举器选块对话框底部的"取消/添加 N 个条目"。重点看这些按钮在**三主题**下的尺寸与文字大小是否与改动前一致。

---

#### 修正：按钮尺寸按主题各自的来源定标准

上一版把"标准按钮"理解成了一套尺寸，方向不对。正确的原则是**每个主题按自己上游的标准**：

| 主题 | 上游性质 | 处置 |
| ---- | -------- | ---- |
| Minecraft | 本项目自行维护 | `--control-height: 44px` 与 `button` 纹理规则**刻意为之**，保持不变；其 `button { height/min-height: 44px; padding: 4px 12px }` 本来就压过所有 Tailwind 尺寸类，所以 Minecraft 下**所有标准按钮已经是同一尺寸** |
| shadcn | shadcn/ui，主流组件库 | **删掉我们的自定义尺寸**，回到组件库默认：`size="sm"` 的 `h-8 px-3` 由 `button.tsx` 的 CVA 串决定，`controls.css` 不再为 shadcn 声明尺寸覆盖 |
| 8bitcn | `TheOrcDev/8bitcn-ui`，第三方小众 registry（依据 `THIRD_PARTY_UI.md` 的来源登记，两个上游均为官方 registry，但 8bitcn 并非主流组件库） | **保持现状**：`sm` 的 `min-height: 30px; padding: 5px 10px` 原样保留，改为只作用于 `html[data-theme="8bitcn"]` |

**4.4 的 `size="xs"` 决定被推翻**：`size` 只是"相对标准按钮的档位"，不该用来把工具栏按钮做成另一种尺寸。replace 页那 10 处已全部改回标准按钮（删除 `size` 属性），`[data-size="xs"]` 规则随之无引用、已删除。

**图标按钮改为正方**：`[data-size^="icon"]` 原来只设了 `width: var(--control-height)`，高度仍来自 Tailwind 的 `size-9`/`size-8`/`size-10`，于是 shadcn 下 `icon` 是 32×36、`icon-sm` 是 32×32、`icon-lg` 是 32×40——**既不方也不统一**。现在宽高与 min/max 全部钉在 `--control-height`，因此每个主题内所有 `icon*` 都是同一个正方形（shadcn/8bitcn 32px、Minecraft 44px）。

**B、C 两类（按 CSS 类改盒子的按钮）全部不动**：关闭按钮、物品栏槽位、flake 工具与快捷栏等本就是**方格热区**，不是标准按钮。

#### 修正：flake 音符盒的标签全部叠在一起

现象是"按钮没有字，所有字叠在一个地方"。根因不是布局参数，而是**层叠作用域被 Portal 切断**：

`ui/components/ui/dialog.tsx` 的 `DialogContent` 用 `<DialogPortal>` 渲染，**对话框内容被移出页面子树**。而音符键的规则写成：

```css
.flake-page.flake-page--edit-mode .flake-page__state-dialog-grid .flake-page__note-keyboard button.flake-page__note-key-button.btn { position: relative; ... }
```

它要求祖先里有 `.flake-page`——Portal 之后**永远不可能匹配**。于是按钮拿不到 `position: relative`，而未被限定的 `.flake-page__note-key-label { position: absolute; inset: 0 }` 只能退到最近的已定位祖先：加载 `modern.css` 的主题退到 `.dialog-surface`，Minecraft 不加载 `modern.css`（`.dialog-surface` 没有 `position: relative`）则退到 `.dialog-overlay { position: fixed }`。两种情况下**每个键的标签都居中对齐到同一个盒子**，于是全部叠在一起。同一条规则里的 `is-sharp` 位移也因此从未生效。

**修复**：把两条规则改为只锚定键盘容器本身（`.flake-page__note-keyboard button.flake-page__note-key-button...`），不再依赖页面祖先。这样：

- 按钮拿到 `position: relative` 与固定宽度，标签 `inset: 0` 正确居中在各自按钮内；
- `is-sharp` 的 `transform: translateY(calc(size * -0.5))` 开始生效——槽位是两格高、按钮居中，上移半格即贴顶，符合 dev/flake 的黑键设计。

**新增第 8 条护栏规则 `css-portal-scope`**：选择器同时要求页面根（`.flake-page` / `.nova-page`）与对话框内容标记（`.dialog-*`、`[data-slot^="dialog"]`、`__state-dialog`）时报错——这正是 Portal 切断作用域的形态，而它对既有全部检查都是不可见的。用例 15 个。

> 说明：音符键的高度在 shadcn 下是标准按钮的 36px，而 `--flake-state-note-button-size` 是 `--control-height` 的 32px，因此上移半格（16px）会比槽位留白（14px）多 2px。Minecraft 下两者同为 44px，完全吻合。这一点未改，因为 shadcn 的高度按你的要求应保持组件库默认。

#### 本轮退出验证

| 检查 | 结果 |
| ---- | ---- |
| `size="xs"` 残留 | ✅ 0（replace 10 处已恢复标准） |
| `[data-size^="icon"]` | ✅ 宽高 min/max 全部 = `--control-height`，产物层位为 `components`（压过 `vendor` 的 `size-*`） |
| `[data-size="sm"]` | ✅ 仅作用于 `8bitcn` |
| `css-portal-scope` | ✅ 0（规则在旧选择器上验证会触发） |
| `npx tsc --noEmit` / `check:layers` / `check:application-boundaries` / `check:ui-styles` | ✅ 全部 0 违规 |
| `npm run build` | ✅ 通过 |
| 测试套件 | ✅ 与 HEAD 完全一致 |

**待人工验收**
1. replace 页：10 个按钮恢复标准尺寸后，那条挤了 4 个按钮 + 输入框的 `.replace-unit-header` 是否仍然排得下（你已实测统一字号不会挤压，但高度变化仍需确认）。
2. 三主题的图标按钮（各 3 处 `icon`/`icon-sm`、1 处 `lg` 除外）现在应为正方，且同一主题内一致。
3. shadcn 的 `size="sm"` 按钮（library 13 处、properties 13 处）恢复组件库默认后的观感。
4. **flake 音符盒**：打开调试棒 → 音符盒的 `note` 属性，键上应各自显示音名（不再叠在一起），带 `#` 的键应上移半格。

**待人工验收**：三主题下的四个转换点——资产管理器的"来源/子选项"两行、枚举器工具栏的分类筛选、方块选择对话框的分类、属性页版本转换的目标版本。重点看 MC 主题：下拉触发器现在应当是**输入框外观**（深色底、灰色描边、像素箭头），而不是贴图按钮；其悬停与展开状态不应变黄。另外请注意，MC 主题**其它既有的下拉**（设置页、flake 页等）也会随之从"贴图按钮外观"变为"输入框外观"——这是本轮有意的统一，但属于可见变化。

> **测试自身的一个缺陷（已修正）**。给 `minecraft-controls.test.mjs` 增加触发器断言时，发现它此前**也是假绿的**：`themeOf()` 对无值的 `[data-theme]` 返回 `null`（含义是"任何主题"），而过滤条件把它当成"别的主题"排除掉了——于是**整个共享层的规则都被踢出候选集**，断言只在主题自己的规则里比较，自然怎么改都通过。
>
> 修正后重跑了完整的可证伪性验证，5 条断言现在**各自**单独回退对应修复都会失败：单选框 `border-radius`、复选框透明底、触发器输入框外观、触发器 hover 重述、共享层 `background-color` 简写。并补了一条自检断言，要求共享层至少有 20 条选择器处于可参与比较的状态，防止这类"候选集被清空"再次悄悄发生。
>
> 需要说明的是：Phase 4.0 那两个缺陷的**根因判断与修复本身**是可靠的——它们由独立的级联求解脚本和手算优先级共同确认，也不依赖这个测试；出问题的是测试对回归的**检出能力**。**一个无法失败的测试比没有测试更糟**，这已经是同一个教训的第二次。

---

### Phase 4.5 实施记录 ✅ 遗留 `btn` 类清理

**基数复核**：带遗留 `btn` 标记的 `<Button>` 为 **65 处**（原估 75），无显式 `variant` 的多达 **163 处**（原估 45）。

**只做了机械的那一半**：删掉调用点上的 `btn` 标记。安全依据是 `<Button>` 的 CVA 基础串里已经有 `btn`（`"nova-control btn inline-flex …"`），调用点重复传它**只是把类名写了两遍，渲染完全不变**。49 处 `className="btn"` 整个属性删除，17 处从多类列表里摘掉 `btn`（其余类保留）。匹配用**空白切分后的精确相等**而非词边界正则——`replace-entry-remove-btn` 这类后缀包含 `btn` 的类必须保留。

护栏新增第 9 条规则 `tsx-redundant-btn-class`，仓库当前为 0。

**没有做的另一半：给 163 处裸 `<Button>` 指定语义变体。** 这不是清理而是产品决策——`variant` 决定"主要 / 次要 / 危险"，逐按钮判断属于设计意图；当前分布（默认 secondary 163、ghost 69、secondary 38、default 11、destructive 7）看起来是既有的一致选择而非遗漏。**这一项留给你决定**：若希望某些位置（例如各对话框 footer 的确认按钮）提升为 `variant="default"`，给出范围我一次做完。

### Phase 4.7 实施记录 ✅ 下拉与对话框收敛

**`BlockSearchSelect` 已经收敛**，无需改动：它用 Radix `Popover.Root/Trigger/Portal/Content` 与共享 `Input`，属性下拉复用 `Dropdown`。

**`BlockPickerDialog` 已改为复用共享 `Dialog`**。它原来手写了一套 `dialog-overlay` + `dialog-content` 加自绘标题栏，等于**放弃了 Portal、焦点陷阱与 Esc 关闭**，也拿不到主题给 `.dialog-surface` 的样式。现改为 `<Dialog title={title} onClose={onClose} width="lg" className="flake-page__inventory-dialog">`，与姊妹组件 `creativeInventoryDialog` 完全一致（后者本来就这么写）。顺带把里面裸的 `<input className="input">` 换成共享 `<Input>`，移除因此不再使用的 `Button` 导入。

悬停提示 `CreativeInventoryTooltip` 原挂在 overlay 内，现移到 `Dialog` 之外——它是 `position: fixed; z-index: 9999`，DOM 位置无关。

**未动**：`AssetManagerWindow.tsx` 的 `AssetManagerDialog` 同样手写 overlay/content，但它用 `subwindow-frame` 外壳（不是 `.dialog-surface`），且不在本步范围。它目前缺 Esc 关闭与焦点陷阱，可作独立议题。

### Phase 4.8 实施记录 ✅ 变体清理

确认 `variant="outline"` 与 `variant="link"` **业务代码零使用**。按方案的第二个选项：**在 `UiTestPage` 补用例**，而不是删除主题规则。理由是这两个变体属于上游 `buttonVariants` 的 API（源码自持的上游组件），删样式会让 API 里的变体静默失效；放进控件画廊后它们**变得可见、可回归**，也更符合本项目"每个状态都要能看到和验证"的做法。现在画廊"按钮变体"一行覆盖全部六种：default / secondary / outline / ghost / link / destructive。

### Phase 4 全部完成后的验证

| 检查 | 结果 |
| ---- | ---- |
| 调用点遗留 `btn` 标记 | ✅ 65 → **0**（`tsx-redundant-btn-class` 守住） |
| `variant` 覆盖 | ✅ 六种均有使用（outline/link 由画廊覆盖） |
| 手写 dialog chrome | ✅ `BlockPickerDialog` 已收敛；仅 `AssetManagerDialog`（`subframe` 外壳）保留 |
| `npx tsc --noEmit` / `check:layers` / `check:application-boundaries` / `check:ui-styles` | ✅ 全部 0 违规 |
| `npm run build` | ✅ 通过 |
| 测试套件 | ✅ 与 HEAD 完全一致（`check:ui-styles` 16 个用例） |

**待人工验收**
1. 方块选择对话框（replace 页与 flake 页的"选择方块"）现在走共享 `Dialog`：标题栏样式、**Esc 可关闭**、打开后焦点进入对话框、点遮罩关闭、悬停提示位置。
2. `UiTestPage` 新增的"按钮变体"一行，在三主题下六种变体的观感。

---

### Minecraft 按钮变体贴图（Phase 4 之后的增量）

**命名规则**：`button_<状态>[_<变体>]_min.png`，变体段可省略。状态词表 `normal / hover / check / check_hover / disabled`（`active` 复用 `normal`）。

**已定案的映射**

| `data-variant` | 贴图变体段 | 说明 |
| -------------- | ---------- | ---- |
| `secondary`（CVA 的默认变体，200 个按钮） | 无（就是不带变体段的基础那套） | `button_normal_min.png` 等 5 张即这套 |
| `default` | **`primary`** | 代码名与贴图名不同，按决定保留；文件内已写注释记录该映射 |
| `outline` | `outline` | 独立一套 5 张 |
| `destructive` | `destructive` | 4 个状态；缺 `button_disabled_destructive_min.png`，禁用态刻意回落共享的禁用贴图 |
| `ghost` / `link` | 不需要 | `appearance.css` 已给 `border-image: none`，是无边框按钮 |

**实现方式：变量间接，而不是逐变体写 `url()`**

朴素做法是给每个变体写 5 条规则，但每条都要**压过** `button:hover:not(:disabled)` 这类状态规则的优先级，而且**漏写一个状态就会静默沿用基础贴图**。因此改成：

```css
button[data-slot="button"][data-variant="default"] {
    --ln-minecraft-button-normal-image: url("…button_normal_primary_min.png");
    /* hover / check / check-hover / disabled 同上 */
}
/* 状态规则只读变量，未定义时回落到基础贴图 */
button:hover:not(:disabled) {
    --ln-minecraft-button-image: var(--ln-minecraft-button-hover-image, url("…button_hover_min.png"));
}
```

变体规则只提供"支撑变量"，不参与贴图的层叠竞争，所以**没有优先级问题**；没有贴图集的变体（secondary、destructive）什么都不用写，自然回落。

**新增 2 个测试**（`minecraft-controls.test.mjs`，现共 7 个用例）

1. 每个有贴图的变体必须声明 `normal / hover / check / check_hover` 四个状态，且每个 `url()` 指向的文件**真实存在**。`disabled` 是**可选的**：禁用按钮在各变体下长得一样是合理的默认，所以 `destructive` 就刻意回落共享的禁用贴图；其余四个状态则是"变体必须看起来不同于基础套"的部分，缺一个就报错。
2. 6 条基础回退 `url()` 指向的文件也真实存在。

这两条针对变量间接带来的两个静默失败模式：漏一个状态 → 该状态沿用基础贴图；文件名拼错 → 变量解析为空、回落基础贴图。两者都不报错、只会"看起来差一点"。**均已验证可证伪**：删掉 primary 的 hover 状态 → 失败 1 个；文件名改成 `primray` → 失败 1 个。

**顺带修掉护栏的一个假阳性**：`token-undefined-reference` 原来扫描原始源码，于是注释里写 `var(--ln-minecraft-button-<state>-image, …)` 被当成"引用未定义令牌"报错。已改为扫描**注释屏蔽后**的文本，并验证真实未定义引用仍会报出。新增 1 个用例（`check:ui-styles` 现共 17 个）。

**主要按钮的常态文字色是深灰，不是白。** primary 是浅色底板，白字看不清，因此常态取 `#3f3f3f`（Minecraft 调色板里既有的深灰，原本用作文字投影色）；悬停、按下、选中仍然是黄色 `#ffff00`，禁用仍是 `#808080`，三者不变。

实现上同样用变量间接：基础规则写 `color: var(--ln-minecraft-button-normal-color, #ffffff)`，`default` 变体只声明 `--ln-minecraft-button-normal-color: #3f3f3f`。变体只提供一个支撑变量、**不参与 `color` 的层叠竞争**，所以 `button:hover:not(:disabled)`、`button[aria-pressed="true"]`、`button:disabled` 这些更高优先级的状态规则照常生效，悬停/按下/禁用都不受它影响。

### UI 测试页：变体按钮的三种状态可切换

`UiTestPage` 的"按钮变体"一行原来只能看到常态（以及它的悬停），按下与禁用两个状态无法观察。现在**每个变体按钮点击后循环 常态 → 按下 → 禁用**，三个状态都能逐一检查。

- 按下态用 `aria-pressed`，禁用态用真正的 `disabled`（否则主题的 `:disabled` 规则不会生效，测的就不是真东西）。
- **一个 `<button disabled>` 会吞掉自己的点击**，所以按钮外面包了一层 `span.button-variant-demo` 作为点击目标，并在 `.ui-control-gallery` 作用域内给它 `:disabled` 时加 `pointer-events: none`，让点击穿透到外层继续循环。这两条规则只作用于控件画廊，不影响业务界面。
- 悬停态仍然只能靠真实鼠标悬停观察（无法用状态模拟）。

**待人工验收**：切到 Minecraft 主题，看三个有贴图的变体在 常态 / 悬停 / 按下选中 / 按下选中+悬停 / 禁用 下的贴图与文字色——
`default`（主要）：常态**深灰**字，悬停与按下黄字，禁用灰字；
`outline`（描边）：常态白字（标准）；
`destructive`（危险）：常态白字，禁用态与其它变体同款；
`secondary` 应仍是原来的基础观感。

### Phase 5 — 清理僵尸层

**做**：删除 `widget/range.css`、`widget/check.css`；把 `widget/button.css`、`widget/input.css`、`widget/table.css`、`widget/nav.css`、`widget/subwindow.css` 的裸元素选择器改写为 `[data-slot]`；`widget/popup.css`、`widget/dialog.css`、`widget/content.css`、`widget/text.css` 的类选择器并入 `components`。

**退出**：`ui/themes/minecraft/` 中裸元素选择器为 0；文件数从 15 降至不超过 6。

---

### 回归修复：Minecraft 复选框被裁剪成圆角矩形

人工验收发现 Minecraft 主题的复选框被裁成**圆角矩形**，而贴图本身是方的（单选框正常）。

**根因：共享层的一条基础规则多了个 `button` 元素，压住了所有主题覆盖。**

```
ui/styles/controls.css:66
html[data-theme] button[data-slot="checkbox"] { ... border-radius: 4px; ... }   (0,2,2)
html[data-theme="minecraft"] [data-slot="checkbox"] { border-radius: 0 }        (0,2,1)  ← 永远输
```

`html[data-theme]`(0,1,1) + `button`(0,0,1) + `[data-slot="checkbox"]`(0,1,0) = **(0,2,2)**，比主题覆盖的 (0,2,1) 高一级，所以 `4px` 恒定胜出。`background-image` 会被 `border-radius` 裁切（`background-clip` 默认 `border-box`），方形贴图的四角因此被切掉。

**单选框为什么没事**：它的共享基础规则 `html[data-theme] [data-slot="radio-group-item"][data-choice-layout="control"]` **没有** `button` 元素，是 (0,3,1)，与主题覆盖同分，靠文档顺序（`appearance.css` 在 `controls.css` 之后）取胜。

**修复**：删掉那个多余的 `button` 元素 → 变成 (0,2,1)，与 8bitcn / Minecraft 的覆盖同分，后者在文件里更靠后，于是正常胜出。它仍然压得住更低的 `[data-slot="checkbox"]` 基础规则，shadcn 的 4px 不受影响。

> 注意这不是"删掉 4px"——`4px` 是 shadcn 的正确取值，问题只在优先级。测试里专门加了这条反向校验：若有人图省事删掉声明，测试会失败。

**新增测试**（`minecraft-controls.test.mjs` 现 8 个用例）：断言 Minecraft 复选框胜出的 `border-radius` 为 `0`，并校验共享层那条 `4px` 仍然存在。**测试先写、修复前失败**，失败信息直接指出肇事选择器：

```
the texture is clipped by html[data-theme] button[data-slot="checkbox"] in ui/styles/controls.css
```

**同类隐患（已排查，本次未改）**：`controls.css:52`（`box-shadow` 等重置）与 `controls.css:152`（单选框基础外观）也带了 `button` 元素，同样是 (0,2,2)。目前没有可见影响——这两个属性上主题覆盖或另有同分规则接管——但若将来主题想覆盖这些属性，会踩相同的坑。
### 回归修复：音符盒键盘（`tsx-glued-class-token`）

人工验收发现 Minecraft 主题下音符盒键盘整体错乱：有升调的按键**文字全部消失**、按键失去固定宽度被拉成整行、所有音阶字符串**叠在区域中央**、变音符号跑到**区域右上角**。

**根因是 Phase 4.5 清理 `btn` 标记时的脚本缺陷。** 那一步把 65 处调用点的 `btn` 从 `className` 里删掉，其中 17 处 `btn` 后面还跟着别的类名。脚本用

```js
body.split(/(\s+)/).filter((piece) => !/^\s+$/.test(piece) && piece !== "btn").join("")
```

把**空白分隔符和 `btn` 一起过滤掉了**，于是

```
"btn flake-page__note-key-button is-sharp"  →  "flake-page__note-key-buttonis-sharp"
```

`is-sharp` 被粘到前一个类名上，得到一个新的、谁都不认识的类名。结果两条规则同时失效：

| 失效的规则 | 丢失的声明 | 症状 |
| ---------- | ---------- | ---- |
| `.flake-page__note-keyboard button.flake-page__note-key-button.btn` | `position: relative`、`width/min/max-width`、`overflow: hidden` | 按键没有固定宽度；`.flake-page__note-key-label`（`position: absolute; inset: 0`）的包含块退到远处祖先，所有标签叠在该祖先中央；`overflow: hidden` 的裁剪也没了 |
| `.flake-page__note-keyboard button.flake-page__note-key-button.is-sharp` | `transform: translateY(-50%)` | 升调按键不再上移半分；`.flake-page__note-key-sharp`（`position: absolute; top: 3px; right: 4px`）同样相对于远处祖先定位，跑到区域右上角 |

第二条规则也解释了为什么**只有带升调的按键**完全没有文字：`.is-sharp` 失效后该按键的宽度约束一并消失。

**修复**：恢复被粘坏的类名（`"flake-page__note-key-button is-sharp"`）。脚本对全部 17 处做了同样的粘合风险检查，**只有这一处受损**（其余 16 处 `btn` 后只有一个类名，没有粘合可能）。

**护栏新增第 11 条规则：`tsx-glued-class-token`** —— 类名列表以空白分隔，若一个令牌恰好能拆成两个项目 CSS 里真实存在的类名，说明某次改写丢掉了分隔符。这类损坏对其它所有检查都是隐形的（`tsx-` 规则只看已知类名，粘合产生的是未知类名；`.btn` 由 CVA 基础串提供，元素上依然有），只有跑起来看页面才会发现，所以值得单独拦住。已用夹具验证可证伪。

> 教训：批量改写 `className` 的脚本**必须按令牌边界操作**——先把字符串切成令牌，删掉目标令牌，再用单个空格重新连接；不要用「过滤掉空白片段再 `join("")`」的写法。
### Phase 5 实施记录（部分完成）

#### ✅ 文件数：15 → 3（退出标准之一达成）

`nav-icons.css` 与 `widget/*.css`（12 个）合并为 `widgets.css`，`theme.css` 与 `appearance.css` 保留：

| 文件 | 行数 |
| ---- | ---- |
| `theme.css` | 25（入口 + 主题级令牌） |
| `widgets.css` | 820（原 13 个文件） |
| `appearance.css` | 222（调色板与逐控件外观） |

**合并的唯一风险是顺序**：这些规则全在 `@layer components` 里，文件间的 `@import` 顺序就是层叠顺序。所以合并脚本按 `theme.css` 的导入顺序拼接，并在删除任何文件之前**逐条比较合并前后的 (选择器, 属性) 序列**——81 条规则完全一致才继续。

**合并踩到的一个坑：相对资源路径。** 这些文件原来在 `widget/` 下，`url("../resource/…")` 指向上一级；合并到主题根目录后同样的写法会指向 `ui/themes/resource/…` —— **40 条贴图路径全部失效**。已改为 `./resource/…`，并顺带加了第 10 条护栏规则 `css-unresolved-asset`（见下）。

#### 5a ✅ 标记迁移：51 个裸元素 → 0

方案原本要把 MC 的裸元素选择器改写为 `[data-slot]`，但页面里还有 51 个裸的表单/表格元素**依赖**这些选择器，收窄会立刻让它们失去样式。所以先做标记迁移：

| 文件 | 迁移前 | 内容 |
| ---- | ------ | ---- |
| `windows/enumerator/EnumeratorWindow.tsx` | 40 | 5 个 `<input>` → `<Input>`；3 组 `<table>` → `<Table>/<TableHeader>/<TableBody>/<TableRow>/<TableHead>/<TableCell>` |
| `windows/asset_manager/AssetManagerContent.tsx` | 9 | 1 组 `<table>` |
| `main/pages/statistics/StatisticsPage.tsx` | 1 | `<textarea>` → `<Textarea>` |
| `components/VirtualSpacerCell.tsx` | 1 | `<td>` → `<TableCell>` |

现在全仓库（`ui/components/ui` 之外）**裸表单/表格元素为 0**。

**顺带让 `table.tsx` 转发 ref**（唯一一处偏离上游 registry 的改动）：`button`/`input`/`checkbox`/`textarea` 都转发 ref，只有 table 这一族没有；而虚拟占位单元格必须拿到 ref 才能发布高度。已在文件头注明原因。

**虚拟占位单元格的几何没有被破坏**：`TableCell` 会带上共享的 `[data-slot="table-cell"] { padding: 10px 12px }`，而 `.material-list-virtual-spacer-cell` 早就写着 `padding: 0 !important`（`enumerator.css`、`materials.css`），`!important` 压得住，高度仍然精确。

> **⚠️ 这一步会改变那 4 个页面的观感。** 加了 `data-slot` 之后，共享层的 `[data-slot="table-head"] { background: var(--muted); color: var(--muted-foreground); font-weight: 500 }`、`[data-slot="table-cell"] { padding: 10px 12px }`、表头底色、行 hover 等规则**第一次作用到这些表格上**（以前它们是裸元素，只有页面自己的 CSS）。这正是统一的目标，但属于可见变化，需要按第 5 节矩阵逐页确认。输入框同理：`<Input>` 自带 `nova-control input` 基础类，比裸 `<input class="input">` 多出 `data-slot="input"` 相关的焦点与禁用样式。

#### 5b ✅ MC 裸元素选择器收窄：42 → 0

标记迁移完成后，`ui/themes/minecraft/` 的裸元素选择器已全部收窄到 `[data-slot]`。**Phase 5 的两条退出标准均已达成：文件数 3（≤6）、裸元素选择器 0。**

盘点后共有 **116** 处裸元素选择器，走了两条不同的改写路径，搞反就会写出永不匹配的选择器：

| 形态 | 处理 | 例 |
| ---- | ---- | -- |
| 复合选择器里**已有** `data-slot` | **删除**多余的裸元素名 | `button[data-slot="checkbox"]` → `[data-slot="checkbox"]` |
| 复合选择器里**没有** `data-slot` | **替换**为对应槽位 | `button.material-list-close-button` → `[data-slot="button"].material-list-close-button` |
| `input[type="text"]` 一族 | 前缀替换，保留 type 过滤 | `[data-slot="input"][type="text"]` |
| `:where(input, textarea)` 列表 | 逐个替换，`select` 直接丢弃 | `:where([data-slot="input"], [data-slot="textarea"])` |
| `table > tbody > tr:nth-of-type(odd) > *` | 每个复合段分别替换，子代组合器保留 | `[data-slot="table"] > [data-slot="table-body"] > [data-slot="table-row"]:nth-of-type(odd) > *` |
| `input[type="button"\|"submit"\|"reset"]`（15 处） | **整条删除** | 已确认不存在这类元素（所有输入都是 `Input` 组件） |

**`svg` 与 `a` 保留裸选择器**：它们没有对应组件、也没有槽位可挂，护栏的裸元素清单也不包含它们。

**行为变化（已确认可接受）**：MC 的 `button` 规则不再作用于 Radix 渲染的按钮（复选框、单选框、下拉触发器）。它们本来就不该继承按钮贴图 —— `appearance.css` 里那一堆"撤销按钮贴图"的声明就是为抵消它而存在的。现在它们只由各自的槽位规则和 `controls.css` 决定外观，继承链更短。

**改动量**：119 处选择器改写（`appearance.css` 20、`widgets.css` 99），构建产物中 `[data-slot=button]` 存在、裸 `button` 规则已消失，56 张贴图仍在。

**验证**

| 检查 | 结果 |
| ---- | ---- |
| `ui/themes/minecraft/` 裸元素选择器 | ✅ **0**（原 42） |
| `ui/themes/minecraft/` 文件数 | ✅ **3**（原 15） |
| 全项目 `css-components-bare-element-selector` | ✅ 167 → **125**（剩余全在共享层与其它主题） |
| 层叠反转 | ✅ 0 |
| `tsc` / `check:layers` / `check:application-boundaries` / `check:ui-styles` / `build` | ✅ 全部通过 |
| 测试套件 | ✅ 与 HEAD 完全一致（`check:ui-styles` 18 个用例） |

**待人工验收**：Minecraft 主题下的完整走查 —— 这次是真正改变匹配范围的一步。重点：三主题的表格（表头底色、行悬停、奇偶行）、输入框/文本域的焦点与禁用态、按钮的五个状态与三个变体、复选框/单选框/下拉触发器（它们不再继承按钮贴图）、滚动条、Unifont 字体。
#### 新增护栏规则：`css-unresolved-asset`

每个 `url()` 必须解析到真实文件。断链是**静默**的——浏览器只是不绘制，只有跑起来看才发现少了贴图；而移动/合并样式表正好会批量打断相对路径（本次就断了 40 条）。

- 只屏蔽注释、**保留字符串内容**：`maskCssText` 会连字符串一起屏蔽，而 `url()` 的路径正是字符串，用它会让规则变成空转（这条是被新加的单测抓出来的，最初的"可证伪"验证也因此假通过）。
- 跳过 `data:` / `http(s):` / `#`（文档内 SVG 引用）。
- 仓库当前 **0** 条；测试用夹具验证会触发。

#### 验证

| 检查 | 结果 |
| ---- | ---- |
| 合并前后规则序列 | ✅ 81 条完全一致 |
| 主题 CSS 中 `url()` 解析 | ✅ 57 条全部有效 |
| 构建产物 | ✅ 56 张 MC 贴图以 data URI 内联在主题 chunk 中 |
| 层叠反转 | ✅ 0 |
| `check:layers` / `check:application-boundaries` / `check:ui-styles` | ✅ 0 违规 |
| `test:ui-styles` | ✅ 18 个用例 |
| 测试套件 | ✅ 与 HEAD 完全一致（本次修改顺带修正了 3 个测试里写死的旧文件路径） |

**待人工验收**：三主题的 Minecraft 主题整体观感——本次只是文件重排，规则未改，但贴图路径从 `../resource/` 改成了 `./resource/`，值得确认按钮、滚动条、输入框、字体（Unifont）都正常加载。
### Phase 6 — 页面 CSS 收编

**做**：把 `ui/windows/**/*.css`（8 个文件，913 行）收进 `utilities` 层；删除其中的外观属性（`background`、`border`、`box-shadow`、`transform`），保留布局属性。`library.css:31,56,365` 的手动清零规则在本阶段删除——层序已保证 `components` 优先。

**退出**：页面 CSS 中外观属性为 0；无 `border-image: none` / `box-shadow: none` / `transform: none` 一类补丁。

---

### Phase 6 实施记录（退出标准经修正）

#### 原退出标准不成立

原方案要求"页面 CSS 中外观属性为 0"。实测 `ui/windows/**/*.css` 8 个文件 913 行里有 **188 条外观声明**，按用途分：

| 类别 | 数量 | 能否删 |
| ---- | ---- | ------ |
| 零化补丁（`0` / `none` / `transparent` / `inherit`） | 66 | 逐条判断 |
| **真正的页面设计**（底色、圆角、边框、渐变） | 106 | **不能** |
| `outline` 焦点环与拖放目标高亮 | 15 | **不能**（无障碍/功能） |
| 定位变换（`translate`，用于居中） | 1 | **不能** |

关键事实：这些设计挂在**普通元素**上，没有组件能给它们外观——

- `.library-tile` → `<article className="library-tile">`
- `.library-collection` → `<section className="library-collection">`
- `.library-tag-chip` → `<span>`

删掉 `background` / `border-radius` 不会"统一"到组件外观，只会让卡片、磁贴、标签变成**透明无框**。`outline` 那 15 条里包含 `.library-category[data-drop-target]` 的拖放高亮和 `.library-tile:has(.library-tile-open:focus-visible)` 的键盘焦点环，删掉是功能与无障碍回归。

**修正后的退出标准**：*留下的每一条外观声明，都必须能说明"为什么这个属性必须由页面自己拥有"*。据上述分类，页面自有外观属于正当职责；只有"纯属撤销 components 规则且已无对手"的声明才算冗余。

#### 实际清理：43 行僵尸规则

按新标准，可**证明**冗余的只有一类：**选择器已无法匹配任何元素**（孤儿规则）。判定方式不是猜——把全部 TSX 的类名令牌（含模板字符串的静态前缀）建成集合，只有当一条规则里的**所有**类名都不在其中时才算孤儿。逐个复核 21 个候选类名，并用 AST 排除 `library-category-${kind}` 这类动态拼接的可能。

删掉 39 条孤儿规则、43 行，分布在 6 个文件：`library.css`(-7)、`replace.css`(-14)、`statistics/materials.css`(-10)、`render.css`(-6)、`properties.css`(-4)、`settings.css`(-1)。典型如 `.library-tag-form`、`.replace-block`、`.beta-statistics-non-items`、`.render-progress-fill`——都是页面改版后遗留的。外观声明 188 → 181（8 条随孤儿规则一起消失），布局声明 1550 → 1440。

**66 条零化补丁一条都没删**：脚本查重（同选择器重复声明、被后续同选择器覆盖）结果为 0，说明没有一条是"写了两次"。它们各自都还在压着一个真实的 `components` 规则——例如 `.library-tile-open` 上的 `border: 0` / `border-image: none` 是让磁贴里的按钮**不要**长出 Minecraft 的按钮贴图，删掉就是回归。这类"页面故意让组件变平"的声明属于合法覆盖，不是僵尸。

#### 踩到的坑：`@layer a, b, c;` 是语句型 at-rule

清理空 `@media` 块时，判定写成"没有子节点即为空"，于是把每个文件顶部的 **`@layer theme, vendor, tokens, base, components, utilities;`** 也当成空块删掉了——它是*语句型* at-rule，本来就没有子节点。8 个页面的层序声明全被抹掉，会静默改变层序。

修复方式：判定必须先排除语句型（`at.nodes === undefined` 直接跳过），只处理 `@media` / `@supports` / `@container` 这类*块型*。教训：**"没有子节点"不等于"空"**——at-rule 有两种形态，删除类操作必须先分辨形态。

#### 未做与决定

- **页面 CSS 不迁入 `utilities`**。`utilities` 在 `components` 之上，迁过去会让页面压过主题；而不迁、留在 `components`（现状）已经正确：页面用 `html[data-theme]` 前缀和更高特异度拿到它需要的覆盖。那份"把页面外观属性删空再迁层"的方案在数据上不成立，故不执行。
- **孤儿检测脚本留在 `.tmp/`**（`.tmp/phase6-redundant.mjs`），不提升为护栏规则：动态拼接类名（模板字符串）会让静态判定产生假阳性，作为常驻护栏风险大于收益。它更适合作为周期性维护工具手动跑。
- `analyze-cascade-flips.mjs` 同样留在 `.tmp/`：它需要人工解读，不适合放进 CI 判定。

#### 验证

| 检查 | 结果 |
| ---- | ---- |
| 页面 CSS 改动 | ✅ 纯删除（6 文件 -43 行，无新增） |
| 8 个页面 CSS 仍各含 1 条层序声明 | ✅ |
| `check:layers` / `check:application-boundaries` / `check:ui-styles` | ✅ 0 违规 |
| `tsc` / `build` | ✅ |
| 测试套件 | ✅ 与 HEAD 完全一致 |
| 发现数 | 171（未变，被删规则本就无违规） |

**待人工验收**：8 个页面在 9 组主题/明暗下的外观——本次只删了匹配不到元素的规则，理论上零影响，但涉及 6 个文件，建议扫一眼 library、replace、render、statistics 四个改动最多的页面。

### 按钮选中态：上游没有，我们也没画出来（V1.21）

#### 先回答“是库没有，还是我们漏了”

**两者都成立**，只是不在同一层：

| 上游文件 | 声明的状态 | 有没有选中态 |
| -------- | ---------- | ------------ |
| shadcn/ui `new-york-v4/button.json` | `hover` / `focus-visible` / `disabled` / `aria-invalid` | **没有**。`buttonVariants` 只描述“动作按钮”的六个变体与四个状态 |
| shadcn/ui `new-york-v4/toggle.json` | `hover` / `focus-visible` / `disabled` / `aria-invalid` + `data-[state=on]:bg-accent data-[state=on]:text-accent-foreground` | 有，在 **Toggle** 上 |
| 8bitcn/ui `button.json` | 包一层 shadcn Button，只加 `active:translate-y-1` 与像素边框装饰 | **没有**（`active` 是按下瞬间，不是选中态） |
| 8bitcn/ui `toggle.json` | `data-[state=on]:bg-primary data-[state=on]:text-primary-foreground` | 有，在 **Toggle** 上，且口径与 shadcn 不同（主色实心，不是 accent） |

所以“Button 没有选中态”是这两个 registry 的标准分工：**动作**归 Button，**开关**归 Toggle。但 Nova 并没有 Toggle——它把 Button 当开关用，全仓 23 处 `aria-pressed`（9 个文件：侧栏导航、素材库分类与图标页签、Reden 提供方与结果卡、资产管理器来源、flake 工具与快捷栏、渲染/属性页模式切换、UI 测试页）。这部分是**我们自己的缺失**：`themes/modern.css` 里确实写着一条 `button[aria-pressed="true"] { background: var(--accent); border-color: var(--primary) }`，但它从来没生效过。

#### 根因：一条 (0,1,1) 的规则去撞 (0,4,1)

```
button[aria-pressed="true"]                          (0,1,1)  themes/modern.css
html[data-theme]:not(...) [data-slot="button"]
  [data-variant="secondary"]                         (0,4,1)  styles/controls.css   ← 恒胜
button[data-variant="ghost"]                         (0,2,1)  themes/modern.css      ← 也胜
```

`secondary` 是 CVA 的默认变体，普通 `<Button>` 全是它；`ghost` 覆盖了导航行、素材库磁贴、快捷栏槽位。两条规则都只声明“常态长什么样”，于是**按下的按钮与常态逐像素相同**。这不是本轮收敛引入的回归：`controls.css` 的 `secondary` 规则在收敛前就已存在（`706697fe^` 可查），这条 `aria-pressed` 规则从写下的那天起就没赢过。Minecraft 之所以看不出问题，是它把选中态画在 `button_check_*.png` 贴图上，走的是另一条路径。

#### 修复

- **共享层声明状态，不复制 Toggle 组件**：`styles/controls.css` 新增 (0,5,1) 规则，只覆盖 `secondary` / `outline`，同时认 `aria-pressed="true"` 与 Radix Toggle 的 `data-state="on"`。
- **只动两个静默变体**：`default` / `destructive` 是实心的，填充本身就是渲染页与属性页模式切换的“选中外观”，改色会变成第二个主按钮；`ghost` / `link` 是结构控件（导航行、磁贴、快捷栏），选中外观归页面 CSS 所有，而 `controls.css` 在页面 CSS 之前加载。
- **令牌驱动、按各主题上游定口径**：新增 `--nova-selected-surface` / `--nova-selected-hover` / `--nova-selected-foreground`。
  - shadcn：**不用 accent**。本主题常态按钮已经是 `--secondary`（#e4e4e7），而 accent（#eeeeef）比它更亮——实测 1.09:1，等于没有选中态。改为 `color-mix(in oklab, var(--secondary) 74%, var(--primary))`，即“向动作色走一步”：与常态 1.85:1、与页面 2.25:1、正文仍有 7.6:1；深色下 2.03:1 / 2.85:1 / 5.9:1。悬停再走一步（70%），避免悬停回到更亮的 accent 造成“按下后反而被取消”的错觉，同时压住步长以保住正文对比度。
  - 8bitcn：按其自家 Toggle 用 `--primary` 实心板，阶梯边框改 `--primary-foreground`（原 `--border` 绿会溶进填充），外投影改内嵌阴影，读起来像“压下去”的像素板。
  - Minecraft：贴图不动，只把 `data-state="on"` 也加入 check 贴图规则，将来换成真 Toggle 不会静默掉状态。
- 删除 `themes/modern.css` 里那条从未胜出的 `button[aria-pressed="true"]`（外观只留一处来源）。

#### 踩到的坑：令牌不能声明在 `components` 层

第一版把三个 `--nova-selected-*` 写在 `modern.css` 的 `@layer components :root` 里，8bitcn 的覆写写在 `@layer tokens :root`。层序是 `theme < vendor < tokens < base < components < utilities`，于是**低层的覆写被高层的基础声明压过**——截图上 8bitcn 的“选中”仍是浅绿而不是主色。把基础声明移到 `tokens` 层后，同层内按样式表顺序（8bitcn 的 sheet 在 `modern.css` 之后）决出，覆写才生效。这和 V1.7 的复选框、V1.19 的圆角是同一类错误：**先看层，再看特异性，最后才看源码顺序**。

#### 验证

| 检查 | 结果 |
| ---- | ---- |
| 新增 `button-selected-state.test.mjs` | ✅ 8 用例：三主题选中值、常态不变、实心/结构变体不被接管、两种属性等价、模型自检 |
| 级联求解器抽成 `scripts/fixtures/css-cascade.mjs` | ✅ 与 `minecraft-controls.test.mjs` 共用；新增 `:is()`/`:where()` 展开与 `:not([data-theme="x"])` 排除 |
| 可证伪 | ✅ 把选中底色改回 `var(--secondary)` → 用例失败；改回即通过 |
| `check:ui-styles` / `check:layers` | ✅ 169 = 基线，0 回归；0 新增层违规 |
| `tsc --noEmit` | ✅ |
| 六张无头 Edge 截图（三主题 × 明暗） | ✅ 见 `.tmp/preview/`（临时验证页，未入库） |

> 未能执行：`vite build`。本机沙箱禁止 esbuild 启动子进程（`spawn EPERM`），构建需在正常终端里跑一次。CSS 本身已由 postcss 解析（测试即解析全部样式表），未改任何 TSX。

#### 待人工验收

- 三主题 × 明暗下 `UiTestPage` 的“按钮”与“按钮变体”两行：点击循环 常态 → 按下 → 禁用，按下态现在应当**一眼可分**。
- 业务里的 23 处 `aria-pressed`：资产管理器的来源/子选项、Reden 提供方与结果卡、素材库图标页签、flake 编辑工具与快捷栏、渲染/属性页模式切换（后者用 `default`+`ghost` 切换变体，观感应不变）。
- 结构控件应保持原样：侧栏导航项、素材库分类行、磁贴、快捷栏槽位、音符键（它们各自有页面级选中样式，不能被通用规则接管）。

## 5. 回归矩阵

每阶段结束执行。构建与 CLI 检查见 [DEVELOPMENT.md](DEVELOPMENT.md)。

| 维度 | 检查项 | 方式 |
| ---- | ------ | ---- |
| 主题 | 三主题 × 浅色/深色/跟随系统，共 9 组 | 人工逐组截图 |
| 控件 | `UiTestPage`（`ui/windows/main/pages/ui-test/UiTestPage.tsx`）全部控件，在 9 组下外观正确 | 人工 |
| 控件 | 复选框 checked / unchecked / indeterminate；单选 checked / unchecked；滑块拖动、禁用、键盘；下拉打开、键盘选择、溢出滚动 | 人工 |
| 结构 | `ui/windows/main/pages/` 下 10 个页面 + `ui/windows/` 下 6 个子窗口，在 9 组下无错位 | 人工 |
| 层叠 | `dist/assets/*.css` 无层规则数为 0；`components` 层无裸元素选择器 | `npm run check:ui-styles` |
| 层叠 | **构建产物中 `.bg-primary` 位于 `vendor` 层**（vendor CSS 一旦排到 `components` 之上，控件外观会被 Tailwind 工具类接管，而这条只有看产物才能发现） | 构建后检查 |
| 约束 | 无新增无层规则；无新增 `.btn-*`；业务代码外观工具类为 0；无未定义令牌引用；无系统色（`forced-colors` 除外）；vendor 导入不在 `components` 之上 | `npm run check:ui-styles` |
| 护栏 | 检测规则与基线机制本身未被改坏；仓库基线未陈旧 | `npm run test:ui-styles` |
| 既有 | `npm run build`、`check:layers`、`check:application-boundaries`、`test:layer-check`、`test:application-boundaries` | CLI |
| 焦点 | Tab 顺序、焦点环可见性、Radix 焦点陷阱、`prefers-reduced-motion` | 人工 |

> 说明：仓库现有文档已明确"构建及 CLI 通过不代表实机通过"（`DEVELOPMENT.md:60`）。主题视觉属于必须人工验收的项目，不得以静态检查替代。

---

### 上游合并后的收敛流程

上游的开发重心在渲染核心，UI 常带着未优化的样式回来。合并后按下面的顺序收敛，不要手工重做：

1. **先量再改**：`npm run check:ui-styles` 会以计数基线报出回归；`npm run check:ui-styles:report` 看总量。确认新增项来自上游后，用 `npm run check:ui-styles:update-baseline` 接受现状（基线是“不再增长”的闸门，不是目标值）。
2. **页面外观重新拆分**：页面 CSS 又混入外观声明时，运行 `node .tmp/split-page-appearance-2.mjs`（脚本顶部 `TARGETS` 改成需要处理的文件；它会按原选择器与 `@container` 上下文搬进 `ui/styles/page-appearance.css`，并用 `(选择器, 属性, 值)` 多重集校验，丢失即中止）。脚本在 `.tmp/` 下、不进版本库，丢失时按 V1.30 的记录重写。
3. **裸元素**：先跑 `node .tmp/recon-bare-elements.mjs` 看每条的判定。`SAFE(slot-only)` 才能机械替换；`MIXED` 说明该元素存在不带槽位的用法，**不要**把 `button` 直接换成 `[data-slot="button"]`——素材库卡片/分类行用的是 `library-tile` / `library-category` 等专用槽位。
4. **主题与字阶**：`themes.test.mjs` 校验主题目录、id 与 Rust 一致；`css-theme-type-token` 校验四档字阶；新增主题漏定义会直接失败。
5. **层序**：`css-sublayer-assignment` 拒绝把规则写回父层 `components`，`css-vendor-layer-above-components` 拒绝 Tailwind 越层。

合并冲突的取舍原则：**优先保留本文件的层序声明、`@layer` 子层包装与 `data-slot` 化选择器**；上游新增的裸元素/字面色/`!important` 先进基线，再按批次处理，避免一次合并就把 UI 重新拖回“群魔乱舞”。

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
| `ui/button.tsx` | `button`, `button-decorations`；结构性容器经 `themeSlot` 改用自有槽位：`library-tile` / `library-category` / `library-category-expander` | 已消费 |
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