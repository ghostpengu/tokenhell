use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{LimitReport, Provider, UsageEvent};

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Clone, Debug, Default)]
pub struct FileRow {
    pub inode: u64,
    pub size: u64,
    pub mtime: i64,
    pub offset: u64,
    pub parser_version: u32,
    pub state: serde_json::Value,
}

/// One counted event, with everything needed to price it.
#[derive(Clone, Debug, Default)]
pub struct EventRow {
    pub provider: String,
    pub ts: i64,
    pub model: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cache_write_1h: u64,
    pub reasoning: u64,
    pub web_search: u32,
    pub tier: Option<String>,
    pub cost_reported: Option<f64>,
    /// Aggregated delta (may span several requests): per-request tiers can't be applied.
    pub fallback: bool,
}

impl EventRow {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
    /// Prompt size of the request (all input incl. cache), used for context-length price tiers.
    pub fn prompt_tokens(&self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }
    pub fn has_tier(&self, flag: &str) -> bool {
        self.tier.as_deref().is_some_and(|t| t.split(',').any(|f| f == flag))
    }
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS files (
    path TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    inode INTEGER NOT NULL,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    offset INTEGER NOT NULL,
    parser_version INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS events (
    provider TEXT NOT NULL,
    dedup_key TEXT NOT NULL,
    file TEXT,
    session TEXT,
    ts INTEGER NOT NULL,
    model TEXT,
    input INTEGER NOT NULL DEFAULT 0,
    output INTEGER NOT NULL DEFAULT 0,
    cache_read INTEGER NOT NULL DEFAULT 0,
    cache_write INTEGER NOT NULL DEFAULT 0,
    reasoning INTEGER NOT NULL DEFAULT 0,
    cost_reported REAL,
    fallback INTEGER NOT NULL DEFAULT 0,
    cache_write_1h INTEGER NOT NULL DEFAULT 0,
    web_search INTEGER NOT NULL DEFAULT 0,
    tier TEXT,
    PRIMARY KEY (provider, dedup_key)
);
CREATE INDEX IF NOT EXISTS events_ts ON events(ts);
CREATE INDEX IF NOT EXISTS events_file ON events(file);
CREATE INDEX IF NOT EXISTS events_session ON events(provider, session, fallback);
CREATE TABLE IF NOT EXISTS limits (
    provider TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    observed_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT NOT NULL);
"#;

/// Events that count: fallback events are dropped when the same session has primary events.
const COUNTED: &str = r#"
    SELECT * FROM events e
    WHERE e.fallback = 0
       OR e.session IS NULL
       OR NOT EXISTS (
            SELECT 1 FROM events p
            WHERE p.provider = e.provider AND p.session = e.session AND p.fallback = 0
       )
"#;

/// Add columns introduced after v0.1 to existing databases.
fn migrate(conn: &Connection) -> Result<()> {
    let cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('events')")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for (col, ddl) in [
        ("cache_write_1h", "ALTER TABLE events ADD COLUMN cache_write_1h INTEGER NOT NULL DEFAULT 0"),
        ("web_search", "ALTER TABLE events ADD COLUMN web_search INTEGER NOT NULL DEFAULT 0"),
        ("tier", "ALTER TABLE events ADD COLUMN tier TEXT"),
    ] {
        if !cols.iter().any(|c| c == col) {
            conn.execute(ddl, [])?;
        }
    }
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn file(&self, path: &str) -> Result<Option<FileRow>> {
        let c = self.conn();
        Ok(c.query_row(
            "SELECT inode, size, mtime, offset, parser_version, state FROM files WHERE path = ?1",
            [path],
            |r| {
                let state: String = r.get(5)?;
                Ok(FileRow {
                    inode: r.get::<_, i64>(0)? as u64,
                    size: r.get::<_, i64>(1)? as u64,
                    mtime: r.get(2)?,
                    offset: r.get::<_, i64>(3)? as u64,
                    parser_version: r.get(4)?,
                    state: serde_json::from_str(&state).unwrap_or_default(),
                })
            },
        )
        .optional()?)
    }

    /// Atomically: optionally wipe the file's previous events, upsert new events, save the checkpoint.
    pub fn commit_file(
        &self,
        provider: Provider,
        path: &str,
        reset: bool,
        events: &[UsageEvent],
        row: &FileRow,
    ) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        if reset {
            tx.execute("DELETE FROM events WHERE file = ?1", [path])?;
        }
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO events (provider, dedup_key, file, session, ts, model, input, output,
                    cache_read, cache_write, reasoning, cost_reported, fallback, cache_write_1h, web_search, tier)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT (provider, dedup_key) DO UPDATE SET
                    file = excluded.file, session = excluded.session, ts = excluded.ts,
                    model = COALESCE(excluded.model, events.model),
                    input = excluded.input, output = excluded.output,
                    cache_read = excluded.cache_read, cache_write = excluded.cache_write,
                    reasoning = excluded.reasoning, cost_reported = excluded.cost_reported,
                    fallback = excluded.fallback, cache_write_1h = excluded.cache_write_1h,
                    web_search = excluded.web_search, tier = excluded.tier",
            )?;
            for e in events {
                stmt.execute(params![
                    provider.as_str(),
                    e.dedup_key,
                    path,
                    e.session,
                    e.ts,
                    e.model,
                    e.input as i64,
                    e.output as i64,
                    e.cache_read as i64,
                    e.cache_write as i64,
                    e.reasoning as i64,
                    e.cost_reported,
                    e.fallback as i64,
                    e.cache_write_1h as i64,
                    e.web_search as i64,
                    e.tier,
                ])?;
            }
        }
        tx.execute(
            "INSERT INTO files (path, provider, inode, size, mtime, offset, parser_version, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (path) DO UPDATE SET inode = excluded.inode, size = excluded.size,
                mtime = excluded.mtime, offset = excluded.offset,
                parser_version = excluded.parser_version, state = excluded.state",
            params![
                path,
                provider.as_str(),
                row.inode as i64,
                row.size as i64,
                row.mtime,
                row.offset as i64,
                row.parser_version,
                row.state.to_string(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Replace all events of a whole provider "virtual file" (used by API-backed sources like Cursor).
    pub fn replace_virtual(&self, provider: Provider, key: &str, events: &[UsageEvent]) -> Result<()> {
        let row = FileRow { parser_version: 1, ..Default::default() };
        self.commit_file(provider, key, false, events, &row)
    }

    /// All counted events (fallback-deduped) with `from_ms <= ts < to_ms`.
    pub fn events_in(&self, from_ms: i64, to_ms: i64) -> Result<Vec<EventRow>> {
        let c = self.conn();
        let sql = format!(
            "SELECT provider, ts, model, input, output, cache_read, cache_write, cache_write_1h,
                    reasoning, web_search, tier, cost_reported, fallback
             FROM ({COUNTED}) WHERE ts >= ?1 AND ts < ?2"
        );
        let mut stmt = c.prepare_cached(&sql)?;
        let rows = stmt.query_map([from_ms, to_ms], |r| {
            Ok(EventRow {
                provider: r.get(0)?,
                ts: r.get(1)?,
                model: r.get(2)?,
                input: r.get::<_, i64>(3)? as u64,
                output: r.get::<_, i64>(4)? as u64,
                cache_read: r.get::<_, i64>(5)? as u64,
                cache_write: r.get::<_, i64>(6)? as u64,
                cache_write_1h: r.get::<_, i64>(7)? as u64,
                reasoning: r.get::<_, i64>(8)? as u64,
                web_search: r.get::<_, i64>(9)? as u32,
                tier: r.get(10)?,
                cost_reported: r.get(11)?,
                fallback: r.get::<_, i64>(12)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_limit(&self, report: &LimitReport) -> Result<()> {
        let c = self.conn();
        c.execute(
            "INSERT INTO limits (provider, json, observed_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (provider) DO UPDATE SET json = excluded.json, observed_at = excluded.observed_at",
            params![report.provider.as_str(), serde_json::to_string(report)?, report.observed_at],
        )?;
        Ok(())
    }

    pub fn limits(&self) -> Result<Vec<LimitReport>> {
        let c = self.conn();
        let mut stmt = c.prepare_cached("SELECT json FROM limits")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out: Vec<LimitReport> =
            rows.filter_map(|r| r.ok()).filter_map(|s| serde_json::from_str(&s).ok()).collect();
        out.sort_by_key(|r| r.provider);
        Ok(out)
    }

    pub fn delete_limit(&self, provider: Provider) -> Result<()> {
        self.conn().execute("DELETE FROM limits WHERE provider = ?1", [provider.as_str()])?;
        Ok(())
    }

    pub fn kv_get(&self, k: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT v FROM kv WHERE k = ?1", [k], |r| r.get(0))
            .optional()?)
    }

    pub fn kv_set(&self, k: &str, v: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            [k, v],
        )?;
        Ok(())
    }
}
