use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerBuildMode {
    Normal,
    FastExperimental,
}

static BUILD_MODE: OnceLock<ViewerBuildMode> = OnceLock::new();
static WRITER_PIPELINE_DISABLED: OnceLock<bool> = OnceLock::new();
static HEADLESS_PREBUILD_DISABLED: OnceLock<bool> = OnceLock::new();

fn env_flag_enabled(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

pub fn viewer_build_mode() -> ViewerBuildMode {
    *BUILD_MODE.get_or_init(|| {
        match std::env::var("LBA_VIEWER_BUILD_MODE")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("fast" | "fast_experimental" | "experimental") => {
                ViewerBuildMode::FastExperimental
            }
            _ => ViewerBuildMode::Normal,
        }
    })
}

pub fn compact_cache_v2_enabled() -> bool {
    env_flag_enabled("LBA_ENABLE_COMPACT_CACHE_V2")
        || matches!(viewer_build_mode(), ViewerBuildMode::FastExperimental)
}

pub fn fast_path_enabled() -> bool {
    !env_flag_enabled("LBA_DISABLE_FAST_PATH")
}

pub fn fast_path_non_occluding_enabled() -> bool {
    env_flag_enabled("LBA_ENABLE_FAST_PATH_NON_OCCLUDING")
        || matches!(viewer_build_mode(), ViewerBuildMode::FastExperimental)
}

pub fn fast_path_half_slab_enabled() -> bool {
    env_flag_enabled("LBA_ENABLE_FAST_PATH_HALF_SLAB")
        || matches!(viewer_build_mode(), ViewerBuildMode::FastExperimental)
}

pub fn fast_path_stair_half_enabled() -> bool {
    env_flag_enabled("LBA_ENABLE_FAST_PATH_STAIR_HALF")
        || matches!(viewer_build_mode(), ViewerBuildMode::FastExperimental)
}

pub fn fast_path_carpet_enabled() -> bool {
    env_flag_enabled("LBA_ENABLE_FAST_PATH_CARPET")
        || matches!(viewer_build_mode(), ViewerBuildMode::FastExperimental)
}

pub fn writer_pipeline_enabled() -> bool {
    !*WRITER_PIPELINE_DISABLED.get_or_init(|| env_flag_enabled("LBA_DISABLE_WRITER_PIPELINE"))
}

pub fn headless_prebuild_enabled() -> bool {
    !*HEADLESS_PREBUILD_DISABLED.get_or_init(|| env_flag_enabled("LBA_DISABLE_HEADLESS_PREBUILD"))
}
