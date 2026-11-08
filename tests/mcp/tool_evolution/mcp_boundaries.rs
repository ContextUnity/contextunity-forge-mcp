pub(super) use super::*;
pub(super) use crate::analysis_fixture::{persisted_state, AnalysisFixture, PersistedWriteMarker};
pub(super) use crate::harness::McpTestHarness;
pub(super) use serde_json::json;

pub(super) const DIAGNOSTICS_SOURCE: &str = concat!(
    "from pydantic import BaseModel\r\n",
    "from pathlib import Path\r\n",
    "import requests\r\n",
    "class Record(BaseModel):\r\n",
    "    title: str\r\n",
    "def dynamic(callback):\r\n",
    "    callback('é argument')\r\n",
    "def unknown(value):\r\n",
    "    value.run('é argument')\r\n",
    "def consume(record: Record):\r\n",
    "    record.model_dump()\r\n",
    "    Path('é.txt').exists()\r\n",
    "    requests.get('https://example.invalid')\r\n",
    "    missing_target('é argument')\r\n",
    "    Record.model_validate({'title': 'ok'})\r\n",
    "class Known: pass\r\n",
    "def known(value: Known):\r\n",
    "    value.missing('known method')\r\n",
    "def computed(make):\r\n",
    "    make()('computed callback')\r\n",
    "    make().run('computed receiver')\r\n",
);

#[path = "mcp_boundaries/ast_and_search.rs"]
mod ast_and_search;
#[path = "mcp_boundaries/graph_and_impact.rs"]
mod graph_and_impact;
#[path = "mcp_boundaries/navigation_and_inspect.rs"]
mod navigation_and_inspect;
#[path = "mcp_boundaries/sql_and_analyze.rs"]
mod sql_and_analyze;
#[path = "mcp_boundaries/tasks_and_workspace.rs"]
mod tasks_and_workspace;
