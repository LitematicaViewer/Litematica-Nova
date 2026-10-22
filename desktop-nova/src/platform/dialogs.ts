import { localizeMessage } from "../services/i18n";
import {
  ask as tauriAsk,
  confirm as tauriConfirm,
  message as tauriMessage,
  open as tauriOpen,
  save as tauriSave,
  type OpenDialogOptions,
  type SaveDialogOptions,
  type MessageDialogButtons,
} from "@tauri-apps/plugin-dialog";

export function openDialog(options?: OpenDialogOptions): Promise<string | string[] | null> {
  return tauriOpen(localizedFileOptions(options));
}

export function saveDialog(options?: SaveDialogOptions): Promise<string | null> {
  return tauriSave(localizedFileOptions(options));
}

export function messageDialog(
  message: string,
  options?: Parameters<typeof tauriMessage>[1],
): ReturnType<typeof tauriMessage> {
  return tauriMessage(localizeMessage(message), localizedMessageOptions(options));
}

export function confirmDialog(message: string, options?: Parameters<typeof tauriConfirm>[1]): Promise<boolean> {
  return tauriConfirm(localizeMessage(message), localizedMessageOptions(options));
}

export function askDialog(message: string, options?: Parameters<typeof tauriAsk>[1]): Promise<boolean> {
  return tauriAsk(localizeMessage(message), localizedMessageOptions(options));
}

function localizedFileOptions<T extends OpenDialogOptions | SaveDialogOptions>(options: T | undefined): T | undefined {
  if (!options) return options;
  return { ...options, title: options.title ? localizeMessage(options.title) : options.title,
    filters: options.filters?.map(filter => ({ ...filter, name: localizeMessage(filter.name) })) };
}
type NativeDialogLabels = Exclude<Parameters<typeof tauriMessage>[1], string | undefined>
  & Exclude<Parameters<typeof tauriConfirm>[1], string | undefined>;
function localizedMessageOptions(options: string | NativeDialogLabels | undefined): string | NativeDialogLabels | undefined {
  if (typeof options === "string") return localizeMessage(options);
  if (!options) return options;
  const localized: NativeDialogLabels = { ...options };
  if (localized.title) localized.title = localizeMessage(localized.title);
  if (localized.okLabel) localized.okLabel = localizeMessage(localized.okLabel);
  if (localized.cancelLabel) localized.cancelLabel = localizeMessage(localized.cancelLabel);
  if (localized.buttons) localized.buttons = localizedButtons(localized.buttons);
  return localized;
}
function localizedButtons(buttons: MessageDialogButtons): MessageDialogButtons {
  // Default enum buttons belong to the OS; preserve their stable API values.
  if (typeof buttons === "string") return buttons;
  if (typeof buttons.ok === "string") {
    return typeof buttons.cancel === "string"
      ? { ok: localizeMessage(buttons.ok), cancel: localizeMessage(buttons.cancel) }
      : { ok: localizeMessage(buttons.ok) };
  }
  if (typeof buttons.yes === "string" && typeof buttons.no === "string" && typeof buttons.cancel === "string") {
    return { yes: localizeMessage(buttons.yes), no: localizeMessage(buttons.no), cancel: localizeMessage(buttons.cancel) };
  }
  return buttons;
}
