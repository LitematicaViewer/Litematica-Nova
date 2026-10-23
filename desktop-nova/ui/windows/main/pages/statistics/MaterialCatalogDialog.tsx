import React, { useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, X } from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";
import {
  displayFilePath, formatMaterialQuantity, getUiLocale, materialDisplayName, readMaterialDetail, t,
  type MaterialDetail, type MaterialItem, type StatsData,
} from "../../../../../src/business/facade";
import { useI18n } from "../../../../i18n";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Dropdown } from "../../../../components/Dropdown";
import { VirtualSpacerItem } from "../../../../components/VirtualSpacerCell";
import { useVirtualWindow } from "../../../../components/useVirtualWindow";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Input } from "../../../../components/ui/input";
import { RecipeTreeView } from "./RecipeTreeView";

interface MaterialCatalogDialogProps {
  data: StatsData | null;
  currentFile: string;
  onClose: () => void;
  /** Keep this component mounted so closing it preserves lookup preferences. */
  open?: boolean;
  onRequestContainerStats?: (include: boolean) => Promise<StatsData>;
}

export function filterCatalogMaterials(materials: readonly MaterialItem[], search: string, category: string, sort: string): MaterialItem[] {
  const query = search.trim().toLocaleLowerCase();
  return materials.filter(item => {
    if (category === "placed" && item.blockCount <= 0 || category === "containers" && item.containerItemCount <= 0
      || category === "variants" && item.id === (item.itemId || item.id)) return false;
    return !query || `${materialDisplayName(item)} ${item.id} ${item.itemId || ""}`.toLocaleLowerCase().includes(query);
  }).sort((left, right) => {
    const byName = () => materialDisplayName(left).localeCompare(materialDisplayName(right), getUiLocale()) || left.id.localeCompare(right.id);
    return sort === "count_asc" ? left.totalCount - right.totalCount || byName()
      : sort === "count_desc" ? right.totalCount - left.totalCount || byName() : byName();
  });
}

function CatalogRows({ rows, snapshotAvailable, selectedId, individual, busy, onSelect }: {
  rows: MaterialItem[]; snapshotAvailable: boolean; selectedId: string | null; individual: boolean; busy: boolean;
  onSelect: (item: MaterialItem, element: HTMLButtonElement) => void;
}) {
  const { locale } = useI18n();
  const listRef = useRef<HTMLDivElement>(null);
  // Mounted with Content (not the persistent dialog root), so the existing
  // virtual-window hook can observe a real scroll element on first effect.
  const virtual = useVirtualWindow(rows, listRef, 56);
  useEffect(() => { if (listRef.current) listRef.current.scrollTop = 0; }, [rows]);
  return <div className="material-catalog-list" ref={listRef} aria-label={t("statistics.catalogTitle")} aria-busy={busy}>
    {!snapshotAvailable ? <p className="material-catalog-empty">{t("statistics.catalogUnavailable")}</p> : rows.length ? <ul className="material-catalog-items">
      {virtual.topSpacerHeight > 0 && <VirtualSpacerItem className="material-catalog-spacer" height={virtual.topSpacerHeight} />}
      {virtual.visibleItems.map(item => <li key={item.id}><Button themeSlot="material-catalog-row" variant="ghost" className="material-catalog-row" data-selected={selectedId === item.id}
        aria-pressed={selectedId === item.id} onClick={event => onSelect(item, event.currentTarget)} title={`${materialDisplayName(item)}\n${item.id}`}>
        <span className="material-catalog-item"><BlockIcon blockId={item.iconHint} lookupMode="item_first" /><span>
          <span className="material-catalog-name">{materialDisplayName(item)}</span><span className="material-catalog-id">{item.id}</span>
        </span></span>
        <span className="material-catalog-quantity" title={item.totalCount.toLocaleString(locale)}>{formatMaterialQuantity(item, item.totalCount, individual)}</span>
      </Button></li>)}
      {virtual.bottomSpacerHeight > 0 && <VirtualSpacerItem className="material-catalog-spacer" height={virtual.bottomSpacerHeight} />}
    </ul> : <p className="material-catalog-empty">{t("statistics.noMatches")}</p>}
  </div>;
}

