import React, { useEffect, useRef, useState } from "react";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { ImageOff } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { entryStatusLabel, type LibraryEntryView } from "./libraryEntryView";

/**
 * Shared card bindings (D.5).
 *
 * The grid, the grouped/stacked board and the list's card fallback all render the
 * same card, so the selection / drag / preview wiring is passed as one object
 * instead of a dozen callbacks per call site.
 */
export interface LibraryCardBinding {
  isCurrent: (entry: LibraryEntryView) => boolean;
  isInspected: (entry: LibraryEntryView) => boolean;
  isChecked: (entry: LibraryEntryView) => boolean;
  previewUrl: (entry: LibraryEntryView) => string;
  previewLoading: (entry: LibraryEntryView) => boolean;
  previewError: (entry: LibraryEntryView) => string;
  onVisibility: (entryId: string, visible: boolean) => void;
  busy: boolean;
  onInspect: (entry: LibraryEntryView) => void;
  onToggle: (entry: LibraryEntryView, checked: boolean) => void;
  onActivate: (entry: LibraryEntryView) => void;
  onDragStart: (entry: LibraryEntryView, event: React.DragEvent) => void;
  onDragEnd: () => void;
  onDrop: (entry: LibraryEntryView, event: React.DragEvent) => void;
}

/**
 * One logical projection as a grid card (renamed from the A-phase `ProjectionCard`).
 *
 * The replica (重数) badge only appears when an entry has more than one replica and
 * sits at the card's bottom-right; the card itself is a structural container, not an
 * action button (`themeSlot="library-tile"`, see docs/UI_ARCHITECTURE.md V1.24).
 */
export function EntryCard({ entry, binding }: { entry: LibraryEntryView; binding: LibraryCardBinding }) {
  useI18n();
  const card = useRef<HTMLElement>(null);
  const visibility = useRef(binding.onVisibility); visibility.current = binding.onVisibility;
  useEffect(() => {
    const element = card.current; if (!element) return;
    const observer = new IntersectionObserver(entries => visibility.current(entry.entryId, entries.some(item => item.isIntersecting)), { rootMargin: "160px" });
    observer.observe(element);
    return () => { observer.disconnect(); visibility.current(entry.entryId, false); };
  }, [entry.entryId]);
  const [failedImage, setFailedImage] = useState("");
  const previewUrl = binding.previewUrl(entry);
  const hasImage = !!previewUrl && failedImage !== previewUrl;
  const inspected = binding.isInspected(entry);
  const checked = binding.isChecked(entry);
  const statusText = entry.status === "ok" ? `${entry.totalBlocks.toLocaleString()} 方块` : entryStatusLabel(entry);
  return (
    <article
      ref={card}
      className="library-tile"
      data-entry-id={entry.entryId}
      data-inspected={inspected || undefined}
      data-checked={checked || undefined}
      draggable={!binding.busy}
      onDragStart={(event) => binding.onDragStart(entry, event)}
      onDragEnd={binding.onDragEnd}
      onDragOver={(event) => { if (!binding.busy) event.preventDefault(); }}
      onDrop={(event) => binding.onDrop(entry, event)}
    >
      <div className="library-tile-selection">
        <Checkbox
          checked={checked}
          disabled={binding.busy}
          aria-label={`选择 ${entry.displayName || entry.fileName}`}
          onCheckedChange={(value) => binding.onToggle(entry, value === true)}
        />
      </div>
      {binding.isCurrent(entry) && <span className="library-current-marker">当前</span>}
      <Button
        className="library-tile-open"
        variant="ghost"
        themeSlot="library-tile"
        aria-pressed={inspected}
        aria-label={`查看 ${entry.displayName || entry.fileName} 的详情`}
        onClick={() => binding.onInspect(entry)}
        onDoubleClick={() => { if (!binding.busy && entry.status === "ok") binding.onActivate(entry); }}
      >
        <span className="library-tile-image">
          {hasImage
            ? <img src={previewUrl} alt="" loading="lazy" draggable={false} onError={() => setFailedImage(previewUrl)} />
            : <span className="library-preview-placeholder" title={binding.previewError(entry)}><ThemeIcon name="picture_error" size={30} fallback={ImageOff} /><span>{binding.previewLoading(entry) ? t("library.previewLoading") : binding.previewError(entry) ? t("library.previewReadFailed", { error: binding.previewError(entry) }) : t("library.previewMissing")}</span></span>}
        </span>
        <span className="library-tile-name" title={entry.displayName || entry.fileName}>{entry.displayName || entry.fileName}</span>
        <span className="library-tile-meta">{statusText}</span>
      </Button>
      {entry.replicaCount > 1 && (
        <span
          className="library-replica-badge"
          data-replica-count={entry.replicaCount}
          title={`${entry.replicaCount} 份副本`}
          aria-label={`重数 ${entry.replicaCount}`}
        >
          {entry.replicaCount}
        </span>
      )}
    </article>
  );
}
