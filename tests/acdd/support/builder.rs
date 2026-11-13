use super::*;

/// Canonical milestone documents used by ACDD tests.
#[derive(Clone, Copy)]
pub(crate) enum MilestoneFixtureTemplate {
    Linear,
    BranchingDeps,
    Cancellation,
}

/// Build a milestone document while letting the production parser validate it.
pub(crate) struct MilestoneFixtureBuilder {
    pub(crate) frontmatter: MilestoneFixtureFrontmatter,
    pub(crate) outcome: Option<String>,
    pub(crate) tasks: Vec<MilestoneFixtureTask>,
}

#[derive(serde::Serialize)]
pub(crate) struct MilestoneFixtureFrontmatter {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) doc_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) invariants: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) closure: Option<MilestoneFixtureClosure>,
}

#[derive(serde::Serialize)]
pub(crate) struct MilestoneFixtureClosure {
    pub(crate) reason: String,
}

#[derive(serde::Serialize)]
pub(crate) struct MilestoneFixtureConcurrency {
    pub(crate) mode: String,
}

#[derive(serde::Serialize)]
pub(crate) struct MilestoneFixtureTask {
    pub(crate) task_ref: String,
    pub(crate) target: String,
    pub(crate) proof_policy: String,
    pub(crate) scope: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) contract_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) concurrency: Option<MilestoneFixtureConcurrency>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) scope_roots: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) invariants: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) receipt: Option<Receipt>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) subtasks: Vec<contextunity_forge_mcp::core::tasks::SubtaskSpec>,
    #[serde(skip)]
    pub(crate) narrative: Option<String>,
}

impl MilestoneFixtureBuilder {
    pub(crate) fn new(id: &str) -> Self {
        Self {
            frontmatter: MilestoneFixtureFrontmatter {
                id: id.into(),
                title: id.into(),
                doc_type: Some("contract"),
                status: Some("active".into()),
                started_at: Some("2026-10-01T10:00:00Z".into()),
                repository: None,
                project: None,
                invariants: Vec::new(),
                depends_on: Vec::new(),
                closure: None,
            },
            outcome: None,
            tasks: Vec::new(),
        }
    }

