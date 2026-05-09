import type { NavigationItem } from "./types";

export const navItems: NavigationItem[] = [
    { key: "home", label: "主页", shortLabel: "主页", icon: "home" },
    { key: "library", label: "投影库", shortLabel: "库", icon: "gallery" },
    { key: "properties", label: "属性", shortLabel: "属性", icon: "properties" },
    { key: "statistics", label: "统计", shortLabel: "统计", icon: "statistics" },
    { key: "flake", label: "分层", shortLabel: "分层", icon: "flake" },
    { key: "render", label: "渲染", shortLabel: "渲染", icon: "render" },
    { key: "replace", label: "替换", shortLabel: "替换", icon: "replace" },
    { key: "generate", label: "生成", shortLabel: "生成", icon: "ui_debug" },
    { key: "ui_test", label: "UI 测试", shortLabel: "测试", icon: "ui_debug" },
    { key: "options", label: "选项", shortLabel: "选项", icon: "options" }
];
