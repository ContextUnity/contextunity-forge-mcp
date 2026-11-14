---
title: "Cross-Platform and macOS Boundary Architecture"
doc_type: architecture
---

# Cross-Platform and macOS Boundary Architecture

Forge MCP runs across Linux (`x86_64`, `aarch64`) and macOS Darwin/APFS (`x86_64`, `arm64`). Operating-system and filesystem differences must sit behind explicit library and test-substrate boundaries rather than per-caller workarounds.

## 1. Canonical path, symlink confinement, and URI boundary

> [!IMPORTANT]
> Invariant: Production MCP adapters resolve workspace roots to canonical paths, validate filesystem confinement without bypassing internal symlinks, and emit percent-encoded `file://` URIs strictly from canonical coordinates.
> - INV-XPLAT-CANONICAL-ROOT: Tool and query handlers canonicalize workspace and database containment boundaries before admitting paths or formatting coordinates. [`resolve_target_root_and_db`](../../src/mcp/server.rs)
> - INV-XPLAT-SYSTEM-ROOT-SYMLINK: Filesystem confinement exempts only declared OS root aliases (`/var`, `/tmp`, `/etc` on macOS) and rejects all other ancestor or leaf symlinks. [`is_system_root_symlink`](../../crates/forge-core/src/fs.rs) [`confined_database_open_path`](../../crates/forge-core/src/fs.rs)
> - INV-XPLAT-TEST-URI-CONTRACT: Test workspaces keep uncanonicalized OS temp directories to exercise system-symlink confinement while filesystem-derived expected coordinates use canonical paths and production percent-encoding rules. [`file_uri_for_path`](../../src/core/format/discovery.rs)

### Rules

1. **System temp aliases belong to the input boundary.** On macOS, `std::env::temp_dir()` resolves under `/var/folders/...`, where `/var` symlinks to `private/var`. Keep temporary test roots uncanonicalized at setup so OS-root alias exemption and ancestor confinement remain under real test coverage.
2. **Database opens require canonicalized parent confinement.** [`confined_database_open_path`](../../crates/forge-core/src/fs.rs) verifies every existing ancestor against [`is_system_root_symlink`](../../crates/forge-core/src/fs.rs) and canonicalizes only the parent before returning `<canonical_parent>/<file_name>`, allowing `SQLITE_OPEN_NOFOLLOW` (`O_NOFOLLOW`) to inspect the leaf database file on macOS without tripping on `/var` or `/tmp`.
3. **Relative child resolution does not canonicalize internal symlinks.** [`checked_child`](../../crates/forge-core/src/fs.rs) appends relative segments onto an already-canonical workspace root. It never canonicalizes child components, preserving rejection of escaping leaf symlinks.
4. **Never assemble expected URIs manually across tests.** Use [`Workspace`](../../tests/common/workspace.rs) accessors `canonical_root(&self) -> PathBuf`, `canonical_path(&self, relative: impl AsRef<Path>) -> PathBuf`, and `file_uri(&self, relative: impl AsRef<Path>, line: Option<usize>) -> String`. For another root use `canonical_file_uri(root: &Path, relative: impl AsRef<Path>, line: Option<usize>) -> String` in `common::workspace`. URI helpers validate relative coordinates, canonicalize the root, and call production [`file_uri_for_path`](../../src/core/format/discovery.rs).

## 2. APFS case-insensitivity and Unicode path boundary

> [!IMPORTANT]
> Invariant: Case-preserving and case-insensitive volumes must use exact case-matched inventory membership and preserve filesystem-reported Unicode spelling rather than rely on raw path `.exists()` probes.
> - INV-XPLAT-APFS-CASE-INVENTORY: Volume case-sensitivity probing isolates case-folding checks, and delta/rebuild path classifiers must not reject case-only renames based on raw `.exists()` or `symlink_metadata()` hits. [`is_case_insensitive_volume`](../../crates/forge-core/src/fs.rs) [`delta_inner`](../../src/engine/pipeline.rs)

### Rules

1. **Detect case folding dynamically per volume.** Query [`is_case_insensitive_volume`](../../crates/forge-core/src/fs.rs) instead of assuming OS-wide behavior (Linux ext4 vs case-insensitive APFS vs case-sensitive APFS).
2. **Check exact entry spelling for missing inventory keys.** The bounded inventory walk records watched exact entries, including excluded and ignored-name entries. Delta and server admission classify remaining missing keys together using a bounded cache of parent directory entries after confined resolution. An exact existing entry outside the inventory is excluded; a missing spelling can be a deletion only if it was indexed. Inspection failures and symlinks fail closed. A raw `exists()` or `symlink_metadata()` hit does not establish spelling on a case-insensitive volume.
3. **Preserve filesystem-reported Unicode spelling.** Scanner keys retain the UTF-8 spelling reported by the volume; NFC and NFD are not normalized by Forge. Their distinctness follows the volume. URI formatting percent-encodes the resulting path once.

