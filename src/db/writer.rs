use crate::core::models::compact_graph::Graph as CompactGraph;
use crate::core::typed_facts::{decode_facts, TypedFacts};
use crate::engine::languages::module_name;
use crate::{
    core::{commitments, models::*, schema::SCHEMA_DDL},
    engine::{
        ast, docs, linker,
        scanner::{self, BuildCandidate, FileEntry},
    },
};
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use rusqlite::{
    limits::Limit, params, params_from_iter, types::Value as SqlValue, Connection, OpenFlags,
    OptionalExtension, ToSql,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::Read,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Condvar, Mutex,
    },
    thread,
    time::Instant,
};
#[path = "batch.rs"]
mod batch;
#[path = "delta.rs"]
mod delta;
#[path = "dictionary.rs"]
mod dictionary;
#[path = "ingest.rs"]
mod ingest;

use batch::*;
pub use delta::delta;
use dictionary::*;
use ingest::*;

static NONCE: AtomicU64 = AtomicU64::new(0);
// Bound source, parser-tree, and extracted-fact working memory per active file.
const EXTRACTION_MEMORY_MULTIPLIER: u64 = 8;
const MAX_MULTI_VALUE_BATCH_ROWS: usize = 500;

type DependencyRegistry = crate::engine::languages::dependency_registry::DependencyRegistry;

#[derive(Clone, Copy)]
struct BuildInputs<'a> {
    dependency_registry: &'a DependencyRegistry,
    memory_budget: u64,
    schema_ready: bool,
}

#[derive(Default)]
struct ByteBudgetState {
    in_flight_bytes: u64,
    peak_in_flight_bytes: u64,
    max_item_bytes: u64,
    oversized_admissions: u64,
}

struct ByteBudget {
    limit: u64,
    state: Mutex<ByteBudgetState>,
    available: Condvar,
}

