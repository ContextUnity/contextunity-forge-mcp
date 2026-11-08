pub(crate) use super::gate_graph::*;
use crate::tasks::profile::GateDriver;
use anyhow::{bail, Context, Result};

/// Resolve the registered agent role for the selected gate, if it is agent-driven.
pub(super) fn resolve_subagent_role(cursor: &GateCursor) -> Option<String> {
    match cursor.gate.driver() {
        Some(GateDriver::Agent) => cursor.gate.role.clone(),
        Some(GateDriver::Command | GateDriver::Container) | None => None,
    }
}

/// Build the stable breadcrumb key for a task gate claim.
pub(crate) fn breadcrumb(task_id: &str, cursor: &GateCursor) -> Result<(String, String)> {
    let (milestone, task_ref) = task_id
        .split_once(':')
        .context("TASK_BREADCRUMB_INVALID: task id must contain a milestone and task reference")?;
    let milestone_id = milestone
        .rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .context("TASK_BREADCRUMB_INVALID: milestone id is missing")?;
    if task_ref.trim().is_empty() {
        bail!("TASK_BREADCRUMB_INVALID: task reference is missing");
    }
    let leaf_stage = cursor
        .canonical_stage_id
        .rsplit_once('.')
        .map(|(_, leaf)| leaf)
        .unwrap_or(&cursor.stage_id);
    let breadcrumb = format!(
        "{milestone_id}/{}/{task_ref}/{leaf_stage}",
        cursor.gate_path
    );
    Ok((milestone_id.to_owned(), breadcrumb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::profile::GateProfile;

    #[test]
    fn nested_containers_preserve_conditions_and_failure_defaults() {
        let children = "      - id: build\n        when: code\n      - id: review\n        max_attempts: 1\n        fail: own\n        loop: {scope: own}\n";
        for (templates, body) in [
            (String::new(), format!("gates:\n{children}")),
            (
                format!("gate_templates:\n  cycle:\n    gates:\n{children}"),
                "use: cycle\n".to_owned(),
            ),
        ] {
            let yaml = format!("{templates}gates:\n  - id: cycle\n    when: seam\n    fail: retry\n    loop: {{scope: retry, symbols: contract}}\n    max_attempts: 3\n    {body}  - id: retry\n  - id: contract\n  - id: own\n");
            let profile: GateProfile = serde_yaml::from_str(&yaml).unwrap();
            let cursors = collect_cursors(&profile).unwrap();
            let build = &cursors[0];
            assert_eq!(build.when.as_deref(), Some("seam & code"));
            assert_eq!(build.gate.on_fail.as_deref(), Some("retry"));
            assert_eq!(
                build
                    .gate
                    .reject_routing
                    .as_ref()
                    .unwrap()
                    .symbols
                    .as_deref(),
                Some("contract")
            );
            assert_eq!(build.gate.max_attempts, Some(3));
            let review = &cursors[1].gate;
            assert_eq!(review.on_fail.as_deref(), Some("own"));
            assert_eq!(
                review.reject_routing.as_ref().unwrap().scope.as_deref(),
                Some("own")
            );
            assert_eq!(review.max_attempts, Some(1));
        }
    }

    #[test]
    fn container_pass_is_inherited_by_terminal_template_leaf() {
        let profile: GateProfile = serde_yaml::from_str("gate_templates:\n  cycle:\n    gates:\n      - id: build\n      - id: review\ngates:\n  - id: cycle\n    use: cycle\n    pass: done\n  - id: ignored\n  - id: done\n").unwrap();
        let cursors = collect_cursors(&profile).unwrap();
        assert_eq!(cursors[1].canonical_stage_id, "cycle.review");
        assert_eq!(cursors[1].gate.on_success.as_deref(), Some("done"));
    }
}
