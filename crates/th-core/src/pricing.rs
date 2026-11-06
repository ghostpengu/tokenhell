//! Per-request API-equivalent cost from LiteLLM's model price map (cached on disk, refreshed weekly).
//!
//! Every event is priced on its own, because tiers depend on the individual request:
//! - context-length tiers (`*_above_100k_tokens` on Haiku 5.5, `*_above_272k_tokens` on GPT-6.x):
//!   the whole request is billed at the higher rate when its prompt (input + cache) exceeds the threshold
//! - 5-minute vs 1-hour cache writes (`cache_creation_input_token_cost[_above_1hr]`)
//! - service tiers (`*_priority`, `*_flex`)
//! - Claude fast mode (2x on all token categories) and US-only inference (1.1x on all categories)
//! - Claude web search: $10 per 1,000 searches
//!
//! Rules verified against https://platform.claude.com/docs/en/about-claude/pricing (2026-11-06).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};
use serde_json::Value;

use crate::store::EventRow;

pub const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// Claude fast mode is priced at 2x standard (Opus 5.5 $8/$40 vs $4/$20; Opus 5 / 4.8 $10/$50 vs $5/$25),
/// with prompt-caching multipliers applied on top, i.e. every token category doubles.
const CLAUDE_FAST_MULTIPLIER: f64 = 2.0;
/// `inference_geo: "us"` on Claude 4.6+: 1.1x on all token categories.
const CLAUDE_US_MULTIPLIER: f64 = 1.1;
const CLAUDE_WEB_SEARCH_USD: f64 = 10.0 / 1000.0;

/// Numeric `*cost*` fields of one LiteLLM entry, by field name.
#[derive(Debug, Default)]
struct Entry {
    fields: HashMap<String, f64>,
    /// Context thresholds in tokens found in field names (`_above_<N>k_tokens`), ascending.
    thresholds: Vec<u64>,
}

impl Entry {
    fn from_json(e: &Value) -> Option<Entry> {
        let obj = e.as_object()?;
        obj.get("input_cost_per_token")?.as_f64()?;
        let mut fields = HashMap::new();
        let mut thresholds = vec![];
        for (k, v) in obj {
            if !k.contains("cost") {
                continue;
            }
            let Some(n) = v.as_f64() else { continue };
            if let Some(t) = parse_threshold(k) {
                if !thresholds.contains(&t) {
                    thresholds.push(t);
                }
            }
            fields.insert(k.clone(), n);
        }
        thresholds.sort_unstable();
        Some(Entry { fields, thresholds })
    }

    /// Highest threshold the prompt exceeds, as a field-name suffix.
    fn threshold_suffix(&self, prompt_tokens: u64) -> Option<String> {
        self.thresholds
            .iter()
            .rev()
            .find(|t| prompt_tokens > **t)
            .map(|t| format!("_above_{}k_tokens", t / 1000))
    }

    /// Most specific rate available: base+threshold+tier → base+threshold → base+tier → base.
    fn rate(&self, base: &str, threshold: Option<&str>, tier: Option<&str>) -> Option<f64> {
        let th = threshold.unwrap_or("");
        let tr = tier.unwrap_or("");
        let mut cands = vec![];
        if !th.is_empty() && !tr.is_empty() {
            cands.push(format!("{base}{th}{tr}"));
        }
        if !th.is_empty() {
            cands.push(format!("{base}{th}"));
        }
        if !tr.is_empty() {
            cands.push(format!("{base}{tr}"));
        }
        cands.push(base.to_string());
        cands.iter().find_map(|k| self.fields.get(k).copied())
    }
}

