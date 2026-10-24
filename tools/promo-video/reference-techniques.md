# 镜头参考与Nova化改造

官方 Remotion 四项 Skill（best-practices/markup/create/render）已在V开始时实际重新读取，固定版本4.0.538。采用用户明确的自主制作、先样片后全片流程，未开启交互式GUI探索。

video-shotcraft 固定参考提交：`43bd349a136651865ecfa6ea1cf012f6e957d304`。已读取SKILL、pipeline、aesthetic-rules、final-review；只把项目当公开镜头语法参考，不执行任何其安装/发布/采集脚本，不接触生产凭据。代码与文档上游许可证Apache-2.0；本片未使用其音频、品牌图或示例产品画面，不把示例片混入本片。下载的8个Gallery预览仅用于本地学习（`.reference-cache/`，Git忽略）。全部本片TSX为Nova定制实现，不整段复制模板。

| 参考技术 | 本片功能 | Nova化参数与取舍 | 精确来源 |
|---|---|---|---|
| spotlight-hero-card | 建筑Hero | 单建筑主体，1.00–1.02轻位移；保留焦点与落定，不套钨丝灯/金色/扫光 | `references/shots/opening/spotlight-hero-card.md` / `demos/opening/spotlight-hero-card/SpotlightHeroCard.tsx` / 同名Gallery MP4 |
| text-as-mask | 蓝图→实际方块 | 用工程边界+横向mask揭示原始GPU图；不做46倍眩晕冲字 | `references/shots/opening/text-as-mask.md` / `demos/opening/text-as-mask/TextAsMask.tsx` / 同名Gallery |
| deck-deal-flyin | 投影库卡片归位 | 六卡18帧左右落定、9帧错峰、16°收平；暗灰真实网格，不搬26张论文卡 | `references/shots/ui-entrance/deck-deal-flyin.md` / `demos/ui-entrance/deck-deal-flyin/DeckDealFlyin.tsx` / 同名Gallery |
| row-embed | 实际材料行 | 12°→0°收平，7帧错峰、16帧入场，终点为表格行；不群发扫光 | `references/shots/ui-entrance/row-embed.md` / `demos/ui-entrance/row-embed/RowEmbed.tsx` / 同名Gallery |
| cursor-flyover | 指针引导控件→结果 | 光标先导4–6帧，轻弧38px，40帧点击，结果静置；密集文字正视，取消shake和模糊 | `references/shots/camera/cursor-flyover.md` / `demos/camera/cursor-flyover/CursorFlyover.tsx` / 同名Gallery |
| type-and-filter | 库搜索 | 6帧/字符（60fps）、搜索→列表结果跨连续两镜；目标回到首格，不悬空 | `references/shots/interaction/type-and-filter.md` / `demos/interaction/type-and-filter/TypeAndFilter.tsx` / 同名Gallery |
| chart-live-moves · unit-dot-swarm-regroup | 材料与备货数值后果 | 借鉴同一数据变成结果；不画与产品无关的增长曲线/点阵，84=64+20真实算术 | `references/shots/data/chart-live-moves.md` / `demos/data/chart-live-moves/UnitDotSwarmRegroupV2.tsx` / `unit-dot-swarm-regroup.mp4` |
| ui-strip-away-outro | 从工作区到下载CTA | 外围UI退场、单建筑和品牌保留≥72帧；不做发布按钮假操作 | `references/shots/outro/ui-strip-away-outro.md` / `demos/outro/ui-strip-away-outro/UiStripAwayOutro.tsx` / 同名Gallery |

Gallery索引已核对上述实际style-key与media地址，预览已下载并抽取参考帧；部分配方只借鉴运动因果而刻意不用装饰性表现，以上不宣称与样片像素一致。官方 Remotion Prompt Showcase 的Product Demo定位用于确认“操作→结果”叙事，不复制第三方品牌或媒体。

动态变化全部按frame驱动，无墙钟、运行时网络、未固定随机数或CSS animation。图形、字幕、SFX与状态共享时间线；最终成片/关键帧和本文件交由独立Review Agent审查。
