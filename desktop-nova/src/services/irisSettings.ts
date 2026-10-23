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
  requires_scene_reload?: boolean;
}
export interface IrisSettingsChange { profile: string | null; values: Record<string, string>; restore_defaults?: boolean }
function settingsArgs(root: string, pack: string): string[] {
  return ["iris-settings", "--root", root, "--pack", pack, "--language", getUiLocale() === "zh-CN" ? "zh_cn" : "en_us"];
}
export function readIrisSettings(root: string, pack: string): Promise<IrisSettingsSnapshot> {
  return invoke("iris_shaderpack_settings", { args: settingsArgs(root, pack) });
}
export function saveIrisSettings(root: string, pack: string, input: IrisSettingsChange): Promise<IrisSettingsSnapshot> {
  return invoke("iris_shaderpack_settings", { args: [...settingsArgs(root, pack), "--save-json", JSON.stringify(input)] });
}
