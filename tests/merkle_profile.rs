use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const BUCKETS: usize = 4096;
type Hash = [u8; 32];
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Result<Self> {
        let p = std::env::temp_dir().join(format!(
            "forge_merkle_{}_{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&p)?;
        Ok(Self(p))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn bucket(key: &str) -> usize {
    let h = Sha256::digest(key.as_bytes());
    ((h[0] as usize) << 4) | ((h[1] as usize) >> 4)
}
fn branch(a: Hash, b: Hash) -> Hash {
    let mut h = Sha256::new();
    h.update(b"branch-v1");
    h.update(a);
    h.update(b);
    h.finalize().into()
}
fn bucket_hash(index: usize, values: impl IntoIterator<Item = (String, String)>) -> Hash {
    let mut h = Sha256::new();
    h.update(b"bucket-v1");
    h.update((index as u64).to_le_bytes());
    for (key, digest) in values {
        h.update((key.len() as u64).to_le_bytes());
        h.update(key);
        h.update((digest.len() as u64).to_le_bytes());
        h.update(digest);
    }
    h.finalize().into()
}
fn read_bucket(c: &Connection, b: usize) -> Result<Hash> {
    let mut s = c.prepare_cached(
        "SELECT domain,digest FROM domain_commitments WHERE bucket=?1 ORDER BY domain",
    )?;
    let rows = s
        .query_map([b], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
    Ok(bucket_hash(b, rows))
}
fn node(c: &Connection, id: usize) -> Result<Hash> {
    let v: Vec<u8> = c
        .prepare_cached("SELECT digest FROM tree WHERE id=?1")?
        .query_row([id], |r| r.get(0))?;
    v.try_into()
        .map_err(|_| anyhow::anyhow!("invalid hash length"))
}
fn save_node(c: &Connection, id: usize, hash: Hash) -> Result<()> {
    c.prepare_cached("INSERT OR REPLACE INTO tree VALUES(?1,?2)")?
        .execute(params![id, hash.as_slice()])?;
    Ok(())
}
fn prove(c: &Connection, b: usize, expected: Hash) -> Result<()> {
    let mut h = read_bucket(c, b)?;
    let mut id = BUCKETS + b;
    ensure!(h == node(c, id)?, "bucket mismatch");
    while id > 1 {
        let sibling = node(c, id ^ 1)?;
        h = if id.is_multiple_of(2) {
            branch(h, sibling)
        } else {
            branch(sibling, h)
        };
        id /= 2;
    }
    ensure!(h == expected, "root proof mismatch");
    Ok(())
}
fn reconstruct(c: &Connection) -> Result<Vec<Hash>> {
    let mut buckets = vec![Vec::new(); BUCKETS];
    let mut s = c.prepare("SELECT domain,digest FROM domain_commitments ORDER BY domain")?;
    for r in s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = r?;
        buckets[bucket(&k)].push((k, v));
    }
    let mut hashes = vec![[0; 32]; BUCKETS * 2];
    for (i, values) in buckets.into_iter().enumerate() {
        hashes[BUCKETS + i] = bucket_hash(i, values);
    }
    for i in (1..BUCKETS).rev() {
        hashes[i] = branch(hashes[i * 2], hashes[i * 2 + 1]);
    }
    Ok(hashes)
}
fn flat(c: &Connection) -> Result<Vec<Hash>> {
    let mut out = Vec::new();
    for table in contextunity_forge_mcp::core::commitments::DOMAINS {
        let prefix = format!("leaf:[\"{table}\",");
        let mut s=c.prepare_cached("SELECT domain,digest FROM domain_commitments WHERE domain>=?1 AND domain<?2 ORDER BY domain")?;
        let values = s
            .query_map(params![prefix, format!("{prefix}\u{10ffff}")], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
        let mut h = Sha256::new();
        h.update(contextunity_forge_mcp::core::commitments::ALGORITHM);
        h.update(table);
        for (k, v) in values {
            h.update((k.len() as u64).to_le_bytes());
            h.update(k);
            h.update(v);
        }
        out.push(h.finalize().into());
    }
    Ok(out)
}
fn database(
    w: &Workspace,
    name: &str,
    values: &[(String, String)],
    tree: bool,
) -> Result<Connection> {
    let mut c = Connection::open(w.0.join(name))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA cache_size=-64000; CREATE TABLE domain_commitments(domain TEXT PRIMARY KEY,digest TEXT NOT NULL,bucket INTEGER NOT NULL);")?;
    let tx = c.transaction()?;
    for (k, v) in values {
        tx.prepare_cached("INSERT INTO domain_commitments VALUES(?1,?2,?3)")?
            .execute(params![k, v, bucket(k)])?;
    }
    tx.commit()?;
    if tree {
        c.execute_batch("CREATE INDEX by_bucket ON domain_commitments(bucket,domain);CREATE TABLE tree(id INTEGER PRIMARY KEY,digest BLOB NOT NULL);")?;
        let hashes = reconstruct(&c)?;
        let tx = c.transaction()?;
        for (i, h) in hashes.into_iter().enumerate().skip(1) {
            save_node(&tx, i, h)?;
        }
        tx.commit()?;
    } else {
        c.execute_batch("CREATE TABLE flat_roots(id INTEGER PRIMARY KEY,digest BLOB NOT NULL)")?;
    }
    c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    Ok(c)
}
fn size(c: &Connection) -> Result<i64> {
    Ok(c.query_row("SELECT (SELECT page_count FROM pragma_page_count)*(SELECT page_size FROM pragma_page_size)",[],|r|r.get(0))?)
}

#[test]
#[ignore = "set FORGE_MERKLE_DB to a read-only source index; measures a prototype, not production verification"]
fn merkle_owner_aggregate_profile() -> Result<()> {
    let source = std::env::var("FORGE_MERKLE_DB")?;
    let c = Connection::open_with_flags(
        &source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    c.execute_batch("PRAGMA query_only=ON;BEGIN")?;
    let values=c.prepare("SELECT domain,digest FROM domain_commitments WHERE domain LIKE 'leaf:%' ORDER BY domain")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    c.execute_batch("COMMIT")?;
    drop(c);
    ensure!(!values.is_empty(), "no owner leaves");
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, _) in &values {
        let parsed: Vec<String> = serde_json::from_str(
            k.strip_prefix("leaf:")
                .ok_or_else(|| anyhow::anyhow!("invalid key"))?,
        )?;
        ensure!(parsed.len() == 2, "invalid leaf tuple");
        owners.entry(parsed[1].clone()).or_default().push(k.clone());
    }
    let w = Workspace::new()?;
    let started = Instant::now();
    let mut baseline = database(&w, "flat.sqlite", &values, false)?;
    let flat_build = started.elapsed().as_secs_f64() * 1000.;
    let started = Instant::now();
    let mut candidate = database(&w, "tree.sqlite", &values, true)?;
    let tree_build = started.elapsed().as_secs_f64() * 1000.;
    let mut occupancy = vec![0usize; BUCKETS];
    for (k, _) in &values {
        occupancy[bucket(k)] += 1;
    }
    println!(
        "MERKLE_SETUP {}",
        serde_json::json!({"domains":contextunity_forge_mcp::core::commitments::DOMAINS.len(),"leaves":values.len(),"owners":owners.len(),"flat_build_ms":flat_build,"tree_build_ms":tree_build,"flat_bytes":size(&baseline)?,"tree_bytes":size(&candidate)?,"tree_hash_array_bytes":BUCKETS*2*32,"max_bucket_leaves":occupancy.iter().max(),"input_string_bytes":values.iter().map(|(k,v)|k.len()+v.len()).sum::<usize>(),"source":source})
    );
    let owner_keys: Vec<_> = owners.keys().collect();
    let mut trusted_root = node(&candidate, 1)?;
    for count in [1, 34, 100] {
        ensure!(owner_keys.len() >= count, "insufficient owners");
        let mut selected = BTreeSet::new();
        for i in 0..count {
            selected.extend(
                owners[owner_keys[i * owner_keys.len() / count]]
                    .iter()
                    .cloned(),
            );
        }
        let buckets: BTreeSet<_> = selected.iter().map(|k| bucket(k)).collect();
        for round in 0..7 {
            let changed: Vec<_> = selected
                .iter()
                .map(|k| {
                    (
                        k.clone(),
                        hex::encode(Sha256::digest(format!("{k}:{count}:{round}").as_bytes())),
                    )
                })
                .collect();
            let mut elapsed = [0.; 2];
            for candidate_first in if round % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                if candidate_first {
                    let root = trusted_root;
                    let start = Instant::now();
                    let tx = candidate
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    for b in &buckets {
                        prove(&tx, *b, root)?;
                    }
                    for (k, v) in &changed {
                        tx.prepare_cached(
                            "UPDATE domain_commitments SET digest=?2 WHERE domain=?1",
                        )?
                        .execute(params![k, v])?;
                    }
                    let mut level = BTreeSet::new();
                    for b in &buckets {
                        save_node(&tx, BUCKETS + b, read_bucket(&tx, *b)?)?;
                        level.insert((BUCKETS + b) / 2);
                    }
                    while !level.is_empty() {
                        let mut next = BTreeSet::new();
                        for id in level {
                            save_node(&tx, id, branch(node(&tx, id * 2)?, node(&tx, id * 2 + 1)?))?;
                            if id > 1 {
                                next.insert(id / 2);
                            }
                        }
                        level = next;
                    }
                    let new_root = node(&tx, 1)?;
                    for b in &buckets {
                        prove(&tx, *b, new_root)?;
                    }
                    tx.commit()?;
                    candidate.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
                    elapsed[1] = start.elapsed().as_secs_f64() * 1000.;
                    ensure!(
                        new_root == reconstruct(&candidate)?[1],
                        "independent full reconstruction mismatch"
                    );
                    trusted_root = new_root;
                } else {
                    let start = Instant::now();
                    let tx = baseline
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    for (k, v) in &changed {
                        tx.prepare_cached(
                            "UPDATE domain_commitments SET digest=?2 WHERE domain=?1",
                        )?
                        .execute(params![k, v])?;
                    }
                    let sealed = flat(&tx)?;
                    for (i, h) in sealed.iter().enumerate() {
                        tx.prepare_cached("INSERT OR REPLACE INTO flat_roots VALUES(?1,?2)")?
                            .execute(params![i, h.as_slice()])?;
                    }
                    let verified = flat(&tx)?;
                    for (i, h) in verified.iter().enumerate() {
                        let stored: Vec<u8> =
                            tx.query_row("SELECT digest FROM flat_roots WHERE id=?1", [i], |r| {
                                r.get(0)
                            })?;
                        ensure!(stored.as_slice() == h, "flat verify mismatch");
                    }
                    tx.commit()?;
                    baseline.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
                    elapsed[0] = start.elapsed().as_secs_f64() * 1000.;
                }
            }
            println!(
                "MERKLE_RUN {}",
                serde_json::json!({"owners":count,"changed_leaves":selected.len(),"buckets":buckets.len(),"round":round,"warmup":round==0,"flat_ms":elapsed[0],"tree_ms":elapsed[1]})
            );
        }
    }
    let root = node(&candidate, 1)?;
    let key = &values[0].0;
    candidate.execute(
        "UPDATE domain_commitments SET digest='tampered' WHERE domain=?1",
        [key],
    )?;
    ensure!(
        prove(&candidate, bucket(key), root).is_err(),
        "tampered leaf accepted"
    );
    println!(
        "MERKLE_MEMORY {}",
        serde_json::json!({"process_status":fs::read_to_string("/proc/self/status")?.lines().filter(|l|l.starts_with("VmRSS:")||l.starts_with("VmHWM:")).collect::<Vec<_>>(),"note":"whole benchmark process, includes both SQLite connections and input strings; not per-algorithm peak"})
    );
    Ok(())
}
