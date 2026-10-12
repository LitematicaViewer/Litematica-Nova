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
  const labels = ["版本与发布日期", "新增功能", "功能优化", "问题修复", "已知问题", "后续计划"];
  const aliases: Record<string, string> = { 新增: "新增功能", 优化: "功能优化", 修复: "问题修复", 注意事项: "已知问题" };
  const sections = new Map(labels.map((label) => [label, [] as string[]]));
  let active = "已知问题";
  for (const raw of (notes || "").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || /^#\s/.test(line)) continue;
    const heading = line.replace(/^#+\s*/, "").replace(/[：:]$/, "").trim();
    const label = aliases[heading] || heading;
    if (sections.has(label)) { active = label; continue; }
    if (/^(?:版本|日期)\s*[：:]/.test(line)) { sections.get("版本与发布日期")!.push(line); continue; }
    sections.get(active)!.push(line.replace(/^[-*]\s+/, ""));
  }
  return labels.map((label) => ({ label, lines: sections.get(label)!.length ? sections.get(label)! : ["未提供"] }));
}