impl ByteBudget {
    fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            limit: limit.max(1),
            state: Mutex::new(ByteBudgetState::default()),
            available: Condvar::new(),
        })
    }

    fn acquire(self: &Arc<Self>, bytes: u64, cancelled: &AtomicBool) -> Result<MemoryPermit> {
        let bytes = bytes.max(1);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            if cancelled.load(Ordering::Acquire) {
                bail!("cold pipeline cancelled while waiting for memory budget");
            }
            let oversized = bytes > self.limit;
            let admitted = if oversized {
                state.in_flight_bytes == 0
            } else {
                state.in_flight_bytes <= self.limit.saturating_sub(bytes)
            };
            if admitted {
                state.in_flight_bytes = state.in_flight_bytes.saturating_add(bytes);
                state.peak_in_flight_bytes = state.peak_in_flight_bytes.max(state.in_flight_bytes);
                state.max_item_bytes = state.max_item_bytes.max(bytes);
                if oversized {
                    state.oversized_admissions = state.oversized_admissions.saturating_add(1);
                }
                return Ok(MemoryPermit {
                    budget: Arc::clone(self),
                    bytes,
                });
            }
            state = self
                .available
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    fn release(&self, bytes: u64) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.in_flight_bytes = state.in_flight_bytes.saturating_sub(bytes);
        self.available.notify_all();
    }

    fn wake_waiters(&self) {
        self.available.notify_all();
    }

    fn snapshot(&self) -> ByteBudgetState {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl Clone for ByteBudgetState {
    fn clone(&self) -> Self {
        Self {
            in_flight_bytes: self.in_flight_bytes,
            peak_in_flight_bytes: self.peak_in_flight_bytes,
            max_item_bytes: self.max_item_bytes,
            oversized_admissions: self.oversized_admissions,
        }
    }
}

struct MemoryPermit {
    budget: Arc<ByteBudget>,
    bytes: u64,
}

impl Drop for MemoryPermit {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}

struct PipelineControl {
    cancelled: AtomicBool,
    failure: Mutex<Option<String>>,
    budget: Arc<ByteBudget>,
}

impl PipelineControl {
    fn new(budget: Arc<ByteBudget>) -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            failure: Mutex::new(None),
            budget,
        }
    }

    fn cancel(&self, failure: impl std::fmt::Display) {
        self.cancelled.store(true, Ordering::Release);
        let mut current = self
            .failure
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if current.is_none() {
            *current = Some(failure.to_string());
        }
        drop(current);
        self.budget.wake_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

#[derive(Default)]
struct PipelineCounters {
    extracted_files: AtomicUsize,
    ingested_files: AtomicUsize,
    extraction_nanos: AtomicU64,
    ingestion_nanos: AtomicU64,
}

struct PipelineWork {
    #[cfg(test)]
    sequence: usize,
    candidate: BuildCandidate,
    permit: MemoryPermit,
}

struct PipelineItem {
    file: FileEntry,
    facts: TypedFacts,
    permit: MemoryPermit,
}

struct ExtractionWorkerContext {
    root: PathBuf,
    adapter: Arc<scanner::Adapter>,
    dependencies: Arc<DependencyRegistry>,
    work_rx: Arc<Mutex<Receiver<PipelineWork>>>,
    result_tx: SyncSender<std::result::Result<PipelineItem, String>>,
    control: Arc<PipelineControl>,
    counters: Arc<PipelineCounters>,
    #[cfg(test)]
    test_hooks: Option<Arc<PipelineTestHooks>>,
}

struct PipelineConsumerOutput {
    connection: Connection,
    entries: Vec<FileEntry>,
    facts: BTreeMap<String, TypedFacts>,
}

#[derive(Clone)]
struct PipelineMetrics {
    budget_bytes: u64,
    peak_in_flight_bytes: u64,
    max_item_bytes: u64,
    oversized_admissions: u64,
    extracted_files: usize,
    ingested_files: usize,
    scan_ms: f64,
    extraction_ms: f64,
    sqlite_ingest_ms: f64,
    scanned_files: usize,
    scanned_bytes: u64,
}

#[cfg(test)]
struct PipelineTestHooks {
    overlap: bool,
    fail_consumer: bool,
    overlap_barrier: std::sync::Barrier,
    overlap_participants: AtomicUsize,
}

#[cfg(test)]
impl PipelineTestHooks {
    fn overlap() -> Arc<Self> {
        Arc::new(Self {
            overlap: true,
            fail_consumer: false,
            overlap_barrier: std::sync::Barrier::new(3),
            overlap_participants: AtomicUsize::new(0),
        })
    }

    fn fail_consumer() -> Arc<Self> {
        Arc::new(Self {
            overlap: false,
            fail_consumer: true,
            overlap_barrier: std::sync::Barrier::new(3),
            overlap_participants: AtomicUsize::new(0),
        })
    }
}

fn sqlite_cache_size_kib(memory_budget: u64) -> u64 {
    (memory_budget / 8 / 1024).clamp(4_096, 128_000)
}

#[derive(Debug)]
/// Reports that a source file changed after its inventory digest was scanned.
pub struct SourceSnapshotMismatch(pub String);

impl std::fmt::Display for SourceSnapshotMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SourceSnapshotMismatch {}

/// Extracts one scanned file and rejects changes to its source snapshot.
///
/// # Errors
///
/// Returns [`SourceSnapshotMismatch`] if the file no longer matches its digest.
pub fn extract(root: &Path, file: &FileEntry, adapter: &scanner::Adapter) -> Result<Facts> {
    let dependencies = crate::engine::languages::dependency_registry::DependencyRegistry::try_collect_with_scan_config(root, Some(adapter))?;
    Ok(extract_typed(root, file, adapter, &dependencies)?.into_public())
}

fn extract_typed(
    root: &Path,
    file: &FileEntry,
    adapter: &scanner::Adapter,
    dependencies: &crate::engine::languages::dependency_registry::DependencyRegistry,
) -> Result<TypedFacts> {
    let path = scanner::resolve_file_path(root, adapter, &file.path)?;
    let source = fs::read_to_string(&path)?;
    extract_typed_source(file, &source, dependencies)
}

fn extract_typed_source(
    file: &FileEntry,
    source: &str,
    dependencies: &crate::engine::languages::dependency_registry::DependencyRegistry,
) -> Result<TypedFacts> {
    let source_digest = commitments::hash(source.as_bytes());
    extract_typed_source_with_digest(file, source, &source_digest, dependencies)
}

fn extract_typed_source_with_digest(
    file: &FileEntry,
    source: &str,
    source_digest: &str,
    dependencies: &crate::engine::languages::dependency_registry::DependencyRegistry,
) -> Result<TypedFacts> {
    if source_digest != file.digest {
        return Err(SourceSnapshotMismatch(format!(
            "source changed during extraction: {}",
            file.path
        ))
        .into());
    }
    if file.is_doc {
        let sections = docs::extract(&file.path, source, file.mtime_ns as f64 / 1e9)?;
        let nodes = sections
            .iter()
            .map(|d| Node {
                id: d.doc_id.clone(),
                kind: "document".into(),
                name: d.section_title.clone(),
                qualname: d.doc_id.clone(),
                path: d.path.clone(),
                line: 1,
                end_line: source.lines().count().max(1),
                is_test: false,
                language: "markdown".into(),
                generated: false,
                details: json!({"doc_type":d.doc_type,"is_invariant":d.is_invariant}),
            })
            .collect();
        Ok(TypedFacts::from_public(Facts {
            docs: sections,
            nodes,
            ..Facts::default()
        }))
    } else {
        ast::extract_typed_with_registry(&file.path, &file.language, source, dependencies)
    }
}

fn extract_candidate(
    root: &Path,
    candidate: &BuildCandidate,
    adapter: &scanner::Adapter,
    dependencies: &crate::engine::languages::dependency_registry::DependencyRegistry,
) -> Result<(FileEntry, TypedFacts)> {
    let path = scanner::resolve_file_path(root, adapter, &candidate.path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut source_file = options.open(&path)?;
    if !candidate.matches_metadata(&source_file.metadata()?) {
        return Err(SourceSnapshotMismatch(format!(
            "source changed during extraction: {}",
            candidate.path
        ))
        .into());
    }
    let source_capacity =
        usize::try_from(candidate.bytes).context("source size exceeds addressable memory")?;
    let mut source = String::new();
    source.try_reserve_exact(source_capacity)?;
    source_file
        .by_ref()
        .take(adapter.limits.max_file_bytes.saturating_add(1))
        .read_to_string(&mut source)?;
    if source.len() as u64 > adapter.limits.max_file_bytes || source.len() as u64 != candidate.bytes
    {
        return Err(SourceSnapshotMismatch(format!(
            "source changed during extraction: {}",
            candidate.path
        ))
        .into());
    }
    if !candidate.matches_metadata(&source_file.metadata()?) {
        return Err(SourceSnapshotMismatch(format!(
            "source changed during extraction: {}",
            candidate.path
        ))
        .into());
    }
    let source_digest = commitments::hash(source.as_bytes());
    let file = candidate.clone().into_entry(source_digest.clone());
    let facts = extract_typed_source_with_digest(&file, &source, &source_digest, dependencies)?;
    Ok((file, facts))
}

fn prepare_pipeline_schema(
    conn: &mut Connection,
    memory_budget: u64,
    #[cfg(test)] test_hooks: Option<&PipelineTestHooks>,
) -> Result<()> {
    let cache_kb = sqlite_cache_size_kib(memory_budget);
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-{cache_kb}; PRAGMA cache_spill=OFF;"
    ))?;
    let (tables, _) = SCHEMA_DDL
        .split_once("CREATE INDEX")
        .context("schema indexes missing")?;
    conn.execute_batch(tables)?;
    #[cfg(test)]
    if test_hooks.is_some_and(|hooks| hooks.fail_consumer) {
        conn.execute_batch(
            "CREATE TEMP TRIGGER pipeline_test_consumer_failure
             BEFORE INSERT ON main.source_inventory
             WHEN NEW.path='src/unit_00.rs'
             BEGIN SELECT RAISE(ABORT,'injected pipeline SQLite consumer failure'); END;",
        )?;
    }
    Ok(())
}

