#![recursion_limit = "512"]

#[cfg(not(feature = "public-beta"))]
compile_error!("this isolated public core requires the public-beta feature");

pub mod analyze;
pub mod backend_provenance;
pub mod block_entity;
pub mod build_mode;
pub mod business;
pub mod cache_layer;
pub mod cli;
pub mod frame_capture;
pub mod generate_projection;
pub mod item_icons;
pub mod item_names;
pub mod mesh;
pub mod metadata_edit;
pub mod model;
pub mod native_viewer;
pub mod nbt;
pub mod projection_marker;
pub mod projection_preview;
pub mod recipe_cache;
pub mod recipe_tree;
pub mod replace_blocks;
pub mod resource_pack_32x;
pub mod runtime_paths;
pub mod stats_api;
pub mod stockpile;
pub mod stockpile_schema;
pub mod stockpile_serve;
pub mod stockpile_zip;
pub mod storage;
pub mod visual;

pub mod canonical_ir;
mod process_io;
mod viewer_session;
