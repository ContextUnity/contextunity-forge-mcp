#![cfg(all(feature = "lang-python", feature = "lang-typescript"))]

use crate::common::Workspace;
use contextunity_forge_mcp::{core::commitments, db::reader};
use rusqlite::Connection;

fn assert_cold_equivalent(workspace: &Workspace) {
    let incremental = workspace.open();
    commitments::verify(&incremental).unwrap();
    let cold_path = workspace.path(".forge/cold.sqlite");
    workspace.build_to(&cold_path);
    let cold = reader::open(&cold_path, workspace.root()).unwrap();
    commitments::verify(&cold).unwrap();
    for index in ["node_search", "doc_search"] {
        assert_eq!(
            fts_postings(&incremental, index),
            fts_postings(&cold, index),
            "logical {index} postings must match the cold snapshot"
        );
    }
    for query in [
        "SELECT id||'|'||kind||'|'||qualname||'|'||line||'|'||end_line||'|'||details FROM nodes ORDER BY id",
        "SELECT (SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||(SELECT path FROM path_dictionary WHERE path_id=edges.path_id)||'|'||line||'|'||occurrence_count||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.evidence_id)||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=edges.confidence_id) FROM edges ORDER BY (SELECT id FROM nodes WHERE node_hash=src_hash),(SELECT id FROM nodes WHERE node_hash=dst_hash),kind",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=owner_id)||'|'||(SELECT id FROM nodes WHERE node_hash=src_hash)||'|'||(SELECT id FROM nodes WHERE node_hash=dst_hash)||'|'||kind||'|'||line||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=edge_occurrences.evidence_id)||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=edge_occurrences.confidence_id) FROM edge_occurrences ORDER BY owner_id,ordinal",
        "SELECT (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id)||'|'||line||'|'||(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id)||'|'||status||'|'||(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id) FROM resolution_coverage ORDER BY (SELECT path FROM path_dictionary WHERE path_id=resolution_coverage.path_id),line,(SELECT expression FROM coverage_expressions WHERE expression_id=resolution_coverage.expression_id),status,(SELECT evidence FROM coverage_evidence WHERE evidence_id=resolution_coverage.evidence_id)",
        r#"SELECT json_array(p.path, c.line, e.expression, c.status, v.evidence, c.language)
           FROM coverage_owner_language c
           JOIN path_dictionary p ON p.path_id=c.path_id
           JOIN coverage_expressions e ON e.expression_id=c.expression_id
           JOIN coverage_evidence v ON v.evidence_id=c.evidence_id
           ORDER BY p.path, c.line, e.expression, c.status, v.evidence, c.language"#,
        r#"SELECT json_array(p.path, c.language, c.status, c.records)
           FROM coverage_language_counts c
           JOIN path_dictionary p ON p.path_id=c.path_id
           ORDER BY p.path, c.language, c.status"#,
        "SELECT n.id||'|'||p.path||'|'||n.line FROM node_search JOIN nodes n ON n.node_hash=node_search.rowid JOIN path_dictionary p ON p.path_id=n.path_id WHERE node_search MATCH 'execute' ORDER BY n.id,p.path,n.line",
        "SELECT p.path||'|'||f.path||'|'||d.kind||'|'||d.symbol FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id JOIN files f ON f.path_hash=d.target_hash WHERE d.target_hash IS NOT NULL ORDER BY p.path,f.path,d.kind,d.symbol",
        "SELECT domain||'|'||lower(hex(digest)) FROM domain_commitments WHERE owner_id=0 ORDER BY domain",
        "SELECT value FROM metadata WHERE key='output_root'",
    ] {
        assert_eq!(rows(&incremental, query), rows(&cold, query), "{query}");
    }
}
fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn fts_postings(conn: &Connection, index: &str) -> Vec<String> {
    let (table, vocab) = match index {
        "node_search" => ("node_search", "node_search_commitment_vocab"),
        "doc_search" => ("doc_search", "doc_search_commitment_vocab"),
        _ => panic!("unsupported FTS index: {index}"),
    };
    let query_only: bool = conn
        .query_row("PRAGMA query_only", [], |row| row.get(0))
        .unwrap();
    if query_only {
        conn.pragma_update(None, "query_only", false).unwrap();
    }
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE temp.{vocab} USING fts5vocab(main,{table},'instance')"
    ))
    .unwrap();
    if query_only {
        conn.pragma_update(None, "query_only", true).unwrap();
    }
    rows(
        conn,
        &format!(
            "SELECT term||'|'||doc||'|'||col||'|'||offset FROM temp.{vocab} ORDER BY term,doc,col,offset"
        ),
    )
}

