import { useLayoutEffect, useRef } from "react";

/**
 * The custom property through which a virtualized list hands its spacer geometry
 * to the stylesheet.
 *
 * The `height` declaration itself lives in CSS (`materials.css` for the summary
 * list and the detail table, `enumerator.css` for the enumerator window), so
 * markup only supplies the value and never declares a style property inline.
 * Both stylesheets must keep a `height: var(--material-list-virtual-spacer-height)`
 * rule for any surface that renders these spacers.
 */
const VIRTUAL_SPACER_HEIGHT_PROPERTY = "--material-list-virtual-spacer-height";

/**
 * Applies the resolved spacer height before the browser paints, so the spacer is
 * never briefly laid out at `auto` height.
 */
function useSpacerHeight<T extends HTMLElement>(height: number) {
  const ref = useRef<T | null>(null);
  const resolvedHeight = Math.max(0, height);

  useLayoutEffect(() => {
    ref.current?.style.setProperty(VIRTUAL_SPACER_HEIGHT_PROPERTY, `${resolvedHeight}px`);
  }, [resolvedHeight]);

  return ref;
}

/**
 * Zero-content table cell that reserves vertical space so a virtualized table
 * keeps the full scroll height of the list.
 *
 * A spacer that fails to take its height makes the scroll container's total
 * height follow the number of rendered rows only; the container then clamps
 * `scrollTop`, the scroll listener feeds the clamped value back into state and
 * the list oscillates between two offsets.
 */
export function VirtualSpacerCell({ colSpan, height }: { colSpan: number; height: number }) {
  const ref = useSpacerHeight<HTMLTableCellElement>(height);
  return <td ref={ref} colSpan={colSpan} className="material-list-virtual-spacer-cell" />;
}

/**
 * List-item twin of {@link VirtualSpacerCell} for virtualized `<ul>` lists,
 * which cannot use a table cell to reserve space.
 */
export function VirtualSpacerItem({ className, height }: { className: string; height: number }) {
  const ref = useSpacerHeight<HTMLLIElement>(height);
  return <li ref={ref} className={className} aria-hidden />;
}
