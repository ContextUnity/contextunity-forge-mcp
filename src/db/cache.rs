use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

pub(crate) fn shared_lock(path: &Path) -> Result<File> {
    generation_lock(path, false)
}

pub(crate) fn exclusive_lock(path: &Path) -> Result<File> {
    generation_lock(path, true)
}

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
pub fn matches(path: &Path, root: &Path, seal: &str, identity: &Identity) -> bool {
    fn read(path: &Path, root: &Path, seal: &str, identity: &Identity) -> Result<bool> {
        let receipt_path = suffix(path, ".verified.json");
        let metadata = fs::symlink_metadata(&receipt_path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16384 {
            return Ok(false);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
                return Ok(false);
            }
        }
        let r: Receipt = serde_json::from_slice(&fs::read(receipt_path)?)?;
        Ok(r.version == 1
            && r.schema == "2"
            && r.algorithm == crate::core::commitments::ALGORITHM
            && r.root == root.canonicalize()?
            && r.database == path.canonicalize()?
            && r.output_root == seal
            && seal.len() == 64
            && seal.bytes().all(|b| b.is_ascii_hexdigit())
            && r.identity == *identity)
    }
    read(path, root, seal, identity).unwrap_or(false)
}
pub fn publish_verified(path: &Path, root: &Path, seal: &str) -> Result<bool> {
    let before = identity(path)?;
    publish_verified_identity(path, root, seal, &before)
}
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
pub fn invalidate(path: &Path) {
    let _ = fs::remove_file(suffix(path, ".verified.json"));
}
