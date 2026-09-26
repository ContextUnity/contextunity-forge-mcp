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
    generated INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS nodes (
    node_id INTEGER PRIMARY KEY,
    id TEXT UNIQUE NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualname TEXT NOT NULL,
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    is_test INTEGER NOT NULL,
    language TEXT NOT NULL,
    generated INTEGER NOT NULL,
    details TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS edges (
    edge_id INTEGER PRIMARY KEY,
    src_public_id TEXT NOT NULL,
    dst_public_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    evidence TEXT NOT NULL,
    confidence TEXT NOT NULL,
    occurrence_count INTEGER NOT NULL,
    UNIQUE (src_public_id, dst_public_id, kind)
);

CREATE TABLE IF NOT EXISTS local_facts (
    path TEXT PRIMARY KEY,
    source_digest TEXT NOT NULL,
    language TEXT NOT NULL,
    is_test INTEGER NOT NULL,
    generated INTEGER NOT NULL,
    facts_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS owned_nodes (
    owner TEXT NOT NULL,
    public_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualname TEXT NOT NULL,
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    is_test INTEGER NOT NULL,
    language TEXT NOT NULL,
    generated INTEGER NOT NULL,
    details_json TEXT NOT NULL,
    PRIMARY KEY (owner, public_id)
);

CREATE TABLE IF NOT EXISTS edge_occurrences (
    owner TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    src TEXT NOT NULL,
    dst TEXT NOT NULL,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    evidence TEXT NOT NULL,
    confidence TEXT NOT NULL,
    PRIMARY KEY (owner, ordinal)
);

CREATE TABLE IF NOT EXISTS owned_search (
    owner TEXT NOT NULL,
    public_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    search_text TEXT NOT NULL,
    PRIMARY KEY (owner, public_id, ordinal)
);

CREATE TABLE IF NOT EXISTS dependencies (
    owner TEXT NOT NULL,
    target TEXT,
    kind TEXT NOT NULL,
    symbol TEXT NOT NULL,
    resolution TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS reverse_dependencies (
    target TEXT NOT NULL,
    owner TEXT NOT NULL,
    kind TEXT NOT NULL,
    symbol TEXT NOT NULL,
    resolution TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS shared_owners (
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    owner TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    PRIMARY KEY (kind, key, owner, ordinal)
);

CREATE TABLE IF NOT EXISTS file_commitments (
    path TEXT PRIMARY KEY,
    digest TEXT NOT NULL,
    inventory_hash TEXT NOT NULL,
    facts_hash TEXT NOT NULL,
    nodes_hash TEXT NOT NULL,
    edges_hash TEXT NOT NULL,
    search_hash TEXT NOT NULL,
    deps_hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS search_text (
    public_id TEXT PRIMARY KEY,
    search_text TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS errors (
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    message TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS resolution_coverage (
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    expression TEXT NOT NULL,
    status TEXT NOT NULL,
    evidence TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS domain_commitments (
    domain TEXT PRIMARY KEY,
    digest TEXT NOT NULL
);

-- Documentation Sections & Architectural Invariants Table
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

-- Virtual FTS5 Tables
CREATE VIRTUAL TABLE IF NOT EXISTS node_search USING fts5(
    search_text,
    content='',
    detail='none',
    columnsize=0,
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

-- Essential Performance Indexes
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_qualname ON nodes(qualname);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_path ON nodes(path);
CREATE INDEX IF NOT EXISTS idx_edges_src ON edges(src_public_id);
CREATE INDEX IF NOT EXISTS idx_edges_dst ON edges(dst_public_id);
CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);
CREATE INDEX IF NOT EXISTS idx_edges_path ON edges(path);
CREATE INDEX IF NOT EXISTS idx_doc_sections_path ON doc_sections(path);
CREATE INDEX IF NOT EXISTS idx_doc_sections_type ON doc_sections(doc_type);
CREATE INDEX IF NOT EXISTS idx_owned_nodes_public ON owned_nodes(public_id);
CREATE INDEX IF NOT EXISTS idx_owned_nodes_owner ON owned_nodes(owner, public_id);
CREATE INDEX IF NOT EXISTS idx_occurrences_pair ON edge_occurrences(src,dst,kind);
CREATE INDEX IF NOT EXISTS idx_occurrences_owner ON edge_occurrences(owner, ordinal);
CREATE INDEX IF NOT EXISTS idx_dependencies_owner ON dependencies(owner);
CREATE INDEX IF NOT EXISTS idx_reverse_owner ON reverse_dependencies(owner);
CREATE INDEX IF NOT EXISTS idx_reverse_target ON reverse_dependencies(target);
CREATE INDEX IF NOT EXISTS idx_coverage_owner ON resolution_coverage(path);
CREATE INDEX IF NOT EXISTS idx_coverage_status ON resolution_coverage(status);
CREATE INDEX IF NOT EXISTS idx_errors_owner ON errors(path);
CREATE INDEX IF NOT EXISTS idx_shared_owners_owner ON shared_owners(owner);
"#;
