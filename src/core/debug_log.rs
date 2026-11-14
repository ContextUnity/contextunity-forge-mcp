use chrono::Local;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

fn report_log_error(operation: &str, destination: &Path, error: &dyn std::fmt::Display) {
    let _ = writeln!(
        std::io::stderr().lock(),
        "debug log {operation} {}: {error}",
        destination.display()
    );
}

fn write_entry(root: &Path, date: &str, entry: &DebugLogEntry<'_>) {
    let directory = root.join(".forge");
    if let Err(error) = fs::create_dir_all(&directory) {
        report_log_error("create directory", &directory, &error);
        return;
    }
    let destination = directory.join(format!("command-log-debug--{date}.jsonl"));
    let line = match serde_json::to_string(entry) {
        Ok(line) => line,
        Err(error) => {
            report_log_error("serialize", &destination, &error);
            return;
        }
    };
    let mut file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&destination)
    {
        Ok(file) => file,
        Err(error) => {
            report_log_error("open", &destination, &error);
            return;
        }
    };
    if let Err(error) = writeln!(file, "{line}") {
        report_log_error("write", &destination, &error);
    }
}

#[derive(Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
/// Enumerates the supported debug log entry values.
pub enum DebugLogEntry<'a> {
    /// Represents the cli case.
    Cli {
        /// The timestamp value.
        timestamp: String,
        /// The command value.
        command: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// Optional parameters value.
        parameters: Option<serde_json::Value>,
        /// The output value.
        output: serde_json::Value,
    },
    /// Represents the mcp case.
    Mcp {
        /// The timestamp value.
        timestamp: String,
        /// The tool value.
        tool: &'a str,
        /// The parameters value.
        parameters: serde_json::Value,
        /// The output value.
        output: serde_json::Value,
    },
}

/// Performs log cli.
pub fn log_cli(
    root: &Path,
    command: &str,
    parameters: Option<&serde_json::Value>,
    output: &serde_json::Value,
) {
    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%Y-%m-%dT%H:%M:%S%.3f").to_string();

    let entry = DebugLogEntry::Cli {
        timestamp: time_str,
        command,
        parameters: parameters.cloned(),
        output: output.clone(),
    };

    write_entry(root, &date_str, &entry);
}

/// Performs log mcp.
pub fn log_mcp(root: &Path, tool: &str, parameters: &str, output: &serde_json::Value) {
    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%Y-%m-%dT%H:%M:%S%.3f").to_string();

    let params_val = serde_json::from_str::<serde_json::Value>(parameters)
        .unwrap_or_else(|_| serde_json::Value::String(parameters.to_string()));

    let entry = DebugLogEntry::Mcp {
        timestamp: time_str,
        tool,
        parameters: params_val,
        output: output.clone(),
    };

    write_entry(root, &date_str, &entry);
}

/// Performs log command.
pub fn log_command(root: &Path, command: &str, input: Option<&str>, output: &str) {
    let output_val = serde_json::from_str::<serde_json::Value>(output)
        .unwrap_or_else(|_| serde_json::Value::String(output.to_string()));
    let input_val = input.map(|inp| {
        serde_json::from_str::<serde_json::Value>(inp)
            .unwrap_or_else(|_| serde_json::Value::String(inp.to_string()))
    });
    log_cli(root, command, input_val.as_ref(), &output_val);
}
