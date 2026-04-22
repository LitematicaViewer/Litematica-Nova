# 表格控件样式约定

## 总体目标（演进方向）

### 统一样式

- **目标**：应用中**所有** `QTableWidget`（及同类表格视图）在视觉与交互语义上**共用同一套**表格规范（度量、配色、字体与对齐等由统一 Token / 抽象层提供），避免按窗口、按 `objectName` 在多处 QSS 或零散 `setStyleSheet` 维护多套分叉。
- **与现状的关系**：当前仍以 `table_list_*` 列表型 Profile 为基准，并与 `themes/list_view_supplement` 等对少数表格的追加样式并存；后续实现抽象层时，以本文 Token 为准**逐步收敛**到单一来源。


## 跨主题通用约定

### 表的结构

**关于命名前缀 `table_list_`**：`table_list` 表示「**列表型表格**」这一 UI Profile（多列、可选缩略图/拖拽列、与 `ContentListTableWidget` 几何与交互对齐）。**不是**「内容物」业务模块的缩写；业务表请用各自的 `objectName` 区分。参考实现为 UI 测试页 `UITestContentListTable`。**Minecraft** 与 **Metro8 / Metro10** 共用同一套几何 Token（行高除外）与同一最小高度 **400**；配色与 Chrome 按主题在下列表中分栏约定。代码真源：**Metro** 见 `litematicaba/ui/table/metro10_list_profile.py`（含 `content_list_row_height_px`）；**Minecraft** 列表型配色、supplement QSS 片段、行高/视口常量见 `litematicaba/ui/table/minecraft_list_profile.py`（`MINECRAFT_LIST_TABLE_TOKENS`、`minecraft_list_view_supplement_qss` 等）。

#### 表头行高

#### 单元格行高 - `table_list_row_height`
用于确定单元格行高；代码侧以 `setRowHeight` 为主（`content_list_row_height_px`）。**Minecraft** 下与 `themes/minecraft.py` 中 `QTableView::item` 的 `min-height`（**44px**）一致。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|60|与 `METRO10_LIST_TABLE_TOKENS.row_height` 同源|
|Metro10|metro10|60|参考 Windows 设置 > 应用和功能界面的行高|
|Minecraft|minecraft|44|常量 `minecraft_list_profile.TABLE_LIST_ROW_HEIGHT_MINECRAFT_PX`；mcmeta 操作表行高同值|

#### 列表最小高度 - `table_list_min_height`
列表型表格控件整体最小高度（内容物列表与 **Metro10 / Minecraft** 下 supplement 覆盖的 mcmeta 类表均 **400**）。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|400|内容列表构造；Metro8 supplement 表另见 `MCMETA_TABLE_MIN_HEIGHT_PX_BY_THEME`|
|Metro10|metro10|400|控件 `setMinimumHeight`；`MCMETA_TABLE_MIN_HEIGHT_PX_BY_THEME["Metro10"]` 同源|
|Minecraft|minecraft|400|`MCMETA_TABLE_MIN_HEIGHT_PX_BY_THEME["Minecraft"]` 与内容列表均对齐本 Token|

#### 缩略图边长 - `table_list_thumb_px`
缩略图列内图标边长（正方形）。**Minecraft** 与 Metro 列表共用内容物列表布局，取值与 Metro 一致。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|40|列内垂直居中|
|Metro10|metro10|40|同上|
|Minecraft|minecraft|40|同上|

#### 自由排序拖拽列宽 - `table_list_drag_col_w`
自由排序模式下，拖拽手柄列固定宽度。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|24|仅自由排序模式使用|
|Metro10|metro10|24|同上|
|Minecraft|minecraft|24|同上|

#### 缩略图列宽 - `table_list_thumb_col_w`
缩略图列整体宽度（含列内边距占位）。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|56|固定列|
|Metro10|metro10|56|同上|
|Minecraft|minecraft|56|同上|

#### 列表扁平 Chrome（样式表片段）- `table_list_flat_chrome_qss`
表格外壳透明、**无外边框**、去网格线等；**仅 Metro8 / Metro10** 内容列表使用 `METRO_LIST_FLAT_TABLE_QSS`。**Minecraft** 内容列表为 `setStyleSheet("")`，表壳视觉由 **`list_view_supplement`** 与全局 Minecraft QSS 承担。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|（见 `METRO_LIST_FLAT_TABLE_QSS`）|`view.py`|
|Metro10|metro10|（同上）|同上|
|Minecraft|minecraft|（不适用本片段）|黑底表壳见 Minecraft 节 supplement 表|

