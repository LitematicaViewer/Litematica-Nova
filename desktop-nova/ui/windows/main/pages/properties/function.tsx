import { MetadataForm, RegionEdit } from "./PropertiesPage";

/**
 * 转换时间戳为日期时间字符串
 * @param value 时间戳
 * @returns 日期时间字符串
 */
export function toLocalDateTime(value: unknown): string {
  const n = Number(value || 0);
  if (!Number.isFinite(n) || n <= 0) return "";
  const d = new Date(n);
  const pad = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/**
 * 转换日期时间字符串为时间戳
 * @param value 日期时间字符串
 * @returns 时间戳
 */
export function fromLocalDateTime(value: string): number | undefined {
  if (!value) return undefined;
  const ms = new Date(value).getTime();
  return Number.isFinite(ms) ? ms : undefined;
}

/**
 * 转换meta数据为表单数据
 * @param data meta数据
 * @returns 表单数据
 */
export function metadataToForm(data: any): MetadataForm {
  const meta = data?.metadata || {};
  const regions = (data?.regions || []).map((region: any): RegionEdit => ({
    originalName: region.name || "Unnamed",
    name: region.name || "Unnamed",
    position: region.position || { x: 0, y: 0, z: 0 },
    size: region.size || { x: 0, y: 0, z: 0 },
  }));
  return {
    name: meta.name || "",
    author: meta.author || "",
    description: meta.description || "",
    time_created: toLocalDateTime(meta.time_created),
    time_modified: toLocalDateTime(meta.time_modified),
    litematic_version: Number(meta.litematic_version || 6),
    litematic_subversion: Number(meta.litematic_subversion || 1),
    minecraft_data_version: Number(meta.minecraft_data_version || 0),
    regions,
  };
}

/**
 * 构建patch数据
 * @param form 表单数据
 * @returns patch数据
 */
export function buildPatch(form: MetadataForm) {
  return {
    name: form.name,
    author: form.author,
    description: form.description,
    time_created: fromLocalDateTime(form.time_created),
    time_modified: fromLocalDateTime(form.time_modified),
    litematic_version: Number(form.litematic_version) || 6,
    litematic_subversion: Number(form.litematic_subversion) || 1,
    minecraft_data_version: Number(form.minecraft_data_version) || 0,
    regions: form.regions
      .filter((region) => region.name !== region.originalName)
      .map((region) => ({ old_name: region.originalName, new_name: region.name })),
  };
}

/**
 * 验证区域列表
 * @param regions 区域列表
 * @returns 错误信息
 */
export function validateRegions(regions: RegionEdit[]): string {
  const names = new Set<string>();
  for (const region of regions) {
    const name = region.name.trim();
    if (!name) return "区域名不能为空。";
    if (names.has(name)) return `区域名重复：${name}`;
    names.add(name);
  }
  return "";
}

