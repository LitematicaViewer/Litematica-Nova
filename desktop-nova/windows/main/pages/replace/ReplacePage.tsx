import { useState } from "react";
import type { ActiveFile } from "../../../../shell/types";
import { runReplaceBlocks } from "../../../../services/novaBackendAdapter";

const defaultRules = JSON.stringify(
    [
        {
            from: { name: "minecraft:stone", properties: {} },
            to: { name: "minecraft:stone_bricks", properties: {} }
        }
    ],
    null,
    2
);

export function ReplacePage({ activeFile }: { activeFile: ActiveFile | null }) {
    const [rulesJson, setRulesJson] = useState(defaultRules);
    const [outputPath, setOutputPath] = useState("");
    const [log, setLog] = useState("");
    const [busy, setBusy] = useState(false);

    const run = async (dryRun: boolean) => {
        setBusy(true);
        setLog("");
        try {
            JSON.parse(rulesJson);
            if (!activeFile?.path) {
                throw new Error("请先选择 .litematic 文件。");
            }
            if (!outputPath.trim()) {
                throw new Error("请填写输出 .litematic 路径。");
            }
            const result = await runReplaceBlocks(activeFile.path, rulesJson, outputPath.trim(), dryRun);
            setLog(result);
        } catch (error) {
            setLog(String(error));
        } finally {
            setBusy(false);
        }
    };

    return (
        <section className="page-pad replace-page">
            <div className="toolbar">
                <h2>方块替换</h2>
                <span className="muted">Nova 外观，调用当前 Litematica-BA replace-blocks 后端。</span>
            </div>
            <p>{activeFile ? activeFile.path : "请先在首页或属性页选择 .litematic。"}</p>
            <fieldset>
                <legend>替换规则 JSON</legend>
                <p className="muted">block id 与 properties 保持英文，中文只用于界面显示。</p>
                <textarea
                    style={{ width: "100%", minHeight: 220, fontFamily: "monospace" }}
                    value={rulesJson}
                    onChange={(event) => setRulesJson(event.target.value)}
                />
            </fieldset>
            <fieldset>
                <legend>输出</legend>
                <div className="filter-row">
                    <input
                        placeholder="C:\\path\\to\\replaced.litematic"
                        value={outputPath}
                        onChange={(event) => setOutputPath(event.target.value)}
                    />
                    <button type="button" disabled={busy || !activeFile} onClick={() => run(true)}>Dry-run</button>
                    <button type="button" disabled={busy || !activeFile} onClick={() => run(false)}>Apply</button>
                </div>
            </fieldset>
            <fieldset>
                <legend>后端输出</legend>
                <pre style={{ whiteSpace: "pre-wrap", minHeight: 120 }}>{log || "尚未运行。"}</pre>
            </fieldset>
        </section>
    );
}
