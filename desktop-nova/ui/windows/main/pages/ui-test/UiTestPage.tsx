import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { Checkbox } from "../../../../components/ui/checkbox";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Slider } from "../../../../components/ui/slider";
import { RadioGroup, RadioGroupItem } from "../../../../components/ui/radio-group";
import { Input } from "../../../../components/ui/input";
import { Button } from "../../../../components/ui/button";
import { TableHeader, TableRow, TableHead, TableBody, TableCell, Table } from "../../../../components/ui/table";
import React, { useEffect, useId, useState } from "react";

import { Dialog } from "../../../../components/Dialog";
import { Dropdown } from "../../../../components/Dropdown";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { Box, ChevronDown, Download, FileText, FolderOpen, LayoutGrid, List, Search, X } from "lucide-react";
import "./ui-test.css";
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
  const id = useId();
  const input = React.isValidElement(children) && children.type === Input;
  return (
    <div className="form-row">
      {input ? <label htmlFor={id}>{label}</label> : <span id={id}>{label}</span>}
      <div role={input ? undefined : "group"} aria-labelledby={input ? undefined : id}>
        {input ? React.cloneElement(children as React.ReactElement<{id?: string}>, {id}) : children}
      </div>
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
  const [cardSelected, setCardSelected] = useState(false);
  const [libraryFilter, setLibraryFilter] = useState("all");
  const [librarySort, setLibrarySort] = useState("recent");

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
          <div className="button-row">
            <label className="setting-row"><Checkbox /> {t("ui.checkboxExample")}</label>
            <label className="setting-row"><Checkbox defaultChecked="indeterminate" /> {locale === "en-US" ? "Partially selected" : "部分选中"}</label>
            <label className="setting-row"><Checkbox defaultChecked disabled /> {t("ui.disabled")}</label>
          </div>
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
            {/* The icon specimen exists so every theme can be checked against one
                canonical in-button icon size (shared `svg.lucide` / `--nova-icon-size`). */}
            <Button type="button"><FolderOpen size={16} aria-hidden="true" />{t("ui.iconButton")}</Button>
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
                  role={state === "disabled" ? "button" : undefined}
                  tabIndex={state === "disabled" ? 0 : undefined}
                  aria-label={state === "disabled" ? t("ui.variantHelp", {variant:t(entry.labelKey), state:t(VARIANT_DEMO_STATE_LABELS[state])}) : undefined}
                  title={t("ui.variantHelp", {variant:t(entry.labelKey), state:t(VARIANT_DEMO_STATE_LABELS[state])})}
                  onClick={() => advanceVariantDemo(entry.variant)}
                  onKeyDown={event => { if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) { event.preventDefault(); advanceVariantDemo(entry.variant); } }}
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
      {/* 卡片与面板是各页面复用的两个容器原语。这里给出完整解剖与状态，
          主题只需覆盖 `[data-slot="card"]` / `[data-slot="panel"]` 及其部件。 */}
      <fieldset>
        <legend>{t("ui.cards")}</legend>
        <div className="ui-test-card-grid">
          <article className="nova-card" data-slot="card">
            <h3 data-slot="card-title">{t("ui.cardTitle")}</h3>
            <p data-slot="card-body">{t("ui.cardBody")}</p>
            <div data-slot="card-meta">
              <span>{t("ui.cardMeta")}</span>
              <span>{t("ui.cardMetaSize")}</span>
            </div>
            <div data-slot="card-actions">
              <Button type="button">{t("ui.cardAction")}</Button>
              <Button type="button" variant="outline">{t("ui.cardActionAlt")}</Button>
            </div>
          </article>

          <article className="nova-card" data-slot="card">
            <img data-slot="card-media" src={thumbExampleUrl} alt="" />
            <h3 data-slot="card-title">{t("ui.cardMediaTitle")}</h3>
            <p data-slot="card-body">{t("ui.cardMediaBody")}</p>
          </article>

          <article className="nova-card" data-slot="card" data-variant="subtle">
            <span data-slot="card-stat">1,248</span>
            <span data-slot="card-body">{t("ui.cardStatLabel")}</span>
            <div data-slot="card-meta"><span>{t("ui.cardStatMeta")}</span></div>
          </article>

          <Button
            type="button"
            themeSlot="card"
            variant="ghost"
            className="nova-card"
            data-interactive
            aria-pressed={cardSelected}
            onClick={() => setCardSelected(!cardSelected)}
          >
            <span data-slot="card-title">{t("ui.cardInteractiveTitle")}</span>
            <span data-slot="card-body">{t("ui.cardInteractiveBody")}</span>
            <span data-slot="card-meta">{cardSelected ? t("ui.cardSelected") : t("ui.cardSelectHint")}</span>
          </Button>

          <article className="nova-card ui-test-empty-card" data-slot="card" data-variant="subtle">
            <Box size={28} aria-hidden="true" />
            <p data-slot="card-body">{t("ui.cardEmptyBody")}</p>
            <Button type="button" variant="outline">{t("ui.cardEmptyAction")}</Button>
          </article>
        </div>
      </fieldset>
      <fieldset>
        <legend>{t("ui.panels")}</legend>
        <div className="ui-test-panel-grid">
          <section className="nova-panel" data-slot="panel">
            <header data-slot="panel-header">
              <div>
                <h3 data-slot="panel-title">{t("ui.panelTitle")}</h3>
                <p data-slot="panel-subtitle">{t("ui.panelSubtitle")}</p>
              </div>
              <div data-slot="panel-actions">
                <Button type="button" variant="ghost" size="icon-sm" aria-label={t("ui.panelClose")}>
                  <X size={16} aria-hidden="true" />
                </Button>
              </div>
            </header>
            <div data-slot="panel-body">
              <div data-slot="panel-row"><span data-slot="panel-label">{t("ui.panelLabelSize")}</span><span data-slot="panel-value">{t("ui.panelValueSize")}</span></div>
              <div data-slot="panel-row"><span data-slot="panel-label">{t("ui.panelLabelCount")}</span><span data-slot="panel-value">{t("ui.panelValueCount")}</span></div>
              <div data-slot="panel-row"><span data-slot="panel-label">{t("ui.panelLabelState")}</span><span data-slot="panel-value">{t("ui.panelValueState")}</span></div>
            </div>
            <footer data-slot="panel-footer">
              <span className="muted">{t("ui.panelFooterHint")}</span>
              <div data-slot="panel-actions">
                <Button type="button" variant="outline">{t("ui.panelCancel")}</Button>
                <Button type="button">{t("ui.panelApply")}</Button>
              </div>
            </footer>
          </section>

          <details className="nova-panel" data-slot="panel" data-collapsible>
            <summary data-slot="panel-header">
              <div>
                <h3 data-slot="panel-title">{t("ui.panelCollapsibleTitle")}</h3>
                <p data-slot="panel-subtitle">{t("ui.panelCollapsibleSubtitle")}</p>
              </div>
              <div data-slot="panel-actions">
                <ChevronDown size={16} aria-hidden="true" data-slot="panel-chevron" />
              </div>
            </summary>
            <div data-slot="panel-body">
              <p className="muted">{t("ui.panelCollapsibleBody")}</p>
              <div data-slot="panel-row"><span data-slot="panel-label">{t("ui.panelLabelRows")}</span><span data-slot="panel-value">128</span></div>
            </div>
          </details>

          <section className="nova-panel" data-slot="panel" data-variant="subtle">
            <header data-slot="panel-header">
              <div>
                <h3 data-slot="panel-title">{t("ui.panelToolbarTitle")}</h3>
                <p data-slot="panel-subtitle">{t("ui.panelToolbarSubtitle")}</p>
              </div>
            </header>
            <div data-slot="panel-body">
              <div className="ui-test-panel-toolbar">
                <Button type="button">{t("ui.panelToolbarPrimary")}</Button>
                <Button type="button" variant="outline">{t("ui.panelToolbarSecondary")}</Button>
                <Button type="button" variant="ghost">{t("ui.panelToolbarGhost")}</Button>
                <span className="ui-test-panel-grow" />
                <span data-slot="panel-label">{t("ui.panelToolbarHint")}</span>
              </div>
              <ul className="ui-test-panel-list">
                <li><span data-slot="panel-label">{t("ui.panelRowOne")}</span><span data-slot="panel-value">1,024</span></li>
                <li><span data-slot="panel-label">{t("ui.panelRowTwo")}</span><span data-slot="panel-value">256</span></li>
                <li><span data-slot="panel-label">{t("ui.panelRowThree")}</span><span data-slot="panel-value">64</span></li>
              </ul>
            </div>
          </section>
        </div>
      </fieldset>
      {/* 投影库的内容样式：工具条、分类行、磁贴、列表行、空态与详情区。
          直接复用 library-* 契约，主题与页面样式改动会在这一节同步显形。 */}
      <fieldset>
        <legend>{t("ui.librarySection")}</legend>
        <div className="library-page">
        <div className="library-toolbar">
          <h2>{t("ui.libraryTitle")}</h2>
          <Button type="button"><ThemeIcon name="import" size={16} fallback={FolderOpen} />{t("ui.libraryImport")}</Button>
          <div className="library-toolbar-second-row">
            <div className="library-search">
              <ThemeIcon name="search" size={16} fallback={Search} />
              <Input type="search" aria-label={t("ui.librarySearch")} placeholder={t("ui.librarySearch")} />
            </div>
            <span className="library-view-switch" role="group" aria-label={t("ui.libraryViews")}>
              <Button type="button" size="icon" variant="ghost" aria-pressed="true" aria-label={t("ui.libraryGridView")}><LayoutGrid size={16} aria-hidden="true" /></Button>
              <Button type="button" size="icon" variant="ghost" aria-label={t("ui.libraryListView")}><List size={16} aria-hidden="true" /></Button>
              <Button type="button" size="icon" variant="ghost" aria-label={t("ui.libraryDetailView")}><FileText size={16} aria-hidden="true" /></Button>
            </span>
            <div className="library-filter-control">
              <Dropdown ariaLabel={t("ui.libraryFilter")} value={libraryFilter} onChange={setLibraryFilter} options={[
                { value: "all", label: t("ui.libraryFilterAll") },
                { value: "ok", label: t("ui.libraryFilterOk") },
                { value: "missing", label: t("ui.libraryFilterMissing") },
              ]} />
            </div>
            <div className="library-filter-control">
              <Dropdown ariaLabel={t("ui.librarySort")} value={librarySort} onChange={setLibrarySort} options={[
                { value: "recent", label: t("ui.librarySortRecent") },
                { value: "name", label: t("ui.librarySortName") },
              ]} />
            </div>
          </div>
        </div>

        <div className="ui-test-library-grid">
          <aside className="library-categories" aria-label={t("ui.libraryCategories")}>
            <div className="library-pane-heading">
              <h3>{t("ui.libraryCategories")}</h3>
              <span>12</span>
            </div>
            <nav className="library-category-list" aria-label={t("ui.libraryCategories")}>
              <Button type="button" variant="ghost" themeSlot="library-category" className="library-category" aria-pressed="true">
                <ThemeIcon name="all" size={16} fallback={LayoutGrid} /><span>{t("ui.libraryAll")}</span><small>12</small>
              </Button>
              <Button type="button" variant="ghost" themeSlot="library-category" className="library-category">
                <ThemeIcon name="folder" size={16} fallback={FolderOpen} /><span>{t("ui.libraryFolder")}</span><small>7</small>
              </Button>
              <Button type="button" variant="ghost" themeSlot="library-category" className="library-category">
                <ThemeIcon name="unknown" size={16} fallback={Box} /><span>{t("ui.libraryUntagged")}</span><small>5</small>
              </Button>
            </nav>
          </aside>

          <div className="ui-test-library-content">
            <div className="ui-test-library-tiles">
              <article className="library-tile" data-inspected>
                <div className="library-tile-selection">
                  <Checkbox aria-label={t("ui.librarySelectTile")} />
                </div>
                <span className="library-current-marker">{t("ui.libraryCurrent")}</span>
                <Button type="button" variant="ghost" themeSlot="library-tile" className="library-tile-open" aria-pressed="true">
                  <span className="library-tile-image"><img src={thumbExampleUrl} alt="" /></span>
                  <span className="library-tile-name">{t("ui.libraryTileName")}</span>
                  <span className="library-tile-meta">{t("ui.libraryTileMeta")}</span>
                </Button>
                <span className="library-replica-badge" data-replica-count="2" title={t("ui.libraryReplica")} aria-label={t("ui.libraryReplica")}>2</span>
              </article>

              <article className="library-tile">
                <div className="library-tile-selection">
                  <Checkbox aria-label={t("ui.librarySelectTile")} />
                </div>
                <Button type="button" variant="ghost" themeSlot="library-tile" className="library-tile-open">
                  <span className="library-tile-image">
                    <span className="library-preview-placeholder">
                      <ThemeIcon name="picture_error" size={30} fallback={Box} />
                      <span>{t("ui.libraryPreviewMissing")}</span>
                    </span>
                  </span>
                  <span className="library-tile-name">{t("ui.libraryTileBroken")}</span>
                  <span className="library-tile-meta">{t("ui.libraryFileMissing")}</span>
                </Button>
              </article>
            </div>

            <Table className="library-list-table" data-view="list">
              <TableHeader>
                <TableRow>
                  <TableHead className="library-list-select-col"><Checkbox checked="indeterminate" aria-label={t("ui.librarySelectPage")} /></TableHead>
                  <TableHead>{t("ui.libraryColumnName")}</TableHead>
                  <TableHead>{t("ui.libraryColumnReplicas")}</TableHead>
                  <TableHead>{t("ui.libraryColumnBlocks")}</TableHead>
                  <TableHead>{t("ui.libraryColumnAuthor")}</TableHead>
                  <TableHead>{t("ui.libraryColumnStatus")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow className="library-list-row" data-inspected aria-selected>
                  <TableCell className="library-list-select-col"><Checkbox aria-label={t("ui.librarySelectTile")} /></TableCell>
                  <TableCell><span className="library-list-name">{t("ui.libraryTileName")}</span><span className="library-replica-chip" data-replica-count="2" title={t("ui.libraryReplica")}>×2</span></TableCell>
                  <TableCell>2</TableCell>
                  <TableCell>10,044</TableCell>
                  <TableCell>CharaDust</TableCell>
                  <TableCell>{t("ui.libraryStatusOk")}</TableCell>
                </TableRow>
                <TableRow className="library-list-row">
                  <TableCell className="library-list-select-col"><Checkbox aria-label={t("ui.librarySelectTile")} /></TableCell>
                  <TableCell><span className="library-list-name">{t("ui.libraryTileBroken")}</span></TableCell>
                  <TableCell>1</TableCell>
                  <TableCell>—</TableCell>
                  <TableCell>—</TableCell>
                  <TableCell>{t("ui.libraryFileMissing")}</TableCell>
                </TableRow>
              </TableBody>
            </Table>

            <div className="library-empty-state">
              <Box size={40} aria-hidden="true" />
              <h3>{t("ui.libraryEmptyTitle")}</h3>
              <p>{t("ui.libraryEmptyBody")}</p>
              <Button type="button">{t("ui.libraryImport")}</Button>
            </div>

            <div className="library-detail-content">
              <div className="library-detail-preview"><img src={thumbExampleUrl} alt="" /></div>
              <h3 className="library-detail-name">{t("ui.libraryTileName")}</h3>
              <p className="library-detail-author">{t("ui.libraryAuthorLine")}</p>
              <dl className="library-detail-facts">
                <div><dt>{t("ui.libraryFactSize")}</dt><dd>31 × 12 × 27</dd></div>
                <div><dt>{t("ui.libraryFactBlocks")}</dt><dd>10,044</dd></div>
                <div><dt>{t("ui.libraryFactVersion")}</dt><dd>Minecraft Java 1.21.8</dd></div>
                <div><dt>{t("ui.libraryFactReplicas")}</dt><dd>2</dd></div>
              </dl>
              <div className="library-detail-actions">
                <Button type="button">{t("ui.libraryOpen")}</Button>
                <Button type="button" variant="outline"><Download size={16} aria-hidden="true" />{t("ui.libraryExport")}</Button>
              </div>
            </div>
          </div>
        </div>
        </div>
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
                  tabIndex={0}
                  onClick={() => setSelectedTableRow(isSelected ? "" : label)}
                  onKeyDown={event => { if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) { event.preventDefault(); setSelectedTableRow(isSelected ? "" : label); } }}
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
              <Input aria-label={t("ui.inputExample")} defaultValue={t("ui.inputExample")} />
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
