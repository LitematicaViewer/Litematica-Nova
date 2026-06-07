import { getUserConfigFilePath, getWorkspaceRoot, readImageBase64 } from "./backend";
import { getBlockIconDataUrl } from "./blockIconResolver";
import type { LayerPaletteEntry } from "./layerService";

type FlakeStateHintMode = "mask" | "replace";
type FlakeOverlayBlendMode = "normal" | "subtract";

interface FlakeStateHintRule {
  mode: FlakeStateHintMode;
  imageRelPaths?: string[];
  iconBlockIds?: string[];
  overlayIconBlockIds?: string[];
  overlayBlendMode?: FlakeOverlayBlendMode;
}

interface ResolveFlakeLayerBlockImageInput {
  blockId: string;
  paletteEntry: LayerPaletteEntry | null | undefined;
  propertyPool: Array<Record<string, string>>;
  enabled: boolean;
}

// 遮罩
const STAGE_1_HINT_RELPATH = "data/flake/state_hint/stage_1.png";
const HANGING_TRUE_HINT_RELPATH = "data/flake/state_hint/hanging_true.png";
const SNOWY_TRUE_HINT_RELPATH = "data/flake/state_hint/snowy_true.png";
const WATERLOGGED_TRUE_BLOCK_ID = "minecraft:water";
// 反色遮罩
const LEVEL_0_OVERLAY_REPATH = "data/flake/state_overlay/level_0.png";
const AXIS_X_OVERLAY_REPATH = "data/flake/state_overlay/axis_x.png";
const AXIS_Z_OVERLAY_REPATH = "data/flake/state_overlay/axis_z.png";
// 替换
const GRASS_BLOCK_TOP_BLOCK_ID = "minecraft:grass_block_top";
// const GRASS_PATH_TOP_BLOCK_ID = "minecraft:grass_path_top";
const DIRT_PATH_TOP_BLOCK_ID = "minecraft:dirt_path_top";
const PODZOL_TOP_BLOCK_ID = "minecraft:podzol_top";
const MYCELIUM_TOP_BLOCK_ID = "minecraft:mycelium_top";
const WATER_TOP_BLOCK_ID = "minecraft:water_top";
const LAVA_TOP_BLOCK_ID = "minecraft:lava_top";
const OAK_LOG_TOP_BLOCK_ID = "minecraft:oak_log_top";
const SPRUCE_LOG_TOP_BLOCK_ID = "minecraft:spruce_log_top";
const BIRCH_LOG_TOP_BLOCK_ID = "minecraft:birch_log_top";
const JUNGLE_LOG_TOP_BLOCK_ID = "minecraft:jungle_log_top";
const ACACIA_LOG_TOP_BLOCK_ID = "minecraft:acacia_log_top";
const DARK_OAK_LOG_TOP_BLOCK_ID = "minecraft:dark_oak_log_top";
const CHERRY_LOG_TOP_BLOCK_ID = "minecraft:cherry_log_top";
const MANGROVE_LOG_TOP_BLOCK_ID = "minecraft:mangrove_log_top";

const hintImageCache = new Map<string, Promise<string | null>>();

function joinPath(root: string, relativePath: string): string {
  const separator = root.includes("\\") ? "\\" : "/";
  return `${root.replace(/[\\/]+$/, "")}${separator}${relativePath.replace(/^[\\/]+/, "").replace(/\//g, separator)}`;
}

