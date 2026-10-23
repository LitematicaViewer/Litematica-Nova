import React, { useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, ChevronRight, RotateCcw } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Dropdown } from "../../../../components/Dropdown";
import { useI18n } from "../../../../i18n";
import { readIrisSettings, saveIrisSettings, type IrisOption, type IrisSettingsSnapshot } from "../../../../../src/services/irisSettings";

export function IrisSettingsPanel({ root, pack, onSelectPack, onApplyPack }: {
  root: string; pack: string;
  onSelectPack: (name: string) => void;
  onApplyPack: (root: string, name: string) => Promise<void>;
}) {
  const { t, locale } = useI18n();
  const [opened, setOpened] = useState(false);
  const [snapshot, setSnapshot] = useState<IrisSettingsSnapshot | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [profile, setProfile] = useState<string | null>(null);
  const [screen, setScreen] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [failed, setFailed] = useState(false);
  const generation = useRef(0);
  const pendingPack = useRef(false);
  const install = (next: IrisSettingsSnapshot) => {
    setSnapshot(next); setValues(Object.fromEntries(next.options.map((option) => [option.id, option.value])));
    setProfile(next.selected_profile); setDirty(false); setScreen(""); setHistory([]);
  };
  useEffect(() => {
    const current = ++generation.current;
    setSnapshot(null); setFeedback(""); setDirty(false);
    if (!opened || !root.trim() || !pack.trim()) { setBusy(false); return; }
    setBusy(true);
    readIrisSettings(root.trim(), pack.trim()).then((next) => {
      if (current === generation.current) { install(next); setDirty(pendingPack.current); }
    }).catch((error) => {
      if (current === generation.current) { setFailed(true); setFeedback(t("iris.failed", { error: String(error) })); }
    }).finally(() => { if (current === generation.current) setBusy(false); });
    return () => { generation.current++; };
  }, [opened, root, pack, locale]);
  const options = useMemo(() => new Map(snapshot?.options.map((option) => [option.id, option]) || []), [snapshot]);
  const activeScreen = snapshot?.screens.find((entry) => entry.id === screen);
  const change = (id: string, value: string) => { setValues((current) => ({ ...current, [id]: value })); setDirty(true); setFeedback(""); };
  const apply = async (restore = false) => {
    if (busy || !snapshot) return;
    const current = generation.current;
    setBusy(true); setFeedback(""); setFailed(false);
    try {
      const next = await saveIrisSettings(snapshot.root, snapshot.pack, { profile, values, restore_defaults: restore });
      await onApplyPack(snapshot.root, snapshot.pack);
      if (current !== generation.current) return;
      pendingPack.current = false; install(next); setFeedback(t(next.requires_scene_reload ? "iris.nextScene" : next.applied_to_active_viewer ? "iris.applied" : "iris.saved"));
    } catch (error) {
      if (current === generation.current) { setFailed(true); setFeedback(t("iris.failed", { error: String(error) })); }
    } finally { if (current === generation.current) setBusy(false); }
  };
  const optionControl = (option: IrisOption) => {
    const value = values[option.id] ?? option.value;
    const id = `iris-option-${option.id}`;
    const allowed = option.values.map((entry) => ({ value: entry, label: entry }));
    return <div className="iris-option-row" key={option.id}>
      <label className="iris-option-label" htmlFor={id} title={option.description || undefined}>{option.label}</label>
      <div className="iris-option-control">
        {option.kind === "boolean" ? <label className="iris-boolean-control">
          <Checkbox id={id} disabled={busy} checked={value === "true"} onCheckedChange={(checked) => change(option.id, checked === true ? "true" : "false")} />
          <span>{t(value === "true" ? "iris.enabled" : "iris.disabled")}</span>
        </label> : option.slider && option.values.length > 1 ? <div className="iris-slider-control">
          <input id={id} type="range" min={0} max={option.values.length - 1} step={1} disabled={busy}
            value={Math.max(0, option.values.indexOf(value))} aria-label={option.label}
            aria-valuetext={value} onChange={(event) => change(option.id, option.values[Number(event.target.value)])} />
          <output htmlFor={id}>{value}</output>
        </div> : <Dropdown id={id} disabled={busy} value={value} ariaLabel={option.label} options={allowed} onChange={(value) => change(option.id, value)} />}
        <span className="iris-option-default">{t("iris.default", { value: option.default_value })}</span>
      </div>
    </div>;
  };
  return <section className="iris-settings-panel" aria-label={t("iris.title")}>
    <h3 className="iris-settings-title">{t("iris.title")}</h3>
    {!root.trim() || !pack.trim() ? <p>{t("iris.noPack")}</p> : !opened ? <Button type="button" onClick={() => setOpened(true)}>{t("iris.load")}</Button> : <>
      {busy && !snapshot && <p role="status">{t("iris.loading")}</p>}
      {snapshot && <>
        <div className="form-row"><label className="form-label" htmlFor="iris-pack-choice">{t("iris.pack")}</label>
          <Dropdown id="iris-pack-choice" value={pack} disabled={busy} ariaLabel={t("iris.pack")}
            options={snapshot.packs.map((pack) => ({ value: pack.name, label: pack.name }))} onChange={(name) => { pendingPack.current = true; onSelectPack(name); }} />
        </div>
        {snapshot.profiles.length > 0 && <div className="form-row"><label className="form-label" htmlFor="iris-profile-choice">{t("iris.profile")}</label>
          <Dropdown id="iris-profile-choice" disabled={busy} value={profile || ""} ariaLabel={t("iris.profile")}
            options={[{ value: "", label: t("iris.custom") }, ...snapshot.profiles.map((profile) => ({ value: profile.id, label: profile.label }))]}
            onChange={(value) => { setProfile(value || null); const chosen = snapshot.profiles.find((entry) => entry.id === value); if (chosen) setValues({ ...Object.fromEntries(snapshot.options.map((option) => [option.id, option.default_value])), ...chosen.values }); setDirty(true); setFeedback(""); }} />
        </div>}
        <div className="iris-menu-heading">
          {history.length > 0 && <Button type="button" disabled={busy} onClick={() => { setScreen(history[history.length - 1]); setHistory((value) => value.slice(0, -1)); }}><ArrowLeft aria-hidden="true" />{t("iris.back")}</Button>}
          <strong>{activeScreen?.title || (screen === "__all" ? t("iris.allOptions") : snapshot.pack)}</strong>
          <Button type="button" disabled={busy} onClick={() => { setHistory((value) => [...value, screen]); setScreen("__all"); }}>{t("iris.allOptions")}</Button>
        </div>
        <div className="iris-options-grid" data-columns={activeScreen?.columns || 2}>
          {screen === "__all" || !activeScreen ? snapshot.options.map(optionControl) : activeScreen.elements.map((element, index) => {
            if (element.kind === "option") { const option = options.get(element.option); return option ? optionControl(option) : null; }
            if (element.kind === "link") return <Button key={`link-${index}`} className="iris-screen-link" type="button" disabled={busy} onClick={() => { setHistory((value) => [...value, screen]); setScreen(element.screen); }}>{element.label}<ChevronRight aria-hidden="true" /></Button>;
            return element.kind === "empty" ? <div className="iris-menu-space" key={`space-${index}`} aria-hidden="true" /> : null;
          })}
        </div>
        {!snapshot.options.length && <p>{t("iris.noOptions")}</p>}
        <div className="settings-actions iris-settings-actions">
          <Button type="button" disabled={busy || !dirty} onClick={() => apply()}>{t(busy ? "iris.saving" : "iris.save")}</Button>
          <Button type="button" disabled={busy} onClick={() => apply(true)}><RotateCcw aria-hidden="true" />{t("iris.restore")}</Button>
          {dirty && <span role="status">{t("iris.unsaved")}</span>}
        </div>
      </>}
    </>}
    {feedback && <p className="settings-feedback" role={failed ? "alert" : "status"}>{feedback}</p>}
  </section>;
}
