use crate::cli;
use crate::common::Workspace;
use clap::Parser;
use contextunity_forge_mcp::cli::Cli;
use std::io::{self, Write};

struct FailingWriter(io::ErrorKind);

impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(self.0, "output unavailable"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn cli_writer_preserves_broken_pipe_success_and_other_write_failures() {
    let workspace = Workspace::with_files(&[
        ("forge-mcp.yaml", "profile: minimal\n"),
        ("notes.txt", "hello\n"),
    ]);
    for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::Other] {
        let cli = Cli::try_parse_from([
            "fixture",
            "--root",
            workspace.root().to_str().unwrap(),
            "read",
            "notes.txt",
        ])
        .unwrap();
        let result = runtime().block_on(cli.run_with_writer(&mut FailingWriter(kind)));
        if kind == io::ErrorKind::BrokenPipe {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result
                    .unwrap_err()
                    .downcast_ref::<io::Error>()
                    .unwrap()
                    .kind(),
                kind
            );
        }
    }
}

#[test]
fn docs_check_preserves_output_write_panics_for_success_and_error_reports() {
    for case in ["success", "broken-link", "missing-root"] {
        let workspace = Workspace::with_files(&[(
            "README.md",
            if case == "broken-link" {
                "[bad](absent.md)\n"
            } else {
                "# Valid\n"
            },
        )]);
        let root = if case == "missing-root" {
            workspace.path("absent")
        } else {
            workspace.root().to_path_buf()
        };
        let cli = Cli::try_parse_from([
            "fixture",
            "--root",
            root.to_str().unwrap(),
            "--format",
            "json",
            "docs",
            "check",
        ])
        .unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime().block_on(cli.run_with_writer(&mut FailingWriter(io::ErrorKind::BrokenPipe)))
        }));
        let panic = panic.expect_err(case);
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap();
        assert_eq!(
            message, "failed printing to stdout: output unavailable",
            "{case}"
        );
    }
}

#[test]
fn real_cli_preserves_captured_output_parser_errors_and_process_exit_status() {
    let workspace = Workspace::with_files(&[
        ("forge-mcp.yaml", "profile: minimal\n"),
        ("notes.txt", "hello\n"),
    ]);
    for args in [
        vec!["--format", "json", "read", "notes.txt"],
        vec!["read", "../notes.txt"],
        vec!["--format", "invalid", "read", "notes.txt"],
    ] {
        let captured = cli::run_cli(&workspace, &args);
        let process = crate::common::mcp_client::run_cli(&workspace, &args);
        assert_eq!(captured.status.code(), process.status.code(), "{args:?}");
        assert_eq!(captured.stdout, process.stdout, "{args:?}");
        assert_eq!(captured.stderr, process.stderr, "{args:?}");
    }
}
