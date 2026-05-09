import { useState } from "react";
import { runProjectionPlan } from "../../../../services/novaBackendAdapter";

const defaultPlan = JSON.stringify(
    {
        version: 1,
        metadata: {
            name: "Nova Generated Projection",
            author: "Litematica-BA",
            description: "Generated from the Nova UI adapter."
        },
        minecraft_data_version: 3953,
        regions: [
            {
                name: "main",
                origin: [0, 0, 0],
                size: [8, 4, 8],
                operations: [
                    {
                        type: "floor",
                        from: [0, 0, 0],
                        to: [7, 0, 7],
                        block: { name: "minecraft:stone", properties: {} }
                    }
                ]
            }
        ]
    },
    null,
    2
);

export function GeneratePage() {
    const [planJson, setPlanJson] = useState(defaultPlan);
    const [outputPath, setOutputPath] = useState("");
    const [log, setLog] = useState("");
    const [busy, setBusy] = useState(false);

    const run = async (dryRun: boolean) => {
        setBusy(true);
        setLog("");
        try {
            JSON.parse(planJson);
            if (!outputPath.trim()) {
                throw new Error("请填写输出 .litematic 路径。");
            }
            const result = await runProjectionPlan(planJson, outputPath.trim(), dryRun);
            setLog(result);
        } catch (error) {
            setLog(String(error));
        } finally {
            setBusy(false);
        }
    };

    return (
        <section className="page-pad generate-page">
            <div className="toolbar">
                <h2>生成投影</h2>
                <span className="muted">Nova 外观，调用当前 Litematica-BA generate 后端。</span>
            </div>
            <fieldset>
                <legend>Plan JSON</legend>
                <p className="muted">字段、block id、property key/value 必须保持英文；这里不会把中文写入 plan。</p>
                <textarea
                    style={{ width: "100%", minHeight: 280, fontFamily: "monospace" }}
                    value={planJson}
                    onChange={(event) => setPlanJson(event.target.value)}
                />
            </fieldset>
            <fieldset>
                <legend>输出</legend>
                <div className="filter-row">
                    <input
                        placeholder="C:\\path\\to\\output.litematic"
                        value={outputPath}
                        onChange={(event) => setOutputPath(event.target.value)}
                    />
                    <button type="button" disabled={busy} onClick={() => run(true)}>Dry-run</button>
                    <button type="button" disabled={busy} onClick={() => run(false)}>Apply</button>
                </div>
            </fieldset>
            <fieldset>
                <legend>后端输出</legend>
                <pre style={{ whiteSpace: "pre-wrap", minHeight: 120 }}>{log || "尚未运行。"}</pre>
            </fieldset>
        </section>
    );
}
