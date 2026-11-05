import { readLayerContainer } from "../platform/applicationOperations";

export interface ContainerItem {
  id: string;
  count: number;
  slot: number;
}

export interface ContainerData {
  block_id: string;
  position: { x: number; y: number; z: number };
  items: ContainerItem[];
}

export type ContainerType = "chest" | "shulker_box" | "barrel" | "hopper";

export async function loadContainerData(sourcePath: string, x: number, y: number, z: number): Promise<ContainerData | null> {
  const data = await readLayerContainer({ sourcePath, x, y, z });
  return data ? {
    block_id: data.block_id,
    position: { x: data.x, y: data.y, z: data.z },
    items: data.items.map(({ id, count, slot }) => ({ id, count: count ?? NaN, slot: slot ?? NaN })),
  } : null;
}

/**
 * 检查方块ID是否为容器类型
 */
export function isContainerBlock(blockId: string): boolean {
  return getContainerType(blockId) !== null;
}

/**
 * 获取容器类型（用于UI显示）
 * 返回 null 表示不支持的容器类型
 */
export function getContainerType(blockId: string): ContainerType | null {
  const id = blockId.toLowerCase();

  if (id.includes("chest") && !id.includes("ender_chest")) {
    return "chest";
  }

  if (id.includes("shulker_box")) {
    return "shulker_box";
  }

  if (id.includes("barrel")) {
    return "barrel";
  }

  if (id.includes("hopper")) {
    return "hopper";
  }

  return null;
}
