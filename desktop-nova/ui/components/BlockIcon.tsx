import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../src/services/i18n";
import { useI18n } from "../i18n";
import React, { useState, useEffect } from "react";
import { listenEvent } from "../../src/platform/events";
import { getBlockIconDataUrl } from "../../src/business/facade";

/**
 * Renders a Minecraft block or item icon at the shared UI icon size.
 */
export function BlockIcon({ blockId, lookupMode = "default", slot = "material_list" }: {
  blockId: string; lookupMode?: "default" | "item_first" | "block_first"; slot?: "material_list" | "layering";
}) {
  useI18n();
  const [src, setSrc] = useState<string | null>(null);

  const [resourceRevision, setResourceRevision] = useState(0);
  useEffect(() => {
    const pending = listenEvent("game-resources-changed", () => setResourceRevision((value) => value + 1)).catch(() => undefined);
    return () => { void pending.then((unlisten) => unlisten?.()); };
  }, []);
  useEffect(() => {
    setSrc(null);
    if (!blockId) return;

    let active = true;
    const fetchIcon = async () => {
      const dataUrl = await getBlockIconDataUrl(blockId, slot, lookupMode).catch(() => null);
      if (active) setSrc(dataUrl);
    };
    
    fetchIcon();
    
    return () => { active = false; };
  }, [blockId, slot, lookupMode, resourceRevision]);
  
  if (src) {
    return <img className="block-icon-image" src={src} alt="" />;
  }
  
  return (
    <div className="block-icon-fallback" role="img" aria-label={t("library.noItemIcon")} title={t("library.noNamedIcon", { id: blockId })}>
      ?
    </div>
  );
}


