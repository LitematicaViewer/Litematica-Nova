import React from "react";
import { ContextMenu } from "radix-ui";
import { Box, ChevronDown, ChevronRight, FolderPlus, LayoutGrid, Star, Trash2, Clock, Folder } from "lucide-react";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Button } from "../../../../components/ui/button";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { categoryPath, type LibraryCategory, type LibraryState } from "../../../../../src/business/facade";
import { CategoryIcon } from "./categoryIcon";
import type { LibraryEntryView } from "./libraryEntryView";

export interface LibraryRecentItem {
  key: string;
  label: string;
  entryId?: string;
  path?: string;
}

const SORTS = (a: LibraryCategory, b: LibraryCategory) => a.sort_index - b.sort_index;

/**
 * Left-hand navigation (D.8): 全库（根）/ 未分类 / 分类树 / 收藏夹 / 最近使用.
 *
 * 「全库」is the root that aggregates every entry; 「未分类」filters `categoryIds`
 * empty (design 2.4.5). Favorites and recent are two independent entries (5.8.1.7c).
 * Category rows are structural navigation, not action buttons (`themeSlot="library-category"`).
 */
export function LibraryCategoryTree({
  state,
  totalCount,
  untaggedCount,
  busy,
  categoryFilter,
  untaggedOnly,
  favoritesOnly,
  recentOnly,
  expandedCategories,
  dropTarget,
  hasDrag,
  favorites,
  recent,
  categoryEntryCount,
  onToggleExpanded,
  onSelectAll,
  onSelectUntagged,
  onSelectCategory,
  onSelectFavorites,
  onSelectRecent,
  onSelectRecentEntry,
  onSelectRecentPath,
  onInspect,
  onDragOverCategory,
  onDragLeaveCategory,
  onDropCategory,
  onOpenCategoryDialog,
  onOpenIconDialog,
  onOpenTrash,
  trashCount,
}: {
  state: LibraryState;
  totalCount: number;
  untaggedCount: number;
  busy: boolean;
  categoryFilter: string;
  untaggedOnly: boolean;
  favoritesOnly: boolean;
  recentOnly: boolean;
  expandedCategories: string[];
  dropTarget: string;
  hasDrag: boolean;
  favorites: LibraryEntryView[];
  recent: LibraryRecentItem[];
  categoryEntryCount: (categoryId: string) => number;
  onToggleExpanded: (id: string) => void;
  onSelectAll: () => void;
  onSelectUntagged: () => void;
  onSelectCategory: (id: string) => void;
  onSelectFavorites: () => void;
  onSelectRecent: () => void;
  onSelectRecentEntry: (entryId: string) => void;
  onSelectRecentPath: (path: string) => void;
  onInspect: (entry: LibraryEntryView) => void;
  onDragOverCategory: (id: string, event: React.DragEvent) => void;
  onDragLeaveCategory: () => void;
  onDropCategory: (id: string, event: React.DragEvent) => void;
  onOpenCategoryDialog: (kind: "create" | "rename" | "move" | "delete", category?: LibraryCategory) => void;
  onOpenIconDialog: (categoryId: string) => void;
  onOpenTrash: () => void;
  trashCount: number;
}) {
  const categories = state.categories || [];
  const dropHandlers = (id: string) => ({
    onDragOver: (event: React.DragEvent) => onDragOverCategory(id, event),
    onDragLeave: onDragLeaveCategory,
    onDrop: (event: React.DragEvent) => onDropCategory(id, event),
  });

  const renderCategoryTree = (parentId: string | null, depth = 1): React.ReactNode => (
    <ul className="library-category-tree" role={parentId ? "group" : "tree"} aria-label={parentId ? undefined : "分类树"}>
      {categories.filter((entry) => entry.parentId === parentId).sort(SORTS).map((category) => {
        const hasChildren = categories.some((item) => item.parentId === category.id);
        const expanded = expandedCategories.includes(category.id);
        const count = categoryEntryCount(category.id);
        return (
          <li key={category.id} role="none">
            <ContextMenu.Root>
              <ContextMenu.Trigger asChild>
                <div className="library-category-row">
                  {hasChildren
                    ? <Button className="library-category-expander" variant="ghost" size="icon-sm" themeSlot="library-category-expander" disabled={busy} aria-label={`${expanded ? "收起" : "展开"} ${category.name}`} onClick={() => onToggleExpanded(category.id)}>{expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</Button>
                    : <span className="library-category-expander-spacer" />}
                  <Button variant="ghost" className="library-category" themeSlot="library-category" role="treeitem" aria-level={depth} aria-expanded={hasChildren ? expanded : undefined} aria-selected={categoryFilter === category.id} aria-pressed={categoryFilter === category.id} data-category-tree-node={category.id} disabled={busy}
                    data-drop-target={dropTarget === category.id || undefined} onClick={() => onSelectCategory(category.id)} onKeyDown={(event) => {
                      const treeNodes = () => Array.from(event.currentTarget.closest('[role="tree"]')?.querySelectorAll<HTMLButtonElement>('[data-category-tree-node]') || []);
                      const focusCategory = (id?: string | null) => treeNodes().find((node) => node.dataset.categoryTreeNode === id)?.focus();
                      if (event.key === "ArrowRight" && hasChildren) { event.preventDefault(); if (!expanded) onToggleExpanded(category.id); else focusCategory(categories.filter((item) => item.parentId === category.id).sort(SORTS)[0]?.id); }
                      else if (event.key === "ArrowLeft") { event.preventDefault(); if (hasChildren && expanded) onToggleExpanded(category.id); else focusCategory(category.parentId); }
                      else if (["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) { event.preventDefault(); const nodes = treeNodes(); const index = nodes.indexOf(event.currentTarget); nodes[event.key === "Home" ? 0 : event.key === "End" ? nodes.length - 1 : Math.max(0, Math.min(nodes.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)))]?.focus(); }
                    }} {...dropHandlers(category.id)}>
                    <CategoryIcon icon={category.icon} />
                    <span title={categoryPath(state, category.id)}>{category.name}</span>
                    <small>{count}</small>
                  </Button>
                </div>
              </ContextMenu.Trigger>
              <ContextMenu.Portal>
                <ContextMenu.Content className="library-category-menu" collisionPadding={8}>
                  <ContextMenu.Item className="library-category-menu-item" disabled={busy} onSelect={() => onOpenCategoryDialog("create", category)}>新建子分类</ContextMenu.Item>
                  <ContextMenu.Item className="library-category-menu-item" disabled={busy} onSelect={() => onOpenCategoryDialog("rename", category)}>重命名</ContextMenu.Item>
                  <ContextMenu.Item className="library-category-menu-item" disabled={busy} onSelect={() => onOpenCategoryDialog("move", category)}>移动分类…</ContextMenu.Item>
                  <ContextMenu.Item className="library-category-menu-item" disabled={busy} onSelect={() => onOpenIconDialog(category.id)}>更换图标…</ContextMenu.Item>
                  <ContextMenu.Separator className="library-category-menu-separator" />
                  <ContextMenu.Item className="library-category-menu-item" disabled={busy} onSelect={() => onOpenCategoryDialog("delete", category)}>删除分类…</ContextMenu.Item>
                </ContextMenu.Content>
              </ContextMenu.Portal>
            </ContextMenu.Root>
            {hasChildren && expanded && renderCategoryTree(category.id, depth + 1)}
          </li>
        );
      })}
    </ul>
  );

  return (
    <aside className="library-categories" aria-label="投影分类">
      <div className="library-pane-heading"><h3>我的投影</h3><span>{totalCount}</span></div>
      <div className="library-category-tools"><Button size="sm" variant="ghost" disabled={busy} onClick={() => onOpenCategoryDialog("create")}><ThemeIcon name="folder_new" size={14} fallback={FolderPlus} />新建分类</Button></div>
      <ScrollArea className="library-category-scroll">
        <nav className="library-category-list" aria-label="分类筛选">
          <Button variant="ghost" className="library-category" themeSlot="library-category" aria-pressed={!categoryFilter && !untaggedOnly && !favoritesOnly && !recentOnly} onClick={onSelectAll}><ThemeIcon name="all" size={16} fallback={LayoutGrid} /><span>全库</span><small>{totalCount}</small></Button>
          <Button variant="ghost" className="library-category" themeSlot="library-category" aria-pressed={untaggedOnly} data-drop-target={dropTarget === "untagged" || undefined} onClick={onSelectUntagged} {...dropHandlers("")}><ThemeIcon name="unknown" size={16} fallback={Box} /><span>未分类</span><small>{untaggedCount}</small></Button>
          {renderCategoryTree(null)}

          <Button variant="ghost" className="library-category library-tree-entry" themeSlot="library-category" aria-pressed={favoritesOnly} onClick={onSelectFavorites}><Star size={16} aria-hidden="true" /><span>收藏夹</span><small>{favorites.length}</small></Button>
          {favoritesOnly && (favorites.length
            ? <ul className="library-tree-sublist">{favorites.map((entry) => <li key={entry.entryId}><Button variant="ghost" size="sm" className="library-tree-item" onClick={() => onInspect(entry)}>{entry.displayName || entry.fileName}</Button></li>)}</ul>
            : <p className="library-tree-empty muted">还没有收藏。</p>)}

          <Button variant="ghost" className="library-category library-tree-entry" themeSlot="library-category" aria-pressed={recentOnly} onClick={onSelectRecent}><Clock size={16} aria-hidden="true" /><span>最近使用</span><small>{recent.length}</small></Button>
          {recentOnly && (recent.length
            ? <ul className="library-tree-sublist">{recent.map((item) => <li key={item.key}><Button variant="ghost" size="sm" className="library-tree-item" onClick={() => { if (item.entryId) onSelectRecentEntry(item.entryId); else if (item.path) onSelectRecentPath(item.path); }}>{item.label}</Button></li>)}</ul>
            : <p className="library-tree-empty muted">还没有最近使用记录。</p>)}
          {hasDrag && dropTarget === "untagged" && <span className="library-drop-hint muted">松开可移入未分类</span>}
        </nav>
      </ScrollArea>
      <Button variant="ghost" className="library-category library-trash-button" themeSlot="library-category" aria-haspopup="dialog" onClick={onOpenTrash}><ThemeIcon name="delete" size={16} fallback={Trash2} /><span>回收站</span><small>{trashCount}</small></Button>
      <span className="library-tree-legend muted"><Folder size={12} aria-hidden="true" />右键分类可管理</span>
    </aside>
  );
}