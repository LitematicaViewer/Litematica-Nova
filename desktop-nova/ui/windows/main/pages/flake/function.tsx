import React, { useRef, useState, useLayoutEffect } from "react";
import { BlockIcon } from "../../../../components/BlockIcon";
import { FlakeHoverBlock } from "./FlakePage";
import { LayerSliceMeta } from "../../../../../src/services/layerService";

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
 * 格式化状态值。
 * @param value 状态值
 * @returns 格式化后的状态值
 */
export function formatStateValue(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (Array.isArray(value)) {
    return value.map((entry) => formatStateValue(entry)).filter(Boolean).join(", ");
  }
  if (typeof value === "object") {
    const row = value as Record<string, unknown>;
    const namedKey = ["name", "key", "property", "prop"].find((key) => typeof row[key] === "string");
    const valueKey = ["value", "val"].find((key) => row[key] !== undefined);
    if (namedKey && valueKey) {
      return `${String(row[namedKey])}=${String(row[valueKey])}`;
    }
    return formatStateRecord(row);
  }
  return "";
}

/**
 * 解析层块状态。
 * @param entry 层块状态
 * @param propertyPool 属性池
 * @returns 解析后的层块状态
 */
export function resolveLayerBlockStates(entry: any, propertyPool: any[]): string {
  if (!entry) return "无";

  const parts: string[] = [];
  const push = (value: unknown) => {
    const text = formatStateValue(value);
    if (text) parts.push(text);
  };

  if (typeof entry.property_id === "number" && entry.property_id >= 0 && entry.property_id < propertyPool.length) {
    push(propertyPool[entry.property_id]);
  }

  if (entry.block_state) push(entry.block_state);
  if (entry.state) push(entry.state);
  if (entry.states) push(entry.states);
  if (entry.properties) push(entry.properties);

  const propertyRefKeys = ["property_ids", "property_indices", "property_refs", "state_ids", "state_indices"];
  for (const key of propertyRefKeys) {
    const refs = entry[key];
    if (!Array.isArray(refs)) continue;
    for (const ref of refs) {
      if (typeof ref === "number" && ref >= 0 && ref < propertyPool.length) {
        push(propertyPool[ref]);
      } else {
        push(ref);
      }
    }
  }

  const normalized = Array.from(new Set(parts.flatMap((part) => part.split(",").map((item) => item.trim()).filter(Boolean))));
  return normalized.length ? normalized.join(", ") : "无";
}

/**
 * 方块悬浮提示。
 * @param x 悬浮提示的x坐标
 * @param y 悬浮提示的y坐标
 * @param item 悬浮提示的方块
 * @returns 悬浮提示的组件
 */
export function FlakeBlockTooltip({ x, y, item }: { x: number; y: number; item: FlakeHoverBlock | null; }) {
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
  }, [x, y, item]);

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
      <div className="material-list-hover-popup-row">方块ID：{item.id}</div>
      <div className="material-list-hover-popup-row">方块状态：{item.states}</div>
      <div className="material-list-hover-popup-row">x={item.x} y={item.y} z={item.z}</div>
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
  }, [x, y, item]);

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
        <strong className="material-list-hover-popup-name">{item.name}</strong>
      </div>
      <div className="material-list-hover-popup-row">方块ID：{item.id}</div>
    </div>
  );
}

/**
 * 适应视图。
 * @param meta 层切片元数据
 * @param viewport 视图元素
 * @param setScale 设置缩放比例
 * @param setOffset 设置偏移量
 */
export function fitView(
  meta: LayerSliceMeta,
  viewport: HTMLElement | null,
  setScale: (scale: number) => void,
  setOffset: (offset: { x: number; y: number; }) => void) {
  const width = viewport?.parentElement?.clientWidth || viewport?.clientWidth || 600;
  const height = viewport?.parentElement?.clientHeight || viewport?.clientHeight || 600;
  const maxDim = Math.max(meta.size_x, meta.size_z);
  if (maxDim <= 0) return;
  const initialScale = Math.min(10, Math.max(0.5, (Math.min(width, height) * 0.8) / maxDim));
  setScale(initialScale);
  setOffset({
    x: width / 2 - (meta.size_x * initialScale) / 2,
    y: height / 2 - (meta.size_z * initialScale) / 2,
  });
}


