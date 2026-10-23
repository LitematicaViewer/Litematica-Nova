import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
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

const CONTAINER_LAYOUTS: Record<ContainerType, { titleKey: string; slotCount: number }> = {
  chest: { titleKey: "flake.container.chest", slotCount: 27 },
  shulker_box: { titleKey: "flake.container.shulker", slotCount: 27 },
  barrel: { titleKey: "flake.container.barrel", slotCount: 27 },
  hopper: { titleKey: "flake.container.hopper", slotCount: 5 },
};

/**
 * Renders a container inventory dialog for flake block inspection.
 */
export function ContainerDialog({ onClose, containerType, items, position }: ContainerDialogProps) {
  useI18n();
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
    <Dialog title={t(layout.titleKey)} subtitle={t("flake.containerPosition", {x:position.x,y:position.y,z:position.z})} onClose={onClose} className="flake-page__container-dialog" width="sm">

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
