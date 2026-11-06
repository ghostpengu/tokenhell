//! Codex CLI sessions: `$CODEX_HOME/{sessions,archived_sessions}/**/*.jsonl`.
//!
//! Newer Codex writes `token_usage_record` lines (one per response, keyed by `response_id`).
//! Older versions only write `event_msg/token_count` with a cumulative `total_token_usage`;
//! for those we emit *fallback* deltas, which the store ignores when a session has records.
//!
//! OpenAI semantics: `input_tokens` includes cached input; `output_tokens` includes reasoning.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ingest::{HistorySource, env_or_home, file_stem, has_ext};
use crate::model::{Provider, UsageEvent, j};

pub struct Codex;

pub fn codex_home() -> PathBuf {
    env_or_home("CODEX_HOME", &[".codex"]).remove(0)
}

#[derive(Default, Clone, Copy, PartialEq, Debug)]
struct Tot {
    input: u64,
    cached: u64,
    cache_write: u64,
    output: u64,
    reasoning: u64,
}

impl Tot {
    fn from(v: &Value) -> Tot {
        Tot {
            input: j::u(v, &["input_tokens"]),
            cached: j::u(v, &["cached_input_tokens"]),
            cache_write: j::u(v, &["cache_write_input_tokens"]),
            output: j::u(v, &["output_tokens"]),
            reasoning: j::u(v, &["reasoning_output_tokens"]),
        }
    }
    fn to_json(self) -> Value {
        json!([self.input, self.cached, self.cache_write, self.output, self.reasoning])
    }
    fn from_json(v: &Value) -> Option<Tot> {
        let a = v.as_array()?;
        let g = |i: usize| a.get(i).and_then(Value::as_u64).unwrap_or(0);
        Some(Tot { input: g(0), cached: g(1), cache_write: g(2), output: g(3), reasoning: g(4) })
    }
    /// None if any counter went backwards (counter reset).
    fn minus(self, o: Tot) -> Option<Tot> {
        Some(Tot {
            input: self.input.checked_sub(o.input)?,
            cached: self.cached.checked_sub(o.cached)?,
            cache_write: self.cache_write.checked_sub(o.cache_write)?,
            output: self.output.checked_sub(o.output)?,
            reasoning: self.reasoning.checked_sub(o.reasoning)?,
        })
    }
    fn is_zero(self) -> bool {
        self == Tot::default()
    }
    fn event(self, key: String, session: Option<String>, ts: i64, model: Option<String>, tier: Option<String>, fallback: bool) -> UsageEvent {
        UsageEvent {
            dedup_key: key,
            session,
            ts,
            model,
            input: self.input.saturating_sub(self.cached).saturating_sub(self.cache_write),
            output: self.output,
            cache_read: self.cached,
            cache_write: self.cache_write,
            reasoning: self.reasoning,
            tier,
            fallback,
            ..Default::default()
        }
    }
}

impl HistorySource for Codex {
    fn provider(&self) -> Provider {
        Provider::Codex
    }

    fn roots(&self) -> Vec<PathBuf> {
        let h = codex_home();
        vec![h.join("sessions"), h.join("archived_sessions")]
    }

    fn matches(&self, path: &Path) -> bool {
        has_ext(path, "jsonl")
    }

    fn parser_version(&self) -> u32 {
        2 // v2: service tier
    }

