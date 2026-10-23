import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../src/services/i18n";
import { useI18n } from "../i18n";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Dropdown } from "./Dropdown";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { Popover } from "radix-ui";
import { BlockIcon } from "./BlockIcon";
import { getAllBlocks, loadDatabases, getBlockProperties, translateKey, translateValue } from "../../src/business/facade";
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
  allowedBlocks,
  blockNames,
}: {
  value: string;
  onChange: (blockId: string) => void;
  disabled?: boolean;
  allowedBlocks?: string[];
  blockNames?: Record<string, string>;
}) {
  const { revision } = useI18n();
  const [loadError, setLoadError] = useState("");
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [blocks, setBlocks] = useState<string[]>(() => getAllBlocks().sort());
  const [activeIndex, setActiveIndex] = useState(0);
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (allowedBlocks) { setBlocks([...allowedBlocks].sort()); return; }
    let active = true;
    Promise.all([loadDatabases(), initI18n()]).then(() => {
      if (active) setBlocks(getAllBlocks().sort());
    }).catch((error) => { if (active) setLoadError(String(error)); });
    return () => {
      active = false;
    };
  }, [allowedBlocks]);

  const currentId = normalizeBlockId(value);
  const current = useMemo(() => ({ ...buildOption(currentId), label: blockNames?.[currentId] || translateBlockId(currentId) }), [currentId, blockNames, blocks, revision]);
  const options = useMemo(() => blocks.map((id) => ({ ...buildOption(id), label: blockNames?.[id] || translateBlockId(id) })), [blocks, blockNames, revision]);
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
    <Popover.Root open={open} onOpenChange={setOpen}>
      <div className="block-search-select">
        <Popover.Trigger asChild>
          <Button
            className="block-search-button input"
            type="button"
            disabled={disabled}
          >
            <BlockIcon blockId={currentId} />
            <span className="block-search-current">
              <strong>{current.label}</strong>
              <span>{current.id}</span>
            </span>
          </Button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content
            className="block-search-menu"
            data-slot="block-search-content"
            aria-label={t("library.chooseBlock")}
            align="start"
            sideOffset={4}
            collisionPadding={8}
          >
            <Input
              className="input block-search-input"
              autoFocus
              aria-label={t("library.searchBlocks")}
              value={query}
              placeholder={t("library.blockSearchPlaceholder")}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={handleKeyDown}
            />
            {blocks.length === 0 ? (
              <div className="block-search-empty">
                {localizeMessage(loadError) || t("library.blocksLoading")}
              </div>
            ) : (
              <div
                ref={listRef}
                className="block-search-list"
                onWheel={(event) => event.stopPropagation()}
              >
                {filtered.map((option, index) => (
                  <Button
                    key={option.id}
                    type="button"
                    tabIndex={-1}
                    data-active={index === activeIndex}
                    className={`block-search-item ${index === activeIndex ? "active" : ""}`}
                    onMouseEnter={() => setActiveIndex(index)}
                    onMouseDown={(event) => {
                      event.preventDefault();
                    }}
                    onClick={() => commit(option.id)}
                  >
                    <BlockIcon blockId={option.id} />
                    <span>
                      <strong>{option.label}</strong>
                      <small>{option.id}</small>
                    </span>
                  </Button>
                ))}
                {filtered.length === 0 && <div className="block-search-empty">{t("library.noMatchingBlocks")}</div>}
              </div>
            )}
          </Popover.Content>
        </Popover.Portal>
      </div>
    </Popover.Root>
  );
}




/** Shared state editor used by block replacement and cross-version conversion. */
export function BlockPropertySelector({ blockId, value, onChange, isTo, properties, exact = false }: {
  blockId: string; value: Record<string, string>; onChange: (value: Record<string, string>) => void;
  isTo: boolean; properties?: Record<string, string[]>; exact?: boolean;
}) {
  useI18n();
  const props = properties || getBlockProperties(blockId);
  const keys = Object.keys(props).sort();
  if (!keys.length) return null;
  return <div className="replace-property-list">{keys.map((key) => <div className="replace-property" key={key}>
    <span>{translateKey(key)}</span>
    <Dropdown ariaLabel={translateKey(key)} value={value[key] || (exact ? props[key][0] : isTo ? "$keep" : "")} options={[
      ...(exact ? [] : isTo ? [{ label: t("library.keepOriginalState"), value: "$keep" }] : [{ label: t("library.allStatuses"), value: "" }]),
      ...props[key].map((item) => ({ label: translateValue(key, item), value: item })),
    ]} onChange={(item) => { const next = { ...value }; if (!item) delete next[key]; else next[key] = item; onChange(next); }} />
  </div>)}</div>;
}
