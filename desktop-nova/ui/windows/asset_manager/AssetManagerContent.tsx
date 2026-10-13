import { useCallback, useEffect, useMemo, useState } from "react";

import {
  activateGameResource,
  checkGameResourceHealth,
  cleanupLocalTempFiles,
  deleteGameResource,
  downloadVaultBlockIconsFromVault,
  downloadVaultItemIconsFromVault,
  downloadWikiBlockIconsFromMinecraftWiki,
  downloadWikiEnumCatalogsFromMinecraftWiki,
  ensureBuiltinBlockIconsExtractedNative,
  ensureBuiltinItemIconsExtractedNative,
  getPathInfo,
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
  pickLitematicFile,
  registerExternalIconDirectory,
  resolveBlockIconVariant,
  supportsBlockIconLayering,
  type BlockIconSlot,
  type GameResourceEntry,
  type GameResourceHealthRow,
  type GameResourceKind,
  type GameResourceSnapshot,
} from "../../../src/business/facade";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../components/ui/table";
import { Button } from "../../components/ui/button";
import { Dropdown } from "../../components/Dropdown";

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
}

const ASSET_NAV_ITEMS: AssetNavItem[] = [
  { key: "block_icon", label: "方块图标" },
  { key: "item_icon", label: "物品图标" },
  { key: "language", label: "语言" },
  { key: "game_data", label: "游戏数据" },
  { key: "enum_catalog", label: "枚举全集" },
];

