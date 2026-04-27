import type { ActiveFile } from "../../../../shell/types";

const directions = ["西北", "北", "东北", "西", "顶视", "东", "西南", "南", "东南"];

export function RenderPage({ activeFile }: { activeFile: ActiveFile | null }) {
    return (
        <section className="page-pad render-page">
            <p>{activeFile ? activeFile.path : "请在「属性」页加载 .litematic。"}</p>
            <div className="filter-row">
                <label>渲染方案：</label>
                <select defaultValue="nbt-viewer">
                    <option value="deepslate">deepslate</option>
                    <option value="nbt-viewer">nbt-viewer</option>
                </select>
            </div>
            <div className="render-layout">
                <div className="render-main">
                    <div className="filter-row">
                        <label>选择区域：</label>
                        <select><option>全部区域</option></select>
                    </div>
                    <div className="button-row">
                        <button type="button">材料列表（当前区域）</button>
                        <button type="button">截屏...</button>
                        <button type="button">导出...</button>
                    </div>
                    <label>渲染界面</label>
                    <p className="muted">无可用文件。</p>
                    <div className="render-viewport">3D 预览</div>
                </div>
                <fieldset className="render-settings">
                    <legend>渲染设置</legend>
                    <button type="button">重新加载3D</button>
                    <label>观察方向</label>
                    <div className="direction-grid">
                        {directions.map((direction) => (
                            <button key={direction} type="button">{direction}</button>
                        ))}
                    </div>
                    <label>视角 FOV（0°=正交）</label>
                    <div className="filter-row">
                        <input type="range" min="0" max="110" defaultValue="70" />
                        <span>70</span>
                    </div>
                    <button type="button">恢复默认视角</button>
                </fieldset>
            </div>
        </section>
    );
}