## 3. SQLite WAL lifecycle and generation identity boundary

> [!IMPORTANT]
> Invariant: Reader connection eviction, read-only probing, write-ahead log checkpointing, and `Identity` sampling obey strict ordering so SQLite connection closure never invalidates an in-flight admission baseline or a preserved receipt.
> - INV-XPLAT-ADMISSION-ORDER: Close or evict any stale cached reader handle before sampling the database generation `Identity` and opening the replacement reader. [`Server::admit`](../../src/mcp/server.rs) [`open`](../../crates/forge-index/src/db/reader.rs)
> - INV-XPLAT-WAL-CHECKPOINT-BOUNDARY: Writers finish WAL cleanup before sealing verified receipts, check checkpoint busy status explicitly, and handle rollback WAL cleanup without breaking pre-transaction receipt identity. [`identity`](../../crates/forge-index/src/db/cache.rs) [`persist_delta`](../../crates/forge-index/src/db/delta.rs)

This boundary refines [Concurrency and Isolation](concurrency.md) and [ADR 0002](../adr/0002-generational-sqlite-concurrency.md):

1. **Account for WAL side effects of `SQLITE_OPEN_READ_ONLY` connections.** SQLite may open `<db>-wal` and `<db>-shm` for a read-only handle in WAL mode. Closing a reader can leave sidecars for a later writer to checkpoint and close, and connection closure can change database metadata. Do not infer sidecar absence or identity stability from a successful read or rollback.
2. **Sample the replacement baseline after connection eviction.** [`Server::admit`](../../src/mcp/server.rs) may sample identity to check a cached slot, but evicts a stale slot before sampling the replacement baseline and opening [`open`](../../crates/forge-index/src/db/reader.rs). It also evicts the slot before calling `rebuild` or `delta` so no same-process reader lock blocks checkpointing.
3. **Sample query post-check before `ROLLBACK`.** In `read_with_adapter`, sample `after` identity while holding the shared generation lock before ending the read transaction (`ROLLBACK`), preventing self-inflicted close noise while catching concurrent writers. Keep the connection for warm reads; writers under the exclusive generation lock use the index cache's process-local registry to evict idle slots before sidecar cleanup. Rebinding a slot replaces its previous registry entry.
4. **Never discard WAL checkpoint results.** The shared checkpoint helper accepts only a non-busy result with all frames checkpointed, or SQLite's `(-1, -1)` no-WAL frame counts. Cold population uses TRUNCATE before switching journal modes; candidate publication uses PASSIVE and delta uses TRUNCATE. A delta checkpoint error occurs after commit and says so. Close the writer before sampling receipt identity. Under the generation lock, checkpoint and close existing output sidecars through SQLite and refuse replacement if WAL, SHM, or journal sidecars remain. Stage cleanup attempts all three sidecar removals, including dangling symlinks, and reports each failure. Candidate failure preserves the prior generation; a cleanup failure is attached to that error with the stage path. After publication, cleanup failure appears in `cleanup_error` and receipt publication failure is reported as a diagnostic with `verification_cache` false. Ensure transaction rollbacks either leave the database metadata and `-wal` absent state untouched or reconcile receipts deterministically.
5. **Revalidate rollback identity drift.** Under the exclusive generation lock, delta recovery preserves a receipt only if it matched the admitted root, database, seal, permissions, and identity before persistence. After a failed persistence attempt, an unchanged identity retains that receipt. If identity changed, recovery checks the original receipt again, verifies the old output root and complete Merkle commitments through a confined read-only connection, closes it, and requires a stable identity with no nonempty WAL before atomically publishing a replacement. Failed verification or publication attempts receipt invalidation and reports recovery and invalidation errors; subsequent admission must fully verify any generation without a matching receipt. A missing or mismatched receipt is never promoted by rollback recovery.

## 4. BSD sockets, signals, process identity, and POSIX shell portability

