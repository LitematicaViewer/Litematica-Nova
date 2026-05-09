import React, { useState, useEffect } from "react";
import { getBlockIconDataUrl } from "../services/blockIconResolver";

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
    return <img src={src} style={{ width: 32, height: 32, objectFit: 'contain', imageRendering: "pixelated" }} />;
  }
  
  return (
    <div style={{ width: 32, height: 32, border: "1px solid var(--border)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 10, backgroundColor: 'rgba(0,0,0,0.5)', color: '#fff' }}>
      {blockId.replace("minecraft:", "").slice(0, 2).toUpperCase()}
    </div>
  );
}
