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
    pub time_created: i64,
    pub time_modified: i64,
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
    pub regions: Vec<AnalysisRegionOutput>,
    pub analysis: AnalysisSummary,
    pub derived: AnalysisDerivedOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRegionOutput {
    pub name: String,
    pub position: EnclosingSize,
    pub size: EnclosingSize,
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
pub struct VisualAllLayersOutput {
    pub metadata: MetadataOutput,
    pub chunk_size: u32,
    pub layers: Vec<LayerSliceOutput>,
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

fn default_vertex_light() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TexturedVertexOutput {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    #[serde(default)]
    pub emissive_tag: f32,
    /// 烘焙顶点亮度 = 原版方向 shade（CardinalLighting）x 顶点 AO
    /// （BlockModelLighter 四块平均）。vanilla 光照模式直接使用。
    #[serde(default = "default_vertex_light")]
    pub light: f32,
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
    /// All entity payload outside the discriminator. Ordinary entity
    /// rendering depends on fields such as `Pos`, `Rotation`, `variant`,
    /// equipment and inventories, so a typed litematic round trip must not
    /// discard them after DataFix has produced the MC 26.2 representation.
    #[serde(default, flatten)]
    pub data: BTreeMap<String, fastnbt::Value>,
}

impl EntityNbt {
    pub fn entity_id(&self) -> Option<&str> {
        self.id_lower.as_deref().or(self.id_upper.as_deref())
    }

    pub fn field(&self, name: &str) -> Option<&fastnbt::Value> {
        self.data.get(name)
    }

    pub fn fields(&self) -> &BTreeMap<String, fastnbt::Value> {
        &self.data
    }
}

#[cfg(test)]
mod entity_tests {
    use fastnbt::Value;

    use super::EntityNbt;

    #[test]
    fn entity_nbt_preserves_render_payload_through_nbt_round_trip() {
        let mut entity = EntityNbt {
            id_lower: Some("minecraft:painting".to_owned()),
            ..EntityNbt::default()
        };
        entity.data.insert(
            "Pos".to_owned(),
            Value::List(vec![
                Value::Double(1.5),
                Value::Double(2.0),
                Value::Double(-3.5),
            ]),
        );
        entity.data.insert(
            "variant".to_owned(),
            Value::String("minecraft:unpacked".to_owned()),
        );
        entity.data.insert("facing".to_owned(), Value::Byte(4));

        let encoded = fastnbt::to_bytes(&entity).expect("serialize entity NBT");
        let decoded: EntityNbt = fastnbt::from_bytes(&encoded).expect("deserialize entity NBT");

        assert_eq!(decoded.entity_id(), Some("minecraft:painting"));
        assert_eq!(decoded.field("Pos"), entity.field("Pos"));
        assert_eq!(decoded.field("variant"), entity.field("variant"));
        assert_eq!(decoded.field("facing"), entity.field("facing"));
    }
}
