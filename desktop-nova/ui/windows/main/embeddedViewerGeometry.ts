import type { PresentationPlacement } from "../../../src/business/facade";
import {
  getApplicationWindowOrigin,
  getApplicationWindowVisibility,
  subscribeApplicationWindowGeometry,
} from "../../../src/platform/nativeWindowGeometry";

/**
 * Converts a DOM element's CSS bounds into physical screen pixels for native viewer placement.
 */
export function elementToPhysicalRect(element: HTMLElement) {
  const rect = element.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  return {
    x: Math.round(rect.left * dpr),
    y: Math.round(rect.top * dpr),
    width: Math.round(rect.width * dpr),
    height: Math.round(rect.height * dpr),
  };
}

/**
 * Converts a DOM element's bounds into rounded CSS pixel coordinates.
 */
export function elementToCssRect(element: HTMLElement) {
  const rect = element.getBoundingClientRect();
  return {
    x: Math.round(rect.left),
    y: Math.round(rect.top),
    width: Math.round(rect.width),
    height: Math.round(rect.height),
  };
}

/**
 * Checks whether a viewer rect is large enough to pass to the embedded native window.
 */
export function isUsableEmbeddedRect(rect: { width: number; height: number }): boolean {
  return rect.width > 8 && rect.height > 8;
}

/** Resolves a DOM host to the product-level physical screen placement contract. */
export async function elementToPresentationPlacement(
  element: HTMLElement,
): Promise<PresentationPlacement> {
  const clientRect = elementToPhysicalRect(element);
  const origin = await getApplicationWindowOrigin();
  return {
    x: origin.x + clientRect.x,
    y: origin.y + clientRect.y,
    width: clientRect.width,
    height: clientRect.height,
    dpiScale: window.devicePixelRatio || 1,
    focused: document.hasFocus(),
  };
}

/** Native controls are clipped to the page's visible scroll/viewport bounds.
 * DWM clips the child to its HWND parent; this also keeps it out of HTML toolbars.
 * CSS pixels (including WebView zoom) are converted once to physical pixels.
 */
export async function elementToNativeChildPlacement(element: HTMLElement): Promise<PresentationPlacement> {
  if (document.visibilityState === "hidden" || !await getApplicationWindowVisibility()) {
    return { x: 0, y: 0, width: 0, height: 0, dpiScale: window.devicePixelRatio || 1, focused: false };
  }
  const rect = element.getBoundingClientRect();
  let left = Math.max(0, rect.left), top = Math.max(0, rect.top);
  let right = Math.min(window.innerWidth, rect.right), bottom = Math.min(window.innerHeight, rect.bottom);
  for (let parent = element.parentElement; parent; parent = parent.parentElement) {
    const style = window.getComputedStyle(parent);
    const bounds = parent.getBoundingClientRect();
    if (/(auto|scroll|hidden|clip)/.test(style.overflowX)) { left = Math.max(left, bounds.left); right = Math.min(right, bounds.right); }
    if (/(auto|scroll|hidden|clip)/.test(style.overflowY)) { top = Math.max(top, bounds.top); bottom = Math.min(bottom, bounds.bottom); }
  }
  const origin = await getApplicationWindowOrigin();
  const scale = window.devicePixelRatio || 1;
  return { x: origin.x + Math.round(left * scale), y: origin.y + Math.round(top * scale),
    width: Math.max(0, Math.round((right - left) * scale)), height: Math.max(0, Math.round((bottom - top) * scale)),
    dpiScale: scale, focused: false };
}

/** Keeps DOM placement in presentation while the platform adapter owns Tauri events. */
export async function subscribeNativeWindowGeometry(
  listener: () => void,
): Promise<() => void> {
  return subscribeApplicationWindowGeometry(listener);
}