/// `..._above_272k_tokens...` → 272_000. Ignores `_above_1hr` / `_above_12hr` (cache TTLs).
fn parse_threshold(field: &str) -> Option<u64> {
    field.match_indices("_above_").find_map(|(i, _)| {
        let rest = &field[i + 7..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() || !rest[digits.len()..].starts_with("k_tokens") {
            return None;
        }
        digits.parse::<u64>().ok().map(|k| k * 1000)
    })
}

#[derive(Default)]
pub struct Pricing {
    map: HashMap<String, Arc<Entry>>,
    /// Unix seconds of the cached file.
    pub updated: Option<i64>,
}

fn file(dir: &Path) -> PathBuf {
    dir.join("prices.json")
}

impl Pricing {
    pub fn from_litellm(text: &str) -> Result<Pricing> {
        let v: Value = serde_json::from_str(text)?;
        let Some(obj) = v.as_object() else { bail!("price map is not an object") };
        let mut map: HashMap<String, Arc<Entry>> = HashMap::new();
        let mut parsed: Vec<(&String, Arc<Entry>)> = vec![];
        for (k, e) in obj {
            if let Some(entry) = Entry::from_json(e) {
                parsed.push((k, Arc::new(entry)));
            }
        }
        // Exact keys first; then bare names from "provider/model" keys only when not already present.
        for (k, e) in &parsed {
            map.insert(k.to_lowercase(), e.clone());
        }
        for (k, e) in &parsed {
            if let Some((_, bare)) = k.rsplit_once('/') {
                map.entry(bare.to_lowercase()).or_insert_with(|| e.clone());
            }
        }
        if map.len() < 50 {
            bail!("price map looks truncated ({} entries)", map.len());
        }
        Ok(Pricing { map, updated: None })
    }

    pub fn load(dir: &Path) -> Pricing {
        let f = file(dir);
        let Ok(text) = std::fs::read_to_string(&f) else { return Pricing::default() };
        let mut p = Pricing::from_litellm(&text).unwrap_or_default();
        p.updated = std::fs::metadata(&f)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        p
    }

    pub fn is_stale(&self) -> bool {
        self.updated.is_none_or(|t| chrono::Utc::now().timestamp() - t > 7 * 86_400)
    }

    pub async fn refresh(http: &reqwest::Client, dir: &Path) -> Result<Pricing> {
        let text = http.get(LITELLM_URL).send().await?.error_for_status()?.text().await?;
        let mut p = Pricing::from_litellm(&text)?;
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("prices.json.tmp");
        std::fs::write(&tmp, &text)?;
        std::fs::rename(tmp, file(dir))?;
        p.updated = Some(chrono::Utc::now().timestamp());
        Ok(p)
    }

    fn entry(&self, model: &str) -> Option<&Entry> {
        let mut m = model.to_lowercase();
        if let Some(i) = m.find('[') {
            m.truncate(i);
        }
        let m = m.rsplit('/').next().unwrap_or(&m).trim().to_string();
        if let Some(e) = self.map.get(&m) {
            return Some(e);
        }
        // Strip a trailing date suffix like -20251001, then progressively truncate,
        // keeping at least two segments: gpt-6.1-terra-high → gpt-6.1-terra → gpt-6.1
        let parts: Vec<&str> = m.split('-').collect();
        let mut end = parts.len();
        while end > 0 && parts[end - 1].len() >= 4 && parts[end - 1].chars().all(|c| c.is_ascii_digit()) {
            end -= 1;
        }
        while end >= 2 {
            if let Some(e) = self.map.get(&parts[..end].join("-")) {
                return Some(e);
            }
            end -= 1;
        }
        None
    }

    pub fn knows(&self, model: &str) -> bool {
        self.entry(model).is_some()
    }

    /// API-equivalent USD for one request; None when the model has no known price.
    pub fn estimate(&self, e: &EventRow) -> Option<f64> {
        let model = e.model.as_deref()?;
        let entry = self.entry(model)?;
        let is_claude = e.provider == "claude" || model.to_lowercase().contains("claude");
        // Context tiers are per request; an aggregated delta can't be judged, so price it at base rates.
        let th = if e.fallback { None } else { entry.threshold_suffix(e.prompt_tokens()) };
        let th = th.as_deref();
        let has_fast_rates = entry.fields.contains_key("input_cost_per_token_fast");
        let tier = if e.has_tier("priority") {
            Some("_priority")
        } else if e.has_tier("flex") {
            Some("_flex")
        } else if e.has_tier("fast") && has_fast_rates {
            Some("_fast")
        } else {
            None
        };
        let r = |base: &str| entry.rate(base, th, tier);

        let input = r("input_cost_per_token")?;
        let output = r("output_cost_per_token").unwrap_or(0.0);
        let cache_read = r("cache_read_input_token_cost").unwrap_or(input);
        let write_5m = r("cache_creation_input_token_cost").unwrap_or(input * 1.25);
        let write_1h = r("cache_creation_input_token_cost_above_1hr").unwrap_or(write_5m);
        let long = e.cache_write_1h.min(e.cache_write);
        let short = e.cache_write - long;

        let mut usd = e.input as f64 * input
            + e.output as f64 * output
            + e.cache_read as f64 * cache_read
            + short as f64 * write_5m
            + long as f64 * write_1h;
        if is_claude {
            if e.has_tier("fast") && !has_fast_rates {
                usd *= CLAUDE_FAST_MULTIPLIER;
            }
            if e.has_tier("us") {
                usd *= CLAUDE_US_MULTIPLIER;
            }
            usd += e.web_search as f64 * CLAUDE_WEB_SEARCH_USD;
        }
        Some(usd)
    }

    /// Tool-reported cost when present (e.g. Cursor), else our per-request estimate.
    pub fn cost(&self, e: &EventRow) -> Option<f64> {
        e.cost_reported.or_else(|| self.estimate(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pricing() -> Pricing {
        let mut m = serde_json::Map::new();
        for i in 0..60 {
            m.insert(format!("filler-{i}"), json!({"input_cost_per_token": 1e-6}));
        }
        // Values per platform.claude.com pricing page.
        m.insert("claude-opus-5-5".into(), json!({
            "input_cost_per_token": 4e-6, "output_cost_per_token": 2e-5,
            "cache_read_input_token_cost": 2e-7, "cache_creation_input_token_cost": 5e-6,
            "cache_creation_input_token_cost_above_1hr": 8e-6}));
        m.insert("claude-haiku-5-5".into(), json!({
            "input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7,
            "cache_read_input_token_cost": 1e-8, "cache_creation_input_token_cost": 1.25e-7,
            "cache_creation_input_token_cost_above_1hr": 2e-7,
            "input_cost_per_token_above_100k_tokens": 5e-7, "output_cost_per_token_above_100k_tokens": 2.5e-6,
            "cache_read_input_token_cost_above_100k_tokens": 5e-8,
            "cache_creation_input_token_cost_above_100k_tokens": 6.25e-7,
            "cache_creation_input_token_cost_above_1hr_above_100k_tokens": 1e-6}));
        m.insert("gpt-6.1-terra".into(), json!({
            "input_cost_per_token": 1e-6, "output_cost_per_token": 8e-6, "cache_read_input_token_cost": 1e-7,
            "input_cost_per_token_above_272k_tokens": 2e-6, "output_cost_per_token_above_272k_tokens": 1.2e-5,
            "cache_read_input_token_cost_above_272k_tokens": 2e-7,
            "input_cost_per_token_priority": 2e-6, "output_cost_per_token_priority": 1.6e-5}));
        Pricing::from_litellm(&Value::Object(m).to_string()).unwrap()
    }

    fn ev(provider: &str, model: &str) -> EventRow {
        EventRow { provider: provider.into(), model: Some(model.into()), ..Default::default() }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn threshold_parsing() {
        assert_eq!(parse_threshold("input_cost_per_token_above_272k_tokens"), Some(272_000));
        assert_eq!(parse_threshold("cache_creation_input_token_cost_above_1hr_above_100k_tokens"), Some(100_000));
        assert_eq!(parse_threshold("cache_creation_input_token_cost_above_1hr"), None);
    }

    #[test]
    fn claude_cache_ttls() {
        let p = pricing();
        let mut e = ev("claude", "claude-opus-5-5");
        e.cache_write = 1_000_000;
        e.cache_write_1h = 600_000;
        // 400k × $5/M + 600k × $8/M = 2.0 + 4.8
        assert!(close(p.estimate(&e).unwrap(), 6.8));
        e.cache_read = 1_000_000; // + $0.20
        assert!(close(p.estimate(&e).unwrap(), 7.0));
    }

    #[test]
    fn claude_fast_us_and_search() {
        let p = pricing();
        let mut e = ev("claude", "claude-opus-5-5");
        e.input = 1_000_000;
        e.output = 100_000; // 4 + 2 = 6
        e.tier = Some("fast,us".into());
        e.web_search = 3;
        assert!(close(p.estimate(&e).unwrap(), 6.0 * 2.0 * 1.1 + 0.03));
    }

    #[test]
    fn haiku_long_prompt_tier_applies_to_whole_request() {
        let p = pricing();
        let mut e = ev("claude", "claude-haiku-5-5");
        e.input = 10_000;
        e.cache_read = 95_000; // prompt 105k > 100k
        e.output = 1_000;
        let want = 10_000.0 * 5e-7 + 95_000.0 * 5e-8 + 1_000.0 * 2.5e-6;
        assert!(close(p.estimate(&e).unwrap(), want));
        e.cache_read = 80_000; // 90k → base tier
        let want = 10_000.0 * 1e-7 + 80_000.0 * 1e-8 + 1_000.0 * 5e-7;
        assert!(close(p.estimate(&e).unwrap(), want));
    }

    #[test]
    fn openai_long_context_and_priority() {
        let p = pricing();
        let mut e = ev("codex", "gpt-6.1-terra");
        e.input = 100_000;
        e.cache_read = 200_000; // 300k > 272k
        e.output = 10_000;
        assert!(close(p.estimate(&e).unwrap(), 100_000.0 * 2e-6 + 200_000.0 * 2e-7 + 10_000.0 * 1.2e-5));
        e.cache_read = 0;
        e.tier = Some("priority".into());
        assert!(close(p.estimate(&e).unwrap(), 100_000.0 * 2e-6 + 10_000.0 * 1.6e-5));
    }

    #[test]
    fn reported_cost_wins_and_unknown_is_none() {
        let p = pricing();
        let mut e = ev("cursor", "grok-bot-default");
        e.input = 1000;
        assert_eq!(p.cost(&e), None);
        e.cost_reported = Some(0.42);
        assert_eq!(p.cost(&e), Some(0.42));
        assert!(p.knows("anthropic/claude-opus-5-5[1m]"));
        assert!(p.knows("gpt-6.1-terra-high"));
    }
}
