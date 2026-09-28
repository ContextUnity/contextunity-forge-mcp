use contextunity_forge_mcp::{
    core::response::{QueryOptions, ResponsePolicy},
    db::{lint, reader, writer},
    engine::languages,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("forge_lint_{}_{nonce}", std::process::id()));
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
    fn populate(&self) -> BTreeSet<String> {
        let mut profiles = BTreeSet::new();
        for profile in languages::profiles() {
            let source = match profile.id() {
                "python" => "def\n",
                "rust" => "fn broken( {\n",
                "javascript" | "typescript" => "function broken( {\n",
                "vue" => "<script>function broken( {</script>\n",
                "proto" => "message Broken { invalid = ;\n",
                "html" => "<div attr=\"",
                "yaml" => "key: [one, two\n",
                "toml" => "key = [\n",
                "go" => "package main\nfunc broken( {\n",
                "java" => "class Broken { void broken( }\n",
                "csharp" => "class Broken { void Broken( }\n",
                "kotlin" => "fun broken( {\n",
                "php" => "<?php function broken( {\n",
                "ruby" => "def broken(\n",
                "c" | "cpp" => "void broken( {\n",
                language => panic!("add a syntax fixture for compiled language {language}"),
            };
            self.write(&format!("src/broken.{}", profile.extensions()[0]), source);
            profiles.insert(profile.id().to_owned());
        }
        self.write(
            "README.md",
            "# Guide\nThis document is not syntax-checked.\n",
        );
        self.write("ignored.unknown", "This file has no compiled grammar.\n");
        profiles
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stored_syntax_lint_covers_compiled_profiles_without_mutating_index() {
    let workspace = Workspace::new();
    let expected = workspace.populate();
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let before = fs::read(workspace.db()).unwrap();
    let options =
        QueryOptions::resolve(&ResponsePolicy::default(), Some(100), 0, None, None).unwrap();
    if expected.is_empty() {
        assert!(lint::syntax_paged(&conn, "", &options).is_err());
        return;
    }
    let result = lint::syntax_paged(&conn, "", &options).unwrap();
    let coverage = &result["coverage"];
    let actual: BTreeSet<_> = coverage["languages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["language"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(coverage["indexed_source_files"], expected.len());
    assert_eq!(coverage["other_indexed_files"], 1);
    assert_eq!(coverage["unindexed_files"], "not_enumerated");
    let diagnostics = result["diagnostics"]["items"].as_array().unwrap();
    let reported: BTreeSet<_> = diagnostics
        .iter()
        .map(|row| row["language"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(reported, expected, "{result}");
    for item in diagnostics {
        assert_eq!(item["rule_id"], "syntax.parse");
        assert_eq!(item["severity"], "error");
        assert!(item["line"].as_u64().unwrap() >= 1);
        assert!(!item["message"].as_str().unwrap().is_empty());
    }
    for target in [
        "README.md",
        "ignored.unknown",
        "not-indexed",
        "../outside",
        " / ",
        " src/../outside ",
        "SELECT * FROM nodes",
    ] {
        assert!(
            lint::syntax_paged(&conn, target, &options).is_err(),
            "{target}"
        );
    }
    assert_eq!(fs::read(workspace.db()).unwrap(), before);
}

#[test]
fn absence_of_stored_diagnostics_is_not_a_full_lint_success_claim() {
    let Some(profile) = languages::profiles().next() else {
        return;
    };
    let workspace = Workspace::new();
    let path = format!("source.{}", profile.extensions()[0]);
    workspace.write(&path, "\n");
    writer::build(&workspace.0, &workspace.db(), None).unwrap();
    let conn = reader::open(&workspace.db(), &workspace.0).unwrap();
    let options =
        QueryOptions::resolve(&ResponsePolicy::default(), Some(10), 0, None, None).unwrap();
    let result = lint::syntax_paged(&conn, &path, &options).unwrap();
    assert_eq!(result["status"], "no_stored_syntax_diagnostics");
    assert_eq!(result["diagnostics"]["total"], 0);
    assert_eq!(
        result["coverage"]["checks"],
        json!(["stored_parser_diagnostics"])
    );
    assert!(result["coverage"]["limitations"]
        .as_str()
        .unwrap()
        .contains("does not prove syntactic validity"));
}

struct Client {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<String>,
    id: usize,
}
impl Client {
    fn new(workspace: &Workspace) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_contextunity-forge-mcp"))
            .arg("--root")
            .arg(&workspace.0)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || sender.send(line).is_err() {
                break;
            }
        });
        let mut client = Self {
            child,
            input,
            output,
            id: 0,
        };
        client.request("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"lint-test","version":"1"}}));
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client.input.flush().unwrap();
        client
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let line = self
                .output
                .recv_timeout(Duration::from_secs(30))
                .expect("MCP response timeout");
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == self.id {
                return response;
            }
        }
    }
    fn call(&mut self, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({"name":"code_map_analyze","arguments":arguments}),
        )
    }
    fn payload(&mut self, arguments: Value) -> Value {
        let response = self.call(arguments);
        assert_ne!(response["result"]["isError"], true, "{response}");
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_lint_is_opt_in_bounded_and_rejects_incompatible_requests() {
    let workspace = Workspace::new();
    if workspace.populate().is_empty() {
        return;
    }
    let mut client = Client::new(&workspace);
    let normal = client.payload(json!({"target":""}));
    assert!(normal.get("mode").is_none());
    let first = client.payload(json!({"target":"","lint":true,"limit":1}));
    assert_eq!(first["mode"], "lint");
    assert_eq!(first["diagnostics"]["items"].as_array().unwrap().len(), 1);
    let mut offset = first["diagnostics"]["next_offset"].clone();
    let generation = first["diagnostics"]["generation"].clone();
    let mut seen = first["diagnostics"]["items"].as_array().unwrap().clone();
    while !offset.is_null() {
        let next = client.payload(
            json!({"target":"","lint":true,"limit":1,"offset":offset,"generation":generation}),
        );
        seen.extend(
            next["diagnostics"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
        offset = next["diagnostics"]["next_offset"].clone();
    }
    assert_eq!(
        seen.len(),
        first["diagnostics"]["total"].as_u64().unwrap() as usize
    );
    let all = client.payload(json!({"target":"","lint":true,"limit":100}));
    assert_eq!(json!(seen), all["diagnostics"]["items"]);
    for arguments in [
        json!({"target":"","lint":true,"include_cycles":true}),
        json!({"target":"","lint":true,"offset":1}),
        json!({"target":"","lint":true,"generation":"stale"}),
        json!({"target":"README.md","lint":true}),
        json!({"target":"ignored.unknown","lint":true}),
        json!({"target":"SELECT count(*) FROM nodes","lint":true}),
    ] {
        let result = client.call(arguments);
        assert_eq!(result["result"]["isError"], true, "{result}");
    }
    let sql = client.payload(json!({"target":"SELECT count(*) AS total FROM nodes"}));
    assert!(sql["rows"]["items"][0]["total"].as_u64().unwrap() > 0);
}
