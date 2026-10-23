#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::type_complexity
)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use anyhow::{Result, anyhow};

use crate::build_mode::{
    compact_cache_v2_enabled, fast_path_carpet_enabled, fast_path_enabled,
    fast_path_half_slab_enabled, fast_path_non_occluding_enabled, fast_path_stair_half_enabled,
};

use crate::model::{
    ChunkCoordOutput, CompactSurfaceOutput, LayerBlockOutput, LayerSliceOutput, MeshBatchOutput,
    MeshChunkOutput, MeshIndexOutput, MeshOutput, MetadataOutput, PaletteEntryOutput,
    TexturedVertexOutput, VisualOutput, VisualSummaryOutput,
};
use crate::nbt::{
    RegionBounds, bits_for_palette, for_each_palette_index, region_bounds,
    region_coord_to_storage_coord, region_volume, storage_to_region_coords,
};
use crate::visual::{VisualCatalog, format_block_state, is_air_state, load_visual_catalog};

const FACE_VERTICES: [[[f32; 3]; 4]; 6] = [
    [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
    ],
    [
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
    ],
    [
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    ],
    [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ],
];

const FACE_NEIGHBORS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];
const FAST_PATH_EMPTY: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkKey {
    pub cx: i32,
    pub cy: i32,
    pub cz: i32,
}

impl ChunkKey {
    pub const fn new(cx: i32, cy: i32, cz: i32) -> Self {
        Self { cx, cy, cz }
    }

    pub fn from_world_position(position: [f32; 3], chunk_size: u32) -> Self {
        let chunk_size = chunk_size as f32;
        Self {
            cx: (position[0] / chunk_size).floor() as i32,
            cy: (position[1] / chunk_size).floor() as i32,
            cz: (position[2] / chunk_size).floor() as i32,
        }
    }

    pub fn center(self, chunk_size: u32) -> [f32; 3] {
        let chunk_size = chunk_size as f32;
        [
            (self.cx as f32 + 0.5) * chunk_size,
            (self.cy as f32 + 0.5) * chunk_size,
            (self.cz as f32 + 0.5) * chunk_size,
        ]
    }

    pub fn distance_squared(self, other: ChunkKey) -> i32 {
        let dx = self.cx - other.cx;
        let dy = self.cy - other.cy;
        let dz = self.cz - other.cz;
        dx * dx + dy * dy + dz * dz
    }

    pub fn horizontal_distance_squared(self, other: ChunkKey) -> i32 {
        let dx = self.cx - other.cx;
        let dz = self.cz - other.cz;
        dx * dx + dz * dz
    }

    pub fn horizontal_axis_distance(self, other: ChunkKey) -> i32 {
        let dx = (self.cx - other.cx).abs();
        let dz = (self.cz - other.cz).abs();
        dx.max(dz)
    }
}

#[derive(Debug, Clone)]
pub struct ChunkIndexEntry {
    pub key: ChunkKey,
    pub non_air_blocks: u32,
}

pub struct ChunkSceneIndex {
    game_data_content_id: String,
    root: crate::nbt::LitematicRoot,
    catalog: VisualCatalog,
    render_info: Vec<BlockRenderInfo>,
    region_palette_semantics: HashMap<String, Vec<PaletteSemantic>>,
    chunk_size: u32,
    chunk_counts: BTreeMap<ChunkKey, u32>,
    chunk_entries: Vec<ChunkIndexEntry>,
}

impl ChunkSceneIndex {
    pub fn load(path: &Path, chunk_size: u32) -> Result<Self> {
        let game_data_content_id = crate::game_data::active_context()?.content_id;
        let input_read_started_at = Instant::now();
        println!(
            "[TRACE_WALL] stage=input_read event=start path={} chunk_size={}",
            path.display(),
            chunk_size
        );
        let (root, catalog) = load_visual_catalog(path)?;
        println!(
            "[TRACE_WALL] stage=input_read event=end elapsed_ms={}",
            input_read_started_at.elapsed().as_millis()
        );
        let render_info = build_palette_render_info(&catalog);
        let palette_semantic_started_at = Instant::now();
        let region_palette_semantics =
            build_region_palette_semantics(&root, &catalog, &render_info)?;
        println!(
            "[MESH_CACHE] block_class_cache_miss palette_entries={} cached_render_classes={}",
            catalog.palette.len(),
            render_info.len()
        );
        println!(
            "[MESH_CACHE] palette_semantics_cache_ready regions={} prepare_ms={}",
            region_palette_semantics.len(),
            palette_semantic_started_at.elapsed().as_millis()
        );
        let chunk_counts = scan_chunk_counts(&root, &catalog, chunk_size)?;
        let chunk_entries = sorted_chunk_entries(&chunk_counts);
        Ok(Self {
            game_data_content_id,
            root,
            catalog,
            render_info,
            region_palette_semantics,
            chunk_size,
            chunk_counts,
            chunk_entries,
        })
    }

    pub fn game_data_content_id(&self) -> &str { &self.game_data_content_id }

    pub fn source_min_y(&self) -> i32 { self.catalog.bounds.map_or(0, |bounds| bounds.min_y) }

    pub fn metadata(&self) -> &MetadataOutput {
        &self.catalog.metadata
    }

    pub fn palette(&self) -> &[PaletteEntryOutput] {
        &self.catalog.palette
    }

    pub fn property_pool(&self) -> &[BTreeMap<String, String>] {
        &self.catalog.property_pool
    }

    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    pub fn chunk_count(&self) -> usize {
        self.chunk_entries.len()
    }

    pub fn chunk_entries(&self) -> &[ChunkIndexEntry] {
        &self.chunk_entries
    }

    pub fn contains_chunk(&self, key: ChunkKey) -> bool {
        self.chunk_counts.contains_key(&key)
    }

    pub fn non_air_blocks(&self, key: ChunkKey) -> Option<u32> {
        self.chunk_counts.get(&key).copied()
    }

