import { FlakeStateHintRule, normalizeBlockId } from "../../flakeStateHintResolver";
import { isBlockInEnumerator } from "../enumeratorLoader";
import { applyWaterloggedOverlay } from "./stateWaterlogged";


// 遮罩
export const STAGE_1_HINT_RELPATH = "data/flake/state_hint/stage_1.png";
export const HANGING_TRUE_HINT_RELPATH = "data/flake/state_hint/hanging_true.png";
export const SNOWY_TRUE_HINT_RELPATH = "data/flake/state_hint/snowy_true.png";
export const PERSISTENT_FALSE_BLOCK_ID = "data/flake/state_hint/persistent_false.png";
// 反色遮罩
export const LEVEL_0_OVERLAY_RELPATH = "data/flake/state_overlay/level_0.png";
export const OCCUPIED_TRUE_RELPATH = "data/flake/state_overlay/occupied_true.png";
export const AXIS_X_OVERLAY_RELPATH = "data/flake/state_overlay/axis_x.png";
export const AXIS_Z_OVERLAY_RELPATH = "data/flake/state_overlay/axis_z.png";
export const FACING_NORTH_OVERLAY_RELPATH = "data/flake/state_overlay/facing_north.png";
export const FACING_SOUTH_OVERLAY_RELPATH = "data/flake/state_overlay/facing_south.png";
export const FACING_WEST_OVERLAY_RELPATH = "data/flake/state_overlay/facing_west.png";
export const FACING_EAST_OVERLAY_RELPATH = "data/flake/state_overlay/facing_east.png";
export const FACING_NORTH_OVERLAY_RELPATH_2 = "data/flake/state_overlay/facing_north_2.png";
export const FACING_SOUTH_OVERLAY_RELPATH_2 = "data/flake/state_overlay/facing_south_2.png";
export const FACING_WEST_OVERLAY_RELPATH_2 = "data/flake/state_overlay/facing_west_2.png";
export const FACING_EAST_OVERLAY_RELPATH_2 = "data/flake/state_overlay/facing_east_2.png";
// 替换
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
export const STRIPPED_BAMBOO_BLOCK_TOP_BLOCK_ID = "minecraft:stripped_bamboo_block_top"
// 去皮标记
export const STRIPPED_X_RELPATH = "data/flake/state_hint/stripped_x.png";
// 雕纹书架
export const CHISELED_BOOKSHELF_EMPTY_BLOCK_ID = "minecraft:chiseled_bookshelf_0";
export const SLOT_0_OCCUPIED_TRUE_RELPATH = "data/flake/state_hint/slot_0_occupied_true.png";
export const SLOT_1_OCCUPIED_TRUE_RELPATH = "data/flake/state_hint/slot_1_occupied_true.png";
export const SLOT_2_OCCUPIED_TRUE_RELPATH = "data/flake/state_hint/slot_2_occupied_true.png";
export const SLOT_3_OCCUPIED_TRUE_RELPATH = "data/flake/state_hint/slot_3_occupied_true.png";
export const SLOT_4_OCCUPIED_TRUE_RELPATH = "data/flake/state_hint/slot_4_occupied_true.png";
export const SLOT_5_OCCUPIED_TRUE_RELPATH = "data/flake/state_hint/slot_5_occupied_true.png";
// 拉杆
export const LEVER_GORUND_RELPATH = "data/flake/redstone_display/lever_ground.png"
const LEVER_GORUND_ON_RELPATH = "data/flake/redstone_display/lever_ground_on.png"
const LEVER_WALL_RELPATH = "data/flake/redstone_display/lever_wall.png"
const LEVER_WALL_ON_RELPATH = "data/flake/redstone_display/lever_wall_on.png"
const LEVER_CEILING_RELPATH = "data/flake/redstone_display/lever_ceiling.png"
const LEVER_CEILING_ON_RELPATH = "data/flake/redstone_display/lever_ceiling_on.png"
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

// 16 种染料颜色，顺序与 Minecraft 调试模式一致。后续床、地毯、羊毛等
// 彩色方块都可以复用这份列表并通过字符串拼接触发贴图名。
export const DYE_COLORS = [
  "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray",
  "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
] as const;

// 床：颜色 + part 拼接贴图名，避免为每种颜色复制一份规则。
const BED_BLOCK_IDS = new Set<string>(DYE_COLORS.map((color) => `minecraft:${color}_bed`));
// block_2d 的床顶贴图默认朝向 east；canvas 正角度为顺时针，因此
// east=0°、south=90°、west=180°、north=270°。
const BED_FACING_QUARTER_TURNS: Record<string, number> = {
  east: 0,
  south: 1,
  west: 2,
  north: 3,
};

