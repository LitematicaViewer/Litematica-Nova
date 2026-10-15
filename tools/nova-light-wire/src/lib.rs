use std::io::{Read, Write};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_COMPRESSED_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_DECOMPRESSED_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_BATCH_TARGETS: usize = 512;
pub const MAX_WAVE_BATCHES: usize = 6;
pub const MAX_WAVE_TARGET_VOXELS: usize = 20_000_000;
pub const MAX_QUADS: usize = 1_000_000;
pub type Position = [i32; 3];
pub type Bounds = [Position; 2];

/// Concrete input semantics resolved from the client's actual resource catalog.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateInput {
    pub index: u32,
    pub is_air: bool,
    pub emission: u8,
    pub dampening: u8,
    pub can_occlude: bool,
    pub shape_occlusion: bool,
    pub faces: [Vec<[f64; 4]>; 6],
    pub full_faces: [bool; 6],
}

/// Runs address X-contiguous, then Z, then Y cells in one 16^3 section.
/// Unlisted cells are the explicitly supplied canonical-air state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectionInput {
    pub position: Position,
    pub runs: Vec<[u32; 3]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldInput {
    pub protocol: u32,
    pub resource_identity: String,
    pub regions: Vec<Bounds>,
    pub sections: Vec<Position>,
    pub halo: u32,
    pub air_state: u32,
    pub states: Vec<StateInput>,
    pub populated: Vec<SectionInput>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisteredWorld { pub scene_id: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderRequest { pub positions: Vec<Position>, pub halo:u32, pub fixed_edge:Option<i32> }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderOutput { pub positions: Vec<Position>, pub edge: i32, pub groups:Vec<CachedPlan> }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CachedPlan {pub positions:Vec<Position>,pub targets:Vec<Bounds>,pub reserved_domain_voxels:usize}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanRequest { pub positions: Vec<Position>, pub halo: u32, pub edge: i32 }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanOutput { pub count: usize, pub targets: Vec<Bounds>, pub reserved_domain_voxels: usize }

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Environment {
    pub sky: bool,
    pub min_y: i32,
    pub height: i32,
    pub expand_to_projection: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolveRequest {
    pub scene_id: String,
    pub environment: Environment,
    pub targets: Vec<Bounds>,
}

/// Transport coalescing only: each inner batch retains its original shared
/// domain, solve strategy, result order and statistics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolveWaveRequest {
    pub scene_id: String,
    pub environment: Environment,
    pub batches: Vec<Vec<Bounds>>,
}
pub type SolveWaveOutput = Vec<Result<SolveOutput,String>>;

pub fn target_voxels(targets:&[Bounds])->Result<usize,String> {
    targets.iter().try_fold(0usize,|total,bounds| {
        let volume=(0..3).try_fold(1usize,|volume,axis| {
            let length=i64::from(bounds[1][axis])-i64::from(bounds[0][axis])+1;
            if length<=0 {return Err("invalid target sample bounds".to_owned());}
            volume.checked_mul(usize::try_from(length).map_err(|_|"target sample axis overflow")?).ok_or_else(||"target sample volume overflow".to_owned())
        })?;
        total.checked_add(volume).ok_or_else(||"target samples overflow".to_owned())
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldOutput {
    pub bounds: Bounds,
    pub block: Vec<u8>,
    pub sky: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolveOutput {
    pub build_height: [i32; 2],
    pub fields: Vec<FieldOutput>,
    /// Exact source-solver counters, in the stable protocol order.
    pub statistics: [u64; 16],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LightingSample {
    pub position: Position,
    pub raw_light: Option<i32>,
    pub collision_full: Result<bool,String>,
    pub light_permeable: bool,
    pub shade_brightness: Result<f32,String>,
    pub solid_render: bool,
    pub emission: u8,
    pub emissive_rendering: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuadInput {
    pub position: Position,
    pub positions: [[f32; 3]; 4],
    pub direction: u8,
    pub shade_direction_override: Option<u8>,
    pub emission: u8,
    pub ambient_occlusion_enabled: bool,
    pub first_part_ambient_occlusion: bool,
    pub culled: bool,
    pub tint_argb: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BakeRequest {
    pub cardinal: [f32; 6],
    pub samples: Vec<LightingSample>,
    pub quads: Vec<QuadInput>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuadOutput {
    pub mode: u8,
    pub colors: [[u8; 4]; 4],
    pub iris_tints: [[u8; 4]; 4],
    pub iris_ao: [u8; 4],
    pub iris_old_ao: [u8; 4],
    pub lights: [[i16; 2]; 4],
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let plain = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if plain.len() > MAX_DECOMPRESSED_BYTES { return Err("light request exceeds decompressed limit".into()); }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&plain).map_err(|e| e.to_string())?;
    let encoded = encoder.finish().map_err(|e| e.to_string())?;
    if encoded.len() > MAX_COMPRESSED_BYTES { return Err("light request exceeds compressed limit".into()); }
    Ok(encoded)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    if bytes.len() > MAX_COMPRESSED_BYTES { return Err("light input exceeds compressed limit".into()); }
    let mut plain = Vec::new();
    flate2::read::ZlibDecoder::new(bytes).take((MAX_DECOMPRESSED_BYTES + 1) as u64)
        .read_to_end(&mut plain).map_err(|e| e.to_string())?;
    if plain.len() > MAX_DECOMPRESSED_BYTES { return Err("light input exceeds decompressed limit".into()); }
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}

pub fn content_id(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
