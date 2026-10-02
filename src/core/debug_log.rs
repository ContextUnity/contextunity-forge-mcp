use chrono::Local;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

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

    let log_dir = root.join(".forge");
    if fs::create_dir_all(&log_dir).is_err() {
        return;
    }

    let entry = DebugLogEntry::Cli {
        timestamp: time_str,
        command,
        parameters: parameters.cloned(),
        output: output.clone(),
    };

    if let Ok(line) = serde_json::to_string(&entry) {
        let jsonl_file = log_dir.join(format!("command-log-debug--{date_str}.jsonl"));
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(jsonl_file)
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// Performs log mcp.
pub fn log_mcp(root: &Path, tool: &str, parameters: &str, output: &serde_json::Value) {
    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%Y-%m-%dT%H:%M:%S%.3f").to_string();

    let log_dir = root.join(".forge");
    if fs::create_dir_all(&log_dir).is_err() {
        return;
    }

    let params_val = serde_json::from_str::<serde_json::Value>(parameters)
        .unwrap_or_else(|_| serde_json::Value::String(parameters.to_string()));

    let entry = DebugLogEntry::Mcp {
        timestamp: time_str,
        tool,
        parameters: params_val,
        output: output.clone(),
    };

    if let Ok(line) = serde_json::to_string(&entry) {
        let jsonl_file = log_dir.join(format!("command-log-debug--{date_str}.jsonl"));
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(jsonl_file)
        {
            let _ = writeln!(file, "{line}");
        }
    }
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
