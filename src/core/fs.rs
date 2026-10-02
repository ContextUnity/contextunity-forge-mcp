use anyhow::{bail, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NONCE: AtomicU64 = AtomicU64::new(0);

/// Performs atomic write.
pub fn atomic_write(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let parent = absolute.parent().context("missing parent")?;
    let mut current = PathBuf::new();
    for c in parent.components() {
        current.push(c);
        match fs::symlink_metadata(&current) {
            Ok(m) if m.file_type().is_symlink() => bail!(
                "output parent must not traverse symlink: {}",
                current.display()
            ),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    if fs::symlink_metadata(&absolute).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("output must not be a symlink");
    }
    let temporary = parent.join(format!(
        ".forge-write-{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if executable { 0o755 } else { 0o600 });
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &absolute)?;
        OpenOptions::new().read(true).open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
