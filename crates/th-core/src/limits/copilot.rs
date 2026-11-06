//! GitHub Copilot quota via `api.github.com/copilot_internal/user` (undocumented).
//! Token: Copilot editor plugins' `~/.config/github-copilot/{apps,hosts}.json`.

use anyhow::{Result, anyhow, bail};
use serde_json::Value;

use crate::model::{LimitReport, LimitWindow, Provider, j};

fn token() -> Option<String> {
    let dir = dirs::home_dir()?.join(".config/github-copilot");
    for f in ["apps.json", "hosts.json"] {
        let Ok(text) = std::fs::read_to_string(dir.join(f)) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        if let Some(obj) = v.as_object() {
            for (k, entry) in obj {
                if k.starts_with("github.com") {
                    if let Some(t) = j::s(entry, &["oauth_token"]) {
                        return Some(t.to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn available() -> bool {
    token().is_some()
}

pub async fn poll(http: &reqwest::Client) -> LimitReport {
    match fetch(http).await {
        Ok(r) => r,
        Err(e) => LimitReport::error(Provider::Copilot, format!("{e:#}")),
    }
}

async fn fetch(http: &reqwest::Client) -> Result<LimitReport> {
    let t = token().ok_or_else(|| anyhow!("no Copilot login found (~/.config/github-copilot)"))?;
    let resp = http
        .get("https://api.github.com/copilot_internal/user")
        .header("Authorization", format!("token {t}"))
        .header("Accept", "application/json")
        .header("X-GitHub-Api-Version", "2025-04-01")
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("copilot_internal/user returned {}", resp.status());
    }
    Ok(parse(&resp.json().await?))
}

pub(crate) fn parse(v: &Value) -> LimitReport {
    let mut r = LimitReport::new(Provider::Copilot);
    r.plan = j::s(v, &["copilot_plan"]).map(str::to_string);
    let reset = j::ts_s(v.get("quota_reset_date_utc").or(v.get("quota_reset_date")));
    for (key, label) in [("premium_interactions", "premium"), ("chat", "chat"), ("completions", "completions")] {
        let Some(q) = j::get(v, &["quota_snapshots", key]) else { continue };
        if q.get("unlimited").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let pct_left = j::f(q, &["percent_remaining"]);
        let ent = j::f(q, &["entitlement"]);
        let rem = j::f(q, &["remaining"]);
        r.windows.push(LimitWindow {
            key: key.into(),
            label: label.into(),
            used_pct: pct_left.map(|p| (100.0 - p).clamp(0.0, 100.0)),
            resets_at: reset,
            detail: match (ent, rem) {
                (Some(e), Some(rm)) => Some(format!("{:.0} / {:.0}", e - rm, e)),
                _ => None,
            },
        });
    }
    r
}
