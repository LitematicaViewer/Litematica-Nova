use std::env;
use std::path::PathBuf;

use anyhow::{Result, bail};

pub struct CliArgs {
    pub command: String,
    pub input: PathBuf,
    pub include_entities: bool,
    pub include_container_items: bool,
    pub json: bool,
    pub chunk_size: u32,
    pub output: Option<PathBuf>,
    pub plan: Option<PathBuf>,
    pub patch: Option<PathBuf>,
    pub rules: Option<PathBuf>,
    pub dry_run: bool,
    pub force: bool,
    pub scope: Option<String>,
    pub region: Option<String>,
    pub layer: Option<i32>,
    pub y: Option<i32>,
    pub y_start: Option<i32>,
    pub y_end: Option<i32>,
    pub cx: Option<i32>,
    pub cy: Option<i32>,
    pub cz: Option<i32>,
    pub offset: usize,
    pub limit: usize,
}

pub fn parse_args() -> Result<CliArgs> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        bail!("usage: litematica_core <command> <file|--input <file>> [options]");
    };
    if command == "replace-blocks" {
        return parse_replace_blocks_args(command, args.collect());
    }
    if command == "generate" {
        return parse_generate_args(command, args.collect());
    }
    if command == "edit-metadata" {
        return parse_edit_metadata_args(command, args.collect());
    }

    let mut input = None;
    let mut include_entities = false;
    let mut include_container_items = false;
    let mut json = false;
    let mut chunk_size = 32_u32;
    let mut output = None;
    let mut scope = None;
    let mut region = None;
    let mut layer = None;
    let mut y = None;
    let mut y_start = None;
    let mut y_end = None;
    let mut cx = None;
    let mut cy = None;
    let mut cz = None;
    let mut offset = 0_usize;
    let mut limit = 64_usize;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --input");
                };
                input = Some(PathBuf::from(path));
            }
            "--include-entities" => include_entities = true,
            "--include-container-items" => include_container_items = true,
            "--json" => json = true,
            "--scope" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --scope");
                };
                scope = Some(value);
            }
            "--region" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --region");
                };
                region = Some(value);
            }
            "--layer" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --layer");
                };
                layer = Some(value.parse()?);
            }
            _ if arg.starts_with("--chunk-size=") => {
                let value = arg.trim_start_matches("--chunk-size=");
                chunk_size = value.parse()?;
            }
            _ if arg.starts_with("--input=") => {
                let value = arg.trim_start_matches("--input=");
                if value.is_empty() {
                    bail!("missing path after --input=");
                }
                input = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--scope=") => {
                let value = arg.trim_start_matches("--scope=");
                if value.is_empty() {
                    bail!("missing value after --scope=");
                }
                scope = Some(value.to_string());
            }
            _ if arg.starts_with("--region=") => {
                let value = arg.trim_start_matches("--region=");
                if value.is_empty() {
                    bail!("missing value after --region=");
                }
                region = Some(value.to_string());
            }
            _ if arg.starts_with("--layer=") => {
                layer = Some(arg.trim_start_matches("--layer=").parse()?);
            }
            _ if arg.starts_with("--y=") => {
                y = Some(arg.trim_start_matches("--y=").parse()?);
            }
            _ if arg.starts_with("--y-start=") => {
                y_start = Some(arg.trim_start_matches("--y-start=").parse()?);
            }
            _ if arg.starts_with("--y-end=") => {
                y_end = Some(arg.trim_start_matches("--y-end=").parse()?);
            }
            _ if arg.starts_with("--cx=") => {
                cx = Some(arg.trim_start_matches("--cx=").parse()?);
            }
            _ if arg.starts_with("--cy=") => {
                cy = Some(arg.trim_start_matches("--cy=").parse()?);
            }
            _ if arg.starts_with("--cz=") => {
                cz = Some(arg.trim_start_matches("--cz=").parse()?);
            }
            _ if arg.starts_with("--offset=") => {
                offset = arg.trim_start_matches("--offset=").parse()?;
            }
            _ if arg.starts_with("--limit=") => {
                limit = arg.trim_start_matches("--limit=").parse()?;
            }
            "--output" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --output");
                };
                output = Some(PathBuf::from(path));
            }
            _ if arg.starts_with("--output=") => {
                let value = arg.trim_start_matches("--output=");
                if value.is_empty() {
                    bail!("missing path after --output=");
                }
                output = Some(PathBuf::from(value));
            }
            _ if arg.starts_with('-') => bail!("unknown argument: {arg}"),
            _ => {
                if input.is_some() {
                    bail!("unexpected positional argument: {arg}");
                }
                input = Some(PathBuf::from(arg));
            }
        }
    }

    let input = input.ok_or_else(|| anyhow::anyhow!("missing input file path"))?;

    Ok(CliArgs {
        command,
        input,
        include_entities,
        include_container_items,
        json,
        chunk_size,
        output,
        plan: None,
        patch: None,
        rules: None,
        dry_run: false,
        force: false,
        scope,
        region,
        layer,
        y,
        y_start,
        y_end,
        cx,
        cy,
        cz,
        offset,
        limit,
    })
}

