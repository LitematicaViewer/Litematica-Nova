#![recursion_limit = "512"]

#[cfg(not(feature = "public-beta"))]
compile_error!("this isolated public core requires the public-beta feature");

pub mod analyze;
pub mod backend_provenance;
pub mod block_entity;
pub mod block_database;
pub mod build_mode;
pub mod business;
pub mod cache_layer;
pub mod cli;
pub mod frame_capture;
pub mod generate_projection;
pub mod game_data;

#[cfg(test)]
#[test]
#[ignore = "release fixture: NOVA_TEST_SLIM_DEFAULT, NOVA_TEST_LOOSE_DEFAULT and fresh LBA_DATA_ROOT"]
fn jar_resource_tests_candidate_default_all_resources_and_consumers() -> anyhow::Result<()> {
    game_data::jar_resource_tests::check_candidate(|context| {
        assert_eq!(crate::item_names::selected_name("block.minecraft.stone", "en_us")?.as_deref(), Some("Stone"));
        assert!(crate::recipe_cache::load_available_recipes(&context.game_version)?.unwrap().contains_key("minecraft:crafting_table"));
        assert!(crate::recipe_cache::load_item_tags()?["minecraft:planks"].iter().any(|i| i == "minecraft:oak_planks"));
        assert!(crate::block_database::selected_palette_color("minecraft:white_stained_glass", &std::collections::BTreeMap::new())?.is_some());
        let texture = crate::block_database::selected_texture(context, "minecraft:block/white_stained_glass")?;
        assert!(texture.pixels().any(|p| p[3] > 0 && p[3] < 255));
        Ok(())
    })
}
pub mod layer_resources;
pub mod item_icons;
pub mod item_names;
pub mod mesh;
pub mod metadata_edit;
pub mod model;
pub mod native_viewer;
pub(crate) mod native_viewport;
#[cfg(target_os = "macos")]
pub(crate) mod macos_presentation;
#[cfg(target_os = "macos")]
pub(crate) mod mac_shared_viewport;
#[cfg(target_os = "macos")]
pub(crate) mod mac_fast_composition;
pub mod quick_resource_preflight;
#[cfg(target_os = "windows")]
mod windows_composition;
pub mod nbt;
pub mod projection_marker;
pub mod projection_preview;
pub mod recipe_cache;
pub mod recipe_tree;
pub mod replace_blocks;
pub mod resource_pack_32x;
pub mod runtime_paths;
pub mod stats_api;
pub mod material_export;
pub mod material_detail;
pub mod material_catalog;
pub mod stockpile;
pub mod stockpile_schema;
pub mod stockpile_serve;
pub mod stockpile_zip;
pub mod storage;
pub mod visual;
pub mod layer_view;
pub mod version_conversion;
mod version_entity_registry;
mod version_entity_stable;
mod version_brain_memory;
mod version_legacy_encoding;
mod version_payload_migration;

pub mod canonical_ir;
mod process_io;
mod viewer_session;

pub mod world_export;

pub mod data_worker_protocol;
pub mod data_worker;
pub mod composition_scene;
pub mod composition_transform;
pub mod composition;
pub mod composition_editor;
pub mod schematic_formats;
pub mod workspace;
pub mod data_view;
mod data_scope;
mod statistics_rules;
