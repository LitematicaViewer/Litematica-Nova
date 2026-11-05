import { useCreativeIdentities } from "../../../../components/useCreativeIdentities";
import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { translateBlockId } from "../../../../../src/business/facade";
import type { CreativeInventoryCatalog, CreativeInventoryItem } from "../../../../../src/platform/files";
import { listenEvent } from "../../../../../src/platform/events";
import { bundledCreativeGroups, filterCreativeItems, invalidateCreativeInventoryCatalog, prepareCreativeInventoryCatalog, type CreativeCatalogSource } from "../../../../../src/services/creativeInventoryCatalog";
import type { SceneBlockState } from "../../../../../src/platform/sceneView";
import { Dropdown } from "../../../../components/Dropdown";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { BlockIcon } from "../../../../components/BlockIcon";
import { CreativeInventoryTooltip } from "./function";
import "../../../../styles/flake.css";

type Carried = { blockState: string; itemId: string; name: string; state?: SceneBlockState };
type CatalogData = { data: CreativeInventoryCatalog | null; loading: boolean; error: string };
type CatalogState = CatalogData & {key: string; knownGroups: {id: string; label: string}[]};
const cold: CatalogData = { data: null, loading: true, error: "" };
const emptyItems: CreativeInventoryItem[] = [];
const firstRows = new Set(Array.from({length:45}, (_, i) => i));

// Mounted by the scene, not the dialog. Opening/closing the inventory doesn't reload it.
export function useCreativeInventoryCatalog({ version, contentId, resourceIdentity, extraCollections, enabled = true }: CreativeCatalogSource & { enabled?: boolean }): CatalogState {
  const { locale } = useI18n();
  const [revision, setRevision] = useState(0);
  const collections = useMemo(() => JSON.stringify(extraCollections ?? []), [extraCollections]);
  const key = enabled ? JSON.stringify([version, contentId, resourceIdentity, collections, locale, revision]) : "";
  const [resolved, setResolved] = useState<{key:string; contentId:string; version:string} | null>(null);
  const exactContent = contentId || (resolved?.key === key ? resolved.contentId : undefined);
  const exactVersion = version || (resolved?.key === key ? resolved.version : undefined);
  // Always-known groups and exact bundled registry metadata are available
  // before any item/icon query. Never publish bundled categories for a Mod.
  const knownGroups = useMemo(() => [
    ...(extraCollections ?? []).map(({id, name}) => ({id, label:name})),
    {id:"base:dv-blocks", label:t("flake.creativeAll")},
    ...bundledCreativeGroups(exactContent, exactVersion).map(name => ({id:"category:" + name, label:name})),
  ], [extraCollections, locale, exactContent, exactVersion]);
  const [state, setState] = useState<CatalogData & {key: string} | null>(null);
  useEffect(() => {
    const subscription = listenEvent("resource-game-data-changed", () => {
      invalidateCreativeInventoryCatalog();
      setRevision(value => value + 1);
    });
    return () => { void subscription.then(unlisten => unlisten()).catch(() => undefined); };
  }, []);
  useEffect(() => {
    if (!key) return;
    let active = true;
    void prepareCreativeInventoryCatalog({version, contentId, resourceIdentity, extraCollections: JSON.parse(collections)}, locale,
      (id, v) => { if (active) setResolved({key, contentId:id, version:v}); })
      .then(data => { if (active) setState({key, data, loading:false, error:""}); })
      .catch(reason => { if (active) setState({key, data:null, loading:false, error:String(reason)}); });
    return () => { active = false; };
  }, [key, version, contentId, resourceIdentity, collections, locale]);
  return {...(state?.key === key ? state : cold), key, knownGroups};
}

