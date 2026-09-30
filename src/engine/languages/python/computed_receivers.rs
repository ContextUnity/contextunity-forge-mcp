use crate::{core::models::ReceiverHint, engine::ast::{bounded_expression, text}};
use tree_sitter::Node;

fn static_callee(node: Node<'_>) -> bool {
    match node.kind() {
        "identifier" => true,
        "attribute" => node.child_by_field_name("attribute").is_some_and(|member| member.kind() == "identifier")
            && node.child_by_field_name("object").is_some_and(static_callee),
        _ => false,
    }
}

pub(super) fn receiver_hint(call: Node<'_>, source: &str) -> Option<ReceiverHint> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "attribute" { return None; }
    let member = function.child_by_field_name("attribute")?;
    if member.kind() != "identifier" { return None; }
    let receiver = function.child_by_field_name("object")?;
    match receiver.kind() {
        "string" => {
            let start = receiver.named_child(0)?;
            if start.kind() != "string_start" { return None; }
            let prefix = text(start, source).split(['\'', '"']).next()?;
            if prefix.bytes().any(|byte| matches!(byte, b'b' | b'B' | b'f' | b'F')) { return None; }
            Some(ReceiverHint::StringLiteral { member: text(member, source).to_owned() })
        }
        "call" => {
            let callee = receiver.child_by_field_name("function")?;
            if !static_callee(callee) { return None; }
            if callee.kind() == "identifier" && text(callee, source) == "super" {
                let arguments = receiver.child_by_field_name("arguments")?;
                if arguments.named_child_count() != 0 { return None; }
                Some(ReceiverHint::Super { member: text(member, source).to_owned() })
            } else {
                Some(ReceiverHint::CallResult { callee: bounded_expression(text(callee, source)), member: text(member, source).to_owned() })
            }
        }
        _ => None,
    }
}
