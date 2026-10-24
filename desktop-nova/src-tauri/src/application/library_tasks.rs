//! Single-queue serial background task framework for the projection library.
//!
//! All library I/O tasks (scan / checksum / batch file operations / restore /
//! export / index) enter **one** serial queue: at any instant at most one task
//! runs (spec 4.1.5). The queue never writes library data itself; it only reads
//! and manipulates files, reporting per-item results so the TS service layer can
//! commit library state once at the terminal event (design 2.1.3).
//!
//! Cancellation is cooperative: executors check a flag between items, keep the
//! results of already-finished items, and leave untouched items alone (4.2.6).
//! Progress is reported as `processed / total` plus the current item.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::application::library_export::{self, ExportRequest};
use crate::application::library_index::{self, IndexedEntry};
use crate::application::library_trash::{self, TrashMoveItem, TrashRestoreItem};

/// Streaming SHA-256 read size. Matches the viewer-backend hashing buffer so a
/// GB-scale projection is never loaded into memory at once.
pub(crate) const HASH_BUFFER_BYTES: usize = 256 * 1024;

/// The format data worker owns content recognition. The library only routes a
/// path to it; extensions below are an inexpensive scan candidate filter.
pub(crate) type FormatInspector = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

// ---------------------------------------------------------------------------
// Task vocabulary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TaskKind {
    Scan,
    Checksum,
    BatchCopy,
    BatchRename,
    BatchTrash,
    Restore,
    ExportZip,
    Index,
}

impl TaskKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            TaskKind::Scan => "scan",
            TaskKind::Checksum => "checksum",
            TaskKind::BatchCopy => "batch-copy",
            TaskKind::BatchRename => "batch-rename",
            TaskKind::BatchTrash => "batch-trash",
            TaskKind::Restore => "restore",
            TaskKind::ExportZip => "export-zip",
            TaskKind::Index => "index",
        }
    }
}

/// Folder reference handed to the scanner: only `tracked` (which includes the
/// built-in import folder) participates in scanning; `non_archive` is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LibraryFolderType {
    Tracked,
    NonArchive,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryFolderRef {
    pub path: String,
    #[serde(rename = "type")]
    pub folder_type: LibraryFolderType,
}

/// Recheck granularity: whole library / one folder / one entry / one replica.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum RecheckScope {
    All,
    Folder {
        #[serde(rename = "folderPath")]
        folder_path: String,
    },
    Entry {
        #[serde(rename = "entryId")]
        entry_id: String,
    },
    Replica {
        #[serde(rename = "replicaId")]
        replica_id: String,
    },
}

