import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
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
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../components/ui/table";
import { Dropdown } from "../../components/Dropdown";
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
  { key: "block_icon", get label() { return t("library.blockIcons"); }, icon: "asset-block" },
  { key: "item_icon", get label() { return t("library.itemIcons"); }, icon: "asset-item" },
  { key: "language", get label() { return t("library.language"); }, icon: "asset-language" },
  { key: "game_data", get label() { return t("library.gameData"); }, icon: "asset-db" },
  { key: "enum_catalog", get label() { return t("library.enumCatalog"); }, icon: "asset-enum" },
];

const BUILTIN_BLOCK_ICON_ID = "builtin:block_icon:nova";
const BUILTIN_ITEM_ICON_ID = "builtin:item_icon:nova";

const SOURCE_OPTIONS_BY_ROUTE: Record<AssetRoute, { value: AssetSource; label: string }[]> = {
  block_icon: [
    { value: "builtin", get label() { return t("library.builtin"); } },
    { value: "local_directory", get label() { return t("library.localDirectory"); } },
    { value: "vault", label: "CCVault" },
    { value: "wiki", label: "Minecraft Wiki" },
    { value: "wiki_zh", get label() { return t("library.chineseWiki"); } },
  ],
  item_icon: [
    { value: "builtin", get label() { return t("library.builtin"); } },
    { value: "local_directory", get label() { return t("library.localDirectory"); } },
    { value: "vault", label: "CCVault" },
  ],
  language: [
    { value: "builtin", get label() { return t("library.builtin"); } },
    { value: "local_json", get label() { return t("library.localJson"); } },
    { value: "github", label: "Github/InventivetalentDev" },
  ],
  game_data: [
    { value: "builtin", get label() { return t("library.builtin"); } },
    { value: "local_json", get label() { return t("library.localJson"); } },
  ],
  enum_catalog: [
    { value: "builtin", get label() { return t("library.builtin"); } },
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
      return t("library.blockIconIntro");
    case "item_icon":
      return t("library.itemIconIntro");
    case "language":
      return t("library.languageIntro");
    case "game_data":
      return t("library.gameDataIntro");
    case "enum_catalog":
      return t("library.enumIntro");
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
      return t("library.builtin");
    case "imported":
    case "external":
      return t("library.localSource");
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
  if (!value) return t("library.unknown");
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return t("library.unknown");
  return new Intl.DateTimeFormat(getUiLocale(), { dateStyle: "short" }).format(date);
}

function blockUsageLabel(entry: GameResourceEntry): string {
  return [entry.active_material_list ? t("library.materialSlot") : "", entry.active_layering ? t("library.slot2d") : ""]
    .filter(Boolean)
    .join("/");
}

function activationLabel(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "item_icon") return entry.active_layering ? t("library.active") : "";
  if (route === "language" || route === "game_data" || route === "enum_catalog") {
    return entry.active ? t("library.active") : "";
  }
  return blockUsageLabel(entry);
}

function displayEntryName(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "block_icon" || route === "item_icon") {
    const prefix = route === "block_icon" ? (resolveBlockIconVariant(entry) === "2d" ? "[2D] " : "[3D] ") : "";
    if (entry.source === "builtin") return route === "block_icon" ? t("library.builtinPrefixed", { prefix: prefix }) : t("library.builtinParenthesized");
    if (entry.source === "external") return `${prefix}${baseName(entry.root_path || entry.label)}/`;
    if (entry.source === "vault") return `${prefix}CCVault`;
    return `${prefix}${entry.label}`;
  }
  if (route === "language") {
    if (entry.source === "builtin") return t("library.builtinLanguage", { language: entry.language || "zh_cn" });
    if (entry.source === "imported") return baseName(entry.file_relpath || entry.label);
    if (entry.source === "github") return entry.language || entry.label;
    return entry.label;
  }
  if (route === "game_data") {
    if (entry.source === "builtin") return t("library.builtinParenthesized");
    if (entry.source === "imported") return baseName(entry.data_relpath || entry.label);
    return entry.label;
  }
  if (route === "enum_catalog") {
    if (entry.source === "builtin") return t("library.builtinParenthesized");
    if (entry.source === "wiki") return "Minecraft Wiki";
    return entry.label;
  }
  return entry.label;
}

