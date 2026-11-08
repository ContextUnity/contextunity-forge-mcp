use crate::harness::{HarnessResult, McpTestHarness};
use serde_json::{json, Value};

impl crate::common::Workspace {
    pub fn cli_text(&self, args: &[&str]) -> anyhow::Result<String> {
        let output = crate::cli::run_cli(self, args);
        anyhow::ensure!(
            output.status.success(),
            "CLI failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }

    pub fn query(&self, args: &[&str]) -> anyhow::Result<Value> {
        let output = crate::cli::run_cli_json(self, args);
        anyhow::ensure!(
            output.status.success(),
            "CLI failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }
}

impl McpTestHarness {
    fn select(
        &mut self,
        tool: &str,
        field: &str,
        input: &str,
        mut options: Value,
    ) -> HarnessResult<Value> {
        options[field] = json!(input);
        self.payload(tool, options)
    }

    pub fn inspect(&mut self, selector: &str, options: Value) -> HarnessResult<Value> {
        self.select("code_map_inspect", "selector", selector, options)
    }
    pub fn explain(&mut self, selector: &str, options: Value) -> HarnessResult<Value> {
        self.select("code_map_explain", "selector", selector, options)
    }
    pub fn search(&mut self, pattern: &str, options: Value) -> HarnessResult<Value> {
        self.select("code_map_search", "pattern", pattern, options)
    }
    pub fn document(&mut self, path: &str, options: Value) -> HarnessResult<Value> {
        self.select("get_doc", "path_or_id", path, options)
    }
    pub fn source(&mut self, selector: &str, options: Value) -> HarnessResult<Value> {
        self.select("forge_read", "selector", selector, options)
    }
}