fn parse_replace_blocks_args(command: String, raw_args: Vec<String>) -> Result<CliArgs> {
    let mut input = None;
    let mut output = None;
    let mut rules = None;
    let mut dry_run = false;
    let mut args = raw_args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --input");
                };
                input = Some(PathBuf::from(path));
            }
            "--output" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --output");
                };
                output = Some(PathBuf::from(path));
            }
            "--rules" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --rules");
                };
                rules = Some(PathBuf::from(path));
            }
            "--dry-run" => dry_run = true,
            "--summary" => {}
            _ if arg.starts_with("--input=") => {
                let value = arg.trim_start_matches("--input=");
                if value.is_empty() {
                    bail!("missing path after --input=");
                }
                input = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--output=") => {
                let value = arg.trim_start_matches("--output=");
                if value.is_empty() {
                    bail!("missing path after --output=");
                }
                output = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--rules=") => {
                let value = arg.trim_start_matches("--rules=");
                if value.is_empty() {
                    bail!("missing path after --rules=");
                }
                rules = Some(PathBuf::from(value));
            }
            _ if arg.starts_with('-') => bail!("unknown argument: {arg}"),
            _ => {
                if input.is_some() {
                    bail!("unexpected positional argument: {arg}");
                }
                input = Some(PathBuf::from(arg));
            }
        }
    }

    let input = input.ok_or_else(|| anyhow::anyhow!("replace-blocks requires --input <path>"))?;
    if rules.is_none() {
        bail!("replace-blocks requires --rules <path>");
    }
    if !dry_run && output.is_none() {
        bail!("replace-blocks requires --output <path> unless --dry-run is set");
    }

    Ok(CliArgs {
        command,
        input,
        include_entities: false,
        include_container_items: false,
        json: false,
        chunk_size: 32,
        output,
        plan: None,
        patch: None,
        rules,
        dry_run,
        force: false,
        scope: None,
        region: None,
        layer: None,
        y: None,
        y_start: None,
        y_end: None,
        cx: None,
        cy: None,
        cz: None,
        offset: 0,
        limit: 64,
    })
}

fn parse_generate_args(command: String, raw_args: Vec<String>) -> Result<CliArgs> {
    let mut plan = None;
    let mut output = None;
    let mut dry_run = false;
    let mut force = false;
    let mut json = false;
    let mut args = raw_args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--plan" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --plan");
                };
                plan = Some(PathBuf::from(path));
            }
            "--output" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --output");
                };
                output = Some(PathBuf::from(path));
            }
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            "--force=true" => force = true,
            "--force=false" => force = false,
            "--json" => json = true,
            _ if arg.starts_with("--plan=") => {
                let value = arg.trim_start_matches("--plan=");
                if value.is_empty() {
                    bail!("missing path after --plan=");
                }
                plan = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--output=") => {
                let value = arg.trim_start_matches("--output=");
                if value.is_empty() {
                    bail!("missing path after --output=");
                }
                output = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--force=") => {
                force = arg.trim_start_matches("--force=").parse()?;
            }
            _ if arg.starts_with('-') => bail!("unknown argument: {arg}"),
            _ => bail!("unexpected positional argument: {arg}"),
        }
    }

    if plan.is_none() {
        bail!("generate requires --plan <path>");
    }
    if !dry_run && output.is_none() {
        bail!("generate requires --output <path> unless --dry-run is set");
    }

    Ok(CliArgs {
        command,
        input: PathBuf::new(),
        include_entities: false,
        include_container_items: false,
        json,
        chunk_size: 32,
        output,
        plan,
        patch: None,
        rules: None,
        dry_run,
        force,
        scope: None,
        region: None,
        layer: None,
        y: None,
        y_start: None,
        y_end: None,
        cx: None,
        cy: None,
        cz: None,
        offset: 0,
        limit: 64,
    })
}

fn parse_edit_metadata_args(command: String, raw_args: Vec<String>) -> Result<CliArgs> {
    let mut input = None;
    let mut output = None;
    let mut patch = None;
    let mut args = raw_args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --input");
                };
                input = Some(PathBuf::from(path));
            }
            "--output" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --output");
                };
                output = Some(PathBuf::from(path));
            }
            "--patch" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --patch");
                };
                patch = Some(PathBuf::from(path));
            }
            _ if arg.starts_with("--input=") => {
                let value = arg.trim_start_matches("--input=");
                if value.is_empty() {
                    bail!("missing path after --input=");
                }
                input = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--output=") => {
                let value = arg.trim_start_matches("--output=");
                if value.is_empty() {
                    bail!("missing path after --output=");
                }
                output = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--patch=") => {
                let value = arg.trim_start_matches("--patch=");
                if value.is_empty() {
                    bail!("missing path after --patch=");
                }
                patch = Some(PathBuf::from(value));
            }
            _ if arg.starts_with('-') => bail!("unknown argument: {arg}"),
            _ => {
                if input.is_some() {
                    bail!("unexpected positional argument: {arg}");
                }
                input = Some(PathBuf::from(arg));
            }
        }
    }

    Ok(CliArgs {
        command,
        input: input.ok_or_else(|| anyhow::anyhow!("edit-metadata requires --input <path>"))?,
        include_entities: false,
        include_container_items: false,
        json: true,
        chunk_size: 32,
        output,
        plan: None,
        patch: Some(patch.ok_or_else(|| anyhow::anyhow!("edit-metadata requires --patch <path>"))?),
        rules: None,
        dry_run: false,
        force: false,
        scope: None,
        region: None,
        layer: None,
        y: None,
        y_start: None,
        y_end: None,
        cx: None,
        cy: None,
        cz: None,
        offset: 0,
        limit: 64,
    })
}
