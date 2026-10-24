import React from "react";
import type { GroupingMode, LibraryGroup, LibraryEntryView } from "./libraryEntryView";

/**
 * Grouping / stacking board (D.9, design 2.4.2).
 *
 * Dimension grouping partitions the filtered entries into labelled sections and
 * stacks collapse each group's entries into a single unit (the container renders a
 * representative card). The deferred "自由磁贴列" (7.1.2 / 5.8.1.4) is intentionally
 * absent.
 */
export function LibraryGrouping({
  groups,
  mode,
  renderEntries,
  renderStack,
}: {
  groups: LibraryGroup[];
  mode: GroupingMode;
  renderEntries: (entries: LibraryEntryView[]) => React.ReactNode;
  renderStack: (group: LibraryGroup) => React.ReactNode;
}) {
  if (mode === "stack") {
    return (
      <div className="library-stack-board" data-stacked="true">
        {groups.map((group) => (
          <section key={group.key} className="library-stack-unit" data-stack-key={group.key}>
            <header className="library-group-heading">
              <h3>{group.label}</h3>
              <span className="library-group-count">{group.entries.length}</span>
            </header>
            {renderStack(group)}
          </section>
        ))}
      </div>
    );
  }
  return (
    <div className="library-group-board" data-stacked="false">
      {groups.map((group) => (
        <section key={group.key} className="library-group" data-group-key={group.key}>
          <header className="library-group-heading">
            <h3>{group.label}</h3>
            <span className="library-group-count">{group.entries.length}</span>
          </header>
          {renderEntries(group.entries)}
        </section>
      ))}
    </div>
  );
}