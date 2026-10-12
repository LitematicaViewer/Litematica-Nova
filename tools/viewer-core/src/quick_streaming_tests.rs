//! CLI-only coverage of the actual legacy quick mesh/upload preparation path.
use super::*;

#[test]
fn quick_upload_expands_compact_faces_without_changing_source_or_raw_indices() {
    let mut mesh = PreparedChunkMesh::placeholder();
    mesh.translucent_indices = vec![0, 1, 2];
    for face in 0..6 {
        mesh.compact_surfaces.push(CompactSurfaceRecord {
            face_index: face,
            min: [-8.0, 0.0, -3.0],
            max: [16.0, 0.5, 9.0],
            color: [0.2, 0.4, 0.6],
        });
    }
    let gpu = mesh.gpu_upload_mesh();
    assert_eq!(gpu.vertices.len(), mesh.vertex_count());
    assert_eq!(gpu.indices.len() + gpu.translucent_indices.len(), mesh.index_count());
    assert!(gpu.compact_surfaces.is_empty());
    assert_eq!(&gpu.indices[..mesh.indices.len()], &mesh.indices);
    assert_eq!(gpu.translucent_indices, mesh.translucent_indices);
    assert_eq!(mesh.compact_surfaces.len(), 6, "CPU cache stays compact");
    assert!(gpu.indices.iter().all(|&index| (index as usize) < gpu.vertices.len()));
}

#[test]
fn quick_upload_reuses_already_expanded_cache_mesh() {
    let mesh = PreparedChunkMesh::placeholder();
    assert!(matches!(mesh.gpu_upload_mesh(), Cow::Borrowed(_)));
}

#[test]
fn quick_budget_admits_one_oversized_chunk_but_keeps_disabled_and_per_frame_limits() {
    let mut usage = BudgetUsage::default();
    assert!(usage.can_fit(200_000, 300_000, 2, 72_000, 108_000));
    usage.consume(200_000, 300_000);
    assert!(!usage.can_fit(24, 36, 2, 72_000, 108_000));
    assert!(!BudgetUsage::default().can_fit(24, 36, 0, 0, 0));
    assert!(!BudgetUsage::default().can_fit(24, 36, 2, 0, 0));
    let mut normal = BudgetUsage::default();
    assert!(normal.can_fit(24, 36, 2, 72_000, 108_000));
    normal.consume(24, 36);
    assert!(normal.can_fit(24, 36, 2, 72_000, 108_000));
    normal.consume(24, 36);
    assert!(!normal.can_fit(24, 36, 2, 72_000, 108_000));
}

fn drain_costs(mut costs: Vec<(usize, usize)>, budget: BuildBudget) -> (usize, usize) {
    let mut frames = 0;
    while !costs.is_empty() {
        let mut usage = BudgetUsage::default();
        let before = costs.len();
        costs.retain(|&(vertices, indices)| {
            if usage.can_fit(vertices, indices, budget.max_chunks_per_frame,
                budget.max_vertices_per_frame, budget.max_indices_per_frame) {
                usage.consume(vertices, indices);
                false
            } else {
                true
            }
        });
        frames += 1;
        if before == costs.len() { break; }
    }
    (frames, costs.len())
}

