import React from 'react';
import { openProjectionViewer, selectLitematicFile } from '../../business/facade';
import { loadLibrary, addOrUpdateRecord } from '../../business/facade';

export function HomePage({ currentFile, setCurrentFile, setRoute }: any) {
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
    if (currentFile) {
      openProjectionViewer(currentFile);
    } else {
      setRoute("render");
    }
  };

  return (
    <div className="nova-page-center">
      <div className="nova-home-panel">
        <h1 className="home-title">Litematica Blueprint Assistant</h1>
        <p className="home-subtitle">
          第一阶段主链：打开 .litematic、做结构分析、构建单一 3D cache，<br />
          再用同一份 cache 驱动嵌入式预览和弹窗 viewer。
        </p>

        <div className="nova-home-actions">
          <button className="btn mc-btn-large" onClick={handleOpenLitematic}>
            打开 .litematic
          </button>
          <button className="btn mc-btn-large" onClick={handleStats}>
            查看分析统计
          </button>
          <button className="btn mc-btn-large" onClick={handleRender}>
            进入 3D 渲染页
          </button>
        </div>
      </div>
    </div>
  );
}


