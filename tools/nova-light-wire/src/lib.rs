use std::io::{Read, Write};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_COMPRESSED_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_DECOMPRESSED_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_BATCH_TARGETS: usize = 512;
pub const MAX_WAVE_BATCHES: usize = 6;
pub const MAX_WAVE_TARGET_VOXELS: usize = 20_000_000;
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Environment {
    pub sky: bool,
    pub min_y: i32,
    pub height: i32,
    pub expand_to_projection: bool,
    /// Locally resolved inclusive bounds; the original policy remains explicit.
    #[serde(default)]
    pub resolved_height: Option<[i32; 2]>,
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

const COMPACT_MAGIC: &[u8; 4] = b"NLB2";
const COMPACT_ZSTD_MAGIC: &[u8; 4] = b"NLB3";
pub fn encode_compact<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let plain = postcard::to_allocvec(value).map_err(|e| e.to_string())?;
    if plain.len() > MAX_DECOMPRESSED_BYTES { return Err("compact light plain byte limit".into()); }
    let compressed = lz4_flex::block::compress(&plain);
    if compressed.len() + 8 > MAX_COMPRESSED_BYTES { return Err("compact light compressed byte limit".into()); }
    let mut encoded = Vec::with_capacity(compressed.len() + 8);
    encoded.extend_from_slice(COMPACT_MAGIC);
    encoded.extend_from_slice(&(plain.len() as u32).to_le_bytes());
    encoded.extend_from_slice(&compressed);
    Ok(encoded)
}
pub fn decode_compact<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    decode_compact_bounded(bytes,MAX_COMPRESSED_BYTES,MAX_DECOMPRESSED_BYTES)
}
pub fn decode_compact_bounded<T: DeserializeOwned>(bytes: &[u8],compressed_limit:usize,plain_limit:usize) -> Result<T, String> {
    // A caller may tighten its lane's budget, never raise the protocol limits.
    let compressed_limit=compressed_limit.min(MAX_COMPRESSED_BYTES);
    let plain_limit=plain_limit.min(MAX_DECOMPRESSED_BYTES);
    if bytes.len() < 8 || bytes.len() > compressed_limit || (&bytes[..4]!=COMPACT_MAGIC && &bytes[..4]!=COMPACT_ZSTD_MAGIC) {
        return Err("invalid compact light frame".into());
    }
    let length = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if length > plain_limit { return Err("compact light decompressed byte limit".into()); }
    let mut plain = vec![0; length];
    let written = if &bytes[..4]==COMPACT_ZSTD_MAGIC {
        decode_zstd_into(&bytes[8..],&mut plain,28)?
    } else {lz4_flex::block::decompress_into(&bytes[8..], &mut plain).map_err(|e| e.to_string())?};
    if written != length { return Err("compact light plain length mismatch".into()); }
    let (result, rest) = postcard::take_from_bytes(&plain).map_err(|e| e.to_string())?;
    if !rest.is_empty() { return Err("compact light trailing data".into()); }
    Ok(result)
}

/// Compact scene registration and shared-light fields preserve their exact
/// postcard representation. Both bounded transport codecs remain supported.
pub fn encode_compact_zstd<T:Serialize>(value:&T)->Result<Vec<u8>,String> {
    let plain=postcard::to_allocvec(value).map_err(|e|e.to_string())?;
    if plain.len()>MAX_DECOMPRESSED_BYTES {return Err("compact light plain byte limit".into());}
    let compressed=zstd::bulk::compress(&plain,1).map_err(|e|e.to_string())?;
    if compressed.len()+8>MAX_COMPRESSED_BYTES {return Err("compact light compressed byte limit".into());}
    let mut output=Vec::with_capacity(compressed.len()+8);
    output.extend_from_slice(COMPACT_ZSTD_MAGIC);output.extend_from_slice(&(plain.len() as u32).to_le_bytes());output.extend_from_slice(&compressed);
    Ok(output)
}
fn decode_zstd_into(frame:&[u8],plain:&mut [u8],window_log:u32)->Result<usize,String> {
    if zstd::zstd_safe::find_frame_compressed_size(frame).map_err(|e|e.to_string())?!=frame.len() {
        return Err("compact zstd trailing data".into());
    }
    let mut decoder=zstd::bulk::Decompressor::new().map_err(|e|e.to_string())?;
    decoder.set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(window_log)).map_err(|e|e.to_string())?;
    decoder.decompress_to_buffer(frame,plain).map_err(|e|e.to_string())
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
    decode_bounded(bytes,MAX_COMPRESSED_BYTES,MAX_DECOMPRESSED_BYTES)
}
/// Per-operation limits narrow the existing transport budget before expansion.
pub fn decode_bounded<T: DeserializeOwned>(bytes:&[u8],compressed_limit:usize,plain_limit:usize)->Result<T,String> {
    if compressed_limit>MAX_COMPRESSED_BYTES || plain_limit>MAX_DECOMPRESSED_BYTES || bytes.len()>compressed_limit {return Err("light input exceeds compressed limit".into());}
    let mut plain = Vec::new();
    flate2::read::ZlibDecoder::new(bytes).take((plain_limit + 1) as u64)
        .read_to_end(&mut plain).map_err(|e| e.to_string())?;
    if plain.len() > plain_limit { return Err("light input exceeds decompressed limit".into()); }
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}

