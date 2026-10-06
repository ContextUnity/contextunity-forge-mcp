use super::{encode_int, encode_text, key, new_leaf_hasher};
use anyhow::{Context, Result};
use hashbrown::{HashMap, HashSet};
use rayon::prelude::*;
use rusqlite::{types::ValueRef, Connection};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};

fn coverage_dictionary(
    conn: &Connection,
    sql: &str,
    encoded: Option<&str>,
) -> Result<Option<HashMap<i64, String>>> {
    let mut statement = conn.prepare_cached(sql)?;
    let mut rows = if let Some(encoded) = encoded {
        statement.query([encoded])?
    } else {
        statement.query([])?
    };
    let mut dictionary = HashMap::new();
    while let Some(row) = rows.next()? {
        let (ValueRef::Integer(id), ValueRef::Text(text)) = (row.get_ref(0)?, row.get_ref(1)?)
        else {
            return Ok(None);
        };
        let Ok(text) = std::str::from_utf8(text) else {
            return Ok(None);
        };
        dictionary.insert(id, text.to_owned());
    }
    Ok(Some(dictionary))
}

fn owner_language_expression_dictionary(
    conn: &Connection,
    filter: &str,
    encoded: Option<&str>,
) -> Result<Option<HashMap<i64, String>>> {
    let mut statement = conn.prepare(&format!(
        "SELECT DISTINCT expression_id FROM coverage_owner_language {filter}"
    ))?;
    let mut rows = if let Some(encoded) = encoded {
        statement.query([encoded])?
    } else {
        statement.query([])?
    };
    let mut expression_ids = HashSet::new();
    while let Some(row) = rows.next()? {
        let ValueRef::Integer(id) = row.get_ref(0)? else {
            return Ok(None);
        };
        expression_ids.insert(id);
    }
    drop(rows);
    drop(statement);

    let mut expression_ids: Vec<_> = expression_ids.into_iter().collect();
    expression_ids.sort_unstable();
    let mut dictionary = HashMap::with_capacity(expression_ids.len());
    for ids in expression_ids.chunks(900) {
        let mut placeholders = String::with_capacity(ids.len() * 2);
        for index in 0..ids.len() {
            if index > 0 {
                placeholders.push(',');
            }
            placeholders.push('?');
        }
        let sql = format!(
            "SELECT expression_id,expression FROM coverage_expressions WHERE expression_id IN ({placeholders})"
        );
        let mut statement = conn.prepare_cached(&sql)?;
        let mut rows = statement.query(rusqlite::params_from_iter(ids.iter()))?;
        while let Some(row) = rows.next()? {
            let (ValueRef::Integer(id), ValueRef::Text(value)) = (row.get_ref(0)?, row.get_ref(1)?)
            else {
                return Ok(None);
            };
            let Ok(value) = std::str::from_utf8(value) else {
                return Ok(None);
            };
            dictionary.insert(id, value.to_owned());
        }
    }
    Ok(Some(dictionary))
}

