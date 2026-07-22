import { image } from "@tauri-apps/api";
import { getUserConfigFilePath, getWorkspaceRoot, readImageBase64 } from "./backend";
import { getBlockIconDataUrl } from "./blockIconResolver";
import type { LayerPaletteEntry } from "./layerService";
import { isBlockInEnumerator } from "./flake/enumeratorLoader";
import { resolveManualStateHintRule } from "./flake/stateHintRules/stateDefault";
import { resolveRedstoneWireStateHintImage } from "./flake/stateHintRules/blockRedstone";

// 含水方块枚举器的同步缓存
let waterloggedBlocksSet: Set<string> | null = null;

// 预加载含水方块枚举器（在模块初始化时异步加载）
(async () => {
  try {
    // 触发加载，利用 enumeratorLoader 的内部缓存
    const testBlock = "minecraft:chest";
    await isBlockInEnumerator(testBlock, "enumerator/system_enum/state_waterlogged.json");
    // 注意：这里只是预热缓存，实际检查仍然需要调用 isBlockInEnumerator
  } catch (error) {
    console.warn("Failed to preload waterlogged enumerator:", error);
  }
})();

type FlakeStateHintMode = "mask" | "replace";

export interface FlakeStateHintRule {
  mode: FlakeStateHintMode;
  imageRelPaths?: string[];
  subtractImageRelPaths?: string[];
  iconBlockIds?: string[];
  baseImageRelPaths?: string[];
  baseImageRotateQuarterTurns?: number;
  overlayIconBlockIds?: string[];
  subtractOverlayIconBlockIds?: string[];
}

interface ResolveFlakeLayerBlockImageInput {
  blockId: string;
  paletteEntry: LayerPaletteEntry | null | undefined;
  propertyPool: Array<Record<string, string>>;
  enabled: boolean;
}

const hintImageCache = new Map<string, Promise<string | null>>();

// 缺失遮罩贴图时使用的万能兜底遮罩。
const ALT_HINT_RELPATH = "data/flake/state_hint/alt.png";
// 仅对遮罩层（state_hint / state_overlay）启用兜底，base/替换类贴图不受影响。
const MASK_FALLBACK_PREFIXES = ["data/flake/state_hint/", "data/flake/state_overlay/"];

function isMaskRelPath(relativePath: string): boolean {
  const normalized = relativePath.replace(/\\/g, "/");
  return MASK_FALLBACK_PREFIXES.some((prefix) => normalized.startsWith(prefix));
}

function joinPath(root: string, relativePath: string): string {
  const separator = root.includes("\\") ? "\\" : "/";
  return `${root.replace(/[\\/]+$/, "")}${separator}${relativePath.replace(/^[\\/]+/, "").replace(/\//g, separator)}`;
}

export function normalizeBlockId(blockId: string): string {
  const normalized = String(blockId || "").trim().toLowerCase();
  if (!normalized) return "";
  return normalized.includes(":") ? normalized : `minecraft:${normalized}`;
}

function appendStateRecord(target: Map<string, string>, source: Record<string, unknown> | null | undefined): void {
  if (!source) return;
  for (const [key, value] of Object.entries(source)) {
    if (!key || value === null || value === undefined) continue;
    target.set(String(key), String(value));
  }
}

function appendUnknownState(target: Map<string, string>, value: unknown): void {
  if (value === null || value === undefined) return;
  if (Array.isArray(value)) {
    for (const entry of value) {
      appendUnknownState(target, entry);
    }
    return;
  }
  if (typeof value === "object") {
    appendStateRecord(target, value as Record<string, unknown>);
  }
}

export function extractLayerPaletteStates(
  entry: LayerPaletteEntry | null | undefined,
  propertyPool: Array<Record<string, string>>,
): Record<string, string> {
  const states = new Map<string, string>();
  if (!entry) return {};

  if (typeof entry.property_id === "number" && entry.property_id >= 0 && entry.property_id < propertyPool.length) {
    appendStateRecord(states, propertyPool[entry.property_id]);
  }

  appendUnknownState(states, entry.block_state);
  appendUnknownState(states, entry.state);
  appendUnknownState(states, entry.states);
  appendUnknownState(states, entry.properties);

  const refKeys: Array<keyof LayerPaletteEntry> = [
    "property_ids",
    "property_indices",
    "property_refs",
    "state_ids",
    "state_indices",
  ];
  for (const key of refKeys) {
    const refs = entry[key];
    if (!Array.isArray(refs)) continue;
    for (const ref of refs) {
      if (typeof ref === "number" && ref >= 0 && ref < propertyPool.length) {
        appendStateRecord(states, propertyPool[ref]);
      }
    }
  }

  return Object.fromEntries(states.entries());
}

