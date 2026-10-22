import { t, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
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
  title = t("replace.pickerTitle"),
  currentBlocks = [],
}: {
  onClose: () => void;
  onAddBlocks: (blockIds: string[]) => void;
  title?: string;
  currentBlocks?: string[];
}) {
  const { locale } = useI18n();
  const [collections, setCollections] = useState<EnumeratorCollection[]>([]);
  const [loading, setLoading] = useState(true);
  const [expression, setExpression] = useState("");
  const [preview, setPreview] = useState<string[]>([]);
  const [nonBlockWarning, setNonBlockWarning] = useState<UiMessage[]>([]);
  const [evalError, setEvalError] = useState<UiMessage>("");

  useEffect(() => {
    let active = true;
    loadEnumeratorCollections(currentBlocks.length ? [{ id:"runtime:replace", name:t("replace.currentResults"), symbol:"L", category:"runtime", valueType:"block", version:"runtime", values:currentBlocks, description:t("replace.originalBlocks"), updatedAt:"runtime", readOnly:true }] : [])
      .then(collections => { if(active) setCollections(collections); })
      .catch(error => { if(active) { setCollections([]); setEvalError({key:"statistics.collectionsFailed",params:{error:String(error)}}); } })
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
        ? [...rejected.slice(0, 5), ...(rejected.length > 5 ? [{key:"replace.rejectedCount",params:{count:rejected.length}}] : [])]
        : []
    );
  }, [expression, collections, currentBlocks]);

  const collectionHints = useMemo(
    () =>
      collections
        .filter((c) => BLOCK_LIKE_TYPES.has(c.valueType))
        .map((c) => `${localizeMessage(c.name)}${c.symbol ? ` (${c.symbol})` : ""}`)
        .slice(0, 8),
    [collections, locale]
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
            <label className="enum-picker-label">{t("replace.expression")}</label>
            <Input
              className="input enum-picker-input"
              value={expression}
              placeholder={t("replace.expressionHint")}
              onChange={(e) => setExpression(e.target.value)}
              autoFocus
            />
            {!loading && collectionHints.length > 0 && (
              <div className="enum-picker-hint">{t("replace.availableCollections", {collections:collectionHints.join(locale === "zh-CN" ? "、" : ", ")})}</div>
            )}
          </div>

          {evalError && (
            <div className="enum-picker-error">{t("replace.expressionError", {error:localizeMessage(evalError)})}</div>
          )}

          {nonBlockWarning.length > 0 && (
            <div className="enum-picker-warning">
              {t("replace.nonBlockWarning", {entries:nonBlockWarning.map(localizeMessage).join(locale === "zh-CN" ? "、" : ", ")})}
            </div>
          )}

          {preview.length > 0 && (
            <div>
              <div className="enum-picker-preview-label">
                {t("replace.pickerPreview", {count:preview.length.toLocaleString(locale)})}
              </div>
              <div className="enum-picker-preview-grid">
                {preview.slice(0, 100).map((id) => (
                  <div key={id} className="enum-picker-preview-item" title={translateBlockId(id)}>
                    <BlockIcon blockId={id} lookupMode="block_first" />
                    <span className="enum-picker-preview-id">{id.replace("minecraft:", "")}</span>
                  </div>
                ))}
                {preview.length > 100 && (
                  <span className="enum-picker-preview-more">{t("replace.moreEntries", {count:(preview.length-100).toLocaleString(locale)})}</span>
                )}
              </div>
            </div>
          )}

          {preview.length === 0 && !evalError && expression.trim() && !loading && (
            <div className="enum-picker-empty">{t("replace.emptySet")}</div>
          )}

          <div className="enum-picker-buttons">
            <Button onClick={onClose}>{t("editor.cancel")}</Button>
            <Button
              variant={preview.length > 0 ? "default" : "secondary"}
              disabled={preview.length === 0}
              onClick={handleConfirm}
            >
              {t("replace.addEntries", {count:preview.length.toLocaleString(locale)})}
            </Button>
          </div>
        </div>
    </Dialog>
  );
}
