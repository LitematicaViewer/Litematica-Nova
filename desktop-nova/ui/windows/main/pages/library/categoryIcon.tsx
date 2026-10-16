import React, { useEffect, useRef, useState } from "react";
import { Folder, Warehouse, Package, Hammer, Building2, Wheat, Crosshair, Boxes, Flame, Trees, Skull, Mountain, Anvil, Box, Cpu, Cog, Layers, Star, BookOpen } from "lucide-react";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import type { LibraryCategoryIcon } from "../../../../../src/business/facade";
import { displayPathText } from "../../../../../src/business/facade";
import { LIBRARY_ICON_PRESETS, LIBRARY_LUCIDE_ICON_NAMES, validateLibraryIconDataUrl } from "../../../../../src/business/projectionLibrary";

const icons: Record<string, React.ElementType> = { Folder, Warehouse, Package, Hammer, Building2, Wheat, Crosshair, Boxes, Flame, Trees, Skull, Mountain, Anvil, Box, Cpu, Cog, Layers, Star, BookOpen };
export function CategoryIcon({ icon }: { icon: LibraryCategoryIcon }) {
  if (icon.kind === "minecraft") return <span className="library-category-icon"><BlockIcon blockId={icon.blockId} /></span>;
  if (icon.kind === "image") return <img className="library-category-icon" src={icon.dataUrl} alt="" />;
  const Icon = icons[icon.name] || Folder;
  return <Icon className="library-category-icon" size={16} strokeWidth={1.75} aria-hidden="true" />;
}
const minecraftIcons = ["chest", "shulker_box", "crafting_table", "tnt", "furnace", "oak_sapling", "iron_ingot", "hopper", "stone", "wheat", "redstone", "oak_log"];
export function CategoryIconDialog({ name, icon, busy, onClose, onSubmit }: { name: string; icon: LibraryCategoryIcon; busy: boolean; onClose: () => void; onSubmit: (icon: LibraryCategoryIcon) => Promise<void> }) {
  const [choice, setChoice] = useState(icon); const [kind, setKind] = useState(icon.kind);
  const [blockId, setBlockId] = useState(icon.kind === "minecraft" ? icon.blockId : "minecraft:chest");
  const [error, setError] = useState(""); const uploadEpoch = useRef(0);
  useEffect(() => () => { uploadEpoch.current++; }, []);
  const upload = async (file?: File) => {
    if (!file) return;
    const epoch = ++uploadEpoch.current; setError("");
    try {
      if (file.size > 1024 * 1024 || !["image/png", "image/jpeg", "image/webp", "image/svg+xml"].includes(file.type)) throw new Error("请选择 1 MB 以内的 PNG、JPEG、WebP 或 SVG。");
      const dataUrl = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onerror = () => reject(new Error("图片读取失败，请重新选择。")); reader.onload = () => resolve(String(reader.result)); reader.readAsDataURL(file); });
      const checked = validateLibraryIconDataUrl(dataUrl);
      if (uploadEpoch.current === epoch) setChoice({ kind: "image", dataUrl: checked });
    } catch (failure) { if (uploadEpoch.current === epoch) setError(String(failure)); }
  };
  return <Dialog title={`选择「${name}」的图标`} width="md" busy={busy} onClose={onClose} footer={<><Button disabled={busy} onClick={onClose}>取消</Button><Button variant="default" disabled={busy || choice.kind !== kind || !!error || (kind === "minecraft" && (!/^minecraft:[a-z0-9_]+$/.test(blockId) || choice.kind !== "minecraft" || choice.blockId !== blockId))} onClick={() => void onSubmit(choice).catch((failure) => setError(String(failure)))}>保存图标</Button></>}>
    <div className="library-icon-tabs" role="group" aria-label="图标来源">{([['lucide', 'SVG 图标'], ['minecraft', 'Minecraft'], ['image', '上传图片']] as const).map(([value, label]) => <Button key={value} aria-pressed={kind === value} disabled={busy} onClick={() => { uploadEpoch.current++; setKind(value); setError(""); }}>{label}</Button>)}</div>
    {kind === "lucide" && <>
      <p>用途预设只选择图标，不会创建分类。</p>
      <div className="library-icon-grid library-icon-presets">{LIBRARY_ICON_PRESETS.map(([id, label, value]) => <Button key={id} variant="ghost" aria-label={`选择${label}图标`} aria-pressed={choice.kind === "lucide" && choice.name === value} disabled={busy} onClick={() => setChoice({ kind: "lucide", name: value })}><CategoryIcon icon={{ kind: "lucide", name: value }} /><span>{label}</span></Button>)}</div>
      <div className="library-icon-grid">{LIBRARY_LUCIDE_ICON_NAMES.filter((value) => !LIBRARY_ICON_PRESETS.some(([, , preset]) => preset === value)).map((value) => <Button key={value} variant="ghost" aria-label={`选择 ${value} 图标`} aria-pressed={choice.kind === "lucide" && choice.name === value} disabled={busy} onClick={() => setChoice({ kind: "lucide", name: value })}><CategoryIcon icon={{ kind: "lucide", name: value }} /></Button>)}</div>
    </>}
    {kind === "minecraft" && <><p>读取真实游戏物品资源，缺失时显示缺失提示。</p><div className="library-icon-grid">{minecraftIcons.map((value) => <Button key={value} variant="ghost" aria-label={`选择 minecraft:${value} 图标`} aria-pressed={choice.kind === "minecraft" && choice.blockId === `minecraft:${value}`} disabled={busy} onClick={() => { setBlockId(`minecraft:${value}`); setChoice({ kind: "minecraft", blockId: `minecraft:${value}` }); }}><CategoryIcon icon={{ kind: "minecraft", blockId: `minecraft:${value}` }} /></Button>)}</div><label htmlFor="library-icon-block-id">其他物品 ID</label><Input id="library-icon-block-id" value={blockId} disabled={busy} onChange={(event) => { const value = event.target.value; setBlockId(value); setError(""); if (/^minecraft:[a-z0-9_]+$/.test(value)) setChoice({ kind: "minecraft", blockId: value }); }} /></>}
    {kind === "image" && <div className="library-icon-upload"><label htmlFor="library-icon-file">图片 / 基础 SVG（1 MB 以内，SVG 64 KB 以内）</label><Input id="library-icon-file" type="file" disabled={busy} accept="image/png,image/jpeg,image/webp,image/svg+xml" onChange={(event) => { void upload(event.target.files?.[0]); event.target.value = ""; }} /><p>SVG 不接受脚本、动画、样式、外链或嵌入图片。</p>{choice.kind === "image" && <img className="library-icon-upload-preview" src={choice.dataUrl} alt="图标预览" />}</div>}
    {error && <p className="dialog-message dialog-message-error" role="alert">{displayPathText(error)}</p>}
  </Dialog>;
}
