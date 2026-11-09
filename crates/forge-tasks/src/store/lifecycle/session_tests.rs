use super::resolve_session_fingerprint;
use std::collections::BTreeMap;

#[test]
fn explicit_runner_transport_preserves_identity_without_host_lookup() {
    let mut requested = Vec::new();
    let resolved = resolve_session_fingerprint(Some("explicit-runner-session"), |key| {
        requested.push(key.to_owned());
        Some("other-session".to_owned())
    });
    assert_eq!(resolved.as_deref(), Some("explicit-runner-session"));
    assert!(requested.is_empty());
}

#[test]
fn lifecycle_session_metadata_uses_only_explicit_forge_transport() {
    for canonical in [None, Some(" "), Some("forge-session")] {
        let mut environment = BTreeMap::from([
            ("CODEX_THREAD_ID", "codex-session"),
            ("AGY_CONVERSATION_ID", "antigravity-session"),
            ("CLAUDE_SESSION_ID", "claude-session"),
            ("CURSOR_SESSION_ID", "cursor-session"),
        ]);
        if let Some(canonical) = canonical {
            environment.insert("FORGE_AGENT_SESSION_ID", canonical);
        }
        let mut requested = Vec::new();
        let resolved = resolve_session_fingerprint(Some(" "), |key| {
            requested.push(key.to_owned());
            environment.get(key).map(|value| (*value).to_owned())
        });
        assert_eq!(
            resolved.as_deref(),
            canonical.filter(|value| !value.trim().is_empty())
        );
        assert_eq!(requested, ["FORGE_AGENT_SESSION_ID"]);
    }
}
