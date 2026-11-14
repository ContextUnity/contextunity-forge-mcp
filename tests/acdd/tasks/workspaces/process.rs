use crate::acdd::tasks::harness::TaskHarness;
use contextunity_forge_mcp::core::tasks::gates::Evidence;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

impl TaskHarness {
    pub fn rejecting_pre_commit_hook(root: &Path) -> anyhow::Result<PathBuf> {
        let hook = root.join(".git/forced-hooks/pre-commit");
        fs::create_dir_all(hook.parent().unwrap())?;
        Self::reject_pre_commit_hook(&hook)?;
        Ok(hook)
    }

    pub fn reject_pre_commit_hook(hook: &Path) -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        fs::write(hook, "#!/bin/sh\nexit 47\n")?;
        fs::set_permissions(hook, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    pub fn recover_with_rewind_fault(
        root: &Path,
        checkpoint_ref: &str,
    ) -> anyhow::Result<(Output, Output)> {
        let real_git = TaskHarness::revision(root, "HEAD");
        let ref_lock = root.join(".git").join(format!("{checkpoint_ref}.lock"));
        fs::create_dir_all(ref_lock.parent().unwrap())?;
        // Hold the real ref lock so Git's update-ref fails through recovery's production path.
        let lock_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ref_lock)?;
        drop(lock_file);
        let recovery = crate::acdd::mcp::in_process_cli::run_cli_args(&[
            "--root".into(),
            root.as_os_str().to_owned(),
            "milestone".into(),
            "db-recover".into(),
            "--format".into(),
            "json".into(),
        ]);
        fs::remove_file(ref_lock)?;
        Ok((real_git, recovery))
    }
}

pub(crate) struct GcPublicationFixture {
    root: PathBuf,
    shim: PathBuf,
}

pub(crate) struct GcMatrixFixture {
    workspace: crate::common::Workspace,
    baseline: crate::acdd::snapshot::FixtureSnapshot,
    pub seed_outputs: Vec<Output>,
}

