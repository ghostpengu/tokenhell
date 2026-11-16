use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecursiveMode, Watcher};

use crate::model::{Provider, UsageEvent};
use crate::store::{FileRow, Store};

pub enum SourceMode {
    /// Append-only JSONL: parse new complete lines from the stored byte offset.
    Append,
    /// Whole-file snapshot (JSON / SQLite): re-parse everything when size or mtime changes.
    Snapshot,
}

pub trait HistorySource: Send + Sync {
    fn provider(&self) -> Provider;
    /// Directories to scan and watch. Missing ones are skipped.
    fn roots(&self) -> Vec<PathBuf>;
    fn matches(&self, path: &Path) -> bool;
    fn mode(&self) -> SourceMode {
        SourceMode::Append
    }
    /// Bump to force a re-parse of all files of this source.
    fn parser_version(&self) -> u32;
    /// Append mode: parse one complete line. `state` persists between calls for the same file.
    fn parse_line(&self, _path: &Path, _state: &mut serde_json::Value, _line: &str, _out: &mut Vec<UsageEvent>) {}
    /// Snapshot mode: parse the whole file.
    fn parse_snapshot(&self, _path: &Path, _out: &mut Vec<UsageEvent>) -> Result<()> {
        Ok(())
    }
    /// Extra files whose change should trigger a re-parse of `path` (e.g. SQLite `-wal`).
    fn watch_alias(&self, path: &Path) -> Option<PathBuf> {
        let _ = path;
        None
    }
}

pub struct Ingestor {
    pub sources: Vec<Box<dyn HistorySource>>,
    pub store: Arc<Store>,
    roots: RwLock<Vec<Vec<SourceRoot>>>,
}

#[derive(Clone)]
struct SourceRoot {
    /// Keep the configured spelling for stable database keys.
    logical: PathBuf,
    /// FSEvents can report the physical spelling of a symlinked root.
    physical: PathBuf,
}

/// Max bytes read from one file per pass, so a huge first scan doesn't spike memory.
const MAX_CHUNK: u64 = 32 * 1024 * 1024;

impl Ingestor {
    pub fn new(store: Arc<Store>, sources: Vec<Box<dyn HistorySource>>) -> Self {
        let roots = sources.iter().map(|source| source.roots().into_iter().map(|logical| {
            let physical = std::fs::canonicalize(&logical).unwrap_or_else(|_| logical.clone());
            SourceRoot { logical, physical }
        }).collect()).collect();
        Ingestor { sources, store, roots: RwLock::new(roots) }
    }

    fn refresh_roots(&self) -> Vec<Vec<SourceRoot>> {
        let mut roots = self.roots.write().unwrap_or_else(|e| e.into_inner());
        for root in roots.iter_mut().flatten() {
            root.physical = std::fs::canonicalize(&root.logical).unwrap_or_else(|_| root.logical.clone());
        }
        roots.clone()
    }

    /// Full scan of all roots. Returns number of events written.
    pub fn scan_all(&self) -> usize {
        let mut n = 0;
        let roots = self.refresh_roots();
        for (src, source_roots) in self.sources.iter().zip(roots) {
            for root in source_roots.into_iter().map(|r| r.logical) {
                if !root.exists() {
                    continue;
                }
                for entry in walkdir::WalkDir::new(&root).follow_links(false).into_iter().flatten() {
                    let p = entry.path();
                    if entry.file_type().is_file() && src.matches(p) {
                        n += self.process_watched_path(p).map_or(0, |(_, count)| count);
                    }
                }
            }
        }
        n
    }

    /// Process one changed path (from the watcher). Returns events written.
    pub fn process_path(&self, path: &Path) -> usize {
        self.process_watched_path(path).map_or(0, |(_, count)| count)
    }