pub(super) fn canonical_coverage_leaves(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<Option<BTreeMap<String, String>>> {
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let encoded = encoded.as_deref();
    let filter = if owners.is_some() {
        "WHERE path_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let path_sql = if owners.is_some() {
        "SELECT path_id,path FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1))"
    } else {
        "SELECT path_id,path FROM path_dictionary"
    };
    let expression_sql = if owners.is_some() {
        format!("SELECT expression_id,expression FROM coverage_expressions WHERE expression_id IN(SELECT expression_id FROM resolution_coverage {filter})")
    } else {
        "SELECT expression_id,expression FROM coverage_expressions".to_owned()
    };
    let evidence_sql = if owners.is_some() {
        format!("SELECT evidence_id,evidence FROM coverage_evidence WHERE evidence_id IN(SELECT evidence_id FROM resolution_coverage {filter})")
    } else {
        "SELECT evidence_id,evidence FROM coverage_evidence".to_owned()
    };
    let Some(paths) = coverage_dictionary(conn, path_sql, encoded)? else {
        return Ok(None);
    };
    let Some(expressions) = coverage_dictionary(conn, &expression_sql, encoded)? else {
        return Ok(None);
    };
    let Some(evidence) = coverage_dictionary(conn, &evidence_sql, encoded)? else {
        return Ok(None);
    };
    let mut statuses = HashSet::new();
    {
        let mut statement = conn.prepare_cached(&format!(
            "SELECT DISTINCT status FROM resolution_coverage {filter}"
        ))?;
        let mut rows = if let Some(encoded) = encoded {
            statement.query([encoded])?
        } else {
            statement.query([])?
        };
        while let Some(row) = rows.next()? {
            let ValueRef::Text(status) = row.get_ref(0)? else {
                return Ok(None);
            };
            let Ok(status) = std::str::from_utf8(status) else {
                return Ok(None);
            };
            statuses.insert(status.to_owned());
        }
    }
    let mut statement = conn.prepare(&format!(
        "SELECT path_id,line,expression_id,status,evidence_id FROM resolution_coverage {filter}"
    ))?;
    let mut rows = if let Some(encoded) = encoded {
        statement.query([encoded])?
    } else {
        statement.query([])?
    };
    struct CoverageRow<'a> {
        line: i64,
        expression: &'a str,
        status: &'a str,
        evidence: &'a str,
    }
    let mut by_owner: HashMap<&str, Vec<CoverageRow<'_>>> = HashMap::new();
    let mut accepted = 0;
    while let Some(row) = rows.next()? {
        let (
            ValueRef::Integer(path),
            ValueRef::Integer(line),
            ValueRef::Integer(expression),
            ValueRef::Text(status),
            ValueRef::Integer(proof),
        ) = (
            row.get_ref(0)?,
            row.get_ref(1)?,
            row.get_ref(2)?,
            row.get_ref(3)?,
            row.get_ref(4)?,
        )
        else {
            return Ok(None);
        };
        let owner = paths
            .get(&path)
            .with_context(|| format!("resolution_coverage references missing owner path {path}"))?;
        let expression = expressions.get(&expression).with_context(|| {
            format!("resolution_coverage references missing expression {expression}")
        })?;
        let proof = evidence
            .get(&proof)
            .with_context(|| format!("resolution_coverage references missing evidence {proof}"))?;
        let Ok(status) = std::str::from_utf8(status) else {
            return Ok(None);
        };
        let Some(status) = statuses.get(status) else {
            return Ok(None);
        };
        by_owner
            .entry(owner.as_str())
            .or_default()
            .push(CoverageRow {
                line,
                expression: expression.as_str(),
                status: status.as_str(),
                evidence: proof.as_str(),
            });
        accepted += 1;
    }
    let hash_owner = |(owner, mut rows): (&str, Vec<CoverageRow<'_>>)| {
        rows.sort_unstable_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then_with(|| left.expression.as_bytes().cmp(right.expression.as_bytes()))
                .then_with(|| left.status.as_bytes().cmp(right.status.as_bytes()))
                .then_with(|| left.evidence.as_bytes().cmp(right.evidence.as_bytes()))
        });
        let mut hasher = new_leaf_hasher("resolution_coverage", owner);
        for row in rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, owner);
            encode_int(&mut hasher, row.line);
            encode_text(&mut hasher, row.expression);
            encode_text(&mut hasher, row.status);
            encode_text(&mut hasher, row.evidence);
        }
        (
            key("resolution_coverage", owner),
            hex::encode(hasher.finalize()),
        )
    };
    let leaves = if accepted >= 8192 {
        by_owner.into_par_iter().map(hash_owner).collect()
    } else {
        by_owner.into_iter().map(hash_owner).collect()
    };
    Ok(Some(leaves))
}

pub(super) fn canonical_owner_language_leaves(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<Option<BTreeMap<String, String>>> {
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let encoded = encoded.as_deref();
    let filter = if owners.is_some() {
        "WHERE path_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let path_sql = format!(
        "SELECT path_id,path FROM path_dictionary WHERE path_id IN(SELECT path_id FROM coverage_owner_language {filter})"
    );
    let evidence_sql = format!(
        "SELECT evidence_id,evidence FROM coverage_evidence WHERE evidence_id IN(SELECT evidence_id FROM coverage_owner_language {filter})"
    );
    let Some(paths) = coverage_dictionary(conn, &path_sql, encoded)? else {
        return Ok(None);
    };
    let Some(expressions) = owner_language_expression_dictionary(conn, filter, encoded)? else {
        return Ok(None);
    };
    let Some(evidence) = coverage_dictionary(conn, &evidence_sql, encoded)? else {
        return Ok(None);
    };
    let mut statement = conn.prepare(&format!(
        "SELECT path_id,line,expression_id,status,evidence_id,language FROM coverage_owner_language {filter}"
    ))?;
    let mut rows = if let Some(encoded) = encoded {
        statement.query([encoded])?
    } else {
        statement.query([])?
    };
    struct OwnerLanguageRow<'a> {
        line: i64,
        expression: &'a str,
        status: String,
        evidence: &'a str,
        language: String,
    }
    let mut by_owner: HashMap<&str, Vec<OwnerLanguageRow<'_>>> = HashMap::new();
    while let Some(row) = rows.next()? {
        let (
            ValueRef::Integer(path),
            ValueRef::Integer(line),
            ValueRef::Integer(expression),
            ValueRef::Text(status),
            ValueRef::Integer(proof),
            ValueRef::Text(language),
        ) = (
            row.get_ref(0)?,
            row.get_ref(1)?,
            row.get_ref(2)?,
            row.get_ref(3)?,
            row.get_ref(4)?,
            row.get_ref(5)?,
        )
        else {
            return Ok(None);
        };
        let (Some(owner), Some(expression), Some(proof)) = (
            paths.get(&path),
            expressions.get(&expression),
            evidence.get(&proof),
        ) else {
            return Ok(None);
        };
        let (Ok(status), Ok(language)) =
            (std::str::from_utf8(status), std::str::from_utf8(language))
        else {
            return Ok(None);
        };
        by_owner
            .entry(owner.as_str())
            .or_default()
            .push(OwnerLanguageRow {
                line,
                expression: expression.as_str(),
                status: status.to_owned(),
                evidence: proof.as_str(),
                language: language.to_owned(),
            });
    }
    let hash_owner = |(owner, mut rows): (&str, Vec<OwnerLanguageRow<'_>>)| {
        rows.sort_unstable_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then_with(|| left.expression.as_bytes().cmp(right.expression.as_bytes()))
                .then_with(|| left.status.as_bytes().cmp(right.status.as_bytes()))
                .then_with(|| left.evidence.as_bytes().cmp(right.evidence.as_bytes()))
                .then_with(|| left.language.as_bytes().cmp(right.language.as_bytes()))
        });
        let mut hasher = new_leaf_hasher("coverage_owner_language", owner);
        for row in rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, owner);
            encode_int(&mut hasher, row.line);
            encode_text(&mut hasher, row.expression);
            encode_text(&mut hasher, &row.status);
            encode_text(&mut hasher, row.evidence);
            encode_text(&mut hasher, &row.language);
        }
        (
            key("coverage_owner_language", owner),
            hex::encode(hasher.finalize()),
        )
    };
    let leaves = if by_owner.len() >= 8192 {
        by_owner.into_par_iter().map(hash_owner).collect()
    } else {
        by_owner.into_iter().map(hash_owner).collect()
    };
    Ok(Some(leaves))
}

