//! Background inverted index for the projection library (spec 4.1.4 / 5.8.1.5).
//!
//! The index is a **read-only projection** of the library data that lives in TS:
//! Rust never owns the library truth (design 2.1.2 / 2.3.4). An `index` task
//! hands the current indexable entry set over the single serial queue, and this
//! module diffs it by `entryId` — adding, updating and removing only what
//! changed — so a library mutation never triggers a full rebuild (design 2.4.3).
//!
//! Persistence is a derived artifact under `projection-library/index/index.json`
//! written through the shared atomic-write primitive. It is safe to delete: the
//! next `index` task rebuilds it. While a build is running the index reports
//! `ready=false` and still serves the already-indexed partial results.
//!
//! Tokenization lowercases and splits on non-alphanumeric / camelCase bounds;
//! prefix lookup walks the sorted token keys (`BTreeMap::range`) with a binary
//! search, breaking as soon as a token no longer starts with the query term.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// On-disk index schema. A mismatch is treated as "not loaded" so the derived
/// index is simply rebuilt instead of failing.
pub(crate) const INDEX_SCHEMA_VERSION: u32 = 1;
/// Fixed derived-data directory under the user config directory.
pub(crate) const INDEX_DIRECTORY: &str = "projection-library/index";
/// Fixed file name (裁决 4): `projection-library/index/index.json`.
pub(crate) const INDEX_FILE_NAME: &str = "index.json";

/// One entry's indexable projection handed over by the TS service layer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct IndexedEntry {
    pub entry_id: String,
    pub display_name: String,
    pub file_name: String,
    pub author: String,
    pub description: String,
    pub tags: Vec<String>,
    pub category_path: String,
    pub version: String,
}

/// Searchable fields (`IndexField ∈ { displayName, fileName, author, description,
/// tags, categoryPath, version }`, design 2.2.2(C)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum IndexField {
    DisplayName,
    FileName,
    Author,
    Description,
    Tags,
    CategoryPath,
    Version,
}

impl IndexField {
    pub(crate) const ALL: [IndexField; 7] = [
        IndexField::DisplayName,
        IndexField::FileName,
        IndexField::Author,
        IndexField::Description,
        IndexField::Tags,
        IndexField::CategoryPath,
        IndexField::Version,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            IndexField::DisplayName => "displayName",
            IndexField::FileName => "fileName",
            IndexField::Author => "author",
            IndexField::Description => "description",
            IndexField::Tags => "tags",
            IndexField::CategoryPath => "categoryPath",
            IndexField::Version => "version",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct IndexQuery {
    pub text: String,
    #[serde(default)]
    pub fields: Option<Vec<IndexField>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IndexQueryResult {
    pub entry_ids: Vec<String>,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IndexStatus {
    pub ready: bool,
    pub indexed_count: usize,
    pub building: bool,
}

/// Result of one incremental `index` task run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexUpdateOutcome {
    pub indexed_count: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub cancelled: bool,
}

/// Sorted token → entryId set, per field.
type FieldIndex = BTreeMap<String, BTreeMap<String, BTreeSet<String>>>;

/// Persisted index file. `entries` is the diff baseline; `fields` the inverted
/// map. Both are rebuilt from `entries` if the map is missing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    schema_version: u32,
    #[serde(default)]
    entries: BTreeMap<String, IndexedEntry>,
    #[serde(default)]
    fields: FieldIndex,
}

#[derive(Default)]
struct IndexInner {
    loaded: bool,
    ready: bool,
    entries: BTreeMap<String, IndexedEntry>,
    fields: FieldIndex,
}

/// Shared, thread-safe index. The queue worker builds it while IPC reads query it.
#[derive(Default)]
pub(crate) struct LibraryIndexState {
    inner: Mutex<IndexInner>,
    building: AtomicBool,
}

static SHARED: OnceLock<LibraryIndexState> = OnceLock::new();

/// Process-wide index instance shared by the `index` executor and the IPC shell.
pub(crate) fn shared() -> &'static LibraryIndexState {
    SHARED.get_or_init(LibraryIndexState::default)
}

pub(crate) fn index_directory(config_directory: &Path) -> PathBuf {
    config_directory.join(INDEX_DIRECTORY)
}

pub(crate) fn index_file_path(config_directory: &Path) -> PathBuf {
    index_directory(config_directory).join(INDEX_FILE_NAME)
}

impl LibraryIndexState {
    fn lock(&self) -> std::sync::MutexGuard<'_, IndexInner> {
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// Read-only lookup. Never mutates the index beyond the first lazy load.
    pub(crate) fn query(&self, config_directory: &Path, query: &IndexQuery) -> IndexQueryResult {
        let mut inner = self.lock();
        inner.ensure_loaded(config_directory);
        let ready = inner.ready && !self.building.load(Ordering::SeqCst);
        let selected = selected_fields(query);
        let terms = query_terms(&query.text);
        if terms.is_empty() {
            return IndexQueryResult { entry_ids: inner.entries.keys().cloned().collect(), ready };
        }
        let mut matched: Option<BTreeSet<String>> = None;
        for term in &terms {
            let mut term_ids = BTreeSet::new();
            for field in &selected {
                if let Some(field_map) = inner.fields.get(field.as_str()) {
                    term_ids.extend(prefix_matches(field_map, term));
                }
            }
            matched = Some(match matched {
                None => term_ids,
                Some(previous) => previous.intersection(&term_ids).cloned().collect(),
            });
            if matched.as_ref().is_some_and(BTreeSet::is_empty) {
                break;
            }
        }
        IndexQueryResult { entry_ids: matched.unwrap_or_default().into_iter().collect(), ready }
    }

