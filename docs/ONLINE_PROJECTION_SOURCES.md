# Beta 0.2 在线投影资源

查询日期：2026-10-13。真实网站入口与 **应用内 API 集成** 分开呈现；不宣称网页入口等于完成 API 对接。

| 来源及 Primary 地址 | 本版接入方式 | 已确认信息 / 当前边界 |
| --- | --- | --- |
| [RedenMC](https://redenmc.com/) | 保留现有搜索、详情、参数化下载 API | 官网提供 [OpenAPI](https://api.redenmc.com/)；实际文件经现有 Core NBT 投影解析器验证后入库。 |
| [Minemev](https://www.minemev.com/) | 系统浏览器打开原站 | 官网列出技术资料、`.litematic` 下载及 3D 预览；本版未对接下载 API。 |
| [MCodex](https://mcodex.org/) | 系统浏览器打开原站 | Primary 页面确认 MCodex 标题；正文需要网站运行，本次未核实公开 API 文档，不推测接口。 |
| [Choculaterie](https://choculaterie.com/) | 系统浏览器打开原站 | 页面正文未解析；[官方状态页](https://status.choculaterie.com/)的 API 服务名称不代表公开 API 契约。 |
| [投影工坊](https://www.mcimg.com/) | 系统浏览器打开原站 | 原站[投稿须知](https://www.mcimg.com/forum-post/105.html)出现在索引中；本次直接访问超时，不宣称当前可下载。旧 `haomc.com` 已变为无关站点，未使用。 |
| [Abfielder](https://abfielder.com/) | 系统浏览器打开原站 | 官网公开投影库与独立商店；付费/登录保留在原站，不对接未文档化 API。 |
| [MineSchematic](https://mineschematic.com/) | 系统浏览器打开原站 | 官网明确 `.litematic` / `.schem` / `.nbt`、3D 预览及免账号下载；本版未对接下载 API。 |
| [Shulkr](https://www.shulkr.com/en) | 系统浏览器打开原站 | 官网提供投影库、预览、编辑、转换；本版未对接下载 API。 |

## 下载与入库语义

- Reden 独立窗口使用 `reden-library` capability；事件发送/监听与系统浏览器打开具有明确权限。
- 夸克 / 百度网盘附件（包括 API 重定向）返回外链，不抓取登录网页。用户确认后才调用系统浏览器；取消不会下载、打开或入库。
- 直链下载不依赖后缀或 MIME 认定成功：临时内容通过已有 `ApplicationOperations.inspect_project → Core analyze` 真实 NBT/投影解析后才移交正式文件；无效内容清理临时文件，不覆盖旧文件。
- 下载、持久入库、主窗口事件广播是三个结果。广播失败保留“已下载并入库”结果并提示手动刷新；入库失败明确保留下载文件位置。
- 网页来源不代理第三方登录，不获取 Cookie，不模拟付费下载；浏览器下载完成后走现有本地投影导入入口。

## 验收

仅运行定向 CLI / React handler 状态测试及 Rust 下载控制测试。跨窗口真实事件、系统浏览器启动与三主题显示由人工验收；未进行自动 GUI / 视觉验收。