type InventoryProps = {
  catalog: CatalogState;
  onClose: () => void;
  quickbarSlots: string[];
  quickbarIdentity?: { itemIds: Record<string, string>; error: string };
  onChangeQuickbarSlots: (updater: (current: string[]) => string[]) => void;
  typedSlots?: (SceneBlockState | null)[];
  onSelectState?: (index: number, state: SceneBlockState | null) => void;
};
export function CreativeInventoryDialog(props: InventoryProps) {
  // A carried typed state is only valid for the registry it came from.
  return <InventoryDialogBody key={props.catalog.key} {...props} />;
}
function InventoryDialogBody({ catalog, onClose, quickbarSlots, quickbarIdentity, onChangeQuickbarSlots, typedSlots, onSelectState }: InventoryProps) {
  const { locale } = useI18n();
  const [category, setCategory] = useState("");
  const [search, setSearch] = useState("");
  const { data, loading } = catalog;
  const [selectionError, setError] = useState("");
  const localIdentity = useCreativeIdentities(quickbarSlots, data?.version, data?.contentId, !quickbarIdentity && !!data);
  const identity = quickbarIdentity ?? localIdentity;
  const error = catalog.error || selectionError || identity.error;
  const [carried, setCarried] = useState<Carried | null>(null);
  const [pointer, setPointer] = useState({ x: 0, y: 0 });
  const [hover, setHover] = useState<Carried | null>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!carried) return;
    const move = (event: MouseEvent) => setPointer({ x: event.clientX, y: event.clientY });
    window.addEventListener("mousemove", move);
    return () => window.removeEventListener("mousemove", move);
  }, [carried]);
  useEffect(() => {
    previewRef.current?.style.setProperty("--flake-carry-x", `${pointer.x}px`);
    previewRef.current?.style.setProperty("--flake-carry-y", `${pointer.y}px`);
  }, [pointer, carried]);
  const options = useMemo(() => (data?.categories ?? catalog.knownGroups).map(value => ({
    value: value.id, label: value.id.startsWith("category:") ? t("flake.category." + value.id.slice(9)) : value.label,
  })), [data, catalog.knownGroups, locale]);
  const selected = options.some(option => option.value === category) ? category : options[0]?.value ?? "";
  const items = useMemo(() => data ? filterCreativeItems(data, selected, search) : emptyItems, [data, selected, search]);
  // Radix mounts the portal after its owner. A callback ref also starts the
  // observer when the full catalog was already ready before the dialog opened.
  const [grid, setGrid] = useState<HTMLDivElement | null>(null);
  const [visibility, setVisibility] = useState<{items: CreativeInventoryItem[]; indices: Set<number>} | null>(null);
  const visibleIcons = visibility?.items === items ? visibility.indices : firstRows;
  // All slots participate in native scroll/focus. Only visible/nearby icons
  // start native icon reads or subscribe to resource changes.
  useLayoutEffect(() => {
    if (!grid) return;
    grid.scrollTop = 0;
    setHover(null);
    let active = true;
    const observer = new IntersectionObserver(entries => {
      if (!active) return;
      setVisibility(current => {
        const next = new Set(current?.items === items ? current.indices : []);
        for (const entry of entries) {
          const index = Number((entry.target as HTMLElement).dataset.itemIndex);
          if (entry.isIntersecting) next.add(index); else next.delete(index);
        }
        return {items, indices:next};
      });
    }, {root:grid, rootMargin:"96px 0px"});
    grid.querySelectorAll("[data-item-index]").forEach(slot => observer.observe(slot));
    return () => { active = false; observer.disconnect(); };
  }, [items, grid]);
  const carriedState = carried?.blockState;
  const slots = useMemo(() => <>
    {items.map((item, index) => <Button themeSlot="inventory-slot" variant="ghost" key={item.blockState} data-item-index={index} draggable
      className={carriedState === item.blockState ? "flake-page__creative-slot is-carried-source" : "flake-page__creative-slot"} aria-label={item.name}>
      {visibleIcons.has(index) && <BlockIcon blockId={item.itemId} lookupMode="item_first" />}
    </Button>)}
    {Array.from({length:Math.max(0, 45-items.length)}, (_, i) => <div key={i} className="flake-page__creative-slot is-empty" aria-hidden />)}
  </>, [items, visibleIcons, carriedState]);
  const eventSlot = (target: EventTarget) => target instanceof Element ? target.closest<HTMLElement>("[data-item-index]") : null;
  const eventItem = (target: EventTarget) => { const slot = eventSlot(target); return slot ? items[Number(slot.dataset.itemIndex)] : undefined; };
  const showHover = (value: Carried, event: React.MouseEvent) => {
    if (!carried) setHover(value);
    setPointer({ x: event.clientX, y: event.clientY });
  };
  const focusItem = (value: Carried | null, element: HTMLElement) => {
    if (!value || carried) return; const rect=element.getBoundingClientRect(); setHover(value); setPointer({x:rect.right,y:rect.bottom});
  };
  const slotValue = (index: number): Carried | null => {
    const value = quickbarSlots[index];
    if (!value) return null;
    const itemId = identity.itemIds[value] ?? value;
    return { blockState: value, itemId, name: translateBlockId(itemId), state: typedSlots?.[index] ?? undefined };
  };
  const putSlot = (index: number, next: Carried | null) => {
    if (onSelectState) {
      // Typed states originate in the Rust registry query or the native hotbar.
      if (next && !next.state) { setError(t("scene.inventoryStateMissing")); return; }
      onSelectState(index, next?.state ?? null);
    } else {
      onChangeQuickbarSlots(current => current.map((value, i) => i === index ? next?.blockState ?? "" : value));
    }
  };
  const takeItem = (item: CreativeInventoryItem, event: React.MouseEvent) => {
    event.stopPropagation(); setHover(null); setCarried(carried ? null : item);
    setPointer({ x: event.clientX, y: event.clientY });
  };
  const changeSlot = (index: number, event: React.MouseEvent) => {
    event.stopPropagation(); setHover(null);
    const previous = slotValue(index);
    putSlot(index, carried); setCarried(previous);
    setPointer({ x: event.clientX, y: event.clientY });
  };
  return <Dialog title={t("flake.inventory")} onClose={onClose} width="lg" className="flake-page__inventory-dialog">
    <section onClick={() => { setCarried(null); setHover(null); }} onKeyDown={event => {
      if (event.defaultPrevented || event.nativeEvent.isComposing || event.ctrlKey || event.altKey || event.metaKey || event.target instanceof Element && event.target.closest('input,textarea,select,[role="combobox"],[role="listbox"],[contenteditable=true]')) return;
      if (/^[1-9]$/.test(event.key) && (carried || hover)) { event.preventDefault(); event.stopPropagation(); putSlot(Number(event.key)-1, carried || hover); setCarried(null); }
    }}>
      <div className="flake-page__inventory-dialog-body">
        <div className="flake-page__creative-inventory-controls">
          <label className="flake-page__creative-inventory-control"><span className="nova-muted">{t("editor.category")}</span>
            <Dropdown value={selected} onChange={setCategory} options={options} ariaLabel={t("flake.creativeCategory")} /></label>
          <label className="flake-page__creative-inventory-control flake-page__creative-inventory-search"><span className="nova-muted">{t("editor.search")}</span>
            <Input value={search} autoComplete="off" spellCheck={false} aria-label={t("editor.search")} placeholder={t("flake.filterBlocks")} onChange={event => setSearch(event.target.value)} /></label>
          <div className="flake-page__creative-inventory-range nova-muted nova-small"><span aria-live="polite">{data && t("flake.creativeCount", {count:items.length})}</span>
          </div>
        </div>
        <div className="flake-page__creative-inventory-stage"><div className="flake-page__creative-inventory-surface">
          <div ref={setGrid} className="flake-page__creative-inventory-grid flake-page__creative-inventory-grid--scroll"
            tabIndex={0} role="group" aria-label={t("flake.inventory")} aria-busy={loading}
            onScroll={() => setHover(null)} onMouseLeave={() => setHover(null)} onBlur={() => setHover(null)}
            onFocus={event => { const slot = eventSlot(event.target); if (slot) focusItem(items[Number(slot.dataset.itemIndex)], slot); }}
            onClick={event => { const item = eventItem(event.target); if (item) takeItem(item, event); }}
            onMouseMove={event => { const item = eventItem(event.target); if (item) showHover(item, event); else setHover(null); }}
            onDragStart={event => { const item = eventItem(event.target); if (item) { setCarried(item); event.dataTransfer.effectAllowed = "copy"; event.dataTransfer.setData("text/plain", item.blockState); } }}>
            {slots}</div>
          <div className="flake-page__creative-hotbar-grid">{quickbarSlots.map((_, index) => {
            const value = slotValue(index);
            return <Button themeSlot="inventory-slot" variant="ghost" key={index} className={`flake-page__creative-hotbar-slot${value ? " has-item" : ""}`}
              aria-label={t("flake.hotbarItem", { index: index + 1, item: value?.name ?? t("flake.emptySlot") })}
              onClick={event => changeSlot(index, event)} onFocus={event => focusItem(value,event.currentTarget)} onBlur={() => setHover(null)} draggable={!!value}
              onDragStart={event => { if(value){setCarried(value);event.dataTransfer.effectAllowed="copy";event.dataTransfer.setData("text/plain",value.blockState);} }}
              onContextMenu={event => {event.preventDefault();event.stopPropagation();putSlot(index,null);setCarried(null);}}
              onDragOver={event => { if (carried) event.preventDefault(); }}
              onDrop={event => { event.preventDefault(); event.stopPropagation(); if (carried) { putSlot(index, carried); setCarried(null); } }}
              onMouseEnter={event => { if (value) showHover(value, event); }} onMouseMove={event => { if (value) showHover(value, event); }} onMouseLeave={() => setHover(null)}>
              {value && <BlockIcon blockId={value.itemId} lookupMode="item_first" />}<kbd>{index+1}</kbd></Button>;
          })}</div>
          {!loading && !error && !items.length && <div className="flake-page__creative-inventory-overlay nova-muted">{t("flake.noBlocks")}</div>}
          {loading && <div className="flake-page__creative-inventory-overlay nova-muted" role="status">{t("flake.creativePreparingItems")}</div>}
          {!loading && error && <div className="flake-page__creative-inventory-overlay nova-error" role="alert">{error}</div>}
        </div></div>
      </div>
    </section>
    {carried && <div ref={previewRef} className="flake-page__creative-carry-preview"><BlockIcon blockId={carried.itemId} lookupMode="item_first" /></div>}
    {!carried && <CreativeInventoryTooltip x={pointer.x} y={pointer.y} item={hover ? { id: hover.itemId, name: hover.name } : null} />}
  </Dialog>;
}
