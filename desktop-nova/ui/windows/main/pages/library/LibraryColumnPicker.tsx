import { Popover } from "radix-ui";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { LIBRARY_COLUMNS } from "./libraryEntryView";

/**
 * Configurable-column control (D.6 / D.7, spec 5.8.1.3).
 *
 * Shared by the list and detail views. The content is portaled, so it is styled by
 * a standalone class rather than a `.library-page` descendant selector (see the
 * `css-portal-scope` guardrail in docs/UI_ARCHITECTURE.md).
 */
export function LibraryColumnPicker({
  selected,
  onToggle,
  disabled = false,
}: {
  selected: string[];
  onToggle: (id: string, checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <Button size="sm" variant="ghost" disabled={disabled} aria-label="配置详细信息列">
          列
        </Button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content className="library-column-menu" collisionPadding={8} align="end" sideOffset={4}>
          <p className="library-column-menu-title">显示列</p>
          {LIBRARY_COLUMNS.map((column) => (
            <label key={column.id} className="library-column-option">
              <Checkbox
                checked={selected.includes(column.id)}
                disabled={disabled}
                onCheckedChange={(value) => onToggle(column.id, value === true)}
              />
              <span>{column.label}</span>
            </label>
          ))}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}