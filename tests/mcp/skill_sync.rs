use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Parser, Tag};

#[test]
fn test_global_skill_matches_local_skill_when_present() {
    let repo_skill = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/contextunity-forge");
    assert!(
        repo_skill.is_dir(),
        "Repository ContextUnity Forge skill is missing at {}.",
        repo_skill.display()
    );

    let global_skill =
        if let Some(test_root) = std::env::var_os("CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR") {
            PathBuf::from(test_root)
        } else {
            let home = std::env::var_os("HOME").unwrap_or_else(|| {
                panic!(
                    "HOME is not set; cannot locate the required global ContextUnity Forge skill."
                )
            });
            PathBuf::from(home).join(".agents/skills/contextunity-forge")
        };
    assert!(
        global_skill.is_dir(),
        "Required global ContextUnity Forge skill installation is missing at {}. Install the skill there or set CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR when running an isolated sync test.",
        global_skill.display()
    );

    let repo_files = read_skill_files(&repo_skill).unwrap_or_else(|error| {
        panic!(
            "Cannot read repository skill tree at {}: {error}",
            repo_skill.display()
        )
    });
    let global_files = read_skill_files(&global_skill).unwrap_or_else(|error| {
        panic!(
            "Cannot read installed skill tree at {}: {error}",
            global_skill.display()
        )
    });

    let missing_from_global: Vec<_> = repo_files
        .keys()
        .filter(|path| !global_files.contains_key(*path))
        .collect();
    let extra_in_global: Vec<_> = global_files
        .keys()
        .filter(|path| !repo_files.contains_key(*path))
        .collect();
    let content_drift: Vec<_> = repo_files
        .iter()
        .filter_map(|(path, repo_content)| {
            global_files
                .get(path)
                .filter(|global_content| *global_content != repo_content)
                .map(|_| path)
        })
        .collect();

    assert!(
        missing_from_global.is_empty()
            && extra_in_global.is_empty()
            && content_drift.is_empty(),
        "Installed ContextUnity Forge skill at {} differs from repository skill at {}. Missing relative files: {missing_from_global:?}; extra relative files: {extra_in_global:?}; changed content: {content_drift:?}. Synchronize the installed skill.",
        global_skill.display(),
        repo_skill.display()
    );
}

fn read_skill_files(root: &Path) -> io::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    collect_skill_files(root, root, &mut files)?;
    Ok(files)
}

fn collect_skill_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_skill_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative_path = path.strip_prefix(root).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, error.to_string())
            })?;
            files.insert(relative_path.to_path_buf(), fs::read(path)?);
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported skill entry at {}", path.display()),
            ));
        }
    }
    Ok(())
}

#[test]
fn test_skill_markdown_links_resolve_from_supported_install_layouts() {
    let repo_skill = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/contextunity-forge");
    let skill_files = read_skill_files(&repo_skill).unwrap_or_else(|error| {
        panic!(
            "Cannot read repository skill tree at {}: {error}",
            repo_skill.display()
        )
    });
    let temp_root = TemporaryDirectory::new()
        .unwrap_or_else(|error| panic!("Cannot create temporary skill install root: {error}"));
    let layouts = [
        (
            "user-level",
            temp_root
                .path
                .join("home/.agents/skills/contextunity-forge"),
        ),
        (
            "repository-local",
            temp_root
                .path
                .join("repository/.agents/skills/contextunity-forge"),
        ),
    ];

    let mut unresolved_layouts = Vec::new();
    for (layout, installed_skill) in layouts {
        write_skill_files(&installed_skill, &skill_files).unwrap_or_else(|error| {
            panic!(
                "Cannot copy the skill into the isolated {layout} installation at {}: {error}",
                installed_skill.display()
            )
        });
        if let Err(error) = validate_installed_skill_links(&installed_skill) {
            unresolved_layouts.push(format!(
                "{layout} installation at {}: {error}",
                installed_skill.display()
            ));
        }
    }
    assert!(
        unresolved_layouts.is_empty(),
        "Markdown links do not resolve in supported installed skill layouts: {unresolved_layouts:?}"
    );
}

fn write_skill_files(root: &Path, files: &BTreeMap<PathBuf, Vec<u8>>) -> io::Result<()> {
    for (relative_path, contents) in files {
        let destination = root.join(relative_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, contents)?;
    }
    Ok(())
}

fn validate_installed_skill_links(skill_root: &Path) -> Result<(), String> {
    let canonical_root = fs::canonicalize(skill_root)
        .map_err(|error| format!("cannot resolve install root: {error}"))?;
    let installed_files = read_skill_files(skill_root)
        .map_err(|error| format!("cannot read installed skill tree: {error}"))?;

    for (relative_path, contents) in installed_files {
        if !is_markdown_file(&relative_path) {
            continue;
        }
        let markdown_path = skill_root.join(&relative_path);
        let content = String::from_utf8(contents)
            .map_err(|error| format!("{} is not UTF-8: {error}", markdown_path.display()))?;
        let canonical_markdown_path = fs::canonicalize(&markdown_path)
            .map_err(|error| format!("cannot resolve {}: {error}", markdown_path.display()))?;

        for event in Parser::new(&content) {
            let destination = match event {
                Event::Start(Tag::Link { dest_url, .. })
                | Event::Start(Tag::Image { dest_url, .. }) => dest_url.to_string(),
                _ => continue,
            };
            if has_uri_scheme(&destination) {
                continue;
            }

            let file_target = destination
                .split(['#', '?'])
                .next()
                .unwrap_or_default();
            if file_target.is_empty() {
                continue;
            }

            let target_path = canonical_markdown_path
                .parent()
                .expect("Markdown file has an installation directory")
                .join(file_target);
            let canonical_target = fs::canonicalize(&target_path).map_err(|error| {
                format!(
                    "link `{destination}` in {} points to missing target {}: {error}",
                    markdown_path.display(),
                    target_path.display()
                )
            })?;
            if !canonical_target.starts_with(&canonical_root) {
                return Err(format!(
                    "link `{destination}` in {} escapes installed skill root to {}",
                    markdown_path.display(),
                    canonical_target.display()
                ));
            }
        }
    }
    Ok(())
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "md")
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".md.example"))
}

fn has_uri_scheme(destination: &str) -> bool {
    let Some((scheme, _)) = destination.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme.chars().enumerate().all(|(index, character)| {
            if index == 0 {
                character.is_ascii_alphabetic()
            } else {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            }
        })
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new() -> io::Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| io::Error::other(error.to_string()))?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "contextunity-forge-skill-links-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
