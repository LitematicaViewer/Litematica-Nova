import { emitEvent, listenEvent } from "../../src/platform/events";
import { synchronizeApplicationWindowBackground } from "../../src/platform/nativeWindowGeometry";
import { DEFAULT_THEME_ID, THEMES, normalizeThemeId as normalizeManifestThemeId, themeKeyOf } from "../../src/services/themeManifest";

/** The theme list lives in `src/services/themeManifest.ts`; this module only applies it. */
export const defaultThemeId = DEFAULT_THEME_ID;
export type ThemeId = import("../../src/services/themeManifest").ThemeId;
export type ColorMode = "light" | "dark" | "system";

const themeStylesheetLinkId = "litematicanova-theme-css";
const themeStorageKey = "litematicanova-theme-id";
const themeChangedEvent = "litematicanova-theme-changed";
let memoryThemeId: ThemeId | undefined;
const colorModeStorageKey = "litematicanova-color-mode";
const colorModeChangedEvent = "litematicanova-color-mode-changed";
let memoryColorMode: ColorMode | undefined;
let systemColorQuery: MediaQueryList | undefined;
let backgroundSyncFrame: number | undefined;

const synchronizeNativeBackground = () => {
    if (backgroundSyncFrame !== undefined) cancelAnimationFrame(backgroundSyncFrame);
    backgroundSyncFrame = requestAnimationFrame(() => {
        backgroundSyncFrame = undefined;
        // Resolve CSS colors through the browser, including color()/oklch(),
        // instead of relying on the native API's narrower string parser.
        const swatch = document.createElement("canvas");
        swatch.width = swatch.height = 1;
        const context = swatch.getContext("2d");
        if (!context) return;
        context.fillStyle = getComputedStyle(document.documentElement).backgroundColor;
        context.fillRect(0, 0, 1, 1);
        const [red, green, blue, alpha] = context.getImageData(0, 0, 1, 1).data;
        // Preserve the actual theme color beneath premultiplied native child
        // surfaces. Do not replace the renderer's transparent clear with a fill.
        synchronizeApplicationWindowBackground([red, green, blue, alpha]).catch(() => undefined);
    });
};

export const normalizeColorMode = (mode: unknown): ColorMode =>
    mode === "light" || mode === "dark" ? mode : "system";

const storedColorMode = () => {
    try { return window.localStorage.getItem(colorModeStorageKey); }
    catch { return null; }
};

export const currentColorMode = (): ColorMode => memoryColorMode ?? normalizeColorMode(storedColorMode());

/** System changes update every mounted document without changing the saved preference. */
const applyColorMode = () => {
    if (!systemColorQuery && window.matchMedia) {
        systemColorQuery = window.matchMedia("(prefers-color-scheme: dark)");
        systemColorQuery.addEventListener("change", applyColorMode);
    }
    const mode = currentColorMode();
    const resolved = mode === "system" ? (systemColorQuery?.matches ? "dark" : "light") : mode;
    document.documentElement.dataset.colorMode = mode;
    document.documentElement.dataset.colorScheme = resolved;
    document.documentElement.classList.toggle("dark", resolved === "dark");
    synchronizeNativeBackground();
};

const cacheColorMode = (mode: unknown) => {
    const normalized = normalizeColorMode(mode);
    memoryColorMode = normalized;
    try {
        if (window.localStorage.getItem(colorModeStorageKey) !== normalized) {
            window.localStorage.setItem(colorModeStorageKey, normalized);
        }
    } catch { /* Native window events still synchronize storage-denied windows. */ }
    return normalized;
};

export const announceColorModeChange = (mode: string) => {
    const normalized = cacheColorMode(mode);
    applyColorMode();
    window.dispatchEvent(new CustomEvent(colorModeChangedEvent, { detail: normalized }));
    emitEvent(colorModeChangedEvent, normalized).catch(() => undefined);
    return normalized;
};

const subscribeToColorModeChanges = (callback: (mode: ColorMode) => void) => {
    let previous = currentColorMode();
    let active = true;
    const apply = (mode: unknown) => {
        if (!active) return;
        const next = cacheColorMode(mode);
        applyColorMode();
        if (next !== previous) { previous = next; callback(next); }
    };
    const onStorage = (event: StorageEvent) => {
        if (event.key === colorModeStorageKey && event.newValue) apply(storedColorMode());
    };
    const onCustom = (event: Event) => apply((event as CustomEvent).detail);
    window.addEventListener("storage", onStorage);
    window.addEventListener(colorModeChangedEvent, onCustom);
    const unlisten = listenEvent<string>(colorModeChangedEvent, (event) => apply(event.payload)).catch(() => undefined);
    return () => {
        active = false;
        window.removeEventListener("storage", onStorage);
        window.removeEventListener(colorModeChangedEvent, onCustom);
        unlisten.then((off) => off?.());
    };
};

