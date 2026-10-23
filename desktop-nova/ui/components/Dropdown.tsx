import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../src/services/i18n";
import { useI18n } from "../i18n";
import React from "react";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";

export function Dropdown({ value, options, onChange, renderValue, disabled = false, ariaLabel, id, className = "" }: {
  value: string;
  options: { label: string; value: string }[];
  onChange: (value: string) => void;
  renderValue?: (value: string) => string;
  disabled?: boolean;
  ariaLabel?: string;
  id?: string;
  className?: string;
}) {
  useI18n();
  // Radix reserves an empty item value for its placeholder. Prefix every domain
  // value rather than inventing a sentinel that could collide with user values.
  const encodeValue = (entry: string) => `value:${entry}`;
  const selected = options.find((option) => option.value === value);
  const display = renderValue ? renderValue(value) : (selected?.label || value);
  return (
    <div className={`dropdown-container ${className}`}>
      <Select value={selected ? encodeValue(value) : ""}
        disabled={disabled || options.length === 0}
        onValueChange={(next) => onChange(next.slice("value:".length))}>
        <SelectTrigger id={id} aria-label={ariaLabel}>
          <SelectValue placeholder={t("library.choosePlaceholder")}><span className="dropdown-value">{display || t("library.choosePlaceholder")}</span></SelectValue>
        </SelectTrigger>
        <SelectContent position="popper" align="start" collisionPadding={8} aria-label={ariaLabel || display || t("library.choosePlaceholder")}>
          {options.map((option) => <SelectItem key={option.value} value={encodeValue(option.value)}>{option.label}</SelectItem>)}
        </SelectContent>
      </Select>
    </div>
  );
}
