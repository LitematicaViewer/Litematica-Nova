import type { PresentationPlacement } from "../../../src/business/facade";
import type { SceneUiRegion } from "../../../src/platform/sceneView";
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
export function nativeClientPlacement(element: HTMLElement): PresentationPlacement {
  const rect = element.getBoundingClientRect();
  let left = Math.max(0, rect.left), top = Math.max(0, rect.top);
  let right = Math.min(window.innerWidth, rect.right), bottom = Math.min(window.innerHeight, rect.bottom);
  for (let parent = element.parentElement; parent; parent = parent.parentElement) {
    const style = window.getComputedStyle(parent);
    const bounds = parent.getBoundingClientRect();
    if (/(auto|scroll|hidden|clip)/.test(style.overflowX)) { left = Math.max(left, bounds.left); right = Math.min(right, bounds.right); }
    if (/(auto|scroll|hidden|clip)/.test(style.overflowY)) { top = Math.max(top, bounds.top); bottom = Math.min(bottom, bounds.bottom); }
  }
  const scale = window.devicePixelRatio || 1;
  return { x: Math.round(left * scale), y: Math.round(top * scale),
    width: Math.max(0, Math.round((right - left) * scale)), height: Math.max(0, Math.round((bottom - top) * scale)),
    dpiScale: scale, focused: false };
}

export async function elementToNativeChildPlacement(element: HTMLElement): Promise<PresentationPlacement> {
  if (document.visibilityState === "hidden" || !await getApplicationWindowVisibility()) {
    return { x: 0, y: 0, width: 0, height: 0, dpiScale: window.devicePixelRatio || 1, focused: false };
  }
  const origin = await getApplicationWindowOrigin();
  const client = nativeClientPlacement(element);
  return { ...client, x: origin.x + client.x, y: origin.y + client.y };
}

/** A Win32 child-window hole exposes the WebView *behind* it; transparent CSS
 * does not composite over the scene. Only expose the stroke and ink of outlined
 * slots. Keep nine separate hit boxes so the GPU window still handles clicks in
 * an empty centre and the gaps continue to belong to the camera. */
export function sceneOverlayGeometry(
  surfaces: HTMLElement[], slots: HTMLElement[], outlined: boolean,
  viewport: SceneUiRegion, origin: { x: number; y: number }, scale: number,
) {
  const region = (left: number, top: number, right: number, bottom: number): SceneUiRegion => {
    const x = (value: number) => Math.max(0, Math.min(viewport.width, Math.round(value * scale) + origin.x - viewport.x));
    const y = (value: number) => Math.max(0, Math.min(viewport.height, Math.round(value * scale) + origin.y - viewport.y));
    return { x: x(left), y: y(top), width: Math.max(0, x(right) - x(left)), height: Math.max(0, y(bottom) - y(top)) };
  };
  const bounds = (element: HTMLElement) => {
    const b = element.getBoundingClientRect(); return region(b.left, b.top, b.right, b.bottom);
  };
  const hotbarRegions = slots.map(bounds);
  const ink = slots.map((slot, index) => {
    if (!outlined) return [hotbarRegions[index]];
    const {left, top, right, bottom} = slot.getBoundingClientRect();
    const edge = parseFloat(getComputedStyle(slot).borderTopWidth);
    return [
      region(left, top, right, top + edge), region(left, bottom - edge, right, bottom),
      region(left, top + edge, left + edge, bottom - edge), region(right - edge, top + edge, right, bottom - edge),
      ...Array.from(slot.querySelectorAll<HTMLElement>("kbd, .block-icon-image, .block-icon-fallback"), bounds),
    ];
  });
  const contains = (a: SceneUiRegion, b: SceneUiRegion) => a.x <= b.x && a.y <= b.y
    && a.x + a.width >= b.x + b.width && a.y + a.height >= b.y + b.height;
  const prune = (rects: SceneUiRegion[]) => {
    const result: SceneUiRegion[] = [];
    for (const rect of rects) {
      if (!rect.width || !rect.height || result.some(other => contains(other, rect))) continue;
      for (let i = result.length - 1; i >= 0; i--) if (contains(rect, result[i])) result.splice(i, 1);
      result.push(rect);
    }
    return result;
  };
  const panels = surfaces.map(bounds);
  let uiRegions = prune([...panels, ...ink.flat()]);
  // Win32 accepts 64 holes. Overlapping menus usually subsume slot ink;
  // if many independent surfaces remain, collapse slots one at a time.
  for (let i = 0; uiRegions.length > 64 && i < slots.length; i++) {
    ink[i] = [hotbarRegions[i]];
    uiRegions = prune([...panels, ...ink.flat()]);
  }
  if (uiRegions.length > 64) {
    const tail = uiRegions.splice(63);
    const x = Math.min(...tail.map(r => r.x)), y = Math.min(...tail.map(r => r.y));
    uiRegions.push({ x, y, width: Math.max(...tail.map(r => r.x + r.width)) - x, height: Math.max(...tail.map(r => r.y + r.height)) - y });
  }
  return { uiRegions, hotbarRegions };
}

/** Keeps DOM placement in presentation while the platform adapter owns Tauri events. */
export async function subscribeNativeWindowGeometry(
  listener: () => void,
): Promise<() => void> {
  return subscribeApplicationWindowGeometry(listener);
}

/** Shared dialogs use a black theme backdrop. Combine the actual open owners;
 * removing the inner dialog must not remove the outer dialog's dimming. */
export function nativeBackdropOpacity(elements: HTMLElement[]): number {
  let transmission = 1;
  for (const element of elements) {
    const style = getComputedStyle(element);
    const color = style.backgroundColor;
    // Computed rgb()/rgba() serializes the existing --overlay token. Transparent
    // input-only surfaces (menus) do not request a visual backdrop.
    const match = color.match(/^rgba?\(\s*0[ ,]+0[ ,]+0(?:[ ,/]+([\d.]+)(%)?)?\s*\)$/);
    if (!match) continue;
    const alpha = match[1] === undefined ? 1 : Number(match[1]) / (match[2] ? 100 : 1);
    const opacity = Number(style.opacity);
    transmission *= 1 - Math.max(0, Math.min(1, alpha * (Number.isFinite(opacity) ? opacity : 1)));
  }
  return 1 - transmission;
}
