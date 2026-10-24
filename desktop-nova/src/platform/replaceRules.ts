import { invoke } from "./tauri";
export interface ReplaceRuleRow { index: number; unitIndex: number | null; label: string; kind: "rule" | "separator" }
export interface ReplaceRulesView { rows: ReplaceRuleRow[]; totalRules: number }
export function queryReplaceRules(input: { items: unknown[]; query: string; sort: "execution" | "label" }, operationId = crypto.randomUUID()): Promise<ReplaceRulesView> {
  return invoke("application_data_query", { input: { operationId, kind: "replaceRules", input } });
}
