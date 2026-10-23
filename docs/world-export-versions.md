# 全版本虚空世界导出

## 功能与交互设计

用户从属性页选取已保存投影，填写世界名、输出父目录、**目标游戏版本**，点击导出，得到新的世界目录。默认展示这三项；高级选项只放投影最小角坐标和目标允许高度。导出时显示真实阶段/已写区域数并提供取消；成功显示实际目标版本、完整目录及“打开文件夹”。没有逐版本的内部格式设置暴露到 GUI。

目标是投影的完整原始语义，不受当前相机、分层、裁剪、光影或截图影响。导出不要求初始化渲染器、访问 GPU、领钥或启动 JVM。

- 源投影 `MinecraftDataVersion` 唯一对应已支持发行版时作为默认值；多个发行版共用一个 DataVersion 或源未知时，使用当前有效游戏数据版本，仍由用户明确确认。
- 方块、方块实体、实体、tick 共用一个坐标平移；目标高度、世界边界、不同非空气重叠、重复 UUID 和目标不认识的实际使用状态先检查。
- 复用已有 payload 迁移与版本数据适配。缺失类型/物品/组件或不明确的有损转换显式中止并列出对象及区域；不偷偷替换为空气。只有旧别名在真实 numeric 语义中唯一指向相同 ID/metadata 才自动规范化。
- 整个投影的实体及容器原始 NBT保留；跨版本按现有明确规则迁移。**这不等于任何未来/模组字段都能自动改写，也不等于所有任意新版内容都能无损降级。**
- 主世界永久使用空气层的 flat generator，结构/装饰关闭；不是仅预写一圈空气。旧版仅承诺主世界虚空，下界/末地按对应版本默认生成，界面明确提示；新版只配置主世界维度。没有宣称三个维度全部虚空。
- 旁观者出生在投影最小角附近，不添加平台，不改投影数据。

## 实现与格式来源

`tools/viewer-core/src/world_export.rs` 是原生 Rust writer；`data/world-export/profiles.json` 是 63 个精确发行版 profile，通过 `include_str!` 随 Core 编译，不依赖工作目录或开发机路径。`tools/mc26-oracle/scripts/export-world-profiles.py` 仅供开发端从实际版本 JAR、官方映射/现有 MCP映射和已完成 oracle 数据重新生成来源记录；不是产品 JVM依赖，也不是宣称可自动理解任意未来 JAR 的分析器。

profile 记录实际 client JAR身份、chunk/level/flat/bit-storage/dimension/state codec类身份及字段证据；完整标识在 JSON 的 `evidence`。语义数据已有63版独立 oracle；世界格式的运行时验证另记下表，二者不混用。

| 格式变化 | 处理 |
| --- | --- |
| 1.8–1.12.2 numeric Anvil | `Blocks`/`Data`/可选`Add`；真实旧ID/metadata；旧biome、heightmap与合法未完成照明标志 |
| 1.13–1.15.2 palette | 命名状态和跨64位边界的packed longs；最小4bit |
| 1.13 特有照明恢复 | 实际loader要求2048字节SkyLight/BlockLight；写`decorated` protochunk与真实发光状态的`Lights`位置，由游戏LIGHTED阶段计算，不虚标fullchunk或已照明 |
| 1.16–1.17.1 palette | 元素不跨long，使用每long填充；1.17起实体独立region |
| 1.18起现代chunk | 无Level外包，`sections`/`block_states`/`block_entities`、负Y、section biome |
| 1.21.9起spawn | 实际codec使用`spawn` compound而非SpawnX/Y/Z |
| 26.1起目录/世界生成 | 主世界位于`dimensions/minecraft/overworld`；world_gen_settings/game_rules独立保存 |
| 26.3 block state codec | 保存的palette采用实际`id`/`properties`，不再套旧`Name`/`Properties` |
| DragonFight | 只在实际PrimaryLevelData包含该字段的1.16–1.21.11写合法初始数据；26.x不混入旧字段 |

现有 Host 的受监督后台操作承载导出；task ID对应取消/进度文件，不控制窗口。输出父目录同卷创建本任务独占暂存目录，所有文件关闭同步后，以不覆盖目标的原子rename发布。失败/取消只清理本次目录；已有目录保留。Windows 对实际短暂文件共享导致的最终rename错误有界恢复（最多200ms），不重跑整个任务；目标出现或其他错误立即失败。

## 运行时验证记录（2026-10-24）

- 语义数据覆盖：**63/63**（复用既有实际 oracle，不是本轮重新伪造）。
- 原生格式实现覆盖：**63/63**。
- 目标游戏无GUI读取→保存退出→重开、内容比较与新区块虚空：**59/63**，逐版结果见下表。
- **游戏画面、容器交互及应用导出对话框的人工/自动GUI验收未执行**：本会话原生GUI控制接口不可用。CLI及真实 dedicated-server验证不替代目视验收，不标为全部验收完成。