    fn process_watched_path(&self, path: &Path) -> Option<(PathBuf, usize)> {
        let physical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let roots = self.roots.read().unwrap_or_else(|e| e.into_inner());
        let mut candidates = Vec::new();
        for (index, source_roots) in roots.iter().enumerate() {
            for root in source_roots {
                if let Ok(relative) = physical.strip_prefix(&root.physical) {
                    candidates.push((root.physical.components().count(), index, root.logical.join(relative)));
                } else if let Ok(relative) = path.strip_prefix(&root.logical) {
                    candidates.push((root.logical.components().count(), index, root.logical.join(relative)));
                }
            }
        }
        // A nested root is more specific than an earlier source accepting the same extension.
        candidates.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, index, logical_path) in candidates {
            let src = &self.sources[index];
            let target = if src.matches(&logical_path) { Some(logical_path) } else { src.watch_alias(&logical_path) };
            if let Some(target) = target.filter(|p| p.is_file()) {
                let count = self.process(src.as_ref(), &target).unwrap_or_else(|e| {
                    tracing::warn!("ingest {}: {e:#}", target.display());
                    0
                });
                return Some((target, count));
            }
        }
        None
    }

    fn process(&self, src: &dyn HistorySource, path: &Path) -> Result<usize> {
        let meta = std::fs::metadata(path)?;
        let key = path.to_string_lossy().to_string();
        let prev = self.store.file(&key)?;
        let (size, mtime) = match src.mode() {
            SourceMode::Snapshot => fingerprint(path, &meta),
            SourceMode::Append => (meta.len(), mtime_ms(&meta)),
        };
        let inode = meta.ino();
        let version = src.parser_version();

        match src.mode() {
            SourceMode::Snapshot => {
                if let Some(p) = &prev {
                    if p.size == size && p.mtime == mtime && p.parser_version == version {
                        return Ok(0);
                    }
                }
                let mut events = vec![];
                src.parse_snapshot(path, &mut events)?;
                let row = FileRow { inode, size, mtime, offset: size, parser_version: version, state: Default::default() };
                self.store.commit_file(src.provider(), &key, true, &events, &row)?;
                Ok(events.len())
            }
            SourceMode::Append => {
                let (mut row, reset) = match prev {
                    Some(p) if p.inode == inode && p.parser_version == version && size >= p.offset => (p, false),
                    _ => (
                        FileRow { inode, size: 0, mtime: 0, offset: 0, parser_version: version, state: serde_json::json!({}) },
                        true,
                    ),
                };
                if !reset && row.offset == size {
                    return Ok(0);
                }
                let mut total = 0;
                let mut reset = reset;
                // Loop in chunks until caught up.
                loop {
                    let mut f = std::fs::File::open(path)?;
                    f.seek(SeekFrom::Start(row.offset))?;
                    let mut buf = Vec::new();
                    f.take(MAX_CHUNK).read_to_end(&mut buf)?;
                    let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') else {
                        // No complete line yet (partial write). If chunk is full the line is huge: skip it.
                        if buf.len() as u64 >= MAX_CHUNK {
                            row.offset += buf.len() as u64;
                            continue;
                        }
                        break;
                    };
                    let complete = &buf[..=last_nl];
                    let text = String::from_utf8_lossy(complete);
                    let mut events = Vec::new();
                    for line in text.lines() {
                        let line = line.trim();
                        if !line.is_empty() {
                            src.parse_line(path, &mut row.state, line, &mut events);
                        }
                    }
                    row.offset += complete.len() as u64;
                    row.size = size;
                    row.mtime = mtime;
                    row.inode = inode;
                    self.store.commit_file(src.provider(), &key, reset, &events, &row)?;
                    reset = false;
                    total += events.len();
                    if (buf.len() as u64) < MAX_CHUNK {
                        break;
                    }
                }
                Ok(total)
            }
        }
    }

    /// Blocking: watch all roots and call `on_change` (debounced) whenever new events land.
    /// Also does a full rescan every `rescan` interval to catch missed events / new roots.
    pub fn watch(self: Arc<Self>, rescan: Duration, on_change: impl Fn(&[PathBuf], bool, bool) + Send + 'static) -> Result<()> {
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                for p in ev.paths {
                    let _ = tx.send(p);
                }
            }
        })?;
        let mut watched: HashSet<PathBuf> = HashSet::new();
        let watch_roots = |w: &mut notify::RecommendedWatcher, watched: &mut HashSet<PathBuf>| {
            for roots in self.roots.read().unwrap_or_else(|e| e.into_inner()).iter() {
                for root in roots.iter().map(|r| r.logical.clone()) {
                    if root.exists() && !watched.contains(&root) {
                        match w.watch(&root, RecursiveMode::Recursive) {
                            Ok(()) => {
                                watched.insert(root);
                            }
                            Err(e) => tracing::warn!("watch {}: {e}", root.display()),
                        }
                    }
                }
            }
        };
        watch_roots(&mut watcher, &mut watched);
        // Catch writes between the initial scan and watcher registration.
        let caught_up = self.scan_all();
        let mut hot = HotFiles::default();
        hot.refresh(&self.store);
        on_change(&[], caught_up > 0, true);

        let debounce = Duration::from_millis(400);
        let mut last_rescan = Instant::now();
        let mut last_poll = Instant::now();
        loop {
            let idle = if hot.is_empty() { Duration::from_secs(30) } else { HOT_POLL };
            let first = match rx.recv_timeout(idle.saturating_sub(last_poll.elapsed())) {
                Ok(p) => Some(p),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            };
            let mut pending: HashSet<PathBuf> = first.into_iter().collect();
            if !pending.is_empty() {
                let deadline = Instant::now() + debounce;
                while let Ok(p) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    pending.insert(p);
                }
            }
            let mut n = 0;
            let mut processed = Vec::new();
            for p in &pending {
                if let Some((target, count)) = self.process_watched_path(p) {
                    n += count;
                    hot.track(&target);
                    processed.push(target);
                }
            }
            if last_poll.elapsed() >= HOT_POLL {
                for (target, count) in hot.poll(&self) {
                    n += count;
                    if !processed.contains(&target) {
                        processed.push(target);
                    }
                }
                last_poll = Instant::now();
            }
            let rescanned = last_rescan.elapsed() >= rescan;
            if rescanned {
                n += self.scan_all();
                watch_roots(&mut watcher, &mut watched);
                hot.refresh(&self.store);
                last_rescan = Instant::now();
            }
            if n > 0 || !processed.is_empty() || rescanned {
                on_change(&processed, n > 0, rescanned);
            }
        }
    }
}

