//! Adapters for tools not installed on the dev machine. Formats come from the tools' own
//! source / tokscale (see research/FINDINGS.md). Parsing is tolerant: unknown shapes are skipped.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::ingest::{HistorySource, SourceMode, env_or_home, file_stem, has_ext};
use crate::model::{Provider, UsageEvent, j};

// ---------------------------------------------------------------- Gemini CLI
/// `~/.gemini/tmp/<project_hash>/chats/*.json` — whole-session snapshots.
/// Gemini: prompt `input` includes `cached`; `thoughts` are billed as output.
pub struct Gemini;

impl HistorySource for Gemini {
    fn provider(&self) -> Provider {
        Provider::Gemini
    }
    fn roots(&self) -> Vec<PathBuf> {
        env_or_home("GEMINI_CLI_HOME", &[""]).into_iter().map(|h| h.join(".gemini/tmp")).collect()
    }
    fn matches(&self, path: &Path) -> bool {
        (has_ext(path, "json") || has_ext(path, "jsonl"))
            && path.parent().and_then(Path::file_name).is_some_and(|n| n == "chats")
    }
    fn mode(&self) -> SourceMode {
        SourceMode::Snapshot
    }
    fn parser_version(&self) -> u32 {
        1
    }
    fn parse_snapshot(&self, path: &Path, out: &mut Vec<UsageEvent>) -> Result<()> {
        let text = std::fs::read_to_string(path)?;
        let mut msgs: Vec<Value> = vec![];
        let mut session = file_stem(path);
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(s) = j::s(&v, &["sessionId"]) {
                session = s.to_string();
            }
            msgs = v.get("messages").and_then(Value::as_array).cloned().unwrap_or_default();
        } else {
            // JSONL variant: one message per line.
            msgs.extend(text.lines().filter_map(|l| serde_json::from_str(l).ok()));
        }
        for (i, m) in msgs.iter().enumerate() {
            let Some(t) = m.get("tokens") else { continue };
            let input = j::u(t, &["input"]);
            let cached = j::u(t, &["cached"]);
            let thoughts = j::u(t, &["thoughts"]);
            let ev = UsageEvent {
                dedup_key: format!("{session}:{}", j::s(m, &["id"]).map(str::to_string).unwrap_or(i.to_string())),
                session: Some(session.clone()),
                ts: j::ts_ms(m.get("timestamp")).unwrap_or(0),
                model: j::s(m, &["model"]).map(str::to_string),
                input: input.saturating_sub(cached) + j::u(t, &["tool"]),
                output: j::u(t, &["output"]) + thoughts,
                cache_read: cached,
                cache_write: 0,
                reasoning: thoughts,
                cost_reported: None,
                fallback: false,
                ..Default::default()
            };
            if ev.total() > 0 {
                out.push(ev);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Pi
/// `~/.pi/agent/sessions/<cwd>/*.jsonl`, assistant `message.usage {input, output, cacheRead, cacheWrite, cost}`.
pub struct Pi;

impl HistorySource for Pi {
    fn provider(&self) -> Provider {
        Provider::Pi
    }
    fn roots(&self) -> Vec<PathBuf> {
        env_or_home("PI_HOME", &[".pi/agent"]).into_iter().map(|h| h.join("sessions")).collect()
    }
    fn matches(&self, path: &Path) -> bool {
        has_ext(path, "jsonl")
    }
    fn parser_version(&self) -> u32 {
        1
    }
    fn parse_line(&self, path: &Path, _s: &mut Value, line: &str, out: &mut Vec<UsageEvent>) {
        if !line.contains("\"usage\"") {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let msg = v.get("message").unwrap_or(&v);
        let Some(u) = msg.get("usage") else { return };
        let session = file_stem(path);
        let id = j::s(&v, &["id"]).or(j::s(msg, &["id"])).map(str::to_string);
        let Some(id) = id else { return };
        let ev = UsageEvent {
            dedup_key: format!("{session}:{id}"),
            session: Some(session),
            ts: j::ts_ms(v.get("timestamp").or(msg.get("timestamp"))).unwrap_or(0),
            model: j::s(msg, &["model"]).map(str::to_string),
            input: j::u(u, &["input"]),
            output: j::u(u, &["output"]),
            cache_read: j::u(u, &["cacheRead"]),
            cache_write: j::u(u, &["cacheWrite"]),
            reasoning: 0,
            cost_reported: j::f(u, &["cost", "total"]),
            fallback: false,
            ..Default::default()
        };
        if ev.total() > 0 {
            out.push(ev);
        }
    }
}

// ---------------------------------------------------------------- Qwen Code
/// `~/.qwen/projects/<p>/chats/*.jsonl`, Gemini-style `usageMetadata`.
pub struct Qwen;

impl HistorySource for Qwen {
    fn provider(&self) -> Provider {
        Provider::Qwen
    }
    fn roots(&self) -> Vec<PathBuf> {
        env_or_home("QWEN_HOME", &[".qwen"]).into_iter().map(|h| h.join("projects")).collect()
    }
    fn matches(&self, path: &Path) -> bool {
        has_ext(path, "jsonl")
    }
    fn parser_version(&self) -> u32 {
        1
    }
    fn parse_line(&self, path: &Path, _s: &mut Value, line: &str, out: &mut Vec<UsageEvent>) {
        if !line.contains("usageMetadata") {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let Some(u) = v.get("usageMetadata") else { return };
        let session = j::s(&v, &["sessionId"]).map(str::to_string).unwrap_or_else(|| file_stem(path));
        let ts = j::ts_ms(v.get("timestamp")).unwrap_or(0);
        let id = j::s(&v, &["uuid"]).or(j::s(&v, &["id"])).map(str::to_string).unwrap_or(ts.to_string());
        let prompt = j::u(u, &["promptTokenCount"]);
        let cached = j::u(u, &["cachedContentTokenCount"]);
        let thoughts = j::u(u, &["thoughtsTokenCount"]);
        let ev = UsageEvent {
            dedup_key: format!("{session}:{id}"),
            session: Some(session),
            ts,
            model: j::s(&v, &["model"]).map(str::to_string),
            input: prompt.saturating_sub(cached),
            output: j::u(u, &["candidatesTokenCount"]) + thoughts,
            cache_read: cached,
            cache_write: 0,
            reasoning: thoughts,
            cost_reported: None,
            fallback: false,
            ..Default::default()
        };
        if ev.total() > 0 {
            out.push(ev);
        }
    }
}

// ---------------------------------------------------------------- Kimi CLI / Kimi Code
/// `~/.kimi/sessions/**/wire.jsonl` (StatusUpdate.token_usage) and
/// `~/.kimi-code/sessions/**/wire.jsonl` (usage.record).
pub struct Kimi;

impl HistorySource for Kimi {
    fn provider(&self) -> Provider {
        Provider::Kimi
    }
    fn roots(&self) -> Vec<PathBuf> {
        let home = dirs::home_dir().unwrap_or_default();
        vec![home.join(".kimi/sessions"), home.join(".kimi-code/sessions")]
    }
    fn matches(&self, path: &Path) -> bool {
        path.file_name().is_some_and(|n| n == "wire.jsonl")
    }
    fn parser_version(&self) -> u32 {
        1
    }
    fn parse_line(&self, path: &Path, _s: &mut Value, line: &str, out: &mut Vec<UsageEvent>) {
        if !(line.contains("token_usage") || line.contains("usage.record")) {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let session = path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let ts = j::ts_ms(v.get("timestamp").or(v.get("time"))).unwrap_or(0);
        let ev = if let Some(u) = j::get(&v, &["message", "payload", "token_usage"]) {
            let id = j::s(&v, &["message", "payload", "message_id"])
                .or(j::s(&v, &["message", "message_id"]))
                .map(str::to_string)
                .unwrap_or(ts.to_string());
            UsageEvent {
                dedup_key: format!("{session}:{id}"),
                session: Some(session),
                ts,
                model: None,
                input: j::u(u, &["input_other"]),
                output: j::u(u, &["output"]),
                cache_read: j::u(u, &["input_cache_read"]),
                cache_write: j::u(u, &["input_cache_creation"]),
                ..Default::default()
            }
        } else if j::s(&v, &["type"]) == Some("usage.record") {
            let Some(u) = v.get("usage") else { return };
            if j::s(&v, &["usageScope"]).is_some_and(|s| s != "turn" && s != "request") {
                return; // session-scope totals would double count
            }
            let id = j::s(&v, &["id"]).map(str::to_string).unwrap_or(ts.to_string());
            UsageEvent {
                dedup_key: format!("{session}:{id}"),
                session: Some(session),
                ts,
                model: j::s(&v, &["model"]).map(str::to_string),
                input: j::u(u, &["inputOther"]),
                output: j::u(u, &["output"]),
                cache_read: j::u(u, &["inputCacheRead"]),
                cache_write: j::u(u, &["inputCacheCreation"]),
                ..Default::default()
            }
        } else {
            return;
        };
        if ev.total() > 0 {
            out.push(ev);
        }
    }
}

// ---------------------------------------------------------------- OpenCode
/// `~/.local/share/opencode/opencode*.db` (SQLite, read-only) and legacy
/// `storage/message/<session>/*.json` files. Assistant messages carry `tokens` and `cost`.
pub struct OpenCode;

fn opencode_msg(v: &Value, session_hint: &str) -> Option<UsageEvent> {
    let t = v.get("tokens")?;
    let id = j::s(v, &["id"])?;
    let session = j::s(v, &["sessionID"]).unwrap_or(session_hint).to_string();
    let ev = UsageEvent {
        dedup_key: id.to_string(),
        session: Some(session),
        ts: j::ts_ms(j::get(v, &["time", "completed"]).or(j::get(v, &["time", "created"]))).unwrap_or(0),
        model: j::s(v, &["modelID"]).map(str::to_string),
        input: j::u(t, &["input"]),
        output: j::u(t, &["output"]) + j::u(t, &["reasoning"]),
        cache_read: j::u(t, &["cache", "read"]),
        cache_write: j::u(t, &["cache", "write"]),
        reasoning: j::u(t, &["reasoning"]),
        cost_reported: j::f(v, &["cost"]).filter(|c| *c > 0.0),
        fallback: false,
        ..Default::default()
    };
    (ev.total() > 0).then_some(ev)
}

impl HistorySource for OpenCode {
    fn provider(&self) -> Provider {
        Provider::OpenCode
    }
    fn roots(&self) -> Vec<PathBuf> {
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".local/share"));
        vec![base.join("opencode")]
    }
    fn matches(&self, path: &Path) -> bool {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        (name.starts_with("opencode") && name.ends_with(".db"))
            || (has_ext(path, "json") && path.to_string_lossy().contains("/storage/message/"))
    }
    fn watch_alias(&self, path: &Path) -> Option<PathBuf> {
        let s = path.to_string_lossy();
        s.strip_suffix("-wal").filter(|b| b.ends_with(".db")).map(PathBuf::from)
    }
    fn mode(&self) -> SourceMode {
        SourceMode::Snapshot
    }
    fn parser_version(&self) -> u32 {
        1
    }
    fn parse_snapshot(&self, path: &Path, out: &mut Vec<UsageEvent>) -> Result<()> {
        if has_ext(path, "json") {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
            let hint = path.parent().map(file_stem).unwrap_or_default();
            out.extend(opencode_msg(&v, &hint));
            return Ok(());
        }
        let conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // Schema: message(id, session_id, data JSON). Tolerate absence.
        let Ok(mut stmt) = conn.prepare("SELECT id, session_id, data FROM message") else { return Ok(()) };
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?))
        })?;
        for (id, session, data) in rows.flatten() {
            let Ok(mut v) = serde_json::from_str::<Value>(&data) else { continue };
            if v.get("id").is_none() {
                v["id"] = Value::String(id);
            }
            out.extend(opencode_msg(&v, session.as_deref().unwrap_or("")));
        }
        Ok(())
    }
}
