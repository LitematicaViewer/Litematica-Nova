# Nova 0.4 双语官方代码视频

本目录独立于产品依赖和发布流程，使用自己的 package-lock.json。R阶段已由主Agent确认：0.4.0正式Release已发布，源Tag beta-0.4.0（3cd63），公开main 8a0ee8cd。此工程不修改产品、私有核心或正式Release。

## 一键复现

在本目录使用PowerShell，Node 22及FFmpeg/ffprobe需可用。本机可仅激活当前进程的隔离工具链：

```powershell
. 'D:\NovaCI\toolchains\windows\Activate.ps1'
npm ci
# 已冻结发布、素材和内部样式放行后，一条命令生成双语MP4/SRT，
# 在两片完成后生成SVG/PNG封面，并完成最终媒体及关键帧检查。
pwsh -NoProfile -File scripts/render.ps1 -Mode final
```

独立依赖：Remotion 4.0.538、React19.3.1、TypeScript5.9.3、Playwright CLI0.1.22；本机FFmpeg/ffprobe8.1。正式影片两条均120秒、1080p60、H.264/AAC。`NovaOfficialZh`/`NovaOfficialEn`共享时间线，但导航、控件、标题和SRT各自本地化。

## 工程结构

- `src/theme.ts`：发布shadcn色板与字体；蓝图标注使用低饱和蓝。
- `src/timeline/shots.ts`：44镜头/120秒权威时间线；`scripts/export-story.mjs`生成shot-plan.json、双语SRT和可选录音稿。
- `src/scenes/`：独立视频UI演出，不反写产品业务；图形只使用真实Standard最终资产。
- `src/components/Pointer.tsx`：量取真实演出控件位置并归一到电影画布；不是独立维护一套易错坐标。
- `assets/original/`：原创公开展馆、正式Core数据/导出、真实GPU帧；来源与冻结身份以素材清单为准。
- `assets/audio/`：原创CC0配乐和SFX，无第三方采样；无旁白，不假称录音完成。
- `scripts/check.ps1 -Media -Keyframes`：实际媒体参数、每镜头开头/动作/落定/出镜四帧、逐镜头contact及索引。技术通过不代替独立审美/声音/真实性审查。
- `output/`：真实MP4/SRT、三张styleframe、Proof、证据文档、keyframes及后置SVG/PNG封面。

## 官方Skills与参考

六项官方Remotion Skill和官方Playwright CLI Skill安装在`.agents/skills/`，已真实读取。Remotion Skill固定上游提交6c67adcd9cdbe6eb84d76342da89b225f9161cb2。官方文档：

- https://www.remotion.dev/docs/ai/skills
- https://github.com/remotion-dev/skills
- https://playwright.dev/agent-cli/installation

本片参考video-shotcraft的8项实际recipe/Gallery/demo TSX，固定提交43bd349a136651865ecfa6ea1cf012f6e957d304；Apache-2.0代码/文档。只借鉴镜头语法，自写Nova组件，不复制其品牌画面、音频或执行未知脚本；详细映射见reference-techniques.md。参考下载保存在Git忽略的.reference-cache，不进入产品发布。

## 真实性与素材边界

产品事实见source-of-truth.md；视觉/音频基准见design-spec.md。UI为发布源码派生的可重复演示，常驻演示标识；真实GPU帧、Core二维占用图与抽象蓝图图形分别明确，不互相冒充。世界导出展示实际文件，不伪造已进游戏。当前私有算法、密钥、日志、内部地址与用户路径不进入素材或影片。

源素材、音频、输出默认不进入Git；本机工作区保留真实文件。重新取得工程时必须先提供同一冻结素材清单，不拿缺图占位当成功。早期Fast样片仅为内部方向预览；最终用户要求所有建筑渲染图使用真实Standard，不把Fast换标签。