    pub fn build_chunk_meshes(&self, keys: &[ChunkKey]) -> Result<Vec<MeshChunkOutput>> {
        let batch_started_at = Instant::now();
        let selected_keys = keys
            .iter()
            .copied()
            .filter(|key| self.contains_chunk(*key))
            .collect::<Vec<_>>();
        if selected_keys.is_empty() {
            return Ok(Vec::new());
        }

        let target_set = selected_keys.iter().copied().collect::<HashSet<_>>();
        let expanded_neighbor_chunks = expand_target_chunks(&target_set).len();
        println!(
            "[MESH_CACHE] block_class_cache_hit palette_entries={} selected_chunks={}",
            self.render_info.len(),
            selected_keys.len()
        );
        println!(
            "[MESH_CACHE] render_info_cache_hit palette_entries={} selected_chunks={}",
            self.render_info.len(),
            selected_keys.len()
        );
        let semantic_started_at = Instant::now();
        println!(
            "[TRACE_WALL] stage=semantic_prepare event=start chunks={}",
            selected_keys.len()
        );
        let (occupied, chunk_blocks, access_stats) = collect_target_chunk_blocks(
            &self.root,
            &self.catalog,
            &self.region_palette_semantics,
            self.chunk_size,
            &selected_keys,
            &self.chunk_counts,
            None,
        )?;
        let semantic_prepare_ms = semantic_started_at.elapsed().as_millis();
        println!(
            "[TRACE_WALL] stage=semantic_prepare event=end chunks={} elapsed_ms={}",
            selected_keys.len(),
            semantic_prepare_ms
        );
        println!(
            "[MESH_BUILD] batch_chunks={} shared_neighbor_chunks={} target_blocks={} occluding_cells={}",
            selected_keys.len(),
            expanded_neighbor_chunks,
            access_stats.target_block_count,
            access_stats.occluding_cells
        );
        println!(
            "[MESH_BUILD] block_access_cost region_chunk_intersections={} palette_reads={} target_blocks={} occluding_cells={}",
            access_stats.region_chunk_intersections,
            access_stats.palette_reads,
            access_stats.target_block_count,
            access_stats.occluding_cells
        );
        println!(
            "[SEMANTIC_TIMING] blockstate_normalize_ms={} render_semantics_lookup_ms={} palette_lookup_ms={} occlusion_precheck_ms={} chunk_bucket_ms={}",
            access_stats.blockstate_normalize_ms,
            access_stats.render_semantics_lookup_ms,
            access_stats.palette_lookup_ms,
            access_stats.occlusion_precheck_ms,
            access_stats.chunk_bucket_ms
        );
        let mesh_build_started_at = Instant::now();
        println!(
            "[TRACE_WALL] stage=mesh_build event=start chunks={}",
            selected_keys.len()
        );
        let mut chunks = Vec::with_capacity(selected_keys.len());
        let mut total_generated_faces = 0_usize;
        let mut total_culled_faces = 0_usize;
        let mut total_face_counts = [0_usize; 6];
        let mut total_culled_by_neighbor = 0_usize;
        let mut total_non_full_block_faces = 0_usize;
        let mut total_non_full_occlusion_rule = 0_usize;
        let mut total_chunk_boundary_faces = 0_usize;
        let mut total_eligible_fast_path_blocks = 0_usize;
        let mut total_fast_path_blocks = 0_usize;
        let mut total_fallback_blocks = 0_usize;
        let mut total_fast_path_visible_faces = 0_usize;
        let mut total_fast_path_output_quads = 0_usize;
        let mut total_fast_path_opaque_blocks = 0_usize;
        let mut total_fast_path_non_occluding_blocks = 0_usize;
        let mut total_fast_path_half_slab_blocks = 0_usize;
        let mut total_fast_path_carpet_blocks = 0_usize;
        let mut total_fast_path_stair_half_blocks = 0_usize;
        let mut total_fast_path_opaque_visible_faces = 0_usize;
        let mut total_fast_path_non_occluding_visible_faces = 0_usize;
        let mut total_fast_path_half_slab_visible_faces = 0_usize;
        let mut total_fast_path_carpet_visible_faces = 0_usize;
        let mut total_fast_path_stair_half_visible_faces = 0_usize;
        let mut total_fast_path_opaque_output_quads = 0_usize;
        let mut total_fast_path_non_occluding_output_quads = 0_usize;
        let mut total_fast_path_half_slab_output_quads = 0_usize;
        let mut total_fast_path_carpet_output_quads = 0_usize;
        let mut total_fast_path_stair_half_output_quads = 0_usize;
        let mut total_fallback_low_benefit_blocks = 0_usize;
        let mut total_fallback_non_full_cuboid_blocks = 0_usize;
        let mut total_fallback_cuboid_half_slab_blocks = 0_usize;
        let mut total_fallback_cuboid_stair_half_blocks = 0_usize;
        let mut total_fallback_cuboid_carpet_blocks = 0_usize;
        let mut total_fallback_cuboid_snow_blocks = 0_usize;
        let mut total_fallback_crossed_planes_blocks = 0_usize;
        let mut total_fallback_cross_column_blocks = 0_usize;
        let mut total_fast_path_used_chunks = 0_usize;
        let mut total_fast_path_eligible_chunks = 0_usize;
        let mut total_fast_path_low_benefit_chunks = 0_usize;
        for (key, blocks) in selected_keys.into_iter().zip(chunk_blocks) {
            let (chunk, face_stats) = build_mesh_chunk_output(
                key,
                self.chunk_size,
                blocks,
                &occupied,
                &self.render_info,
                &self.catalog,
            );
            total_generated_faces += face_stats.generated_faces;
            total_culled_faces += face_stats.culled_faces;
            total_face_counts[0] += face_stats.generated_neg_x;
            total_face_counts[1] += face_stats.generated_pos_x;
            total_face_counts[2] += face_stats.generated_neg_y;
            total_face_counts[3] += face_stats.generated_pos_y;
            total_face_counts[4] += face_stats.generated_neg_z;
            total_face_counts[5] += face_stats.generated_pos_z;
            total_culled_by_neighbor += face_stats.culled_by_neighbor;
            total_non_full_block_faces += face_stats.non_full_block_faces_generated;
            total_non_full_occlusion_rule += face_stats.non_full_block_occlusion_rule;
            total_chunk_boundary_faces += face_stats.chunk_boundary_faces;
            total_eligible_fast_path_blocks += face_stats.eligible_fast_path_blocks;
            total_fast_path_blocks += face_stats.fast_path_blocks;
            total_fallback_blocks += face_stats.fallback_blocks;
            total_fast_path_visible_faces += face_stats.fast_path_visible_faces;
            total_fast_path_output_quads += face_stats.fast_path_output_quads;
            total_fast_path_opaque_blocks += face_stats.fast_path_opaque_full_cube_blocks;
            total_fast_path_non_occluding_blocks +=
                face_stats.fast_path_non_occluding_full_cube_blocks;
            total_fast_path_half_slab_blocks += face_stats.fast_path_half_slab_blocks;
            total_fast_path_carpet_blocks += face_stats.fast_path_carpet_blocks;
            total_fast_path_stair_half_blocks += face_stats.fast_path_stair_half_blocks;
            total_fast_path_opaque_visible_faces += face_stats.fast_path_opaque_visible_faces;
            total_fast_path_non_occluding_visible_faces +=
                face_stats.fast_path_non_occluding_visible_faces;
            total_fast_path_half_slab_visible_faces += face_stats.fast_path_half_slab_visible_faces;
            total_fast_path_carpet_visible_faces += face_stats.fast_path_carpet_visible_faces;
            total_fast_path_stair_half_visible_faces +=
                face_stats.fast_path_stair_half_visible_faces;
            total_fast_path_opaque_output_quads += face_stats.fast_path_opaque_output_quads;
            total_fast_path_non_occluding_output_quads +=
                face_stats.fast_path_non_occluding_output_quads;
            total_fast_path_half_slab_output_quads += face_stats.fast_path_half_slab_output_quads;
            total_fast_path_carpet_output_quads += face_stats.fast_path_carpet_output_quads;
            total_fast_path_stair_half_output_quads += face_stats.fast_path_stair_half_output_quads;
            total_fallback_low_benefit_blocks += face_stats.fallback_low_benefit_blocks;
            total_fallback_non_full_cuboid_blocks += face_stats.fallback_non_full_cuboid_blocks;
            total_fallback_cuboid_half_slab_blocks += face_stats.fallback_cuboid_half_slab_blocks;
            total_fallback_cuboid_stair_half_blocks += face_stats.fallback_cuboid_stair_half_blocks;
            total_fallback_cuboid_carpet_blocks += face_stats.fallback_cuboid_carpet_blocks;
            total_fallback_cuboid_snow_blocks += face_stats.fallback_cuboid_snow_blocks;
            total_fallback_crossed_planes_blocks += face_stats.fallback_crossed_planes_blocks;
            total_fallback_cross_column_blocks += face_stats.fallback_cross_column_blocks;
            total_fast_path_used_chunks += usize::from(face_stats.fast_path_used_chunk);
            total_fast_path_eligible_chunks += usize::from(face_stats.fast_path_eligible_chunk);
            total_fast_path_low_benefit_chunks +=
                usize::from(face_stats.fast_path_low_benefit_chunk);
            chunks.push(chunk);
        }
        let mesh_build_ms = mesh_build_started_at.elapsed().as_millis();
        println!(
            "[TRACE_WALL] stage=mesh_build event=end chunks={} elapsed_ms={}",
            chunks.len(),
            mesh_build_ms
        );
        println!(
            "[VIEWER_MESH] x_faces_fixed minus_x_winding=ccw_outward plus_x_winding=ccw_outward requested_chunks={} built_chunks={}",
            keys.len(),
            chunks.len()
        );
        println!(
            "[VIEWER_MESH] faces_generated={} faces_culled={}",
            total_generated_faces, total_culled_faces
        );
        println!(
            "[VIEWER_MESH] face_counts neg_x={} pos_x={} neg_y={} pos_y={} neg_z={} pos_z={}",
            total_face_counts[0],
            total_face_counts[1],
            total_face_counts[2],
            total_face_counts[3],
            total_face_counts[4],
            total_face_counts[5]
        );
        println!(
            "[VIEWER_MESH] face_culled_by_neighbor total={} chunk_boundary_faces={}",
            total_culled_by_neighbor, total_chunk_boundary_faces
        );
        println!(
            "[VIEWER_MESH] non_full_block_faces_generated={} non_full_block_occlusion_rule=non_full_blocks_do_not_occlude_neighbors count={}",
            total_non_full_block_faces, total_non_full_occlusion_rule
        );
        println!(
            "[VIEWER_MESH_FASTPATH] eligible_fast_path_blocks={} fast_path_blocks={} fallback_blocks={} fast_path_visible_faces={} fast_path_output_quads={} opaque_full_cube_blocks={} non_occluding_full_cube_blocks={} half_slab_blocks={} carpet_blocks={} stair_half_blocks={} merge_key=current_mode_face_equivalence(palette_id+geometry_class)",
            total_eligible_fast_path_blocks,
            total_fast_path_blocks,
            total_fallback_blocks,
            total_fast_path_visible_faces,
            total_fast_path_output_quads,
            total_fast_path_opaque_blocks,
            total_fast_path_non_occluding_blocks,
            total_fast_path_half_slab_blocks,
            total_fast_path_carpet_blocks,
            total_fast_path_stair_half_blocks
        );
        println!(
            "[VIEWER_MESH_COVERAGE] built_chunks={} eligible_fast_path_chunks={} used_fast_path_chunks={} low_benefit_chunks={} block_coverage={:.3} face_coverage={:.3} chunk_coverage={:.3}",
            chunks.len(),
            total_fast_path_eligible_chunks,
            total_fast_path_used_chunks,
            total_fast_path_low_benefit_chunks,
            if access_stats.target_block_count == 0 {
                0.0
            } else {
                total_fast_path_blocks as f64 / access_stats.target_block_count as f64
            },
            if total_generated_faces == 0 {
                0.0
            } else {
                total_fast_path_visible_faces as f64 / total_generated_faces as f64
            },
            if chunks.is_empty() {
                0.0
            } else {
                total_fast_path_used_chunks as f64 / chunks.len() as f64
            }
        );
        println!(
            "[VIEWER_MESH_FALLBACK] low_benefit_blocks={} non_full_cuboid_blocks={} crossed_planes_blocks={} cross_column_blocks={} fallback_total_blocks={}",
            total_fallback_low_benefit_blocks,
            total_fallback_non_full_cuboid_blocks,
            total_fallback_crossed_planes_blocks,
            total_fallback_cross_column_blocks,
            total_fallback_blocks
        );
        println!(
            "[VIEWER_MESH_NON_FULL_CUBOID] half_slab={} stair_half={} carpet={} snow_layer={} single_cuboid_axis_aligned=true",
            total_fallback_cuboid_half_slab_blocks,
            total_fallback_cuboid_stair_half_blocks,
            total_fallback_cuboid_carpet_blocks,
            total_fallback_cuboid_snow_blocks
        );
        println!(
            "[VIEWER_MESH_FASTPATH_CLASS] class=opaque_full_cube hit_blocks={} visible_faces={} output_quads={} vertex_delta={} index_delta={}",
            total_fast_path_opaque_blocks,
            total_fast_path_opaque_visible_faces,
            total_fast_path_opaque_output_quads,
            total_fast_path_opaque_visible_faces
                .saturating_sub(total_fast_path_opaque_output_quads)
                * 4,
            total_fast_path_opaque_visible_faces
                .saturating_sub(total_fast_path_opaque_output_quads)
                * 6
        );
        println!(
            "[VIEWER_MESH_FASTPATH_CLASS] class=non_occluding_full_cube hit_blocks={} visible_faces={} output_quads={} vertex_delta={} index_delta={}",
            total_fast_path_non_occluding_blocks,
            total_fast_path_non_occluding_visible_faces,
            total_fast_path_non_occluding_output_quads,
            total_fast_path_non_occluding_visible_faces
                .saturating_sub(total_fast_path_non_occluding_output_quads)
                * 4,
            total_fast_path_non_occluding_visible_faces
                .saturating_sub(total_fast_path_non_occluding_output_quads)
                * 6
        );
        println!(
            "[VIEWER_MESH_FASTPATH_CLASS] class=half_slab_horizontal hit_blocks={} visible_faces={} output_quads={} vertex_delta={} index_delta={}",
            total_fast_path_half_slab_blocks,
            total_fast_path_half_slab_visible_faces,
            total_fast_path_half_slab_output_quads,
            total_fast_path_half_slab_visible_faces
                .saturating_sub(total_fast_path_half_slab_output_quads)
                * 4,
            total_fast_path_half_slab_visible_faces
                .saturating_sub(total_fast_path_half_slab_output_quads)
                * 6
        );
        println!(
            "[VIEWER_MESH_FASTPATH_CLASS] class=carpet_horizontal hit_blocks={} visible_faces={} output_quads={} vertex_delta={} index_delta={}",
            total_fast_path_carpet_blocks,
            total_fast_path_carpet_visible_faces,
            total_fast_path_carpet_output_quads,
            total_fast_path_carpet_visible_faces
                .saturating_sub(total_fast_path_carpet_output_quads)
                * 4,
            total_fast_path_carpet_visible_faces
                .saturating_sub(total_fast_path_carpet_output_quads)
                * 6
        );
        println!(
            "[VIEWER_MESH_FASTPATH_CLASS] class=stair_half_horizontal hit_blocks={} visible_faces={} output_quads={} vertex_delta={} index_delta={}",
            total_fast_path_stair_half_blocks,
            total_fast_path_stair_half_visible_faces,
            total_fast_path_stair_half_output_quads,
            total_fast_path_stair_half_visible_faces
                .saturating_sub(total_fast_path_stair_half_output_quads)
                * 4,
            total_fast_path_stair_half_visible_faces
                .saturating_sub(total_fast_path_stair_half_output_quads)
                * 6
        );
        println!(
            "[MESH_TIMING] semantic_prepare_ms={} mesh_build_ms={} total_ms={}",
            semantic_prepare_ms,
            mesh_build_ms,
            batch_started_at.elapsed().as_millis()
        );
        Ok(chunks)
    }


    pub fn build_layer_index_output(&self) -> Result<VisualOutput> {
        let Some(bounds) = self.catalog.bounds else {
            return Ok(VisualOutput {
                metadata: self.catalog.metadata.clone(),
                visual: VisualSummaryOutput {
                    chunk_size: self.chunk_size,
                    size_x: self.catalog.metadata.enclosing_size.x,
                    size_y: self.catalog.metadata.enclosing_size.y,
                    size_z: self.catalog.metadata.enclosing_size.z,
                    palette: self.catalog.palette.clone(),
                    property_pool: self.catalog.property_pool.clone(),
                    layers: Vec::new(),
                    chunks: Vec::new(),
                },
            });
        };

        let size_y = self.catalog.metadata.enclosing_size.y.max(0) as usize;
        let mut layers = (0..size_y)
            .map(|y| LayerSliceOutput {
                y: y as i32,
                blocks: Vec::new(),
            })
            .collect::<Vec<_>>();

        for region in self.root.regions.values() {
            let volume = region_volume(&region.size)?;
            let width = i64::from(region.size.x).unsigned_abs() as usize;
            let length = i64::from(region.size.z).unsigned_abs() as usize;
            let nbits = bits_for_palette(region.block_state_palette.len());
            let mut region_palette_ids =
                Vec::<Option<usize>>::with_capacity(region.block_state_palette.len());
            for block in &region.block_state_palette {
                let state = format_block_state(block);
                if is_air_state(&state) {
                    region_palette_ids.push(None);
                } else {
                    let palette_id = self
                        .catalog
                        .state_lookup
                        .get(&state)
                        .copied()
                        .ok_or_else(|| anyhow!("missing palette entry for state {}", state))?;
                    region_palette_ids.push(Some(palette_id));
                }
            }

            for_each_palette_index(
                &region.block_states,
                volume,
                nbits,
                region.block_state_palette.len(),
                |index, palette_index| {
                    let Some(Some(palette_id)) = region_palette_ids.get(palette_index) else {
                        return Ok(());
                    };
                    let y = index / (width * length);
                    let i_in_layer = index % (width * length);
                    let z = i_in_layer / width;
                    let x = i_in_layer % width;
                    let (rx, ry, rz) = storage_to_region_coords(x, y, z, &region.size);
                    let global_y = region.position.y + ry - bounds.min_y;
                    if global_y < 0 || global_y as usize >= layers.len() {
                        return Ok(());
                    }
                    layers[global_y as usize].blocks.push(LayerBlockOutput {
                        x: region.position.x + rx - bounds.min_x,
                        z: region.position.z + rz - bounds.min_z,
                        palette_id: *palette_id,
                    });
                    Ok(())
                },
            )?;
        }

        Ok(VisualOutput {
            metadata: self.catalog.metadata.clone(),
            visual: VisualSummaryOutput {
                chunk_size: self.chunk_size,
                size_x: self.catalog.metadata.enclosing_size.x,
                size_y: self.catalog.metadata.enclosing_size.y,
                size_z: self.catalog.metadata.enclosing_size.z,
                palette: self.catalog.palette.clone(),
                property_pool: self.catalog.property_pool.clone(),
                layers,
                chunks: Vec::new(),
            },
        })
    }
}

