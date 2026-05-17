import React, { useEffect, useMemo, useState } from "react";

import {
  activateGameResource,
  checkGameResourceHealth,
  deleteGameResource,
  GameResourceEntry,
  GameResourceHealthRow,
  GameResourceKind,
  GameResourceSnapshot,
  importGameDataResource,
  importLanguageResource,
  initI18n,
  invalidateBlockIconCache,
  invalidateBlockstateDbCache,
  invalidateI18nCache,
  listGameResourceRegistry,
  loadDatabases,
  openDialog,
  registerExternalIconDirectory,
} from "../../../src/business/facade";
import { NavIcon } from "../../shell/NavIcon";

type AssetRoute = "block_icon" | "item_icon" | "language" | "game_data";

interface AssetNavItem {
  key: AssetRoute;
  label: string;
  icon: string;
}

const ASSET_NAV_ITEMS: AssetNavItem[] = [
  { key: "block_icon", label: "方块图标", icon: "asset-block" },
  { key: "item_icon", label: "物品图标", icon: "asset-item" },
  { key: "language", label: "语言", icon: "asset-language" },
  { key: "game_data", label: "游戏数据", icon: "asset-db" },
];

function resourceKindTitle(kind: GameResourceKind): string {
  switch (kind) {
    case "language": return "游戏语言";
    case "block_icon": return "方块图标";
    case "item_icon": return "物品图标";
    case "game_data": return "版本化游戏数据";
  }
}

function resourceEntryPath(entry: GameResourceEntry): string {
  return entry.root_path || entry.root_relpath || entry.file_relpath || entry.data_relpath || "";
}

function resourceActiveText(snapshot: GameResourceSnapshot | null): string {
  if (!snapshot) return "未加载";
  return [
    `语言=${snapshot.active_language.label}`,
    `材料图标=${snapshot.active_material_list_icons.label}`,
    `分层方块=${snapshot.active_layering_block_icons.label}`,
    `分层物品=${snapshot.active_layering_item_icons.label}`,
    `数据=${snapshot.active_game_data.label}`,
  ].join(" / ");
}

function routeIntro(route: AssetRoute): string {
  switch (route) {
    case "block_icon": return "管理材料列表与分层视图使用的方块图标来源。";
    case "item_icon": return "管理分层视图中可用的物品图标来源。";
    case "language": return "导入并切换 Minecraft 语言 JSON。";
    case "game_data": return "导入并切换 BlockState 数据库与属性翻译。";
  }
}

function activeStatus(kind: GameResourceKind, entry: GameResourceEntry): string {
  if (kind === "language" || kind === "game_data") return entry.active ? "当前" : "";
  if (kind === "block_icon") {
    return [
      entry.active_material_list ? "材料" : "",
      entry.active_layering ? "分层" : "",
    ].filter(Boolean).join(" / ");
  }
  return entry.active_layering ? "分层" : "";
}

/**
 * Renders the game asset manager body used by the standalone asset-manager subwindow.
 */
