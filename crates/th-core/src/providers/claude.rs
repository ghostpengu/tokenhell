//! Claude Code transcripts: `~/.claude/projects/**/*.jsonl`.
//! One assistant response can be streamed as several lines; dedup on (requestId, message.id), last wins.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ingest::{HistorySource, env_or_home, has_ext};
use crate::model::{Provider, UsageEvent, j};

pub struct Claude;

impl HistorySource for Claude {
    fn provider(&self) -> Provider {
        Provider::Claude
    }

    fn roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = env_or_home("CLAUDE_CONFIG_DIR", &[".claude", ".config/claude"])
            .into_iter()
            .map(|d| d.join("projects"))
            .collect();
        roots.dedup();
        roots
    }

    fn matches(&self, path: &Path) -> bool {
        has_ext(path, "jsonl")
    }

    fn parser_version(&self) -> u32 {
        2 // v2: 1h cache-write split, web searches, fast / US tiers
    }

    fn parse_line(&self, _path: &Path, _state: &mut Value, line: &str, out: &mut Vec<UsageEvent>) {
        // Cheap pre-filter before full JSON parse.
        if !line.contains("\"usage\"") {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        if j::s(&v, &["type"]) != Some("assistant") {
            return;
        }
        let Some(usage) = j::get(&v, &["message", "usage"]) else { return };
        let model = j::s(&v, &["message", "model"]);
        if model == Some("<synthetic>") {
            return;
        }
        let key = match (j::s(&v, &["requestId"]), j::s(&v, &["message", "id"])) {
            (Some(r), Some(m)) => format!("{r}:{m}"),
            (None, Some(m)) => m.to_string(),
            (Some(r), None) => r.to_string(),
            (None, None) => match j::s(&v, &["uuid"]) {
                Some(u) => u.to_string(),
                None => return,
            },
        };
        let Some(ts) = j::ts_ms(v.get("timestamp")) else { return };
        let ev = UsageEvent {
            dedup_key: key,
            session: j::s(&v, &["sessionId"]).map(str::to_string),
            ts,
            model: model.map(str::to_string),
            input: j::u(usage, &["input_tokens"]),
            output: j::u(usage, &["output_tokens"]),
            cache_read: j::u(usage, &["cache_read_input_tokens"]),
            cache_write: j::u(usage, &["cache_creation_input_tokens"]),
            cache_write_1h: j::u(usage, &["cache_creation", "ephemeral_1h_input_tokens"]),
            reasoning: 0,
            web_search: j::u(usage, &["server_tool_use", "web_search_requests"]) as u32,
            tier: claude_tier(usage),
            cost_reported: j::f(&v, &["costUSD"]),
            fallback: false,
        };
        if ev.total() > 0 {
            out.push(ev);
        }
    }
}

/// Pricing modifiers recorded on the usage object.
fn claude_tier(usage: &Value) -> Option<String> {
    let mut t = vec![];
    if j::s(usage, &["speed"]) == Some("fast") {
        t.push("fast");
    }
    if j::s(usage, &["inference_geo"]) == Some("us") {
        t.push("us");
    }
    if j::s(usage, &["service_tier"]) == Some("priority") {
        t.push("priority");
    }
    (!t.is_empty()).then(|| t.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_keys_streamed_lines() {
        let line = r#"{"type":"assistant","timestamp":"2026-11-06T10:00:00.000Z","requestId":"req_1","sessionId":"s1",
            "message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":10,"output_tokens":20,
            "cache_read_input_tokens":300,"cache_creation_input_tokens":40,
            "cache_creation":{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":30},
            "server_tool_use":{"web_search_requests":2},"speed":"fast","inference_geo":"global"}}}"#
            .replace('\n', "");
        let mut out = vec![];
        Claude.parse_line(Path::new("x.jsonl"), &mut Value::Null, &line, &mut out);
        assert_eq!(out.len(), 1);
        let e = &out[0];
        assert_eq!(e.dedup_key, "req_1:msg_1");
        assert_eq!((e.input, e.output, e.cache_read, e.cache_write), (10, 20, 300, 40));
        assert_eq!(e.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!((e.cache_write_1h, e.web_search), (30, 2));
        assert_eq!(e.tier.as_deref(), Some("fast"));
    }

    #[test]
    fn skips_user_and_synthetic() {
        let mut out = vec![];
        Claude.parse_line(Path::new("x"), &mut Value::Null, r#"{"type":"user","message":{"usage":{}}}"#, &mut out);
        Claude.parse_line(
            Path::new("x"),
            &mut Value::Null,
            r#"{"type":"assistant","timestamp":"2026-11-06T10:00:00Z","message":{"id":"a","model":"<synthetic>","usage":{"output_tokens":5}}}"#,
            &mut out,
        );
        assert!(out.is_empty());
    }
}
