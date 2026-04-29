import { useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent } from "react";

import {
    applyThemeStylesheet,
    currentThemeId,
    subscribeToThemeChanges,
    themeClassName
} from "../../shell/themeRuntime";

interface MaterialListWindowProps {
    initialFilePath?: string | null;
}

interface MaterialRow {
    id: string;
    name: string;
    total: number;
}

const invalidFilenameChars = /[<>:"/\\|?*\n\r\t]/g;

const filenameSafeSegment = (value: string) => {
    const safe = value.replace(invalidFilenameChars, "_").trim().replace(/[ .]+$/g, "");
    return safe || "export";
};

const basename = (path: string) => path.split(/[\\/]/).pop() || path;

const stripExtension = (name: string) => name.replace(/\.[^.]+$/, "");

const csvEscape = (value: string) => `"${value.replace(/"/g, "\"\"")}"`;

const exportTitle = (sourceName: string | null, regionName: string) => {
    if (!sourceName) {
        return "原理图的材料清单";
    }
    if (regionName === "全部区域") {
        return `原理图的材料清单 '${stripExtension(sourceName)}' (1 of 1 区域)`;
    }
    return `原理图的材料清单 '${stripExtension(sourceName)}' (选定区域：${regionName})`;
};

const textTable = (title: string, rows: MaterialRow[]) => {
    const itemWidth = Math.max(7, "Item".length, ...rows.map((row) => row.name.length));
    const totalWidth = Math.max(7, "Total".length, ...rows.map((row) => String(row.total).length));
    const titleInner = itemWidth + 1 + totalWidth;
    const sep = `+${"-".repeat(itemWidth)}+${"-".repeat(totalWidth)}+`;
    const textCell = (value: string, width: number) => (` ${value.trim()}`).slice(0, width).padEnd(width, " ");
    const dataRow = (item: string, total: number) =>
        `|${textCell(item, itemWidth)}|${String(total).padStart(totalWidth, " ")}|`;
    const header = `|${textCell("Item", itemWidth)}|${textCell("Total", totalWidth)}|`;

    return [
        sep,
        `|${textCell(title, titleInner)}|`,
        sep,
        header,
        sep,
        ...rows.map((row) => dataRow(row.name, row.total)),
        sep,
        header,
        sep
    ].join("\n") + "\n";
};

const parseCsvLine = (line: string) => {
    const cells: string[] = [];
    let cell = "";
    let quoted = false;
    for (let i = 0; i < line.length; i += 1) {
        const char = line[i];
        if (char === '"') {
            if (quoted && line[i + 1] === '"') {
                cell += '"';
                i += 1;
            } else {
                quoted = !quoted;
            }
        } else if (char === "," && !quoted) {
            cells.push(cell.trim());
            cell = "";
        } else {
            cell += char;
        }
    }
    cells.push(cell.trim());
    return cells;
};

const parseCsvRows = (text: string): MaterialRow[] => {
    const rows: MaterialRow[] = [];
    const lines = text.replace(/^\uFEFF/, "").split(/\r?\n/).filter((line) => line.trim());
    const start = lines[0]?.toLowerCase().includes("item") && lines[0]?.toLowerCase().includes("total") ? 1 : 0;
    for (const line of lines.slice(start)) {
        const [name, totalRaw] = parseCsvLine(line);
        const total = Number.parseInt(totalRaw, 10);
        if (!name || Number.isNaN(total)) {
            continue;
        }
        rows.push({ id: name, name, total });
    }
    return rows.sort((left, right) => right.total - left.total || left.name.localeCompare(right.name));
};

const downloadText = (filename: string, content: string, type: string) => {
    const url = URL.createObjectURL(new Blob([content], { type }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = filename;
    anchor.click();
    URL.revokeObjectURL(url);
};

/**
 * Standalone material list window that mirrors the main shell theme.
 */
export function MaterialListWindow({ initialFilePath = null }: MaterialListWindowProps) {
    const fileInputRef = useRef<HTMLInputElement | null>(null);
    const [themeId, setThemeId] = useState(currentThemeId());
    const [sourcePath, setSourcePath] = useState(initialFilePath || "");
    const [sourceName, setSourceName] = useState(initialFilePath ? basename(initialFilePath) : "");
    const [rows, setRows] = useState<MaterialRow[]>([]);
    const [multiplier, setMultiplier] = useState(1);
    const [includeEntities, setIncludeEntities] = useState(false);
    const [regionName, setRegionName] = useState("全部区域");
    const [exportFormat, setExportFormat] = useState<"csv" | "txt">("csv");
    const [status, setStatus] = useState(
        initialFilePath
            ? "已接收投影文件路径。材料扫描后端接入后可直接重新加载。"
            : "未选择文件。可以先打开窗口，再在这里选择 .litematic 或材料 CSV。"
    );

    useEffect(() => {
        applyThemeStylesheet(themeId);
    }, [themeId]);

    useEffect(() => {
        setThemeId(currentThemeId());
        return subscribeToThemeChanges(setThemeId);
    }, []);

    const displayRows = useMemo(
        () => rows.map((row) => ({ ...row, total: row.total * multiplier })),
        [rows, multiplier]
    );

    const totalMaterials = useMemo(
        () => displayRows.reduce((sum, row) => sum + row.total, 0),
        [displayRows]
    );

    const workbookLabel = sourceName || "未选择文件";

    const handleFileSelected = async (event: ChangeEvent<HTMLInputElement>) => {
        const file = event.target.files?.[0];
        if (!file) {
            return;
        }
        setSourceName(file.name);
        setSourcePath(file.name);
        setRegionName("全部区域");
        setRows([]);

        if (file.name.toLowerCase().endsWith(".csv")) {
            const parsedRows = parseCsvRows(await file.text());
            setRows(parsedRows);
            setStatus(parsedRows.length ? `已从 CSV 载入 ${parsedRows.length} 行材料。` : "CSV 中没有可识别的 Item/Total 行。");
        } else {
            setStatus("已选择投影文件。当前新 UI 已解除激活文件限制，扫描逻辑等待后端接入。设置后可重新加载。");
        }
        event.target.value = "";
    };

    const handleReload = () => {
        if (!sourceName) {
            setStatus("请先选择 .litematic 文件或材料 CSV。窗口本身不依赖主窗口激活文件。");
            return;
        }
        if (sourceName.toLowerCase().endsWith(".csv")) {
            setStatus("CSV 已载入。如需更新，请重新选择该 CSV 文件。 ");
            return;
        }
        setStatus("材料扫描后端尚未接入新 UI；窗口打开与文件选择流程已可独立使用。 ");
    };

    const handleExport = () => {
        if (!displayRows.length) {
            setStatus("没有可导出的材料数据。 ");
            return;
        }
        const now = new Date();
        const stamp = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}_${String(now.getHours()).padStart(2, "0")}.${String(now.getMinutes()).padStart(2, "0")}.${String(now.getSeconds()).padStart(2, "0")}`;
        const safeSource = filenameSafeSegment(stripExtension(sourceName || "export"));
        if (exportFormat === "csv") {
            const content = [
                "Item,Total",
                ...displayRows.map((row) => `${csvEscape(row.name)},${row.total}`)
            ].join("\n") + "\n";
            downloadText(`material_list_${safeSource}_${stamp}.csv`, content, "text/csv;charset=utf-8");
            return;
        }
        downloadText(
            `material_list_${safeSource}_${stamp}.txt`,
            textTable(exportTitle(sourceName || null, regionName), displayRows),
            "text/plain;charset=utf-8"
        );
    };

    return (
        <main className={["material-list-window", themeClassName(themeId)].filter(Boolean).join(" ")}>
            <input
                ref={fileInputRef}
                className="visually-hidden"
                type="file"
                accept=".litematic,.csv"
                onChange={handleFileSelected}
            />
            <section className="material-list-panel">
                <div className="filter-row material-list-source-row">
                    <label htmlFor="material-list-workbook">工作簿：</label>
                    <select id="material-list-workbook" value={workbookLabel} onChange={() => undefined}>
                        <option value={workbookLabel}>{workbookLabel}</option>
                    </select>
                    <button type="button" onClick={() => fileInputRef.current?.click()}>选择文件...</button>
                </div>
                <div className="filter-row">
                    <label htmlFor="material-list-region">区域：</label>
                    <select id="material-list-region" value={regionName} onChange={(event) => setRegionName(event.target.value)}>
                        <option>全部区域</option>
                    </select>
                    <label className="material-list-check">
                        <input
                            type="checkbox"
                            checked={includeEntities}
                            onChange={(event) => setIncludeEntities(event.target.checked)}
                        />
                        统计实体
                    </label>
                    <span className="toolbar-spacer" />
                    <button type="button" onClick={handleReload}>重新加载</button>
                </div>
                <div className="filter-row">
                    <label htmlFor="material-list-multiplier">倍率：</label>
                    <input
                        id="material-list-multiplier"
                        className="material-list-multiplier"
                        type="number"
                        min="1"
                        max="9999"
                        value={multiplier}
                        onChange={(event) => setMultiplier(Math.max(1, Number.parseInt(event.target.value || "1", 10)))}
                    />
                    <span className="muted">行数：{displayRows.length}，总数：{totalMaterials}</span>
                    <span className="toolbar-spacer" />
                    <label htmlFor="material-list-export-format">格式：</label>
                    <select
                        id="material-list-export-format"
                        value={exportFormat}
                        onChange={(event) => setExportFormat(event.target.value as "csv" | "txt")}
                    >
                        <option value="csv">CSV</option>
                        <option value="txt">文本</option>
                    </select>
                    <button type="button" onClick={handleExport}>写入文件...</button>
                </div>
                <p className="muted material-list-source-path">{sourcePath || "未选择文件"}</p>
                <div className="material-list-table-wrap">
                    <table className="material-list-table">
                        <thead>
                            <tr>
                                <th className="material-list-icon-col">图标</th>
                                <th>名称</th>
                                <th className="material-list-total-col">总数</th>
                            </tr>
                        </thead>
                        <tbody>
                            {displayRows.length ? displayRows.map((row) => (
                                <tr key={row.id}>
                                    <td className="material-list-icon-cell"><span className="material-icon-placeholder" /></td>
                                    <td>{row.name}</td>
                                    <td className="material-list-number">{row.total}</td>
                                </tr>
                            )) : (
                                <tr>
                                    <td colSpan={3} className="material-list-empty">
                                        暂无材料数据。请选择材料 CSV，或等待新 UI 接入 .litematic 扫描后点击“重新加载”。
                                    </td>
                                </tr>
                            )}
                        </tbody>
                    </table>
                </div>
                <p className="muted material-list-status">{status}</p>
            </section>
        </main>
    );
}
