import React from "react";
import type { LucideIcon } from "lucide-react";
import { currentThemeId, themeResourceKey } from "../shell/themeRuntime";

/**
 * Functional icons that a theme may override.
 *
 * Themes ship optional sets under `resource/icon/button/` and `resource/icon/tool/`
 * (`svg` or `png`). A theme that has the file replaces the generic lucide glyph;
 * a theme that does not keeps the `fallback` icon. Missing files are therefore not
 * an error: dropping `<name>.svg` into a theme folder is enough to light it up
 * after the next build, with no call-site change.
 *
 * SVG art is deliberately treated as a *mask*, not an `<img>`: theme SVGs are
 * drawn with `currentColor`, and an `<img>` cannot inherit the button's colour
 * (it would stay black in dark mode and on filled variants). Masking paints the
 * glyph with `currentColor`, so it follows the label through light/dark, hover,
 * pressed and primary/destructive fills. PNG art (the pixel themes) stays a real
 * image so its colours survive.
 */

export type ThemeIconSet = "button" | "tool";

type IconAsset = { url: string; format: "svg" | "png" };

const assetPatterns: Record<ThemeIconSet, Record<string, Record<string, IconAsset>>> = {
    button: mapAssets(import.meta.glob<string>("../themes/*/resource/icon/button/*.{png,svg}", {
        eager: true,
        import: "default",
        query: "?url",
    }), "button"),
    tool: mapAssets(import.meta.glob<string>("../themes/*/resource/icon/tool/*.{png,svg}", {
        eager: true,
        import: "default",
        query: "?url",
    }), "tool"),
};

function mapAssets(modules: Record<string, string>, set: ThemeIconSet) {
    const assets: Record<string, Record<string, IconAsset>> = {};
    for (const [path, url] of Object.entries(modules)) {
        const match = path.match(new RegExp(`themes/([^/]+)/resource/icon/${set}/([^/]+)\\.(svg|png)$`));
        if (!match) continue;
        const [, themeKey, name, format] = match;
        (assets[themeKey] ||= {})[name] = { url, format: format as IconAsset["format"] };
    }
    return assets;
}

/** The active theme's asset for an icon, if that theme ships one. */
export function resolveThemeIcon(set: ThemeIconSet, theme: string, name: string): IconAsset | undefined {
    return assetPatterns[set][themeResourceKey(theme)]?.[name];
}

export interface ThemeIconProps {
    /** File name inside the theme's `icon/<set>/` folder, without extension. */
    name: string;
    set?: ThemeIconSet;
    /** Defaults to the active theme. */
    theme?: string;
    size?: number;
    className?: string;
    /** Rendered when the active theme has no asset for `name`. */
    fallback?: LucideIcon;
    strokeWidth?: number;
    "aria-hidden"?: boolean | "true" | "false";
}

export function ThemeIcon({
    name,
    set = "button",
    theme,
    size = 16,
    className,
    fallback: Fallback,
    strokeWidth,
    "aria-hidden": ariaHidden = true,
}: ThemeIconProps) {
    const asset = resolveThemeIcon(set, theme ?? currentThemeId(), name);

    if (!asset) {
        return Fallback
            ? <Fallback size={size} className={className} strokeWidth={strokeWidth} aria-hidden={ariaHidden} focusable={false} />
            : null;
    }

    if (asset.format === "svg") {
        return <span
            className={["theme-icon", className].filter(Boolean).join(" ")}
            aria-hidden={ariaHidden}
            style={{ width: size, height: size, WebkitMaskImage: `url("${asset.url}")`, maskImage: `url("${asset.url}")` }}
        />;
    }

    return <img
        className={["theme-icon", "theme-icon-image", className].filter(Boolean).join(" ")}
        src={asset.url}
        width={size}
        height={size}
        alt=""
        aria-hidden={ariaHidden}
        draggable={false}
    />;
}
