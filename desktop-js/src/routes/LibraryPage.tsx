import React, { useState, useEffect } from 'react';
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { open as openShell } from "@tauri-apps/plugin-shell";
import { loadLibrary, saveLibrary, addOrUpdateRecord, analyzeFile, LibraryState, ProjectionRecord } from '../services/libraryStore';
import { checkFileExists } from '../services/backend';

function formatSize(numBytes: number): string {
  let value = Math.max(0, numBytes);
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  while (value >= 1024.0 && index < units.length - 1) {
    value /= 1024.0;
    index++;
  }
  return index === 0 ? `${Math.floor(value)} ${units[index]}` : `${value.toFixed(1)} ${units[index]}`;
}

function formatDate(timestamp: number): string {
  if (!timestamp) return "-";
  const d = new Date(timestamp);
  const pad = (n: number) => n.toString().padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function ProjectionCard({ record, isCurrent, onSetCurrent, onOpenFolder, onReanalyze, onRemove }: any) {
  const r: ProjectionRecord = record;
  const tags = r.tags && r.tags.length > 0 ? r.tags.join(" / ") : "无标签";
  const internalName = "Unnamed"; // placeholder as per req
  const author = r.author || "Unknown";
  
  return (
    <div 
      className="library-card"
      style={{ 
        border: isCurrent ? '2px solid var(--primary)' : '',
        background: isCurrent ? 'var(--mc-btn-active-bg)' : 'var(--surface)',
      }}
      onClick={onSetCurrent}
    >
      <div className="lib-card-left">
        <div className="lib-card-title">{r.displayName}</div>
        <div className="lib-card-text">标签：{tags}</div>
        <div className="lib-card-text">内部名称：{internalName} &nbsp;&nbsp;&nbsp;&nbsp; 作者：{author}</div>
        <div className="lib-card-highlight">原文件：{r.fileName}</div>
        <div className="lib-card-path" title={r.path}>{r.path}</div>
        <div className="lib-card-text">
          导入：{formatDate(r.lastAnalyzedAt)} &nbsp;&nbsp;&nbsp;&nbsp; 最近使用：{formatDate(r.lastAnalyzedAt)}
        </div>
        
        {r.status !== 'ok' && (
          <div style={{ color: r.status === 'missing' ? '#ff6666' : 'orange', marginTop: 8, fontSize: '0.9em' }}>
            {r.status === 'missing' ? '文件已丢失' : `解析失败: ${r.lastError}`}
          </div>
        )}

        <div style={{ flex: 1 }} />
        <div className="lib-card-actions">
          <button className="btn" onClick={(e) => { e.stopPropagation(); onSetCurrent(); }}>打开</button>
          <button className="btn" disabled>另存为</button>
          <button className="btn" disabled>更改属性</button>
          <button className="btn" onClick={(e) => { e.stopPropagation(); onRemove(); }}>删除</button>
        </div>
      </div>
      
      <div className="lib-card-right">
        <div className="lib-card-size">{formatSize(r.fileSize)}</div>
        <div className="lib-card-preview-box">
          暂无预览图
        </div>
        <button className="btn" style={{ width: '100%' }} disabled>生成预览</button>
      </div>
    </div>
  );
}

export function LibraryPage({ currentFile, setCurrentFile, setRoute }: any) {
  const [state, setState] = useState<LibraryState>({ records: [] });
  const [search, setSearch] = useState("");
  const [tagFilter, setTagFilter] = useState("");
  const [sortMode, setSortMode] = useState("manual");
  const [limit, setLimit] = useState(20);
  const [isRefreshing, setIsRefreshing] = useState(false);

  useEffect(() => {
    loadLibrary().then(setState);
  }, []);

  const handleSelect = async () => {
    const res = await openDialog({ filters: [{ name: "Litematic", extensions: ["litematic"] }] });
    if (res && typeof res === "string") {
      setCurrentFile(res);
      const newState = await addOrUpdateRecord(state, res);
      setState(newState);
      setRoute("properties");
    }
  };

  const handleRefresh = async () => {
    setIsRefreshing(true);
    const newState = { ...state };
    for (let i = 0; i < newState.records.length; i++) {
      const rec = newState.records[i];
      const exists = await checkFileExists(rec.path);
      if (!exists) {
        rec.status = "missing";
      } else {
        const analysis = await analyzeFile(rec.path);
        Object.assign(rec, analysis);
        rec.lastAnalyzedAt = Date.now();
      }
    }
    await saveLibrary(newState);
    setState(newState);
    setIsRefreshing(false);
  };

  const handleRemove = async (path: string) => {
    const newState = { ...state, records: state.records.filter(r => r.path !== path) };
    await saveLibrary(newState);
    setState(newState);
    if (currentFile === path) {
        setCurrentFile("");
    }
  };

  const handleReanalyze = async (path: string) => {
    const newState = await addOrUpdateRecord(state, path);
    setState(newState);
  };

  const handleOpenFolder = async (path: string) => {
    try {
      const exists = await checkFileExists(path);
      if (!exists) {
        alert("文件已失效");
        return;
      }
      const dir = path.substring(0, Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/')));
      await openShell(dir);
    } catch(e) {
      alert("打开失败");
    }
  };

  // Filter and sort
  let visibleRecords = state.records.filter(r => {
    if (search) {
      const q = search.toLowerCase();
      if (!r.displayName.toLowerCase().includes(q) && 
          !r.fileName.toLowerCase().includes(q) && 
          !r.path.toLowerCase().includes(q) && 
          !r.author.toLowerCase().includes(q) && 
          !r.description.toLowerCase().includes(q)) return false;
    }
    if (tagFilter) {
      if (tagFilter === "已失效" && r.status !== "missing") return false;
      if (tagFilter === "解析失败" && r.status !== "parse_error") return false;
      if (tagFilter === "无标签" && r.tags.length > 0) return false;
    }
    return true;
  });

  if (sortMode === "recent_import") {
    // preserve order since we unshift
  } else if (sortMode === "recent_mod") {
    visibleRecords.sort((a,b) => b.lastAnalyzedAt - a.lastAnalyzedAt);
  } else if (sortMode === "blocks") {
    visibleRecords.sort((a,b) => b.totalBlocks - a.totalBlocks);
  } else if (sortMode === "name") {
    visibleRecords.sort((a,b) => a.displayName.localeCompare(b.displayName));
  }
  
  if (limit !== 0) {
    visibleRecords = visibleRecords.slice(0, limit);
  }

  const issueCount = state.records.filter(r => r.status === "missing" || r.status === "parse_error").length;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <div style={{ fontSize: '1.2em', fontWeight: 'bold' }}>投影库</div>
        <div style={{ color: 'var(--fg)', opacity: 0.5, flex: 1, fontSize: '0.9em' }}>
          {state.records.length} 条记录，{issueCount} 条需关注
        </div>
        <div style={{ fontSize: '0.9em' }}>最近保留</div>
        <select className="input" style={{ width: 80 }} value={limit} onChange={e => setLimit(Number(e.target.value))}>
          <option value="20">20</option>
          <option value="50">50</option>
          <option value="100">100</option>
          <option value="200">200</option>
          <option value="0">全部</option>
        </select>
        <button className="btn" onClick={handleRefresh} disabled={isRefreshing}>{isRefreshing ? "刷新中..." : "刷新校验"}</button>
      </div>

      <div style={{ display: 'flex', gap: 12 }}>
        <input className="input" style={{ flex: 1 }} placeholder="搜索投影名、原文件名或原地址" value={search} onChange={e => setSearch(e.target.value)} />
        <select className="input" style={{ width: 140 }} value={tagFilter} onChange={e => setTagFilter(e.target.value)}>
          <option value="">全部标签</option>
          <option value="无标签">无标签</option>
          <option value="已失效">已失效</option>
          <option value="解析失败">解析失败</option>
        </select>
        <select className="input" style={{ width: 120 }} value={sortMode} onChange={e => setSortMode(e.target.value)}>
          <option value="manual">手动排序</option>
          <option value="recent_import">最近导入</option>
          <option value="recent_mod">最近修改</option>
          <option value="blocks">方块数高到低</option>
          <option value="name">名称 A-Z</option>
        </select>
        <button className="btn" style={{ minWidth: 100 }}>应用排序</button>
      </div>

      {state.records.length === 0 ? (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', paddingTop: 20, opacity: 0.35 }}>
          <div style={{ textAlign: 'center', maxWidth: 600 }}>
            <p style={{ margin: 0, fontSize: '0.95em' }}>投影库还没有内容。请先通过默认导入打开一个 .litematic 文件。</p>
            {/* hidden button to allow opening a file manually for testing if needed, though hidden to match screenshot */}
            <button className="btn" onClick={handleSelect} style={{ opacity: 0, position: 'absolute', top: -9999 }}>外部选择文件...</button>
          </div>
        </div>
      ) : (
        <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 8, paddingRight: 4 }}>
          {visibleRecords.map(r => (
            <ProjectionCard 
              key={r.path}
              record={r}
              isCurrent={currentFile === r.path}
              onSetCurrent={() => setCurrentFile(r.path)}
              onOpenFolder={() => handleOpenFolder(r.path)}
              onReanalyze={() => handleReanalyze(r.path)}
              onRemove={() => handleRemove(r.path)}
            />
          ))}
        </div>
      )}
    </div>
  );
}


