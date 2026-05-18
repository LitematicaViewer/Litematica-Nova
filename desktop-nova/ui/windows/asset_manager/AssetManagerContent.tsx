import { useEffect, useMemo, useState } from "react";

import {
  activateGameResource,
  checkGameResourceHealth,
  deleteGameResource,
  downloadRemoteMinecraftLanguage,
  downloadVaultBlockIcons,
  downloadVaultItemIcons,
  downloadWikiEnumCatalog,
  GameResourceEntry,
  GameResourceHealthRow,
  GameResourceKind,
  GameResourceSnapshot,
  getUserConfigFilePath,
  importGameDataResource,
  importLanguageResource,
  initI18n,
  invalidateBlockIconCache,
  invalidateBlockstateDbCache,
  invalidateI18nCache,
  listGameResourceRegistry,
  listRemoteMinecraftLanguageBranches,
  listRemoteMinecraftLanguages,
  loadDatabases,
  openDialog,
  openWorkspacePath,
  registerExternalIconDirectory,
} from "../../../src/business/facade";
import { listenEvent } from "../../../src/platform/events";
import { NavIcon } from "../../shell/NavIcon";

type ManagedAssetRoute = "block_icon" | "item_icon" | "language" | "game_data" | "enum_catalog";
type AssetRoute = ManagedAssetRoute;
type AssetSource = "builtin" | "local_directory" | "vault" | "wiki" | "local_json" | "github";
type AssetSourceState = Record<AssetRoute, AssetSource>;
type AssetTableColumnKey = "name" | "source" | "version" | "language" | "usage" | "status" | "date" | "actions";

interface AssetNavItem {
  key: AssetRoute;
  label: string;
  icon: string;
}

interface AssetSourceOption {
  value: AssetSource;
  label: string;
}

interface AssetTableColumn {
  key: AssetTableColumnKey;
  label: string;
}

interface VaultBlockIconProgressPayload {
  current: number;
  total: number;
  downloaded: number;
  status: string;
}

const ASSET_NAV_ITEMS: AssetNavItem[] = [
  { key: "block_icon", label: "方块图标", icon: "asset-block" },
  { key: "item_icon", label: "物品图标", icon: "asset-item" },
  { key: "language", label: "语言", icon: "asset-language" },
  { key: "game_data", label: "游戏数据", icon: "asset-db" },
  { key: "enum_catalog", label: "枚举全集", icon: "asset-enum" },
];

const DEFAULT_SOURCE_BY_ROUTE: AssetSourceState = {
  block_icon: "builtin",
  item_icon: "builtin",
  language: "builtin",
  game_data: "builtin",
  enum_catalog: "builtin",
};

const SOURCE_OPTIONS_BY_ROUTE: Record<AssetRoute, AssetSourceOption[]> = {
  block_icon: [
    { value: "builtin", label: "内建" },
    { value: "local_directory", label: "本地目录" },
    { value: "vault", label: "CCVault" },
    { value: "wiki", label: "Minecraft Wiki" },
  ],
  item_icon: [
    { value: "builtin", label: "内建" },
    { value: "local_directory", label: "本地目录" },
    { value: "vault", label: "CCVault" },
    { value: "wiki", label: "Minecraft Wiki" },
  ],
  language: [
    { value: "builtin", label: "内建" },
    { value: "local_json", label: "本地JSON" },
    { value: "github", label: "Github/InventivetalentDev" },
  ],
  game_data: [
    { value: "builtin", label: "内建" },
    { value: "local_json", label: "本地JSON" },
  ],
  enum_catalog: [
    { value: "builtin", label: "内建" },
    { value: "wiki", label: "Minecraft Wiki" },
  ],
};

