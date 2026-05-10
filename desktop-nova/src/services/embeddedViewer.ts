export type { EmbeddedRect, EmbeddedViewerPurpose, EmbeddedViewerStatus } from "../platform/embeddedViewer";
export {
  getEmbeddedViewerStatus,
  hideEmbeddedViewer,
  showEmbeddedViewer,
  startEmbeddedViewer,
  stopEmbeddedViewer,
  updateEmbeddedViewerBounds,
} from "../platform/embeddedViewer";

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

export function elementToCssRect(element: HTMLElement) {
  const rect = element.getBoundingClientRect();
  return {
    x: Math.round(rect.left),
    y: Math.round(rect.top),
    width: Math.round(rect.width),
    height: Math.round(rect.height),
  };
}

export function isUsableEmbeddedRect(rect: { width: number; height: number }): boolean {
  return rect.width > 8 && rect.height > 8;
}