const themeStylesheetUrls = Object.entries(
    (import.meta as ImportMeta & { glob: <T>(pattern: string, options: Record<string, unknown>) => Record<string, T> }).glob<string>("../themes/*/theme.css", {
        eager: true,
        import: "default",
        query: "?url"
    })
).reduce<Record<string, string>>((themes, [path, url]) => {
    const themeKey = path.match(/\.\.\/themes\/([^/]+)\//)?.[1];
    if (themeKey) themes[themeKey] = url;
    return themes;
}, {});

export const themeResourceKey = (themeId: string) => themeKeyOf(themeId);

/** Canonical ids do not depend on which stylesheets happen to be installed. */
export const normalizeThemeId = (themeId: unknown): ThemeId => normalizeManifestThemeId(themeId);

export const themeClassName = (themeId: string) => `theme-${themeResourceKey(normalizeThemeId(themeId))}`;

const stylesheetReady = new Map<string, Promise<void>>();

/** Preload each sheet once; inactive theme CSS must never affect the active theme. */
const ensureThemeStylesheets = () => {
    for (const theme of THEMES) {
        const key = theme.directory;
        const href = themeStylesheetUrls[key];
        if (!href || stylesheetReady.has(key)) continue;
        const link = document.createElement("link");
        link.id = `${themeStylesheetLinkId}-${key}`;
        link.rel = "stylesheet";
        link.media = "not all";
        stylesheetReady.set(key, new Promise<void>((resolve) => {
            link.onload = () => { resolve(); synchronizeNativeBackground(); };
            link.onerror = () => resolve();
        }));
        link.href = href;
        document.head.appendChild(link);
    }
};

/** Apply to the document as well as the React root, including body-hosted portals. */
export const applyThemeStylesheet = (themeId: string) => {
    const normalized = normalizeThemeId(themeId);
    const themeKey = themeResourceKey(normalized);
    document.documentElement.dataset.theme = themeKey;
    applyColorMode();
    for (const className of Array.from(document.body.classList)) {
        // Any `theme-*` class is ours, including ids from retired themes and any
        // theme added later without touching this file.
        if (className.startsWith("theme-")) {
            document.body.classList.remove(className);
        }
    }
    document.body.classList.add(themeClassName(normalized));
    ensureThemeStylesheets();
    for (const key of stylesheetReady.keys()) {
        const link = document.getElementById(`${themeStylesheetLinkId}-${key}`) as HTMLLinkElement | null;
        if (link) link.media = key === themeKey ? "all" : "not all";
    }
    synchronizeNativeBackground();
};

const storedThemeId = () => {
    try {
        return window.localStorage.getItem(themeStorageKey) || window.localStorage.getItem("theme");
    } catch {
        return null;
    }
};

/** Memory also works when storage is unavailable in a native child window. */
export const currentThemeId = (fallbackThemeId = defaultThemeId): ThemeId =>
    memoryThemeId ?? normalizeThemeId(storedThemeId() || fallbackThemeId);

const cacheThemeId = (themeId: unknown) => {
    const normalized = normalizeThemeId(themeId);
    memoryThemeId = normalized;
    try {
        if (window.localStorage.getItem(themeStorageKey) !== normalized) {
            window.localStorage.setItem(themeStorageKey, normalized);
        }
    } catch {
        // The native event channel and memory cache remain available.
    }
    return normalized;
};

/** User changes are broadcast; receiving them never broadcasts again. */
export const announceThemeChange = (themeId: string) => {
    const normalized = cacheThemeId(themeId);
    applyThemeStylesheet(normalized);
    window.dispatchEvent(new CustomEvent(themeChangedEvent, { detail: normalized }));
    emitEvent(themeChangedEvent, normalized).catch(() => undefined);
    return normalized;
};

/** Coalesce native/storage echoes, but keep main and child windows bidirectional. */
export const subscribeToThemeChanges = (callback: (themeId: ThemeId) => void, onColorModeChange: (mode: ColorMode) => void = () => {}) => {
    const offColorMode = subscribeToColorModeChanges(onColorModeChange);
    let lastThemeId = currentThemeId();
    let active = true;
    const apply = (themeId: unknown) => {
        if (!active) return;
        const normalized = cacheThemeId(themeId);
        applyThemeStylesheet(normalized);
        if (normalized !== lastThemeId) {
            lastThemeId = normalized;
            callback(normalized);
        }
    };
    const onStorage = (event: StorageEvent) => {
        if (event.key !== themeStorageKey || !event.newValue) return;
        // A queued storage event can arrive after a newer native event. Read
        // the current shared value instead of resurrecting its stale payload.
        const latest = storedThemeId();
        if (latest) apply(latest);
    };
    const onCustom = (event: Event) => {
        const detail = (event as CustomEvent<string>).detail;
        if (typeof detail === "string") apply(detail);
    };
    window.addEventListener("storage", onStorage);
    window.addEventListener(themeChangedEvent, onCustom);
    const unlistenPromise = listenEvent<string>(themeChangedEvent, (event) => apply(event.payload)).catch(() => undefined);
    return () => {
        active = false;
        offColorMode();
        window.removeEventListener("storage", onStorage);
        window.removeEventListener(themeChangedEvent, onCustom);
        unlistenPromise.then((unlisten) => unlisten?.());
    };
};

/** Before React's first frame, prefer live cache in children and disk on app startup. */
export const initializeWindowTheme = async (
    readSavedConfig: () => Promise<{ config: { theme: string; color_mode?: string } }>,
    preferSavedConfig = false,
) => {
    const cached = memoryThemeId !== undefined || storedThemeId() !== null;
    applyThemeStylesheet(currentThemeId());
    if (cached && !preferSavedConfig) {
        await Promise.all(stylesheetReady.values());
        return;
    }
    let receivedChange = false;
    let receivedColorChange = false;
    const unsubscribe = subscribeToThemeChanges(() => { receivedChange = true; }, () => { receivedColorChange = true; });
    try {
        const info = await readSavedConfig();
        if (!receivedChange) announceThemeChange(info.config.theme);
        if (!receivedColorChange) announceColorModeChange(info.config.color_mode || "system");
    } catch {
        // Cached/default styling still opens the app if the config command fails.
    } finally {
        await Promise.all(stylesheetReady.values());
        unsubscribe();
    }
};
