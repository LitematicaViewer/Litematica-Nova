import React, { useState, useEffect } from 'react';
import { loadDatabases } from './services/blockstateDb';
import { initI18n } from './services/i18n';
import { HomePage } from './routes/HomePage';
import { LibraryPage } from './routes/LibraryPage';
import { PropertiesPage } from './routes/PropertiesPage';
import { StatisticsPage } from './routes/StatisticsPage';
import { FlakePage } from './routes/FlakePage';
import { ReplacePage } from './routes/ReplacePage';
import { GeneratePage } from './routes/GeneratePage';
import { RenderPage } from './routes/RenderPage';
import { SettingsPage } from './routes/SettingsPage';
import { startNativeViewer } from './services/backend';

const NAV_ICON_SVGS: Record<string, string> = {
  home: '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M2 7.5 8 2l6 5.5V14H9.5V9.5h-3V14H2z" fill="currentColor"/></svg>',
  gallery: '<svg width="16" height="16" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1" fill="none" stroke="currentColor"/><path d="M4 11l3-3 2 2 1.5-1.5L13 11" fill="none" stroke="currentColor"/></svg>',
  properties: '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M4 2h6l2 2v10H4z" fill="none" stroke="currentColor"/><path d="M6 7h4M6 10h4" stroke="currentColor"/></svg>',
  statistics: '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M3 13V8m4 5V4m4 9V6" stroke="currentColor" stroke-width="2"/></svg>',
  flake: '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M8 2v12M3 5l10 6M13 5 3 11" stroke="currentColor"/></svg>',
  render: '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M8 2 2.5 5v6L8 14l5.5-3V5z" fill="none" stroke="currentColor"/><path d="M2.5 5 8 8l5.5-3M8 8v6" stroke="currentColor"/></svg>',
  replace: '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M4 5h8l-2-2M12 11H4l2 2" fill="none" stroke="currentColor"/></svg>',
  ui_debug: '<svg width="16" height="16" viewBox="0 0 16 16"><rect x="3" y="3" width="10" height="10" fill="none" stroke="currentColor"/><path d="M5 6h6M5 9h4" stroke="currentColor"/></svg>',
  options: '<svg width="16" height="16" viewBox="0 0 16 16"><circle cx="8" cy="8" r="2" fill="none" stroke="currentColor"/><path d="M8 2v2m0 8v2M2 8h2m8 0h2M3.8 3.8l1.4 1.4m5.6 5.6 1.4 1.4m0-8.4-1.4 1.4m-5.6 5.6-1.4 1.4" stroke="currentColor"/></svg>',
};

function NavIcon({ name, isMc }: { name: string, isMc: boolean }) {
  const svg = NAV_ICON_SVGS[name] || '<svg width="16" height="16" viewBox="0 0 16 16"><rect width="16" height="16" fill="none" stroke="currentColor" /></svg>';
  const color = isMc ? '#ffffff' : 'currentColor';
  
  return <div style={{ width: 16, height: 16, display: 'flex', alignItems: 'center', justifyContent: 'center', color }} dangerouslySetInnerHTML={{ __html: svg }} />;
}

export function App() {
  const [currentFile, setCurrentFile] = useState<string>("");
  const [theme, setThemeState] = useState(() => localStorage.getItem("theme") === "minecraft" ? "minecraft" : "metro10");
  const [route, setRoute] = useState("home");
  const [expanded, setExpanded] = useState(true);

  useEffect(() => {
    localStorage.setItem("theme", theme);
    document.body.className = `theme-${theme}`;
  }, [theme]);

  const setTheme = (value: string) => {
    setThemeState(value === "minecraft" ? "minecraft" : "metro10");
  };

  useEffect(() => { 
    loadDatabases(); 
    initI18n();
  }, []);

  const isMc = theme === "minecraft";

  const topPages: Record<string, { name: string, icon: string, comp: React.FC<any> }> = {
    home: { name: "主页", icon: "home", comp: HomePage },
    library: { name: "投影库", icon: "gallery", comp: LibraryPage },
    properties: { name: "属性", icon: "properties", comp: PropertiesPage },
    statistics: { name: "统计", icon: "statistics", comp: StatisticsPage },
    flake: { name: "分层", icon: "flake", comp: FlakePage },
    render: { name: "渲染", icon: "render", comp: RenderPage },
    replace: { name: "替换", icon: "replace", comp: ReplacePage },
    generate: { name: "生成", icon: "ui_debug", comp: GeneratePage },
  };


  const bottomPages: Record<string, { name: string, icon: string, comp: React.FC<any> }> = {
    uitest: { name: "UI 测试", icon: "ui_debug", comp: () => <div style={{padding: 12}}>UI 测试 (占位)</div> },
    settings: { name: "选项", icon: "options", comp: SettingsPage },
  };

  const allPages = { ...topPages, ...bottomPages };
  const Page = allPages[route]?.comp || HomePage;

  return (
    <div className="app-container">
      <div className={`sidebar ${expanded ? 'expanded' : 'collapsed'}`}>
        <div className="hamburger-area">
          <div 
            className="btn" 
            style={{ width: 48, height: 48, padding: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', backgroundColor: isMc ? 'transparent' : 'inherit', border: 'none', boxShadow: 'none' }}
            onClick={() => setExpanded(!expanded)} 
            title={expanded ? "收起侧栏" : "展开侧栏"}
          >
            {isMc ? (expanded ? '«' : '»') : (!expanded ? '»' : '«')}
          </div>
        </div>

        <div className="sidebar-item-container" style={{ flex: 1 }}>
          {Object.entries(topPages).map(([k, v]) => {
            const active = route === k;
            return (
              <div 
                key={k} 
                className={`sidebar-item ${active ? 'active' : ''}`} 
                onClick={() => setRoute(k)}
                title={!expanded ? v.name : ''}
              >
                {active && !isMc && <div className="sidebar-indicator" />}
                <div className="sidebar-icon">
                  <NavIcon name={v.icon} isMc={isMc} />
                </div>
                {expanded && (
                  <div className="sidebar-text">
                    {v.name}
                  </div>
                )}
              </div>
            );
          })}
        </div>
        
        <div className="sidebar-item-container" style={{ marginBottom: 12 }}>
          {Object.entries(bottomPages).map(([k, v]) => {
            const active = route === k;
            return (
              <div 
                key={k} 
                className={`sidebar-item ${active ? 'active' : ''}`} 
                onClick={() => setRoute(k)}
                title={!expanded ? v.name : ''}
              >
                {active && !isMc && <div className="sidebar-indicator" />}
                <div className="sidebar-icon">
                  <NavIcon name={v.icon} isMc={isMc} />
                </div>
                {expanded && (
                  <div className="sidebar-text">
                    {v.name}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
      <div className="main-content">
        {/* topbar removed to match screenshot style */}
        <div className="page-content">
          <Page currentFile={currentFile} setCurrentFile={setCurrentFile} setRoute={setRoute} theme={theme} setTheme={setTheme} />
        </div>
      </div>
    </div>
  );
}
