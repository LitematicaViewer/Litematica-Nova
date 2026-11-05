import { useEffect, useState } from "react";
import { evaluateEnumeratorExpression, type EnumeratorCollection } from "../../src/business/facade";
import { useI18n } from "../i18n";

const empty = { values: [] as string[], dependencies: [] as string[], error: "" };
export function useEnumeratorExpression(expression: string, collections: EnumeratorCollection[]) {
  const { locale } = useI18n();
  const [view, setView] = useState<{ expression: string; collections: EnumeratorCollection[]; locale: string; data: typeof empty }>();
  useEffect(() => {
    if (!expression.trim() || !collections.length) return;
    let active = true;
    void evaluateEnumeratorExpression(expression, collections).then(data => {
      if (active) setView({expression, collections, locale, data});
    }).catch(reason => {
      if (active) setView({expression, collections, locale, data: {...empty, error: String(reason)}});
    });
    return () => { active = false; };
  }, [expression, collections, locale]);
  return view?.expression === expression && view.collections === collections && view.locale === locale ? view.data : empty;
}
