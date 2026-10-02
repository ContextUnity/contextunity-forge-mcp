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

CREATE TRIGGER IF NOT EXISTS files_path_dictionary_insert
AFTER INSERT ON files BEGIN
    INSERT OR IGNORE INTO path_dictionary(path) VALUES(NEW.path);
END;

CREATE TABLE IF NOT EXISTS nodes_data (
    node_id INTEGER PRIMARY KEY,
    id TEXT UNIQUE NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualname TEXT NOT NULL,
    path_id INTEGER NOT NULL,
    line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    is_test INTEGER NOT NULL,
    language TEXT NOT NULL,
    generated INTEGER NOT NULL,
    details TEXT NOT NULL,
    node_hash INTEGER NOT NULL UNIQUE
);

CREATE VIEW IF NOT EXISTS nodes AS
SELECT n.node_id,n.id,n.kind,n.name,n.qualname,p.path,n.line,n.end_line,
       n.is_test,n.language,n.generated,n.details,n.node_hash
FROM nodes_data n LEFT JOIN path_dictionary p ON p.path_id=n.path_id;

CREATE TRIGGER IF NOT EXISTS nodes_insert
INSTEAD OF INSERT ON nodes BEGIN
    INSERT OR IGNORE INTO path_dictionary(path) VALUES(NEW.path);
    INSERT INTO nodes_data VALUES(NEW.node_id,NEW.id,NEW.kind,NEW.name,NEW.qualname,
        (SELECT path_id FROM path_dictionary WHERE path=NEW.path),NEW.line,NEW.end_line,
        NEW.is_test,NEW.language,NEW.generated,NEW.details,NEW.node_hash);
END;

CREATE TRIGGER IF NOT EXISTS nodes_delete
INSTEAD OF DELETE ON nodes BEGIN
    DELETE FROM nodes_data WHERE node_id=OLD.node_id;
END;

CREATE TRIGGER IF NOT EXISTS nodes_update
INSTEAD OF UPDATE ON nodes BEGIN
    INSERT OR IGNORE INTO path_dictionary(path) VALUES(NEW.path);
    UPDATE nodes_data SET node_id=NEW.node_id,id=NEW.id,kind=NEW.kind,name=NEW.name,
        qualname=NEW.qualname,path_id=(SELECT path_id FROM path_dictionary WHERE path=NEW.path),
        line=NEW.line,end_line=NEW.end_line,is_test=NEW.is_test,language=NEW.language,
        generated=NEW.generated,details=NEW.details,node_hash=NEW.node_hash
    WHERE node_id=OLD.node_id;
END;

CREATE TABLE IF NOT EXISTS edges_raw (
    src_hash INTEGER NOT NULL,
    kind TEXT NOT NULL,
    dst_hash INTEGER NOT NULL,
    path_id INTEGER NOT NULL,
    line INTEGER NOT NULL,
    evidence NOT NULL,
    confidence NOT NULL,
    occurrence_count INTEGER NOT NULL,
    PRIMARY KEY (src_hash, kind, dst_hash)
) WITHOUT ROWID;

CREATE VIEW IF NOT EXISTS edges AS
SELECT s.id AS src_public_id, d.id AS dst_public_id, e.kind, p.path, e.line,
       CASE WHEN typeof(e.evidence)='integer' THEN ev.evidence ELSE e.evidence END AS evidence,
       CASE WHEN typeof(e.confidence)='integer' THEN cv.evidence ELSE e.confidence END AS confidence,
       e.occurrence_count
FROM edges_raw e
JOIN path_dictionary p ON p.path_id=e.path_id
JOIN nodes s ON s.node_hash=e.src_hash
JOIN nodes d ON d.node_hash=e.dst_hash
LEFT JOIN coverage_evidence ev ON typeof(e.evidence)='integer' AND ev.evidence_id=e.evidence
LEFT JOIN coverage_evidence cv ON typeof(e.confidence)='integer' AND cv.evidence_id=e.confidence;