impl GcMatrixFixture {
    pub fn begin_case(&self, pending: bool) -> anyhow::Result<(TaskHarness, GcPublicationFixture)> {
        self.baseline.restore(&self.workspace);
        let mut task = TaskHarness::attach(&self.workspace, "forge-mcp/m-gc:candidate");
        task.advance_to("build")?;
        task.claim_gate("build", "builder");
        self.workspace.write(
            "src/lib.rs",
            &format!("pub fn unique_provisional_candidate() -> bool {{ {pending} }}\n"),
        );
        let fault = TaskHarness::gc_publication_fault(&self.workspace, pending)?;
        Ok((task, fault))
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct GcWindow {
    pub candidate: String,
    pub exists_after_gc: bool,
    pub gc_attempted: bool,
}

#[derive(serde::Deserialize)]
pub(crate) struct GcConstruction {
    pub gc_attempted: bool,
    pub exit: i32,
}

impl TaskHarness {
    pub fn gc_publication_fault(
        workspace: &crate::common::Workspace,
        pending: bool,
    ) -> anyhow::Result<GcPublicationFixture> {
        use std::os::unix::fs::PermissionsExt;
        workspace.write(
            ".git/info/exclude",
            ".forge/\nfixture-gc-window.json\nfixture-gc-construction.json\n",
        );
        let shim = workspace.root().join("fixture-bin/git");
        if pending {
            workspace.write(
                "fixture-publication-fault",
                "isolate post-SQL keeper failure\n",
            );
        }
        workspace.write("fixture-bin/git", r#"#!/usr/bin/env python3
import subprocess,sys,pathlib,json
r=subprocess.run(['/usr/bin/git',*sys.argv[1:]],capture_output=True)
if r.returncode==0 and 'commit-tree' in sys.argv and pathlib.Path('fixture-publication-fault').exists() and '-m' not in sys.argv:
 state=subprocess.run(['/usr/bin/git','show',r.stdout.decode().strip()+':checkpoint.json'],capture_output=True)
 if state.returncode==0:
  state=json.loads(state.stdout)
  if state.get('intent') is None and any(row[0]=='build' and row[2]=='passed' for row in state.get('gates',[])):
   lock=pathlib.Path('.git/refs/forge/state/forge-mcp/forge-mcp/m-gc/candidate.lock')
   lock.parent.mkdir(parents=True,exist_ok=True);lock.write_text('workspace publication lock')
if r.returncode==0 and len(sys.argv)>1 and sys.argv[1]=='-c' and 'hash-object' in sys.argv and '-t' in sys.argv and 'commit' in sys.argv:
 gc=subprocess.run(['/usr/bin/git','gc','--prune=now'],capture_output=True)
 pathlib.Path('fixture-gc-construction.json').write_text(json.dumps({'gc_attempted':True,'exit':gc.returncode}))
if r.returncode==0 and len(sys.argv)>1 and sys.argv[1]=='commit-tree' and '-m' in sys.argv and any(a.startswith('forge-snapshot:') for a in sys.argv):
 sha=r.stdout.decode().strip()
 subprocess.run(['/usr/bin/git','reflog','expire','--expire=now','--all'],check=True,capture_output=True)
 subprocess.run(['/usr/bin/git','gc','--prune=now'],check=True,capture_output=True)
 check=subprocess.run(['/usr/bin/git','cat-file','-e',sha],capture_output=True)
 pathlib.Path('fixture-gc-window.json').write_text(json.dumps({'candidate':sha,'exists_after_gc':check.returncode==0,'gc_attempted':True}))
sys.stdout.buffer.write(r.stdout);sys.stderr.buffer.write(r.stderr);sys.exit(r.returncode)
"#);
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();

        Ok(GcPublicationFixture {
            root: workspace.root().into(),
            shim,
        })
    }
}

impl GcPublicationFixture {
    pub fn temporary_pack_present(&self) -> anyhow::Result<bool> {
        for entry in fs::read_dir(self.root.join(".git/objects/pack"))? {
            if entry?.path().extension().is_some_and(|ext| ext == "keep") {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn release_publication_lock(&self) -> anyhow::Result<()> {
        fs::remove_file(
            self.root
                .join(".git/refs/forge/state/forge-mcp/forge-mcp/m-gc/candidate.lock"),
        )?;
        Ok(())
    }

    pub fn recover(&self) -> Output {
        crate::acdd::mcp::in_process_cli::run_cli_args(
            &[
                "--root",
                self.root.to_str().unwrap(),
                "--format",
                "json",
                "milestone",
                "db-recover",
            ]
            .map(Into::into),
        )
    }

    pub fn submit_build(&self, id: &str, evidence: &Evidence) -> anyhow::Result<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .args([
                "--root",
                self.root.to_str().unwrap(),
                "--format",
                "json",
                "task",
                "submit",
                id,
                "--stage",
                "build",
                "--action",
                "pass",
                "--evidence",
                &serde_json::to_string(evidence)?,
            ])
            .env(
                "FORGE_AGENT_SESSION_ID",
                TaskHarness::session_for(&evidence.worker_id),
            )
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.shim.parent().unwrap().display(),
                    std::env::var("PATH")?
                ),
            )
            .current_dir(&self.root)
            .output()?)
    }

    pub fn observations(&self) -> anyhow::Result<(GcWindow, GcConstruction)> {
        Ok((
            serde_json::from_slice(&fs::read(self.root.join("fixture-gc-window.json"))?)?,
            serde_json::from_slice(&fs::read(self.root.join("fixture-gc-construction.json"))?)?,
        ))
    }
}

impl TaskHarness {
    pub fn gc_matrix() -> GcMatrixFixture {
        let workspace = Self::gc_workspace();
        let seed_outputs = Self::protect_gc_seed(workspace.root());
        let baseline = crate::acdd::snapshot::FixtureSnapshot::capture(&workspace);
        GcMatrixFixture {
            workspace,
            baseline,
            seed_outputs,
        }
    }

    pub fn gc_workspace() -> crate::common::Workspace {
        let isolated = crate::acdd::support::acdd_workspace(crate::acdd::support::DEFAULT_SEAM, "");
        isolated.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        isolated.write("src/lib.rs", "pub fn before() {}\n");
        isolated.write("docs/milestones/010-gc.md", "---\nid: m-gc\nstatus: active\n---\n```yaml\ntask_ref: candidate\ntarget: Preserve provisional candidate\nproof_policy: direct-proof\nscope: [src/]\n```\n");
        contextunity_forge_mcp::engine::tasks::manage(
            isolated.root(),
            serde_json::from_value(
                serde_json::json!({"action":"sync","milestone_ref":"docs/milestones/010-gc.md"}),
            )
            .unwrap(),
        )
        .unwrap();
        isolated
    }
}
