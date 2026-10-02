use anyhow::{bail, Result};
use serde_json::{json, Value};

#[cfg(target_os = "linux")]
/// Sends SIGHUP to matching servers and returns their process IDs.
pub fn run() -> Result<Value> {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};

    let executable = std::env::current_exe()?;
    let uid = unsafe { libc::geteuid() };
    let mut signaled = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() as i32 {
            continue;
        }
        let proc = entry.path();
        let Ok(metadata) = std::fs::metadata(&proc) else {
            continue;
        };
        if metadata.uid() != uid {
            continue;
        }
        let Ok(target) = std::fs::read_link(proc.join("exe")) else {
            continue;
        };
        let target = target.as_os_str().as_bytes();
        if target != executable.as_os_str().as_bytes()
            && target.strip_suffix(b" (deleted)") != Some(executable.as_os_str().as_bytes())
        {
            continue;
        }
        let Ok(cmdline) = std::fs::read(proc.join("cmdline")) else {
            continue;
        };
        if !cmdline
            .split(|byte| *byte == 0)
            .skip(1)
            .any(|arg| arg == b"serve")
        {
            continue;
        }
        if unsafe { libc::kill(pid, libc::SIGHUP) } == 0 {
            signaled.push(pid);
        } else {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    if signaled.is_empty() {
        bail!("no running contextunity-forge-mcp serve process found for this executable and user");
    }
    Ok(json!({"signaled_pids": signaled}))
}

#[cfg(not(target_os = "linux"))]
/// Reports that local process discovery is unavailable on this platform.
pub fn run() -> Result<Value> {
    bail!("reload process discovery is supported on Linux only")
}
