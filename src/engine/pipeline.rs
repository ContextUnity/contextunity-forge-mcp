//! Indexing pipeline orchestration for cold builds and incremental delta runs.

use crate::core::{
    commitments,
    config::{
        self, checked_child, load_adapter, resolve_file_path, resolve_memory_budget_bytes, Adapter,
        ENGINE_SCHEMA_VERSION, INDEX_SEMANTICS_VERSION,
    },
    fs::canonical_root,
    models::{stable_hash64, Edge, Facts, FileEntry, Node},
    typed_facts::{decode_facts, TypedFacts},
};
use crate::db::writer::cache::{self, ExtractionCache};
use crate::engine::{
    ast, docs,
    languages::{dependency_registry::DependencyRegistry, module_name},
    linker,
    scanner::{self, BuildCandidate},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
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

static NONCE: AtomicU64 = AtomicU64::new(0);
const EXTRACTION_MEMORY_MULTIPLIER: u64 = 8;

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
    extraction_cache_hits: AtomicUsize,
    extraction_nanos: AtomicU64,
    ingestion_nanos: AtomicU64,
}

struct PipelineWork {
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
    adapter: Arc<Adapter>,
    dependencies: Arc<DependencyRegistry>,
    extraction_cache: Arc<ExtractionCache>,
    effective_ruleset_hash: Arc<str>,
    work_rx: Arc<Mutex<Receiver<PipelineWork>>>,
    result_tx: SyncSender<std::result::Result<PipelineItem, String>>,
    control: Arc<PipelineControl>,
    counters: Arc<PipelineCounters>,
    test_hooks: Option<Arc<PipelineTestHooks>>,
}

struct ColdPipelineInputs<'a> {
    root: &'a Path,
    adapter: &'a Adapter,
    dependencies: Arc<DependencyRegistry>,
    extraction_cache: Arc<ExtractionCache>,
    effective_ruleset_hash: Arc<str>,
    connection: Connection,
    budget_bytes: u64,
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
    extraction_cache_hits: usize,
    scan_ms: f64,
    extraction_ms: f64,
    sqlite_ingest_ms: f64,
    scanned_files: usize,
    scanned_bytes: u64,
}

/// Test synchronization and fault-injection hooks for pipeline concurrency.
pub struct PipelineTestHooks {
    /// Forces producer, worker, and consumer synchronization.
    pub overlap: bool,
    /// Simulates SQLite consumer failure.
    pub fail_consumer: bool,
    /// Coordination barrier for concurrency overlap testing.
    pub overlap_barrier: std::sync::Barrier,
    /// Tracks count of participants that reached the overlap barrier.
    pub overlap_participants: AtomicUsize,
}

impl PipelineTestHooks {
    /// Creates overlap synchronization hooks.
    pub fn overlap() -> Arc<Self> {
        Arc::new(Self {
            overlap: true,
            fail_consumer: false,
            overlap_barrier: std::sync::Barrier::new(3),
            overlap_participants: AtomicUsize::new(0),
        })
    }

    /// Creates consumer-failure fault injection hooks.
    pub fn fail_consumer() -> Arc<Self> {
        Arc::new(Self {
            overlap: false,
            fail_consumer: true,
            overlap_barrier: std::sync::Barrier::new(3),
            overlap_participants: AtomicUsize::new(0),
        })
    }
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
pub fn extract(root: &Path, file: &FileEntry, adapter: &Adapter) -> Result<Facts> {
    let dependencies = DependencyRegistry::try_collect_with_scan_config(root, Some(adapter))?;
    Ok(extract_typed(root, file, adapter, &dependencies)?.into_public())
}

fn extract_typed(
    root: &Path,
    file: &FileEntry,
    adapter: &Adapter,
    dependencies: &DependencyRegistry,
) -> Result<TypedFacts> {
    let path = resolve_file_path(root, adapter, &file.path)?;
    let source = fs::read_to_string(&path)?;
    extract_typed_source(file, &source, dependencies)
}

fn extract_typed_source(
    file: &FileEntry,
    source: &str,
    dependencies: &DependencyRegistry,
) -> Result<TypedFacts> {
    let source_digest = commitments::hash(source.as_bytes());
    extract_typed_source_with_digest(file, source, &source_digest, dependencies)
}

fn extract_typed_source_with_digest(
    file: &FileEntry,
    source: &str,
    source_digest: &str,
    dependencies: &DependencyRegistry,
) -> Result<TypedFacts> {
    if source_digest != file.digest {
        return Err(SourceSnapshotMismatch(format!(
            "source changed during extraction: {}",
            file.path
        ))
        .into());
    }
    let mut typed = if file.is_doc {
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
        TypedFacts::from_public(Facts {
            docs: sections,
            nodes,
            ..Facts::default()
        })
    } else {
        ast::extract_typed_with_registry(&file.path, &file.language, source, dependencies)?
    };
    sanitize_extracted_facts(&mut typed);
    Ok(typed)
}

fn sanitize_extracted_facts(facts: &mut TypedFacts) {
    for node in &mut facts.facts.nodes {
        if node.details.get("classic_global") == Some(&serde_json::Value::Bool(true)) {
            if facts.flows.state(node).is_some() {
                if let Some(details) = node.details.as_object_mut() {
                    details.insert("value_flow".into(), serde_json::Value::Null);
                }
            } else if let Some(details) = node.details.as_object_mut() {
                details.remove("value_flow");
            }
        }
    }
}

fn extract_candidate(
    root: &Path,
    candidate: &BuildCandidate,
    adapter: &Adapter,
    dependencies: &DependencyRegistry,
    extraction_cache: &ExtractionCache,
    effective_ruleset_hash: &str,
) -> Result<(FileEntry, TypedFacts, bool)> {
    let path = resolve_file_path(root, adapter, &candidate.path)?;
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
    let key = cache::key(
        &source_digest,
        &candidate.path,
        &candidate.language,
        INDEX_SEMANTICS_VERSION,
        effective_ruleset_hash,
    );
    let cache_limit = candidate
        .bytes
        .saturating_mul(64)
        .max(512 * 1024)
        .min(adapter.limits.max_file_bytes.saturating_mul(16));
    if let Some(mut facts) = extraction_cache.load(&key, cache_limit) {
        let mtime = candidate.mtime_ns as f64 / 1e9;
        for section in &mut facts.docs {
            section.mtime = mtime;
        }
        sanitize_extracted_facts(&mut facts);
        return Ok((file, facts, true));
    }
    let facts = extract_typed_source_with_digest(&file, &source, &source_digest, dependencies)?;
    extraction_cache.store(&key, &facts);
    Ok((file, facts, false))
}

fn run_cold_pipeline(
    inputs: ColdPipelineInputs<'_>,
) -> Result<(PipelineConsumerOutput, PipelineMetrics)> {
    let ColdPipelineInputs {
        root,
        adapter,
        dependencies,
        extraction_cache,
        effective_ruleset_hash,
        connection,
        budget_bytes,
        test_hooks,
    } = inputs;
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
    let consumer_hooks = test_hooks.clone();
    let consumer = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            consume_pipeline_results(
                connection,
                result_rx,
                consumer_control.as_ref(),
                consumer_counters.as_ref(),
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
        let worker_extraction_cache = Arc::clone(&extraction_cache);
        let worker_ruleset_hash = Arc::clone(&effective_ruleset_hash);
        let worker_rx = Arc::clone(&work_rx);
        let worker_tx = result_tx.clone();
        let worker_control = Arc::clone(&control);
        let panic_control = Arc::clone(&worker_control);
        let worker_counters = Arc::clone(&counters);
        let worker_hooks = test_hooks.clone();
        workers.push(thread::spawn(move || {
            let context = ExtractionWorkerContext {
                root: worker_root,
                adapter: worker_adapter,
                dependencies: worker_dependencies,
                extraction_cache: worker_extraction_cache,
                effective_ruleset_hash: worker_ruleset_hash,
                work_rx: worker_rx,
                result_tx: worker_tx,
                control: worker_control,
                counters: worker_counters,
                test_hooks: worker_hooks,
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| extraction_worker(context)));
            if outcome.is_err() {
                panic_control.cancel("cold pipeline extraction worker panicked");
            }
        }));
    }
    drop(result_tx);
    drop(work_rx);

