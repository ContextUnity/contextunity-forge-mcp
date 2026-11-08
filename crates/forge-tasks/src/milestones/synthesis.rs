use crate::core::git::{git_status, git_stdout};
use crate::core::tasks::{
    AcddError, AcddErrorCode, ContractStatus, ContractType, Milestone, MilestoneContractEntry,
    MilestoneContractMatrix, RepositorySpec, TaskSpec,
};
use crate::db::tasks_store::TasksStore;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Validate local task dependencies and return stable, manifest-ordered execution waves.
pub(crate) fn validate_wave_dag(
    milestone: &Milestone,
) -> std::result::Result<Vec<Vec<String>>, AcddError> {
    let task_refs = milestone
        .tasks
        .iter()
        .map(|task| task.task_ref.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut completed = std::collections::BTreeSet::new();
    let mut waves = Vec::new();

    while completed.len() < milestone.tasks.len() {
        let wave = milestone
            .tasks
            .iter()
            .filter(|task| !completed.contains(&task.task_ref))
            .filter(|task| {
                task.depends_on
                    .iter()
                    .filter(|dependency| task_refs.contains(dependency.as_str()))
                    .all(|dependency| completed.contains(dependency))
            })
            .map(|task| task.task_ref.clone())
            .collect::<Vec<_>>();

        if wave.is_empty() {
            return Err(AcddError::new(
                AcddErrorCode::MilestoneDagCycle,
                "task dependency graph contains a cycle",
            ));
        }

        validate_wave_scopes(milestone, &wave)?;
        completed.extend(wave.iter().cloned());
        waves.push(wave);
    }

    Ok(waves)
}

fn validate_wave_scopes(
    milestone: &Milestone,
    wave: &[String],
) -> std::result::Result<(), AcddError> {
    for (index, left_ref) in wave.iter().enumerate() {
        let Some(left) = milestone
            .tasks
            .iter()
            .find(|task| task.task_ref == *left_ref)
        else {
            continue;
        };
        for right_ref in wave.iter().skip(index + 1) {
            let Some(right) = milestone
                .tasks
                .iter()
                .find(|task| task.task_ref == *right_ref)
            else {
                continue;
            };
            for left_path in task_paths(left) {
                for right_path in task_paths(right) {
                    if paths_overlap(&left_path, &right_path) {
                        return Err(AcddError::new(
                            AcddErrorCode::TaskScopeInvalid,
                            format!(
                                "same-wave tasks '{}' and '{}' overlap at '{}' and '{}'",
                                left.task_ref,
                                right.task_ref,
                                left_path.display(),
                                right_path.display()
                            ),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn task_paths(task: &TaskSpec) -> impl Iterator<Item = PathBuf> + '_ {
    task.scope
        .iter()
        .chain(task.extended_scope.iter())
        .map(PathBuf::from)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

/// Find public symbols admitted by more than one task and describe their shared contract.
pub fn extract_shared_helpers(milestone: &Milestone) -> Vec<MilestoneContractEntry> {
    let mut entries = Vec::new();
    for (index, source) in milestone.tasks.iter().enumerate() {
        let source_symbols = source
            .symbols
            .iter()
            .chain(source.extended_symbols.iter())
            .collect::<std::collections::BTreeSet<_>>();
        for target in milestone.tasks.iter().skip(index + 1) {
            let target_symbols = target
                .symbols
                .iter()
                .chain(target.extended_symbols.iter())
                .collect::<std::collections::BTreeSet<_>>();
            for symbol in source_symbols.intersection(&target_symbols) {
                let contract_type = shared_contract_type(symbol);
                entries.push(MilestoneContractEntry {
                    milestone_id: milestone.id.clone(),
                    source_task: source.task_ref.clone(),
                    target_task: target.task_ref.clone(),
                    contract_type,
                    contract_ref: (*symbol).clone(),
                    revision: source.contract_revision.max(target.contract_revision),
                    status: ContractStatus::Synthesized,
                });
            }
        }
    }
    entries
}

fn shared_contract_type(symbol: &str) -> ContractType {
    let kind = symbol
        .split_once(':')
        .map(|(kind, _)| kind)
        .unwrap_or_default();
    if matches!(
        kind.to_ascii_lowercase().as_str(),
        "type" | "struct" | "enum" | "trait" | "class" | "interface"
    ) {
        ContractType::SharedType
    } else {
        ContractType::SharedHelper
    }
}

/// Synthesize stable seam and shared-symbol rows and verify their manifest parity.
pub fn synthesize_milestone_contracts(
    milestone: &Milestone,
) -> std::result::Result<MilestoneContractMatrix, AcddError> {
    validate_wave_dag(milestone)?;
    let mut entries = Vec::new();

    for consumer in &milestone.tasks {
        for dependency in &consumer.depends_on {
            let Some(provider) = milestone
                .tasks
                .iter()
                .find(|task| task.task_ref == *dependency)
            else {
                continue;
            };
            entries.push(MilestoneContractEntry {
                milestone_id: milestone.id.clone(),
                source_task: provider.task_ref.clone(),
                target_task: consumer.task_ref.clone(),
                contract_type: ContractType::SeamDependency,
                contract_ref: format!("{}->{}", provider.task_ref, consumer.task_ref),
                revision: provider.contract_revision.max(consumer.contract_revision),
                status: ContractStatus::Synthesized,
            });
        }
    }
    entries.extend(extract_shared_helpers(milestone));
    entries.sort_by(|left, right| {
        (
            &left.source_task,
            &left.target_task,
            left.contract_type,
            &left.contract_ref,
        )
            .cmp(&(
                &right.source_task,
                &right.target_task,
                right.contract_type,
                &right.contract_ref,
            ))
    });

    let matrix = MilestoneContractMatrix {
        milestone_id: milestone.id.clone(),
        entries,
    };
    super::verify_contract_matrix_parity(milestone, &matrix)?;
    Ok(matrix)
}

/// Validate a manifest and pre-scaffold one worktree for each declared repository.
pub(crate) fn preflight_milestone_init(
    root: &Path,
    manifest_path: &Path,
    in_place: bool,
) -> Result<Value> {
    preflight_milestone_init_inner(root, manifest_path, in_place)
        .map_err(|error| anyhow!("MILESTONE_PREFLIGHT_FAILED: {error:#}"))
}

struct Participant {
    name: String,
    git_root: PathBuf,
    worktree: PathBuf,
    branch: String,
    exists: bool,
}

fn preflight_milestone_init_inner(
    root: &Path,
    manifest_path: &Path,
    in_place: bool,
) -> Result<Value> {
    let root = root
        .canonicalize()
        .context("workspace root is unavailable")?;
    let settings = crate::engine::tasks::TaskSettings::load(&root)?;
    let config_root = crate::engine::tasks::primary_worktree_root(&root)
        .unwrap_or_else(|| root.clone())
        .canonicalize()?;
    let full_manifest = if manifest_path.is_absolute() {
        manifest_path.to_path_buf()
    } else {
        root.join(manifest_path)
    };
    let metadata = fs::symlink_metadata(&full_manifest)
        .with_context(|| format!("manifest '{}' is unavailable", full_manifest.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        bail!("manifest must be a regular file, not a symlink");
    }
    let full_manifest = full_manifest.canonicalize()?;
    if !full_manifest.starts_with(&root) {
        bail!("manifest must be inside the selected workspace");
    }
    let manifest_ref = full_manifest
        .strip_prefix(&root)?
        .to_string_lossy()
        .replace('\\', "/");
    let text = fs::read_to_string(&full_manifest)?;
    let frontmatter = manifest_frontmatter(&text)?;
    let declared_project = frontmatter
        .get("project")
        .and_then(serde_yaml::Value::as_str);
    let default_project = settings
        .task_project
        .as_deref()
        .unwrap_or(&settings.task_repository);
    let project =
        settings.resolve_project(Path::new(&manifest_ref), declared_project, default_project)?;
    let milestone = Milestone::parse_with_identity(&text, &settings.task_repository, &project)?;

    validate_required_skills(&milestone.required_skills, &root, &config_root)?;
    validate_sqlite_revision_ranges(&milestone)?;
    let matrix = synthesize_milestone_contracts(&milestone)?;
    let adapter = load_adapter_file(&config_root)?;
    let workspace_profile = crate::core::tasks::profile::load_for_workspace(&config_root)?;
    let profile = match milestone.acdd_profile.as_deref() {
        Some(pin) => crate::core::tasks::profile::load_pinned_over(
            &config_root,
            pin,
            workspace_profile.as_ref(),
        )?,
        None => workspace_profile,
    };
    validate_profile_module_requirements(profile.as_ref(), &adapter.modules)?;
    for task in &milestone.tasks {
        if let Some(pin) = task.acdd_profile.as_deref() {
            let task_profile =
                crate::core::tasks::profile::load_pinned_over(&config_root, pin, profile.as_ref())?;
            validate_profile_module_requirements(task_profile.as_ref(), &adapter.modules)
                .with_context(|| format!("task '{}' profile preflight failed", task.task_ref))?;
        }
    }
    if !adapter.modules.is_enabled("blackboard") {
        bail!("FEATURE_DISABLED: milestone contract publication requires the blackboard subsystem");
    }
    let participants = resolve_participants(&milestone, &settings, &adapter, &config_root)?;
    let participants = preflight_participants(participants, &milestone.id, in_place)?;

    let mut counts = BTreeMap::new();
    for entry in &matrix.entries {
        let key = match entry.contract_type {
            ContractType::SharedHelper => "shared_helper",
            ContractType::SeamDependency => "seam_dependency",
            ContractType::SharedType => "shared_type",
        };
        *counts.entry(key).or_insert(0usize) += 1;
    }
    let summary = json!({
        "event": "milestone_contracts_synthesized",
        "milestone_id": milestone.id,
        "contract_count": matrix.entries.len(),
        "contract_types": counts,
    })
    .to_string();

    let mut created: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    if !in_place {
        for participant in participants
            .iter()
            .filter(|participant| !participant.exists)
        {
            if let Err(error) = create_worktree(participant) {
                rollback_worktrees(&created);
                return Err(error).context("failed to scaffold repository worktrees");
            }
            created.push((
                participant.git_root.clone(),
                participant.worktree.clone(),
                participant.branch.clone(),
            ));
        }
    }

    let store_result = (|| -> Result<()> {
        let database = crate::engine::tasks::database_path(&root)?;
        let mut store = crate::db::tasks_store::TasksStore::open_project(
            &database,
            &milestone.repository,
            &milestone.project,
        )?;
        store.set_profile(profile);
        store.set_workspace_root(&root);
        TasksStore::sync_with_contract_matrix_and_initial_message(
            &mut store,
            &milestone,
            &manifest_ref,
            &root,
            &matrix,
            crate::db::tasks_store::BlackboardPostInput {
                milestone_ref: &manifest_ref,
                task_id: None,
                subtask_ref: None,
                gate: None,
                author: "milestone-init",
                topic: "draft",
                payload: &summary,
            },
        )?;
        Ok(())
    })();
    if let Err(error) = store_result {
        rollback_worktrees(&created);
        return Err(error).context("failed to synchronize milestone task and contract records");
    }

    Ok(json!({
        "milestone_id": milestone.id,
        "manifest": manifest_ref,
        "in_place": in_place,
        "repositories": participants.iter().map(|participant| json!({
            "name": participant.name,
            "root": participant.git_root,
            "worktree": if in_place { Value::Null } else { json!(participant.worktree) },
            "created": created.iter().any(|(_, path, _)| path == &participant.worktree),
        })).collect::<Vec<_>>(),
        "contract_count": matrix.entries.len(),
    }))
}

fn manifest_frontmatter(text: &str) -> Result<serde_yaml::Value> {
    let normalized = text.replace("\r\n", "\n");
    let body = normalized
        .strip_prefix("---\n")
        .context("manifest requires YAML frontmatter")?;
    let (header, _) = body
        .split_once("\n---\n")
        .context("manifest frontmatter is not terminated")?;
    Ok(serde_yaml::from_str(header)?)
}

fn load_adapter_file(root: &Path) -> Result<crate::core::config::AdapterFile> {
    let primary = root.join("forge-mcp.yaml");
    let text = fs::read_to_string(primary).context("forge-mcp.yaml is required for preflight")?;
    serde_yaml::from_str(&text).context("forge-mcp.yaml is invalid")
}

fn validate_sqlite_revision_ranges(milestone: &Milestone) -> Result<()> {
    for task in &milestone.tasks {
        i64::try_from(task.contract_revision).with_context(|| {
            format!(
                "task '{}' contract revision exceeds SQLite integer range",
                task.task_ref
            )
        })?;
    }
    Ok(())
}

fn validate_profile_module_requirements(
    profile: &crate::core::tasks::profile::GateProfile,
    modules: &crate::core::tasks::catalog::SubsystemRegistry,
) -> Result<()> {
    for required in &profile.modules.required {
        if !modules.is_enabled(required) {
            bail!(
                "FEATURE_REQUIRED_BY_PROFILE: selected ACDD profile requires disabled Forge subsystem '{required}'"
            );
        }
    }

    for gate in &profile.gates {
        validate_gate_module_requirements(gate, modules)?;
    }
    for template in profile.gate_templates.values() {
        for gate in &template.gates {
            validate_gate_module_requirements(gate, modules)?;
        }
    }
    Ok(())
}

fn validate_gate_module_requirements(
    gate: &crate::core::tasks::profile::GateDef,
    modules: &crate::core::tasks::catalog::SubsystemRegistry,
) -> Result<()> {
    for tool in &gate.tools {
        if let Some(subsystem) =
            crate::core::tasks::catalog::SubsystemRegistry::subsystem_for_tool(tool)
        {
            if !modules.is_enabled(subsystem) {
                bail!(
                    "FEATURE_REQUIRED_BY_PROFILE: gate '{}' requires tool '{}' from disabled Forge subsystem '{subsystem}'",
                    gate.id,
                    tool
                );
            }
        }
    }
    for child in &gate.gates {
        validate_gate_module_requirements(child, modules)?;
    }
    if let Some(tasks) = &gate.tasks {
        for child in &tasks.gates {
            validate_gate_module_requirements(child, modules)?;
        }
    }
    Ok(())
}

fn validate_required_skills(skills: &[String], root: &Path, config_root: &Path) -> Result<()> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let codex_home = std::env::var_os("CODEX_HOME").map(PathBuf::from);
    for skill in skills {
        if skill.is_empty()
            || !skill
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            bail!("required_skills contains an invalid skill name '{skill}'");
        }
        let mut candidates = vec![
            root.join(".agents/skills").join(skill).join("SKILL.md"),
            root.join(".codex/skills").join(skill).join("SKILL.md"),
            root.join("skills").join(skill).join("SKILL.md"),
            config_root
                .join(".agents/skills")
                .join(skill)
                .join("SKILL.md"),
            config_root
                .join(".codex/skills")
                .join(skill)
                .join("SKILL.md"),
            config_root.join("skills").join(skill).join("SKILL.md"),
        ];
        if let Some(home) = &home {
            candidates.push(home.join(".agents/skills").join(skill).join("SKILL.md"));
            candidates.push(home.join(".codex/skills").join(skill).join("SKILL.md"));
        }
        if let Some(codex_home) = &codex_home {
            candidates.push(codex_home.join("skills").join(skill).join("SKILL.md"));
        }
        if !candidates.iter().any(|path| path.is_file()) {
            bail!("required skill '{skill}' is not installed");
        }
    }
    Ok(())
}

fn resolve_participants(
    milestone: &Milestone,
    settings: &crate::engine::tasks::TaskSettings,
    adapter: &crate::core::config::AdapterFile,
    config_root: &Path,
) -> Result<Vec<(String, PathBuf)>> {
    let linked = adapter
        .linked_workspaces
        .clone()
        .or_else(|| adapter.workspaces.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|workspace| workspace.enabled.unwrap_or(true))
        .map(|workspace| (workspace.name.clone(), workspace.path))
        .collect::<BTreeMap<_, _>>();
    let primary = crate::engine::tasks::primary_worktree_root(config_root)
        .unwrap_or_else(|| config_root.to_path_buf())
        .canonicalize()?;
    let declarations = if milestone.repositories.is_empty() {
        vec![RepositorySpec::Name(milestone.repository.clone())]
    } else {
        milestone.repositories.clone()
    };
    let mut participants = Vec::new();
    for declaration in declarations {
        let (name, explicit_path) = match declaration {
            RepositorySpec::Name(name) => (name, None),
            RepositorySpec::Detailed(definition) => (definition.name, definition.path),
        };
        let path = if let Some(path) = explicit_path {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                config_root.join(path)
            }
        } else if name == milestone.repository || name == settings.task_repository {
            primary.clone()
        } else if let Some(path) = linked.get(&name) {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                config_root.join(path)
            }
        } else {
            bail!("repository participant '{name}' has no configured or declared path");
        };
        let canonical = path.canonicalize().with_context(|| {
            format!(
                "repository participant '{name}' is unavailable at {}",
                path.display()
            )
        })?;
        if !canonical.is_dir() {
            bail!("repository participant '{name}' is not a directory");
        }
        participants.push((name, canonical));
    }
    if participants.is_empty() {
        bail!("manifest must declare at least one repository participant");
    }
    Ok(participants)
}

fn preflight_participants(
    participants: Vec<(String, PathBuf)>,
    milestone_id: &str,
    in_place: bool,
) -> Result<Vec<Participant>> {
    let branch = format!("milestone/{milestone_id}");
    let mut result = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (name, path) in participants {
        let git_root = PathBuf::from(
            git_stdout(
                &path,
                &["rev-parse", "--show-toplevel"],
                "MILESTONE_PARTICIPANT_GIT_FAILED",
            )
            .with_context(|| format!("participant '{name}' is not a Git repository"))?,
        )
        .canonicalize()
        .context("Git participant root is unavailable")?;
        if !seen.insert(git_root.clone()) {
            continue;
        }
        let worktree = git_root.join(".worktrees").join(milestone_id);
        let mut participant = Participant {
            name,
            git_root: git_root.clone(),
            worktree: worktree.clone(),
            branch: branch.clone(),
            exists: false,
        };
        if in_place {
            result.push(participant);
            continue;
        }

        let format = git_output(&git_root, &["check-ref-format", "--branch", &branch])?;
        if format.trim() != branch {
            bail!("branch name '{branch}' is invalid");
        }
        let worktrees = git_output(&git_root, &["worktree", "list", "--porcelain"])?;
        let existing_branch = worktree_branch(&worktrees, &worktree);
        if worktree.exists() {
            if existing_branch.as_deref() != Some(branch.as_str()) {
                bail!(
                    "worktree path '{}' exists without branch '{branch}'",
                    worktree.display()
                );
            }
            participant.exists = true;
        } else if git_output_status(
            &git_root,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )? {
            bail!("branch '{branch}' already exists outside the milestone worktree");
        }
        result.push(participant);
    }
    Ok(result)
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    git_stdout(root, args, "MILESTONE_GIT_COMMAND_FAILED")
        .with_context(|| format!("unable to run git {}", args.join(" ")))
}

fn git_output_status(root: &Path, args: &[&str]) -> Result<bool> {
    git_status(root, args)
        .with_context(|| format!("unable to run git {}", args.join(" ")))
        .map(|status| status.success())
}

fn worktree_branch(list: &str, expected_path: &Path) -> Option<String> {
    let expected = normalize_existing_or_target(expected_path);
    let mut current_path = None;
    let mut branch = None;
    for line in list.lines().chain(std::iter::once("")) {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(normalize_existing_or_target(Path::new(path)));
            branch = None;
        } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
            branch = Some(value.to_owned());
        } else if line.is_empty() {
            if current_path.as_deref() == Some(expected.as_path()) {
                return branch;
            }
            current_path = None;
            branch = None;
        }
    }
    None
}

fn normalize_existing_or_target(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let Some(parent) = path.parent() else {
        return path.to_path_buf();
    };
    parent
        .canonicalize()
        .map(|parent| parent.join(path.file_name().unwrap_or_default()))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn create_worktree(participant: &Participant) -> Result<()> {
    if let Some(parent) = participant.worktree.parent() {
        fs::create_dir_all(parent)?;
    }
    let path = participant.worktree.to_string_lossy();
    git_output(
        &participant.git_root,
        &["worktree", "add", "-b", &participant.branch, &path, "HEAD"],
    )?;
    Ok(())
}

fn rollback_worktrees(created: &[(PathBuf, PathBuf, String)]) {
    for (root, path, branch) in created.iter().rev() {
        let worktree_path = path.to_string_lossy();
        let _ = git_status(root, &["worktree", "remove", "--force", &worktree_path]);
        let _ = git_status(root, &["branch", "-D", branch]);
    }
}