#[derive(Clone, Copy)]
struct TargetBlock {
    gx: i32,
    gy: i32,
    gz: i32,
    palette_id: usize,
}

struct OccupancyGrid {
    chunk_size: usize,
    chunk_index: HashMap<ChunkKey, usize>,
    bits: Vec<Vec<u64>>,
}

impl OccupancyGrid {
    fn new(chunk_size: usize, chunk_keys: Vec<ChunkKey>) -> Self {
        let chunk_index = chunk_keys
            .iter()
            .copied()
            .enumerate()
            .map(|(index, key)| (key, index))
            .collect::<HashMap<_, _>>();
        let chunk_volume = chunk_size
            .saturating_mul(chunk_size)
            .saturating_mul(chunk_size);
        let words_per_chunk = chunk_volume.div_ceil(64);
        let bits = (0..chunk_keys.len())
            .map(|_| vec![0_u64; words_per_chunk])
            .collect::<Vec<_>>();
        Self {
            chunk_size,
            chunk_index,
            bits,
        }
    }

    fn set_local(&mut self, chunk_index: usize, local_x: usize, local_y: usize, local_z: usize) {
        let bit_index = (local_y * self.chunk_size + local_z) * self.chunk_size + local_x;
        let word_index = bit_index / 64;
        let mask = 1_u64 << (bit_index % 64);
        self.bits[chunk_index][word_index] |= mask;
    }

    fn contains_world(&self, gx: i32, gy: i32, gz: i32) -> bool {
        let chunk_key = ChunkKey::new(
            gx.div_euclid(self.chunk_size as i32),
            gy.div_euclid(self.chunk_size as i32),
            gz.div_euclid(self.chunk_size as i32),
        );
        let Some(&chunk_index) = self.chunk_index.get(&chunk_key) else {
            return false;
        };
        let local_x = (gx - chunk_key.cx * self.chunk_size as i32) as usize;
        let local_y = (gy - chunk_key.cy * self.chunk_size as i32) as usize;
        let local_z = (gz - chunk_key.cz * self.chunk_size as i32) as usize;
        let bit_index = (local_y * self.chunk_size + local_z) * self.chunk_size + local_x;
        let word_index = bit_index / 64;
        let mask = 1_u64 << (bit_index % 64);
        (self.bits[chunk_index][word_index] & mask) != 0
    }

    fn len(&self) -> usize {
        self.bits
            .iter()
            .map(|words| {
                words
                    .iter()
                    .map(|word| word.count_ones() as usize)
                    .sum::<usize>()
            })
            .sum()
    }
}

#[derive(Default, Clone, Copy)]
struct BlockAccessStats {
    region_chunk_intersections: usize,
    palette_reads: usize,
    target_block_count: usize,
    occluding_cells: usize,
    blockstate_normalize_ms: u128,
    render_semantics_lookup_ms: u128,
    palette_lookup_ms: u128,
    occlusion_precheck_ms: u128,
    chunk_bucket_ms: u128,
}

#[derive(Default, Clone, Copy)]
struct MeshFaceStats {
    generated_faces: usize,
    culled_faces: usize,
    generated_neg_x: usize,
    generated_pos_x: usize,
    generated_neg_y: usize,
    generated_pos_y: usize,
    generated_neg_z: usize,
    generated_pos_z: usize,
    culled_by_neighbor: usize,
    non_full_block_faces_generated: usize,
    non_full_block_occlusion_rule: usize,
    chunk_boundary_faces: usize,
    eligible_fast_path_blocks: usize,
    fast_path_blocks: usize,
    fallback_blocks: usize,
    fast_path_visible_faces: usize,
    fast_path_output_quads: usize,
    fast_path_opaque_full_cube_blocks: usize,
    fast_path_non_occluding_full_cube_blocks: usize,
    fast_path_half_slab_blocks: usize,
    fast_path_carpet_blocks: usize,
    fast_path_stair_half_blocks: usize,
    fast_path_opaque_visible_faces: usize,
    fast_path_non_occluding_visible_faces: usize,
    fast_path_half_slab_visible_faces: usize,
    fast_path_carpet_visible_faces: usize,
    fast_path_stair_half_visible_faces: usize,
    fast_path_opaque_output_quads: usize,
    fast_path_non_occluding_output_quads: usize,
    fast_path_half_slab_output_quads: usize,
    fast_path_carpet_output_quads: usize,
    fast_path_stair_half_output_quads: usize,
    fallback_low_benefit_blocks: usize,
    fallback_non_full_cuboid_blocks: usize,
    fallback_cuboid_half_slab_blocks: usize,
    fallback_cuboid_stair_half_blocks: usize,
    fallback_cuboid_carpet_blocks: usize,
    fallback_cuboid_snow_blocks: usize,
    fallback_crossed_planes_blocks: usize,
    fallback_cross_column_blocks: usize,
    fast_path_used_chunk: bool,
    fast_path_eligible_chunk: bool,
    fast_path_low_benefit_chunk: bool,
}

#[derive(Clone)]
struct BlockRenderInfo {
    geometry: BlockGeometry,
    occludes_neighbors: bool,
}

#[derive(Clone)]
enum BlockGeometry {
    FullCube,
    Cuboid {
        min: [f32; 3],
        max: [f32; 3],
        non_full: bool,
        kind: CuboidKind,
    },
    CrossedPlanes {
        min_y: f32,
        max_y: f32,
        half_width: f32,
    },
    CrossColumn {
        min_y: f32,
        max_y: f32,
        half_thickness: f32,
    },
}

#[derive(Clone, Copy)]
enum CuboidKind {
    HalfSlab,
    StairHalf,
    Carpet,
    SnowLayer,
}

#[derive(Clone, Copy)]
struct PaletteSemantic {
    palette_id: Option<usize>,
    occludes_neighbors: bool,
}

