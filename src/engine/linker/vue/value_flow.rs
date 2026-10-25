use crate::core::models::Node;

pub(in crate::engine::linker) fn setup_scope_complete(
    owner: &Node,
    parent_is_module: bool,
) -> bool {
    owner.kind == "template_scope" && owner.language == "vue" && parent_is_module
}

pub(in crate::engine::linker) fn owns_module_fields(owner: &Node) -> bool {
    owner.kind == "module" && owner.language == "vue"
}
