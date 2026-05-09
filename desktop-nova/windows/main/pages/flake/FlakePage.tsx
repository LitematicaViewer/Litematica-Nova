import type { ActiveFile } from "../../../../shell/types";

export function FlakePage({
    activeFile,
    onOpenMaterialList
}: {
    activeFile: ActiveFile | null;
    onOpenMaterialList: () => void;
}) {
    return (
        <section className="page-pad flake-page">
            <p>{activeFile ? activeFile.path : "请在“属性”页加载 .litematic。"}</p>
            <div className="filter-row">
                <label>子区域：</label>
                <select>
                    <option>整个投影</option>
                </select>
            </div>
            <fieldset>
                <legend>层级控制（Y 轴俯视切片）</legend>
                <div className="layer-control">
                    <label>层索引：</label>
                    <input type="range" min="0" max="0" defaultValue="0" />
                    <button type="button">重置视角</button>
                    <span />
                    <span>层 Y = 0</span>
                    <div className="button-row">
                        <button type="button">--</button>
                        <button type="button">-</button>
                        <button type="button">+</button>
                        <button type="button">++</button>
                    </div>
                </div>
            </fieldset>
            <div className="slice-canvas">请先在“渲染”页构建 3D cache。</div>
            <p className="muted">分层数据尚未加载。</p>
            <button type="button" onClick={onOpenMaterialList}>材料列表（当前区域）</button>
        </section>
    );
}