impl BlockRenderInfo {
    fn fast_path_merge_key(&self, palette_id: usize) -> Option<(u32, bool, FastPathClass)> {
        match (&self.geometry, self.occludes_neighbors) {
            (BlockGeometry::FullCube, true) => {
                Some((palette_id as u32, true, FastPathClass::OpaqueFullCube))
            }
            (BlockGeometry::FullCube, false) => fast_path_non_occluding_enabled().then_some((
                palette_id as u32,
                false,
                FastPathClass::NonOccludingFullCube,
            )),
            (
                BlockGeometry::Cuboid {
                    min,
                    max,
                    non_full,
                    kind,
                },
                false,
            ) if *non_full
                && matches!(kind, CuboidKind::HalfSlab)
                && fast_path_half_slab_enabled() =>
            {
                let class = half_slab_class(*min, *max)?;
                Some((palette_id as u32, false, class))
            }
            (
                BlockGeometry::Cuboid {
                    min,
                    max,
                    non_full,
                    kind,
                },
                false,
            ) if *non_full && matches!(kind, CuboidKind::Carpet) && fast_path_carpet_enabled() => {
                carpet_class(*min, *max).map(|class| (palette_id as u32, false, class))
            }
            (
                BlockGeometry::Cuboid {
                    min,
                    max,
                    non_full,
                    kind,
                },
                false,
            ) if *non_full
                && matches!(kind, CuboidKind::StairHalf)
                && fast_path_stair_half_enabled() =>
            {
                let class = stair_half_class(*min, *max)?;
                Some((palette_id as u32, false, class))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum FastPathClass {
    OpaqueFullCube,
    NonOccludingFullCube,
    BottomHalfSlab,
    TopHalfSlab,
    Carpet,
    BottomStairHalf,
    TopStairHalf,
}

fn half_slab_class(min: [f32; 3], max: [f32; 3]) -> Option<FastPathClass> {
    let same = |a: f32, b: f32| (a - b).abs() < f32::EPSILON;
    if same(min[0], 0.0) && same(max[0], 1.0) && same(min[2], 0.0) && same(max[2], 1.0) {
        if same(min[1], 0.0) && same(max[1], 0.5) {
            return Some(FastPathClass::BottomHalfSlab);
        }
        if same(min[1], 0.5) && same(max[1], 1.0) {
            return Some(FastPathClass::TopHalfSlab);
        }
    }
    None
}

fn carpet_class(min: [f32; 3], max: [f32; 3]) -> Option<FastPathClass> {
    let same = |a: f32, b: f32| (a - b).abs() < f32::EPSILON;
    if same(min[0], 0.0)
        && same(max[0], 1.0)
        && same(min[1], 0.0)
        && same(max[1], 0.0625)
        && same(min[2], 0.0)
        && same(max[2], 1.0)
    {
        return Some(FastPathClass::Carpet);
    }
    None
}

fn stair_half_class(min: [f32; 3], max: [f32; 3]) -> Option<FastPathClass> {
    let same = |a: f32, b: f32| (a - b).abs() < f32::EPSILON;
    if same(min[0], 0.0) && same(max[0], 1.0) && same(min[2], 0.0) && same(max[2], 1.0) {
        if same(min[1], 0.0) && same(max[1], 0.5) {
            return Some(FastPathClass::BottomStairHalf);
        }
        if same(min[1], 0.5) && same(max[1], 1.0) {
            return Some(FastPathClass::TopStairHalf);
        }
    }
    None
}

fn sorted_chunk_entries(chunk_counts: &BTreeMap<ChunkKey, u32>) -> Vec<ChunkIndexEntry> {
    let mut entries = chunk_counts
        .iter()
        .map(|(&key, &non_air_blocks)| ChunkIndexEntry {
            key,
            non_air_blocks,
        })
        .collect::<Vec<_>>();
    let focus = scene_focus_chunk(chunk_counts);
    entries.sort_by_key(|entry| {
        let vertical_bias = (entry.key.cy - focus.cy).abs();
        (
            entry.key.horizontal_distance_squared(focus),
            entry.key.cy,
            vertical_bias,
            entry.key.cx,
            entry.key.cz,
        )
    });
    entries
}

fn build_palette_render_info(catalog: &VisualCatalog) -> Vec<BlockRenderInfo> {
    catalog
        .palette
        .iter()
        .map(|entry| {
            let properties = catalog
                .property_pool
                .get(entry.property_id)
                .cloned()
                .unwrap_or_default();
            classify_block_render_info(&entry.block_id, &properties)
        })
        .collect()
}

fn build_region_palette_semantics(
    root: &crate::nbt::LitematicRoot,
    catalog: &VisualCatalog,
    render_info: &[BlockRenderInfo],
) -> Result<HashMap<String, Vec<PaletteSemantic>>> {
    let mut cache = HashMap::<String, Vec<PaletteSemantic>>::with_capacity(root.regions.len());
    for (region_name, region) in &root.regions {
        let normalize_started_at = Instant::now();
        let palette_ids = build_region_palette_ids(region, catalog)?;
        let normalize_ms = normalize_started_at.elapsed().as_millis();
        let render_lookup_started_at = Instant::now();
        let semantics = palette_ids
            .into_iter()
            .map(|palette_id| {
                let occludes_neighbors = palette_id
                    .and_then(|id| render_info.get(id))
                    .map(|info| info.occludes_neighbors)
                    .unwrap_or(false);
                PaletteSemantic {
                    palette_id,
                    occludes_neighbors,
                }
            })
            .collect::<Vec<_>>();
        println!(
            "[MESH_CACHE] region_palette_semantics_cached region={} palette_entries={} blockstate_normalize_ms={} render_semantics_lookup_ms={}",
            region_name,
            semantics.len(),
            normalize_ms,
            render_lookup_started_at.elapsed().as_millis()
        );
        cache.insert(region_name.clone(), semantics);
    }
    Ok(cache)
}

fn classify_block_render_info(
    block_id: &str,
    properties: &BTreeMap<String, String>,
) -> BlockRenderInfo {
    let suffix = block_id.strip_prefix("minecraft:").unwrap_or(block_id);
    let transparent_shell = is_transparent_shell(block_id);
    if suffix.ends_with("_slab") {
        let slab_type = properties
            .get("type")
            .map(|value| value.as_str())
            .unwrap_or("bottom");
        return match slab_type {
            "double" => BlockRenderInfo {
                geometry: BlockGeometry::FullCube,
                occludes_neighbors: true,
            },
            "top" => BlockRenderInfo {
                geometry: BlockGeometry::Cuboid {
                    min: [0.0, 0.5, 0.0],
                    max: [1.0, 1.0, 1.0],
                    non_full: true,
                    kind: CuboidKind::HalfSlab,
                },
                occludes_neighbors: false,
            },
            _ => BlockRenderInfo {
                geometry: BlockGeometry::Cuboid {
                    min: [0.0, 0.0, 0.0],
                    max: [1.0, 0.5, 1.0],
                    non_full: true,
                    kind: CuboidKind::HalfSlab,
                },
                occludes_neighbors: false,
            },
        };
    }
    if suffix.ends_with("_stairs") {
        let half = properties
            .get("half")
            .map(|value| value.as_str())
            .unwrap_or("bottom");
        return BlockRenderInfo {
            geometry: BlockGeometry::Cuboid {
                min: [0.0, if half == "top" { 0.5 } else { 0.0 }, 0.0],
                max: [1.0, if half == "top" { 1.0 } else { 0.5 }, 1.0],
                non_full: true,
                kind: CuboidKind::StairHalf,
            },
            occludes_neighbors: false,
        };
    }
    if suffix == "snow" {
        let layers = properties
            .get("layers")
            .and_then(|value| value.parse::<u8>().ok())
            .map(|value| value.clamp(1, 8))
            .unwrap_or(1);
        let height = (layers as f32 / 8.0).clamp(0.125, 1.0);
        return BlockRenderInfo {
            geometry: BlockGeometry::Cuboid {
                min: [0.0, 0.0, 0.0],
                max: [1.0, height, 1.0],
                non_full: height < 1.0,
                kind: CuboidKind::SnowLayer,
            },
            occludes_neighbors: height >= 1.0,
        };
    }
    if suffix.ends_with("_carpet") || suffix == "moss_carpet" || suffix == "pale_moss_carpet" {
        return BlockRenderInfo {
            geometry: BlockGeometry::Cuboid {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 0.0625, 1.0],
                non_full: true,
                kind: CuboidKind::Carpet,
            },
            occludes_neighbors: false,
        };
    }
    if matches!(
        suffix,
        "grass"
            | "azalea"
            | "flowering_azalea"
            | "cave_vines"
            | "cave_vines_plant"
            | "big_dripleaf"
            | "big_dripleaf_stem"
            | "small_dripleaf"
            | "fern"
            | "dead_bush"
            | "short_grass"
            | "tall_grass"
            | "sunflower"
            | "lilac"
            | "rose_bush"
            | "peony"
            | "large_fern"
            | "dandelion"
            | "poppy"
            | "blue_orchid"
            | "allium"
            | "azure_bluet"
            | "red_tulip"
            | "orange_tulip"
            | "white_tulip"
            | "pink_tulip"
            | "oxeye_daisy"
            | "cornflower"
            | "lily_of_the_valley"
            | "wither_rose"
            | "torchflower"
            | "pitcher_plant"
            | "seagrass"
            | "tall_seagrass"
            | "kelp"
            | "bamboo_sapling"
            | "cactus_flower"
    ) || suffix.ends_with("_sapling")
        || suffix.ends_with("_crop")
        || suffix.ends_with("_flower")
        || suffix.ends_with("_bush")
        || suffix.ends_with("_mushroom")
    {
        return BlockRenderInfo {
            geometry: BlockGeometry::CrossedPlanes {
                min_y: 0.0,
                max_y: 1.0,
                half_width: 0.42,
            },
            occludes_neighbors: false,
        };
    }
    if suffix.ends_with("_pane")
        || matches!(suffix, "iron_bars" | "chain" | "end_rod" | "lightning_rod")
    {
        return BlockRenderInfo {
            geometry: BlockGeometry::CrossColumn {
                min_y: 0.0,
                max_y: 1.0,
                half_thickness: 0.0625,
            },
            occludes_neighbors: false,
        };
    }
    if suffix.ends_with("_wall") || suffix.ends_with("_fence") || suffix.ends_with("_gate") {
        return BlockRenderInfo {
            geometry: BlockGeometry::CrossColumn {
                min_y: 0.0,
                max_y: 1.0,
                half_thickness: 0.125,
            },
            occludes_neighbors: false,
        };
    }
    // Flat / thin attachment blocks that must NOT occlude their neighbors.
    // Falling through to the full-cube default below would cull the faces of
    // adjacent blocks (e.g. redstone wire hiding the top of the block under it,
    // a rail/pressure plate hiding the block it sits on). occludes=false is the
    // safe direction: at worst a few extra faces, never a hole.
    if matches!(
        suffix,
        "redstone_wire"
            | "tripwire"
            | "tripwire_hook"
            | "lever"
            | "torch"
            | "repeater"
            | "comparator"
            | "lily_pad"
            | "hanging_roots"
    ) || suffix.ends_with("_rail")
        || suffix.ends_with("_button")
        || suffix.ends_with("_pressure_plate")
        || suffix.ends_with("_torch")
        || suffix.ends_with("_sign")
        || suffix.ends_with("_banner")
        || suffix.ends_with("_candle")
    {
        return BlockRenderInfo {
            geometry: BlockGeometry::Cuboid {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 0.0625, 1.0],
                non_full: true,
                kind: CuboidKind::Carpet,
            },
            occludes_neighbors: false,
        };
    }
    if transparent_shell {
        return BlockRenderInfo {
            geometry: BlockGeometry::FullCube,
            occludes_neighbors: false,
        };
    }
    BlockRenderInfo {
        geometry: BlockGeometry::FullCube,
        occludes_neighbors: true,
    }
}

fn scene_focus_chunk(chunk_counts: &BTreeMap<ChunkKey, u32>) -> ChunkKey {
    let Some((&first_key, _)) = chunk_counts.first_key_value() else {
        return ChunkKey::new(0, 0, 0);
    };
    let Some((&last_key, _)) = chunk_counts.last_key_value() else {
        return first_key;
    };
    ChunkKey::new(
        (first_key.cx + last_key.cx) / 2,
        (first_key.cy + last_key.cy) / 2,
        (first_key.cz + last_key.cz) / 2,
    )
}

fn is_transparent_shell(block_id: &str) -> bool {
    let suffix = block_id.strip_prefix("minecraft:").unwrap_or(block_id);
    suffix.contains("glass")
        || suffix.contains("ice")
        || suffix.ends_with("leaves")
        || matches!(
            suffix,
            "slime_block" | "honey_block" | "beacon" | "sea_lantern" | "lantern" | "soul_lantern"
        )
}

fn sorted_chunk_coords(entries: &[ChunkIndexEntry]) -> Vec<ChunkCoordOutput> {
    entries
        .iter()
        .map(|entry| ChunkCoordOutput {
            cx: entry.key.cx,
            cy: entry.key.cy,
            cz: entry.key.cz,
            non_air_blocks: entry.non_air_blocks,
        })
        .collect()
}

fn build_region_palette_ids(
    region: &crate::nbt::RegionNbt,
    catalog: &VisualCatalog,
) -> Result<Vec<Option<usize>>> {
    let mut palette_ids = Vec::with_capacity(region.block_state_palette.len());
    for block in &region.block_state_palette {
        let state = format_block_state(block);
        if is_air_state(&state) {
            palette_ids.push(None);
            continue;
        }
        let palette_id = *catalog
            .state_lookup
            .get(&state)
            .ok_or_else(|| anyhow!("missing palette entry for state {}", state))?;
        palette_ids.push(Some(palette_id));
    }
    Ok(palette_ids)
}

fn scan_chunk_counts(
    root: &crate::nbt::LitematicRoot,
    catalog: &VisualCatalog,
    chunk_size: u32,
) -> Result<BTreeMap<ChunkKey, u32>> {
    let Some(bounds) = catalog.bounds else {
        return Ok(BTreeMap::new());
    };

    let chunk_size = chunk_size as i32;
    let mut chunk_counts = BTreeMap::<ChunkKey, u32>::new();
    for region in root.regions.values() {
        let volume = region_volume(&region.size)?;
        let width = i64::from(region.size.x).unsigned_abs() as usize;
        let length = i64::from(region.size.z).unsigned_abs() as usize;
        let nbits = bits_for_palette(region.block_state_palette.len());
        let palette_ids = build_region_palette_ids(region, catalog)?;

        for_each_palette_index(
            &region.block_states,
            volume,
            nbits,
            region.block_state_palette.len(),
            |index, palette_index| {
                if palette_ids.get(palette_index).copied().flatten().is_none() {
                    return Ok(());
                }

                let y = index / (width * length);
                let i_in_layer = index % (width * length);
                let z = i_in_layer / width;
                let x = i_in_layer % width;
                let (rx, ry, rz) = storage_to_region_coords(x, y, z, &region.size);
                let global_x = region.position.x + rx - bounds.min_x;
                let global_y = region.position.y + ry - bounds.min_y;
                let global_z = region.position.z + rz - bounds.min_z;
                let key = ChunkKey::new(
                    global_x.div_euclid(chunk_size),
                    global_y.div_euclid(chunk_size),
                    global_z.div_euclid(chunk_size),
                );
                *chunk_counts.entry(key).or_insert(0) += 1;
                Ok(())
            },
        )?;
    }

    Ok(chunk_counts)
}

fn select_chunk_batch(
    entries: &[ChunkIndexEntry],
    offset: usize,
    limit: usize,
) -> Vec<ChunkIndexEntry> {
    entries.iter().skip(offset).take(limit).cloned().collect()
}

fn expand_target_chunks(target_set: &HashSet<ChunkKey>) -> HashSet<ChunkKey> {
    let mut expanded = HashSet::with_capacity(target_set.len() * 27);
    for &key in target_set {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    expanded.insert(ChunkKey::new(key.cx + dx, key.cy + dy, key.cz + dz));
                }
            }
        }
    }
    expanded
}

fn chunk_global_bounds(chunk_key: ChunkKey, chunk_size: i32) -> RegionBounds {
    RegionBounds {
        min_x: chunk_key.cx * chunk_size,
        max_x: chunk_key.cx * chunk_size + chunk_size - 1,
        min_y: chunk_key.cy * chunk_size,
        max_y: chunk_key.cy * chunk_size + chunk_size - 1,
        min_z: chunk_key.cz * chunk_size,
        max_z: chunk_key.cz * chunk_size + chunk_size - 1,
    }
}