function normalizeBlockId(blockId: string): string {
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

/** 图标解析规则 */
function resolveManualStateHintRule(blockId: string, states: Record<string, string>): FlakeStateHintRule | null {
  switch (normalizeBlockId(blockId)) {
    // 草方块
    case "minecraft:grass_block": {
      const imageRelPaths: string[] = [];
      if (states.snowy === "true") {
        imageRelPaths.push(SNOWY_TRUE_HINT_RELPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [GRASS_BLOCK_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 草径
    // case "minecraft:grass_path":
    //   return { mode: "replace", iconBlockIds: [GRASS_PATH_TOP_BLOCK_ID] };
    // 土径
    case "minecraft:dirt_path":
      return { mode: "replace", iconBlockIds: [DIRT_PATH_TOP_BLOCK_ID] };
    // 灰化土
    case "minecraft:podzol": {
      const imageRelPaths: string[] = [];
      if (states.snowy === "true") {
        imageRelPaths.push(SNOWY_TRUE_HINT_RELPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [PODZOL_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 菌丝体
    case "minecraft:mycelium":{
      const imageRelPaths: string[] = [];
      if (states.snowy === "true") {
        imageRelPaths.push(SNOWY_TRUE_HINT_RELPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [MYCELIUM_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 水
    case "minecraft:water": {
      const imageRelPaths: string[] = [];
      if (states.level === "0") {
        imageRelPaths.push(LEVEL_0_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [WATER_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 熔岩
    case "minecraft:lava": {
      const imageRelPaths: string[] = [];
      if (states.level === "0") {
        imageRelPaths.push(LEVEL_0_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [LAVA_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 树苗
    case "minecraft:oak_sapling":
    case "minecraft:spruce_sapling":
    case "minecraft:birch_sapling":
    case "minecraft:jungle_sapling":
    case "minecraft:acacia_sapling":
    case "minecraft:dark_oak_sapling":
    case "minecraft:cherry_sapling": {
      const imageRelPaths: string[] = [];
      if (states.stage === "1") {
        imageRelPaths.push(STAGE_1_HINT_RELPATH);
      }
      return imageRelPaths.length ? { mode: "mask", imageRelPaths } : null;
    }
    // 红树胎生苗
    case "minecraft:mangrove_propagule": {
      const imageRelPaths: string[] = [];
      const overlayIconBlockIds: string[] = [];
      
      if (states.hanging === "true") {
        imageRelPaths.push(HANGING_TRUE_HINT_RELPATH);
      }
      if (states.stage === "1") {
        imageRelPaths.push(STAGE_1_HINT_RELPATH);
      }
      if (states.waterlogged === "true") {
        overlayIconBlockIds.push(WATERLOGGED_TRUE_BLOCK_ID);
      }
      return imageRelPaths.length || overlayIconBlockIds.length
        ? {
            mode: "mask",
            imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
            overlayIconBlockIds: overlayIconBlockIds.length ? overlayIconBlockIds : undefined,
          }
        : null;
    }
    // 木头
    case "minecraft:oak_wood":
    case "minecraft:spruce_wood":
    case "minecraft:birch_wood":
    case "minecraft:jungle_wood":
    case "minecraft:acacia_wood":
    case "minecraft:dark_oak_wood":
    case "minecraft:cherry_wood":
    case "minecraft:mangrove_wood":
    {
      const imageRelPaths: string[] = [];
      if (states.axis === "x") {
        imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
      }
      if (states.axis === "z") {
        imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
      }
      return imageRelPaths.length
        ? { mode: "mask", imageRelPaths, overlayBlendMode: "subtract" }
        : null;
    }
    // 橡木原木
    case "minecraft:oak_log": {
      const imageRelPaths: string[] = [];
      if (states.axis === "x") {
        imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
      }
      if (states.axis === "z") {
        imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [OAK_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    case "minecraft:spruce_log":
    case "minecraft:birch_log":
    case "minecraft:jungle_log":
    case "minecraft:acacia_log":
    case "minecraft:dark_oak_log":
    case "minecraft:cherry_log":
    case "minecraft:mangrove_log":
    
    default:
      return null;
  }
}

async function readHintImageDataUrl(relativePath: string): Promise<string | null> {
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

async function readHintImageDataUrls(relativePaths: string[]): Promise<string[] | null> {
  try {
    const urls = await Promise.all(relativePaths.map((relativePath) => readHintImageDataUrl(relativePath)));
    if (urls.some((url) => !url)) return null;
    return urls.filter((url): url is string => !!url);
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

async function resolveRuleBaseImageUrls(rule: FlakeStateHintRule): Promise<string[] | null> {
  const iconBlockIds = rule.iconBlockIds;
  if (iconBlockIds && iconBlockIds.length > 0) {
    return await readLayeringIconDataUrls(iconBlockIds);
  }
  return null;
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("image load failed"));
    image.src = url;
  });
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
    context.clearRect(0, 0, width, height);
    context.drawImage(baseImage, 0, 0, width, height);

    for (const overlayImage of overlayImages) {
      const overlayCanvas = document.createElement("canvas");
      overlayCanvas.width = width;
      overlayCanvas.height = height;
      const overlayContext = overlayCanvas.getContext("2d", { willReadFrequently: true });
      if (!overlayContext) return null;
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
  const rule = resolveManualStateHintRule(blockId, states);
  if (!rule) return baseIconUrl;

  const baseImageUrls = await resolveRuleBaseImageUrls(rule);
  const resolvedBaseIconUrl = baseImageUrls && baseImageUrls.length > 0
    ? (baseImageUrls[baseImageUrls.length - 1] || baseIconUrl)
    : baseIconUrl;

  const hintImageUrls = await resolveRuleImageUrls(rule);
  if (rule.mode === "replace" && (!hintImageUrls || hintImageUrls.length === 0)) {
    return resolvedBaseIconUrl;
  }

  if (!hintImageUrls || hintImageUrls.length === 0) return baseIconUrl;
  if (!resolvedBaseIconUrl) return baseIconUrl;

  const masked = rule.overlayBlendMode === "subtract"
    ? await applySubtractOverlays(resolvedBaseIconUrl, hintImageUrls)
    : await applyMaskOverlays(resolvedBaseIconUrl, hintImageUrls);
  return masked || baseIconUrl;
}