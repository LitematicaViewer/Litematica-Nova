import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
import React from "react";
import type { QuickResourceAction, QuickResourceReport } from "../../../src/business/facade";
import { Dialog } from "../../components/Dialog";
import { Button } from "../../components/ui/button";

export function QuickResourceDialog({ report, busy, onChoose }: {
  report: QuickResourceReport; busy: boolean; onChoose: (action: QuickResourceAction) => Promise<void>;
}) {
  useI18n();
  const mib = (bytes: number) => `${(bytes / (1024 * 1024)).toLocaleString(getUiLocale(), {minimumFractionDigits:1,maximumFractionDigits:1})} MiB`;
  const measured = !!report.estimate && !!report.budget && report.pressureBasisPoints !== null;
  const preloadAllowed = measured && (report.pressure === "normal" || report.pressure === "warning");
  return <Dialog title={t("render.loadingMethod")} width="md" busy={busy} onClose={() => void onChoose("cancel")}
    footer={<><Button variant="ghost" disabled={busy} onClick={() => void onChoose("cancel")}>{t("common.cancel")}</Button>
      <Button disabled={busy || !measured} onClick={() => void onChoose("reduced_realtime")}>{t("render.reduceWithoutShadows")}</Button>
      <Button disabled={busy || !preloadAllowed} onClick={() => void onChoose("preload")}>{t("render.preload")}</Button>
      <Button disabled={busy} onClick={() => void onChoose("realtime")}>{t("render.realtime")}</Button></>}>
    <p>{measured ? t("render.memoryUpperBound", { memory: mib(report.estimate!.gpuPeakUpperBytes) }) : t("render.memoryUnknown")}</p>
    {report.budget && <p>{t("render.gpuBudget", {adapter:report.budget.adapterName,budget:mib(report.budget.budgetBytes),used:mib(report.budget.currentUsageBytes),available:mib(report.budget.safeAvailableBytes)})}</p>}
    {report.invalidReason && <p role="alert">{localizeMessage(report.invalidReason)}</p>}
  </Dialog>;
}
