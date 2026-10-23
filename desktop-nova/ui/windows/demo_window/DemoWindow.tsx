import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
import { useEffect, useLayoutEffect, useState } from "react";

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
  useI18n();
    const [themeId, setThemeId] = useState(currentThemeId);

    useLayoutEffect(() => {
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
                    <h2 className="subwindow-title">{t("library.demoTitle")}</h2>
                    <p className="muted subwindow-subtitle">
                        {t("library.demoDescription")}</p>
                </div>
            </div>
            <div className="subwindow-body">
                <div className="subwindow-demo-fill" />
            </div>
        </main>
    );
}
