import React, { useEffect, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { getUserConfig, saveUserConfig } from "../../../../../src/platform/appData";
import { listenEvent } from "../../../../../src/platform/events";
import { useI18n } from "../../../../i18n";
import { UI_SCALE_PRESETS, parseUiScaleDraft, stepUiScale } from "./uiScaleInput";

export function UiScaleControl() {
  const { locale } = useI18n();
  const en = locale === "en-US";
  const [value, setValue] = useState(1);
  const [input, setInput] = useState("1.00");
  const [error, setError] = useState("");
  const queue = React.useRef<Promise<unknown>>(Promise.resolve());
  useEffect(() => {
    let active = true;
    const accept = (next: number) => { if (active) { setValue(next); setInput(next.toFixed(2)); } };
    const listening = listenEvent<number>("nova-ui-scale-changed", event => accept(event.payload));
    void getUserConfig().then(info => accept(info.config.ui_scale ?? 1)).catch(reason => setError(String(reason)));
    return () => { active = false; void listening.then(off => off()); };
  }, []);
  const save = (next: number) => {
    if (!Number.isFinite(next) || next < .5 || next > 5 || Math.abs(next * 100 - Math.round(next * 100)) > .000001) {
      setError(en ? "Enter a value from 0.50 to 5.00 with at most two decimals." : "请输入 0.50–5.00 之间、最多两位小数的倍率。"); return;
    }
    setError("");
    // Serialize intentions; a slower older disk write cannot replace the latest zoom.
    queue.current = queue.current.catch(() => undefined).then(async () => {
      const info = await saveUserConfig({ ui_scale: next });
      setValue(info.config.ui_scale); setInput(info.config.ui_scale.toFixed(2));
    }).catch(reason => setError(String(reason)));
  };
  return <div className="form-row ui-scale-row">
    <label className="form-label" htmlFor="nova-ui-scale">{en ? "Interface scale" : "界面缩放"}</label>
    <div className="form-field ui-scale-field">
      <div className="ui-scale-controls">
        <Button aria-label={en ? "Decrease scale" : "缩小界面"} disabled={value <= .5} onClick={() => save(stepUiScale(value, -1) ?? NaN)}><Minus size={16} /></Button>
        <input id="nova-ui-scale" aria-label={en ? "Interface scale" : "界面缩放"} type="range" min="0.5" max="5" step="0.05" value={value} onChange={event => save(Number(event.target.value))} />
        <Button aria-label={en ? "Increase scale" : "放大界面"} disabled={value >= 5} onClick={() => save(stepUiScale(value, 1) ?? NaN)}><Plus size={16} /></Button>
        <input aria-label={en ? "Precise scale" : "精确倍率"} className="input ui-scale-value" type="text" inputMode="decimal" list="nova-ui-scale-presets" value={input} onChange={event => setInput(event.target.value)} onBlur={() => save(parseUiScaleDraft(input) ?? NaN)} onKeyDown={event => { if (event.key === "Enter") { event.currentTarget.blur(); } }} />
        <span aria-hidden="true">×</span>
        <datalist id="nova-ui-scale-presets">{UI_SCALE_PRESETS.map(preset => <option key={preset} value={preset.toFixed(2)} />)}</datalist>
        <Button onClick={() => save(1)}>{en ? "Reset" : "恢复默认"}</Button>
      </div>
      <small className="settings-help-text">{en ? "Scale all application text and controls without changing 3D precision. Reset anytime: Ctrl/⌘+Alt+0." : "调整全应用文字、按钮、导航与窗口内容大小；不会改变三维渲染精度。随时按 Ctrl/⌘+Alt+0 恢复默认。"}</small>
      {error && <span role="alert">{error}</span>}
    </div>
  </div>;
}
