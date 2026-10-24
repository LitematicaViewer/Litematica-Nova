/** Input affordances only; the Host validates and owns the saved zoom. */
export const UI_SCALE_PRESETS = [.5, .75, .9, 1, 1.1, 1.25, 1.5, 2, 2.5, 3, 4, 5] as const;
export function parseUiScaleDraft(draft: string): number | null {
  const text = draft.trim().replace(/×$/, "");
  if (!/^\d+(?:\.\d{1,2})?$/.test(text)) return null;
  const value = Number(text);
  return Number.isFinite(value) && value >= .5 && value <= 5 ? value : null;
}
export function stepUiScale(value: number, direction: -1 | 1): number | null {
  return parseUiScaleDraft((value + direction * .05).toFixed(2));
}