    fn parse_line(&self, path: &Path, state: &mut Value, line: &str, out: &mut Vec<UsageEvent>) {
        if !(line.contains("token_usage_record")
            || line.contains("token_count")
            || line.contains("turn_context")
            || line.contains("session_meta")
            || line.contains("thread_settings_applied"))
        {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        if !state.is_object() {
            *state = json!({});
        }
        let ty = j::s(&v, &["type"]).unwrap_or_default();
        let p = v.get("payload").cloned().unwrap_or(Value::Null);
        let sess: Option<String> =
            state.get("session").and_then(Value::as_str).map(str::to_string).or_else(|| Some(file_stem(path)));
        let mdl: Option<String> = state.get("model").and_then(Value::as_str).map(str::to_string);
        let tier: Option<String> = state.get("tier").and_then(Value::as_str).map(str::to_string);
        let ts = j::ts_ms(v.get("timestamp")).unwrap_or(0);

        match ty {
            "session_meta" => {
                if let Some(id) = j::s(&p, &["id"]).or(j::s(&p, &["session_id"])) {
                    state["session"] = json!(id);
                }
            }
            "turn_context" => {
                if let Some(m) = j::s(&p, &["model"]) {
                    state["model"] = json!(m);
                }
            }
            "token_usage_record" => {
                let Some(usage) = p.get("usage") else { return };
                let t = Tot::from(usage);
                if t.is_zero() {
                    return;
                }
                let key = match j::s(&p, &["response_id"]) {
                    Some(r) => format!("r:{r}"),
                    None => format!(
                        "r:{}:{}:{}",
                        sess.clone().unwrap_or_default(),
                        j::s(&p, &["turn_id"]).unwrap_or_default(),
                        j::u(&v, &["ordinal"])
                    ),
                };
                out.push(t.event(key, sess, ts, mdl, tier, false));
            }
            "event_msg" if j::s(&p, &["type"]) == Some("thread_settings_applied") => {
                if let Some(m) = j::s(&p, &["thread_settings", "model"]) {
                    state["model"] = json!(m);
                }
                // OpenAI tiers: "priority" (Codex "fast"), "flex"; default/auto = standard.
                let t = match j::s(&p, &["thread_settings", "service_tier"]) {
                    Some("priority") | Some("fast") => json!("priority"),
                    Some("flex") => json!("flex"),
                    _ => Value::Null,
                };
                state["tier"] = t;
            }
            "event_msg" if j::s(&p, &["type"]) == Some("token_count") => {
                let Some(total) = j::get(&p, &["info", "total_token_usage"]) else { return };
                let cur = Tot::from(total);
                let prev = state.get("last").and_then(Tot::from_json).unwrap_or_default();
                let delta = cur.minus(prev).unwrap_or(cur);
                state["last"] = cur.to_json();
                if delta.is_zero() {
                    return;
                }
                let key = format!("c:{}:{}", sess.clone().unwrap_or_default(), j::u(total, &["total_tokens"]));
                out.push(delta.event(key, sess, ts, mdl, tier, true));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str]) -> Vec<UsageEvent> {
        let mut st = json!({});
        let mut out = vec![];
        for l in lines {
            Codex.parse_line(Path::new("rollout-x.jsonl"), &mut st, l, &mut out);
        }
        out
    }

    #[test]
    fn records_are_primary_and_normalised() {
        let out = run(&[
            r#"{"timestamp":"2026-11-06T10:00:00Z","type":"session_meta","payload":{"id":"S"}}"#,
            r#"{"timestamp":"2026-11-06T10:00:00Z","type":"turn_context","payload":{"model":"gpt-6.1-terra"}}"#,
            r#"{"timestamp":"2026-11-06T10:00:01Z","ordinal":3,"type":"token_usage_record","payload":{"response_id":"resp_1","usage":{"input_tokens":17542,"cached_input_tokens":13312,"cache_write_input_tokens":0,"output_tokens":170,"reasoning_output_tokens":20,"total_tokens":17712}}}"#,
        ]);
        assert_eq!(out.len(), 1);
        let e = &out[0];
        assert_eq!(e.dedup_key, "r:resp_1");
        assert_eq!(e.input, 17542 - 13312);
        assert_eq!(e.cache_read, 13312);
        assert_eq!(e.output, 170);
        assert_eq!(e.total(), 17712);
        assert_eq!(e.model.as_deref(), Some("gpt-6.1-terra"));
        assert!(!e.fallback);
    }

    #[test]
    fn token_count_deltas_skip_repeats_and_handle_reset() {
        let tc = |i: u64, o: u64| {
            format!(
                r#"{{"timestamp":"2026-11-06T10:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{i},"cached_input_tokens":0,"output_tokens":{o},"reasoning_output_tokens":0,"total_tokens":{}}}}}}}}}"#,
                i + o
            )
        };
        let a = tc(100, 10);
        let b = tc(100, 10); // repeated snapshot
        let c = tc(250, 30);
        let d = tc(50, 5); // counter reset (compaction / new thread)
        let out = run(&[&a, &b, &c, &d]);
        assert_eq!(out.len(), 3);
        assert_eq!((out[0].input, out[0].output), (100, 10));
        assert_eq!((out[1].input, out[1].output), (150, 20));
        assert_eq!((out[2].input, out[2].output), (50, 5));
        assert!(out.iter().all(|e| e.fallback));
    }
}