const DEFAULT_SOURCE_BY_ROUTE: Record<AssetRoute, AssetSource> = {
  block_icon: "builtin",
  item_icon: "builtin",
  language: "builtin",
  game_data: "builtin",
  enum_catalog: "builtin",
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

/** Source dropdown value -> the option object, for calls that need the label too. */
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

/** Whether this entry currently serves the given block-icon slot. */
function entryActiveFor(route: AssetRoute, entry: GameResourceEntry, slot: BlockIconResourceSlot): boolean {
  if (route === "block_icon") {
    return slot === "layering" ? !!entry.active_layering : !!entry.active_material_list;
  }
  if (route === "item_icon") return !!entry.active_layering;
  return !!entry.active;
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
  if (entry.kind !== "block_icon") return "--";
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
 * Reads and writes the resource registry restored in L1; every activation also
 * drops the matching runtime cache so the change takes effect without a restart.
 */
export function AssetManagerContent({ onClose }: { onClose?: () => void }) {
  const [route, setRoute] = useState<AssetRoute>("block_icon");
  const [blockIconSlot, setBlockIconSlot] = useState<BlockIconResourceSlot>("material_list");
  const [sourceByRoute, setSourceByRoute] = useState(DEFAULT_SOURCE_BY_ROUTE);
  const [snapshot, setSnapshot] = useState<GameResourceSnapshot | null>(null);
  const [health, setHealth] = useState<GameResourceHealthRow[]>([]);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [remoteBranches, setRemoteBranches] = useState<string[]>([]);
  const [remoteLanguages, setRemoteLanguages] = useState<string[]>([]);
  const [remoteBranch, setRemoteBranch] = useState("");

  const refresh = useCallback(async () => {
    try {
      setSnapshot(await listGameResourceRegistry());
    } catch (error: any) {
      setStatus(`加载资源列表失败：${String(error?.message || error)}`);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

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

  /** Drops the runtime caches that the activated resource feeds. */
  const invalidateFor = useCallback(async (kind: AssetRoute) => {
    if (kind === "block_icon" || kind === "item_icon") {
      invalidateBlockIconCache();
      return;
    }
    if (kind === "language") {
      invalidateI18nCache();
      return;
    }
    if (kind === "game_data") {
      invalidateBlockstateDbCache();
    }
  }, []);

  const run = useCallback(
    async (label: string, action: () => Promise<string>) => {
      setBusy(true);
      setStatus(`${label}…`);
      try {
        setStatus(await action());
      } catch (error: any) {
        setStatus(`${label}失败：${String(error?.message || error)}`);
      } finally {
        setBusy(false);
        await refresh();
      }
    },
    [refresh],
  );

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

  const handleExtractBuiltin = () => {
    const slot = route;
    void run("正在解包内建图标", async () => {
      const result =
        slot === "item_icon"
          ? await ensureBuiltinItemIconsExtractedNative(true)
          : await ensureBuiltinBlockIconsExtractedNative(true);
      await refresh();
      return `内建图标已解包到 ${result.target_dir}（${result.extracted} 个文件）`;
    });
  };

  const handleDownload = () => {
    const source = sourceByRoute[route];
    void run(`正在从 ${sourceLabel(route, source)} 获取`, async () => {
      if (route === "block_icon" && source === "vault") {
        const result = await downloadVaultBlockIconsFromVault();
        return `已下载方块图标 ${result.downloaded}/${result.total} 到 ${result.target_dir}`;
      }
      if (route === "item_icon" && source === "vault") {
        const result = await downloadVaultItemIconsFromVault();
        return `已下载物品图标 ${result.downloaded}/${result.total} 到 ${result.target_dir}`;
      }
      if (route === "block_icon" && (source === "wiki" || source === "wiki_zh")) {
        const result = await downloadWikiBlockIconsFromMinecraftWiki(blockIconSlot, source);
        return `已从 Wiki 下载方块图标 ${result.downloaded}/${result.total} 到 ${result.target_dir}`;
      }
      if (route === "enum_catalog" && source === "wiki") {
        const result = await downloadWikiEnumCatalogsFromMinecraftWiki();
        return `已下载枚举全集：方块 ${result.blocks}、物品 ${result.items}、魔咒 ${result.enchantments}、实体 ${result.entities}`;
      }
      throw new Error(`来源「${sourceLabel(route, source)}」不支持自动获取`);
    });
  };

  const handleImport = () => {
    const source = sourceByRoute[route];
    void run("正在导入", async () => {
      if (route === "language" && source === "local_json") {
        const picked = await openDialog({
          multiple: false,
          filters: [{ name: "Language JSON", extensions: ["json"] }],
        });
        const path = Array.isArray(picked) ? picked[0] : picked;
        if (!path) return "已取消导入";
        await importLanguageResource(path);
        invalidateI18nCache();
        return `已导入语言包 ${path}`;
      }
      if (route === "game_data" && source === "local_json") {
        const dbPicked = await openDialog({
          multiple: false,
          filters: [{ name: "BlockState JSON", extensions: ["json"] }],
        });
        const dbPath = Array.isArray(dbPicked) ? dbPicked[0] : dbPicked;
        if (!dbPath) return "已取消导入";
        const i18nPicked = await openDialog({
          multiple: false,
          filters: [{ name: "BlockState i18n JSON", extensions: ["json"] }],
        });
        const i18nPath = Array.isArray(i18nPicked) ? i18nPicked[0] : i18nPicked || undefined;
        await importGameDataResource(dbPath, i18nPath);
        invalidateBlockstateDbCache();
        return `已导入游戏数据 ${dbPath}`;
      }
      if ((route === "block_icon" || route === "item_icon") && source === "local_directory") {
        const picked = await openDialog({ directory: true, multiple: false });
        const dir = Array.isArray(picked) ? picked[0] : picked;
        if (!dir) return "已取消导入";
        // Item icons live in the layering slot; block icons follow the selected slot.
        const slot: BlockIconResourceSlot = route === "block_icon" ? blockIconSlot : "layering";
        await registerExternalIconDirectory(route, dir, slot);
        invalidateBlockIconCache();
        return `已登记外部图标目录 ${dir}`;
      }
      throw new Error(`来源「${sourceLabel(route, source)}」不支持本地导入`);
    });
  };

  const handleRefreshRemote = () => {
    void run("正在获取远端语言分支", async () => {
      const branches = await listRemoteMinecraftLanguageBranches();
      setRemoteBranches(branches);
      return `远端分支 ${branches.length} 个`;
    });
  };

  const handleLoadLanguages = (branch: string) => {
    setRemoteBranch(branch);
    void run(`正在获取分支 ${branch} 的语言`, async () => {
      const languages = await listRemoteMinecraftLanguages(branch);
      setRemoteLanguages(languages);
      return `分支 ${branch} 有 ${languages.length} 种语言`;
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

  const handlePathInfo = () => {
    void run("正在检查路径", async () => {
      const info = await getPathInfo(snapshot?.config_dir || "");
      return `配置目录：${info.normalized}（存在=${info.exists}）`;
    });
  };

  const source = sourceByRoute[route];
  const managedSource =
    source !== "builtin" &&
    ((route === "block_icon" && (source === "local_directory" || source === "vault" || source === "wiki" || source === "wiki_zh")) ||
      (route === "item_icon" && (source === "local_directory" || source === "vault")) ||
      (route === "language" && (source === "local_json" || source === "github")) ||
      (route === "game_data" && source === "local_json") ||
      (route === "enum_catalog" && source === "wiki"));

  return (
    <main className="asset-manager-window">
      <header className="asset-manager-header">
        <h1>游戏资源管理</h1>
        {onClose ? (
          <Button className="btn" onClick={onClose}>
            关闭窗口
          </Button>
        ) : null}
      </header>

      <nav className="asset-manager-nav">
        {ASSET_NAV_ITEMS.map((item) => (
          <Button
            key={item.key}
            className={item.key === route ? "btn asset-manager-nav-item active" : "btn asset-manager-nav-item"}
            onClick={() => setRoute(item.key)}
          >
            {item.label}
          </Button>
        ))}
      </nav>

      {route === "block_icon" ? (
        <div className="asset-manager-slot-row">
          <span>激活槽位</span>
          <Dropdown
            value={blockIconSlot}
            options={[
              { value: "material_list", label: "材料列表" },
              { value: "layering", label: "分层画布" },
            ]}
            onChange={(value) => setBlockIconSlot(value as BlockIconResourceSlot)}
            ariaLabel="方块图标激活槽位"
          />
          <span className="asset-manager-hint">3D 图标不能用于分层槽，切换来源时会自动限制。</span>
        </div>
      ) : null}

      <div className="asset-manager-source-row">
        <span>来源</span>
        <Dropdown
          value={source}
          options={SOURCE_OPTIONS_BY_ROUTE[route]}
          onChange={(value) => setSourceByRoute((current) => ({ ...current, [route]: value as AssetSource }))}
          ariaLabel="资源来源"
        />
        {source === "builtin" ? (
          <Button className="btn" onClick={handleExtractBuiltin} disabled={busy}>
            解包内建图标
          </Button>
        ) : null}
        {managedSource ? (
          <>
            <Button className="btn" onClick={handleDownload} disabled={busy}>
              获取
            </Button>
            <Button className="btn" onClick={handleImport} disabled={busy}>
              导入
            </Button>
          </>
        ) : null}
        {route === "language" && source === "github" ? (
          <>
            <Button className="btn" onClick={handleRefreshRemote} disabled={busy}>
              刷新远端列表
            </Button>
            <Dropdown
              value={remoteBranch}
              options={remoteBranches.map((branch) => ({ value: branch, label: branch }))}
              onChange={handleLoadLanguages}
              ariaLabel="远端语言分支"
            />
            <Dropdown
              value=""
              options={remoteLanguages.map((language) => ({ value: language, label: language }))}
              onChange={(language) => {
                void run(`正在下载语言 ${language}`, async () => {
                  const { downloadRemoteMinecraftLanguage } = await import("../../../src/business/facade");
                  await downloadRemoteMinecraftLanguage(remoteBranch, language);
                  invalidateI18nCache();
                  return `已下载语言 ${language}`;
                });
              }}
              ariaLabel="远端语言"
            />
          </>
        ) : null}
        <Button className="btn" onClick={() => void refresh()} disabled={busy}>
          刷新
        </Button>
        <Button className="btn" onClick={handleHealth} disabled={busy}>
          健康检查
        </Button>
        <Button className="btn" onClick={handleOpenRoot} disabled={busy}>
          打开根目录
        </Button>
        <Button className="btn" onClick={handlePathInfo} disabled={busy}>
          路径信息
        </Button>
      </div>

      <div className="asset-manager-status" role="status">
        {status || "选择来源后可激活、获取或导入资源；内建资源始终作为离线回退。"}
      </div>

      <div className="asset-manager-table-wrap">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>名称</TableHead>
              <TableHead>来源</TableHead>
              <TableHead>版本</TableHead>
              {route === "block_icon" ? <TableHead>用于</TableHead> : null}
              <TableHead>激活</TableHead>
              <TableHead>日期</TableHead>
              <TableHead>操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {entries.length === 0 ? (
              <TableRow>
                <TableCell colSpan={route === "block_icon" ? 7 : 6}>暂无登记项</TableCell>
              </TableRow>
            ) : (
              entries.map((entry) => (
                <TableRow key={entry.id}>
                  <TableCell title={entry.id}>{entry.label}</TableCell>
                  <TableCell>{sourceTag(entry)}</TableCell>
                  <TableCell>{versionOf(route, entry)}</TableCell>
                  {route === "block_icon" ? <TableCell>{usageLabel(entry)}</TableCell> : null}
                  <TableCell>{activationLabel(route, entry)}</TableCell>
                  <TableCell>{formatDate(entry.installed_at)}</TableCell>
                  <TableCell>
                    <Button
                      className="btn"
                      onClick={() => handleActivate(entry)}
                      disabled={busy || entry.id === activeEntryId}
                    >
                      {entry.id === activeEntryId ? "当前" : "激活"}
                    </Button>
                    {entry.builtin ? null : (
                      <Button className="btn" onClick={() => handleDelete(entry)} disabled={busy}>
                        删除登记
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>

      {health.length ? (
        <section className="asset-manager-health">
          <h2>健康检查</h2>
          <ul>
            {health.map((row) => (
              <li key={row.label} className={row.ok ? "ok" : "error"}>
                {row.ok ? "✓" : "✗"} {row.label}：{row.detail}
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      <footer className="asset-manager-footer">
        <Button className="btn" onClick={() => void cleanupLocalTempFiles()} disabled={busy}>
          清理本地临时文件
        </Button>
        <span className="asset-manager-hint">
          切换资源后会自动失效对应缓存（图标 / 语言 / BlockState），无需重启。
        </span>
      </footer>
    </main>
  );
}
