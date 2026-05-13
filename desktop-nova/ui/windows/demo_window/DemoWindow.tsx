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
        <main className={["subwindow-standalone-page", themeClassName(themeId)].filter(Boolean).join(" ")}>
            <div className="subwindow-title-bar">
                <div className="subwindow-title-stack">
                    <h2 className="subwindow-title">子窗口样式演示</h2>
                    <p className="muted subwindow-subtitle">
                        这个窗口独立于遮罩层，用于检查独立子窗口的主题、背景和边框效果。
                    </p>
                </div>
            </div>
            <div className="subwindow-body">
                <div className="subwindow-demo-fill" />
            </div>
        </main>
    );
}
