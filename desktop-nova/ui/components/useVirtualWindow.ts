import { RefObject, useEffect, useMemo, useState } from "react";

export interface VirtualWindowResult<T> {
  visibleItems: T[];
  startIndex: number;
  topSpacerHeight: number;
  bottomSpacerHeight: number;
}

/**
 * Virtualises a list inside a scroll container.
 *
 * `rowHeight` must be the height the browser actually lays out for one row, not
 * a round number: the spacer heights are derived from it, so a mismatch makes
 * the container's total height disagree with the rows it renders. A table's
 * sticky header is measured separately and subtracted from the scroll offset,
 * because the first row starts below it.
 */
export function useVirtualWindow<T>(
  items: T[],
  containerRef: RefObject<HTMLElement | null>,
  rowHeight: number,
  overscan = 8,
): VirtualWindowResult<T> {
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [leadingInset, setLeadingInset] = useState(0);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;

    let frame = 0;
    const updateMetrics = () => {
      frame = 0;
      // A `<thead>` (virtualized tables keep it sticky) occupies the first
      // pixels of the scroll range; without subtracting it the offset maps to a
      // row that is almost one full row too far down.
      const header = element.querySelector<HTMLElement>("thead");
      setScrollTop(element.scrollTop);
      setViewportHeight(element.clientHeight);
      setLeadingInset(header ? header.getBoundingClientRect().height : 0);
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
    const scrolledRows = Math.max(0, scrollTop - leadingInset);
    const rawStart = Math.floor(scrolledRows / rowHeight);
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
  }, [items, leadingInset, overscan, rowHeight, scrollTop, viewportHeight]);
}
