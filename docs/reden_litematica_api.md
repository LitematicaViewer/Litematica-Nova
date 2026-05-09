# RedenMC 在线投影库 API

本文记录 desktop-nova 接入 RedenMC 在线投影库时实测到的接口行为。

## 搜索

接口：

```text
GET https://redenmc.com/api/mc-services/litematica/search?q=<关键词>
```

参数：

- `q`：搜索关键词。已测试“刷石机”和“世界吞噬者”。

返回：

- `d`：结果数组。
- `offset`、`limit`、`estimatedTotalHits`、`count`、`processingTimeMs`、`query`、`downloads`：搜索元数据。
- 机器 ID 是 `d[].key`，详情和下载接口都使用该字段。
- 可展示字段包括：
  - `name`
  - `type`
  - `author.username`
  - `featureTags`
  - `categoryTag`
  - `versions`
  - `summary`
  - `description`
  - `downloads`
  - `upVotes`
  - `updatedAt`
  - `thumbnailUrl`
  - `imageUrl`

## 详情

接口：

```text
GET https://redenmc.com/api/mc-services/yisibite/<machineId>/info/en
```

参数：

- `machineId`：搜索结果里的 `key`。

返回：

- `d[0]`：机器详情。
- `type`：
  - `LitematicaShare`：普通附件下载。
  - `LitematicaGen`：重复结构/参数化下载。
- `hasX`、`hasY`、`hasZ`：是否需要对应尺寸参数。
- `conditions`：每个轴的尺寸规则。
- `attachments`：普通附件列表。
- `description`、`summary`、`images`、`status`、`source`、`language`：展示元数据。

附件结构示例：

```json
{
  "name": "example.litematic",
  "url": "https://static.redenmc.com/upload/.../1",
  "size": 1741,
  "description": null
}
```

附件序号规则：

- 下载 path 使用 1-based 序号。
- `attachments[0]` 对应 `/download/1`。
- 实测 `/download/0` 返回 404 JSON。

## 普通附件下载

接口：

```text
GET https://redenmc.com/api/mc-services/yisibite/<machineId>/download/<attachmentIndex>
```

参数：

- `machineId`：机器 ID。
- `attachmentIndex`：1-based 附件序号。

处理规则：

- 成功下载且内容确认为 `.litematic` 后才能入库。
- 如果返回 HTML、外部网盘页面、错误 JSON，必须报真实错误，不得伪装成投影文件。

## 参数化下载

接口：

```text
GET https://redenmc.com/api/mc-services/yisibite/<machineId>?xSize=<x>&ySize=<y>&zSize=<z>
```

参数：

- `xSize`
- `ySize`
- `zSize`

尺寸规则来自详情接口的 `conditions`。

前端必须校验：

- min
- max
- mod
- 是否需要该轴

非法尺寸应在前端拦截并显示明确错误。

## 登录、Referer、CORS

实测搜索、详情、普通下载和参数化下载不需要登录。

部分静态下载或外部跳转可能受上游策略影响；desktop-nova 应展示真实状态码和错误消息。

## 本地集成策略

- 下载缓存写入 `%AppData%\Litematica-BA\desktop-nova\reden\downloads\`。
- 不把下载文件写入项目目录。
- 下载完成后 analyze。
- analyze 成功后加入 AppData 投影库。
- 设置 currentFile。
- RedenMC API 样例保存在 `docs/reden-api-samples/`，只保存小 JSON 或响应头，不提交大型 `.litematic`。

