import { invoke } from "./tauri";

export interface UserConfig {
  theme: string;
  render_display_mode: string;
  preview_mode: string;
  material_list_window_behavior: string;
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
