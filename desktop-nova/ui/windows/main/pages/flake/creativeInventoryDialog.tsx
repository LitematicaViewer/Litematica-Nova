import { Dropdown } from "../../../../components/Dropdown";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import React, { useState, useRef, useEffect, useMemo, useLayoutEffect } from "react";
import { type EnumeratorCollection, loadEnumeratorCollections } from "../../../../../src/business/facade";
import { translateBlockId } from "../../../../../src/business/facade";
import { BlockIcon } from "../../../../components/BlockIcon";
import { CreativeInventoryTooltip } from "./function";


export function CreativeInventoryDialog({
  onClose, quickbarSlots, onChangeQuickbarSlots, extraCollections = [],
}: {
  onClose: () => void;
  quickbarSlots: string[];
  onChangeQuickbarSlots: (updater: (current: string[]) => string[]) => void;
  /** 额外注入的分类（如"当前统计结果"），显示在分类列表顶部。 */
  extraCollections?: { id: string; name: string; values: string[] }[];
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
          .filter((collection) => (collection.category === "creative" || collection.id === "base:dv-blocks") && collection.values.length > 0)
          .sort((left, right) => {
            if (left.id === "base:dv-blocks") return -1;
            if (right.id === "base:dv-blocks") return 1;
            return left.name.localeCompare(right.name, "zh-CN");
          });
        setCollections(creativeCollections);
        setSelectedCollectionId((previous) => {
          const allIds = new Set([
            ...extraCollections.map((c) => c.id),
            ...creativeCollections.map((c) => c.id),
          ]);
          if (previous && allIds.has(previous)) return previous;
          // 优先选中第一个额外分类
          return extraCollections.length > 0
            ? extraCollections[0].id
            : (creativeCollections[0]?.id || "");
        });
      })
      .catch((nextError: any) => {
        if (!active) return;
        setCollections([]);
        setSelectedCollectionId(extraCollections.length > 0 ? extraCollections[0].id : "");
        setError(String(nextError || "创造模式枚举加载失败。"));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  // extraCollections 在组件生命周期内不变，安全忽略
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** 合并后的全部分类：额外分类在前，枚举分类在后。 */
  const allCollections = useMemo(
    () => [
      ...extraCollections.map((c) => ({ id: c.id, name: c.name, values: c.values })),
      ...collections.map((c) => ({ id: c.id, name: c.name, values: c.values })),
    ],
    [collections, extraCollections]
  );

  const selectedCollection = useMemo(
    () => allCollections.find((collection) => collection.id === selectedCollectionId) || null,
    [allCollections, selectedCollectionId]
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
    () => allCollections.map((collection) => ({ label: collection.name, value: collection.id })),
    [allCollections]
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

  const handleHoverItem = (blockId: string, event: React.MouseEvent<HTMLButtonElement>) => {
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

  const handleInventorySlotClick = (blockId: string, event: React.MouseEvent<HTMLButtonElement>) => {
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

  const handleCreativeHotbarSlotClick = (index: number, event: React.MouseEvent<HTMLButtonElement>) => {
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
    <Dialog title="创造模式物品栏" onClose={onClose} width="lg" className="flake-page__inventory-dialog">
      <section onClick={handleDropOutside}>
        <div className="flake-page__inventory-dialog-body">
          <div className="flake-page__creative-inventory-controls">
            <label className="flake-page__creative-inventory-control">
              <span className="nova-muted">分类</span>
              <Dropdown value={selectedCollectionId} onChange={setSelectedCollectionId} options={collectionOptions} ariaLabel="创造模式分类" />
            </label>
            <label className="flake-page__creative-inventory-control flake-page__creative-inventory-search">
              <span className="nova-muted">搜索</span>
              <Input className="input" value={search} placeholder="按方块名或 ID 过滤" onChange={(event) => setSearch(event.target.value)} />
            </label>
            <div className="flake-page__creative-inventory-range nova-muted nova-small">
              {loading ? "加载中..." : `${rangeStart}-${rangeEnd} / ${filteredValues.length}`}
            </div>
          </div>

          <div className="flake-page__creative-inventory-stage">
            <div className="flake-page__creative-inventory-surface" onWheel={handleWheel}>
              <div className="flake-page__creative-inventory-grid">
                {visibleValues.map((blockId, index) => blockId ? (
                  <Button variant="ghost"
                    key={`${blockId}-${index}`}
                    className={carryBlockId === blockId ? "flake-page__creative-slot is-carried-source" : "flake-page__creative-slot"}
                    aria-label={translateBlockId(blockId)}
                    onClick={(event) => handleInventorySlotClick(blockId, event)}
                    onMouseEnter={(event) => handleHoverItem(blockId, event)}
                    onMouseMove={(event) => handleHoverItem(blockId, event)}
                    onMouseLeave={handleHoverLeave}
                  >
                    <BlockIcon blockId={blockId} />
                  </Button>
                ) : (
                  <div key={`empty-${index}`} className="flake-page__creative-slot is-empty" aria-hidden />
                ))}
              </div>

              <div className="flake-page__creative-hotbar-grid">
                {quickbarSlots.map((blockId, index) => (
                  <Button variant="ghost"
                    key={`creative-hotbar-${index + 1}`}
                    className={blockId ? "flake-page__creative-hotbar-slot has-item" : "flake-page__creative-hotbar-slot"}
                    aria-label={`快捷栏 ${index + 1}${blockId ? `：${translateBlockId(blockId)}` : "：空"}`}
                    onClick={(event) => handleCreativeHotbarSlotClick(index, event)}
                    onMouseEnter={blockId ? (event) => handleHoverItem(blockId, event) : undefined}
                    onMouseMove={blockId ? (event) => handleHoverItem(blockId, event) : undefined}
                    onMouseLeave={blockId ? handleHoverLeave : undefined}
                  >
                    {blockId ? <BlockIcon blockId={blockId} /> : null}
                  </Button>
                ))}
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
    </Dialog>
  );
}
