import { RefObject, useEffect, useMemo, useState } from "react";

export interface VirtualWindowResult<T> {
  visibleItems: T[];
  startIndex: number;
  topSpacerHeight: number;
  bottomSpacerHeight: number;
}

export function useVirtualWindow<T>(
  items: T[],
  containerRef: RefObject<HTMLElement | null>,
  rowHeight: number,
  overscan = 8,
): VirtualWindowResult<T> {
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;

    let frame = 0;
    const updateMetrics = () => {
      frame = 0;
      setScrollTop(element.scrollTop);
      setViewportHeight(element.clientHeight);
    };
    const scheduleUpdate = () => {
      if (frame) return;
      frame = window.requestAnimationFrame(updateMetrics);
    };

    updateMetrics();
    element.addEventListener("scroll", scheduleUpdate, { passive: true });
    const observer = new ResizeObserver(scheduleUpdate);
    observer.observe(element);

    return () => {
      element.removeEventListener("scroll", scheduleUpdate);
      observer.disconnect();
      if (frame) {
        window.cancelAnimationFrame(frame);
      }
    };
  }, [containerRef]);

  return useMemo(() => {
    if (!items.length || rowHeight <= 0) {
      return {
        visibleItems: [],
        startIndex: 0,
        topSpacerHeight: 0,
        bottomSpacerHeight: 0,
      };
    }

    const visibleCount = Math.max(1, Math.ceil(viewportHeight / rowHeight));
    const rawStart = Math.floor(scrollTop / rowHeight);
    const startIndex = Math.max(0, rawStart - overscan);
    const endIndex = Math.min(items.length, rawStart + visibleCount + overscan);
    const topSpacerHeight = startIndex * rowHeight;
    const bottomSpacerHeight = Math.max(0, (items.length - endIndex) * rowHeight);

    return {
      visibleItems: items.slice(startIndex, endIndex),
      startIndex,
      topSpacerHeight,
      bottomSpacerHeight,
    };
  }, [items, overscan, rowHeight, scrollTop, viewportHeight]);
}