    let producer_control = Arc::clone(&control);
    let producer_adapter = Arc::clone(&adapter);
    let producer_budget = Arc::clone(&byte_budget);
    let producer_hooks = test_hooks.clone();
    let producer = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let started = Instant::now();
            let mut sequence = 0usize;
            let scan = scanner::stream_for_build(&root, &producer_adapter, |candidate| {
                let estimate = candidate
                    .bytes
                    .saturating_mul(EXTRACTION_MEMORY_MULTIPLIER)
                    .max(1);
                let permit = producer_budget
                    .acquire(estimate, &producer_control.cancelled)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                let current_sequence = {
                    let current = sequence;
                    sequence = sequence.saturating_add(1);
                    current
                };
                work_tx
                    .send(PipelineWork {
                        sequence: current_sequence,
                        candidate,
                        permit,
                    })
                    .map_err(|error| {
                        std::io::Error::other(format!(
                            "cold pipeline worker channel closed: {error}"
                        ))
                    })?;
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
        extraction_cache_hits: counters.extraction_cache_hits.load(Ordering::Acquire),
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
        extraction_cache,
        effective_ruleset_hash,
        work_rx,
        result_tx,
        control,
        counters,
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
        if test_hooks
            .as_deref()
            .is_some_and(|hooks| hooks.overlap && work.sequence == 1)
        {
            let hooks = test_hooks.as_deref().expect("checked above");
            hooks.overlap_participants.fetch_add(1, Ordering::AcqRel);
            hooks.overlap_barrier.wait();
        }
        let extraction_started = Instant::now();
        let extracted = extract_candidate(
            &root,
            &work.candidate,
            &adapter,
            &dependencies,
            &extraction_cache,
            &effective_ruleset_hash,
        );
        counters.extraction_nanos.fetch_add(
            extraction_started
                .elapsed()
                .as_nanos()
                .min(u64::MAX as u128) as u64,
            Ordering::AcqRel,
        );
        let (file, facts, cache_hit) = match extracted {
            Ok(value) => value,
            Err(error) => {
                let message = error.to_string();
                control.cancel(&message);
                let _ = result_tx.send(Err(message));
                drop(work);
                continue;
            }
        };
        if cache_hit {
            counters
                .extraction_cache_hits
                .fetch_add(1, Ordering::AcqRel);
        }
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
    test_hooks: Option<&PipelineTestHooks>,
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
    adapter: &Adapter,
    dependencies: Arc<DependencyRegistry>,
    extraction_cache: Arc<ExtractionCache>,
    memory_budget: u64,
    test_hooks: Option<Arc<PipelineTestHooks>>,
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
        crate::db::writer::prepare_pipeline_schema(
            &mut conn,
            memory_budget,
            test_hooks.as_deref().is_some_and(|h| h.fail_consumer),
        )?;
        let (mut streamed, pipeline_metrics) = run_cold_pipeline(ColdPipelineInputs {
            root,
            adapter,
            dependencies: Arc::clone(&dependencies),
            extraction_cache: Arc::clone(&extraction_cache),
            effective_ruleset_hash: Arc::<str>::from(cache::effective_ruleset_hash(
                &adapter.digest,
                dependencies.digest(),
            )),
            connection: conn,
            budget_bytes: memory_budget,
            test_hooks: test_hooks.clone(),
        })?;
        streamed
            .entries
            .sort_by(|left, right| left.path.cmp(&right.path));
        let preparing_components = Instant::now();
        let _ = ast::route_includes::compose_django_route_includes(&mut streamed.facts);
        components(&mut streamed.facts);
        let components_ms = preparing_components.elapsed().as_secs_f64() * 1000.;

        let linking = Instant::now();
        let graph = linker::link_compact_with_typed_registry(&streamed.facts, None, Some(root), &dependencies);
        let link_ms = linking.elapsed().as_secs_f64() * 1000.;

        let mut report = crate::db::writer::populate(
            &mut streamed.connection,
            root,
            adapter,
            &streamed.entries,
            &streamed.facts,
            &graph,
            crate::db::writer::BuildInputs {
                manifest_digest: dependencies.digest(),
                memory_budget,
                schema_ready: true,
            },
        )?;
        report["link_ms"] = json!(link_ms);
        report["preencoded_facts_ms"] = json!(0.0);
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
            "extraction_cache_hits": pipeline_metrics.extraction_cache_hits,
        });
        report["sqlite_ingest_ms"] = json!(pipeline_metrics.sqlite_ingest_ms);
        report["framework_rule_diagnostics"] = json!(dependencies.framework_rule_diagnostics(
            streamed
                .entries
                .iter()
                .map(|file| (file.path.as_str(), file.language.as_str()))
        ));
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
        let _generation_lock = crate::db::cache::exclusive_lock(&output)?;
        let wal = PathBuf::from(format!("{}-wal", output.display()));
        let shm = PathBuf::from(format!("{}-shm", output.display()));
        let _ = fs::remove_file(&wal);
        let _ = fs::remove_file(&shm);
        fs::rename(&temporary, &output)?;
        report["verification_cache"] = json!(crate::db::cache::publish_verified(
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
pub fn build(root: &Path, output: &Path, adapter_path: Option<&Path>) -> Result<Value> {
    build_inner(root, output, adapter_path, None)
}

/// Builds index with test synchronization and fault-injection hooks.
pub fn build_with_test_hooks(
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
    test_hooks: Option<Arc<PipelineTestHooks>>,
) -> Result<Value> {
    build_inner_with_cache(
        root,
        output,
        adapter_path,
        Arc::new(ExtractionCache::new(cache::default_cache_root())),
        test_hooks,
    )
}

/// Builds index overriding default extraction cache directory.
pub fn build_with_cache_dir(
    root: &Path,
    output: &Path,
    adapter_path: Option<&Path>,
    cache_dir: &Path,
) -> Result<Value> {
    build_inner_with_cache(
        root,
        output,
        adapter_path,
        Arc::new(ExtractionCache::new(Some(cache_dir.to_owned()))),
        None,
    )
}

fn build_inner_with_cache(
    root: &Path,
    output: &Path,
    adapter_path: Option<&Path>,
    extraction_cache: Arc<ExtractionCache>,
    test_hooks: Option<Arc<PipelineTestHooks>>,
) -> Result<Value> {
    let started = Instant::now();
    let root = canonical_root(root)?;
    let adapter = load_adapter(&root, adapter_path)?;
    config::check_root_scope(&root, adapter.limits.allow_broad_root)?;
    let dependency_manifests = scanner::dependency_manifests_for_build(&root, &adapter)?;
    let dependencies =
        DependencyRegistry::try_collect_with_paths(
            &root,
            Some(&adapter),
            &dependency_manifests,
        )?;
    let budget = resolve_memory_budget_bytes(&adapter.limits)?;
    let dependencies = Arc::new(dependencies);
    let mut report = atomic_build(
        &root,
        output,
        &adapter,
        dependencies,
        extraction_cache,
        budget,
        test_hooks,
    )?;
    report["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    report["output"] = json!(output);
    Ok(report)
}

// ---------------------------------------------------------------------------
// Incremental Delta Indexing
// ---------------------------------------------------------------------------

fn delta_components(
    conn: &Connection,
    modified: &BTreeSet<String>,
    facts: &BTreeMap<String, TypedFacts>,
    replace_files: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    let mut st = conn.prepare("SELECT path.path,owner.path FROM nodes n JOIN path_dictionary path ON path.path_id=n.path_id JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE n.kind='component'")?;
    let previous = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let mut st = conn.prepare("SELECT owner.path FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE n.kind='module' AND owner.path NOT IN(SELECT value FROM json_each(?1))")?;
    let paths = st
        .query_map([serde_json::to_string(modified)?], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let current = component_owners(paths.iter().map(String::as_str).chain(
        facts.iter().filter_map(|(path, f)| {
            f.nodes
                .iter()
                .any(|n| n.kind == "module")
                .then_some(path.as_str())
        }),
    ));
    for (root, owner) in &previous {
        if current.get(root) != Some(owner) {
            replace_files.insert(owner.clone());
        }
    }
    for (root, owner) in &current {
        if previous.get(root) != Some(owner) {
            replace_files.insert(owner.clone());
        }
    }
    Ok(current)
}

type RustImplShapes = BTreeMap<String, BTreeMap<String, String>>;

fn rust_impl_shapes(facts: &Facts) -> Result<RustImplShapes> {
    let imports: BTreeSet<_> = facts
        .references
        .iter()
        .filter(|reference| reference.kind == "imports")
        .map(|reference| {
            (
                reference.expression.as_str(),
                reference.alias.as_deref(),
                reference.module.as_deref(),
            )
        })
        .collect();
    let mut shapes = RustImplShapes::new();
    let mut implementation_keys = std::collections::HashMap::new();
    for implementation in facts.nodes.iter().filter(|node| node.kind == "impl") {
        let identity = serde_json::json!({
            "receiver_type": implementation.details["receiver_type"],
            "impl_trait": implementation.details["impl_trait"],
            "generic_impl": implementation.details["generic_impl"],
            "uniform_generic_impl": implementation.details["uniform_generic_impl"],
            "imports": &imports,
        });
        let key = serde_json::to_string(&identity)?;
        implementation_keys.insert(implementation.id.as_str(), key.clone());
        shapes.entry(key).or_default();
    }
    let by_id: std::collections::HashMap<_, _> = facts
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    for edge in facts.edges.iter().filter(|edge| edge.kind == "contains") {
        let (Some(implementation), Some(method)) = (
            implementation_keys.get(edge.src.as_str()),
            by_id.get(edge.dst.as_str()),
        ) else {
            continue;
        };
        if method.kind != "method" {
            continue;
        }
        let signature = serde_json::json!({
            "name": method.name,
            "is_static": method.details["is_static"],
            "unproven_bounds": method.details["unproven_bounds"],
            "signature": method.details["signature"],
            "param_types": method.details["param_types"],
            "return_type": method.details["return_type"],
        });
        shapes
            .get_mut(implementation)
            .expect("implementation shape was initialized")
            .insert(serde_json::to_string(&signature)?, method.name.clone());
    }
    Ok(shapes)
}

fn changed_rust_impl_methods(
    conn: &Connection,
    changed_rust_paths: &BTreeSet<String>,
    facts: &BTreeMap<String, TypedFacts>,
) -> Result<BTreeSet<String>> {
    let mut changed_methods = BTreeSet::new();
    for path in changed_rust_paths {
        let old_blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT facts_blob FROM local_facts WHERE path=?1",
                [path],
                |row| row.get(0),
            )
            .optional()?;
        let old_shapes = old_blob
            .as_deref()
            .map(decode_facts)
            .transpose()?
            .map(|typed| rust_impl_shapes(&typed.facts))
            .transpose()?
            .unwrap_or_default();
        let new_shapes = facts
            .get(path)
            .map(|typed| rust_impl_shapes(&typed.facts))
            .transpose()?
            .unwrap_or_default();
        if old_shapes == new_shapes {
            continue;
        }
        for key in old_shapes
            .keys()
            .chain(new_shapes.keys())
            .collect::<BTreeSet<_>>()
        {
            if old_shapes.get(key) == new_shapes.get(key) {
                continue;
            }
            changed_methods.extend(
                old_shapes
                    .get(key)
                    .into_iter()
                    .chain(new_shapes.get(key))
                    .flat_map(|methods| methods.values().cloned()),
            );
        }
    }
    Ok(changed_methods)
}

fn django_route_include_closure(
    conn: &Connection,
    facts: &BTreeMap<String, TypedFacts>,
    modified: &BTreeSet<String>,
    inventory: &BTreeMap<&str, &FileEntry>,
) -> Result<BTreeSet<String>> {
    let mut module_paths = std::collections::HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT owner.path,n.qualname FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE n.kind='module' AND n.language='python'",
        )?;
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (path, module) = row?;
            if modified.contains(&path) || inventory.contains_key(path.as_str()) {
                module_paths.insert(
                    (
                        crate::core::config::workspace_path(&path).0.to_owned(),
                        module,
                    ),
                    path,
                );
            }
        }
    }
    for (path, typed) in facts {
        let workspace = crate::core::config::workspace_path(path).0.to_owned();
        for module in typed
            .facts
            .nodes
            .iter()
            .filter(|node| node.kind == "module" && node.language == "python")
        {
            module_paths.insert((workspace.clone(), module.qualname.clone()), path.clone());
        }
    }

    let mut include_rows = Vec::new();
    {
        let mut statement = conn.prepare(
            "SELECT owner.path,n.details FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE n.kind='route' AND json_extract(n.details,'$.include_module') IS NOT NULL",
        )?;
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (owner, details) = row?;
            if modified.contains(&owner) || !inventory.contains_key(owner.as_str()) {
                continue;
            }
            let details: serde_json::Value = serde_json::from_str(&details)?;
            if let Some(module) = details["include_module"].as_str() {
                include_rows.push((owner, module.to_owned()));
            }
        }
    }
    for (path, typed) in facts {
        for route in typed.facts.nodes.iter().filter(|node| node.kind == "route") {
            if let Some(module) = route.details["include_module"].as_str() {
                include_rows.push((path.clone(), module.to_owned()));
            }
        }
    }