function displayVersion(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "block_icon") return resolveBlockIconVariant(entry) === "2d" ? "2D" : "3D";
  if (route === "language") {
    return entry.source === "github" ? entry.branch || entry.version || t("library.unknown") : t("library.unknown");
  }
  if (route === "enum_catalog") {
    return entry.source === "builtin" ? "2025/4/22" : formatDate(entry.installed_at);
  }
  return t("library.unknown");
}

function blockIconSlotLabel(slot: BlockIconResourceSlot): string {
  return slot === "layering" ? t("library.layeringSlot") : t("library.materialListSlot");
}

/** Sources that can only write one slot; `null` means the user chooses. */
function lockedBlockIconSlotForSource(source: AssetSource): BlockIconResourceSlot | null {
  if (source === "builtin") return "layering";
  if (source === "vault") return "material_list";
  return null; // wiki / wiki_zh / local_directory can target either slot
}

function blockIconSourceHint(source: AssetSource, slot: BlockIconResourceSlot): string {
  if (source === "builtin") return t("library.builtinSlotHint");
  if (source === "vault") return t("library.vaultSlotHint");
  if (source === "wiki" || source === "wiki_zh") {
    const label = source === "wiki_zh" ? t("library.wikiChineseShort") : t("library.wikiEnglishShort");
    return slot === "layering"
      ? t("library.wiki2dHint", { source: label })
      : t("library.wiki3dHint", { source: label });
  }
  return slot === "layering" ? t("library.slot2dHint") : t("library.slot3dHint");
}

function resourceActiveText(snapshot: GameResourceSnapshot | null): UiMessage {
  if (!snapshot) return message("library.notLoaded");
  return message("library.activeResources", {
    language: snapshot.active_language.label,
    materials: snapshot.active_material_list_icons.label,
    layerBlocks: snapshot.active_layering_block_icons.label,
    layerItems: snapshot.active_layering_item_icons.label,
    data: snapshot.active_game_data.label,
    catalog: snapshot.active_enum_catalog.label,
  });
}

