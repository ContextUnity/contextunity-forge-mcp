use super::{populate, sqlite_cache_size_kib};
use crate::engine::scanner;
use rusqlite::Connection;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct TemporaryRoot(PathBuf);

impl TemporaryRoot {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "forge_writer_cache_test_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sqlite_cache_size_is_clamped_in_kibibytes() {
    assert_eq!(sqlite_cache_size_kib(0), 4_096);
    assert_eq!(sqlite_cache_size_kib(64 * 1024 * 1024), 8_192);
    assert_eq!(sqlite_cache_size_kib(u64::MAX), 128_000);

    let root = TemporaryRoot::new();
    let adapter = scanner::load_adapter(&root.0, None).unwrap();
    let mut connection = Connection::open_in_memory().unwrap();
    connection.pragma_update(None, "cache_size", 1).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA cache_size", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );

    populate(
        &mut connection,
        &root.0,
        &adapter,
        &[],
        &BTreeMap::new(),
        None,
    )
    .unwrap();

    let cache_size = connection
        .query_row("PRAGMA cache_size", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert!(cache_size < 0, "cache_size must use negative KiB units");
    let cache_size_kib = cache_size
        .checked_neg()
        .and_then(|value| u64::try_from(value).ok())
        .expect("negative cache size must fit KiB range");
    assert!((4_096..=128_000).contains(&cache_size_kib));
}