#### 表格外边框（Metro10 收敛目标）
**不作为**跨主题 Token 表逐主题罗列；在 Metro10 上，与列表型 flat chrome 对齐时：**`list_view_supplement` 所覆盖的表格**（如版本管理表、操作表、语言文件表等 `_SEL` 目标）使用 **`border: none`**，与 `table_list_flat_chrome_qss` 一致。**Minecraft** 下同一批表在 supplement 中为 **2px #555555**（见 Minecraft 节）。

#### 网格线 - `setShowGrid`
是否显示单元格网格线**不纳入**「全表统一 Token」：按主题分支或业务在代码 / QSS 中单独约定即可。本仓库惯例：**Metro 列表、Minecraft 内容列表、Metro10/Minecraft 下上述 supplement 表一般为 `setShowGrid(false)`**；其它主题若需网格线（例如 `ContentListTableWidget` 在非 Metro、非 Minecraft 分支）在实现里单独保留，不强行并入 `table_list_*`。

#### 表体视口末行下留白填充 - `table_list_viewport_below_row_fill`（mcmeta / supplement 表路径）
当表格 `setMinimumHeight` 大于实际行内容高度时，视口在末行之下可能露出「窗」底色；代码通过 `apply_mcmeta_table_viewport_fill_below_items(table, theme_id)` 将视口 `Window` 角色设为与表体主底色一致，避免 Metro 透明 item + QSS 误透出高亮色、或 Minecraft 黑表下露出白条。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#ffffff|与 `table_list_item_bg_primary`（Metro 浅色表）一致|
|Metro10|metro10|#ffffff|同上|
|Minecraft|minecraft|#000000|与 supplement 中表 `background-color` 一致；`minecraft_list_profile.MCMETA_VIEWPORT_FILL_BELOW_ITEMS_MINECRAFT_HEX`|

### 表的配色方案

下列 Token 面向「列表型表格」Profile。**Metro8 / Metro10**：与 `_MetroContentListDelegate` 自绘一致（无斑马纹）。**Minecraft**：`setAlternatingRowColors(true)`，常态文本列由样式引擎铺斑马纹；悬停/选中由 `_MinecraftContentListDelegate` 自绘；色值见下表「Minecraft」列。

#### 表头背景颜色 - `table_list_header_bg`
表头区域常态下的默认背景色。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#ffffff|透明段 QSS + 表头 section 透明|
|Metro10|metro10|#ffffff|与 Metro10 扁平白底列表风格一致|
|Minecraft|minecraft|#000000|继承 `minecraft.py` 全局 QWidget / 表头绘制|

#### 表项背景颜色（主要） - `table_list_item_bg_primary`
常态下行内文本列底色（非悬停、非选中）。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#ffffff|Flat 列表|
|Metro10|metro10|#ffffff|Flat 列表无斑马纹时的默认单元格底|
|Minecraft|minecraft|#000000|内容列表斑马纹基底之一；与 supplement 表 `background-color` 一致|

#### 表项背景颜色（次要） - `table_list_item_bg_secondary`
启用交替行时的「偶数/奇数」另一路底色。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#ffffff|当前关闭交替行，与主要相同|
|Metro10|metro10|#ffffff|关闭交替行，与主要相同|
|Minecraft|minecraft|#1a1a1a|与 supplement `alternate-background-color` 一致|

#### 悬停行背景颜色 - `table_list_hover_bg`

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#e6e6e6|内容列表与 Metro10 共用 `_METRO_HOVER_BG`；mcmeta 类表悬停 token 另为 `#d0d0d0`|
|Metro10|metro10|#e6e6e6|与缩略图/拖拽列 cell widget 背景对齐|
|Minecraft|minecraft|#2b2b2b|`MCMETA_TABLE_ROW_HOVER_BG_HEX_BY_THEME["Minecraft"]`；整行悬停语义|

#### 选中行背景颜色 - `table_list_selected_bg`

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#cccccc|与 Metro 列表 delegate 一致时可查|
|Metro10|metro10|#cccccc|行选时文本列与 widget 列对齐|
|Minecraft|minecraft|#3c3c3c|内容列表行选；`TABLE_LIST_SELECTED_BG_MINECRAFT_HEX`；与全局 QWidget 背景 **#3c3c3c** 一致，**非**操作表 QSS 的 `#000000` 选中|

#### 表头文字颜色 - `table_list_header_text_fg`
表头内常态下的默认文本色。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#000000|随 Metro 列表|
|Metro10|metro10|#000000|与 `_MetroContentListDelegate` 中 `_METRO_TEXT_FG` 一致|
|Minecraft|minecraft|#c6c6c6|继承 `minecraft.py` 全局 `color: #c6c6c6`（表头未单独覆写时）|

