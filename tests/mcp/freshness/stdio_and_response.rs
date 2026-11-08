use super::*;

const DOCUMENT_FILES: &[(&str, &str)] = &[
    ("main.py", "def target(): pass\ndef caller(): target()\n"),
    (
        "tests/test_main.py",
        "from main import target\ndef helper_one(): target()\ndef test_target(): helper_one()\n",
    ),
    (
        "README.md",
        "# Contract\n\nThe `target` function preserves the documented contract.\n",
    ),
];

static DOCUMENT_WORKSPACE: std::sync::OnceLock<Workspace> = std::sync::OnceLock::new();

fn document_workspace() -> &'static Workspace {
    DOCUMENT_WORKSPACE.get_or_init(|| {
        let workspace = Workspace::with_files(DOCUMENT_FILES);
        workspace.build();
        workspace
    })
}

fn response_text(client: &mut McpTestHarness, tool: &str, arguments: Value) -> String {
    client
        .text(tool, arguments)
        .expect("MCP tool should succeed")
}

static REFERENCE_WORKSPACE: std::sync::OnceLock<Workspace> = std::sync::OnceLock::new();

fn reference_workspace() -> &'static Workspace {
    REFERENCE_WORKSPACE.get_or_init(|| {
        let workspace = Workspace::with_files(&[
            ("forge-mcp.yaml", "roots: [src]\ndocs: [docs/reference]\n"),
            ("src/overview.py", "def code_map_overview(): pass\n"),
            (
                "src/candidates.py",
                "def shared(): return 'first'\ndef shared(): return 'second'\n",
            ),
            (
                "docs/reference/mcp-tools.md",
                "---\ntitle: MCP tools\ndoc_type: reference\n---\n# MCP tools\n## Overview one\n[`code_map_overview`](../../src/overview.py#L1)\n\n## Overview two\n[`code_map_overview`](../../src/overview.py#L1)\n\n## Overview three\n[`code_map_overview`](../../src/overview.py#L1)\n\n## Overview four\n[`code_map_overview`](../../src/overview.py#L1)\n\n## Ambiguous candidates\n[`shared`](../../src/candidates.py#L1)\n",
            ),
        ]);
        workspace.build();
        workspace
    })
}

#[path = "document_responses.rs"]
mod document_responses;
#[path = "governance_responses.rs"]
mod governance_responses;
#[path = "inventory_and_adapter_progression.rs"]
mod inventory_and_adapter_progression;
#[path = "reference_identity.rs"]
mod reference_identity;
#[path = "reference_responses.rs"]
mod reference_responses;