/// How often recently active files are stat'ed.
const HOT_POLL: Duration = Duration::from_secs(1);
/// Files untouched for this long stop being polled (the periodic rescan still covers them).
const HOT_WINDOW_MS: i64 = 30 * 60_000;

/// FSEvents reports content changes when a writer closes the file. Codex appends to its rollout
/// through a long-lived handle, so those appends produce no event until the session ends. Stat
/// recently active files on a short interval to catch writes the watcher never sees.
#[derive(Default)]
struct HotFiles {
    /// Logical path → last seen (size, mtime ms).
    files: std::collections::HashMap<PathBuf, (u64, i64)>,
}

impl HotFiles {
    fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Start polling a file that was just processed.
    fn track(&mut self, path: &Path) {
        if let Ok(meta) = std::fs::metadata(path) {
            self.files.insert(path.to_path_buf(), fingerprint(path, &meta));
        }
    }

    /// Add files recently checkpointed by a scan, using their stored fingerprint, so a write that
    /// landed after the checkpoint is picked up on the next poll.
    fn refresh(&mut self, store: &Store) {
        let since = chrono::Utc::now().timestamp_millis() - HOT_WINDOW_MS;
        if let Ok(rows) = store.recent_files(since) {
            for (path, size, mtime) in rows {
                self.files.entry(PathBuf::from(path)).or_insert((size, mtime));
            }
        }
    }

    /// Re-process files whose size or mtime changed. Returns (path, events written).
    fn poll(&mut self, ing: &Ingestor) -> Vec<(PathBuf, usize)> {
        let cutoff = chrono::Utc::now().timestamp_millis() - HOT_WINDOW_MS;
        let mut changed = Vec::new();
        self.files.retain(|path, seen| {
            let Ok(meta) = std::fs::metadata(path) else { return false };
            let now = fingerprint(path, &meta);
            if now != *seen {
                *seen = now;
                if let Some(hit) = ing.process_watched_path(path) {
                    changed.push(hit);
                }
            }
            now.1 >= cutoff
        });
        changed
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.mtime() * 1000 + meta.mtime_nsec() / 1_000_000
}

/// (size, mtime ms), folding in a SQLite `-wal` sidecar if present: in WAL mode new rows land in
/// `-wal` and the main file stays untouched until a checkpoint.
fn fingerprint(path: &Path, meta: &std::fs::Metadata) -> (u64, i64) {
    let (mut size, mut mtime) = (meta.len(), mtime_ms(meta));
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    // A read-only SQLite connection can create an empty WAL for a closed database.
    // Its timestamp is reader activity, not new usage.
    if let Some(w) = std::fs::metadata(&wal).ok().filter(|w| w.len() > 0) {
        size += w.len();
        mtime = mtime.max(mtime_ms(&w));
    }
    (size, mtime)
}

/// `$VAR` if set (may be a comma list), else defaults under home.
pub(crate) fn env_or_home(var: &str, defaults: &[&str]) -> Vec<PathBuf> {
    if let Ok(v) = std::env::var(var) {
        let v: Vec<PathBuf> = v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).collect();
        if !v.is_empty() {
            return v;
        }
    }
    let home = dirs::home_dir().unwrap_or_default();
    defaults.iter().map(|d| home.join(d)).collect()
}

pub(crate) fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e == ext)
}

