import React, { useState } from "react";
import { getUiLocale, t, type StockpileRecipeTree } from "../../../../../src/business/facade";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Button } from "../../../../components/ui/button";

interface RecipeTreeViewProps {
  node: StockpileRecipeTree;
  iconImages?: Record<string, string>;
  itemNames?: Record<string, Record<string, string>>;
  language?: string;
  showUnresolved?: boolean;
  depth?: number;
}

/** Display only: task snapshots and the material catalog share the same tree. */
export function RecipeTreeView({ node, iconImages, itemNames, language = "auto", showUnresolved = true, depth = 0 }: RecipeTreeViewProps) {
  const [open, setOpen] = useState(depth < 1);
  const locale = language === "zh-CN" || language === "en-US" ? language : getUiLocale();
  const name = (id: string, fallback?: string) => fallback || itemNames?.[id]?.[locale] || t("editor.unknown");
  const icon = (key: string) => iconImages?.[key]
    ? <img className="block-icon-image" src={iconImages[key]} alt="" />
    : <BlockIcon blockId={key} lookupMode="item_first" />;
  const ingredients = node.ingredients || [];
  const possible = node.possible_items || [];
  const processLabel = t(`stockpile.process.${node.process_type}`);
  return <div className="stockpile-recipe-node">
    <div className="stockpile-recipe-heading">{icon(node.icon_key || node.item_id)}
      <span><strong title={node.item_id}>{name(node.item_id, node.display_name)}</strong></span>
      {!!node.children?.length && <Button variant="ghost" size="sm" aria-expanded={open} onClick={() => setOpen(!open)}>{t(open ? "stockpile.collapse" : "stockpile.expand")}</Button>}
    </div>
    <dl className="stockpile-detail-facts">
      <div><dt>{t("stockpile.recipeType")}</dt><dd title={node.recipe_type}>{node.recipe_type_name || t("editor.unknown")}</dd></div>
      <div><dt>{t("stockpile.process")}</dt><dd>{processLabel === `stockpile.process.${node.process_type}` ? node.process_type : processLabel}</dd></div>
      <div><dt>{t("stockpile.recipeQuantity")}</dt><dd>{node.needed_count}</dd></div>
      <div><dt>{t("stockpile.recipeOutput")}</dt><dd>{node.output_count}</dd></div>
      <div><dt>{t("stockpile.recipeBatches")}</dt><dd>{node.batch_count}</dd></div>
      <div><dt>{t("stockpile.recipeExtra")}</dt><dd>{node.extra_output}</dd></div>
      <div><dt>{t("stockpile.recipeDepth")}</dt><dd>{node.depth}</dd></div>
    </dl>
    {!!ingredients.length && <ul className="stockpile-recipe-ingredients">{ingredients.map((ingredient, index) => <li key={`${ingredient.item_id}:${index}`}>
      {icon(ingredient.icon_key || ingredient.item_id)}{name(ingredient.item_id, ingredient.display_name)} × {ingredient.needed_count}
      {ingredient.tag && <span title={ingredient.tag}> · {ingredient.tag_display_name || t("editor.unknown")}</span>}{showUnresolved && ingredient.unresolved && <span> · {ingredient.unresolved_reason}</span>}
      {!!ingredient.possible_items?.length && <span> · {ingredient.possible_items.slice(0, 12).map(id => name(id)).join(", ")}{ingredient.possible_items.length > 12 && ` (+${ingredient.possible_items.length - 12})`}</span>}
    </li>)}</ul>}
    {node.tag && <p title={node.tag}>{node.tag_display_name || t("editor.unknown")}</p>}
    {!!possible.length && <p className="muted">{possible.slice(0, 12).map(id => name(id)).join(", ")}{possible.length > 12 && ` (+${possible.length - 12})`}</p>}
    {showUnresolved && node.unresolved && <p className="muted">{t("stockpile.unresolvedReason", { reason: node.unresolved_reason || "unresolved" })}</p>}
    {node.requires_fuel && <p>{t("stockpile.fuelRequired")}{node.fuel_estimate != null && ` · ${node.fuel_estimate}`}</p>}
    {node.decorative_smithing && <p>{t("stockpile.decorativeSmithing")}</p>}
    {open && node.children?.map(child => <RecipeTreeView key={child.node_id} node={child} iconImages={iconImages} itemNames={itemNames} language={language} showUnresolved={showUnresolved} depth={depth + 1} />)}
  </div>;
}