function resolveBedStateHintRule(blockId: string, states: Record<string, string>): FlakeStateHintRule | null {
  const id = normalizeBlockId(blockId);
  if (!BED_BLOCK_IDS.has(id)) return null;
  const part = states.part;
  if (part !== "head" && part !== "foot") return null;
  const color = id.slice("minecraft:".length, -"_bed".length);
  return {
    mode: "replace",
    iconBlockIds: [`minecraft:${color}_bed_top_${part}`],
    baseImageRotateQuarterTurns: BED_FACING_QUARTER_TURNS[states.facing || ""] ?? 0,
    subtractImageRelPaths: states.occupied === "true" ? [OCCUPIED_TRUE_RELPATH] : undefined,
    // 没有 *_bed_top_* 时不把 occupied 遮罩盖到通用床图标上。
    requiresBase: true,
  };
}

// 楼梯：遮罩默认朝向 north，facing 只旋转/镜像这一层遮罩。
const STAIRS_HALF_BOTTOM_SHAPE_STRAIGHT_RELPATH = "data/flake/state_hint/stairs_half_bottom_shape_straight.png";
const STAIRS_HALF_BOTTOM_SHAPE_INNER_LEFT_RELPATH = "data/flake/state_hint/stairs_half_bottom_shape_inner_left.png";
const STAIRS_HALF_BOTTOM_SHAPE_OUTER_LEFT_RELPATH = "data/flake/state_hint/stairs_half_bottom_shape_outer_left.png";
// half=top：黑=原色，灰=半透明，白=全透明；作为 alpha 层削减底图透明度。
const STAIRS_HALF_TOP_SHAPE_STRAIGHT_RELPATH = "data/flake/state_overlay/stairs_half_top_shape_straight.png";
const STAIRS_HALF_TOP_SHAPE_INNER_LEFT_RELPATH = "data/flake/state_overlay/stairs_half_top_shape_inner_left.png";
const STAIRS_HALF_TOP_SHAPE_OUTER_LEFT_RELPATH = "data/flake/state_overlay/stairs_half_top_shape_outer_left.png";
// facing=north 时遮罩不旋转；canvas 正角度为顺时针。
const STAIRS_FACING_QUARTER_TURNS: Record<string, number> = {
  north: 0,
  east: 1,
  south: 2,
  west: 3,
};
// right 系列复用 left 贴图并水平镜像（镜像先于旋转，由 imageLayers 保证）。
const STAIRS_SHAPE_MASKS: Record<string, { imageRelPath: string; flipHorizontal?: boolean }> = {
  straight: { imageRelPath: STAIRS_HALF_BOTTOM_SHAPE_STRAIGHT_RELPATH },
  inner_left: { imageRelPath: STAIRS_HALF_BOTTOM_SHAPE_INNER_LEFT_RELPATH },
  inner_right: { imageRelPath: STAIRS_HALF_BOTTOM_SHAPE_INNER_LEFT_RELPATH, flipHorizontal: true },
  outer_left: { imageRelPath: STAIRS_HALF_BOTTOM_SHAPE_OUTER_LEFT_RELPATH },
  outer_right: { imageRelPath: STAIRS_HALF_BOTTOM_SHAPE_OUTER_LEFT_RELPATH, flipHorizontal: true },
};
const STAIRS_HALF_TOP_SHAPE_MASKS: Record<string, { imageRelPath: string; flipHorizontal?: boolean }> = {
  straight: { imageRelPath: STAIRS_HALF_TOP_SHAPE_STRAIGHT_RELPATH },
  inner_left: { imageRelPath: STAIRS_HALF_TOP_SHAPE_INNER_LEFT_RELPATH },
  inner_right: { imageRelPath: STAIRS_HALF_TOP_SHAPE_INNER_LEFT_RELPATH, flipHorizontal: true },
  outer_left: { imageRelPath: STAIRS_HALF_TOP_SHAPE_OUTER_LEFT_RELPATH },
  outer_right: { imageRelPath: STAIRS_HALF_TOP_SHAPE_OUTER_LEFT_RELPATH, flipHorizontal: true },
};

/**
 * 楼梯母方块例外表（手动维护区）。
 *
 * 一般规则：去掉 `_stairs` 后缀，例如 cobblestone_stairs -> cobblestone。
 * 若母方块贴图名与去后缀结果不同，在此补一条：
 *   key   = 楼梯 ID（不含 minecraft:）
 *   value = block_2d 图标名（不含 .png）
 *
 * 待补参考（当前 2D 包中“去掉 _stairs 后没有同名图标”的楼梯）：
 *   暂无
 */
