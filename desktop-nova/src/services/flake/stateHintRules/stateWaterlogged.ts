// const WATERLOGGED_TRUE_BLOCK_ID = "minecraft:water";
const WATERLOGGED_TRUE_RELPATH = "data/flake/state_hint/waterlogged_true.png";

/**
 * 为含水（waterlogged）状态添加水方块图标到覆盖层数组
 * 
 * @param states - 方块状态对象
 * @param imageRelPaths - 遮罩图像路径数组
 * @param overlayIconBlockIds - 已有的覆盖图标数组
 * @returns 是否需要添加水方块图标到覆盖层数组
 */
export function applyWaterloggedOverlay(
  states: Record<string, string>,
  imageRelPaths: string[],
  overlayIconBlockIds: string[]
): void {
  if (states.waterlogged === "true") {
    // overlayIconBlockIds.push(WATERLOGGED_TRUE_BLOCK_ID);
    imageRelPaths.push(WATERLOGGED_TRUE_RELPATH);
  }
}