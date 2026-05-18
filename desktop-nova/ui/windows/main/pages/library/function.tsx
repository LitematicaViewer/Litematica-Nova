import { DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT } from "./sendDialog";

/**
 * 压缩文件夹路径，保留最后 N 级目录。
 * @param path 文件夹路径
 * @param tailCount 保留的目录级数
 * @returns 压缩后的文件夹路径
 */
export function compactFolderPath(path: string, tailCount: number): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const separator = normalized.includes("\\") ? "\\" : "/";
  const parts = normalized.split(/[\\/]+/).filter(Boolean);
  const safeTailCount = Number.isFinite(tailCount) && tailCount >= 1 ? Math.floor(tailCount) : DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT;
  const hasWindowsDrive = /^[A-Za-z]:/.test(normalized);
  const hasUnixRoot = normalized.startsWith("/");
  const visiblePrefixSegments = hasWindowsDrive || !hasUnixRoot ? 1 : 0;
  if (parts.length <= safeTailCount + visiblePrefixSegments) return normalized;
  const driveOrRoot = hasWindowsDrive ? normalized.match(/^[A-Za-z]:/)?.[0] || "" : hasUnixRoot ? "" : parts[0];
  const tail = parts.slice(-safeTailCount).join(separator);
  return driveOrRoot ? `${driveOrRoot}${separator}...${separator}${tail}` : `${separator}...${separator}${tail}`;
}

/**
 * 确保文件名以 `.litematic` 结尾。
 * @param fileName 文件名
 * @returns 确保后的文件名
 */
export function ensureLitematicFileName(fileName: string): string {
  const trimmed = fileName.trim();
  if (!trimmed) return "";
  return /\.litematic$/i.test(trimmed) ? trimmed : `${trimmed}.litematic`;
}

