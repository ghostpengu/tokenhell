//! Cursor CLI stop-hook turn totals. Raw input includes both cache buckets.
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ingest::HistorySource;
use crate::model::{Provider, UsageEvent, j};

pub struct CursorHook;

impl HistorySource for CursorHook {
    fn provider(&self) -> Provider {
        Provider::Cursor
    }

    fn roots(&self) -> Vec<PathBuf> {
        // Create the directory up front. The watcher only attaches to roots that
        // already exist, and a later rescan is ten minutes apart.
        let Some(home) = dirs::home_dir() else { return vec![] };
        let dir = home.join("Library/Application Support/io.github.ghostpengu.tokenhell/cursor-hook");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("cursor hook dir: {e}");
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
            }
        }
        vec![dir]
    }

    fn matches(&self, path: &Path) -> bool {
        self.roots().iter().any(|root| path == root.join("usage.jsonl"))
    }

    fn parser_version(&self) -> u32 {
        1
    }

    fn parse_line(&self, _path: &Path, _state: &mut Value, line: &str, out: &mut Vec<UsageEvent>) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let Some(conversation) = j::s(&v, &["conversation_id"]).filter(|s| !s.is_empty()) else { return };
        let Some(generation) = j::s(&v, &["generation_id"]).filter(|s| !s.is_empty()) else { return };
        let Some(ts) = v.get("ts").and_then(Value::as_i64) else { return };
        let read = j::u(&v, &["cache_read_tokens"]);
        let write = j::u(&v, &["cache_write_tokens"]);
        let ev = UsageEvent {
            // The hook script writes CLI builds only. IDE stop events are omitted there
            // because dashboard usage-events already store them under different keys.
            // Reconciliation is still needed if the dashboard starts listing these CLI turns.
            dedup_key: format!("hook:{conversation}:{generation}"),
            session: Some(conversation.to_owned()),
            ts,
            model: j::s(&v, &["model"]).map(str::to_owned),
            input: j::u(&v, &["input_tokens"]).saturating_sub(read).saturating_sub(write),
            output: j::u(&v, &["output_tokens"]),
            cache_read: read,
            cache_write: write,
            ..Default::default()
        };
        if ev.total() > 0 {
            out.push(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{FileRow, Store};

    fn parse(line: &str) -> Vec<UsageEvent> {
        let mut out = vec![];
        CursorHook.parse_line(Path::new("usage.jsonl"), &mut Value::Null, line, &mut out);
        out
    }

    #[test]
    fn normalizes_inclusive_input_and_clamps() {
        let events = parse(r#"{"ts":1794000000000,"conversation_id":"c","generation_id":"g","model":"grok-4.7","input_tokens":100,"output_tokens":8,"cache_read_tokens":60,"cache_write_tokens":25}"#);
        let e = &events[0];
        assert_eq!((e.input, e.output, e.cache_read, e.cache_write), (15, 8, 60, 25));
        assert_eq!(e.model.as_deref(), Some("grok-4.7"));
        assert_eq!(e.cost_reported, None);
        let events = parse(r#"{"ts":1794000000000,"conversation_id":"c","generation_id":"g","input_tokens":10,"cache_read_tokens":20,"cache_write_tokens":5}"#);
        assert_eq!(events[0].input, 0);
    }

    #[test]
    fn missing_and_malformed_fields() {
        let events = parse(r#"{"ts":1794000000000,"conversation_id":"c","generation_id":"g","output_tokens":7,"input_tokens":null}"#);
        assert_eq!((events[0].input, events[0].output), (0, 7));
        assert_eq!(events[0].model, None);
        assert!(parse(r#"{"ts":1794000000000,"conversation_id":"c","output_tokens":7}"#).is_empty());
        assert!(parse(r#"{"ts":1794000000000,"conversation_id":"c","generation_id":"g"}"#).is_empty());
        assert!(parse("{broken").is_empty());
    }

    #[test]
    fn duplicate_generations_are_upserted() {
        let store = Store::open_in_memory().unwrap();
        let events = parse(r#"{"ts":1794000000000,"conversation_id":"c","generation_id":"g","input_tokens":10}"#);
        assert_eq!(events[0].dedup_key, "hook:c:g");
        store.commit_file(Provider::Cursor, "usage.jsonl", false, &events, &FileRow::default()).unwrap();
        store.commit_file(Provider::Cursor, "usage.jsonl", false, &events, &FileRow::default()).unwrap();
        let saved = store.events_in(0, i64::MAX).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].input, 10);
    }
}
