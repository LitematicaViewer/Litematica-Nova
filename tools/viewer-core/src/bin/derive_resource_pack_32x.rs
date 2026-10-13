use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use litematica_core::resource_pack_32x::derive_resource_pack_32x;

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let Some(input_flag) = arguments.next() else {
        bail!("usage: derive_resource_pack_32x --input <directory> --output <directory>");
    };
    let Some(input) = arguments.next() else {
        bail!("--input requires a directory path");
    };
    let Some(output_flag) = arguments.next() else {
        bail!("usage: derive_resource_pack_32x --input <directory> --output <directory>");
    };
    let Some(output) = arguments.next() else {
        bail!("--output requires a directory path");
    };
    if input_flag != "--input" || output_flag != "--output" || arguments.next().is_some() {
        bail!("usage: derive_resource_pack_32x --input <directory> --output <directory>");
    }

    let report = derive_resource_pack_32x(&PathBuf::from(input), &PathBuf::from(output))?;
    println!(
        "{}",
        serde_json::to_string(&report).context("cannot serialize derivation report")?
    );
    Ok(())
}
