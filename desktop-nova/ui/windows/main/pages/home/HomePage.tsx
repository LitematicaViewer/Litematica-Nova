import { Button } from "../../../../components/ui/button";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { FolderOpen, FileText, BarChart3, Box, Library, ArrowRight } from "lucide-react";
import "./home.css";
import React from 'react';
import { selectLitematicFile } from '../../../../../src/business/facade';
import { loadLibrary, addOrUpdateRecord, displayFilePath } from '../../../../../src/business/facade';

export function HomePage({ currentFile, setCurrentFile, setRoute, buildVersion }: any) {
  const handleOpenLitematic = async () => {
    const res = await selectLitematicFile();
    if (res) {
      setCurrentFile(res);
      setRoute("properties");

      const state = await loadLibrary();
      await addOrUpdateRecord(state, res);
    }
  };

  const handleStats = () => {
    setRoute("statistics");
  };

  const handleRender = () => {
    setRoute("render");
  };

  const currentName = currentFile ? String(currentFile).split(/[\\/]/).pop() : "";

  return (
    <ScrollArea className="home-scroll-area" aria-label="开始使用 Nova">
      <div className="home-page">
        <section className="home-introduction">
          <h1>Litematica-Nova</h1>
          <p className="home-lead muted">查看材料和结构，整理你的每一份投影。</p>
          <div className="home-start-actions">
            <Button variant="default" size="lg" onClick={handleOpenLitematic}><FolderOpen size={20} aria-hidden="true" />打开投影文件</Button>
            <Button variant="ghost" onClick={() => setRoute("library")}><Library size={18} aria-hidden="true" />浏览投影库<ArrowRight size={16} aria-hidden="true" /></Button>
          </div>
          {buildVersion && <p className="home-beta-note muted">版本 {buildVersion}</p>}
        </section>

        <section className="home-current group-box" aria-labelledby="home-current-title">
          <h2 id="home-current-title">{currentFile ? "继续当前投影" : "打开后，从这里继续"}</h2>
          {currentFile ? <div className="home-current-file">
            <FileText size={28} aria-hidden="true" />
            <div><strong>{currentName}</strong><span className="muted" title={displayFilePath(currentFile)}>{displayFilePath(currentFile)}</span></div>
          </div> : <p className="home-empty muted">选择一个 .litematic 文件，即可查看属性、材料和结构。</p>}
          <div className="home-workflows">
            <Button variant="ghost" aria-label="查看投影属性" onClick={() => setRoute("properties")} disabled={!currentFile}>
              <FileText size={22} aria-hidden="true" /><span><strong>投影属性</strong><small className="muted">名称、区域与快速预览</small></span><ArrowRight size={18} aria-hidden="true" />
            </Button>
            <Button variant="ghost" aria-label="查看材料与统计" onClick={handleStats} disabled={!currentFile}>
              <BarChart3 size={22} aria-hidden="true" /><span><strong>材料与统计</strong><small className="muted">准备材料，核对数量</small></span><ArrowRight size={18} aria-hidden="true" />
            </Button>
            <Button variant="ghost" aria-label="打开三维预览" onClick={handleRender} disabled={!currentFile}>
              <Box size={22} aria-hidden="true" /><span><strong>三维预览</strong><small className="muted">查看投影的三维结构</small></span><ArrowRight size={18} aria-hidden="true" />
            </Button>
          </div>
        </section>
      </div>
    </ScrollArea>
  );
}
