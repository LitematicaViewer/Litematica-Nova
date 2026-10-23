import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
import { displayPathText } from "../../../src/business/facade";
import { displayFilePath } from "../../../src/business/facade";
import { X } from "lucide-react";
import { Button } from "../../components/ui/button";
import { ScrollArea } from "../../components/ui/scroll-area";
import { Dialog } from "../../components/Dialog";
import { Input } from "../../components/ui/input";
import { useEffect, useLayoutEffect, useRef, useState } from "react";

import {
  getRedenSizeRules,
  importDownloadedLitematic,
  notifyRedenImport,
  redenExternalDriveLink,
  openExternalUrl,
  loadLibrary,
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  openRedenLibraryWindow as openRedenLibraryDesktopWindow,
  RedenMachine,
  RedenSizes,
  RedenDownloadOutput,
  validateRedenSizes,
} from "../../../src/business/facade";
import { ONLINE_PROJECTION_PROVIDERS, searchOnlineSource, fetchOnlineSourceDetail, downloadOnlineSource, cancelOnlineRequest, onlineRequestId } from "../../../src/business/reden";
import { emitEvent } from "../../../src/platform/events";
import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";
import { projectionLibraryImportedEvent } from "../libraryEvents";
import "../main/pages/library/library.css";

function formatSize(numBytes: number): string {
  let value = Math.max(0, numBytes);
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  while (value >= 1024.0 && index < units.length - 1) {
    value /= 1024.0;
    index++;
  }
  return index === 0 ? `${new Intl.NumberFormat(getUiLocale()).format(Math.floor(value))} ${units[index]}` : `${new Intl.NumberFormat(getUiLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(value)} ${units[index]}`;
}

function formatDate(timestamp: number): string {
  return timestamp ? new Intl.DateTimeFormat(getUiLocale(), { dateStyle: "short", timeStyle: "short" }).format(timestamp) : "-";
}

function formatRedenDate(timestamp?: number): string {
  return timestamp ? formatDate(timestamp) : "-";
}

function redenTags(item: RedenMachine): string {
  const tags = [
    item.categoryTag?.name || item.categoryTag?.tag,
    ...(item.featureTags || []).map((tag) => tag.name || tag.tag),
    ...(item.versions || []),
  ].filter(Boolean);
  return tags.length ? tags.join(" / ") : "-";
}

function redenSummary(item: RedenMachine): string {
  return (item.summary || item.description || "").replace(/\s+/g, " ").trim();
}

/**
 * Opens the online projection library according to the user's configured child-window behavior.
 */
export async function openRedenLibraryWithWindowBehavior(showOverlay: () => void): Promise<void> {
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
  const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
  if (behavior === "independent_window") {
    try {
      await openRedenLibraryDesktopWindow();
      return;
    } catch {
      // Browser preview cannot create a desktop window, so fall back to the in-window dialog.
    }
  }
  showOverlay();
}

