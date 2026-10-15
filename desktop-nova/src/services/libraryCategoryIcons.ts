import type { LibraryCategoryIcon } from "./libraryStore";

/** Suggested icon uses, not automatically created library categories. */
export const LIBRARY_ICON_PRESETS = [
  ["warehouse", "仓库", "Warehouse"], ["packer", "打包机", "Package"], ["crafting", "合成站", "Hammer"],
  ["building", "建筑", "Building2"], ["farm", "农场", "Wheat"], ["cannon", "各类炮", "Crosshair"],
  ["all-items", "全物品", "Boxes"], ["furnace", "熔炉组", "Flame"], ["trees", "树场", "Trees"],
  ["mobs", "刷怪塔", "Skull"], ["stone", "刷石机", "Mountain"], ["iron", "刷铁机", "Anvil"], ["other", "杂项", "Folder"],
] as const;

export const LIBRARY_LUCIDE_ICON_NAMES = ["Folder", "Warehouse", "Package", "Hammer", "Building2", "Wheat", "Crosshair", "Boxes", "Flame", "Trees", "Skull", "Mountain", "Anvil", "Box", "Cpu", "Cog", "Layers", "Star", "BookOpen"] as const;
const MAX_IMAGE_BYTES = 1024 * 1024;
const MAX_SVG_BYTES = 64 * 1024;
const SVG_ELEMENTS = new Set(["svg", "g", "path", "rect", "circle", "ellipse", "polygon", "polyline", "line", "title", "desc"]);
const SVG_ATTRIBUTES = new Set(["xmlns", "viewBox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry", "d", "points", "fill", "stroke", "stroke-width", "fill-rule", "clip-rule", "stroke-linecap", "stroke-linejoin", "stroke-miterlimit", "stroke-dasharray", "stroke-dashoffset", "opacity", "fill-opacity", "stroke-opacity", "transform", "preserveAspectRatio"]);

/** Basic geometry only: no script, animation, style, link, image, use or external reference. */
export function sanitizeLibrarySvg(source: string): string {
  if (new TextEncoder().encode(source).length > MAX_SVG_BYTES) throw new Error("SVG 图标请控制在 64 KB 以内。");
  if (/<!DOCTYPE|<!ENTITY|<\?/i.test(source)) throw new Error("SVG 图标不能包含文档声明或实体。");
  const doc = new DOMParser().parseFromString(source, "image/svg+xml");
  const root = doc.documentElement;
  if (root.localName !== "svg" || doc.querySelector("parsererror")) throw new Error("SVG 图标格式无效。");
  for (const element of [root, ...Array.from(root.querySelectorAll("*"))]) {
    if (!SVG_ELEMENTS.has(element.localName) || (element.namespaceURI && element.namespaceURI !== "http://www.w3.org/2000/svg")) throw new Error("SVG 仅支持基础图形；请移除脚本、图片、动画或外链。");
    for (const attribute of Array.from(element.attributes)) {
      if (!SVG_ATTRIBUTES.has(attribute.name) || (attribute.namespaceURI && attribute.name !== "xmlns") || /url\s*\(|javascript:|data:|https?:|file:|@import/i.test(attribute.value) && attribute.name !== "xmlns") throw new Error("SVG 图标包含不支持的样式或外部引用。");
      if (attribute.name === "xmlns" && attribute.value !== "http://www.w3.org/2000/svg") throw new Error("SVG 命名空间无效。");
      if (["fill", "stroke"].includes(attribute.name) && !/^(?:none|currentColor|transparent|#[0-9a-f]{3,8}|[a-z]+|rgba?\([\d\s.,%]+\))$/i.test(attribute.value)) throw new Error("SVG 颜色需使用纯色。");
    }
  }
  root.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  return new XMLSerializer().serializeToString(root);
}

export function validateLibraryIconDataUrl(dataUrl: string): string {
  const match = /^data:(image\/(?:png|jpeg|webp|svg\+xml));base64,([a-z0-9+/]*={0,2})$/i.exec(dataUrl);
  if (!match || dataUrl.length > Math.ceil(MAX_IMAGE_BYTES * 4 / 3) + 100) throw new Error("请选择 1 MB 以内的 PNG、JPEG、WebP 或 SVG 图标。");
  const bytes = atob(match[2]);
  if (!bytes.length || bytes.length > MAX_IMAGE_BYTES) throw new Error("图标文件为空或超过 1 MB。");
  const mime = match[1].toLowerCase();
  if (mime === "image/svg+xml") {
    const svg = sanitizeLibrarySvg(new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(bytes, (character) => character.charCodeAt(0))));
    const encoded = btoa(Array.from(new TextEncoder().encode(svg), (byte) => String.fromCharCode(byte)).join(""));
    return `data:image/svg+xml;base64,${encoded}`;
  }
  const valid = mime === "image/png" ? bytes.startsWith("\x89PNG\r\n\x1a\n")
    : mime === "image/jpeg" ? bytes.startsWith("\xff\xd8\xff")
    : bytes.startsWith("RIFF") && bytes.slice(8, 12) === "WEBP";
  if (!valid) throw new Error("图标内容与图片类型不一致，请重新选择文件。");
  return `data:${mime};base64,${match[2]}`;
}

export function normalizeLibraryCategoryIcon(raw: unknown): LibraryCategoryIcon {
  if (!raw || typeof raw !== "object") return { kind: "lucide", name: "Folder" };
  const icon = raw as LibraryCategoryIcon;
  if (icon.kind === "lucide" && LIBRARY_LUCIDE_ICON_NAMES.includes(icon.name as typeof LIBRARY_LUCIDE_ICON_NAMES[number])) return { kind: "lucide", name: icon.name };
  if (icon.kind === "minecraft" && /^minecraft:[a-z0-9_]+$/.test(icon.blockId)) return { kind: "minecraft", blockId: icon.blockId };
  if (icon.kind === "image") { try { return { kind: "image", dataUrl: validateLibraryIconDataUrl(icon.dataUrl) }; } catch { /* Invalid persisted icon never becomes executable content. */ } }
  return { kind: "lucide", name: "Folder" };
}
