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
              aria-label="选择本页全部投影"
              onCheckedChange={(value) => onToggleAll(value === true)}
            />
          </TableHead>
          <TableHead>名称</TableHead>
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
            onClick={() => binding.onInspect(entry)}
            onDoubleClick={() => { if (!binding.busy && entry.status === "ok") binding.onActivate(entry); }}
          >
            <TableCell className="library-list-select-col" onClick={(event) => event.stopPropagation()}>
              <Checkbox
                checked={binding.isChecked(entry)}
                disabled={binding.busy}
                aria-label={`选择 ${entry.displayName || entry.fileName}`}
                onCheckedChange={(value) => binding.onToggle(entry, value === true)}
              />
            </TableCell>
            <TableCell>
              <span className="library-list-name" title={entry.displayName || entry.fileName}>{entry.displayName || entry.fileName}</span>
              {entry.replicaCount > 1 && (
                <span className="library-replica-chip" data-replica-count={entry.replicaCount} title={`${entry.replicaCount} 份副本`}>
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