/** Wire estimates are bytes; only presentation converts them to binary units. */
export function formatResourceBytes(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes) || bytes < 0) return "不可用";
  const gib = bytes / (1024 ** 3);
  if (gib >= 1) return `${gib.toFixed(2)} GiB`;
  const mib = bytes / (1024 ** 2);
  return `${mib.toFixed(mib >= 10 ? 0 : 1)} MiB`;
}
