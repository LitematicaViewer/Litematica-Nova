import { t } from "../../src/services/i18n";
import { useI18n } from "../i18n";
import React, { useEffect, useId, useMemo, useRef, useState } from "react";
import { Popover } from "radix-ui";
import { ChevronDown } from "lucide-react";
import { Input } from "./ui/input";

export interface EditableDropdownOption {
  label: string;
  value: string;
}

/**
 * Combobox for values that users may type as well as pick.
 *
 * The field is the control: typing filters the list, `Enter` commits either the
 * highlighted option or the raw text, and losing focus commits what is in the
 * field. It is deliberately separate from `Dropdown` so that the Radix Select
 * route stays a thin, selection-only wrapper.
 */
export function EditableDropdown({
  value,
  options,
  onChange,
  disabled = false,
  ariaLabel,
  id,
  className = "",
  placeholder,
}: {
  value: string;
  options: EditableDropdownOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  ariaLabel?: string;
  id?: string;
  className?: string;
  placeholder?: string;
}) {
  useI18n();
  const listId = useId();
  const listRef = useRef<HTMLDivElement | null>(null);
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(value);
  const [activeIndex, setActiveIndex] = useState(0);

  const selected = options.find((option) => option.value === value);
  const selectedLabel = selected?.label ?? value;

  useEffect(() => { setDraft(selectedLabel); }, [selectedLabel]);

  const query = draft.trim().toLowerCase();
  const filtered = useMemo(() => {
    if (!query || query === selectedLabel.trim().toLowerCase()) return options;
    return options.filter((option) =>
      option.label.toLowerCase().includes(query) || option.value.toLowerCase().includes(query));
  }, [options, query, selectedLabel]);

  useEffect(() => {
    if (!open) return;
    const selectedIndex = filtered.findIndex((option) => option.value === value);
    setActiveIndex(selectedIndex >= 0 ? selectedIndex : 0);
  }, [filtered, open, value]);

  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>('[data-active="true"]')?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  const commit = (option: EditableDropdownOption) => {
    onChange(option.value);
    setDraft(option.label);
    setOpen(false);
  };

  const commitDraft = () => {
    const needle = draft.trim();
    if (!needle) {
      setDraft(selectedLabel);
      setOpen(false);
      return;
    }
    const match = options.find((option) =>
      option.label.toLowerCase() === needle.toLowerCase() || option.value.toLowerCase() === needle.toLowerCase());
    onChange(match ? match.value : needle);
    setDraft(match ? match.label : needle);
    setOpen(false);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (disabled || event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setOpen(true);
      setActiveIndex((prev) => Math.min(prev + 1, Math.max(filtered.length - 1, 0)));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setOpen(true);
      setActiveIndex((prev) => Math.max(prev - 1, 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (open && filtered[activeIndex]) commit(filtered[activeIndex]);
      else commitDraft();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      setDraft(selectedLabel);
      setOpen(false);
    } else if (event.key === "Tab") {
      setOpen(false);
    }
  };

  return (
    <Popover.Root open={open && !disabled} onOpenChange={(next) => { if (!disabled) setOpen(next); }}>
      <Popover.Anchor asChild>
        <div className={`nova-combobox ${className}`.trim()} data-slot="combobox">
          <Input
            id={id}
            role="combobox"
            aria-label={ariaLabel}
            aria-expanded={open}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={open && filtered[activeIndex] ? `${listId}-${activeIndex}` : undefined}
            autoComplete="off"
            disabled={disabled}
            placeholder={placeholder ?? t("library.choosePlaceholder")}
            value={draft}
            onChange={(event) => { setDraft(event.target.value); setOpen(true); }}
            onFocus={() => { if (!disabled) setOpen(true); }}
            onBlur={() => { if (open) commitDraft(); }}
            onClick={() => { if (!disabled) setOpen(true); }}
            onKeyDown={handleKeyDown}
          />
          <ChevronDown data-slot="combobox-chevron" aria-hidden="true" />
        </div>
      </Popover.Anchor>
      <Popover.Portal>
        <Popover.Content
          data-native-ui-surface
          data-slot="select-content"
          className="dropdown-menu nova-combobox-menu"
          aria-label={ariaLabel}
          align="start"
          sideOffset={4}
          collisionPadding={8}
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => event.preventDefault()}
        >
          <div id={listId} role="listbox" aria-label={ariaLabel} className="nova-combobox-list" ref={listRef}>
            {filtered.map((option, index) => (
              <div
                key={option.value}
                id={`${listId}-${index}`}
                role="option"
                aria-selected={option.value === value}
                data-active={index === activeIndex}
                data-slot="select-item"
                className="dropdown-item"
                onMouseEnter={() => setActiveIndex(index)}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => commit(option)}
              >
                {option.label}
              </div>
            ))}
            {filtered.length === 0 && <div className="nova-combobox-empty">{t("ui.noMatchingOptions")}</div>}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
