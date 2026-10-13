import { Dialog } from "../../../../components/Dialog";
import React, { useMemo } from "react";
import { BlockIcon } from "../../../../components/BlockIcon";
import { translateBlockId } from "../../../../../src/business/facade";
import type { ContainerType } from "../../../../../src/business/facade";

interface ContainerItem {
  id: string;
  count: number;
  slot: number;
}

interface ContainerDialogProps {
  onClose: () => void;
  containerType: ContainerType;
  items: ContainerItem[];
  position: { x: number; y: number; z: number };
}

const CONTAINER_LAYOUTS: Record<ContainerType, { title: string; slotCount: number }> = {
  chest: { title: "箱子", slotCount: 27 },
  shulker_box: { title: "潜影盒", slotCount: 27 },
  barrel: { title: "木桶", slotCount: 27 },
  hopper: { title: "漏斗", slotCount: 5 },
};

/**
 * Renders a container inventory dialog for flake block inspection.
 */
export function ContainerDialog({ onClose, containerType, items, position }: ContainerDialogProps) {
  const layout = CONTAINER_LAYOUTS[containerType];
  const slots = useMemo(() => {
    const result: Array<ContainerItem | null> = Array(layout.slotCount).fill(null);
    items.forEach((item) => {
      if (item.slot >= 0 && item.slot < layout.slotCount) {
        result[item.slot] = item;
      }
    });
    return result;
  }, [items, layout.slotCount]);

  return (
    <Dialog title={layout.title} subtitle={`位置：(${position.x}, ${position.y}, ${position.z})`} onClose={onClose} className="flake-page__container-dialog" width="sm">

        <div className={`flake-page__container-body flake-page__container-body--${containerType}`}>
          <div className={`flake-page__container-grid flake-page__container-grid--${containerType}`}>
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
    </Dialog>
  );
}
