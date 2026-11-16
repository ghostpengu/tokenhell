//! Antigravity CLI (`agy`) and ACP conversation stores, opened read-only.
//!
//! `gen_metadata.data` is a CortexStepGeneratorMetadata protobuf: field 1 is ChatModelMetadata, whose
//! field 4 is ModelUsageStats. Its fields 2/3/4/5 are uncached input, total output,
//! cache write and cache read; field 9 is thinking already included in output.
//! Field 1 is a model enum, NOT an extra input-token count. Verified against the
//! descriptors shipped in agy and its transcript counters on 2026-11-15.
//! Modern builds date generations through packed step indices (field 2) joined
//! to `steps.metadata` field 1; older builds also carry ChatModel field 9.4.
//! This is an internal format; unknown/malformed protobuf records are skipped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::ingest::{HistorySource, SourceMode, env_or_home, file_stem, has_ext};
use crate::model::{Provider, UsageEvent};

pub struct Antigravity;

impl HistorySource for Antigravity {
    fn provider(&self) -> Provider {
        Provider::Antigravity
    }

    fn roots(&self) -> Vec<PathBuf> {
        // TokenHell override: one or more Antigravity data directories, as for other sources.
        env_or_home("ANTIGRAVITY_HOME", &[".gemini/antigravity-cli", ".gemini/antigravity-acp"])
            .into_iter()
            .map(|p| p.join("conversations"))
            .collect()
    }

    fn matches(&self, path: &Path) -> bool {
        has_ext(path, "db") && path.parent().and_then(Path::file_name).is_some_and(|n| n == "conversations")
    }

    fn watch_alias(&self, path: &Path) -> Option<PathBuf> {
        let target = path.with_file_name(path.file_name()?.to_str()?.strip_suffix("-wal")?);
        self.matches(&target).then_some(target)
    }

    fn mode(&self) -> SourceMode {
        SourceMode::Snapshot
    }

    fn parser_version(&self) -> u32 {
        1
    }

    fn parse_snapshot(&self, path: &Path, out: &mut Vec<UsageEvent>) -> Result<()> {
        let mut conn =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        // All tables must come from the same snapshot while agy is adding generations.
        let tx = conn.transaction()?;
        let session = tx
            .query_row("SELECT cascade_id FROM trajectory_meta LIMIT 1", [], |r| r.get::<_, Option<String>>(0))
            .optional()?
            .flatten()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| file_stem(path));
        let created: Option<Vec<u8>> =
            tx.query_row("SELECT data FROM trajectory_metadata_blob LIMIT 1", [], |r| r.get(0)).optional()?.flatten();
        let created_ms = created.as_deref().and_then(Proto::parse).and_then(|p| timestamp(p.bytes(2)?));
        let mut steps = tx.prepare("SELECT idx, metadata FROM steps WHERE metadata IS NOT NULL")?;
        let mut step_times = HashMap::new();
        let mut rows = steps.query([])?;
        while let Some(row) = rows.next()? {
            let idx: u64 = row.get(0)?;
            let data: Vec<u8> = row.get(1)?;
            if let Some(ts) = Proto::parse(&data).and_then(|p| timestamp(p.bytes(1)?)) {
                step_times.insert(idx, ts);
            }
        }

        let mut generations = tx.prepare("SELECT idx, data FROM gen_metadata WHERE data IS NOT NULL ORDER BY idx")?;
        let mut rows = generations.query([])?;
        while let Some(row) = rows.next()? {
            let idx: i64 = row.get(0)?;
            let data: Vec<u8> = row.get(1)?;
            if let Some(ev) = generation(&data, idx, &session, &step_times, created_ms) {
                out.push(ev);
            }
        }
        Ok(())
    }
}

