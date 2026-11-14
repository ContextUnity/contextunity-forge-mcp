use anyhow::{bail, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
    time::{Duration, Instant},
};

/// Process-local reader eviction callback owned by the reader's lifecycle.
pub type IdleReader = dyn Fn(&Path) -> Result<()> + Send + Sync;
type ReaderRegistry = Vec<(PathBuf, Weak<IdleReader>)>;
static READERS: OnceLock<Mutex<ReaderRegistry>> = OnceLock::new();

/// Resolve the registry key without following the database leaf.
pub fn registry_path(path: &Path) -> PathBuf {
    path.parent()
        .and_then(|parent| parent.canonicalize().ok())
        .and_then(|parent| path.file_name().map(|name| parent.join(name)))
        .unwrap_or_else(|| path.to_path_buf())
}

/// Registers the current database for a reader, replacing its prior binding.
pub fn register_idle_reader(path: &Path, reader: &Arc<IdleReader>) -> Result<()> {
    let mut readers = READERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("reader registry lock poisoned"))?;
    let weak = Arc::downgrade(reader);
    readers.retain(|(_, entry)| entry.strong_count() > 0 && !entry.ptr_eq(&weak));
    readers.push((registry_path(path), weak));
    Ok(())
}

/// Evicts matching idle readers and fences registrations until the returned guard is dropped.
pub fn release_idle_readers(path: &Path) -> Result<MutexGuard<'static, ReaderRegistry>> {
    let path = registry_path(path);
    let mut readers = READERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("reader registry lock poisoned"))?;
    readers.retain(|(_, entry)| entry.strong_count() > 0);
    for (_, reader) in readers.iter().filter(|(key, _)| *key == path) {
        if let Some(reader) = reader.upgrade() {
            reader(&path)?;
        }
    }
    Ok(readers)
}

/// Counts registrations for a reader, for lifecycle regression checks.
#[doc(hidden)]
pub fn idle_reader_registration_count(reader: &Arc<IdleReader>) -> Result<usize> {
    let readers = READERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("reader registry lock poisoned"))?;
    let weak = Arc::downgrade(reader);
    Ok(readers
        .iter()
        .filter(|(_, entry)| entry.ptr_eq(&weak))
        .count())
}

