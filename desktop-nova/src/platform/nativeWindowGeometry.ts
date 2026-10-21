import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@tauri-apps/api/core";

/** Transparent GPU children expose the native parent, not the clipped WebView. */
export async function synchronizeApplicationWindowBackground(color: [number, number, number, number]): Promise<void> {
  if (!isTauri()) return;
  await getCurrentWindow().setBackgroundColor(color);
}

export async function getApplicationWindowOrigin(): Promise<{ x: number; y: number }> {
  const origin = await getCurrentWindow().innerPosition();
  return { x: origin.x, y: origin.y };
}

export async function getApplicationWindowVisibility(): Promise<boolean> {
  const window = getCurrentWindow();
  const [visible, minimized] = await Promise.all([window.isVisible(), window.isMinimized()]);
  return visible && !minimized;
}

export async function subscribeApplicationWindowGeometry(
  listener: () => void,
): Promise<() => void> {
  const novaWindow = getCurrentWindow();
  const notify = () => listener();
  const unlisteners = await Promise.all([
    novaWindow.onMoved(notify),
    novaWindow.onResized(notify),
    novaWindow.onScaleChanged(notify),
    novaWindow.onFocusChanged(notify),
  ]);
  return () => unlisteners.forEach((unlisten) => unlisten());
}

/** Requests closing the current auxiliary window through the native platform seam. */
export function closeApplicationWindow(): Promise<void> {
  return getCurrentWindow().close();
}

export function getApplicationWindowLabel(): string { return getCurrentWindow().label; }

/** The Host remains the only exit authority, including draft ACK and owned-task teardown. */
export function subscribeApplicationWindowCloseRequested(listener: () => void): Promise<() => void> {
  return getCurrentWindow().onCloseRequested(event => {
    // Never let the API's default destroy() bypass the Host close gate.
    event.preventDefault();
    listener();
  });
}