fn intersect_bounds(a: RegionBounds, b: RegionBounds) -> Option<RegionBounds> {
    let min_x = a.min_x.max(b.min_x);
    let max_x = a.max_x.min(b.max_x);
    let min_y = a.min_y.max(b.min_y);
    let max_y = a.max_y.min(b.max_y);
    let min_z = a.min_z.max(b.min_z);
    let max_z = a.max_z.min(b.max_z);
    if min_x > max_x || min_y > max_y || min_z > max_z {
        return None;
    }
    Some(RegionBounds {
        min_x,
        max_x,
        min_y,
        max_y,
        min_z,
        max_z,
    })
}

fn translate_region_bounds(region: &crate::nbt::RegionNbt, bounds: RegionBounds) -> RegionBounds {
    let region_bounds = region_bounds(region);
    RegionBounds {
        min_x: region_bounds.min_x - bounds.min_x,
        max_x: region_bounds.max_x - bounds.min_x,
        min_y: region_bounds.min_y - bounds.min_y,
        max_y: region_bounds.max_y - bounds.min_y,
        min_z: region_bounds.min_z - bounds.min_z,
        max_z: region_bounds.max_z - bounds.min_z,
    }
}

fn collect_target_chunk_blocks(
    root: &crate::nbt::LitematicRoot,
    catalog: &VisualCatalog,
    region_palette_semantics: &HashMap<String, Vec<PaletteSemantic>>,
    chunk_size: u32,
    selected: &[ChunkKey],
    chunk_counts: &BTreeMap<ChunkKey, u32>,
    non_occluding_palette: Option<&HashSet<usize>>,
) -> Result<(OccupancyGrid, Vec<Vec<TargetBlock>>, BlockAccessStats)> {
    let Some(bounds) = catalog.bounds else {
        return Ok((
            OccupancyGrid::new(chunk_size as usize, Vec::new()),
            Vec::new(),
            BlockAccessStats::default(),
        ));
    };

    let chunk_size = chunk_size as i32;
    let target_set = selected.iter().copied().collect::<HashSet<_>>();
    let expanded_chunks = expand_target_chunks(&target_set)
        .into_iter()
        .collect::<Vec<_>>();
    let selected_index = selected
        .iter()
        .copied()
        .enumerate()
        .map(|(index, key)| (key, index))
        .collect::<HashMap<_, _>>();
    let occupied_capacity = expanded_chunks
        .iter()
        .filter_map(|key| chunk_counts.get(key))
        .copied()
        .sum::<u32>() as usize;
    let mut occupied = OccupancyGrid::new(chunk_size as usize, expanded_chunks.clone());
    let mut chunk_blocks = selected
        .iter()
        .map(|chunk_key| {
            let capacity = chunk_counts.get(chunk_key).copied().unwrap_or(0) as usize;
            Vec::<TargetBlock>::with_capacity(capacity)
        })
        .collect::<Vec<_>>();
    let mut access_stats = BlockAccessStats::default();

    for (region_name, region) in &root.regions {
        let region_global_bounds = translate_region_bounds(region, bounds);
        let volume = region_volume(&region.size)?;
        let width = i64::from(region.size.x).unsigned_abs() as usize;
        let length = i64::from(region.size.z).unsigned_abs() as usize;
        let layer_stride = width * length;
        let nbits = bits_for_palette(region.block_state_palette.len());
        let palette_semantics = region_palette_semantics
            .get(region_name)
            .ok_or_else(|| anyhow!("missing palette semantics for region {}", region_name))?;
        let mut decoded_palette_indices = Vec::<usize>::with_capacity(volume);
        for_each_palette_index(
            &region.block_states,
            volume,
            nbits,
            region.block_state_palette.len(),
            |_, palette_index| {
                decoded_palette_indices.push(palette_index);
                Ok(())
            },
        )?;

        let palette_lookup_started_at = Instant::now();
        for (expanded_index, &chunk_key) in expanded_chunks.iter().enumerate() {
            access_stats.region_chunk_intersections += 1;
            let Some(intersection) = intersect_bounds(
                region_global_bounds,
                chunk_global_bounds(chunk_key, chunk_size),
            ) else {
                continue;
            };
            let target_bucket_index = selected_index.get(&chunk_key).copied();
            let occlusion_started_at = Instant::now();
            let chunk_bucket_started_at = target_bucket_index.map(|_| Instant::now());
            let chunk_origin_x = chunk_key.cx * chunk_size;
            let chunk_origin_y = chunk_key.cy * chunk_size;
            let chunk_origin_z = chunk_key.cz * chunk_size;
            for gy in intersection.min_y..=intersection.max_y {
                let region_y = gy + bounds.min_y - region.position.y;
                let Some(store_y) = region_coord_to_storage_coord(region_y, region.size.y) else {
                    continue;
                };
                for gz in intersection.min_z..=intersection.max_z {
                    let region_z = gz + bounds.min_z - region.position.z;
                    let Some(store_z) = region_coord_to_storage_coord(region_z, region.size.z)
                    else {
                        continue;
                    };
                    for gx in intersection.min_x..=intersection.max_x {
                        let region_x = gx + bounds.min_x - region.position.x;
                        let Some(store_x) = region_coord_to_storage_coord(region_x, region.size.x)
                        else {
                            continue;
                        };

                        let index = store_y * layer_stride + store_z * width + store_x;
                        let Some(&palette_index) = decoded_palette_indices.get(index) else {
                            continue;
                        };
                        access_stats.palette_reads += 1;
                        let Some(semantic) = palette_semantics.get(palette_index).copied() else {
                            continue;
                        };
                        let Some(palette_id) = semantic.palette_id else {
                            continue;
                        };

                        if semantic.occludes_neighbors
                            && !non_occluding_palette
                                .is_some_and(|palette| palette.contains(&palette_id))
                        {
                            occupied.set_local(
                                expanded_index,
                                (gx - chunk_origin_x) as usize,
                                (gy - chunk_origin_y) as usize,
                                (gz - chunk_origin_z) as usize,
                            );
                        }
                        if let Some(bucket_index) = target_bucket_index {
                            chunk_blocks[bucket_index].push(TargetBlock {
                                gx,
                                gy,
                                gz,
                                palette_id,
                            });
                            access_stats.target_block_count += 1;
                        }
                    }
                }
            }
            access_stats.occlusion_precheck_ms += occlusion_started_at.elapsed().as_millis();
            if let Some(bucket_started_at) = chunk_bucket_started_at {
                access_stats.chunk_bucket_ms += bucket_started_at.elapsed().as_millis();
            }
        }
        access_stats.palette_lookup_ms += palette_lookup_started_at.elapsed().as_millis();
    }

    let _ = occupied_capacity;
    access_stats.occluding_cells = occupied.len();
    Ok((occupied, chunk_blocks, access_stats))
}

fn build_mesh_chunk_output(
    key: ChunkKey,
    chunk_size: u32,
    blocks: Vec<TargetBlock>,
    occupied: &OccupancyGrid,
    render_info: &[BlockRenderInfo],
    catalog: &VisualCatalog,
) -> (MeshChunkOutput, MeshFaceStats) {
    let mut vertices = Vec::<[f32; 3]>::new();
    let mut indices = Vec::<u32>::new();
    let mut color_indices = Vec::<u32>::new();
    let mut compact_surfaces = Vec::<CompactSurfaceOutput>::new();
    let mut face_stats = MeshFaceStats::default();
    let use_fast_path = fast_path_enabled();
    let chunk_size_usize = chunk_size as usize;
    let chunk_volume = chunk_size_usize
        .saturating_mul(chunk_size_usize)
        .saturating_mul(chunk_size_usize);
    let chunk_origin_x = key.cx * chunk_size as i32;
    let chunk_origin_y = key.cy * chunk_size as i32;
    let chunk_origin_z = key.cz * chunk_size as i32;
    let mut fast_path_grid = vec![FAST_PATH_EMPTY; chunk_volume];
    let mut fast_path_occludes_grid = vec![false; chunk_volume];
    let mut fast_path_class_grid = vec![0_u8; chunk_volume];
    let mut fast_path_block_count = 0_usize;
    let mut fast_path_non_occluding_count = 0_usize;
    let mut fast_path_half_slab_count = 0_usize;
    let mut fast_path_carpet_count = 0_usize;
    let mut fast_path_stair_half_count = 0_usize;
    let mut fallback_blocks = Vec::<TargetBlock>::new();

    for block in blocks {
        let Some(info) = render_info.get(block.palette_id) else {
            continue;
        };
        if use_fast_path
            && let Some((merge_key, occludes_neighbors, fast_path_class)) =
                info.fast_path_merge_key(block.palette_id)
        {
            let local_x = (block.gx - chunk_origin_x) as usize;
            let local_y = (block.gy - chunk_origin_y) as usize;
            let local_z = (block.gz - chunk_origin_z) as usize;
            let index = fast_path_grid_index(chunk_size_usize, local_x, local_y, local_z);
            fast_path_grid[index] = merge_key;
            fast_path_occludes_grid[index] = occludes_neighbors;
            fast_path_class_grid[index] = match fast_path_class {
                FastPathClass::OpaqueFullCube => 1,
                FastPathClass::NonOccludingFullCube => 2,
                FastPathClass::BottomHalfSlab => 3,
                FastPathClass::TopHalfSlab => 4,
                FastPathClass::Carpet => 5,
                FastPathClass::BottomStairHalf => 6,
                FastPathClass::TopStairHalf => 7,
            };
            fast_path_block_count += 1;
            face_stats.eligible_fast_path_blocks += 1;
            face_stats.fast_path_eligible_chunk = true;
            match fast_path_class {
                FastPathClass::OpaqueFullCube => face_stats.fast_path_opaque_full_cube_blocks += 1,
                FastPathClass::NonOccludingFullCube => {
                    face_stats.fast_path_non_occluding_full_cube_blocks += 1;
                    fast_path_non_occluding_count += 1;
                }
                FastPathClass::BottomHalfSlab | FastPathClass::TopHalfSlab => {
                    face_stats.fast_path_half_slab_blocks += 1;
                    fast_path_half_slab_count += 1;
                }
                FastPathClass::Carpet => {
                    face_stats.fast_path_carpet_blocks += 1;
                    fast_path_carpet_count += 1;
                }
                FastPathClass::BottomStairHalf | FastPathClass::TopStairHalf => {
                    face_stats.fast_path_stair_half_blocks += 1;
                    fast_path_stair_half_count += 1;
                }
            }
            continue;
        }
        match &info.geometry {
            BlockGeometry::Cuboid { kind, .. } => {
                face_stats.fallback_non_full_cuboid_blocks += 1;
                match kind {
                    CuboidKind::HalfSlab => face_stats.fallback_cuboid_half_slab_blocks += 1,
                    CuboidKind::StairHalf => face_stats.fallback_cuboid_stair_half_blocks += 1,
                    CuboidKind::Carpet => face_stats.fallback_cuboid_carpet_blocks += 1,
                    CuboidKind::SnowLayer => face_stats.fallback_cuboid_snow_blocks += 1,
                }
            }
            BlockGeometry::CrossedPlanes { .. } => face_stats.fallback_crossed_planes_blocks += 1,
            BlockGeometry::CrossColumn { .. } => face_stats.fallback_cross_column_blocks += 1,
            BlockGeometry::FullCube => {}
        }
        fallback_blocks.push(block);
    }

    let use_greedy_fast_path = use_fast_path
        && fast_path_block_count >= chunk_size_usize * chunk_size_usize
        && fast_path_block_count >= fallback_blocks.len().saturating_mul(2)
        && (fast_path_non_occluding_count == 0
            || fast_path_non_occluding_count >= chunk_size_usize * chunk_size_usize * 4
            || fast_path_non_occluding_count * 8 <= fast_path_block_count)
        && (fast_path_half_slab_count == 0
            || fast_path_half_slab_count >= chunk_size_usize * 2
            || fast_path_half_slab_count * 6 <= fast_path_block_count)
        && (fast_path_carpet_count == 0
            || fast_path_carpet_count >= chunk_size_usize * 2
            || fast_path_carpet_count * 6 <= fast_path_block_count)
        && (fast_path_stair_half_count == 0
            || fast_path_stair_half_count >= chunk_size_usize * 2
            || fast_path_stair_half_count * 6 <= fast_path_block_count);
    if !use_greedy_fast_path && fast_path_block_count > 0 {
        face_stats.fast_path_low_benefit_chunk = true;
        face_stats.fallback_low_benefit_blocks += fast_path_block_count;
        for (index, palette_id) in fast_path_grid.iter().copied().enumerate() {
            if palette_id == FAST_PATH_EMPTY {
                continue;
            }
            let (local_x, local_y, local_z) = fast_path_grid_coords(chunk_size_usize, index);
            fallback_blocks.push(TargetBlock {
                gx: chunk_origin_x + local_x as i32,
                gy: chunk_origin_y + local_y as i32,
                gz: chunk_origin_z + local_z as i32,
                palette_id: palette_id as usize,
            });
        }
        fast_path_block_count = 0;
        fast_path_grid.fill(FAST_PATH_EMPTY);
        fast_path_occludes_grid.fill(false);
        fast_path_class_grid.fill(0);
    }

    face_stats.fast_path_blocks = fast_path_block_count;
    face_stats.fallback_blocks = fallback_blocks.len();
    if fast_path_block_count > 0 {
        face_stats.fast_path_used_chunk = true;
        emit_greedy_full_cube_faces(
            &mut vertices,
            &mut indices,
            &mut color_indices,
            &mut compact_surfaces,
            &mut face_stats,
            catalog,
            key,
            chunk_size,
            &fast_path_grid,
            &fast_path_occludes_grid,
            &fast_path_class_grid,
            occupied,
        );
    }

    for block in fallback_blocks {
        let Some(info) = render_info.get(block.palette_id) else {
            continue;
        };
        match &info.geometry {
            BlockGeometry::FullCube => emit_cuboid(
                &mut vertices,
                &mut indices,
                &mut color_indices,
                &mut face_stats,
                CuboidSpec {
                    key,
                    chunk_size,
                    palette_id: block.palette_id,
                    min: [0.0, 0.0, 0.0],
                    max: [1.0, 1.0, 1.0],
                    non_full: false,
                    neighbor_culling: true,
                },
                block.gx,
                block.gy,
                block.gz,
                occupied,
                render_info,
            ),
            BlockGeometry::Cuboid {
                min, max, non_full, ..
            } => emit_cuboid(
                &mut vertices,
                &mut indices,
                &mut color_indices,
                &mut face_stats,
                CuboidSpec {
                    key,
                    chunk_size,
                    palette_id: block.palette_id,
                    min: *min,
                    max: *max,
                    non_full: *non_full,
                    neighbor_culling: false,
                },
                block.gx,
                block.gy,
                block.gz,
                occupied,
                render_info,
            ),
            BlockGeometry::CrossedPlanes {
                min_y,
                max_y,
                half_width,
            } => emit_crossed_planes(
                &mut vertices,
                &mut indices,
                &mut color_indices,
                &mut face_stats,
                block,
                *min_y,
                *max_y,
                *half_width,
            ),
            BlockGeometry::CrossColumn {
                min_y,
                max_y,
                half_thickness,
            } => {
                let center = 0.5_f32;
                emit_cuboid(
                    &mut vertices,
                    &mut indices,
                    &mut color_indices,
                    &mut face_stats,
                    CuboidSpec {
                        key,
                        chunk_size,
                        palette_id: block.palette_id,
                        min: [center - *half_thickness, *min_y, 0.0],
                        max: [center + *half_thickness, *max_y, 1.0],
                        non_full: true,
                        neighbor_culling: false,
                    },
                    block.gx,
                    block.gy,
                    block.gz,
                    occupied,
                    render_info,
                );
                emit_cuboid(
                    &mut vertices,
                    &mut indices,
                    &mut color_indices,
                    &mut face_stats,
                    CuboidSpec {
                        key,
                        chunk_size,
                        palette_id: block.palette_id,
                        min: [0.0, *min_y, center - *half_thickness],
                        max: [1.0, *max_y, center + *half_thickness],
                        non_full: true,
                        neighbor_culling: false,
                    },
                    block.gx,
                    block.gy,
                    block.gz,
                    occupied,
                    render_info,
                );
            }
        }
    }

    (
        MeshChunkOutput {
            cx: key.cx,
            cy: key.cy,
            cz: key.cz,
            vertices,
            indices,
            color_indices,
            compact_surfaces,
            textured_vertices: Vec::new(),
            translucent_indices: Vec::new(),
        },
        face_stats,
    )
}

