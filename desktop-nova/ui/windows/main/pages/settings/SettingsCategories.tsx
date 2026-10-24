import React from "react";
import { Button } from "../../../../components/ui/button";
import { useI18n } from "../../../../i18n";

export const SETTINGS_CATEGORIES = ["appearance", "data", "rendering", "shaders", "ai", "diagnostics", "maintenance"] as const;
export type SettingsCategory = typeof SETTINGS_CATEGORIES[number];
export function SettingsCategories({ category, onSelect }: { category: SettingsCategory; onSelect: (category: SettingsCategory) => void }) {
  const { locale } = useI18n();
  const labels = locale === "en-US"
    ? ["Appearance", "Data & directories", "Rendering", "Shaders & resource packs", "AI", "Advanced & diagnostics", "Maintenance"]
    : ["外观", "数据与目录", "渲染", "光影与资源包", "AI", "高级与诊断", "维护"];
  return <nav className="settings-categories" aria-label={locale === "en-US" ? "Settings categories" : "设置分类"}>
    {SETTINGS_CATEGORIES.map((key, index) => <Button key={key} variant="ghost" aria-current={category === key ? "page" : undefined} onClick={() => onSelect(key)}>{labels[index]}</Button>)}
  </nav>;
}