fn generation(
    data: &[u8],
    idx: i64,
    session: &str,
    steps: &HashMap<u64, i64>,
    created_ms: Option<i64>,
) -> Option<UsageEvent> {
    let generation = Proto::parse(data)?;
    let chat = Proto::parse(generation.bytes(1)?)?;
    let usage = Proto::parse(chat.bytes(4)?)?;
    let ts = generation
        .indices(2)?
        .into_iter()
        .find_map(|i| steps.get(&i).copied())
        .or_else(|| Proto::parse(chat.bytes(9)?).and_then(|p| timestamp(p.bytes(4)?)))
        .or(created_ms)?;
    // Generator field 4 is an execution ID shared by many calls, not a response ID.
    let id = usage.text(11).or_else(|| usage.text(7)).unwrap_or_else(|| format!("gen:{idx}"));
    let ev = UsageEvent {
        dedup_key: format!("{session}:{id}"),
        session: Some(session.to_string()),
        ts,
        model: chat.text(19).or_else(|| chat.text(21)),
        input: usage.uint(2),
        output: usage.uint(3),
        cache_write: usage.uint(4),
        cache_read: usage.uint(5),
        reasoning: usage.uint(9),
        ..Default::default()
    };
    // Avoid overflow or wrapping SQLite's signed counters on corrupt records.
    let total = ev.input.checked_add(ev.output)?.checked_add(ev.cache_read)?.checked_add(ev.cache_write)?;
    (total > 0 && total <= i64::MAX as u64 && ev.reasoning <= ev.output).then_some(ev)
}

fn timestamp(data: &[u8]) -> Option<i64> {
    let p = Proto::parse(data)?;
    let seconds = p.uint(1);
    let nanos = p.uint(2);
    // Reject zero/unset dates and values that aren't a protobuf Timestamp.
    if seconds == 0 || seconds > 253_402_300_799 || nanos >= 1_000_000_000 {
        return None;
    }
    Some((seconds * 1000 + nanos / 1_000_000) as i64)
}

/// A bounded wire reader for the few protobuf fields needed here. No generated schema,
/// recursive decoding, or interpretation of prompt/response/tool payloads is needed.
#[derive(Clone, Copy)]
enum Field<'a> {
    Uint(u64),
    Bytes(&'a [u8]),
}

struct Proto<'a>(Vec<(u32, Field<'a>)>);

impl<'a> Proto<'a> {
    fn parse(mut data: &'a [u8]) -> Option<Self> {
        let mut fields = Vec::new();
        while !data.is_empty() {
            let tag = varint(&mut data)?;
            let number = tag >> 3;
            if number == 0 || number > 0x1fff_ffff {
                return None;
            }
            let field = match tag & 7 {
                0 => Some(Field::Uint(varint(&mut data)?)),
                2 => {
                    let len = usize::try_from(varint(&mut data)?).ok()?;
                    let bytes = data.get(..len)?;
                    data = &data[len..];
                    Some(Field::Bytes(bytes))
                }
                1 => {
                    data = data.get(8..)?;
                    None
                }
                5 => {
                    data = data.get(4..)?;
                    None
                }
                _ => return None,
            };
            if let Some(field) = field {
                fields.push((number as u32, field));
            }
        }
        Some(Self(fields))
    }