const STAIRS_BASE_BLOCK_OVERRIDES: Record<string, string> = {
  oak_stairs: "oak_planks",
  acacia_stairs: "acacia_planks",
  birch_stairs: "birch_planks",
  dark_oak_stairs: "dark_oak_planks",
  cherry_stairs: "cherry_planks",
  jungle_stairs: "jungle_planks",
  pale_oak_stairs: "pale_oak_planks",
  spruce_stairs: "spruce_planks",
  mangrove_stairs: "mangrove_planks",
  crimson_stairs: "crimson_planks",
  warped_stairs: "warped_planks",
  poplar_stairs: "poplar_planks",

  brick_stairs: "bricks",
  deepslate_brick_stairs: "deepslate_bricks",
  deepslate_tile_stairs: "deepslate_tiles",
  cinnabar_brick_stairs: "cinnabar_bricks",
  end_stone_brick_stairs: "end_stone_bricks",
  mossy_stone_brick_stairs: "mossy_stone_bricks",
  mud_brick_stairs: "mud_bricks",
  nether_brick_stairs: "nether_bricks",
  polished_blackstone_brick_stairs: "polished_blackstone_bricks",
  prismarine_brick_stairs: "prismarine_bricks",
  resin_brick_stairs: "resin_bricks",
  stone_brick_stairs: "stone_bricks",
  sulfur_brick_stairs: "sulfur_bricks",
  tuff_brick_stairs: "tuff_bricks",

  purpur_stairs: "purpur_block",
  quartz_stairs: "quartz_block",
};

async function resolveStairsStateHintRule(blockId: string, states: Record<string, string>): Promise<FlakeStateHintRule | null> {
  const id = normalizeBlockId(blockId);
  const half = states.half;
  if (!id.endsWith("_stairs") || (half !== "bottom" && half !== "top")) return null;
  if (!(await isBlockInEnumerator(id, "enumerator/system_enum/blockset_stairs.json"))) return null;

  const isTop = half === "top";
  const shapeMask = (isTop ? STAIRS_HALF_TOP_SHAPE_MASKS : STAIRS_SHAPE_MASKS)[states.shape || ""];
  if (!shapeMask) return null;

  const stairKey = id.slice("minecraft:".length);
  const baseName = STAIRS_BASE_BLOCK_OVERRIDES[stairKey] ?? stairKey.slice(0, -"_stairs".length);
  if (!baseName) return null;

  const imageRelPaths: string[] = [];
  const overlayIconBlockIds: string[] = [];
  // 楼梯同时属于 state_waterlogged：把含水遮罩追加到普通遮罩里；
  // 形状遮罩单独放进 imageLayers，旋转/镜像不会影响含水遮罩。
  applyWaterloggedOverlay(states, imageRelPaths, overlayIconBlockIds);

  return {
    mode: "replace",
    iconBlockIds: [`minecraft:${baseName}`],
    imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
    overlayIconBlockIds: overlayIconBlockIds.length ? overlayIconBlockIds : undefined,
    imageLayers: [{
      imageRelPath: shapeMask.imageRelPath,
      flipHorizontal: shapeMask.flipHorizontal,
      rotateQuarterTurns: STAIRS_FACING_QUARTER_TURNS[states.facing || ""] ?? 0,
      // bottom 用黑色半透明遮罩压出形状；top 用灰度图直接削减 alpha。
      effect: isTop ? "alpha" : "overlay",
    }],
  };
}

