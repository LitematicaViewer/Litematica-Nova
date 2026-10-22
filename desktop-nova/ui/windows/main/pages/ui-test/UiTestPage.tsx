import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { Checkbox } from "../../../../components/ui/checkbox";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Slider } from "../../../../components/ui/slider";
import { RadioGroup, RadioGroupItem } from "../../../../components/ui/radio-group";
import { Input } from "../../../../components/ui/input";
import { Button } from "../../../../components/ui/button";
import { TableHeader, TableRow, TableHead, TableBody, TableCell, Table } from "../../../../components/ui/table";
import React, { useEffect, useState } from "react";

import { Dialog } from "../../../../components/Dialog";
import { Dropdown } from "../../../../components/Dropdown";
import {
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  openUiDemoWindow,
} from "../../../../../src/business/facade";
import { currentThemeId, subscribeToThemeChanges, themeResourceKey } from "../../../../shell/themeRuntime";

const fallbackThumbExampleUrl = new URL("../../../../shell/resource/thumb_example.png", import.meta.url).href;

// The button-variant gallery cycles each demo through these three states.
type VariantDemoState = "normal" | "check" | "disabled";
const VARIANT_DEMO_CYCLE: VariantDemoState[] = ["normal", "check", "disabled"];
const VARIANT_DEMO_STATE_LABELS: Record<VariantDemoState, string> = {
  normal: "ui.normal",
  check: "ui.pressed",
  disabled: "ui.disabled",
};
const BUTTON_VARIANT_DEMOS = [
  { variant: "default", labelKey: "ui.primary" },
  { variant: "secondary", labelKey: "ui.secondary" },
  { variant: "outline", labelKey: "ui.outline" },
  { variant: "ghost", labelKey: "ui.ghost" },
  { variant: "link", labelKey: "ui.link" },
  { variant: "destructive", labelKey: "ui.destructive" },
] as const;

