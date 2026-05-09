import type { ActiveFile } from "../../../../shell/types";
import { FormRow } from "../common/FormRow";

const directions = ["西北", "北", "东北", "西", "顶视", "中心", "西南", "南", "东南"];

export function RenderPage({
    activeFile,
    onOpenMaterialList,
    onOpenViewer
}: {
    activeFile: ActiveFile | null;
    onOpenMaterialList: () => void;
    onOpenViewer: () => void;
}) {
    return (
        <section className="page-pad render-page">
            <p>{activeFile ? activeFile.path : "请先在首页或属性页加载 .litematic 文件。"}</p>
            <FormRow label="渲染后端" htmlFor="render-engine">
                <select id="render-engine" value="lba-native" disabled>
                    <option value="lba-native">Litematica-BA 原生 Viewer</option>
                </select>
            </FormRow>
            <div className="render-layout">
                <div className="render-main">
                    <FormRow label="区域" htmlFor="render-region">
                        <select id="render-region" value="all" disabled><option value="all">全部区域</option></select>
                    </FormRow>
                    <div className="button-row">
                        <button type="button" disabled={!activeFile} onClick={onOpenViewer}>打开弹窗 Viewer</button>
                        <button type="button" disabled={!activeFile} onClick={onOpenMaterialList}>材料列表</button>
                    </div>
                    <label>渲染视口</label>
                    <p className="muted">本轮保持 Nova 外观，先接 popup viewer；embedded viewer 后续接入。</p>
                    <div className="render-viewport">原生 Viewer 桥接</div>
                </div>
                <fieldset className="render-settings">
                    <legend>相机占位</legend>
                    <button type="button" disabled>重新加载 embedded 3D</button>
                    <label>观察方向</label>
                    <div className="direction-grid">
                        {directions.map((direction) => (
                            <button key={direction} type="button" disabled>{direction}</button>
                        ))}
                    </div>
                </fieldset>
            </div>
        </section>
    );
}