/** Non-modal material lookup. It reads only the supplied projection snapshot. */
export function MaterialCatalogDialog({ data, currentFile, onClose, open = true, onRequestContainerStats }: MaterialCatalogDialogProps) {
  const { locale } = useI18n();
  const [snapshot, setSnapshot] = useState(data);
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState("all");
  const [sort, setSort] = useState("count_desc");
  const [individual, setIndividual] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [recipeOpen, setRecipeOpen] = useState(false);
  const [selectedRecipe, setSelectedRecipe] = useState("");
  const [detail, setDetail] = useState<MaterialDetail | null>(null);
  const [detailBusy, setDetailBusy] = useState(false);
  const [detailError, setDetailError] = useState("");
  const [containerBusy, setContainerBusy] = useState(false);
  const [containerError, setContainerError] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const selectedRowFocus = useRef<HTMLButtonElement | null>(null);
  const containerGeneration = useRef(0);
  const detailCache = useRef(new Map<string, MaterialDetail>());

  useEffect(() => {
    containerGeneration.current += 1;
    setSnapshot(data); setContainerBusy(false); setContainerError("");
    detailCache.current.clear(); setDetail(null); setDetailError("");
    setRecipeOpen(false); setSelectedRecipe("");
    setSelectedId(null); setDetailOpen(false);
  }, [data, currentFile]);
  useEffect(() => () => { containerGeneration.current += 1; }, []);

  const rows = useMemo(() => filterCatalogMaterials(snapshot?.materials || [], search, category, sort), [snapshot, search, category, sort, locale]);
  const selected = snapshot?.materials.find(item => item.id === selectedId);
  const includeContainers = snapshot?.containerScan.enabled === true;
  const rawMaterial = selected && snapshot?.raw?.material_items?.find((item: any) => (item.item_identity || item.block_id || item.id) === selected.id);
  const materialSemantics = String(snapshot?.raw?.material_semantics || "");
  const dataVersion = Number(snapshot?.raw?.metadata?.minecraft_data_version);

  useEffect(() => {
    let active = true;
    setDetail(null); setDetailError(""); setDetailBusy(false);
    if (!open || !recipeOpen || !selected || !rawMaterial) return;
    const key = JSON.stringify([selected.id, selected.totalCount, selectedRecipe]);
    const cached = detailCache.current.get(key);
    if (cached) { setDetail(cached); return; }
    setDetailBusy(true);
    void readMaterialDetail({ material: rawMaterial, materialSemantics, dataVersion, selectedRecipe: selectedRecipe || undefined })
      .then(result => {
        if (!active) return;
        detailCache.current.set(key, result); setDetail(result);
      }).catch(failure => { if (active) setDetailError(String(failure)); })
      .finally(() => { if (active) setDetailBusy(false); });
    return () => { active = false; };
  }, [open, recipeOpen, selected, rawMaterial, materialSemantics, dataVersion, selectedRecipe]);

  const changeContainers = async (include: boolean) => {
    if (!onRequestContainerStats || containerBusy || include === includeContainers) return;
    const generation = ++containerGeneration.current;
    setContainerBusy(true); setContainerError("");
    try {
      const result = await onRequestContainerStats(include);
      if (generation !== containerGeneration.current) return;
      setSnapshot(result); detailCache.current.clear(); setDetail(null);
      setRecipeOpen(false); setSelectedRecipe("");
      if (!result.materials.some(item => item.id === selectedId)) { setSelectedId(null); setDetailOpen(false); }
    } catch (failure) { if (generation === containerGeneration.current) setContainerError(String(failure)); }
    finally { if (generation === containerGeneration.current) setContainerBusy(false); }
  };
  const selectMaterial = (item: MaterialItem, element: HTMLButtonElement) => {
    if (selectedId !== item.id) { setDetail(null); setRecipeOpen(false); setSelectedRecipe(""); setDetailError(""); }
    selectedRowFocus.current = element;
    setSelectedId(item.id); setDetailOpen(true);
  };
  const detailName = selected ? materialDisplayName(selected) : "";
  const detailIcon = selected ? detail?.iconImages[selected.iconHint] : undefined;
  const fileName = displayFilePath(currentFile).split(/[\\/]/).pop() || currentFile;

  return <DialogPrimitive.Root open={open} modal={false} onOpenChange={next => { if (!next) onClose(); }}>
    <DialogPrimitive.Portal>
      <DialogPrimitive.Content className="material-catalog-dialog" aria-describedby={undefined}
        onInteractOutside={event => event.preventDefault()}
        onOpenAutoFocus={event => {
          event.preventDefault(); previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
          searchRef.current?.focus();
        }}
        onCloseAutoFocus={event => {
          event.preventDefault();
          if (document.activeElement === document.body || event.currentTarget instanceof HTMLElement && event.currentTarget.contains(document.activeElement)) previousFocus.current?.focus();
        }}>
        <header className="material-catalog-header">
          <DialogPrimitive.Title className="material-catalog-title">{t("statistics.catalogTitle")}</DialogPrimitive.Title>
          <span className="material-catalog-source" title={displayFilePath(currentFile)}>{fileName}</span>
          <DialogPrimitive.Close asChild><Button className="material-catalog-close" variant="ghost" size="icon-sm" aria-label={t("editor.closeWindow")}><X size={17} /></Button></DialogPrimitive.Close>
        </header>
        <div className="material-catalog-toolbar">
          <Input ref={searchRef} type="search" value={search} onChange={event => setSearch(event.target.value)} placeholder={t("editor.materialSearchHint")} aria-label={t("editor.materialSearch")} />
          <Dropdown value={category} onChange={setCategory} ariaLabel={t("stockpile.category")} options={[
            { value: "all", label: t("statistics.catalogAll") }, { value: "placed", label: t("statistics.catalogPlaced") },
            { value: "containers", label: t("statistics.catalogContainers") }, { value: "variants", label: t("statistics.catalogVariants") },
          ]} />
          <Dropdown value={sort} onChange={setSort} ariaLabel={t("stockpile.sort")} options={["count_desc", "count_asc", "name"].map(value => ({ value, label: t(`stockpile.sort.${value}`) }))} />
        </div>
        <div className="material-catalog-options">
          <label className="beta-material-check"><Checkbox checked={individual} onCheckedChange={value => setIndividual(value === true)} />{t("statistics.individual")}</label>
          <label className="beta-material-check"><Checkbox checked={includeContainers} disabled={!snapshot || !onRequestContainerStats || containerBusy} onCheckedChange={value => void changeContainers(value === true)} />{t("statistics.includeContainers")}</label>
          {snapshot && <span className="material-catalog-summary">{t("statistics.materialKinds", { visible: rows.length, total: snapshot.materials.length })}</span>}
        </div>
        <p className="material-catalog-summary material-catalog-status">{t("statistics.catalogReadOnly")}</p>
        {containerBusy && <p className="material-catalog-status" role="status">{t("statistics.catalogLoadingContainers")}</p>}
        {containerError && <p className="material-catalog-error" role="alert">{t("statistics.catalogContainersFailed", { error: containerError })}</p>}
        <div className="material-catalog-workspace" data-detail-open={detailOpen && !!selected}>
          <CatalogRows rows={rows} snapshotAvailable={!!snapshot} selectedId={selectedId} individual={individual} busy={containerBusy} onSelect={selectMaterial} />
          <aside className="material-catalog-detail" aria-label={t("stockpile.materialDetails")}>
            {selected ? <>
              <Button themeSlot="material-catalog-back" variant="ghost" size="sm" className="material-catalog-back" onClick={() => {
                setDetailOpen(false);
                window.requestAnimationFrame(() => { if (selectedRowFocus.current?.isConnected) selectedRowFocus.current.focus(); else searchRef.current?.focus(); });
              }}><ArrowLeft size={15} />{t("statistics.catalogBack")}</Button>
              <div className="material-catalog-detail-title">{detailIcon ? <img className="block-icon-image" src={detailIcon} alt="" /> : <BlockIcon blockId={selected.iconHint} lookupMode="item_first" />}<h3>{detailName}</h3></div>
              <dl className="material-catalog-detail-facts">
                <div><dt>{t("stockpile.required")}</dt><dd>{formatMaterialQuantity(selected, selected.totalCount, individual)}</dd></div>
                <div><dt>{t("statistics.catalogStackSize")}</dt><dd>{selected.stackSize}</dd></div>
                <div><dt>{t("statistics.buildQuantity")}</dt><dd>{selected.blockCount.toLocaleString(locale)}</dd></div>
                <div><dt>{t("statistics.containerItems")}</dt><dd>{selected.containerItemCount.toLocaleString(locale)}</dd></div>
                <div><dt>{t("statistics.catalogItemId")}</dt><dd>{selected.itemId || selected.id}</dd></div>
                <div><dt>{t("statistics.catalogIdentity")}</dt><dd>{selected.id}</dd></div>
              </dl>
              {selected.hasCustomStackSize ? <p className="material-catalog-summary">{t("statistics.customStack")}</p> : !selected.canFitContainer && <p className="material-catalog-summary">{t("statistics.cannotFit")}</p>}
              {selected.id !== (selected.itemId || selected.id) && <p className="material-catalog-summary">{t("statistics.catalogVariantHint")}</p>}
              <details className="material-catalog-detail-group" open={recipeOpen} onToggle={event => setRecipeOpen(event.currentTarget.open)}>
                <summary>{t("stockpile.recipeAndMaterials")}</summary>
                {detailBusy && <p className="material-catalog-status" role="status">{t("statistics.catalogLoadingDetail")}</p>}
                {detailError && <p className="material-catalog-error" role="alert">{t("statistics.catalogDetailFailed", { error: detailError })}</p>}
                {!rawMaterial && <p className="material-catalog-status">{t("statistics.catalogUnavailable")}</p>}
                {detail && <>
                  {!!detail.recipeOptions.length && <label className="material-export-field"><span>{t("statistics.catalogRecipeChoice")}</span><Dropdown value={selectedRecipe} onChange={setSelectedRecipe} ariaLabel={t("statistics.catalogRecipeChoice")}
                    options={[{ value: "", label: t("statistics.catalogRecipeAuto") }, ...detail.recipeOptions.map(option => ({ value: option.recipe_id, label: `${option.recipe_id} · ×${option.output_count}` }))]} /></label>}
                  {detail.recipeTree ? <RecipeTreeView key={`${selected.id}:${selectedRecipe}`} node={detail.recipeTree} iconImages={detail.iconImages} itemNames={detail.itemNames} language={locale} /> : <p className="material-catalog-status">{t("stockpile.recipeUnavailable")}</p>}
                </>}
              </details>
              <details className="material-catalog-detail-group"><summary>{t("statistics.catalogSnapshotDetails")}</summary>
                {!selected.regionCounts.length && !selected.layerCounts.length && <p className="material-catalog-status">{t("statistics.catalogNoDistribution")}</p>}
                <dl className="material-catalog-detail-facts">
                  {selected.regionCounts.map(region => <div key={region.region}><dt>{region.region}</dt><dd>{region.count.toLocaleString(locale)}</dd></div>)}
                  {selected.layerCounts.map(layer => <div key={`${layer.layer}:${layer.worldY}`}><dt>{t("statistics.catalogLayers", { layer: layer.layer, worldY: layer.worldY })}</dt><dd>{layer.count.toLocaleString(locale)}</dd></div>)}
                </dl>
              </details>
            </> : <p className="material-catalog-empty">{t("stockpile.selectMaterial")}</p>}
          </aside>
        </div>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  </DialogPrimitive.Root>;
}
