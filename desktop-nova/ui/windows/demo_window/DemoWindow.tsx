import { useEffect, useState } from "react";

import {
    applyThemeStylesheet,
    currentThemeId,
    subscribeToThemeChanges,
    themeClassName
} from "../../shell/themeRuntime";

/**
 * Minimal standalone window used to verify child-window theme styling.
 */
export function DemoWindow() {
    const [themeId, setThemeId] = useState(currentThemeId());

    useEffect(() => {
        applyThemeStylesheet(themeId);
    }, [themeId]);

    useEffect(() => {
        setThemeId(currentThemeId());
        return subscribeToThemeChanges(setThemeId);
    }, []);

    return (
        <main
            className={["page-pad", themeClassName(themeId)].filter(Boolean).join(" ")}
            style={{
                minHeight: "100vh",
                boxSizing: "border-box",
                display: "flex",
                flexDirection: "column",
                gap: 12,
                background: "var(--surface)",
                color: "var(--text)"
            }}
        >
            <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <div>
                    <h2 style={{ margin: 0 }}>子窗口样式演示</h2>
                    <p className="muted" style={{ margin: "6px 0 0" }}>
                        这个窗口独立于遮罩层，用于检查独立子窗口的主题、背景和边框效果。
                    </p>
                </div>
            </div>
            <div
                style={{
                    flex: 1,
                    minHeight: 320,
                    border: "1px dashed var(--border)",
                    background: "var(--surface-elevated)",
                    borderRadius: 8
                }}
            />
        </main>
    );
}