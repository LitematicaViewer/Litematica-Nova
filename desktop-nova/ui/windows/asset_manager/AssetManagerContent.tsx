import { useCallback, useEffect, useMemo, useState } from "react";
import { Boxes, Database, Languages, ListTree, Package } from "lucide-react";

import {
  activateGameResource,
  checkGameResourceHealth,
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
  type BlockIconSlot,
  type GameResourceEntry,
  type GameResourceHealthRow,
  type GameResourceKind,
  type GameResourceSnapshot,
} from "../../../src/business/facade";
import { Button } from "../../components/ui/button";

/**
 * Game resources module — layout and wording follow `docs/design.md` §2.11.2.
 *
 * Per category (Line.1..Line.5):
 *   Line.1  title + subtitle
 *   Line.2  source dropdown (default 内建)
 *   Line.3  sub-option 1 / sub-option 2 (placeholders unless the source needs them)
 *   Line.4  primary action: 重新解包 (内建) / 导入... (本地) / 下载 (远端)
 *   Line.5  content table
 * Common footer: status text on the left; 刷新 / 健康检查 / 打开根目录... on the right.
 */

type AssetRoute = GameResourceKind;
type BlockIconResourceSlot = BlockIconSlot;

/** Source values from the spec: builtin / local / vault / wiki / github. */
type AssetSource = "builtin" | "local_directory" | "local_json" | "vault" | "wiki" | "wiki_zh" | "github";

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

/** Line.1 subtitle per category. */
const ROUTE_SUBTITLE: Record<AssetRoute, string> = {
  block_icon: "方块图标用于材料列表与分层画布，两个用途可分别指定来源。",
  item_icon: "物品图标用于创造模式物品栏与物品形态方块。",
  language: "语言包决定界面文字与方块名称的显示。",
  game_data: "游戏数据为 BlockState 数据库及其属性翻译。",
  enum_catalog: "枚举全集提供方块／物品／魔咒／实体的完整数据值列表。",
};

/** Line.2 options, exactly as listed in the spec. */
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

/** Source column values shown in the table, per spec. */
const SOURCE_TAG: Record<string, string> = {
  builtin: "builtin",
  imported: "local",
  external: "local",
  vault: "vault",
  wiki: "wiki",
  wiki_zh: "wiki",
  github: "github",
};

/** Spec: builtin/local version is 未知 until the enumerator supplies it. */
function versionOf(entry: GameResourceEntry): string {
  if (entry.source === "builtin" || entry.source === "imported" || entry.source === "external") {
    return entry.version || "未知";
  }
  return entry.version || "未知";
}

/** Spec Line.5 name column per source. */
function displayName(route: AssetRoute, entry: GameResourceEntry): string {
  if (entry.source === "builtin") {
    const label = entry.label.replace(/^（内建）/, "");
    // 语言 category: show the language code after the builtin marker.
    if (route === "language") return `（内建）${entry.language || label}`;
    return `（内建）${label}`;
  }
  if (entry.source === "external") {
    // Local directory: show `目录名称/` (trailing separator per spec).
    const dir = entry.root_path || entry.label;
    const normalized = dir.replace(/\\/g, "/").replace(/\/+$/, "");
    const base = normalized.split("/").filter(Boolean).pop() || normalized;
    return `${base}/`;
  }
  if (entry.source === "imported") {
    // Local JSON: show the file name including its extension.
    const file = entry.file_relpath || entry.label;
    return file.split(/[\\/]/).filter(Boolean).pop() || file;
  }
  if (entry.source === "vault") return "CCVault";
  if (entry.source === "wiki" || entry.source === "wiki_zh") return "Minecraft Wiki";
  if (entry.source === "github") return entry.language || entry.label;
  return entry.label;
}

function formatDate(value?: string): string {
  if (!value) return "--";
  try {
    return new Date(value).toLocaleString("zh-CN");
  } catch {
    return value;
  }
}

function isActiveFor(entry: GameResourceEntry, slot: BlockIconResourceSlot): boolean {
  if (entry.kind === "block_icon") {
    return slot === "layering" ? !!entry.active_layering : !!entry.active_material_list;
  }
  if (entry.kind === "item_icon") return !!entry.active_layering;
  return !!entry.active;
}

