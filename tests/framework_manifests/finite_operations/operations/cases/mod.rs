use super::*;

mod endpoint_projection;
mod factory_members;
mod file_route;
mod named_binding;
mod symbol_attachment;

pub(super) fn all() -> [Case; 5] {
    [
        symbol_attachment::case(),
        file_route::case(),
        factory_members::case(),
        named_binding::case(),
        endpoint_projection::case(),
    ]
}

pub(super) fn assert_file_route_projection(
    conn: &Connection,
    phase: &str,
    rewrite: &impl Fn(&str) -> String,
) {
    file_route::assert_unsupported_routes(conn, phase, rewrite);
}

pub(super) fn assert_symbol_attachment_projection(conn: &Connection, phase: &str, active: bool) {
    symbol_attachment::assert_attachment_projection(conn, phase, active);
}

pub(super) fn assert_factory_member_projection(
    conn: &Connection,
    phase: &str,
    rewrite: &impl Fn(&str) -> String,
) {
    factory_members::assert_unconfigured_member_reads(conn, phase, rewrite);
}

pub(super) fn assert_unknown_endpoint_inputs(payloads: &[Value], phase: &str) {
    endpoint_projection::assert_unknown_endpoint_inputs(payloads, phase);
}

pub(super) fn run_named_binding_transitions(
    workspace: &Workspace,
    case: &Case,
    phase: &str,
    manifest: &str,
    source: &str,
    rewrite: &impl Fn(&str) -> String,
) {
    named_binding::run_delta_transitions(workspace, case, phase, manifest, source, rewrite);
}
