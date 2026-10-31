use tree_sitter::Node;

pub(crate) fn bindings<'tree>(pattern: Node<'tree>, mut emit: impl FnMut(Node<'tree>)) {
    if matches!(pattern.kind(), "identifier" | "shorthand_field_identifier") {
        emit(pattern);
        return;
    }
    let mut cursor = pattern.walk();
    loop {
        let node = cursor.node();
        let descend = if cursor.field_name() == Some("type") {
            false
        } else {
            match node.kind() {
                "identifier" | "shorthand_field_identifier" => {
                    emit(node);
                    false
                }
                "tuple_struct_pattern"
                | "struct_pattern"
                | "field_pattern"
                | "tuple_pattern"
                | "slice_pattern"
                | "reference_pattern"
                | "ref_pattern"
                | "mut_pattern"
                | "captured_pattern"
                | "or_pattern" => true,
                _ => false,
            }
        };
        if descend && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}
