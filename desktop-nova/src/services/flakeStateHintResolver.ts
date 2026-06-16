import { image } from "@tauri-apps/api";
import { getUserConfigFilePath, getWorkspaceRoot, readImageBase64 } from "./backend";
import { getBlockIconDataUrl } from "./blockIconResolver";
import type { LayerPaletteEntry } from "./layerService";
import { applyWaterloggedOverlay } from "./flake/stateHintRules/stateWaterlogged";
import { isBlockInEnumerator } from "./flake/enumeratorLoader";

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
type FlakeOverlayBlendMode = "normal" | "subtract";

interface FlakeStateHintRule {
  mode: FlakeStateHintMode;
  imageRelPaths?: string[];
  iconBlockIds?: string[];
  baseImageRelPaths?: string[];
  baseImageRotateQuarterTurns?: number;
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
const PERSISTENT_FALSE_BLOCK_ID = "data/flake/state_hint/persistent_false.png";

// 反色遮罩
const LEVEL_0_OVERLAY_REPATH = "data/flake/state_overlay/level_0.png";
const AXIS_X_OVERLAY_REPATH = "data/flake/state_overlay/axis_x.png";
const AXIS_Z_OVERLAY_REPATH = "data/flake/state_overlay/axis_z.png";
// 替换
  // 草皮
const GRASS_BLOCK_TOP_BLOCK_ID = "minecraft:grass_block_top";
// const GRASS_PATH_TOP_BLOCK_ID = "minecraft:grass_path_top";
const DIRT_PATH_TOP_BLOCK_ID = "minecraft:dirt_path_top";
const PODZOL_TOP_BLOCK_ID = "minecraft:podzol_top";
const MYCELIUM_TOP_BLOCK_ID = "minecraft:mycelium_top";
  //液体
const WATER_TOP_BLOCK_ID = "minecraft:water_top";
const LAVA_TOP_BLOCK_ID = "minecraft:lava_top";
  // 原木
const OAK_LOG_TOP_BLOCK_ID = "minecraft:oak_log_top";
const SPRUCE_LOG_TOP_BLOCK_ID = "minecraft:spruce_log_top";
const BIRCH_LOG_TOP_BLOCK_ID = "minecraft:birch_log_top";
const JUNGLE_LOG_TOP_BLOCK_ID = "minecraft:jungle_log_top";
const ACACIA_LOG_TOP_BLOCK_ID = "minecraft:acacia_log_top";
const DARK_OAK_LOG_TOP_BLOCK_ID = "minecraft:dark_oak_log_top";
const CHERRY_LOG_TOP_BLOCK_ID = "minecraft:cherry_log_top";
const MANGROVE_LOG_TOP_BLOCK_ID = "minecraft:mangrove_log_top";
  // 发射器
const DISPENSER_TOP_RELPATH = "data/flake/redstone_display/dispenser_top.png";
const DISPENSER_TOP_ON_RELPATH = "data/flake/redstone_display/dispenser_top_on.png";
const DISPENSER_FRONT_VERTICAL_RELPATH = "data/flake/redstone_display/dispenser_front_vertical.png";
const DISPENSER_FRONT_VERTICAL_ON_RELPATH = "data/flake/redstone_display/dispenser_front_vertical_on.png";
const DISPENSER_BOTTOM_RELPATH = "data/flake/redstone_display/dispenser_bottom.png";
const DISPENSER_BOTTOM_ON_RELPATH = "data/flake/redstone_display/dispenser_bottom_on.png";
  // 投掷器
const DROPPER_TOP_RELPATH = "data/flake/redstone_display/dropper_top.png";
const DROPPER_TOP_ON_RELPATH = "data/flake/redstone_display/dropper_top_on.png";
const DROPPER_FRONT_VERTICAL_RELPATH = "data/flake/redstone_display/dropper_front_vertical.png";
const DROPPER_FRONT_VERTICAL_ON_RELPATH = "data/flake/redstone_display/dropper_front_vertical_on.png";
const DROPPER_BOTTOM_RELPATH = "data/flake/redstone_display/dropper_bottom.png";
const DROPPER_BOTTOM_ON_RELPATH = "data/flake/redstone_display/dropper_bottom_on.png";
  // 漏斗
const HOPPER_INSIDE_DOWN_RELPATH = "data/flake/redstone_display/hopper_inside.png";
const HOPPER_INSIDE_DOWN_ON_RELPATH = "data/flake/redstone_display/hopper_inside_on.png";
const HOPPER_INSIDE_VERT_RELPATH = "data/flake/redstone_display/hopper_inside2.png";
const HOPPER_INSIDE_VERT_ON_RELPATH = "data/flake/redstone_display/hopper_inside2_on.png";
const HOPPER_TOP_RELPATH = "data/flake/redstone_display/hopper_top.png";
const HOPPER_TOP_ON_RELPATH = "data/flake/redstone_display/hopper_top_on.png";
  // 侦测器
const OBSERVER_BLOCK_ID = "minecraft:observer";
const OBSERVER_BACK_BLOCK_ID = "minecraft:observer_back";
const OBSERVER_TOP_RELPATH = "data/flake/redstone_display/observer_top.png";
  // 活塞
const PISTON_TOP_RELPATH = "data/flake/redstone_display/piston_top.png";
const PISTON_TOP_ON_RELPATH = "data/flake/redstone_display/piston_top_on.png";
const PISTON_BOTTOM_RELPATH = "data/flake/redstone_display/piston_bottom.png";
const PISTON_BOTTOM_ON_RELPATH = "data/flake/redstone_display/piston_bottom_on.png";
const PISTON_SIDE_RELPATH = "data/flake/redstone_display/piston_side.png";
const PISTON_SIDE_ON_RELPATH = "data/flake/redstone_display/piston_side_on.png";
const PISTON_INNER_RELPATH = "data/flake/redstone_display/piston_inner.png";
const PISTON_INNER2_RELPATH = "data/flake/redstone_display/piston_inner2.png";
const PISTON_HEAD_BLOCK_ID = "minecraft:piston_head";
  // 黏性活塞
const PISTON_TOP_STICKY_RELPATH = "data/flake/redstone_display/piston_top_sticky.png";
const PISTON_TOP_STICKY_ON_RELPATH = "data/flake/redstone_display/piston_top_sticky_on.png";
const PISTON_BOTTOM_STICKY_RELPATH = "data/flake/redstone_display/piston_bottom_sticky.png";
const PISTON_BOTTOM_STICKY_ON_RELPATH = "data/flake/redstone_display/piston_bottom_sticky_on.png";
const PISTON_SIDE_STICKY_RELPATH = "data/flake/redstone_display/piston_side_sticky.png";
const PISTON_SIDE_STICKY_ON_RELPATH = "data/flake/redstone_display/piston_side_sticky_on.png";
const PISTON_INNER_STICKY_RELPATH = "data/flake/redstone_display/piston_inner_sticky.png";
const PISTON_HEAD_STICKY_RELPATH = "data/flake/redstone_display/piston_head_sticky.png";
const PISTON_INNER2_STICKY_RELPATH = "data/flake/redstone_display/piston_inner2_sticky.png";
  // 红石中继器
const REPEATOR1_RELPATH = "data/flake/redstone_display/repeater1.png";
const REPEATOR_ON1_RELPATH = "data/flake/redstone_display/repeater_on1.png";
const REPEATOR2_RELPATH = "data/flake/redstone_display/repeater2.png";
const REPEATOR_ON2_RELPATH = "data/flake/redstone_display/repeater_on2.png";
const REPEATOR3_RELPATH = "data/flake/redstone_display/repeater3.png";
const REPEATOR_ON3_RELPATH = "data/flake/redstone_display/repeater_on3.png";
const REPEATOR4_RELPATH = "data/flake/redstone_display/repeater4.png";
const REPEATOR_ON4_RELPATH = "data/flake/redstone_display/repeater_on4.png";
const REPEATOR_LOCK_RELPATH = "data/flake/redstone_display/repeater_lock.png";
  // 红石比较器
const COMPARATOR_RELPATH = "data/flake/redstone_display/comparator.png";
const COMPARATOR_ON_RELPATH = "data/flake/redstone_display/comparator_on.png";
const COMPARATOR2_RELPATH = "data/flake/redstone_display/comparator2.png";
const COMPARATOR2_ON_RELPATH = "data/flake/redstone_display/comparator2_on.png";
  // 红石线
const REDSTONE_WIRE_BASE_RELPATH = "data/flake/redstone_display/redstone_wire_base.png";
const REDSTONE_WIRE_NORTH_RELPATH = "data/flake/redstone_display/redstone_wire_north.png";
const REDSTONE_WIRE_EAST_RELPATH = "data/flake/redstone_display/redstone_wire_east.png";
const REDSTONE_WIRE_SOUTH_RELPATH = "data/flake/redstone_display/redstone_wire_south.png";
const REDSTONE_WIRE_WEST_RELPATH = "data/flake/redstone_display/redstone_wire_west.png";
const REDSTONE_WIRE_NORTH_UP_RELPATH = "data/flake/redstone_display/redstone_wire_north_up.png";
const REDSTONE_WIRE_EAST_UP_RELPATH = "data/flake/redstone_display/redstone_wire_east_up.png";
const REDSTONE_WIRE_SOUTH_UP_RELPATH = "data/flake/redstone_display/redstone_wire_south_up.png";
const REDSTONE_WIRE_WEST_UP_RELPATH = "data/flake/redstone_display/redstone_wire_west_up.png";
const REDSTONE_WIRE_SIDE_RELPATHS: Record<string, string> = {
  north: REDSTONE_WIRE_NORTH_RELPATH,
  east: REDSTONE_WIRE_EAST_RELPATH,
  south: REDSTONE_WIRE_SOUTH_RELPATH,
  west: REDSTONE_WIRE_WEST_RELPATH,
};

const REDSTONE_WIRE_UP_RELPATHS: Record<string, string> = {
  north: REDSTONE_WIRE_NORTH_UP_RELPATH,
  east: REDSTONE_WIRE_EAST_UP_RELPATH,
  south: REDSTONE_WIRE_SOUTH_UP_RELPATH,
  west: REDSTONE_WIRE_WEST_UP_RELPATH,
};
const REDSTONE_WIRE_DIRECTIONS = ["north", "east", "south", "west"] as const;
  // 数字
const NUM_0_RELPATH = "data/flake/redstone_display/num_0.png";
const NUM_1_RELPATH = "data/flake/redstone_display/num_1.png";
const NUM_2_RELPATH = "data/flake/redstone_display/num_2.png";
const NUM_3_RELPATH = "data/flake/redstone_display/num_3.png";
const NUM_4_RELPATH = "data/flake/redstone_display/num_4.png";
const NUM_5_RELPATH = "data/flake/redstone_display/num_5.png";
const NUM_6_RELPATH = "data/flake/redstone_display/num_6.png";
const NUM_7_RELPATH = "data/flake/redstone_display/num_7.png";
const NUM_8_RELPATH = "data/flake/redstone_display/num_8.png";
const NUM_9_RELPATH = "data/flake/redstone_display/num_9.png";
const NUM_10_RELPATH = "data/flake/redstone_display/num_10.png";
const NUM_11_RELPATH = "data/flake/redstone_display/num_11.png";
const NUM_12_RELPATH = "data/flake/redstone_display/num_12.png";
const NUM_13_RELPATH = "data/flake/redstone_display/num_13.png";
const NUM_14_RELPATH = "data/flake/redstone_display/num_14.png";
const NUM_15_RELPATH = "data/flake/redstone_display/num_15.png";
const REDSTONE_WIRE_NUMBER_RELPATHS: Record<string, string> = {
  "0": NUM_0_RELPATH,
  "1": NUM_1_RELPATH,
  "2": NUM_2_RELPATH,
  "3": NUM_3_RELPATH,
  "4": NUM_4_RELPATH,
  "5": NUM_5_RELPATH,
  "6": NUM_6_RELPATH,
  "7": NUM_7_RELPATH,
  "8": NUM_8_RELPATH,
  "9": NUM_9_RELPATH,
  "10": NUM_10_RELPATH,
  "11": NUM_11_RELPATH,
  "12": NUM_12_RELPATH,
  "13": NUM_13_RELPATH,
  "14": NUM_14_RELPATH,
  "15": NUM_15_RELPATH,
};
// 红石颜色
const REDSTONE_WIRE_POWER_COLORS: Record<string, string> = {
  "0": "#4C0000",
  "1": "#700000",
  "2": "#7A0000",
  "3": "#840000",
  "4": "#8E0000",
  "5": "#990000",
  "6": "#A30000",
  "7": "#AD0000",
  "8": "#B70000",
  "9": "#C10000",
  "10": "#CC0000",
  "11": "#D60000",
  "12": "#E00000",
  "13": "#EA0000",
  "14": "#F41B00",
  "15": "#FF3200",
};
  // 地上的火把
const TORCH_HEAD_RELPATH = "data/flake/state_replace/torch_head.png";
const SOUL_TORCH_HEAD_RELPATH = "data/flake/state_replace/soul_torch_head.png";
const COPPER_TORCH_HEAD_RELPATH = "data/flake/state_replace/copper_torch_head.png";
const REDSTONE_TORCH_HEAD_RELPATH = "data/flake/redstone_display/redstone_torch_head.png";
const REDSTONE_TORCH_HEAD_OFF_RELPATH = "data/flake/redstone_display/redstone_torch_head_off.png";
  // 墙上的火把
const WALL_TORCH_BLOCK_ID = "minecraft:wall_torch";
const SOUL_WALL_TORCH_BLOCK_ID = "minecraft:soul_wall_torch";
const COPPER_WALL_TORCH_BLOCK_ID = "minecraft:copper_wall_torch";
const REDSTONE_WALL_TORCH_BLOCK_ID = "minecraft:redstone_wall_torch";
const REDSTONE_WALL_TORCH_OFF_BLOCK_ID = "minecraft:redstone_wall_torch_off";

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
async function resolveManualStateHintRule(blockId: string, states: Record<string, string>): Promise<FlakeStateHintRule | null> {
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
      applyWaterloggedOverlay(states, imageRelPaths, overlayIconBlockIds);
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
    // 云杉原木
    case "minecraft:spruce_log": {
      const imageRelPaths: string[] = [];
      if (states.axis === "x") {
        imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
      }
      if (states.axis === "z") {
        imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [SPRUCE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 白桦原木
    case "minecraft:birch_log": {
      const imageRelPaths: string[] = [];
      if (states.axis === "x") {
        imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
      }
      if (states.axis === "z") {
        imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [BIRCH_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 丛林原木
    case "minecraft:jungle_log": {
      const imageRelPaths: string[] = [];
      if (states.axis === "x") {
        imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
      }
      if (states.axis === "z") {
        imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [JUNGLE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 金合欢原木
    case "minecraft:acacia_log": {
      const imageRelPaths: string[] = [];
      if (states.axis === "x") {
        imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
      }
      if (states.axis === "z") {
        imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [ACACIA_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 深色橡木原木
    case "minecraft:dark_oak_log": {
      const imageRelPaths: string[] = [];
        if (states.axis === "x") {
          imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
        }
        if (states.axis === "z") {
          imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
        }
      return {
        mode: "replace",
        iconBlockIds: [DARK_OAK_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 樱花原木
    case "minecraft:cherry_log": {
      const imageRelPaths: string[] = [];
      if (states.axis === "x") {
        imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
      }
      if (states.axis === "z") {
        imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [CHERRY_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 红树原木
    case "minecraft:mangrove_log": {
      const imageRelPaths: string[] = [];
      if (states.axis === "x") {
        imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
      }
      if (states.axis === "z") {
        imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
      }
      return {
        mode: "replace",
        iconBlockIds: [MANGROVE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        overlayBlendMode: "subtract",
      };
    }
    // 树叶
    case "minecraft:oak_leaves":
    case "minecraft:spruce_leaves":
    case "minecraft:birch_leaves":
    case "minecraft:jungle_leaves":
    case "minecraft:acacia_leaves":
    case "minecraft:dark_oak_leaves":
    case "minecraft:cherry_leaves":
    case "minecraft:mangrove_leaves": 
    case "minecraft:azalea_leaves":
    case "minecraft:flowering_azalea_leaves":
    {
      const imageRelPaths: string[] = [];
      const overlayIconBlockIds: string[] = [];
      if (states.persistent === "false") {
        imageRelPaths.push(PERSISTENT_FALSE_BLOCK_ID);
      }
      applyWaterloggedOverlay(states, imageRelPaths, overlayIconBlockIds);
      return imageRelPaths.length || overlayIconBlockIds.length
        ? {
            mode: "mask",
            imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
            overlayIconBlockIds: overlayIconBlockIds.length ? overlayIconBlockIds : undefined,
          }
        : null;
    }
    // 发射器
    case "minecraft:dispenser": {
      const facing = states.facing;
      const triggered = states.triggered === "true";
      if (facing === "west") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DISPENSER_TOP_ON_RELPATH : DISPENSER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 1,
        };
      }
      if (facing === "south") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DISPENSER_TOP_ON_RELPATH : DISPENSER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 0,
        };
      }
      if (facing === "east") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DISPENSER_TOP_ON_RELPATH : DISPENSER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 3,
        };
      }
      if (facing === "north") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DISPENSER_TOP_ON_RELPATH : DISPENSER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 2,
        };
      }
      if (facing === "up") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DISPENSER_FRONT_VERTICAL_ON_RELPATH : DISPENSER_FRONT_VERTICAL_RELPATH],
        };
      }
      if (facing === "down") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DISPENSER_BOTTOM_ON_RELPATH : DISPENSER_BOTTOM_RELPATH],
        };
      }
      return null;
    }
    // 投掷器
    case "minecraft:dropper": {
      const facing = states.facing;
      const triggered = states.triggered === "true";
      if (facing === "west") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DROPPER_TOP_ON_RELPATH : DROPPER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 1,
        };
      }
      if (facing === "south") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DROPPER_TOP_ON_RELPATH : DROPPER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 0,
        };
      }
      if (facing === "east") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DROPPER_TOP_ON_RELPATH : DROPPER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 3,
        };
      }
      if (facing === "north") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DROPPER_TOP_ON_RELPATH : DROPPER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 2,
        };
      }
      if (facing === "up") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DROPPER_FRONT_VERTICAL_ON_RELPATH : DROPPER_FRONT_VERTICAL_RELPATH],
        };
      }
      if (facing === "down") {
        return {
          mode: "replace",
          baseImageRelPaths: [triggered ? DROPPER_BOTTOM_ON_RELPATH : DROPPER_BOTTOM_RELPATH],
        };
      }
      return null;
    }

    // 漏斗
    case "minecraft:hopper": {
      const imageRelPaths: string[] = [];
      if (states.facing === "north") {
        if (states.enabled === "true") {
          imageRelPaths.push(HOPPER_TOP_RELPATH);
          return {
          mode: "replace",
          baseImageRelPaths: [HOPPER_INSIDE_VERT_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: 0,
        };
        } else {
          imageRelPaths.push(HOPPER_TOP_ON_RELPATH);
          return {
            mode: "replace",
            baseImageRelPaths: [HOPPER_INSIDE_VERT_ON_RELPATH],
            imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
            baseImageRotateQuarterTurns: 0,
          };
        }
      }
      if (states.facing === "east") {
        if (states.enabled === "true") {
          imageRelPaths.push(HOPPER_TOP_RELPATH);
          return {
          mode: "replace",
          baseImageRelPaths: [HOPPER_INSIDE_VERT_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: 1,
        };
        } else {
          imageRelPaths.push(HOPPER_TOP_ON_RELPATH);
          return {
            mode: "replace",
            baseImageRelPaths: [HOPPER_INSIDE_VERT_ON_RELPATH],
            imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
            baseImageRotateQuarterTurns: 1,
          };
        }
      }
      if (states.facing === "south") {
        if (states.enabled === "true") {
          imageRelPaths.push(HOPPER_TOP_RELPATH);
          return {
          mode: "replace",
          baseImageRelPaths: [HOPPER_INSIDE_VERT_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: 2,
        };
        } else {
          imageRelPaths.push(HOPPER_TOP_ON_RELPATH);
          return {
            mode: "replace",
            baseImageRelPaths: [HOPPER_INSIDE_VERT_ON_RELPATH],
            imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
            baseImageRotateQuarterTurns: 2,
          };
        }
      }
      if (states.facing === "west") {
        if (states.enabled === "true") {
          imageRelPaths.push(HOPPER_TOP_RELPATH);
          return {
          mode: "replace",
          baseImageRelPaths: [HOPPER_INSIDE_VERT_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: 3,
        };
        } else {
          imageRelPaths.push(HOPPER_TOP_ON_RELPATH);
          return {
            mode: "replace",
            baseImageRelPaths: [HOPPER_INSIDE_VERT_ON_RELPATH],
            imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
            baseImageRotateQuarterTurns: 3,
          };
        }
      }
      if (states.facing === "down") {
        if (states.enabled === "true") {
          imageRelPaths.push(HOPPER_TOP_RELPATH);
          return {
          mode: "replace",
          baseImageRelPaths: [HOPPER_INSIDE_DOWN_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: 3,
        };
        } else {
          imageRelPaths.push(HOPPER_TOP_ON_RELPATH);
          return {
            mode: "replace",
            baseImageRelPaths: [HOPPER_INSIDE_DOWN_ON_RELPATH],
            imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
            baseImageRotateQuarterTurns: 3,
          };
        }
      }
    }
    case "minecraft:observer": {
      const facing = states.facing;
      if (facing === "north") {
        return {
          mode: "replace",
          baseImageRelPaths: [OBSERVER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 2,
        };
      }
      if (facing === "east") {
        return {
          mode: "replace",
          baseImageRelPaths: [OBSERVER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 3,
        };
      }
      if (facing === "south") {
        return {
          mode: "replace",
          baseImageRelPaths: [OBSERVER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 4,
        };
      }
      if (facing === "west") {
        return {
          mode: "replace",
          baseImageRelPaths: [OBSERVER_TOP_RELPATH],
          baseImageRotateQuarterTurns: 1,
        };
      }
      if (facing === "down") {
        return {
          mode: "replace",
          iconBlockIds: [OBSERVER_BACK_BLOCK_ID],
        };
      }
      return null;
    }

    // 活塞
    case "minecraft:piston": {
      const extended = states.extended === "true";
      const facing = states.facing;
      if (facing === "north") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_SIDE_ON_RELPATH : PISTON_SIDE_RELPATH],
          baseImageRotateQuarterTurns: 0,
        };
      }
      if (facing === "east") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_SIDE_ON_RELPATH : PISTON_SIDE_RELPATH],
          baseImageRotateQuarterTurns: 1,
        };
      }
      if (facing === "south") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_SIDE_ON_RELPATH : PISTON_SIDE_RELPATH],
          baseImageRotateQuarterTurns: 2,
        };
      }
      if (facing === "west") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_SIDE_ON_RELPATH : PISTON_SIDE_RELPATH],
          baseImageRotateQuarterTurns: 3,
        };
      }
      if (facing === "up") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_INNER_RELPATH : PISTON_TOP_RELPATH],
        };
      }
      if (facing === "down") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_BOTTOM_ON_RELPATH : PISTON_BOTTOM_RELPATH],
        };
      }
      return null;
    }
    // 活塞头
    case "minecraft:piston_head": {
      const facing = states.facing;
      const type = states.type;
      if (type === "normal") {
        if (facing === "north") {
          return {
            mode: "replace",
            iconBlockIds: [PISTON_HEAD_BLOCK_ID],
            baseImageRotateQuarterTurns: 0,
          };
        }
        if (facing === "east") {
          return {
            mode: "replace",
            iconBlockIds: [PISTON_HEAD_BLOCK_ID],
            baseImageRotateQuarterTurns: 1,
          };
        }
        if (facing === "south") {
          return {
            mode: "replace",
            iconBlockIds: [PISTON_HEAD_BLOCK_ID],
            baseImageRotateQuarterTurns: 2,
          };
        }
        if (facing === "west") {
          return {
            mode: "replace",
            iconBlockIds: [PISTON_HEAD_BLOCK_ID],
            baseImageRotateQuarterTurns: 3,
          };
        }
        if (facing === "up"){
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_TOP_ON_RELPATH],
          };
        }
        if (facing === "down"){
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_INNER2_RELPATH],
          };
        }
      }
      if (type === "sticky") {
        if (facing === "north") {
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_HEAD_STICKY_RELPATH],
            baseImageRotateQuarterTurns: 0,
          };
        }
        if (facing === "east") {
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_HEAD_STICKY_RELPATH],
            baseImageRotateQuarterTurns: 1,
          };
        }
        if (facing === "south") {
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_HEAD_STICKY_RELPATH],
            baseImageRotateQuarterTurns: 2,
          };
        }
        if (facing === "west") {
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_HEAD_STICKY_RELPATH],
            baseImageRotateQuarterTurns: 3,
          };
        }
        if (facing === "up"){
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_TOP_STICKY_ON_RELPATH],
          };
        }
        if (facing === "down"){
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_INNER2_STICKY_RELPATH],
          };
        }
      }
      return null;
    }
    // 黏性活塞
    case "minecraft:sticky_piston": {
      const extended = states.extended === "true";
      const facing = states.facing;
      if (facing === "north") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_SIDE_STICKY_ON_RELPATH : PISTON_SIDE_STICKY_RELPATH],
          baseImageRotateQuarterTurns: 0,
        };
      }
      if (facing === "east") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_SIDE_STICKY_ON_RELPATH : PISTON_SIDE_STICKY_RELPATH],
          baseImageRotateQuarterTurns: 1,
        };
        }
      if (facing === "south") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_SIDE_STICKY_ON_RELPATH : PISTON_SIDE_STICKY_RELPATH],
          baseImageRotateQuarterTurns: 2,
        };
      }
      if (facing === "west") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_SIDE_STICKY_ON_RELPATH : PISTON_SIDE_STICKY_RELPATH],
          baseImageRotateQuarterTurns: 3,
        };
      }
      if (facing === "up") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_INNER_STICKY_RELPATH : PISTON_TOP_STICKY_RELPATH],
        };
      }
      if (facing === "down") {
        return {
          mode: "replace",
          baseImageRelPaths: [extended ? PISTON_BOTTOM_STICKY_ON_RELPATH : PISTON_BOTTOM_STICKY_RELPATH],
        };
      }
      return null;
    }
    // 红石中继器
    case "minecraft:repeater": {
      const imageRelPaths: string[] = [];
      const delay = states.delay;
      const facing = states.facing;
      const powered = states.powered;
      const locked = states.locked;
      let rotateQuarterTurns = 0;
      if (locked === "true") {
        imageRelPaths.push(REPEATOR_LOCK_RELPATH);
      }
      if (facing === "north") {
        rotateQuarterTurns = 2;
      }
      if (facing === "east") {
        rotateQuarterTurns = 3;
      }
      if (facing === "south") {
        rotateQuarterTurns = 0;
      }
      if (facing === "west") {
        rotateQuarterTurns = 1;
      }
      if (delay === "1") {
        return {
          mode: "replace",
          baseImageRelPaths: [powered === "true" ? REPEATOR_ON1_RELPATH : REPEATOR1_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: rotateQuarterTurns,
        };
      }
      if (delay === "2") {
        return {
          mode: "replace",
          baseImageRelPaths: [powered === "true" ? REPEATOR_ON2_RELPATH : REPEATOR2_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: rotateQuarterTurns,
        };
      }
      if (delay === "3") {
        return {
          mode: "replace",
          baseImageRelPaths: [powered === "true" ? REPEATOR_ON3_RELPATH : REPEATOR3_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: rotateQuarterTurns,
        };
      }
      if (delay === "4") {
        return {
          mode: "replace",
          baseImageRelPaths: [powered === "true" ? REPEATOR_ON4_RELPATH : REPEATOR4_RELPATH],
          imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
          baseImageRotateQuarterTurns: rotateQuarterTurns,
        };
      }
      return null;
    }
    // 红石比较器
    case "minecraft:comparator": {
      const mode = states.mode;
      const facing = states.facing;
      const powered = states.powered;
      let rotateQuarterTurns = 0;
      if (facing === "north") {
        rotateQuarterTurns = 2;
      }
      if (facing === "east") {
        rotateQuarterTurns = 3;
      }
      if (facing === "south") {
        rotateQuarterTurns = 0;
      }
      if (facing === "west") {
        rotateQuarterTurns = 1;
      }
      if (mode === "subtract") {
        return {
          mode: "replace",
          baseImageRelPaths: [powered === "true" ? COMPARATOR2_ON_RELPATH : COMPARATOR2_RELPATH],
          baseImageRotateQuarterTurns: rotateQuarterTurns,
        };
      }
      if (mode === "compare") {
        return {
          mode: "replace",
          baseImageRelPaths: [powered === "true" ? COMPARATOR_ON_RELPATH : COMPARATOR_RELPATH],
          baseImageRotateQuarterTurns: rotateQuarterTurns,
        };
      }
      return null;
    }
    // 红石线由专用合成流程处理
    case "minecraft:redstone_wire":
      return null;
    // 墙上的火把
    case "minecraft:wall_torch": {
      const imageRelPaths: string[] = [];
      const facing = states.facing;
      let rotateQuarterTurns = 0;
      if (facing === "north") {
        rotateQuarterTurns = 0;
      }
      if (facing === "east") {
        rotateQuarterTurns = 1;
      }
      if (facing === "south") {
        rotateQuarterTurns = 2;
      }
      if (facing === "west") {
        rotateQuarterTurns = 3;
      }
      return {
        mode: "replace",
        iconBlockIds: [WALL_TORCH_BLOCK_ID],
        baseImageRotateQuarterTurns: rotateQuarterTurns,
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      }
    }
    case "minecraft:soul_wall_torch": {
      const imageRelPaths: string[] = [];
      const facing = states.facing;
      let rotateQuarterTurns = 0;
      if (facing === "north") {
        rotateQuarterTurns = 0;
      }
      if (facing === "east") {
        rotateQuarterTurns = 1;
      }
      if (facing === "south") {
        rotateQuarterTurns = 2;
      }
      if (facing === "west") {
        rotateQuarterTurns = 3;
      }
      return {
        mode: "replace",
        iconBlockIds: [SOUL_WALL_TORCH_BLOCK_ID],
        baseImageRotateQuarterTurns: rotateQuarterTurns,
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      }
    }
    case "minecraft:copper_wall_torch": {
      const imageRelPaths: string[] = [];
      const facing = states.facing;
      let rotateQuarterTurns = 0;
      if (facing === "north") {
        rotateQuarterTurns = 0;
      }
      if (facing === "east") {
        rotateQuarterTurns = 1;
      }
      if (facing === "south") {
        rotateQuarterTurns = 2;
      }
      if (facing === "west") {
        rotateQuarterTurns = 3;
      }
      return {
        mode: "replace",
        iconBlockIds: [COPPER_WALL_TORCH_BLOCK_ID],
        baseImageRotateQuarterTurns: rotateQuarterTurns,
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      }
    }
    case "minecraft:redstone_wall_torch": {
      const imageRelPaths: string[] = [];
      const facing = states.facing;
      const lit = states.lit;
      let rotateQuarterTurns = 0;
      if (facing === "north") {
        rotateQuarterTurns = 0;
      }
      if (facing === "east") {
        rotateQuarterTurns = 1;
      }
      if (facing === "south") {
        rotateQuarterTurns = 2;
      }
      if (facing === "west") {
        rotateQuarterTurns = 3;
      }
      return {
        mode: "replace",
        iconBlockIds: lit === "true" ? [REDSTONE_WALL_TORCH_BLOCK_ID] : [REDSTONE_WALL_TORCH_OFF_BLOCK_ID],
        baseImageRotateQuarterTurns: rotateQuarterTurns,
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      }
    }

    // 默认
    default: {
      // 枚举器匹配：含水方块
      const normalizedId = normalizeBlockId(blockId);
      if (await isBlockInEnumerator(normalizedId, "enumerator/system_enum/state_waterlogged.json")) {
        const imageRelPaths: string[] = [];
        const overlayIconBlockIds: string[] = [];
        applyWaterloggedOverlay(states, imageRelPaths, overlayIconBlockIds);
        if (imageRelPaths.length > 0 || overlayIconBlockIds.length > 0) {
          return {
            mode: "mask",
            imageRelPaths: imageRelPaths.length > 0 ? imageRelPaths : undefined,
            overlayIconBlockIds: overlayIconBlockIds.length > 0 ? overlayIconBlockIds : undefined,
          };
        }
      }
      return null;
    }
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

async function tintImageDataUrl(url: string, colorHex: string): Promise<string | null> {
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

async function composeImageDataUrls(layerUrls: string[]): Promise<string | null> {
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

async function resolveRedstoneWireStateHintImage(states: Record<string, string>): Promise<string | null> {
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