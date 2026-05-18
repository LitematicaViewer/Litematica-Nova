use std::fs::File;
use std::io::{self, BufWriter, Read, Write};

use anyhow::Result;
use litematica_core::{
    analyze, cache_layer, cli, generate_projection, mesh, metadata_edit, recipe_cache,
    replace_blocks, runtime_paths, stats_api, stockpile, stockpile_serve, stockpile_zip, visual,
};
use serde::Serialize;

fn emit_output<T: Serialize>(value: &T, output: Option<&std::path::Path>) -> Result<()> {
    match output {
        Some(path) => {
            let file = File::create(path)?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer(&mut writer, value)?;
            writer.flush()?;
        }
        None => {
            let stdout = io::stdout();
            let mut writer = BufWriter::new(stdout.lock());
            serde_json::to_writer(&mut writer, value)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = cli::parse_args()?;
    let runtime_paths = runtime_paths::ensure_runtime_layout()?;
    match args.command.as_str() {
        "runtime-paths" => {
            emit_output(&runtime_paths, args.output.as_deref())?;
        }
        "stockpile export-data" => {
            let output = stockpile::export_materials_data(
                &args.input,
                args.output.as_deref(),
                args.include_container_items,
                args.minecraft_version.as_deref(),
            )?;
            emit_output(&output, None)?;
        }
        "stockpile recipe-status" => {
            let minecraft_version = args
                .minecraft_version
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --minecraft-version"))?;
            let output = recipe_cache::recipe_status(minecraft_version)?;
            emit_output(&output, None)?;
        }
        "stockpile recipe-fetch" => {
            let minecraft_version = args
                .minecraft_version
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --minecraft-version"))?;
            let output = recipe_cache::fetch_recipe_cache(minecraft_version)?;
            emit_output(&output, None)?;
        }
        "stockpile export-zip" => {
            let mode = args.stockpile_mode.as_deref().unwrap_or("single").parse()?;
            let output = stockpile_zip::export_stockpile_zip(
                &args.input,
                args.output.as_deref(),
                args.include_container_items,
                args.minecraft_version.as_deref(),
                mode,
            )?;
            emit_output(&output, None)?;
        }
        "stockpile serve" => {
            let bind = args
                .bind
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --bind"))?;
            stockpile_serve::serve_stockpile_zip(&args.input, bind)?;
        }
        "stockpile session-info" => {
            let output = stockpile_serve::session_info(&args.input)?;
            emit_output(&output, None)?;
        }
        "stockpile session-reset" => {
            let output = stockpile_serve::session_reset(&args.input, args.yes)?;
            emit_output(&output, None)?;
        }
        "stockpile session-export" => {
            let output_path = args
                .output
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --output"))?;
            let output = stockpile_serve::session_export(&args.input, output_path)?;
            emit_output(&output, None)?;
        }
        "stockpile session-import" => {
            let input_path = args
                .session_input
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --input"))?;
            let output = stockpile_serve::session_import(&args.input, input_path, args.replace)?;
            emit_output(&output, None)?;
        }
        "stockpile config-show" => {
            let output = stockpile_serve::config_show(&args.input)?;
            emit_output(&output, None)?;
        }
        "stockpile config-set" => {
            let key = args
                .config_key
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --key"))?;
            let value = args
                .config_value
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --value"))?;
            let output = stockpile_serve::config_set(&args.input, key, value)?;
            emit_output(&output, None)?;
        }
        "stockpile config-reset" => {
            let output = stockpile_serve::config_reset(&args.input, args.yes)?;
            emit_output(&output, None)?;
        }
        "stockpile set-access-password" => {
            let password = read_stdin_password(args.password_stdin)?;
            let output = stockpile_serve::set_access_password(&args.input, &password)?;
            emit_output(&output, None)?;
        }
        "stockpile clear-access-password" => {
            let output = stockpile_serve::clear_access_password(&args.input)?;
            emit_output(&output, None)?;
        }
        "stockpile set-admin-password" => {
            let password = read_stdin_password(args.password_stdin)?;
            let output = stockpile_serve::set_admin_password(&args.input, &password)?;
            emit_output(&output, None)?;
        }
        "stockpile clear-admin-password" => {
            let output = stockpile_serve::clear_admin_password(&args.input)?;
            emit_output(&output, None)?;
        }
        "stockpile whitelist-add" => {
            let user = args
                .user
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --user"))?;
            let output = stockpile_serve::whitelist_add(&args.input, user)?;
            emit_output(&output, None)?;
        }
        "stockpile whitelist-remove" => {
            let user = args
                .user
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --user"))?;
            let output = stockpile_serve::whitelist_remove(&args.input, user)?;
            emit_output(&output, None)?;
        }
        "stockpile whitelist-list" => {
            let output = stockpile_serve::whitelist_list(&args.input)?;
            emit_output(&output, None)?;
        }
        "stats" => {
            let output = stats_api::build_stats_output(&args.input, args.include_container_items)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "materials" => {
            let scope = match (args.scope.as_deref(), args.region.as_ref(), args.layer) {
                (_, Some(region), None) => stats_api::MaterialScope::Region(region.clone()),
                (_, None, Some(layer)) => stats_api::MaterialScope::Layer(layer),
                (Some("all"), None, None) | (None, None, None) => stats_api::MaterialScope::All,
                (Some(other), None, None) => {
                    anyhow::bail!("unsupported --scope value for materials: {other}")
                }
                _ => anyhow::bail!("use exactly one of --scope all, --region <name>, --layer <y>"),
            };
            let output = stats_api::build_materials_output(
                &args.input,
                scope,
                args.include_container_items,
            )?;
            emit_output(&output, args.output.as_deref())?;
        }
        "analyze" => {
            let output = analyze::analyze_litematic(&args.input, args.include_entities)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "visual-meta" => {
            let output = visual::build_visual_meta_output(&args.input, args.chunk_size)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "visualize-layer" => {
            let y = args
                .y
                .ok_or_else(|| anyhow::anyhow!("missing --y for visualize-layer"))?;
            let output = visual::build_visual_layer_output(&args.input, args.chunk_size, y)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "cache-layer-meta" => {
            let output = cache_layer::build_cache_layer_meta_output(&args.input)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "cache-layer" => {
            let y = args
                .y
                .ok_or_else(|| anyhow::anyhow!("missing --y for cache-layer"))?;
            let output = cache_layer::build_cache_layer_output(&args.input, y)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "visualize" => {
            let output = visual::build_visual_output(&args.input, args.chunk_size)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "mesh-index" => {
            let output = mesh::build_mesh_index_output(&args.input, args.chunk_size)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "mesh-batch" => {
            let output = mesh::build_mesh_batch_output(
                &args.input,
                args.chunk_size,
                args.offset,
                args.limit,
            )?;
            emit_output(&output, args.output.as_deref())?;
        }
        "mesh" => {
            let output = mesh::build_mesh_output(&args.input, args.chunk_size)?;
            emit_output(&output, args.output.as_deref())?;
        }
        "replace-blocks" => {
            let rules = args
                .rules
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --rules for replace-blocks"))?;
            let summary = replace_blocks::replace_blocks(
                &args.input,
                args.output.as_deref(),
                rules,
                args.dry_run,
            )?;
            emit_output(&summary, None)?;
        }
        "generate" => {
            let plan = args
                .plan
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --plan for generate"))?;
            let summary = generate_projection::generate_projection(
                plan,
                args.output.as_deref(),
                args.dry_run,
                args.force,
            )?;
            emit_output(&summary, None)?;
        }
        "edit-metadata" => {
            let patch = args
                .patch
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --patch for edit-metadata"))?;
            let summary = metadata_edit::edit_metadata(&args.input, args.output.as_deref(), patch)?;
            emit_output(&summary, None)?;
        }
        other => anyhow::bail!("unknown command: {other}"),
    }
    Ok(())
}

fn read_stdin_password(enabled: bool) -> Result<String> {
    if !enabled {
        anyhow::bail!("password input requires --password-stdin");
    }
    let mut value = String::new();
    io::stdin().read_to_string(&mut value)?;
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    if value.is_empty() {
        anyhow::bail!("password cannot be empty");
    }
    Ok(value)
}
