import {
  ask as tauriAsk,
  confirm as tauriConfirm,
  message as tauriMessage,
  open as tauriOpen,
  save as tauriSave,
  type OpenDialogOptions,
  type SaveDialogOptions,
} from "@tauri-apps/plugin-dialog";

export function openDialog(options?: OpenDialogOptions): Promise<string | string[] | null> {
  return tauriOpen(options);
}

export function saveDialog(options?: SaveDialogOptions): Promise<string | null> {
  return tauriSave(options);
}

export function messageDialog(
  message: string,
  options?: Parameters<typeof tauriMessage>[1],
): ReturnType<typeof tauriMessage> {
  return tauriMessage(message, options);
}

export function confirmDialog(message: string, options?: Parameters<typeof tauriConfirm>[1]): Promise<boolean> {
  return tauriConfirm(message, options);
}

export function askDialog(message: string, options?: Parameters<typeof tauriAsk>[1]): Promise<boolean> {
  return tauriAsk(message, options);
}