    pub(crate) fn load_template(template: MilestoneFixtureTemplate) -> &'static str {
        match template {
            MilestoneFixtureTemplate::Linear => include_str!("../fixtures/milestones/linear.md"),
            MilestoneFixtureTemplate::BranchingDeps => {
                include_str!("../fixtures/milestones/branching_deps.md")
            }
            MilestoneFixtureTemplate::Cancellation => {
                include_str!("../fixtures/milestones/cancellation.md")
            }
        }
    }

    pub(crate) fn title(mut self, title: &str) -> Self {
        self.frontmatter.title = title.into();
        self
    }

    pub(crate) fn status(mut self, status: &str) -> Self {
        self.frontmatter.status = Some(status.into());
        self.frontmatter.started_at = (status != "planned").then(|| "2026-10-01T10:00:00Z".into());
        self
    }

    pub(crate) fn without_status(mut self) -> Self {
        self.frontmatter.status = None;
        self.frontmatter.started_at = None;
        self
    }

    pub(crate) fn without_doc_type(mut self) -> Self {
        self.frontmatter.doc_type = None;
        self
    }

    pub(crate) fn started_at(mut self, timestamp: &str) -> Self {
        self.frontmatter.started_at = Some(timestamp.into());
        self
    }

    pub(crate) fn without_started_at(mut self) -> Self {
        self.frontmatter.started_at = None;
        self
    }

    pub(crate) fn closure_reason(mut self, reason: &str) -> Self {
        self.frontmatter.closure = Some(MilestoneFixtureClosure {
            reason: reason.into(),
        });
        self
    }

    pub(crate) fn repository(mut self, repository: &str) -> Self {
        self.frontmatter.repository = Some(repository.into());
        self
    }

    pub(crate) fn project(mut self, project: &str) -> Self {
        self.frontmatter.project = Some(project.into());
        self
    }

    pub(crate) fn invariants(mut self, invariants: &[&str]) -> Self {
        self.frontmatter.invariants = fixture_strings(invariants);
        self
    }

    pub(crate) fn depends_on(mut self, dependencies: &[&str]) -> Self {
        self.frontmatter.depends_on = fixture_strings(dependencies);
        self
    }

    pub(crate) fn task(self, task_ref: &str, scope: &[&str]) -> Self {
        self.task_with_deps(task_ref, scope, &[])
    }

    pub(crate) fn task_with_deps(
        mut self,
        task_ref: &str,
        scope: &[&str],
        dependencies: &[&str],
    ) -> Self {
        self.tasks.push(MilestoneFixtureTask {
            task_ref: task_ref.into(),
            target: format!("Deliver {task_ref}"),
            proof_policy: "seam-test-first".into(),
            scope: fixture_strings(scope),
            contract_revision: None,
            concurrency: None,
            scope_roots: Vec::new(),
            depends_on: fixture_strings(dependencies),
            invariants: Vec::new(),
            status: None,
            agent_type: None,
            receipt: None,
            subtasks: Vec::new(),
            narrative: None,
        });
        self
    }

    pub(crate) fn task_target(mut self, target: &str) -> Self {
        self.last_task().target = target.into();
        self
    }

    pub(crate) fn task_status(mut self, status: &str) -> Self {
        self.last_task().status = Some(status.into());
        self
    }

    pub(crate) fn task_proof_policy(mut self, proof_policy: &str) -> Self {
        self.last_task().proof_policy = proof_policy.into();
        self
    }

    pub(crate) fn task_contract_revision(mut self, revision: u64) -> Self {
        self.last_task().contract_revision = Some(revision);
        self
    }

    pub(crate) fn task_concurrency_mode(mut self, mode: &str) -> Self {
        self.last_task().concurrency = Some(MilestoneFixtureConcurrency { mode: mode.into() });
        self
    }

    pub(crate) fn task_receipt(mut self, receipt: &Receipt) -> Self {
        self.last_task().receipt = Some(receipt.clone());
        self
    }

    pub(crate) fn task_subtasks(
        mut self,
        subtasks: &[contextunity_forge_mcp::core::tasks::SubtaskSpec],
    ) -> Self {
        self.last_task().subtasks = subtasks.to_vec();
        self
    }

    pub(crate) fn task_scope_roots(mut self, roots: &[&str]) -> Self {
        self.last_task().scope_roots = fixture_strings(roots);
        self
    }

    pub(crate) fn task_invariants(mut self, invariants: &[&str]) -> Self {
        self.last_task().invariants = fixture_strings(invariants);
        self
    }

    pub(crate) fn build(self) -> String {
        let header = serde_yaml::to_string(&self.frontmatter).unwrap();
        let header = header.strip_prefix("---\n").unwrap_or(&header);
        let heading = self.frontmatter.title.replace(['\r', '\n'], " ");
        let mut document = format!("---\n{header}---\n# {heading}\n");
        if let Some(outcome) = self.outcome {
            document.push_str(&format!("\n## Outcome and purpose\n{outcome}\n"));
        }
        for task in self.tasks {
            let yaml = serde_yaml::to_string(&task).unwrap();
            let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml);
            // The production parser treats a trimmed line of three backticks as
            // the fence terminator even inside a YAML block scalar. JSON is a
            // YAML-compatible mapping and escapes embedded newlines safely.
            let escaped;
            let yaml = if yaml.lines().any(|line| line.trim() == "```") {
                escaped = format!("{}\n", serde_json::to_string(&task).unwrap());
                escaped.as_str()
            } else {
                yaml
            };
            document.push_str(&format!(
                "\n### task: {}\n```yaml\n{yaml}```\n",
                task.task_ref
            ));
            if let Some(narrative) = task.narrative {
                document.push_str(&narrative);
                document.push('\n');
            }
        }
        document
    }

    fn last_task(&mut self) -> &mut MilestoneFixtureTask {
        self.tasks
            .last_mut()
            .expect("add a task before setting its fields")
    }
}

pub(crate) fn fixture_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

pub(crate) const SPEC: &str = "---\nid: m-test\ntitle: Tasks\ndoc_type: contract\nstatus: active\ninvariants: [isolated]\n---\n# Tasks\n```yaml\ntask_ref: first\ntarget: Deliver first\nproof_policy: seam-test-first\nscope: [src/]\nscope_roots: [src/, tests/]\n```\n```yaml\ntask_ref: second\ntarget: Deliver second\nproof_policy: seam-test-first\nscope: [src/]\ndepends_on: [first]\n```\n";

