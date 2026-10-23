import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, ChevronRight, RotateCcw, X } from "lucide-react";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Dropdown } from "../../components/Dropdown";
import { useI18n } from "../../i18n";
import { listenEvent } from "../../../src/platform/events";
import { applyThemeStylesheet, currentThemeId, subscribeToThemeChanges, themeClassName } from "../../shell/themeRuntime";
import { readIrisSettings, saveIrisSettings, closeIrisSettings, type IrisOption, type IrisSettingsSnapshot } from "../../../src/services/irisSettings";

type Selection = { root: string; pack: string };
type Draft = { values: Record<string, string>; profile: string | null; screen: string; history: string[]; scroll: number };
// The native window hides on close: drafts remain session-local, never settings.
const drafts = new Map<string, Draft>();
const draftKey = (snapshot: IrisSettingsSnapshot) => JSON.stringify([snapshot.root, snapshot.pack, snapshot.pack_identity]);
const valuesOf = (snapshot: IrisSettingsSnapshot) => Object.fromEntries(snapshot.options.map(option => [option.id, option.value]));
function validDraft(snapshot: IrisSettingsSnapshot, old?: Draft): Draft {
  const values = valuesOf(snapshot);
  if (old) for (const option of snapshot.options) if (option.values.includes(old.values[option.id])) values[option.id] = old.values[option.id];
  return { values, profile: old?.profile && snapshot.profiles.some(profile => profile.id === old.profile) ? old.profile : null,
    screen: old?.screen === "__all" || snapshot.screens.some(screen => screen.id === old?.screen) ? old!.screen : "",
    history: old?.history.filter(id => id === "__all" || snapshot.screens.some(screen => screen.id === id)) ?? [], scroll: old?.scroll ?? 0 };
}
function selectedProfile(snapshot: IrisSettingsSnapshot, values: Record<string, string>): string | null {
  return snapshot.profiles.find(profile => Object.entries(profile.values).every(([id, value]) => values[id] === value))?.id ?? null;
}