pub(super) fn canonical_language_count_leaves(
    conn: &Connection,
    owners: Option<&BTreeSet<String>>,
) -> Result<Option<BTreeMap<String, String>>> {
    let encoded = owners.map(serde_json::to_string).transpose()?;
    let encoded = encoded.as_deref();
    let filter = if owners.is_some() {
        "WHERE path_id IN(SELECT path_id FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1)))"
    } else {
        ""
    };
    let path_sql = if owners.is_some() {
        "SELECT path_id,path FROM path_dictionary WHERE path IN(SELECT value FROM json_each(?1))"
    } else {
        "SELECT path_id,path FROM path_dictionary"
    };
    let Some(paths) = coverage_dictionary(conn, path_sql, encoded)? else {
        return Ok(None);
    };
    let mut statement = conn.prepare(&format!(
        "SELECT path_id,language,status,records FROM coverage_language_counts {filter}"
    ))?;
    let mut rows = if let Some(encoded) = encoded {
        statement.query([encoded])?
    } else {
        statement.query([])?
    };
    struct CountRow {
        language: String,
        status: String,
        records: i64,
    }
    let mut by_owner: HashMap<&str, Vec<CountRow>> = HashMap::new();
    while let Some(row) = rows.next()? {
        let (
            ValueRef::Integer(path),
            ValueRef::Text(language),
            ValueRef::Text(status),
            ValueRef::Integer(records),
        ) = (
            row.get_ref(0)?,
            row.get_ref(1)?,
            row.get_ref(2)?,
            row.get_ref(3)?,
        )
        else {
            return Ok(None);
        };
        let Some(owner) = paths.get(&path) else {
            return Ok(None);
        };
        let (Ok(language), Ok(status)) =
            (std::str::from_utf8(language), std::str::from_utf8(status))
        else {
            return Ok(None);
        };
        by_owner.entry(owner.as_str()).or_default().push(CountRow {
            language: language.to_owned(),
            status: status.to_owned(),
            records,
        });
    }
    let hash_owner = |(owner, mut rows): (&str, Vec<CountRow>)| {
        rows.sort_unstable_by(|left, right| {
            left.language
                .as_bytes()
                .cmp(right.language.as_bytes())
                .then_with(|| left.status.as_bytes().cmp(right.status.as_bytes()))
                .then_with(|| left.records.cmp(&right.records))
        });
        let mut hasher = new_leaf_hasher("coverage_language_counts", owner);
        for row in rows {
            hasher.update([0xff]);
            encode_text(&mut hasher, owner);
            encode_text(&mut hasher, &row.language);
            encode_text(&mut hasher, &row.status);
            encode_int(&mut hasher, row.records);
        }
        (
            key("coverage_language_counts", owner),
            hex::encode(hasher.finalize()),
        )
    };
    let leaves = if by_owner.len() >= 8192 {
        by_owner.into_par_iter().map(hash_owner).collect()
    } else {
        by_owner.into_iter().map(hash_owner).collect()
    };
    Ok(Some(leaves))
}
