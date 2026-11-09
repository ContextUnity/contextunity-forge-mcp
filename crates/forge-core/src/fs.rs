use anyhow::{bail, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NONCE: AtomicU64 = AtomicU64::new(0);

fn path_error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

/// Resolve a workspace root to a canonical directory.
pub fn canonical_root(path: &Path) -> std::io::Result<PathBuf> {
    let root = path.canonicalize()?;
    if !root.is_dir() {
        return Err(path_error("workspace root must be a directory"));
    }
    Ok(root)
}

/// Lexically normalize a path without requiring it to exist on disk.
/// Resolves `.` and `..` components.
pub fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = components.last() {
                    components.pop();
                } else if !matches!(components.last(), Some(Component::RootDir)) {
                    components.push(component);
                }
            }
            c => components.push(c),
        }
    }
    components.into_iter().collect()
}

/// Return whether a path consists only of normal relative components.
pub fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|part| {
            matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

/// Return whether a path consists only of normal relative components (no `.`, `..`, root, or prefix).
pub fn is_normalized_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

/// Return whether a string contains POSIX shell metacharacters that could enable command injection or argument splitting.
pub fn contains_shell_metacharacters(value: &str) -> bool {
    value.chars().any(|c| {
        matches!(
            c,
            '$' | '`'
                | ';'
                | '&'
                | '|'
                | '('
                | ')'
                | '{'
                | '}'
                | '<'
                | '>'
                | '"'
                | '\''
                | '\\'
                | '\n'
                | '\r'
                | '\t'
        )
    })
}

/// Return whether a string is a safe project-relative command path without shell metacharacters.
pub fn is_safe_project_path(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let path_ref = Path::new(trimmed);
    is_normalized_relative(path_ref) && !contains_shell_metacharacters(trimmed)
}

/// Return whether a string is a valid alphanumeric/underscore identifier symbol.
pub fn is_valid_symbol_identifier(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Return whether a string is a valid qualified symbol identifier (e.g. `foo::bar::baz` or `foo.bar`).
pub fn is_valid_qualified_symbol_identifier(symbol: &str) -> bool {
    if symbol.is_empty() || contains_shell_metacharacters(symbol) {
        return false;
    }
    symbol
        .split("::")
        .flat_map(|seg| seg.split('.'))
        .all(is_valid_symbol_identifier)
}

/// Return whether a path represents a documentation file.
pub fn is_doc_path(p: &str) -> bool {
    p.starts_with("docs/")
        || p.starts_with("docs\\")
        || p.ends_with(".md")
        || p.ends_with(".markdown")
        || p.ends_with(".txt")
        || p.ends_with(".rst")
        || p.ends_with(".adoc")
}

/// Resolve a relative path without traversing a symlink component.
pub fn checked_child(root: &Path, relative: &Path) -> std::io::Result<PathBuf> {
    if !safe_relative(relative) {
        return Err(path_error("path must be a relative plain path"));
    }
    let target = root.join(relative);
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let name = match component {
            std::path::Component::CurDir => continue,
            std::path::Component::Normal(name) => name,
            _ => return Err(path_error("path must be relative")),
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(path_error("path must not traverse a symlink"));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                // Remaining nonexistent components cannot contain symlinks; recorded deletions retain admission.
                return Ok(target);
            }
            Err(source) => return Err(source),
        }
    }
    if target.exists() && !target.canonicalize()?.starts_with(root) {
        return Err(path_error("path escapes workspace root"));
    }
    Ok(target)
}