每个精确版本样本均使用三个独立区域、负坐标、chunk/region边界、该版本上下高度边界；包含6个实际方块（3块石头、3只带方向状态的箱子）、3个容器（Slot2、3颗钻石、RepairCost7）及1个ArmorStand（位置、旋转、NoGravity）。1.9+额外核对Tags；1.8实际Entity存储不支持Tags，不把测试端虚构字段的丢失当导出缺陷。每版实际游戏查询这些坐标，并在新生成的(128,128)chunk检查零非空气方块。保存前后比较方块状态、位置、容器物品和实体姿态。

旧版没有forceload，使用真实启动加载三个样本出生区域和一个新空区域，各保存重开两次；1.13也走同一路径。现代版有界等待forceload真实完成，执行实际方块查询，再保存退出并第二次打开。不以只有进程exit0、日志无fatal或NBT自读代替内容检查。

证据在开发工作区 `.tmp/world-export/`：`numeric-runtime-results.json`、`server-runs/<version>/result.json`或`result-modern.json`与对应真实游戏日志、原生导出输入/输出和语义快照。原生输入来自各版真实注册表；没有修改用户存档。

| 精确版本 | DataVersion | 格式家族 | 目标高度 | 主世界目录 | 实际JAR标识前12位 | 运行时往返 |
| --- | ---: | --- | --- | --- | --- | --- |
| 1.8 | 无此字段 | numeric | 0…255 | `.` | `ccccd0a92f76` | 通过 |
| 1.9 | 169 | numeric | 0…255 | `.` | `0273b5c79d6b` | 通过 |
| 1.9.2 | 176 | numeric | 0…255 | `.` | `ef87eb71250d` | 通过 |
| 1.9.4 | 184 | numeric | 0…255 | `.` | `23e90103a1ca` | 通过 |
| 1.10 | 510 | numeric | 0…255 | `.` | `c00f8170ebf4` | 通过 |
| 1.10.1 | 511 | numeric | 0…255 | `.` | `338b205f20c2` | 通过 |
| 1.10.2 | 512 | numeric | 0…255 | `.` | `7cdf7fcdc1c9` | 通过 |
| 1.11 | 819 | numeric | 0…255 | `.` | `f0d11987ea74` | 通过 |
| 1.11.2 | 922 | numeric | 0…255 | `.` | `be3fff4f2cc0` | 通过 |
| 1.12 | 1139 | numeric | 0…255 | `.` | `59e84cf6acc3` | 通过 |
| 1.12.1 | 1241 | numeric | 0…255 | `.` | `19331fb29bf8` | 通过 |
| 1.12.2 | 1343 | numeric | 0…255 | `.` | `8ada07da5ee7` | 通过 |
| 1.13 | 1519 | palette_contiguous | 0…255 | `.` | `d807a5ed9428` | 通过 |
| 1.13.1 | 1628 | palette_contiguous | 0…255 | `.` | `f0b4cf6d1a87` | 通过 |
| 1.13.2 | 1631 | palette_contiguous | 0…255 | `.` | `3410887ba652` | 待运行 |
| 1.14 | 1952 | palette_contiguous | 0…255 | `.` | `93907cbf0655` | 通过 |
| 1.14.1 | 1957 | palette_contiguous | 0…255 | `.` | `7194d1326cf7` | 通过 |
| 1.14.2 | 1963 | palette_contiguous | 0…255 | `.` | `9b740c9fd495` | 通过 |
| 1.14.3 | 1968 | palette_contiguous | 0…255 | `.` | `406306267743` | 通过 |
| 1.14.4 | 1976 | palette_contiguous | 0…255 | `.` | `b3b2a798e2d6` | 通过 |
| 1.15 | 2225 | palette_contiguous | 0…255 | `.` | `41eee66b4c8e` | 通过 |
| 1.15.1 | 2227 | palette_contiguous | 0…255 | `.` | `6e28ecd008d0` | 通过 |
| 1.15.2 | 2230 | palette_contiguous | 0…255 | `.` | `4a73008a73f3` | 通过 |
| 1.16 | 2566 | palette_padded | 0…255 | `.` | `00740ef4475d` | 通过 |
| 1.16.1 | 2567 | palette_padded | 0…255 | `.` | `b4831e7b63b1` | 通过 |
| 1.16.2 | 2578 | palette_padded | 0…255 | `.` | `19960f4e3c60` | 通过 |
| 1.16.3 | 2580 | palette_padded | 0…255 | `.` | `da85197babd9` | 通过 |
| 1.16.4 | 2584 | palette_padded | 0…255 | `.` | `d6740464ee2b` | 通过 |
| 1.16.5 | 2586 | palette_padded | 0…255 | `.` | `00b5ebbc33e9` | 通过 |
| 1.17 | 2724 | palette_padded | 0…255 | `.` | `14122287d4e4` | 通过 |
| 1.17.1 | 2730 | palette_padded | 0…255 | `.` | `a49b4a56c5bb` | 通过 |
| 1.18 | 2860 | root_sections | -64…319 | `.` | `381e090ef225` | 通过 |
| 1.18.1 | 2865 | root_sections | -64…319 | `.` | `737c28a56c89` | 通过 |
| 1.18.2 | 2975 | root_sections | -64…319 | `.` | `1d09e3639644` | 通过 |
| 1.19 | 3105 | root_sections | -64…319 | `.` | `ffd4fb3037d4` | 通过 |
| 1.19.1 | 3117 | root_sections | -64…319 | `.` | `146d4018cfed` | 通过 |
| 1.19.2 | 3120 | root_sections | -64…319 | `.` | `e1ac65de9b47` | 通过 |
| 1.19.3 | 3218 | root_sections | -64…319 | `.` | `b7228c23dbc8` | 通过 |
| 1.19.4 | 3337 | root_sections | -64…319 | `.` | `0e79cf7f07c1` | 通过 |
| 1.20 | 3463 | root_sections | -64…319 | `.` | `5ac9ead13574` | 通过 |
| 1.20.1 | 3465 | root_sections | -64…319 | `.` | `56b71336d2b4` | 通过 |
| 1.20.2 | 3578 | root_sections | -64…319 | `.` | `fa1a19be56a5` | 通过 |
| 1.20.3 | 3698 | root_sections | -64…319 | `.` | `bef219382249` | 通过 |
| 1.20.4 | 3700 | root_sections | -64…319 | `.` | `9221ab461a49` | 通过 |
| 1.20.5 | 3837 | root_sections | -64…319 | `.` | `49d5949adbb1` | 通过 |
| 1.20.6 | 3839 | root_sections | -64…319 | `.` | `02dfd345ac1a` | 通过 |
| 1.21 | 3953 | root_sections | -64…319 | `.` | `4b56bc9b44de` | 通过 |
| 1.21.1 | 3955 | root_sections | -64…319 | `.` | `499f6897d183` | 通过 |
| 1.21.2 | 4080 | root_sections | -64…319 | `.` | `613091da0f62` | 通过 |
| 1.21.3 | 4082 | root_sections | -64…319 | `.` | `25190c556bc5` | 通过 |
| 1.21.4 | 4189 | root_sections | -64…319 | `.` | `c17c450c6e72` | 通过 |
| 1.21.5 | 4325 | root_sections | -64…319 | `.` | `522672ad20b4` | 通过 |
| 1.21.6 | 4435 | root_sections | -64…319 | `.` | `9592df8587d6` | 通过 |
| 1.21.7 | 4438 | root_sections | -64…319 | `.` | `6d0aa64c6a0a` | 通过 |
| 1.21.8 | 4440 | root_sections | -64…319 | `.` | `ea74e9c5e92d` | 通过 |
| 1.21.9 | 4554 | root_sections | -64…319 | `.` | `40c0a32c4660` | 通过 |
| 1.21.10 | 4556 | root_sections | -64…319 | `.` | `e65ca028bc58` | 通过 |
| 1.21.11 | 4671 | root_sections | -64…319 | `.` | `1473c9489ac5` | 通过 |
| 26.1 | 4786 | root_sections | -64…319 | `dimensions/minecraft/overworld` | `bc6194c61566` | 旧失败待修正复测 |
| 26.1.1 | 4788 | root_sections | -64…319 | `dimensions/minecraft/overworld` | `8c109be6177f` | 通过 |
| 26.1.2 | 4790 | root_sections | -64…319 | `dimensions/minecraft/overworld` | `b1b315857266` | 待运行 |
| 26.2 | 4903 | root_sections | -64…319 | `dimensions/minecraft/overworld` | `40896ee9f1e2` | 通过 |
| 26.3 | 5023 | root_sections | -64…319 | `dimensions/minecraft/overworld` | `4508d006323f` | 旧失败待修正复测 |