const TABLE_COLUMNS_BY_ROUTE: Record<AssetRoute, AssetTableColumn[]> = {
  block_icon: [
    { key: "name", label: "名称" },
    { key: "source", label: "来源" },
    { key: "version", label: "版本" },
    { key: "usage", label: "用于" },
    { key: "actions", label: "操作" },
  ],
  item_icon: [
    { key: "name", label: "名称" },
    { key: "source", label: "来源" },
    { key: "version", label: "版本" },
    { key: "status", label: "状态" },
    { key: "actions", label: "操作" },
  ],
  language: [
    { key: "name", label: "名称" },
    { key: "source", label: "来源" },
    { key: "language", label: "语言" },
    { key: "version", label: "版本" },
    { key: "status", label: "状态" },
    { key: "actions", label: "操作" },
  ],
  game_data: [
    { key: "name", label: "名称" },
    { key: "source", label: "来源" },
    { key: "version", label: "版本" },
    { key: "status", label: "状态" },
    { key: "actions", label: "操作" },
  ],
  enum_catalog: [
    { key: "name", label: "名称" },
    { key: "source", label: "来源" },
    { key: "date", label: "日期" },
    { key: "status", label: "状态" },
    { key: "actions", label: "操作" },
  ],
};

const ROUTE_ROOT_RELPATH: Record<AssetRoute, string> = {
  block_icon: "minecraft-assets/block_icon",
  item_icon: "minecraft-assets/item",
  language: "minecraft-assets/language",
  game_data: "minecraft-assets/game-data",
  enum_catalog: "enumerator/base",
};

const BUILTIN_BLOCK_ICON_ID = "builtin:block_icon:nova";
const BUILTIN_ITEM_ICON_ID = "builtin:item_icon:nova";
const BUILTIN_LANGUAGE_ID = "builtin:language:zh_cn";
const BUILTIN_GAME_DATA_ID = "builtin:game_data:26.1";
const BUILTIN_ENUM_CATALOG_ID = "builtin:enum_catalog:base";

function baseName(path: string): string {
  return path.split(/[\\/]+/).filter(Boolean).pop() || path;
}

function parentDir(path: string): string {
  const normalized = path.replace(/[\\/]+$/, "");
  const index = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  return index >= 0 ? normalized.slice(0, index) : normalized;
}

function resourceActiveText(snapshot: GameResourceSnapshot | null): string {
  if (!snapshot) return "未加载";
  return [
    `语言=${snapshot.active_language.label}`,
    `材料图标=${snapshot.active_material_list_icons.label}`,
    `分层方块=${snapshot.active_layering_block_icons.label}`,
    `分层物品=${snapshot.active_layering_item_icons.label}`,
    `数据=${snapshot.active_game_data.label}`,
    `枚举=${snapshot.active_enum_catalog.label}`,
  ].join(" / ");
}

function routeIntro(route: AssetRoute): string {
  switch (route) {
    case "block_icon": return "管理材料列表与分层视图使用的方块图标来源。";
    case "item_icon": return "管理分层视图中可用的物品图标来源。";
    case "language": return "导入、下载并切换 Minecraft 语言 JSON。";
    case "game_data": return "导入并切换 BlockState 数据库与属性翻译。";
    case "enum_catalog": return "下载并切换枚举器使用的基础全集文件。";
  }
}

function isManagedAssetRoute(route: AssetRoute): route is ManagedAssetRoute {
  return true;
}

function sourceTag(entry: GameResourceEntry): string {
  switch (entry.source) {
    case "builtin": return "builtin";
    case "imported":
    case "external": return "local";
    case "vault": return "vault";
    case "github": return "github";
    case "wiki": return "wiki";
  }
}

