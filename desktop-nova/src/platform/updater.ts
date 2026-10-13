import { invoke } from "./tauri";
import { listenEvent } from "./events";

export interface UpdateView {
  phase: "idle" | "disabled" | "checking" | "available" | "current" | "skipped" | "downloading" | "downloaded" | "waiting" | "installing" | "cancelled" | "error" | "restartRequired";
  currentVersion: string;
  version: string | null;
  date: string | null;
  notes: string | null;
  downloaded: number;
  total: number | null;
  source: string | null;
  message: string | null;
}
export const checkForUpdate = (manual = false) => invoke<UpdateView>("application_update_check", { manual });
export const downloadUpdate = () => invoke<UpdateView>("application_update_download");
export const installUpdate = () => invoke<void>("application_update_install");
export const restartAfterUpdateFailure = () => invoke<void>("application_update_restart");
export const cancelUpdate = () => invoke<UpdateView>("application_update_cancel");
export const skipUpdate = () => invoke<UpdateView>("application_update_skip");
export const readUpdateState = () => invoke<UpdateView>("application_update_snapshot");
export const onUpdateState = (callback: (view: UpdateView) => void) => listenEvent<UpdateView>("nova-update-state", (event) => callback(event.payload));
