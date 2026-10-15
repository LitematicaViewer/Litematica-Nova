use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use viewer_launch_contract::{
    IrisFallback, ViewerBackend, ViewerContent, ViewerRenderer, ViewerShaderpack,
    WeatherPrecipitation,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct V5CompatibilityConfig {
    #[serde(default = "default_backend")]
    pub(crate) backend: String,
    #[serde(default)]
    pub(crate) shaderpack_root: String,
    #[serde(default)]
    pub(crate) shaderpack_name: String,
    #[serde(default = "default_iris_fallback")]
    pub(crate) iris_fallback: String,
    #[serde(default)]
    pub(crate) fabric_mods_dir: String,
    #[serde(default)]
    pub(crate) resource_packs: Vec<String>,
}

impl Default for V5CompatibilityConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            shaderpack_root: String::new(),
            shaderpack_name: String::new(),
            iris_fallback: default_iris_fallback(),
            fabric_mods_dir: String::new(),
            resource_packs: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct V5CompatibilityConfigInput {
    backend: Option<String>,
    shaderpack_root: Option<String>,
    shaderpack_name: Option<String>,
    iris_fallback: Option<String>,
    fabric_mods_dir: Option<String>,
    resource_packs: Option<Vec<String>>,
}

impl V5CompatibilityConfig {
    pub(crate) fn apply_input(&mut self, input: V5CompatibilityConfigInput) -> Result<(), String> {
        if let Some(value) = input.backend {
            self.backend = value;
        }
        if let Some(value) = input.shaderpack_root {
            self.shaderpack_root = value;
        }
        if let Some(value) = input.shaderpack_name {
            self.shaderpack_name = value;
        }
        if let Some(value) = input.iris_fallback {
            self.iris_fallback = value;
        }
        if let Some(value) = input.fabric_mods_dir {
            self.fabric_mods_dir = value;
        }
        if let Some(value) = input.resource_packs {
            self.resource_packs = value;
        }
        *self = self.normalized()?;
        Ok(())
    }

    /// Inputs handled exclusively by the native V5 pipeline must never be
    /// launched through the legacy normal/fast render paths.
    pub(crate) fn requires_full_mode(&self) -> bool {
        let backend = self.backend.trim().to_ascii_lowercase();
        (!backend.is_empty() && backend != "auto")
            || !self.shaderpack_root.trim().is_empty()
            || !self.shaderpack_name.trim().is_empty()
            || !self.fabric_mods_dir.trim().is_empty()
            || self
                .resource_packs
                .iter()
                .any(|value| !value.trim().is_empty())
    }

    pub(crate) fn normalized(&self) -> Result<Self, String> {
        let backend = match self.backend.trim().to_ascii_lowercase().as_str() {
            "auto" => "auto",
            "vulkan" => "vulkan",
            "iris-gl46" => "iris-gl46",
            value => {
                return Err(format!(
                    "V5 backend {value:?} 无效；必须是 auto、vulkan 或 iris-gl46"
                ));
            }
        }
        .to_owned();
        let iris_fallback = match self.iris_fallback.trim().to_ascii_lowercase().as_str() {
            "fail" => "fail",
            "vulkan" => "vulkan",
            value => {
                return Err(format!(
                    "Iris fallback {value:?} 无效；必须是 fail 或 vulkan"
                ));
            }
        }
        .to_owned();
        let shaderpack_root = self.shaderpack_root.trim().to_owned();
        let shaderpack_name = self.shaderpack_name.trim().to_owned();
        let fabric_mods_dir = self.fabric_mods_dir.trim().to_owned();
        let mut seen = HashSet::new();
        let resource_packs = self
            .resource_packs
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .filter(|value| seen.insert((*value).to_owned()))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        let root_present = !shaderpack_root.is_empty();
        let name_present = !shaderpack_name.is_empty();
        if root_present != name_present {
            return Err("shaderpack root 和 name 必须同时填写或同时留空".to_owned());
        }
        if name_present
            && (shaderpack_name == "."
                || shaderpack_name == ".."
                || shaderpack_name.contains('/')
                || shaderpack_name.contains('\\'))
        {
            return Err("shaderpack name 必须是 root 下的单个文件或目录名".to_owned());
        }
        if backend == "vulkan" && root_present {
            return Err("Vulkan backend 不能应用 shaderpack；请选择 auto 或 iris-gl46".to_owned());
        }
        if backend == "iris-gl46" && !root_present {
            return Err("iris-gl46 backend 必须同时配置 shaderpack root 和 name".to_owned());
        }

        Ok(Self {
            backend,
            shaderpack_root,
            shaderpack_name,
            iris_fallback,
            fabric_mods_dir,
            resource_packs,
        })
    }
}

fn default_backend() -> String {
    "auto".to_owned()
}

fn default_iris_fallback() -> String {
    "fail".to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompatibilityScope {
    SceneCache,
    Render,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ViewerCompatibilityPlan {
    content: ViewerContent,
    renderer: Option<ViewerRenderer>,
    fingerprint: String,
}

/// Resolved launch inputs without a content fingerprint.
///
/// Nova's UI process may use this lightweight value to forward the user's
/// configured paths to `nova-render-service`. Content hashing and recursive
/// identity work belong to that service, not to the Tauri/WebView process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ViewerCompatibilityInputs {
    content: ViewerContent,
    renderer: Option<ViewerRenderer>,
}

impl ViewerCompatibilityInputs {
    pub(crate) fn resolve(
        config: &V5CompatibilityConfig,
        workspace_root: &Path,
        scope: CompatibilityScope,
    ) -> Result<Self, String> {
        let (content, renderer, _) = resolve_compatibility_inputs(config, workspace_root, scope)?;
        Ok(Self { content, renderer })
    }

    pub(crate) const fn content(&self) -> &ViewerContent {
        &self.content
    }

    pub(crate) const fn renderer(&self) -> Option<&ViewerRenderer> {
        self.renderer.as_ref()
    }
}

impl ViewerCompatibilityPlan {
    pub(crate) fn build(
        config: &V5CompatibilityConfig,
        workspace_root: &Path,
        scope: CompatibilityScope,
    ) -> Result<Self, String> {
        let (content, renderer, identity_paths) =
            resolve_compatibility_inputs(config, workspace_root, scope)?;

        let fingerprint =
            compatibility_fingerprint(scope, &content, renderer.as_ref(), &identity_paths)?;
        Ok(Self {
            content,
            renderer,
            fingerprint,
        })
    }

    pub(crate) fn content(&self) -> &ViewerContent {
        &self.content
    }

    pub(crate) fn renderer(&self) -> Option<&ViewerRenderer> {
        self.renderer.as_ref()
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

fn resolve_compatibility_inputs(
    config: &V5CompatibilityConfig,
    workspace_root: &Path,
    scope: CompatibilityScope,
) -> Result<(ViewerContent, Option<ViewerRenderer>, Vec<PathBuf>), String> {
    let config = config.normalized()?;
    let mut identity_paths = Vec::new();
    let mut fabric_mods_dir = None;
    let mut resource_packs = Vec::new();

    if !config.fabric_mods_dir.is_empty() {
        let path = resolve_path(workspace_root, &config.fabric_mods_dir);
        if !path.is_dir() {
            return Err(format!(
                "Fabric mods 目录不存在或不是目录：{}",
                path.display()
            ));
        }
        identity_paths.push(path.clone());
        fabric_mods_dir = Some(path);
    }
    for configured in &config.resource_packs {
        let path = resolve_path(workspace_root, configured);
        if !path.is_file() && !path.is_dir() {
            return Err(format!("资源包不存在或不是文件/目录：{}", path.display()));
        }
        identity_paths.push(path.clone());
        resource_packs.push(path);
    }
    let content = ViewerContent {
        fabric_mods_dir,
        resource_packs,
    };

    let renderer = if scope == CompatibilityScope::Render {
        let backend = match config.backend.as_str() {
            "auto" => ViewerBackend::Auto,
            "vulkan" => ViewerBackend::Vulkan,
            "iris-gl46" => ViewerBackend::IrisGl46,
            _ => unreachable!("normalized V5 backend"),
        };
        let iris_fallback = match config.iris_fallback.as_str() {
            "fail" => IrisFallback::Fail,
            "vulkan" => IrisFallback::Vulkan,
            _ => unreachable!("normalized Iris fallback"),
        };
        let mut shaderpack = None;
        if !config.shaderpack_root.is_empty() {
            let root = resolve_path(workspace_root, &config.shaderpack_root);
            if !root.is_dir() {
                return Err(format!(
                    "shaderpack root 不存在或不是目录：{}",
                    root.display()
                ));
            }
            let selected = root.join(&config.shaderpack_name);
            if !selected.is_file() && !selected.is_dir() {
                return Err(format!(
                    "shaderpack 在 root 中不存在：{}",
                    selected.display()
                ));
            }
            identity_paths.push(selected.clone());
            shaderpack = Some(ViewerShaderpack { path: selected });
        }
        Some(ViewerRenderer {
            backend,
            iris_fallback,
            shaderpack,
            vulkan_adapter: None,
            precipitation: WeatherPrecipitation::Auto,
        })
    } else {
        None
    };

    Ok((content, renderer, identity_paths))
}

fn resolve_path(workspace_root: &Path, configured: &str) -> PathBuf {
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    }
}

fn compatibility_fingerprint(
    scope: CompatibilityScope,
    content: &ViewerContent,
    renderer: Option<&ViewerRenderer>,
    identity_paths: &[PathBuf],
) -> Result<String, String> {
    let mut digest = Sha256::new();
    digest.update(b"lba-v5-compatibility-v3\0");
    digest.update(match scope {
        CompatibilityScope::SceneCache => b"scene-cache\0".as_slice(),
        CompatibilityScope::Render => b"render\0".as_slice(),
    });
    let settings = serde_json::to_vec(&(content, renderer))
        .map_err(|error| format!("序列化 Viewer 兼容配置失败：{error}"))?;
    digest.update((settings.len() as u64).to_le_bytes());
    digest.update(settings);
    for path in identity_paths {
        update_path_identity(&mut digest, path)?;
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn update_path_identity(digest: &mut Sha256, path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("读取兼容资源元数据失败 {}：{error}", path.display()))?;
    let path_text = path.to_string_lossy();
    digest.update((path_text.len() as u64).to_le_bytes());
    digest.update(path_text.as_bytes());
    digest.update(metadata.len().to_le_bytes());
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    digest.update(modified_nanos.to_le_bytes());

    let file_type = metadata.file_type();
    digest.update([file_type.is_file() as u8, file_type.is_dir() as u8]);
    if !file_type.is_dir() {
        return Ok(());
    }

    let mut entries = fs::read_dir(path)
        .map_err(|error| format!("读取兼容资源目录失败 {}：{error}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("遍历兼容资源目录失败 {}：{error}", path.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        update_path_identity(digest, &entry.path())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempFixture {
        root: PathBuf,
    }

    impl TempFixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "lba-desktop-v5-compat-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("mods")).unwrap();
            fs::create_dir_all(root.join("shaderpacks")).unwrap();
            fs::create_dir_all(root.join("resourcepacks")).unwrap();
            fs::write(root.join("shaderpacks/pack.zip"), b"shader").unwrap();
            fs::write(root.join("resourcepacks/resources.zip"), b"resources").unwrap();
            Self { root }
        }
    }

    impl Drop for TempFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn default_is_valid_and_selects_typed_auto_without_machine_paths() {
        let config = V5CompatibilityConfig::default();
        assert_eq!(config.backend, "auto");
        assert_eq!(config.iris_fallback, "fail");
        assert!(config.shaderpack_root.is_empty());
        assert!(config.fabric_mods_dir.is_empty());
        assert!(config.resource_packs.is_empty());
        let plan = ViewerCompatibilityPlan::build(
            &config,
            Path::new("unused"),
            CompatibilityScope::Render,
        )
        .unwrap();
        assert_eq!(plan.content(), &ViewerContent::default());
        let renderer = plan.renderer().unwrap();
        assert_eq!(renderer.backend, ViewerBackend::Auto);
        assert_eq!(renderer.iris_fallback, IrisFallback::Fail);
        assert!(renderer.shaderpack.is_none());
    }

    #[test]
    fn scene_cache_only_receives_fabric_and_resource_inputs() {
        let fixture = TempFixture::new();
        let config = V5CompatibilityConfig {
            backend: "iris-gl46".to_owned(),
            shaderpack_root: "shaderpacks".to_owned(),
            shaderpack_name: "pack.zip".to_owned(),
            iris_fallback: "vulkan".to_owned(),
            fabric_mods_dir: "mods".to_owned(),
            resource_packs: vec!["resourcepacks/resources.zip".to_owned()],
        };
        let plan =
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::SceneCache)
                .unwrap();
        assert!(plan.content().fabric_mods_dir.is_some());
        assert_eq!(plan.content().resource_packs.len(), 1);
        assert!(plan.renderer().is_none());
    }

    #[test]
    fn shaderpack_changes_do_not_invalidate_the_scene_cache_layer() {
        let fixture = TempFixture::new();
        let mut config = V5CompatibilityConfig {
            backend: "auto".to_owned(),
            shaderpack_root: "shaderpacks".to_owned(),
            shaderpack_name: "pack.zip".to_owned(),
            iris_fallback: "fail".to_owned(),
            fabric_mods_dir: "mods".to_owned(),
            resource_packs: vec!["resourcepacks/resources.zip".to_owned()],
        };
        let first =
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::SceneCache)
                .unwrap();

        config.backend = "iris-gl46".to_owned();
        config.iris_fallback = "vulkan".to_owned();
        let second =
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::SceneCache)
                .unwrap();

        assert_eq!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn resource_pack_file_changes_invalidate_the_scene_cache_layer() {
        let fixture = TempFixture::new();
        let config = V5CompatibilityConfig {
            backend: "auto".to_owned(),
            shaderpack_root: String::new(),
            shaderpack_name: String::new(),
            iris_fallback: "fail".to_owned(),
            fabric_mods_dir: String::new(),
            resource_packs: vec!["resourcepacks/resources.zip".to_owned()],
        };
        let first =
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::SceneCache)
                .unwrap();

        fs::write(
            fixture.root.join("resourcepacks/resources.zip"),
            b"changed resources",
        )
        .unwrap();
        let second =
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::SceneCache)
                .unwrap();

        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn render_plan_validates_and_fingerprints_the_full_compatibility_identity() {
        let fixture = TempFixture::new();
        let mut config = V5CompatibilityConfig {
            backend: "iris-gl46".to_owned(),
            shaderpack_root: "shaderpacks".to_owned(),
            shaderpack_name: "pack.zip".to_owned(),
            iris_fallback: "vulkan".to_owned(),
            fabric_mods_dir: "mods".to_owned(),
            resource_packs: vec!["resourcepacks/resources.zip".to_owned()],
        };
        let first =
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::Render)
                .unwrap();
        let renderer = first.renderer().unwrap();
        assert_eq!(renderer.backend, ViewerBackend::IrisGl46);
        assert_eq!(
            renderer
                .shaderpack
                .as_ref()
                .and_then(|shaderpack| shaderpack.path.file_name())
                .and_then(|name| name.to_str()),
            Some("pack.zip")
        );

        config.iris_fallback = "fail".to_owned();
        let second =
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::Render)
                .unwrap();
        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn incomplete_or_contradictory_shaderpack_settings_are_rejected() {
        let fixture = TempFixture::new();
        let mut config = V5CompatibilityConfig {
            backend: "iris-gl46".to_owned(),
            ..V5CompatibilityConfig::default()
        };
        assert!(
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::Render)
                .unwrap_err()
                .contains("必须同时配置")
        );

        config.backend = "vulkan".to_owned();
        config.shaderpack_root = "shaderpacks".to_owned();
        config.shaderpack_name = "pack.zip".to_owned();
        assert!(
            ViewerCompatibilityPlan::build(&config, &fixture.root, CompatibilityScope::Render)
                .unwrap_err()
                .contains("不能应用 shaderpack")
        );
    }

    #[test]
    fn v5_only_inputs_force_the_full_render_path() {
        assert!(!V5CompatibilityConfig::default().requires_full_mode());

        for config in [
            V5CompatibilityConfig {
                backend: "iris-gl46".to_owned(),
                ..V5CompatibilityConfig::default()
            },
            V5CompatibilityConfig {
                shaderpack_root: "shaderpacks".to_owned(),
                ..V5CompatibilityConfig::default()
            },
            V5CompatibilityConfig {
                fabric_mods_dir: "mods".to_owned(),
                ..V5CompatibilityConfig::default()
            },
            V5CompatibilityConfig {
                resource_packs: vec!["resources.zip".to_owned()],
                ..V5CompatibilityConfig::default()
            },
        ] {
            assert!(config.requires_full_mode(), "config={config:?}");
        }
    }
}
