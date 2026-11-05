import { t } from "../../../../../src/services/i18n";
import { Download, FileText, FolderOpen, FolderPlus, LayoutGrid, List, MoreHorizontal, RefreshCw, Search, X } from "lucide-react";
import { Popover } from "radix-ui";
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
  onNewComposition,
  onImportClipboard,
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
  onNewComposition: () => void;
  onImportClipboard?: () => void;
}) {
  return (
    <header className="library-toolbar">
      <h2>{t("library.libraryTitle")}</h2>
      <Button variant="default" onClick={onImport} disabled={busy} aria-busy={isOpening}><ThemeIcon name="import" size={16} fallback={FolderPlus} />{isOpening ? t("library.importing") : t("library.importSchematic")}</Button>
      <Button onClick={onNewComposition}><FolderPlus size={16} aria-hidden="true" />{t("library.ux.newComposition")}</Button>
      <div className="library-toolbar-second-row">
      <div className="library-management library-download-link"><Button onClick={onDownload}><ThemeIcon name="download" size={16} fallback={Download} />{t("library.downloadSchematic")}</Button></div>
      <div className="library-search">
        <ThemeIcon name="search" size={16} fallback={Search} />
        <Input type="search" aria-label={t("library.searchSchematics")} placeholder={t("library.ux.searchLibrary")} value={search} onChange={(event) => onSearchChange(event.target.value)} />
        {search && <Button variant="ghost" size="icon-sm" aria-label={t("library.ux.clearSearch")} onClick={() => onSearchChange("")}><ThemeIcon name="close" size={14} fallback={X} /></Button>}
      </div>
        <span className="library-search-status" role="status" data-index-ready={indexReady || undefined}>
          {search && <>
          {searching ? t("library.ux.searching") : indexReady ? t("library.ux.searchCount", { value0: searchMatches }) : t("library.ux.indexProgress", { value0: searchMatches })}
          </>}
        </span>
      <span className="library-view-switch" role="group" aria-label={t("library.ux.views")}>
        <Button size="icon" variant="ghost" aria-pressed={view === "grid"} aria-label={t("library.ux.grid")} title={t("library.ux.grid")} disabled={busy} onClick={() => onViewChange("grid")}><LayoutGrid size={16} aria-hidden="true" /></Button>
        <Button size="icon" variant="ghost" aria-pressed={view === "list"} aria-label={t("library.ux.list")} title={t("library.ux.list")} disabled={busy} onClick={() => onViewChange("list")}><List size={16} aria-hidden="true" /></Button>
        <Button size="icon" variant="ghost" aria-pressed={view === "detail"} aria-label={t("library.ux.detailView")} title={t("library.ux.detailView")} disabled={busy} onClick={() => onViewChange("detail")}><FileText size={16} aria-hidden="true" /></Button>
      </span>
      <div className="library-filter-control"><Dropdown ariaLabel={t("library.fileStatusFilter")} value={statusFilter} onChange={onStatusFilterChange} options={[{ value: "", label: t("stockpile.filter.all") }, { value: "ok", label: t("library.statusOk") }, { value: "missing", label: t("library.fileMissing") }, { value: "parse_error", label: t("library.parseFailed") }]} /></div>
      <div className="library-filter-control"><Dropdown ariaLabel={t("library.schematicSort")} value={sortMode} onChange={onSortModeChange} options={[{ value: "manual", label: t("library.manualSort") }, { value: "recent_analyzed", label: t("library.lastAnalyzed") }, { value: "blocks", label: t("properties.blocks") }, { value: "name", label: t("library.nameSort") }]} /></div>
      <Popover.Root><Popover.Trigger asChild><Button variant="ghost" aria-label={t("library.ux.management")}><MoreHorizontal size={16} aria-hidden="true" />{t("library.ux.more")}</Button></Popover.Trigger><Popover.Portal><Popover.Content className="library-management-popover" align="end" sideOffset={4} collisionPadding={8} onKeyDown={event => event.stopPropagation()}>
      <Button variant="ghost" onClick={onImportClipboard}>{t("library.ux.clipboard")}</Button>
      {view !== "detail" && (
        <>
          <div className="library-filter-control"><Dropdown ariaLabel={t("library.ux.groupBy")} value={groupDimension} onChange={(value) => onGroupDimensionChange(value as GroupingDimension)} options={[{ value: "none", label: t("library.ux.noGrouping") }, { value: "author", label: t("library.ux.byAuthor") }, { value: "version", label: t("library.ux.byVersion") }, { value: "size", label: t("library.ux.bySize") }]} /></div>
          <div className="library-filter-control"><Dropdown ariaLabel={t("library.ux.groupStyle")} value={groupMode} disabled={groupDimension === "none"} onChange={(value) => onGroupModeChange(value as GroupingMode)} options={[{ value: "group", label: t("library.ux.groups") }, { value: "stack", label: t("library.ux.stacks") }]} /></div>
        </>
      )}
      <Button variant="ghost" onClick={onManageFolders}><ThemeIcon name="explorer" size={16} fallback={FolderOpen} />{t("library.manageFolders")}</Button>
      <Button variant="ghost" onClick={onExport} disabled={busy}><ThemeIcon name="download" size={16} fallback={Download} />{t("library.ux.exportZip")}</Button>
      <Button variant="ghost" onClick={onOpenTaskCenter} aria-haspopup="dialog">{t("library.ux.taskCenter")}</Button>
      <Button variant="ghost" onClick={() => onRescanAll(false)} disabled={busy}><ThemeIcon name="refresh" size={16} fallback={RefreshCw} />{t("library.ux.rescan")}</Button>
      <Button variant="ghost" onClick={() => onRescanAll(true)} disabled={busy} title={t("library.ux.forceHint")}><ThemeIcon name="refresh" size={16} fallback={RefreshCw} />{t("library.ux.forceRecheck")}</Button>
      <Button variant="ghost" aria-label={isRefreshing ? t("library.libraryRefreshing") : t("library.refreshLibrary")} title={t("library.refreshLibrary")} disabled={busy} aria-busy={isRefreshing} onClick={onRefresh}><ThemeIcon name="refresh" size={16} fallback={RefreshCw} />{t("library.refreshLibrary")}</Button>
      </Popover.Content></Popover.Portal></Popover.Root>
      </div>
    </header>
  );
}
