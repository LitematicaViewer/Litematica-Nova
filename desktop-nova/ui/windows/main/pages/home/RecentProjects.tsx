import React, { useEffect, useRef, useState } from "react";
import { invoke } from "../../../../../src/platform/tauri";
import { getPathInfo } from "../../../../../src/business/facade";
import { useI18n } from "../../../../i18n";
import { RecentProjectRow } from "./RecentProjectRow";

interface RecentProject { path: string; name: string; modifiedAt: number; previewUrl?: string }
export function RecentProjects({ onOpen }: { onOpen: (path: string) => void }) {
  const { locale } = useI18n();
  const [projects, setProjects] = useState<RecentProject[]>([]);
  const [error, setError] = useState("");
  const [opening, setOpening] = useState("");
  const openingRef = useRef(false);
  const open = async (path: string) => {
    if (openingRef.current) return;
    openingRef.current = true; setOpening(path); setError("");
    try {
      const info = await getPathInfo(path);
      if (!info.exists || info.is_dir) throw new Error(locale === "en-US" ? "This file was moved or deleted. Open its new location or choose another project." : "该文件已移动或删除。请从新位置打开，或选择其他项目。");
      onOpen(path);
    } catch (reason) { setError(String(reason)); }
    finally { openingRef.current = false; setOpening(""); }
  };
  useEffect(() => {
    let active = true;
    void invoke<{ projects: RecentProject[] }>("application_recent_projects")
      .then(result => { if (active) setProjects(result.projects); })
      .catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, []);
  return <section className="home-recent">
    <h2>{locale === "en-US" ? "Recent projects" : "最近项目"}</h2>
    {error && <p role="alert">{error}</p>}
    {!error && !projects.length && <p className="muted">{locale === "en-US" ? "Open a schematic to start working." : "打开投影，开始工作。"}</p>}
    <div className="home-recent-list">{projects.map(project => <RecentProjectRow
      key={project.path}
      name={project.name}
      date={project.modifiedAt ? new Date(project.modifiedAt).toLocaleDateString(locale) : ""}
      previewUrl={project.previewUrl}
      title={project.path}
      busy={opening === project.path}
      disabled={!!opening}
      onOpen={() => void open(project.path)}
    />)}</div>
  </section>;
}
