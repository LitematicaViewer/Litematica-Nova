import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { Button } from "../../../../components/ui/button";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { FolderOpen, FileText, BarChart3, Box, Library, ArrowRight } from "lucide-react";
import "./home.css";
import { RecentProjects } from "./RecentProjects";
import React, { useRef, useState } from 'react';
import { selectLitematicFile } from '../../../../../src/business/facade';
import { loadLibrary, addOrUpdateRecord, displayFilePath } from '../../../../../src/business/facade';

export function HomePage({ currentFile, setCurrentFile, setRoute, buildVersion }: any) {
  const { locale } = useI18n();
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState("");
  const openingRef = useRef(false);
  const handleOpenLitematic = async () => {
    if (openingRef.current) return;
    openingRef.current = true; setOpening(true); setError("");
    try {
    const res = await selectLitematicFile();
    if (res) {
      const state = await loadLibrary();
      await addOrUpdateRecord(state, res);
      setCurrentFile(res);
      setRoute("properties");
    }
    } catch (reason) { setError(String(reason)); }
    finally { openingRef.current = false; setOpening(false); }
  };

  const handleStats = () => {
    setRoute("statistics");
  };

  const handleRender = () => {
    setRoute("render");
  };

  const currentName = currentFile ? String(currentFile).split(/[\\/]/).pop() : "";

  return (
    <ScrollArea className="home-scroll-area" aria-label={t("home.start")}>
      <div className="home-page">
        <section className="home-introduction">
          <h1>Litematica-Nova</h1>
          <p className="home-lead muted">{t("home.lead")}</p>
          <div className="home-start-actions">
            <Button variant="default" onClick={handleOpenLitematic} disabled={opening}><ThemeIcon name="folder" size={18} fallback={FolderOpen} />{t("home.open")}</Button>
            <Button variant="ghost" onClick={() => setRoute("library")}><ThemeIcon name="gallery" size={18} fallback={Library} />{t("home.browse")}<ArrowRight size={16} aria-hidden="true" /></Button>
          </div>
          {error && <p role="alert" className="home-open-error">{error}</p>}
          {buildVersion && <p className="home-beta-note muted">{t("home.version", {version:buildVersion})}</p>}
        </section>

        <div className="home-continue-grid">
        <RecentProjects onOpen={path => { setCurrentFile(path); setRoute("properties"); }} />
        <section className="home-current" aria-labelledby="home-current-title">
          <h2 id="home-current-title">{currentFile ? t("home.continue") : t("home.openToContinue")}</h2>
          {currentFile ? <div className="home-current-file">
            <FileText size={28} aria-hidden="true" />
            <div><strong>{currentName}</strong><span className="muted" title={displayFilePath(currentFile)}>{displayFilePath(currentFile)}</span></div>
          </div> : <p className="home-empty muted">{t("home.empty")}</p>}
          <div className="home-workflows">
            <Button variant="ghost" aria-label={t("home.propertiesAria")} onClick={() => setRoute("properties")} disabled={!currentFile}>
              <ThemeIcon name="properties" size={22} fallback={FileText} /><span className="home-workflow-copy"><strong>{t("properties.title")}</strong><small className="muted">{t("home.propertiesHint")}</small></span><ArrowRight size={18} aria-hidden="true" />
            </Button>
            <Button variant="ghost" aria-label={t("home.statisticsAria")} onClick={handleStats} disabled={!currentFile}>
              <ThemeIcon name="statistics" size={22} fallback={BarChart3} /><span className="home-workflow-copy"><strong>{t("home.statistics")}</strong><small className="muted">{t("home.statisticsHint")}</small></span><ArrowRight size={18} aria-hidden="true" />
            </Button>
            <Button variant="ghost" aria-label={t("home.previewAria")} onClick={handleRender} disabled={!currentFile}>
              <ThemeIcon name="render" size={22} fallback={Box} /><span className="home-workflow-copy"><strong>{t("home.preview")}</strong><small className="muted">{t("home.previewHint")}</small></span><ArrowRight size={18} aria-hidden="true" />
            </Button>
            <Button variant="ghost" onClick={() => setRoute("flake")} disabled={!currentFile}>{t("shell.layers")}</Button>
            <Button variant="ghost" onClick={() => setRoute("composition")} disabled={!currentFile}>{locale === "en-US" ? "Compose / Split" : "组合/拆分"}</Button>
          </div>
        </section>
        </div>
      </div>
    </ScrollArea>
  );
}