const themeThumbExampleUrls = Object.entries(
  import.meta.glob<string>("../../../../themes/*/resource/thumb_example.png", {
    eager: true,
    import: "default",
    query: "?url",
  }),
).reduce<Record<string, string>>((thumbUrls, [path, url]) => {
  const themeKey = path.match(/themes\/([^/]+)\//)?.[1];
  if (themeKey) {
    thumbUrls[themeKey] = url;
  }
  return thumbUrls;
}, {});

const thumbExampleUrlForTheme = (themeId: string) =>
  themeThumbExampleUrls[themeResourceKey(themeId)] || fallbackThumbExampleUrl;

function FormRow({ label, children }: { label: string; children: React.ReactNode }) {
  const { locale } = useI18n();
  return (
    <div className="form-row">
      <label>{label}</label>
      <div>{children}</div>
    </div>
  );
}

/**
 * Renders the theme/widget smoke-test page used to validate shell styling.
 */
export function UiTestPage() {
  const { locale } = useI18n();
  const [themeId, setThemeId] = useState(() => currentThemeId());
  const [showDemoOverlay, setShowDemoOverlay] = useState(false);
  const [showTemplateDialog, setShowTemplateDialog] = useState(false);
  const [selectedTableRow, setSelectedTableRow] = useState("");
  const [selectedOption, setSelectedOption] = useState("first");
  const [sampleTheme, setSampleTheme] = useState("shadcn");
  const [sampleTemplate, setSampleTemplate] = useState("template-a");
  const [variantDemoStates, setVariantDemoStates] = useState<Record<string, VariantDemoState>>({});

  // Each variant button cycles through the three states a theme can style, so the
  // pressed (`aria-pressed`) and disabled textures can be inspected, not just hover.
  const advanceVariantDemo = (variant: string) => {
    setVariantDemoStates((current) => {
      const now = current[variant] ?? "normal";
      const next = VARIANT_DEMO_CYCLE[(VARIANT_DEMO_CYCLE.indexOf(now) + 1) % VARIANT_DEMO_CYCLE.length];
      return { ...current, [variant]: next };
    });
  };
  useEffect(() => subscribeToThemeChanges(setThemeId), []);

  const thumbExampleUrl = thumbExampleUrlForTheme(themeId);
  const renderThumbExample = () =>
    thumbExampleUrl ? <img className="ui-test-thumb" src={thumbExampleUrl} alt={t("ui.thumbnail")} /> : <span />;

  const handleOpenDemoWindow = async () => {
    const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
    const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
    if (behavior === "independent_window") {
      try {
        await openUiDemoWindow();
        return;
      } catch {
        window.open("demo_window.html", "_blank", "popup=yes,width=720,height=520");
        return;
      }
    }
    setShowDemoOverlay(true);
  };

  return (
    <ScrollArea className="ui-control-gallery">
      <h2>{t("ui.title")}</h2>
      <p className="muted">{t("ui.subtitle")}</p>
      <fieldset>
        <legend>{t("ui.form")}</legend>
        <FormRow label={t("ui.radio")}>
          <RadioGroup defaultValue="a" aria-label={t("ui.radioExample")}>
            <label className="setting-row"><RadioGroupItem value="a" /> {t("ui.optionA")}</label>
            <label className="setting-row"><RadioGroupItem value="b" /> {t("ui.optionB")}</label>
          </RadioGroup>
        </FormRow>
        <FormRow label={t("ui.checkbox")}>
          <label><Checkbox  /> {t("ui.checkboxExample")}</label>
        </FormRow>
        <FormRow label={t("ui.singleLine")}><Input defaultValue={t("ui.defaultInput")} /></FormRow>
        <FormRow label={t("ui.dropdown")}>
          <Dropdown value={sampleTheme} onChange={setSampleTheme} ariaLabel={t("ui.themeExample")} options={[
            {value: "shadcn", label: "shadcn/ui"}, {value: "8bitcn", label: "8bitcn/ui"}, {value: "Minecraft", label: "Minecraft"}, {value: "Metro10", label: "Metro10"},
          ]} />
        </FormRow>
        <FormRow label={t("ui.keyboardDropdown")}>
          <Dropdown ariaLabel={t("ui.keyboardDropdownExample")} value={selectedOption} onChange={setSelectedOption} options={[
            { value: "first", label: t("ui.first") },
            { value: "second", label: t("ui.second") },
            { value: "third", label: t("ui.third") },
          ]} />
        </FormRow>
        <FormRow label={t("ui.slider")}><Slider defaultValue={[40]} aria-label={t("ui.sliderExample")} /></FormRow>
        <FormRow label={t("ui.button")}>
          <div className="button-row">
            <Button type="button">{t("ui.regularButton")}</Button>
            <Button type="button" aria-pressed="true">{t("ui.checkable")}</Button>
            <Button type="button" disabled>{t("ui.disabled")}</Button>
          </div>
        </FormRow>
        {/* Every `variant` in buttonVariants is shown here. `outline` and `link` have no
            business use yet, so without a gallery case their theme rules would be dead
            CSS nobody could check.
            Each button cycles 常态 → 按下 → 禁用 so all three states can be inspected;
            previously only the resting state (and its hover) was reachable. A disabled
            <button> swallows its own clicks, hence the wrapper as the click target. */}
        <FormRow label={t("ui.variantTitle")}>
          <div className="button-row">
            {BUTTON_VARIANT_DEMOS.map((entry) => {
              const state = variantDemoStates[entry.variant] ?? "normal";
              return (
                <span
                  key={entry.variant}
                  className="button-variant-demo"
                  title={t("ui.variantHelp", {variant:t(entry.labelKey), state:t(VARIANT_DEMO_STATE_LABELS[state])})}
                  onClick={() => advanceVariantDemo(entry.variant)}
                >
                  <Button
                    type="button"
                    variant={entry.variant}
                    aria-pressed={state === "check"}
                    disabled={state === "disabled"}
                  >
                    {t(entry.labelKey)}
                  </Button>
                </span>
              );
            })}
          </div>
        </FormRow>
        <FormRow label={t("ui.dialog")}>
          <div className="button-row">
            <Button type="button" onClick={() => setShowTemplateDialog(true)}>{t("ui.openDialog")}</Button>
          </div>
        </FormRow>
        <FormRow label={t("ui.subwindow")}>
          <div className="button-row">
            <Button type="button" onClick={handleOpenDemoWindow}>{t("ui.openDemo")}</Button>
          </div>
        </FormRow>
      </fieldset>
      <fieldset>
        <legend>{t("ui.table")}</legend>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("ui.tableThumbnail")}</TableHead>
              <TableHead>{t("ui.text")}</TableHead>
              <TableHead>{t("ui.data")}</TableHead>
              <TableHead>{t("ui.emptyColumn")}</TableHead>
              <TableHead>{t("ui.actions")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {[
              ["ui.number", "999"],
              ["ui.largeNumber", "99,999"],
              ["ui.decimal", "9.99"],
              ["ui.percent", "99%"],
              ["ui.decimalPercent", "99.99%"],
              ["ui.currency", "$99.00"],
            ].map(([label, value], index) => {
              const isSelected = selectedTableRow === label;
              return (
                <TableRow
                  key={label}
                  className={isSelected ? "table-row-selected" : ""}
                  aria-selected={isSelected}
                  onClick={() => setSelectedTableRow(label)}
                >
                  <TableCell>{renderThumbExample()}</TableCell>
                  <TableCell>{t(label)}</TableCell>
                  <TableCell>{value}</TableCell>
                  <TableCell />
                  <TableCell>
                    {index < 3 ? (
                      <div className="button-row">
                        <Button type="button" disabled={index === 1} aria-pressed={index === 2}>
                          {index === 1 ? t("ui.unavailableButton") : index === 2 ? t("ui.selectableButton") : t("ui.testButtonOne")}
                        </Button>
                        <Button type="button">{t("ui.testButtonTwo")}</Button>
                      </div>
                    ) : null}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </fieldset>
      {showTemplateDialog ? (
        <Dialog
          title={t("ui.dialogTemplate")}
          subtitle={t("ui.dialogSubtitle")}
          width="md"
          onClose={() => setShowTemplateDialog(false)}
          footer={(
            <>
              <Button type="button" onClick={() => setShowTemplateDialog(false)}>{t("editor.cancel")}</Button>
              <Button type="button" onClick={() => setShowTemplateDialog(false)}>{t("ui.done")}</Button>
            </>
          )}
        >
          <div className="dialog-form-grid">
            <span className="dialog-label">{t("ui.headingArea")}</span>
            <span className="muted">{t("ui.headingHint")}</span>
            <span className="dialog-label">{t("ui.bodyArea")}</span>
            <div className="dialog-field-stack">
              <Input defaultValue={t("ui.inputExample")} />
              <Dropdown ariaLabel={t("ui.templateExample")} value={sampleTemplate} onChange={setSampleTemplate} options={[
                {value: "template-a", label: t("ui.templateA")}, {value: "template-b", label: t("ui.templateB")},
              ]} />
              <label className="setting-row"><Checkbox  defaultChecked /> {t("ui.inlineBoolean")}</label>
            </div>
            <span className="dialog-label">{t("ui.explanationArea")}</span>
            <div className="dialog-message">{t("ui.messageHint")}</div>
          </div>
        </Dialog>
      ) : null}
      {showDemoOverlay && (
        <Dialog title={t("ui.demoTitle")} onClose={() => setShowDemoOverlay(false)}>
            <div className="subwindow-body">
              <div className="muted">{t("ui.demoHint")}</div>
              <div className="subwindow-demo-fill" />
            </div>
        </Dialog>
      )}
    </ScrollArea>
  );
}
