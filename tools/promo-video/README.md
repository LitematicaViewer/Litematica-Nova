# Nova 双语代码视频工程

本目录独立于产品前后端依赖，使用自己的 `package-lock.json`。当前仅完成 S 阶段工具和空 Composition 骨架，未编写分镜、产品文案或视频，未采集界面、启动浏览器或渲染试片。

## 工具及 Skills

- 官方 Remotion 4.0.538；六项 Skills 来自 `remotion-dev/skills` 的 `6c67adcd9cdbe6eb84d76342da89b225f9161cb2`。
- `remotion-best-practices`、`remotion-create`、`remotion-markup`、`remotion-render`、`remotion-multimedia`、`remotion-captions` 均保存在 `.agents/skills/`，名称与官方一致。
- 官方 `@playwright/cli` 0.1.22；`playwright-cli` Skill 安装到 `.agents/skills/`。使用确定脚本采集，不进行逐次看屏决策。
- FFmpeg / ffprobe 复用本机已安装的 8.1，不安装全局依赖。
- Skills 供后续 Agent 会话自动发现；本轮已直接读取 `SKILL.md`。

官方来源：

- <https://www.remotion.dev/docs/ai/skills>
- <https://github.com/remotion-dev/skills>
- <https://playwright.dev/agent-cli/installation>

## 本地复现

从仓库根目录进入本目录，使用 PowerShell；只激活当前进程的隔离 Node 工具链：

```powershell
. 'D:\NovaCI\toolchains\windows\Activate.ps1'
Set-Location tools/promo-video
npm ci
npm exec --no -- remotion --help
npm exec --no -- playwright-cli --help
pwsh -NoProfile -File scripts/check.ps1
```

此工程的真实 CLI 为 `playwright-cli`；不要假定 `playwright cli` 在任意安装组合下均可用。官方安装方式是 `npm exec --no -- playwright-cli install --skills=agents`。这不运行界面采集；S 阶段仅查看帮助与版本。

`ZhIntro` / `EnIntro` 当前是 1 帧的空骨架，并非可交付视频。只有主 Agent 核实 R 阶段全部发布成功、发出 `R_RELEASED=true` 后，才能开始 V 阶段内容制作。

## 后续渲染接线

V 阶段完成内容后，在不提交的 `output/release-manifest.json` 记录已核实的 `R_RELEASED`、`version`、`release_url` 和 `video_content_ready`，再运行：

```powershell
pwsh -NoProfile -File scripts/render.ps1
pwsh -NoProfile -File scripts/check.ps1 -Media
```

`render.ps1` 为中英文 MP4 的统一入口，发布未确认或内容未完成时会明确终止。`check.ps1 -Media` 只读取媒体元数据并检查字幕存在，不代替最终内容验收。素材身份、字幕内容、关键帧及授权核对在 V 阶段接入，S 阶段不伪造通过结果。

原始素材和音频分别保存在 `assets/original/`、`assets/audio/`；输出保存在 `output/`。这些目录默认不进入 Git。不得放入生产凭据、私有渲染源码、内部地址或个人用户文件。