fn run_cold_pipeline(
    root: &Path,
    adapter: &scanner::Adapter,
    dependencies: Arc<DependencyRegistry>,
    connection: Connection,
    budget_bytes: u64,
    #[cfg(test)] test_hooks: Option<Arc<PipelineTestHooks>>,
) -> Result<(PipelineConsumerOutput, PipelineMetrics)> {
    let worker_count = rayon::current_num_threads().max(1);
    let channel_capacity = worker_count.saturating_mul(2).max(1);
    let byte_budget = ByteBudget::new(budget_bytes);
    let control = Arc::new(PipelineControl::new(Arc::clone(&byte_budget)));
    let counters = Arc::new(PipelineCounters::default());
    let (work_tx, work_rx) = mpsc::sync_channel::<PipelineWork>(channel_capacity);
    let work_rx = Arc::new(Mutex::new(work_rx));
    let (result_tx, result_rx) =
        mpsc::sync_channel::<std::result::Result<PipelineItem, String>>(channel_capacity);

    let consumer_control = Arc::clone(&control);
    let consumer_counters = Arc::clone(&counters);
    #[cfg(test)]
    let consumer_hooks = test_hooks.clone();
    let consumer = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            consume_pipeline_results(
                connection,
                result_rx,
                consumer_control.as_ref(),
                consumer_counters.as_ref(),
                #[cfg(test)]
                consumer_hooks.as_deref(),
            )
        }));
        match outcome {
            Ok(result) => result,
            Err(_) => {
                consumer_control.cancel("cold pipeline SQLite consumer panicked");
                Err(anyhow::anyhow!("cold pipeline SQLite consumer panicked"))
            }
        }
    });

    let root = root.to_path_buf();
    let adapter = Arc::new(adapter.clone());
    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let worker_root = root.clone();
        let worker_adapter = Arc::clone(&adapter);
        let worker_dependencies = Arc::clone(&dependencies);
        let worker_rx = Arc::clone(&work_rx);
        let worker_tx = result_tx.clone();
        let worker_control = Arc::clone(&control);
        let panic_control = Arc::clone(&worker_control);
        let worker_counters = Arc::clone(&counters);
        #[cfg(test)]
        let worker_hooks = test_hooks.clone();
        workers.push(thread::spawn(move || {
            let context = ExtractionWorkerContext {
                root: worker_root,
                adapter: worker_adapter,
                dependencies: worker_dependencies,
                work_rx: worker_rx,
                result_tx: worker_tx,
                control: worker_control,
                counters: worker_counters,
                #[cfg(test)]
                test_hooks: worker_hooks,
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| extraction_worker(context)));
            if outcome.is_err() {
                panic_control.cancel("cold pipeline extraction worker panicked");
            }
        }));
    }
    drop(result_tx);

    let producer_control = Arc::clone(&control);
    let producer_adapter = Arc::clone(&adapter);
    let producer_budget = Arc::clone(&byte_budget);
    #[cfg(test)]
    let producer_hooks = test_hooks.clone();
    let producer = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let started = Instant::now();
            #[cfg(test)]
            let mut sequence = 0usize;
            let scan = scanner::stream_for_build(&root, &producer_adapter, |candidate| {
                let estimate = candidate
                    .bytes
                    .saturating_mul(EXTRACTION_MEMORY_MULTIPLIER)
                    .max(1);
                let permit = producer_budget
                    .acquire(estimate, &producer_control.cancelled)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                #[cfg(test)]
                let current_sequence = {
                    let current = sequence;
                    sequence = sequence.saturating_add(1);
                    current
                };
                work_tx
                    .send(PipelineWork {
                        #[cfg(test)]
                        sequence: current_sequence,
                        candidate,
                        permit,
                    })
                    .map_err(|error| {
                        std::io::Error::other(format!(
                            "cold pipeline worker channel closed: {error}"
                        ))
                    })?;
                #[cfg(test)]
                if producer_hooks
                    .as_deref()
                    .is_some_and(|hooks| hooks.overlap && current_sequence == 2)
                {
                    let hooks = producer_hooks.as_deref().expect("checked above");
                    hooks.overlap_participants.fetch_add(1, Ordering::AcqRel);
                    hooks.overlap_barrier.wait();
                }
                Ok(())
            })?;
            Ok::<_, std::io::Error>((scan, started.elapsed().as_secs_f64() * 1000.))
        }));
        match outcome {
            Ok(Ok(report)) => Ok(report),
            Ok(Err(error)) => {
                producer_control.cancel(&error);
                Err(error)
            }
            Err(_) => {
                producer_control.cancel("cold pipeline scanner panicked");
                Err(std::io::Error::other("cold pipeline scanner panicked"))
            }
        }
    });

    let scan_result = match producer.join() {
        Ok(result) => result,
        Err(_) => {
            control.cancel("cold pipeline scanner thread panicked");
            Err(std::io::Error::other(
                "cold pipeline scanner thread panicked",
            ))
        }
    };
    if let Err(error) = &scan_result {
        control.cancel(error);
    }
    for worker in workers {
        if worker.join().is_err() {
            control.cancel("cold pipeline extraction worker thread panicked");
        }
    }
    drop(work_rx);
    let consumer_result = match consumer.join() {
        Ok(result) => result,
        Err(_) => {
            control.cancel("cold pipeline SQLite consumer thread panicked");
            Err(anyhow::anyhow!(
                "cold pipeline SQLite consumer thread panicked"
            ))
        }
    };
    if let Some(failure) = control.failure() {
        bail!("{failure}");
    }
    let (scan, scan_ms) = scan_result.context("cold pipeline scan failed")?;
    let output = consumer_result?;
    let byte_state = byte_budget.snapshot();
    let metrics = PipelineMetrics {
        budget_bytes,
        peak_in_flight_bytes: byte_state.peak_in_flight_bytes,
        max_item_bytes: byte_state.max_item_bytes,
        oversized_admissions: byte_state.oversized_admissions,
        extracted_files: counters.extracted_files.load(Ordering::Acquire),
        ingested_files: counters.ingested_files.load(Ordering::Acquire),
        scan_ms,
        extraction_ms: counters.extraction_nanos.load(Ordering::Acquire) as f64 / 1_000_000.,
        sqlite_ingest_ms: counters.ingestion_nanos.load(Ordering::Acquire) as f64 / 1_000_000.,
        scanned_files: scan.files,
        scanned_bytes: scan.bytes,
    };
    Ok((output, metrics))
}