pub(crate) fn file_stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use crate::providers::Codex;

    struct RootedSource { root: PathBuf, provider: Provider }
    impl HistorySource for RootedSource {
        fn provider(&self) -> Provider { self.provider }
        fn roots(&self) -> Vec<PathBuf> { vec![self.root.clone()] }
        fn matches(&self, path: &Path) -> bool { has_ext(path, "jsonl") }
        fn parser_version(&self) -> u32 { if self.provider == Provider::Codex { Codex.parser_version() } else { 1 } }
        fn parse_line(&self, path: &Path, state: &mut serde_json::Value, line: &str, out: &mut Vec<UsageEvent>) {
            if self.provider == Provider::Codex { Codex.parse_line(path, state, line, out); }
        }
    }

    #[test]
    fn watched_codex_logs_use_their_own_parser_and_track_turns() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        let codex = dir.path().join("codex");
        std::fs::create_dir_all(&codex).unwrap();
        let path = codex.join("session.jsonl");
        std::fs::write(&path, "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n").unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        // Reproduce a checkpoint previously claimed by the wrong JSONL parser.
        let key = path.to_str().unwrap();
        store.commit_file(Provider::Claude, key, false, &[], &FileRow {
            parser_version: 2, ..Default::default()
        }).unwrap();
        let ing = Ingestor::new(store.clone(), vec![
            Box::new(RootedSource { root: claude, provider: Provider::Claude }),
            Box::new(RootedSource { root: codex, provider: Provider::Codex }),
        ]);
        assert_eq!(ing.process_path(&path), 0);
        assert_eq!(store.file(key).unwrap().unwrap().state["turn_active"], true);
        assert_eq!(store.active_codex_files(0).unwrap().len(), 1);

        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        writeln!(file, "{{\"timestamp\":{now},\"type\":\"token_usage_record\",\"payload\":{{\"response_id\":\"r1\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":25}}}}}}").unwrap();
        assert_eq!(ing.process_path(&path), 1);
        let sum = crate::summary::summarize(&store, &crate::pricing::Pricing::default(), crate::summary::Range::Today).unwrap();
        assert_eq!(sum.tokens.total, 125);
        assert_eq!(sum.providers[0].provider, Provider::Codex);

        writeln!(file, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"turn_aborted\"}}}}").unwrap();
        assert_eq!(ing.process_path(&path), 0);
        assert_eq!(store.file(key).unwrap().unwrap().state["turn_active"], false);
        assert!(store.active_codex_files(0).unwrap().is_empty());
    }

    #[test]
    fn hot_files_catch_appends_through_a_held_open_handle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        // One handle for the whole session, as Codex does: FSEvents stays silent until close.
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\"}}}}").unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let ing = Ingestor::new(store.clone(), vec![
            Box::new(RootedSource { root: dir.path().to_path_buf(), provider: Provider::Codex }),
        ]);
        ing.scan_all();
        let mut hot = HotFiles::default();
        hot.refresh(&store);
        assert!(hot.poll(&ing).is_empty(), "unchanged files are not re-processed");

        let now = chrono::Utc::now().timestamp_millis();
        writeln!(file, "{{\"timestamp\":{now},\"type\":\"token_usage_record\",\"payload\":{{\"response_id\":\"r1\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":25}}}}}}").unwrap();
        assert_eq!(hot.poll(&ing), vec![(path.clone(), 1)]);
        assert!(hot.poll(&ing).is_empty());

        writeln!(file, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\"}}}}").unwrap();
        assert_eq!(hot.poll(&ing), vec![(path.clone(), 0)]);
        assert_eq!(store.file(path.to_str().unwrap()).unwrap().unwrap().state["turn_active"], false);

        drop(file);
        std::fs::remove_file(&path).unwrap();
        assert!(hot.poll(&ing).is_empty());
        assert!(hot.is_empty(), "deleted files stop being polled");
    }

    #[test]
    fn symlinked_and_overlapping_roots_keep_one_checkpoint() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let actual = dir.path().join("actual");
        std::fs::create_dir_all(&actual).unwrap();
        let link = dir.path().join("linked");
        symlink(&actual, &link).unwrap();
        let logical_path = link.join("session.jsonl");
        let physical_path = std::fs::canonicalize(&actual).unwrap().join("session.jsonl");
        std::fs::write(&logical_path, "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n").unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let ing = Ingestor::new(store.clone(), vec![
            Box::new(RootedSource { root: dir.path().to_path_buf(), provider: Provider::Claude }),
            Box::new(RootedSource { root: link, provider: Provider::Codex }),
        ]);
        ing.scan_all();
        assert_eq!(store.file(logical_path.to_str().unwrap()).unwrap().unwrap().state["turn_active"], true);

        let mut f = std::fs::OpenOptions::new().append(true).open(&logical_path).unwrap();
        writeln!(f, "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\"}}}}").unwrap();
        let (processed, count) = ing.process_watched_path(&physical_path).unwrap();
        assert_eq!(processed, logical_path);
        assert_eq!(count, 0);
        assert_eq!(store.file(logical_path.to_str().unwrap()).unwrap().unwrap().state["turn_active"], false);
        assert!(store.file(physical_path.to_str().unwrap()).unwrap().is_none());
    }

}
