import { EntryCard, type LibraryCardBinding } from "./EntryCard";
import type { LibraryEntryView } from "./libraryEntryView";

/**
 * Grid view (D.5): a responsive card board over the current page of entries.
 * Pagination and the scroll container are owned by the orchestration container so
 * the grid, list and detail views share one footer and one scroll pane.
 */
export function LibraryGrid({ entries, binding }: { entries: LibraryEntryView[]; binding: LibraryCardBinding }) {
  return (
    <div className="library-card-grid" data-view="grid">
      {entries.map((entry) => <EntryCard key={entry.entryId} entry={entry} binding={binding} />)}
    </div>
  );
}