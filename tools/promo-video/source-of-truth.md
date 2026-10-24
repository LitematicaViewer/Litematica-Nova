# Nova 0.4 视频产品事实与素材表

正式来源：`https://github.com/LitematicaViewer/Litematica-Nova/releases/tag/beta-0.4.0`。
冻结源码 Tag `3cd63`，公开 main `8a0ee8cd`；只读该发布树，不用开发线宣称功能。路径以下均指发布树。

| 功能 | 0.4证据 | UI状态/镜头 | 素材 | 风险/处理 |
|---|---|---|---|---|
| 十项一级导航 | `desktop-nova/ui/shell/PrimaryNavigation.tsx` | S03、全片产品窗口 | 发布组件布局/真实文案 | 视频UI演出，持续标演示数据 |
| 投影库搜索、分类、网格、详情、收藏 | `ui/windows/main/pages/library/{LibraryPage,LibraryToolbar,LibraryDetail}.tsx` | S05–S10 | 原创展馆GPU预览与演示卡片 | 不冒充真实用户库或真实导入录屏 |
| Fast/Standard/Full入口；图像统一Standard | `pages/render/RenderPage.tsx`、发布说明 | S11–S14 | 实际建筑图仅使用已确认Standard帧；其余模式只演入口控件 | 不做三模式实景对照，不将Standard图换标签演Fast/Full |
| 二维分层 | `pages/flake/FlakePage.tsx`，正式Core `visualize-layer`输出 | S15–S16 | `layer-32/44/60.json` | 明确标二维Core数据视图，非三维GPU层图 |
| 结构/材料/容器统计 | `pages/statistics/StatisticsPage.tsx`，正式Core `stats` | S17–S20 | `observatory-stats.json` | 当前原创投影0容器，显示0，不编容器内容 |
| CSV/网页材料导出 | `executeConfiguredMaterialExport`，正式Core `materials-export` | S21、S32–S36 | `exports/Nova-Observatory-materials.csv/.html` | 两文件实际存在，15行CSV；不虚构文件生成 |
| 独立备货任务/准备与剩余/保存恢复 | `pages/statistics/StockpileTaskPanel.tsx` | S22–S26 | 原创真实材料数量+演示任务状态 | 84=64+20，单材料76.2%；总需求55909不伪成单材料 |
| 属性与替换、另存 | `pages/properties/PropertiesPage.tsx`、`pages/replace/ReplacePage.tsx` | S27–S31 | 真实96×100×72、1区域、0实体 | 替换状态演示明确标注，不伪造替换后的GPU图 |
| 世界导出 | `pages/generate/GeneratePage.tsx`、发布说明与正式Core实际输出 | S34–S36 | `exports/Nova-Observatory-World/level.dat`（553B）与`region/r.0.0.mca`（131072B），30chunks、55909块 | 已生成Java1.20.1真实存档；尚未游戏实测，不说已加载游戏 |
| 持久多实例、整数平移、直角旋转、镜像、拆分 | `pages/composition/CompositionPage.tsx`、`docs/releases/BETA_0_4_0.md` | S37–S39 | 发布字段和实例/范围UI演示 | 本轮无实机验收，不伪造组合/拆分GPU结果 |
| 多格式 | 发布说明、`src/platform/schematicFormats.ts` | S40 | Litematica/原版Structure/Create/AnvilCraft/Building Gadgets | 不宣称所有未知格式或所有数据无损转换 |
| 主题/缩放 | `src/services/themeManifest.ts`、`ui/themes/shadcn/theme.css`、设置页 | S41–S42 | 五主题名称；shadcn明暗真实tokens；0.50–5.00范围 | 只演真实已发布视觉token和控件，不伪造未实现主题 |
| Windows/Linux/macOS发行 | 正式Release安装包 | S43、S44 | 真实0.4.0下载页 | macOS未公证；不宣称全部GPU实机通过 |

## 原创模型数据

`Nova Azure Observatory`（蓝图观测展馆）：本片专用原创建筑，CC0-1.0；`nova-observatory.litematic` 和源计划保存在 `assets/original/`。正式Core输出确认96×100×72、55,909非空气方块、15种材料、1区域、0实体。材料真实示例：平滑石英20744、磨制深板岩13160、氧化铜7770、石英砖4855、切制铜84、海晶灯8。

数据层图由正式Core原始数据绘制，仅表示占用位置。卡片中的A/B等是独立视频demo数据标记，不是私有用户工程。影片不展示用户路径、日志、服务器地址、私有实现或凭据。

## 证据等级

1. Actual GPU：素材Agent通过正式发行Viewer采集，采集清单为准；逐文件模式、尺寸、来源必须能核对。
2. Actual Core data/output：正式0.4 Core的stats、visualize-layer和materials-export实际文件。
3. Published-UI demo：取发布源码布局、tokens及控件，在独立Remotion工程演出，可重复；常驻“发布界面演示 · 演示数据”，不冒充运行截图。
4. Abstract annotation：蓝图线框/指示标注是明确的电影图形，不当作Nova算法输出。

GPU素材最终身份以 `assets/original/asset-manifest.json` 或素材Agent交付清单为准。用户已指定最终渲染图统一真实Standard：Hero、卡片、观察、片尾与封面都使用Standard最终冻结资产；Fast早期内部style/proof不作最终成片证据，也不重标。模式选择控件仍保留实际三种模式，但不伪造三模式图像对照。

## 实际发布React UI附加素材

`product-ui-zh-CN.png` / `product-ui-en-US.png`：主 Agent 以Tag同源前端构建，在独立Edge headless上下文与明确的空demo原生适配器采集的实际Home页面（1600×1000）；元数据见`product-ui-capture.json`。不是手绘复刻，也不是原生桌面运行截图，不构成图形/安装实机验收。用于片后SVG封面的真实UI小窗，不用它假造有用户投影的界面。



## 最终冻结的Standard图像

`STANDARD_READY=true` 后冻结：`hero-standard.png` 是正式0.4生产Standard（Unlit）真实GPU的CaptureViewer输出，1904×1401 RGBA；`hero-standard-contained.png` 仅裁掉完整Alpha包围框之外的透明留白，798×698，原图框[589,495,1387,1193]，模型完整。非Fast改标、非Full图。素材Agent已确认86个Cell真实完成提交与驻留。

最终电影的Hero、库卡片、属性、Standard观察和片尾均只绑定这个Standard图。S14改为真实已发布的拖拽/第一人称操作控件演示，画面不假造角度变化；额外视角采集取消。S12/S13只演实际模式/加载方式控件，不展示替代模式的伪实景。旧Fast style/proof只是先期内部方向检查，不能作为最终Standard素材来源。

## HTML导出大小与安全副本说明

影片在CSV/HTML结果镜头中显示的`247,230 bytes`是正式Core实际导出完成时测得的原始HTML字节数，与中文、英文片使用的同一演示快照一致。之后仅为素材公开安全，将HTML元数据里的本机cache_dir/manifest_path替换为不含私有绝对路径的展示标签，交付安全副本为`246,964 bytes`（最终asset-manifest.json为准）；15行材料、数量、配方和功能结果均未变化。视频没有展示原始私有路径，不把脱敏副本说成未经处理的原文件；此266字节差异不是业务结果变化。