fn extraction_worker(context: ExtractionWorkerContext) {
    let ExtractionWorkerContext {
        root,
        adapter,
        dependencies,
        work_rx,
        result_tx,
        control,
        counters,
        #[cfg(test)]
        test_hooks,
    } = context;
    loop {
        let next = {
            let receiver = work_rx.lock().unwrap_or_else(|error| error.into_inner());
            receiver.recv()
        };
        let Ok(work) = next else {
            break;
        };
        if control.is_cancelled() {
            drop(work);
            continue;
        }
        #[cfg(test)]
        if test_hooks
            .as_deref()
            .is_some_and(|hooks| hooks.overlap && work.sequence == 1)
        {
            let hooks = test_hooks.as_deref().expect("checked above");
            hooks.overlap_participants.fetch_add(1, Ordering::AcqRel);
            hooks.overlap_barrier.wait();
        }
        let extraction_started = Instant::now();
        let extracted = extract_candidate(&root, &work.candidate, &adapter, &dependencies);
        counters.extraction_nanos.fetch_add(
            extraction_started
                .elapsed()
                .as_nanos()
                .min(u64::MAX as u128) as u64,
            Ordering::AcqRel,
        );
        let (file, facts) = match extracted {
            Ok(value) => value,
            Err(error) => {
                let message = error.to_string();
                control.cancel(&message);
                let _ = result_tx.send(Err(message));
                drop(work);
                continue;
            }
        };
        counters.extracted_files.fetch_add(1, Ordering::AcqRel);
        let item = PipelineItem {
            file,
            facts,
            permit: work.permit,
        };
        if let Err(error) = result_tx.send(Ok(item)) {
            control.cancel(format!("cold pipeline SQLite consumer stopped: {error}"));
        }
    }
}

fn consume_pipeline_results(
    mut connection: Connection,
    result_rx: Receiver<std::result::Result<PipelineItem, String>>,
    control: &PipelineControl,
    counters: &PipelineCounters,
    #[cfg(test)] test_hooks: Option<&PipelineTestHooks>,
) -> Result<PipelineConsumerOutput> {
    let transaction = connection.transaction()?;
    let mut statement =
        transaction.prepare_cached("INSERT INTO source_inventory VALUES(?1,'indexed',?2,?3)")?;
    let mut entries = Vec::new();
    let mut facts = BTreeMap::new();
    while let Ok(result) = result_rx.recv() {
        let item = match result {
            Ok(item) => item,
            Err(error) => {
                control.cancel(error);
                continue;
            }
        };
        if control.is_cancelled() {
            drop(item);
            continue;
        }
        #[cfg(test)]
        if counters.ingested_files.load(Ordering::Acquire) == 0
            && test_hooks.is_some_and(|hooks| hooks.overlap)
        {
            let hooks = test_hooks.expect("checked above");
            hooks.overlap_participants.fetch_add(1, Ordering::AcqRel);
            hooks.overlap_barrier.wait();
        }
        let ingestion_started = Instant::now();
        let PipelineItem {
            file,
            facts: typed_facts,
            permit,
        } = item;
        let inserted =
            statement.execute(params![file.path, file.digest, i64::try_from(file.bytes)?]);
        counters.ingestion_nanos.fetch_add(
            ingestion_started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            Ordering::AcqRel,
        );
        match inserted {
            Ok(_) => {
                let path = file.path.clone();
                entries.push(file);
                facts.insert(path, typed_facts);
                counters.ingested_files.fetch_add(1, Ordering::AcqRel);
            }
            Err(error) => control.cancel(error),
        }
        drop(permit);
    }
    drop(statement);
    if let Some(failure) = control.failure() {
        transaction.rollback()?;
        bail!("{failure}");
    }
    transaction.commit()?;
    Ok(PipelineConsumerOutput {
        connection,
        entries,
        facts,
    })
}
fn policy(root: &Path, adapter: &scanner::Adapter) -> Value {
    json!({
        "adapter_path": adapter.adapter_path.as_ref().map(|path| path.strip_prefix(root).unwrap_or(path).to_string_lossy()),
        "roots": adapter.roots.iter().map(|p| p.strip_prefix(root).unwrap_or(p).to_string_lossy()).collect::<Vec<_>>(),
        "limits": &adapter.limits,
        "ignored_names": adapter.ignored_names,
        "adapter_version": adapter.adapter_version,
        "digest": adapter.digest,
        "linked_workspaces": adapter.linked_workspaces,
        "owners": adapter.owners,
        "aliases": adapter.aliases,
        "milestones": adapter.milestones,
        "plans": adapter.plans,
    })
}

