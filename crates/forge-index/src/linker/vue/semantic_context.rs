use crate::core::models::Node;

pub(in crate::linker) fn template_import_visible(owner: &Node) -> bool {
    owner.kind == "template_scope" && owner.language == "vue"
}
