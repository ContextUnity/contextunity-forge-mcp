use crate::acdd::tasks::harness::TaskHarness;
use std::{path::Path, process::Output};

impl TaskHarness {
    pub fn revision_text(root: &Path, reference: &str) -> anyhow::Result<String> {
        let output = Self::revision(root, reference);
        anyhow::ensure!(
            output.status.success(),
            "Git revision read failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    pub fn verified_revision_text(root: &Path, reference: &str) -> anyhow::Result<String> {
        let output = Self::git_output(root, &["rev-parse", "--verify", reference]);
        anyhow::ensure!(
            output.status.success(),
            "Git revision verification failed for '{reference}': {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    pub fn commit_source(
        root: &crate::common::Workspace,
        path: &str,
        source: &str,
        message: &str,
    ) -> anyhow::Result<String> {
        root.write(path, source);
        for output in [
            Self::git_output(root.root(), &["add", "--", path]),
            Self::git_output(root.root(), &["commit", "--quiet", "-m", message]),
        ] {
            anyhow::ensure!(
                output.status.success(),
                "Fixture commit failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Self::revision_text(root.root(), "HEAD")
    }

    pub fn exact_tree_fixture(files: &[(&str, &str)]) -> anyhow::Result<crate::common::Workspace> {
        let root = crate::common::Workspace::new();
        crate::acdd::support::init_git(root.root());
        let emptied = Self::git_output(root.root(), &["read-tree", "--empty"]);
        anyhow::ensure!(
            emptied.status.success(),
            "Fixture index reset failed: {emptied:?}"
        );
        for (path, contents) in files {
            root.write(path, contents);
        }
        let paths: Vec<_> = files.iter().map(|(path, _)| *path).collect();
        for output in [
            Self::stage_changes(root.root(), &paths),
            Self::commit_fixture(root.root(), "init"),
        ] {
            anyhow::ensure!(
                output.status.success(),
                "Fixture tree creation failed: {output:?}"
            );
        }
        Ok(root)
    }

    pub fn stage_unrelated_file(
        root: &crate::common::Workspace,
        path: &str,
        contents: &str,
    ) -> Output {
        root.write(path, contents);
        Self::stage_changes(root.root(), &[path])
    }

    pub fn stage_deletion(root: &Path, path: &str) -> Output {
        Self::git_output(root, &["rm", path])
    }

    pub fn commit_fixture_with_test_identity(root: &Path, message: &str) -> Output {
        Self::git_output(
            root,
            &[
                "-c",
                "user.name=ACDD test",
                "-c",
                "user.email=acdd-test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                message,
            ],
        )
    }

    pub fn initial_fixture_commit(root: &Path) -> [Output; 2] {
        [
            Self::git_output(root, &["add", "-A"]),
            Self::git_output(root, &["commit", "-q", "-m", "initial"]),
        ]
    }

    pub fn protect_gc_seed(root: &Path) -> Vec<Output> {
        let refs = Self::git_output(
            root,
            &[
                "for-each-ref",
                "--format=%(objectname)",
                "refs/forge/state/",
            ],
        );
        let mut outputs = vec![];
        for (index, object) in std::str::from_utf8(&refs.stdout)
            .unwrap()
            .lines()
            .enumerate()
        {
            outputs.push(Self::update_reference(
                root,
                &format!("refs/fixture/gc-seed/{index}"),
                object,
                None,
            ));
        }
        outputs.push(refs);
        outputs
    }

    pub fn stage_changes(root: &Path, paths: &[&str]) -> Output {
        Self::git_output(root, &[&["add"][..], paths].concat())
    }

    pub fn stage_tracked_changes(root: &Path, paths: &[&str]) -> Output {
        Self::git_output(root, &[&["add", "-u"][..], paths].concat())
    }

    pub fn commit_fixture(root: &Path, message: &str) -> Output {
        Self::git_output(root, &["commit", "-m", message])
    }

    pub fn revision(root: &Path, reference: &str) -> Output {
        Self::git_output(root, &["rev-parse", reference])
    }

    pub fn snapshot_diff(root: &Path, commit: &str) -> Output {
        Self::git_output(root, &["show", commit])
    }

    pub fn snapshot_content(root: &Path, commit: &str, path: &str) -> Output {
        Self::git_output(root, &["show", &format!("{commit}:{path}")])
    }

    pub fn snapshot_parents(root: &Path, commit: &str) -> Output {
        Self::git_output(root, &["rev-list", "--parents", "-n", "1", commit])
    }

    pub fn snapshot_paths(root: &Path, commit: &str) -> Output {
        Self::git_output(root, &["ls-tree", "-r", "--name-only", commit])
    }

    pub fn references(root: &Path, prefix: &str) -> Output {
        Self::git_output(root, &["for-each-ref", "--format=%(refname)", prefix])
    }

    pub fn update_reference(
        root: &Path,
        reference: &str,
        commit: &str,
        expected: Option<&str>,
    ) -> Output {
        let mut args = vec!["update-ref", reference, commit];
        args.extend(expected);
        Self::git_output(root, &args)
    }

    pub fn delete_reference(root: &Path, reference: &str) -> Output {
        Self::git_output(root, &["update-ref", "-d", reference])
    }

    pub fn object_exists(root: &Path, object: &str) -> Output {
        Self::git_output(root, &["cat-file", "-e", object])
    }

    pub fn staged_entries(root: &Path, paths: &[&str]) -> Output {
        let mut args = vec!["ls-files", "--stage", "-z"];
        if !paths.is_empty() {
            args.push("--");
            args.extend_from_slice(paths);
        }
        Self::git_output(root, &args)
    }

    pub fn configure_hooks(root: &Path, path: Option<&str>) -> Output {
        match path {
            Some(path) => Self::git_output(root, &["config", "core.hooksPath", path]),
            None => Self::git_output(root, &["config", "--unset", "core.hooksPath"]),
        }
    }

    pub fn porcelain_status(root: &Path) -> Output {
        Self::git_output(root, &["status", "--porcelain"])
    }

    pub fn tracked_status(root: &Path, paths: &[&str]) -> Output {
        Self::git_output(
            root,
            &[
                &["status", "--short", "--untracked-files=no", "--"][..],
                paths,
            ]
            .concat(),
        )
    }

    pub fn commit_message(root: &Path) -> Output {
        Self::git_output(root, &["log", "-1", "--format=%B"])
    }

    pub fn restore_head_paths(root: &Path, paths: &[&str]) -> Output {
        Self::git_output(root, &[&["checkout", "HEAD", "--"][..], paths].concat())
    }

    pub fn branch_name(root: &Path) -> Output {
        Self::git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"])
    }

    pub fn create_branch(root: &Path, branch: &str) -> Output {
        Self::git_output(root, &["checkout", "-b", branch])
    }

    pub fn checkout_branch(root: &Path, branch: &str) -> Output {
        Self::git_output(root, &["checkout", branch])
    }

    pub fn merge_branch(root: &Path, branch: &str, message: &str) -> Output {
        Self::git_output(root, &["merge", "--no-ff", branch, "-m", message])
    }

    pub fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> Output {
        Self::git_output(root, &["merge-base", "--is-ancestor", ancestor, descendant])
    }

    pub fn create_worktree(root: &Path, path: &Path, branch: &str) -> Output {
        Self::git_output(
            root,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                branch,
                path.to_str().unwrap(),
            ],
        )
    }

    pub fn collect_unreachable_snapshot(root: &Path, obsolete_ref: &str) -> [Output; 3] {
        [
            Self::delete_reference(root, obsolete_ref),
            Self::git_output(root, &["reflog", "expire", "--expire=now", "--all"]),
            Self::git_output(root, &["gc", "--prune=now"]),
        ]
    }

    pub fn remove_worktree(root: &Path, path: &Path) -> Output {
        Self::git_output(
            root,
            &["worktree", "remove", "--force", path.to_str().unwrap()],
        )
    }
}