## 已发现并修正的实际格式错误

1. 初版1.13缺少必需光照nibble数组，游戏报告`ChunkNibbleArrays should be 2048 bytes not: 0`并丢弃chunk。改为合法decorated protochunk、完整数组和发光位置队列；只重测受影响1.13家族。
2. 初版26.1沿用根目录region，真实游戏从新维度目录读取，导致原生文件存在但游戏为空。改用实际DimensionType路径。
3. 初版26.3沿用Name/Properties，真实codec报告`No key id`，不能把恢复为空气当通过。按实际StateHolder写新字段。
4. 1.19最初比较器只认`[Server]`，实际游戏使用`<Server>`；从原始日志重读已执行成功的七项查询修正记录，没有额外重跑游戏。
5. 部分旧Java日志系统在保存完成后的shutdown报告`Stream Closed`；26.2/26.3本机性能计数器报告Perflib错误。它们与具体存档语义检查分别记录，不伪装成无错误日志。

## 验收边界与已知缺口

- 以上是明确样本与格式范围的运行时验证，不宣称已逐一验遍所有实体姿态、所有嵌套物品组件和全部跨版本降级组合。
- 各格式家族的游戏目视检查与应用GUI流程仍待可用GUI环境执行；Mac/Linux真实文件事务未实机执行。
- 任意位置引用和未来扩展字段需要相应版本语义适配，现有明确支持的引用（乘客、Leash、悬挂/休眠/归属坐标等）按同一平移处理；未知不受支持字段不冒称自动转换。
- 1.12床专用绘制是既有渲染缺口，与本导出领域和已补齐的语义数据无关，本任务没有修渲染器。