#[test]
fn body_edits_refresh_owner_without_relinking_consumers_and_line_moves_relink() {
    let w = Workspace::new();
    w.write(
        "provider.py",
        "def left(): return 1\ndef right(): return 2\ndef value(): return left()\n",
    );
    for i in 0..30 {
        w.write(
            format!("consumer_{i}.py"),
            "from provider import value\ndef consume(): return value()\n",
        );
    }
    w.write("docs/api.md", "# API\nUse `provider.value`.\n");
    w.build();
    w.write(
        "provider.py",
        "def left(): return 1\ndef right(): return 2\ndef value(): return right()\n\n",
    );
    let delta = w.delta(&["provider.py"]);
    assert_eq!(delta["affected_owners"], 1, "{delta}");
    assert_eq!(delta["loaded_fact_files"], 0, "{delta}");
    let conn = w.open();
    let targets = rows(&conn, "SELECT (SELECT id FROM nodes WHERE node_hash=e.dst_hash) AS dst_public_id FROM edges e JOIN nodes n ON n.node_hash=e.src_hash WHERE n.qualname='provider.value' AND e.kind='calls'");
    assert!(
        targets.iter().any(|target| target.contains("right")),
        "{targets:?}"
    );
    assert!(
        !targets.iter().any(|target| target.contains("left")),
        "{targets:?}"
    );
    drop(conn);
    assert_cold_equivalent(&w);
    w.write(
        "provider.py",
        "def left(): return 1\ndef right(): unused = 0; return 2\ndef value(): return right()\n\n",
    );
    let local_edit = w.delta(&["provider.py"]);
    assert_eq!(local_edit["affected_owners"], 1, "{local_edit}");
    assert_eq!(local_edit["loaded_fact_files"], 0, "{local_edit}");
    assert_cold_equivalent(&w);
    w.write(
        "provider.py",
        "\ndef left(): return 1\ndef right(): return 2\ndef value(): return right()\n",
    );
    let moved = w.delta(&["provider.py"]);
    assert!(moved["affected_owners"].as_u64().unwrap() > 1, "{moved}");
    assert_cold_equivalent(&w);
}

#[test]
fn resolution_digest_matches_exact_contract_changes() {
    let w = Workspace::new();
    w.write(
        "provider.ts",
        "export class First { constructor(value: number) {} }\nexport class Second { constructor(value: number) {} }\nexport function build(): First { return new First(1); }\n",
    );
    w.write(
        "consumer.ts",
        "import { build } from './provider';\nexport function consume() { return build(); }\n",
    );
    w.build();

    w.write(
        "provider.ts",
        "export class First { constructor(value: number) {} }\nexport class Second { constructor(value: number) {} }\nexport function build(): First { return new First(2); }\n",
    );
    let unchanged = w.delta(&["provider.ts"]);
    assert_eq!(unchanged["affected_owners"], 1, "{unchanged}");
    assert_eq!(unchanged["loaded_fact_files"], 0, "{unchanged}");
    assert_cold_equivalent(&w);

    w.write(
        "provider.ts",
        "export class First { constructor(value: number) {} }\nexport class Second { constructor(value: number) {} }\nexport function build(): Second { return new Second(2); }\n",
    );
    let changed = w.delta(&["provider.ts"]);
    assert_eq!(changed["affected_owners"], 2, "{changed}");
    assert_eq!(
        changed["loaded_fact_files"], 1,
        "the importer must reload after an exact contract change: {changed}"
    );
    assert_cold_equivalent(&w);
}

