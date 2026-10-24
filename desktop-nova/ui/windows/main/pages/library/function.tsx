import { displayFilePath } from "../../../../../src/business/projectionLibrary";

/**
 * 分发窗口与本地库浏览器共用的“尾部路径级数”默认值。
 * 放在这里而非 `sendDialog.tsx`，避免 `function` 与 `sendDialog` 形成循环依赖。
 */
export const DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT = 3;

/**
 * 压缩文件夹路径，保留最后 N 级目录。
 * @param path 文件夹路径
 * @param tailCount 保留的目录级数
 * @returns 压缩后的文件夹路径
 */
export function compactFolderPath(path: string, tailCount: number): string {
  const readable = displayFilePath(path.trim());
  const normalized = /^[a-z]:[\\/]$/i.test(readable) || readable === "/" ? readable : readable.replace(/[\\/]+$/, "");
  const separator = normalized.includes("\\") ? "\\" : "/";
  const parts = normalized.split(/[\\/]+/).filter(Boolean);
  const safeTailCount = Number.isFinite(tailCount) && tailCount >= 1 ? Math.floor(tailCount) : DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT;
  const hasWindowsDrive = /^[A-Za-z]:/.test(normalized);
  const hasUnixRoot = normalized.startsWith("/");
  const uncRoot = normalized.match(/^\\\\[^\\]+\\[^\\]+/);
  if (uncRoot) {
    const tail = normalized.slice(uncRoot[0].length).split(/[\\/]+/).filter(Boolean);
    return tail.length <= safeTailCount ? normalized : `${uncRoot[0]}\\...\\${tail.slice(-safeTailCount).join("\\")}`;
  }
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

/**
 * 文件名校验（供分发窗口与回收区还原改名复用）。返回空字符串表示合法。
 */
export function replicaNameError(fileName: string): string {
  const normalized = ensureLitematicFileName(fileName);
  if (!normalized) return "请填写文件名。";
  if (/[\\/:*?"<>|]/.test(normalized)) return "文件名不能包含 \\ / : * ? \" < > | 等字符。";
  return "";
}