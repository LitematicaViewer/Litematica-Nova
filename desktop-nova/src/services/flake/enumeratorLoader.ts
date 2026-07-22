import { readAppDataFile } from "../../platform/files";

/**
 * 枚举器缓存，用于存储已加载的方块ID集合
 */
const enumeratorCache = new Map<string, Set<string>>();

/**
 * 加载并缓存枚举器文件
 * 
 * @param relativePath - 枚举器文件的相对路径（如 "enumerator/system_enum/state_waterlogged.json"）
 * @returns 方块ID集合
 */
async function loadEnumerator(relativePath: string): Promise<Set<string>> {
  const cached = enumeratorCache.get(relativePath);
  if (cached) {
    return cached;
  }

  try {
    const raw = await readAppDataFile(relativePath);
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      console.warn(`Enumerator file ${relativePath} is not an array`);
      return new Set();
    }

    const normalized = parsed
      .map((id) => String(id || "").trim().toLowerCase())
      .filter(Boolean);
    
    const result = new Set(normalized);
    enumeratorCache.set(relativePath, result);
    return result;
  } catch (error) {
    console.warn(`Failed to load enumerator ${relativePath}:`, error);
    return new Set();
  }
}

/**
 * 检查方块ID是否在指定的枚举器中
 * 
 * @param blockId - 方块ID（已规范化）
 * @param enumeratorPath - 枚举器文件相对路径
 * @returns 是否匹配
 */
export async function isBlockInEnumerator(
  blockId: string,
  enumeratorPath: string
): Promise<boolean> {
  const enumerator = await loadEnumerator(enumeratorPath);
  return enumerator.has(blockId);
}

/**
 * 清除枚举器缓存（用于测试或热重载）
 */
export function clearEnumeratorCache(): void {
  enumeratorCache.clear();
}