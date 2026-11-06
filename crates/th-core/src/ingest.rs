use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
}

/// Max bytes read from one file per pass, so a huge first scan doesn't spike memory.
const MAX_CHUNK: u64 = 32 * 1024 * 1024;

impl Ingestor {
    pub fn new(store: Arc<Store>, sources: Vec<Box<dyn HistorySource>>) -> Self {
        Ingestor { sources, store }
    }

    /// Full scan of all roots. Returns number of events written.
    pub fn scan_all(&self) -> usize {
        let mut n = 0;
        for src in &self.sources {
            for root in src.roots() {
                if !root.exists() {
                    continue;
                }
                for entry in walkdir::WalkDir::new(&root).follow_links(false).into_iter().flatten() {
                    let p = entry.path();
                    if entry.file_type().is_file() && src.matches(p) {
                        n += self.process(src.as_ref(), p).unwrap_or_else(|e| {
                            tracing::warn!("ingest {}: {e:#}", p.display());
                            0
                        });
                    }
                }
            }
        }
        n
    }

    /// Process one changed path (from the watcher). Returns events written.
    pub fn process_path(&self, path: &Path) -> usize {
        for src in &self.sources {
            let target = if src.matches(path) {
                Some(path.to_path_buf())
            } else {
                src.watch_alias(path)
            };
            if let Some(t) = target {
                if t.is_file() {
                    return self.process(src.as_ref(), &t).unwrap_or_else(|e| {
                        tracing::warn!("ingest {}: {e:#}", t.display());
                        0
                    });
                }
            }
        }
        0
    }

    fn process(&self, src: &dyn HistorySource, path: &Path) -> Result<usize> {
        let meta = std::fs::metadata(path)?;
        let key = path.to_string_lossy().to_string();
        let prev = self.store.file(&key)?;
        let mut size = meta.len();
        let mut mtime = meta.mtime() * 1000 + meta.mtime_nsec() / 1_000_000;
        if matches!(src.mode(), SourceMode::Snapshot) {
            // SQLite in WAL mode: new rows land in `-wal` and the main file stays untouched until a
            // checkpoint, so fold the WAL into the fingerprint or we'd skip fresh data.
            let mut wal = path.as_os_str().to_owned();
            wal.push("-wal");
            if let Ok(w) = std::fs::metadata(&wal) {
                size += w.len();
                mtime = mtime.max(w.mtime() * 1000 + w.mtime_nsec() / 1_000_000);
            }
        }
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
    pub fn watch(self: Arc<Self>, rescan: Duration, on_change: impl Fn() + Send + 'static) -> Result<()> {
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
            for src in &self.sources {
                for root in src.roots() {
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

        let debounce = Duration::from_millis(400);
        let mut last_rescan = Instant::now();
        loop {
            let first = match rx.recv_timeout(Duration::from_secs(30)) {
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
            for p in pending {
                n += self.process_path(&p);
            }
            if last_rescan.elapsed() >= rescan {
                watch_roots(&mut watcher, &mut watched);
                n += self.scan_all();
                last_rescan = Instant::now();
            }
            if n > 0 {
                on_change();
            }
        }
    }
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
