//! Grok local history.
//! - xAI Grok Build: `$GROK_HOME/sessions/<ws>/<session>/updates.jsonl`, `turn_completed` updates.
//! - superagent-ai grok-cli: `~/.grok/grok.db` SQLite `usage_events` (read-only).
//!
//! Grok Build semantics: `inputTokens` includes cache read + creation; `outputTokens` includes reasoning.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::ingest::{HistorySource, SourceMode, env_or_home, has_ext};
use crate::model::{Provider, UsageEvent, j};

pub fn grok_home() -> PathBuf {
    env_or_home("GROK_HOME", &[".grok"]).remove(0)
}

/// Grok Build `updates.jsonl` (append-only).
pub struct GrokBuild;

fn bucket(u: &Value) -> (u64, u64, u64, u64, u64) {
    let input = j::u(u, &["inputTokens"]);
    let cr = j::u(u, &["cachedReadTokens"]).max(j::u(u, &["cacheReadTokens"]));
    let cw = j::u(u, &["cacheCreationTokens"]).max(j::u(u, &["cacheWriteTokens"]));
    let out = j::u(u, &["outputTokens"]);
    let reasoning = j::u(u, &["reasoningTokens"]);
    (input.saturating_sub(cr).saturating_sub(cw), out, cr, cw, reasoning)
}

impl HistorySource for GrokBuild {
    fn provider(&self) -> Provider {
        Provider::Grok
    }
    fn roots(&self) -> Vec<PathBuf> {
        vec![grok_home().join("sessions")]
    }
    fn matches(&self, path: &Path) -> bool {
        path.file_name().is_some_and(|n| n == "updates.jsonl")
    }
    fn parser_version(&self) -> u32 {
        1
    }
    fn parse_line(&self, path: &Path, _s: &mut Value, line: &str, out: &mut Vec<UsageEvent>) {
        if !line.contains("turn_completed") {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let Some(up) = j::get(&v, &["params", "update"]) else { return };
        if j::s(up, &["sessionUpdate"]) != Some("turn_completed") {
            return;
        }
        let Some(usage) = up.get("usage") else { return };
        let session = j::s(&v, &["params", "sessionId"])
            .map(str::to_string)
            .or_else(|| path.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_default();
        let turn = j::s(up, &["prompt_id"])
            .or(j::s(up, &["promptId"]))
            .or(j::s(&v, &["params", "_meta", "eventId"]))
            .map(str::to_string);
        let Some(turn) = turn else { return };
        let ts = j::ts_ms(j::get(&v, &["params", "_meta", "agentTimestampMs"]).or(v.get("timestamp"))).unwrap_or(0);
        let cost = j::f(usage, &["costUsdTicks"]).map(|t| t / 1e10);
        let base = format!("grok:{session}:prompt:{turn}");

        let per_model = usage.get("modelUsage").and_then(Value::as_object).filter(|m| !m.is_empty());
        match per_model {
            Some(models) => {
                let n = models.len();
                for (model, u) in models {
                    let (i, o, cr, cw, r) = bucket(u);
                    out.push(UsageEvent {
                        dedup_key: format!("{base}:{model}"),
                        session: Some(session.clone()),
                        ts,
                        model: Some(model.clone()),
                        input: i,
                        output: o,
                        cache_read: cr,
                        cache_write: cw,
                        reasoning: r,
                        // Turn cost can't be split reliably across models; attach it only when single-model.
                        cost_reported: if n == 1 { cost } else { None },
                        fallback: false,
                        ..Default::default()
                    });
                }
            }
            None => {
                let (i, o, cr, cw, r) = bucket(usage);
                out.push(UsageEvent {
                    dedup_key: base,
                    session: Some(session),
                    ts,
                    model: None,
                    input: i,
                    output: o,
                    cache_read: cr,
                    cache_write: cw,
                    reasoning: r,
                    cost_reported: cost,
                    fallback: false,
                    ..Default::default()
                });
            }
        }
        out.retain(|e| e.total() > 0);
    }
}

/// superagent-ai/grok-cli `~/.grok/grok.db`.
pub struct GrokCliDb;

impl HistorySource for GrokCliDb {
    fn provider(&self) -> Provider {
        Provider::Grok
    }
    fn roots(&self) -> Vec<PathBuf> {
        vec![grok_home()]
    }
    fn matches(&self, path: &Path) -> bool {
        path.file_name().is_some_and(|n| n == "grok.db")
    }
    fn watch_alias(&self, path: &Path) -> Option<PathBuf> {
        (path.file_name()? == "grok.db-wal").then(|| path.with_file_name("grok.db"))
    }
    fn mode(&self) -> SourceMode {
        SourceMode::Snapshot
    }
    fn parser_version(&self) -> u32 {
        1
    }
    fn parse_snapshot(&self, path: &Path, out: &mut Vec<UsageEvent>) -> Result<()> {
        if !has_ext(path, "db") {
            return Ok(());
        }
        let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, session_id, model, input_tokens, output_tokens, cost_micros, created_at FROM usage_events",
        ) else {
            return Ok(());
        };
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        for (id, session, model, i, o, cost, created) in rows.flatten() {
            let ev = UsageEvent {
                dedup_key: format!("db:{id}"),
                session,
                ts: j::ts_ms(created.map(Value::String).as_ref()).unwrap_or(0),
                model,
                input: i.unwrap_or(0).max(0) as u64,
                output: o.unwrap_or(0).max(0) as u64,
                cost_reported: cost.filter(|c| *c > 0).map(|c| c as f64 / 1e6),
                ..Default::default()
            };
            if ev.total() > 0 {
                out.push(ev);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_build_turn_per_model() {
        let line = r#"{"method":"session/update","params":{"sessionId":"S","update":{"sessionUpdate":"turn_completed","prompt_id":"P","usage":{"inputTokens":1000,"outputTokens":100,"cachedReadTokens":400,"cacheCreationTokens":0,"reasoningTokens":20,"costUsdTicks":1230000,"modelUsage":{"grok-4.5":{"inputTokens":1000,"outputTokens":100,"cachedReadTokens":400,"reasoningTokens":20}}}},"_meta":{"eventId":"E","agentTimestampMs":1790900000000}}}"#;
        let mut out = vec![];
        GrokBuild.parse_line(Path::new("updates.jsonl"), &mut Value::Null, line, &mut out);
        assert_eq!(out.len(), 1);
        let e = &out[0];
        assert_eq!(e.dedup_key, "grok:S:prompt:P:grok-4.5");
        assert_eq!((e.input, e.cache_read, e.output), (600, 400, 100));
        assert_eq!(e.ts, 1790900000000);
        assert!((e.cost_reported.unwrap() - 0.000123).abs() < 1e-9);
    }
}