#[cfg(test)]
mod milestone_fixture_tests {
    use super::*;

    #[test]
    fn milestone_fixture_builder_round_trips_fence_lines_in_task_fields() {
        let template = Milestone::parse(
            MilestoneFixtureBuilder::load_template(MilestoneFixtureTemplate::Cancellation),
            "forge-mcp",
        )
        .unwrap();
        let mut receipt = template.tasks[1].receipt.clone().unwrap();
        for content in [
            "first\n```\nsecond",
            "first\n  ```  \nsecond",
            "first\r\n```\r\nsecond",
        ] {
            receipt.evidence = json!({"message": content});
            let source = MilestoneFixtureBuilder::new("m-fence")
                .task("first", &["src/shared.rs"])
                .task_target(content)
                .task_invariants(&[content])
                .task_subtasks(&[contextunity_forge_mcp::core::tasks::SubtaskSpec {
                    subtask_ref: "child".into(),
                    title: content.into(),
                    status: "ready".into(),
                    evidence: None,
                }])
                .task_receipt(&receipt)
                .task_with_deps("second", &["src/shared.rs"], &["first"])
                .build();
            let milestone = Milestone::parse(&source, "forge-mcp").unwrap();
            assert_eq!(milestone.tasks.len(), 2);
            assert_eq!(milestone.tasks[0].target, content);
            assert_eq!(milestone.tasks[0].invariants, [content]);
            assert_eq!(milestone.tasks[0].subtasks[0].title, content);
            assert_eq!(
                milestone.tasks[0].receipt.as_ref().unwrap().evidence["message"],
                content
            );
            assert_eq!(milestone.tasks[1].depends_on, ["first"]);
        }
    }

    #[test]
    fn milestone_fixture_builder_round_trips_typed_values_and_rejects_bad_dependencies() {
        let mut builder = MilestoneFixtureBuilder::new("m-builder")
            .title("A title: with \"quotes\"")
            .status("planned")
            .repository("sample-repo")
            .project("sample-project")
            .invariants(&["Keep: values intact"])
            .depends_on(&["m-upstream"]);
        builder.outcome = Some("Visible outcome.".into());
        builder = builder
            .task("first", &["src/shared.rs"])
            .task_target("Deliver: first \"result\"")
            .task_scope_roots(&["src/"])
            .task_invariants(&["reviewed: yes"])
            .task_proof_policy("direct-proof")
            .task_contract_revision(2)
            .task_subtasks(&[contextunity_forge_mcp::core::tasks::SubtaskSpec {
                subtask_ref: "child".into(),
                title: "Preserve task details".into(),
                status: "ready".into(),
                evidence: None,
            }])
            .task_with_deps("second", &["src/second.rs"], &["first"])
            .task_status("cancelled");
        builder.tasks[0].agent_type = Some("builder".into());
        builder.tasks[0].narrative = Some("Notes for the first task.".into());
        let source = builder.build();
        let milestone = Milestone::parse(&source, "fallback").unwrap();
        assert_eq!(milestone.id, "m-builder");
        assert_eq!(milestone.repository, "sample-repo");
        assert_eq!(milestone.project, "sample-project");
        assert_eq!(milestone.status.as_deref(), Some("planned"));
        assert_eq!(milestone.invariants, ["Keep: values intact"]);
        assert_eq!(milestone.depends_on, ["m-upstream"]);
        assert_eq!(milestone.tasks[0].target, "Deliver: first \"result\"");
        assert_eq!(milestone.tasks[0].scope_roots, ["src/"]);
        assert_eq!(milestone.tasks[0].invariants, ["reviewed: yes"]);
        assert_eq!(milestone.tasks[0].agent_type.as_deref(), Some("builder"));
        assert_eq!(milestone.tasks[0].proof_policy, "direct-proof");
        assert_eq!(milestone.tasks[0].contract_revision, 2);
        assert_eq!(milestone.tasks[0].subtasks[0].subtask_ref, "child");
        assert_eq!(milestone.tasks[1].depends_on, ["first"]);
        assert_eq!(milestone.tasks[1].status.as_deref(), Some("cancelled"));
        assert!(source.contains("Visible outcome."));
        assert!(source.contains("Notes for the first task."));
        let header = source
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap()
            .0;
        let header: serde_yaml::Value = serde_yaml::from_str(header).unwrap();
        assert_eq!(header["title"].as_str(), Some("A title: with \"quotes\""));
        assert!(header.get("started_at").is_none());

        let invalid = MilestoneFixtureBuilder::new("m-builder")
            .task_with_deps("first", &["src/first.rs"], &["missing"])
            .build();
        assert!(Milestone::parse(&invalid, "fallback").is_err());

        let receipt_source =
            MilestoneFixtureBuilder::load_template(MilestoneFixtureTemplate::Cancellation);
        let template = Milestone::parse(receipt_source, "fallback").unwrap();
        let receipt = template.tasks[1].receipt.as_ref().unwrap();
        let archived = MilestoneFixtureBuilder::new("m-archive")
            .without_status()
            .without_doc_type()
            .task("delivered", &["src/delivered.rs"])
            .task_status("completed")
            .task_receipt(receipt)
            .build();
        let parsed = Milestone::parse(&archived, "fallback").unwrap();
        assert!(parsed.status.is_none());
        assert_eq!(parsed.tasks[0].receipt.as_ref(), Some(receipt));
        assert!(!archived.contains("doc_type:"));

        let cancelled = MilestoneFixtureBuilder::new("m-cancelled")
            .status("cancelled")
            .started_at("2026-09-01T00:00:00Z")
            .closure_reason("Work transferred")
            .build();
        assert_eq!(
            Milestone::parse(&cancelled, "fallback")
                .unwrap()
                .status
                .as_deref(),
            Some("cancelled")
        );
        assert!(cancelled.contains("2026-09-01T00:00:00Z"));

        let active_without_start = MilestoneFixtureBuilder::new("m-no-start")
            .status("active")
            .without_started_at()
            .build();
        let header: serde_yaml::Value = serde_yaml::from_str(
            active_without_start
                .strip_prefix("---\n")
                .unwrap()
                .split_once("\n---\n")
                .unwrap()
                .0,
        )
        .unwrap();
        assert_eq!(header["status"].as_str(), Some("active"));
        assert!(header.get("started_at").is_none());
    }

