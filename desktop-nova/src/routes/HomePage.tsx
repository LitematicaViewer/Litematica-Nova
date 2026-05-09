import React from 'react';
import { open } from "@tauri-apps/plugin-dialog";
import { executeBackend, startNativeViewer } from '../services/backend';
import { loadLibrary, addOrUpdateRecord } from '../services/libraryStore';

export function HomePage({ currentFile, setCurrentFile, setRoute }: any) {
  const handleOpenLitematic = async () => {
    // 复用 LibraryPage 中的选择逻辑，这里可以简单调用 Tauri 弹窗或跳到库
    const res = await open({ filters: [{ name: "Litematic", extensions: ["litematic"] }] });
    if (res && typeof res === "string") {
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
      startNativeViewer(currentFile);
    } else {
      setRoute("render");
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%' }}>
      <div style={{ textAlign: 'center', maxWidth: 600 }}>
        <h1 style={{ fontSize: '2em', marginBottom: 16 }}>Litematica Blueprint Assistant</h1>
        <p style={{ opacity: 0.8, marginBottom: 40, lineHeight: 1.6 }}>
          第一阶段主链：打开 .litematic、做结构分析、构建单一 3D cache，<br />
          再用同一份 cache 驱动嵌入式预览和弹窗 viewer。
        </p>

        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 18 }}>
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
