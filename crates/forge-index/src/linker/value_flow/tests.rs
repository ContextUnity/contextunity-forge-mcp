use super::*;

#[test]
fn uncertain_field_writes_invalidate_declared_summary() {
    let known = TypeTarget::Builtin("str".into());
    assert!(matches!(
        merge(&known, &TypeTarget::Unknown),
        TypeTarget::Unknown
    ));
    assert!(matches!(
        merge(&known, &TypeTarget::Builtin("int".into())),
        TypeTarget::Ambiguous
    ));
}

#[test]
fn computed_receiver_key_uses_borrowed_hash_lookup() {
    let source = String::from("owner");
    let position = SourcePosition { line: 7, column: 4 };
    let mut cache = HashMap::new();
    cache.insert(
        (source.as_str(), position, "Widget().first", "calls"),
        TypeTarget::Unknown,
    );
    let query = String::from("owner");
    assert!(
        cache
            .get(&ComputedKey {
                source: &query,
                position,
                expression: "Widget().second",
                kind: "calls"
            })
            .is_none()
    );
    assert!(
        cache
            .get(&ComputedKey {
                source: &query,
                position,
                expression: "Widget().first",
                kind: "references"
            })
            .is_none()
    );
    assert!(
        cache
            .get(&ComputedKey {
                source: &query,
                expression: "Widget().first",
                kind: "calls",
                position
            })
            .is_some()
    );
    assert!(
        cache
            .get(&ComputedKey {
                source: &query,
                expression: "Widget().first",
                kind: "calls",
                position: SourcePosition { line: 7, column: 5 }
            })
            .is_none()
    );
}
