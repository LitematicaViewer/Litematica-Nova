import { invoke } from "./tauri";

export interface PathInfo {
  raw: string;
  normalized: string;
  parent_dir: string;
  parent_exists: boolean;
  exists: boolean;
  is_dir: boolean;
  is_file: boolean;
  is_absolute: boolean;
  has_litematic_ext: boolean;
}

export interface ProjectionPreviewImageOutput {
  width: number;
  height: number;
  data_url: string;
}

export interface CopyFileToDirectoryOutput {
  target_path: string;
  overwritten: boolean;
  bytes_copied: number;
}

export function readWorkspaceFile(path: string): Promise<string> {
  return invoke("read_file_string", { path });
}

export function writeWorkspaceFile(path: string, content: string): Promise<void> {
  return invoke("write_file_string", { path, content });
}

export function writeTextFileAbsolute(path: string, content: string): Promise<void> {
  return invoke("write_text_file_absolute", { path, content });
}

export function readAppDataFile(relativePath: string): Promise<string> {
  return invoke("read_user_config_file", { relativePath });
}

export function writeAppDataFile(relativePath: string, content: string): Promise<void> {
  return invoke("write_user_config_file", { relativePath, content });
}

export function getAppDataFilePath(relativePath: string): Promise<string> {
  return invoke("get_user_config_file_path", { relativePath });
}

export function checkFileExists(path: string): Promise<boolean> {
  return invoke("check_file_exists", { path });
}

export function listLitematicFilesInDirectory(path: string, recursive = true): Promise<string[]> {
  return invoke("list_litematic_files_in_directory", { path, recursive });
}

export function getWorkspaceRoot(): Promise<string> {
  return invoke("get_workspace_root");
}

export function getPathInfo(path: string): Promise<PathInfo> {
  return invoke("get_path_info", { path });
}

export function openWorkspacePath(path: string): Promise<void> {
  return invoke("open_workspace_path", { path });
}

export function openFileParentDir(filePath: string): Promise<void> {
  return invoke("open_file_parent_dir", { filePath });
}

export function copyFileToDirectory(sourcePath: string, targetDirectory: string, targetFileName: string, overwrite = false): Promise<CopyFileToDirectoryOutput> {
  return invoke("copy_file_to_directory", { sourcePath, targetDirectory, targetFileName, overwrite });
}

export function readImageBase64(path: string): Promise<string> {
  return invoke("read_image_base64", { path });
}

export function readProjectionPreviewImage(path: string): Promise<ProjectionPreviewImageOutput | null> {
  return invoke("read_projection_preview_image", { filePath: path });
}

export function cleanupLocalTempFiles(): Promise<string> {
  return invoke("cleanup_local_temp_files");
}