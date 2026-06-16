import { executeBackend } from "./backend";

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

/**
 * 从 litematic 文件中获取指定位置的容器内容
 * @param filePath litematic 文件路径
 * @param x 方块X坐标
 * @param y 方块Y坐标
 * @param z 方块Z坐标
 */
export async function loadContainerData(
  filePath: string,
  regionName: string,
  x: number,
  y: number,
  z: number
): Promise<ContainerData | null> {
  // 使用 queueMicrotask 延迟执行，避免阻塞主线程
  return new Promise((resolve) => {
    queueMicrotask(async () => {
      try {
        console.log("loadContainerData 调用参数:", { filePath, regionName, x, y, z });
        
        // 使用 read-block-entity 命令读取指定位置的 block entity
        const out = await executeBackend("litematica_core.exe", [
          "read-block-entity",
          "--input",
          filePath,
          `--x=${x}`,
          `--y=${y}`,
          `--z=${z}`,
          "--json",
        ]);
        
        console.log("read-block-entity 原始输出:", out);
        
        if (!out || out.trim() === "null") {
          console.log("未找到 block entity");
          resolve(null);
          return;
        }
        
        const parsed = JSON.parse(out);
        console.log("read-block-entity 返回:", parsed);
        
        if (!parsed) {
          resolve(null);
          return;
        }
        
        // 解析物品
        const items = parseContainerItems(parsed.nbt);
        
        resolve({
          block_id: parsed.block_id,
          position: { x: parsed.x, y: parsed.y, z: parsed.z },
          items,
        });
      } catch (error) {
        console.error("Failed to load container data:", error);
        resolve(null);
      }
    });
  });
}

/**
 * 从 tile entity NBT 数据中解析物品列表
 */
function parseContainerItems(tileEntity: any): ContainerItem[] {
  const items: ContainerItem[] = [];
  
  // 尝试查找 Items 字段（大小写不敏感）
  const itemsField = findCaseInsensitive(tileEntity, "Items");
  
  if (Array.isArray(itemsField)) {
    itemsField.forEach((item: any) => {
      const id = findCaseInsensitive(item, "id");
      const count = findCaseInsensitive(item, "Count") || 1;
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
  const containerTypes = [
    "chest",
    "trapped_chest",
    "shulker_box",
    "barrel",
    "furnace",
    "blast_furnace",
    "smoker",
    "hopper",
    "dropper",
    "dispenser",
  ];
  
  const id = blockId.toLowerCase();
  return containerTypes.some((type) => id.includes(type));
}

/**
 * 获取容器类型（用于UI显示）
 * 返回 null 表示不支持的容器类型
 */
export function getContainerType(blockId: string): "chest" | "shulker_box" | "barrel" | null {
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
  
  return null;
}