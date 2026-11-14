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
    if target.exists() {
        let canonical_root = root.canonicalize()?;
        if !target.canonicalize()?.starts_with(&canonical_root) {
            return Err(path_error("path escapes workspace root"));
        }
    }
    Ok(target)
}

/// Returns `true` when `path` is a macOS OS-level root symlink (`/var`, `/tmp`, `/etc`
/// aliasing `/private/...`) that system temp and workspace paths traverse legitimately.
pub fn is_system_root_symlink(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        matches!(path.to_str(), Some("/var" | "/tmp" | "/etc"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// Validates that `path` and its existing ancestor directories do not traverse a
/// non-system-root symlink, returning `path` with its parent canonicalized so
/// `SQLITE_OPEN_NOFOLLOW` can enforce `O_NOFOLLOW` on the final file even under
/// macOS `/var` -> `/private/var` or `/tmp` -> `/private/tmp` paths.
pub fn confined_database_open_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    if fs::symlink_metadata(&absolute)?.file_type().is_symlink() {
        bail!("database must not be a symlink");
    }
    let parent = absolute.parent().context("database path needs a parent")?;
    let mut ancestor = PathBuf::new();
    for part in parent.components() {
        ancestor.push(part);
        if !is_system_root_symlink(&ancestor)
            && fs::symlink_metadata(&ancestor).is_ok_and(|m| m.file_type().is_symlink())
        {
            bail!("database parent must not traverse a symlink");
        }
    }
    let file_name = absolute
        .file_name()
        .context("database path needs a file name")?;
    Ok(parent.canonicalize()?.join(file_name))
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
            Ok(m) if m.file_type().is_symlink() && !is_system_root_symlink(&current) => bail!(
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
        #[cfg(unix)]
        let existing_permissions = if executable.is_none() {
            fs::symlink_metadata(path)
                .ok()
                .filter(|meta| !meta.file_type().is_symlink())
                .map(|meta| meta.permissions())
        } else {
            None
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if let Some(executable) = executable {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if executable { 0o755 } else { 0o600 });
        }
        let mut file = options.open(temporary)?;
        file.write_all(bytes)?;
        #[cfg(unix)]
        if let Some(permissions) = existing_permissions {
            file.set_permissions(permissions)?;
        }
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

/// Resolve a path by canonicalizing its deepest existing ancestor and appending
/// the remaining normalized components.
pub fn resolve_path_with_existing_prefix(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_owned())
    };
    let normalized = normalize_path(&absolute);
    canonicalize_with_missing_tail(&normalized).unwrap_or(normalized)
}

/// Canonicalize the deepest resolvable ancestor and retain the unresolved tail.
/// Returns `None` when no ancestor can be resolved.
pub fn canonicalize_with_missing_tail(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Some(canonical);
    }
    let mut tail = Vec::new();
    let mut cursor = path;
    while let (Some(parent), Some(name)) = (cursor.parent(), cursor.file_name()) {
        tail.push(name.to_os_string());
        cursor = parent;
        if let Ok(mut resolved) = fs::canonicalize(cursor) {
            for part in tail.into_iter().rev() {
                resolved.push(part);
            }
            return Some(resolved);
        }
    }
    None
}

#[cfg(unix)]
fn flip_first_ascii_alpha(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    let mut flipped = false;
    for ch in name.chars() {
        if !flipped && ch.is_ascii_alphabetic() {
            out.push(if ch.is_ascii_lowercase() {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            });
            flipped = true;
        } else {
            out.push(ch);
        }
    }
    flipped.then_some(out)
}

/// Returns `true` when the volume backing `path` (or its deepest existing ancestor)
/// resolves ASCII case variants to the same directory entry without writing to disk.
pub fn is_case_insensitive_volume(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let _ = path;
        true
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Some(existing) = path.ancestors().find(|p| fs::symlink_metadata(p).is_ok()) else {
            return false;
        };
        let Ok(base_meta) = fs::symlink_metadata(existing) else {
            return false;
        };
        let target_dev = base_meta.dev();

        let same_inode_under_flip =
            |entry_path: &Path, entry_meta: &fs::Metadata| -> Option<bool> {
                let parent = entry_path.parent()?;
                let name = entry_path.file_name()?.to_str()?;
                let flipped = flip_first_ascii_alpha(name)?;
                let alt = parent.join(flipped);
                match fs::symlink_metadata(&alt) {
                    Ok(alt_meta) => {
                        Some(alt_meta.dev() == target_dev && alt_meta.ino() == entry_meta.ino())
                    }
                    Err(_) => Some(false),
                }
            };

        for ancestor in existing.ancestors() {
            let Ok(meta) = fs::symlink_metadata(ancestor) else {
                break;
            };
            if meta.dev() != target_dev {
                break;
            }
            if let Some(matched) = same_inode_under_flip(ancestor, &meta) {
                return matched;
            }
        }
        if existing.is_dir() {
            if let Ok(entries) = fs::read_dir(existing) {
                for entry in entries.flatten().take(16) {
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    if meta.dev() != target_dev {
                        continue;
                    }
                    if let Some(matched) = same_inode_under_flip(&entry.path(), &meta) {
                        return matched;
                    }
                }
            }
        }
        false
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        false
    }
}

