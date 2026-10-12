import React, { useState, useEffect } from "react";
import { getBlockIconDataUrl } from "../../src/business/facade";

/**
 * Renders a Minecraft block or item icon at the shared UI icon size.
 */
export function BlockIcon({ blockId }: { blockId: string }) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    setSrc(null);
    if (!blockId) return;

    let active = true;
    const fetchIcon = async () => {
      const dataUrl = await getBlockIconDataUrl(blockId).catch(() => null);
      if (active) setSrc(dataUrl);
    };
    
    fetchIcon();
    
    return () => { active = false; };
  }, [blockId]);
  
  if (src) {
    return <img className="block-icon-image" src={src} alt="" />;
  }
  
  return (
    <div className="block-icon-fallback" role="img" aria-label="暂无物品图标" title={`暂无图标：${blockId}`}>
      ?
    </div>
  );
}