#[test]
fn additions_deletions_and_signature_changes_still_relink_consumers() {
    let w = Workspace::new();
    w.write("provider.py", "def value(): return 1\n");
    w.write(
        "consumer.py",
        "from provider import value, future\ndef consume(): return value(), future()\n",
    );
    w.write(
        "docs/api.md",
        "# API\nUse `provider.future` and `provider.value`.\n",
    );
    w.build();
    for source in [
        "def value(arg=1): return arg\n",
        "def value(arg=1): return arg\ndef future(): return 2\n",
        "def future(): return 2\n",
    ] {
        w.write("provider.py", source);
        let delta = w.delta(&["provider.py"]);
        assert!(delta["affected_owners"].as_u64().unwrap() > 1, "{delta}");
        assert_cold_equivalent(&w);
    }
}

#[test]
fn default_export_swap_updates_importers() {
    let w = Workspace::new();
    w.write(
        "provider.ts",
        "export default function first() { return 1; }\nexport function second() { return 2; }\n",
    );
    w.write(
        "consumer.ts",
        "import value from './provider';\nexport function consume() { return value(); }\n",
    );
    w.build();
    w.write(
        "provider.ts",
        "export function first() { return 1; }\nexport default function second() { return 2; }\n",
    );
    let delta = w.delta(&["provider.ts"]);
    assert_eq!(delta["affected_owners"], 2, "{delta}");
    assert_cold_equivalent(&w);
}

#[test]
fn linked_body_edit_preserves_cross_workspace_consumers() {
    let w = Workspace::new();
    let linked = Workspace::new();
    linked.write("src/provider.py", "def value(): return 1\n");
    w.write(
        "local/consumer.py",
        "from provider import value\ndef consume(): return value()\n",
    );
    w.write("forge-mcp.yaml", &format!("roots: [local]\nlinked_workspaces:\n  - name: library\n    path: {}\n    roots: [src]\n", linked.root().display()));
    w.build();
    linked.write("src/provider.py", "def value(): return 2\n\n");
    let delta = w.delta(&["[library]/src/provider.py"]);
    assert_eq!(delta["affected_owners"], 1, "{delta}");
    assert_cold_equivalent(&w);
}

