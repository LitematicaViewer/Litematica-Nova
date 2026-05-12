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
