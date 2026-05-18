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
    pub minecraft_version: Option<String>,
    pub bind: Option<String>,
    pub yes: bool,
    pub replace: bool,
    pub session_input: Option<PathBuf>,
    pub config_key: Option<String>,
    pub config_value: Option<String>,
    pub user: Option<String>,
    pub password_stdin: bool,
    pub stockpile_mode: Option<String>,
    pub access_password_stdin: bool,
    pub admin_password_stdin: bool,
    pub whitelist_file: Option<PathBuf>,
    pub allow_guest_readonly: Option<bool>,
    pub admin_page_enabled: Option<bool>,
    pub stockpile_targets: Vec<String>,
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
    if command == "runtime-paths" {
        return parse_runtime_paths_args(command, args.collect());
    }
    if command == "stockpile" {
        return parse_stockpile_args(args.collect());
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
        minecraft_version: None,
        bind: None,
        yes: false,
        replace: false,
        session_input: None,
        config_key: None,
        config_value: None,
        user: None,
        password_stdin: false,
        stockpile_mode: None,
        access_password_stdin: false,
        admin_password_stdin: false,
        whitelist_file: None,
        allow_guest_readonly: None,
        admin_page_enabled: None,
        stockpile_targets: Vec::new(),
    })
}

fn parse_runtime_paths_args(command: String, raw_args: Vec<String>) -> Result<CliArgs> {
    let mut output = None;
    let mut args = raw_args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
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
            _ => bail!("unexpected positional argument: {arg}"),
        }
    }

    Ok(CliArgs {
        command,
        input: PathBuf::new(),
        include_entities: false,
        include_container_items: false,
        json: true,
        chunk_size: 32,
        output,
        plan: None,
        patch: None,
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
        minecraft_version: None,
        bind: None,
        yes: false,
        replace: false,
        session_input: None,
        config_key: None,
        config_value: None,
        user: None,
        password_stdin: false,
        stockpile_mode: None,
        access_password_stdin: false,
        admin_password_stdin: false,
        whitelist_file: None,
        allow_guest_readonly: None,
        admin_page_enabled: None,
        stockpile_targets: Vec::new(),
    })
}

