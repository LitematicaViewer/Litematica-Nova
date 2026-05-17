import React from "react";
import type { SyntheticEvent } from "react";

import { normalizeThemeId, themeResourceKey, webDefaultThemeId } from "./themeRuntime";

type NavIconUrls = Record<string, string>;

const iconNameFromPath = (path: string) => path.split("/").pop()?.replace(/\.[^.]+$/, "") || "";

const shellNavIconUrls = Object.entries(
  import.meta.glob<string>("./resource/icon/nav/*.{svg,png}", {
    eager: true,
    import: "default",
    query: "?url",
  }),
).reduce<NavIconUrls>((icons, [path, url]) => {
  const iconName = iconNameFromPath(path);
  if (iconName) {
    icons[iconName] = url;
  }
  return icons;
}, {});

const themedNavIconUrls = Object.entries(
  import.meta.glob<string>("../themes/*/resource/icon/nav/*.{svg,png}", {
    eager: true,
    import: "default",
    query: "?url",
  }),
).reduce<Record<string, NavIconUrls>>((themes, [path, url]) => {
  const themeKey = path.match(/\.\.\/themes\/([^/]+)\//)?.[1];
  const iconName = iconNameFromPath(path);
  if (themeKey && iconName) {
    themes[themeKey] = themes[themeKey] || {};
    themes[themeKey][iconName] = url;
  }
  return themes;
}, {});

const fallbackNavIconUrl = (iconName: string) =>
  shellNavIconUrls[iconName] ||
  shellNavIconUrls.undefined ||
  themedNavIconUrls.metro10?.[iconName] ||
  themedNavIconUrls.metro10?.undefined ||
  "";

const navIconUrl = (themeId: string, iconName: string) => {
  const normalizedThemeId = normalizeThemeId(themeId);
  const themeKey = themeResourceKey(normalizedThemeId);
  if (themeKey === themeResourceKey(webDefaultThemeId)) {
    return fallbackNavIconUrl(iconName);
  }
  return themedNavIconUrls[themeKey]?.[iconName] || fallbackNavIconUrl(iconName);
};

function handleNavIconError(event: SyntheticEvent<HTMLImageElement>, iconName: string) {
  const fallbackUrl = fallbackNavIconUrl(iconName);
  if (fallbackUrl && event.currentTarget.src !== fallbackUrl) {
    event.currentTarget.src = fallbackUrl;
  }
}

/**
 * Renders a themed navigation icon using the same lookup rules as the main shell.
 */
export function NavIcon({ name, theme }: { name: string; theme: string }) {
  return (
    <img
      className={`nav-icon nav-icon-${name}`}
      src={navIconUrl(theme, name)}
      alt=""
      aria-hidden="true"
      onError={(event) => handleNavIconError(event, name)}
    />
  );
}
