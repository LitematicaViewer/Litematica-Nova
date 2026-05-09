export type PageKey =
    | "home"
    | "library"
    | "properties"
    | "statistics"
    | "flake"
    | "render"
    | "replace"
    | "generate"
    | "ui_test"
    | "options";

export interface NavigationItem {
    key: PageKey;
    label: string;
    shortLabel: string;
    icon: string;
}

export interface ActiveFile {
    name: string;
    path: string;
    size: string;
}

export interface Snapshot {
    name: string;
    active_file: string | null;
    theme: string;
}

export interface MetricRow {
    label: string;
    value: string;
}
