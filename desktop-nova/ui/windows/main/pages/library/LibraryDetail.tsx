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
        <p>点击卡片查看详情</p>
      </div>
    );
  }
  const activeColumns = LIBRARY_COLUMNS.filter((column) => columns.includes(column.id));
  const missingReplicas = entry.replicas.filter((replica) => replica.status === "missing");
  return (
    <div className="library-detail-content" data-variant={variant}>
      <div className="library-detail-preview">
        {previewUrl
          ? <img src={previewUrl} alt={`${entry.displayName} 预览`} />
          : <span className="library-preview-placeholder"><ThemeIcon name="picture_error" size={36} fallback={ImageOff} /><span>{previewLoading ? t("library.previewLoading") : previewError ? t("library.previewReadFailed", { error: previewError }) : t("library.previewMissing")}</span></span>}
      </div>
      <div className="library-detail-heading">
        <h3 className="library-detail-name">{entry.displayName || entry.fileName}</h3>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-pressed={favorite}
          aria-label={favorite ? "取消收藏" : "加入收藏夹"}
          title={favorite ? "取消收藏" : "加入收藏夹"}
          disabled={busy}
          onClick={onToggleFavorite}
        >
          <Star size={15} fill={favorite ? "currentColor" : "none"} aria-hidden="true" />
        </Button>
        {onClose && <Button variant="ghost" size="icon-sm" aria-label="关闭投影详情" onClick={onClose}><ThemeIcon name="close" size={15} fallback={X} /></Button>}
      </div>
      <p className="library-detail-author">{entry.author ? `作者 · ${entry.author}` : "未填写作者"}</p>
      {entry.status !== "ok" && <div className="library-detail-error" role="status">{entryStatusLabel(entry)}{entry.lastError && <p>{entry.lastError}</p>}</div>}
      <div className="library-detail-actions">
        <Button variant="default" disabled={busy || entry.status !== "ok"} onClick={onActivate}>设为当前投影</Button>
        <Button disabled={busy || entry.status !== "ok"} onClick={onOpenProperties}>查看属性</Button>
      </div>

      {variant === "full" ? (
        <div className="library-detail-section">
          <div className="library-detail-section-heading"><h4>详细信息</h4><LibraryColumnPicker selected={columns} onToggle={onToggleColumn} disabled={busy} /></div>
          <dl className="library-detail-facts">
            {activeColumns.map((column) => (
              <div key={column.id}><dt>{column.label}</dt><dd>{column.value(entry)}</dd></div>
            ))}
          </dl>
        </div>
      ) : (
        <dl className="library-detail-facts">
          <div><dt>选区名</dt><dd>{entry.primary.regionNames === undefined ? "未读取（刷新或查看属性后补齐）" : entry.primary.regionNames.length ? entry.primary.regionNames.map((name, index) => <div key={index}>{name || "（空名称）"}</div>) : "无选区"}</dd></div>
          {typeof entry.primary.internalName === "string" && <div><dt>内部名称</dt><dd>{entry.primary.internalName || "未填写"}</dd></div>}
          <div><dt>方块数量</dt><dd>{entry.totalBlocks.toLocaleString()}</dd></div>
          <div><dt>尺寸</dt><dd>{entry.enclosingSize.x} × {entry.enclosingSize.y} × {entry.enclosingSize.z}</dd></div>
          <div><dt>重数</dt><dd>{entry.replicaCount}</dd></div>
          <div><dt>文件大小</dt><dd>{formatSize(entry.fileSize)}</dd></div>
          <div><dt>最近分析</dt><dd>{formatEntryDate(entry.lastAnalyzedAt)}</dd></div>
        </dl>
      )}

      {entry.description && <p className="library-detail-description">{entry.description}</p>}

      <div className="library-detail-section">
        <h4>分类</h4>
        <div className="library-detail-categories">
          {entry.categoryIds.length ? entry.categoryIds.map((id) => <span className="library-category-membership" key={id} data-category-id={id}>{categoryLabel(id)}</span>) : <span className="muted">未分类</span>}
        </div>
        <Button size="sm" disabled={busy} onClick={onMoveCategory}>移动到分类…</Button>
      </div>

      <div className="library-detail-section">
        <h4>预览图</h4>
        <div className="library-detail-actions">
          <Button size="sm" disabled={busy || generatingPreview || entry.status !== "ok"} aria-busy={generatingPreview} onClick={onGeneratePreview}>{generatingPreview ? t("library.previewGenerating") : t(previewUrl ? "library.previewRegenerate" : "library.previewGenerate")}</Button>
        </div>
      </div>

      <div className="library-detail-section">
        <h4>文件</h4>
        <p className="library-detail-path" title={entry.primary.path}>{entry.primary.path}</p>
        <div className="library-detail-actions">
          <Button size="sm" onClick={onOpenFolder}>打开文件夹</Button>
          <Button size="sm" disabled={busy || entry.status === "missing"} onClick={onCopy}>复制到…</Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={onTrash}><ThemeIcon name="delete" size={14} fallback={Trash2} />移入回收站</Button>
        </div>
      </div>

      {missingReplicas.length > 0 && (
        <div className="library-detail-section" data-section="reposition">
          <h4>文件丢失（{missingReplicas.length}）</h4>
          <ul className="library-reposition-list">
            {missingReplicas.map((replica) => (
              <li key={replica.id}>
                <span className="library-reposition-path" title={replica.path}>{replica.fileName || replica.path}</span>
                <Button size="sm" disabled={busy || repositioning} onClick={() => onRepositionReplica(replica.id)}>重新定位…</Button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}