use crate::core::models::Node;

pub(in crate::engine::linker) fn template_import_visible(owner: &Node) -> bool {
    owner.kind == "template_scope" && owner.language == "vue"
}
