use super::*;

mod endpoint_projection;
mod factory_members;
mod file_route;
pub(super) mod named_binding;
mod symbol_attachment;

pub(super) fn for_kind(kind: OperationKind) -> Case {
    match kind {
        OperationKind::SymbolAttachment => symbol_attachment::case(),
        OperationKind::FileRoute => file_route::case(),
        OperationKind::FactoryMembers => factory_members::case(),
        OperationKind::NamedBinding => named_binding::case(),
        OperationKind::EndpointProjection => endpoint_projection::case(),
    }
}
