import { useCallback, useEffect, useMemo, useState } from "react";
import { Boxes, Database, Languages, ListTree, Package } from "lucide-react";

import {
  activateGameResource,
  checkGameResourceHealth,
  cleanupLocalTempFiles,
  deleteGameResource,
  downloadRemoteMinecraftLanguage,
  downloadVaultBlockIconsFromVault,
  downloadVaultItemIconsFromVault,
  downloadWikiBlockIconsFromMinecraftWiki,
  downloadWikiEnumCatalogsFromMinecraftWiki,
  ensureBuiltinBlockIconsExtractedNative,
  ensureBuiltinItemIconsExtractedNative,
  importGameDataResource,
  importLanguageResource,
  invalidateBlockIconCache,
  invalidateBlockstateDbCache,
  invalidateI18nCache,
  listGameResourceRegistry,
  listRemoteMinecraftLanguageBranches,
  listRemoteMinecraftLanguages,
  openDialog,
  openWorkspacePath,
  registerExternalIconDirectory,
  resolveBlockIconVariant,
  supportsBlockIconLayering,
  type BlockIconSlot,
  type GameResourceEntry,
  type GameResourceHealthRow,
  type GameResourceKind,
  type GameResourceSnapshot,
} from "../../../src/business/facade";
import { Button } from "../../components/ui/button";

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
  icon: typeof Boxes;
}

const ASSET_NAV_ITEMS: AssetNavItem[] = [
  { key: "block_icon", label: "方块图标", icon: Boxes },
  { key: "item_icon", label: "物品图标", icon: Package },
  { key: "language", label: "语言", icon: Languages },
  { key: "game_data", label: "游戏数据", icon: Database },
  { key: "enum_catalog", label: "枚举全集", icon: ListTree },
];

const ROUTE_INTRO: Record<AssetRoute, string> = {
  block_icon: "管理方块图标来源，材料列表与分层画布可使用不同的图标包。",
  item_icon: "管理物品图标来源，用于创造模式物品栏与物品形态方块。",
  language: "管理语言包来源，切换后界面与方块名称会立即使用新的语言文件。",
  game_data: "管理 BlockState 数据库与其属性翻译，决定方块状态提示可用的版本。",
  enum_catalog: "管理枚举全集（方块／物品／魔咒／实体），可跟随 Minecraft 版本更新。",
};

/** Sources that can be fetched automatically (`获取` button). */
const FETCHABLE: Record<AssetRoute, AssetSource[]> = {
  block_icon: ["builtin", "vault", "wiki", "wiki_zh"],
  item_icon: ["builtin", "vault"],
  language: ["github"],
  game_data: [],
  enum_catalog: ["wiki"],
};

/** Sources that import from a local file/directory (`导入` button). */
const IMPORTABLE: Record<AssetRoute, AssetSource[]> = {
  block_icon: ["local_directory"],
  item_icon: ["local_directory"],
  language: ["local_json"],
  game_data: ["local_json"],
  enum_catalog: [],
};

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

const DEFAULT_SOURCE_BY_ROUTE: Record<AssetRoute, AssetSource> = {
  block_icon: "builtin",
  item_icon: "builtin",
  language: "builtin",
  game_data: "builtin",
  enum_catalog: "builtin",
};

function sourceLabel(route: AssetRoute, source: AssetSource): string {
  return SOURCE_OPTIONS_BY_ROUTE[route].find((option) => option.value === source)?.label || source;
}

function sourceTag(entry: GameResourceEntry): string {
  switch (entry.source) {
    case "builtin":
      return "内建";
    case "imported":
      return "导入";
    case "external":
      return "本地目录";
    case "github":
      return "Github";
    case "vault":
      return "CCVault";
    case "wiki":
      return "Minecraft Wiki";
    case "wiki_zh":
      return "中文维基";
    default:
      return entry.source;
  }
}

function formatDate(value?: string): string {
  if (!value) return "--";
  try {
    return new Date(value).toLocaleString("zh-CN");
  } catch {
    return value;
  }
}