impl Default for RecheckScope {
    fn default() -> Self {
        RecheckScope::All
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScanParams {
    pub folders: Vec<LibraryFolderRef>,
    #[serde(default = "default_true")]
    pub use_cache: bool,
    #[serde(default)]
    pub force_recheck: bool,
    #[serde(default)]
    pub scope: RecheckScope,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChecksumParams {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CopyItem {
    pub source_path: String,
    pub target_directory: String,
    pub target_file_name: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchCopyParams {
    pub items: Vec<CopyItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameItem {
    pub source_path: String,
    pub target_file_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchRenameParams {
    pub items: Vec<RenameItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchTrashParams {
    pub items: Vec<TrashMoveItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestoreParams {
    pub items: Vec<TrashRestoreItem>,
}

/// `index` task params: the current indexable entry set, diffed by `entryId`
/// against the persisted index (D.1 / design 2.4.3). TS owns the entry → index
/// projection; Rust only stores and diffs it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IndexTaskParams {
    #[serde(default)]
    pub entries: Vec<IndexedEntry>,
}

const fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Event payloads (design 2.2.3)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskFailure {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskProgressEvent {
    pub task_id: String,
    pub kind: TaskKind,
    pub phase: String,
    pub processed: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskFinishedEvent {
    pub task_id: String,
    pub kind: TaskKind,
    pub status: String,
    pub summary: serde_json::Value,
    pub failures: Vec<TaskFailure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskDescriptor {
    pub task_id: String,
    pub kind: TaskKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskQueueSnapshot {
    pub pending: Vec<TaskDescriptor>,
    pub running: Option<TaskDescriptor>,
}

/// Sink abstraction so the queue core stays independent of Tauri and testable.
pub(crate) trait TaskEventSink: Send + Sync {
    fn task_progress(&self, event: &TaskProgressEvent);
    fn task_finished(&self, event: &TaskFinishedEvent);
    fn task_queue_changed(&self, snapshot: &TaskQueueSnapshot);
}

/// Config-directory provider so the framework never depends on host globals.
pub(crate) type ConfigDirectoryProvider = Arc<dyn Fn() -> Result<PathBuf, String> + Send + Sync>;

// ---------------------------------------------------------------------------
// Queue internals
// ---------------------------------------------------------------------------

struct TaskEnvelope {
    id: String,
    kind: TaskKind,
    params: serde_json::Value,
}

#[derive(Default)]
struct QueueState {
    pending: VecDeque<TaskEnvelope>,
    running: Option<TaskDescriptor>,
    cancel_flags: HashSet<String>,
}

#[derive(Debug, Clone)]
struct CachedChecksum {
    size: u64,
    mtime_ms: u64,
    sha256: String,
}

struct TaskOutcome {
    status: &'static str,
    summary: serde_json::Value,
    failures: Vec<TaskFailure>,
}

impl TaskOutcome {
    fn completed(summary: serde_json::Value, failures: Vec<TaskFailure>) -> Self {
        Self { status: "completed", summary, failures }
    }

    fn cancelled(summary: serde_json::Value, failures: Vec<TaskFailure>) -> Self {
        Self { status: "cancelled", summary, failures }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            status: "failed",
            summary: serde_json::json!({ "message": message.into() }),
            failures: Vec::new(),
        }
    }
}

pub(crate) struct LibraryTaskQueue {
    state: Mutex<QueueState>,
    available: Condvar,
    started: AtomicBool,
    next_id: AtomicU64,
    sink: Arc<dyn TaskEventSink>,
    config_directory: ConfigDirectoryProvider,
    checksum_cache: Mutex<HashMap<String, CachedChecksum>>,
    format_inspector: OnceLock<FormatInspector>,
    self_weak: OnceLock<Weak<LibraryTaskQueue>>,
}

impl LibraryTaskQueue {
    pub(crate) fn new(
        sink: Arc<dyn TaskEventSink>,
        config_directory: ConfigDirectoryProvider,
    ) -> Arc<Self> {
        let queue = Arc::new(Self {
            state: Mutex::new(QueueState::default()),
            available: Condvar::new(),
            started: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            sink,
            config_directory,
            checksum_cache: Mutex::new(HashMap::new()),
            format_inspector: OnceLock::new(),
            self_weak: OnceLock::new(),
        });
        let _ = queue.self_weak.set(Arc::downgrade(&queue));
        queue
    }

    pub(crate) fn set_format_inspector(&self, inspector: FormatInspector) -> Result<(), String> {
        self.format_inspector.set(inspector).map_err(|_| "projection format inspector is already configured".to_owned())?;
        self.checksum_cache.lock().unwrap_or_else(|poison|poison.into_inner()).clear();
        Ok(())
    }

    pub(crate) fn compute_projection_checksum(&self, path: &Path) -> Result<LitematicChecksum, String> {
        if let Some(inspect) = self.format_inspector.get() {
            if !path.is_file() { return Err(format!("文件不存在或不可读: {}", path.display())); }
            inspect(path)?;
            let (sha256, byte_count) = hash_file_sha256(path)?;
            Ok(LitematicChecksum { sha256, byte_count })
        } else {
            // Preserve the old fixture entrypoint. Production installs the
            // authoritative data-worker inspector before exposing this queue.
            compute_litematic_checksum(path)
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// Enqueues a task and returns its id. The worker is spawned lazily.
    pub(crate) fn submit(&self, kind: TaskKind, params: serde_json::Value) -> String {
        let id = format!("library-task-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        {
            let mut state = self.lock_state();
            state.pending.push_back(TaskEnvelope { id: id.clone(), kind, params });
        }
        self.ensure_worker();
        self.notify_queue_changed();
        self.available.notify_all();
        id
    }

    /// Requests cooperative cancellation. Returns `false` when no such task is
    /// pending or running (already finished or unknown).
    pub(crate) fn cancel(&self, task_id: &str) -> bool {
        let mut state = self.lock_state();
        let exists = state.running.as_ref().is_some_and(|d| d.task_id == task_id)
            || state.pending.iter().any(|envelope| envelope.id == task_id);
        if !exists {
            return false;
        }
        state.cancel_flags.insert(task_id.to_owned());
        drop(state);
        self.available.notify_all();
        true
    }

    pub(crate) fn snapshot(&self) -> TaskQueueSnapshot {
        let state = self.lock_state();
        TaskQueueSnapshot {
            pending: state
                .pending
                .iter()
                .map(|envelope| TaskDescriptor { task_id: envelope.id.clone(), kind: envelope.kind })
                .collect(),
            running: state.running.clone(),
        }
    }

    fn ensure_worker(&self) {
        if self
            .started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            if let Some(worker) = self.self_weak.get().and_then(Weak::upgrade) {
                let _ = thread::Builder::new()
                    .name("library-task-worker".to_owned())
                    .spawn(move || worker.run_loop());
            }
        }
    }

    fn run_loop(self: Arc<Self>) {
        loop {
            let envelope = {
                let mut state = self.lock_state();
                while state.pending.is_empty() {
                    state = self
                        .available
                        .wait(state)
                        .unwrap_or_else(|poison| poison.into_inner());
                }
                state.pending.pop_front().expect("non-empty queue")
            };
            let descriptor = TaskDescriptor { task_id: envelope.id.clone(), kind: envelope.kind };
            {
                let mut state = self.lock_state();
                state.running = Some(descriptor);
            }
            self.notify_queue_changed();

            let outcome = if self.is_cancelled(&envelope.id) {
                TaskOutcome::cancelled(serde_json::json!({ "processed": 0 }), Vec::new())
            } else {
                self.execute(&envelope)
            };

            {
                let mut state = self.lock_state();
                state.running = None;
                state.cancel_flags.remove(&envelope.id);
            }
            self.sink.task_finished(&TaskFinishedEvent {
                task_id: envelope.id.clone(),
                kind: envelope.kind,
                status: outcome.status.to_owned(),
                summary: outcome.summary,
                failures: outcome.failures,
            });
            self.notify_queue_changed();
        }
    }

    fn is_cancelled(&self, task_id: &str) -> bool {
        self.lock_state().cancel_flags.contains(task_id)
    }

    fn notify_queue_changed(&self) {
        self.sink.task_queue_changed(&self.snapshot());
    }

    fn emit_progress(
        &self,
        task_id: &str,
        kind: TaskKind,
        phase: &str,
        processed: u64,
        total: u64,
        current_path: Option<String>,
        message: Option<String>,
    ) {
        self.sink.task_progress(&TaskProgressEvent {
            task_id: task_id.to_owned(),
            kind,
            phase: phase.to_owned(),
            processed,
            total,
            current_path,
            message,
        });
    }

    // -----------------------------------------------------------------------
    // Execution dispatch
    // -----------------------------------------------------------------------

    fn execute(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        match envelope.kind {
            TaskKind::Scan => self.run_scan(envelope),
            TaskKind::Checksum => self.run_checksum(envelope),
            TaskKind::BatchCopy => self.run_batch_copy(envelope),
            TaskKind::BatchRename => self.run_batch_rename(envelope),
            TaskKind::BatchTrash => self.run_batch_trash(envelope),
            TaskKind::Restore => self.run_restore(envelope),
            TaskKind::Index => self.run_index(envelope),
            TaskKind::ExportZip => self.run_export_zip(envelope),
        }
    }

    fn run_checksum(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        let params: ChecksumParams = match parse_params(envelope.kind, &envelope.params) {
            Ok(params) => params,
            Err(outcome) => return outcome,
        };
        let total = params.paths.len() as u64;
        let mut failures = Vec::new();
        let mut processed = 0_u64;
        self.emit_progress(&envelope.id, TaskKind::Checksum, "checksum", 0, total, None, None);
        for path in &params.paths {
            if self.is_cancelled(&envelope.id) {
                return TaskOutcome::cancelled(
                    serde_json::json!({ "processed": processed, "total": total }),
                    failures,
                );
            }
            if let Err(reason) = self.compute_projection_checksum(Path::new(path)) {
                failures.push(TaskFailure { path: path.clone(), reason });
            }
            processed += 1;
            self.emit_progress(
                &envelope.id,
                TaskKind::Checksum,
                "checksum",
                processed,
                total,
                Some(path.clone()),
                None,
            );
        }
        TaskOutcome::completed(
            serde_json::json!({ "processed": processed, "total": total }),
            failures,
        )
    }

    fn run_scan(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        let params: ScanParams = match parse_params(envelope.kind, &envelope.params) {
            Ok(params) => params,
            Err(outcome) => return outcome,
        };
        let tracked: Vec<&LibraryFolderRef> = params
            .folders
            .iter()
            .filter(|folder| folder.folder_type == LibraryFolderType::Tracked)
            .collect();
        let tracked_count = tracked.len() as u64;
        // Entry / replica granularity must be expanded to concrete paths by the
        // TS layer, which owns the entry->replica mapping; a scan cannot resolve
        // ids on its own. Surface it instead of silently scanning everything.
        let scoped_folders: Vec<&LibraryFolderRef> = match &params.scope {
            RecheckScope::All => tracked,
            RecheckScope::Folder { folder_path } => tracked
                .into_iter()
                .filter(|folder| paths_equal(&folder.path, folder_path))
                .collect(),
            RecheckScope::Entry { entry_id } => {
                return TaskOutcome::failed(format!(
                    "entry 粒度需由 TS 展开为具体路径后提交 checksum 任务（entryId={entry_id}）"
                ))
            }
            RecheckScope::Replica { replica_id } => {
                return TaskOutcome::failed(format!(
                    "replica 粒度需由 TS 展开为具体路径后提交 checksum 任务（replicaId={replica_id}）"
                ))
            }
        };

        let mut files: Vec<ScannedFile> = Vec::new();
        let mut failures: Vec<TaskFailure> = Vec::new();
        let mut candidates: Vec<PathBuf> = Vec::new();
        for folder in &scoped_folders {
            collect_litematic_files(Path::new(&folder.path), &mut candidates, &mut failures);
        }

        let total = candidates.len() as u64;
        self.emit_progress(&envelope.id, TaskKind::Scan, "scan", 0, total, None, None);
        let mut processed = 0_u64;
        for path in &candidates {
            if self.is_cancelled(&envelope.id) {
                return TaskOutcome::cancelled(
                    serde_json::json!({ "processed": processed, "total": total }),
                    failures,
                );
            }
            match self.scan_one(path, params.use_cache, params.force_recheck) {
                Ok(scanned) => files.push(scanned),
                Err(reason) => failures.push(TaskFailure {
                    path: path.display().to_string(),
                    reason,
                }),
            }
            processed += 1;
            self.emit_progress(
                &envelope.id,
                TaskKind::Scan,
                "scan",
                processed,
                total,
                Some(path.display().to_string()),
                None,
            );
        }
        let scanned_count = files.len() as u64;
        let cache_hits = files.iter().filter(|file| file.from_cache).count() as u64;
        let summary = serde_json::json!({
            "scannedCount": scanned_count,
            "checksumComputed": scanned_count.saturating_sub(cache_hits),
            "cacheHits": cache_hits,
            "skippedNonArchive": params.folders.len() as u64 - tracked_count,
            "files": files,
        });
        TaskOutcome::completed(summary, failures)
    }

    /// Computes (or reuses) a file fingerprint and refreshes the session cache.
    /// A cache hit requires an identical `size + mtime`; `forceRecheck` bypasses.
    fn scan_one(
        &self,
        path: &Path,
        use_cache: bool,
        force_recheck: bool,
    ) -> Result<ScannedFile, String> {
        let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
        let size = metadata.len();
        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        let key = path.display().to_string();

        let cached = if use_cache && !force_recheck {
            self.checksum_cache
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .get(&key)
                .filter(|entry| entry.size == size && entry.mtime_ms == mtime_ms)
                .map(|entry| entry.sha256.clone())
        } else {
            None
        };

        let (checksum, from_cache) = match cached {
            Some(sha256) => (sha256, true),
            None => {
                if let Some(inspect) = self.format_inspector.get() { inspect(path)?; }
                let (sha256, _bytes) = hash_file_sha256(path)?;
                self.store_cache(&key, size, mtime_ms, &sha256);
                (sha256, false)
            }
        };

        Ok(ScannedFile {
            path: key,
            file_name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            folder_path: path
                .parent()
                .map(|parent| parent.display().to_string())
                .unwrap_or_default(),
            file_size: size,
            mtime_ms,
            checksum,
            from_cache,
        })
    }

    fn store_cache(&self, key: &str, size: u64, mtime_ms: u64, sha256: &str) {
        self.checksum_cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(
                key.to_owned(),
                CachedChecksum { size, mtime_ms, sha256: sha256.to_owned() },
            );
    }

    fn run_batch_copy(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        let params: BatchCopyParams = match parse_params(envelope.kind, &envelope.params) {
            Ok(params) => params,
            Err(outcome) => return outcome,
        };
        let total = params.items.len() as u64;
        self.emit_progress(&envelope.id, TaskKind::BatchCopy, "copy", 0, total, None, None);
        let mut failures = Vec::new();
        let mut copied = 0_u64;
        let mut processed = 0_u64;
        for item in &params.items {
            if self.is_cancelled(&envelope.id) {
                return TaskOutcome::cancelled(
                    serde_json::json!({ "copied": copied, "processed": processed, "total": total }),
                    failures,
                );
            }
            match crate::copy_file_to_directory_core(
                Path::new(&item.source_path),
                Path::new(&item.target_directory),
                &item.target_file_name,
                item.overwrite,
            ) {
                Ok(_) => copied += 1,
                Err(reason) => failures.push(TaskFailure {
                    path: item.source_path.clone(),
                    reason,
                }),
            }
            processed += 1;
            self.emit_progress(
                &envelope.id,
                TaskKind::BatchCopy,
                "copy",
                processed,
                total,
                Some(item.source_path.clone()),
                None,
            );
        }
        TaskOutcome::completed(serde_json::json!({ "copied": copied, "total": total }), failures)
    }

    fn run_batch_rename(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        let params: BatchRenameParams = match parse_params(envelope.kind, &envelope.params) {
            Ok(params) => params,
            Err(outcome) => return outcome,
        };
        let total = params.items.len() as u64;
        self.emit_progress(&envelope.id, TaskKind::BatchRename, "rename", 0, total, None, None);
        let mut failures = Vec::new();
        let mut renamed = 0_u64;
        let mut processed = 0_u64;
        for item in &params.items {
            if self.is_cancelled(&envelope.id) {
                return TaskOutcome::cancelled(
                    serde_json::json!({ "renamed": renamed, "processed": processed, "total": total }),
                    failures,
                );
            }
            match crate::rename_replica_core(Path::new(&item.source_path), &item.target_file_name) {
                Ok(_) => renamed += 1,
                Err(reason) => failures.push(TaskFailure {
                    path: item.source_path.clone(),
                    reason,
                }),
            }
            processed += 1;
            self.emit_progress(
                &envelope.id,
                TaskKind::BatchRename,
                "rename",
                processed,
                total,
                Some(item.source_path.clone()),
                None,
            );
        }
        TaskOutcome::completed(serde_json::json!({ "renamed": renamed, "total": total }), failures)
    }

    fn run_batch_trash(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        let params: BatchTrashParams = match parse_params(envelope.kind, &envelope.params) {
            Ok(params) => params,
            Err(outcome) => return outcome,
        };
        let total = params.items.len() as u64;
        self.emit_progress(&envelope.id, TaskKind::BatchTrash, "trash", 0, total, None, None);
        if self.is_cancelled(&envelope.id) {
            return TaskOutcome::cancelled(serde_json::json!({ "processed": 0 }), Vec::new());
        }
        let config_directory = match (self.config_directory)() {
            Ok(directory) => directory,
            Err(reason) => return TaskOutcome::failed(reason),
        };
        let results = library_trash::move_items(&config_directory, &params.items);
        let failures = failures_from_move(&results);
        let trashed = results.iter().filter(|result| result.ok).count() as u64;
        for (index, _) in results.iter().enumerate() {
            self.emit_progress(
                &envelope.id,
                TaskKind::BatchTrash,
                "trash",
                index as u64 + 1,
                total,
                None,
                None,
            );
        }
        TaskOutcome::completed(serde_json::json!({ "trashed": trashed, "total": total }), failures)
    }

    fn run_restore(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        let params: RestoreParams = match parse_params(envelope.kind, &envelope.params) {
            Ok(params) => params,
            Err(outcome) => return outcome,
        };
        let total = params.items.len() as u64;
        self.emit_progress(&envelope.id, TaskKind::Restore, "restore", 0, total, None, None);
        if self.is_cancelled(&envelope.id) {
            return TaskOutcome::cancelled(serde_json::json!({ "processed": 0 }), Vec::new());
        }
        let results = library_trash::restore_items(&params.items);
        let failures = failures_from_restore(&results);
        let restored = results.iter().filter(|result| result.ok).count() as u64;
        for (index, _) in results.iter().enumerate() {
            self.emit_progress(
                &envelope.id,
                TaskKind::Restore,
                "restore",
                index as u64 + 1,
                total,
                None,
                None,
            );
        }
        TaskOutcome::completed(serde_json::json!({ "restored": restored, "total": total }), failures)
    }

    /// Incrementally updates the background inverted index, then persists it.
    /// The index is derived data: a failed write never fails the task (queries
    /// keep serving the in-memory index; the next run rewrites the file).
    fn run_index(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        let params: IndexTaskParams = match parse_params(envelope.kind, &envelope.params) {
            Ok(params) => params,
            Err(outcome) => return outcome,
        };
        let config_directory = match (self.config_directory)() {
            Ok(directory) => directory,
            Err(reason) => return TaskOutcome::failed(reason),
        };
        let total = params.entries.len() as u64;
        self.emit_progress(&envelope.id, TaskKind::Index, "index", 0, total, None, None);
        let should_cancel = || self.is_cancelled(&envelope.id);
        let outcome = library_index::shared().apply_entries(
            &config_directory,
            &params.entries,
            &should_cancel,
        );
        self.emit_progress(
            &envelope.id,
            TaskKind::Index,
            "index",
            outcome.indexed_count as u64,
            total,
            None,
            None,
        );
        let summary = serde_json::json!({
            "indexedCount": outcome.indexed_count,
            "added": outcome.added,
            "updated": outcome.updated,
            "removed": outcome.removed,
        });
        if outcome.cancelled {
            TaskOutcome::cancelled(summary, Vec::new())
        } else {
            TaskOutcome::completed(summary, Vec::new())
        }
    }

    /// Writes the flat export zip (E.5). Missing sources are reported per entry;
    /// a target-level failure (unwritable / no space) aborts the whole export and
    /// is surfaced as a task failure so no half-product is left (5.9.3.4).
    fn run_export_zip(&self, envelope: &TaskEnvelope) -> TaskOutcome {
        let request: ExportRequest = match parse_params(envelope.kind, &envelope.params) {
            Ok(request) => request,
            Err(outcome) => return outcome,
        };
        let total = request.entries.len() as u64;
        self.emit_progress(&envelope.id, TaskKind::ExportZip, "export", 0, total, None, None);
        let should_cancel = || self.is_cancelled(&envelope.id);
        match library_export::write_export_zip(&request, &should_cancel) {
            Ok(library_export::ExportOutcome::Completed { files, entry_count, failures }) => {
                let task_failures = failures
                    .into_iter()
                    .map(|(path, reason)| TaskFailure { path, reason })
                    .collect::<Vec<_>>();
                self.emit_progress(
                    &envelope.id,
                    TaskKind::ExportZip,
                    "export",
                    entry_count as u64,
                    total,
                    Some(request.output_path.clone()),
                    None,
                );
                TaskOutcome::completed(
                    serde_json::json!({
                        "entryCount": entry_count,
                        "outputPath": request.output_path,
                        "files": files,
                    }),
                    task_failures,
                )
            }
            Ok(library_export::ExportOutcome::Cancelled) => TaskOutcome::cancelled(
                serde_json::json!({ "processed": 0, "total": total }),
                Vec::new(),
            ),
            Err(reason) => TaskOutcome::failed(reason),
        }
    }
}

fn failures_from_move(results: &[library_trash::TrashMoveResult]) -> Vec<TaskFailure> {
    results
        .iter()
        .filter(|result| !result.ok)
        .map(|result| TaskFailure {
            path: result.source_path.clone(),
            reason: result.reason.clone().unwrap_or_else(|| "unknown".to_owned()),
        })
        .collect()
}

fn failures_from_restore(results: &[library_trash::TrashRestoreResult]) -> Vec<TaskFailure> {
    results
        .iter()
        .filter(|result| !result.ok)
        .map(|result| TaskFailure {
            path: result.trash_path.clone(),
            reason: result.reason.clone().unwrap_or_else(|| "unknown".to_owned()),
        })
        .collect()
}

fn parse_params<T: serde::de::DeserializeOwned>(
    kind: TaskKind,
    params: &serde_json::Value,
) -> Result<T, TaskOutcome> {
    serde_json::from_value(params.clone())
        .map_err(|error| TaskOutcome::failed(format!("{} 任务参数无效: {error}", kind.as_str())))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScannedFile {
    path: String,
    file_name: String,
    folder_path: String,
    file_size: u64,
    mtime_ms: u64,
    checksum: String,
    #[serde(skip)]
    from_cache: bool,
}

// ---------------------------------------------------------------------------
// Fingerprints and enumeration
// ---------------------------------------------------------------------------

/// Streams the compressed `.litematic` bytes through SHA-256 without loading the
/// file into memory, matching the viewer-core `HashingReader` semantics. The file
/// name never participates; only the byte content does (spec 5.2.1.1).
pub(crate) fn hash_file_sha256(path: &Path) -> Result<(String, u64), String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("无法读取文件 {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    let mut bytes = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("读取文件失败 {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
    }
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LitematicChecksum {
    pub sha256: String,
    pub byte_count: u64,
}

/// IPC shell for `compute_litematic_checksum`: rejects non-litematic input,
/// never mutates the file, and delegates stream hashing to `hash_file_sha256`.
pub(crate) fn compute_litematic_checksum(path: &Path) -> Result<LitematicChecksum, String> {
    if !path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("litematic"))
    {
        return Err(format!("不是 litematic 文件: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("文件不存在或不可读: {}", path.display()));
    }
    let (sha256, byte_count) = hash_file_sha256(path)?;
    Ok(LitematicChecksum { sha256, byte_count })
}

fn collect_litematic_files(
    directory: &Path,
    files: &mut Vec<PathBuf>,
    failures: &mut Vec<TaskFailure>,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            failures.push(TaskFailure {
                path: directory.display().to_string(),
                reason: error.to_string(),
            });
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.is_dir() {
            collect_litematic_files(&path, files, failures);
        } else if is_schematic_candidate(&path) {
            files.push(path);
        }
    }
}

pub(crate) fn is_schematic_candidate(path: &Path) -> bool {
    path.extension().and_then(std::ffi::OsStr::to_str).is_some_and(|extension| {
        ["litematic", "nbt", "json", "bg", "bgcp"].iter().any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}

fn paths_equal(left: &str, right: &str) -> bool {
    let left = Path::new(left);
    let right = Path::new(right);
    if let (Ok(a), Ok(b)) = (fs::canonicalize(left), fs::canonicalize(right)) {
        return a == b;
    }
    left == right
}

// ---------------------------------------------------------------------------
// Command-layer path scope validation (spec 4.3.1)
// ---------------------------------------------------------------------------

/// Accepts only paths inside the managed scope: library folders, mounted
/// folders, the built-in import folder, the trash, or a user-specified absolute
/// path. Relative paths are rejected because they cannot be attributed.
pub(crate) fn validate_task_path_scope(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("任务路径不能为空".to_owned());
    }
    if !Path::new(trimmed).is_absolute() {
        return Err(format!(
            "任务路径必须在允许范围内（库内/已挂载/内置导入/回收区/用户显式指定），需为绝对路径: {trimmed}"
        ));
    }
    Ok(())
}

fn validate_paths<I: IntoIterator<Item = String>>(paths: I) -> Result<(), String> {
    for path in paths {
        validate_task_path_scope(&path)?;
    }
    Ok(())
}

/// Parses and range-checks the params before enqueue so a submission with an
/// out-of-scope path fails at the command layer, never mid-task.
pub(crate) fn validate_submission(kind: TaskKind, params: &serde_json::Value) -> Result<(), String> {
    match kind {
        TaskKind::Scan => {
            let parsed: ScanParams =
                serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
            validate_paths(parsed.folders.into_iter().map(|folder| folder.path))
        }
        TaskKind::Checksum => {
            let parsed: ChecksumParams =
                serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
            validate_paths(parsed.paths)
        }
        TaskKind::BatchCopy => {
            let parsed: BatchCopyParams =
                serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
            validate_paths(parsed.items.iter().flat_map(|item| {
                [item.source_path.clone(), item.target_directory.clone()]
            }))
        }
        TaskKind::BatchRename => {
            let parsed: BatchRenameParams =
                serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
            validate_paths(parsed.items.into_iter().map(|item| item.source_path))
        }
        TaskKind::BatchTrash => {
            let parsed: BatchTrashParams =
                serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
            validate_paths(parsed.items.into_iter().map(|item| item.source_path))
        }
        TaskKind::Restore => {
            let parsed: RestoreParams =
                serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
            validate_paths(parsed.items.iter().flat_map(|item| {
                [item.trash_path.clone(), item.target_path.clone()]
            }))
        }
        TaskKind::Index => {
            let _parsed: IndexTaskParams =
                serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
            Ok(())
        }
        TaskKind::ExportZip => {
            let parsed: ExportRequest =
                serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
            validate_task_path_scope(&parsed.output_path)?;
            validate_paths(parsed.entries.into_iter().map(|entry| entry.source_path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct RecordingSink {
        progress: Mutex<Vec<TaskProgressEvent>>,
        finished: Mutex<Vec<TaskFinishedEvent>>,
        queue_changes: Mutex<Vec<TaskQueueSnapshot>>,
        cancel_target: Mutex<Option<String>>,
        queue: Mutex<Option<Arc<LibraryTaskQueue>>>,
        progress_calls: AtomicUsize,
    }

    impl TaskEventSink for RecordingSink {
        fn task_progress(&self, event: &TaskProgressEvent) {
            self.progress_calls.fetch_add(1, Ordering::SeqCst);
            self.progress.lock().unwrap().push(event.clone());
            let target = self.cancel_target.lock().unwrap().clone();
            if target.as_deref() == Some(event.task_id.as_str()) && event.processed >= 1 {
                if let Some(queue) = self.queue.lock().unwrap().clone() {
                    queue.cancel(&event.task_id);
                }
                *self.cancel_target.lock().unwrap() = None;
            }
        }

        fn task_finished(&self, event: &TaskFinishedEvent) {
            self.finished.lock().unwrap().push(event.clone());
        }

        fn task_queue_changed(&self, snapshot: &TaskQueueSnapshot) {
            self.queue_changes.lock().unwrap().push(snapshot.clone());
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nova-library-tasks-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn build_queue(sink: Arc<RecordingSink>) -> Arc<LibraryTaskQueue> {
        let config = Arc::new(|| Ok(std::env::temp_dir()));
        let queue = LibraryTaskQueue::new(sink.clone(), config);
        *sink.queue.lock().unwrap() = Some(Arc::clone(&queue));
        queue
    }

    fn wait_idle(queue: &Arc<LibraryTaskQueue>, sink: &RecordingSink, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let settled = sink.finished.lock().unwrap().len() >= expected
                && queue.snapshot().running.is_none()
                && queue.snapshot().pending.is_empty();
            if settled {
                return;
            }
            assert!(Instant::now() < deadline, "task did not settle in time");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn finished_statuses(sink: &RecordingSink) -> Vec<String> {
        sink.finished
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.status.clone())
            .collect()
    }

    #[test]
    fn queue_runs_tasks_serially_in_submission_order() {
        let dir = test_dir("serial");
        let mut paths = Vec::new();
        for index in 0..4 {
            let path = dir.join(format!("{index}.litematic"));
            fs::write(&path, format!("payload-{index}")).unwrap();
            paths.push(path.display().to_string());
        }
        let sink = Arc::new(RecordingSink::default());
        let queue = build_queue(sink.clone());

        let first = queue.submit(TaskKind::Checksum, serde_json::json!({ "paths": paths }));
        let second = queue.submit(TaskKind::Checksum, serde_json::json!({ "paths": paths }));
        let third = queue.submit(TaskKind::Checksum, serde_json::json!({ "paths": paths }));
        wait_idle(&queue, &sink, 3);

        let finished = sink.finished.lock().unwrap();
        let ids: Vec<String> = finished.iter().map(|event| event.task_id.clone()).collect();
        assert_eq!(ids, vec![first, second, third]);
        drop(finished);
        assert_eq!(finished_statuses(&sink), vec!["completed", "completed", "completed"]);

        // Queue-change events must accompany submission, running and completion.
        let changes = sink.queue_changes.lock().unwrap();
        assert!(!changes.is_empty(), "queue change events must be emitted");
        assert!(changes.iter().any(|snapshot| snapshot.running.is_some()),
            "at least one snapshot must show a running task");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancellation_between_items_keeps_completed_results() {
        let dir = test_dir("cancel");
        let mut paths = Vec::new();
        for index in 0..64 {
            let path = dir.join(format!("{index}.litematic"));
            fs::write(&path, vec![index as u8; 512]).unwrap();
            paths.push(path.display().to_string());
        }
        let sink = Arc::new(RecordingSink::default());
        let queue = build_queue(sink.clone());

        let task_id = queue.submit(TaskKind::Checksum, serde_json::json!({ "paths": paths }));
        *sink.cancel_target.lock().unwrap() = Some(task_id.clone());
        wait_idle(&queue, &sink, 1);

        let finished = sink.finished.lock().unwrap();
        let event = &finished[0];
        assert_eq!(event.status, "cancelled");
        assert!(event.failures.is_empty());
        let processed = event.summary["processed"].as_u64().unwrap_or(0);
        assert!(processed >= 1, "already-finished items must be kept and reported");
        assert!(processed < 64, "cancellation must stop before the tail");
        drop(finished);

        // Progress events count up to the point of cancellation.
        let progress = sink.progress.lock().unwrap();
        assert!(progress.iter().any(|event| event.task_id == task_id && event.processed >= 1));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn per_item_failures_are_reported_without_failing_the_batch() {
        let dir = test_dir("failures");
        let good = dir.join("good.litematic");
        fs::write(&good, b"ok").unwrap();
        let missing = dir.join("missing.litematic");
        let sink = Arc::new(RecordingSink::default());
        let queue = build_queue(sink.clone());

        queue.submit(
            TaskKind::Checksum,
            serde_json::json!({ "paths": [good.display().to_string(), missing.display().to_string()] }),
        );
        wait_idle(&queue, &sink, 1);

        let finished = sink.finished.lock().unwrap();
        assert_eq!(finished[0].status, "completed");
        assert_eq!(finished[0].failures.len(), 1);
        assert_eq!(finished[0].failures[0].path, missing.display().to_string());
        assert!(!finished[0].failures[0].reason.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_params_fail_the_task_without_panicking() {
        let sink = Arc::new(RecordingSink::default());
        let queue = build_queue(sink.clone());
        queue.submit(TaskKind::Scan, serde_json::json!({ "folders": "not-an-array" }));
        wait_idle(&queue, &sink, 1);
        assert_eq!(finished_statuses(&sink), vec!["failed"]);
    }

    #[test]
    fn scan_skips_non_archive_folders_and_reuses_size_mtime_cache() {
        let dir = test_dir("scan-cache");
        let tracked_dir = dir.join("tracked");
        let non_archive_dir = dir.join("non-archive");
        fs::create_dir_all(&tracked_dir).unwrap();
        fs::create_dir_all(&non_archive_dir).unwrap();
        fs::write(tracked_dir.join("a.litematic"), b"alpha").unwrap();
        fs::write(tracked_dir.join("b.litematic"), b"beta").unwrap();
        fs::write(non_archive_dir.join("c.litematic"), b"gamma").unwrap();

        let sink = Arc::new(RecordingSink::default());
        let queue = build_queue(sink.clone());
        let params = serde_json::json!({
            "folders": [
                { "path": tracked_dir.display().to_string(), "type": "tracked" },
                { "path": non_archive_dir.display().to_string(), "type": "non_archive" }
            ],
            "useCache": true,
            "forceRecheck": false,
            "scope": { "kind": "all" }
        });
        queue.submit(TaskKind::Scan, params.clone());
        wait_idle(&queue, &sink, 1);
        let first = sink.finished.lock().unwrap()[0].clone();
        assert_eq!(first.status, "completed");
        assert_eq!(first.summary["scannedCount"].as_u64(), Some(2));
        assert_eq!(first.summary["skippedNonArchive"].as_u64(), Some(1));
        assert_eq!(first.summary["cacheHits"].as_u64(), Some(0));
        assert_eq!(first.summary["files"].as_array().unwrap().len(), 2);

        queue.submit(TaskKind::Scan, params.clone());
        wait_idle(&queue, &sink, 2);
        let second = sink.finished.lock().unwrap()[1].clone();
        assert_eq!(second.summary["cacheHits"].as_u64(), Some(2));

        // forceRecheck bypasses the size+mtime cache.
        let mut forced = params.clone();
        forced["forceRecheck"] = serde_json::json!(true);
        queue.submit(TaskKind::Scan, forced);
        wait_idle(&queue, &sink, 3);
        let third = sink.finished.lock().unwrap()[2].clone();
        assert_eq!(third.summary["cacheHits"].as_u64(), Some(0));
        assert_eq!(third.summary["checksumComputed"].as_u64(), Some(2));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn batch_copy_reports_conflicts_without_overwriting() {
        let dir = test_dir("copy");
        let source = dir.join("source.litematic");
        fs::write(&source, b"source").unwrap();
        let target_dir = dir.join("target");
        fs::create_dir_all(&target_dir).unwrap();
        let existing = target_dir.join("taken.litematic");
        fs::write(&existing, b"existing").unwrap();

        let sink = Arc::new(RecordingSink::default());
        let queue = build_queue(sink.clone());
        queue.submit(
            TaskKind::BatchCopy,
            serde_json::json!({
                "items": [
                    {
                        "sourcePath": source.display().to_string(),
                        "targetDirectory": target_dir.display().to_string(),
                        "targetFileName": "new.litematic",
                        "overwrite": false
                    },
                    {
                        "sourcePath": source.display().to_string(),
                        "targetDirectory": target_dir.display().to_string(),
                        "targetFileName": "taken.litematic",
                        "overwrite": false
                    }
                ]
            }),
        );
        wait_idle(&queue, &sink, 1);
        let finished = sink.finished.lock().unwrap();
        assert_eq!(finished[0].status, "completed");
        assert_eq!(finished[0].summary["copied"].as_u64(), Some(1));
        assert_eq!(finished[0].failures.len(), 1);
        assert_eq!(fs::read(&existing).unwrap(), b"existing");
        assert!(target_dir.join("new.litematic").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn batch_trash_runs_through_the_task_queue() {
        let dir = test_dir("task-trash");
        let config = dir.join("config");
        let source_dir = dir.join("library");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("a.litematic");
        fs::write(&source, b"bytes").unwrap();

        let sink = Arc::new(RecordingSink::default());
        let config_clone = config.clone();
        let queue = LibraryTaskQueue::new(
            sink.clone(),
            Arc::new(move || Ok(config_clone.clone())),
        );
        *sink.queue.lock().unwrap() = Some(Arc::clone(&queue));
        queue.submit(
            TaskKind::BatchTrash,
            serde_json::json!({
                "items": [{ "sourcePath": source.display().to_string(), "trashName": "a.litematic" }]
            }),
        );
        wait_idle(&queue, &sink, 1);
        let finished = sink.finished.lock().unwrap();
        assert_eq!(finished[0].status, "completed");
        assert_eq!(finished[0].summary["trashed"].as_u64(), Some(1));
        assert!(!source.exists());
        assert!(library_trash::trash_directory(&config).join("a.litematic").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn index_task_builds_and_persists_the_inverted_index() {
        let dir = test_dir("task-index");
        let config = dir.join("config");
        let sink = Arc::new(RecordingSink::default());
        let config_clone = config.clone();
        let queue = LibraryTaskQueue::new(
            sink.clone(),
            Arc::new(move || Ok(config_clone.clone())),
        );
        *sink.queue.lock().unwrap() = Some(Arc::clone(&queue));
        queue.submit(
            TaskKind::Index,
            serde_json::json!({
                "entries": [{
                    "entryId": "entry:a",
                    "displayName": "Redstone Machine",
                    "fileName": "a.litematic",
                    "author": "Alice",
                    "description": "",
                    "tags": ["automation"],
                    "categoryPath": "Redstone",
                    "version": "1.20"
                }]
            }),
        );
        wait_idle(&queue, &sink, 1);
        let finished = sink.finished.lock().unwrap();
        assert_eq!(finished[0].status, "completed");
        assert_eq!(finished[0].summary["indexedCount"].as_u64(), Some(1));
        assert_eq!(finished[0].summary["added"].as_u64(), Some(1));
        assert!(crate::application::library_index::index_file_path(&config).is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn path_scope_rejects_relative_paths_and_accepts_absolute() {
        assert!(validate_task_path_scope("relative/a.litematic").is_err());
        assert!(validate_task_path_scope("  ").is_err());
        let absolute = std::env::temp_dir().join("a.litematic");
        assert!(validate_task_path_scope(&absolute.display().to_string()).is_ok());
    }

    #[test]
    fn checksum_requires_a_litematic_extension() {
        let dir = test_dir("checksum-extension");
        let plain = dir.join("notes.txt");
        fs::write(&plain, b"text").unwrap();
        let error = compute_litematic_checksum(&plain).unwrap_err();
        assert!(error.contains("不是 litematic 文件"));
        let litematic = dir.join("real.litematic");
        fs::write(&litematic, b"bytes").unwrap();
        let checksum = compute_litematic_checksum(&litematic).unwrap();
        assert_eq!(checksum.byte_count, 5);
        assert_eq!(checksum.sha256.len(), 64);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn new_format_scan_candidates_require_the_authoritative_inspector_and_cache_validated_identity() {
        let dir=test_dir("format-inspector");
        fs::write(dir.join("structure.nbt"),b"valid fixture").unwrap();
        fs::write(dir.join("template.json"),b"not a template").unwrap();
        fs::write(dir.join("image.png"),b"image").unwrap();
        let mut candidates=Vec::new();let mut failures=Vec::new();collect_litematic_files(&dir,&mut candidates,&mut failures);
        assert_eq!(candidates.len(),2);assert!(failures.is_empty());
        let sink=Arc::new(RecordingSink::default());let queue=build_queue(sink);
        let calls=Arc::new(AtomicUsize::new(0));let seen=calls.clone();
        queue.set_format_inspector(Arc::new(move|path|{
            seen.fetch_add(1,Ordering::SeqCst);
            if fs::read(path).map_err(|e|e.to_string())?==b"valid fixture"{Ok(())}else{Err("actual format inspection failed".into())}
        })).unwrap();
        let path=dir.join("structure.nbt");assert!(!queue.scan_one(&path,true,false).unwrap().from_cache);assert!(queue.scan_one(&path,true,false).unwrap().from_cache);
        assert!(queue.scan_one(&dir.join("template.json"),true,false).unwrap_err().contains("inspection failed"));assert_eq!(calls.load(Ordering::SeqCst),2);
        assert!(!queue.scan_one(&path,true,true).unwrap().from_cache);assert_eq!(calls.load(Ordering::SeqCst),3);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn explicit_projection_checksum_routes_non_litematic_content_without_extension_authority(){
        let dir=test_dir("format-checksum");let path=dir.join("template.any-extension");fs::write(&path,b"content").unwrap();
        let sink=Arc::new(RecordingSink::default());let queue=build_queue(sink);let calls=Arc::new(AtomicUsize::new(0));let seen=calls.clone();
        queue.set_format_inspector(Arc::new(move|_|{seen.fetch_add(1,Ordering::SeqCst);Ok(())})).unwrap();
        assert_eq!(queue.compute_projection_checksum(&path).unwrap().byte_count,7);assert_eq!(calls.load(Ordering::SeqCst),1);
        assert!(!is_schematic_candidate(&path));assert!(is_schematic_candidate(Path::new("a.NBT")));assert!(is_schematic_candidate(Path::new("a.bgcp")));assert!(!is_schematic_candidate(Path::new("a.schem")));
        fs::remove_dir_all(&dir).unwrap();
    }
}