#### 表项文字颜色 - `table_list_text_fg`
常态下的默认文本色。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#000000|Metro delegate|
|Metro10|metro10|#000000|同上|
|Minecraft|minecraft|#ffffff|内容列表自绘分支 `_MinecraftContentListDelegate`；`MCMETA_TABLE_ROW_HOVER_FG` 可选|

#### 悬停行文字颜色 - `table_list_hover_text_fg`
悬停下的默认文本色。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#000000|未单独改色|
|Metro10|metro10|#000000|未单独改色|
|Minecraft|minecraft|#ffffff|与 `table_list_text_fg` 一致|

#### 选中行文字颜色 - `table_list_selected_text_fg`
选中下的默认文本色。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|#000000|未单独改色|
|Metro10|metro10|#000000|未单独改色|
|Minecraft|minecraft|#ffffff|与 `table_list_text_fg` 一致|

### 表的文字样式

#### 表头文字字体 - `table_list_header_font_family`
表头内字体族。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|系统默认字体（运行时）|同 Metro10 路径|
|Metro10|metro10|系统默认字体（运行时）|`metro10.py` 未设 `font-family`；由 `theme.py` 用 `_cached_base_font` 重置|
|Minecraft|minecraft|Unifont 等|`minecraft.py`：`"Unifont", "GNU Unifont", "Courier New", monospace`；|

#### 表头文字大小 - `table_list_header_font_size`
表头文字字号。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|10pt|随 Metro 全局|
|Metro10|metro10|10pt|来自 `metro10.py` 顶层 `QWidget { font-size: 10pt; }`|
|Minecraft|minecraft|12pt|来自 `minecraft.py` 顶层 `QWidget { font-size: 12pt; }`|

#### 单元格文字字体 - `table_list_cell_font_family`
单元格内字体族。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|系统默认字体（运行时）|同 Metro10|
|Metro10|metro10|系统默认字体（运行时）|由 `theme.py` 字体重置|
|Minecraft|minecraft|Unifont 等|同表头，全局 Minecraft QSS|

#### 单元格文字大小 - `table_list_cell_font_size`
单元格文字字号。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|10pt|随 Metro 全局|
|Metro10|metro10|10pt|随 Metro 全局|
|Minecraft|minecraft|12pt|随 Minecraft 全局|

#### 标准列文字对齐方向 - `table_list_standard_col_text_align`
标准文本列（如 名称/时间/额外列）的对齐方向。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|Left + VCenter|`view.py` `_alignment_for_column_header`|
|Metro10|metro10|Left + VCenter|除「大小」外左对齐|
|Minecraft|minecraft|Left + VCenter|同一逻辑|

#### 数据列文字对齐方向 - `table_list_data_col_text_align`
数值类数据列（当前为“大小”）的对齐方向。

|主题|内部名称|值|备注|
|---|---|---|---|
|Metro8|metro8|Right + VCenter|「大小」列|
|Metro10|metro10|Right + VCenter|同上|
|Minecraft|minecraft|Right + VCenter|同上|


## 特定主题约定

### Metro10

#### 基准样本（抽象层对齐目标）

| 项 | 内容 |
|---|---|
| `objectName` | `UITestContentListTable` |
| 实现类 | `ContentListTableWidget`（`src/litematicaba/ui/content_display/list_table/view.py`） |
| 主题判定 | `Metro8` / `Metro10` 走同一套「Metro 列表」分支（`is_metro_list_table_theme`，见 `litematicaba/ui/table/metro10_list_profile.py`） |

#### 列表型表格（Metro10）补充说明

- **代码真源**：Metro10 下列表型 Token 的集中定义与扁平 Chrome 行为见 `litematicaba/ui/table/metro10_list_profile.py`；`ContentListTableWidget` 与 `list_view_supplement` 中 Metro10 相关数值从该模块派生。UI 测试页在 Metro10 下通过 `bind_ui_test_tables_to_metro10_list_profile` 将内容物列表与操作表显式对齐到同一 Profile。
- **度量、配色、文字样式**：均以 `## 跨主题通用约定` 中 `table_list_*` Token 为准（`### 表的结构`、`### 表的配色方案`、`### 表的文字样式`）。
- **与 `list_view_supplement` 的关系（Metro10）**：版本/操作/语言等表走 supplement 时，**控件最小高度**与 **`table_list_min_height`（400）** 一致（见 `themes/list_view_supplement.py` 中 `MCMETA_TABLE_MIN_HEIGHT_PX_BY_THEME["Metro10"]`）。**外边框**为 **`none`**，与 `table_list_flat_chrome_qss` 一致（见上文「表格外边框」）。
- **`::item:selected`（supplement 路径）**：这些表业务上多为 `NoSelection`，但首列常带 `ItemIsSelectable`，Qt 仍可能出现 current/样式表选中绘制；若将选中画成高饱和蓝（如 `#0078d4`），易在表体下方留白处误铺色。当前实现为 **背景 `#ffffff`、文字 `#1a1a1a`**，与「无行选 + 自绘行悬停」一致。**列表型 Profile** 的**行选**仍以 `ContentListTableWidget` / `_MetroContentListDelegate` 为准（选中底 `#cccccc`），两类表勿混用选中语义。

