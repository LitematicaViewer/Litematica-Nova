use std::path::PathBuf;

use anyhow::{Result, bail};
use litematica_core::stockpile_serve;

fn main() -> Result<()> {
    let args = ServerArgs::parse()?;
    stockpile_serve::serve_stockpile_root(&args.root, &args.bind)?;
    Ok(())
}

struct ServerArgs {
    root: PathBuf,
    bind: String,
}

impl ServerArgs {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut root = None;
        let mut bind = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--root" => {
                    let Some(value) = args.next() else {
                        bail!("missing value after --root");
                    };
                    root = Some(PathBuf::from(value));
                }
                "--bind" => {
                    let Some(value) = args.next() else {
                        bail!("missing value after --bind");
                    };
                    bind = Some(value);
                }
                _ if arg.starts_with("--root=") => {
                    root = Some(PathBuf::from(arg.trim_start_matches("--root=")));
                }
                _ if arg.starts_with("--bind=") => {
                    bind = Some(arg.trim_start_matches("--bind=").to_string());
                }
                "-h" | "--help" => {
                    bail!(
                        "usage: stockpile_server --root <extracted-stockpile-dir> --bind <addr:port>"
                    );
                }
                _ => bail!("unknown argument: {arg}"),
            }
        }
        Ok(Self {
            root: root.ok_or_else(|| anyhow::anyhow!("stockpile_server requires --root <dir>"))?,
            bind: bind
                .ok_or_else(|| anyhow::anyhow!("stockpile_server requires --bind <addr:port>"))?,
        })
    }
}
