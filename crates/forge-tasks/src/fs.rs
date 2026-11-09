use anyhow::{bail, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub use forge_core::fs::{
    atomic_replace, atomic_write, canonical_root, checked_child, contains_shell_metacharacters,
    is_doc_path, is_normalized_relative, is_safe_project_path,
    is_valid_qualified_symbol_identifier, is_valid_symbol_identifier, normalize_path,
    safe_relative,
};

pub fn path_component_matches(component: &str, pattern: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        component.starts_with(prefix)
            && component.ends_with(suffix)
            && component.len() >= prefix.len() + suffix.len()
    } else {
        component == pattern
    }
}

pub fn matches_path_pattern(rel_path: &str, pattern: &str) -> bool {
    let rel_norm = rel_path.trim_matches('/').replace('\\', "/");
    let pat_norm = pattern.trim_matches('/').replace('\\', "/");
    if pat_norm.is_empty() || rel_norm.is_empty() {
        return false;
    }
    if pat_norm.contains('*') {
        let pattern_parts: Vec<&str> = pat_norm.split('/').collect();
        let path_parts: Vec<&str> = rel_norm.split('/').collect();
        if path_parts.len() < pattern_parts.len() {
            return false;
        }
        for (i, part) in pattern_parts.iter().enumerate() {
            if !path_component_matches(path_parts[i], part) {
                return false;
            }
        }
        true
    } else {
        rel_norm == pat_norm || rel_norm.starts_with(&format!("{}/", pat_norm))
    }
}

fn lock_suffix(path: &Path, extension: &str) -> PathBuf {
    let mut file = path.as_os_str().to_owned();
    file.push(extension);
    PathBuf::from(file)
}

pub fn exclusive_lock(path: &Path) -> Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        use std::time::{Duration, Instant};

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(lock_suffix(path, ".lock"))?;
        if !file.metadata()?.is_file() {
            bail!("database lock must be a regular file");
        }
        let operation = libc::LOCK_EX;
        let started = Instant::now();
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) } == 0 {
                return Ok(file);
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == std::io::ErrorKind::WouldBlock {
                if started.elapsed() >= Duration::from_secs(30) {
                    bail!("database generation lock is busy; retry after active readers finish");
                }
            } else {
                return Err(error.into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        bail!("generation locks require Unix file locking")
    }
}
