import React, { useEffect, useState } from "react";
import { FileText } from "lucide-react";
import { invoke } from "../../../../../src/platform/tauri";
import { Button } from "../../../../components/ui/button";
import { useI18n } from "../../../../i18n";

interface RecentProject { path: string; name: string; modifiedAt: number; previewUrl?: string }
export function RecentProjects({ onOpen }: { onOpen: (path: string) => void }) {
  const { locale } = useI18n();
  const [projects, setProjects] = useState<RecentProject[]>([]);
  const [error, setError] = useState("");
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
    <div className="home-recent-list">{projects.map(project => <Button variant="ghost" key={project.path} title={project.path} onClick={() => onOpen(project.path)}>
      {project.previewUrl ? <img src={project.previewUrl} alt="" /> : <FileText size={18} aria-hidden="true" />}<span>{project.name}</span>
      <small>{project.modifiedAt ? new Date(project.modifiedAt).toLocaleDateString(locale) : ""}</small>
    </Button>)}</div>
  </section>;
}
