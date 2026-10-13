import { useLayoutEffect, useRef } from "react";

/**
 * Zero-content table cell that reserves vertical space so a virtualized table
 * keeps the full scroll height of the list.
 *
 * The height is applied twice on purpose:
 *   1. an inline `height` style, so the spacer works even if the stylesheet that
 *      defines `.material-list-virtual-spacer-cell` is missing (that rule lived
 *      in the pre-merge `ui/shell/widget/subwindow.css` and was lost when the
 *      stylesheet was swapped), and
 *   2. the `--material-list-virtual-spacer-height` custom property, so existing
 *      stylesheets keep working unchanged.
 *
 * A spacer that fails to take its height makes the scroll container's total
 * height follow the number of rendered rows only; the container then clamps
 * `scrollTop`, the scroll listener feeds the clamped value back into state and
 * the list oscillates between two offsets.
 */
export function VirtualSpacerCell({
  colSpan,
  height,
}: {
  colSpan: number;
  height: number;
}) {
  const cellRef = useRef<HTMLTableCellElement | null>(null);
  const resolvedHeight = Math.max(0, height);

  useLayoutEffect(() => {
    const cell = cellRef.current;
    if (!cell) return;
    cell.style.setProperty("--material-list-virtual-spacer-height", `${resolvedHeight}px`);
  }, [resolvedHeight]);

  return (
    <td
      ref={cellRef}
      colSpan={colSpan}
      className="material-list-virtual-spacer-cell"
      style={{ height: `${resolvedHeight}px` }}
    />
  );
}
