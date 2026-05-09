import { emit, listen } from "@tauri-apps/api/event";

export const webDefaultThemeId = "WebDefault";

const legacyDefaultThemeId = "QTDefault";
const themeStylesheetLinkId = "litematicanova-theme-css";
const themeStorageKey = "litematicanova-theme-id";
const themeChangedEvent = "litematicanova-theme-changed";

const themeStylesheetUrls = Object.entries(
    import.meta.glob<string>("../themes/*/theme.css", {
        eager: true,
        import: "default",
        query: "?url"
    })
).reduce<Record<string, string>>((themes, [path, url]) => {
    const themeKey = path.match(/\.\.\/themes\/([^/]+)\//)?.[1];
    if (themeKey) {
        themes[themeKey] = url;
    }
    return themes;
}, {});

/**
 * Converts a theme id into the normalized resource key used by theme asset maps.
 */
export const themeResourceKey = (themeId: string) => themeId.trim().toLowerCase();

/**
 * Resolves legacy and unknown theme ids to a theme supported by the web UI.
 */
export const normalizeThemeId = (themeId: string) => {
    const trimmedThemeId = themeId.trim();
    const themeKey = themeResourceKey(trimmedThemeId);
    if (!trimmedThemeId || trimmedThemeId === legacyDefaultThemeId || themeKey === themeResourceKey(webDefaultThemeId)) {
        return webDefaultThemeId;
    }
    return themeStylesheetUrls[themeKey] ? trimmedThemeId : webDefaultThemeId;
};

/**
 * Returns the root class name that activates theme-scoped CSS rules.
 */
export const themeClassName = (themeId: string) => {
    const themeKey = themeResourceKey(normalizeThemeId(themeId));
    return themeStylesheetUrls[themeKey] ? `theme-${themeKey}` : "";
};

const themeStylesheetUrl = (themeId: string) =>
    themeStylesheetUrls[themeResourceKey(normalizeThemeId(themeId))];

/**
 * Applies the selected theme stylesheet to the current window document.
 */
export const applyThemeStylesheet = (themeId: string) => {
    const href = themeStylesheetUrl(themeId);
    const existingLink = document.getElementById(themeStylesheetLinkId) as HTMLLinkElement | null;

    if (!href) {
        existingLink?.remove();
        return;
    }

    if (existingLink) {
        existingLink.href = href;
        return;
    }

    const link = document.createElement("link");
    link.id = themeStylesheetLinkId;
    link.rel = "stylesheet";
    link.href = href;
    document.head.appendChild(link);
};

/**
 * Reads the most recent theme selected by any UI window.
 */
export const currentThemeId = (fallbackThemeId = webDefaultThemeId) => {
    try {
        return normalizeThemeId(window.localStorage.getItem(themeStorageKey) || fallbackThemeId);
    } catch {
        return normalizeThemeId(fallbackThemeId);
    }
};

const persistThemeId = (themeId: string) => {
    const normalizedThemeId = normalizeThemeId(themeId);
    try {
        window.localStorage.setItem(themeStorageKey, normalizedThemeId);
    } catch {
        // Storage may be unavailable in some test/browser contexts.
    }
    return normalizedThemeId;
};

/**
 * Stores and broadcasts a theme change so secondary windows can follow the shell.
 */
export const announceThemeChange = (themeId: string) => {
    const normalizedThemeId = persistThemeId(themeId);
    window.dispatchEvent(new CustomEvent(themeChangedEvent, { detail: normalizedThemeId }));
    emit(themeChangedEvent, normalizedThemeId).catch(() => undefined);
    return normalizedThemeId;
};

/**
 * Subscribes to theme changes from local storage, same-window events, and Tauri window events.
 */
export const subscribeToThemeChanges = (callback: (themeId: string) => void) => {
    const apply = (themeId: string) => callback(normalizeThemeId(themeId));
    const onStorage = (event: StorageEvent) => {
        if (event.key === themeStorageKey && event.newValue) {
            apply(event.newValue);
        }
    };
    const onCustom = (event: Event) => {
        const detail = (event as CustomEvent<string>).detail;
        if (detail) {
            apply(detail);
        }
    };

    window.addEventListener("storage", onStorage);
    window.addEventListener(themeChangedEvent, onCustom);
    const unlistenPromise = listen<string>(themeChangedEvent, (event) => apply(event.payload)).catch(() => undefined);

    return () => {
        window.removeEventListener("storage", onStorage);
        window.removeEventListener(themeChangedEvent, onCustom);
        unlistenPromise.then((unlisten) => unlisten?.());
    };
};
