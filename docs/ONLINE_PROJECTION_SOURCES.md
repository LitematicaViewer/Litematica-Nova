# Beta 0.2 在线投影资源

查询日期：2026-10-13。适配依据是原站实际前端 HTML/JavaScript 与只读响应，不以是否公布 API 文档决定接入。未运行抓取的 JavaScript，不读取或保存第三方 Cookie、登录令牌。

| 来源及 Primary 地址 | 应用内功能 | 本轮实际证据 / 边界 |
| --- | --- | --- |
| [RedenMC](https://redenmc.com/) | 搜索、详情、普通附件与参数化生成 | 实查前端搜索、`yisibite` 详情/下载请求及搜索 JSON；复用既有下载和真实 NBT 投影解析。 |
| [Minemev](https://www.minemev.com/) | 搜索、结果字段及真实原始详情页 | 前端 `load_posts` 调用 `/api/v2/search`，只读响应为 `posts`；按 `url_redirect` 或 `/p/{vendor}/{uuid}` 打开真实原页。未观察到自站下载请求，下载在原页完成，不捏造下载 API。 |
| [MCodex](https://mcodex.org/) | 搜索、详情、用户获取附件后下载 | 实测 `api.mcodex.org/api/posts` 列表；前端明确详情 `/api/posts/{id}`、点击下载才请求 `/api/posts/downloads/{id}` 与 `cdn.mcodex.org`。详情使用 `translation.title/description_html`，版本对象映射名称，原页是 `/posts/{id}`。详情与下载响应由实际用户请求检验，不将本轮静态源码证据冒充已下载成功。 |
| [Choculaterie](https://choculaterie.com/) | 原站入口 | 关键前端资源实际返回 `Site rebooting…` HTML；本轮处于维护状态，未伪造接口成功。 |
| [投影工坊](https://www.mcimg.com/) | 搜索、详情、官方附件端点下载 | 实测 `/shimu/api/forum/post/list` 与 `/{id}` JSON。按官方前端 `/shimu/api/forum/post/{id}/resource?name=…` 请求附件；不直接访问 `data-url` 上传路径绕过登录、付费或验证。官方端点仍可要求登录/权限，失败明确保留原页入口。 |
| [Abfielder](https://abfielder.com/) | 搜索、详情、下载 landing/直链 | 实测 SSR 表单 `/Products/BrowseProducts.php?search=…&page=1`、详情 `/Products/ProductDetails.php?id=…` 与下载 landing。用户下载时才读取 `#downloadLink`；短期下载地址仅在内存使用、不记录或持久缓存。 |
| [MineSchematic](https://mineschematic.com/) | 原站入口 | 实际页面有搜索字段 `q` 与 `/s/<slug>` 详情入口；完整查询/附件链本轮未验证，没有猜接口或声称已集成。 |
| [Shulkr](https://www.shulkr.com/en) | 原站入口 | 已查真实站点及前端入口；完整查询/附件链本轮未验证，明确保留原站状态。 |

## 限流与取消

- 所有应用内请求共用主机队列：FIFO、并发 1；完整响应读取释放后至少间隔 2 秒，包括手动重定向的每一跳。不会后台遍历来源、加载卡片图片或爬取所有条目。
- 搜索输入 debounce 500ms。相同 JSON 请求缓存 30 秒，最多 64 项；队列获得名额后再次检查缓存，不重复发送刚完成的同请求。结果索引最多 128 项。
- HTTP 429 不自动重试；主机冷却至少 60 秒，支持 `Retry-After` 秒数与 HTTP 日期，尊重更长等待时间。
- 取消、切换来源或关闭窗口会取消等待任务，不再新增该任务的 HTTP 请求；已经在读取的响应在正常检查点结束，迟到结果不会回写当前页面。

## 下载与入库

- `reden-library` capability 明确允许事件发送/监听与系统浏览器打开。
- 夸克 / 百度网盘附件（包括重定向）返回外链；先弹确认，确认后才打开系统浏览器，取消不下载、不打开、不入库。原站详情/下载同样先确认。
- 直链不凭后缀、MIME 或压缩魔数认定成功：复用 `ApplicationOperations.inspect_project → Core analyze` 对临时内容作真实 NBT/投影解析，通过后才移交正式文件。HTML、空内容、损坏流、普通非投影 NBT 不入库。仅提供本版实际解析支持的投影类型；其它原站文件走原页，不伪装转换。
- 下载、持久入库、刷新广播是三个结果：广播失败仍显示“已下载并入库”并提示手动刷新；入库失败保留真实下载文件位置。
- 不代理登录、付费或站点验证，不绕过 WAF，不发登录请求；官方资源端点失败不改走裸上传路径。

## 证据与验收

本地只读研究证据位于 `.tmp/beta-ui-corrections/online-source-observation/`、`online-frontend-api-evidence.json` 与 `online-api-*` 文件；没有保存认证内容。CLI 验证覆盖输入接口、取消/旧结果、共享限流状态及字段解析；真实系统浏览器、跨窗口消息、三主题和实际各站下载由人工验收，不以编译/单测代替运行成功。