    let mut relations = BTreeSet::new();
    for (parent, module) in include_rows {
        let workspace = crate::core::config::workspace_path(&parent)
            .0
            .to_owned();
        if let Some(child) = module_paths.get(&(workspace, module)) {
            relations.insert((parent, child.clone()));
        }
    }
    let mut closure = modified.clone();
    loop {
        let mut changed = false;
        for (parent, child) in &relations {
            if closure.contains(parent) || closure.contains(child) {
                changed |= closure.insert(parent.clone());
                changed |= closure.insert(child.clone());
            }
        }
        if !changed {
            break;
        }
    }
    Ok(closure)
}

fn semantic_fact_owners(
    conn: &Connection,
    facts: &BTreeMap<String, TypedFacts>,
    seeds: &BTreeSet<String>,
    inventory: &BTreeMap<&str, &FileEntry>,
) -> Result<BTreeSet<String>> {
    let mut keys = BTreeSet::new();
    for (_, typed) in facts.iter().filter(|(path, _)| seeds.contains(*path)) {
        for node in &typed.facts.nodes {
            for fact in crate::engine::languages::semantic::decode(node)? {
                keys.insert(crate::engine::languages::semantic::dependency_key(
                    &fact.identity,
                ));
            }
        }
    }
    let mut old = conn.prepare("SELECT k.key FROM shared_owners s JOIN shared_keys k ON k.key_hash=s.key_hash JOIN path_dictionary p ON p.path_id=s.owner_id WHERE s.kind_id=1 AND p.path IN(SELECT value FROM json_each(?1)) AND typeof(k.key)='text' AND k.key LIKE 'semantic:%'")?;
    for key in old.query_map([serde_json::to_string(seeds)?], |row| {
        row.get::<_, String>(0)
    })? {
        keys.insert(key?);
    }
    let mut owners = conn.prepare("SELECT DISTINCT p.path FROM shared_owners s JOIN shared_keys k ON k.key_hash=s.key_hash JOIN path_dictionary p ON p.path_id=s.owner_id WHERE s.kind_id=1 AND k.key IN(SELECT value FROM json_each(?1))")?;
    let paths = owners
        .query_map([serde_json::to_string(&keys)?], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(paths
        .into_iter()
        .filter(|path| inventory.contains_key(path.as_str()))
        .collect())
}

fn persisted_possible_call_owners(
    conn: &Connection,
    changed_rust_paths: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    if changed_rust_paths.is_empty() {
        return Ok(BTreeSet::new());
    }
    let encoded_paths = serde_json::to_string(changed_rust_paths)?;
    let mut owners = BTreeSet::new();
    let mut statement = conn.prepare(
        "SELECT DISTINCT owner.path FROM edge_occurrences candidate JOIN nodes target ON target.node_hash=candidate.dst_hash JOIN path_dictionary target_path ON target_path.path_id=target.path_id JOIN path_dictionary owner ON owner.path_id=candidate.owner_id WHERE candidate.kind='possible_calls' AND target_path.path IN(SELECT value FROM json_each(?1))",
    )?;
    for row in statement.query_map([encoded_paths], |row| row.get::<_, String>(0))? {
        owners.insert(row?);
    }
    Ok(owners)
}

fn unchanged_trait_dispatch_owners(
    conn: &Connection,
    implementation_methods: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    if implementation_methods.is_empty() {
        return Ok(BTreeSet::new());
    }
    let encoded_methods = serde_json::to_string(implementation_methods)?;
    let mut owners = BTreeSet::new();
    let mut statement = conn.prepare(
        "SELECT DISTINCT owner.path FROM edge_occurrences call JOIN nodes declaration ON declaration.node_hash=call.dst_hash JOIN path_dictionary owner ON owner.path_id=call.owner_id WHERE call.kind='calls' AND declaration.kind='method' AND declaration.name IN(SELECT value FROM json_each(?1)) AND EXISTS(SELECT 1 FROM resolution_coverage coverage JOIN coverage_expressions expression ON expression.expression_id=coverage.expression_id WHERE coverage.path_id=call.owner_id AND coverage.line=call.line AND coverage.status='unresolved' AND expression.expression='trait_object_dispatch')",
    )?;
    for row in statement.query_map([encoded_methods], |row| row.get::<_, String>(0))? {
        owners.insert(row?);
    }
    Ok(owners)
}

fn contract_mentions_import(
    value: &Value,
    imported_names: &BTreeSet<String>,
    qualified_aliases: &BTreeSet<String>,
) -> bool {
    fn qualified_alias_matches(value: &str, aliases: &BTreeSet<String>) -> bool {
        aliases.iter().any(|alias| {
            value == alias
                || value
                    .strip_prefix(alias)
                    .and_then(|suffix| suffix.strip_prefix('.'))
                    .is_some_and(|member| !member.is_empty())
        })
    }

    match value {
        Value::String(value) => {
            imported_names.contains(value) || qualified_alias_matches(value, qualified_aliases)
        }
        Value::Array(values) => values
            .iter()
            .any(|value| contract_mentions_import(value, imported_names, qualified_aliases)),
        Value::Object(values) => {
            if values
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "unknown")
            {
                return true;
            }
            if values
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "field")
            {
                let Some(receiver) = values.get("receiver").and_then(Value::as_str) else {
                    return true;
                };
                return qualified_alias_matches(receiver, qualified_aliases)
                    || !imported_names.contains(receiver);
            }
            if let Some(opaque_value) = values.get("opaque_value") {
                return opaque_value.is_null()
                    || contract_mentions_import(opaque_value, imported_names, qualified_aliases);
            }
            if values.keys().any(|key| key.starts_with("opaque")) || values.is_empty() {
                return true;
            }
            values
                .values()
                .any(|value| contract_mentions_import(value, imported_names, qualified_aliases))
        }
        _ => false,
    }
}

#[derive(Default)]
struct ImportTargetKinds {
    module: bool,
    non_module: bool,
    unknown: bool,
}

