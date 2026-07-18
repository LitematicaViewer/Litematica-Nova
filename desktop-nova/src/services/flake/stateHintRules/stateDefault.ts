import { FlakeStateHintRule, normalizeBlockId } from "../../flakeStateHintResolver";
import { isBlockInEnumerator } from "../enumeratorLoader";
import { applyWaterloggedOverlay } from "./stateWaterlogged";


// 遮罩
export const STAGE_1_HINT_RELPATH = "data/flake/state_hint/stage_1.png";
export const HANGING_TRUE_HINT_RELPATH = "data/flake/state_hint/hanging_true.png";
export const SNOWY_TRUE_HINT_RELPATH = "data/flake/state_hint/snowy_true.png";
export const PERSISTENT_FALSE_BLOCK_ID = "data/flake/state_hint/persistent_false.png";
// 反色遮罩
export const LEVEL_0_OVERLAY_REPATH = "data/flake/state_overlay/level_0.png";
export const AXIS_X_OVERLAY_REPATH = "data/flake/state_overlay/axis_x.png";
export const AXIS_Z_OVERLAY_REPATH = "data/flake/state_overlay/axis_z.png";
// 替换
// 草皮
export const GRASS_BLOCK_TOP_BLOCK_ID = "minecraft:grass_block_top";
// const GRASS_PATH_TOP_BLOCK_ID = "minecraft:grass_path_top";
export const DIRT_PATH_TOP_BLOCK_ID = "minecraft:dirt_path_top";
export const PODZOL_TOP_BLOCK_ID = "minecraft:podzol_top";
export const MYCELIUM_TOP_BLOCK_ID = "minecraft:mycelium_top";
//液体
export const WATER_TOP_BLOCK_ID = "minecraft:water_top";
export const LAVA_TOP_BLOCK_ID = "minecraft:lava_top";
// 原木
export const OAK_LOG_TOP_BLOCK_ID = "minecraft:oak_log_top";
export const SPRUCE_LOG_TOP_BLOCK_ID = "minecraft:spruce_log_top";
export const BIRCH_LOG_TOP_BLOCK_ID = "minecraft:birch_log_top";
export const JUNGLE_LOG_TOP_BLOCK_ID = "minecraft:jungle_log_top";
export const ACACIA_LOG_TOP_BLOCK_ID = "minecraft:acacia_log_top";
export const DARK_OAK_LOG_TOP_BLOCK_ID = "minecraft:dark_oak_log_top";
export const CHERRY_LOG_TOP_BLOCK_ID = "minecraft:cherry_log_top";
export const MANGROVE_LOG_TOP_BLOCK_ID = "minecraft:mangrove_log_top";
export const BAMBOO_BLOCK_TOP_BLOCK_ID = "minecraft:bamboo_block_top"
// 去皮标记
export const STRIPPED_X_RELPATH = "data/flake/state_hint/stripped_x.png";
// 发射器
export const DISPENSER_TOP_RELPATH = "data/flake/redstone_display/dispenser_top.png";
export const DISPENSER_TOP_ON_RELPATH = "data/flake/redstone_display/dispenser_top_on.png";
export const DISPENSER_FRONT_VERTICAL_RELPATH = "data/flake/redstone_display/dispenser_front_vertical.png";
export const DISPENSER_FRONT_VERTICAL_ON_RELPATH = "data/flake/redstone_display/dispenser_front_vertical_on.png";
export const DISPENSER_BOTTOM_RELPATH = "data/flake/redstone_display/dispenser_bottom.png";
export const DISPENSER_BOTTOM_ON_RELPATH = "data/flake/redstone_display/dispenser_bottom_on.png";
// 投掷器
export const DROPPER_TOP_RELPATH = "data/flake/redstone_display/dropper_top.png";
export const DROPPER_TOP_ON_RELPATH = "data/flake/redstone_display/dropper_top_on.png";
export const DROPPER_FRONT_VERTICAL_RELPATH = "data/flake/redstone_display/dropper_front_vertical.png";
export const DROPPER_FRONT_VERTICAL_ON_RELPATH = "data/flake/redstone_display/dropper_front_vertical_on.png";
export const DROPPER_BOTTOM_RELPATH = "data/flake/redstone_display/dropper_bottom.png";
export const DROPPER_BOTTOM_ON_RELPATH = "data/flake/redstone_display/dropper_bottom_on.png";
// 漏斗
export const HOPPER_INSIDE_DOWN_RELPATH = "data/flake/redstone_display/hopper_inside.png";
export const HOPPER_INSIDE_DOWN_ON_RELPATH = "data/flake/redstone_display/hopper_inside_on.png";
export const HOPPER_INSIDE_VERT_RELPATH = "data/flake/redstone_display/hopper_inside2.png";
export const HOPPER_INSIDE_VERT_ON_RELPATH = "data/flake/redstone_display/hopper_inside2_on.png";
export const HOPPER_TOP_RELPATH = "data/flake/redstone_display/hopper_top.png";
export const HOPPER_TOP_ON_RELPATH = "data/flake/redstone_display/hopper_top_on.png";
// 侦测器
const OBSERVER_BLOCK_ID = "minecraft:observer";
export const OBSERVER_BACK_BLOCK_ID = "minecraft:observer_back";
export const OBSERVER_TOP_RELPATH = "data/flake/redstone_display/observer_top.png";
// 活塞
export const PISTON_TOP_RELPATH = "data/flake/redstone_display/piston_top.png";
export const PISTON_TOP_ON_RELPATH = "data/flake/redstone_display/piston_top_on.png";
export const PISTON_BOTTOM_RELPATH = "data/flake/redstone_display/piston_bottom.png";
export const PISTON_BOTTOM_ON_RELPATH = "data/flake/redstone_display/piston_bottom_on.png";
export const PISTON_SIDE_RELPATH = "data/flake/redstone_display/piston_side.png";
export const PISTON_SIDE_ON_RELPATH = "data/flake/redstone_display/piston_side_on.png";
export const PISTON_INNER_RELPATH = "data/flake/redstone_display/piston_inner.png";
export const PISTON_INNER2_RELPATH = "data/flake/redstone_display/piston_inner2.png";
export const PISTON_HEAD_BLOCK_ID = "minecraft:piston_head";
// 黏性活塞
export const PISTON_TOP_STICKY_RELPATH = "data/flake/redstone_display/piston_top_sticky.png";
export const PISTON_TOP_STICKY_ON_RELPATH = "data/flake/redstone_display/piston_top_sticky_on.png";
export const PISTON_BOTTOM_STICKY_RELPATH = "data/flake/redstone_display/piston_bottom_sticky.png";
export const PISTON_BOTTOM_STICKY_ON_RELPATH = "data/flake/redstone_display/piston_bottom_sticky_on.png";
export const PISTON_SIDE_STICKY_RELPATH = "data/flake/redstone_display/piston_side_sticky.png";
export const PISTON_SIDE_STICKY_ON_RELPATH = "data/flake/redstone_display/piston_side_sticky_on.png";
export const PISTON_INNER_STICKY_RELPATH = "data/flake/redstone_display/piston_inner_sticky.png";
export const PISTON_HEAD_STICKY_RELPATH = "data/flake/redstone_display/piston_head_sticky.png";
export const PISTON_INNER2_STICKY_RELPATH = "data/flake/redstone_display/piston_inner2_sticky.png";
// 红石中继器
export const REPEATOR1_RELPATH = "data/flake/redstone_display/repeater1.png";
export const REPEATOR_ON1_RELPATH = "data/flake/redstone_display/repeater_on1.png";
export const REPEATOR2_RELPATH = "data/flake/redstone_display/repeater2.png";
export const REPEATOR_ON2_RELPATH = "data/flake/redstone_display/repeater_on2.png";
export const REPEATOR3_RELPATH = "data/flake/redstone_display/repeater3.png";
export const REPEATOR_ON3_RELPATH = "data/flake/redstone_display/repeater_on3.png";
export const REPEATOR4_RELPATH = "data/flake/redstone_display/repeater4.png";
export const REPEATOR_ON4_RELPATH = "data/flake/redstone_display/repeater_on4.png";
export const REPEATOR_LOCK_RELPATH = "data/flake/redstone_display/repeater_lock.png";
// 红石比较器
export const COMPARATOR_RELPATH = "data/flake/redstone_display/comparator.png";
export const COMPARATOR_ON_RELPATH = "data/flake/redstone_display/comparator_on.png";
export const COMPARATOR2_RELPATH = "data/flake/redstone_display/comparator2.png";
export const COMPARATOR2_ON_RELPATH = "data/flake/redstone_display/comparator2_on.png";
// 红石线
export const REDSTONE_WIRE_BASE_RELPATH = "data/flake/redstone_display/redstone_wire_base.png";
const REDSTONE_WIRE_NORTH_RELPATH = "data/flake/redstone_display/redstone_wire_north.png";
const REDSTONE_WIRE_EAST_RELPATH = "data/flake/redstone_display/redstone_wire_east.png";
const REDSTONE_WIRE_SOUTH_RELPATH = "data/flake/redstone_display/redstone_wire_south.png";
const REDSTONE_WIRE_WEST_RELPATH = "data/flake/redstone_display/redstone_wire_west.png";
const REDSTONE_WIRE_NORTH_UP_RELPATH = "data/flake/redstone_display/redstone_wire_north_up.png";
const REDSTONE_WIRE_EAST_UP_RELPATH = "data/flake/redstone_display/redstone_wire_east_up.png";
const REDSTONE_WIRE_SOUTH_UP_RELPATH = "data/flake/redstone_display/redstone_wire_south_up.png";
const REDSTONE_WIRE_WEST_UP_RELPATH = "data/flake/redstone_display/redstone_wire_west_up.png";
export const REDSTONE_WIRE_SIDE_RELPATHS: Record<string, string> = {
  north: REDSTONE_WIRE_NORTH_RELPATH,
  east: REDSTONE_WIRE_EAST_RELPATH,
  south: REDSTONE_WIRE_SOUTH_RELPATH,
  west: REDSTONE_WIRE_WEST_RELPATH,
};
export const REDSTONE_WIRE_UP_RELPATHS: Record<string, string> = {
  north: REDSTONE_WIRE_NORTH_UP_RELPATH,
  east: REDSTONE_WIRE_EAST_UP_RELPATH,
  south: REDSTONE_WIRE_SOUTH_UP_RELPATH,
  west: REDSTONE_WIRE_WEST_UP_RELPATH,
};
export const REDSTONE_WIRE_DIRECTIONS = ["north", "east", "south", "west"] as const;
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
export const REDSTONE_WIRE_NUMBER_RELPATHS: Record<string, string> = {
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
export const REDSTONE_WIRE_POWER_COLORS: Record<string, string> = {
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
export const WALL_TORCH_BLOCK_ID = "minecraft:wall_torch";
export const SOUL_WALL_TORCH_BLOCK_ID = "minecraft:soul_wall_torch";
export const COPPER_WALL_TORCH_BLOCK_ID = "minecraft:copper_wall_torch";
export const REDSTONE_WALL_TORCH_BLOCK_ID = "minecraft:redstone_wall_torch";
export const REDSTONE_WALL_TORCH_OFF_BLOCK_ID = "minecraft:redstone_wall_torch_off";


/** 图标解析规则 */
export async function resolveManualStateHintRule(blockId: string, states: Record<string, string>): Promise<FlakeStateHintRule | null> {
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
    case "minecraft:mycelium": {
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
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
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
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
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
    // 去皮木头
    case "minecraft:stripped_oak_wood":
    case "minecraft:stripped_spruce_wood":
    case "minecraft:stripped_birch_wood":
    case "minecraft:stripped_jungle_wood":
    case "minecraft:stripped_acacia_wood":
    case "minecraft:stripped_dark_oak_wood":
    case "minecraft:stripped_cherry_wood":
    case "minecraft:stripped_mangrove_wood":
    // 红树根
    case "minecraft:muddy_mangrove_roots":
      {
        const imageRelPaths: string[] = [];
        if (states.axis === "x") {
          imageRelPaths.push(AXIS_X_OVERLAY_REPATH);
        }
        if (states.axis === "z") {
          imageRelPaths.push(AXIS_Z_OVERLAY_REPATH);
        }
        return imageRelPaths.length
          ? { mode: "mask", subtractImageRelPaths: imageRelPaths }
          : null;
      }
    // 橡木原木
    case "minecraft:oak_log":
    case "minecraft:stripped_oak_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_oak_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [OAK_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    // 云杉原木
    case "minecraft:spruce_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_spruce_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [SPRUCE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 白桦原木
    case "minecraft:birch_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_birch_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [BIRCH_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 丛林原木
    case "minecraft:jungle_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_jungle_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [JUNGLE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 金合欢原木
    case "minecraft:acacia_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_acacia_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [ACACIA_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 深色橡木原木
    case "minecraft:dark_oak_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_dark_oak_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [DARK_OAK_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 樱花原木
    case "minecraft:cherry_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_cherry_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [CHERRY_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 红树原木
    case "minecraft:mangrove_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_mangrove_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [MANGROVE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
    // 竹块
    case "minecraft:bamboo_block":{
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_REPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_REPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_bamboo_block") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [BAMBOO_BLOCK_TOP_BLOCK_ID],
        subtractImageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
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
        if (facing === "up") {
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_TOP_ON_RELPATH],
          };
        }
        if (facing === "down") {
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
        if (facing === "up") {
          return {
            mode: "replace",
            baseImageRelPaths: [PISTON_TOP_STICKY_ON_RELPATH],
          };
        }
        if (facing === "down") {
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
    // 火把
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
      };
    }
    case "minecraft:torch": {
      return {
        mode: "replace",
        baseImageRelPaths: [TORCH_HEAD_RELPATH]
      }
    }
    // 灵魂火把
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
      };
    }
    case "minecraft:soul_torch": {
      return {
        mode: "replace",
        baseImageRelPaths: [SOUL_TORCH_HEAD_RELPATH]
      }
    }
    // 铜火把
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
      };
    }
    case "minecraft:copper_torch": {
      return {
        mode: "replace",
        baseImageRelPaths: [COPPER_TORCH_HEAD_RELPATH]
      }
    }
    // 红石火把
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
      };
    }
    case "minecraft:redstone_torch": {
      const imageRelPaths: string[] = [];
      const lit = states.lit;
      return {
        mode: "replace",
        baseImageRelPaths: lit === "true" ? [REDSTONE_TORCH_HEAD_RELPATH] : [REDSTONE_TORCH_HEAD_OFF_RELPATH],
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



