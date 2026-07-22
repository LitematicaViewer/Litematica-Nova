import React, { useMemo } from "react";
import { BlockIcon } from "../../../../components/BlockIcon";
import { translateBlockId } from "../../../../../src/services/i18n";

interface ContainerItem {
  id: string;
  count: number;
  slot: number;
}

interface ContainerDialogProps {
  onClose: () => void;
  containerType: "chest" | "shulker_box" | "barrel";
  items: ContainerItem[];
  position: { x: number; y: number; z: number };
}

/**
 * Renders a container inventory dialog for flake block inspection.
 */
export function ContainerDialog({ onClose, containerType, items, position }: ContainerDialogProps) {
  // 27个物品槽，3行9列
  const slots = useMemo(() => {
    const result: Array<ContainerItem | null> = Array(27).fill(null);
    items.forEach((item) => {
      if (item.slot >= 0 && item.slot < 27) {
        result[item.slot] = item;
      }
    });
    return result;
  }, [items]);

  const containerTitle = containerType === "chest" ? "箱子" : containerType === "barrel" ? "木桶" : "潜影盒";

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <section
        className="dialog-content flake-page__container-dialog"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="subwindow-title-row">
          <div>
            <h3 className="subwindow-title">{containerTitle}</h3>
            <p className="subwindow-subtitle nova-muted">
              位置: ({position.x}, {position.y}, {position.z})
            </p>
          </div>
          <button
            className="btn subwindow-close-button"
            type="button"
            aria-label="关闭窗口"
            onClick={onClose}
          >
            ×
          </button>
        </div>

        <div className="flake-page__container-body">
          <div className="flake-page__container-grid">
            {slots.map((item, index) => {
              return (
                <div
                  key={index}
                  className="flake-page__container-slot"
                  title={item ? `${translateBlockId(item.id)} x${item.count}` : undefined}
                >
                  {item ? (
                    <>
                      <BlockIcon blockId={item.id} />
                      {item.count > 1 ? (
                        <span className="flake-page__container-slot-count">{item.count}</span>
                      ) : null}
                    </>
                  ) : null}
                </div>
              );
            })}
          </div>
        </div>
      </section>
    </div>
  );
}
