/** Primary websites checked for Beta 0.2; web-only sites are not represented as API integrations. */
export interface OnlineProjectionProvider {
  id: string;
  name: string;
  url: string;
  mode: "reden_api" | "web_only";
  note: string;
}

export const ONLINE_PROJECTION_PROVIDERS: readonly OnlineProjectionProvider[] = [
  { id: "reden", name: "RedenMC", url: "https://redenmc.com/", mode: "reden_api", note: "应用内搜索、详情与真实投影下载；网盘链接交由浏览器。" },
  { id: "minemev", name: "Minemev", url: "https://www.minemev.com/", mode: "web_only", note: "技术 Minecraft 资料库与网页 3D 预览；在原站下载后从本地导入。" },
  { id: "mcodex", name: "MCodex", url: "https://mcodex.org/", mode: "web_only", note: "已核实网站入口；本版未接入公开 API，仅打开原站。" },
  { id: "choculaterie", name: "Choculaterie", url: "https://choculaterie.com/", mode: "web_only", note: "仅打开原站，未接入应用内搜索或下载 API。" },
  { id: "projection-workshop", name: "投影工坊", url: "https://www.mcimg.com/", mode: "web_only", note: "原站公开投影投稿资料已检索到，但当前访问超时；仅提供网页入口。" },
  { id: "abfielder", name: "Abfielder", url: "https://abfielder.com/", mode: "web_only", note: "免费投影库及独立商店；登录、付费和下载流程保留在原站。" },
  { id: "mineschematic", name: "MineSchematic", url: "https://mineschematic.com/", mode: "web_only", note: "网页预览与 .litematic / .schem / .nbt 下载；下载后从本地导入。" },
  { id: "shulkr", name: "Shulkr", url: "https://www.shulkr.com/en", mode: "web_only", note: "网页投影库、预览、编辑与格式转换；下载后从本地导入。" },
];