export function AssetManagerContent({ onClose }: { onClose?: () => void }) {
  const [route, setRoute] = useState<AssetRoute>("block_icon");
  const [blockIconSlot, setBlockIconSlot] = useState<BlockIconResourceSlot>("material_list");
  const [sourceByRoute, setSourceByRoute] = useState(DEFAULT_SOURCE_BY_ROUTE);
  const [snapshot, setSnapshot] = useState<GameResourceSnapshot | null>(null);
  const [health, setHealth] = useState<GameResourceHealthRow[]>([]);
  const [statusText, setStatusText] = useState("");
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
      setStatusText(String(error?.message || error));
    }
  }, []);

  useEffect(() => {
    void refreshGameResources();
  }, [refreshGameResources]);

  const entries = snapshot ? snapshot.entries[route] || [] : [];

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
      try {
        setStatusText(await action());
      } catch (error: any) {
        setStatusText(`${label}失败：${String(error?.message || error)}`);
      } finally {
        setBusy(false);
        await refreshGameResources();
      }
    },
    [refreshGameResources],
  );

  // ── Line.4: primary action, label depends on the selected source ──────────
  const primaryActionLabel = useMemo(() => {
    if (selectedSource === "builtin") return "重新解包";
    if (selectedSource === "local_directory" || selectedSource === "local_json") return "导入...";
    return "下载";
  }, [selectedSource]);

  const handlePrimaryAction = () => {
    const source = selectedSource;
    const label = primaryActionLabel;
    void run(label === "导入..." ? "正在导入" : label === "下载" ? "正在下载" : "正在解包", async () => {
      // 内建 -> repack
      if (source === "builtin") {
        if (route === "item_icon") {
          const result = await ensureBuiltinItemIconsExtractedNative(true);
          return `已重新解包内建物品图标（${result.extracted} 个文件）`;
        }
        const result = await ensureBuiltinBlockIconsExtractedNative(true);
        return `已重新解包内建方块图标（${result.extracted} 个文件）`;
      }

      // 本地 -> import
      if (source === "local_directory") {
        const picked = await openDialog({ directory: true });
        const dir = typeof picked === "string" ? picked : null;
        if (!dir) return "已取消导入";
        const slot: BlockIconResourceSlot = route === "block_icon" ? blockIconSlot : "layering";
        await registerExternalIconDirectory(route as "block_icon" | "item_icon", dir, slot);
        invalidateBlockIconCache();
        return `已导入目录 ${dir}`;
      }
      if (source === "local_json") {
        if (route === "language") {
          const picked = await openDialog({ filters: [{ name: "Language JSON", extensions: ["json"] }] });
          const path = typeof picked === "string" ? picked : null;
          if (!path) return "已取消导入";
          await importLanguageResource(path);
          invalidateI18nCache();
          return `已导入语言文件 ${path}`;
        }
        if (route === "game_data") {
          const picked = await openDialog({ filters: [{ name: "BlockState JSON", extensions: ["json"] }] });
          const path = typeof picked === "string" ? picked : null;
          if (!path) return "已取消导入";
          await importGameDataResource(path);
          invalidateBlockstateDbCache();
          return `已导入游戏数据 ${path}`;
        }
        throw new Error("该分类不支持本地 JSON 导入");
      }

      // 远端 -> download
      if (route === "block_icon" && source === "vault") {
        const result = await downloadVaultBlockIconsFromVault();
        return `已下载 CCVault 方块图标 ${result.downloaded}/${result.total}`;
      }
      if (route === "item_icon" && source === "vault") {
        const result = await downloadVaultItemIconsFromVault();
        return `已下载 CCVault 物品图标 ${result.downloaded}/${result.total}`;
      }
      if (route === "block_icon" && (source === "wiki" || source === "wiki_zh")) {
        const result = await downloadWikiBlockIconsFromMinecraftWiki(blockIconSlot, source);
        return `已下载 Wiki 方块图标 ${result.downloaded}/${result.total}`;
      }
      if (route === "enum_catalog" && source === "wiki") {
        const result = await downloadWikiEnumCatalogsFromMinecraftWiki();
        return `已下载枚举全集：方块 ${result.blocks}、物品 ${result.items}、魔咒 ${result.enchantments}、实体 ${result.entities}`;
      }
      if (route === "language" && source === "github") {
        if (!selectedLanguageBranch || !selectedRemoteLanguage) {
          throw new Error("请先选择版本与语言");
        }
        await downloadRemoteMinecraftLanguage(selectedLanguageBranch, selectedRemoteLanguage);
        invalidateI18nCache();
        return `已下载语言 ${selectedLanguageBranch}/${selectedRemoteLanguage}`;
      }
      throw new Error(`来源「${source}」不支持该分类`);
    });
  };

  const handleResetRemoteCatalog = () => {
    void run("正在刷新远端列表", async () => {
      const branches = await listRemoteMinecraftLanguageBranches();
      setLanguageBranches(branches);
      setSelectedLanguageBranch("");
      setRemoteLanguages([]);
      setSelectedRemoteLanguage("");
      return `已获取远端版本 ${branches.length} 个`;
    });
  };

  const handleBranchChange = (branch: string) => {
    setSelectedLanguageBranch(branch);
    setSelectedRemoteLanguage("");
    void run(`正在获取版本 ${branch} 的语言`, async () => {
      const languages = await listRemoteMinecraftLanguages(branch);
      setRemoteLanguages(languages);
      return `版本 ${branch} 有 ${languages.length} 种语言`;
    });
  };

  // ── Line.5: 操作 buttons ──────────────────────────────────────────────────
  const handleApply = (entry: GameResourceEntry, slot?: BlockIconResourceSlot) => {
    void run(`正在应用「${displayName(route, entry)}」`, async () => {
      await activateGameResource(entry.kind, entry.id, slot);
      await invalidateFor(entry.kind);
      return `已应用「${displayName(route, entry)}」`;
    });
  };

  const handleDelete = (entry: GameResourceEntry) => {
    void run("正在删除登记", async () => {
      await deleteGameResource(entry.kind, entry.id);
      await invalidateFor(entry.kind);
      return `已删除登记「${displayName(route, entry)}」`;
    });
  };

  const handleHealth = () => {
    void run("正在进行健康检查", async () => {
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

  // ── Line.3: sub-option dropdowns ──────────────────────────────────────────
  const renderSubOptionControls = () => {
    if (route === "language" && selectedSource === "github") {
      return (
        <>
          <select
            className="input asset-manager-select"
            aria-label="子选项1：版本"
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
            aria-label="子选项2：语言"
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
    // Spec: sub-options are placeholders for every other category.
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

  // ── Line.5: table columns per spec ────────────────────────────────────────
  const tableColumns = useMemo(() => {
    const columns: { key: string; label: string }[] = [{ key: "name", label: "名称" }];
    columns.push({ key: "source", label: "来源" });
    if (route === "language") columns.push({ key: "language", label: "语言" });
    columns.push({ key: "version", label: "版本" });
    if (route === "enum_catalog") columns.push({ key: "installed", label: "日期" });
    if (route === "block_icon") columns.push({ key: "usage", label: "用于" });
    if (route !== "block_icon") columns.push({ key: "active", label: "状态" });
    columns.push({ key: "actions", label: "操作" });
    return columns;
  }, [route]);

  const renderCell = (entry: GameResourceEntry, key: string) => {
    switch (key) {
      case "name":
        return displayName(route, entry);
      case "source":
        return SOURCE_TAG[entry.source] || entry.source;
      case "language":
        return entry.language || "--";
      case "version":
        return versionOf(entry);
      case "installed":
        return entry.source === "builtin" ? "2025/4/22" : formatDate(entry.installed_at);
      case "usage": {
        // Spec: 材料 / 分层 / 材料/分层, from the slots this entry currently serves.
        const slots: string[] = [];
        if (entry.active_material_list) slots.push("材料");
        if (entry.active_layering) slots.push("分层");
        return slots.length ? slots.join("/") : "--";
      }
      case "active":
        return isActiveFor(entry, blockIconSlot) ? "已激活" : "--";
      case "actions":
        return (
          <div className="game-resource-row-actions">
            {route === "block_icon" ? (
              <>
                <button
                  className="btn"
                  type="button"
                  disabled={busy}
                  onClick={() => handleApply(entry, "material_list")}
                >
                  用于材料
                </button>
                <button
                  className="btn"
                  type="button"
                  disabled={busy}
                  onClick={() => handleApply(entry, "layering")}
                >
                  用于分层
                </button>
              </>
            ) : (
              <button className="btn" type="button" disabled={busy} onClick={() => handleApply(entry)}>
                应用
              </button>
            )}
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
        <aside className="asset-manager-sidebar">
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
          {/* Line.1 标题与副标题 */}
          <div className="asset-manager-heading">
            <h3>{selectedNavItem.label}</h3>
            <p className="muted">{ROUTE_SUBTITLE[route]}</p>
          </div>

          <div className="asset-manager-panel">
            <div className="asset-manager-config">
              {/* Line.2 来源 */}
              <div className="asset-manager-form-row asset-manager-form-row-source">
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

              {/* Line.3 子选项1 / 子选项2 */}
              <div className="asset-manager-form-row asset-manager-form-row-suboptions">
                <span className="asset-manager-form-label">子选项</span>
                <div className="asset-manager-inline-controls">
                  {renderSubOptionControls()}
                  {route === "language" && selectedSource === "github" ? (
                    <button className="btn" type="button" disabled={busy} onClick={handleResetRemoteCatalog}>
                      刷新远端列表
                    </button>
                  ) : null}
                </div>
              </div>
            </div>

            {/* Line.4 【获取】 */}
            <div className="asset-manager-actions">
              <button className="btn" type="button" disabled={busy} onClick={handlePrimaryAction}>
                {primaryActionLabel}
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

            {/* Line.5 内容表格 */}
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
          </div>

          {/* 通用页脚 */}
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
            </div>
          </footer>
        </section>
      </div>
    </main>
  );
}
