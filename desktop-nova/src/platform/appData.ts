import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "./tauri";

export type GraphicsApiPreference = "auto" | "dx12" | "vulkan";
export type MotionPreference = "system" | "off" | "reduced" | "standard" | "rich";

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
  ui_scale: number;
  ui_language: "zh-CN" | "en-US" | "system";
  ui_locale: "zh-CN" | "en-US";
  theme: string;
  color_mode: "light" | "dark" | "system";
  motion_preference: MotionPreference;
  camera_input_mode: "drag" | "pointer_lock";
  camera_speed: number;
  camera_sensitivity: number;
  render_display_mode: string;
  preview_mode: string;
  graphics_backend: GraphicsApiPreference;
  material_list_window_behavior: string;
  show_ui_test_page: boolean;
  debug_mode: boolean;
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

/** Version of the executable hosting this webview. */
export function getApplicationVersion(): Promise<string> { return getVersion(); }