/// Checkpoints all WAL frames, rejecting busy or incomplete results.
pub fn checkpoint(conn: &Connection, truncate: bool) -> Result<()> {
    let mode = if truncate { "TRUNCATE" } else { "PASSIVE" };
    let (busy, log, done): (i64, i64, i64) =
        conn.query_row(&format!("PRAGMA wal_checkpoint({mode})"), [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    if busy != 0 || !((log == -1 && done == -1) || (log >= 0 && done == log)) {
        bail!("WAL checkpoint {mode} incomplete: busy={busy}, log={log}, checkpointed={done}");
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Represents file identity data.
pub struct FileIdentity {
    device: u64,
    inode: u64,
    ctime: i64,
    ctime_ns: i64,
    mtime: i64,
    mtime_ns: i64,
    size: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Represents identity data.
pub struct Identity {
    main: FileIdentity,
    wal: Option<FileIdentity>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    schema: String,
    algorithm: String,
    root: PathBuf,
    database: PathBuf,
    output_root: String,
    identity: Identity,
}
fn file_identity(path: &Path) -> Result<FileIdentity> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        bail!("database/cache path must be a regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(FileIdentity {
            device: m.dev(),
            inode: m.ino(),
            ctime: m.ctime(),
            ctime_ns: m.ctime_nsec(),
            mtime: m.mtime(),
            mtime_ns: m.mtime_nsec(),
            size: m.len(),
        })
    }
    #[cfg(not(unix))]
    {
        bail!("generation identity cache requires Unix metadata")
    }
}
fn suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn generation_lock(path: &Path, exclusive: bool) -> Result<File> {
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(suffix(path, ".lock"))?;
        if !file.metadata()?.is_file() {
            bail!("database lock must be a regular file");
        }
        let operation = if exclusive {
            libc::LOCK_EX
        } else {
            libc::LOCK_SH
        };
        let started = Instant::now();
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) } == 0 {
                return Ok(file);
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == std::io::ErrorKind::WouldBlock {
                if started.elapsed() >= Duration::from_secs(30) {
                    bail!("database generation lock is busy; retry after active readers finish");
                }
            } else {
                return Err(error.into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, exclusive);
        bail!("generation locks require Unix file locking")
    }
}

/// Acquires a shared lock on the SQLite generation lock file.
pub fn shared_lock(path: &Path) -> Result<File> {
    generation_lock(path, false)
}

/// Acquires an exclusive lock on the SQLite generation lock file.
pub fn exclusive_lock(path: &Path) -> Result<File> {
    generation_lock(path, true)
}

/// Performs identity.
pub fn identity(path: &Path) -> Result<Identity> {
    let main = file_identity(path)?;
    let wal_path = suffix(path, "-wal");
    let wal = match fs::symlink_metadata(&wal_path) {
        Ok(m) if m.len() > 0 => Some(file_identity(&wal_path)?),
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    Ok(Identity { main, wal })
}
fn push_file_diffs(
    prefix: &str,
    before: &FileIdentity,
    after: &FileIdentity,
    diffs: &mut Vec<String>,
) {
    if before.device != after.device {
        diffs.push(format!(
            "{prefix}.device: {} -> {}",
            before.device, after.device
        ));
    }
    if before.inode != after.inode {
        diffs.push(format!(
            "{prefix}.inode: {} -> {}",
            before.inode, after.inode
        ));
    }
    if before.ctime != after.ctime || before.ctime_ns != after.ctime_ns {
        diffs.push(format!(
            "{prefix}.ctime: {}.{:09} -> {}.{:09}",
            before.ctime, before.ctime_ns, after.ctime, after.ctime_ns
        ));
    }
    if before.mtime != after.mtime || before.mtime_ns != after.mtime_ns {
        diffs.push(format!(
            "{prefix}.mtime: {}.{:09} -> {}.{:09}",
            before.mtime, before.mtime_ns, after.mtime, after.mtime_ns
        ));
    }
    if before.size != after.size {
        diffs.push(format!("{prefix}.size: {} -> {}", before.size, after.size));
    }
}

/// Describes the exact attribute differences between two database generation identities.
pub fn describe_diff(before: &Identity, after: &Identity) -> String {
    let mut diffs = Vec::new();
    push_file_diffs("main", &before.main, &after.main, &mut diffs);
    match (&before.wal, &after.wal) {
        (None, None) => {}
        (Some(b), None) => diffs.push(format!(
            "wal: present(ino={},size={},ctime={}.{:09},mtime={}.{:09}) -> absent",
            b.inode, b.size, b.ctime, b.ctime_ns, b.mtime, b.mtime_ns
        )),
        (None, Some(a)) => diffs.push(format!(
            "wal: absent -> present(ino={},size={},ctime={}.{:09},mtime={}.{:09})",
            a.inode, a.size, a.ctime, a.ctime_ns, a.mtime, a.mtime_ns
        )),
        (Some(b), Some(a)) => push_file_diffs("wal", b, a, &mut diffs),
    }
    if diffs.is_empty() {
        "none".into()
    } else {
        diffs.join(", ")
    }
}

/// Verifies the receipt for `path`, returning a descriptive mismatch error if invalid.
pub fn matches_with_reason(
    path: &Path,
    root: &Path,
    seal: &str,
    identity: &Identity,
) -> Result<()> {
    let receipt_path = suffix(path, ".verified.json");
    let metadata = fs::symlink_metadata(&receipt_path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16384 {
        bail!(
            "receipt is missing, non-regular, or oversized ({})",
            metadata.len()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let euid = unsafe { libc::geteuid() };
        if metadata.uid() != euid || metadata.mode() & 0o077 != 0 {
            bail!(
                "receipt ownership/permissions mismatch (uid={}, euid={}, mode={:#o})",
                metadata.uid(),
                euid,
                metadata.mode()
            );
        }
    }
    let r: Receipt = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    if r.version != 1 || r.schema != "2" || r.algorithm != crate::core::commitments::ALGORITHM {
        bail!(
            "receipt header mismatch (version={}, schema={}, algorithm={})",
            r.version,
            r.schema,
            r.algorithm
        );
    }
    let canonical_root = root.canonicalize()?;
    if r.root != canonical_root {
        bail!(
            "receipt root mismatch: expected {}, got {}",
            canonical_root.display(),
            r.root.display()
        );
    }
    let canonical_db = path.canonicalize()?;
    if r.database != canonical_db {
        bail!(
            "receipt database mismatch: expected {}, got {}",
            canonical_db.display(),
            r.database.display()
        );
    }
    if r.output_root != seal || seal.len() != 64 || !seal.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("receipt output_root mismatch");
    }
    if r.identity != *identity {
        bail!(
            "receipt identity mismatch ({})",
            describe_diff(&r.identity, identity)
        );
    }
    Ok(())
}

/// Performs matches.
pub fn matches(path: &Path, root: &Path, seal: &str, identity: &Identity) -> bool {
    matches_with_reason(path, root, seal, identity).is_ok()
}
/// Performs publish verified.
pub fn publish_verified(path: &Path, root: &Path, seal: &str) -> Result<bool> {
    let before = identity(path)?;
    publish_verified_identity(path, root, seal, &before)
}
/// Performs publish verified identity.
pub fn publish_verified_identity(
    path: &Path,
    root: &Path,
    seal: &str,
    before: &Identity,
) -> Result<bool> {
    if identity(path)? != *before {
        return Ok(false);
    }
    if before.wal.is_some() {
        invalidate(path);
        return Ok(false);
    }
    let receipt = Receipt {
        version: 1,
        schema: "2".into(),
        algorithm: crate::core::commitments::ALGORITHM.into(),
        root: root.canonicalize()?,
        database: path.canonicalize()?,
        output_root: seal.into(),
        identity: before.clone(),
    };
    crate::core::fs::atomic_write(
        &suffix(path, ".verified.json"),
        &serde_json::to_vec(&receipt)?,
        false,
    )?;
    if identity(path)? != *before {
        invalidate(path);
        return Ok(false);
    }
    Ok(true)
}
/// Performs invalidate.
pub fn invalidate(path: &Path) {
    if let Err(error) = invalidate_checked(path) {
        eprintln!(
            "verified receipt invalidation failed for {}: {error:#}",
            suffix(path, ".verified.json").display()
        );
    }
}

/// Removes a receipt, reporting failures other than an already absent receipt.
pub fn invalidate_checked(path: &Path) -> Result<()> {
    match fs::remove_file(suffix(path, ".verified.json")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
