import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import React, { useRef, useState, useLayoutEffect } from "react";
import { BlockIcon } from "../../../../components/BlockIcon";
import type { FlakeHoverBlock } from "./FlakePage";
import { translateBlockId } from "../../../../../src/business/facade";

/**
 * 格式化状态记录。
 * @param record 状态记录
 * @returns 格式化后的状态记录
 */
export function formatStateRecord(record: Record<string, unknown>): string {
  return Object.entries(record)
    .filter(([key]) => key)
    .map(([key, value]) => `${key}=${String(value)}`)
    .join(", ");
}

/**
 * 方块悬浮提示。
 * @param x 悬浮提示的x坐标
 * @param y 悬浮提示的y坐标
 * @param item 悬浮提示的方块
 * @returns 悬浮提示的组件
 */
export function FlakeBlockTooltip({
  x,
  y,
  item,
  highlightEmptyState = false,
}: {
  x: number;
  y: number;
  item: FlakeHoverBlock | null;
  highlightEmptyState?: boolean;
}) {
  const { locale } = useI18n();
  const popupRef = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState(() => ({ left: x + 14, top: y + 14 }));

  useLayoutEffect(() => {
    if (!item) return;
    const popup = popupRef.current;
    const offset = 14;
    const margin = 4;
    if (!popup) {
      setPosition({ left: x + offset, top: y + offset });
      return;
    }

    const rect = popup.getBoundingClientRect();
    let left = x + offset;
    let top = y + offset;

    if (left + rect.width > window.innerWidth - margin) {
      left = x - rect.width - offset;
    }
    if (top + rect.height > window.innerHeight - margin) {
      top = y - rect.height - offset;
    }

    left = Math.max(margin, Math.min(left, window.innerWidth - rect.width - margin));
    top = Math.max(margin, Math.min(top, window.innerHeight - rect.height - margin));
    setPosition((previous) => previous.left === left && previous.top === top ? previous : { left, top });
  }, [x, y, item, locale]);

  useLayoutEffect(() => {
    const popup = popupRef.current;
    if (!popup) return;
    popup.style.setProperty("--material-list-popup-left", `${position.left}px`);
    popup.style.setProperty("--material-list-popup-top", `${position.top}px`);
  }, [position.left, position.top]);

  if (!item) return null;

  return (
    <div ref={popupRef} className="material-list-hover-popup" role="tooltip">
      <div className="material-list-hover-popup-row material-list-hover-popup-item-row">
        <BlockIcon blockId={item.id} />
        <span className="material-list-hover-popup-name">{item.name}</span>
      </div>
      <div className="material-list-hover-popup-row">{t("flake.blockIdLabel")} {item.id}</div>
      <div className={highlightEmptyState && !item.hasStates ? "material-list-hover-popup-row flake-page__empty-state-row" : "material-list-hover-popup-row"}>{t("flake.blockStatesLabel")} {item.hasStates ? item.states : t("editor.none")}</div>
      <div className="material-list-hover-popup-row">x={item.worldX} y={item.worldY} z={item.worldZ}</div>
      <div className="material-list-hover-popup-row">{t("flake.region")} {item.region}</div>
    </div>
  );
}

/**
 * 创意库存悬浮提示。
 * @param x 悬浮提示的x坐标
 * @param y 悬浮提示的y坐标
 * @param item 悬浮提示的方块
 * @returns 悬浮提示的组件
 */
export function CreativeInventoryTooltip({ x, y, item }: { x: number; y: number; item: { id: string; name: string; } | null; }) {
  const { locale } = useI18n();
  const popupRef = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState(() => ({ left: x + 14, top: y + 14 }));

  useLayoutEffect(() => {
    if (!item) return;
    const popup = popupRef.current;
    const offset = 14;
    const margin = 4;
    if (!popup) {
      setPosition({ left: x + offset, top: y + offset });
      return;
    }

    const rect = popup.getBoundingClientRect();
    let left = x + offset;
    let top = y + offset;

    if (left + rect.width > window.innerWidth - margin) {
      left = x - rect.width - offset;
    }
    if (top + rect.height > window.innerHeight - margin) {
      top = y - rect.height - offset;
    }

    left = Math.max(margin, Math.min(left, window.innerWidth - rect.width - margin));
    top = Math.max(margin, Math.min(top, window.innerHeight - rect.height - margin));
    setPosition((previous) => previous.left === left && previous.top === top ? previous : { left, top });
  }, [x, y, item, locale]);

  useLayoutEffect(() => {
    const popup = popupRef.current;
    if (!popup) return;
    popup.style.setProperty("--material-list-popup-left", `${position.left}px`);
    popup.style.setProperty("--material-list-popup-top", `${position.top}px`);
  }, [position.left, position.top]);

  if (!item) return null;

  return (
    <div ref={popupRef} className="material-list-hover-popup" role="tooltip">
      <div className="material-list-hover-popup-row">
        <strong className="material-list-hover-popup-name">{translateBlockId(item.id)}</strong>
      </div>
      <div className="material-list-hover-popup-row">{t("flake.blockIdLabel")} {item.id}</div>
    </div>
  );
}