    /// Readiness snapshot for the "index is building, showing partial results" UI.
    pub(crate) fn status(&self, config_directory: &Path) -> IndexStatus {
        let mut inner = self.lock();
        inner.ensure_loaded(config_directory);
        let building = self.building.load(Ordering::SeqCst);
        IndexStatus {
            ready: inner.ready && !building,
            indexed_count: inner.entries.len(),
            building,
        }
    }

    /// Applies one incremental update: entries absent from `entries` are removed,
    /// new ones are added, changed ones re-tokenized. Results of already-applied
    /// entries survive cancellation (4.2.6).
    pub(crate) fn apply_entries(
        &self,
        config_directory: &Path,
        entries: &[IndexedEntry],
        should_cancel: &dyn Fn() -> bool,
    ) -> IndexUpdateOutcome {
        let mut inner = self.lock();
        inner.ensure_loaded(config_directory);
        self.building.store(true, Ordering::SeqCst);

        let incoming: BTreeMap<String, IndexedEntry> = entries
            .iter()
            .map(|entry| (entry.entry_id.clone(), entry.clone()))
            .collect();

        let mut added = 0usize;
        let mut updated = 0usize;
        let mut removed = 0usize;
        let mut cancelled = false;

        let stale: Vec<String> = inner
            .entries
            .keys()
            .filter(|id| !incoming.contains_key(*id))
            .cloned()
            .collect();
        for id in stale {
            if should_cancel() {
                cancelled = true;
                break;
            }
            if let Some(old) = inner.entries.remove(&id) {
                unindex_entry(&mut inner.fields, &old);
                removed += 1;
            }
        }

        if !cancelled {
            for (id, entry) in &incoming {
                if should_cancel() {
                    cancelled = true;
                    break;
                }
                let existing = inner.entries.get(id).cloned();
                if existing.as_ref() == Some(entry) {
                    continue;
                }
                match existing {
                    Some(old) => {
                        unindex_entry(&mut inner.fields, &old);
                        updated += 1;
                    }
                    None => added += 1,
                }
                index_entry(&mut inner.fields, entry);
                inner.entries.insert(id.clone(), entry.clone());
            }
        }

        inner.ready = true;
        let indexed_count = inner.entries.len();
        let file = IndexFile {
            schema_version: INDEX_SCHEMA_VERSION,
            entries: inner.entries.clone(),
            fields: inner.fields.clone(),
        };
        drop(inner);

        // Persistence is best-effort: the in-memory index still answers queries,
        // and the derived file is rebuilt on the next task (design 2.3.4).
        let _ = persist_index(config_directory, &file);
        self.building.store(false, Ordering::SeqCst);

        IndexUpdateOutcome { indexed_count, added, updated, removed, cancelled }
    }
}

impl IndexInner {
    fn ensure_loaded(&mut self, config_directory: &Path) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let path = index_file_path(config_directory);
        let Ok(bytes) = fs::read(&path) else { return };
        let Ok(file) = serde_json::from_slice::<IndexFile>(&bytes) else { return };
        if file.schema_version != INDEX_SCHEMA_VERSION {
            return;
        }
        self.entries = file.entries;
        self.fields = file.fields;
        if self.fields.is_empty() && !self.entries.is_empty() {
            self.fields = build_field_index(&self.entries);
        }
        self.ready = true;
    }
}

fn persist_index(config_directory: &Path, file: &IndexFile) -> Result<(), String> {
    let directory = index_directory(config_directory);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = file_path_in(&directory);
    let bytes = serde_json::to_vec_pretty(file).map_err(|error| error.to_string())?;
    let _serial = crate::CONFIG_WRITE_LOCK.lock().map_err(|_| "配置保存锁异常".to_owned())?;
    crate::write_file_atomically(&path, &bytes).map_err(|error| error.to_string())
}

