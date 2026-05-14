import React, { useEffect, useState } from "react";

import { Dialog } from "../../../../components/Dialog";
import {
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  openUiDemoWindow,
} from "../../../../../src/business/facade";
import { currentThemeId, subscribeToThemeChanges, themeResourceKey } from "../../../../shell/themeRuntime";

const fallbackThumbExampleUrl = new URL("../../../../shell/resource/thumb_example.png", import.meta.url).href;

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
    <section className="page-pad">
      <h2>UI 测试页（title）</h2>
      <p className="muted">主题与排版自检（subtitle）</p>
      <fieldset>
        <legend>表单</legend>
        <FormRow label="单选：">
          <label className="setting-row"><input type="radio" name="sample" defaultChecked /> 选项 A</label>
          <label className="setting-row"><input type="radio" name="sample" /> 选项 B</label>
        </FormRow>
        <FormRow label="复选：">
          <label><input type="checkbox" /> 示例复选框</label>
        </FormRow>
        <FormRow label="单行输入："><input defaultValue="默认系统输入框" /></FormRow>
        <FormRow label="下拉框：">
          <select defaultValue="QTDefault">
            <option>QTDefault</option>
            <option>Metro10</option>
            <option>Minecraft</option>
          </select>
        </FormRow>
        <FormRow label="滑动条："><input type="range" defaultValue="40" /></FormRow>
        <FormRow label="按钮：">
          <div className="button-row">
            <button type="button">普通按钮</button>
            <button type="button" aria-pressed="true">可勾选</button>
            <button type="button" disabled>禁用</button>
          </div>
        </FormRow>
        <FormRow label="对话框：">
          <div className="button-row">
            <button type="button" onClick={() => setShowTemplateDialog(true)}>打开标准对话框模板</button>
          </div>
        </FormRow>
        <FormRow label="子窗口：">
          <div className="button-row">
            <button type="button" onClick={handleOpenDemoWindow}>打开空的演示窗口</button>
          </div>
        </FormRow>
      </fieldset>
      <fieldset>
        <legend>表格</legend>
        <table>
          <thead>
            <tr>
              <th>缩略图</th>
              <th>文字</th>
              <th>数据</th>
              <th>空列</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["常规数字", "999"],
              ["大数字", "99,999"],
              ["小数点", "9.99"],
              ["百分比", "99%"],
              ["带小数点的百分比", "99.99%"],
              ["货币", "$99.00"],
            ].map(([label, value], index) => (
              <tr key={label}>
                <td>{renderThumbExample()}</td>
                <td>{label}</td>
                <td>{value}</td>
                <td />
                <td>
                  {index < 3 ? (
                    <div className="button-row">
                      <button type="button" disabled={index === 1} aria-pressed={index === 2}>
                        {index === 1 ? "不可用按钮1" : index === 2 ? "可选中按钮1" : "测试按钮1"}
                      </button>
                      <button type="button">测试按钮2</button>
                    </div>
                  ) : null}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </fieldset>
      {showTemplateDialog ? (
        <Dialog
          title="标准对话框模板"
          subtitle="用于普通确认、表单和轻量交互；它始终是主窗口内遮罩，不提供独立窗口模式。"
          width="md"
          onClose={() => setShowTemplateDialog(false)}
          footer={(
            <>
              <button type="button" onClick={() => setShowTemplateDialog(false)}>取消</button>
              <button type="button">主要操作</button>
            </>
          )}
        >
          <div className="dialog-form-grid">
            <span className="dialog-label">标题区</span>
            <span className="muted">上方包含标题、副标题和关闭按钮。</span>
            <span className="dialog-label">正文区</span>
            <div className="dialog-field-stack">
              <input defaultValue="可直接放输入框、选择框和说明文案" />
              <select defaultValue="template-a">
                <option value="template-a">模板选项 A</option>
                <option value="template-b">模板选项 B</option>
              </select>
              <label className="setting-row"><input type="checkbox" defaultChecked /> 支持内联布尔选项</label>
            </div>
            <span className="dialog-label">说明区</span>
            <div className="dialog-message">这里适合放路径预览、确认文案、结果提示或错误信息。</div>
          </div>
        </Dialog>
      ) : null}
      {showDemoOverlay && (
        <div className="dialog-overlay" onClick={() => setShowDemoOverlay(false)}>
          <div
            className="dialog-content subwindow-frame"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="subwindow-title-bar">
              <h3 className="subwindow-title">子窗口样式演示</h3>
              <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={() => setShowDemoOverlay(false)}>×</button>
            </div>
            <div className="subwindow-body">
              <div className="muted">当前按“子窗口行为”设置，以主窗口遮罩方式打开这个演示窗口。</div>
              <div className="subwindow-demo-fill" />
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
