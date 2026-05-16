import { LocalLibraryFolder, ProjectionRecord } from "../../../../../src/services/libraryStore";
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

/**
 * 格式化文件大小。
 * @param numBytes 文件大小
 * @returns 格式化后的文件大小
 */
export function formatSize(numBytes: number): string {
  let value = Math.max(0, numBytes);
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  while (value >= 1024.0 && index < units.length - 1) {
    value /= 1024.0;
    index++;
  }
  return index === 0 ? `${Math.floor(value)} ${units[index]}` : `${value.toFixed(1)} ${units[index]}`;
}

/**
 * 格式化日期。
 * @param timestamp 时间戳
 * @returns 格式化后的日期
 */
export function formatDate(timestamp: number): string {
  if (!timestamp) return "-";
  const d = new Date(timestamp);
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/**
 * 获取记录状态标签。
 * @param record 记录
 * @returns 记录状态标签
 */
export function recordStatusLabel(record: ProjectionRecord): string {
  if (record.status === "missing") return "文件丢失";
  if (record.status === "parse_error") return "解析失败";
  return "";
}

/**
 * 获取父目录。
 * @param path 路径
 * @returns 父目录
 */
export function parentDirectory(path: string): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const lastSeparatorIndex = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  return lastSeparatorIndex >= 0 ? normalized.slice(0, lastSeparatorIndex) : "";
}

/**
 * 选择默认发送文件夹。
 * @param record 记录
 * @param folders 文件夹列表
 * @returns 默认发送文件夹
 */
export function pickDefaultSendFolder(record: ProjectionRecord, folders: LocalLibraryFolder[]): string {
  const sourceParent = parentDirectory(record.path).toLowerCase();
  return folders.find((folder) => folder.path.toLowerCase() !== sourceParent)?.path || folders[0]?.path || "";
}

