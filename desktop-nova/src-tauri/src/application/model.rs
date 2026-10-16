use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Stable project reference understood by the application layer.
///
/// It intentionally identifies the user's source without claiming that a
/// render artifact exists or is valid for it.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ProjectSource {
    pub(crate) path: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RenderDisplayMode {
    Normal,
    Standard,
    FastExperimental,
    #[default]
    Full,
}

impl RenderDisplayMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Standard => "standard",
            Self::FastExperimental => "fast_experimental",
            Self::Full => "full",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RenderBuildMode {
    #[default]
    Prebuilt,
    Realtime,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum LayerSelection {
    #[default]
    All,
    Single {
        world_y: i32,
    },
    Range {
        min_world_y: i32,
        max_world_y: i32,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HighlightRules {
    pub(crate) enabled: bool,
    pub(crate) block_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GraphicsBackendPreference {
    #[default]
    Auto,
    Vulkan,
    IrisGl46,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IrisFallbackPreference {
    #[default]
    Fail,
    Vulkan,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WeatherPreference {
    #[default]
    Auto,
    Rain,
    Snow,
    None,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ShaderSelection {
    pub(crate) path: Option<PathBuf>,
}

/// Persistent, user-visible choices from which the RenderFacade derives the
/// renderer-facing request. No runtime artifact/GPU state belongs here.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ProjectRenderSettings {
    pub(crate) display_mode: RenderDisplayMode,
    pub(crate) build_mode: RenderBuildMode,
    pub(crate) layers: LayerSelection,
    pub(crate) highlights: HighlightRules,
    pub(crate) backend: GraphicsBackendPreference,
    pub(crate) shader: ShaderSelection,
    pub(crate) iris_fallback: IrisFallbackPreference,
    pub(crate) precipitation: WeatherPreference,
    pub(crate) allow_time_modification: bool,
    pub(crate) allow_weather_modification: bool,
    pub(crate) fabric_mods_dir: Option<PathBuf>,
    pub(crate) resource_packs: Vec<PathBuf>,
}

impl Default for ProjectRenderSettings {
    fn default() -> Self {
        Self {
            display_mode: RenderDisplayMode::default(),
            build_mode: RenderBuildMode::default(),
            layers: LayerSelection::default(),
            highlights: HighlightRules::default(),
            backend: GraphicsBackendPreference::default(),
            shader: ShaderSelection::default(),
            iris_fallback: IrisFallbackPreference::default(),
            precipitation: WeatherPreference::default(),
            allow_time_modification: true,
            allow_weather_modification: true,
            fabric_mods_dir: None,
            resource_packs: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PresentationMode {
    #[default]
    Hidden,
    Popup,
    Overlay,
    Embedded,
}

/// Physical screen placement is presentation state, never Viewer runtime
/// ownership. DPI remains explicit so the application does not reinterpret a
/// CSS rectangle as device pixels.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct PresentationPlacement {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) dpi_scale: f64,
    pub(crate) focused: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct PresentationIntent {
    pub(crate) mode: PresentationMode,
    pub(crate) visible: bool,
    pub(crate) placement: Option<PresentationPlacement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) presentation_epoch: Option<u64>,
}

/// Observed presentation state reported by the Viewer after a successful
/// present. This is deliberately separate from the user's desired intent.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PresentationState {
    #[default]
    Hidden,
    Visible,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct UserVisibleRenderRequest {
    pub(crate) source: ProjectSource,
    pub(crate) settings: ProjectRenderSettings,
    pub(crate) presentation: PresentationIntent,
}

/// Application-owned handle for one exact prospective prebuilt request.
///
/// This is the only preflight identifier visible to presentation code. The
/// Render Service admission token remains private inside `RenderFacade`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub(crate) struct ResourcePreflightId(pub(crate) String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResourcePreflightSeverity {
    Unmeasured,
    Normal,
    Warning,
    Hard,
    Invalid,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResourcePreflightAction {
    ContinuePrebuilt,
    SwitchRealtime,
    UseBasicVulkan,
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ResourcePreflightComponents {
    pub(crate) exact_geometry_bytes: u64,
    pub(crate) draw_metadata_bytes: u64,
    pub(crate) block_entity_bytes: u64,
    pub(crate) texture_and_shader_bytes: u64,
    pub(crate) render_target_bytes: u64,
    pub(crate) runtime_reserve_bytes: u64,
}

/// Product-semantic resource assessment exposed to React.
///
/// Adapter identity, renderer capability details and the one-time wire token
/// are intentionally absent. Byte components are retained because users can
/// inspect measured GPU storage without confusing it with driver VRAM usage.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ResourcePreflightView {
    pub(crate) preflight_id: ResourcePreflightId,
    pub(crate) severity: ResourcePreflightSeverity,
    pub(crate) conservative_vram_bytes: Option<u64>,
    pub(crate) safe_available_vram_bytes: Option<u64>,
    pub(crate) pressure_percent: Option<f64>,
    pub(crate) components: Option<ResourcePreflightComponents>,
    pub(crate) invalid_reason: Option<String>,
    pub(crate) allowed_actions: Vec<ResourcePreflightAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub(crate) struct OperationId(pub(crate) String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationState {
    Requested,
    Running,
    BuildCompleted,
    ArtifactReady,
    ViewerPreparing,
    RealtimeReady,
    GpuAccepted,
    GpuResident,
    ViewerPrepared,
    RuntimeReleased,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct OperationProgress {
    pub(crate) phase: String,
    pub(crate) activity: Option<String>,
    pub(crate) completed: Option<u64>,
    pub(crate) total: Option<u64>,
    pub(crate) elapsed_ms: u64,
}

/// Coarse application projection sent to presentation code. Renderer runtime
/// identities, process handles and artifact layout details are deliberately
/// absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RenderOperationView {
    pub(crate) operation_id: OperationId,
    pub(crate) source: ProjectSource,
    pub(crate) display_mode: RenderDisplayMode,
    pub(crate) build_mode: RenderBuildMode,
    pub(crate) state: OperationState,
    pub(crate) progress: OperationProgress,
    pub(crate) presentation: PresentationIntent,
    pub(crate) presentation_state: PresentationState,
    pub(crate) analysis: Option<ProjectAnalysis>,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ApplicationRenderSnapshot {
    pub(crate) operations: Vec<RenderOperationView>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ProductPreviewImage {
    pub(crate) preview_path: String,
    pub(crate) data_url: String,
}

/// Typed, product-facing analysis persisted alongside a ready artifact. The
/// application deserializes the renderer-owned JSON once at the boundary so
/// presentation code never receives an opaque artifact blob.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct ProjectAnalysis {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) material_semantics: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) derived: Option<AnalysisDerived>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) non_item_blocks: Option<Vec<NonItemBlockSummary>>,
    pub(crate) metadata: ProjectMetadata,
    pub(crate) structure_stats: StructureStats,
    pub(crate) material_items: Vec<MaterialItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) structural_block_counts: Option<Vec<StructuralBlockCount>>,
    pub(crate) regions: Vec<RegionSummary>,
    pub(crate) layers_summary: Vec<LayerSummary>,
    pub(crate) entity_summary: EntitySummary,
    pub(crate) container_scan: ContainerScanSummary,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct AnalysisDerived {
    pub(crate) building: BuildingAnalysis,
    pub(crate) material_counts: Vec<AnalysisCount>,
    pub(crate) category_totals: Vec<AnalysisCount>,
    pub(crate) categorized_groups: Vec<AnalysisCategory>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct BuildingAnalysis {
    pub(crate) density: f64,
    pub(crate) fluid_count: u64,
    pub(crate) fluid_ratio: f64,
    pub(crate) redstone_count: u64,
    pub(crate) redstone_ratio: f64,
    pub(crate) building_type: String,
    pub(crate) unique_block_types: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AnalysisCount { pub(crate) key: String, pub(crate) count: u64 }
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AnalysisCategory { pub(crate) category: String, pub(crate) total: u64, pub(crate) rows: Vec<AnalysisCount> }

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct NonItemBlockSummary {
    pub(crate) block_id: String,
    pub(crate) display_name: String,
    pub(crate) count: u64,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct StructuralBlockCount {
    pub(crate) block_id: String,
    pub(crate) count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) scope_counts: Option<Vec<StructuralBlockScopeCount>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct StructuralBlockScopeCount {
    pub(crate) region: String,
    pub(crate) layer: i32,
    pub(crate) count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ProjectMetadata {
    pub(crate) name: String,
    pub(crate) author: String,
    pub(crate) description: String,
    pub(crate) time_created: i64,
    pub(crate) time_modified: i64,
    pub(crate) total_blocks: i32,
    pub(crate) total_volume: i32,
    pub(crate) region_count: i32,
    pub(crate) enclosing_size: IntegerVector,
    pub(crate) litematic_version: i32,
    pub(crate) litematic_subversion: i32,
    pub(crate) minecraft_data_version: i32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct IntegerVector {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct StructureStats {
    pub(crate) scope: AnalysisScope,
    pub(crate) total_non_air_blocks: u64,
    pub(crate) unique_block_types: usize,
    pub(crate) total_regions: usize,
    pub(crate) total_layers: usize,
    pub(crate) total_entities: usize,
    pub(crate) total_tile_entities: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AnalysisScope {
    pub(crate) kind: String,
    pub(crate) region: Option<String>,
    pub(crate) layer: Option<i32>,
    pub(crate) world_y: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct MaterialItem {
    pub(crate) block_id: String,
    pub(crate) display_name: String,
    pub(crate) count: u64,
    pub(crate) block_count: u64,
    pub(crate) container_item_count: u64,
    pub(crate) total_count: u64,
    pub(crate) icon_hint: String,
    pub(crate) region_counts: Option<Vec<RegionCount>>,
    pub(crate) layer_counts: Option<Vec<LayerCount>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RegionCount {
    pub(crate) region: String,
    pub(crate) count: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct LayerCount {
    pub(crate) layer: i32,
    pub(crate) world_y: i32,
    pub(crate) count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RegionSummary {
    pub(crate) name: String,
    pub(crate) position: IntegerVector,
    pub(crate) size: IntegerVector,
    pub(crate) bounds: RegionBounds,
    pub(crate) total_non_air_blocks: u64,
    pub(crate) unique_block_types: usize,
    pub(crate) total_entities: usize,
    pub(crate) total_tile_entities: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RegionBounds {
    pub(crate) min_x: i32,
    pub(crate) max_x: i32,
    pub(crate) min_y: i32,
    pub(crate) max_y: i32,
    pub(crate) min_z: i32,
    pub(crate) max_z: i32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct LayerSummary {
    pub(crate) layer: i32,
    pub(crate) world_y: i32,
    pub(crate) total_non_air_blocks: u64,
    pub(crate) unique_block_types: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct EntitySummary {
    pub(crate) total_entities: usize,
    pub(crate) items: Vec<EntityCount>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct EntityCount {
    pub(crate) entity_id: String,
    pub(crate) count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ContainerScanSummary {
    pub(crate) enabled: bool,
    pub(crate) containers_scanned: usize,
    pub(crate) item_stacks_scanned: usize,
    pub(crate) warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_roundtrip_preserves_material_semantics_and_original_structure_identifiers() {
        let mut value = serde_json::json!({
            "metadata":{"name":"test","author":"","description":"","time_created":0,"time_modified":0,"total_blocks":3,"total_volume":3,"region_count":1,"enclosing_size":{"x":3,"y":1,"z":1},"litematic_version":6,"litematic_subversion":1,"minecraft_data_version":3700},
            "structure_stats":{"scope":{"kind":"all"},"total_non_air_blocks":3,"unique_block_types":2,"total_regions":1,"total_layers":1,"total_entities":0,"total_tile_entities":0},
            "material_items":[],"regions":[],"layers_summary":[],"entity_summary":{"total_entities":0,"items":[]},
            "container_scan":{"enabled":false,"containers_scanned":0,"item_stacks_scanned":0,"warnings":[]},
            "material_semantics":"minecraft-inventory-v3",
            "non_item_blocks":[{"block_id":"minecraft:piston_head","display_name":"活塞头","count":1,"reason":"no_item"}],
            "structural_block_counts":[{"block_id":"minecraft:redstone_wire","count":2},{"block_id":"minecraft:piston_head","count":1}]
        });
        value["derived"]=serde_json::json!({"building":{"density":0.0,"fluid_count":0,"fluid_ratio":0.0,"redstone_count":0,"redstone_ratio":0.0,"building_type":"too_small","unique_block_types":0},"material_counts":[],"category_totals":[],"categorized_groups":[]});
        let analysis: ProjectAnalysis = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(analysis.material_semantics.as_deref(), Some("minecraft-inventory-v3"));
        assert_eq!(analysis.derived.as_ref().unwrap().building.fluid_ratio,0.0);
        assert_eq!(analysis.derived.as_ref().unwrap().building.building_type,"too_small");
        assert_eq!(serde_json::to_value(&analysis).unwrap()["derived"],value["derived"]);
        assert_eq!(analysis.non_item_blocks.as_ref().unwrap()[0].count, 1);
        assert_eq!(analysis.structural_block_counts.as_ref().unwrap().iter().map(|block|block.count).sum::<u64>(),3);
        let reloaded: ProjectAnalysis = serde_json::from_value(serde_json::to_value(&analysis).unwrap()).unwrap();
        assert_eq!(analysis,reloaded);
        for field in ["material_semantics","non_item_blocks","structural_block_counts"] { value.as_object_mut().unwrap().remove(field); }
        let legacy: ProjectAnalysis = serde_json::from_value(value).unwrap();
        assert!(legacy.material_semantics.is_none());
        assert!(legacy.non_item_blocks.is_none());
        assert!(legacy.structural_block_counts.is_none());
    }

    #[test]
    fn persistent_intent_serialization_has_no_renderer_runtime_fields() {
        let request = UserVisibleRenderRequest {
            source: ProjectSource {
                path: PathBuf::from(r"C:\projections\castle.litematic"),
            },
            settings: ProjectRenderSettings::default(),
            presentation: PresentationIntent::default(),
        };
        let json = serde_json::to_string(&request).unwrap();
        for forbidden in [
            "residentSlot",
            "commitSeq",
            "directoryIndex",
            "gpuFence",
            "uploadAck",
            "artifactOffset",
            "workerPid",
            "viewerPid",
        ] {
            assert!(
                !json.contains(forbidden),
                "unexpected runtime field {forbidden}"
            );
        }
    }

    #[test]
    fn presentation_intent_is_not_a_runtime_lifecycle() {
        let hidden = PresentationIntent::default();
        assert_eq!(hidden.mode, PresentationMode::Hidden);
        assert!(!hidden.visible);
        assert!(hidden.placement.is_none());
    }

    #[test]
    fn render_display_mode_keeps_stable_wire_values() {
        assert_eq!(RenderDisplayMode::Normal.as_str(), "normal");
        assert_eq!(
            RenderDisplayMode::FastExperimental.as_str(),
            "fast_experimental"
        );
        assert_eq!(RenderDisplayMode::Full.as_str(), "full");
    }

    #[test]
    fn application_defaults_preserve_existing_product_behavior() {
        let settings = ProjectRenderSettings::default();
        assert_eq!(settings.build_mode, RenderBuildMode::Prebuilt);
        assert_eq!(settings.iris_fallback, IrisFallbackPreference::Fail);
        assert!(settings.allow_time_modification);
        assert!(settings.allow_weather_modification);
    }

    #[test]
    fn resource_preflight_view_contains_no_renderer_admission_or_adapter_identity() {
        let view = ResourcePreflightView {
            preflight_id: ResourcePreflightId("preflight-1".to_owned()),
            severity: ResourcePreflightSeverity::Warning,
            conservative_vram_bytes: Some(8_500),
            safe_available_vram_bytes: Some(10_000),
            pressure_percent: Some(85.0),
            components: Some(ResourcePreflightComponents {
                exact_geometry_bytes: 5_000,
                draw_metadata_bytes: 500,
                block_entity_bytes: 250,
                texture_and_shader_bytes: 1_000,
                render_target_bytes: 750,
                runtime_reserve_bytes: 1_000,
            }),
            invalid_reason: None,
            allowed_actions: vec![
                ResourcePreflightAction::ContinuePrebuilt,
                ResourcePreflightAction::SwitchRealtime,
                ResourcePreflightAction::UseBasicVulkan,
                ResourcePreflightAction::Cancel,
            ],
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("preflightId"));
        assert!(json.contains("use_basic_vulkan"));
        for forbidden in ["admissionToken", "adapter", "heapIndex", "deviceHandle"] {
            assert!(
                !json.contains(forbidden),
                "unexpected renderer-private field {forbidden}"
            );
        }
    }
}
