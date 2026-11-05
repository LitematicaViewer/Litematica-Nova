import { useCreativeIdentities } from "./useCreativeIdentities";
import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../src/services/i18n";
import { useI18n } from "../i18n";
import { displayPathText } from "../../src/business/facade";
import React, { useState, useEffect, useMemo } from "react";
import { type EnumeratorCollection, loadEnumeratorCollections } from "../../src/business/facade";
import { translateBlockId } from "../../src/business/facade";
import { BlockIcon } from "./BlockIcon";
import { Dropdown } from "./Dropdown";
import { Dialog } from "./Dialog";
import { Input } from "./ui/input";
import { Button } from "./ui/button";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { CreativeInventoryTooltip } from "../windows/main/pages/flake/function";

/**
 * A general-purpose creative-mode block picker dialog.
 * Decoupled from quickbar: clicking a block immediately calls onPickBlock(blockId).
 * extraCollections are shown at the top (e.g. "当前统计结果").
 */
export function BlockPickerDialog({
  onClose,
  onPickBlock,
  title = t("library.chooseBlock"),
  extraCollections = [],
}: {
  onClose: () => void;
  onPickBlock: (blockId: string) => void;
  title?: string;
  /** Extra collections injected at the top of the dropdown, e.g. current file's material list. */
  extraCollections?: { id: string; name: string; values: string[] }[];
}) {
  const { locale, revision } = useI18n();
  const [collections, setCollections] = useState<EnumeratorCollection[]>([]);
  const [selectedCollectionId, setSelectedCollectionId] = useState(() =>
    extraCollections.length > 0 ? extraCollections[0].id : ""
  );
  const [search, setSearch] = useState("");
  const [pageStart, setPageStart] = useState(0);
  const [collectionsLoading, setLoading] = useState(true);
  const [collectionError, setError] = useState<UiMessage>("");
  const [hoverItem, setHoverItem] = useState<{ id: string; name: string } | null>(null);
  const [hoverPos, setHoverPos] = useState({ x: 0, y: 0 });

  useEffect(() => {
    let active = true;
    setLoading(true);
    loadEnumeratorCollections()
      .then((all) => {
        if (!active) return;
        const creative = all
          .filter((c) => (c.category === "creative" || c.id === "base:dv-blocks") && c.values.length > 0)
          .sort((a, b) => {
            if (a.id === "base:dv-blocks") return -1;
            if (b.id === "base:dv-blocks") return 1;
            return a.name.localeCompare(b.name, getUiLocale());
          });
        setCollections(creative);
        setSelectedCollectionId((prev) => {
          const allIds = new Set([...extraCollections.map((c) => c.id), ...creative.map((c) => c.id)]);
          if (prev && allIds.has(prev)) return prev;
          return extraCollections.length > 0 ? extraCollections[0].id : (creative[0]?.id || "");
        });
      })
      .catch((err) => { if (active) { setCollections([]); setError(String(err)); } })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  // extraCollections identity doesn't change after mount; safe to omit from deps
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** All available collections: extra ones first, then creative inventory ones. */
  const allCollections = useMemo(
    () => [
      ...extraCollections.map((c) => ({ ...c, isExtra: true })),
      ...collections.map((c) => ({ id: c.id, name: c.readOnly && c.id === "base:dv-blocks" ? t("library.collection.base:dv-blocks") : c.name, values: c.values, isExtra: false })),
    ],
    [collections, extraCollections, locale]
  );

  const identity = useCreativeIdentities(allCollections.find(c => c.id === selectedCollectionId)?.values ?? []);
  const selectedValues = identity.values;
  const loading = collectionsLoading || identity.loading;
  const error = collectionError || identity.error;
  const itemId = (source: string) => identity.itemIds[source] ?? source;

  const filtered = useMemo(() => {
    const kw = search.trim().toLowerCase();
    if (!kw) return selectedValues;
    return selectedValues.filter(
      (id) => id.toLowerCase().includes(kw) || translateBlockId(identity.itemIds[id] ?? id).toLowerCase().includes(kw)
    );
  }, [search, selectedValues, identity.itemIds, revision]);

  const maxPage = filtered.length <= 45 ? 0 : Math.floor((filtered.length - 1) / 9) * 9;
  const visible = useMemo(() => {
    const page = filtered.slice(pageStart, pageStart + 45);
    return [...page, ...Array.from({ length: Math.max(0, 45 - page.length) }, () => "")];
  }, [filtered, pageStart]);

  useEffect(() => { setPageStart(0); }, [selectedCollectionId, search]);
  useEffect(() => { setPageStart((p) => Math.min(p, maxPage)); }, [maxPage]);

  const handleWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    setPageStart((p) => {
      if (e.deltaY > 0) return Math.min(maxPage, p + 9);
      if (e.deltaY < 0) return Math.max(0, p - 9);
      return p;
    });
  };

  const changePage = (delta: number) => { setHoverItem(null); setPageStart(current => Math.max(0, Math.min(maxPage, current + delta))); };
  const pageKey = (event: React.KeyboardEvent) => {
    if (event.nativeEvent.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.target instanceof Element && event.target.closest('input,textarea,select,[role="combobox"],[contenteditable="true"]')) return;
    if (event.key === "PageUp" || event.key === "PageDown") { event.preventDefault(); event.stopPropagation(); changePage(event.key === "PageUp" ? -45 : 45); }
  };
  const rangeStart = filtered.length ? pageStart + 1 : 0;
  const rangeEnd = filtered.length ? Math.min(filtered.length, pageStart + 45) : 0;

  return (
    <>
      {/* Was a hand-rolled `dialog-overlay` + `dialog-content` pair, which meant no
          portal, no focus trap and no Escape-to-close. The shared Dialog supplies all
          three and carries the same `flake-page__inventory-dialog` styling that its
          sibling creativeInventoryDialog already uses. */}
      <Dialog title={title} onClose={onClose} width="lg" className="flake-page__inventory-dialog">
        <div className="flake-page__inventory-dialog-body" onKeyDown={pageKey}>
          <div className="flake-page__creative-inventory-controls">
            <label className="flake-page__creative-inventory-control">
              <span className="nova-muted">{t("library.categories")}</span>
              <Dropdown
                value={selectedCollectionId}
                onChange={setSelectedCollectionId}
                ariaLabel={t("library.categories")}
                options={allCollections.length === 0
                  ? [{ label: t("library.loadingOption"), value: "" }]
                  : allCollections.map((c) => ({ label: c.name, value: c.id }))}
              />
            </label>
            <label className="flake-page__creative-inventory-control flake-page__creative-inventory-search">
              <span className="nova-muted">{t("library.search")}</span>
              <Input className="input" value={search} placeholder={t("library.blockFilterPlaceholder")} onChange={(e) => setSearch(e.target.value)} />
            </label>
            <div className="flake-page__creative-inventory-range nova-muted nova-small">
              <span aria-live="polite">{loading ? t("library.loading") : `${rangeStart}-${rangeEnd} / ${filtered.length}`}</span>
              <Button variant="ghost" size="icon" disabled={loading || pageStart === 0} aria-label={t("library.previousPage")} title={t("library.previousPage")} onClick={() => changePage(-45)}><ChevronLeft size={16} aria-hidden /></Button>
              <Button variant="ghost" size="icon" disabled={loading || pageStart >= maxPage} aria-label={t("library.nextPage")} title={t("library.nextPage")} onClick={() => changePage(45)}><ChevronRight size={16} aria-hidden /></Button>
            </div>
          </div>
          <div className="flake-page__creative-inventory-stage">
            <div className="flake-page__creative-inventory-surface" onWheel={handleWheel}>
              <div className="flake-page__creative-inventory-grid">
                {visible.map((blockId, idx) =>
                  blockId ? (
                    <Button variant="ghost" type="button"
                      key={`${blockId}-${idx}`}
                      className="flake-page__creative-slot"
                      aria-label={translateBlockId(itemId(blockId))}
                      disabled={loading}
                      onFocus={event => { const rect=event.currentTarget.getBoundingClientRect(); setHoverItem({id:itemId(blockId),name:translateBlockId(itemId(blockId))}); setHoverPos({x:rect.right,y:rect.bottom}); }}
                      onBlur={() => setHoverItem(null)}
                      onClick={() => { onPickBlock(blockId); onClose(); }}
                      onMouseEnter={(e) => { setHoverItem({ id: itemId(blockId), name: translateBlockId(itemId(blockId)) }); setHoverPos({ x: e.clientX, y: e.clientY }); }}
                      onMouseMove={(e) => { setHoverPos({ x: e.clientX, y: e.clientY }); }}
                      onMouseLeave={() => setHoverItem(null)}
                    >
                      <BlockIcon blockId={itemId(blockId)} lookupMode="item_first" />
                    </Button>
                  ) : (
                    <div key={`empty-${idx}`} className="flake-page__creative-slot is-empty" aria-hidden />
                  )
                )}
              </div>
              {loading && <div className="flake-page__creative-inventory-overlay nova-muted">{t("library.creativeCategoriesLoading")}</div>}
              {!loading && error && <div className="flake-page__creative-inventory-overlay nova-error">{displayPathText(localizeMessage(error))}</div>}
              {!loading && !error && !filtered.length && <div className="flake-page__creative-inventory-overlay nova-muted">{t("library.noBlocks")}</div>}
            </div>
          </div>
        </div>
      </Dialog>
      {/* Fixed-position tooltip, so it lives beside the dialog rather than inside it. */}
      <CreativeInventoryTooltip x={hoverPos.x} y={hoverPos.y} item={hoverItem ? { ...hoverItem, name: translateBlockId(hoverItem.id) } : null} />
    </>
  );
}
