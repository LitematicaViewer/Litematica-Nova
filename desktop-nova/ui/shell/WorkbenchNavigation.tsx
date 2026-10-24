import React from "react";
import { Button } from "../components/ui/button";
import { useI18n } from "../i18n";

export const WORKBENCH_ROUTES = ["properties", "render", "flake", "statistics", "replace", "composition"] as const;
export type WorkbenchRoute = typeof WORKBENCH_ROUTES[number];
export function isWorkbenchRoute(route: string): route is WorkbenchRoute {
  return (WORKBENCH_ROUTES as readonly string[]).includes(route);
}
export function WorkbenchNavigation({ route, onNavigate }: { route: string; onNavigate: (route: WorkbenchRoute) => void }) {
  const { locale } = useI18n();
  const labels = locale === "en-US"
    ? ["Overview", "3D", "Layers", "Stockpile", "Replace", "Compose / Split"]
    : ["概览", "三维", "分层", "备货", "替换", "组合/拆分"];
  return <nav className="workbench-tabs" aria-label={locale === "en-US" ? "Workbench" : "工作台"}>
    {WORKBENCH_ROUTES.map((key, index) => <Button key={key} variant="ghost" aria-current={route === key ? "page" : undefined} onClick={() => onNavigate(key)}>{labels[index]}</Button>)}
  </nav>;
}
