import React, { useState, useEffect, useMemo } from "react";
import {
  loadEnumeratorCollections,
  evaluateEnumeratorExpression,
  type EnumeratorCollection,
} from "../../../../../src/services/enumeratorService";
import { translateBlockId } from "../../../../../src/services/i18n";
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
}: {
  onClose: () => void;
  onAddBlocks: (blockIds: string[]) => void;
  title?: string;
}) {
  const [collections, setCollections] = useState<EnumeratorCollection[]>([]);
  const [loading, setLoading] = useState(true);
  const [expression, setExpression] = useState("");
  const [preview, setPreview] = useState<string[]>([]);
  const [nonBlockWarning, setNonBlockWarning] = useState<string[]>([]);
  const [evalError, setEvalError] = useState("");

  useEffect(() => {
    loadEnumeratorCollections()
      .then(setCollections)
      .catch(() => setCollections([]))
      .finally(() => setLoading(false));
  }, []);

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
    const blockValues = allValues.filter((v) => v.startsWith("minecraft:") && !v.includes(" "));
    const rejected = allValues.filter((v) => !blockValues.includes(v));

    setPreview(blockValues);
    setNonBlockWarning(
      rejected.length > 0
        ? rejected.slice(0, 5).concat(rejected.length > 5 ? [`... 共 ${rejected.length} 项`] : [])
        : []
    );
  }, [expression, collections]);

  const collectionHints = useMemo(
    () =>
      collections
        .filter((c) => BLOCK_LIKE_TYPES.has(c.valueType))
        .map((c) => `${c.name}${c.symbol ? ` (${c.symbol})` : ""}`)
        .slice(0, 8),
    [collections]
  );

  const handleConfirm = () => {
    if (preview.length === 0) return alert("没有可添加的方块条目。");
    onAddBlocks(preview);
    onClose();
  };

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <section
        className="dialog-content dialog-content--enum-picker"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="subwindow-title-row">
          <h3 className="subwindow-title">{title}</h3>
          <button className="btn subwindow-close-button" onClick={onClose}>×</button>
        </div>

        <div className="enum-picker-content">
          {/* Expression input */}
          <div>
            <label className="enum-picker-label">集合名称 / 集合表达式</label>
            <input
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
                    <BlockIcon blockId={id} />
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
            <button className="btn btn-md" onClick={onClose}>取消</button>
            <button
              className={`btn btn-md${preview.length > 0 ? " btn-primary" : ""}`}
              disabled={preview.length === 0}
              onClick={handleConfirm}
            >
              添加 {preview.length > 0 ? `${preview.length} 个` : ""}条目
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
