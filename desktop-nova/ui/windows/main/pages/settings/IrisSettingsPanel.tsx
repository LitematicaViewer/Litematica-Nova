import React, { useState } from "react";
import { Button } from "../../../../components/ui/button";
import { useI18n } from "../../../../i18n";
import { openIrisSettings } from "../../../../../src/services/irisSettings";

/** Settings keep their ordinary page; editing lives in a native owned window. */
export function IrisSettingsPanel({ root, pack }: { root: string; pack: string }) {
  const { t } = useI18n();
  const [error, setError] = useState("");
  return <section className="iris-settings-panel" aria-label={t("iris.title")}>
    <h3 className="iris-settings-title">{t("iris.title")}</h3>
    {!root.trim() || !pack.trim() ? <p>{t("iris.noPack")}</p> :
      <Button type="button" onClick={() => { setError(""); void openIrisSettings(root, pack).catch(error => setError(String(error))); }}>{t("iris.load")}</Button>}
    {error && <p role="alert">{t("iris.failed", { error })}</p>}
  </section>;
}