fn parse_stockpile_args(raw_args: Vec<String>) -> Result<CliArgs> {
    let mut args = raw_args.into_iter();
    let Some(subcommand) = args.next() else {
        bail!(
            "stockpile requires a subcommand: export-data | export-zip | recipe-status | recipe-fetch | serve"
        );
    };
    let mut input = None;
    let mut output = None;
    let mut include_container_items = false;
    let mut minecraft_version = None;
    let mut bind = None;
    let mut yes = false;
    let mut replace = false;
    let mut session_input = None;
    let mut config_key = None;
    let mut config_value = None;
    let mut user = None;
    let mut password_stdin = false;
    let mut stockpile_mode = None;
    let mut access_password_stdin = false;
    let mut admin_password_stdin = false;
    let mut whitelist_file = None;
    let mut allow_guest_readonly = None;
    let mut admin_page_enabled = None;
    let mut stockpile_targets = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --input");
                };
                if subcommand == "session-import" {
                    session_input = Some(PathBuf::from(path));
                } else {
                    input = Some(PathBuf::from(path));
                }
            }
            "--output" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --output");
                };
                output = Some(PathBuf::from(path));
            }
            "--include-container-items" => include_container_items = true,
            "--yes" => yes = true,
            "--replace" => replace = true,
            "--password-stdin" => password_stdin = true,
            "--password" => bail!("stockpile password options must use --password-stdin"),
            "--access-password-stdin" => access_password_stdin = true,
            "--admin-password-stdin" => admin_password_stdin = true,
            "--whitelist-file" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --whitelist-file");
                };
                whitelist_file = Some(PathBuf::from(path));
            }
            "--allow-guest-readonly" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --allow-guest-readonly");
                };
                allow_guest_readonly = Some(parse_bool_arg("--allow-guest-readonly", &value)?);
            }
            "--admin-page-enabled" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --admin-page-enabled");
                };
                admin_page_enabled = Some(parse_bool_arg("--admin-page-enabled", &value)?);
            }
            "--target" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --target");
                };
                validate_stockpile_target(&value)?;
                stockpile_targets.push(value);
            }
            "--mode" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --mode");
                };
                if value != "single" && value != "multi" {
                    bail!("stockpile --mode must be single or multi");
                }
                stockpile_mode = Some(value);
            }
            "--user" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --user");
                };
                if value.is_empty() {
                    bail!("missing value after --user");
                }
                user = Some(value);
            }
            "--key" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --key");
                };
                if value.is_empty() {
                    bail!("missing value after --key");
                }
                config_key = Some(value);
            }
            "--value" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --value");
                };
                if value.is_empty() {
                    bail!("missing value after --value");
                }
                config_value = Some(value);
            }
            "--minecraft-version" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --minecraft-version");
                };
                if value.is_empty() {
                    bail!("missing value after --minecraft-version");
                }
                minecraft_version = Some(value);
            }
            "--zip" => {
                let Some(path) = args.next() else {
                    bail!("missing path after --zip");
                };
                input = Some(PathBuf::from(path));
            }
            "--bind" => {
                let Some(value) = args.next() else {
                    bail!("missing value after --bind");
                };
                bind = Some(value);
            }
            _ if arg.starts_with("--input=") => {
                let value = arg.trim_start_matches("--input=");
                if value.is_empty() {
                    bail!("missing path after --input=");
                }
                if subcommand == "session-import" {
                    session_input = Some(PathBuf::from(value));
                } else {
                    input = Some(PathBuf::from(value));
                }
            }
            _ if arg.starts_with("--output=") => {
                let value = arg.trim_start_matches("--output=");
                if value.is_empty() {
                    bail!("missing path after --output=");
                }
                output = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--minecraft-version=") => {
                let value = arg.trim_start_matches("--minecraft-version=");
                if value.is_empty() {
                    bail!("missing value after --minecraft-version=");
                }
                minecraft_version = Some(value.to_string());
            }
            _ if arg.starts_with("--zip=") => {
                let value = arg.trim_start_matches("--zip=");
                if value.is_empty() {
                    bail!("missing path after --zip=");
                }
                input = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--bind=") => {
                let value = arg.trim_start_matches("--bind=");
                if value.is_empty() {
                    bail!("missing value after --bind=");
                }
                bind = Some(value.to_string());
            }
            _ if arg.starts_with("--key=") => {
                let value = arg.trim_start_matches("--key=");
                if value.is_empty() {
                    bail!("missing value after --key=");
                }
                config_key = Some(value.to_string());
            }
            _ if arg.starts_with("--value=") => {
                let value = arg.trim_start_matches("--value=");
                if value.is_empty() {
                    bail!("missing value after --value=");
                }
                config_value = Some(value.to_string());
            }
            _ if arg.starts_with("--user=") => {
                let value = arg.trim_start_matches("--user=");
                if value.is_empty() {
                    bail!("missing value after --user=");
                }
                user = Some(value.to_string());
            }
            _ if arg.starts_with("--password=") => {
                bail!("stockpile password options must use --password-stdin");
            }
            _ if arg.starts_with("--whitelist-file=") => {
                let value = arg.trim_start_matches("--whitelist-file=");
                if value.is_empty() {
                    bail!("missing path after --whitelist-file=");
                }
                whitelist_file = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--allow-guest-readonly=") => {
                let value = arg.trim_start_matches("--allow-guest-readonly=");
                allow_guest_readonly = Some(parse_bool_arg("--allow-guest-readonly", value)?);
            }
            _ if arg.starts_with("--admin-page-enabled=") => {
                let value = arg.trim_start_matches("--admin-page-enabled=");
                admin_page_enabled = Some(parse_bool_arg("--admin-page-enabled", value)?);
            }
            _ if arg.starts_with("--target=") => {
                let value = arg.trim_start_matches("--target=");
                validate_stockpile_target(value)?;
                stockpile_targets.push(value.to_string());
            }
            _ if arg.starts_with("--mode=") => {
                let value = arg.trim_start_matches("--mode=");
                if value != "single" && value != "multi" {
                    bail!("stockpile --mode must be single or multi");
                }
                stockpile_mode = Some(value.to_string());
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

    if matches!(subcommand.as_str(), "recipe-status" | "recipe-fetch") && input.is_some() {
        bail!("stockpile {subcommand} does not accept an input file");
    }
    if matches!(subcommand.as_str(), "recipe-status" | "recipe-fetch") && output.is_some() {
        bail!("stockpile {subcommand} does not accept --output");
    }
    if matches!(subcommand.as_str(), "recipe-status" | "recipe-fetch")
        && minecraft_version.is_none()
    {
        bail!("stockpile {subcommand} requires --minecraft-version <version>");
    }
    if !matches!(
        subcommand.as_str(),
        "export-data"
            | "export-zip"
            | "recipe-status"
            | "recipe-fetch"
            | "serve"
            | "session-info"
            | "session-reset"
            | "session-export"
            | "session-import"
            | "config-show"
            | "config-set"
            | "config-reset"
            | "set-access-password"
            | "clear-access-password"
            | "set-admin-password"
            | "clear-admin-password"
            | "whitelist-add"
            | "whitelist-remove"
            | "whitelist-list"
    ) {
        bail!("unsupported stockpile subcommand: {subcommand}");
    }
    if subcommand == "serve" && bind.is_none() {
        bail!("stockpile serve requires --bind <addr:port>");
    }
    if matches!(
        subcommand.as_str(),
        "session-info"
            | "session-reset"
            | "session-export"
            | "session-import"
            | "config-show"
            | "config-set"
            | "config-reset"
            | "set-access-password"
            | "clear-access-password"
            | "set-admin-password"
            | "clear-admin-password"
            | "whitelist-add"
            | "whitelist-remove"
            | "whitelist-list"
    ) && input.is_none()
    {
        bail!("stockpile {subcommand} requires --zip <path>");
    }
    if subcommand == "session-export" && output.is_none() {
        bail!("stockpile session-export requires --output <path>");
    }
    if subcommand == "session-import" && session_input.is_none() {
        bail!("stockpile session-import requires --input <state.json>");
    }
    if subcommand == "config-set" && (config_key.is_none() || config_value.is_none()) {
        bail!("stockpile config-set requires --key <key> --value <value>");
    }
    if matches!(
        subcommand.as_str(),
        "set-access-password" | "set-admin-password"
    ) && !password_stdin
    {
        bail!("stockpile {subcommand} requires --password-stdin");
    }
    if matches!(subcommand.as_str(), "whitelist-add" | "whitelist-remove") && user.is_none() {
        bail!("stockpile {subcommand} requires --user <id>");
    }

    Ok(CliArgs {
        command: format!("stockpile {subcommand}"),
        input: if matches!(
            subcommand.as_str(),
            "export-data"
                | "export-zip"
                | "serve"
                | "session-info"
                | "session-reset"
                | "session-export"
                | "session-import"
                | "config-show"
                | "config-set"
                | "config-reset"
                | "set-access-password"
                | "clear-access-password"
                | "set-admin-password"
                | "clear-admin-password"
                | "whitelist-add"
                | "whitelist-remove"
                | "whitelist-list"
        ) {
            input.ok_or_else(|| {
                if subcommand == "serve" {
                    anyhow::anyhow!("stockpile serve requires --zip <path>")
                } else {
                    anyhow::anyhow!("stockpile {subcommand} requires --input <path>")
                }
            })?
        } else {
            PathBuf::new()
        },
        include_entities: false,
        include_container_items,
        json: true,
        chunk_size: 32,
        output,
        plan: None,
        patch: None,
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
        minecraft_version,
        bind,
        yes,
        replace,
        session_input,
        config_key,
        config_value,
        user,
        password_stdin,
        stockpile_mode,
        access_password_stdin,
        admin_password_stdin,
        whitelist_file,
        allow_guest_readonly,
        admin_page_enabled,
        stockpile_targets,
    })
}