function RedenLibraryPanel({ onClose }: { onClose?: () => void }) {
  useI18n();
  const [redenQuery, setRedenQuery] = useState(t("library.exampleCobblestoneGenerator"));
  const [redenResults, setRedenResults] = useState<RedenMachine[]>([]);
  const [redenDetail, setRedenDetail] = useState<RedenMachine | null>(null);
  const [redenStatus, setRedenStatus] = useState<UiMessage>("");
  const [redenBusy, setRedenBusy] = useState("");
  const [redenSizes, setRedenSizes] = useState<RedenSizes>({});
  const [providerId, setProviderId] = useState("reden");
  const [externalLink, setExternalLink] = useState<{ url: string; label: string; drive: boolean } | null>(null);
  const [searchDirty, setSearchDirty] = useState(false);
  const request = useRef<string | null>(null);
  const provider = ONLINE_PROJECTION_PROVIDERS.find((item) => item.id === providerId) || ONLINE_PROJECTION_PROVIDERS[0];
  const beginRequest = () => {
    if (request.current) void cancelOnlineRequest(request.current).catch(() => undefined);
    const id = onlineRequestId(); request.current = id; return id;
  };
  const cancelRequest = () => {
    const id = request.current; request.current = null; setRedenBusy(""); setSearchDirty(false);
    if (id) void cancelOnlineRequest(id).catch(() => undefined);
    setRedenStatus(message("library.onlineRequestCancelled"));
  };
  useEffect(() => () => { if (request.current) void cancelOnlineRequest(request.current).catch(() => undefined); }, []);
  useEffect(() => {
    if (!searchDirty || provider.mode !== "native") return;
    const timer = setTimeout(() => void handleRedenSearch(), 500);
    return () => clearTimeout(timer);
  }, [redenQuery, searchDirty, providerId]);

  const handleRedenSearch = async () => {
    const query = redenQuery.trim();
    if (!query) {
      setRedenStatus(message("library.enterSearchQuery"));
      return;
    }
    const id = beginRequest(); setSearchDirty(false); setRedenBusy("search");
    setRedenStatus("");
    setRedenDetail(null);
    try {
      const response = await searchOnlineSource(providerId, query, id);
      if (request.current !== id) return;
      const results = response.d || [];
      setRedenResults(results);
      setRedenStatus(results.length ? message("library.providerResultCount", { provider: provider.name, count: results.length }) : message("library.providerNoResults", { provider: provider.name }));
    } catch (err: any) {
      if (request.current === id) setRedenStatus(message("library.providerSearchFailed", { provider: provider.name, error: String(err) }));
    } finally {
      if (request.current === id) { request.current = null; setRedenBusy(""); }
    }
  };

  const handleRedenDetail = async (machineId: string, downloadFiles = false) => {
    const id = beginRequest();
    setRedenBusy(`detail:${machineId}`);
    setRedenStatus("");
    try {
      const detail = await fetchOnlineSourceDetail(providerId, machineId, id, downloadFiles);
      if (request.current !== id) return;
      setRedenDetail(detail);
      const rules = getRedenSizeRules(detail);
      setRedenSizes({
        xSize: detail.hasX ? rules.x.min : undefined,
        ySize: detail.hasY ? rules.y.min : undefined,
        zSize: detail.hasZ ? rules.z.min : undefined,
      });
      setRedenStatus(message("library.providerDetailsLoaded", { provider: provider.name, name: detail.name || detail.key }));
    } catch (err: any) {
      if (request.current === id) setRedenStatus(message("library.providerDetailsFailed", { provider: provider.name, error: String(err) }));
    } finally {
      if (request.current === id) { request.current = null; setRedenBusy(""); }
    }
  };

  const importRedenPath = async (path: string) => {
    const libraryState = await loadLibrary();
    await importDownloadedLitematic(libraryState, path);
    return await notifyRedenImport(() => emitEvent(projectionLibraryImportedEvent, { path }));
  };

  const finishRedenDownload = async (output: RedenDownloadOutput, generated = false, requestId?: string) => {
    const isCurrent = () => !requestId || request.current === requestId;
    if (!isCurrent()) return;
    if (output.kind === "external_link") {
      const drive = ["quark", "baidu"].includes(output.provider);
      const label = output.provider === "quark" ? t("library.quarkDrive") : output.provider === "baidu" ? t("library.baiduDrive") : ONLINE_PROJECTION_PROVIDERS.find(item => item.id === output.provider)?.name || t("library.originalSite");
      setExternalLink({ url: output.url, label, drive });
      setRedenStatus(message("library.externalDownloadPrompt"));
      return;
    }
    try {
      const warning = await importRedenPath(output.path);
      if (isCurrent()) setRedenStatus(message(generated ? "library.providerGeneratedImported" : "library.providerImported", { provider: provider.name, name: output.file_name, size: formatSize(output.bytes), warning: warning || "" }));
    } catch (error) {
      if (isCurrent()) setRedenStatus(message("library.providerImportFailed", { provider: provider.name, name: output.file_name, error: String(error), path: displayFilePath(output.path) }));
    }
  };

  const confirmExternalLink = async () => {
    if (!externalLink) return;
    setRedenBusy("external:open");
    try {
      await openExternalUrl(externalLink.url);
      setRedenStatus(message("library.externalBrowserOpened", { site: externalLink.label }));
      setExternalLink(null);
    } catch (error) { setRedenStatus(message("library.externalBrowserFailed", { error: String(error) })); }
    finally { setRedenBusy(""); }
  };

  const handleRedenDownloadAttachment = async (detail: RedenMachine, index: number) => {
    const driveLink = redenExternalDriveLink(detail.attachments?.[index]?.url);
    if (driveLink) {
      await finishRedenDownload(driveLink);
      return;
    }
    setRedenBusy(`download:${index}`);
    const id = beginRequest();
    setRedenStatus("");
    try {
      const output = await downloadOnlineSource(providerId, detail.key, index, id);
      if (request.current !== id) return;
      await finishRedenDownload(output, false, id);
    } catch (err: any) {
      if (request.current === id) setRedenStatus(message("library.providerDownloadFailed", { provider: provider.name, error: String(err) }));
    } finally {
      if (request.current === id) { request.current = null; setRedenBusy(""); }
    }
  };

  const handleRedenDownloadParametric = async (detail: RedenMachine) => {
    const errors = validateRedenSizes(detail, redenSizes);
    if (errors.length) {
      setRedenStatus(message("library.invalidRedenDimensions", { error: errors.join("；") }));
      return;
    }
    setRedenBusy("download:parametric");
    const id = beginRequest();
    setRedenStatus("");
    try {
      const output = await downloadOnlineSource(providerId, detail.key, 0, id, redenSizes);
      if (request.current !== id) return;
      await finishRedenDownload(output, true, id);
    } catch (err: any) {
      if (request.current === id) setRedenStatus(message("library.providerStructureFailed", { provider: provider.name, error: String(err) }));
    } finally {
      if (request.current === id) { request.current = null; setRedenBusy(""); }
    }
  };

  const statusClassName = [
    "subwindow-status-text",
    (typeof redenStatus === "string" ? /失败|不合法|failed|invalid/i.test(redenStatus) : /Failed|invalid/i.test(redenStatus.key)) ? "subwindow-status-text-error" : "",
  ].filter(Boolean).join(" ");

  return (
    <section className="subwindow-panel-shell library-aux-panel library-reden-panel">
      <div className="subwindow-title-bar">
        <div className="subwindow-title-stack">
          <h3 className="subwindow-title">{t("library.onlineLibraryTitle")}</h3>
        </div>
        {onClose ? <Button className="subwindow-close-button" type="button" aria-label={t("library.closeWindow")} onClick={onClose}><X size={18} aria-hidden="true" /></Button> : null}
      </div>

      <div className="subwindow-body">
        <div className="subwindow-toolbar subwindow-wrap-row" aria-label={t("library.onlineSources")}>
          {ONLINE_PROJECTION_PROVIDERS.map((item) => <Button key={item.id} variant={item.id === providerId ? "default" : "ghost"}
            aria-pressed={item.id === providerId} onClick={() => { cancelRequest(); setProviderId(item.id); setRedenResults([]); setRedenDetail(null); setExternalLink(null); setRedenStatus(""); }}>{item.name}</Button>)}
        </div>
        {provider.mode === "website" ? <section className="reden-detail-stack">
          <h4>{provider.name} {t("library.webOnlySuffix")}</h4>
          <p>{provider.url}</p>
          <Button disabled={!!redenBusy} onClick={() => setExternalLink({ url: provider.url, label: provider.name, drive: false })}>{t("library.openOriginalSite")}</Button>
        </section> : <>
        <div className="subwindow-toolbar subwindow-wrap-row">
          <Input
            className="input nova-input-flex"
            value={redenQuery}
            aria-label={t("library.searchOnlineSchematics")}
            placeholder={t("library.onlineSearchPlaceholder")}
            onChange={(event) => { setRedenQuery(event.target.value); setSearchDirty(true); }}
            onKeyDown={(event) => { if (event.key === "Enter") handleRedenSearch(); }}
          />
          <Button disabled={!!redenBusy} onClick={handleRedenSearch}>
            {redenBusy === "search" ? t("library.searching") : t("library.search")}
          </Button>
          {redenBusy && <Button variant="ghost" onClick={cancelRequest}>{t("library.cancelWaiting")}</Button>}
          <Button variant="ghost" onClick={() => setExternalLink({ url: provider.url, label: provider.name, drive: false })}>{t("library.originalSiteEntry")}</Button>
        </div>

        <div className="reden-content-grid">
          <div className="reden-results-pane">
            <div className="reden-pane-title">{t("library.searchResults")}</div>
            <ScrollArea className="library-online-results-scroll"><div className="reden-result-list">
              {redenResults.length > 0 ? redenResults.map((item) => {
                const resultClassName = [
                  "library-card",
                  "reden-result-card",
                  redenDetail?.key === item.key ? "library-card-current" : "",
                ].filter(Boolean).join(" ");
                return (
                  <Button
                    key={item.key}
                    className={resultClassName}
                    variant="ghost"
                    disabled={!!redenBusy}
                    aria-pressed={redenDetail?.key === item.key}
                    onClick={() => {
                      if (redenBusy === `detail:${item.key}`) return;
                      handleRedenDetail(item.key);
                    }}

                  >
                    <div className="lib-card-left reden-result-card-body">
                      <div className="lib-card-line-main">
                        <div className="lib-card-heading">
                          <span className="lib-card-title">{item.name || item.key}</span>
                        </div>
                      </div>
                      <div className="lib-card-text">{t("library.sourceAccess", { provider: provider.name })}</div>
                      <div className="lib-card-text">{t("library.tagsLabel")}{redenTags(item)}</div>
                      <div className="reden-result-row">
                        <div className="reden-result-meta">{t("library.authorLabel")}{item.author?.username || "-"}</div>
                        <div className="reden-result-meta reden-result-stats">{t("library.downloadsLabel")}{item.downloads ?? "-"} {t("library.favoritesLabel")}{item.upVotes ?? "-"}</div>
                      </div>
                      <div className="reden-result-date">{t("library.updatedLabel")}{formatRedenDate(item.updatedAt)}</div>
                      {redenSummary(item) && <div className="reden-result-summary">{redenSummary(item).slice(0, 120)}</div>}
                    </div>
                  </Button>
                );
              }) : (
                <div className="subwindow-empty-state">
                  {t("library.noSearchResults")}</div>
              )}
            </div></ScrollArea>
          </div>

          <ScrollArea className="reden-detail-pane">
            {redenDetail ? (
              <div className="reden-detail-stack">
                <div>
                  <div className="reden-detail-title">{redenDetail.name || redenDetail.key}</div>
                  <div className="reden-detail-meta">ID：{redenDetail.key} {t("library.typeLabel")}{redenDetail.type || "-"} {t("library.authorInlineLabel")}{redenDetail.author?.username || "-"}</div>
                  {redenSummary(redenDetail) && <div className="reden-detail-summary">{redenSummary(redenDetail)}</div>}
                  {redenDetail.pageUrl && <Button variant="ghost" onClick={() => setExternalLink({ url: redenDetail.pageUrl!, label: provider.name, drive: false })}>{t("library.originalDetails")}</Button>}
                  {providerId === "minemev" && <Button onClick={() => void finishRedenDownload({ kind: "external_link", provider: "minemev", url: redenDetail.pageUrl || provider.url })}>{t("library.downloadOriginal")}</Button>}
                  {redenDetail.downloadListAvailable && <Button disabled={!!redenBusy} onClick={() => void handleRedenDetail(redenDetail.key, true)}>{t("library.fetchAttachments")}</Button>}
                </div>

                {(redenDetail.attachments || []).length > 0 && (
                  <div className="reden-section-stack">
                    <div className="reden-section-title">{t("library.normalAttachments")}</div>
                    {(redenDetail.attachments || []).map((attachment, index) => (
                      <div key={`${attachment.name}-${index}`} className="reden-attachment-row">
                        <div className="nova-flex-1">
                          <div>{index + 1}. {attachment.name || `attachment-${index + 1}.litematic`}</div>
                          <div className="reden-attachment-meta">{attachment.size ? formatSize(attachment.size) : "-"} {attachment.description || ""}</div>
                        </div>
                        <Button disabled={!!redenBusy} onClick={() => handleRedenDownloadAttachment(redenDetail, index)}>
                          {redenBusy === `download:${index}` ? t("library.downloading") : t("library.downloadImport")}
                        </Button>
                      </div>
                    ))}
                  </div>
                )}

                {(redenDetail.hasX || redenDetail.hasY || redenDetail.hasZ) && (
                  <div className="reden-section-stack">
                    <div className="reden-section-title">{t("library.repeatedDimensions")}</div>
                    <div className="reden-size-grid">
                      {(["x", "y", "z"] as const).map((axis) => {
                        const enabled = axis === "x" ? redenDetail.hasX : axis === "y" ? redenDetail.hasY : redenDetail.hasZ;
                        if (!enabled) return null;
                        const field = `${axis}Size` as keyof RedenSizes;
                        const rules = getRedenSizeRules(redenDetail)[axis];
                        const hint = [
                          rules.min !== undefined ? `min ${rules.min}` : "",
                          rules.max !== undefined ? `max ${rules.max}` : "",
                          rules.mod ? `mod ${rules.mod.step},${rules.mod.offset}` : "",
                        ].filter(Boolean).join(" / ");
                        return (
                          <label key={axis} className="reden-size-field">
                            <span>{axis.toUpperCase()} ({hint || t("library.noDimensionRule")})</span>
                            <Input
                              className="input"
                              type="number"
                              value={redenSizes[field] ?? ""}
                              onChange={(event) => setRedenSizes((current) => ({ ...current, [field]: Number(event.target.value) }))}
                            />
                          </label>
                        );
                      })}
                    </div>
                    <Button className="reden-parametric-button" disabled={!!redenBusy} onClick={() => handleRedenDownloadParametric(redenDetail)}>
                      {redenBusy === "download:parametric" ? t("library.generatingDownload") : t("library.generateDownloadImport")}
                    </Button>
                  </div>
                )}
              </div>
            ) : (
              <div className="subwindow-status-text reden-empty-detail">{t("library.selectSearchResult")}</div>
            )}
          </ScrollArea>
        </div>
        </>}

        {redenStatus && (
          <footer className="reden-footer">
            <div className={statusClassName}>
              {localizeMessage(redenStatus)}
            </div>
          </footer>
        )}
        {externalLink && <Dialog title={t("library.openNamed", { name: externalLink.label })} onClose={() => setExternalLink(null)} busy={redenBusy === "external:open"}
          footer={<><Button variant="ghost" disabled={redenBusy === "external:open"} onClick={() => setExternalLink(null)}>{t("library.cancel")}</Button>
            <Button disabled={redenBusy === "external:open"} onClick={() => void confirmExternalLink()}>{t("library.confirmExternalBrowser")}</Button></>}>
          <p>{externalLink.drive ? t("library.externalDriveHint") : t("library.externalSiteHint")}</p>
          <p>{externalLink.url}</p>
        </Dialog>}
      </div>
    </section>
  );
}

/**
 * Overlay dialog form of the online projection library.
 */
export function RedenLibraryDialog({ onClose }: { onClose: () => void }) {
  useI18n();
  return (
    <Dialog title={t("library.downloadSchematic")} width="lg" className="library-window-dialog" onClose={onClose}>
      <RedenLibraryPanel />
    </Dialog>
  );
}

/**
 * Renders the standalone desktop window used for the online projection library.
 */
export function RedenLibraryWindow() {
  useI18n();
  const [themeId, setThemeId] = useState(currentThemeId);

  useLayoutEffect(() => {
    applyThemeStylesheet(themeId);
  }, [themeId]);

  useEffect(() => {
    setThemeId(currentThemeId());
    return subscribeToThemeChanges(setThemeId);
  }, []);

  return (
    <main className={["subwindow-standalone-page", themeClassName(themeId)].filter(Boolean).join(" ")}>
      <RedenLibraryPanel />
    </main>
  );
}
