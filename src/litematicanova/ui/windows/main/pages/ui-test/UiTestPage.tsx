import { FormRow } from "../common/FormRow";

export function UiTestPage() {
    return (
        <section className="page-pad">
            <h2>UI 测试页（title）</h2>
            <p className="muted">主题与排版自检（subtitle）</p>
            <fieldset>
                <legend>表单</legend>
                <FormRow label="单选：">
                    <label><input type="radio" name="sample" defaultChecked /> 选项 A</label>
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
        </section>
    );
}