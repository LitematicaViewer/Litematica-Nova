import { emit as tauriEmit, listen as tauriListen } from "@tauri-apps/api/event";

export function emitEvent<T>(event: string, payload?: T): Promise<void> {
  return tauriEmit(event, payload);
}

export function listenEvent<T>(
  event: string,
  handler: Parameters<typeof tauriListen<T>>[1],
): Promise<Awaited<ReturnType<typeof tauriListen<T>>>> {
  return tauriListen<T>(event, handler);
}