/// Compares two paths by filesystem identity when existing, and by canonicalized
/// existing-ancestor resolution (respecting case-insensitive volumes) when absent.
pub fn same_filesystem_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    #[cfg(unix)]
    if let (Ok(left_meta), Ok(right_meta)) =
        (fs::symlink_metadata(left), fs::symlink_metadata(right))
    {
        use std::os::unix::fs::MetadataExt;
        if left_meta.dev() == right_meta.dev() && left_meta.ino() == right_meta.ino() {
            return true;
        }
    }
    let resolved_left = resolve_path_with_existing_prefix(left);
    let resolved_right = resolve_path_with_existing_prefix(right);
    if resolved_left == resolved_right {
        return true;
    }
    if is_case_insensitive_volume(&resolved_left) || is_case_insensitive_volume(&resolved_right) {
        let mut left_parts = resolved_left.components();
        let mut right_parts = resolved_right.components();
        loop {
            match (left_parts.next(), right_parts.next()) {
                (None, None) => return true,
                (Some(l), Some(r)) if l == r => continue,
                (Some(std::path::Component::Normal(l)), Some(std::path::Component::Normal(r))) => {
                    let (Some(ls), Some(rs)) = (l.to_str(), r.to_str()) else {
                        return false;
                    };
                    if ls.to_lowercase() != rs.to_lowercase() {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }
    false
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

    #[test]
    fn test_is_system_root_symlink_only_exempts_macos_os_roots() {
        assert!(!is_system_root_symlink(Path::new("/")));
        assert!(!is_system_root_symlink(Path::new("/var/folders")));
        assert!(!is_system_root_symlink(Path::new("/tmp/evil-link")));
        assert_eq!(
            is_system_root_symlink(Path::new("/var")),
            cfg!(target_os = "macos")
        );
        assert_eq!(
            is_system_root_symlink(Path::new("/tmp")),
            cfg!(target_os = "macos")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_confined_database_open_path_canonicalizes_parent_and_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("forge_core_db_open_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let _cleanup = TempDirDrop(base.clone());
        let real_dir = base.join("real");
        fs::create_dir_all(&real_dir).unwrap();
        let db = real_dir.join("code-map.sqlite");
        fs::write(&db, b"sqlite").unwrap();

        let resolved = confined_database_open_path(&db).unwrap();
        assert_eq!(resolved, db);

        let symlink_file = base.join("alias.sqlite");
        symlink(&db, &symlink_file).unwrap();
        assert!(confined_database_open_path(&symlink_file).is_err());

        let symlink_dir = base.join("link_dir");
        symlink(&real_dir, &symlink_dir).unwrap();
        assert!(confined_database_open_path(&symlink_dir.join("code-map.sqlite")).is_err());

        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn test_checked_child_allows_uncanonicalized_root_and_rejects_child_symlinks() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("forge_core_checked_child_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let _cleanup = TempDirDrop(base.clone());
        let real_root = base.join("private_var/ws");
        fs::create_dir_all(real_root.join("src")).unwrap();
        fs::write(real_root.join("src/lib.rs"), b"pub fn ok() {}").unwrap();

        let alias_root = base.join("var_ws");
        symlink(&real_root, &alias_root).unwrap();
        assert_eq!(
            checked_child(&alias_root, Path::new("src/lib.rs")).unwrap(),
            alias_root.join("src/lib.rs")
        );

        let outside = base.join("outside.rs");
        fs::write(&outside, b"pub fn secret() {}").unwrap();
        symlink(&outside, real_root.join("src/escape.rs")).unwrap();
        assert!(checked_child(&alias_root, Path::new("src/escape.rs")).is_err());

        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_replace_preserves_existing_permissions_and_applies_default_for_new() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("forge_core_atomic_replace_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let _cleanup = TempDirDrop(base.clone());
        fs::create_dir_all(&base).unwrap();

        let private_file = base.join("secret.md");
        fs::write(&private_file, b"v1").unwrap();
        fs::set_permissions(&private_file, fs::Permissions::from_mode(0o600)).unwrap();
        atomic_replace(&private_file, b"v2").unwrap();
        assert_eq!(
            fs::metadata(&private_file).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let script_file = base.join("run.sh");
        fs::write(&script_file, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&script_file, fs::Permissions::from_mode(0o755)).unwrap();
        atomic_replace(&script_file, b"#!/bin/sh\necho ok\n").unwrap();
        assert_eq!(
            fs::metadata(&script_file).unwrap().permissions().mode() & 0o777,
            0o755
        );

        let new_file = base.join("brand-new.md");
        atomic_replace(&new_file, b"hello").unwrap();
        assert_eq!(fs::read(&new_file).unwrap(), b"hello");

        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn test_same_filesystem_path_matches_absent_files_under_symlinked_ancestors() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("forge_core_same_fs_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let _cleanup = TempDirDrop(base.clone());
        let real_root = base.join("real");
        fs::create_dir_all(&real_root).unwrap();
        let alias_root = base.join("alias");
        symlink(&real_root, &alias_root).unwrap();

        let absent_real = real_root.join(".forge/state.sqlite");
        let absent_alias = alias_root.join(".forge/sub/../state.sqlite");
        assert!(same_filesystem_path(&absent_real, &absent_alias));

        let other_absent = alias_root.join(".forge/code-map.sqlite");
        assert!(!same_filesystem_path(&absent_real, &other_absent));

        let _ = fs::remove_dir_all(&base);
    }
    #[cfg(unix)]
    struct TempDirDrop(PathBuf);

    #[cfg(unix)]
    impl Drop for TempDirDrop {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
