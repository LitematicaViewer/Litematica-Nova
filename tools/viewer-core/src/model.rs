use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnclosingSize {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataOutput {
    pub name: String,
    pub author: String,
    pub description: String,
    pub total_blocks: i32,
    pub total_volume: i32,
    pub region_count: i32,
    pub enclosing_size: EnclosingSize,
    pub litematic_version: i32,
    pub litematic_subversion: i32,
    pub minecraft_data_version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_non_air_blocks: u64,
    pub block_counts: HashMap<String, u64>,
    pub entity_counts: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountEntryOutput {
    pub key: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorizedCountGroupOutput {
    pub category: String,
    pub total: u64,
    pub rows: Vec<CountEntryOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildingStatsOutput {
    pub density: f64,
    pub fluid_count: u64,
    pub fluid_ratio: f64,
    pub redstone_count: u64,
    pub redstone_ratio: f64,
    pub building_type: String,
    pub unique_block_types: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisDerivedOutput {
    pub material_counts: Vec<CountEntryOutput>,
    pub category_totals: Vec<CountEntryOutput>,
    pub categorized_groups: Vec<CategorizedCountGroupOutput>,
    pub building: BuildingStatsOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisOutput {
    pub metadata: MetadataOutput,
    pub analysis: AnalysisSummary,
    pub derived: AnalysisDerivedOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaletteEntryOutput {
    pub block_id: String,
    pub property_id: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerBlockOutput {
    pub x: i32,
    pub z: i32,
    pub palette_id: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerSliceOutput {
    pub y: i32,
    pub blocks: Vec<LayerBlockOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkBlockOutput {
    pub lx: u8,
    pub ly: u8,
    pub lz: u8,
    pub palette_id: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkOutput {
    pub cx: i32,
    pub cy: i32,
    pub cz: i32,
    pub blocks: Vec<ChunkBlockOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualSummaryOutput {
    pub chunk_size: u32,
    pub size_x: i32,
    pub size_y: i32,
    pub size_z: i32,
    pub palette: Vec<PaletteEntryOutput>,
    pub property_pool: Vec<BTreeMap<String, String>>,
    pub layers: Vec<LayerSliceOutput>,
    pub chunks: Vec<ChunkOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualOutput {
    pub metadata: MetadataOutput,
    pub visual: VisualSummaryOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualMetaOutput {
    pub metadata: MetadataOutput,
    pub visual: VisualMetaSummaryOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualMetaSummaryOutput {
    pub chunk_size: u32,
    pub size_x: i32,
    pub size_y: i32,
    pub size_z: i32,
    pub palette: Vec<PaletteEntryOutput>,
    pub property_pool: Vec<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualLayerOutput {
    pub metadata: MetadataOutput,
    pub chunk_size: u32,
    pub y: i32,
    pub blocks: Vec<LayerBlockOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkCoordOutput {
    pub cx: i32,
    pub cy: i32,
    pub cz: i32,
    pub non_air_blocks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshChunkOutput {
    pub cx: i32,
    pub cy: i32,
    pub cz: i32,
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub color_indices: Vec<u32>,
    #[serde(default)]
    pub compact_surfaces: Vec<CompactSurfaceOutput>,
    #[serde(default)]
    pub textured_vertices: Vec<TexturedVertexOutput>,
    #[serde(default)]
    pub translucent_indices: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactSurfaceOutput {
    pub face_index: u8,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub palette_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TexturedVertexOutput {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    #[serde(default)]
    pub emissive_tag: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshOutput {
    pub metadata: MetadataOutput,
    pub chunk_size: u32,
    pub palette: Vec<PaletteEntryOutput>,
    pub property_pool: Vec<BTreeMap<String, String>>,
    pub chunks: Vec<MeshChunkOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshIndexOutput {
    pub metadata: MetadataOutput,
    pub chunk_size: u32,
    pub palette: Vec<PaletteEntryOutput>,
    pub property_pool: Vec<BTreeMap<String, String>>,
    pub chunks: Vec<ChunkCoordOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshBatchOutput {
    pub metadata: MetadataOutput,
    pub chunk_size: u32,
    pub palette: Vec<PaletteEntryOutput>,
    pub property_pool: Vec<BTreeMap<String, String>>,
    pub offset: usize,
    pub limit: usize,
    pub total_chunks: usize,
    pub chunks: Vec<MeshChunkOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BlockStateNbt {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntityNbt {
    #[serde(rename = "id")]
    pub id_lower: Option<String>,
    #[serde(rename = "Id")]
    pub id_upper: Option<String>,
}

impl EntityNbt {
    pub fn entity_id(&self) -> Option<&str> {
        self.id_lower.as_deref().or(self.id_upper.as_deref())
    }
}