fn glass_sandwich_trace_enabled() -> bool {
    std::env::var("LBA_GLASS_SANDWICH_TRACE")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false)
}

fn froglight_sandwich_trace_enabled() -> bool {
    std::env::var("LBA_FROGLIGHT_SANDWICH_TRACE")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false)
}

fn face_index_label(face_index: usize) -> &'static str {
    match face_index {
        0 => "west",
        1 => "east",
        2 => "down",
        3 => "up",
        4 => "north",
        5 => "south",
        _ => "?",
    }
}

fn opposite_face_index(face_index: usize) -> usize {
    match face_index {
        0 => 1,
        1 => 0,
        2 => 3,
        3 => 2,
        4 => 5,
        5 => 4,
        _ => face_index,
    }
}

fn is_froglight_local(local: &str) -> bool {
    local.ends_with("_froglight")
}

fn froglight_trace_palette_local(catalog: &VisualCatalog, palette_id: usize) -> Option<&str> {
    catalog.palette.get(palette_id).map(|entry| {
        entry
            .block_id
            .strip_prefix("minecraft:")
            .unwrap_or(entry.block_id.as_str())
    })
}

fn froglight_trace_should_log_palette(catalog: &VisualCatalog, palette_id: usize) -> bool {
    froglight_trace_palette_local(catalog, palette_id).is_some_and(is_froglight_local)
}

fn froglight_trace_log_visible_face(
    catalog: &VisualCatalog,
    palette_id: usize,
    face_index: usize,
    gx: i32,
    gy: i32,
    gz: i32,
    width: i32,
    height: i32,
    class_id: u8,
    route: &str,
) {
    if !froglight_sandwich_trace_enabled()
        || !froglight_trace_should_log_palette(catalog, palette_id)
    {
        return;
    }
    println!(
        "[LBA_FROGLIGHT_SANDWICH_TRACE_20260424A] stage=visible_face route={} pos=({}, {}, {}) local={} face={} class_id={} span={}x{}",
        route,
        gx,
        gy,
        gz,
        froglight_trace_palette_local(catalog, palette_id).unwrap_or("<none>"),
        face_index_label(face_index),
        class_id,
        width,
        height,
    );
}

fn froglight_trace_log_output_quad(
    stage: &str,
    catalog: &VisualCatalog,
    palette_id: usize,
    face_index: usize,
    min: [i32; 3],
    max: [i32; 3],
    width: i32,
    height: i32,
    route: &str,
) {
    if !froglight_sandwich_trace_enabled()
        || !froglight_trace_should_log_palette(catalog, palette_id)
    {
        return;
    }
    println!(
        "[LBA_FROGLIGHT_SANDWICH_TRACE_20260424A] stage={} route={} local={} face={} bounds=({}, {}, {})..({}, {}, {}) span={}x{}",
        stage,
        route,
        froglight_trace_palette_local(catalog, palette_id).unwrap_or("<none>"),
        face_index_label(face_index),
        min[0],
        min[1],
        min[2],
        max[0],
        max[1],
        max[2],
        width,
        height,
    );
}

struct CuboidSpec {
    key: ChunkKey,
    chunk_size: u32,
    palette_id: usize,
    min: [f32; 3],
    max: [f32; 3],
    non_full: bool,
    neighbor_culling: bool,
}

fn emit_greedy_full_cube_faces(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    compact_surfaces: &mut Vec<CompactSurfaceOutput>,
    face_stats: &mut MeshFaceStats,
    catalog: &VisualCatalog,
    chunk_key: ChunkKey,
    chunk_size: u32,
    fast_path_grid: &[u32],
    fast_path_occludes_grid: &[bool],
    fast_path_class_grid: &[u8],
    occupied: &OccupancyGrid,
) {
    let chunk_size_i32 = chunk_size as i32;
    let chunk_size_usize = chunk_size as usize;
    let base_x = chunk_key.cx * chunk_size_i32;
    let base_y = chunk_key.cy * chunk_size_i32;
    let base_z = chunk_key.cz * chunk_size_i32;
    let mask_len = (chunk_size_i32 * chunk_size_i32) as usize;
    let mut mask = vec![FAST_PATH_EMPTY; mask_len];
    let mut mask_class = vec![0_u8; mask_len];

    emit_half_slab_side_faces(
        vertices,
        indices,
        color_indices,
        face_stats,
        chunk_key,
        chunk_size,
        fast_path_grid,
        fast_path_class_grid,
    );

    for face_index in 0..6 {
        for slice in 0..chunk_size_i32 {
            mask.fill(FAST_PATH_EMPTY);
            mask_class.fill(0);
            for v in 0..chunk_size_i32 {
                for u in 0..chunk_size_i32 {
                    let (gx, gy, gz) =
                        fast_path_face_cell_world(face_index, base_x, base_y, base_z, slice, u, v);
                    let local_x = (gx - base_x) as usize;
                    let local_y = (gy - base_y) as usize;
                    let local_z = (gz - base_z) as usize;
                    let index_3d =
                        fast_path_grid_index(chunk_size_usize, local_x, local_y, local_z);
                    let palette_id = fast_path_grid[index_3d];
                    if palette_id == FAST_PATH_EMPTY {
                        continue;
                    }
                    let class_id = fast_path_class_grid[index_3d];
                    let face_visible = match class_id {
                        1 | 2 => is_fast_path_face_visible(
                            chunk_size_usize,
                            gx,
                            gy,
                            gz,
                            face_index,
                            base_x,
                            base_y,
                            base_z,
                            fast_path_grid,
                            fast_path_occludes_grid,
                            occupied,
                        ),
                        3..=7 => matches!(face_index, 2 | 3),
                        _ => false,
                    };
                    if face_visible {
                        let index = (v * chunk_size_i32 + u) as usize;
                        mask[index] = palette_id;
                        mask_class[index] = class_id;
                        froglight_trace_log_visible_face(
                            catalog,
                            palette_id as usize,
                            face_index,
                            gx,
                            gy,
                            gz,
                            1,
                            1,
                            class_id,
                            if compact_cache_v2_enabled() && class_id == 1 {
                                "compact_candidate"
                            } else {
                                "greedy_candidate"
                            },
                        );
                        face_stats.fast_path_visible_faces += 1;
                        match class_id {
                            1 => face_stats.fast_path_opaque_visible_faces += 1,
                            2 => face_stats.fast_path_non_occluding_visible_faces += 1,
                            3 | 4 => face_stats.fast_path_half_slab_visible_faces += 1,
                            5 => face_stats.fast_path_carpet_visible_faces += 1,
                            6 | 7 => face_stats.fast_path_stair_half_visible_faces += 1,
                            _ => {}
                        }
                    } else if matches!(class_id, 1 | 2) {
                        face_stats.culled_faces += 1;
                        face_stats.culled_by_neighbor += 1;
                    }
                }
            }

            let width = chunk_size_i32 as usize;
            let height = chunk_size_i32 as usize;
            let mut row = 0_usize;
            while row < height {
                let mut col = 0_usize;
                while col < width {
                    let index = row * width + col;
                    let palette_id = mask[index];
                    if palette_id == FAST_PATH_EMPTY {
                        col += 1;
                        continue;
                    }
                    let class_id = mask_class[index];

                    let mut quad_width = 1_usize;
                    while col + quad_width < width
                        && mask[row * width + col + quad_width] == palette_id
                        && mask_class[row * width + col + quad_width] == class_id
                    {
                        quad_width += 1;
                    }

                    let mut quad_height = 1_usize;
                    'grow: while row + quad_height < height {
                        for span_x in 0..quad_width {
                            if mask[(row + quad_height) * width + col + span_x] != palette_id
                                || mask_class[(row + quad_height) * width + col + span_x]
                                    != class_id
                            {
                                break 'grow;
                            }
                        }
                        quad_height += 1;
                    }

                    if matches!(class_id, 3..=7) {
                        emit_greedy_non_full_horizontal_quad(
                            vertices,
                            indices,
                            color_indices,
                            face_stats,
                            face_index,
                            chunk_key,
                            chunk_size,
                            palette_id as usize,
                            class_id,
                            base_x,
                            base_y,
                            base_z,
                            slice,
                            col as i32,
                            row as i32,
                            quad_width as i32,
                            quad_height as i32,
                        );
                    } else if compact_cache_v2_enabled() && class_id == 1 {
                        emit_compact_surface_quad(
                            compact_surfaces,
                            catalog,
                            face_index,
                            palette_id as usize,
                            base_x,
                            base_y,
                            base_z,
                            slice,
                            col as i32,
                            row as i32,
                            quad_width as i32,
                            quad_height as i32,
                        );
                        face_stats.fast_path_output_quads += 1;
                    } else {
                        emit_greedy_face_quad(
                            vertices,
                            indices,
                            color_indices,
                            face_stats,
                            catalog,
                            face_index,
                            chunk_key,
                            chunk_size,
                            palette_id as usize,
                            base_x,
                            base_y,
                            base_z,
                            slice,
                            col as i32,
                            row as i32,
                            quad_width as i32,
                            quad_height as i32,
                        );
                    }
                    match class_id {
                        1 => face_stats.fast_path_opaque_output_quads += 1,
                        2 => face_stats.fast_path_non_occluding_output_quads += 1,
                        3 | 4 => face_stats.fast_path_half_slab_output_quads += 1,
                        5 => face_stats.fast_path_carpet_output_quads += 1,
                        6 | 7 => face_stats.fast_path_stair_half_output_quads += 1,
                        _ => {}
                    }

                    for clear_y in 0..quad_height {
                        for clear_x in 0..quad_width {
                            mask[(row + clear_y) * width + col + clear_x] = FAST_PATH_EMPTY;
                            mask_class[(row + clear_y) * width + col + clear_x] = 0;
                        }
                    }
                    col += quad_width;
                }
                row += 1;
            }
        }
    }
}

