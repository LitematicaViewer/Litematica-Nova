use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

pub const LBA_DATA_ROOT_ENV: &str = "LBA_DATA_ROOT";

#[derive(Debug, Clone, Serialize)]
pub struct RuntimePathEntry {
    pub path: PathBuf,
    pub exists: bool,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimePaths {
    pub app_root: PathBuf,
    pub data_root: RuntimePathEntry,
    pub cache_root: RuntimePathEntry,
    pub recipe_cache_root: RuntimePathEntry,
    pub item_icons_cache_root: RuntimePathEntry,
    pub item_names_cache_root: RuntimePathEntry,
    pub manifests_cache_root: RuntimePathEntry,
    pub stockpile_root: RuntimePathEntry,
    pub stockpile_exports_root: RuntimePathEntry,
    pub stockpile_sessions_root: RuntimePathEntry,
    pub stockpile_projects_root: RuntimePathEntry,
    pub stockpile_tmp_root: RuntimePathEntry,
    pub data_root_source: String,
}

pub fn app_root() -> Result<PathBuf> {
    locate_app_root()
}

pub fn data_root() -> Result<PathBuf> {
    data_root_with_source().map(|(path, _source)| path)
}

pub fn cache_root() -> Result<PathBuf> {
    Ok(data_root()?.join("cache"))
}

pub fn recipe_cache_root() -> Result<PathBuf> {
    Ok(cache_root()?.join("recipes"))
}

pub fn item_icons_cache_root() -> Result<PathBuf> {
    Ok(cache_root()?.join("item-icons"))
}

pub fn item_names_cache_root() -> Result<PathBuf> {
    Ok(cache_root()?.join("item-names"))
}

pub fn manifests_cache_root() -> Result<PathBuf> {
    Ok(cache_root()?.join("manifests"))
}

pub fn stockpile_root() -> Result<PathBuf> {
    Ok(data_root()?.join("stockpile"))
}

pub fn stockpile_exports_root() -> Result<PathBuf> {
    Ok(stockpile_root()?.join("exports"))
}

pub fn stockpile_sessions_root() -> Result<PathBuf> {
    Ok(stockpile_root()?.join("sessions"))
}

pub fn stockpile_projects_root() -> Result<PathBuf> {
    Ok(stockpile_root()?.join("projects"))
}

pub fn stockpile_tmp_root() -> Result<PathBuf> {
    Ok(stockpile_root()?.join("tmp"))
}

pub fn ensure_runtime_layout() -> Result<RuntimePaths> {
    let app_root = app_root()?;
    let (data_root, data_root_source) = data_root_with_source()?;
    let cache_root = data_root.join("cache");
    let recipe_cache_root = cache_root.join("recipes");
    let item_icons_cache_root = cache_root.join("item-icons");
    let item_names_cache_root = cache_root.join("item-names");
    let manifests_cache_root = cache_root.join("manifests");
    let stockpile_root = data_root.join("stockpile");
    let stockpile_exports_root = stockpile_root.join("exports");
    let stockpile_sessions_root = stockpile_root.join("sessions");
    let stockpile_projects_root = stockpile_root.join("projects");
    let stockpile_tmp_root = stockpile_root.join("tmp");

    for path in [
        &data_root,
        &cache_root,
        &recipe_cache_root,
        &item_icons_cache_root,
        &item_names_cache_root,
        &manifests_cache_root,
        &stockpile_root,
        &stockpile_exports_root,
        &stockpile_sessions_root,
        &stockpile_projects_root,
        &stockpile_tmp_root,
    ] {
        fs::create_dir_all(path)
            .with_context(|| format!("create runtime directory failed: {}", path.display()))?;
    }

    Ok(RuntimePaths {
        app_root,
        data_root: path_entry(data_root),
        cache_root: path_entry(cache_root),
        recipe_cache_root: path_entry(recipe_cache_root),
        item_icons_cache_root: path_entry(item_icons_cache_root),
        item_names_cache_root: path_entry(item_names_cache_root),
        manifests_cache_root: path_entry(manifests_cache_root),
        stockpile_root: path_entry(stockpile_root),
        stockpile_exports_root: path_entry(stockpile_exports_root),
        stockpile_sessions_root: path_entry(stockpile_sessions_root),
        stockpile_projects_root: path_entry(stockpile_projects_root),
        stockpile_tmp_root: path_entry(stockpile_tmp_root),
        data_root_source,
    })
}

fn data_root_with_source() -> Result<(PathBuf, String)> {
    if let Some(path) = env::var_os(LBA_DATA_ROOT_ENV).filter(|value| !value.is_empty()) {
        let path = absolutize(PathBuf::from(path))?;
        return Ok((path, LBA_DATA_ROOT_ENV.to_string()));
    }

    Ok((app_root()?.join("data"), "app_root/data".to_string()))
}

fn locate_app_root() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir);
    }
    if let Ok(current_exe) = env::current_exe() {
        candidates.push(current_exe);
    }

    for candidate in candidates {
        for dir in candidate.ancestors() {
            if is_app_root(dir) {
                return Ok(dir.to_path_buf());
            }
        }
    }

    if let Ok(current_exe) = env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        return Ok(parent.to_path_buf());
    }

    bail!("failed to locate app root from current executable or current directory")
}

fn is_app_root(path: &Path) -> bool {
    path.join("data").is_dir()
        || path.join("bin").join("viewer-backend").is_dir()
        || path
            .join("tools")
            .join("viewer-core")
            .join("Cargo.toml")
            .is_file()
}

fn absolutize(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(env::current_dir()
        .context("get current directory failed")?
        .join(path))
}

fn path_entry(path: PathBuf) -> RuntimePathEntry {
    RuntimePathEntry {
        exists: path.exists(),
        is_dir: path.is_dir(),
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_runtime_paths_are_under_data() {
        let data = data_root().expect("data root");
        assert_eq!(cache_root().expect("cache root"), data.join("cache"));
        assert_eq!(
            recipe_cache_root().expect("recipe cache root"),
            data.join("cache").join("recipes")
        );
        assert_eq!(
            item_names_cache_root().expect("item names cache root"),
            data.join("cache").join("item-names")
        );
        assert_eq!(
            stockpile_root().expect("stockpile root"),
            data.join("stockpile")
        );
    }

    #[test]
    fn ensure_runtime_layout_creates_expected_runtime_dirs() {
        let paths = ensure_runtime_layout().expect("runtime layout");
        assert!(paths.data_root.is_dir);
        assert!(paths.cache_root.is_dir);
        assert!(paths.recipe_cache_root.is_dir);
        assert!(paths.item_icons_cache_root.is_dir);
        assert!(paths.item_names_cache_root.is_dir);
        assert!(paths.manifests_cache_root.is_dir);
        assert!(paths.stockpile_root.is_dir);
        assert!(paths.stockpile_exports_root.is_dir);
        assert!(paths.stockpile_sessions_root.is_dir);
        assert!(paths.stockpile_projects_root.is_dir);
        assert!(paths.stockpile_tmp_root.is_dir);
    }
}