async function readHintImageDataUrlRaw(relativePath: string): Promise<string | null> {
  if (!hintImageCache.has(relativePath)) {
    hintImageCache.set(relativePath, (async () => {
      try {
        const workspaceRoot = await getWorkspaceRoot();
        const candidates = [
          joinPath(workspaceRoot, relativePath),
          joinPath(workspaceRoot, `desktop-nova/${relativePath}`),
          await getUserConfigFilePath(relativePath),
        ];
        for (const candidate of candidates) {
          try {
            const dataUrl = await readImageBase64(candidate);
            if (dataUrl) return dataUrl;
          } catch {
            // Try the next path.
          }
        }
        return null;
      } catch {
        return null;
      }
    })());
  }
  return await hintImageCache.get(relativePath)!;
}

async function readHintImageDataUrl(relativePath: string): Promise<string | null> {
  const dataUrl = await readHintImageDataUrlRaw(relativePath);
  if (dataUrl) return dataUrl;
  // 遮罩贴图缺失时回退到万能遮罩 alt.png，避免整个覆盖层被丢弃。
  if (isMaskRelPath(relativePath) && relativePath !== ALT_HINT_RELPATH) {
    return await readHintImageDataUrlRaw(ALT_HINT_RELPATH);
  }
  return null;
}

export async function readHintImageDataUrls(relativePaths: string[]): Promise<string[] | null> {
  try {
    const urls = await Promise.all(relativePaths.map((relativePath) => readHintImageDataUrl(relativePath)));
    if (urls.some((url) => !url)) return null;
    return urls.filter((url): url is string => !!url);
  } catch {
    return null;
  }
}

function parseHexColor(hex: string): [number, number, number] | null {
  const normalized = String(hex || "").trim();
  const matched = normalized.match(/^#([0-9a-fA-F]{6})$/);
  if (!matched) return null;
  const value = matched[1];
  return [
    Number.parseInt(value.slice(0, 2), 16),
    Number.parseInt(value.slice(2, 4), 16),
    Number.parseInt(value.slice(4, 6), 16),
  ];
}

export async function tintImageDataUrl(url: string, colorHex: string): Promise<string | null> {
  try {
    const rgb = parseHexColor(colorHex);
    if (!rgb) return null;
    const [red, green, blue] = rgb;
    const image = await loadImage(url);
    const width = Math.max(1, image.naturalWidth || image.width || 16);
    const height = Math.max(1, image.naturalHeight || image.height || 16);
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.imageSmoothingEnabled = false;
    context.clearRect(0, 0, width, height);
    context.drawImage(image, 0, 0, width, height);
    context.globalCompositeOperation = "source-in";
    context.fillStyle = `rgb(${red}, ${green}, ${blue})`;
    context.fillRect(0, 0, width, height);
    context.globalCompositeOperation = "source-over";
    return canvas.toDataURL("image/png");
  } catch {
    return null;
  }
}

export async function composeImageDataUrls(layerUrls: string[]): Promise<string | null> {
  try {
    if (!layerUrls.length) return null;
    const images = await Promise.all(layerUrls.map((url) => loadImage(url)));
    const width = Math.max(1, ...images.map((image) => image.naturalWidth || image.width || 16));
    const height = Math.max(1, ...images.map((image) => image.naturalHeight || image.height || 16));
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.imageSmoothingEnabled = false;
    context.clearRect(0, 0, width, height);
    for (const image of images) {
      context.drawImage(image, 0, 0, width, height);
    }
    return canvas.toDataURL("image/png");
  } catch {
    return null;
  }
}

/** 读取图标数据URL 
 * @param blockIds - 图标ID列表
 * @returns 图标数据URL列表
*/
async function readLayeringIconDataUrls(blockIds: string[]): Promise<string[] | null> {
  try {
    const urls = await Promise.all(blockIds.map((blockId) => getBlockIconDataUrl(blockId, "layering")));
    if (urls.some((url) => !url)) return null;
    return urls.filter((url): url is string => !!url);
  } catch {
    return null;
  }
}

/** 解析图标数据URL
 * @param rule - 图标解析规则
 * @returns 图标数据URL列表
 */
async function resolveRuleImageUrls(rule: FlakeStateHintRule): Promise<string[] | null> {
  const urls: string[] = [];

  const overlayIconBlockIds = rule.overlayIconBlockIds;
  if (overlayIconBlockIds && overlayIconBlockIds.length > 0) {
    const overlayIconUrls = await readLayeringIconDataUrls(overlayIconBlockIds);
    if (!overlayIconUrls) return null;
    urls.push(...overlayIconUrls);
  }

  const imageRelPaths = rule.imageRelPaths;
  if (imageRelPaths && imageRelPaths.length > 0) {
    const hintUrls = await readHintImageDataUrls(imageRelPaths);
    if (!hintUrls) return null;
    urls.push(...hintUrls);
  }

  return urls.length ? urls : null;
}

async function resolveRuleSubtractImageUrls(rule: FlakeStateHintRule): Promise<string[] | null> {
  const urls: string[] = [];

  const subtractOverlayIconBlockIds = rule.subtractOverlayIconBlockIds;
  if (subtractOverlayIconBlockIds && subtractOverlayIconBlockIds.length > 0) {
    const overlayIconUrls = await readLayeringIconDataUrls(subtractOverlayIconBlockIds);
    if (!overlayIconUrls) return null;
    urls.push(...overlayIconUrls);
  }

  const subtractImageRelPaths = rule.subtractImageRelPaths;
  if (subtractImageRelPaths && subtractImageRelPaths.length > 0) {
    const hintUrls = await readHintImageDataUrls(subtractImageRelPaths);
    if (!hintUrls) return null;
    urls.push(...hintUrls);
  }

  return urls.length ? urls : null;
}

async function resolveRuleBaseImageUrls(rule: FlakeStateHintRule): Promise<string[] | null> {
  let baseImageUrls: string[] | null = null;

  const baseImageRelPaths = rule.baseImageRelPaths;
  if (baseImageRelPaths && baseImageRelPaths.length > 0) {
    baseImageUrls = await readHintImageDataUrls(baseImageRelPaths);
  } else {
    const iconBlockIds = rule.iconBlockIds;
    if (iconBlockIds && iconBlockIds.length > 0) {
      baseImageUrls = await readLayeringIconDataUrls(iconBlockIds);
    }
  }

  if (!baseImageUrls) return null;

  const rotateQuarterTurns = rule.baseImageRotateQuarterTurns || 0;
  if (rotateQuarterTurns === 0) return baseImageUrls;

  const rotatedUrls = await Promise.all(baseImageUrls.map((url) => rotateImageDataUrl(url, rotateQuarterTurns)));
  if (rotatedUrls.some((url) => !url)) return null;
  return rotatedUrls.filter((url): url is string => !!url);
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("image load failed"));
    image.src = url;
  });
}

