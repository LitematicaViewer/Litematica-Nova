import React, { useEffect, useState } from "react";
import { getBlockIconDataUrl } from "../../src/business/facade";
import { listenEvent } from "../../src/platform/events";

/**
 * Renders a Minecraft block or item icon at the shared UI icon size.
 */
export function BlockIcon({ blockId, lookupMode = "default" }: { blockId: string; lookupMode?: "default" | "item_first" }) {
  const [src, setSrc] = useState<string | null>(null);
  const [resourceRevision, setResourceRevision] = useState(0);

  useEffect(() => {
    const unlistenPromise = listenEvent("resource-block-icons-changed", () => {
      setResourceRevision((value) => value + 1);
    }).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, []);

  useEffect(() => {
    if (!blockId) return;
    setSrc(null);

    let active = true;
    const fetchIcon = async () => {
      const dataUrl = await getBlockIconDataUrl(blockId, "material_list", lookupMode);
      if (active) setSrc(dataUrl);
    };

    fetchIcon();

    return () => { active = false; };
  }, [blockId, lookupMode, resourceRevision]);

  if (src) {
    return <img className="block-icon-image" src={src} alt="" />;
  }

  return (
    <div className="block-icon-fallback">
      {blockId.replace("minecraft:", "").slice(0, 2).toUpperCase()}
    </div>
  );
}
