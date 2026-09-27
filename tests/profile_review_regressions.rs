#![cfg(any(
    feature = "lang-rust",
    feature = "lang-ruby",
    feature = "lang-c",
    feature = "lang-cpp",
    feature = "lang-php",
    feature = "lang-java"
))]

use contextunity_forge_mcp::db::{reader, traversal, writer};
use rusqlite::Connection;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("forge_review_{}_{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn db(&self) -> PathBuf {
        self.0.join(".forge/code-map.sqlite")
    }
    fn build(&self) {
        writer::build(&self.0, &self.db(), None).unwrap();
    }
    fn open(&self) -> Connection {
        reader::open(&self.db(), &self.0).unwrap()
    }
    fn delta_matches_cold(&self, paths: &[&str]) {
        writer::delta(
            &self.0,
            &self.db(),
            &paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
        )
        .unwrap();
        let before = snapshot(&self.open());
        self.build();
        assert_eq!(before, snapshot(&self.open()));
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn strings(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
fn snapshot(conn: &Connection) -> Vec<Vec<String>> {
    ["SELECT id||'|'||qualname||'|'||details FROM nodes ORDER BY id",
     "SELECT src||'|'||dst||'|'||kind||'|'||owner||'|'||line||'|'||confidence FROM edge_occurrences ORDER BY owner,src,dst,kind,line",
     "SELECT path||'|'||line||'|'||expression||'|'||status||'|'||evidence FROM resolution_coverage ORDER BY path,line,expression,status,evidence"]
    .iter().map(|sql|strings(conn,sql)).collect()
}
fn assert_clean(conn: &Connection) {
    let errors: i64 = conn
        .query_row("SELECT count(*) FROM errors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(errors, 0);
}
#[cfg(any(
    feature = "lang-c",
    feature = "lang-cpp",
    feature = "lang-php",
    feature = "lang-java"
))]
fn calls(conn: &Connection) -> Vec<String> {
    strings(conn,"SELECT s.name||'->'||d.name FROM edge_occurrences e JOIN nodes s ON s.id=e.src JOIN nodes d ON d.id=e.dst WHERE e.kind='calls' ORDER BY s.name,d.name")
}
fn assert_uncertain_removal(conn: &Connection, selector: &str) {
    let result = traversal::removal(conn, selector).unwrap();
    assert_eq!(result["safe_to_remove"], false, "{result}");
    assert!(
        result["unresolved_references"].as_u64().unwrap() > 0,
        "{result}"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn rust_owner_namespace_alternatives_precede_linked_providers() {
    let owner = Workspace::new();
    let linked = Workspace::new();
    owner.write(
        "forge-mcp.yaml",
        &format!(
            "roots: [.]\nlinked_workspaces:\n  - name: provider\n    path: '{}'\n    roots: [.]\n",
            linked.0.display()
        ),
    );
    owner.write(
        "lib.rs",
        "mod pkg; use pkg::go; pub fn caller(){go::target();}\n",
    );
    owner.write("pkg.rs", "pub mod go {pub fn target(){}}\n");
    owner.build();
    for step in 0..7 {
        let changed = match step {
            0 => {
                linked.write("pkg/go.rs", "pub fn target(){}\n");
                vec!["[provider]/pkg/go.rs"]
            }
            1 => {
                fs::remove_file(linked.0.join("pkg/go.rs")).unwrap();
                vec!["[provider]/pkg/go.rs"]
            }
            2 => {
                linked.write("pkg/go.rs", "pub fn target(){}\n");
                vec!["[provider]/pkg/go.rs"]
            }
            3 => {
                owner.write("pkg.rs", "pub mod go;\n");
                owner.write("pkg/go.rs", "pub fn target(){}\n");
                vec!["pkg.rs", "pkg/go.rs"]
            }
            4 => {
                fs::remove_file(owner.0.join("pkg/go.rs")).unwrap();
                owner.write("pkg.rs", "pub mod other {}\n");
                vec!["pkg.rs", "pkg/go.rs"]
            }
            5 => {
                owner.write("pkg/mod.rs", "pub mod go {pub fn target(){}}\n");
                vec!["pkg/mod.rs"]
            }
            _ => {
                fs::remove_file(owner.0.join("pkg.rs")).unwrap();
                fs::remove_file(owner.0.join("pkg/mod.rs")).unwrap();
                vec!["pkg.rs", "pkg/mod.rs"]
            }
        };
        owner.delta_matches_cold(&changed);
        let conn = owner.open();
        assert_clean(&conn);
        let targets=strings(&conn,"SELECT d.path FROM edge_occurrences e JOIN nodes s ON s.id=e.src JOIN nodes d ON d.id=e.dst WHERE e.kind='calls' AND s.name='caller'");
        let expected = match step {
            0..=2 => vec!["pkg.rs"],
            3 => vec!["pkg/go.rs"],
            6 => vec!["[provider]/pkg/go.rs"],
            _ => vec![],
        };
        assert_eq!(targets, expected, "provider transition {step}");
        if matches!(step, 4 | 5) {
            assert_uncertain_removal(&conn, "lib.rs");
        }
    }
}

#[cfg(feature = "lang-ruby")]
#[test]
fn ruby_value_expression_positions_retain_uncertainty() {
    for expression in [
        "helper",
        "return helper",
        "consume(helper)",
        "value = helper",
        "if helper; 1; end",
        "[helper]",
        "{value: helper}",
        "helper + 1",
        "(helper)",
        "helper ? 1 : 0",
    ] {
        let w = Workspace::new();
        let source = format!(
            "def helper; 1; end\ndef consume(value); 1; end\ndef caller; {expression}; end\n"
        );
        w.write("sample.rb", &source);
        w.build();
        let conn = w.open();
        assert_clean(&conn);
        let uncertain:i64=conn.query_row("SELECT count(*) FROM resolution_coverage WHERE expression='helper' AND status='unresolved'",[],|r|r.get(0)).unwrap();
        assert!(uncertain > 0, "bare expression: {expression}");
        assert_uncertain_removal(&conn, "helper");
        drop(conn);
        w.write("sample.rb", &format!("\n{source}"));
        w.delta_matches_cold(&["sample.rb"]);
    }
    let w = Workspace::new();
    w.write("sample.rb","def helper(value, optional=1, *rest, **kwargs, &block); local = 1; local, other = [1,2]; 1; end\n");
    w.build();
    let conn = w.open();
    assert_clean(&conn);
    assert!(
        strings(&conn, "SELECT expression FROM resolution_coverage").is_empty(),
        "declarations and binding names are not calls"
    );
}

#[cfg(any(feature = "lang-c", feature = "lang-cpp"))]
#[test]
fn c_and_cpp_collect_every_pointer_declarator() {
    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        extension: String,
        source: String,
        expected: String,
    }
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/c_callable_bindings.json")).unwrap();
    assert_eq!(cases.len(), 113);
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in cases {
        let enabled = match case.extension.as_str() {
            "c" => cfg!(feature = "lang-c"),
            "cpp" => cfg!(feature = "lang-cpp"),
            other => panic!("unknown fixture extension: {other}"),
        };
        if !enabled {
            continue;
        }
        checked += 1;
        let w = Workspace::new();
        let path = format!("sample.{}", case.extension);
        w.write(&path, &case.source);
        w.build();
        let conn = w.open();
        assert_clean(&conn);
        let actual = strings(&conn,"SELECT DISTINCT s.name||'->'||d.name FROM edge_occurrences e JOIN nodes s ON s.id=e.src JOIN nodes d ON d.id=e.dst WHERE e.kind='calls' ORDER BY s.name,d.name");
        let expected = match case.expected.as_str() {
            "unknown" => vec!["control->helper"],
            "unknown_no_control" => vec![],
            "exact" | "exact_duplicate" => vec!["caller->helper", "control->helper"],
            other => panic!("unknown fixture expectation: {other}"),
        };
        if actual != expected {
            failures.push(format!(
                "{}: expected {expected:?}, got {actual:?}",
                case.name
            ));
        }
        if case.expected.starts_with("unknown") {
            let final_call_line = case
                .source
                .lines()
                .enumerate()
                .filter(|(_, line)| line.contains("return helper();"))
                .map(|(index, _)| index + 1)
                .last()
                .unwrap();
            let uncertain: i64 = conn.query_row("SELECT count(*) FROM resolution_coverage WHERE expression='helper' AND line=?1 AND status='unresolved'", [final_call_line], |r|r.get(0)).unwrap();
            if uncertain == 0 {
                failures.push(format!(
                    "{}: final helper invocation lacks uncertainty",
                    case.name
                ));
            }
            assert_uncertain_removal(&conn, "helper");
        }
        drop(conn);
        w.write(&path, &format!("\n{}", case.source));
        w.delta_matches_cold(&[&path]);
        let direct = "int helper(void){return 1;}\nint caller(void){return helper();}\n";
        w.write(&path, direct);
        w.delta_matches_cold(&[&path]);
        assert_eq!(
            calls(&w.open()),
            ["caller->helper"],
            "{}: removed binding",
            case.name
        );
    }
    assert!(checked > 0);
    if cfg!(all(feature = "lang-c", feature = "lang-cpp")) {
        assert_eq!(checked, 113);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[cfg(feature = "lang-php")]
#[test]
fn php_receiver_syntax_preserves_variable_identity() {
    for expression in [
        "$Service->helper()",
        "$Service?->helper()",
        "$Service::helper()",
        "$Service->$method()",
        "Service::$method()",
    ] {
        let w = Workspace::new();
        let source=format!("<?php\nclass Service {{ public static function helper(){{return 1;}} public function known(){{return $this->helper();}} }}\nfunction named(){{return Service::helper();}}\nfunction caller(){{global $Service,$method; return {expression};}}\n");
        w.write("sample.php", &source);
        w.build();
        let conn = w.open();
        assert_clean(&conn);
        assert_eq!(
            calls(&conn),
            ["known->helper", "named->helper"],
            "{expression}"
        );
        assert_uncertain_removal(&conn, "helper");
        drop(conn);
        w.write("sample.php", &source.replacen("<?php", "<?php\n", 1));
        w.delta_matches_cold(&["sample.php"]);
    }
}

#[cfg(feature = "lang-java")]
#[test]
fn java_method_names_are_separate_from_value_names() {
    let w = Workspace::new();
    let source="class Sample {\n static int helper(){return 42;}\n static int parameter(int helper){return helper();}\n static int local(){int helper=0;return helper();}\n static int unknown(Sample Sample){return Sample.helper();}\n static int named(){return Sample.helper();}\n}\n";
    w.write("Sample.java", source);
    w.build();
    let conn = w.open();
    assert_clean(&conn);
    assert_eq!(
        calls(&conn),
        ["local->helper", "named->helper", "parameter->helper"]
    );
    assert_uncertain_removal(&conn, "helper");
    drop(conn);
    w.write("Sample.java", &format!("\n{source}"));
    w.delta_matches_cold(&["Sample.java"]);
}
