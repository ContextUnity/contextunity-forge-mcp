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
    assert_eq!(fetch.details["is_method"], true);
    assert_eq!(fetch.details["is_static"], false);
    assert!(facts
        .nodes
        .iter()
        .any(|n| n.kind == "function" && n.name == "standalone_helper"));
}
