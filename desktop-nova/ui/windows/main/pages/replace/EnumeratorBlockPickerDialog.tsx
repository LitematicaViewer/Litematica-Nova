import React, { useState, useEffect, useMemo } from "react";
import {
  loadEnumeratorCollections,
  evaluateEnumeratorExpression,
  type EnumeratorCollection,
} from "../../../../../src/business/facade";
import { translateBlockId } from "../../../../../src/business/facade";
import { getAllBlocks } from "../../../../../src/business/replace";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { BlockIcon } from "../../../../components/BlockIcon";

const BLOCK_LIKE_TYPES = new Set(["block", "mixed", "unknown"]);

/**
 * Enumerator-based block picker for the Replace module.
 * Supports collection name, symbol, or full set expression.
 * Filters out non-block values (items/entities/enchantments).
 * onAddBlocks receives the filtered block ID list.
 */
export function EnumeratorBlockPickerDialog({
  onClose,
  onAddBlocks,
  title = "从枚举器选择方块",
  currentBlocks = [],
}: {
  onClose: () => void;
  onAddBlocks: (blockIds: string[]) => void;
  title?: string;
  currentBlocks?: string[];
}) {
  const [collections, setCollections] = useState<EnumeratorCollection[]>([]);
  const [loading, setLoading] = useState(true);
  const [expression, setExpression] = useState("");
  const [preview, setPreview] = useState<string[]>([]);
  const [nonBlockWarning, setNonBlockWarning] = useState<string[]>([]);
  const [evalError, setEvalError] = useState("");

  useEffect(() => {
    let active = true;
    loadEnumeratorCollections(currentBlocks.length ? [{ id:"runtime:replace", name:"当前统计结果", symbol:"L", category:"runtime", valueType:"block", version:"runtime", values:currentBlocks, description:"当前投影原始结构方块", updatedAt:"runtime", readOnly:true }] : [])
      .then(collections => { if(active) setCollections(collections); })
      .catch(error => { if(active) { setCollections([]); setEvalError(`读取枚举集合失败：${error}`); } })
      .finally(() => { if(active) setLoading(false); });
    return () => { active = false; };
  }, [currentBlocks]);

  useEffect(() => {
    if (!expression.trim() || collections.length === 0) {
      setPreview([]);
      setNonBlockWarning([]);
      setEvalError("");
      return;
    }
    const result = evaluateEnumeratorExpression(expression, collections);
    if (result.error) {
      setEvalError(result.error);
      setPreview([]);
      setNonBlockWarning([]);
      return;
    }
    setEvalError("");

    const allValues = result.values;
    const blockUniverse = new Set([...getAllBlocks(), ...currentBlocks, ...collections.filter(c => c.id === "base:dv-blocks").flatMap(c => c.values)]);
    const blockValues = allValues.filter((v) => blockUniverse.has(v));
    const rejected = allValues.filter((v) => !blockValues.includes(v));

    setPreview(blockValues);
    setNonBlockWarning(
      rejected.length > 0
        ? rejected.slice(0, 5).concat(rejected.length > 5 ? [`... 共 ${rejected.length} 项`] : [])
        : []
    );
  }, [expression, collections, currentBlocks]);

  const collectionHints = useMemo(
    () =>
      collections
        .filter((c) => BLOCK_LIKE_TYPES.has(c.valueType))
        .map((c) => `${c.name}${c.symbol ? ` (${c.symbol})` : ""}`)
        .slice(0, 8),
    [collections]
  );

  const handleConfirm = () => {
    if (preview.length === 0) return;
    onAddBlocks(preview);
    onClose();
  };

  return (
    <Dialog title={title} onClose={onClose} width="lg">

        <div className="enum-picker-content">
          {/* Expression input */}
          <div>
            <label className="enum-picker-label">集合名称 / 集合表达式</label>
            <Input
              className="input enum-picker-input"
              value={expression}
              placeholder="例：base:dv-blocks  或  木头 ∪ 石头  或  A-(map_*)"
              onChange={(e) => setExpression(e.target.value)}
              autoFocus
            />
            {!loading && collectionHints.length > 0 && (
              <div className="enum-picker-hint">可用集合：{collectionHints.join("、")}</div>
            )}
          </div>

          {evalError && (
            <div className="enum-picker-error">⚠ 表达式错误：{evalError}</div>
          )}

          {nonBlockWarning.length > 0 && (
            <div className="enum-picker-warning">
              ⚠ 以下条目不是方块 ID，将被过滤掉：{nonBlockWarning.join("、")}
            </div>
          )}

          {preview.length > 0 && (
            <div>
              <div className="enum-picker-preview-label">
                预览（共 {preview.length} 个方块，将全部添加为条目）：
              </div>
              <div className="enum-picker-preview-grid">
                {preview.slice(0, 100).map((id) => (
                  <div key={id} className="enum-picker-preview-item" title={translateBlockId(id)}>
                    <BlockIcon blockId={id} lookupMode="block_first" />
                    <span className="enum-picker-preview-id">{id.replace("minecraft:", "")}</span>
                  </div>
                ))}
                {preview.length > 100 && (
                  <span className="enum-picker-preview-more">...还有 {preview.length - 100} 个</span>
                )}
              </div>
            </div>
          )}

          {preview.length === 0 && !evalError && expression.trim() && !loading && (
            <div className="enum-picker-empty">表达式计算结果为空集</div>
          )}

          <div className="enum-picker-buttons">
            <Button className="btn btn-md" onClick={onClose}>取消</Button>
            <Button
              className={`btn btn-md${preview.length > 0 ? " btn-primary" : ""}`}
              disabled={preview.length === 0}
              onClick={handleConfirm}
            >
              添加 {preview.length > 0 ? `${preview.length} 个` : ""}条目
            </Button>
          </div>
        </div>
    </Dialog>
  );
}