export function IrisOptionsWindow() {
  const { t, locale } = useI18n();
  const [theme, setTheme] = useState(currentThemeId);
  const [selection, setSelection] = useState<Selection>(() => {
    const query = new URLSearchParams(location.search); return { root: query.get("root") ?? "", pack: query.get("pack") ?? "" };
  });
  const [reloadNonce, setReloadNonce] = useState(0);
  const [snapshot, setSnapshot] = useState<IrisSettingsSnapshot | null>(null);
  const [draft, setDraft] = useState<Draft>({ values: {}, profile: null, screen: "", history: [], scroll: 0 });
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [failed, setFailed] = useState(false);
  const request = useRef(0);
  const content = useRef<HTMLDivElement>(null);
  const draftRef = useRef(draft); draftRef.current = draft;
  const snapshotRef = useRef(snapshot); snapshotRef.current = snapshot;
  useLayoutEffect(() => { applyThemeStylesheet(theme); }, [theme]);
  useEffect(() => subscribeToThemeChanges(setTheme), []);
  useEffect(() => {
    const unlisten = listenEvent<Selection>("iris-options-select-pack", event => {
      if (snapshotRef.current) drafts.set(draftKey(snapshotRef.current), draftRef.current);
      setSelection(current => current.root === event.payload.root && current.pack === event.payload.pack ? current : event.payload);
      setReloadNonce(value => value + 1);
    });
    return () => { void unlisten.then(unlisten => unlisten()); };
  }, []);
  useEffect(() => {
    const generation = ++request.current;
    setBusy(true); setFeedback(""); setFailed(false); setSnapshot(null);
    readIrisSettings(selection.root, selection.pack).then(next => {
      if (generation !== request.current) return;
      setSnapshot(next); if (next.discarded_saved_values?.length) setFeedback(t("iris.previousInvalid", { options: next.discarded_saved_values.join(", ") }));
      const nextDraft = validDraft(next, drafts.get(draftKey(next)));
      if (!drafts.has(draftKey(next))) nextDraft.profile = next.selected_profile; setDraft(nextDraft);
      requestAnimationFrame(() => { if (content.current) content.current.scrollTop = nextDraft.scroll; });
    }).catch(error => { if (generation === request.current) { setFailed(true); setFeedback(String(error)); } })
      .finally(() => { if (generation === request.current) setBusy(false); });
    return () => { request.current++; };
  }, [selection.root, selection.pack, locale, reloadNonce]);
  useEffect(() => { if (snapshot) drafts.set(draftKey(snapshot), { ...draft, scroll: content.current?.scrollTop ?? draft.scroll }); }, [draft, snapshot]);
  const optionMap = useMemo(() => new Map(snapshot?.options.map(option => [option.id, option]) ?? []), [snapshot]);
  const active = snapshot?.screens.find(screen => screen.id === draft.screen);
  const dirty = !!snapshot && (snapshot.selected_for_rendering === false || draft.profile !== snapshot.selected_profile || snapshot.options.some(option => draft.values[option.id] !== option.value));
  const changeValues = (values: Record<string, string>) => {
    if (!snapshot) return;
    setDraft(current => ({ ...current, values, profile: selectedProfile(snapshot, values) })); setFeedback("");
  };
  const navigate = (screen: string) => { setDraft(current => ({ ...current, screen, history: [...current.history, current.screen], scroll: 0 })); if (content.current) content.current.scrollTop = 0; };
  const apply = async () => {
    if (!snapshot || busy || !dirty) return;
    const generation = request.current; setBusy(true); setFailed(false); setFeedback("");
    try {
      const next = await saveIrisSettings(snapshot.root, snapshot.pack, { profile: draft.profile, values: draft.values });
      if (generation !== request.current) return;
      setSnapshot(next); // Keep navigation/scroll, then adopt the confirmed effective values/profile.
      setDraft(current => ({ ...current, values: valuesOf(next), profile: next.selected_profile, scroll: content.current?.scrollTop ?? current.scroll }));
      setFeedback(t(next.requires_scene_reload ? "iris.nextScene" : next.applied_to_active_viewer ? "iris.applied" : "iris.saved"));
    } catch (error) { if (generation === request.current) { setFailed(true); setFeedback(String(error)); } }
    finally { if (generation === request.current) setBusy(false); }
  };
  const profileControl = (key: string) => snapshot && snapshot.profiles.length > 0 ? <div className="iris-option-row" key={key}>
    <label htmlFor={`iris-profile-${key}`}>{t("iris.profile")}</label>
    <Dropdown id={`iris-profile-${key}`} disabled={busy} value={draft.profile ?? ""} ariaLabel={t("iris.profile")}
      options={[{ value: "", label: t("iris.custom") }, ...snapshot.profiles.map(profile => ({ value: profile.id, label: profile.label }))]}
      onChange={id => { const profile = snapshot.profiles.find(profile => profile.id === id);
        setDraft(current => ({ ...current, profile: profile?.id ?? null, values: profile ? { ...current.values, ...profile.values } : current.values })); setFeedback(""); }} />
  </div> : null;
  const optionControl = (option: IrisOption) => {
    const value = draft.values[option.id] ?? option.value;
    const valueLabel = (raw: string) => option.value_labels?.[raw] ?? raw;
    const id = `iris-${option.id}`;
    const readonly = option.presentation === "readonly" || option.presentation === "information" || option.values.length === 1;
    return <div className={`iris-option-row${readonly ? " iris-option-information" : ""}`} key={option.id}>
      <label className="iris-option-label" htmlFor={readonly ? undefined : id}>{option.label}</label>
      {readonly ? <div className="iris-option-readonly">{valueLabel(value) && <span>{valueLabel(value)}</span>}{option.description && <p>{option.description}</p>}</div> : <div className="iris-option-control">
        {option.kind === "boolean" ? <label className="iris-boolean-control"><Checkbox id={id} checked={value === "true"} disabled={busy}
          onCheckedChange={checked => changeValues({ ...draft.values, [option.id]: checked === true ? "true" : "false" })} /><span>{t(value === "true" ? "iris.enabled" : "iris.disabled")}</span></label>
          : option.slider && option.values.length > 1 ? <div className="iris-slider-control"><input id={id} type="range" min={0} max={option.values.length - 1} step={1} disabled={busy}
            aria-label={option.label} aria-valuetext={valueLabel(value)} value={Math.max(0, option.values.indexOf(value))}
            onChange={event => changeValues({ ...draft.values, [option.id]: option.values[Number(event.target.value)] })} /><output htmlFor={id}>{valueLabel(value)}</output></div>
            : <Dropdown id={id} disabled={busy} value={value} ariaLabel={option.label} options={option.values.map(raw => ({ value: raw, label: valueLabel(raw) || raw }))}
              onChange={value => changeValues({ ...draft.values, [option.id]: value })} />}
        <span className="iris-option-default">{t("iris.default", { value: valueLabel(option.default_value) })}</span>
      </div>}
      {!readonly && option.description && <details className="iris-option-description"><summary>{t("iris.description")}</summary><p>{option.description}</p></details>}
    </div>;
  };
  return <main className={`iris-options-window ${themeClassName(theme)}`}>
    <header className="iris-window-header"><div><h1>{t("iris.title")}</h1><span className="iris-pack-name">{selection.pack}</span></div>
      <Button variant="ghost" aria-label={t("iris.close")} onClick={() => void closeIrisSettings().catch(error => { setFailed(true); setFeedback(String(error)); })}><X aria-hidden="true" /></Button>
    </header>
    {snapshot && <div className="iris-navigation">
      <Dropdown id="iris-pack" disabled={busy} value={selection.pack} ariaLabel={t("iris.pack")}
        options={snapshot.packs.map(pack => ({ value: pack.name, label: pack.name }))} onChange={pack => setSelection(current => ({ ...current, pack }))} />
      <div className="iris-menu-heading">
        {draft.history.length > 0 && <Button variant="ghost" onClick={() => { setDraft(current => ({ ...current, screen: current.history[current.history.length - 1] ?? "", history: current.history.slice(0, -1), scroll: 0 })); if (content.current) content.current.scrollTop = 0; }}><ArrowLeft aria-hidden="true" />{t("iris.back")}</Button>}
        <strong>{active?.title || (draft.screen === "__all" ? t("iris.allOptions") : t("iris.categories"))}</strong>
        {draft.screen !== "__all" && <Button variant="ghost" onClick={() => navigate("__all")}>{t("iris.allOptions")}</Button>}
      </div>
    </div>}
    <div className="iris-options-scroll" ref={content} onScroll={event => {
      const scroll = event.currentTarget.scrollTop; draftRef.current = { ...draftRef.current, scroll };
      if (snapshotRef.current) drafts.set(draftKey(snapshotRef.current), draftRef.current);
    }}>
      {!snapshot && busy && <p role="status">{t("iris.loading")}</p>}
      {snapshot && <div className="iris-options-grid" data-columns={active?.columns ?? 1}>
        {draft.screen === "__all" || !active ? <>{profileControl("all")}{snapshot.options.map(optionControl)}</> : active.elements.map((element, index) => {
          if (element.kind === "option") { const option = optionMap.get(element.option); return option ? optionControl(option) : <p role="alert" key={index}>{t("iris.missingOption", { option: element.option })}</p>; }
          if (element.kind === "profile") return profileControl(String(index));
          if (element.kind === "link") return <Button key={`link-${index}`} className="iris-screen-link" variant="outline" onClick={() => navigate(element.screen)}>{element.label}<ChevronRight aria-hidden="true" /></Button>;
          return <div className="iris-menu-space" key={`space-${index}`} aria-hidden="true" />;
        })}
      </div>}
      {snapshot && !snapshot.options.length && <p>{t("iris.noOptions")}</p>}
    </div>
    <footer className="iris-window-footer">
      {feedback && <p role={failed ? "alert" : "status"} className={failed ? "iris-error" : ""}>{feedback}</p>}
      <span role="status">{busy && snapshot ? t("iris.saving") : dirty ? t("iris.unsaved") : t("iris.noChanges")}</span>
      <div className="iris-settings-actions">
        <Button variant="outline" disabled={!snapshot || busy} onClick={() => { if (snapshot) changeValues(Object.fromEntries(snapshot.options.map(option => [option.id, option.default_value]))); }}><RotateCcw aria-hidden="true" />{t("iris.restore")}</Button>
        <Button disabled={busy || !dirty} onClick={() => void apply()}>{t("iris.save")}</Button>
      </div>
    </footer>
  </main>;
}
