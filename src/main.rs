use clap::Parser;
use std::io::{self, Write};

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    match contextunity_forge_mcp::cli::Cli::parse().run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
