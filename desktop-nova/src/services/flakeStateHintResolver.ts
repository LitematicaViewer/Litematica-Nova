import { getUserConfigFilePath, getWorkspaceRoot, readImageBase64 } from "./backend";
import { getBlockIconDataUrl } from "./blockIconResolver";
import type { LayerPaletteEntry } from "./layerService";

type FlakeStateHintMode = "mask" | "replace";

interface FlakeStateHintRule {
  mode: FlakeStateHintMode;
  imageRelPaths: string[];
}

interface ResolveFlakeLayerBlockImageInput {
  blockId: string;
  paletteEntry: LayerPaletteEntry | null | undefined;
  propertyPool: Array<Record<string, string>>;
  enabled: boolean;
}

const STAGE_1_HINT_RELPATH = "data/flake/state_hint/stage_1.png";
const HANGING_TRUE_HINT_RELPATH = "data/flake/state_hint/hanging_true.png";
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
      if (states.hanging === "true") {
        imageRelPaths.push(HANGING_TRUE_HINT_RELPATH);
      }
      if (states.stage === "1") {
        imageRelPaths.push(STAGE_1_HINT_RELPATH);
      }
      return imageRelPaths.length ? { mode: "mask", imageRelPaths } : null;
    }
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

  const hintImageUrls = await readHintImageDataUrls(rule.imageRelPaths);
  if (!hintImageUrls || hintImageUrls.length === 0) return baseIconUrl;

  if (rule.mode === "replace") {
    return hintImageUrls[hintImageUrls.length - 1] || baseIconUrl;
  }

  if (!baseIconUrl) return baseIconUrl;
  const masked = await applyMaskOverlays(baseIconUrl, hintImageUrls);
  return masked || baseIconUrl;
}