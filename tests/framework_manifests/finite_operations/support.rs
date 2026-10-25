use super::*;
use serde_json::json;

pub(super) fn assert_delta_cold_parity(workspace: &Workspace) {
    workspace.assert_cold_equivalent();
    let incremental = workspace.open();
    let cold = reader::open(&workspace.0.join(".forge/cold.sqlite"), &workspace.0).unwrap();
    for sql in [
        "SELECT json_array(id,kind,qualname,line,end_line,details) FROM nodes ORDER BY id",
        "SELECT json_array(p.path,d.ordinal,f.path,d.kind,d.symbol,d.resolution) FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id LEFT JOIN files f ON f.path_hash=d.target_hash ORDER BY p.path,d.ordinal",
        "SELECT json_array(d.domain,CASE WHEN d.owner_id=0 THEN NULL ELSE p.path END,hex(d.digest)) FROM domain_commitments d LEFT JOIN path_dictionary p ON p.path_id=d.owner_id ORDER BY d.domain,CASE WHEN d.owner_id=0 THEN NULL ELSE p.path END",
    ] {
        assert_eq!(query_rows(&incremental, sql), query_rows(&cold, sql), "{sql}");
    }
}

pub(super) fn semantic(id: &str, operation: Value) -> Value {
    json!({"id":id,"namespace":"proof","scope":"project","activation":[],"operation":operation})
}

pub(super) fn syntax(id: &str, language: &str, operation: Value) -> Value {
    json!({"id":id,"language":language,"activation":[],"operation":operation})
}

pub(super) fn pack(semantic_rules: Vec<Value>, syntax_rules: Vec<Value>) -> Value {
    json!({"semantic_rules":semantic_rules,"syntax_rules":syntax_rules})
}

pub(super) fn server(member: &str) -> Value {
    json!({"kind":"constructed_member","module":"webkit","constructors":["Server"],"member":member})
}