function activationLabel(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "block_icon") {
    const slots: string[] = [];
    if (entry.active_material_list) slots.push("材料列表");
    if (entry.active_layering) slots.push("分层");
    return slots.length ? slots.join(" / ") : "--";
  }
  if (route === "item_icon") return entry.active_layering ? "分层" : "--";
  return entry.active ? "已激活" : "--";
}

function usageLabel(entry: GameResourceEntry): string {
  const variant = entry.block_icon_variant || "2d";
  const parts = [variant === "3d" ? "3D" : "2D"];
  if (supportsBlockIconLayering(entry)) parts.push("可用于分层");
  return parts.join(" · ");
}

function versionOf(route: AssetRoute, entry: GameResourceEntry): string {
  if (route === "language") return entry.language || entry.version || "--";
  if (route === "enum_catalog") return entry.source === "builtin" ? "内建" : formatDate(entry.installed_at);
  return entry.version || "--";
}

/**
 * Game-resource manager window.
 *
 * Layout follows the pre-merge design: fixed category sidebar, then a content
 * column of heading → configuration form → actions → resource list → footer.
 * Every picker is a native `<select>` styled like a text input, so keyboard
 * navigation works without a custom popup.
 */
export function AssetManagerContent({ onClose }: { onClose?: () => void }) {
  const [route, setRoute] = useState<AssetRoute>("block_icon");
  const [blockIconSlot, setBlockIconSlot] = useState<BlockIconResourceSlot>("material_list");
  const [sourceByRoute, setSourceByRoute] = useState(DEFAULT_SOURCE_BY_ROUTE);
  const [snapshot, setSnapshot] = useState<GameResourceSnapshot | null>(null);
  const [health, setHealth] = useState<GameResourceHealthRow[]>([]);
  const [statusText, setStatusText] = useState("");
  const [log, setLog] = useState("");
  const [busy, setBusy] = useState(false);
  const [languageBranches, setLanguageBranches] = useState<string[]>([]);
  const [remoteLanguages, setRemoteLanguages] = useState<string[]>([]);
  const [selectedLanguageBranch, setSelectedLanguageBranch] = useState("");
  const [selectedRemoteLanguage, setSelectedRemoteLanguage] = useState("");

  const selectedSource = sourceByRoute[route];
  const selectedNavItem = ASSET_NAV_ITEMS.find((item) => item.key === route) || ASSET_NAV_ITEMS[0];

  const refreshGameResources = useCallback(async () => {
    try {
      setSnapshot(await listGameResourceRegistry());
    } catch (error: any) {
      setLog(String(error?.message || error));
    }
  }, []);

  useEffect(() => {
    void refreshGameResources();
  }, [refreshGameResources]);

  const entries = snapshot ? snapshot.entries[route] || [] : [];

  const activeEntryId = useMemo(() => {
    if (!snapshot) return "";
    switch (route) {
      case "block_icon":
        return blockIconSlot === "layering"
          ? snapshot.active_layering_block_icons.id
          : snapshot.active_material_list_icons.id;
      case "item_icon":
        return snapshot.active_layering_item_icons.id;
      case "language":
        return snapshot.active_language.id;
      case "game_data":
        return snapshot.active_game_data.id;
      case "enum_catalog":
        return snapshot.active_enum_catalog.id;
      default:
        return "";
    }
  }, [blockIconSlot, route, snapshot]);

  /** Drops the runtime caches fed by the activated resource kind. */
  const invalidateFor = useCallback(async (kind: AssetRoute) => {
    if (kind === "block_icon" || kind === "item_icon") {
      invalidateBlockIconCache();
    } else if (kind === "language") {
      invalidateI18nCache();
    } else if (kind === "game_data") {
      invalidateBlockstateDbCache();
    }
  }, []);

  const run = useCallback(
    async (label: string, action: () => Promise<string>) => {
      setBusy(true);
      setStatusText(`${label}…`);
      setLog("");
      try {
        setStatusText(await action());
      } catch (error: any) {
        const message = String(error?.message || error);
        setStatusText(`${label}失败`);
        setLog(message);
      } finally {
        setBusy(false);
        await refreshGameResources();
      }
    },
    [refreshGameResources],
  );

  const handleFetch = () => {
    const source = selectedSource;
    void run(`正在从 ${sourceLabel(route, source)} 获取`, async () => {
      if (route === "block_icon" && source === "builtin") {
        const result = await ensureBuiltinBlockIconsExtractedNative(true);
        return `内建方块图标已解包（${result.extracted} 个文件）`;
      }
      if (route === "item_icon" && source === "builtin") {
        const result = await ensureBuiltinItemIconsExtractedNative(true);
        return `内建物品图标已解包（${result.extracted} 个文件）`;
      }
      if (route === "block_icon" && source === "vault") {
        const result = await downloadVaultBlockIconsFromVault();
        return `CCVault 方块图标 ${result.downloaded}/${result.total}`;
      }
      if (route === "item_icon" && source === "vault") {
        const result = await downloadVaultItemIconsFromVault();
        return `CCVault 物品图标 ${result.downloaded}/${result.total}`;
      }
      if (route === "block_icon" && (source === "wiki" || source === "wiki_zh")) {
        const result = await downloadWikiBlockIconsFromMinecraftWiki(blockIconSlot, source);
        return `Wiki 方块图标 ${result.downloaded}/${result.total}`;
      }
      if (route === "enum_catalog" && source === "wiki") {
        const result = await downloadWikiEnumCatalogsFromMinecraftWiki();
        return `枚举全集：方块 ${result.blocks}、物品 ${result.items}、魔咒 ${result.enchantments}、实体 ${result.entities}`;
      }
      if (route === "language" && source === "github") {
        if (!selectedLanguageBranch || !selectedRemoteLanguage) {
          throw new Error("请先选择版本与语言");
        }
        await downloadRemoteMinecraftLanguage(selectedLanguageBranch, selectedRemoteLanguage);
        invalidateI18nCache();
        return `已下载语言 ${selectedLanguageBranch}/${selectedRemoteLanguage}`;
      }
      throw new Error(`来源「${sourceLabel(route, source)}」不支持自动获取`);
    });
  };

  const handleImport = () => {
    const source = selectedSource;
    void run("正在导入", async () => {
      if (route === "language" && source === "local_json") {
        const picked = await openDialog({
          filters: [{ name: "Language JSON", extensions: ["json"] }],
        });
        const path = typeof picked === "string" ? picked : null;
        if (!path) return "已取消导入";
        await importLanguageResource(path);
        invalidateI18nCache();
        return `已导入语言包 ${path}`;
      }
      if (route === "game_data" && source === "local_json") {
        const picked = await openDialog({
          filters: [{ name: "BlockState JSON", extensions: ["json"] }],
        });
        const dbPath = typeof picked === "string" ? picked : null;
        if (!dbPath) return "已取消导入";
        await importGameDataResource(dbPath);
        invalidateBlockstateDbCache();
        return `已导入游戏数据 ${dbPath}`;
      }
      if ((route === "block_icon" || route === "item_icon") && source === "local_directory") {
        const picked = await openDialog({ directory: true });
        const dir = typeof picked === "string" ? picked : null;
        if (!dir) return "已取消导入";
        const slot: BlockIconResourceSlot = route === "block_icon" ? blockIconSlot : "layering";
        await registerExternalIconDirectory(route, dir, slot);
        invalidateBlockIconCache();
        return `已登记外部图标目录 ${dir}`;
      }
      throw new Error(`来源「${sourceLabel(route, source)}」不支持本地导入`);
    });
  };

  const handleResetRemoteCatalog = () => {
    void run("正在刷新远端语言列表", async () => {
      const branches = await listRemoteMinecraftLanguageBranches();
      setLanguageBranches(branches);
      setSelectedLanguageBranch("");
      setRemoteLanguages([]);
      setSelectedRemoteLanguage("");
      return `远端分支 ${branches.length} 个`;
    });
  };

  const handleBranchChange = (branch: string) => {
    setSelectedLanguageBranch(branch);
    setSelectedRemoteLanguage("");
    void run(`正在获取分支 ${branch} 的语言`, async () => {
      const languages = await listRemoteMinecraftLanguages(branch);
      setRemoteLanguages(languages);
      return `分支 ${branch} 有 ${languages.length} 种语言`;
    });
  };

  const handleActivate = (entry: GameResourceEntry) => {
    void run(`正在激活「${entry.label}」`, async () => {
      const slot = entry.kind === "block_icon" ? blockIconSlot : undefined;
      await activateGameResource(entry.kind, entry.id, slot);
      await invalidateFor(entry.kind);
      return `已激活「${entry.label}」`;
    });
  };

  const handleDelete = (entry: GameResourceEntry) => {
    void run(`正在删除登记「${entry.label}」`, async () => {
      await deleteGameResource(entry.kind, entry.id);
      await invalidateFor(entry.kind);
      return `已删除登记「${entry.label}」`;
    });
  };

  const handleHealth = () => {
    void run("正在健康检查", async () => {
      const rows = await checkGameResourceHealth();
      setHealth(rows);
      const bad = rows.filter((row) => !row.ok).length;
      return bad ? `健康检查发现 ${bad} 项异常` : "健康检查完成，未发现异常";
    });
  };

  const handleOpenRoot = () => {
    void run("正在打开根目录", async () => {
      if (!snapshot) return "尚未加载资源列表";
      await openWorkspacePath(snapshot.config_dir);
      return `已打开 ${snapshot.config_dir}`;
    });
  };

  const canFetch = FETCHABLE[route].includes(selectedSource);
  const canImport = IMPORTABLE[route].includes(selectedSource);
  const fetchDisabled =
    busy ||
    !canFetch ||
    (route === "language" &&
      selectedSource === "github" &&
      (!selectedLanguageBranch || !selectedRemoteLanguage));

  /** Tab-specific selects, rendered in the `子选项` row. */
  const renderSubOptionControls = () => {
    if (route === "block_icon") {
      return (
        <>
          <select
            className="input asset-manager-select"
            aria-label="方块图标激活槽位"
            value={blockIconSlot}
            onChange={(event) => setBlockIconSlot(event.target.value as BlockIconResourceSlot)}
          >
            <option value="material_list">3D 槽（材料列表）</option>
            <option value="layering">2D 槽（分层）</option>
          </select>
          <select className="input asset-manager-select" aria-label="方块图标变体" disabled value="">
            <option value="">
              {blockIconSlot === "layering"
                ? "3D 图标不可用于分层，切换槽位时会自动限制"
                : "可使用 2D / 3D 图标"}
            </option>
          </select>
        </>
      );
    }
    if (route === "language" && selectedSource === "github") {
      return (
        <>
          <select
            className="input asset-manager-select"
            aria-label="远端版本分支"
            value={selectedLanguageBranch}
            disabled={busy || languageBranches.length === 0}
            onChange={(event) => handleBranchChange(event.target.value)}
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
            aria-label="远端语言"
            value={selectedRemoteLanguage}
            disabled={busy || remoteLanguages.length === 0}
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
    const hints: Record<AssetRoute, string> = {
      block_icon: "",
      item_icon: "物品图标使用 16px 原生尺寸",
      language: selectedSource === "local_json" ? "选择 .json 语言文件" : "内建语言包始终作为回退",
      game_data:
        selectedSource === "local_json"
          ? "选择 BlockState 数据库 .json（属性翻译同名 .zh_cn.json 会自动匹配）"
          : "内建为当前随包版本",
      enum_catalog: selectedSource === "wiki" ? "从 Minecraft Wiki 抓取四个目录" : "内建枚举全集",
    };
    return (
      <>
        <select className="input asset-manager-select" aria-label="子选项" disabled value="">
          <option value="">{hints[route] || "无额外子选项"}</option>
        </select>
      </>
    );
  };

  const tableColumns = [
    { key: "name" as const, label: "名称" },
    { key: "source" as const, label: "来源" },
    { key: "version" as const, label: "版本" },
    ...(route === "block_icon" ? [{ key: "usage" as const, label: "用于" }] : []),
    { key: "active" as const, label: "激活" },
    { key: "installed" as const, label: "日期" },
    { key: "actions" as const, label: "操作" },
  ];

  const renderCell = (entry: GameResourceEntry, key: string) => {
    switch (key) {
      case "name":
        return entry.label;
      case "source":
        return sourceTag(entry);
      case "version":
        return versionOf(route, entry);
      case "usage":
        return usageLabel(entry);
      case "active":
        return activationLabel(route, entry);
      case "installed":
        return formatDate(entry.installed_at);
      case "actions":
        return (
          <div className="game-resource-row-actions">
            <button
              className="btn"
              type="button"
              disabled={busy || entry.id === activeEntryId}
              onClick={() => handleActivate(entry)}
            >
              {entry.id === activeEntryId ? "当前" : "激活"}
            </button>
            {entry.builtin ? null : (
              <button className="btn" type="button" disabled={busy} onClick={() => handleDelete(entry)}>
                删除登记
              </button>
            )}
          </div>
        );
      default:
        return null;
    }
  };

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
        <aside className="sidebar asset-manager-sidebar">
          <nav className="asset-manager-nav" aria-label="游戏资源分类">
            {ASSET_NAV_ITEMS.map((item) => {
              const Icon = item.icon;
              const active = route === item.key;
              return (
                <Button
                  key={item.key}
                  className={`asset-manager-nav-item${active ? " active" : ""}`}
                  type="button"
                  aria-pressed={active}
                  onClick={() => setRoute(item.key)}
                >
                  <Icon className="asset-manager-nav-icon" aria-hidden="true" />
                  <span>{item.label}</span>
                </Button>
              );
            })}
          </nav>
        </aside>

        <section className="asset-manager-content">
          <div className="asset-manager-heading">
            <h3>{selectedNavItem.label}</h3>
            <p className="muted">{ROUTE_INTRO[route]}</p>
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

              <div className="asset-manager-form-row asset-manager-form-row-top">
                <span className="asset-manager-form-label">子选项</span>
                <div className="asset-manager-inline-controls">{renderSubOptionControls()}</div>
              </div>
            </div>

            {/* Actions sit directly below the form, above the list. */}
            <div className="asset-manager-actions">
              <button className="btn" type="button" disabled={fetchDisabled} onClick={handleFetch}>
                {route === "block_icon" && selectedSource === "builtin" ? "提取内建图标" : "获取"}
              </button>
              {canImport ? (
                <button className="btn" type="button" disabled={busy} onClick={handleImport}>
                  导入
                </button>
              ) : null}
              {route === "language" && selectedSource === "github" ? (
                <button className="btn" type="button" disabled={busy} onClick={handleResetRemoteCatalog}>
                  刷新远端列表
                </button>
              ) : null}
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

            <div className="table-wrap asset-manager-table-wrap">
              <table className="table">
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

            <div className="game-resource-note">
              内建资源始终作为离线回退；导入目录会登记外部路径，导入 JSON 与远端下载会复制到用户配置目录下的
              `minecraft-assets` 或 `enumerator/base`。
            </div>
            {log ? <pre className="subwindow-error asset-manager-log">{log}</pre> : null}
          </div>

          <footer className="asset-manager-footer">
            <span className="subwindow-status-text asset-manager-footer-status">{statusText}</span>
            <div className="asset-manager-footer-actions">
              <button className="btn" type="button" onClick={() => void refreshGameResources()}>
                刷新
              </button>
              <button className="btn" type="button" onClick={handleHealth}>
                健康检查
              </button>
              <button className="btn" type="button" onClick={handleOpenRoot}>
                打开根目录...
              </button>
              <button className="btn" type="button" onClick={() => void cleanupLocalTempFiles()}>
                清理本地临时文件
              </button>
              {onClose ? (
                <button className="btn" type="button" onClick={onClose}>
                  关闭窗口
                </button>
              ) : null}
            </div>
          </footer>
        </section>
      </div>
    </main>
  );
}
