use crate::core::models::Node;
use crate::core::semantic::SourcePosition;

use super::super::{semantic_context::Context, value_flow::TypeTarget};

pub(in crate::engine::linker) fn external_factory_result<'a>(
    context: &Context<'a>,
    owner: &'a Node,
    callee: &str,
    at: SourcePosition,
) -> Option<TypeTarget<'a>> {
    let profile = crate::engine::languages::by_id(&owner.language)?;
    (callee == "await fetch"
        && profile.builtin("fetch")
        && profile.builtin_member("Web.Response", "json")
        && context.unshadowed_global(owner, "fetch", at))
    .then(|| TypeTarget::Builtin("Web.Response".to_owned()))
}

pub(in crate::engine::linker) fn receiver_access(owner: &Node, head: &str) -> Option<(bool, bool)> {
    (matches!(owner.language.as_str(), "javascript" | "typescript")
        && head == "this"
        && owner.details["receiver_name"].as_str() == Some(head))
    .then_some((owner.details["is_static"] == true, true))
}