#[test]
#[ignore = "requires LBA_QUICK_CLI_FIXTURE pointing to a real user projection; no window or GPU is created"]
fn quick_real_projection_complete_mesh_and_upload_payload() {
    install_viewer_build_mode(ViewerBuildMode::FastExperimental).unwrap();
    let path = std::env::var_os("LBA_QUICK_CLI_FIXTURE").expect("LBA_QUICK_CLI_FIXTURE");
    let index = ChunkSceneIndex::load(Path::new(&path), 32).unwrap();
    let config = ChunkStreamingConfig::default();
    let camera = OrbitCamera::from_bounds(SceneBounds::from_metadata(index.metadata()), 1280, 720);
    let plan = ChunkResidencyPlan::build(&index, &camera, &config,
        config.background_fill_start_frame, true, false);
    let scheduled = plan.upload_order.iter().chain(&plan.preload_order)
        .copied().collect::<HashSet<_>>();
    let blocks: u64 = index.chunk_entries().iter().map(|entry| u64::from(entry.non_air_blocks)).sum();
    let target = BuildBudget::default().saturating_sub(config.preload_build_soft_budget);
    let target_costs = plan.upload_order.iter().map(|&key| ViewerState::estimate_chunk_build_cost(&index, key)).collect();
    let preload_costs = plan.preload_order.iter().map(|&key| ViewerState::estimate_chunk_build_cost(&index, key)).collect();
    let (target_frames, target_blocked) = drain_costs(target_costs, target);
    let (preload_frames, preload_blocked) = drain_costs(preload_costs, config.preload_build_soft_budget);
    println!("QUICK_CLI_SCHEDULE input={} mode=fast_experimental blocks={} chunks={} scheduled={} target={} preload={} target_frames={} preload_frames={} target_blocked={} preload_blocked={}",
        Path::new(&path).display(), blocks, index.chunk_count(), scheduled.len(), plan.upload_order.len(),
        plan.preload_order.len(), target_frames, preload_frames, target_blocked, preload_blocked);

    let palette = resolve_scene_palette_colors(&index, ViewerDisplayMode::FastExperimental);
    let keys = index.chunk_entries().iter().map(|entry| entry.key).collect::<Vec<_>>();
    let mut built = HashSet::new();
    let mut expected_vertices = 0;
    let mut expected_indices = 0;
    let mut upload_vertices = 0;
    let mut upload_indices = 0;
    let mut compact_faces = 0;
    let mut empty_chunks = 0;
    let mut uploads = Vec::new();
    for batch in keys.chunks(128) {
        let meshes = index.build_chunk_meshes(batch).unwrap();
        assert_eq!(meshes.len(), batch.len());
        for mesh in meshes {
            let key = ChunkKey::new(mesh.cx, mesh.cy, mesh.cz);
            assert!(built.insert(key), "no duplicate chunk submission");
            let Some(prepared) = PreparedChunkMesh::from_mesh_chunk(mesh, &palette, 32) else {
                empty_chunks += 1;
                continue;
            };
            expected_vertices += prepared.vertex_count();
            expected_indices += prepared.index_count();
            compact_faces += prepared.compact_surfaces.len();
            let gpu = prepared.gpu_upload_mesh();
            upload_vertices += gpu.vertices.len();
            upload_indices += gpu.indices.len() + gpu.translucent_indices.len();
            assert!(gpu.indices.iter().chain(&gpu.translucent_indices)
                .all(|&index| (index as usize) < gpu.vertices.len()), "GPU index range");
            uploads.push((prepared.vertex_count(), prepared.index_count()));
        }
    }
    let upload_budget = config.preload_upload_soft_budget;
    let (upload_frames, upload_blocked) = drain_costs(uploads, BuildBudget {
        max_chunks_per_frame: upload_budget.max_chunks_per_frame,
        max_vertices_per_frame: upload_budget.max_vertices_per_frame,
        max_indices_per_frame: upload_budget.max_indices_per_frame,
    });
    println!("QUICK_CLI_GEOMETRY indexed={} built={} empty={} compact_faces={} expected_vertices={} upload_vertices={} expected_indices={} upload_indices={} upload_frames={} upload_blocked={} gpu_execution=false",
        index.chunk_count(), built.len(), empty_chunks, compact_faces, expected_vertices, upload_vertices,
        expected_indices, upload_indices, upload_frames, upload_blocked);
    assert_eq!(scheduled.len(), index.chunk_count(), "fitted view schedules every indexed chunk");
    assert_eq!(built.len(), index.chunk_count(), "every indexed chunk is meshed");
    assert_eq!(upload_vertices, expected_vertices, "every raw and compact vertex reaches upload payload");
    assert_eq!(upload_indices, expected_indices, "every raw and compact index reaches upload payload");
    assert_eq!(target_blocked + preload_blocked + upload_blocked, 0, "soft budgets must not starve indivisible chunks");
}
