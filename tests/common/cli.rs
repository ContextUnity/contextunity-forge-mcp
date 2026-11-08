use super::Workspace;
use clap::Parser;
use contextunity_forge_mcp::cli::Cli;
use std::{ffi::OsString, process::Output};

/// Executes the real Clap parser, CLI dispatcher, and formatter in process.
pub fn run_cli(workspace: &Workspace, args: &[&str]) -> Output {
    let mut argv = vec![OsString::from("--root"), workspace.root().into()];
    argv.extend(args.iter().map(OsString::from));
    run_cli_args(&argv)
}

pub fn run_cli_json(workspace: &Workspace, args: &[&str]) -> Output {
    let mut json_args = vec!["--format", "json"];
    json_args.extend_from_slice(args);
    run_cli(workspace, &json_args)
}

pub fn run_cli_args(args: &[OsString]) -> Output {
    let argv =
        std::iter::once(OsString::from("contextunity-forge-mcp")).chain(args.iter().cloned());
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(error) => {
            let mut output = Output {
                status: exit_status(error.exit_code()),
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
            let message = error.to_string().into_bytes();
            if error.use_stderr() {
                output.stderr = message;
            } else {
                output.stdout = message;
            }
            return output;
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut stdout = Vec::new();
    let positional_root = match &cli.command {
        Some(
            contextunity_forge_mcp::cli::Command::Build { workspace_root, .. }
            | contextunity_forge_mcp::cli::Command::Scan { workspace_root, .. }
            | contextunity_forge_mcp::cli::Command::Delta { workspace_root, .. },
        ) => workspace_root.clone(),
        _ => None,
    };
    let cache_root = cli
        .root
        .clone()
        .or(positional_root)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let result = runtime.block_on(
        cli.run_with_writer_and_cache_dir(&mut stdout, &cache_root.join(".forge/extraction-cache")),
    );
    Output {
        status: exit_status(i32::from(result.is_err())),
        stdout,
        stderr: result
            .err()
            .map_or_else(Vec::new, |error| format!("{error:#}\n").into_bytes()),
    }
}

#[cfg(unix)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code as u32)
}
