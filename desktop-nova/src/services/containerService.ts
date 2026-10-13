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
    items: parseContainerItems(data.nbt),
  } : null;
}

function parseContainerItems(tileEntity: any): ContainerItem[] {
  const items: ContainerItem[] = [];

  // 尝试查找 Items 字段（大小写不敏感）
  const itemsField = findCaseInsensitive(tileEntity, "Items");

  if (Array.isArray(itemsField)) {
    itemsField.forEach((item: any) => {
      const id = findCaseInsensitive(item, "id");
      const count = findCaseInsensitive(item, "Count") ?? 1;
      const slot = findCaseInsensitive(item, "Slot") || 0;

      if (id && id !== "minecraft:air") {
        items.push({
          id: String(id),
          count: Number(count),
          slot: Number(slot),
        });
      }
    });
  }

  return items;
}

/**
 * 大小写不敏感查找对象属性
 */
function findCaseInsensitive(obj: any, key: string): any {
  if (!obj || typeof obj !== "object") return undefined;

  // 先尝试直接匹配
  if (key in obj) return obj[key];

  // 尝试大小写不敏感匹配
  const lowerKey = key.toLowerCase();
  for (const k in obj) {
    if (k.toLowerCase() === lowerKey) {
      return obj[k];
    }
  }

  return undefined;
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
