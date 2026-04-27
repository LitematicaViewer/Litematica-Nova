import type { MetricRow, NavigationItem } from "./types";

export const navItems: NavigationItem[] = [
    { key: "home", label: "主页", shortLabel: "主", icon: "home" },
    { key: "library", label: "投影库", shortLabel: "库", icon: "gallery" },
    { key: "properties", label: "属性", shortLabel: "属", icon: "properties" },
    { key: "statistics", label: "统计", shortLabel: "统", icon: "statistics" },
    { key: "flake", label: "分层", shortLabel: "层", icon: "flake" },
    { key: "render", label: "渲染", shortLabel: "染", icon: "render" },
    { key: "replace", label: "替换", shortLabel: "替", icon: "replace" },
    { key: "ui_test", label: "UI 测试", shortLabel: "测", icon: "ui_debug" },
    { key: "options", label: "选项", shortLabel: "项", icon: "options" }
];

export const emptyMetrics: MetricRow[] = [
    { label: "红石偏度：", value: "-" },
    { label: "统计分类：", value: "-" },
    { label: "液体偏度：", value: "-" },
    { label: "密度：", value: "-" },
    { label: "调试：", value: "-" }
];

export const loadedMetrics: MetricRow[] = [
    { label: "红石偏度：", value: "12.8%" },
    { label: "统计分类：", value: "混合结构" },
    { label: "液体偏度：", value: "3.4%  (128u)" },
    { label: "密度：", value: "28.6%" },
    { label: "调试：", value: "等待接入 Python/Tauri 后端统计结果" }
];
