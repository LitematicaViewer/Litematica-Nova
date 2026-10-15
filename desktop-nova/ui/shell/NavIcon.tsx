import { Box, BarChart3, Boxes, Database, Download, FileText, Home, Languages, Layers, LayoutDashboard, Library, ListTree, Menu, Package, Replace, Settings, Wand2, type LucideIcon } from "lucide-react";
import { themeResourceKey } from "./themeRuntime";

/**
 * Navigation artwork follows the active theme. A theme may ship its own 16x16
 * pixel set under `themes/<theme>/resource/icon/nav/`; every theme that ships no
 * such set keeps the shared vector icons, so a theme without artwork is never
 * left with holes. Icon names are the file names without their extension, and
 * they are shared with the game-resource manager so both sidebars stay in step.
 */
const vectorNavIcons: Record<string, LucideIcon> = {
  hamburger: Menu, home: Home, gallery: Library, properties: FileText,
  statistics: BarChart3, flake: Layers, render: Box, replace: Replace,
  generate: Wand2, options: Settings, ui_debug: LayoutDashboard,
  enumerator: ListTree, update: Download,
  "asset-block": Boxes, "asset-item": Package, "asset-language": Languages,
  "asset-db": Database, "asset-enum": ListTree,
};

const iconNameFromPath = (path: string) => path.split("/").pop()?.replace(/\.[^.]+$/, "") || "";

/** Theme-supplied navigation images, keyed by theme key and then by icon name. */
const themedNavIconUrls = Object.entries(
  import.meta.glob<string>("../themes/*/resource/icon/nav/*.{png,svg}", {
    eager: true,
    import: "default",
    query: "?url",
  }),
).reduce<Record<string, Record<string, string>>>((themes, [path, url]) => {
  const themeKey = path.match(/\.\.\/themes\/([^/]+)\//)?.[1];
  const iconName = iconNameFromPath(path);
  if (!themeKey || !iconName) return themes;
  (themes[themeKey] ||= {})[iconName] = url;
  return themes;
}, {});

/**
 * A theme either ships its own navigation set or keeps the vector icons; inside
 * a set, the theme's `undefined` artwork is the placeholder for a missing name.
 */
const themedNavIconUrl = (theme: string, name: string) => {
  const icons = themedNavIconUrls[themeResourceKey(theme)];
  return icons ? icons[name] || icons.undefined : undefined;
};

export function NavIcon({ name, theme, className }: { name: string; theme: string; className?: string }) {
  const imageUrl = themedNavIconUrl(theme, name);
  if (imageUrl) {
    return <img
      className={["nav-icon", `nav-icon-${name}`, className].filter(Boolean).join(" ")}
      src={imageUrl}
      alt=""
      aria-hidden="true"
      draggable={false}
    />;
  }
  const Icon = vectorNavIcons[name] || Box;
  return <Icon className={["nav-icon", className].filter(Boolean).join(" ")} aria-hidden="true" focusable="false" />;
}
