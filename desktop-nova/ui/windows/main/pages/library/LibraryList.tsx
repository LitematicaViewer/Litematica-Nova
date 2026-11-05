import { t } from "../../../../../src/services/i18n";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../../components/ui/table";
import { Checkbox } from "../../../../components/ui/checkbox";
import { LIBRARY_COLUMNS, type LibraryEntryView } from "./libraryEntryView";
import type { LibraryCardBinding } from "./EntryCard";

/**
 * List view (D.6): a table over the current page of entries with configurable detail
 * columns (reusing the shared `ui/components/ui/table.tsx`). Name and 重数 are always
 * present; the remaining columns come from `LIBRARY_COLUMNS`.
 */
export function LibraryList({
  entries,
  binding,
  columns,
  allSelected,
  someSelected,
  onToggleAll,
}: {
  entries: LibraryEntryView[];
  binding: LibraryCardBinding;
  columns: string[];
  allSelected: boolean;
  someSelected: boolean;
  onToggleAll: (checked: boolean) => void;
}) {
  const activeColumns = LIBRARY_COLUMNS.filter((column) => columns.includes(column.id));
  return (
    <Table className="library-list-table" data-view="list">
      <TableHeader>
        <TableRow>
          <TableHead className="library-list-select-col">
            <Checkbox
              checked={allSelected ? true : someSelected ? "indeterminate" : false}
              disabled={!entries.length || binding.busy}
              aria-label={t("library.selectCurrentPage")}
              onCheckedChange={(value) => onToggleAll(value === true)}
            />
          </TableHead>
          <TableHead>{t("library.name")}</TableHead>
          {activeColumns.map((column) => <TableHead key={column.id}>{column.label}</TableHead>)}
        </TableRow>
      </TableHeader>
      <TableBody>
        {entries.map((entry) => (
          <TableRow
            key={entry.entryId}
            data-entry-id={entry.entryId}
            data-inspected={binding.isInspected(entry) || undefined}
            aria-selected={binding.isInspected(entry)}
            className="library-list-row"
            tabIndex={0}
            onKeyDown={event => {
              if (event.currentTarget !== event.target || event.nativeEvent.isComposing) return;
              if (event.key === "Enter" || event.key === " ") { event.preventDefault(); binding.onInspect(entry); }
            }}
            onClick={() => binding.onInspect(entry)}
            onDoubleClick={() => { if (!binding.busy && entry.status === "ok") binding.onActivate(entry); }}
          >
            <TableCell className="library-list-select-col" onClick={(event) => event.stopPropagation()} onDoubleClick={event => event.stopPropagation()} onKeyDown={event => event.stopPropagation()}>
              <Checkbox
                checked={binding.isChecked(entry)}
                disabled={binding.busy}
                aria-label={t("library.ux.selectName", { value0: entry.displayName || entry.fileName })}
                onCheckedChange={(value) => binding.onToggle(entry, value === true)}
              />
            </TableCell>
            <TableCell>
              <span className="library-list-name" title={entry.displayName || entry.fileName}>{entry.displayName || entry.fileName}</span>
              {entry.replicaCount > 1 && (
                <span className="library-replica-chip" data-replica-count={entry.replicaCount} title={t("library.ux.copies", { value0: entry.replicaCount })}>
                  ×{entry.replicaCount}
                </span>
              )}
            </TableCell>
            {activeColumns.map((column) => <TableCell key={column.id}>{column.id === "replicas" ? `${entry.replicaCount}` : column.value(entry)}</TableCell>)}
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