pub fn content_id(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

#[cfg(test)]
mod compact_tests {
    use super::*;
    #[test]
    fn compact_preserves_float_bits_errors_and_optional_values() {
        #[derive(Serialize,Deserialize)] struct Values { f:[f32;4], d:[f64;3], value:Option<i32>, error:Result<bool,String> }
        let value=Values {f:[-0.0,1.0,f32::from_bits(0x7fc00001),f32::MIN_POSITIVE],d:[-0.0,f64::MIN_POSITIVE,f64::from_bits(0x7ff8000000000001)],value:None,error:Err("real sample error".into())};
        let actual:Values=decode_compact(&encode_compact(&value).unwrap()).unwrap();
        assert_eq!(value.f.map(f32::to_bits),actual.f.map(f32::to_bits));assert_eq!(value.d.map(f64::to_bits),actual.d.map(f64::to_bits));assert_eq!(value.error,actual.error);assert_eq!(actual.value,None);
    }
    #[test]
    fn compact_rejects_oversized_length_before_allocation_and_truncated_frame() {
        let mut frame=b"NLB2".to_vec();frame.extend_from_slice(&((MAX_DECOMPRESSED_BYTES+1) as u32).to_le_bytes());
        assert!(decode_compact::<Vec<u8>>(&frame).unwrap_err().contains("decompressed byte limit"));
        frame[4..8].copy_from_slice(&8u32.to_le_bytes());assert!(decode_compact::<Vec<u8>>(&frame).is_err());
        let old=encode(&vec![1u8,2]).unwrap();assert!(decode_compact::<Vec<u8>>(&old).is_err());
    }
    #[test]
    fn compact_bounded_preserves_both_codecs_and_enforces_each_exact_limit() {
        let value=(0..513u32).map(|i|i.wrapping_mul(10_007)).collect::<Vec<_>>();
        let plain=postcard::to_allocvec(&value).unwrap();
        for frame in [encode_compact(&value).unwrap(),encode_compact_zstd(&value).unwrap()] {
            let actual:Vec<u32>=decode_compact_bounded(&frame,frame.len(),plain.len()).unwrap();
            assert_eq!(actual,value);
            assert!(decode_compact_bounded::<Vec<u32>>(&frame,frame.len()-1,plain.len()).is_err());
            assert!(decode_compact_bounded::<Vec<u32>>(&frame,frame.len(),plain.len()-1).unwrap_err().contains("decompressed byte limit"));
            for end in [0,3,7,frame.len()-1] {assert!(decode_compact_bounded::<Vec<u32>>(&frame[..end],frame.len(),plain.len()).is_err());}
            let mut trailing=frame.clone();trailing.push(0);
            assert!(decode_compact_bounded::<Vec<u32>>(&trailing,trailing.len(),plain.len()).is_err());
            for incorrect in [plain.len()-1,plain.len()+1] {
                let mut invalid=frame.clone();invalid[4..8].copy_from_slice(&(incorrect as u32).to_le_bytes());
                assert!(decode_compact_bounded::<Vec<u32>>(&invalid,invalid.len(),plain.len()+1).is_err());
            }
            let mut oversized=frame;oversized[4..8].copy_from_slice(&((MAX_DECOMPRESSED_BYTES+1) as u32).to_le_bytes());
            assert!(decode_compact_bounded::<Vec<u32>>(&oversized,usize::MAX,usize::MAX).unwrap_err().contains("decompressed byte limit"));
        }
    }
    #[test]
    fn compact_bounded_rejects_concatenated_zstd_frames_and_postcard_trailing_data() {
        let encoded=encode_compact_zstd(&vec![1u8,2,3]).unwrap();
        let mut concatenated=encoded.clone();concatenated.extend_from_slice(&encoded[8..]);
        assert!(decode_compact_bounded::<Vec<u8>>(&concatenated,concatenated.len(),32).unwrap_err().contains("trailing"));
        // A perfectly valid compressed frame is still invalid if its declared
        // postcard payload contains an additional object after the result.
        let mut plain=postcard::to_allocvec(&vec![1u8,2,3]).unwrap();plain.push(0);
        for zstd in [false,true] {
            let mut frame=if zstd {COMPACT_ZSTD_MAGIC.to_vec()} else {COMPACT_MAGIC.to_vec()};
            frame.extend_from_slice(&(plain.len() as u32).to_le_bytes());
            frame.extend_from_slice(&if zstd {zstd::bulk::compress(&plain,1).unwrap()} else {lz4_flex::block::compress(&plain)});
            assert!(decode_compact_bounded::<Vec<u8>>(&frame,frame.len(),plain.len()).unwrap_err().contains("trailing"));
        }
    }
    #[test]
    fn compact_zstd_preserves_exact_concrete_world_and_rejects_trailing_and_budget() {
        let value=WorldInput {protocol:PROTOCOL_VERSION,resource_identity:"actual-resource-identity".into(),regions:vec![[[i32::MIN,0,0],[i32::MIN,0,0]]],sections:vec![[0,0,0]],halo:1,air_state:0,states:vec![StateInput {
            index:0,is_air:true,emission:0,dampening:0,can_occlude:false,shape_occlusion:false,
            faces:std::array::from_fn(|_|vec![[-0.0,0.0,1.0,1.0]]),full_faces:[false;6],
        }],populated:Vec::new()};
        let encoded=encode_compact_zstd(&value).unwrap();
        let decoded:WorldInput=decode_compact(&encoded).unwrap();
        assert_eq!(postcard::to_allocvec(&value).unwrap(),postcard::to_allocvec(&decoded).unwrap());
        let mut invalid=encoded.clone();invalid.push(0);assert!(decode_compact::<WorldInput>(&invalid).unwrap_err().contains("trailing"));
        let mut invalid=encoded;invalid[4..8].copy_from_slice(&u32::MAX.to_le_bytes());assert!(decode_compact::<WorldInput>(&invalid).unwrap_err().contains("decompressed byte limit"));
    }
}