fn validate_stockpile_target(value: &str) -> Result<()> {
    match value {
        "windows-x64" | "linux-x64" | "macos-x64" | "macos-arm64" | "all" => Ok(()),
        _ => bail!(
            "stockpile --target must be windows-x64, linux-x64, macos-x64, macos-arm64, or all"
        ),
    }
}

fn parse_bool_arg(name: &str, value: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => bail!("{name} must be true or false"),
    }
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
        minecraft_version: None,
        bind: None,
        yes: false,
        replace: false,
        session_input: None,
        config_key: None,
        config_value: None,
        user: None,
        password_stdin: false,
        stockpile_mode: None,
        access_password_stdin: false,
        admin_password_stdin: false,
        whitelist_file: None,
        allow_guest_readonly: None,
        admin_page_enabled: None,
        stockpile_targets: Vec::new(),
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
        minecraft_version: None,
        bind: None,
        yes: false,
        replace: false,
        session_input: None,
        config_key: None,
        config_value: None,
        user: None,
        password_stdin: false,
        stockpile_mode: None,
        access_password_stdin: false,
        admin_password_stdin: false,
        whitelist_file: None,
        allow_guest_readonly: None,
        admin_page_enabled: None,
        stockpile_targets: Vec::new(),
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
        minecraft_version: None,
        bind: None,
        yes: false,
        replace: false,
        session_input: None,
        config_key: None,
        config_value: None,
        user: None,
        password_stdin: false,
        stockpile_mode: None,
        access_password_stdin: false,
        admin_password_stdin: false,
        whitelist_file: None,
        allow_guest_readonly: None,
        admin_page_enabled: None,
        stockpile_targets: Vec::new(),
    })
}
