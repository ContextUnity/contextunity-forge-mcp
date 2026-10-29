use super::*;

#[cfg(feature = "lang-python")]
#[test]
fn test_python_ast_extractor() {
    let source = r#"
class ServiceClient:
    """Service client docstring."""
    def __init__(self, host: str):
        self.host = host

    def fetch(self, endpoint: str) -> dict:
        return self._send_request(endpoint)

    def _send_request(self, endpoint: str) -> dict:
        return {"status": "ok"}

def standalone_helper(x: int) -> int:
    return x * 2
"#;
    let facts = ast::extract("test.py", "python", source).expect("python extraction failed");
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "class" && n.name == "ServiceClient"));
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "method" && n.name == "fetch"));
    let fetch = facts
        .nodes
        .iter()
        .find(|n| n.name == "fetch")
        .expect("fetch method missing");
    assert_eq!(fetch.details["receiver_type"], "ServiceClient");
    assert!(fetch.details.get("is_method").is_none());
    assert!(fetch.details.get("is_static").is_none());
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "standalone_helper"));

    let cases = [
        ("def run(): pass\nif __name__ == '__main__':\n    run()\n", true),
        ("def run(): pass\nrun = other\nif __name__ == '__main__':\n    run()\n", false),
        ("def run(): pass\nfor run in []: pass\nif __name__ == '__main__':\n    run()\n", false),
        ("def run(): pass\nrun += 1\nif __name__ == '__main__':\n    run()\n", false),
        ("def run(): pass\nrun = lambda: None\nif __name__ == '__main__':\n    run()\n", false),
        ("def run(): pass\ndel run\nif __name__ == '__main__':\n    run()\n", false),
        ("def run(): pass\nfrom other import run\nif __name__ == '__main__':\n    run()\n", false),
        ("if __name__ == '__main__':\n    run()\ndef run(): pass\n", false),
        ("def run(): pass\ndef run(): pass\nif __name__ == '__main__':\n    run()\n", false),
        ("def run(): pass\n", false),
        ("if False:\n    def run(): pass\nif __name__ == '__main__':\n    run()\n", false),
        ("try:\n    def run(): pass\nexcept Exception: pass\nif __name__ == '__main__':\n    run()\n", false),
        ("@identity\ndef run(): pass\nif __name__ == '__main__':\n    run()\n", true),
    ];
    std::thread::scope(|scope| {
        for (source, expected) in cases {
            scope.spawn(move || {
                for _ in 0..3 {
                    let facts = ast::extract("fixture.py", "python", source).unwrap();
                    let marked = facts.nodes.iter().filter(|n| n.kind == "function" && n.name == "run" && n.details["entrypoint_base"] == "main").count();
                    assert_eq!(marked > 0, expected, "{source}");
                    assert!(ast::extract("fixture.invalid", "unknown-language", "").is_err());
                }
            });
        }
    });
}
