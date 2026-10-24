import {
  readProjectMarkers, editProjectOrigin,
} from "../platform/applicationOperations";
export interface ProjectionMarkerPoint {
  x: number;
  y: number;
  z: number;
}

export interface ProjectionMarkers {
  origin: ProjectionMarkerPoint | null;
}

export interface ProjectionMarkerEditSummary {
  input: string;
  output: string;
  backup_path: string | null;
  origin: ProjectionMarkerPoint | null;
}

export const loadProjectionMarkers = (sourcePath: string): Promise<ProjectionMarkers> =>
  readProjectMarkers({ sourcePath });
export const setProjectionOrigin = (sourcePath: string, origin: ProjectionMarkerPoint, outputPath?: string): Promise<ProjectionMarkerEditSummary> =>
  editProjectOrigin({ sourcePath, origin, outputPath: outputPath ?? null });
export const removeProjectionOrigin = (sourcePath: string, outputPath?: string): Promise<ProjectionMarkerEditSummary> =>
  editProjectOrigin({ sourcePath, origin: null, outputPath: outputPath ?? null });
