use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Claude,
    Codex,
    Cursor,
    Grok,
    Gemini,
    Copilot,
    OpenCode,
    Pi,
    Qwen,
    Kimi,
}

impl Provider {
    pub const ALL: [Provider; 10] = [
        Provider::Claude,
        Provider::Codex,
        Provider::Cursor,
        Provider::Grok,
        Provider::Gemini,
        Provider::Copilot,
        Provider::OpenCode,
        Provider::Pi,
        Provider::Qwen,
        Provider::Kimi,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Cursor => "cursor",
            Provider::Grok => "grok",
            Provider::Gemini => "gemini",
            Provider::Copilot => "copilot",
            Provider::OpenCode => "opencode",
            Provider::Pi => "pi",
            Provider::Qwen => "qwen",
            Provider::Kimi => "kimi",
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Codex => "Codex",
            Provider::Cursor => "Cursor",
            Provider::Grok => "Grok",
            Provider::Gemini => "Gemini CLI",
            Provider::Copilot => "Copilot",
            Provider::OpenCode => "OpenCode",
            Provider::Pi => "Pi",
            Provider::Qwen => "Qwen Code",
            Provider::Kimi => "Kimi",
        }
    }
}

/// One billable model response, normalised so the four token buckets are disjoint:
/// `total = input + output + cache_read + cache_write`.
/// `reasoning` is informational and already included in `output`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageEvent {
    pub dedup_key: String,
    pub session: Option<String>,
    /// Unix milliseconds.
    pub ts: i64,
    pub model: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// Part of `cache_write` written with a 1-hour TTL (priced higher than 5-minute writes).
    pub cache_write_1h: u64,
    pub reasoning: u64,
    /// Server-side web searches billed per request (Claude: $10 / 1k).
    pub web_search: u32,
    /// Pricing modifiers, comma separated: "fast" (Claude fast mode), "us" (US-only inference),
    /// "priority" / "flex" (OpenAI service tiers).
    pub tier: Option<String>,
    /// Cost reported by the tool itself, if any (preferred over our estimate).
    pub cost_reported: Option<f64>,
    /// Fallback events are ignored for a session that also has primary events
    /// (e.g. Codex cumulative-delta events vs per-response records).
    pub fallback: bool,
}

impl UsageEvent {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LimitWindow {
    /// Stable id, e.g. "5h", "week", "week_opus", "monthly", "premium".
    pub key: String,
    pub label: String,
    /// 0..=100, None when the provider doesn't report a percentage.
    pub used_pct: Option<f64>,
    /// Unix seconds.
    pub resets_at: Option<i64>,
    /// Free text like "412 / 500 requests" or "$12.40 of $20".
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResetCredits {
    pub available: u64,
    /// Earliest expiry (Unix seconds) among available credits, when the backend lists them.
    pub next_expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LimitReport {
    pub provider: Provider,
    pub windows: Vec<LimitWindow>,
    pub plan: Option<String>,
    /// Extra one-liners ("$12 credits").
    pub notes: Vec<String>,
    /// Redeemable "full reset" credits (Codex). `#[serde(default)]` keeps older stored rows readable.
    #[serde(default)]
    pub reset_credits: Option<ResetCredits>,
    /// Unix seconds when this data was observed at the source.
    pub observed_at: i64,
    /// True when the data came from a cached/local snapshot rather than a live query.
    pub stale: bool,
    pub error: Option<String>,
}

impl LimitReport {
    pub fn new(provider: Provider) -> Self {
        LimitReport {
            provider,
            windows: vec![],
            plan: None,
            notes: vec![],
            reset_credits: None,
            observed_at: chrono::Utc::now().timestamp(),
            stale: false,
            error: None,
        }
    }

    pub fn error(provider: Provider, msg: impl Into<String>) -> Self {
        let mut r = Self::new(provider);
        r.error = Some(msg.into());
        r
    }

    pub fn max_used(&self) -> Option<f64> {
        self.windows
            .iter()
            .filter_map(|w| w.used_pct)
            .fold(None, |acc, v| Some(acc.map_or(v, |a: f64| a.max(v))))
    }
}

/// Helpers for pulling numbers out of loosely-typed JSON.
pub(crate) mod j {
    use serde_json::Value;

    pub fn u(v: &Value, path: &[&str]) -> u64 {
        get(v, path).and_then(as_u64).unwrap_or(0)
    }

    pub fn s<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
        get(v, path).and_then(Value::as_str)
    }

    pub fn f(v: &Value, path: &[&str]) -> Option<f64> {
        get(v, path).and_then(|x| x.as_f64().or_else(|| x.as_str()?.parse().ok()))
    }

    pub fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
        let mut cur = v;
        for k in path {
            cur = cur.get(*k)?;
        }
        if cur.is_null() { None } else { Some(cur) }
    }

    fn as_u64(v: &Value) -> Option<u64> {
        v.as_u64()
            .or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
            .or_else(|| v.as_str()?.parse().ok())
    }

    /// RFC3339 / ISO string or unix seconds / millis → unix millis.
    pub fn ts_ms(v: Option<&Value>) -> Option<i64> {
        let v = v?;
        if let Some(s) = v.as_str() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                return Some(dt.timestamp_millis());
            }
            return s.parse::<f64>().ok().map(num_to_ms);
        }
        v.as_f64().map(num_to_ms)
    }

    /// Same as ts_ms but returns unix seconds.
    pub fn ts_s(v: Option<&Value>) -> Option<i64> {
        ts_ms(v).map(|m| m / 1000)
    }

    fn num_to_ms(n: f64) -> i64 {
        // < 1e11 → seconds, otherwise millis.
        if n < 1e11 { (n * 1000.0) as i64 } else { n as i64 }
    }
}