pub(super) fn persisted_adapter_path(root: &Path, value: &Value) -> Result<Option<PathBuf>> {
    match value.get("adapter_path") {
        None | Some(Value::Null) => Ok(None),
        Some(path) => Ok(Some(scanner::checked_child(
            root,
            Path::new(path.as_str().context("invalid persisted adapter path")?),
        )?)),
    }
}

fn read_policy(root: &Path, conn: &Connection) -> Result<scanner::Adapter> {
    let raw: String =
        conn.query_row("SELECT value FROM metadata WHERE key='adapter'", [], |r| {
            r.get(0)
        })?;
    let value: Value = serde_json::from_str(&raw)?;
    let adapter_path = persisted_adapter_path(root, &value)?;
    let roots = value["roots"]
        .as_array()
        .context("invalid persisted roots")?
        .iter()
        .map(|v| {
            let p = v.as_str().context("invalid root")?;
            if p.is_empty() {
                Ok(root.to_path_buf())
            } else {
                Ok(scanner::checked_child(root, Path::new(p))?)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let ignored_names = value["ignored_names"]
        .as_array()
        .context("invalid ignore policy")?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).context("invalid ignore name"))
        .collect::<Result<BTreeSet<_>>>()?;
    let limits: scanner::ScannerLimits = serde_json::from_value(
        value
            .get("limits")
            .cloned()
            .context("missing persisted scanner limits")?,
    )
    .context("invalid persisted scanner limits")?;
    let adapter_version = value["adapter_version"].as_str().map(str::to_owned);
    let digest = value["digest"].as_str().unwrap_or_default().to_owned();
    let linked_workspaces: Vec<scanner::LinkedWorkspace> = value
        .get("linked_workspaces")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let owners: std::collections::BTreeMap<String, String> = value
        .get("owners")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let aliases: std::collections::BTreeMap<String, String> = value
        .get("aliases")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let milestones: Vec<String> = value
        .get("milestones")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| vec!["docs/milestones".into()]);
    let plans: Vec<String> = value
        .get("plans")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| vec!["docs/plans".into()]);
    Ok(scanner::Adapter {
        adapter_path,
        response: Default::default(),
        limits,
        roots,
        ignored_names,
        adapter_version,
        digest,
        linked_workspaces,
        owners,
        aliases,
        milestones,
        plans,
        debug: false,
    })
}
fn populate(
    conn: &mut Connection,
    root: &Path,
    adapter: &scanner::Adapter,
    entries: &[FileEntry],
    facts: &BTreeMap<String, TypedFacts>,
    encoded_facts: Option<&hashbrown::HashMap<String, Vec<u8>>>,
    build_inputs: BuildInputs<'_>,
) -> Result<Value> {
    let cache_kb = sqlite_cache_size_kib(build_inputs.memory_budget);
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-{cache_kb}; PRAGMA cache_spill=OFF;"
    ))?;
    conn.set_prepared_statement_cache_capacity(128);
    let (tables, indexes) = SCHEMA_DDL
        .split_once("CREATE INDEX")
        .context("schema indexes missing")?;
    if !build_inputs.schema_ready {
        conn.execute_batch(tables)?;
    }
    let linking = Instant::now();
    let dependencies = build_inputs.dependency_registry;
    let graph = linker::link_compact_with_typed_registry(facts, None, Some(root), dependencies);
    let link_ms = linking.elapsed().as_secs_f64() * 1000.;
    let writing = Instant::now();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    cached(
        &tx,
        "INSERT INTO node_search(node_search) VALUES('delete-all')",
        [],
    )?;
    cached(
        &tx,
        "INSERT INTO doc_search(doc_search) VALUES('delete-all')",
        [],
    )
    .ok();
    cached(&tx, "DELETE FROM shared_keys", [])?;
    for table in commitments::DOMAINS {
        cached(&tx, &format!("DELETE FROM {table}"), [])?;
    }
    cached(&tx, "DELETE FROM path_dictionary", [])?;
    cached(&tx, "DELETE FROM domain_commitments", [])?;
    cached(&tx, "DELETE FROM coverage_evidence", [])?;
    cached(&tx, "DELETE FROM coverage_expressions", [])?;
    for (k, v) in [
        ("schema_version", scanner::ENGINE_SCHEMA_VERSION.to_owned()),
        (
            "index_semantics_version",
            scanner::INDEX_SEMANTICS_VERSION.to_owned(),
        ),
        ("indexer_engine", "contextunity-forge-mcp-rust".to_owned()),
        ("workspace_root", root.to_string_lossy().into_owned()),
        ("adapter", policy(root, adapter).to_string()),
        (
            "adapter_version",
            adapter.adapter_version.clone().unwrap_or_default(),
        ),
        ("adapter_digest", adapter.digest.clone()),
        ("manifest_digest", dependencies.digest().to_owned()),
        (
            "adapter_linked_workspaces",
            serde_json::to_string(&adapter.linked_workspaces)?,
        ),
        ("inventory_snapshot", serde_json::to_string(entries)?),
        (
            "corpus_hash",
            commitments::hash(&serde_json::to_vec(entries)?),
        ),
    ] {
        cached(
            &tx,
            "INSERT OR REPLACE INTO metadata VALUES(?1,?2)",
            params![k, v],
        )?;
    }
    let mut path_cache = PathDictionaryCache::new();
    let all_paths = entries
        .iter()
        .map(|e| e.path.as_str())
        .chain(graph.edges.iter().map(|e| e.path.as_str()))
        .chain(graph.coverage.iter().map(|c| c.path.as_str()))
        .chain(
            entries
                .iter()
                .filter_map(|entry| facts.get(&entry.path))
                .flat_map(|file_facts| file_facts.nodes.iter().map(|node| node.path.as_str())),
        );
    let t_bulk = Instant::now();
    path_cache.bulk_insert(&tx, all_paths)?;
    let bulk_paths_ms = t_bulk.elapsed().as_secs_f64() * 1000.;
    let mut node_paths: hashbrown::HashMap<String, Option<CachedNodePath>> =
        hashbrown::HashMap::with_capacity(facts.values().map(|f| f.nodes.len()).sum());
    let mut shared_paths = hashbrown::HashMap::<&str, Arc<str>>::new();
    for f in facts.values() {
        for n in &f.nodes {
            let shared_path = shared_paths
                .entry(n.path.as_str())
                .or_insert_with(|| Arc::<str>::from(n.path.as_str()));
            node_paths.insert(
                n.id.clone(),
                Some(CachedNodePath::from_shared(&n.id, Arc::clone(shared_path))),
            );
        }
    }

    let t_files = Instant::now();
    let file_timings = persist_files(&tx, &mut path_cache, entries, facts, encoded_facts, true)?;
    let node_id = file_timings.node_id;
    let node_row_count = file_timings.node_row_count;
    let node_row_insert_ms = file_timings
        .node_row_insert_ms
        .context("cold file persistence must report node row insertion time")?;
    let docs_count = file_timings.docs_count;
    let fts_insert_ms = file_timings.fts_insert_ms;
    let encode_facts_ms = file_timings.encode_facts_ms;
    let local_facts_batch_rows = file_timings.local_facts_batch_rows;
    let persist_files_ms = t_files.elapsed().as_secs_f64() * 1000. - fts_insert_ms;
    let t_doc_fts = Instant::now();
    if docs_count > 0 {
        tx.execute(
            "INSERT INTO doc_search(rowid,section_title,content,invariants) SELECT rowid,section_title,content,invariants FROM doc_sections",
            [],
        )?;
    }
    let doc_fts_insert_ms = t_doc_fts.elapsed().as_secs_f64() * 1000.;

    let t_graph = Instant::now();
    let graph_timings = persist_graph(
        &tx,
        &mut path_cache,
        &graph,
        facts,
        None,
        &mut node_paths,
        true,
    )?;
    let persist_graph_ms = t_graph.elapsed().as_secs_f64() * 1000.;

    let rows_ms = writing.elapsed().as_secs_f64() * 1000.;
    tx.commit()?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let index_and_seal = Instant::now();
    let index_sql = format!("CREATE INDEX{indexes}");
    let index_candidate = |conn: &mut Connection| -> Result<f64> {
        let started = Instant::now();
        conn.execute_batch("PRAGMA cache_size=-262144; PRAGMA temp_store=MEMORY;")?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(&index_sql)?;
        tx.commit()?;
        Ok(started.elapsed().as_secs_f64() * 1000.)
    };
    let indexes_ms = index_candidate(conn)?;
    let started = Instant::now();
    let seal = commitments::seal_snapshot(conn)?;
    let seal_ms = started.elapsed().as_secs_f64() * 1000.;
    conn.execute_batch(&format!("PRAGMA cache_size=-{cache_kb};"))?;
    let index_and_seal_ms = index_and_seal.elapsed().as_secs_f64() * 1000.;
    let persist_ms = writing.elapsed().as_secs_f64() * 1000.;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    let mut report = json!({"files":entries.len(),"nodes":node_id,"node_row_count":node_row_count,"node_row_insert_ms":node_row_insert_ms,"edges":graph.edges.len(),"doc_sections":docs_count,"output_root":seal,"schema_version":scanner::ENGINE_SCHEMA_VERSION,"link_ms":link_ms,"persist_ms":persist_ms,"rows_ms":rows_ms,"bulk_paths_ms":bulk_paths_ms,"persist_files_ms":persist_files_ms,"fts_insert_ms":fts_insert_ms,"doc_fts_insert_ms":doc_fts_insert_ms,"persist_graph_ms":persist_graph_ms,"indexes_ms":indexes_ms,"seal_ms":seal_ms,"encode_facts_ms":encode_facts_ms,"sqlite_ingestion":{"mode":"deferred-index-batched","secondary_indexes_deferred":true,"batch_rows":local_facts_batch_rows}});
    report["graph_phases_ms"] = json!({"occurrences_and_dependencies":graph_timings.occurrences,"unique_edges":graph_timings.edges,"coverage_dictionaries":graph_timings.dictionaries,"coverage_and_unresolved":graph_timings.coverage});
    report["index_and_seal_ms"] = json!(index_and_seal_ms);
    report["framework_rule_diagnostics"] = json!(dependencies.framework_rule_diagnostics(
        entries
            .iter()
            .map(|file| (file.path.as_str(), file.language.as_str()))
    ));
    Ok(report)
}
fn components(facts: &mut BTreeMap<String, TypedFacts>) {
    let owners = component_owners(facts.iter().filter_map(|(path, f)| {
        f.nodes
            .iter()
            .any(|n| n.kind == "module")
            .then_some(path.as_str())
    }));
    assign_components(facts, &owners);
}
fn component_owners<'a>(paths: impl Iterator<Item = &'a str>) -> BTreeMap<String, String> {
    let mut owners: BTreeMap<String, String> = BTreeMap::new();
    for path in paths {
        let root = path.split_once('/').map_or(".", |(root, _)| root);
        owners
            .entry(root.to_owned())
            .and_modify(|owner| {
                if path < owner.as_str() {
                    *owner = path.to_owned();
                }
            })
            .or_insert_with(|| path.to_owned());
    }
    owners
}
fn assign_components(facts: &mut BTreeMap<String, TypedFacts>, owners: &BTreeMap<String, String>) {
    for (path, f) in facts {
        f.nodes.retain(|n| n.kind != "component");
        f.edges.retain(|e| !e.src.starts_with("component:"));
        if !f.nodes.iter().any(|n| n.kind == "module") {
            continue;
        }
        let root = path.split_once('/').map_or(".", |(root, _)| root);
        let id = format!("component:{root}");
        if owners.get(root) == Some(path) {
            f.nodes.push(Node {
                id: id.clone(),
                kind: "component".into(),
                name: root.into(),
                qualname: root.into(),
                path: root.into(),
                line: 1,
                end_line: 1,
                is_test: false,
                language: "workspace".into(),
                generated: false,
                details: json!({}),
            });
        }
        f.edges.push(Edge {
            src: id,
            dst: format!("module:{path}"),
            kind: "contains".into(),
            path: path.clone(),
            line: 1,
            evidence: "component ownership".into(),
            confidence: "exact".into(),
        });
    }
}
fn atomic_build(
    root: &Path,
    output: &Path,
    adapter: &scanner::Adapter,
    dependencies: Arc<DependencyRegistry>,
    memory_budget: u64,
    #[cfg(test)] test_hooks: Option<Arc<PipelineTestHooks>>,
) -> Result<Value> {
    let output = if output.is_absolute() {
        output.to_owned()
    } else {
        std::env::current_dir()?.join(output)
    };
    let parent = output.parent().context("output needs a parent")?;
    let mut ancestor = PathBuf::new();
    for part in parent.components() {
        ancestor.push(part);
        if fs::symlink_metadata(&ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("output parent must not traverse a symlink");
        }
    }
    fs::create_dir_all(parent)?;
    let parent = parent.canonicalize()?;
    if fs::symlink_metadata(&output).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("output must not be a symlink");
    }
    let stage = parent.join(format!(
        ".forge-stage-{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&stage)?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(&stage)?;
    }
    let temporary = stage.join("generation.sqlite");
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&temporary)?;
        let mut conn = Connection::open_with_flags(
            &temporary,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        conn.execute_batch("PRAGMA page_size=32768;")?;
        prepare_pipeline_schema(
            &mut conn,
            memory_budget,
            #[cfg(test)]
            test_hooks.as_deref(),
        )?;
        let (mut streamed, pipeline_metrics) = run_cold_pipeline(
            root,
            adapter,
            Arc::clone(&dependencies),
            conn,
            memory_budget,
            #[cfg(test)]
            test_hooks.clone(),
        )?;
        streamed
            .entries
            .sort_by(|left, right| left.path.cmp(&right.path));
        let preparing_components = Instant::now();
        let _ = ast::route_includes::compose_django_route_includes(&mut streamed.facts);
        components(&mut streamed.facts);
        let components_ms = preparing_components.elapsed().as_secs_f64() * 1000.;
        let mut report = populate(
            &mut streamed.connection,
            root,
            adapter,
            &streamed.entries,
            &streamed.facts,
            None,
            BuildInputs {
                dependency_registry: &dependencies,
                memory_budget,
                schema_ready: true,
            },
        )?;
        report["memory_budget_bytes"] = json!(memory_budget);
        report["scanned_files"] = json!(pipeline_metrics.scanned_files);
        report["scanned_bytes"] = json!(pipeline_metrics.scanned_bytes);
        report["scan_ms"] = json!(pipeline_metrics.scan_ms);
        report["extract_ms"] = json!(pipeline_metrics.extraction_ms);
        report["components_ms"] = json!(components_ms);
        report["pipeline"] = json!({
            "budget_bytes": pipeline_metrics.budget_bytes,
            "peak_in_flight_bytes": pipeline_metrics.peak_in_flight_bytes,
            "max_item_bytes": pipeline_metrics.max_item_bytes,
            "oversized_admissions": pipeline_metrics.oversized_admissions,
            "extracted_files": pipeline_metrics.extracted_files,
            "ingested_files": pipeline_metrics.ingested_files,
        });
        report["sqlite_ingest_ms"] = json!(pipeline_metrics.sqlite_ingest_ms);
        let verifying = Instant::now();
        let integrity: String = streamed
            .connection
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            bail!("candidate database failed integrity check: {integrity}");
        }
        report["verify_ms"] = json!(verifying.elapsed().as_secs_f64() * 1000.);
        streamed
            .connection
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        streamed.connection.close().map_err(|(_, e)| e)?;
        let _generation_lock = super::cache::exclusive_lock(&output)?;
        let wal = PathBuf::from(format!("{}-wal", output.display()));
        let shm = PathBuf::from(format!("{}-shm", output.display()));
        let _ = fs::remove_file(&wal);
        let _ = fs::remove_file(&shm);
        fs::rename(&temporary, &output)?;
        report["verification_cache"] = json!(super::cache::publish_verified(
            &output,
            root,
            report["output_root"]
                .as_str()
                .context("missing output root")?
        )
        .unwrap_or(false));
        Ok(report)
    })();
    let _ = fs::remove_dir_all(stage);
    result
}

