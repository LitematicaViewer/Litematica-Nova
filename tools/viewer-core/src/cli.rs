use std::env;
use std::path::PathBuf;

use anyhow::{Result, bail};

pub struct CliArgs {
    pub command: String,
    pub input: PathBuf,
    pub include_entities: bool,
    pub chunk_size: u32,
    pub output: Option<PathBuf>,
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
        bail!(
            "usage: litematica_core <command> <file> [--include-entities] [--chunk-size=N] [--output <path>]"
        );
    };
    let Some(input) = args.next() else {
        bail!("missing input file path");
    };

    let mut include_entities = false;
    let mut chunk_size = 32_u32;
    let mut output = None;
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
            "--include-entities" => include_entities = true,
            _ if arg.starts_with("--chunk-size=") => {
                let value = arg.trim_start_matches("--chunk-size=");
                chunk_size = value.parse()?;
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
            _ => bail!("unknown argument: {arg}"),
        }
    }

    Ok(CliArgs {
        command,
        input: PathBuf::from(input),
        include_entities,
        chunk_size,
        output,
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
