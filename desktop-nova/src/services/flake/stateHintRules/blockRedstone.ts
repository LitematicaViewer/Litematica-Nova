import { readHintImageDataUrls, tintImageDataUrl, composeImageDataUrls } from "../../flakeStateHintResolver";
import { REDSTONE_WIRE_POWER_COLORS, REDSTONE_WIRE_BASE_RELPATH, REDSTONE_WIRE_DIRECTIONS, REDSTONE_WIRE_SIDE_RELPATHS, REDSTONE_WIRE_UP_RELPATHS, REDSTONE_WIRE_NUMBER_RELPATHS } from "./stateDefault";


export async function resolveRedstoneWireStateHintImage(states: Record<string, string>): Promise<string | null> {
  const power = states.power && REDSTONE_WIRE_POWER_COLORS[states.power] ? states.power : "0";
  const tintColor = REDSTONE_WIRE_POWER_COLORS[power] || REDSTONE_WIRE_POWER_COLORS["0"];
  const tintRelPaths: string[] = [REDSTONE_WIRE_BASE_RELPATH];

  for (const direction of REDSTONE_WIRE_DIRECTIONS) {
    const state = states[direction];
    if (state === "side") {
      tintRelPaths.push(REDSTONE_WIRE_SIDE_RELPATHS[direction]);
    }
    if (state === "up") {
      tintRelPaths.push(REDSTONE_WIRE_UP_RELPATHS[direction]);
    }
  }

  const tintUrls = await readHintImageDataUrls(tintRelPaths);
  if (!tintUrls) return null;

  const tintedLayerUrls = await Promise.all(tintUrls.map((url) => tintImageDataUrl(url, tintColor)));
  if (tintedLayerUrls.some((url) => !url)) return null;

  const composedLayerUrls = tintedLayerUrls.filter((url): url is string => !!url);
  const fixedRelPath = REDSTONE_WIRE_NUMBER_RELPATHS[power];
  if (fixedRelPath) {
    const fixedUrls = await readHintImageDataUrls([fixedRelPath]);
    if (fixedUrls) {
      composedLayerUrls.push(...fixedUrls);
    }
  }

  return await composeImageDataUrls(composedLayerUrls);
}