    fn bytes(&self, number: u32) -> Option<&'a [u8]> {
        self.0.iter().rev().find_map(|(n, f)| match f {
            Field::Bytes(b) if *n == number => Some(*b),
            _ => None,
        })
    }

    fn uint(&self, number: u32) -> u64 {
        self.0
            .iter()
            .rev()
            .find_map(|(n, f)| match f {
                Field::Uint(v) if *n == number => Some(*v),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn text(&self, number: u32) -> Option<String> {
        std::str::from_utf8(self.bytes(number)?).ok().filter(|s| !s.is_empty()).map(str::to_string)
    }

    fn indices(&self, number: u32) -> Option<Vec<u64>> {
        let mut values = Vec::new();
        for (_, field) in self.0.iter().filter(|(n, _)| *n == number) {
            match *field {
                Field::Uint(v) => values.push(v),
                Field::Bytes(mut bytes) => {
                    while !bytes.is_empty() {
                        values.push(varint(&mut bytes)?);
                    }
                }
            }
        }
        Some(values)
    }
}

fn varint(data: &mut &[u8]) -> Option<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let (&byte, rest) = data.split_first()?;
        *data = rest;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ingest::Ingestor,
        pricing::Pricing,
        store::Store,
        summary::{self, Range},
    };
    use std::sync::Arc;

    fn encoded(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        while value >= 128 {
            bytes.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        bytes.push(value as u8);
        bytes
    }

    fn uint(number: u32, value: u64) -> Vec<u8> {
        [encoded(u64::from(number) << 3), encoded(value)].concat()
    }

    fn bytes(number: u32, value: &[u8]) -> Vec<u8> {
        [encoded((u64::from(number) << 3) | 2), encoded(value.len() as u64), value.to_vec()].concat()
    }

    fn stamp(seconds: u64) -> Vec<u8> {
        [uint(1, seconds), uint(2, 123_000_000)].concat()
    }

    fn record(id: &str, indices: &[u64], usage: &[u8], legacy_ts: Option<u64>) -> Vec<u8> {
        let usage = [usage.to_vec(), bytes(11, id.as_bytes())].concat();
        let mut chat = [bytes(4, &usage), bytes(19, b"gemini-4-hydrogen")].concat();
        if let Some(ts) = legacy_ts {
            chat.extend(bytes(9, &bytes(4, &stamp(ts))));
        }
        [
            bytes(1, &chat),
            bytes(2, &indices.iter().flat_map(|i| encoded(*i)).collect::<Vec<_>>()),
            bytes(4, b"execution-shared-by-many-generations"),
        ]
        .concat()
    }

    fn usage(input: u64, output: u64, read: u64, write: u64, thinking: u64) -> Vec<u8> {
        [uint(1, 1525), uint(2, input), uint(3, output), uint(4, write), uint(5, read), uint(9, thinking)].concat()
    }

    fn database(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE trajectory_meta (cascade_id TEXT);
            INSERT INTO trajectory_meta VALUES ('session');
            CREATE TABLE trajectory_metadata_blob (data BLOB);
            CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB);
            CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB);
        ",
        )
        .unwrap();
        conn.execute("INSERT INTO trajectory_metadata_blob VALUES (?1)", [bytes(2, &stamp(1_790_000_000))]).unwrap();
        conn
    }

    #[test]
    fn reads_disjoint_usage_and_dates_each_generation_from_its_exact_steps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("renamed-session.db");
        let conn = database(&path);
        for (idx, seconds) in [(1, 1_794_698_000), (300, 1_794_785_000)] {
            conn.execute("INSERT INTO steps VALUES (?1, ?2)", rusqlite::params![idx, bytes(1, &stamp(seconds))])
                .unwrap();
        }
        let records = [
            record("cold", &[1, 2], &usage(15_251, 311, 0, 0, 84), None),
            // Cache reads already live outside input; thinking already lives inside output.
            record("warm", &[300, 301], &usage(5075, 295, 15_252, 40, 82), None),
            record("legacy", &[], &usage(100, 10, 0, 0, 2), Some(1_794_600_000)),
            record("session-date", &[999], &usage(50, 10, 0, 0, 0), None),
            record("zero", &[1], &usage(0, 0, 0, 0, 0), None),
            record("overflow", &[1], &usage(u64::MAX, 1, 0, 0, 0), None),
            vec![0x0a, 0xff], // Truncated protobuf must not hide valid adjacent rows.
        ];
        for (idx, data) in records.iter().enumerate() {
            conn.execute("INSERT INTO gen_metadata VALUES (?1, ?2)", rusqlite::params![idx as i64, data]).unwrap();
        }
        let mut out = vec![];
        Antigravity.parse_snapshot(&path, &mut out).unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!((out[0].input, out[0].output, out[0].reasoning), (15_251, 311, 84));
        assert_eq!(out[0].total(), 15_562, "model enum 1525 is not a token count");
        assert_eq!(out[0].ts, 1_794_698_000_123);
        assert_eq!((out[1].input, out[1].output, out[1].cache_read, out[1].cache_write), (5075, 295, 15_252, 40));
        assert_eq!(out[1].total(), 20_662);
        assert_eq!(out[1].ts, 1_794_785_000_123, "a multi-day session must use per-generation dates");
        assert_eq!(out[2].ts, 1_794_600_000_123);
        assert_eq!(out[3].ts, 1_790_000_000_123);
        assert_eq!(out[0].dedup_key, "session:cold", "identity survives renamed database copies");
        assert_eq!(out[0].model.as_deref(), Some("gemini-4-hydrogen"));
        assert_eq!(out[0].cost_reported, None);
    }

    struct Rooted(PathBuf);
    impl HistorySource for Rooted {
        fn provider(&self) -> Provider {
            Antigravity.provider()
        }
        fn roots(&self) -> Vec<PathBuf> {
            vec![self.0.clone()]
        }
        fn matches(&self, path: &Path) -> bool {
            Antigravity.matches(path)
        }
        fn watch_alias(&self, path: &Path) -> Option<PathBuf> {
            Antigravity.watch_alias(path)
        }
        fn mode(&self) -> SourceMode {
            Antigravity.mode()
        }
        fn parser_version(&self) -> u32 {
            Antigravity.parser_version()
        }
        fn parse_snapshot(&self, path: &Path, out: &mut Vec<UsageEvent>) -> Result<()> {
            Antigravity.parse_snapshot(path, out)
        }
    }

    #[test]
    fn wal_updates_replace_snapshots_and_copied_sessions_are_counted_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("conversations");
        std::fs::create_dir(&root).unwrap();
        let path = root.join("session.db");
        let writer = database(&path);
        writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
        let now = chrono::Utc::now().timestamp() as u64;
        writer
            .execute(
                "INSERT INTO gen_metadata VALUES (0, ?1)",
                [record("response", &[], &usage(100, 10, 0, 0, 2), Some(now))],
            )
            .unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let ing = Ingestor::new(store.clone(), vec![Box::new(Rooted(root.clone()))]);
        assert_eq!(ing.scan_all(), 1);
        assert_eq!(ing.scan_all(), 0);
        let size = std::fs::metadata(&path).unwrap().len();

        writer
            .execute(
                "UPDATE gen_metadata SET data = ?1 WHERE idx = 0",
                [record("response", &[], &usage(100, 20, 300, 0, 5), Some(now))],
            )
            .unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), size, "main file is untouched in WAL mode");
        assert_eq!(ing.process_path(&path.with_file_name("session.db-wal")), 1);
        let sum = summary::summarize(&store, &Pricing::default(), Range::Today).unwrap();
        assert_eq!(sum.tokens.total, 420, "snapshot updates replace earlier counts");
        assert_eq!(sum.providers[0].provider, Provider::Antigravity);
        assert_eq!(sum.providers[0].label, "Antigravity CLI");

        writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
        drop(writer);
        std::fs::copy(&path, root.join("copy.db")).unwrap();
        ing.scan_all();
        assert_eq!(ing.scan_all(), 0, "opening a closed database must not look like new usage");
        let sum = summary::summarize(&store, &Pricing::default(), Range::Today).unwrap();
        assert_eq!(sum.tokens.total, 420, "copied stores must not double count");
        assert!(!Antigravity.matches(&dir.path().join("conversation_summaries.db")));
        assert!(Antigravity.watch_alias(&root.join("session.db-shm")).is_none());
    }

    #[test]
    fn wire_reader_rejects_truncation_overflow_and_invalid_timestamps() {
        for data in [vec![0], vec![0x0b], vec![0x0a, 5, 1], vec![0x08, 0xff], [vec![0x08], vec![0xff; 10]].concat()] {
            assert!(Proto::parse(&data).is_none());
        }
        // Unknown fixed32/fixed64 fields are safely skipped.
        let data = [vec![0x7d, 0, 0, 0, 0, 0x81, 1, 0, 0, 0, 0, 0, 0, 0, 0], uint(2, 42)].concat();
        assert_eq!(Proto::parse(&data).unwrap().uint(2), 42);
        let packed = [uint(2, 1), bytes(2, &encoded(300))].concat();
        assert_eq!(Proto::parse(&packed).unwrap().indices(2).unwrap(), vec![1, 300]);
        assert!(timestamp(&[]).is_none());
        assert!(timestamp(&[uint(1, 1_790_000_000), uint(2, 1_000_000_000)].concat()).is_none());
        assert!(
            generation(&record("undated", &[], &usage(10, 1, 0, 0, 0), None), 0, "S", &HashMap::new(), None).is_none()
        );
    }
}
