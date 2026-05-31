import React, { useState, useRef, useEffect, useMemo, useLayoutEffect } from "react";
import { type EnumeratorCollection, loadEnumeratorCollections } from "../../../../../src/services/enumeratorService";
import { translateBlockId } from "../../../../../src/services/i18n";
import { BlockIcon } from "../../../../components/BlockIcon";
import { CreativeInventoryTooltip } from "./function";


export function CreativeInventoryDialog({
  onClose, quickbarSlots, onChangeQuickbarSlots,
}: {
  onClose: () => void;
  quickbarSlots: string[];
  onChangeQuickbarSlots: (updater: (current: string[]) => string[]) => void;
}) {
  const [collections, setCollections] = useState<EnumeratorCollection[]>([]);
  const [selectedCollectionId, setSelectedCollectionId] = useState("");
  const [search, setSearch] = useState("");
  const [pageStart, setPageStart] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [carryBlockId, setCarryBlockId] = useState("");
  const [carryPosition, setCarryPosition] = useState({ x: 0, y: 0 });
  const [hoverItem, setHoverItem] = useState<{ id: string; name: string; } | null>(null);
  const [hoverPosition, setHoverPosition] = useState({ x: 0, y: 0 });
  const carryPreviewRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    loadEnumeratorCollections()
      .then((allCollections) => {
        if (!active) return;
        const creativeCollections = allCollections
          .filter((collection) => (collection.category === "creative" || collection.id === "base:dv-blocks") && collection.values.length > 0
          )
          .sort((left, right) => {
            if (left.id === "base:dv-blocks") return -1;
            if (right.id === "base:dv-blocks") return 1;
            return left.name.localeCompare(right.name, "zh-CN");
          });
        setCollections(creativeCollections);
        setSelectedCollectionId((previous) => previous && creativeCollections.some((collection) => collection.id === previous)
          ? previous
          : (creativeCollections[0]?.id || ""));
      })
      .catch((nextError: any) => {
        if (!active) return;
        setCollections([]);
        setSelectedCollectionId("");
        setError(String(nextError || "创造模式枚举加载失败。"));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  const selectedCollection = useMemo(
    () => collections.find((collection) => collection.id === selectedCollectionId) || null,
    [collections, selectedCollectionId]
  );

  const filteredValues = useMemo(() => {
    const keyword = search.trim().toLowerCase();
    const source = selectedCollection?.values || [];
    if (!keyword) return source;
    return source.filter((blockId) => {
      const translated = translateBlockId(blockId).toLowerCase();
      return blockId.toLowerCase().includes(keyword) || translated.includes(keyword);
    });
  }, [search, selectedCollection]);

  const maxPageStart = useMemo(() => {
    if (filteredValues.length <= 45) return 0;
    return Math.floor((filteredValues.length - 1) / 9) * 9;
  }, [filteredValues.length]);

  const visibleValues = useMemo(() => {
    const page = filteredValues.slice(pageStart, pageStart + 45);
    const placeholders = Array.from({ length: Math.max(0, 45 - page.length) }, () => "");
    return [...page, ...placeholders];
  }, [filteredValues, pageStart]);

  const collectionOptions = useMemo(
    () => collections.map((collection) => ({ label: collection.name, value: collection.id })),
    [collections]
  );

  useEffect(() => {
    setPageStart(0);
  }, [selectedCollectionId, search]);

  useEffect(() => {
    setPageStart((current) => Math.min(current, maxPageStart));
  }, [maxPageStart]);

  useEffect(() => {
    if (!carryBlockId) return undefined;
    setHoverItem(null);
    const handlePointerMove = (event: MouseEvent) => {
      setCarryPosition({ x: event.clientX, y: event.clientY });
    };
    window.addEventListener("mousemove", handlePointerMove);
    return () => {
      window.removeEventListener("mousemove", handlePointerMove);
    };
  }, [carryBlockId]);

  useLayoutEffect(() => {
    const preview = carryPreviewRef.current;
    if (!preview || !carryBlockId) return;
    preview.style.setProperty("--flake-carry-x", `${carryPosition.x}px`);
    preview.style.setProperty("--flake-carry-y", `${carryPosition.y}px`);
  }, [carryBlockId, carryPosition.x, carryPosition.y]);

  const handleHoverItem = (blockId: string, event: React.MouseEvent<HTMLDivElement>) => {
    if (carryBlockId || !blockId) return;
    setHoverItem({ id: blockId, name: translateBlockId(blockId) });
    setHoverPosition({ x: event.clientX, y: event.clientY });
  };

  const handleHoverLeave = () => {
    setHoverItem(null);
  };

  const handleWheel = (event: React.WheelEvent<HTMLDivElement>) => {
    if (!filteredValues.length) return;
    event.preventDefault();
    setPageStart((current) => {
      if (event.deltaY > 0) return Math.min(maxPageStart, current + 9);
      if (event.deltaY < 0) return Math.max(0, current - 9);
      return current;
    });
  };

  const handleInventorySlotClick = (blockId: string, event: React.MouseEvent<HTMLDivElement>) => {
    event.stopPropagation();
    setHoverItem(null);
    if (!blockId) return;
    if (carryBlockId) {
      setCarryBlockId("");
      return;
    }
    setCarryBlockId(blockId);
    setCarryPosition({ x: event.clientX, y: event.clientY });
  };

  const handleCreativeHotbarSlotClick = (index: number, event: React.MouseEvent<HTMLDivElement>) => {
    event.stopPropagation();
    setHoverItem(null);
    const slotBlockId = quickbarSlots[index] || "";
    if (carryBlockId) {
      onChangeQuickbarSlots((current) => current.map((value, slotIndex) => slotIndex === index ? carryBlockId : value));
      setCarryBlockId("");
      return;
    }
    if (!slotBlockId) return;
    setCarryBlockId(slotBlockId);
    setCarryPosition({ x: event.clientX, y: event.clientY });
    onChangeQuickbarSlots((current) => current.map((value, slotIndex) => slotIndex === index ? "" : value));
  };

  const handleDropOutside = () => {
    setHoverItem(null);
    if (carryBlockId) setCarryBlockId("");
  };

  const rangeStart = filteredValues.length ? pageStart + 1 : 0;
  const rangeEnd = filteredValues.length ? Math.min(filteredValues.length, pageStart + 45) : 0;

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <section className="dialog-content flake-page__inventory-dialog" onClick={(event) => { event.stopPropagation(); handleDropOutside(); } }>
        <div className="subwindow-title-row">
          <div>
            <h3 className="subwindow-title">创造模式物品栏</h3>
            {/* <p className="subwindow-subtitle nova-muted">使用创造模式分类枚举作为数据源，滚轮按整行切换当前显示区间；点选方块会复制到鼠标上，再点快捷栏槽位即可放置。</p> */}
          </div>
          <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}>×</button>
        </div>
        <div className="flake-page__inventory-dialog-body">
          <div className="flake-page__creative-inventory-controls">
            <label className="flake-page__creative-inventory-control">
              <span className="nova-muted">分类</span>
              <select className="input" value={selectedCollectionId} onChange={(event) => setSelectedCollectionId(event.target.value)}>
                {collectionOptions.length === 0 ? <option value="">选择创造模式分类</option> : null}
                {collectionOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
              </select>
            </label>
            <label className="flake-page__creative-inventory-control flake-page__creative-inventory-search">
              <span className="nova-muted">搜索</span>
              <input className="input" value={search} placeholder="按方块名或 ID 过滤" onChange={(event) => setSearch(event.target.value)} />
            </label>
            <div className="flake-page__creative-inventory-range nova-muted nova-small">
              {loading ? "加载中..." : `${rangeStart}-${rangeEnd} / ${filteredValues.length}`}
            </div>
          </div>

          <div className="flake-page__creative-inventory-stage">
            <div className="flake-page__creative-inventory-surface" onWheel={handleWheel}>
              <div className="flake-page__creative-inventory-grid">
                {visibleValues.map((blockId, index) => blockId ? (
                  <div
                    key={`${blockId}-${index}`}
                    className={carryBlockId === blockId ? "flake-page__creative-slot is-carried-source" : "flake-page__creative-slot"}
                    role="button"
                    tabIndex={0}
                    onClick={(event) => handleInventorySlotClick(blockId, event)}
                    onMouseEnter={(event) => handleHoverItem(blockId, event)}
                    onMouseMove={(event) => handleHoverItem(blockId, event)}
                    onMouseLeave={handleHoverLeave}
                  >
                    <BlockIcon blockId={blockId} />
                  </div>
                ) : (
                  <div key={`empty-${index}`} className="flake-page__creative-slot is-empty" aria-hidden />
                ))}
              </div>

              <div className="flake-page__creative-hotbar-grid">
                {quickbarSlots.map((blockId, index) => {
                  return (
                    <div
                      key={`creative-hotbar-${index + 1}`}
                      className={blockId ? "flake-page__creative-hotbar-slot has-item" : "flake-page__creative-hotbar-slot"}
                      role="button"
                      tabIndex={0}
                      onClick={(event) => handleCreativeHotbarSlotClick(index, event)}
                      onMouseEnter={blockId ? (event) => handleHoverItem(blockId, event) : undefined}
                      onMouseMove={blockId ? (event) => handleHoverItem(blockId, event) : undefined}
                      onMouseLeave={blockId ? handleHoverLeave : undefined}
                    >
                      {blockId ? <BlockIcon blockId={blockId} /> : null}
                    </div>
                  );
                })}
              </div>

              {!loading && !error && !filteredValues.length ? (
                <div className="flake-page__creative-inventory-overlay nova-muted">没有可显示的方块。</div>
              ) : null}
              {loading ? <div className="flake-page__creative-inventory-overlay nova-muted">正在加载创造模式分类...</div> : null}
              {!loading && error ? <div className="flake-page__creative-inventory-overlay nova-error">{error}</div> : null}
            </div>
          </div>
        </div>
      </section>
      {carryBlockId ? (
        <div ref={carryPreviewRef} className="flake-page__creative-carry-preview">
          <BlockIcon blockId={carryBlockId} />
        </div>
      ) : null}
      {!carryBlockId ? <CreativeInventoryTooltip x={hoverPosition.x} y={hoverPosition.y} item={hoverItem} /> : null}
    </div>
  );
}
