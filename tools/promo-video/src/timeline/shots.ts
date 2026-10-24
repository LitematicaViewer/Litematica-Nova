export type Kind = 'hero'|'library'|'viewer'|'layer'|'stats'|'stockpile'|'properties'|'replace'|'export'|'compose'|'formats'|'settings'|'outro';
export type Shot = {
 id:string; inSeconds:number; outSeconds:number; act:string; kind:Kind; variant:number;
 problemOrBenefit:string; publishedFeatureEvidence:string; sourceAsset:string;
 openingState:string; userAction:string; visibleResult:string; cameraAndFraming:string;
 motionRecipeAndParameters:string; onScreenTextZh:string; onScreenTextEn:string;
 pointerPath:Array<[number,number]>; transitionOut:string; audioCue:string; qaRisk:string;
};
const spec = [
 [0,2,'hero',0,'让下一座建筑，先被看见。','See your next build.','真实建筑落定','无操作，真实 GPU 主体','建筑成为唯一主角'],
 [2,4,'hero',1,'从蓝图，到真实方块。','Blueprints. Made tangible.','建筑转工程蓝图标注','抽象蓝图遮罩过渡','原始 GPU 图完整显现'],
 [4,6,'hero',2,'不进游戏，也能看清。','Explore without launching Minecraft.','真实建筑与软件窗口','窗口从右侧滑入','真实视图落在 Nova 工作区'],
 [6,8,'hero',3,'Litematica-Nova · Beta 0.4','Litematica-Nova · Beta 0.4','产品与建筑同框','无操作，品牌转场','版本、产品和功能定位落定'],
 [8,10,'library',0,'投影，都有自己的位置。','A place for every schematic.','投影库卡片空槽','演示导入公开原创投影','卡片错峰进入真实网格槽'],
 [10,12.5,'library',1,'按名字，直接找到。','Find it by name.','投影库搜索框为空','鼠标点击搜索并输入展馆关键词','搜索文本逐字出现'],
 [12.5,15,'library',2,'从一整库，到一个目标。','From a library to the right build.','卡片列表待筛选','完成搜索','匹配列表收敛，目标留在网格首位'],
 [15,17.5,'library',3,'分类，让整理更轻松。','Keep your collection organized.','全部投影分类','点击建筑分类','分类高亮与对应条目同步'],
 [17.5,20,'library',4,'预览与详情，一起看。','Preview and details, together.','选中蓝图展馆','点击条目详情','真实尺寸、版本与几何预览出现'],
 [20,22,'library',5,'常用项目，随时回来。','Keep your favorites close.','条目未收藏','点击收藏','收藏状态与收藏夹列表同步'],
 [22,25,'viewer',0,'标准模式，结构清楚。','Standard. Clear structure.','三维页标准模式','查看标准模式真实画面','真实 Standard GPU 画面出现'],
 [25,28,'viewer',1,'实时与预加载，按需选择。','Realtime or prebuilt. Your choice.','三维页标准模式选择','选择加载方式','只展示发布选择控件；有真实该模式素材才显示GPU画面'],
 [28,31,'viewer',2,'三种模式，同一个工作区。','Three modes. One workspace.','三维页完整模式选择','选择完整模式控件','不使用其他模式图像冒充；有独立真实Full素材才显示'],
 [31,34,'viewer',3,'观察方式，随手切换。','Switch the way you explore.','相机拖拽操作方式','选择第一人称操作控件','控件由拖拽变第一人称并显示WASD；真实Standard帧不假造角度变化'],
 [34,36.5,'layer',0,'按层检查，从细节开始。','Inspect the build, layer by layer.','二维分层 Y=32','选择 Y=44','真实 Core 数据对应二维切面变化'],
 [36.5,39,'layer',1,'每个高度，都有答案。','Every height tells a story.','二维分层 Y=44','选择 Y=60','真实 Core 高度60占用图，标明二维数据视图'],
 [39,42,'stats',0,'把建筑，变成清单。','Turn a build into a plan.','结构概览','打开材料统计','55,909 个非空气方块和15种材料'],
 [42,45,'stats',1,'材料数量，一眼掌握。','Know what you need.','真实材料清单','聚焦材料行','真实数量与物品身份逐行落定'],
 [45,48,'stats',2,'按个、按组，顺手换算。','Items or stacks. Your choice.','材料按个显示','点击组单位','8颗海晶灯对应1/8组；真实上限64'],
 [48,51,'stats',3,'容器里的物品，也能统计。','Count container items, too.','统计包含容器选项未选','启用包含容器物品','当前原创投影0容器，结果明确显示0而不虚构物品'],
 [51,54,'export',0,'清单导出，带到施工现场。','Take the list to your build.','材料导出 CSV','点击导出','真实正式 Core 输出的15行 CSV 文件'],
 [54,57,'stockpile',0,'为一次施工，建一个任务。','One build. One focused task.','备货工作台任务列表','新建展馆施工演示任务','15种材料与总需求同步进入任务'],
 [57,60,'stockpile',1,'已备数量，直接记。','Log what you have prepared.','切制铜块需求84，已备0','输入64','已备0→64，剩余84→20'],
 [60,63,'stockpile',2,'剩余多少，不用再心算。','What is left? It is right there.','切制铜块已备64','查看同步剩余','64+20=84；单材料进度76.2%'],
 [63,66,'stockpile',3,'不同项目，独立跟进。','Different projects. Separate progress.','展馆施工任务已记录','切到第二演示任务再切回','两个任务状态独立，切回保留64'],
 [66,68,'stockpile',4,'进度自动保留，下次接着来。','Your progress stays with your task.','备货任务已更新','无虚构保存点击，展示自动持久化状态','持久化状态已保存，演示数据明确标注'],
 [68,71,'properties',0,'结构信息，不用翻文件。','Your structure at a glance.','概览页','选择结构属性','真实96×100×72、1区域、0实体'],
 [71,74,'replace',0,'源与目标，映射清楚。','Clear source-to-target mapping.','替换页源与目标已选择','查看切制铜块→氧化铜映射','显示84个匹配；仅演示控件，不假造 GPU 改色'],
 [74,77,'replace',1,'改动前后，数量说清。','A clear result for every change.','替换映射确认','点击执行替换的UI演示','84个对应材料条目变化，标注演示状态'],
 [77,79.5,'replace',2,'另存一份，保留原件。','Save a copy. Keep your original.','替换结果待保存','选择另存为','明确新文件名，不覆盖源文件的演示结果'],
 [79.5,82,'properties',1,'可读的检查，不是盲目写出。','Check first. Export with clarity.','目标格式能力检查','查看检查结果','已发布功能明确失败原因；不承诺任意版本等价转换'],
 [82,85,'export',1,'需要网页，也有完整清单。','A material list you can open anywhere.','材料导出选择网页','点击网页导出','真实正式 Core 生成的材料 HTML 文件'],
 [85,88,'export',2,'结果文件，真实可用。','Real output. Ready to use.','输出文件列表','查看已生成 CSV 与 HTML','15行清单与247230字节网页结果'],
 [88,91.5,'export',3,'世界导出，也在同一工具里。','World export, in the same toolkit.','世界导出选项','点击Minecraft世界导出','真实level.dat553B及region/r.0.0.mca131072B出现；未宣称游戏已加载'],
 [91.5,95,'export',4,'能力检查，保护需要保留的数据。','Capability checks protect your data.','导出数据检查','查看目标格式能力','实体、附加数据不能保留时给出具体原因'],
 [95,98,'export',5,'准备好，交给下一步。','Ready for your next step.','真实CSV/HTML结果','打开结果预览','真实文件内容接到下一段组合项目'],
 [98,101,'compose',0,'多份投影，一个组合项目。','Multiple schematics. One project.','组合项目实例列表','添加同一原创投影的重复实例','实例列表独立身份与变换值；UI演示不伪造组合GPU'],
 [101,104,'compose',1,'移动、旋转，明确到整数。','Move and rotate with precision.','实例X=0，已有90°配置','输入X=32','X字段0→32，既有90°旋转配置保持'],
 [104,106.5,'compose',2,'拆分范围，清清楚楚。','Make the split boundaries explicit.','XYZ拆分范围表','设置范围0–47、48–95','两个不重叠范围清楚列出，不伪造拖拽几何'],
 [106.5,109,'formats',0,'更多格式，统一工作流。','More formats. One workflow.','格式导入选择器','查看支持格式','显示发布版本的5类格式，不宣称任意外部格式'],
 [109,111.5,'settings',0,'熟悉的界面，也能有新风格。','A familiar workspace. Your own style.','设置外观页','切换明暗模式','真实发布shadcn明暗token变化'],
 [111.5,114,'settings',1,'界面倍率，直接设定。','Set your interface scale.','UI缩放设置1.00×','调整倍率设置到1.25×','仅演设置数值与滑块；不伪称当前窗口已应用1.25倍，发布范围0.50×–5.00×'],
 [114,116,'settings',2,'桌面工具，不只一种系统。','Built for your desktop.','平台下载列表','选择平台包','真实Windows/Linux/macOS安装包，不冒充实机全验'],
 [116,120,'outro',0,'从蓝图，走向下一座建筑。','From a blueprint to your next build.','工作区结束','进入正式下载页CTA','0.4.0真实Release地址和品牌稳定停留'],
] as const;
export const shots: Shot[] = spec.map((s,i) => ({
 id:`S${String(i+1).padStart(2,'0')}`, inSeconds:s[0],outSeconds:s[1],
 act:s[0]<22?'A · Find':s[0]<98?'B · Build':'C · Deliver',kind:s[2],variant:s[3],
 problemOrBenefit:s[4],onScreenTextZh:s[4],onScreenTextEn:s[5],
 openingState:s[6],userAction:s[7],visibleResult:s[8],
 publishedFeatureEvidence:'beta-0.4.0 / 3cd63 / docs/releases/BETA_0_4_0.md; see source-of-truth.md',
 sourceAsset:s[2]==='viewer'&&(s[3]===1||s[3]===2)?'Published mode/loading selector UI; no GPU frame for another mode claimed':
   s[2]==='hero'||s[2]==='viewer'?'assets/original/hero-standard-contained.png: actual 0.4 Standard GPU output':
   s[2]==='layer'?'assets/original/layer-{32,44,60}.json: actual Core output':
   'Published UI source-derived video demonstration; actual Core observatory stats/exports',
 cameraAndFraming:s[2]==='hero'?'single building hero, restrained 2.5D plane':'orthographic product window at true 1.00 scale; motion comes from controls and results, not fake applied zoom',
 motionRecipeAndParameters:s[2]==='library'?'deck-deal-flyin/type-and-filter: 16f landing, 5f card stagger; actual control anchor; result starts 4f after click':
   s[2]==='stats'?'row-embed/unit-dot-swarm-regroup: 16f arrival, no fabricated totals':
   s[2]==='outro'?'ui-strip-away-outro: edge to centre 8f stagger, final brand hold 72f':
   'spotlight-hero-card/cursor-flyover: no shake; bezier(.22,1,.36,1); result hold >=36f',
 pointerPath:[[1280,650],[1030,270],[1180,490]],
 transitionOut:s[2]==='hero'?'blueprint mask / window handoff':'window edge or match-cut; no white flash',
 audioCue:s[2]==='hero'?'soft-impact at 0.4s; swish at 1.4s':'click at .65s; confirm at 1.25s; swish at out-.18s',
 qaRisk:s[2]==='compose'||s[2]==='replace'?'Source-derived UI demo, NOT actual composite GPU/output success':
   s[2]==='export'?'Actual CSV/HTML and Java1.20.1 world files; no in-game loading claimed; raw/sanitized HTML byte sizes documented':
   s[2]==='viewer'?'Asset must identify distinct actual rendering mode; never reuse one frame as three modes':
   'No personal paths, hidden claims, or unreadable UI; demo state explicitly labelled',
}));
export const FPS=60;
export const DURATION=120;
