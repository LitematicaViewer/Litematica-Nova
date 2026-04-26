use std::fs::File;
use std::io::{self, BufWriter, Write};

use anyhow::Result;
use litematica_core::{
    analyze, cache_layer, cli, generate_projection, mesh, replace_blocks, stats_api, visual,
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
    match args.command.as_str() {
        "stats" => {
            let output = stats_api::build_stats_output(&args.input)?;
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
            let output = stats_api::build_materials_output(&args.input, scope)?;
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
        other => anyhow::bail!("unknown command: {other}"),
    }
    Ok(())
}