fn file_path_in(directory: &Path) -> PathBuf {
    directory.join(INDEX_FILE_NAME)
}

fn selected_fields(query: &IndexQuery) -> Vec<IndexField> {
    match &query.fields {
        Some(fields) if !fields.is_empty() => fields.clone(),
        _ => IndexField::ALL.to_vec(),
    }
}

fn query_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in split_terms(text) {
        let lowered = term.to_lowercase();
        if !lowered.is_empty() && !terms.contains(&lowered) {
            terms.push(lowered);
        }
    }
    terms
}

/// Lowercases and splits on non-alphanumeric characters and camelCase bounds.
fn split_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut previous_lowercase = false;
    for character in text.chars() {
        if character.is_alphanumeric() {
            if character.is_uppercase() && previous_lowercase && !current.is_empty() {
                terms.push(std::mem::take(&mut current));
            }
            previous_lowercase = character.is_lowercase();
            current.push(character);
        } else {
            if !current.is_empty() {
                terms.push(std::mem::take(&mut current));
            }
            previous_lowercase = false;
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

fn tokenize(text: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    for term in split_terms(text) {
        let lowered = term.to_lowercase();
        if !lowered.is_empty() {
            tokens.insert(lowered);
        }
    }
    tokens
}

fn entry_field_tokens(entry: &IndexedEntry) -> [(&'static str, BTreeSet<String>); 7] {
    [
        ("displayName", tokenize(&entry.display_name)),
        ("fileName", tokenize(&entry.file_name)),
        ("author", tokenize(&entry.author)),
        ("description", tokenize(&entry.description)),
        ("tags", tokenize(&entry.tags.join(" "))),
        ("categoryPath", tokenize(&entry.category_path)),
        ("version", tokenize(&entry.version)),
    ]
}

fn index_entry(fields: &mut FieldIndex, entry: &IndexedEntry) {
    for (field, tokens) in entry_field_tokens(entry) {
        let field_map = fields.entry(field.to_owned()).or_default();
        for token in tokens {
            field_map.entry(token).or_default().insert(entry.entry_id.clone());
        }
    }
}

fn unindex_entry(fields: &mut FieldIndex, entry: &IndexedEntry) {
    for (field, tokens) in entry_field_tokens(entry) {
        let Some(field_map) = fields.get_mut(field) else { continue };
        for token in tokens {
            if let Some(ids) = field_map.get_mut(&token) {
                ids.remove(&entry.entry_id);
                if ids.is_empty() {
                    field_map.remove(&token);
                }
            }
        }
        if field_map.is_empty() {
            fields.remove(field);
        }
    }
}

fn build_field_index(entries: &BTreeMap<String, IndexedEntry>) -> FieldIndex {
    let mut fields = FieldIndex::new();
    for entry in entries.values() {
        index_entry(&mut fields, entry);
    }
    fields
}

/// Sorted-token prefix lookup: `range` binary-searches to the first token >= the
/// prefix, then walks forward while tokens still start with it.
fn prefix_matches(field_map: &BTreeMap<String, BTreeSet<String>>, prefix: &str) -> BTreeSet<String> {
    let mut matched = BTreeSet::new();
    for (token, ids) in field_map.range(prefix.to_owned()..) {
        if !token.starts_with(prefix) {
            break;
        }
        matched.extend(ids.iter().cloned());
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nova-library-index-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn entry(id: &str, display: &str, author: &str, tags: &[&str], category: &str) -> IndexedEntry {
        IndexedEntry {
            entry_id: id.to_owned(),
            display_name: display.to_owned(),
            file_name: format!("{id}.litematic"),
            author: author.to_owned(),
            description: String::new(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            category_path: category.to_owned(),
            version: "1.20".to_owned(),
        }
    }

    fn never_cancel() -> impl Fn() -> bool {
        || false
    }

    #[test]
    fn builds_prefix_index_and_queries_across_fields() {
        let dir = temp_dir("build");
        let index = LibraryIndexState::default();
        index.apply_entries(
            &dir,
            &[
                entry("entry:a", "Redstone Machine", "Alice", &["automation"], "Redstone / Farms"),
                entry("entry:b", "StoneBrick Wall", "Bob", &["decor"], "Building"),
            ],
            &never_cancel(),
        );

        let status = index.status(&dir);
        assert!(status.ready);
        assert_eq!(status.indexed_count, 2);
        assert!(!status.building);

        let by_display = index.query(&dir, &IndexQuery { text: "redst".to_owned(), fields: None });
        assert_eq!(by_display.entry_ids, vec!["entry:a".to_owned()]);

        // camelCase splitting: "StoneBrick" is indexed as "stone" + "brick".
        let by_camel = index.query(&dir, &IndexQuery { text: "brick".to_owned(), fields: None });
        assert_eq!(by_camel.entry_ids, vec!["entry:b".to_owned()]);

        let by_tag = index.query(&dir, &IndexQuery { text: "autom".to_owned(), fields: None });
        assert_eq!(by_tag.entry_ids, vec!["entry:a".to_owned()]);

        let by_category = index.query(&dir, &IndexQuery { text: "farm".to_owned(), fields: None });
        assert_eq!(by_category.entry_ids, vec!["entry:a".to_owned()]);

        // Field restriction narrows the match: "alice" is only in the author field.
        let author_only = index.query(
            &dir,
            &IndexQuery { text: "alice".to_owned(), fields: Some(vec![IndexField::Author]) },
        );
        assert_eq!(author_only.entry_ids, vec!["entry:a".to_owned()]);
        let display_only = index.query(
            &dir,
            &IndexQuery { text: "alice".to_owned(), fields: Some(vec![IndexField::DisplayName]) },
        );
        assert!(display_only.entry_ids.is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn incremental_update_adds_updates_and_removes_by_entry_id() {
        let dir = temp_dir("incremental");
        let index = LibraryIndexState::default();
        let first = index.apply_entries(
            &dir,
            &[entry("entry:a", "Alpha", "Alice", &[], ""), entry("entry:b", "Beta", "Bob", &[], "")],
            &never_cancel(),
        );
        assert_eq!((first.added, first.updated, first.removed), (2, 0, 0));

        // Re-applying the same set is a no-op (no full rebuild).
        let unchanged = index.apply_entries(
            &dir,
            &[entry("entry:a", "Alpha", "Alice", &[], ""), entry("entry:b", "Beta", "Bob", &[], "")],
            &never_cancel(),
        );
        assert_eq!((unchanged.added, unchanged.updated, unchanged.removed), (0, 0, 0));

        // Change a, drop b, add c.
        let second = index.apply_entries(
            &dir,
            &[entry("entry:a", "Alpha Prime", "Alice", &[], ""), entry("entry:c", "Gamma", "Carol", &[], "")],
            &never_cancel(),
        );
        assert_eq!((second.added, second.updated, second.removed), (1, 1, 1));
        assert_eq!(second.indexed_count, 2);

        assert_eq!(
            index.query(&dir, &IndexQuery { text: "alpha".to_owned(), fields: None }).entry_ids,
            vec!["entry:a".to_owned()]
        );
        assert!(index.query(&dir, &IndexQuery { text: "beta".to_owned(), fields: None }).entry_ids.is_empty());
        assert_eq!(
            index.query(&dir, &IndexQuery { text: "gamma".to_owned(), fields: None }).entry_ids,
            vec!["entry:c".to_owned()]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn persisted_index_survives_reload_and_is_rebuildable() {
        let dir = temp_dir("persist");
        let index = LibraryIndexState::default();
        index.apply_entries(&dir, &[entry("entry:a", "Persisted", "Alice", &["tag"], "")], &never_cancel());
        assert!(index_file_path(&dir).is_file());

        // A fresh process (new state) loads the derived file.
        let reloaded = LibraryIndexState::default();
        let status = reloaded.status(&dir);
        assert!(status.ready);
        assert_eq!(status.indexed_count, 1);
        assert_eq!(
            reloaded.query(&dir, &IndexQuery { text: "persist".to_owned(), fields: None }).entry_ids,
            vec!["entry:a".to_owned()]
        );

        // Deleting the derived file only loses readiness, never library truth.
        fs::remove_file(index_file_path(&dir)).unwrap();
        let cold = LibraryIndexState::default();
        let cold_status = cold.status(&dir);
        assert!(!cold_status.ready);
        assert_eq!(cold_status.indexed_count, 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancellation_keeps_already_applied_entries_and_reports_not_ready() {
        let dir = temp_dir("cancel");
        let index = LibraryIndexState::default();
        index.apply_entries(&dir, &[entry("entry:a", "Alpha", "", &[], "")], &never_cancel());

        let calls = std::sync::atomic::AtomicUsize::new(0);
        let should_cancel = || calls.fetch_add(1, Ordering::SeqCst) >= 1;
        let outcome = index.apply_entries(
            &dir,
            &[entry("entry:a", "Alpha", "", &[], ""), entry("entry:b", "Beta", "", &[], "")],
            &should_cancel,
        );
        assert!(outcome.cancelled);
        assert_eq!(outcome.indexed_count, 1, "the second entry must not be applied");

        // Not building any more, and the applied partial index is queryable.
        let status = index.status(&dir);
        assert!(!status.building);
        assert_eq!(index.query(&dir, &IndexQuery { text: "beta".to_owned(), fields: None }).entry_ids.len(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}