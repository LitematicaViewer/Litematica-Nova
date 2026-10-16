/** Readable spelling only. File operations must keep the original path. */
export function displayFilePath(path: string): string {
  const windows = path.replace(/\//g, "\\");
  if (/^\\\\\?\\UNC\\[^\\]+\\[^\\]+(?:\\|$)/i.test(windows)) return `\\\\${windows.slice(8)}`;
  if (/^\\\\\?\\[a-z]:\\/i.test(windows)) return windows.slice(4);
  if (/^[a-z]:\\/i.test(windows) || /^\\\\(?![?.]\\)[^\\]+\\[^\\]+(?:\\|$)/.test(windows)) return windows;
  return path;
}

/** Lexical identity for ordinary Windows drive/UNC aliases; never folds POSIX or device paths. */
export function windowsPathKey(path: string): string {
  const readable = displayFilePath(path);
  if (/^[a-z]:\\/i.test(readable)) {
    const root = readable.slice(0, 3);
    const tail = readable.slice(3).replace(/\\+/g, "\\").replace(/\\+$/, "");
    return `windows:${root.toLowerCase()}${tail.toLowerCase()}`;
  }
  if (/^\\\\(?![?.]\\)[^\\]+\\[^\\]+(?:\\|$)/.test(readable)) {
    return `windows:${readable.replace(/\\+/g, "\\").replace(/^\\/, "\\\\").replace(/\\+$/, "").toLowerCase()}`;
  }
  return `native:${path === "/" ? path : path.replace(/\/+$/, "")}`;
}

/** Format paths embedded in a status/error message without changing stored errors or IO arguments. */
export function displayPathText(text: string): string {
  return text.replace(/\\\\\?\\UNC\\(?=[^\\]+\\[^\\]+)/gi, "\\\\")
    .replace(/\\\\\?\\(?=[a-z]:[\\/])/gi, "");
}
