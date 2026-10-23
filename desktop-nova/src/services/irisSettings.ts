import { invoke } from "../platform/tauri";
import { getUiLocale } from "./i18n";

export interface IrisOption {
  id: string;
  kind: "boolean" | "string";
  label: string;
  description?: string | null;
  default_value: string;
  value: string;
  values: string[];
  value_labels?: Record<string, string>;
  presentation?: "editable" | "readonly" | "information";
  slider?: boolean;
}
export interface IrisProfile { id: string; label: string; values: Record<string, string> }
export type IrisMenuElement = { kind: "empty" | "profile" } | { kind: "link"; screen: string; label: string } | { kind: "option"; option: string; slider?: boolean };
export interface IrisScreen { id: string; title: string; columns: number; elements: IrisMenuElement[] }
export interface IrisSettingsSnapshot {
  schema: "nova.iris-settings.v1";
  root: string;
  pack: string;
  pack_identity: string;
  packs: Array<{ name: string }>;
  profiles: IrisProfile[];
  selected_profile: string | null;
  options: IrisOption[];
  screens: IrisScreen[];
  saved_values: Record<string, string>;
  applied_to_active_viewer?: boolean;
  selected_for_rendering?: boolean;
  discarded_saved_values?: string[];
  requires_scene_reload?: boolean;
}
export interface IrisSettingsChange { profile: string | null; values: Record<string, string>; restore_defaults?: boolean }
/** The sole display boundary: never use its output as an option ID or persisted value. */
export function irisDisplayText(text: string): string {
  return text.replace(/§x(?:§[0-9a-f]){6}|§[0-9a-fk-or]/gi, "");
}
function displaySnapshot(value: IrisSettingsSnapshot): IrisSettingsSnapshot {
  return { ...value,
    options: value.options.map(option => ({ ...option, label: irisDisplayText(option.label),
      description: option.description == null ? option.description : irisDisplayText(option.description),
      value_labels: Object.fromEntries(option.values.map(raw => [raw, irisDisplayText(option.value_labels?.[raw] ?? raw)])) })),
    profiles: value.profiles.map(profile => ({ ...profile, label: irisDisplayText(profile.label) })),
    screens: value.screens.map(screen => ({ ...screen, title: irisDisplayText(screen.title),
      elements: screen.elements.map(element => element.kind === "link" ? { ...element, label: irisDisplayText(element.label) } : element) })),
  };
}
function settingsArgs(root: string, pack: string): string[] {
  return ["iris-settings", "--root", root, "--pack", pack, "--language", getUiLocale() === "zh-CN" ? "zh_cn" : "en_us"];
}
export async function readIrisSettings(root: string, pack: string): Promise<IrisSettingsSnapshot> {
  return displaySnapshot(await invoke("iris_shaderpack_settings", { args: settingsArgs(root, pack) }));
}
export async function saveIrisSettings(root: string, pack: string, input: IrisSettingsChange): Promise<IrisSettingsSnapshot> {
  return displaySnapshot(await invoke("iris_shaderpack_settings", { args: [...settingsArgs(root, pack), "--save-json", JSON.stringify(input)] }));
}
export function openIrisSettings(root?: string, pack?: string): Promise<void> {
  return invoke("open_iris_options_window", { root: root?.trim() || null, pack: pack?.trim() || null });
}
export function closeIrisSettings(): Promise<void> { return invoke("close_iris_options_window"); }
