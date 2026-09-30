use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub fn log_command(root: &Path, command: &str, input: Option<&str>, output: &str) {
    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

    let log_dir = root.join(".forge-mcp");
    if fs::create_dir_all(&log_dir).is_err() {
        return;
    }

    let log_file = log_dir.join(format!("command-log-debug--{date_str}.log"));
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_file) {
        let _ = writeln!(
            file,
            "================================================================================"
        );
        let _ = writeln!(file, "[{time_str}] COMMAND: {command}");
        if let Some(inp) = input {
            let _ = writeln!(file, "INPUT:\n{inp}");
        }
        let _ = writeln!(file, "OUTPUT:\n{output}\n");
    }
}
