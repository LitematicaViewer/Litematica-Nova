import { displayPathText } from "../../../src/business/facade";
import { displayFilePath } from "../../../src/business/facade";
import { useCallback, useEffect, useMemo, useState } from "react";

import {
  activateGameResource,
  checkGameResourceHealth,
  deleteGameResource,
  downloadRemoteMinecraftLanguage,
  downloadVaultBlockIcons,
  downloadVaultItemIcons,
  downloadWikiBlockIcons,
  downloadWikiEnumCatalog,
  ensureBuiltinBlockIconsExtracted,
  ensureBuiltinItemIconsExtracted,
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
  resolveBlockIconVariant,
  type BlockIconSlot,
  type GameResourceEntry,
  type GameResourceHealthRow,
  type GameResourceKind,
  type GameResourceSnapshot,
} from "../../../src/business/facade";
import { listenEvent } from "../../../src/platform/events";
import { Button } from "../../components/ui/button";
import { NavIcon } from "../../shell/NavIcon";

/**
 * Game resources module — layout, wording and behaviour follow `docs/design.md`
 * §2.11.2, ported from the pre-merge window (dropped by the merge).
 *
 * Per category (Line.1..Line.5):
 *   Line.1  title + subtitle
 *   Line.2  source dropdown (default 内建)
 *   Line.3  sub-option 1 / sub-option 2
 *   Line.4  primary action: 重新解包 (内建) / 导入... (本地) / 下载 (远端)
 *   Line.5  content table
 */

type AssetRoute = GameResourceKind;
type BlockIconResourceSlot = BlockIconSlot;
type AssetSource =
  | "builtin"
  | "local_directory"
  | "vault"
  | "wiki"
  | "wiki_zh"
  | "local_json"
  | "github";

interface AssetNavItem {
  key: AssetRoute;
  label: string;
  /** Theme navigation icon name, shared with the main shell sidebar. */
  icon: string;
}