CREATE TRIGGER IF NOT EXISTS edges_insert
INSTEAD OF INSERT ON edges BEGIN
    SELECT CASE
        WHEN NOT EXISTS(SELECT 1 FROM nodes WHERE id=NEW.src_public_id)
          OR NOT EXISTS(SELECT 1 FROM nodes WHERE id=NEW.dst_public_id)
        THEN RAISE(ABORT, 'edge endpoint not found')
    END;
    INSERT OR IGNORE INTO path_dictionary(path) VALUES(NEW.path);
    INSERT INTO edges_raw(src_hash,kind,dst_hash,path_id,line,evidence,confidence,occurrence_count)
    VALUES(
        (SELECT node_hash FROM nodes WHERE id=NEW.src_public_id), NEW.kind,
        (SELECT node_hash FROM nodes WHERE id=NEW.dst_public_id),
        (SELECT path_id FROM path_dictionary WHERE path=NEW.path), NEW.line,
        CASE WHEN typeof(NEW.evidence) IN('integer','real') THEN CAST(NEW.evidence AS TEXT) ELSE NEW.evidence END,
        CASE WHEN typeof(NEW.confidence) IN('integer','real') THEN CAST(NEW.confidence AS TEXT) ELSE NEW.confidence END,
        NEW.occurrence_count
    )
    ON CONFLICT(src_hash,kind,dst_hash) DO UPDATE
    SET occurrence_count=edges_raw.occurrence_count+excluded.occurrence_count;
END;

