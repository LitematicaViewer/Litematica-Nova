import type { ActiveFile, MetricRow } from "../../../../shell/types";
import { FormRow } from "../common/FormRow";

export function StatisticsPage({
    activeFile,
    metrics,
    onOpenMaterialList
}: {
    activeFile: ActiveFile | null;
    metrics: MetricRow[];
    onOpenMaterialList: () => void;
}) {
    return (
        <section className="page-pad">
            <button className="wide-button" type="button" onClick={onOpenMaterialList}>材料列表</button>
            <p className="muted">
                密度分母为「最后一个子区域」的包围格数（与旧版 LitematicaViewer 一致）；
                非空气方块数 num 为全部子区域累计。
            </p>
            <p>{activeFile ? activeFile.path : "请在「属性」页加载 .litematic。"}</p>
            <div className="toolbar">
                <label><input type="checkbox" /> 统计实体（排除掉落物/蝙蝠/经验球/潜影弹）</label>
                <span className="toolbar-spacer" />
                <button type="button">重新统计</button>
            </div>
            <fieldset>
                <legend>统计指标</legend>
                {metrics.map((metric) => (
                    <FormRow key={metric.label} label={metric.label}>
                        <input readOnly value={metric.value} />
                    </FormRow>
                ))}
            </fieldset>
        </section>
    );
}