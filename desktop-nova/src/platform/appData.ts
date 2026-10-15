import { invoke } from "./tauri";

export type GraphicsApiPreference = "auto" | "dx12" | "vulkan";

export interface V5CompatibilityConfig {
  backend: "auto" | "vulkan" | "iris-gl46";
  shaderpack_root: string;
  shaderpack_name: string;
  iris_fallback: "fail" | "vulkan";
  fabric_mods_dir: string;
  resource_packs: string[];
}

export interface ViewerControlConfig {
  allow_time_modification: boolean;
  allow_weather_modification: boolean;
}

export interface UserConfig {
  theme: string;
  color_mode: "light" | "dark" | "system";
  render_display_mode: string;
  preview_mode: string;
  graphics_backend: GraphicsApiPreference;
  material_list_window_behavior: string;
  show_ui_test_page: boolean;
  local_library_tail_path_count: number;
  statistics_enumerator_info_rules: string[];
  v5_compatibility: V5CompatibilityConfig;
  viewer_controls: ViewerControlConfig;
}

export interface UserConfigInfo {
  config_dir: string;
  default_config_dir: string;
  config: UserConfig;
}

export function getUserConfig(): Promise<UserConfigInfo> {
  return invoke("get_user_config");
}

export function saveUserConfig(input: Partial<UserConfig>): Promise<UserConfigInfo> {
  return invoke("save_user_config", { input });
}

export function chooseUserConfigDir(): Promise<string | null> {
  return invoke("choose_user_config_dir");
}

export function openUserConfigDir(): Promise<void> {
  return invoke("open_user_config_dir");
}

export function setUserConfigDir(path: string, migrate: boolean): Promise<UserConfigInfo> {
  return invoke("set_user_config_dir", { path, migrate });
}

export function resetUserConfigDir(migrate: boolean): Promise<UserConfigInfo> {
  return invoke("reset_user_config_dir", { migrate });
}
