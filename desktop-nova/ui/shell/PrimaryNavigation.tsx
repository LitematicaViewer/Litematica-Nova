import React from "react";
import { Button } from "../components/ui/button";
import { useI18n } from "../i18n";
import { NavIcon } from "./NavIcon";

// Keep the existing page identifiers: links and saved drafts still open the
// same page; only the location and labels of its navigation entry change.
export const PRIMARY_ROUTES = ["home", "library", "properties", "render", "flake", "statistics", "replace", "composition", "generate", "settings"] as const;
export type PrimaryRoute = typeof PRIMARY_ROUTES[number];
export const FILE_REQUIRED_ROUTES = ["properties", "render", "flake", "statistics", "replace"] as const;
export function requiresProjection(route: string): boolean {
  return (FILE_REQUIRED_ROUTES as readonly string[]).includes(route);
}
export function primaryNavigationItems(locale: string) {
  const labels = locale === "en-US"
    ? ["Home", "Library", "Overview", "3D", "Layers", "Stockpile", "Replace", "Compose / Split", "Generate", "Settings"]
    : ["首页", "投影库", "概览", "三维", "分层", "备货", "替换", "组合/拆分", "生成", "设置"];
  const icons = ["home", "gallery", "properties", "render", "flake", "statistics", "replace", "properties", "generate", "options"];
  return PRIMARY_ROUTES.map((route, index) => ({ route, label: labels[index], icon: icons[index] }));
}

export function PrimaryNavigation({ route, onNavigate, theme }: { route: string; onNavigate: (route: PrimaryRoute) => void; theme: string }) {
  const { locale } = useI18n();
  return <nav className="nav-list primary-navigation" aria-label={locale === "en-US" ? "Main navigation" : "主导航"}>
    {primaryNavigationItems(locale).map(item => <Button variant="ghost" key={item.route} type="button"
      data-route={item.route} className={`sidebar-item nav-item${route === item.route ? " active nav-item-active" : ""}`}
      aria-current={route === item.route ? "page" : undefined} aria-label={item.label} title={item.label}
      onClick={() => onNavigate(item.route)}>
      <NavIcon name={item.icon} theme={theme} /><span className="nav-label">{item.label}</span>
    </Button>)}
  </nav>;
}