export function AssetManagerContent({ theme, onClose }: { theme: string; onClose?: () => void }) {
  const { locale } = useI18n();
  const [route, setRoute] = useState<AssetRoute>("block_icon");
  const [sourceByRoute, setSourceByRoute] = useState(DEFAULT_SOURCE_BY_ROUTE);
  const [selectedBlockIconSlot, setSelectedBlockIconSlot] = useState<BlockIconResourceSlot>("layering");
  const [resourceSnapshot, setResourceSnapshot] = useState<GameResourceSnapshot | null>(null);
  const [health, setHealth] = useState<GameResourceHealthRow[]>([]);
  const [statusText, setStatusText] = useState<UiMessage>("");
  const [log, setLog] = useState<UiMessage[]>([]);
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

  const reportStatus = useCallback((status: UiMessage, detail?: UiMessage) => {
    setStatusText(status);
    if (detail !== undefined) setLog(detail ? [status, detail] : [status]);
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
      if (!silent) setStatusText(message("library.resourcesRefreshed"));
      return snapshot;
    },
    [],
  );

  useEffect(() => {
    refreshGameResources(true).catch((error) => reportStatus(message("library.resourcesLoadFailed"), String(error)));
  }, [refreshGameResources, reportStatus]);

  // Icon download progress: without these the UI looks frozen while working.
  useEffect(() => {
    const unlistenPromise = listenEvent<IconDownloadProgressPayload>(
      "vault-block-icons-download-progress",
      (event) => {
        const payload = event.payload;
        if (!payload) return;
        reportStatus(payload.status, message("library.resourceProgress", { current: payload.current, total: payload.total, succeeded: payload.downloaded }));
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
        reportStatus(payload.status, message("library.resourceProgress", { current: payload.current, total: payload.total, succeeded: payload.downloaded }));
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
    async (snapshotPromise: Promise<GameResourceSnapshot>, statusMessage: UiMessage) => {
      const snapshot = await snapshotPromise;
      setResourceSnapshot(snapshot);
      await reloadRuntimeResources();
      reportStatus(statusMessage, resourceActiveText(snapshot));
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
        message("library.languageDownloaded", { version: selectedLanguageBranch, language: selectedRemoteLanguage }),
      );
    } catch (error) {
      reportStatus(message("library.languageDownloadFailed"), String(error));
    } finally {
      setLanguageDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const importLanguage = async () => {
    const selected = await openDialog({ filters: [{ name: t("library.languageJsonFilter"), extensions: ["json"] }] });
    if (typeof selected !== "string") return;
    try {
      await applyResourceSnapshot(importLanguageResource(selected), message("library.languageImported", { path: selected }));
    } catch (error) {
      reportStatus(message("library.languageImportFailed"), String(error));
    }
  };

  const downloadBlockIcons = async () => {
    setBlockIconDownloadBusy(true);
    reportStatus(message("library.vaultBlocksDownloading"), message("library.vaultBlocksDownloading"));
    try {
      const result = await downloadVaultBlockIcons();
      setResourceSnapshot(result.snapshot);
      await reloadRuntimeResources();
      reportStatus(
        message("library.vaultBlocksDownloaded", { downloaded: result.downloaded, total: result.total }),
        message("library.resourcePathSummary", { path: result.target_dir, resources: resourceActiveText(result.snapshot) }),
      );
    } catch (error) {
      reportStatus(message("library.vaultBlocksFailed"), String(error));
    } finally {
      setBlockIconDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const downloadWikiBlockIconsForSlot = async (slot: BlockIconResourceSlot, source: "wiki" | "wiki_zh") => {
    setBlockIconDownloadBusy(true);
    const label = source === "wiki_zh" ? message("library.chineseWiki") : "Minecraft Wiki";
    reportStatus(
      message("library.wikiBlocksDownloading", { source: label, slot: message(slot === "layering" ? "library.layeringSlot" : "library.materialListSlot") }),
      slot === "layering"
        ? message("library.wiki2dDownloading")
        : message("library.wiki3dDownloading"),
    );
    try {
      const result = await downloadWikiBlockIcons(slot, source);
      setResourceSnapshot(result.snapshot);
      await reloadRuntimeResources();
      reportStatus(
        message("library.wikiBlocksDownloaded", { source: label, slot: message(slot === "layering" ? "library.layeringSlot" : "library.materialListSlot"), downloaded: result.downloaded, total: result.total }),
        message("library.resourcePathSummary", { path: result.target_dir, resources: resourceActiveText(result.snapshot) }),
      );
    } catch (error) {
      reportStatus(message("library.wikiBlocksFailed", { source: label }), String(error));
    } finally {
      setBlockIconDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const reextractBuiltinBlockIcons = async (slot: BlockIconResourceSlot) => {
    setBlockIconDownloadBusy(true);
    reportStatus(message("library.builtinBlocksExtracting"), message("library.builtinBlocksExtractDetail"));
    try {
      const result = await ensureBuiltinBlockIconsExtracted(true);
      const snapshot = await activateGameResource("block_icon", BUILTIN_BLOCK_ICON_ID, slot);
      setResourceSnapshot(snapshot);
      await reloadRuntimeResources();
      reportStatus(
        message("library.builtinBlocksExtracted", { slot: message(slot === "layering" ? "library.layeringSlot" : "library.materialListSlot"), extracted: result.extracted }),
        message("library.resourcePathSummary", { path: result.target_dir, resources: resourceActiveText(snapshot) }),
      );
    } catch (error) {
      reportStatus(message("library.builtinBlocksExtractFailed"), String(error));
    } finally {
      setBlockIconDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const downloadItemIcons = async () => {
    setItemIconDownloadBusy(true);
    reportStatus(message("library.vaultItemsDownloading"), message("library.vaultItemsDownloading"));
    try {
      const result = await downloadVaultItemIcons();
      setResourceSnapshot(result.snapshot);
      await reloadRuntimeResources();
      reportStatus(
        message("library.vaultItemsDownloaded", { downloaded: result.downloaded, total: result.total }),
        message("library.resourcePathSummary", { path: result.target_dir, resources: resourceActiveText(result.snapshot) }),
      );
    } catch (error) {
      reportStatus(message("library.vaultItemsFailed"), String(error));
    } finally {
      setItemIconDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const reextractBuiltinItemIcons = async () => {
    setItemIconDownloadBusy(true);
    reportStatus(message("library.builtinItemsExtracting"), message("library.builtinItemsExtractDetail"));
    try {
      const result = await ensureBuiltinItemIconsExtracted(true);
      const snapshot = await refreshGameResources(true);
      await reloadRuntimeResources();
      reportStatus(
        message("library.builtinItemsExtracted", { extracted: result.extracted }),
        message("library.resourcePathSummary", { path: result.target_dir, resources: resourceActiveText(snapshot) }),
      );
    } catch (error) {
      reportStatus(message("library.builtinItemsExtractFailed"), String(error));
    } finally {
      setItemIconDownloadBusy(false);
    }
  };

  const downloadEnumCatalogs = async () => {
    setEnumCatalogDownloadBusy(true);
    reportStatus(message("library.enumDownloading"), message("library.enumDownloadingDetail"));
    try {
      const result = await downloadWikiEnumCatalog();
      setResourceSnapshot(result.snapshot);
      reportStatus(
        message("library.enumDownloaded", { blocks: result.blocks, items: result.items, enchantments: result.enchantments, entities: result.entities }),
        message("library.resourcePathSummary", { path: result.target_dir, resources: resourceActiveText(result.snapshot) }),
      );
    } catch (error) {
      reportStatus(message("library.enumDownloadFailed"), String(error));
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
        message("library.iconDirectoryRegistered", { path: selected }),
      );
    } catch (error) {
      reportStatus(message("library.iconDirectoryRegisterFailed"), String(error));
    } finally {
      setBlockIconDownloadBusy(false);
      await refreshGameResources(true).catch(() => undefined);
    }
  };

  const importGameData = async () => {
    const selected = await openDialog({ filters: [{ name: t("library.blockstateJsonFilter"), extensions: ["json"] }] });
    if (typeof selected !== "string") return;
    try {
      await applyResourceSnapshot(importGameDataResource(selected), message("library.gameDataImported", { path: selected }));
    } catch (error) {
      reportStatus(message("library.gameDataImportFailed"), String(error));
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
          reportStatus(message("library.remoteVersionsRefreshed", { count: branches.length }));
          return;
        }
        setSelectedLanguageBranch(branch);
        setSelectedRemoteLanguage("");
        const languages = await listRemoteMinecraftLanguages(branch);
        setRemoteLanguages(languages);
        reportStatus(message("library.remoteLanguagesCount", { version: branch, count: languages.length }));
      } catch (error) {
        reportStatus(message("library.remoteLanguagesFailed"), String(error));
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
        return applyResourceSnapshot(activateGameResource("language", "builtin:language:zh_cn"), message("library.builtinLanguageActivated"));
      }
    }
    if (route === "game_data") {
      if (selectedSource === "local_json") return importGameData();
      return applyResourceSnapshot(activateGameResource("game_data", "builtin:game_data:26.1"), message("library.builtinDataActivated"));
    }
    if (route === "enum_catalog") {
      if (selectedSource === "wiki") return downloadEnumCatalogs();
      return applyResourceSnapshot(activateGameResource("enum_catalog", "builtin:enum_catalog:base"), message("library.builtinEnumActivated"));
    }
    reportStatus(message("library.sourceUnsupported"));
  };

  const fetchButtonLabel = (): string => {
    if (route === "block_icon" && blockIconDownloadBusy) {
      return selectedSource === "builtin" ? t("library.extracting") : t("library.downloading");
    }
    if (route === "item_icon" && itemIconDownloadBusy) {
      return selectedSource === "builtin" ? t("library.extracting") : t("library.downloading");
    }
    if (route === "language" && languageDownloadBusy) return t("library.downloading");
    if (route === "enum_catalog" && enumCatalogDownloadBusy) return t("library.downloading");
    if (selectedSource === "builtin") return route === "block_icon" || route === "item_icon" ? t("library.extractAgain") : t("library.useBuiltin");
    if (selectedSource === "local_directory" || selectedSource === "local_json") return t("library.importMenu");
    return t("library.download");
  };

  // ── Line.3 sub-options ────────────────────────────────────────────────────
  const renderSubOptionControls = () => {
    if (route === "block_icon") {
      return (
        <>
          <Dropdown
            className="asset-manager-select"
            ariaLabel={t("library.slotOption")}
            value={effectiveBlockIconSlot}
            disabled={!!lockedBlockIconSlot || busy}
            onChange={(next) => setSelectedBlockIconSlot(next as BlockIconResourceSlot)}
            options={[
              { label: t("library.layeringSlot"), value: "layering" },
              { label: t("library.materialListSlot"), value: "material_list" },
            ]}
          />
          {/* A disabled field that only states the source; it has nothing to choose. */}
          <Dropdown
            className="asset-manager-select"
            ariaLabel={t("library.sourceHintOption")}
            value=""
            disabled
            onChange={() => {}}
            options={[{ label: blockIconSourceHint(selectedSource, effectiveBlockIconSlot), value: "" }]}
          />
        </>
      );
    }
    if (remoteLanguageEnabled) {
      return (
        <>
          <Dropdown
            className="asset-manager-select"
            ariaLabel={t("library.versionOption")}
            value={selectedLanguageBranch}
            disabled={languageCatalogBusy || languageBranches.length === 0}
            onChange={(next) => void loadLanguageCatalog(next)}
            options={languageBranches.length === 0
              ? [{ label: t("library.version"), value: "" }]
              : languageBranches.map((branch) => ({ label: branch, value: branch }))}
          />
          <Dropdown
            className="asset-manager-select"
            ariaLabel={t("library.languageOption")}
            value={selectedRemoteLanguage}
            disabled={languageCatalogBusy || remoteLanguages.length === 0}
            onChange={setSelectedRemoteLanguage}
            options={remoteLanguages.length === 0
              ? [{ label: t("library.language"), value: "" }]
              : remoteLanguages.map((language) => ({ label: language, value: language }))}
          />
        </>
      );
    }
    return (
      <>
        <Dropdown className="asset-manager-select" ariaLabel={t("library.option1")} value="" disabled onChange={() => {}} options={[{ label: t("library.option1"), value: "" }]} />
        <Dropdown className="asset-manager-select" ariaLabel={t("library.option2")} value="" disabled onChange={() => {}} options={[{ label: t("library.option2"), value: "" }]} />
      </>
    );
  };

  // ── Line.5 table ──────────────────────────────────────────────────────────
  const tableColumns = useMemo(() => {
    const columns: { key: string; label: string }[] = [
      { key: "name", label: t("library.name") },
      { key: "source", label: t("library.source") },
    ];
    if (route === "language") columns.push({ key: "language", label: t("library.language") });
    columns.push({ key: "version", label: route === "enum_catalog" ? t("library.date") : t("library.version") });
    if (route === "block_icon") columns.push({ key: "usage", label: t("library.usage") });
    if (route !== "block_icon") columns.push({ key: "active", label: t("library.status") });
    columns.push({ key: "actions", label: t("library.actions") });
    return columns;
  }, [route, locale]);

  const handleApply = async (entry: GameResourceEntry, slot?: BlockIconResourceSlot) => {
    try {
      await applyResourceSnapshot(
        activateGameResource(entry.kind, entry.id, slot),
        message("library.resourceApplied", { name: displayEntryName(route, entry) }),
      );
    } catch (error) {
      reportStatus(message("library.resourceApplyFailed"), String(error));
    }
  };

  const handleDelete = async (entry: GameResourceEntry) => {
    try {
      await applyResourceSnapshot(
        deleteGameResource(entry.kind, entry.id),
        message("library.resourceUnregistered", { name: displayEntryName(route, entry) }),
      );
    } catch (error) {
      reportStatus(message("library.resourceUnregisterFailed"), String(error));
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
                <Button type="button" disabled={busy} onClick={() => void handleApply(entry, "material_list")}>
                  {t("library.useForMaterials")}</Button>
                <Button type="button" disabled={busy} onClick={() => void handleApply(entry, "layering")}>
                  {t("library.useForLayers")}</Button>
              </>
            ) : (
              <Button type="button" disabled={busy} onClick={() => void handleApply(entry)}>
                {t("library.apply")}</Button>
            )}
            {entry.builtin ? null : (
              <Button type="button" disabled={busy} onClick={() => void handleDelete(entry)}>
                {t("library.unregister")}</Button>
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
      reportStatus(bad ? message("library.healthIssues", { count: bad }) : message("library.healthOk"));
    } catch (error) {
      reportStatus(message("library.healthFailed"), String(error));
    }
  };

  const handleOpenRoot = async () => {
    if (!resourceSnapshot) return;
    await openWorkspacePath(resourceSnapshot.config_dir);
    reportStatus(message("library.rootOpened", { path: displayFilePath(resourceSnapshot.config_dir) }));
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
        <h3 className="asset-manager-title">{t("library.assetManagerTitle")}</h3>
        {onClose ? (
          <Button className="subwindow-close-button" type="button" aria-label={t("library.closeWindow")} onClick={onClose}>
            ×
          </Button>
        ) : null}
      </div>

      <div className="asset-manager-body">
        <aside className="asset-manager-sidebar">
          <nav className="asset-manager-nav" aria-label={t("library.resourceCategories")}>
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
                  {t("library.source")}</label>
                <Dropdown
                  id="asset-manager-source"
                  className="asset-manager-select"
                  ariaLabel={t("library.source")}
                  value={selectedSource}
                  disabled={busy}
                  onChange={(next) =>
                    setSourceByRoute((current) => ({
                      ...current,
                      [route]: next as AssetSource,
                    }))
                  }
                  options={SOURCE_OPTIONS_BY_ROUTE[route]}
                />
              </div>

              <div className="asset-manager-form-row asset-manager-form-row-suboptions">
                <span className="asset-manager-form-label">{t("library.subOptions")}</span>
                <div className="asset-manager-inline-controls">
                  {renderSubOptionControls()}
                  {remoteLanguageEnabled ? (
                    <Button type="button" disabled={languageCatalogBusy} onClick={() => void loadLanguageCatalog()} >
                      {t("library.refreshRemoteList")}</Button>
                  ) : null}
                </div>
              </div>
            </div>

            <div className="asset-manager-actions">
              <Button type="button" disabled={fetchDisabled} onClick={() => void runFetchAction()}>
                {fetchButtonLabel()}
              </Button>
            </div>

            {health.length > 0 ? (
              <div className="asset-manager-health">
                {health.map((row) => (
                  <div key={localizeMessage(row.label)} className={row.ok ? "game-resource-health-ok" : "game-resource-health-failed"}>
                    {row.ok ? "OK" : t("library.failed")} {localizeMessage(row.label)}
                  </div>
                ))}
              </div>
            ) : null}

            <div className="asset-manager-table-wrap">
              <Table>
                <TableHeader>
                  <TableRow>
                    {tableColumns.map((column) => (
                      <TableHead key={column.key}>{column.label}</TableHead>
                    ))}
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {entries.length > 0 ? (
                    entries.map((entry) => (
                      <TableRow key={entry.id}>
                        {tableColumns.map((column) => (
                          <TableCell key={column.key} className={column.key === "name" ? "game-resource-path" : undefined}>
                            {renderCell(entry, column.key)}
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  ) : (
                    <TableRow>
                      <TableCell colSpan={tableColumns.length} className="asset-manager-empty-row">
                        {t("library.noRegisteredResources")}</TableCell>
                    </TableRow>
                  )}
                </TableBody>
              </Table>
            </div>

            {log.length ? <pre className="subwindow-error asset-manager-log">{log.map(localizeMessage).join("\n")}</pre> : null}
          </div>

          <footer className="asset-manager-footer">
            <span className="subwindow-status-text asset-manager-footer-status">{localizeMessage(statusText)}</span>
            <div className="asset-manager-footer-actions">
              <Button type="button" onClick={() => void refreshGameResources().catch(() => undefined)} >
                {t("library.refresh")}</Button>
              <Button type="button" onClick={() => void handleHealth()}>
                {t("library.healthCheck")}</Button>
              <Button type="button" onClick={() => void handleOpenRoot()}>
                {t("library.openRootMenu")}</Button>
            </div>
          </footer>
        </section>
      </div>
    </main>
  );
}
