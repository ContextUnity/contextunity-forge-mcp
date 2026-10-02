/// Canonical SQLite schema for a Forge index generation.
pub const SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS source_inventory (
    path TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    digest TEXT NOT NULL,
    size INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    path TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    digest TEXT NOT NULL,
    size INTEGER NOT NULL,
    language TEXT,
    is_test INTEGER NOT NULL,
    generated INTEGER NOT NULL,
    path_hash INTEGER NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS path_dictionary (
    path_id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS nodes (
    node_id INTEGER PRIMARY KEY,
    id TEXT UNIQUE NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualname TEXT NOT NULL,
    path_id INTEGER NOT NULL REFERENCES path_dictionary(path_id),
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    is_test INTEGER NOT NULL,
    language TEXT NOT NULL,
    generated INTEGER NOT NULL,
    details TEXT NOT NULL,
    node_hash INTEGER NOT NULL UNIQUE,
    owner_path_id INTEGER NOT NULL REFERENCES path_dictionary(path_id)
);

CREATE TABLE IF NOT EXISTS coverage_evidence (
    evidence_id INTEGER PRIMARY KEY,
    evidence TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS edges (
    src_hash INTEGER NOT NULL,
    kind TEXT NOT NULL,
    dst_hash INTEGER NOT NULL,
    path_id INTEGER NOT NULL REFERENCES path_dictionary(path_id),
    line INTEGER NOT NULL,
    evidence_id INTEGER NOT NULL REFERENCES coverage_evidence(evidence_id),
    confidence_id INTEGER NOT NULL REFERENCES coverage_evidence(evidence_id),
    occurrence_count INTEGER NOT NULL,
    PRIMARY KEY (src_hash, kind, dst_hash)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS local_facts (
    path TEXT PRIMARY KEY,
    source_digest TEXT NOT NULL,
    language TEXT NOT NULL,
    is_test INTEGER NOT NULL,
    generated INTEGER NOT NULL,
    facts_blob BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS edge_occurrences (
    owner_id INTEGER NOT NULL REFERENCES path_dictionary(path_id),
    ordinal INTEGER NOT NULL,
    src_hash INTEGER NOT NULL,
    dst_hash INTEGER NOT NULL,
    kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    evidence_id INTEGER NOT NULL REFERENCES coverage_evidence(evidence_id),
    confidence_id INTEGER NOT NULL REFERENCES coverage_evidence(evidence_id),
    PRIMARY KEY (owner_id, ordinal)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS dependencies (
    owner_id INTEGER NOT NULL REFERENCES path_dictionary(path_id),
    ordinal INTEGER NOT NULL,
    target_hash INTEGER,
    kind TEXT NOT NULL,
    symbol TEXT NOT NULL,
    resolution TEXT NOT NULL,
    PRIMARY KEY (owner_id, ordinal)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS shared_keys (
    key_hash INTEGER PRIMARY KEY,
    key NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS shared_owners (
    kind_id INTEGER NOT NULL CHECK(kind_id BETWEEN 0 AND 2),
    key_hash INTEGER NOT NULL,
    owner_id INTEGER NOT NULL REFERENCES path_dictionary(path_id),
    ordinal INTEGER NOT NULL,
    PRIMARY KEY (owner_id, kind_id, key_hash, ordinal)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS errors (
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    message TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS coverage_expressions (
    expression_id INTEGER PRIMARY KEY,
    expression TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS resolution_coverage (
    path_id INTEGER NOT NULL REFERENCES path_dictionary(path_id),
    line INTEGER NOT NULL,
    expression_id INTEGER NOT NULL REFERENCES coverage_expressions(expression_id),
    status TEXT NOT NULL,
    evidence_id INTEGER NOT NULL REFERENCES coverage_evidence(evidence_id),
    PRIMARY KEY (path_id, line, expression_id, status, evidence_id)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS domain_commitments (
    domain TEXT NOT NULL,
    owner_id INTEGER NOT NULL,
    digest BLOB NOT NULL,
    PRIMARY KEY(domain, owner_id)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS doc_sections (
    doc_id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    section_title TEXT NOT NULL,
    doc_type TEXT NOT NULL,
    content TEXT NOT NULL,
    invariants TEXT NOT NULL,
    referenced_symbols TEXT NOT NULL,
    mtime REAL NOT NULL,
    size INTEGER NOT NULL,
    is_invariant INTEGER NOT NULL DEFAULT 0
);

CREATE VIRTUAL TABLE IF NOT EXISTS node_search USING fts5(
    search_text,
    content='',
    contentless_delete=1,
    detail='full',
    columnsize=1,
    tokenize='unicode61 separators ''._-/'''
);

CREATE VIRTUAL TABLE IF NOT EXISTS doc_search USING fts5(
    section_title,
    content,
    invariants,
    content='doc_sections',
    content_rowid='rowid',
    tokenize='unicode61'
);

CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_qualname ON nodes(qualname);
CREATE INDEX IF NOT EXISTS idx_nodes_path ON nodes(path_id);
CREATE INDEX IF NOT EXISTS idx_nodes_path_text ON nodes(path);
CREATE INDEX IF NOT EXISTS idx_nodes_owner ON nodes(owner_path_id);
CREATE INDEX IF NOT EXISTS idx_edges_dst_kind ON edges(dst_hash, kind);
CREATE INDEX IF NOT EXISTS idx_edges_path ON edges(path_id);
CREATE INDEX IF NOT EXISTS idx_doc_sections_path ON doc_sections(path);
CREATE INDEX IF NOT EXISTS idx_doc_sections_type ON doc_sections(doc_type);
CREATE INDEX IF NOT EXISTS idx_occurrences_pair ON edge_occurrences(src_hash, dst_hash);
CREATE INDEX IF NOT EXISTS idx_dependencies_target ON dependencies(target_hash, kind) WHERE target_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_coverage_status ON resolution_coverage(status) WHERE status IN('unresolved','ambiguous');
CREATE INDEX IF NOT EXISTS idx_coverage_external ON resolution_coverage(path_id) WHERE status='external';
CREATE INDEX IF NOT EXISTS idx_errors_owner ON errors(path);
CREATE INDEX IF NOT EXISTS idx_shared_owners_key ON shared_owners(kind_id,key_hash) WHERE kind_id=1 OR kind_id=2;
CREATE INDEX IF NOT EXISTS idx_shared_keys_node ON shared_keys(key) WHERE typeof(key)='integer';
"#;
