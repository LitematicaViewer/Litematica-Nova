import React, { useEffect, useMemo, useRef, useState } from "react";
import { BlockIcon } from "./BlockIcon";
import { getAllBlocks, loadDatabases } from "../../src/business/facade";
import { initI18n, translateBlockId } from "../../src/business/facade";

export interface BlockSearchOption {
  id: string;
  label: string;
  shortId: string;
}

function normalizeBlockId(blockId: string): string {
  const trimmed = blockId.trim();
  if (!trimmed) return "minecraft:air";
  return trimmed.includes(":") ? trimmed : `minecraft:${trimmed}`;
}

function buildOption(id: string): BlockSearchOption {
  return {
    id,
    label: translateBlockId(id),
    shortId: id.replace("minecraft:", ""),
  };
}

export function BlockSearchSelect({
  value,
  onChange,
  disabled = false,
}: {
  value: string;
  onChange: (blockId: string) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [blocks, setBlocks] = useState<string[]>(() => getAllBlocks().sort());
  const [activeIndex, setActiveIndex] = useState(0);
  const closeTimer = useRef<number | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([loadDatabases(), initI18n()]).then(() => {
      if (active) setBlocks(getAllBlocks().sort());
    });
    return () => {
      active = false;
    };
  }, []);

  const currentId = normalizeBlockId(value);
  const current = useMemo(() => buildOption(currentId), [currentId]);
  const options = useMemo(() => blocks.map(buildOption), [blocks]);
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return options.slice(0, 160);
    return options
      .filter((option) => {
        return (
          option.id.toLowerCase().includes(needle) ||
          option.shortId.toLowerCase().includes(needle) ||
          option.label.toLowerCase().includes(needle)
        );
      })
      .slice(0, 160);
  }, [options, query]);

  useEffect(() => {
    if (!open) return;
    const index = filtered.findIndex((option) => option.id === currentId);
    setActiveIndex(index >= 0 ? index : 0);
  }, [open, filtered, currentId]);

  useEffect(() => {
    const active = listRef.current?.querySelector<HTMLElement>(`[data-active="true"]`);
    active?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  const commit = (blockId: string) => {
    onChange(blockId);
    setQuery("");
    setOpen(false);
  };

  const closeSoon = () => {
    if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(() => setOpen(false), 120);
  };

  const cancelClose = () => {
    if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (disabled) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setOpen(true);
      setActiveIndex((prev) => Math.min(prev + 1, Math.max(filtered.length - 1, 0)));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setOpen(true);
      setActiveIndex((prev) => Math.max(prev - 1, 0));
    } else if (event.key === "Enter") {
      if (open && filtered[activeIndex]) {
        event.preventDefault();
        commit(filtered[activeIndex].id);
      }
    } else if (event.key === "Escape") {
      event.preventDefault();
      setOpen(false);
      setQuery("");
    }
  };

  return (
    <div className="block-search-select" onMouseDown={cancelClose}>
      <button
        className="block-search-button input"
        type="button"
        disabled={disabled}
        onClick={() => setOpen(true)}
      >
        <BlockIcon blockId={currentId} />
        <span className="block-search-current">
          <strong>{current.label}</strong>
          <span>{current.id}</span>
        </span>
      </button>
      {open && (
        <div className="block-search-menu" onMouseDown={(event) => event.preventDefault()}>
          <input
            className="input block-search-input"
            autoFocus
            value={query}
            placeholder="搜索中文名 / minecraft:id / 短 ID"
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={handleKeyDown}
            onBlur={closeSoon}
          />
          {blocks.length === 0 ? (
            <div className="block-search-empty">
              BlockState 数据库未加载，仍可在下方手动输入 ID。
            </div>
          ) : (
            <div
              ref={listRef}
              className="block-search-list"
              onWheel={(event) => event.stopPropagation()}
            >
              {filtered.map((option, index) => (
                <button
                  key={option.id}
                  type="button"
                  data-active={index === activeIndex}
                  className={`block-search-item ${index === activeIndex ? "active" : ""}`}
                  onMouseEnter={() => setActiveIndex(index)}
                  onMouseDown={(event) => {
                    event.preventDefault();
                    commit(option.id);
                  }}
                >
                  <BlockIcon blockId={option.id} />
                  <span>
                    <strong>{option.label}</strong>
                    <small>{option.id}</small>
                  </span>
                </button>
              ))}
              {filtered.length === 0 && <div className="block-search-empty">没有匹配方块</div>}
            </div>
          )}
        </div>
      )}
    </div>
  );
}


