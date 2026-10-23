import { formatNumber, t } from "../services/i18n";
/** Wire estimates are bytes; only presentation converts them to binary units. */
export function formatResourceBytes(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes) || bytes < 0) return t("native.resource.unavailable");
  const gib = bytes / (1024 ** 3);
  if (gib >= 1) return `${formatNumber(gib, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} GiB`;
  const mib = bytes / (1024 ** 2);
  return `${formatNumber(mib, { minimumFractionDigits: mib >= 10 ? 0 : 1, maximumFractionDigits: mib >= 10 ? 0 : 1 })} MiB`;
}
