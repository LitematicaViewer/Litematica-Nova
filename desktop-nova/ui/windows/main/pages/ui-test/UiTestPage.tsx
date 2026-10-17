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
  normal: "常态",
  check: "按下",
  disabled: "禁用",
};
const BUTTON_VARIANT_DEMOS = [
  { variant: "default", label: "主要" },
  { variant: "secondary", label: "次要" },
  { variant: "outline", label: "描边" },
  { variant: "ghost", label: "幽灵" },
  { variant: "link", label: "链接" },
  { variant: "destructive", label: "危险" },
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
    thumbExampleUrl ? <img className="ui-test-thumb" src={thumbExampleUrl} alt="示例缩略图" /> : <span />;

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
      <h2>UI 测试页（title）</h2>
      <p className="muted">主题与排版自检（subtitle）</p>
      <fieldset>
        <legend>表单</legend>
        <FormRow label="单选：">
          <RadioGroup defaultValue="a" aria-label="单选示例">
            <label className="setting-row"><RadioGroupItem value="a" /> 选项 A</label>
            <label className="setting-row"><RadioGroupItem value="b" /> 选项 B</label>
          </RadioGroup>
        </FormRow>
        <FormRow label="复选：">
          <label><Checkbox  /> 示例复选框</label>
        </FormRow>
        <FormRow label="单行输入："><Input defaultValue="默认系统输入框" /></FormRow>
        <FormRow label="下拉框：">
          <Dropdown value={sampleTheme} onChange={setSampleTheme} ariaLabel="主题名称下拉示例" options={[
            {value: "shadcn", label: "shadcn/ui"}, {value: "8bitcn", label: "8bitcn/ui"}, {value: "Minecraft", label: "Minecraft"},
          ]} />
        </FormRow>
        <FormRow label="键盘下拉框：">
          <Dropdown ariaLabel="键盘下拉框示例" value={selectedOption} onChange={setSelectedOption} options={[
            { value: "first", label: "第一项" },
            { value: "second", label: "第二项" },
            { value: "third", label: "第三项" },
          ]} />
        </FormRow>
        <FormRow label="滑动条："><Slider defaultValue={[40]} aria-label="滑动条示例" /></FormRow>
        <FormRow label="按钮：">
          <div className="button-row">
            <Button type="button">普通按钮</Button>
            <Button type="button" aria-pressed="true">可勾选</Button>
            <Button type="button" disabled>禁用</Button>
          </div>
        </FormRow>
        {/* Every `variant` in buttonVariants is shown here. `outline` and `link` have no
            business use yet, so without a gallery case their theme rules would be dead
            CSS nobody could check.
            Each button cycles 常态 → 按下 → 禁用 so all three states can be inspected;
            previously only the resting state (and its hover) was reachable. A disabled
            <button> swallows its own clicks, hence the wrapper as the click target. */}
        <FormRow label="按钮变体（点击循环 常态 → 按下 → 禁用）：">
          <div className="button-row">
            {BUTTON_VARIANT_DEMOS.map((entry) => {
              const state = variantDemoStates[entry.variant] ?? "normal";
              return (
                <span
                  key={entry.variant}
                  className="button-variant-demo"
                  title={`${entry.label} · 当前：${VARIANT_DEMO_STATE_LABELS[state]}（点击切换）`}
                  onClick={() => advanceVariantDemo(entry.variant)}
                >
                  <Button
                    type="button"
                    variant={entry.variant}
                    aria-pressed={state === "check"}
                    disabled={state === "disabled"}
                  >
                    {entry.label}
                  </Button>
                </span>
              );
            })}
          </div>
        </FormRow>
        <FormRow label="对话框：">
          <div className="button-row">
            <Button type="button" onClick={() => setShowTemplateDialog(true)}>打开标准对话框模板</Button>
          </div>
        </FormRow>
        <FormRow label="子窗口：">
          <div className="button-row">
            <Button type="button" onClick={handleOpenDemoWindow}>打开空的演示窗口</Button>
          </div>
        </FormRow>
      </fieldset>
      <fieldset>
        <legend>表格</legend>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>缩略图</TableHead>
              <TableHead>文字</TableHead>
              <TableHead>数据</TableHead>
              <TableHead>空列</TableHead>
              <TableHead>操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {[
              ["常规数字", "999"],
              ["大数字", "99,999"],
              ["小数点", "9.99"],
              ["百分比", "99%"],
              ["带小数点的百分比", "99.99%"],
              ["货币", "$99.00"],
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
                  <TableCell>{label}</TableCell>
                  <TableCell>{value}</TableCell>
                  <TableCell />
                  <TableCell>
                    {index < 3 ? (
                      <div className="button-row">
                        <Button type="button" disabled={index === 1} aria-pressed={index === 2}>
                          {index === 1 ? "不可用按钮1" : index === 2 ? "可选中按钮1" : "测试按钮1"}
                        </Button>
                        <Button type="button">测试按钮2</Button>
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
          title="标准对话框模板"
          subtitle="用于普通确认、表单和轻量交互；它始终是主窗口内遮罩，不提供独立窗口模式。"
          width="md"
          onClose={() => setShowTemplateDialog(false)}
          footer={(
            <>
              <Button type="button" onClick={() => setShowTemplateDialog(false)}>取消</Button>
              <Button type="button" onClick={() => setShowTemplateDialog(false)}>完成</Button>
            </>
          )}
        >
          <div className="dialog-form-grid">
            <span className="dialog-label">标题区</span>
            <span className="muted">上方包含标题、副标题和关闭按钮。</span>
            <span className="dialog-label">正文区</span>
            <div className="dialog-field-stack">
              <Input defaultValue="可直接放输入框、选择框和说明文案" />
              <Dropdown ariaLabel="模板选项示例" value={sampleTemplate} onChange={setSampleTemplate} options={[
                {value: "template-a", label: "模板选项 A"}, {value: "template-b", label: "模板选项 B"},
              ]} />
              <label className="setting-row"><Checkbox  defaultChecked /> 支持内联布尔选项</label>
            </div>
            <span className="dialog-label">说明区</span>
            <div className="dialog-message">这里适合放路径预览、确认文案、结果提示或错误信息。</div>
          </div>
        </Dialog>
      ) : null}
      {showDemoOverlay && (
        <Dialog title="子窗口样式演示" onClose={() => setShowDemoOverlay(false)}>
            <div className="subwindow-body">
              <div className="muted">当前按“子窗口行为”设置，以主窗口遮罩方式打开这个演示窗口。</div>
              <div className="subwindow-demo-fill" />
            </div>
        </Dialog>
      )}
    </ScrollArea>
  );
}
