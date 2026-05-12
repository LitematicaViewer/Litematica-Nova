import React, { useState, useEffect } from "react";
import { getBlockIconDataUrl } from "../../src/business/facade";

export function BlockIcon({ blockId }: { blockId: string }) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    if (!blockId) return;

    let active = true;
    const fetchIcon = async () => {
      const dataUrl = await getBlockIconDataUrl(blockId);
      if (active) setSrc(dataUrl);
    };
    
    fetchIcon();
    
    return () => { active = false; };
  }, [blockId]);
  
  if (src) {
    return <img className="block-icon-image" src={src} alt="" />;
  }
  
  return (
    <div className="block-icon-fallback">
      {blockId.replace("minecraft:", "").slice(0, 2).toUpperCase()}
    </div>
  );
}