> [!IMPORTANT]
> Invariant: Networking fixtures, OS process reload discovery, and installer shell scripts enforce POSIX-portable contracts instead of relying on Linux-specific kernel or GNU userland behavior.
> - INV-XPLAT-BSD-SOCKET-MODE: Accepted sockets from non-blocking listeners explicitly force blocking mode (`set_nonblocking(false)`), configure timeouts, flush writes, and fail loudly on unexpected short writes.
> - INV-XPLAT-PLATFORM-RELOAD-ISOLATION: OS-specific server enumeration is confined behind `#[cfg(target_os = ...)]` boundaries and shares the CLI server grammar. [`run`](../../src/cli/reload.rs)

### Rules

1. **Never assume `accept()` clears `O_NONBLOCK`.** On macOS/BSD sockets, an accepted `TcpStream` inherits `O_NONBLOCK` from a non-blocking `TcpListener` (unlike Linux). Every test HTTP server or socket acceptor using non-blocking accept polling must call `stream.set_nonblocking(false)`, set read and write timeouts, call `flush()`, and panic on short writes for expected `200 OK` payloads.
2. **Isolate OS process inspection.** Keep Linux `/proc/<pid>` traversal and macOS `libproc` / `sysctl` (`KERN_PROCARGS2`) discovery isolated in [`src/cli/reload.rs`](../../src/cli/reload.rs), sharing the `Cli::try_parse_from` server filter. Linux checks the installed path, image device and inode, and ambiguous deleted-marker paths; macOS compares the reported executable path and its resolved alias. Skip processes whose identity or arguments cannot be read.
3. **Keep installer scripts pure POSIX `/bin/sh`.** [`install.sh`](../../install.sh) and companion `skills/install.sh` run under `set -eu` and `LC_ALL=C`, map `uname -s` / `uname -m` (`Darwin` + `arm64` -> `aarch64-apple-darwin`), support `sha256sum` with `shasum -a 256` fallback, use portable `mktemp ...XXXXXXXX` templates, and avoid GNU-only flags (`sed -i`, `readlink -f`, `stat -c`).
4. **Own fixture and installer cleanup.** The installer fixture bounds child execution, terminates its process group even after early exit, and joins its HTTP server. Stdio fixtures own process groups and stop and join their bounded byte readers during teardown. Its local listener uses close-on-exec and address reuse; a quiescent listener's port must be rebindable within a bounded deadline after teardown. Completed connections can remain in `TIME_WAIT`, so `http_harness_releases_unused_listener_port` checks a fresh listener with no accepted connections. A truncated response, HTTP error, or interrupted download must leave the prior binary, client configuration, and skills intact and remove temporary downloads and staging files. A broken pipe or reset is tolerated only for an abandoned 404 response, with a diagnostic; a failed 200 write is surfaced at explicit teardown.

## 5. Fail-loud cross-platform diagnostics boundary

> [!IMPORTANT]
> Invariant: Platform-sensitive boundaries must propagate exact attribute diffs, reasons, exit statuses, and captured stderr rather than collapsing failures into silent booleans or ignored results.
> - INV-XPLAT-DIAGNOSTIC-DIFFS: Generation and receipt identity failures include structured field-level attribute diffs (`device`, `inode`, `ctime`, `mtime`, `size`, `wal`). [`describe_diff`](../../crates/forge-index/src/db/cache.rs) [`matches_with_reason`](../../crates/forge-index/src/db/cache.rs)

### Rules

1. **Expose attribute diffs on generation mismatches.** Surface [`describe_diff`](../../crates/forge-index/src/db/cache.rs) on every generation or admission mismatch (`reader::open`, `persist_delta`, `Server::admit`, `read_with_adapter`).
2. **Prefer reason-returning checks over boolean suppression.** Use [`matches_with_reason`](../../crates/forge-index/src/db/cache.rs) wherever callers or tests distinguish corrupt/drifted receipts from absent receipts; never suppress cache publication errors (`publish_verified_identity`), `-wal`/`-shm` cleanup failures, debug log writes, or socket I/O errors without diagnostic output.
3. **Fail tests with full command and identity context.** Assertions on child processes (`install.sh`, spawned servers) and generation receipts must format exit codes, stdout/stderr, and mismatch reasons on failure.
4. **Preserve rollback failure context.** Delta returns the original persistence error and emits a separate diagnostic if receipt recovery fails. Identity comparisons use [`describe_diff`](../../crates/forge-index/src/db/cache.rs) when recovery observes further drift.
5. **Keep optional debug logging best effort.** Directory creation, serialization, open, and write failures report the operation, destination, and reason on stderr without including request contents. Logging does not change the request result or protocol stdout. Lifecycle correctness errors remain the responsibility of their owning boundaries.