fn emit_half_slab_side_faces(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    face_stats: &mut MeshFaceStats,
    chunk_key: ChunkKey,
    chunk_size: u32,
    fast_path_grid: &[u32],
    fast_path_class_grid: &[u8],
) {
    let chunk_size_usize = chunk_size as usize;
    let base_x = chunk_key.cx * chunk_size as i32;
    let base_y = chunk_key.cy * chunk_size as i32;
    let base_z = chunk_key.cz * chunk_size as i32;
    for (index, palette_id) in fast_path_grid.iter().copied().enumerate() {
        if palette_id == FAST_PATH_EMPTY {
            continue;
        }
        let class_id = fast_path_class_grid[index];
        if !matches!(class_id, 3..=7) {
            continue;
        }
        let (local_x, local_y, local_z) = fast_path_grid_coords(chunk_size_usize, index);
        let (min_y, max_y) = match class_id {
            3 => (0.0, 0.5),
            4 => (0.5, 1.0),
            5 => (0.0, 0.0625),
            6 => (0.0, 0.5),
            7 => (0.5, 1.0),
            _ => unreachable!("invalid non-full fast path class"),
        };
        for face_index in [0_usize, 1, 4, 5] {
            emit_axis_aligned_face(
                vertices,
                indices,
                color_indices,
                face_stats,
                face_index,
                chunk_key,
                chunk_size,
                palette_id as usize,
                base_x + local_x as i32,
                base_y + local_y as i32,
                base_z + local_z as i32,
                [0.0, min_y, 0.0],
                [1.0, max_y, 1.0],
                true,
            );
            face_stats.fast_path_visible_faces += 1;
            face_stats.fast_path_output_quads += 1;
            if class_id == 5 {
                face_stats.fast_path_carpet_visible_faces += 1;
                face_stats.fast_path_carpet_output_quads += 1;
            } else if matches!(class_id, 6 | 7) {
                face_stats.fast_path_stair_half_visible_faces += 1;
                face_stats.fast_path_stair_half_output_quads += 1;
            } else {
                face_stats.fast_path_half_slab_visible_faces += 1;
                face_stats.fast_path_half_slab_output_quads += 1;
            }
        }
    }
}

fn fast_path_grid_index(
    chunk_size: usize,
    local_x: usize,
    local_y: usize,
    local_z: usize,
) -> usize {
    (local_y * chunk_size + local_z) * chunk_size + local_x
}

fn fast_path_grid_coords(chunk_size: usize, index: usize) -> (usize, usize, usize) {
    let local_x = index % chunk_size;
    let yz = index / chunk_size;
    let local_z = yz % chunk_size;
    let local_y = yz / chunk_size;
    (local_x, local_y, local_z)
}

fn fast_path_face_cell_world(
    face_index: usize,
    base_x: i32,
    base_y: i32,
    base_z: i32,
    slice: i32,
    u: i32,
    v: i32,
) -> (i32, i32, i32) {
    match face_index {
        0 | 1 => (base_x + slice, base_y + v, base_z + u),
        2 | 3 => (base_x + u, base_y + slice, base_z + v),
        4 | 5 => (base_x + u, base_y + v, base_z + slice),
        _ => unreachable!("invalid face index"),
    }
}

fn is_fast_path_face_visible(
    chunk_size: usize,
    gx: i32,
    gy: i32,
    gz: i32,
    face_index: usize,
    base_x: i32,
    base_y: i32,
    base_z: i32,
    fast_path_grid: &[u32],
    fast_path_occludes_grid: &[bool],
    occupied: &OccupancyGrid,
) -> bool {
    let offset = FACE_NEIGHBORS[face_index];
    let neighbor = (gx + offset[0], gy + offset[1], gz + offset[2]);
    let local_neighbor_x = neighbor.0 - base_x;
    let local_neighbor_y = neighbor.1 - base_y;
    let local_neighbor_z = neighbor.2 - base_z;
    if local_neighbor_x >= 0
        && local_neighbor_x < chunk_size as i32
        && local_neighbor_y >= 0
        && local_neighbor_y < chunk_size as i32
        && local_neighbor_z >= 0
        && local_neighbor_z < chunk_size as i32
    {
        let index = fast_path_grid_index(
            chunk_size,
            local_neighbor_x as usize,
            local_neighbor_y as usize,
            local_neighbor_z as usize,
        );
        if fast_path_grid[index] != FAST_PATH_EMPTY && fast_path_occludes_grid[index] {
            return false;
        }
    }
    !occupied.contains_world(neighbor.0, neighbor.1, neighbor.2)
}

fn emit_greedy_face_quad(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    face_stats: &mut MeshFaceStats,
    catalog: &VisualCatalog,
    face_index: usize,
    chunk_key: ChunkKey,
    chunk_size: u32,
    palette_id: usize,
    base_x: i32,
    base_y: i32,
    base_z: i32,
    slice: i32,
    start_u: i32,
    start_v: i32,
    width: i32,
    height: i32,
) {
    let (min, max) = match face_index {
        0 => (
            [base_x + slice, base_y + start_v, base_z + start_u],
            [
                base_x + slice,
                base_y + start_v + height,
                base_z + start_u + width,
            ],
        ),
        1 => (
            [base_x + slice + 1, base_y + start_v, base_z + start_u],
            [
                base_x + slice + 1,
                base_y + start_v + height,
                base_z + start_u + width,
            ],
        ),
        2 => (
            [base_x + start_u, base_y + slice, base_z + start_v],
            [
                base_x + start_u + width,
                base_y + slice,
                base_z + start_v + height,
            ],
        ),
        3 => (
            [base_x + start_u, base_y + slice + 1, base_z + start_v],
            [
                base_x + start_u + width,
                base_y + slice + 1,
                base_z + start_v + height,
            ],
        ),
        4 => (
            [base_x + start_u, base_y + start_v, base_z + slice],
            [
                base_x + start_u + width,
                base_y + start_v + height,
                base_z + slice,
            ],
        ),
        5 => (
            [base_x + start_u, base_y + start_v, base_z + slice + 1],
            [
                base_x + start_u + width,
                base_y + start_v + height,
                base_z + slice + 1,
            ],
        ),
        _ => unreachable!("invalid face index"),
    };

    froglight_trace_log_output_quad(
        "output_quad",
        catalog,
        palette_id,
        face_index,
        min,
        max,
        width,
        height,
        "greedy_face_quad",
    );

    emit_axis_aligned_face_bounds(
        vertices,
        indices,
        color_indices,
        face_stats,
        face_index,
        chunk_key,
        chunk_size,
        palette_id,
        [min[0] as f32, min[1] as f32, min[2] as f32],
        [max[0] as f32, max[1] as f32, max[2] as f32],
        false,
    );
    face_stats.fast_path_output_quads += 1;
}

fn emit_compact_surface_quad(
    compact_surfaces: &mut Vec<CompactSurfaceOutput>,
    catalog: &VisualCatalog,
    face_index: usize,
    palette_id: usize,
    base_x: i32,
    base_y: i32,
    base_z: i32,
    slice: i32,
    start_u: i32,
    start_v: i32,
    width: i32,
    height: i32,
) {
    let (min, max) = match face_index {
        0 => (
            [base_x + slice, base_y + start_v, base_z + start_u],
            [
                base_x + slice,
                base_y + start_v + height,
                base_z + start_u + width,
            ],
        ),
        1 => (
            [base_x + slice + 1, base_y + start_v, base_z + start_u],
            [
                base_x + slice + 1,
                base_y + start_v + height,
                base_z + start_u + width,
            ],
        ),
        2 => (
            [base_x + start_u, base_y + slice, base_z + start_v],
            [
                base_x + start_u + width,
                base_y + slice,
                base_z + start_v + height,
            ],
        ),
        3 => (
            [base_x + start_u, base_y + slice + 1, base_z + start_v],
            [
                base_x + start_u + width,
                base_y + slice + 1,
                base_z + start_v + height,
            ],
        ),
        4 => (
            [base_x + start_u, base_y + start_v, base_z + slice],
            [
                base_x + start_u + width,
                base_y + start_v + height,
                base_z + slice,
            ],
        ),
        5 => (
            [base_x + start_u, base_y + start_v, base_z + slice + 1],
            [
                base_x + start_u + width,
                base_y + start_v + height,
                base_z + slice + 1,
            ],
        ),
        _ => unreachable!("invalid face index"),
    };
    froglight_trace_log_output_quad(
        "output_quad",
        catalog,
        palette_id,
        face_index,
        min,
        max,
        width,
        height,
        "compact_surface",
    );
    compact_surfaces.push(CompactSurfaceOutput {
        face_index: face_index as u8,
        min: [min[0] as f32, min[1] as f32, min[2] as f32],
        max: [max[0] as f32, max[1] as f32, max[2] as f32],
        palette_id: palette_id as u32,
    });
}

