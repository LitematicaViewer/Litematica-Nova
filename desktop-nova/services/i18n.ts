import { readWorkspaceFile } from "./backend";

let langMap: Record<string, string> = {};

export async function initI18n() {
  try {
    const raw = await readWorkspaceFile("pack-in/lang/zh_cn.json");
    langMap = JSON.parse(raw);
  } catch (e) {
    console.warn("Failed to load pack-in/lang/zh_cn.json", e);
  }
}

export function translateBlockId(fullId: string): string {
  const displayId = fullId.replace("minecraft:", "");
  return langMap[`block.minecraft.${displayId}`] || langMap[`item.minecraft.${displayId}`] || displayId;
}

export function translateBuildingType(typeStr: string): string {
  switch (typeStr) {
    case "building": return "建筑";
    case "pixel_art": return "像素画";
    case "redstone": return "红石机器";
    case "mixed": return "混合结构";
    case "unknown": return "未知";
    default: return typeStr;
  }
}