#### 抽象层草案（Metro10 首版）

**目标**：各业务表通过「抽象层 + Metro10 实现」应用同一套 Profile，避免在业务代码中散落色值与 `setRowHeight`；`QTDefault` 仍走系统样式，不强制套用本 Profile。

**建议职责划分**

| 模块职责 | 说明 |
|---|---|
| **Token 定义** | 各 `####` 小节定义 Token 名称；表格中的「内部名称」字段表示主题内部名称（如 `metro10`），值列填写该主题下的取值。 |
| **Chrome** | 对 `QTableWidget`：`setShowGrid`、`setAlternatingRowColors`、可选 `setStyleSheet`（扁平透明）、表头透明、鼠标追踪策略（若需行悬停）。 |
| **Metrics** | `setDefaultSectionSize` / 逐行 `setRowHeight`、列宽策略（固定列 + Stretch）、可选 `setMinimumHeight`。 |
| **交互与绘制** | 需要与列表型表格基准一致的行悬停/选中时，使用统一 delegate 或复用现有 `_MetroContentListDelegate` 的绘制契约；含 `QWidget` 的列需同步背景色。 |
| **与补充 QSS 衔接** | 仅对约定 `objectName` 的表格追加 `list_view_supplement`；业务侧保持 `objectName` 稳定，抽象层不负责硬编码对话框选择器。 |

**建议应用顺序（伪流程）**

1. `normalize_theme_id` → 若为 `Metro10`（且非仅系统主题场景），进入 Metro Profile。
2. `apply_metro10_table_chrome(table)`：网格/交替行/样式表/追踪。
3. `apply_metro10_table_list_metrics(table, options)`：行高、最小高度、列宽 profile（是否含缩略图列、是否含拖拽列由 options 决定）。
4. `apply_metro10_table_list_interaction(table, delegate_policy)`：安装 delegate、连接 `selectionModel` 与 viewport 悬停（若与列表型表格基准一致）。
5. 业务填充数据后再次 `sync_row_heights`（若行数动态变化）。

**不在本草案范围内**：`QTDefault` 的完全系统风表格。**Minecraft** 的列表型度量与配色已并入上文 **`table_list_*`** 各表；下列仅作实现索引。

---

### Minecraft（补充索引）

- **抽象层**：`litematicaba/ui/table/minecraft_list_profile.py` — `MinecraftListTableTokens`、`apply_minecraft_content_list_chrome_flags`、`minecraft_list_view_supplement_qss`（由 `list_view_supplement` 注入选择器）、`bind_ui_test_tables_to_minecraft_list_profile`（UI 测试页与 Metro10 绑定对称）。
- **全局 QSS**：`themes/minecraft.py` 中 `QTableView::item` **min-height** 来自 `minecraft_theme_table_item_min_height_px()`，与 `table_list_row_height` 一致。
- **内容列表**：`ContentListTableWidget` 在 Minecraft 下调用 `apply_minecraft_content_list_chrome_flags`，delegate 为 `_MinecraftContentListDelegate`（`view.py`），色值取自 `MINECRAFT_LIST_TABLE_TOKENS`。
- **Supplement**：`LIST_VIEW_QSS_BY_THEME["Minecraft"]` 由 `minecraft_list_view_supplement_qss` 生成；选择器 **`_SEL_MC_ALL_TABLES`** 等定义仍在 `list_view_supplement.py`。
- **mcmeta 操作表 / 语言表**：`widgets/mcmeta_standard_table.py`（`setAlternatingRowColors(true)`、`apply_mcmeta_table_viewport_fill_below_items`）。
- **已决议（原冲突表）**：行高 **44**、列表与 mcmeta 表最小高度 **400**、视口留白填充 **按主题依据** `table_list_viewport_below_row_fill`、内容列表选中 **#3c3c3c**。