async function rotateImageDataUrl(url: string, quarterTurns: number): Promise<string | null> {
  try {
    const normalizedTurns = ((quarterTurns % 4) + 4) % 4;
    if (normalizedTurns === 0) return url;
    const image = await loadImage(url);
    const sourceWidth = Math.max(1, image.naturalWidth || image.width || 16);
    const sourceHeight = Math.max(1, image.naturalHeight || image.height || 16);
    const swapAxis = normalizedTurns % 2 === 1;
    const canvas = document.createElement("canvas");
    canvas.width = swapAxis ? sourceHeight : sourceWidth;
    canvas.height = swapAxis ? sourceWidth : sourceHeight;
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.imageSmoothingEnabled = false;
    context.translate(canvas.width / 2, canvas.height / 2);
    context.rotate(normalizedTurns * Math.PI / 2);
    context.drawImage(image, -sourceWidth / 2, -sourceHeight / 2, sourceWidth, sourceHeight);
    return canvas.toDataURL("image/png");
  } catch {
    return null;
  }
}

async function applyMaskOverlays(baseIconUrl: string, overlayUrls: string[]): Promise<string | null> {
  try {
    const images = await Promise.all([loadImage(baseIconUrl), ...overlayUrls.map((overlayUrl) => loadImage(overlayUrl))]);
    const [baseImage, ...overlayImages] = images;
    const width = Math.max(1, ...images.map((image) => image.naturalWidth || image.width || 16));
    const height = Math.max(1, ...images.map((image) => image.naturalHeight || image.height || 16));
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.imageSmoothingEnabled = false;
    context.clearRect(0, 0, width, height);
    context.drawImage(baseImage, 0, 0, width, height);
    for (const overlayImage of overlayImages) {
      context.drawImage(overlayImage, 0, 0, width, height);
    }
    return canvas.toDataURL("image/png");
  } catch {
    return null;
  }
}

