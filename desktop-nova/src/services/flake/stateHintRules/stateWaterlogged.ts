const WATERLOGGED_TRUE_BLOCK_ID = "minecraft:water";

/**
 * 为含水（waterlogged）状态添加水方块图标到覆盖层数组
 * 
 * @param states - 方块状态对象
 * @param overlayIconBlockIds - 已有的覆盖图标数组
 */
export function applyWaterloggedOverlay(
  states: Record<string, string>,
  overlayIconBlockIds: string[]
): void {
  if (states.waterlogged === "true") {
    overlayIconBlockIds.push(WATERLOGGED_TRUE_BLOCK_ID);
  }
}