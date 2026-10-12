import { useEffect, useRef } from "react";

const KEY = "nova.update-recovery.v1";
const captures = new Map<string, () => unknown>();

function readAll(): Record<string, unknown> {
  const raw = localStorage.getItem(KEY);
  if (!raw) return {};
  const value = JSON.parse(raw);
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("更新草稿格式错误，请保留本地数据后重试。");
  return value;
}

export function readUpdateDraft<T>(key: string): T | undefined {
  return readAll()[key] as T | undefined;
}

export function takeUpdateDraft<T>(key: string): T | undefined {
  const all = readAll(), value = all[key] as T | undefined;
  if (value !== undefined) { delete all[key]; localStorage.setItem(KEY, JSON.stringify(all)); }
  return value;
}

/** Only serializable user drafts; never API keys, credentials, renderer internals or binaries. */
export function useUpdateDraft(key: string, draft: unknown) {
  const latest = useRef(draft); latest.current = draft;
  useEffect(() => {
    const capture = () => latest.current;
    captures.set(key, capture);
    return () => { if (captures.get(key) === capture) captures.delete(key); };
  }, [key]);
}

export function preserveUpdateDrafts() {
  const all = readAll();
  for (const [key, capture] of captures) all[key] = capture();
  // A quota/storage error must abort installation, never silently lose edits.
  localStorage.setItem(KEY, JSON.stringify(all));
}

export function clearCurrentUpdateDrafts() {
  const all = readAll();
  for (const key of captures.keys()) delete all[key];
  localStorage.setItem(KEY, JSON.stringify(all));
}

export function releaseNoteSections(notes: string | null) {
  const labels = ["新增", "优化", "修复", "注意事项"];
  const sections = new Map(labels.map((label) => [label, [] as string[]]));
  let active = "注意事项";
  for (const raw of (notes || "").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || /^(?:#+\s*)?(?:版本|日期)\s*[：:]/.test(line)) continue;
    const heading = line.replace(/^#+\s*/, "").replace(/[：:]$/, "").trim();
    if (sections.has(heading)) { active = heading; continue; }
    sections.get(active)!.push(line.replace(/^[-*]\s+/, ""));
  }
  return labels.map((label) => ({ label, lines: sections.get(label)!.length ? sections.get(label)! : ["未提供"] }));
}
