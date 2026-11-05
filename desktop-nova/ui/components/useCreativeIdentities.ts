import { useEffect, useMemo, useState } from "react";
import { readCreativeIdentities, type CreativeIdentityView } from "../../src/business/facade";
import { listenEvent } from "../../src/platform/events";

const empty = { values: [] as string[], itemIds: {} as Record<string, string> };
export function useCreativeIdentities(values: string[], version?: string, contentId?: string, enabled = true) {
  const [revision, setRevision] = useState(0);
  const [view, setView] = useState<{key: string; data?: CreativeIdentityView; error?: string}>();
  const key = useMemo(() => enabled && values.some(Boolean) ? JSON.stringify([version, contentId, revision, values]) : "",
    [enabled, values, version, contentId, revision]);
  useEffect(() => {
    if (!enabled) return;
    const subscription = listenEvent("resource-game-data-changed", () => setRevision(r => r + 1));
    return () => { void subscription.then(unlisten => unlisten()).catch(() => undefined); };
  }, [enabled]);
  useEffect(() => {
    if (!key) return;
    let active = true;
    const [, , , sources] = JSON.parse(key) as [unknown, unknown, number, string[]];
    void readCreativeIdentities(sources, version, contentId).then(data => {
      if (active) setView({key, data});
    }).catch(reason => { if (active) setView({key, error: String(reason)}); });
    return () => { active = false; };
  }, [key, version, contentId]);
  const current = view?.key === key ? view : undefined;
  return { ...(current?.data ?? empty), loading: !!key && !current, error: current?.error ?? "" };
}
