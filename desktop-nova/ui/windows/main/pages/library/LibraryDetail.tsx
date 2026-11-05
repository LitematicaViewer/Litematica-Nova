import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { Box, ImageOff, Star, Trash2, X } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { LibraryColumnPicker } from "./LibraryColumnPicker";
import { LIBRARY_COLUMNS, entryStatusLabel, formatEntryDate, formatSize, type LibraryEntryView } from "./libraryEntryView";

/**
 * Detail view (D.7): one entry's preview, facts and configurable columns.
 *
 * `variant="inspector"` is the compact right-hand pane used by the grid and list
 * views; `variant="full"` is the detail view itself, which additionally exposes the
 * configurable-column control.
 */
export function LibraryDetail({
  entry,
  variant,
  columns,
  onToggleColumn,
  previewUrl,
  previewLoading,
  previewError,
  busy,
  favorite,
  generatingPreview,
  onClose,
  onActivate,
  onOpenProperties,
  onOpenFolder,
  onCopy,
  onMoveCategory,
  onGeneratePreview,
  onTrash,
  onToggleFavorite,
  categoryLabel,
  onRepositionReplica,
  repositioning,
}: {
  entry: LibraryEntryView | null;
  variant: "inspector" | "full";
  columns: string[];
  onToggleColumn: (id: string, checked: boolean) => void;
  previewUrl: string;
  previewLoading: boolean;
  previewError: string;
  busy: boolean;
  favorite: boolean;
  generatingPreview: boolean;
  onClose?: () => void;
  onActivate: () => void;
  onOpenProperties: () => void;
  onOpenFolder: () => void;
  onCopy: () => void;
  onMoveCategory: () => void;
  onGeneratePreview: () => void;
  onTrash: () => void;
  onToggleFavorite: () => void;
  categoryLabel: (categoryId: string) => string;
  onRepositionReplica: (replicaId: string) => void;
  repositioning: boolean;
}) {
  useI18n();
  if (!entry) {
    return (
      <div className="library-inspector-empty">
        <Box size={32} />
        <p>{t("library.inspectHint")}</p>
      </div>
    );
  }
  const activeColumns = LIBRARY_COLUMNS.filter((column) => columns.includes(column.id));
  const missingReplicas = entry.replicas.filter((replica) => replica.status === "missing");
  return (
    <div className="library-detail-content" data-variant={variant}>
      <div className="library-detail-preview">
        {previewUrl
          ? <img src={previewUrl} alt={t("library.ux.namedPreview", { value0: entry.displayName })} />
          : <span className="library-preview-placeholder"><ThemeIcon name="picture_error" size={36} fallback={ImageOff} /><span>{previewLoading ? t("library.previewLoading") : previewError ? t("library.previewReadFailed", { error: previewError }) : t("library.previewMissing")}</span></span>}
      </div>
      <div className="library-detail-heading">
        <h3 className="library-detail-name">{entry.displayName || entry.fileName}</h3>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-pressed={favorite}
          aria-label={favorite ? t("library.ux.unfavorite") : t("library.ux.favorite")}
          title={favorite ? t("library.ux.unfavorite") : t("library.ux.favorite")}
          disabled={busy}
          onClick={onToggleFavorite}
        >
          <Star size={15} fill={favorite ? "currentColor" : "none"} aria-hidden="true" />
        </Button>
        {onClose && <Button variant="ghost" size="icon-sm" aria-label={t("library.closeDetails")} onClick={onClose}><ThemeIcon name="close" size={15} fallback={X} /></Button>}
      </div>
      <p className="library-detail-author">{entry.author ? t("library.ux.author", { value0: entry.author }) : t("library.noAuthor")}</p>
      {entry.status !== "ok" && <div className="library-detail-error" role="status">{entryStatusLabel(entry)}{entry.lastError && <p>{entry.lastError}</p>}</div>}
      <div className="library-detail-actions">
        <Button variant="default" disabled={busy || entry.status !== "ok"} onClick={onActivate}>{t("library.setCurrentSchematic")}</Button>
        <Button disabled={busy || entry.status !== "ok"} onClick={onOpenProperties}>{t("library.viewProperties")}</Button>
      </div>

      {variant === "full" ? (
        <div className="library-detail-section">
          <div className="library-detail-section-heading"><h4>{t("library.ux.detailInfo")}</h4><LibraryColumnPicker selected={columns} onToggle={onToggleColumn} disabled={busy} /></div>
          <dl className="library-detail-facts">
            {activeColumns.map((column) => (
              <div key={column.id}><dt>{column.label}</dt><dd>{column.value(entry)}</dd></div>
            ))}
          </dl>
        </div>
      ) : (
        <dl className="library-detail-facts">
          <div><dt>{t("properties.regionNames")}</dt><dd>{entry.primary.regionNames === undefined ? t("library.regionNamesUnread") : entry.primary.regionNames.length ? entry.primary.regionNames.map((name, index) => <div key={index}>{name || t("properties.emptyName")}</div>) : t("properties.noRegions")}</dd></div>
          {typeof entry.primary.internalName === "string" && <div><dt>{t("properties.internalName")}</dt><dd>{entry.primary.internalName || t("library.notSpecified")}</dd></div>}
          <div><dt>{t("properties.blocks")}</dt><dd>{entry.totalBlocks.toLocaleString()}</dd></div>
          <div><dt>{t("library.dimensions")}</dt><dd>{entry.enclosingSize.x} × {entry.enclosingSize.y} × {entry.enclosingSize.z}</dd></div>
          <div><dt>{t("library.ux.replicas")}</dt><dd>{entry.replicaCount}</dd></div>
          <div><dt>{t("library.fileSize")}</dt><dd>{formatSize(entry.fileSize)}</dd></div>
          <div><dt>{t("library.lastAnalyzed")}</dt><dd>{formatEntryDate(entry.lastAnalyzedAt)}</dd></div>
        </dl>
      )}

      {entry.description && <p className="library-detail-description">{entry.description}</p>}

      <div className="library-detail-section">
        <h4>{t("editor.category")}</h4>
        <div className="library-detail-categories">
          {entry.categoryIds.length ? entry.categoryIds.map((id) => <span className="library-category-membership" key={id} data-category-id={id}>{categoryLabel(id)}</span>) : <span className="muted">{t("library.uncategorized")}</span>}
        </div>
        <Button size="sm" disabled={busy} onClick={onMoveCategory}>{t("library.moveToCategoryMenu")}</Button>
      </div>

      <div className="library-detail-section">
        <h4>{t("library.preview")}</h4>
        <div className="library-detail-actions">
          <Button size="sm" disabled={busy || generatingPreview || entry.status !== "ok"} aria-busy={generatingPreview} onClick={onGeneratePreview}>{generatingPreview ? t("library.previewGenerating") : t(previewUrl ? "library.previewRegenerate" : "library.previewGenerate")}</Button>
        </div>
      </div>

      <div className="library-detail-section">
        <h4>{t("library.file")}</h4>
        <p className="library-detail-path" title={entry.primary.path}>{entry.primary.path}</p>
        <div className="library-detail-actions">
          <Button size="sm" onClick={onOpenFolder}>{t("library.openFolder")}</Button>
          <Button size="sm" disabled={busy || entry.status === "missing"} onClick={onCopy}>{t("library.copyToMenu")}</Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={onTrash}><ThemeIcon name="delete" size={14} fallback={Trash2} />{t("library.moveToTrash")}</Button>
        </div>
      </div>

      {missingReplicas.length > 0 && (
        <div className="library-detail-section" data-section="reposition">
          <h4>{t("library.ux.missingPrefix")}{missingReplicas.length}）</h4>
          <ul className="library-reposition-list">
            {missingReplicas.map((replica) => (
              <li key={replica.id}>
                <span className="library-reposition-path" title={replica.path}>{replica.fileName || replica.path}</span>
                <Button size="sm" disabled={busy || repositioning} onClick={() => onRepositionReplica(replica.id)}>{t("library.ux.reposition")}</Button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}