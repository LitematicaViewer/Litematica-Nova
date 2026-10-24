import React, { useEffect, useState } from "react";
import { t } from "../../../../../src/services/i18n";
import { saveUserConfig, type MotionPreference } from "../../../../../src/platform/appData";
import { acceptMotionConfig, currentMotionState, subscribeToMotionChanges } from "../../../../shell/motionRuntime";
import { useI18n } from "../../../../i18n";
import { Dropdown } from "../../../../components/Dropdown";

export function MotionControl() {
  useI18n();
  const [state, setState] = useState(currentMotionState);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => subscribeToMotionChanges(() => setState(currentMotionState())), []);
  const save = async (value: MotionPreference) => {
    setPending(true); setError("");
    try { acceptMotionConfig(await saveUserConfig({ motion_preference: value })); }
    catch (reason) { setError(t("settings.motionFailed", { error: String(reason) })); }
    finally { setPending(false); }
  };
  return <div className="form-row">
    <label className="form-label" htmlFor="nova-motion-preference">{t("settings.motion")}</label>
    <div className="form-field">
      <Dropdown id="nova-motion-preference" ariaLabel={t("settings.motion")} value={state.preference} disabled={pending}
        options={(["system", "off", "reduced", "standard", "rich"] as const).map(value => ({ value, label: t(`settings.motion.${value}`) }))}
        onChange={value => void save(value as MotionPreference)} />
      <p id="nova-motion-help" className="settings-help-text">{t("settings.motionHelp")}</p>
      {state.systemReduced && (state.preference === "standard" || state.preference === "rich") && <p className="settings-help-text" role="status">{t("settings.motionSystemReduced")}</p>}
      {error && <p role="alert">{error}</p>}
    </div>
  </div>;
}