function formatDate(value?: string): string {
  if (!value) return "未知";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "未知";
  const year = date.getFullYear();
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${year}/${month}/${day}`;
}

function blockUsageLabel(entry: GameResourceEntry): string {
  return [
    entry.active_material_list ? "材料" : "",
    entry.active_layering ? "分层" : "",
  ].filter(Boolean).join("/");
}

function activationLabel(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "item_icon") return entry.active_layering ? "已激活" : "";
  if (route === "language" || route === "game_data" || route === "enum_catalog") return entry.active ? "已激活" : "";
  return blockUsageLabel(entry);
}

function displayEntryName(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "block_icon" || route === "item_icon") {
    if (entry.source === "builtin") return "（内建）";
    if (entry.source === "external") return `${baseName(entry.root_path || entry.label)}/`;
    if (entry.source === "vault") return "CCVault";
    return entry.label;
  }
  if (route === "language") {
    if (entry.source === "builtin") return `（内建）${entry.language || "zh_cn"}`;
    if (entry.source === "imported") return baseName(entry.file_relpath || entry.label);
    if (entry.source === "github") return entry.language || entry.label;
    return entry.label;
  }
  if (route === "game_data") {
    if (entry.source === "builtin") return "（内建）";
    if (entry.source === "imported") return baseName(entry.data_relpath || entry.label);
    return entry.label;
  }
  if (route === "enum_catalog") {
    if (entry.source === "builtin") return "（内建）";
    if (entry.source === "wiki") return "Minecraft Wiki";
    return entry.label;
  }
  return entry.label;
}

function displayVersion(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "language") {
    return entry.source === "github" ? (entry.branch || entry.version || "未知") : "未知";
  }
  if (route === "enum_catalog") {
    return entry.source === "builtin" ? "2025/4/22" : formatDate(entry.installed_at);
  }
  return "未知";
}

function fetchButtonLabel(route: AssetRoute, source: AssetSource): string {
  if (route === "block_icon" || route === "item_icon") {
    if (source === "builtin") return "重新解包";
    if (source === "local_directory") return "导入...";
    return "下载";
  }
  if (route === "language") {
    if (source === "builtin") return "重新解包";
    if (source === "local_json") return "导入...";
    return "下载";
  }
  if (route === "game_data") {
    return source === "builtin" ? "重新解包" : "导入...";
  }
  return source === "builtin" ? "重新解包" : "下载";
}

/**
 * Renders the game asset manager body used by the standalone asset-manager subwindow.
 */
export function AssetManagerContent({ theme, onClose }: { theme: string; onClose?: () => void }) {
  const [route, setRoute] = useState<AssetRoute>("block_icon");
  const [resourceSnapshot, setResourceSnapshot] = useState<GameResourceSnapshot | null>(null);
  const [resourceHealth, setResourceHealth] = useState<GameResourceHealthRow[]>([]);
  const [statusText, setStatusText] = useState("正在加载资源...");
  const [log, setLog] = useState("");
  const [selectedSourceByRoute, setSelectedSourceByRoute] = useState<AssetSourceState>(DEFAULT_SOURCE_BY_ROUTE);
  const [languageBranches, setLanguageBranches] = useState<string[]>([]);
  const [selectedLanguageBranch, setSelectedLanguageBranch] = useState("");
  const [remoteLanguages, setRemoteLanguages] = useState<string[]>([]);
  const [selectedRemoteLanguage, setSelectedRemoteLanguage] = useState("");
  const [languageCatalogBusy, setLanguageCatalogBusy] = useState(false);
  const [languageDownloadBusy, setLanguageDownloadBusy] = useState(false);
  const [blockIconDownloadBusy, setBlockIconDownloadBusy] = useState(false);
  const [itemIconDownloadBusy, setItemIconDownloadBusy] = useState(false);
  const [enumCatalogDownloadBusy, setEnumCatalogDownloadBusy] = useState(false);

  const selectedNavItem = useMemo(() => ASSET_NAV_ITEMS.find((item) => item.key === route) || ASSET_NAV_ITEMS[0], [route]);
  const selectedSource = selectedSourceByRoute[route];
  const tableColumns = TABLE_COLUMNS_BY_ROUTE[route];
  const isManagedRoute = isManagedAssetRoute(route);
  const entries = isManagedRoute ? resourceSnapshot?.entries[route] || [] : [];
  const remoteLanguageEnabled = route === "language" && selectedSourceByRoute.language === "github";
  const remoteLanguageBusy = remoteLanguageEnabled && (languageCatalogBusy || languageDownloadBusy);

  const reportStatus = (status: string, detail?: string) => {
    setStatusText(status);
    if (detail !== undefined) {
      setLog(detail ? `${status}\n${detail}` : status);
    }
  };

  const refreshGameResources = async (silent = false) => {
    const snapshot = await listGameResourceRegistry();
    setResourceSnapshot(snapshot);
    if (!silent) {
      setStatusText("资源列表已刷新。");
    }
    return snapshot;
  };

  const reloadRuntimeResources = async () => {
    invalidateI18nCache();
    invalidateBlockstateDbCache();
    invalidateBlockIconCache();
    await Promise.all([initI18n(true), loadDatabases(true)]);
  };

  const applyResourceSnapshot = async (snapshotPromise: Promise<GameResourceSnapshot>, message: string) => {
    const snapshot = await snapshotPromise;
    setResourceSnapshot(snapshot);
    await reloadRuntimeResources();
    reportStatus(message, resourceActiveText(snapshot));
  };

  const activateResource = async (kind: GameResourceKind, id: string, slot?: "material_list" | "layering") => {
    await applyResourceSnapshot(activateGameResource(kind, id, slot), "资源激活状态已更新。");
  };

  const activateBuiltinForCurrentRoute = async () => {
    if (route === "block_icon") {
      await activateGameResource("block_icon", BUILTIN_BLOCK_ICON_ID, "material_list");
      await applyResourceSnapshot(
        activateGameResource("block_icon", BUILTIN_BLOCK_ICON_ID, "layering"),
        "已切换到内建方块图标。",
      );
      return;
    }
    if (route === "item_icon") {
      await applyResourceSnapshot(
        activateGameResource("item_icon", BUILTIN_ITEM_ICON_ID, "layering"),
        "已切换到内建物品图标。",
      );
      return;
    }
    if (route === "language") {
      await applyResourceSnapshot(
        activateGameResource("language", BUILTIN_LANGUAGE_ID),
        "已切换到内建语言。",
      );
      return;
    }
    if (route === "game_data") {
      await applyResourceSnapshot(activateGameResource("game_data", BUILTIN_GAME_DATA_ID), "已切换到内建游戏数据。");
      return;
    }
    if (route === "enum_catalog") {
      await applyResourceSnapshot(activateGameResource("enum_catalog", BUILTIN_ENUM_CATALOG_ID), "已切换到内建枚举全集。");
      return;
    }
    reportStatus("无法识别当前资源类型。", route);
  };

  const deleteResource = async (kind: GameResourceKind, id: string) => {
    await applyResourceSnapshot(deleteGameResource(kind, id), "资源登记已删除。");
  };

  const importLanguage = async () => {
    const selected = await openDialog({ filters: [{ name: "Minecraft language JSON", extensions: ["json"] }] });
    if (typeof selected !== "string") return;
    await applyResourceSnapshot(importLanguageResource(selected), `语言资源已导入：${selected}`);
  };

  const downloadLanguage = async () => {
    if (!selectedLanguageBranch || !selectedRemoteLanguage) return;
    setLanguageDownloadBusy(true);
    try {
      await applyResourceSnapshot(
        downloadRemoteMinecraftLanguage(selectedLanguageBranch, selectedRemoteLanguage),
        `语言资源已下载：${selectedLanguageBranch} / ${selectedRemoteLanguage}`,
      );
    } finally {
      setLanguageDownloadBusy(false);
    }
  };

  const downloadBlockIcons = async () => {
    setBlockIconDownloadBusy(true);
    reportStatus("正在下载 CCVault 方块图标...", "正在下载 CCVault 方块图标...");
    try {
      const result = await downloadVaultBlockIcons();
      setResourceSnapshot(result.snapshot);
      await reloadRuntimeResources();
      reportStatus(
        `CCVault 方块图标已下载：${result.downloaded}/${result.total}`,
        `${result.target_dir}\n${resourceActiveText(result.snapshot)}`,
      );
    } catch (error) {
      reportStatus("下载 CCVault 方块图标失败。", String(error));
    } finally {
      setBlockIconDownloadBusy(false);
    }
  };

  const downloadItemIcons = async () => {
    setItemIconDownloadBusy(true);
    reportStatus("正在下载 CCVault 物品图标...", "正在下载 CCVault 物品图标...");
    try {
      const result = await downloadVaultItemIcons();
      setResourceSnapshot(result.snapshot);
      await reloadRuntimeResources();
      reportStatus(
        `CCVault 物品图标已下载：${result.downloaded}/${result.total}`,
        `${result.target_dir}\n${resourceActiveText(result.snapshot)}`,
      );
    } catch (error) {
      reportStatus("下载 CCVault 物品图标失败。", String(error));
    } finally {
      setItemIconDownloadBusy(false);
    }
  };

  const downloadEnumCatalogs = async () => {
    setEnumCatalogDownloadBusy(true);
    reportStatus("正在下载 Minecraft Wiki 枚举全集...", "正在抓取方块、物品、魔咒和实体基础集合...");
    try {
      const result = await downloadWikiEnumCatalog();
      setResourceSnapshot(result.snapshot);
      reportStatus(
        `枚举全集已下载：B=${result.blocks} / I=${result.items} / E=${result.enchantments} / N=${result.entities}`,
        `${result.target_dir}\n${resourceActiveText(result.snapshot)}`,
      );
    } catch (error) {
      reportStatus("下载 Minecraft Wiki 枚举全集失败。", String(error));
    } finally {
      setEnumCatalogDownloadBusy(false);
    }
  };

  const importBlockIconDir = async () => {
    const selected = await openDialog({ directory: true });
    if (typeof selected !== "string") return;
    await applyResourceSnapshot(
      registerExternalIconDirectory("block_icon", selected, "material_list"),
      `方块图标目录已导入：${selected}`,
    );
  };

  const importItemIconDir = async () => {
    const selected = await openDialog({ directory: true });
    if (typeof selected !== "string") return;
    await applyResourceSnapshot(registerExternalIconDirectory("item_icon", selected, "layering"), `物品图标目录已导入：${selected}`);
  };

  const importGameData = async () => {
    const selected = await openDialog({
      multiple: true,
      filters: [{ name: "BlockState JSON", extensions: ["json"] }],
    });
    const files = Array.isArray(selected) ? selected : typeof selected === "string" ? [selected] : [];
    if (files.length === 0) return;
    const zh = files.find((file) => /\.zh_cn\.json$/i.test(file) || /zh_cn/i.test(file));
    const db = files.find((file) => file !== zh) || files[0];
    await applyResourceSnapshot(importGameDataResource(db, zh), `游戏数据已导入：${files.join(", ")}`);
  };

  const checkResources = async () => {
    const rows = await checkGameResourceHealth();
    setResourceHealth(rows);
    reportStatus(
      rows.every((row) => row.ok) ? "健康检查完成。" : "健康检查发现异常。",
      rows.map((row) => `${row.ok ? "OK" : "失败"} ${row.label}\n${row.detail}`).join("\n\n"),
    );
  };

  const openCurrentRootDirectory = async () => {
    try {
      const marker = await getUserConfigFilePath(`${ROUTE_ROOT_RELPATH[route]}/.resource-root`);
      await openWorkspacePath(parentDir(marker));
      setStatusText(`已打开 ${selectedNavItem.label} 根目录。`);
    } catch (error) {
      reportStatus(`打开 ${selectedNavItem.label} 根目录失败。`, String(error));
    }
  };

  const resetRemoteLanguageCatalog = () => {
    setLanguageBranches([]);
    setSelectedLanguageBranch("");
    setRemoteLanguages([]);
    setSelectedRemoteLanguage("");
    setStatusText("远端语言目录已重置，等待重新加载。");
  };

  const runFetchAction = async () => {
    try {
      if (route === "block_icon") {
        if (selectedSource === "builtin") {
          await activateBuiltinForCurrentRoute();
          return;
        }
        if (selectedSource === "local_directory") {
          await importBlockIconDir();
          return;
        }
        if (selectedSource === "vault") {
          await downloadBlockIcons();
          return;
        }
        reportStatus("Minecraft Wiki 方块图标下载暂未实现。", "当前仅支持内建、本地目录和 CCVault。");
        return;
      }
      if (route === "item_icon") {
        if (selectedSource === "builtin") {
          await activateBuiltinForCurrentRoute();
          return;
        }
        if (selectedSource === "local_directory") {
          await importItemIconDir();
          return;
        }
        if (selectedSource === "vault") {
          await downloadItemIcons();
          return;
        }
        reportStatus("Minecraft Wiki 物品图标下载暂未实现。", "当前仅支持内建、本地目录和 CCVault。");
        return;
      }
      if (route === "language") {
        if (selectedSource === "builtin") {
          await activateBuiltinForCurrentRoute();
          return;
        }
        if (selectedSource === "local_json") {
          await importLanguage();
          return;
        }
        await downloadLanguage();
        return;
      }
      if (route === "game_data") {
        if (selectedSource === "builtin") {
          await activateBuiltinForCurrentRoute();
          return;
        }
        await importGameData();
        return;
      }
      if (route === "enum_catalog") {
        if (selectedSource === "builtin") {
          await activateBuiltinForCurrentRoute();
          return;
        }
        await downloadEnumCatalogs();
        return;
      }
      reportStatus("无法识别当前资源类型。", route);
    } catch (error) {
      reportStatus("资源操作失败。", String(error));
    }
  };

  const renderSubOptionControls = () => {
    if (route === "language" && selectedSource === "github") {
      return (
        <>
          <select
            className="input asset-manager-select"
            value={selectedLanguageBranch}
            disabled={remoteLanguageBusy || languageBranches.length === 0}
            onChange={(event) => setSelectedLanguageBranch(event.target.value)}
          >
            {languageBranches.length === 0 ? <option value="">版本</option> : null}
            {languageBranches.map((branch) => <option key={branch} value={branch}>{branch}</option>)}
          </select>
          <select
            className="input asset-manager-select"
            value={selectedRemoteLanguage}
            disabled={remoteLanguageBusy || remoteLanguages.length === 0}
            onChange={(event) => setSelectedRemoteLanguage(event.target.value)}
          >
            {remoteLanguages.length === 0 ? <option value="">语言</option> : null}
            {remoteLanguages.map((language) => <option key={language} value={language}>{language}</option>)}
          </select>
        </>
      );
    }

    const placeholders: Record<AssetRoute, [string, string]> = {
      block_icon: ["子选项1", "子选项2"],
      item_icon: ["子选项1", "子选项2"],
      language: ["版本", "语言"],
      game_data: ["子选项1", "子选项2"],
      enum_catalog: ["子选项1", "子选项2"],
    };

    const [first, second] = placeholders[route];
    return (
      <>
        <select className="input asset-manager-select" disabled value="">
          <option value="">{first}</option>
        </select>
        <select className="input asset-manager-select" disabled value="">
          <option value="">{second}</option>
        </select>
      </>
    );
  };

  const renderCell = (entry: GameResourceEntry, column: AssetTableColumnKey) => {
    switch (column) {
      case "name":
        return displayEntryName(route, entry);
      case "source":
        return sourceTag(entry);
      case "version":
        return displayVersion(route, entry);
      case "language":
        return entry.language || "未知";
      case "usage":
        return blockUsageLabel(entry);
      case "status":
        return activationLabel(route, entry);
      case "date":
        return entry.source === "builtin" ? "2025/4/22" : formatDate(entry.installed_at);
      case "actions":
        return (
          <div className="game-resource-row-actions">
            {route === "block_icon" ? (
              <>
                <button className="btn" type="button" onClick={() => activateResource("block_icon", entry.id, "material_list")}>用于材料</button>
                <button className="btn" type="button" onClick={() => activateResource("block_icon", entry.id, "layering")}>用于分层</button>
              </>
            ) : null}
            {route === "item_icon" ? (
              <button className="btn" type="button" onClick={() => activateResource("item_icon", entry.id, "layering")}>应用</button>
            ) : null}
            {(route === "language" || route === "game_data" || route === "enum_catalog") ? (
              <button className="btn" type="button" onClick={() => activateResource(route, entry.id)}>应用</button>
            ) : null}
            {!entry.builtin ? <button className="btn" type="button" onClick={() => deleteResource(entry.kind, entry.id)}>删除登记</button> : null}
          </div>
        );
    }
  };

  useEffect(() => {
    refreshGameResources(true).catch((error) => reportStatus("加载资源列表失败。", String(error)));
  }, []);

  useEffect(() => {
    const unlistenPromise = listenEvent<VaultBlockIconProgressPayload>(
      "vault-block-icons-download-progress",
      (event) => {
        const payload = event.payload;
        if (!payload) return;
        reportStatus(payload.status, `${payload.current}/${payload.total}，成功 ${payload.downloaded}`);
      },
    ).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, []);

  useEffect(() => {
    const unlistenPromise = listenEvent<VaultBlockIconProgressPayload>(
      "vault-item-icons-download-progress",
      (event) => {
        const payload = event.payload;
        if (!payload) return;
        reportStatus(payload.status, `${payload.current}/${payload.total}，成功 ${payload.downloaded}`);
      },
    ).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, []);

  useEffect(() => {
    if (!remoteLanguageEnabled || languageBranches.length > 0) return;
    let cancelled = false;
    setLanguageCatalogBusy(true);
    listRemoteMinecraftLanguageBranches()
      .then((branches) => {
        if (cancelled) return;
        setLanguageBranches(branches);
        setSelectedLanguageBranch((current) => current || branches[0] || "");
      })
      .catch((error) => {
        if (cancelled) return;
        reportStatus("获取语言分支失败。", String(error));
      })
      .finally(() => {
        if (!cancelled) setLanguageCatalogBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [remoteLanguageEnabled, languageBranches.length]);

  useEffect(() => {
    if (!remoteLanguageEnabled || !selectedLanguageBranch) return;
    let cancelled = false;
    setLanguageCatalogBusy(true);
    setRemoteLanguages([]);
    setSelectedRemoteLanguage("");
    listRemoteMinecraftLanguages(selectedLanguageBranch)
      .then((languages) => {
        if (cancelled) return;
        setRemoteLanguages(languages);
        setSelectedRemoteLanguage((current) => current || languages[0] || "");
      })
      .catch((error) => {
        if (cancelled) return;
        reportStatus(`获取 ${selectedLanguageBranch} 语言列表失败。`, String(error));
      })
      .finally(() => {
        if (!cancelled) setLanguageCatalogBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [remoteLanguageEnabled, selectedLanguageBranch]);

  return (
    <>
      <div className="subwindow-title-bar">
        <div className="subwindow-title-stack">
          <h2 className="subwindow-title">游戏资源管理</h2>
          <p className="muted subwindow-subtitle">{resourceActiveText(resourceSnapshot)}</p>
        </div>
        {onClose ? <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}>×</button> : null}
      </div>
      <div className="subwindow-body asset-manager-body">
        <aside className="sidebar asset-manager-sidebar">
          <nav className="nav-list" aria-label="游戏资源分类">
            {ASSET_NAV_ITEMS.map((item) => {
              const active = route === item.key;
              return (
                <button
                  key={item.key}
                  className={`sidebar-item nav-item ${active ? "active nav-item-active" : ""}`}
                  type="button"
                  aria-pressed={active}
                  onClick={() => setRoute(item.key)}
                >
                  <NavIcon name={item.icon} theme={theme} />
                  <span className="nav-label">{item.label}</span>
                </button>
              );
            })}
          </nav>
        </aside>
        <section className="asset-manager-content">
          <div className="asset-manager-heading">
            <h3>{selectedNavItem.label}</h3>
            <p className="muted">{routeIntro(route)}</p>
          </div>
          <div className="asset-manager-panel">
            <div className="asset-manager-config">
              <div className="asset-manager-form-row">
                <label className="asset-manager-form-label" htmlFor="asset-manager-source">来源</label>
                <select
                  id="asset-manager-source"
                  className="input asset-manager-select"
                  value={selectedSource}
                  onChange={(event) => setSelectedSourceByRoute((current) => ({
                    ...current,
                    [route]: event.target.value as AssetSource,
                  }))}
                >
                  {SOURCE_OPTIONS_BY_ROUTE[route].map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </div>
              <div className="asset-manager-form-row asset-manager-form-row-top">
                <span className="asset-manager-form-label">子选项</span>
                <div className="asset-manager-inline-controls">
                  {renderSubOptionControls()}
                </div>
              </div>
              <div className="asset-manager-form-row">
                <span className="asset-manager-form-label">获取</span>
                <div className="asset-manager-inline-controls asset-manager-fetch-row">
                  <button
                    className="btn"
                    type="button"
                    disabled={
                      blockIconDownloadBusy ||
                      itemIconDownloadBusy ||
                      enumCatalogDownloadBusy ||
                      (route === "language" && selectedSource === "github" && (!selectedLanguageBranch || !selectedRemoteLanguage || remoteLanguageBusy))
                    }
                    onClick={runFetchAction}
                  >
                    {route === "block_icon" && blockIconDownloadBusy ? "下载中..." : null}
                    {route === "item_icon" && itemIconDownloadBusy ? "下载中..." : null}
                    {route === "language" && languageDownloadBusy ? "下载中..." : null}
                    {route === "enum_catalog" && enumCatalogDownloadBusy ? "下载中..." : null}
                    {!(
                      (route === "block_icon" && blockIconDownloadBusy) ||
                      (route === "item_icon" && itemIconDownloadBusy) ||
                      (route === "language" && languageDownloadBusy) ||
                      (route === "enum_catalog" && enumCatalogDownloadBusy)
                    ) ? fetchButtonLabel(route, selectedSource) : null}
                  </button>
                  {route === "language" && selectedSource === "github" ? (
                    <button className="btn" type="button" onClick={resetRemoteLanguageCatalog} disabled={remoteLanguageBusy}>
                      刷新远端列表
                    </button>
                  ) : null}
                </div>
              </div>
            </div>

            {resourceHealth.length > 0 ? (
              <div className="asset-manager-health">
                {resourceHealth.map((row) => (
                  <div key={row.label} className={row.ok ? "game-resource-health-ok" : "game-resource-health-failed"}>
                    {row.ok ? "OK" : "失败"} {row.label}
                  </div>
                ))}
              </div>
            ) : null}

            <div className="table-wrap asset-manager-table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    {tableColumns.map((column) => <th key={column.key}>{column.label}</th>)}
                  </tr>
                </thead>
                <tbody>
                  {entries.length > 0 ? entries.map((entry) => (
                    <tr key={entry.id}>
                      {tableColumns.map((column) => (
                        <td key={column.key} className={column.key === "name" ? "game-resource-path" : undefined}>
                          {renderCell(entry, column.key)}
                        </td>
                      ))}
                    </tr>
                  )) : (
                    <tr>
                      <td colSpan={tableColumns.length} className="asset-manager-empty-row">
                        暂无已登记资源。
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>

            <div className="game-resource-note">
              内建资源始终作为离线回退；导入目录目前登记外部路径，导入 JSON 与远端下载会复制到用户配置目录下的 `minecraft-assets` 或 `enumerator/base`。
            </div>
            {log ? <pre className="subwindow-error asset-manager-log">{log}</pre> : null}
          </div>
          <footer className="asset-manager-footer">
            <span className="subwindow-status-text asset-manager-footer-status">{statusText}</span>
            <div className="asset-manager-footer-actions">
              <button className="btn" type="button" onClick={() => refreshGameResources(false)}>刷新</button>
              <button className="btn" type="button" onClick={checkResources}>健康检查</button>
              <button className="btn" type="button" onClick={openCurrentRootDirectory}>打开根目录...</button>
            </div>
          </footer>
        </section>
      </div>
    </>
  );
}