/// Builds and publishes a sealed index for the workspace.
///
/// Scanning, extraction, and local-fact ingestion overlap under a shared byte
/// budget. Graph rows are persisted before the final Merkle seal and publication.
///
/// # Errors
///
/// Rejects unsafe roots, changed source snapshots, storage failures, and seal failures.
pub fn build(root: &Path, output: &Path, adapter_path: Option<&Path>) -> Result<Value> {
    build_inner(
        root,
        output,
        adapter_path,
        #[cfg(test)]
        None,
    )
}

#[cfg(test)]
fn build_with_test_hooks(
    root: &Path,
    output: &Path,
    adapter_path: Option<&Path>,
    test_hooks: Arc<PipelineTestHooks>,
) -> Result<Value> {
    build_inner(root, output, adapter_path, Some(test_hooks))
}

fn build_inner(
    root: &Path,
    output: &Path,
    adapter_path: Option<&Path>,
    #[cfg(test)] test_hooks: Option<Arc<PipelineTestHooks>>,
) -> Result<Value> {
    let started = Instant::now();
    let root = scanner::canonical_root(root)?;
    let adapter = scanner::load_adapter(&root, adapter_path)?;
    scanner::check_root_scope(&root, adapter.limits.allow_broad_root)?;
    let dependency_manifests = scanner::dependency_manifests_for_build(&root, &adapter)?;
    let dependencies =
        crate::engine::languages::dependency_registry::DependencyRegistry::try_collect_with_paths(
            &root,
            Some(&adapter),
            &dependency_manifests,
        )?;
    let budget = scanner::resolve_memory_budget_bytes(&adapter.limits)?;
    let dependencies = Arc::new(dependencies);
    let mut report = atomic_build(
        &root,
        output,
        &adapter,
        dependencies,
        budget,
        #[cfg(test)]
        test_hooks,
    )?;
    report["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    report["output"] = json!(output);
    Ok(report)
}
fn cached(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> rusqlite::Result<usize> {
    conn.prepare_cached(sql)?.execute(params)
}

fn reference_keys<'a>(facts: &'a TypedFacts) -> Vec<&'a str> {
    let mut keys = Vec::new();
    let mut insert = |expression: &'a str| {
        keys.push(expression);
        for token in expression.split(['.', ':', '/']) {
            if !token.is_empty() && token.chars().all(|c| c.is_alphanumeric() || c == '_') {
                keys.push(token);
            }
        }
    };
    for expression in facts
        .references
        .iter()
        .filter(|r| !r.dynamic)
        .flat_map(|r| std::iter::once(r.expression.as_str()).chain(r.module.as_deref()))
        .chain(
            facts
                .nodes
                .iter()
                .filter(|n| n.kind == "impl")
                .map(|n| n.name.split('<').next().unwrap_or(&n.name).trim()),
        )
        .chain(
            facts
                .docs
                .iter()
                .flat_map(|d| d.referenced_symbols.iter().map(String::as_str)),
        )
    {
        insert(expression);
    }
    for reference in &facts.references {
        if let Some(
            ReceiverHint::CallResult { callee, .. }
            | ReceiverHint::ConstructorResult { callee, .. },
        ) = &reference.receiver_hint
        {
            insert(callee);
        }
    }
    for node in &facts.nodes {
        if let Some(flow) = facts.flows.get(node) {
            flow.reference_keys(&mut insert);
        } else if let Some(raw) = facts.flows.raw(node) {
            crate::core::semantic::ValueFlowFacts::reference_keys_from_value(raw, &mut insert);
        } else {
            crate::core::semantic::ValueFlowFacts::reference_keys_from_details(
                &node.details,
                &mut insert,
            );
        }
        if let Some(exports) = node.details.get("exports").and_then(Value::as_array) {
            for export in exports {
                for key in ["local", "module"] {
                    if let Some(name) = export.get(key).and_then(Value::as_str) {
                        insert(name);
                    }
                }
            }
        }
        if let Some(types) = node.details.get("param_types").and_then(Value::as_object) {
            for ty in types.values().filter_map(Value::as_str) {
                insert(ty);
            }
        }
    }
    if keys.len() >= 8192 {
        keys.par_sort_unstable();
    } else {
        keys.sort_unstable();
    }
    keys.dedup();
    keys
}

#[cfg(test)]
#[path = "writer/memory_budget_tests.rs"]
mod memory_budget_tests;
#[cfg(test)]
#[path = "writer/multi_value_batch_tests.rs"]
mod multi_value_batch_tests;