#[test]
fn python_branch_join_alternative_edits_and_deletions_match_cold_graphs() {
    let w = Workspace::new();
    w.write(
        "branch.py",
        "class Alpha:\n    def execute(self): pass\nclass Beta:\n    def execute(self): pass\ndef run(condition):\n    if condition:\n        receiver = Alpha()\n    else:\n        receiver = Beta()\n    receiver.execute()\n",
    );
    w.build();
    let target_query = "SELECT (SELECT qualname FROM nodes WHERE node_hash=dst_hash) FROM edge_occurrences WHERE kind='possible_calls' AND evidence_id=(SELECT evidence_id FROM coverage_evidence WHERE evidence='receiver.execute') AND owner_id=(SELECT path_id FROM path_dictionary WHERE path='branch.py') ORDER BY (SELECT qualname FROM nodes WHERE node_hash=dst_hash)";
    let initial = rows(&w.open(), target_query);
    assert_eq!(initial.len(), 2, "{initial:?}");
    assert!(initial.iter().any(|target| target.contains("Alpha")));
    assert!(initial.iter().any(|target| target.contains("Beta")));
    assert_eq!(
        rows(
            &w.open(),
            "SELECT CAST(count(*) AS TEXT) FROM edge_occurrences WHERE kind='calls' AND evidence_id=(SELECT evidence_id FROM coverage_evidence WHERE evidence='receiver.execute') AND owner_id=(SELECT path_id FROM path_dictionary WHERE path='branch.py')"
        ),
        ["0"]
    );
    assert_cold_equivalent(&w);

    w.write(
        "branch.py",
        "class Alpha:\n    def execute(self): pass\nclass Gamma:\n    def execute(self): pass\ndef run(condition):\n    if condition:\n        receiver = Alpha()\n    else:\n        receiver = Gamma()\n    receiver.execute()\n",
    );
    w.delta(&["branch.py"]);
    let replaced = rows(&w.open(), target_query);
    assert_eq!(replaced.len(), 2, "{replaced:?}");
    assert!(replaced.iter().any(|target| target.contains("Alpha")));
    assert!(replaced.iter().any(|target| target.contains("Gamma")));
    assert!(replaced.iter().all(|target| !target.contains("Beta")));
    assert_eq!(
        rows(
            &w.open(),
            "SELECT CAST(count(*) AS TEXT) FROM edge_occurrences WHERE kind='calls' AND evidence_id=(SELECT evidence_id FROM coverage_evidence WHERE evidence='receiver.execute') AND owner_id=(SELECT path_id FROM path_dictionary WHERE path='branch.py')"
        ),
        ["0"]
    );
    assert_cold_equivalent(&w);

    w.write(
        "branch.py",
        "class Alpha:\n    def execute(self): pass\nclass Gamma:\n    def execute(self): pass\ndef run(condition):\n    if condition:\n        receiver = Alpha()\n    else:\n        pass\n    receiver.execute()\n",
    );
    w.delta(&["branch.py"]);
    let deleted_alternative = rows(&w.open(), target_query);
    assert_eq!(deleted_alternative.len(), 1, "{deleted_alternative:?}");
    assert!(deleted_alternative[0].contains("Alpha"));
    assert_cold_equivalent(&w);

    use std::fmt::Write as _;
    let branch_source = |count: usize| {
        let mut source = String::new();
        for index in 0..count {
            writeln!(source, "class Choice{index}:\n    def execute(self): pass").unwrap();
        }
        source.push_str("def run(choice):\n");
        for index in 0..count - 1 {
            if index == 0 {
                writeln!(source, "    if choice == {index}:").unwrap();
            } else {
                writeln!(source, "    elif choice == {index}:").unwrap();
            }
            writeln!(source, "        receiver = Choice{index}()").unwrap();
        }
        source.push_str("    else:\n");
        writeln!(source, "        receiver = Choice{}()", count - 1).unwrap();
        source.push_str("    receiver.execute()\n");
        source
    };

    w.write("branch.py", &branch_source(8));
    w.delta(&["branch.py"]);
    assert_eq!(rows(&w.open(), target_query).len(), 8);
    assert_cold_equivalent(&w);

    w.write("branch.py", &branch_source(9));
    w.delta(&["branch.py"]);
    let overflow_conn = w.open();
    assert!(rows(&overflow_conn, target_query).is_empty());
    assert_eq!(
        rows(
            &overflow_conn,
            "SELECT CAST(count(*) AS TEXT) FROM edge_occurrences WHERE kind='calls' AND evidence_id=(SELECT evidence_id FROM coverage_evidence WHERE evidence='receiver.execute') AND owner_id=(SELECT path_id FROM path_dictionary WHERE path='branch.py')"
        ),
        ["0"]
    );
    let overflow_diagnostic = rows(
        &overflow_conn,
        "SELECT v.evidence FROM resolution_coverage c JOIN path_dictionary p ON p.path_id=c.path_id JOIN coverage_expressions x ON x.expression_id=c.expression_id JOIN coverage_evidence v ON v.evidence_id=c.evidence_id WHERE p.path='branch.py' AND x.expression='receiver.execute' ORDER BY c.line",
    );
    assert_eq!(overflow_diagnostic.len(), 1, "{overflow_diagnostic:?}");
    assert!(overflow_diagnostic[0].contains("Unknown/Incomplete"));
    assert!(overflow_diagnostic[0].contains("no truncated target set emitted"));
    drop(overflow_conn);
    assert_cold_equivalent(&w);
}
