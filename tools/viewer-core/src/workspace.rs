//! Small read models for the workspace. File ownership remains with the library.
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use base64::Engine;
use std::{collections::{BTreeMap, BTreeSet}, path::Path};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecentProject {
    path: String,
    name: String,
    modified_at: u64,
    preview_path: Option<String>,
    preview_url: Option<String>,
}

pub fn recent_projects(directory: &Path) -> Result<Value> {
    let path = directory.join("projection-library/js_library.json");
    if !path.exists() { return Ok(serde_json::json!({"projects": []})); }
    let file = std::fs::File::open(path)?;
    // A library document is metadata, never the schematic or its block arrays.
    anyhow::ensure!(file.metadata()?.len() <= 64 * 1024 * 1024, "Library metadata exceeds the supported document size");
    let library: Value = serde_json::from_reader(std::io::BufReader::new(file)).context("Read library metadata")?;
    let mut projects = recent(&library);
    for project in &mut projects {
        let Some(path) = project.preview_path.as_deref().map(Path::new) else { continue; };
        let path = if path.is_absolute() { path.to_path_buf() } else { directory.join(path) };
        if !std::fs::metadata(&path).is_ok_and(|meta| meta.len() <= 2 * 1024 * 1024) { continue; }
        let Ok(bytes) = std::fs::read(path) else { continue; };
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { "image/png" }
            else if bytes.starts_with(b"\xff\xd8\xff") { "image/jpeg" } else { continue; };
        project.preview_url = Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)));
    }
    Ok(serde_json::json!({"projects": projects}))
}

fn recent(library: &Value) -> Vec<RecentProject> {
    let array = |key: &str| library[key].as_array().cloned().unwrap_or_default();
    let entries = array("entries");
    let replicas = array("replicas");
    let redirects: BTreeMap<_, _> = array("redirects").into_iter()
        .filter_map(|value| Some((value["fromId"].as_str()?.to_owned(), value["toId"].as_str()?.to_owned()))).collect();
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    for reference in array("recent") {
        let mut id = reference["entryId"].as_str().unwrap_or_default().to_owned();
        let mut visited = BTreeSet::new();
        while visited.insert(id.clone()) {
            let Some(next) = redirects.get(&id) else { break; };
            id = next.clone();
        }
        let entry = entries.iter().find(|entry| entry["id"].as_str() == Some(&id));
        let replica = replicas.iter().find(|replica| replica["entryId"].as_str() == Some(&id)
            && replica["status"].as_str() == Some("active"));
        let path = reference["path"].as_str().or_else(|| replica.and_then(|item| item["path"].as_str()));
        let Some(path) = path.filter(|path| !path.is_empty()) else { continue; };
        let key = if cfg!(windows) { path.replace('\\', "/").to_lowercase() } else { path.to_owned() };
        if !seen.insert(key) { continue; }
        let name = entry.and_then(|entry| entry["displayName"].as_str()).filter(|name| !name.is_empty())
            .map(str::to_owned).unwrap_or_else(|| path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned());
        output.push(RecentProject {
            path: path.into(), name,
            modified_at: replica.and_then(|item| item["mtime"].as_u64()).unwrap_or_default(),
            preview_path: entry.and_then(|entry| entry["preview"]["path"].as_str()).map(str::to_owned),
            preview_url: None,
        });
        if output.len() == 6 { break; }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_metadata_preserves_order_resolves_identity_and_limits_rows() {
        let library = serde_json::json!({"entries":[{"id":"new","displayName":"Project"}],
            "redirects":[{"fromId":"old","toId":"new"}],
            "replicas":[{"entryId":"new","path":"/project.nbt","status":"active","mtime":123}],
            "recent":[{"kind":"entry","entryId":"old"},{"kind":"path","path":"/project.nbt"}]});
        let rows = recent(&library);
        assert_eq!(rows.len(), 1); assert_eq!(rows[0].name, "Project");
        assert_eq!(rows[0].modified_at, 123);
    }
}
