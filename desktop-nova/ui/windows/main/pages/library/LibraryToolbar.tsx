import { Download, FileText, FolderOpen, FolderPlus, LayoutGrid, List, RefreshCw, Search, X } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Dropdown } from "../../../../components/Dropdown";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import type { GroupingDimension, GroupingMode } from "./libraryEntryView";

export type LibraryView = "grid" | "list" | "detail";

/**
 * Library toolbar (D.4): title, import/download, index-backed search, status filter,
 * sort, view switch, grouping controls and the task-centre entry.
 *
 * The search field is driven by the background index (`queryLibraryIndex` +
 * `subscribeIndexStatus`); while the index is still building the container passes the
 * already-ready partial result and `indexReady=false` for the "索引构建中" hint (5.8.3.1).
 */
export function LibraryToolbar({
  busy,
  isRefreshing,
  isOpening,
  search,
  onSearchChange,
  searching,
  searchMatches,
  indexReady,
  statusFilter,
  onStatusFilterChange,
  sortMode,
  onSortModeChange,
  view,
  onViewChange,
  groupDimension,
  groupMode,
  onGroupDimensionChange,
  onGroupModeChange,
  onImport,
  onDownload,
  onManageFolders,
  onRefresh,
  onRescanAll,
  onOpenTaskCenter,
  onExport,
}: {
  busy: boolean;
  isRefreshing: boolean;
  isOpening: boolean;
  search: string;
  onSearchChange: (value: string) => void;
  searching: boolean;
  searchMatches: number;
  indexReady: boolean;
  statusFilter: string;
  onStatusFilterChange: (value: string) => void;
  sortMode: string;
  onSortModeChange: (value: string) => void;
  view: LibraryView;
  onViewChange: (value: LibraryView) => void;
  groupDimension: GroupingDimension;
  groupMode: GroupingMode;
  onGroupDimensionChange: (value: GroupingDimension) => void;
  onGroupModeChange: (value: GroupingMode) => void;
  onImport: () => void;
  onDownload: () => void;
  onManageFolders: () => void;
  onRefresh: () => void;
  onRescanAll: (forceRecheck: boolean) => void;
  onOpenTaskCenter: () => void;
  onExport: () => void;
}) {
  return (
    <header className="library-toolbar">
      <h2>投影库</h2>
      <Button variant="default" onClick={onImport} disabled={busy} aria-busy={isOpening}><ThemeIcon name="import" size={16} fallback={FolderPlus} />{isOpening ? "正在导入…" : "导入投影"}</Button>
      <Button onClick={onDownload}><ThemeIcon name="download" size={16} fallback={Download} />下载投影</Button>
      <div className="library-search">
        <ThemeIcon name="search" size={16} fallback={Search} />
        <Input type="search" aria-label="搜索投影" placeholder="搜索名称、作者、标签、分类…" value={search} onChange={(event) => onSearchChange(event.target.value)} />
        {search && <Button variant="ghost" size="icon-sm" aria-label="清除搜索" onClick={() => onSearchChange("")}><ThemeIcon name="close" size={14} fallback={X} /></Button>}
      </div>
      {search && (
        <span className="library-search-status" role="status" data-index-ready={indexReady || undefined}>
          {searching ? "搜索中…" : indexReady ? `${searchMatches} 项匹配` : `索引构建中 · 已就绪 ${searchMatches} 项`}
        </span>
      )}
      <span className="library-view-switch" role="group" aria-label="视图切换">
        <Button size="icon" variant="ghost" aria-pressed={view === "grid"} aria-label="网格视图" title="网格视图" disabled={busy} onClick={() => onViewChange("grid")}><LayoutGrid size={16} aria-hidden="true" /></Button>
        <Button size="icon" variant="ghost" aria-pressed={view === "list"} aria-label="列表视图" title="列表视图" disabled={busy} onClick={() => onViewChange("list")}><List size={16} aria-hidden="true" /></Button>
        <Button size="icon" variant="ghost" aria-pressed={view === "detail"} aria-label="详情视图" title="详情视图" disabled={busy} onClick={() => onViewChange("detail")}><FileText size={16} aria-hidden="true" /></Button>
      </span>
      <div className="library-filter-control"><Dropdown ariaLabel="筛选文件状态" value={statusFilter} onChange={onStatusFilterChange} options={[{ value: "", label: "全部状态" }, { value: "ok", label: "正常" }, { value: "missing", label: "文件丢失" }, { value: "parse_error", label: "解析失败" }]} /></div>
      <div className="library-filter-control"><Dropdown ariaLabel="投影排序" value={sortMode} onChange={onSortModeChange} options={[{ value: "manual", label: "手动排序" }, { value: "recent_analyzed", label: "最近分析" }, { value: "blocks", label: "方块数量" }, { value: "name", label: "名称 A–Z" }]} /></div>
      {view !== "detail" && (
        <>
          <div className="library-filter-control"><Dropdown ariaLabel="分组维度" value={groupDimension} onChange={(value) => onGroupDimensionChange(value as GroupingDimension)} options={[{ value: "none", label: "不分组" }, { value: "author", label: "按作者" }, { value: "version", label: "按版本" }, { value: "size", label: "按尺寸" }]} /></div>
          <div className="library-filter-control"><Dropdown ariaLabel="分组方式" value={groupMode} disabled={groupDimension === "none"} onChange={(value) => onGroupModeChange(value as GroupingMode)} options={[{ value: "group", label: "分组" }, { value: "stack", label: "堆叠" }]} /></div>
        </>
      )}
      <Button variant="ghost" onClick={onManageFolders}><ThemeIcon name="explorer" size={16} fallback={FolderOpen} />管理文件夹</Button>
      <Button variant="ghost" onClick={onExport} disabled={busy}><ThemeIcon name="download" size={16} fallback={Download} />打包导出</Button>
      <Button variant="ghost" onClick={onOpenTaskCenter} aria-haspopup="dialog">任务中心</Button>
      <Button variant="ghost" onClick={() => onRescanAll(false)} disabled={busy}><ThemeIcon name="refresh" size={16} fallback={RefreshCw} />重新扫描全部</Button>
      <Button variant="ghost" onClick={() => onRescanAll(true)} disabled={busy} title="忽略文件大小与修改时间缓存，强制重新计算校验值"><ThemeIcon name="refresh" size={16} fallback={RefreshCw} />强制重新校验</Button>
      <Button variant="ghost" size="icon" aria-label={isRefreshing ? "正在刷新投影库" : "刷新投影库"} title="刷新投影库" disabled={busy} aria-busy={isRefreshing} onClick={onRefresh}><ThemeIcon name="refresh" size={16} fallback={RefreshCw} /></Button>
    </header>
  );
}