fn emit_greedy_non_full_horizontal_quad(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    face_stats: &mut MeshFaceStats,
    face_index: usize,
    chunk_key: ChunkKey,
    chunk_size: u32,
    palette_id: usize,
    class_id: u8,
    base_x: i32,
    base_y: i32,
    base_z: i32,
    slice: i32,
    start_u: i32,
    start_v: i32,
    width: i32,
    height: i32,
) {
    let y_offset = match (class_id, face_index) {
        (3, 2) => 0.0,
        (3, 3) => 0.5,
        (4, 2) => 0.5,
        (4, 3) => 1.0,
        (5, 2) => 0.0,
        (5, 3) => 0.0625,
        (6, 2) => 0.0,
        (6, 3) => 0.5,
        (7, 2) => 0.5,
        (7, 3) => 1.0,
        _ => return,
    };
    let min = [base_x + start_u, base_y + slice, base_z + start_v];
    let max = [
        base_x + start_u + width,
        base_y + slice,
        base_z + start_v + height,
    ];
    emit_axis_aligned_face_bounds(
        vertices,
        indices,
        color_indices,
        face_stats,
        face_index,
        chunk_key,
        chunk_size,
        palette_id,
        [min[0] as f32, min[1] as f32 + y_offset, min[2] as f32],
        [max[0] as f32, max[1] as f32 + y_offset, max[2] as f32],
        true,
    );
    face_stats.fast_path_output_quads += 1;
}

fn emit_cuboid(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    face_stats: &mut MeshFaceStats,
    spec: CuboidSpec,
    gx: i32,
    gy: i32,
    gz: i32,
    occupied: &OccupancyGrid,
    _render_info: &[BlockRenderInfo],
) {
    for (face_index, offset_vec) in FACE_NEIGHBORS.iter().enumerate() {
        let neighbor = (gx + offset_vec[0], gy + offset_vec[1], gz + offset_vec[2]);
        if spec.neighbor_culling && occupied.contains_world(neighbor.0, neighbor.1, neighbor.2) {
            face_stats.culled_faces += 1;
            face_stats.culled_by_neighbor += 1;
            continue;
        }
        emit_axis_aligned_face(
            vertices,
            indices,
            color_indices,
            face_stats,
            face_index,
            spec.key,
            spec.chunk_size,
            spec.palette_id,
            gx,
            gy,
            gz,
            spec.min,
            spec.max,
            spec.non_full,
        );
    }
}

fn emit_axis_aligned_face(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    face_stats: &mut MeshFaceStats,
    face_index: usize,
    chunk_key: ChunkKey,
    chunk_size: u32,
    palette_id: usize,
    gx: i32,
    gy: i32,
    gz: i32,
    min: [f32; 3],
    max: [f32; 3],
    non_full: bool,
) {
    emit_axis_aligned_face_bounds(
        vertices,
        indices,
        color_indices,
        face_stats,
        face_index,
        chunk_key,
        chunk_size,
        palette_id,
        [gx as f32 + min[0], gy as f32 + min[1], gz as f32 + min[2]],
        [gx as f32 + max[0], gy as f32 + max[1], gz as f32 + max[2]],
        non_full,
    );
}

fn emit_axis_aligned_face_bounds(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    face_stats: &mut MeshFaceStats,
    face_index: usize,
    chunk_key: ChunkKey,
    chunk_size: u32,
    palette_id: usize,
    min: [f32; 3],
    max: [f32; 3],
    non_full: bool,
) {
    let mut quad = [[0.0_f32; 3]; 4];
    for (vertex_slot, template) in FACE_VERTICES[face_index].iter().enumerate() {
        quad[vertex_slot] = [
            if template[0] == 0.0 { min[0] } else { max[0] },
            if template[1] == 0.0 { min[1] } else { max[1] },
            if template[2] == 0.0 { min[2] } else { max[2] },
        ];
    }
    emit_quad(vertices, indices, color_indices, palette_id, &quad);
    face_stats.generated_faces += 1;
    match face_index {
        0 => face_stats.generated_neg_x += 1,
        1 => face_stats.generated_pos_x += 1,
        2 => face_stats.generated_neg_y += 1,
        3 => face_stats.generated_pos_y += 1,
        4 => face_stats.generated_neg_z += 1,
        5 => face_stats.generated_pos_z += 1,
        _ => {}
    }
    if non_full {
        face_stats.non_full_block_faces_generated += 1;
        face_stats.non_full_block_occlusion_rule += 1;
    }
    if is_chunk_boundary_face(
        chunk_key,
        chunk_size,
        min[0].floor() as i32,
        min[1].floor() as i32,
        min[2].floor() as i32,
        face_index,
    ) {
        face_stats.chunk_boundary_faces += 1;
    }
}

fn is_chunk_boundary_face(
    chunk_key: ChunkKey,
    chunk_size: u32,
    gx: i32,
    gy: i32,
    gz: i32,
    face_index: usize,
) -> bool {
    let neighbor_chunk = ChunkKey::new(
        (gx + FACE_NEIGHBORS[face_index][0]).div_euclid(chunk_size as i32),
        (gy + FACE_NEIGHBORS[face_index][1]).div_euclid(chunk_size as i32),
        (gz + FACE_NEIGHBORS[face_index][2]).div_euclid(chunk_size as i32),
    );
    neighbor_chunk != chunk_key
}

fn emit_crossed_planes(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    face_stats: &mut MeshFaceStats,
    block: TargetBlock,
    min_y: f32,
    max_y: f32,
    half_width: f32,
) {
    let cx = block.gx as f32 + 0.5;
    let cz = block.gz as f32 + 0.5;
    let y0 = block.gy as f32 + min_y;
    let y1 = block.gy as f32 + max_y;
    let quads = [
        [
            [cx - half_width, y0, cz - half_width],
            [cx + half_width, y0, cz + half_width],
            [cx + half_width, y1, cz + half_width],
            [cx - half_width, y1, cz - half_width],
        ],
        [
            [cx + half_width, y0, cz - half_width],
            [cx - half_width, y0, cz + half_width],
            [cx - half_width, y1, cz + half_width],
            [cx + half_width, y1, cz - half_width],
        ],
    ];
    for quad in quads {
        emit_double_sided_quad(vertices, indices, color_indices, block.palette_id, &quad);
        face_stats.generated_faces += 2;
        face_stats.non_full_block_faces_generated += 2;
        face_stats.non_full_block_occlusion_rule += 2;
    }
}

fn emit_quad(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    palette_id: usize,
    quad: &[[f32; 3]; 4],
) {
    let base = vertices.len() as u32;
    vertices.extend_from_slice(quad);
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    color_indices.push(palette_id as u32);
}

fn emit_double_sided_quad(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    color_indices: &mut Vec<u32>,
    palette_id: usize,
    quad: &[[f32; 3]; 4],
) {
    emit_quad(vertices, indices, color_indices, palette_id, quad);
    let reversed = [quad[0], quad[3], quad[2], quad[1]];
    emit_quad(vertices, indices, color_indices, palette_id, &reversed);
}

pub fn build_mesh_index_output(path: &Path, chunk_size: u32) -> Result<MeshIndexOutput> {
    let scene_index = ChunkSceneIndex::load(path, chunk_size)?;
    Ok(MeshIndexOutput {
        metadata: scene_index.metadata().clone(),
        chunk_size,
        palette: scene_index.palette().to_vec(),
        property_pool: scene_index.property_pool().to_vec(),
        chunks: sorted_chunk_coords(scene_index.chunk_entries()),
    })
}

pub fn build_mesh_batch_output(
    path: &Path,
    chunk_size: u32,
    offset: usize,
    limit: usize,
) -> Result<MeshBatchOutput> {
    let scene_index = ChunkSceneIndex::load(path, chunk_size)?;
    let total_chunks = scene_index.chunk_count();
    let selected = select_chunk_batch(scene_index.chunk_entries(), offset, limit);
    let selected_keys = selected.iter().map(|entry| entry.key).collect::<Vec<_>>();
    let chunks = scene_index.build_chunk_meshes(&selected_keys)?;

    Ok(MeshBatchOutput {
        metadata: scene_index.metadata().clone(),
        chunk_size,
        palette: scene_index.palette().to_vec(),
        property_pool: scene_index.property_pool().to_vec(),
        offset,
        limit,
        total_chunks,
        chunks,
    })
}

pub fn build_mesh_output(path: &Path, chunk_size: u32) -> Result<MeshOutput> {
    let scene_index = ChunkSceneIndex::load(path, chunk_size)?;
    let all_keys = scene_index
        .chunk_entries()
        .iter()
        .map(|entry| entry.key)
        .collect::<Vec<_>>();
    let chunks = scene_index.build_chunk_meshes(&all_keys)?;
    Ok(MeshOutput {
        metadata: scene_index.metadata().clone(),
        chunk_size,
        palette: scene_index.palette().to_vec(),
        property_pool: scene_index.property_pool().to_vec(),
        chunks,
    })
}



pub fn build_source_all_layers_output(path: &Path, chunk_size: u32) -> Result<VisualOutput> {
    let (root, catalog) = load_visual_catalog(path)?;
    build_layer_index_output_from_catalog(&root, &catalog, chunk_size)
}


fn build_layer_index_output_from_catalog(
    root: &crate::nbt::LitematicRoot,
    catalog: &VisualCatalog,
    chunk_size: u32,
) -> Result<VisualOutput> {
    let Some(bounds) = catalog.bounds else {
        return Ok(VisualOutput {
            metadata: catalog.metadata.clone(),
            visual: VisualSummaryOutput {
                chunk_size,
                size_x: catalog.metadata.enclosing_size.x,
                size_y: catalog.metadata.enclosing_size.y,
                size_z: catalog.metadata.enclosing_size.z,
                palette: catalog.palette.clone(),
                property_pool: catalog.property_pool.clone(),
                layers: Vec::new(),
                chunks: Vec::new(),
            },
        });
    };

    let size_y = catalog.metadata.enclosing_size.y.max(0) as usize;
    let mut layers = (0..size_y)
        .map(|y| LayerSliceOutput {
            y: y as i32,
            blocks: Vec::new(),
        })
        .collect::<Vec<_>>();

    for region in root.regions.values() {
        let volume = region_volume(&region.size)?;
        let width = i64::from(region.size.x).unsigned_abs() as usize;
        let length = i64::from(region.size.z).unsigned_abs() as usize;
        let nbits = bits_for_palette(region.block_state_palette.len());
        let mut region_palette_ids =
            Vec::<Option<usize>>::with_capacity(region.block_state_palette.len());
        for block in &region.block_state_palette {
            let state = format_block_state(block);
            if is_air_state(&state) {
                region_palette_ids.push(None);
            } else {
                let palette_id = catalog
                    .state_lookup
                    .get(&state)
                    .copied()
                    .ok_or_else(|| anyhow!("missing palette entry for state {}", state))?;
                region_palette_ids.push(Some(palette_id));
            }
        }

        for_each_palette_index(
            &region.block_states,
            volume,
            nbits,
            region.block_state_palette.len(),
            |index, palette_index| {
                let Some(Some(palette_id)) = region_palette_ids.get(palette_index) else {
                    return Ok(());
                };
                let y = index / (width * length);
                let i_in_layer = index % (width * length);
                let z = i_in_layer / width;
                let x = i_in_layer % width;
                let (rx, ry, rz) = storage_to_region_coords(x, y, z, &region.size);
                let global_y = region.position.y + ry - bounds.min_y;
                if global_y < 0 || global_y as usize >= layers.len() {
                    return Ok(());
                }
                layers[global_y as usize].blocks.push(LayerBlockOutput {
                    x: region.position.x + rx - bounds.min_x,
                    z: region.position.z + rz - bounds.min_z,
                    palette_id: *palette_id,
                });
                Ok(())
            },
        )?;
    }

    Ok(VisualOutput {
        metadata: catalog.metadata.clone(),
        visual: VisualSummaryOutput {
            chunk_size,
            size_x: catalog.metadata.enclosing_size.x,
            size_y: catalog.metadata.enclosing_size.y,
            size_z: catalog.metadata.enclosing_size.z,
            palette: catalog.palette.clone(),
            property_pool: catalog.property_pool.clone(),
            layers,
            chunks: Vec::new(),
        },
    })
}