fn module_exposes_provider_contract(
    conn: &Connection,
    owner: &str,
    upstream: &str,
    facts: &mut BTreeMap<String, TypedFacts>,
    loaded_fact_files: &mut usize,
) -> Result<bool> {
    if !facts.contains_key(owner) {
        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT facts_blob FROM local_facts WHERE path=?1",
                [owner],
                |row| row.get(0),
            )
            .optional()?;
        let Some(blob) = blob else {
            return Ok(true);
        };
        facts.insert(owner.to_owned(), decode_facts(&blob)?);
        *loaded_fact_files += 1;
    }
    let typed = facts.get(owner).expect("module facts were loaded");
    let Some(module) = typed.facts.nodes.iter().find(|node| node.kind == "module") else {
        return Ok(true);
    };
    let mut statement = conn.prepare(
        "SELECT DISTINCT edge.line,evidence.evidence,target.kind FROM edge_occurrences edge JOIN nodes target ON target.node_hash=edge.dst_hash JOIN path_dictionary owner ON owner.path_id=edge.owner_id JOIN path_dictionary target_path ON target_path.path_id=target.path_id JOIN coverage_evidence evidence ON evidence.evidence_id=edge.evidence_id WHERE owner.path=?1 AND target_path.path=?2 AND edge.kind='imports'",
    )?;
    let mut import_edges: BTreeMap<(usize, String), ImportTargetKinds> = BTreeMap::new();
    for row in statement.query_map([owner, upstream], |row| {
        Ok((
            row.get::<_, usize>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (line, expression, target_kind) = row?;
        let targets = import_edges.entry((line, expression)).or_default();
        if target_kind == "module" {
            targets.module = true;
        } else if target_kind == "unknown" || target_kind.is_empty() {
            targets.unknown = true;
        } else {
            targets.non_module = true;
        }
    }
    if import_edges.is_empty() {
        return Ok(true);
    }
    let mut imported_names = BTreeSet::new();
    let mut qualified_aliases = BTreeSet::new();
    let mut imported_modules = BTreeSet::new();
    let mut matched_import_edges = BTreeSet::new();
    for reference in typed
        .facts
        .references
        .iter()
        .filter(|reference| reference.kind == "imports" && reference.source == module.id)
    {
        let Some(targets) = import_edges.get(&(reference.line, reference.expression.clone()))
        else {
            continue;
        };
        if targets.unknown || (targets.module && targets.non_module) {
            return Ok(true);
        }
        let Some(import_module) = reference.module.as_deref() else {
            return Ok(true);
        };
        let alias = reference.alias.as_deref().unwrap_or(&reference.expression);
        imported_names.insert(alias.to_owned());
        let is_typescript_namespace_import =
            reference.expression == "*" && alias != "*" && targets.module && !targets.non_module;
        let is_python_module_import =
            module.language == "python" && targets.module && !targets.non_module;
        if is_typescript_namespace_import || is_python_module_import {
            qualified_aliases.insert(alias.to_owned());
        }
        imported_modules.insert(import_module.to_owned());
        matched_import_edges.insert((reference.line, reference.expression.clone()));
    }
    if matched_import_edges.len() != import_edges.len() {
        return Ok(true);
    }

    let Some(exports) = module.details["exports"].as_array() else {
        return Ok(true);
    };
    let explicit_reexport = exports.iter().any(|export| {
        let export_module = export["module"].as_str();
        let local = export["local"].as_str();
        export_module.is_some_and(|source| imported_modules.contains(source))
            || (export_module.is_none()
                && local.is_some_and(|local| imported_names.contains(local)))
    });
    if explicit_reexport {
        return Ok(true);
    }

    let contracts = crate::engine::linker::contracts::Contracts::new_typed(typed);
    for node in typed
        .facts
        .nodes
        .iter()
        .filter(|node| matches!(node.kind.as_str(), "function" | "method"))
    {
        let exported = if module.language == "python" {
            !node.name.starts_with('_')
        } else {
            exports.iter().any(|export| {
                export["local"].as_str() == Some(node.name.as_str())
                    || export["name"].as_str() == Some(node.name.as_str())
            })
        };
        if !exported {
            continue;
        }
        let details = contracts.details(node);
        let return_contract = &details["value_flow"]["return_contract"];
        if return_contract.is_null()
            || contract_mentions_import(return_contract, &imported_names, &qualified_aliases)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn resolution_identity_changed(conn: &Connection, path: &str, facts: &TypedFacts) -> Result<bool> {
    let old_blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get(0),
        )
        .optional()?;
    let Some(old_blob) = old_blob else {
        return Ok(true);
    };
    let old_facts = decode_facts(&old_blob)?;
    let old_contracts = crate::engine::linker::contracts::Contracts::new_typed(&old_facts);
    let new_contracts = crate::engine::linker::contracts::Contracts::new_typed(facts);
    Ok(old_contracts.resolution_digest(&old_facts.facts)?
        != new_contracts.resolution_digest(&facts.facts)?)
}

fn reference_identity_changed(conn: &Connection, path: &str, facts: &TypedFacts) -> Result<bool> {
    let old: Option<Vec<u8>> = conn
        .query_row(
            "SELECT facts_blob FROM local_facts WHERE path=?1",
            [path],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    let Some(old) = old else {
        return Ok(true);
    };
    let old = decode_facts(&old)?;
    Ok(linker::reference_identity_changed(path, &old, facts))
}

fn reference_keys(facts: &TypedFacts) -> Vec<&str> {
    facts.collect_reference_keys(true)
}

/// Performs incremental delta indexing for modified source files.
pub fn delta(root: &Path, db: &Path, modified: &[PathBuf]) -> Result<Value> {
    let started = Instant::now();
    if modified.is_empty() {
        bail!("delta requires at least one modified path");
    }
    let root = canonical_root(root)?;
    let generation_lock = crate::db::cache::exclusive_lock(db)?;
    let (schema_compatible, previous_adapter_path) = {
        let probe = Connection::open_with_flags(
            db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        let schema_compatible = probe
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .as_deref()
            == Some(ENGINE_SCHEMA_VERSION);
        let raw_policy: Option<String> = probe
            .query_row(
                "SELECT value FROM metadata WHERE key='adapter'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let adapter_path = raw_policy
            .map(|raw| {
                let value: Value = serde_json::from_str(&raw)?;
                config::persisted_adapter_path(&root, &value)
            })
            .transpose()?
            .flatten();
        (schema_compatible, adapter_path)
    };
    if !schema_compatible {
        drop(generation_lock);
        return build(&root, db, previous_adapter_path.as_deref());
    }
    let admitted = crate::db::reader::open(db, &root)?;
    let admitted_identity = crate::db::cache::identity(db)?;
    let expected: String = admitted.query_row(
        "SELECT value FROM metadata WHERE key='output_root'",
        [],
        |r| r.get(0),
    )?;
    let adapter = config::read_policy(&root, &admitted)?;
    let current_adapter = load_adapter(&root, adapter.adapter_path.as_deref())?;
    config::check_root_scope(&root, current_adapter.limits.allow_broad_root)?;
    for path in modified {
        checked_child(&root, path)?;
    }
    let manifests_changed = modified.iter().any(|path| {
        path.file_name().and_then(|name| name.to_str()).is_some_and(
            crate::engine::languages::dependency_registry::is_dependency_manifest_filename,
        )
    });
    if current_adapter.digest != adapter.digest
        || current_adapter.linked_workspaces != adapter.linked_workspaces
        || manifests_changed
    {
        drop(admitted);
        drop(generation_lock);
        return build(&root, db, adapter.adapter_path.as_deref());
    }
    let dependencies =
        DependencyRegistry::try_collect_with_scan_config(&root, Some(&adapter))?;
    let previous_manifest_digest = admitted
        .query_row(
            "SELECT value FROM metadata WHERE key='manifest_digest'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if previous_manifest_digest.as_deref() != Some(dependencies.digest()) {
        drop(admitted);
        drop(generation_lock);
        return build(&root, db, adapter.adapter_path.as_deref());
    }
    let previous = crate::db::reader::inventory_snapshot(&admitted)?;
    let scanning = Instant::now();
    let scan = scanner::scan_reusing(&root, &adapter, &previous)?;
    let scan_ms = scanning.elapsed().as_secs_f64() * 1000.;
    let modified: BTreeSet<String> = modified
        .iter()
        .map(|p| {
            checked_child(&root, p)?;
            Ok(p.to_string_lossy().replace('\\', "/"))
        })
        .collect::<Result<_>>()?;
    let inventory: BTreeMap<_, _> = scan.entries.iter().map(|f| (f.path.as_str(), f)).collect();
    let previous: BTreeMap<_, _> = previous.iter().map(|f| (f.path.as_str(), f)).collect();
    for (path, old) in &previous {
        if !modified.contains(*path) && inventory.get(path).is_none_or(|f| f.digest != old.digest) {
            return Err(SourceSnapshotMismatch(format!("unlisted source change: {path}")).into());
        }
    }
    for path in inventory.keys() {
        if !previous.contains_key(path) && !modified.contains(*path) {
            return Err(SourceSnapshotMismatch(format!("unlisted source addition: {path}")).into());
        }
    }
    for path in &modified {
        if !inventory.contains_key(path.as_str()) {
            if root.join(path).exists() {
                bail!("modified path outside admitted inventory: {path}");
            }
            if !previous.contains_key(path.as_str()) {
                bail!("deletion was not in previous inventory: {path}");
            }
        }
    }
    let extracting = Instant::now();
    let mut facts = BTreeMap::new();
    let mut changed_resolution = BTreeSet::new();
    for path in &modified {
        if let Some(file) = inventory.get(path.as_str()) {
            let extracted = extract_typed(&root, file, &adapter, &dependencies)?;
            if !previous.contains_key(path.as_str())
                || resolution_identity_changed(&admitted, path, &extracted)?
                || (linker::needs_reference_identity(path, &extracted)
                    && reference_identity_changed(&admitted, path, &extracted)?)
            {
                changed_resolution.insert(path.clone());
            }
            facts.insert(path.clone(), extracted);
        } else {
            changed_resolution.insert(path.clone());
        }
    }
    let mut names = BTreeSet::new();
    let encoded = serde_json::to_string(&changed_resolution)?;
    {
        let mut st = admitted.prepare(
            "SELECT owner.path,n.name,n.qualname,n.kind FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE owner.path IN(SELECT value FROM json_each(?1))",
        )?;
        for row in st.query_map([&encoded], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })? {
            let (owner, name, qual, kind) = row?;
            names.insert(qual.clone());
            if !matches!(kind.as_str(), "field" | "method" | "route" | "document") {
                let mod_name = module_name(&owner);
                let (_, local_path) = crate::core::config::workspace_path(&owner);
                let local_module = module_name(local_path);
                let src_module = local_path
                    .split_once("/src/")
                    .map(|(_, r)| module_name(r))
                    .or_else(|| local_path.strip_prefix("src/").map(module_name));
                let is_top = qual == name
                    || qual
                        .strip_prefix(mod_name.as_str())
                        .and_then(|s| s.strip_prefix('.'))
                        == Some(name.as_str())
                    || qual
                        .strip_prefix(local_module.as_str())
                        .and_then(|s| s.strip_prefix('.'))
                        == Some(name.as_str())
                    || src_module.as_ref().is_some_and(|m| {
                        qual.strip_prefix(m.as_str())
                            .and_then(|s| s.strip_prefix('.'))
                            == Some(name.as_str())
                    });
                if is_top {
                    names.insert(name);
                }
            }
        }
    }
    for path in &changed_resolution {
        let mod_name = module_name(path);
        names.insert(mod_name.clone());
        let (_, local_path) = crate::core::config::workspace_path(path);
        let local_module = module_name(local_path);
        names.insert(local_module.clone());
        let src_module = local_path
            .split_once("/src/")
            .map(|(_, r)| module_name(r))
            .or_else(|| local_path.strip_prefix("src/").map(module_name));
        if let Some(src_mod) = &src_module {
            names.insert(src_mod.clone());
        }
        let mut stable_module = false;
        if let Some(f) = facts.get(path) {
            let previous_module: Option<(String, String, String)> = admitted
                .prepare_cached("SELECT n.id,n.qualname,n.language FROM nodes n JOIN path_dictionary owner ON owner.path_id=n.owner_path_id WHERE owner.path=?1 AND n.kind='module'")?
                .query_row([path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?;
            stable_module = previous_module
                .as_ref()
                .is_some_and(|(id, qualname, language)| {
                    f.nodes.iter().any(|n| {
                        n.kind == "module"
                            && &n.id == id
                            && &n.qualname == qualname
                            && &n.language == language
                    })
                });
            for n in &f.nodes {
                names.insert(n.qualname.clone());
                if !matches!(n.kind.as_str(), "field" | "method" | "route" | "document") {
                    let is_top = n.qualname == n.name
                        || n.qualname
                            .strip_prefix(mod_name.as_str())
                            .and_then(|s| s.strip_prefix('.'))
                            == Some(n.name.as_str())
                        || n.qualname
                            .strip_prefix(local_module.as_str())
                            .and_then(|s| s.strip_prefix('.'))
                            == Some(n.name.as_str())
                        || src_module.as_ref().is_some_and(|m| {
                            n.qualname
                                .strip_prefix(m.as_str())
                                .and_then(|s| s.strip_prefix('.'))
                                == Some(n.name.as_str())
                        });
                    if is_top {
                        names.insert(n.name.clone());
                    }
                }
            }
        }
        if !stable_module {
            if let Some((_, last)) = local_module.rsplit_once('.') {
                names.insert(last.to_owned());
            }
        }
    }
    let extract_ms = extracting.elapsed().as_secs_f64() * 1000.;
    let hydrating = Instant::now();
    let mut affected = modified.clone();
    {
        if !changed_resolution.is_empty() {
            let mut st = admitted.prepare(
                "SELECT p.path FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id WHERE d.target_hash IN(SELECT path_hash FROM files WHERE path IN(SELECT value FROM json_each(?1)))",
            )?;
            for owner in st.query_map([serde_json::to_string(&changed_resolution)?], |r| {
                r.get::<_, String>(0)
            })? {
                affected.insert(owner?);
            }
        }
        #[cfg(feature = "lang-html")]
        {
            let html_targets: BTreeMap<_, Vec<_>> = modified
                .iter()
                .filter(|path| {
                    inventory
                        .get(path.as_str())
                        .or_else(|| previous.get(path.as_str()))
                        .is_some_and(|file| file.language == "html")
                })
                .fold(BTreeMap::new(), |mut targets, path| {
                    targets
                        .entry(stable_hash64(path))
                        .or_insert_with(Vec::new)
                        .push(path.as_str());
                    targets
                });
            if !html_targets.is_empty() {
                let target_hashes: Vec<_> = html_targets.keys().copied().collect();
                let mut st = admitted.prepare(
                    "SELECT p.path,d.symbol,d.target_hash FROM dependencies d JOIN path_dictionary p ON p.path_id=d.owner_id WHERE d.kind='renders' AND d.target_hash IN(SELECT value FROM json_each(?1))",
                )?;
                let family = crate::engine::languages::LanguageFamily("python");
                for row in st.query_map([serde_json::to_string(&target_hashes)?], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })? {
                    let (owner, symbol, hash) = row?;
                    let Some(target) =
                        crate::core::config::local_template_target(&owner, &symbol)
                    else {
                        continue;
                    };
                    if inventory
                        .get(owner.as_str())
                        .is_some_and(|file| file.language == "python")
                        && html_targets
                            .get(&hash)
                            .is_some_and(|paths| paths.contains(&target.as_str()))
                        && crate::core::config::workspace_path(&owner).0
                            == crate::core::config::workspace_path(&target).0
                        && dependencies.nearest_manifest_scope_for_path(family, &owner)
                            == dependencies.nearest_manifest_scope_for_path(family, &target)
                    {
                        affected.insert(owner);
                    }
                }
            }
        }
        let name_hashes: Vec<_> = names.iter().map(|name| stable_hash64(name)).collect();
        let encoded_names = serde_json::to_string(&names)?;
        let encoded_hashes = serde_json::to_string(&name_hashes)?;
        let mut st = admitted.prepare("SELECT p.path FROM shared_owners s JOIN shared_keys k ON k.key_hash=s.key_hash LEFT JOIN nodes key_node ON typeof(k.key)='integer' AND key_node.node_id=k.key JOIN path_dictionary p ON p.path_id=s.owner_id WHERE s.kind_id=1 AND s.key_hash IN(SELECT value FROM json_each(?1)) AND (CASE WHEN typeof(k.key)='integer' THEN key_node.qualname ELSE k.key END) IN(SELECT value FROM json_each(?2))")?;
        for owner in st.query_map(params![encoded_hashes, encoded_names], |r| {
            r.get::<_, String>(0)
        })? {
            affected.insert(owner?);
        }
    }
    let changed_rust_paths: BTreeSet<_> = changed_resolution
        .iter()
        .filter(|path| {
            inventory
                .get(path.as_str())
                .or_else(|| previous.get(path.as_str()))
                .is_some_and(|file| file.language == "rust")
        })
        .cloned()
        .collect();
    let changed_impl_methods = changed_rust_impl_methods(&admitted, &changed_rust_paths, &facts)?;
    affected.extend(persisted_possible_call_owners(
        &admitted,
        &changed_rust_paths,
    )?);
    affected.extend(unchanged_trait_dispatch_owners(
        &admitted,
        &changed_impl_methods,
    )?);

    let mut loaded_fact_files = 0;
    let mut import_frontier: BTreeSet<_> = changed_resolution
        .iter()
        .filter(|path| {
            inventory
                .get(path.as_str())
                .or_else(|| previous.get(path.as_str()))
                .is_some_and(|file| {
                    matches!(
                        file.language.as_str(),
                        "python" | "javascript" | "typescript" | "vue" | "rust"
                    )
                })
        })
        .cloned()
        .collect();
    let mut visited_imports = import_frontier.clone();
    let mut expanded_imports = import_frontier.clone();
    let mut importing_owners = admitted.prepare(
        "SELECT DISTINCT owner.path,target.path FROM dependencies d JOIN path_dictionary owner ON owner.path_id=d.owner_id JOIN files target ON target.path_hash=d.target_hash WHERE d.kind='imports' AND target.path IN(SELECT value FROM json_each(?1))",
    )?;
    while !import_frontier.is_empty() {
        let importers = importing_owners
            .query_map([serde_json::to_string(&import_frontier)?], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
        let mut next = BTreeSet::new();
        for importer in importers {
            let (owner, upstream) = importer?;
            let supported = inventory.get(owner.as_str()).is_some_and(|file| {
                matches!(
                    file.language.as_str(),
                    "python" | "javascript" | "typescript" | "vue" | "rust"
                )
            });
            if supported && visited_imports.insert(owner.clone()) {
                affected.insert(owner.clone());
            }
            if supported
                && module_exposes_provider_contract(
                    &admitted,
                    &owner,
                    &upstream,
                    &mut facts,
                    &mut loaded_fact_files,
                )?
                && expanded_imports.insert(owner.clone())
            {
                next.insert(owner);
            }
        }
        import_frontier = next;
    }
    drop(importing_owners);
    let route_include_paths =
        django_route_include_closure(&admitted, &facts, &modified, &inventory)?;
    let mut replace_files = modified.clone();
    replace_files.extend(route_include_paths.iter().cloned());
    affected.extend(route_include_paths);
    let component_owners = delta_components(&admitted, &modified, &facts, &mut replace_files)?;
    affected.extend(replace_files.iter().cloned());
    let template_inputs_changed = modified.iter().any(|path| {
        inventory
            .get(path.as_str())
            .or_else(|| previous.get(path.as_str()))
            .is_some_and(|file| matches!(file.language.as_str(), "html" | "python"))
    });
    let old_loader_roots: Vec<(String, String)> = if template_inputs_changed {
        let mut st = admitted.prepare("SELECT p.path,n.name FROM path_dictionary p JOIN nodes n ON n.path_id=p.path_id WHERE p.path IN(SELECT value FROM json_each(?1)) AND n.kind='template_loader_root'")?;
        let rows = st.query_map([serde_json::to_string(&modified)?], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    } else {
        Vec::new()
    };
    if template_inputs_changed {
        let family = crate::engine::languages::LanguageFamily("python");
        let mut changed_roots = Vec::new();
        for (provider, directory) in &old_loader_roots {
            if modified.contains(provider) {
                changed_roots.push((provider.as_str(), directory.as_str()));
            }
        }
        for (provider, file_facts) in &facts {
            if !modified.contains(provider) || !inventory.contains_key(provider.as_str()) {
                continue;
            }
            for node in file_facts
                .nodes
                .iter()
                .filter(|node| node.kind == "template_loader_root")
            {
                changed_roots.push((provider.as_str(), node.name.as_str()));
            }
        }
        for (provider, directory) in changed_roots {
            let Some(scope) = dependencies.nearest_manifest_scope_for_path(family, provider) else {
                continue;
            };
            if !dependencies.declares_for_path(family, provider, "jinja2") {
                continue;
            }
            for (path, file) in &inventory {
                if file.language == "html"
                    && path
                        .strip_prefix(directory)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                    && dependencies.nearest_manifest_scope_for_path(family, path) == Some(scope)
                {
                    affected.insert((*path).to_owned());
                }
            }
        }
    }
    #[cfg(feature = "lang-html")]
    if template_inputs_changed {
        let family = crate::engine::languages::LanguageFamily("python");
        let modified_python: Vec<_> = modified
            .iter()
            .filter(|path| {
                inventory
                    .get(path.as_str())
                    .or_else(|| previous.get(path.as_str()))
                    .is_some_and(|file| file.language == "python")
            })
            .collect();
        if !modified_python.is_empty() {
            let mut old_targets = admitted.prepare(
                "SELECT DISTINCT f.path FROM dependencies d JOIN path_dictionary owner ON owner.path_id=d.owner_id JOIN files f ON f.path_hash=d.target_hash WHERE d.kind='renders' AND owner.path IN(SELECT value FROM json_each(?1))",
            )?;
            for row in old_targets.query_map([serde_json::to_string(&modified_python)?], |row| {
                row.get::<_, String>(0)
            })? {
                let target = row?;
                if inventory
                    .get(target.as_str())
                    .is_some_and(|file| file.language == "html")
                {
                    affected.insert(target);
                }
            }
            for provider in modified_python {
                let Some(provider_facts) = facts.get(provider.as_str()) else {
                    continue;
                };
                if !dependencies.declares_for_path(family, provider, "django") {
                    continue;
                }
                let scope = dependencies.nearest_manifest_scope_for_path(family, provider);
                let workspace = crate::core::config::workspace_path(provider).0;
                for node in provider_facts
                    .nodes
                    .iter()
                    .filter(|node| node.language == "python")
                {
                    let Some(flow) = provider_facts.flows.get(node) else {
                        continue;
                    };
                    for context in &flow.template_contexts {
                        let Some(target) = crate::core::config::local_template_target(
                            provider,
                            &context.target,
                        ) else {
                            continue;
                        };
                        if inventory
                            .get(target.as_str())
                            .is_some_and(|file| file.language == "html")
                            && crate::core::config::workspace_path(&target).0 == workspace
                            && dependencies.nearest_manifest_scope_for_path(family, &target)
                                == scope
                        {
                            affected.insert(target);
                        }
                    }
                }
            }
        }
        let mut frontier: BTreeSet<_> = affected
            .iter()
            .filter(|path| {
                inventory
                    .get(path.as_str())
                    .is_some_and(|file| file.language == "html")
            })
            .cloned()
            .collect();
        let mut visited = frontier.clone();
        let mut old_includes = admitted.prepare(
            "SELECT owner.path,f.path FROM dependencies d JOIN path_dictionary owner ON owner.path_id=d.owner_id JOIN files f ON f.path_hash=d.target_hash WHERE d.kind='includes' AND owner.path IN(SELECT value FROM json_each(?1))",
        )?;
        for _ in 0..16 {
            if frontier.is_empty() {
                break;
            }
            let mut next = BTreeSet::new();
            for row in old_includes.query_map([serde_json::to_string(&frontier)?], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })? {
                let (owner, target) = row?;
                if inventory
                    .get(target.as_str())
                    .is_some_and(|file| file.language == "html")
                    && crate::core::config::workspace_path(&owner).0
                        == crate::core::config::workspace_path(&target).0
                    && dependencies.nearest_manifest_scope_for_path(family, &owner)
                        == dependencies.nearest_manifest_scope_for_path(family, &target)
                    && visited.insert(target.clone())
                {
                    affected.insert(target.clone());
                    next.insert(target);
                }
            }
            for owner in &frontier {
                let Some(file_facts) = facts.get(owner.as_str()) else {
                    continue;
                };
                for reference in file_facts
                    .references
                    .iter()
                    .filter(|reference| reference.kind == "includes")
                {
                    let Some(target) = crate::core::config::local_template_target(
                        owner,
                        &reference.expression,
                    ) else {
                        continue;
                    };
                    if inventory
                        .get(target.as_str())
                        .is_some_and(|file| file.language == "html")
                        && crate::core::config::workspace_path(owner).0
                            == crate::core::config::workspace_path(&target).0
                        && dependencies.nearest_manifest_scope_for_path(family, owner)
                            == dependencies.nearest_manifest_scope_for_path(family, &target)
                        && visited.insert(target.clone())
                    {
                        affected.insert(target.clone());
                        next.insert(target);
                    }
                }
            }
            frontier = next;
        }
    }
    affected.extend(semantic_fact_owners(
        &admitted, &facts, &affected, &inventory,
    )?);
    let mut load_owners: BTreeSet<_> = affected
        .difference(&modified)
        .filter(|path| inventory.contains_key(path.as_str()) && !facts.contains_key(path.as_str()))
        .cloned()
        .collect();
    if template_inputs_changed {
        let mut roots = BTreeSet::new();
        for path in affected.iter().filter(|path| {
            inventory
                .get(path.as_str())
                .is_some_and(|file| file.language == "html")
        }) {
            let mut ancestor = path.rsplit_once('/').map(|(directory, _)| directory);
            while let Some(directory) = ancestor {
                roots.insert(directory.to_owned());
                ancestor = directory.rsplit_once('/').map(|(parent, _)| parent);
            }
        }
        if !roots.is_empty() {
            let mut st = admitted.prepare("SELECT DISTINCT p.path FROM nodes n INDEXED BY idx_nodes_name JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.name IN(SELECT value FROM json_each(?1)) AND n.kind='template_loader_root'")?;
            for row in st.query_map([serde_json::to_string(&roots)?], |row| {
                row.get::<_, String>(0)
            })? {
                let provider = row?;
                if inventory.contains_key(provider.as_str()) && !modified.contains(&provider) {
                    load_owners.insert(provider);
                }
            }
        }
    }
    #[cfg(feature = "lang-html")]
    if template_inputs_changed {
        let family = crate::engine::languages::LanguageFamily("javascript");
        let mut frontier: BTreeSet<_> = affected
            .iter()
            .filter(|path| {
                inventory
                    .get(path.as_str())
                    .is_some_and(|file| file.language == "html")
            })
            .cloned()
            .collect();
        let mut visited = frontier.clone();
        let mut incoming = admitted.prepare(
            "SELECT DISTINCT d.target_hash, owner.path FROM dependencies d JOIN path_dictionary owner ON owner.path_id=d.owner_id WHERE d.kind='includes' AND d.target_hash IN(SELECT value FROM json_each(?1))",
        )?;
        for _ in 0..16 {
            if frontier.is_empty() {
                break;
            }
            let mut targets = BTreeMap::<_, Vec<&str>>::new();
            for path in &frontier {
                targets.entry(stable_hash64(path)).or_default().push(path);
            }
            let hashes: Vec<_> = targets.keys().copied().collect();
            let mut next = BTreeSet::new();
            for row in incoming.query_map([serde_json::to_string(&hashes)?], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })? {
                let (hash, owner) = row?;
                let Some(owner_file) = inventory.get(owner.as_str()) else {
                    continue;
                };
                if owner_file.language != "html"
                    || !targets.get(&hash).is_some_and(|paths| {
                        paths.iter().any(|target| {
                            crate::core::config::workspace_path(&owner).0
                                == crate::core::config::workspace_path(target).0
                                && dependencies.nearest_manifest_scope_for_path(family, &owner)
                                    == dependencies.nearest_manifest_scope_for_path(family, target)
                        })
                    })
                {
                    continue;
                }
                if visited.insert(owner.clone()) {
                    if !modified.contains(&owner) {
                        load_owners.insert(owner.clone());
                    }
                    next.insert(owner);
                }
            }
            frontier = next;
        }
    }
    {
        let mut st = admitted.prepare("SELECT path,facts_blob FROM local_facts WHERE path IN(SELECT value FROM json_each(?1))")?;
        for row in st.query_map([serde_json::to_string(&load_owners)?], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })? {
            let (path, blob) = row?;
            let f = decode_facts(&blob)?;
            facts.insert(path, f);
            loaded_fact_files += 1;
        }
    }
    {
        let catalog: Vec<_> = scan
            .entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry.language.as_str()))
            .collect();
        let mut st = admitted.prepare("SELECT facts_blob FROM local_facts WHERE path=?1")?;
        let mut attempted = BTreeSet::new();
        loop {
            let mut loaded = false;
            for path in linker::required_full_typed_facts(&facts, &affected, &catalog) {
                if facts.contains_key(&path) || !attempted.insert(path.clone()) {
                    continue;
                }
                if let Some(raw) = st
                    .query_row([&path], |row| row.get::<_, Vec<u8>>(0))
                    .optional()?
                {
                    facts.insert(path, decode_facts(&raw)?);
                    loaded_fact_files += 1;
                    loaded = true;
                }
            }
            if !loaded {
                break;
            }
        }
    }
    let composed_routes =
        crate::engine::ast::route_includes::compose_django_route_includes(&mut facts);
    affected.extend(composed_routes.iter().cloned());
    replace_files.extend(composed_routes);
    assign_components(&mut facts, &component_owners);
    let mut tokens = BTreeSet::new();
    let mut doc_symbols = BTreeSet::new();
    for f in facts.values() {
        tokens.extend(reference_keys(f).into_iter().map(str::to_owned));
        doc_symbols.extend(
            f.docs
                .iter()
                .flat_map(|doc| doc.referenced_symbols.iter())
                .map(|symbol| symbol.replace("::", "."))
                .filter(|symbol| !symbol.is_empty()),
        );
    }
    let mut suffix_ids = BTreeSet::new();
    if !doc_symbols.is_empty() {
        let mut terms = BTreeSet::new();
        let mut fallback_symbols = BTreeSet::new();
        for symbol in &doc_symbols {
            let token = symbol
                .is_ascii()
                .then(|| symbol.rsplit(|c: char| !c.is_ascii_alphanumeric()).next())
                .flatten()
                .filter(|token| !token.is_empty() && token.len() <= 128);
            if let Some(token) = token {
                terms.insert(token.to_owned());
            } else {
                fallback_symbols.insert(symbol.as_str());
            }
        }
        let mut st = admitted.prepare("SELECT n.id,n.qualname FROM node_search s JOIN nodes n ON n.node_hash=s.rowid WHERE node_search MATCH ?1")?;
        for chunk in terms.into_iter().collect::<Vec<_>>().chunks(32) {
            let query = chunk
                .iter()
                .map(|term| format!("\"{term}\""))
                .collect::<Vec<_>>()
                .join(" OR ");
            for row in st.query_map([query], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })? {
                let (id, qualname) = row?;
                if qualname
                    .match_indices('.')
                    .any(|(at, _)| doc_symbols.contains(&qualname[at + 1..]))
                {
                    suffix_ids.insert(id);
                }
            }
        }
        if !fallback_symbols.is_empty() {
            let mut st =
                admitted.prepare("SELECT id,qualname FROM nodes WHERE instr(qualname,'.')>0")?;
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (id, qualname) = row?;
                if qualname
                    .match_indices('.')
                    .any(|(at, _)| fallback_symbols.contains(&qualname[at + 1..]))
                {
                    suffix_ids.insert(id);
                }
            }
        }
    }
    let encoded = serde_json::to_string(&tokens)?;
    let suffix_ids = serde_json::to_string(&suffix_ids)?;
    {
        let sql = "SELECT n.id,n.kind,n.name,n.qualname,p.path,n.line,n.end_line,n.is_test,n.language,n.generated,n.details,EXISTS(SELECT 1 FROM shared_owners s JOIN shared_keys k ON k.key_hash=s.key_hash WHERE s.kind_id=2 AND s.key_hash=n.node_hash AND k.key=n.id) FROM nodes n JOIN path_dictionary p ON p.path_id=n.path_id WHERE n.id IN(SELECT id FROM nodes WHERE kind='module' UNION SELECT id FROM nodes WHERE name IN(SELECT value FROM json_each(?1)) UNION SELECT id FROM nodes WHERE qualname IN(SELECT value FROM json_each(?1)) UNION SELECT n2.id FROM shared_owners s JOIN shared_keys k ON k.key_hash=s.key_hash JOIN nodes n2 ON n2.node_hash=s.key_hash AND n2.id=k.key WHERE s.kind_id=2 UNION SELECT value FROM json_each(?2))";
        let mut st = admitted.prepare(sql)?;
        let nodes = st.query_map(params![encoded, suffix_ids], |r| {
            Ok(Node {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                qualname: r.get(3)?,
                path: r.get(4)?,
                line: r.get(5)?,
                end_line: r.get(6)?,
                is_test: r.get(7)?,
                language: r.get(8)?,
                generated: r.get(9)?,
                details: serde_json::from_str(&r.get::<_, String>(10)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        10,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
            })
        })?;
        let complete: BTreeSet<_> = facts.keys().cloned().collect();
        for node in nodes {
            let node = node?;
            if complete.contains(&node.path) || modified.contains(&node.path) {
                continue;
            }
            facts
                .entry(node.path.clone())
                .or_insert_with(TypedFacts::default)
                .push_node(node);
        }
    }
    let hydrate_ms = hydrating.elapsed().as_secs_f64() * 1000.;
    let linking = Instant::now();
    let graph = linker::link_compact_with_typed_registry(
        &facts,
        Some(&affected),
        Some(&root),
        &dependencies,
    );
    let link_ms = linking.elapsed().as_secs_f64() * 1000.;

    drop(admitted);

    let persist_result = crate::db::writer::delta_persist::persist_delta(
        crate::db::writer::delta_persist::DeltaPersistenceInputs {
            db,
            root: &root,
            expected: &expected,
            admitted_identity,
            entries: &scan.entries,
            affected: &affected,
            replace_files: &replace_files,
            facts: &facts,
            graph: &graph,
            manifest_digest: dependencies.digest(),
        },
        |persist_ms, seal_ms, verify_ms, _verify_elapsed| {
            let latest = scanner::scan_reusing(&root, &adapter, &scan.entries)?;
            if latest.entries != scan.entries {
                return Err(SourceSnapshotMismatch(format!(
                    "workspace changed during delta; transaction rolled back (scan={scan_ms:.0}ms extract={extract_ms:.0}ms hydrate={hydrate_ms:.0}ms link={link_ms:.0}ms persist={persist_ms:.0}ms seal={seal_ms:.0}ms verify={verify_ms:.0}ms)"
                ))
                .into());
            }
            Ok(())
        },
    )?;

    drop(generation_lock);

    let mut report = json!({
        "files": scan.files,
        "changed_files": modified.len(),
        "affected_owners": affected.len(),
        "reparsed_files": modified.iter().filter(|p| inventory.contains_key(p.as_str())).count(),
        "loaded_fact_files": loaded_fact_files,
        "rewritten_files": persist_result.rewritten_files,
        "verification_cache": persist_result.verification_cache,
        "output_root": persist_result.output_root,
        "scan_ms": scan_ms,
        "extract_ms": extract_ms,
        "hydrate_ms": hydrate_ms,
        "link_ms": link_ms,
        "persist_ms": persist_result.persist_ms,
        "delete_ms": persist_result.delete_ms,
        "files_ms": persist_result.files_ms,
        "graph_ms": persist_result.graph_ms,
        "aggregate_ms": persist_result.aggregate_ms,
        "commitments_ms": persist_result.commitments_ms,
        "seal_ms": persist_result.seal_ms,
        "verify_ms": persist_result.verify_ms,
        "commit_ms": persist_result.commit_ms,
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.,
    });
    report["framework_rule_diagnostics"] = json!(dependencies.framework_rule_diagnostics(
        scan.entries
            .iter()
            .map(|file| (file.path.as_str(), file.language.as_str()))
    ));
    report["output"] = json!(db);
    Ok(report)
}

#[cfg(all(test, feature = "lang-rust"))]
mod tests {
    use super::*;

    #[test]
    fn added_trait_implementor_refreshes_unchanged_caller_candidates() {
        struct TestRoot(PathBuf);
        impl Drop for TestRoot {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let root = TestRoot(std::env::temp_dir().join(format!(
            "forge-delta-dispatch-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir(&root.0).unwrap();
        fs::create_dir(root.0.join("src")).unwrap();
        for (path, source) in [
            ("defs.rs", "pub trait Work { fn run(&self); }\n"),
            ("alpha.rs", "use crate::defs::Work; pub struct Alpha; impl Work for Alpha { fn run(&self) {} }\n"),
            ("caller.rs", "use crate::defs::Work; pub fn caller(value: &dyn Work) { value.run(); }\n"),
        ] {
            fs::write(root.0.join("src").join(path), source).unwrap();
        }
        let db = root.0.join(".forge/code-map.sqlite");
        let candidates = |path: &Path| {
            let conn = crate::db::reader::open(path, &root.0).unwrap();
            let mut statement = conn.prepare("SELECT d.id FROM edges e JOIN nodes s ON s.node_hash=e.src_hash JOIN nodes d ON d.node_hash=e.dst_hash WHERE s.name='caller' AND e.kind='possible_calls' ORDER BY d.id").unwrap();
            let result = statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            result
        };
        build(&root.0, &db, None).unwrap();
        assert_eq!(
            candidates(&db).len(),
            1,
            "cold build links caller to existing Alpha::run candidate"
        );
        let cold = root.0.join(".forge/cold.sqlite");
        fs::write(
            root.0.join("src/beta.rs"),
            "use crate::defs::Work; pub struct Beta; impl Work for Beta { fn run(&self) {} }\n",
        )
        .unwrap();
        delta(&root.0, &db, &[PathBuf::from("src/beta.rs")]).unwrap();
        build(&root.0, &cold, None).unwrap();
        assert_eq!(
            candidates(&db),
            candidates(&cold),
            "delta and cold builds discover the same dynamic candidates after a new implementor is added"
        );
        assert_eq!(candidates(&db).len(), 2);
    }

    #[test]
    fn import_contract_matching_observes_identifier_boundaries_and_unknowns() {
        let imported = BTreeSet::from(["Api".to_owned()]);
        let aliases = BTreeSet::from(["pkg.service".to_owned()]);

        assert!(contract_mentions_import(
            &serde_json::json!("Api"),
            &imported,
            &aliases
        ));
        assert!(contract_mentions_import(
            &serde_json::json!("pkg.service"),
            &imported,
            &aliases
        ));
        assert!(contract_mentions_import(
            &serde_json::json!("pkg.service.Client"),
            &imported,
            &aliases
        ));
        assert!(!contract_mentions_import(
            &serde_json::json!("ApiKey"),
            &imported,
            &aliases
        ));
        assert!(!contract_mentions_import(
            &serde_json::json!("pkg.service_extra"),
            &imported,
            &aliases
        ));
        assert!(contract_mentions_import(
            &serde_json::json!({ "kind": "unknown" }),
            &imported,
            &aliases
        ));
        assert!(contract_mentions_import(
            &serde_json::json!({
                "kind": "field",
                "receiver": "pkg.service.Client",
                "field": "call"
            }),
            &imported,
            &aliases
        ));
        assert!(!contract_mentions_import(
            &serde_json::json!({
                "kind": "field",
                "receiver": "Api",
                "field": "call"
            }),
            &imported,
            &aliases
        ));
    }

    #[test]
    fn unavailable_or_ambiguous_import_facts_expand_conservatively() {
        use crate::core::models::{Facts, Node, Reference};

        let missing_facts_db = Connection::open_in_memory().unwrap();
        missing_facts_db
            .execute_batch("CREATE TABLE local_facts(path TEXT PRIMARY KEY, facts_blob BLOB);")
            .unwrap();
        let mut facts = BTreeMap::new();
        let mut loaded_fact_files = 0;
        assert!(module_exposes_provider_contract(
            &missing_facts_db,
            "middle.py",
            "provider.py",
            &mut facts,
            &mut loaded_fact_files
        )
        .unwrap());

        let ambiguous_facts_db = Connection::open_in_memory().unwrap();
        ambiguous_facts_db
            .execute_batch(
                "CREATE TABLE local_facts(path TEXT PRIMARY KEY, facts_blob BLOB);
                 CREATE TABLE path_dictionary(path_id INTEGER PRIMARY KEY, path TEXT UNIQUE);
                 CREATE TABLE nodes(node_hash INTEGER PRIMARY KEY, path_id INTEGER, kind TEXT);
                 CREATE TABLE edge_occurrences(line INTEGER, dst_hash INTEGER, owner_id INTEGER, evidence_id INTEGER, kind TEXT);
                 CREATE TABLE coverage_evidence(evidence_id INTEGER PRIMARY KEY, evidence TEXT);
                 INSERT INTO path_dictionary(path_id,path) VALUES(1,'middle.py'),(2,'provider.py');
                 INSERT INTO nodes(node_hash,path_id,kind) VALUES(1,1,'module'),(2,2,'module'),(3,2,'function');
                 INSERT INTO coverage_evidence(evidence_id,evidence) VALUES(1,'provider');
                 INSERT INTO edge_occurrences(line,dst_hash,owner_id,evidence_id,kind) VALUES
                    (1,2,1,1,'imports'),(1,3,1,1,'imports');",
            )
            .unwrap();
        let module = Node {
            id: "module:middle.py".to_owned(),
            kind: "module".to_owned(),
            name: "middle".to_owned(),
            qualname: "middle".to_owned(),
            path: "middle.py".to_owned(),
            line: 1,
            end_line: 1,
            is_test: false,
            language: "python".to_owned(),
            generated: false,
            details: serde_json::json!({"exports": [], "bindings": []}),
        };
        let typed = TypedFacts::from_public(Facts {
            nodes: vec![module.clone()],
            references: vec![Reference {
                source: module.id,
                expression: "provider".to_owned(),
                kind: "imports".to_owned(),
                line: 1,
                column: 0,
                alias: Some("provider".to_owned()),
                module: Some("provider".to_owned()),
                dynamic: false,
                receiver_hint: None,
            }],
            ..Facts::default()
        });
        facts.insert("middle.py".to_owned(), typed);
        assert!(module_exposes_provider_contract(
            &ambiguous_facts_db,
            "middle.py",
            "provider.py",
            &mut facts,
            &mut loaded_fact_files
        )
        .unwrap());
    }

    #[test]
    fn return_contract_and_known_private_import_bound_the_python_frontier() {
        use crate::core::{
            models::{Facts, Node, Reference},
            semantic::{TypeExpr, ValueFlowFacts},
        };

        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE local_facts(path TEXT PRIMARY KEY, facts_blob BLOB);
             CREATE TABLE path_dictionary(path_id INTEGER PRIMARY KEY, path TEXT UNIQUE);
             CREATE TABLE nodes(node_hash INTEGER PRIMARY KEY, path_id INTEGER, kind TEXT);
             CREATE TABLE edge_occurrences(line INTEGER, dst_hash INTEGER, owner_id INTEGER, evidence_id INTEGER, kind TEXT);
             CREATE TABLE coverage_evidence(evidence_id INTEGER PRIMARY KEY, evidence TEXT);
             INSERT INTO path_dictionary(path_id,path) VALUES(1,'middle.py'),(2,'provider.py');
             INSERT INTO nodes(node_hash,path_id,kind) VALUES(1,2,'module');
             INSERT INTO coverage_evidence(evidence_id,evidence) VALUES(1,'provider');
             INSERT INTO edge_occurrences(line,dst_hash,owner_id,evidence_id,kind) VALUES(1,1,1,1,'imports');",
        )
        .unwrap();

        let module = Node {
            id: "module:middle.py".to_owned(),
            kind: "module".to_owned(),
            name: "middle".to_owned(),
            qualname: "middle".to_owned(),
            path: "middle.py".to_owned(),
            line: 1,
            end_line: 2,
            is_test: false,
            language: "python".to_owned(),
            generated: false,
            details: serde_json::json!({
                "exports": [{"local": "visible", "name": "visible"}],
                "bindings": ["_provider", "visible"]
            }),
        };
        let function = Node {
            id: "function:middle.py:visible".to_owned(),
            kind: "function".to_owned(),
            name: "visible".to_owned(),
            qualname: "visible".to_owned(),
            path: "middle.py".to_owned(),
            line: 2,
            end_line: 2,
            is_test: false,
            language: "python".to_owned(),
            generated: false,
            details: serde_json::json!({}),
        };
        let import = Reference {
            source: module.id.clone(),
            expression: "provider".to_owned(),
            kind: "imports".to_owned(),
            line: 1,
            column: 0,
            alias: Some("_provider".to_owned()),
            module: Some("provider".to_owned()),
            dynamic: false,
            receiver_hint: None,
        };
        let public_facts = || Facts {
            nodes: vec![module.clone(), function.clone()],
            references: vec![import.clone()],
            ..Facts::default()
        };

        let mut exposed_facts = TypedFacts::from_public(public_facts());
        exposed_facts.flows.insert(
            function.id.clone(),
            ValueFlowFacts {
                return_type: Some(TypeExpr::Named {
                    name: "_provider.Api".to_owned(),
                }),
                ..ValueFlowFacts::default()
            },
        );
        let mut facts = BTreeMap::from([("middle.py".to_owned(), exposed_facts)]);
        let mut loaded_fact_files = 0;
        assert!(module_exposes_provider_contract(
            &db,
            "middle.py",
            "provider.py",
            &mut facts,
            &mut loaded_fact_files
        )
        .unwrap());

        let mut known_facts = TypedFacts::from_public(public_facts());
        known_facts.flows.insert(
            function.id.clone(),
            ValueFlowFacts {
                return_type: Some(TypeExpr::Named {
                    name: "int".to_owned(),
                }),
                ..ValueFlowFacts::default()
            },
        );
        facts.insert("middle.py".to_owned(), known_facts);
        assert!(!module_exposes_provider_contract(
            &db,
            "middle.py",
            "provider.py",
            &mut facts,
            &mut loaded_fact_files
        )
        .unwrap());

        facts.insert(
            "middle.py".to_owned(),
            TypedFacts::from_public(public_facts()),
        );
        assert!(module_exposes_provider_contract(
            &db,
            "middle.py",
            "provider.py",
            &mut facts,
            &mut loaded_fact_files
        )
        .unwrap());
    }
}
