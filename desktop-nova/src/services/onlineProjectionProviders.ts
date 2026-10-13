/** Adapters come from observed frontend requests/HTML, not a public-documentation prerequisite. */
export interface OnlineProjectionProvider {
  id: string;
  name: string;
  url: string;
  mode: "native" | "website";
  note: string;
}

export const ONLINE_PROJECTION_PROVIDERS: readonly OnlineProjectionProvider[] = [
  { id: "reden", name: "RedenMC", url: "https://redenmc.com/", mode: "native", note: "复用已观察搜索/详情/下载请求；网盘确认后由浏览器处理。" },
  { id: "minemev", name: "Minemev", url: "https://www.minemev.com/", mode: "native", note: "已接自站 /api/v2/search 与真实原页信息；未观察到自站下载请求，下载仍在原页完成。" },
  { id: "mcodex", name: "MCodex", url: "https://mcodex.org/", mode: "native", note: "按实际前端 /api/posts、详情、downloads 和 CDN 链路接入；用户点击才取下载地址。" },
  { id: "choculaterie", name: "Choculaterie", url: "https://choculaterie.com/", mode: "website", note: "实际关键前端资源返回 Site rebooting 维护HTML；原站入口保留，未伪造成功。" },
  { id: "projection-workshop", name: "投影工坊", url: "https://www.mcimg.com/", mode: "native", note: "实查 /shimu 列表/详情/resource；遵守官方资源门禁，不直取上传路径绕过登录或付费。" },
  { id: "abfielder", name: "Abfielder", url: "https://abfielder.com/", mode: "native", note: "实查 SSR 搜索表单、详情与下载landing；短期链接只在用户下载时内存使用。" },
  { id: "mineschematic", name: "MineSchematic", url: "https://mineschematic.com/", mode: "website", note: "已检查真实搜索表单和详情入口；完整附件链本轮未验证，保留原站，不猜测下载接口。" },
  { id: "shulkr", name: "Shulkr", url: "https://www.shulkr.com/en", mode: "website", note: "已检查实际站点与前端入口；完整查询/附件链本轮尚未验证，保留原站。" },
];
