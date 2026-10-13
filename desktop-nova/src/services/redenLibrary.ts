import {
  redenDownloadAttachment,
  redenDownloadParametric,
  redenMachineDetail,
  redenSearchLitematica,
} from "../platform/http";
import { addOrUpdateRecord, LibraryState } from "./libraryStore";

export interface RedenTag {
  tag?: string;
  name?: string;
  description?: string;
}

export interface RedenAuthor {
  id?: number;
  username?: string;
  avatarUrl?: string;
}

export interface RedenAttachment {
  name?: string;
  url?: string;
  size?: number;
  description?: string | null;
}

export interface RedenMachine {
  type?: string;
  key: string;
  pageUrl?: string;
  sourceId?: string;
  downloadListAvailable?: boolean;
  name?: string;
  summary?: string | null;
  description?: string | null;
  updatedAt?: number;
  author?: RedenAuthor;
  downloads?: number;
  upVotes?: number;
  downVotes?: number;
  categoryTag?: RedenTag | null;
  featureTags?: RedenTag[];
  versions?: string[];
  attachments?: RedenAttachment[];
  hasX?: boolean;
  hasY?: boolean;
  hasZ?: boolean;
  conditions?: Record<"x" | "y" | "z", string[]>;
}

export interface RedenSearchResponse {
  d: RedenMachine[];
  count?: number;
  estimatedTotalHits?: number;
  downloads?: number;
}

export interface RedenDetailResponse {
  d: RedenMachine[];
  count?: number;
  downloads?: number;
}

export interface RedenDownloadedProjection {
  kind: "downloaded";
  path: string;
  file_name: string;
  bytes: number;
}

export interface RedenExternalLink {
  kind: "external_link";
  provider: string;
  url: string;
}

export type RedenDownloadOutput = RedenDownloadedProjection | RedenExternalLink;

/** Drive pages are never treated as projection downloads or opened without confirmation. */
export function redenExternalDriveLink(value: string | undefined): RedenExternalLink | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username || url.password) return null;
    const provider = url.hostname === "pan.quark.cn" ? "quark"
      : ["pan.baidu.com", "yun.baidu.com"].includes(url.hostname) ? "baidu" : null;
    return provider ? { kind: "external_link", provider, url: url.href } : null;
  } catch { return null; }
}

export interface RedenSizes {
  xSize?: number;
  ySize?: number;
  zSize?: number;
}

export interface RedenSizeRule {
  min?: number;
  max?: number;
  mod?: { step: number; offset: number };
}

export async function searchRedenLitematica(query: string): Promise<RedenSearchResponse> {
  return await redenSearchLitematica(query);
}

export async function fetchRedenMachineDetail(machineId: string): Promise<RedenMachine> {
  const response = await redenMachineDetail<RedenDetailResponse>(machineId);
  const item = response.d?.[0];
  if (!item) throw new Error("RedenMC detail returned no machine data.");
  return item;
}

export async function downloadRedenAttachment(machineId: string, attachmentIndex: number): Promise<RedenDownloadOutput> {
  return await redenDownloadAttachment(machineId, attachmentIndex);
}

export async function downloadRedenParametric(machineId: string, sizes: RedenSizes): Promise<RedenDownloadOutput> {
  return await redenDownloadParametric(machineId, sizes);
}

function parseRule(rule: string): Partial<RedenSizeRule> {
  const min = /^min\((\d+)\)$/.exec(rule);
  if (min) return { min: Number(min[1]) };
  const max = /^max\((\d+)\)$/.exec(rule);
  if (max) return { max: Number(max[1]) };
  const mod = /^mod\((\d+),(\d+)\)$/.exec(rule);
  if (mod) return { mod: { step: Number(mod[1]), offset: Number(mod[2]) } };
  return {};
}

export function getRedenSizeRules(detail: RedenMachine): Record<"x" | "y" | "z", RedenSizeRule> {
  const conditions = detail.conditions || { x: [], y: [], z: [] };
  const axes: Array<"x" | "y" | "z"> = ["x", "y", "z"];
  return Object.fromEntries(axes.map((axis) => {
    const merged: RedenSizeRule = {};
    for (const ruleText of conditions[axis] || []) {
      const parsed = parseRule(ruleText);
      if (parsed.min !== undefined) merged.min = Math.max(merged.min ?? parsed.min, parsed.min);
      if (parsed.max !== undefined) merged.max = Math.min(merged.max ?? parsed.max, parsed.max);
      if (parsed.mod) merged.mod = parsed.mod;
    }
    return [axis, merged];
  })) as Record<"x" | "y" | "z", RedenSizeRule>;
}

export function validateRedenSizes(detail: RedenMachine, sizes: RedenSizes): string[] {
  const rules = getRedenSizeRules(detail);
  const errors: string[] = [];
  const required: Array<[keyof RedenSizes, "x" | "y" | "z", boolean | undefined]> = [
    ["xSize", "x", detail.hasX],
    ["ySize", "y", detail.hasY],
    ["zSize", "z", detail.hasZ],
  ];
  for (const [field, axis, enabled] of required) {
    if (!enabled) continue;
    const value = sizes[field];
    const rule = rules[axis];
    if (!Number.isInteger(value)) {
      errors.push(`${axis.toUpperCase()} 必须是整数`);
      continue;
    }
    if (rule.min !== undefined && value! < rule.min) errors.push(`${axis.toUpperCase()} 不能小于 ${rule.min}`);
    if (rule.max !== undefined && value! > rule.max) errors.push(`${axis.toUpperCase()} 不能大于 ${rule.max}`);
    if (rule.mod && value! % rule.mod.step !== rule.mod.offset) {
      errors.push(`${axis.toUpperCase()} 必须满足 ${axis} % ${rule.mod.step} = ${rule.mod.offset}`);
    }
  }
  return errors;
}

export async function importDownloadedLitematic(state: LibraryState, path: string): Promise<LibraryState> {
  if (!path.toLowerCase().endsWith(".litematic")) {
    throw new Error(`下载文件不是 .litematic：${path}`);
  }
  return await addOrUpdateRecord(state, path);
}

/** Persistence succeeded before notifying other windows; a broadcast failure is only a refresh warning. */
export async function notifyRedenImport(notify: () => Promise<void>): Promise<string | null> {
  try { await notify(); return null; }
  catch (error) { return `已入库，但主窗口刷新通知失败，请手动刷新：${String(error)}`; }
}
