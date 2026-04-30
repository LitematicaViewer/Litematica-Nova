import { useEffect, useState } from "react";
import { currentThemeId, subscribeToThemeChanges, themeResourceKey } from "../../../../shell/themeRuntime";
import { FormRow } from "../common/FormRow";

const fallbackThumbExampleUrl = new URL("../../../../shell/resource/thumb_example.png", import.meta.url).href;

const themeThumbExampleUrls = Object.entries(
    import.meta.glob<string>("../../../../themes/*/resource/thumb_example.png", {
        eager: true,
        import: "default",
        query: "?url"
    })
).reduce<Record<string, string>>((thumbUrls, [path, url]) => {
    const themeKey = path.match(/themes\/([^/]+)\//)?.[1];
    if (themeKey) {
        thumbUrls[themeKey] = url;
    }
    return thumbUrls;
}, {});

const thumbExampleUrlForTheme = (themeId: string) =>
    themeThumbExampleUrls[themeResourceKey(themeId)] || fallbackThumbExampleUrl;

export function UiTestPage() {
    const [themeId, setThemeId] = useState(() => currentThemeId());
    useEffect(() => subscribeToThemeChanges(setThemeId), []);

    const thumbExampleUrl = thumbExampleUrlForTheme(themeId);
    const renderThumbExample = () => <img className="ui-test-thumb" src={thumbExampleUrl} alt="示例缩略图" />;
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
                    <select><option>QTDefault</option><option>Metro10</option><option>Minecraft</option></select>
                </FormRow>
                <FormRow label="滑动条："><input type="range" defaultValue="40" /></FormRow>
                <FormRow label="按钮：">
                    <div className="button-row">
                        <button type="button">普通按钮</button>
                        <button type="button" aria-pressed="true">可勾选</button>
                        <button type="button" disabled>禁用</button>
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
                        <tr>
                            <td>{renderThumbExample()}</td>
                            <td>常规数字</td>
                            <td>999</td>
                            <td></td>
                            <td>
                                <div className="button-row">
                                    <button type="button">测试按钮1</button>
                                    <button type="button">测试按钮2</button>
                                </div>
                            </td>
                        </tr>
                        <tr>
                            <td>{renderThumbExample()}</td>
                            <td>大数字</td>
                            <td>99,999</td>
                            <td></td>
                            <td>
                                <div className="button-row">
                                    <button type="button" disabled>不可用按钮1</button>
                                    <button type="button">测试按钮2</button>
                                </div>
                            </td>
                        </tr>
                        <tr>
                            <td>{renderThumbExample()}</td>
                            <td>小数点</td>
                            <td>9.99</td>
                            <td></td>
                            <td>
                                <div className="button-row">
                                    <button type="button" aria-pressed="true">可选中按钮1</button>
                                    <button type="button">测试按钮2</button>
                                </div>
                            </td>
                        </tr>
                        <tr>
                            <td>{renderThumbExample()}</td>
                            <td>百分比</td>
                            <td>99%</td>
                            <td></td>
                            <td></td>
                        </tr>
                        <tr>
                            <td>{renderThumbExample()}</td>
                            <td>带小数点的百分比</td>
                            <td>99.99%</td>
                            <td></td>
                            <td></td>
                        </tr>
                        <tr>
                            <td>{renderThumbExample()}</td>
                            <td>货币</td>
                            <td>$99.00</td>
                            <td></td>
                            <td></td>
                        </tr>
                    </tbody>
                </table>
            </fieldset>
        </section>
    );
}