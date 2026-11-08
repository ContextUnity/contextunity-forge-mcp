use super::*;

impl MilestoneHarness {
    pub(crate) fn configured_workspace() -> Self {
        let root = Self::workspace(DEFAULT_SEAM, "");
        root.write(
            "forge-mcp.yaml",
            "roots: [src]\ndocs: [docs]\ntasks_db: .forge/tasks.sqlite\n",
        );
        root
    }

    pub(crate) fn parse_cli(
        &self,
        args: &[&str],
    ) -> Result<contextunity_forge_mcp::cli::Cli, clap::Error> {
        use clap::Parser;
        let root = self.root().to_string_lossy();
        contextunity_forge_mcp::cli::Cli::try_parse_from(
            [
                "contextunity-forge-mcp",
                "--root",
                &root,
                "--format",
                "json",
            ]
            .into_iter()
            .chain(args.iter().copied()),
        )
    }

    pub(crate) fn run_cli(&self, args: &[&str]) -> anyhow::Result<Value> {
        let mut output = Vec::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(self.parse_cli(args)?.run_with_writer(&mut output))?;
        Ok(serde_json::from_slice(&output)?)
    }

    pub(crate) fn cli(&self, args: &[&str], input: Option<&str>) -> Output {
        use std::io::Write;
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"));
        command
            .args(["--format", "json", "--root", self.root().to_str().unwrap()])
            .args(args);
        if let Some(input) = input {
            let mut child = command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        } else {
            command.output().unwrap()
        }
    }

    pub(crate) fn document(&self, path: &str) -> String {
        std::fs::read_to_string(self.path(path)).unwrap()
    }

    pub(crate) fn file_bytes(&self, path: &str) -> Vec<u8> {
        std::fs::read(self.path(path)).unwrap()
    }

    pub(crate) fn list(&self, archive: bool, status: Option<&str>, limit: usize) -> Value {
        milestone_engine::list(self.root(), archive, status, limit).unwrap()
    }

    pub(crate) fn show(&self, id: &str, full: bool) -> Value {
        milestone_engine::show(self.root(), id, full).unwrap()
    }

    fn install_files(&self, catalog: &str) {
        let files: std::collections::BTreeMap<String, Option<String>> =
            serde_json::from_str(catalog).unwrap();
        for (path, content) in files {
            if let Some(content) = content {
                self.write(&path, &content);
            } else {
                std::fs::create_dir_all(self.path(path)).unwrap();
            }
        }
    }

    pub(crate) fn from_catalog(catalog: &str) -> Self {
        let root = Self::workspace(DEFAULT_SEAM, "");
        root.install_files(catalog);
        root
    }

    pub(crate) fn frontmatter(source: &str) -> serde_yaml::Value {
        let header = source
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap()
            .0;
        serde_yaml::from_str(header).unwrap()
    }
}
