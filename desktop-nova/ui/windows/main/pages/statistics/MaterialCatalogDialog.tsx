import React, { useEffect, useRef, useState } from "react";
import { ArrowLeft } from "lucide-react";
import {
  displayFilePath, getDataOperationStatus, getMinecraftTranslations, queryMaterials, readMaterialDetail, t,
  type MaterialDetail, type MaterialQueryPage, type MaterialQueryRow, type MaterialSourceGroup, type MaterialSourceSummary,
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

export function MaterialSources({ summary, groups = [], onLoad, busy = false }: {
  summary?: MaterialSourceSummary; groups?: MaterialSourceGroup[]; busy?: boolean;
  onLoad: (region: string, offset: number) => void;
}) {
  const { locale } = useI18n();
  const [expandedRegion, setExpandedRegion] = useState<string | null>(null);
  return <section className="material-source-section" aria-label={t("statistics.catalogSnapshotDetails")}>
    <h4>{t("statistics.catalogSnapshotDetails")}</h4>
    {summary ? <p className="material-catalog-summary">{t("statistics.sourceSummary", { regions: summary.regionCount, min: summary.minY ?? "—", max: summary.maxY ?? "—", count: summary.totalCount.toLocaleString(locale) })}</p>
      : <p className="material-catalog-summary">{t("statistics.catalogNoDistribution")}</p>}
    <div className="material-source-groups">{groups.map(group => <details className="material-catalog-detail-group" key={group.region} open={expandedRegion === group.region}
      onToggle={event => { if (event.currentTarget.open) { setExpandedRegion(group.region); if (group.layers.length === 0 && group.layerCount > 0) onLoad(group.region, 0); } else setExpandedRegion(previous => previous === group.region ? null : previous); }}>
      <summary><span>{group.displayName}</span><span>{t("statistics.sourceGroup", { min: group.minY ?? "—", max: group.maxY ?? "—", count: group.totalCount.toLocaleString(locale) })}</span></summary>
      {busy && !group.layers.length && <p role="status">{t("editor.loading")}</p>}
      {!!group.unlocatedCount && <p className="material-catalog-summary">{t("statistics.unlocatedCount", { count: group.unlocatedCount })}</p>}
      <dl className="material-catalog-detail-facts">{group.layers.map(layer => <div key={`${layer.layer}:${layer.worldY}`}><dt>{t("statistics.catalogLayers", { layer: layer.layer, worldY: layer.worldY })}</dt><dd>{layer.count.toLocaleString(locale)}</dd></div>)}</dl>
      {group.layerCount > group.limit && <nav className="stockpile-pagination" aria-label={t("statistics.pages")}>
        <Button size="sm" variant="ghost" disabled={busy || group.offset === 0} onClick={() => onLoad(group.region, Math.max(0, group.offset - group.limit))}>{t("statistics.previousPage")}</Button>
        <span>{t("statistics.pageRange", { start: group.offset + 1, end: group.offset + group.layers.length, total: group.layerCount })}</span>
        <Button size="sm" variant="ghost" disabled={busy || group.offset + group.layers.length >= group.layerCount} onClick={() => onLoad(group.region, group.offset + group.limit)}>{t("statistics.nextPage")}</Button>
      </nav>}
    </details>)}</div>
  </section>;
}

function CatalogRows({ page, selectedId, busy, onSelect }: {
  page: MaterialQueryPage | null; selectedId: string | null; busy: boolean; onSelect: (item: MaterialQueryRow) => void;
}) {
  const { locale } = useI18n();
  const listRef = useRef<HTMLDivElement>(null);
  const virtual = useVirtualWindow(page?.rows || [], listRef, 56);
  useEffect(() => { if (listRef.current) listRef.current.scrollTop = 0; }, [page]);
  return <div className="material-catalog-list" ref={listRef} aria-label={t("statistics.catalogTitle")} aria-busy={busy}>
    {page?.rows.length ? <ul className="material-catalog-items">
      {virtual.topSpacerHeight > 0 && <VirtualSpacerItem className="material-catalog-spacer" height={virtual.topSpacerHeight} />}
      {virtual.visibleItems.map((item, index) => <li key={item.id} className={(virtual.startIndex + index) % 2 ? "material-list-row-odd" : "material-list-row-even"}><Button themeSlot="material-catalog-row" variant="ghost" className="material-catalog-row" data-selected={selectedId === item.id}
        aria-pressed={selectedId === item.id} onClick={() => onSelect(item)} title={`${item.name}\n${item.id}`}>
        <span className="material-catalog-item"><BlockIcon blockId={item.iconHint} lookupMode="item_first" /><span className="material-catalog-name">{item.name}</span></span>
        <span className="material-catalog-quantity" title={item.totalCount.toLocaleString(locale)}>{item.quantityText}</span>
      </Button></li>)}
      {virtual.bottomSpacerHeight > 0 && <VirtualSpacerItem className="material-catalog-spacer" height={virtual.bottomSpacerHeight} />}
    </ul> : <p className="material-catalog-empty">{t(busy ? "editor.loading" : "statistics.noMatches")}</p>}
  </div>;
}

/** Read-only content for the existing native material_list window, never a modal. */
export function MaterialCatalogDialog({ currentFile }: { currentFile: string }) {
  const { locale, revision } = useI18n();
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState("all");
  const [sort, setSort] = useState("count_desc");
  const [individual, setIndividual] = useState(false);
  const [includeContainers, setIncludeContainers] = useState(false);
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<MaterialQueryPage | null>(null);
  const [selected, setSelected] = useState<MaterialQueryRow | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [recipeOpen, setRecipeOpen] = useState(false);
  const [selectedRecipe, setSelectedRecipe] = useState("");
  const [detail, setDetail] = useState<MaterialDetail | null>(null);
  const [detailBusy, setDetailBusy] = useState(false);
  const [detailError, setDetailError] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [sourceQuery, setSourceQuery] = useState<{ region?: string; offset?: number; limit?: number }>({});
  const searchRef = useRef<HTMLInputElement>(null);
  const queryGeneration = useRef(0);

  useEffect(() => { setSelected(null); setDetail(null); setPage(null); setOffset(0); setDetailOpen(false); }, [currentFile]);
  useEffect(() => {
    const generation = ++queryGeneration.current;
    if (!currentFile) { setBusy(false); return; }
    const operationId = crypto.randomUUID();
    setBusy(true); setError("");
    const timer = window.setTimeout(() => {
      void queryMaterials({ sourcePath: currentFile, includeContainerItems: includeContainers, search, category, sort,
        individualCounts: individual, offset, limit: 200, uiLocale: locale, languageMap: getMinecraftTranslations() }, operationId)
        .then(result => { if (generation === queryGeneration.current) { setPage(result); setSelected(previous => previous ? result.rows.find(row => row.id === previous.id) || null : null); } })
        .catch(failure => { if (generation === queryGeneration.current) setError(String(failure)); })
        .finally(() => { if (generation === queryGeneration.current) setBusy(false); });
    }, 150);
    return () => { queryGeneration.current += 1; window.clearTimeout(timer); void getDataOperationStatus(operationId, true).catch(() => undefined); };
  }, [currentFile, search, category, sort, individual, includeContainers, offset, locale, revision]);
  useEffect(() => {
    let active = true;
    setDetail(null); setDetailError(""); setDetailBusy(false);
    if (!selected || !currentFile) return;
    setDetailBusy(true);
    const operationId = crypto.randomUUID();
    void readMaterialDetail({ sourcePath: currentFile, materialId: selected.id, includeContainerItems: includeContainers,
      selectedRecipe: selectedRecipe || undefined, uiLocale: locale, languageMap: getMinecraftTranslations(), sourceQuery }, operationId)
      .then(result => { if (active) setDetail(result); })
      .catch(failure => { if (active) setDetailError(String(failure)); })
      .finally(() => { if (active) setDetailBusy(false); });
    return () => { active = false; void getDataOperationStatus(operationId, true).catch(() => undefined); };
  }, [currentFile, selected?.id, selectedRecipe, includeContainers, locale, revision, sourceQuery, page?.sourceRevision]);
  const selectMaterial = (item: MaterialQueryRow) => {
    setSelected(item); setDetailOpen(true); setSelectedRecipe(""); setSourceQuery({}); setRecipeOpen(false);
  };
  const changeFilter = (change: () => void) => { setOffset(0); change(); };
  return <section className="material-catalog-content">
    <header className="material-catalog-header"><h2 className="material-catalog-title">{t("statistics.catalogTitle")}</h2>
      <span className="material-catalog-source" title={displayFilePath(currentFile)}>{displayFilePath(currentFile).split(/[\\/]/).pop()}</span></header>
    <div className="material-catalog-toolbar">
      <Input ref={searchRef} type="search" value={search} onChange={event => changeFilter(() => setSearch(event.target.value))} placeholder={t("editor.materialSearchHint")} aria-label={t("editor.materialSearch")} />
      <Dropdown value={category} onChange={value => changeFilter(() => setCategory(value))} ariaLabel={t("stockpile.category")} options={[
        { value: "all", label: t("statistics.catalogAll") }, { value: "placed", label: t("statistics.catalogPlaced") },
        { value: "containers", label: t("statistics.catalogContainers") }, { value: "variants", label: t("statistics.catalogVariants") },
      ]} />
      <Dropdown value={sort} onChange={value => changeFilter(() => setSort(value))} ariaLabel={t("stockpile.sort")} options={["count_desc", "count_asc", "name"].map(value => ({ value, label: t(`stockpile.sort.${value}`) }))} />
    </div>
    <div className="material-catalog-options">
      <label className="beta-material-check"><Checkbox checked={individual} onCheckedChange={value => changeFilter(() => setIndividual(value === true))} />{t("statistics.individual")}</label>
      <label className="beta-material-check"><Checkbox checked={includeContainers} onCheckedChange={value => changeFilter(() => setIncludeContainers(value === true))} />{t("statistics.includeContainers")}</label>
      {page && <span className="material-catalog-summary">{t("statistics.materialKinds", { visible: page.totalRows, total: page.totalMaterials })}</span>}
    </div>
    <p className="material-catalog-summary material-catalog-status" role="status">{t("statistics.catalogReadOnly")}</p>
    {error && <p className="material-catalog-error" role="alert">{error}</p>}
    <div className="material-catalog-workspace" data-detail-open={detailOpen && !!selected}>
      <CatalogRows page={page} selectedId={selected?.id || null} busy={busy} onSelect={selectMaterial} />
      <aside className="material-catalog-detail" aria-label={t("stockpile.materialDetails")}>
        {selected ? <>
          <Button themeSlot="material-catalog-back" variant="ghost" size="sm" className="material-catalog-back" onClick={() => { setDetailOpen(false); searchRef.current?.focus(); }}><ArrowLeft size={15} />{t("statistics.catalogBack")}</Button>
          <div className="material-catalog-detail-title"><BlockIcon blockId={selected.iconHint} lookupMode="item_first" /><h3>{detail?.material.display_name || selected.name}</h3></div>
          <dl className="material-catalog-detail-facts">
            <div><dt>{t("stockpile.required")}</dt><dd>{selected.quantityText}</dd></div>
            <div><dt>{t("statistics.catalogStackSize")}</dt><dd>{selected.stackSize}</dd></div>
            <div><dt>{t("statistics.buildQuantity")}</dt><dd>{selected.blockCount.toLocaleString(locale)}</dd></div>
            <div><dt>{t("statistics.containerItems")}</dt><dd>{selected.containerItemCount.toLocaleString(locale)}</dd></div>
          </dl>
          <details className="material-catalog-detail-group"><summary>{t("stockpile.itemInformation")}</summary><dl className="material-catalog-detail-facts">
            <div><dt>{t("statistics.catalogItemId")}</dt><dd>{selected.itemId}</dd></div><div><dt>{t("statistics.catalogIdentity")}</dt><dd>{selected.id}</dd></div>
          </dl></details>
          {detailBusy && <p role="status">{t("statistics.catalogLoadingDetail")}</p>}
          {detailError && <p className="material-catalog-error" role="alert">{t("statistics.catalogDetailFailed", { error: detailError })}</p>}
          <details className="material-catalog-detail-group" open={recipeOpen} onToggle={event => setRecipeOpen(event.currentTarget.open)}>
            <summary>{t("stockpile.recipeAndMaterials")}</summary>
            {detail && <>
              {!!detail.recipeOptions.length && <label className="material-export-field"><span>{t("statistics.catalogRecipeChoice")}</span><Dropdown value={selectedRecipe} onChange={setSelectedRecipe} ariaLabel={t("statistics.catalogRecipeChoice")}
                options={[{ value: "", label: t("statistics.catalogRecipeAuto") }, ...detail.recipeOptions.map(option => ({ value: option.recipe_id, label: `${option.display_name} · ${option.recipe_type_name} · ×${option.output_count}` }))]} /></label>}
              {detail.recipeTree ? <RecipeTreeView key={`${selected.id}:${selectedRecipe}`} node={detail.recipeTree} iconImages={detail.iconImages} itemNames={detail.itemNames} language={locale} /> : <p>{t("stockpile.recipeUnavailable")}</p>}
            </>}
          </details>
          <MaterialSources key={selected.id} summary={detail?.sourceSummary || selected.sourceSummary} groups={detail?.sourceGroups || selected.sourceGroups} busy={detailBusy} onLoad={(region, nextOffset) => setSourceQuery({ region, offset: nextOffset, limit: 100 })} />
        </> : <p className="material-catalog-empty">{t("stockpile.selectMaterial")}</p>}
      </aside>
    </div>
    {page && page.totalRows > page.limit && <nav className="stockpile-pagination" aria-label={t("statistics.pages")}>
      <Button size="sm" disabled={busy || page.offset === 0} onClick={() => setOffset(Math.max(0, page.offset - page.limit))}>{t("statistics.previousPage")}</Button>
      <span>{t("statistics.pageRange", { start: page.offset + 1, end: page.offset + page.rows.length, total: page.totalRows })}</span>
      <Button size="sm" disabled={busy || page.offset + page.rows.length >= page.totalRows} onClick={() => setOffset(page.offset + page.limit)}>{t("statistics.nextPage")}</Button>
    </nav>}
  </section>;
}