CREATE TABLE IF NOT EXISTS local_facts (
    path TEXT PRIMARY KEY,
    source_digest TEXT NOT NULL,
    language TEXT NOT NULL,
    is_test INTEGER NOT NULL,
    generated INTEGER NOT NULL,
    facts_blob BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS node_owner_overrides (
    public_id TEXT PRIMARY KEY,
    owner TEXT NOT NULL
);

CREATE VIEW IF NOT EXISTS owned_nodes AS
SELECT n.path AS owner, n.id AS public_id, n.kind, n.name, n.qualname, n.path,
       n.line, n.end_line, n.is_test, n.language, n.generated, n.details AS details_json
FROM nodes n
WHERE NOT EXISTS(SELECT 1 FROM node_owner_overrides o WHERE o.public_id=n.id)
UNION ALL
SELECT o.owner, n.id, n.kind, n.name, n.qualname, n.path, n.line, n.end_line,
       n.is_test, n.language, n.generated, n.details
FROM node_owner_overrides o JOIN nodes n ON n.id=o.public_id;

CREATE TABLE IF NOT EXISTS edge_occurrences_raw (
    owner_id INTEGER NOT NULL,
    ordinal INTEGER NOT NULL,
    src_hash INTEGER NOT NULL,
    dst_hash INTEGER NOT NULL,
    kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    evidence NOT NULL,
    confidence NOT NULL,
    PRIMARY KEY (owner_id, ordinal)
) WITHOUT ROWID;

CREATE VIEW IF NOT EXISTS edge_occurrences AS
SELECT owner.path AS owner,e.ordinal,s.id AS src,d.id AS dst,e.kind,
        owner.path AS path,e.line,
        CASE WHEN typeof(e.evidence)='integer' THEN ev.evidence ELSE e.evidence END AS evidence,
        CASE WHEN typeof(e.confidence)='integer' THEN cv.evidence ELSE e.confidence END AS confidence
FROM edge_occurrences_raw e
JOIN path_dictionary owner ON owner.path_id=e.owner_id
JOIN nodes s ON s.node_hash=e.src_hash
JOIN nodes d ON d.node_hash=e.dst_hash
LEFT JOIN coverage_evidence ev ON typeof(e.evidence)='integer' AND ev.evidence_id=e.evidence
LEFT JOIN coverage_evidence cv ON typeof(e.confidence)='integer' AND cv.evidence_id=e.confidence;

CREATE TABLE IF NOT EXISTS owned_search_raw (
    node_id INTEGER PRIMARY KEY,
    search_text TEXT NOT NULL
);

CREATE VIEW IF NOT EXISTS owned_search AS
SELECT o.owner,o.public_id,0 AS ordinal,
       coalesce(s.search_text,n.name || ' ' || n.qualname || ' ' || n.path ||
           CASE WHEN json_valid(n.details) THEN
               CASE WHEN json_type(n.details,'$.doc')='text' AND json_extract(n.details,'$.doc')!=''
               THEN ' ' || json_extract(n.details,'$.doc') ELSE '' END
           ELSE '' END) AS search_text
FROM owned_nodes o
JOIN nodes n ON n.id=o.public_id
LEFT JOIN owned_search_raw s ON s.node_id=n.node_id;

CREATE TABLE IF NOT EXISTS dependencies_raw (
    owner_id INTEGER NOT NULL,
    ordinal INTEGER NOT NULL,
    target_hash INTEGER,
    kind TEXT NOT NULL,
    symbol TEXT NOT NULL,
    resolution TEXT NOT NULL,
    PRIMARY KEY (owner_id, ordinal)
) WITHOUT ROWID;

CREATE VIEW IF NOT EXISTS dependencies AS
SELECT owner.path AS owner,f.path AS target,d.kind,d.symbol,d.resolution
FROM dependencies_raw d
JOIN path_dictionary owner ON owner.path_id=d.owner_id
LEFT JOIN files f ON f.path_hash=d.target_hash;

CREATE TABLE IF NOT EXISTS shared_keys_raw (
    key_hash INTEGER PRIMARY KEY,
    key NOT NULL
) WITHOUT ROWID;

CREATE VIEW IF NOT EXISTS shared_keys AS
SELECT k.key_hash,CASE WHEN typeof(k.key)='integer' THEN n.qualname ELSE k.key END AS key
FROM shared_keys_raw k LEFT JOIN nodes_data n ON typeof(k.key)='integer' AND n.node_id=k.key;

CREATE TRIGGER IF NOT EXISTS shared_keys_insert
INSTEAD OF INSERT ON shared_keys BEGIN
    INSERT INTO shared_keys_raw VALUES(NEW.key_hash,
        CASE WHEN typeof(NEW.key) IN('integer','real') THEN CAST(NEW.key AS TEXT) ELSE NEW.key END);
END;

CREATE TRIGGER IF NOT EXISTS shared_keys_delete
INSTEAD OF DELETE ON shared_keys BEGIN
    DELETE FROM shared_keys_raw WHERE key_hash=OLD.key_hash;
END;

CREATE TRIGGER IF NOT EXISTS shared_keys_update
INSTEAD OF UPDATE ON shared_keys BEGIN
    UPDATE shared_keys_raw SET key_hash=NEW.key_hash,
        key=CASE WHEN typeof(NEW.key) IN('integer','real') THEN CAST(NEW.key AS TEXT) ELSE NEW.key END
    WHERE key_hash=OLD.key_hash;
END;

CREATE TRIGGER IF NOT EXISTS nodes_materialize_keys_delete
BEFORE DELETE ON nodes_data BEGIN
    UPDATE shared_keys_raw SET key=OLD.qualname WHERE typeof(key)='integer' AND key=OLD.node_id;
END;

CREATE TRIGGER IF NOT EXISTS nodes_materialize_keys_update
BEFORE UPDATE OF node_id,qualname ON nodes_data
WHEN NEW.node_id!=OLD.node_id OR NEW.qualname!=OLD.qualname BEGIN
    UPDATE shared_keys_raw SET key=OLD.qualname WHERE typeof(key)='integer' AND key=OLD.node_id;
END;

CREATE TABLE IF NOT EXISTS shared_owners_raw (
    kind_id INTEGER NOT NULL CHECK(kind_id BETWEEN 0 AND 2),
    key_hash INTEGER NOT NULL,
    owner_id INTEGER NOT NULL,
    ordinal INTEGER NOT NULL,
    PRIMARY KEY (owner_id, kind_id, key_hash, ordinal)
) WITHOUT ROWID;

CREATE VIEW IF NOT EXISTS shared_owners AS
SELECT CASE s.kind_id WHEN 0 THEN 'symbol' WHEN 1 THEN 'reference' ELSE 'default_export' END AS kind,
       k.key,owner.path AS owner,s.ordinal
FROM shared_owners_raw s
JOIN shared_keys k ON k.key_hash=s.key_hash
JOIN path_dictionary owner ON owner.path_id=s.owner_id;

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

CREATE TABLE IF NOT EXISTS errors (
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    message TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS coverage_evidence (
    evidence_id INTEGER PRIMARY KEY,
    evidence TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS resolution_coverage_data (
    path_id INTEGER NOT NULL,
    line INTEGER NOT NULL,
    expression_id INTEGER NOT NULL,
    status TEXT NOT NULL,
    evidence_id INTEGER NOT NULL,
    PRIMARY KEY (path_id, line, expression_id, status, evidence_id)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS coverage_expressions (
    expression_id INTEGER PRIMARY KEY,
    expression TEXT NOT NULL UNIQUE
);

CREATE VIEW IF NOT EXISTS resolution_coverage AS
SELECT p.path,c.line,x.expression,c.status,e.evidence
FROM resolution_coverage_data c
JOIN path_dictionary p ON p.path_id=c.path_id
JOIN coverage_expressions x ON x.expression_id=c.expression_id
JOIN coverage_evidence e ON e.evidence_id=c.evidence_id;

CREATE TRIGGER IF NOT EXISTS resolution_coverage_insert
INSTEAD OF INSERT ON resolution_coverage BEGIN
    INSERT OR IGNORE INTO path_dictionary(path) VALUES(NEW.path);
    INSERT OR IGNORE INTO coverage_expressions(expression) VALUES(NEW.expression);
    INSERT OR IGNORE INTO coverage_evidence(evidence) VALUES(NEW.evidence);
    INSERT OR IGNORE INTO resolution_coverage_data(path_id,line,expression_id,status,evidence_id)
    VALUES((SELECT path_id FROM path_dictionary WHERE path=NEW.path),NEW.line,
        (SELECT expression_id FROM coverage_expressions WHERE expression=NEW.expression),NEW.status,
        (SELECT evidence_id FROM coverage_evidence WHERE evidence=NEW.evidence));
END;

CREATE TRIGGER IF NOT EXISTS resolution_coverage_delete
INSTEAD OF DELETE ON resolution_coverage BEGIN
    DELETE FROM resolution_coverage_data
    WHERE path_id=(SELECT path_id FROM path_dictionary WHERE path=OLD.path)
      AND line=OLD.line
      AND expression_id=(SELECT expression_id FROM coverage_expressions WHERE expression=OLD.expression)
      AND status=OLD.status
      AND evidence_id=(SELECT evidence_id FROM coverage_evidence WHERE evidence=OLD.evidence);
END;

CREATE TABLE IF NOT EXISTS domain_commitments_raw (
    domain TEXT NOT NULL,
    owner_id INTEGER NOT NULL,
    digest BLOB NOT NULL,
    PRIMARY KEY(domain,owner_id)
) WITHOUT ROWID;

CREATE VIEW IF NOT EXISTS domain_commitments AS
SELECT CASE WHEN d.owner_id=0 THEN d.domain ELSE 'leaf:' || json_array(d.domain,p.path) END AS domain,
       CASE WHEN typeof(d.digest)='blob' THEN lower(hex(d.digest)) ELSE d.digest END AS digest
FROM domain_commitments_raw d LEFT JOIN path_dictionary p ON p.path_id=d.owner_id;

CREATE TRIGGER IF NOT EXISTS domain_commitments_insert
INSTEAD OF INSERT ON domain_commitments BEGIN
    INSERT OR IGNORE INTO path_dictionary(path)
    SELECT json_extract(substr(NEW.domain,6),'$[1]') WHERE NEW.domain LIKE 'leaf:%';
    INSERT OR REPLACE INTO domain_commitments_raw VALUES(
        CASE WHEN NEW.domain LIKE 'leaf:%' THEN json_extract(substr(NEW.domain,6),'$[0]') ELSE NEW.domain END,
        CASE WHEN NEW.domain LIKE 'leaf:%' THEN (SELECT path_id FROM path_dictionary
            WHERE path=json_extract(substr(NEW.domain,6),'$[1]')) ELSE 0 END,
        coalesce(unhex(NEW.digest),NEW.digest));
END;

CREATE TRIGGER IF NOT EXISTS domain_commitments_delete
INSTEAD OF DELETE ON domain_commitments BEGIN
    DELETE FROM domain_commitments_raw
    WHERE domain=CASE WHEN OLD.domain LIKE 'leaf:%' THEN json_extract(substr(OLD.domain,6),'$[0]') ELSE OLD.domain END
      AND owner_id=CASE WHEN OLD.domain LIKE 'leaf:%' THEN (SELECT path_id FROM path_dictionary
          WHERE path=json_extract(substr(OLD.domain,6),'$[1]')) ELSE 0 END;
END;

CREATE TRIGGER IF NOT EXISTS domain_commitments_update
INSTEAD OF UPDATE OF digest ON domain_commitments BEGIN
    UPDATE domain_commitments_raw SET digest=coalesce(unhex(NEW.digest),NEW.digest)
    WHERE domain=CASE WHEN OLD.domain LIKE 'leaf:%' THEN json_extract(substr(OLD.domain,6),'$[0]') ELSE OLD.domain END
      AND owner_id=CASE WHEN OLD.domain LIKE 'leaf:%' THEN (SELECT path_id FROM path_dictionary
          WHERE path=json_extract(substr(OLD.domain,6),'$[1]')) ELSE 0 END;
END;

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

-- Essential Performance Indexes
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes_data(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes_data(name);
CREATE INDEX IF NOT EXISTS idx_nodes_qualname ON nodes_data(qualname);
CREATE INDEX IF NOT EXISTS idx_nodes_path ON nodes_data(path_id);
CREATE INDEX IF NOT EXISTS idx_node_owner_overrides_owner ON node_owner_overrides(owner);
CREATE INDEX IF NOT EXISTS idx_edges_dst_kind ON edges_raw(dst_hash, kind);
CREATE INDEX IF NOT EXISTS idx_edges_raw_path ON edges_raw(path_id);
CREATE INDEX IF NOT EXISTS idx_doc_sections_path ON doc_sections(path);
CREATE INDEX IF NOT EXISTS idx_doc_sections_type ON doc_sections(doc_type);
CREATE INDEX IF NOT EXISTS idx_occurrences_pair ON edge_occurrences_raw(src_hash,dst_hash);
CREATE INDEX IF NOT EXISTS idx_dependencies_target ON dependencies_raw(target_hash, kind) WHERE target_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_coverage_status ON resolution_coverage_data(status) WHERE status IN('unresolved','ambiguous');
CREATE INDEX IF NOT EXISTS idx_coverage_external ON resolution_coverage_data(path_id) WHERE status='external';
CREATE INDEX IF NOT EXISTS idx_errors_owner ON errors(path);
CREATE INDEX IF NOT EXISTS idx_shared_owners_key ON shared_owners_raw(kind_id,key_hash) WHERE kind_id=1 OR kind_id=2;
CREATE INDEX IF NOT EXISTS idx_shared_keys_node ON shared_keys_raw(key) WHERE typeof(key)='integer';
"#;