async function applySubtractOverlays(baseIconUrl: string, overlayUrls: string[]): Promise<string | null> {
  try {
    const images = await Promise.all([loadImage(baseIconUrl), ...overlayUrls.map((overlayUrl) => loadImage(overlayUrl))]);
    const [baseImage, ...overlayImages] = images;
    const width = Math.max(1, ...images.map((image) => image.naturalWidth || image.width || 16));
    const height = Math.max(1, ...images.map((image) => image.naturalHeight || image.height || 16));
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) return null;
    context.imageSmoothingEnabled = false;
    context.clearRect(0, 0, width, height);
    context.drawImage(baseImage, 0, 0, width, height);

    for (const overlayImage of overlayImages) {
      const overlayCanvas = document.createElement("canvas");
      overlayCanvas.width = width;
      overlayCanvas.height = height;
      const overlayContext = overlayCanvas.getContext("2d", { willReadFrequently: true });
      if (!overlayContext) return null;
      overlayContext.imageSmoothingEnabled = false;
      overlayContext.clearRect(0, 0, width, height);
      overlayContext.drawImage(overlayImage, 0, 0, width, height);

      const baseData = context.getImageData(0, 0, width, height);
      const overlayData = overlayContext.getImageData(0, 0, width, height);
      const pixels = baseData.data;
      const overlayPixels = overlayData.data;

      for (let index = 0; index < pixels.length; index += 4) {
        const overlayAlpha = overlayPixels[index + 3] / 255;
        if (overlayAlpha <= 0) continue;
        const luminance = (overlayPixels[index] + overlayPixels[index + 1] + overlayPixels[index + 2]) / (255 * 3);
        const factor = Math.max(0, Math.min(1, luminance * overlayAlpha));
        if (factor <= 0) continue;
        pixels[index] = Math.round(pixels[index] * (1 - factor) + (255 - pixels[index]) * factor);
        pixels[index + 1] = Math.round(pixels[index + 1] * (1 - factor) + (255 - pixels[index + 1]) * factor);
        pixels[index + 2] = Math.round(pixels[index + 2] * (1 - factor) + (255 - pixels[index + 2]) * factor);
      }

      context.putImageData(baseData, 0, 0);
    }

    return canvas.toDataURL("image/png");
  } catch {
    return null;
  }
}

export async function resolveFlakeLayerBlockImage({
  blockId,
  paletteEntry,
  propertyPool,
  enabled,
}: ResolveFlakeLayerBlockImageInput): Promise<string | null> {
  const baseIconUrl = await getBlockIconDataUrl(blockId, "layering");
  if (!enabled) return baseIconUrl;

  const states = extractLayerPaletteStates(paletteEntry, propertyPool);
  if (normalizeBlockId(blockId) === "minecraft:redstone_wire") {
    const redstoneWireImage = await resolveRedstoneWireStateHintImage(states);
    return redstoneWireImage || baseIconUrl;
  }

  const rule = await resolveManualStateHintRule(blockId, states);
  if (!rule) return baseIconUrl;

  const baseImageUrls = await resolveRuleBaseImageUrls(rule);
  const resolvedBaseIconUrl = baseImageUrls && baseImageUrls.length > 0
    ? (baseImageUrls[baseImageUrls.length - 1] || baseIconUrl)
    : baseIconUrl;

  const normalHintImageUrls = await resolveRuleImageUrls(rule);
  const subtractHintImageUrls = await resolveRuleSubtractImageUrls(rule);
  const hasNormalOverlays = !!normalHintImageUrls && normalHintImageUrls.length > 0;
  const hasSubtractOverlays = !!subtractHintImageUrls && subtractHintImageUrls.length > 0;
  if (rule.mode === "replace" && !hasNormalOverlays && !hasSubtractOverlays) {
    return resolvedBaseIconUrl;
  }

  if (!hasNormalOverlays && !hasSubtractOverlays) return baseIconUrl;
  if (!resolvedBaseIconUrl) return baseIconUrl;

  let composedIconUrl = resolvedBaseIconUrl;
  if (hasSubtractOverlays) {
    const masked = await applySubtractOverlays(composedIconUrl, subtractHintImageUrls);
    if (!masked) return baseIconUrl;
    composedIconUrl = masked;
  }

  if (hasNormalOverlays) {
    const masked = await applyMaskOverlays(composedIconUrl, normalHintImageUrls);
    if (!masked) return baseIconUrl;
    composedIconUrl = masked;
  }

  return composedIconUrl || baseIconUrl;
}