export function AssetManagerContent({ theme, onClose }: { theme: string; onClose?: () => void }) {
  const [route, setRoute] = useState<AssetRoute>("block_icon");
  const [resourceSnapshot, setResourceSnapshot] = useState<GameResourceSnapshot | null>(null);
  const [resourceHealth, setResourceHealth] = useState<GameResourceHealthRow[]>([]);
  const [log, setLog] = useState("");

  const selectedNavItem = useMemo(() => ASSET_NAV_ITEMS.find((item) => item.key === route) || ASSET_NAV_ITEMS[0], [route]);
  const entries = resourceSnapshot?.entries[route] || [];

  const refreshGameResources = async () => {
    const snapshot = await listGameResourceRegistry();
    setResourceSnapshot(snapshot);
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
    setLog(`${message}\n${resourceActiveText(snapshot)}`);
  };

  const activateResource = async (kind: GameResourceKind, id: string, slot?: "material_list" | "layering") => {
    await applyResourceSnapshot(activateGameResource(kind, id, slot), "资源激活状态已更新。");
  };

  const deleteResource = async (kind: GameResourceKind, id: string) => {
    await applyResourceSnapshot(deleteGameResource(kind, id), "资源登记已删除。");
  };

  const importLanguage = async () => {
    const selected = await openDialog({ filters: [{ name: "Minecraft language JSON", extensions: ["json"] }] });
    if (typeof selected !== "string") return;
    await applyResourceSnapshot(importLanguageResource(selected), `语言资源已导入：${selected}`);
  };

  const importBlockIconDir = async (slot: "material_list" | "layering") => {
    const selected = await openDialog({ directory: true });
    if (typeof selected !== "string") return;
    await applyResourceSnapshot(registerExternalIconDirectory("block_icon", selected, slot), `方块图标目录已登记：${selected}`);
  };

  const importItemIconDir = async () => {
    const selected = await openDialog({ directory: true });
    if (typeof selected !== "string") return;
    await applyResourceSnapshot(registerExternalIconDirectory("item_icon", selected, "layering"), `物品图标目录已登记：${selected}`);
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
    setLog(rows.map((row) => `${row.ok ? "OK" : "失败"} ${row.label}\n${row.detail}`).join("\n\n"));
  };

  useEffect(() => {
    refreshGameResources().catch((err) => setLog(String(err)));
  }, []);

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
          <div className="asset-manager-actions">
            <button className="btn" type="button" onClick={refreshGameResources}>刷新</button>
            <button className="btn" type="button" onClick={checkResources}>健康检查</button>
            {route === "block_icon" ? (
              <>
                <button className="btn" type="button" onClick={() => importBlockIconDir("material_list")}>导入材料图标目录</button>
                <button className="btn" type="button" onClick={() => importBlockIconDir("layering")}>导入分层图标目录</button>
              </>
            ) : null}
            {route === "item_icon" ? <button className="btn" type="button" onClick={importItemIconDir}>导入物品图标目录</button> : null}
            {route === "language" ? <button className="btn" type="button" onClick={importLanguage}>导入语言 JSON</button> : null}
            {route === "game_data" ? <button className="btn" type="button" onClick={importGameData}>导入 BlockState JSON</button> : null}
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
                  <th>名称</th>
                  <th>来源</th>
                  <th>路径/版本</th>
                  <th>状态</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((entry) => (
                  <tr key={entry.id}>
                    <td>{entry.label}</td>
                    <td>{entry.source}</td>
                    <td className="game-resource-path">{entry.version || entry.language || resourceEntryPath(entry)}</td>
                    <td>{activeStatus(route, entry)}</td>
                    <td>
                      <div className="game-resource-row-actions">
                        {(route === "language" || route === "game_data") ? (
                          <button className="btn" type="button" onClick={() => activateResource(route, entry.id)}>应用</button>
                        ) : null}
                        {route === "block_icon" ? (
                          <>
                            <button className="btn" type="button" onClick={() => activateResource(route, entry.id, "material_list")}>材料</button>
                            <button className="btn" type="button" onClick={() => activateResource(route, entry.id, "layering")}>分层</button>
                          </>
                        ) : null}
                        {route === "item_icon" ? (
                          <button className="btn" type="button" onClick={() => activateResource(route, entry.id, "layering")}>分层</button>
                        ) : null}
                        {!entry.builtin ? <button className="btn" type="button" onClick={() => deleteResource(route, entry.id)}>删除登记</button> : null}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="game-resource-note">
            内建资源始终作为离线回退；导入目录目前登记外部路径，导入 JSON 会复制到用户配置目录下的 minecraft-assets。
          </div>
          {log ? <pre className="subwindow-error asset-manager-log">{log}</pre> : null}
        </section>
      </div>
    </>
  );
}
