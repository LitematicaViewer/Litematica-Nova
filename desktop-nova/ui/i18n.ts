import { useSyncExternalStore } from "react";
import { getI18nRevision, subscribeI18nChanges, t, getUiLocale, getUiLanguagePreference, setUiLanguage } from "../src/services/i18n";

/** Subscribe without replacing the component tree or restarting any task. */
export function useI18n() {
  const revision = useSyncExternalStore(subscribeI18nChanges, getI18nRevision, getI18nRevision);
  return { t, revision, locale: getUiLocale(), preference: getUiLanguagePreference(), setLanguage: setUiLanguage };
}
