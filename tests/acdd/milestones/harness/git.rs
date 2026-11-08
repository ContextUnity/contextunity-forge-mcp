use super::*;

impl MilestoneHarness {
    pub(crate) fn commit_fixture_inputs(&self, paths: &[&str]) {
        commit_handoff_fixture_inputs(self, paths);
    }

    pub(crate) fn cherry_pick_commit(&self, commit: &str) -> Output {
        Self::git_at(self.root(), &["cherry-pick", commit]).unwrap()
    }

    pub(crate) fn merge_branch(&self, branch: &str, message: &str) -> Output {
        Self::git_output_at(self.root(), ["merge", "--no-ff", branch, "-m", message])
    }

    pub(crate) fn tree_at(&self, revision: &str) -> String {
        Self::git_text_at(self.root(), &["rev-parse", &format!("{revision}^{{tree}}")])
    }

    pub(crate) fn create_navigation_commit(&self, tree: &str, message: &str) -> String {
        Self::git_text_at(self.root(), &["commit-tree", tree, "-m", message])
    }

    pub(crate) fn refs_with_prefix(&self, prefix: &str) -> String {
        Self::git_text_at(
            self.root(),
            &["for-each-ref", "--format=%(refname)", prefix],
        )
    }

    pub(crate) fn create_orphan_branch(&self, name: &str) -> Output {
        Self::git_at(self.root(), &["checkout", "--orphan", name]).unwrap()
    }

    pub(crate) fn current_branch(&self) -> String {
        Self::git_text_at(self.root(), &["branch", "--show-current"])
    }

    pub(crate) fn create_branch_at(&self, name: &str, base: &str) -> Output {
        Self::git_at(self.root(), &["checkout", "-b", name, base]).unwrap()
    }

    pub(crate) fn select_revision(&self, revision: &str) -> Output {
        Self::git_at(self.root(), &["checkout", revision]).unwrap()
    }

    pub(crate) fn candidate_parents(&self, candidate: &str) -> anyhow::Result<Vec<String>> {
        let output = Self::git_at(
            self.root(),
            &["rev-list", "--parents", "-n", "1", candidate],
        )?;
        Ok(String::from_utf8(output.stdout)?
            .split_whitespace()
            .skip(1)
            .map(str::to_owned)
            .collect())
    }

    pub(crate) fn add_detached_worktree(
        &self,
        path: &Path,
        commit: &str,
    ) -> anyhow::Result<Output> {
        Self::git_at(
            self.root(),
            &[
                "worktree",
                "add",
                "--detach",
                path.to_str()
                    .ok_or_else(|| anyhow::anyhow!("non-UTF8 worktree path"))?,
                commit,
            ],
        )
    }

    /// Raw output is intentional: rejection tests inspect nonzero Git status.
    pub(crate) fn git_output_at(
        root: &Path,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Output {
        crate::acdd::snapshot::git_command(root, &[])
            .args([
                "-c",
                "user.name=ACDD test",
                "-c",
                "user.email=acdd-test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap()
    }

    pub(crate) fn git_at(root: &Path, args: &[&str]) -> anyhow::Result<Output> {
        let output = Self::git_output_at(root, args);
        anyhow::ensure!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    }

    pub(crate) fn git_text_at(root: &Path, args: &[&str]) -> String {
        String::from_utf8(Self::git_at(root, args).unwrap().stdout)
            .unwrap()
            .trim()
            .to_owned()
    }

    pub(crate) fn stage_paths(&self, paths: &[&str]) -> Output {
        let mut args = vec!["add", "--"];
        args.extend_from_slice(paths);
        Self::git_at(self.root(), &args).unwrap()
    }

    pub(crate) fn stage_forced_paths(&self, paths: &[&str]) -> Output {
        let mut args = vec!["add", "--force", "--"];
        args.extend_from_slice(paths);
        Self::git_at(self.root(), &args).unwrap()
    }

    pub(crate) fn commit_staged(&self, message: &str) -> Output {
        Self::git_at(self.root(), &["commit", "-m", message]).unwrap()
    }

    pub(crate) fn commit_tracked(&self, message: &str) -> Output {
        Self::git_at(self.root(), &["commit", "-am", message]).unwrap()
    }

    pub(crate) fn amend_message(&self, message: &str) -> Output {
        Self::git_at(self.root(), &["commit", "--amend", "-m", message]).unwrap()
    }

    pub(crate) fn staged_changes(&self) -> Output {
        Self::git_at(self.root(), &["diff", "--cached", "--binary"]).unwrap()
    }

    pub(crate) fn reset_history_to(&self, commit: &str) -> Output {
        Self::git_at(self.root(), &["reset", "--hard", commit]).unwrap()
    }
}
