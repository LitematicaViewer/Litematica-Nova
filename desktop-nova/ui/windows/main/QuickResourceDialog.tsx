import React from "react";
import type { QuickResourceAction, QuickResourceReport } from "../../../src/business/facade";
import { Dialog } from "../../components/Dialog";
import { Button } from "../../components/ui/button";

export function QuickResourceDialog({ report, busy, onChoose }: {
  report: QuickResourceReport; busy: boolean; onChoose: (action: QuickResourceAction) => Promise<void>;
}) {
  const mib = (bytes: number) => `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  const measured = !!report.estimate && !!report.budget && report.pressureBasisPoints !== null;
  const preloadAllowed = measured && (report.pressure === "normal" || report.pressure === "warning");
  return <Dialog title="加载方式" width="md" busy={busy} onClose={() => void onChoose("cancel")}
    footer={<><Button variant="ghost" disabled={busy} onClick={() => void onChoose("cancel")}>取消</Button>
      <Button disabled={busy || !measured} onClick={() => void onChoose("reduced_realtime")}>降低画质（关闭阴影）</Button>
      <Button disabled={busy || !preloadAllowed} onClick={() => void onChoose("preload")}>预加载</Button>
      <Button disabled={busy} onClick={() => void onChoose("realtime")}>实时加载</Button></>}>
    <p>{measured ? `完整投影显存需求上界 ${mib(report.estimate!.gpuPeakUpperBytes)}` : "完整投影显存需求未确认，预加载尚未获准入。请选择实时加载或取消。"}</p>
    {report.budget && <p>适配器：{report.budget.adapterName}<br />实际预算 {mib(report.budget.budgetBytes)}；当前使用 {mib(report.budget.currentUsageBytes)}；可用余量 {mib(report.budget.safeAvailableBytes)}。</p>}
    {report.invalidReason && <p role="alert">{report.invalidReason}</p>}
  </Dialog>;
}