    #[test]
    fn milestone_fixture_templates_parse_and_cancellation_imports() {
        let linear = Milestone::parse(
            MilestoneFixtureBuilder::load_template(MilestoneFixtureTemplate::Linear),
            "forge-mcp",
        )
        .unwrap();
        assert_eq!(linear.tasks.len(), 2);
        assert_eq!(linear.tasks[0].task_ref, "first");
        assert_eq!(linear.tasks[1].task_ref, "second");
        assert_eq!(linear.tasks[1].depends_on, ["first"]);

        let branching = Milestone::parse(
            MilestoneFixtureBuilder::load_template(MilestoneFixtureTemplate::BranchingDeps),
            "forge-mcp",
        )
        .unwrap();
        assert_eq!(branching.tasks.len(), 4);
        assert_eq!(branching.tasks[1].depends_on, ["first"]);
        assert_eq!(branching.tasks[2].depends_on, ["first"]);
        assert_eq!(branching.tasks[3].depends_on, ["second", "third"]);

        let cancellation_source =
            MilestoneFixtureBuilder::load_template(MilestoneFixtureTemplate::Cancellation);
        let cancellation = Milestone::parse(cancellation_source, "forge-mcp").unwrap();
        assert_eq!(cancellation.tasks.len(), 3);
        assert_eq!(cancellation.tasks[0].status.as_deref(), Some("planned"));
        assert_eq!(cancellation.tasks[1].status.as_deref(), Some("completed"));
        assert_eq!(cancellation.tasks[2].status.as_deref(), Some("cancelled"));

        let workspace = Workspace::new();
        let reference = "docs/milestones/cancellation.md";
        workspace.write(reference, cancellation_source);
        let mut store = TasksStore::open_in_memory("forge-mcp", "forge-mcp").unwrap();
        let imported = store
            .sync(&cancellation, reference, workspace.root())
            .unwrap();
        assert_eq!(imported[1].status, "completed");
        assert!(imported[1].receipt.is_some());
        assert_eq!(imported[2].status, "cancelled");
    }
}
