use crate::acdd::{support::*, tasks::harness::TaskHarness};
use forge_tasks::{
    engine::orchestrator::{task_runner::TaskRunOptions, waves::MilestoneRunOptions},
    tasks::profile::GateDef,
};
use std::{fs, path::Path, time::Duration};

pub(super) const MANIFEST: &str = "docs/milestones/011-wave.md";

pub(super) fn selected_settings(
    root: &Path,
    config: &str,
    profile: &str,
    database: &str,
) -> forge_tasks::tasks::configuration::InvocationSettings {
    forge_tasks::tasks::configuration::InvocationSettings::new(
        root.to_owned(),
        Some(root.join(config)),
        std::sync::Arc::new(
            forge_tasks::tasks::profile::validate_profile_document(
                &fs::read_to_string(root.join(profile)).unwrap(),
            )
            .unwrap(),
        ),
        Some(root.join(profile)),
        root.join(database),
    )
}

pub(super) struct WaveHarness {
    pub task: TaskHarness,
    pub options: MilestoneRunOptions,
}

impl WaveHarness {
    pub fn new(in_place: bool) -> Self {
        let mut profile = runner_profile(DEFAULT_SEAM, "");
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        for role in profile.roles.values_mut() {
            role.guidance = Some("Exercise the real task gate protocol.".into());
            role.runner = Some(format!(
                "python3 {} {} {} {{session_id}} '{{prompt}}' {}",
                quote(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/acdd/fixtures/workflows/wave-task-role.py"
                )),
                quote(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/acdd/orchestrator/lifecycle/role_fixture.py"
                )),
                quote(env!("CARGO_BIN_EXE_contextunity-forge-mcp")),
                quote(&passing_review_proof().to_string())
            ));
        }
        profile.roles.insert("wave_role".into(), serde_json::from_value(json!({"runner":format!("python3 {} '{{prompt}}'", quote(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/acdd/fixtures/workflows/wave-role.py")))})).unwrap());
        let mut tasks = profile
            .gates
            .iter()
            .find(|g| g.tasks.is_some())
            .unwrap()
            .clone();
        for gate in &mut tasks.tasks.as_mut().unwrap().gates {
            gate.guidance.clear();
        }
        let polish: GateDef = serde_json::from_value(
            json!({"id":"polish","proof":"none","role":"wave_role","max_attempts":2}),
        )
        .unwrap();
        let review: GateDef = serde_json::from_value(json!({"id":"milestone_review","role":"wave_role","independent_from":"polish","proof":{"scheme":{"type":"object","required":["decision","summary"],"properties":{"decision":{"type":"string"},"summary":{"type":"string"}}}},"fail":"polish","max_attempts":2})).unwrap();
        let verify: GateDef = serde_json::from_value(
            json!({"id":"verify","command":"check_full_repo","proof":"command"}),
        )
        .unwrap();
        let verify_again: GateDef = serde_json::from_value(
            json!({"id":"verify_again","command":"rust-check-and-test","proof":"command"}),
        )
        .unwrap();
        let handoff: GateDef = serde_json::from_value(
            json!({"id":"handoff","proof":"delivery","receipt":true,"save":"snapshot","auto_commit":false}),
        )
        .unwrap();
        profile.gates = vec![tasks, polish, review, verify, verify_again, handoff];
        profile.validate().unwrap();
        let source = MilestoneFixtureBuilder::new("m-wave")
            .task("first", &["src/lib.rs"])
            .task_with_deps("second", &["src/second.rs"], &["first"])
            .build();
        let task = TaskHarness::disk_with_profile(&source, MANIFEST, profile);
        fs::create_dir_all(task.root().join(".forge/lifecycle")).unwrap();
        fs::write(
            task.root().join(".forge/lifecycle/review-proof.json"),
            serde_json::to_vec(&passing_review_proof()).unwrap(),
        )
        .unwrap();
        fs::write(task.root().join("src/second.rs"), "pub fn second() {}\n").unwrap();
        // Inputs needed by isolated child worktrees must be in their shared base.
        for args in [
            vec!["add", "src/second.rs"],
            vec!["commit", "-m", "wave fixture inputs"],
        ] {
            assert!(git_command(task.root(), &args).status().unwrap().success());
        }
        let exclude = task.root().join(".git/info/exclude");
        let mut ignored = fs::read_to_string(&exclude).unwrap_or_default();
        ignored.push_str("\n.worktrees/\n");
        fs::write(exclude, ignored).unwrap();
        let options = MilestoneRunOptions {
            task: TaskRunOptions {
                worktree: task.root().to_owned(),
                worker_id: "wave-coordinator".into(),
                session_fingerprint: "fixture-wave-coordinator".into(),
                runner_timeout: Duration::from_secs(20),
            },
            in_place,
            target_base: git_head(task.root()),
        };
        Self { task, options }
    }

    pub fn root(&self) -> &Path {
        self.task.root()
    }

    pub fn pre_task_checks(&mut self, stop: bool) {
        let path = self.root().join(".forge/acdd/profile.yaml");
        let mut profile = forge_tasks::tasks::profile::validate_profile_document(
            &fs::read_to_string(&path).unwrap(),
        )
        .unwrap();
        profile.commands.insert(
            "preflight".into(),
            forge_tasks::tasks::profile::CommandDef::Script(
                "printf seen >> .forge/preflight.log".into(),
            ),
        );
        profile.gates.insert(
            0,
            serde_json::from_value(json!({"id":"preflight","command":"preflight","proof":"none"}))
                .unwrap(),
        );
        if stop {
            profile.commands.insert(
                "stop-before-tasks".into(),
                forge_tasks::tasks::profile::CommandDef::Script("exit 31".into()),
            );
            profile.gates.insert(
                1,
                serde_json::from_value(
                    json!({"id":"stop_before_tasks","command":"stop-before-tasks","proof":"none"}),
                )
                .unwrap(),
            );
        }
        profile.validate().unwrap();
        fs::write(path, serde_yaml::to_string(&profile).unwrap()).unwrap();
        for args in [
            vec!["add", "-f", ".forge/acdd/profile.yaml"],
            vec!["commit", "-m", "pre-task observation fixture"],
        ] {
            assert!(git_command(self.root(), &args).status().unwrap().success());
        }
        self.options.target_base = git_head(self.root());
    }

    pub fn partial_pre_task_checks(&mut self, retry: bool) {
        self.pre_task_checks(true);
        let path = self.root().join(".forge/acdd/profile.yaml");
        let mut profile = forge_tasks::tasks::profile::validate_profile_document(
            &fs::read_to_string(&path).unwrap(),
        )
        .unwrap();
        let mut command = "printf seen >> .forge/second-preflight.log; if [ ! -e .forge/preflight-mutation-done ]; then printf '\\n// initial pre-task output\\n' >> src/lib.rs; touch .forge/preflight-mutation-done; fi".to_owned();
        if retry {
            command.push_str("; if [ ! -e .forge/preflight-retry-done ]; then touch .forge/preflight-retry-done; exit 17; fi");
            let stop = profile
                .gates
                .iter_mut()
                .find(|gate| gate.id == "stop_before_tasks")
                .unwrap();
            stop.on_fail = Some("observe_once".into());
            stop.max_attempts = Some(2);
        }
        profile.commands.insert(
            "observe-once".into(),
            forge_tasks::tasks::profile::CommandDef::Script(command),
        );
        profile.gates.insert(1, serde_json::from_value(json!({"id":"observe_once","command":"observe-once","proof":"none","max_attempts":2,"fail":"observe_once"})).unwrap());
        profile.validate().unwrap();
        fs::write(path, serde_yaml::to_string(&profile).unwrap()).unwrap();
        for args in [
            vec!["add", "-f", ".forge/acdd/profile.yaml"],
            vec!["commit", "-m", "partial-prefix observation fixture"],
        ] {
            assert!(git_command(self.root(), &args).status().unwrap().success());
        }
        self.options.target_base = git_head(self.root());
    }
}
