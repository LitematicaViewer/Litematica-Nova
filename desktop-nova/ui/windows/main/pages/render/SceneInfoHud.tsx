import React, { useLayoutEffect, useRef } from "react";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { translateBlockId } from "../../../../../src/business/facade";
import type { SceneInteractionSnapshot } from "../../../../../src/platform/sceneView";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Button } from "../../../../components/ui/button";

export type SceneHit = NonNullable<SceneInteractionSnapshot["hit"]>;
export function ContainerSlots({ container, expanded = false }: { container: NonNullable<SceneHit["container"]>; expanded?: boolean }) {
  const items = new Map(container.items.map(item => [item.slot, item]));
  const shown = expanded ? container.capacity : Math.min(27, container.capacity);
  return <div className="scene-hud-slots" role="list" aria-label={t("scene.container")}>
    {Array.from({ length: shown }, (_, slot) => {
      const item = items.get(slot);
      const label = item ? `${item.displayName || translateBlockId(item.id)} × ${item.count}${item.variant ? ` · ${item.variant}` : ""}` : t("flake.emptySlot");
      return <div className="scene-hud-slot" role="listitem" key={slot} title={`${slot + 1}: ${label}`} aria-label={`${slot + 1}: ${label}`}>
        {item && <><BlockIcon blockId={item.id} lookupMode="item_first" /><span className="scene-hud-count">{item.count}</span>{item.variant && <span className="scene-hud-variant" aria-hidden>◆</span>}</>}
      </div>;
    })}
  </div>;
}

/** Presentation only: the Viewer supplies the hit, authoritative state and inventory. */
export function SceneInfoHud({ hit, pointer, locked = false, onDetails }: { hit: SceneHit; pointer?: [number, number] | null; locked?: boolean; onDetails: () => void }) {
  useI18n();
  const panel = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    const element = panel.current, viewport = element?.parentElement;
    if (!element || !viewport || !pointer) return;
    const dock = viewport.querySelector<HTMLElement>(".scene-editor-dock");
    const place = () => {
      const width=viewport.clientWidth, height=viewport.clientHeight;
      const x=Math.max(8,Math.min(width-element.offsetWidth-8,pointer[0]*width+12));
      let y=Math.max(8,Math.min(height-element.offsetHeight-8,pointer[1]*height+12));
      if (dock) {
        const left = (width - dock.offsetWidth) / 2;
        if (x < left + dock.offsetWidth && x + element.offsetWidth > left && y + element.offsetHeight > dock.offsetTop - 8) {
          y = Math.max(8, dock.offsetTop - element.offsetHeight - 8);
        }
      }
      element.style.setProperty("--scene-hud-x", `${x}px`); element.style.setProperty("--scene-hud-y", `${y}px`);
    };
    place(); const observer=new ResizeObserver(place);observer.observe(viewport);observer.observe(element);if(dock)observer.observe(dock);
    return () => observer.disconnect();
  }, [pointer, hit]);
  const title = hit.displayName || translateBlockId(hit.state.id);
  const source = hit.sourceName || (hit.state.id.startsWith("minecraft:") ? "Minecraft" : t("scene.unknownSource"));
  const properties = Object.entries(hit.state.properties);
  return <section ref={panel} className={pointer ? "scene-info-hud scene-info-hud--pointer" : "scene-info-hud"} data-scene-ui-region data-aim-hud={locked || undefined} aria-label={title}>
    <header className="scene-hud-heading">
      <BlockIcon blockId={hit.state.id} lookupMode="block_first" />
      <div className="scene-hud-heading-text"><strong title={title}>{title}</strong><span title={source}>{source}</span></div>
    </header>
    {properties.length > 0 && <div className="scene-hud-properties" title={properties.map(([key, value]) => `${key}=${value}`).join(", ")}>
      {properties.slice(0, 3).map(([key, value]) => <span key={key}>{key}: <b>{value}</b></span>)}
      {properties.length > 3 && <span>+{properties.length - 3}</span>}
    </div>}
    <div className="scene-hud-position"><span>{hit.position.join(" · ")}</span><span>{hit.distance.toFixed(1)} m · {t(`scene.face.${hit.face}`)}</span></div>
    {hit.container?.status === "known" && (hit.container.items.length ? <ContainerSlots container={hit.container} /> : <p className="scene-hud-empty">{t("scene.empty")}</p>)}
    {hit.container?.status === "unknown" && <p className="scene-hud-empty" title={hit.container.error || undefined}>{t("scene.unknown")}</p>}
    {!locked && <Button variant="ghost" className="scene-hud-details" data-scene-interactive onClick={onDetails}>
      {hit.container && hit.container.capacity > 27 ? t("scene.more", { count: hit.container.capacity }) : t("scene.details")}
    </Button>}
  </section>;
}
