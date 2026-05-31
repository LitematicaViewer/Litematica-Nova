import { useLayoutEffect, useRef } from "react";

export function VirtualSpacerCell({
  colSpan,
  height,
}: {
  colSpan: number;
  height: number;
}) {
  const cellRef = useRef<HTMLTableCellElement | null>(null);

  useLayoutEffect(() => {
    const cell = cellRef.current;
    if (!cell) return;
    cell.style.setProperty("--material-list-virtual-spacer-height", `${Math.max(0, height)}px`);
  }, [height]);

  return <td ref={cellRef} colSpan={colSpan} className="material-list-virtual-spacer-cell" />;
}