/// Performs atomic write.
pub fn atomic_write(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let parent = absolute.parent().context("missing parent")?;
    let mut current = PathBuf::new();
    for c in parent.components() {
        current.push(c);
        match fs::symlink_metadata(&current) {
            Ok(m) if m.file_type().is_symlink() => bail!(
                "output parent must not traverse symlink: {}",
                current.display()
            ),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    if fs::symlink_metadata(&absolute).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("output must not be a symlink");
    }
    let temporary = parent.join(format!(
        ".forge-write-{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    commit_atomic_temp(&absolute, &temporary, bytes, Some(executable), true)
}

/// Atomically replace a file with the supplied contents.
pub fn atomic_replace(path: &Path, contents: &[u8]) -> Result<()> {
    let name = path
        .file_name()
        .context("milestone filename missing")?
        .to_string_lossy();
    let temporary = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    commit_atomic_temp(path, &temporary, contents, None, false)
}

fn commit_atomic_temp(
    path: &Path,
    temporary: &Path,
    bytes: &[u8],
    executable: Option<bool>,
    sync_parent: bool,
) -> Result<()> {
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if let Some(executable) = executable {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if executable { 0o755 } else { 0o600 });
        }
        let mut file = options.open(temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(temporary, path)?;
        if sync_parent {
            let parent = path.parent().context("missing parent")?;
            OpenOptions::new().read(true).open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_normalized_relative() {
        assert!(is_normalized_relative(Path::new("src/lib.rs")));
        assert!(is_normalized_relative(Path::new("tests/acdd/tasks.rs")));
        assert!(is_normalized_relative(Path::new("src/space # λ.rs")));
        assert!(!is_normalized_relative(Path::new("")));
        assert!(!is_normalized_relative(Path::new("/src/lib.rs")));
        assert!(!is_normalized_relative(Path::new("../src/lib.rs")));
        assert!(!is_normalized_relative(Path::new("./src/lib.rs")));
        assert!(!is_normalized_relative(Path::new("foo/../bar.rs")));
    }

    #[test]
    fn test_contains_shell_metacharacters() {
        assert!(!contains_shell_metacharacters("src/lib.rs"));
        assert!(!contains_shell_metacharacters("src/space # λ.rs"));
        assert!(!contains_shell_metacharacters("my_test_fn"));
        assert!(contains_shell_metacharacters("$(touch evil)"));
        assert!(contains_shell_metacharacters("foo; rm -rf"));
        assert!(contains_shell_metacharacters("foo && bar"));
        assert!(contains_shell_metacharacters("foo | bar"));
        assert!(contains_shell_metacharacters("`id`"));
        assert!(contains_shell_metacharacters("foo\nbar"));
        assert!(contains_shell_metacharacters("foo'bar"));
        assert!(contains_shell_metacharacters("foo\"bar"));
    }

    #[test]
    fn test_is_safe_project_path() {
        assert!(is_safe_project_path("src/lib.rs"));
        assert!(is_safe_project_path("src/space # λ.rs"));
        assert!(!is_safe_project_path(""));
        assert!(!is_safe_project_path("   "));
        assert!(!is_safe_project_path("/etc/passwd"));
        assert!(!is_safe_project_path("../outside.rs"));
        assert!(!is_safe_project_path("tests/x$(touch bad).rs"));
    }

    #[test]
    fn test_is_valid_symbol_identifier() {
        assert!(is_valid_symbol_identifier("my_test_fn"));
        assert!(is_valid_symbol_identifier("Test123_abc"));
        assert!(!is_valid_symbol_identifier(""));
        assert!(!is_valid_symbol_identifier("my test"));
        assert!(!is_valid_symbol_identifier("my_test; evil"));
        assert!(!is_valid_symbol_identifier("test-dash"));
    }

    #[test]
    fn test_is_valid_qualified_symbol_identifier() {
        assert!(is_valid_qualified_symbol_identifier("my_test_fn"));
        assert!(is_valid_qualified_symbol_identifier("foo::bar::my_test"));
        assert!(is_valid_qualified_symbol_identifier("module.test_func"));
        assert!(!is_valid_qualified_symbol_identifier(""));
        assert!(!is_valid_qualified_symbol_identifier("foo::"));
        assert!(!is_valid_qualified_symbol_identifier("::bar"));
        assert!(!is_valid_qualified_symbol_identifier("foo::::bar"));
        assert!(!is_valid_qualified_symbol_identifier("foo; rm -rf"));
        assert!(!is_valid_qualified_symbol_identifier("foo::bar bad"));
    }
}