/** Progress payload emitted by the icon download commands. */
interface IconDownloadProgressPayload {
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

const BUILTIN_BLOCK_ICON_ID = "builtin:block_icon:nova";
const BUILTIN_ITEM_ICON_ID = "builtin:item_icon:nova";

const SOURCE_OPTIONS_BY_ROUTE: Record<AssetRoute, { value: AssetSource; label: string }[]> = {
  block_icon: [
    { value: "builtin", label: "内建" },
    { value: "local_directory", label: "本地目录" },
    { value: "vault", label: "CCVault" },
    { value: "wiki", label: "Minecraft Wiki" },
    { value: "wiki_zh", label: "中文 Minecraft 维基百科" },
  ],
  item_icon: [
    { value: "builtin", label: "内建" },
    { value: "local_directory", label: "本地目录" },
    { value: "vault", label: "CCVault" },
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

const DEFAULT_SOURCE_BY_ROUTE: Record<AssetRoute, AssetSource> = {
  block_icon: "builtin",
  item_icon: "builtin",
  language: "builtin",
  game_data: "builtin",
  enum_catalog: "builtin",
};

function routeIntro(route: AssetRoute): string {
  switch (route) {
    case "block_icon":
      return "管理材料槽与 2D 槽使用的图标包；材料相关界面会按 block_icon/block_2d 与激活包名解析，分层画布只走 2D 槽。";
    case "item_icon":
      return "管理 item/激活包；材料相关界面中的物品分支会把它作为 block_itemLike 之后的第二优先级。";
    case "language":
      return "导入、下载并切换 Minecraft 语言 JSON。";
    case "game_data":
      return "导入并切换 BlockState 数据库与属性翻译。";
    case "enum_catalog":
      return "下载并切换枚举器使用的基础全集文件。";
    default:
      return "";
  }
}

function baseName(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").split("/").filter(Boolean).pop() || path;
}

function sourceTag(entry: GameResourceEntry): string {
  switch (entry.source) {
    case "builtin":
      return "builtin";
    case "imported":
    case "external":
      return "local";
    case "vault":
      return "vault";
    case "github":
      return "github";
    case "wiki":
      return "wiki";
    case "wiki_zh":
      return "wiki_zh";
    default:
      return entry.source;
  }
}

function formatDate(value?: string): string {
  if (!value) return "未知";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "未知";
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${date.getFullYear()}/${month}/${day}`;
}

function blockUsageLabel(entry: GameResourceEntry): string {
  return [entry.active_material_list ? "材料槽" : "", entry.active_layering ? "2D 槽" : ""]
    .filter(Boolean)
    .join("/");
}

function activationLabel(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "item_icon") return entry.active_layering ? "已激活" : "";
  if (route === "language" || route === "game_data" || route === "enum_catalog") {
    return entry.active ? "已激活" : "";
  }
  return blockUsageLabel(entry);
}

function displayEntryName(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "block_icon" || route === "item_icon") {
    const prefix = route === "block_icon" ? (resolveBlockIconVariant(entry) === "2d" ? "[2D] " : "[3D] ") : "";
    if (entry.source === "builtin") return route === "block_icon" ? `${prefix}（内建）` : "（内建）";
    if (entry.source === "external") return `${prefix}${baseName(entry.root_path || entry.label)}/`;
    if (entry.source === "vault") return `${prefix}CCVault`;
    return `${prefix}${entry.label}`;
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
  if (route === "block_icon") return resolveBlockIconVariant(entry) === "2d" ? "2D" : "3D";
  if (route === "language") {
    return entry.source === "github" ? entry.branch || entry.version || "未知" : "未知";
  }
  if (route === "enum_catalog") {
    return entry.source === "builtin" ? "2025/4/22" : formatDate(entry.installed_at);
  }
  return "未知";
}

function blockIconSlotLabel(slot: BlockIconResourceSlot): string {
  return slot === "layering" ? "2D 槽（分层）" : "3D 槽（材料列表）";
}

/** Sources that can only write one slot; `null` means the user chooses. */
function lockedBlockIconSlotForSource(source: AssetSource): BlockIconResourceSlot | null {
  if (source === "builtin") return "layering";
  if (source === "vault") return "material_list";
  return null; // wiki / wiki_zh / local_directory can target either slot
}

function blockIconSourceHint(source: AssetSource, slot: BlockIconResourceSlot): string {
  if (source === "builtin") return "内建来源固定写入 2D 槽";
  if (source === "vault") return "CCVault 来源固定写入 3D 槽";
  if (source === "wiki" || source === "wiki_zh") {
    const label = source === "wiki_zh" ? "中文 Wiki" : "英文 Wiki";
    return slot === "layering"
      ? `2D 槽走 ${label} BlockSprite 列表，保持 16x16`
      : `3D 槽走 ${label} 数据值页面，转为 32x32`;
  }
  return slot === "layering" ? "该槽仅接受 2D 方块图标" : "该槽可用 3D，必要时回退 2D";
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

export function AssetManagerContent({ theme, onClose }: { theme: string; onClose?: () => void }) {
  const [route, setRoute] = useState<AssetRoute>("block_icon");
  const [sourceByRoute, setSourceByRoute] = useState(DEFAULT_SOURCE_BY_ROUTE);
  const [selectedBlockIconSlot, setSelectedBlockIconSlot] = useState<BlockIconResourceSlot>("layering");
  const [resourceSnapshot, setResourceSnapshot] = useState<GameResourceSnapshot | null>(null);
  const [health, setHealth] = useState<GameResourceHealthRow[]>([]);
  const [statusText, setStatusText] = useState("");
  const [log, setLog] = useState("");
  const [languageCatalogBusy, setLanguageCatalogBusy] = useState(false);
  const [languageDownloadBusy, setLanguageDownloadBusy] = useState(false);
  const [blockIconDownloadBusy, setBlockIconDownloadBusy] = useState(false);
  const [itemIconDownloadBusy, setItemIconDownloadBusy] = useState(false);
  const [enumCatalogDownloadBusy, setEnumCatalogDownloadBusy] = useState(false);
  const [languageBranches, setLanguageBranches] = useState<string[]>([]);
  const [remoteLanguages, setRemoteLanguages] = useState<string[]>([]);
  const [selectedLanguageBranch, setSelectedLanguageBranch] = useState("");
  const [selectedRemoteLanguage, setSelectedRemoteLanguage] = useState("");

  const selectedSource = sourceByRoute[route];
  const selectedNavItem = ASSET_NAV_ITEMS.find((item) => item.key === route) || ASSET_NAV_ITEMS[0];
  const remoteLanguageEnabled = route === "language" && selectedSource === "github";
  const lockedBlockIconSlot = route === "block_icon" ? lockedBlockIconSlotForSource(selectedSource) : null;
  const effectiveBlockIconSlot: BlockIconResourceSlot =
    route === "block_icon" ? lockedBlockIconSlot || selectedBlockIconSlot : selectedBlockIconSlot;

  const busy =
    languageCatalogBusy ||
    languageDownloadBusy ||
    blockIconDownloadBusy ||
    itemIconDownloadBusy ||
    enumCatalogDownloadBusy;

  const entries = resourceSnapshot ? resourceSnapshot.entries[route] || [] : [];

  const reportStatus = useCallback((status: string, detail?: string) => {
    setStatusText(status);
    if (detail !== undefined) setLog(detail ? `${status}\n${detail}` : status);
  }, []);

  const reloadRuntimeResources = useCallback(async () => {
    invalidateI18nCache();
    invalidateBlockstateDbCache();
    invalidateBlockIconCache();
    await Promise.all([initI18n(true), loadDatabases()]);
  }, []);

  const refreshGameResources = useCallback(
    async (silent = false) => {
      const snapshot = await listGameResourceRegistry();
      setResourceSnapshot(snapshot);
      if (!silent) setStatusText("资源列表已刷新。");
      return snapshot;
    },
    [],
  );

  useEffect(() => {
    refreshGameResources(true).catch((error) => reportStatus("加载资源列表失败。", String(error)));
  }, [refreshGameResources, reportStatus]);

  // Icon download progress: without these the UI looks frozen while working.
  useEffect(() => {
    const unlistenPromise = listenEvent<IconDownloadProgressPayload>(
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
  }, [reportStatus]);

  useEffect(() => {
    const unlistenPromise = listenEvent<IconDownloadProgressPayload>(
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
  }, [reportStatus]);

  // Keep the slot pinned to what the selected source supports.
  useEffect(() => {
    if (!lockedBlockIconSlot) return;
    setSelectedBlockIconSlot((current) => (current === lockedBlockIconSlot ? current : lockedBlockIconSlot));
  }, [lockedBlockIconSlot]);

  const applyResourceSnapshot = useCallback(
    async (snapshotPromise: Promise<GameResourceSnapshot>, message: string) => {
      const snapshot = await snapshotPromise;
      setResourceSnapshot(snapshot);
      await reloadRuntimeResources();
      reportStatus(message, resourceActiveText(snapshot));
    },
    [reloadRuntimeResources, reportStatus],
  );

  // ── Line.4 actions per category ───────────────────────────────────────────
  const downloadLanguage = async () => {
    if (!selectedLanguageBranch || !selectedRemoteLanguage) return;
    setLanguageDownloadBusy(true);
    try {
      await applyResourceSnapshot(
        downloadRemoteMinecraftLanguage(selectedLanguageBranch, selectedRemoteLanguage),
        `语言资源已下载：${selectedLanguageBranch} / ${selectedRemoteLanguage}`,
      );
    } catch (error) {
      reportStatus("下载语言资源失败。", String(error));
    } finally {
      setLanguageDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const importLanguage = async () => {
    const selected = await openDialog({ filters: [{ name: "Minecraft language JSON", extensions: ["json"] }] });
    if (typeof selected !== "string") return;
    try {
      await applyResourceSnapshot(importLanguageResource(selected), `语言资源已导入：${selected}`);
    } catch (error) {
      reportStatus("导入语言资源失败。", String(error));
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
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const downloadWikiBlockIconsForSlot = async (slot: BlockIconResourceSlot, source: "wiki" | "wiki_zh") => {
    setBlockIconDownloadBusy(true);
    const label = source === "wiki_zh" ? "中文 Minecraft 维基百科" : "Minecraft Wiki";
    reportStatus(
      `正在下载${label} ${blockIconSlotLabel(slot)}方块图标...`,
      slot === "layering"
        ? "正在按 BlockSprite 列表抓取 16x16 2D 图标..."
        : "正在按数据值页面抓取并转换 32x32 图标...",
    );
    try {
      const result = await downloadWikiBlockIcons(slot, source);
      setResourceSnapshot(result.snapshot);
      await reloadRuntimeResources();
      reportStatus(
        `${label} 方块图标已下载到${blockIconSlotLabel(slot)}：${result.downloaded}/${result.total}`,
        `${result.target_dir}\n${resourceActiveText(result.snapshot)}`,
      );
    } catch (error) {
      reportStatus(`下载${label} 方块图标失败。`, String(error));
    } finally {
      setBlockIconDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const reextractBuiltinBlockIcons = async (slot: BlockIconResourceSlot) => {
    setBlockIconDownloadBusy(true);
    reportStatus("正在重新提取内建方块图标...", "正在从 block.zip 提取到 minecraft-assets/block_2d/initial ...");
    try {
      const result = await ensureBuiltinBlockIconsExtracted(true);
      const snapshot = await activateGameResource("block_icon", BUILTIN_BLOCK_ICON_ID, slot);
      setResourceSnapshot(snapshot);
      await reloadRuntimeResources();
      reportStatus(
        `内建方块图标已提取并应用到${blockIconSlotLabel(slot)}：${result.extracted}`,
        `${result.target_dir}\n${resourceActiveText(snapshot)}`,
      );
    } catch (error) {
      reportStatus("重新提取内建方块图标失败。", String(error));
    } finally {
      setBlockIconDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
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
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const reextractBuiltinItemIcons = async () => {
    setItemIconDownloadBusy(true);
    reportStatus("正在重新提取内建物品图标...", "正在从 item.zip 提取到 minecraft-assets/item/initial ...");
    try {
      const result = await ensureBuiltinItemIconsExtracted(true);
      const snapshot = await refreshGameResources(true);
      await reloadRuntimeResources();
      reportStatus(
        `内建物品图标已提取：${result.extracted}`,
        `${result.target_dir}\n${resourceActiveText(snapshot)}`,
      );
    } catch (error) {
      reportStatus("重新提取内建物品图标失败。", String(error));
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
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const importBlockIconDir = async (slot: BlockIconResourceSlot) => {
    const selected = await openDialog({ directory: true });
    if (typeof selected !== "string") return;
    setBlockIconDownloadBusy(true);
    try {
      await applyResourceSnapshot(
        registerExternalIconDirectory(route as "block_icon" | "item_icon", selected, slot),
        `图标目录已登记：${selected}`,
      );
    } catch (error) {
      reportStatus("登记图标目录失败。", String(error));
    } finally {
      setBlockIconDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const importGameData = async () => {
    const selected = await openDialog({ filters: [{ name: "Minecraft BlockState JSON", extensions: ["json"] }] });
    if (typeof selected !== "string") return;
    try {
      await applyResourceSnapshot(importGameDataResource(selected), `游戏数据已导入：${selected}`);
    } catch (error) {
      reportStatus("导入游戏数据失败。", String(error));
    }
  };

  const loadLanguageCatalog = useCallback(
    async (branch?: string) => {
      setLanguageCatalogBusy(true);
      try {
        if (!branch) {
          const branches = await listRemoteMinecraftLanguageBranches();
          setLanguageBranches(branches);
          setSelectedLanguageBranch("");
          setRemoteLanguages([]);
          setSelectedRemoteLanguage("");
          reportStatus(`远端版本已刷新：${branches.length} 个`);
          return;
        }
        setSelectedLanguageBranch(branch);
        setSelectedRemoteLanguage("");
        const languages = await listRemoteMinecraftLanguages(branch);
        setRemoteLanguages(languages);
        reportStatus(`版本 ${branch} 有 ${languages.length} 种语言`);
      } catch (error) {
        reportStatus("获取远端语言列表失败。", String(error));
      } finally {
        setLanguageCatalogBusy(false);
      }
    },
    [reportStatus],
  );

  useEffect(() => {
    if (!remoteLanguageEnabled || languageBranches.length > 0) return;
    void loadLanguageCatalog();
  }, [remoteLanguageEnabled, languageBranches.length, loadLanguageCatalog]);

  /** Line.4 primary action. */
  const runFetchAction = async () => {
    if (route === "block_icon") {
      if (selectedSource === "builtin") return reextractBuiltinBlockIcons(effectiveBlockIconSlot);
      if (selectedSource === "vault") return downloadBlockIcons();
      if (selectedSource === "wiki" || selectedSource === "wiki_zh") {
        return downloadWikiBlockIconsForSlot(effectiveBlockIconSlot, selectedSource);
      }
      if (selectedSource === "local_directory") return importBlockIconDir(effectiveBlockIconSlot);
    }
    if (route === "item_icon") {
      if (selectedSource === "builtin") return reextractBuiltinItemIcons();
      if (selectedSource === "vault") return downloadItemIcons();
      if (selectedSource === "local_directory") return importBlockIconDir("layering");
    }
    if (route === "language") {
      if (selectedSource === "local_json") return importLanguage();
      if (selectedSource === "github") return downloadLanguage();
      if (selectedSource === "builtin") {
        return applyResourceSnapshot(activateGameResource("language", "builtin:language:zh_cn"), "已切换到内建语言包。");
      }
    }
    if (route === "game_data") {
      if (selectedSource === "local_json") return importGameData();
      return applyResourceSnapshot(activateGameResource("game_data", "builtin:game_data:26.1"), "已切换到内建游戏数据。");
    }
    if (route === "enum_catalog") {
      if (selectedSource === "wiki") return downloadEnumCatalogs();
      return applyResourceSnapshot(activateGameResource("enum_catalog", "builtin:enum_catalog:base"), "已切换到内建枚举全集。");
    }
    reportStatus("当前来源不支持该分类的获取操作。");
  };

  const fetchButtonLabel = (): string => {
    if (route === "block_icon" && blockIconDownloadBusy) {
      return selectedSource === "builtin" ? "提取中..." : "下载中...";
    }
    if (route === "item_icon" && itemIconDownloadBusy) {
      return selectedSource === "builtin" ? "提取中..." : "下载中...";
    }
    if (route === "language" && languageDownloadBusy) return "下载中...";
    if (route === "enum_catalog" && enumCatalogDownloadBusy) return "下载中...";
    if (selectedSource === "builtin") return route === "block_icon" || route === "item_icon" ? "重新解包" : "使用内建";
    if (selectedSource === "local_directory" || selectedSource === "local_json") return "导入...";
    return "下载";
  };

  // ── Line.3 sub-options ────────────────────────────────────────────────────
  const renderSubOptionControls = () => {
    if (route === "block_icon") {
      return (
        <>
          <select
            className="input asset-manager-select"
            aria-label="子选项1：激活槽位"
            value={effectiveBlockIconSlot}
            disabled={!!lockedBlockIconSlot || busy}
            onChange={(event) => setSelectedBlockIconSlot(event.target.value as BlockIconResourceSlot)}
          >
            <option value="layering">2D 槽（分层）</option>
            <option value="material_list">3D 槽（材料列表）</option>
          </select>
          <select className="input asset-manager-select" aria-label="子选项2：来源说明" disabled value="">
            <option value="">{blockIconSourceHint(selectedSource, effectiveBlockIconSlot)}</option>
          </select>
        </>
      );
    }
    if (remoteLanguageEnabled) {
      return (
        <>
          <select
            className="input asset-manager-select"
            aria-label="子选项1：版本"
            value={selectedLanguageBranch}
            disabled={languageCatalogBusy || languageBranches.length === 0}
            onChange={(event) => void loadLanguageCatalog(event.target.value)}
          >
            {languageBranches.length === 0 ? <option value="">版本</option> : null}
            {languageBranches.map((branch) => (
              <option key={branch} value={branch}>
                {branch}
              </option>
            ))}
          </select>
          <select
            className="input asset-manager-select"
            aria-label="子选项2：语言"
            value={selectedRemoteLanguage}
            disabled={languageCatalogBusy || remoteLanguages.length === 0}
            onChange={(event) => setSelectedRemoteLanguage(event.target.value)}
          >
            {remoteLanguages.length === 0 ? <option value="">语言</option> : null}
            {remoteLanguages.map((language) => (
              <option key={language} value={language}>
                {language}
              </option>
            ))}
          </select>
        </>
      );
    }
    return (
      <>
        <select className="input asset-manager-select" aria-label="子选项1" disabled value="">
          <option value="">子选项1</option>
        </select>
        <select className="input asset-manager-select" aria-label="子选项2" disabled value="">
          <option value="">子选项2</option>
        </select>
      </>
    );
  };

  // ── Line.5 table ──────────────────────────────────────────────────────────
  const tableColumns = useMemo(() => {
    const columns: { key: string; label: string }[] = [
      { key: "name", label: "名称" },
      { key: "source", label: "来源" },
    ];
    if (route === "language") columns.push({ key: "language", label: "语言" });
    columns.push({ key: "version", label: route === "enum_catalog" ? "日期" : "版本" });
    if (route === "block_icon") columns.push({ key: "usage", label: "用于" });
    if (route !== "block_icon") columns.push({ key: "active", label: "状态" });
    columns.push({ key: "actions", label: "操作" });
    return columns;
  }, [route]);

  const handleApply = async (entry: GameResourceEntry, slot?: BlockIconResourceSlot) => {
    try {
      await applyResourceSnapshot(
        activateGameResource(entry.kind, entry.id, slot),
        `已应用：${displayEntryName(route, entry)}`,
      );
    } catch (error) {
      reportStatus("应用资源失败。", String(error));
    }
  };

  const handleDelete = async (entry: GameResourceEntry) => {
    try {
      await applyResourceSnapshot(
        deleteGameResource(entry.kind, entry.id),
        `已删除登记：${displayEntryName(route, entry)}`,
      );
    } catch (error) {
      reportStatus("删除登记失败。", String(error));
    }
  };

  const renderCell = (entry: GameResourceEntry, key: string) => {
    switch (key) {
      case "name":
        return displayEntryName(route, entry);
      case "source":
        return sourceTag(entry);
      case "language":
        return entry.language || "--";
      case "version":
        return displayVersion(route, entry);
      case "usage":
        return blockUsageLabel(entry) || "--";
      case "active":
        return activationLabel(route, entry) || "--";
      case "actions":
        return (
          <div className="game-resource-row-actions">
            {route === "block_icon" ? (
              <>
                <button className="btn" type="button" disabled={busy} onClick={() => void handleApply(entry, "material_list")}>
                  用于材料
                </button>
                <button className="btn" type="button" disabled={busy} onClick={() => void handleApply(entry, "layering")}>
                  用于分层
                </button>
              </>
            ) : (
              <button className="btn" type="button" disabled={busy} onClick={() => void handleApply(entry)}>
                应用
              </button>
            )}
            {entry.builtin ? null : (
              <button className="btn" type="button" disabled={busy} onClick={() => void handleDelete(entry)}>
                删除登记
              </button>
            )}
          </div>
        );
      default:
        return null;
    }
  };

  const handleHealth = async () => {
    try {
      const rows = await checkGameResourceHealth();
      setHealth(rows);
      const bad = rows.filter((row) => !row.ok).length;
      reportStatus(bad ? `健康检查发现 ${bad} 项异常。` : "健康检查完成，未发现异常。");
    } catch (error) {
      reportStatus("健康检查失败。", String(error));
    }
  };

  const handleOpenRoot = async () => {
    if (!resourceSnapshot) return;
    await openWorkspacePath(resourceSnapshot.config_dir);
    reportStatus(`已打开根目录：${displayFilePath(resourceSnapshot.config_dir)}`);
  };

  const fetchDisabled =
    busy ||
    (remoteLanguageEnabled && (!selectedLanguageBranch || !selectedRemoteLanguage)) ||
    route === "game_data" ||
    (route === "enum_catalog" && selectedSource !== "wiki") ||
    (route === "language" && selectedSource === "builtin");

  return (
    <main className="asset-manager-window">
      <div className="asset-manager-header">
        <h3 className="asset-manager-title">游戏资源管理</h3>
        {onClose ? (
          <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}>
            ×
          </button>
        ) : null}
      </div>

      <div className="asset-manager-body">
        <aside className="asset-manager-sidebar">
          <nav className="asset-manager-nav" aria-label="游戏资源分类">
            {ASSET_NAV_ITEMS.map((item) => {
              const active = route === item.key;
              return (
                <Button
                  key={item.key}
                  className={`asset-manager-nav-item${active ? " active" : ""}`}
                  type="button"
                  aria-pressed={active}
                  onClick={() => setRoute(item.key)}
                >
                  <NavIcon name={item.icon} theme={theme} className="asset-manager-nav-icon" />
                  <span>{item.label}</span>
                </Button>
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
                <label className="asset-manager-form-label" htmlFor="asset-manager-source">
                  来源
                </label>
                <select
                  id="asset-manager-source"
                  className="input asset-manager-select"
                  value={selectedSource}
                  disabled={busy}
                  onChange={(event) =>
                    setSourceByRoute((current) => ({
                      ...current,
                      [route]: event.target.value as AssetSource,
                    }))
                  }
                >
                  {SOURCE_OPTIONS_BY_ROUTE[route].map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>

              <div className="asset-manager-form-row asset-manager-form-row-suboptions">
                <span className="asset-manager-form-label">子选项</span>
                <div className="asset-manager-inline-controls">
                  {renderSubOptionControls()}
                  {remoteLanguageEnabled ? (
                    <button
                      className="btn"
                      type="button"
                      disabled={languageCatalogBusy}
                      onClick={() => void loadLanguageCatalog()}
                    >
                      刷新远端列表
                    </button>
                  ) : null}
                </div>
              </div>
            </div>

            <div className="asset-manager-actions">
              <button className="btn" type="button" disabled={fetchDisabled} onClick={() => void runFetchAction()}>
                {fetchButtonLabel()}
              </button>
            </div>

            {health.length > 0 ? (
              <div className="asset-manager-health">
                {health.map((row) => (
                  <div key={row.label} className={row.ok ? "game-resource-health-ok" : "game-resource-health-failed"}>
                    {row.ok ? "OK" : "失败"} {row.label}
                  </div>
                ))}
              </div>
            ) : null}

            <div className="asset-manager-table-wrap">
              <table>
                <thead>
                  <tr>
                    {tableColumns.map((column) => (
                      <th key={column.key}>{column.label}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {entries.length > 0 ? (
                    entries.map((entry) => (
                      <tr key={entry.id}>
                        {tableColumns.map((column) => (
                          <td key={column.key} className={column.key === "name" ? "game-resource-path" : undefined}>
                            {renderCell(entry, column.key)}
                          </td>
                        ))}
                      </tr>
                    ))
                  ) : (
                    <tr>
                      <td colSpan={tableColumns.length} className="asset-manager-empty-row">
                        暂无已登记资源。
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>

            {log ? <pre className="subwindow-error asset-manager-log">{log}</pre> : null}
          </div>

          <footer className="asset-manager-footer">
            <span className="subwindow-status-text asset-manager-footer-status">{statusText}</span>
            <div className="asset-manager-footer-actions">
              <button
                className="btn"
                type="button"
                onClick={() => void refreshGameResources().catch(() => undefined)}
              >
                刷新
              </button>
              <button className="btn" type="button" onClick={() => void handleHealth()}>
                健康检查
              </button>
              <button className="btn" type="button" onClick={() => void handleOpenRoot()}>
                打开根目录...
              </button>
            </div>
          </footer>
        </section>
      </div>
    </main>
  );
}