/** 图标解析规则 */
export async function resolveManualStateHintRule(blockId: string, states: Record<string, string>): Promise<FlakeStateHintRule | null> {
  // 彩色方块在 switch 前统一解析，避免为 16 种颜色重复规则体。
  const bedRule = resolveBedStateHintRule(blockId, states);
  if (bedRule) return bedRule;
  const stairsRule = await resolveStairsStateHintRule(blockId, states);
  if (stairsRule) return stairsRule;
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
        imageRelPaths.push(LEVEL_0_OVERLAY_RELPATH);
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
        imageRelPaths.push(LEVEL_0_OVERLAY_RELPATH);
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
          imageRelPaths.push(AXIS_X_OVERLAY_RELPATH);
        }
        if (states.axis === "z") {
          imageRelPaths.push(AXIS_Z_OVERLAY_RELPATH);
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
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
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
    case "minecraft:spruce_log":
    case "minecraft:stripped_spruce_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_spruce_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [SPRUCE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    // 白桦原木
    case "minecraft:birch_log":
    case "minecraft:stripped_birch_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_birch_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [BIRCH_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    // 丛林原木
    case "minecraft:jungle_log":
    case "minecraft:stripped_jungle_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_jungle_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [JUNGLE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    // 金合欢原木
    case "minecraft:acacia_log":
    case "minecraft:stripped_acacia_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_acacia_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [ACACIA_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    // 深色橡木原木
    case "minecraft:dark_oak_log":
    case "minecraft:stripped_dark_oak_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_dark_oak_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [DARK_OAK_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    // 樱花原木
    case "minecraft:cherry_log":
    case "minecraft:stripped_cherry_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_cherry_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [CHERRY_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    // 红树原木
    case "minecraft:mangrove_log":
    case "minecraft:stripped_mangrove_log": {
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      if (normalizeBlockId(blockId) === "minecraft:stripped_mangrove_log") {
        imageRelPaths.push(STRIPPED_X_RELPATH); // 去皮标记
      }
      return {
        mode: "replace",
        iconBlockIds: [MANGROVE_LOG_TOP_BLOCK_ID],
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    // 竹块
    case "minecraft:bamboo_block":{
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      return {
        mode: "replace",
        iconBlockIds: [BAMBOO_BLOCK_TOP_BLOCK_ID],
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
      };
    }
    //去皮竹块
    case "minecraft:stripped_bamboo_block":{
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      if (states.axis === "x") {
        subtractImageRelPaths.push(AXIS_X_OVERLAY_RELPATH); // 朝向标记
      }
      if (states.axis === "z") {
        subtractImageRelPaths.push(AXIS_Z_OVERLAY_RELPATH); // 朝向标记
      }
      return {
        mode: "replace",
        iconBlockIds: [STRIPPED_BAMBOO_BLOCK_TOP_BLOCK_ID],
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
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
    // 拉杆
    case "minecraft:lever":{
      const facing = states.facing;
      const face = states.face;
      const powered = states.powered;
      let baseImageRelPath = "";
      let rotateQuarterTurns = 0;
      switch(face) {
        case "floor":
          { baseImageRelPath = powered === "true" ? LEVER_GORUND_ON_RELPATH : LEVER_GORUND_RELPATH; break; }
        case "wall":
          { baseImageRelPath = powered === "true" ? LEVER_WALL_ON_RELPATH : LEVER_WALL_RELPATH; break; }
        case "ceiling":
          { baseImageRelPath = powered === "true" ? LEVER_CEILING_ON_RELPATH : LEVER_CEILING_RELPATH; break; }
      }
      switch(facing) {
        case "north":
          { rotateQuarterTurns = 0; break; }
        case "east":
          { rotateQuarterTurns = 1; break; }
        case "south":
          { rotateQuarterTurns = 2; break; }
        case "west":
          { rotateQuarterTurns = 3; break; }
      }
      return {
        mode: "replace",
        baseImageRelPaths: [baseImageRelPath],
        baseImageRotateQuarterTurns: rotateQuarterTurns,
      };
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
    // 雕纹书架
    case "minecraft:chiseled_bookshelf":{
      const imageRelPaths: string[] = [];
      const subtractImageRelPaths: string[] = [];
      const facing = states.facing;
      const s0 = states.slot_0_occupied;
      const s1 = states.slot_1_occupied;
      const s2 = states.slot_2_occupied;
      const s3 = states.slot_3_occupied;
      const s4 = states.slot_4_occupied;
      const s5 = states.slot_5_occupied;
      if(s0 === "true"){
        imageRelPaths.push(SLOT_0_OCCUPIED_TRUE_RELPATH);
      }
      if(s1 === "true"){
        imageRelPaths.push(SLOT_1_OCCUPIED_TRUE_RELPATH);
      }
      if(s2 === "true"){
        imageRelPaths.push(SLOT_2_OCCUPIED_TRUE_RELPATH);
      }
      if(s3 === "true"){
        imageRelPaths.push(SLOT_3_OCCUPIED_TRUE_RELPATH);
      }
      if(s4 === "true"){
        imageRelPaths.push(SLOT_4_OCCUPIED_TRUE_RELPATH);
      }
      if(s5 === "true"){
        imageRelPaths.push(SLOT_5_OCCUPIED_TRUE_RELPATH);
      }
      switch(facing){
        case "north":
          { subtractImageRelPaths.push(FACING_NORTH_OVERLAY_RELPATH_2); break; }
        case "south":
          { subtractImageRelPaths.push(FACING_SOUTH_OVERLAY_RELPATH_2); break; }
        case "west":
          { subtractImageRelPaths.push(FACING_WEST_OVERLAY_RELPATH_2); break; }
        case "east":
          { subtractImageRelPaths.push(FACING_EAST_OVERLAY_RELPATH_2); break; }
      }
      return {
        mode: "replace",
        iconBlockIds: [CHISELED_BOOKSHELF_EMPTY_BLOCK_ID],
        subtractImageRelPaths: subtractImageRelPaths.length ? subtractImageRelPaths : undefined,
        imageRelPaths: imageRelPaths.length ? imageRelPaths : undefined,
      };
    }
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
