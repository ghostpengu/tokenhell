//! Claude subscription windows via the (undocumented) OAuth usage endpoint that powers `/usage`.
//! The token is read from Claude Code's own credential store and never refreshed or written back.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::model::{LimitReport, LimitWindow, Provider, j};

const URL: &str = "https://api.anthropic.com/api/oauth/usage";

#[derive(Clone)]
struct Creds {
    token: String,
    expires_at_ms: Option<i64>,
    plan: Option<String>,
}

async fn read_creds() -> Result<Creds> {
    // 1) File-backed installs.
    let file = crate::ingest::env_or_home("CLAUDE_CONFIG_DIR", &[".claude"])
        .into_iter()
        .map(|d| d.join(".credentials.json"))
        .find(|p| p.is_file());
    let raw = if let Some(f) = file {
        tokio::fs::read_to_string(f).await?
    } else {
        // 2) macOS Keychain (approved once by the user).
        super::keychain::generic_password("Claude Code-credentials", None).await?
    };
    let v: Value = serde_json::from_str(&raw).context("credential JSON")?;
    let o = v.get("claudeAiOauth").unwrap_or(&v);
    Ok(Creds {
        token: j::s(o, &["accessToken"]).ok_or_else(|| anyhow!("no OAuth token (API-key login?)"))?.to_string(),
        expires_at_ms: o.get("expiresAt").and_then(Value::as_i64),
        plan: j::s(o, &["subscriptionType"]).map(str::to_string),
    })
}

pub async fn poll(http: &reqwest::Client) -> LimitReport {
    match fetch(http).await {
        Ok(r) => r,
        Err(e) => LimitReport::error(Provider::Claude, format!("{e:#}")),
    }
}

fn cache() -> &'static std::sync::Mutex<Option<Creds>> {
    static C: std::sync::OnceLock<std::sync::Mutex<Option<Creds>>> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

fn expired(c: &Creds) -> bool {
    c.expires_at_ms.is_some_and(|t| t < chrono::Utc::now().timestamp_millis() + 30_000)
}

/// Cached until expiry so the Keychain isn't touched on every poll.
async fn creds() -> Result<Creds> {
    if let Some(c) = cache().lock().unwrap_or_else(|e| e.into_inner()).clone().filter(|c| !expired(c)) {
        return Ok(c);
    }
    let c = read_creds().await?;
    if expired(&c) {
        bail!("login expired: open Claude Code once to refresh it");
    }
    *cache().lock().unwrap_or_else(|e| e.into_inner()) = Some(c.clone());
    Ok(c)
}

async fn fetch(http: &reqwest::Client) -> Result<LimitReport> {
    let creds = creds().await?;
    let resp = http
        .get(URL)
        .bearer_auth(&creds.token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        *cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
        bail!("not authorised ({status}): open Claude Code to refresh login");
    }
    if !status.is_success() {
        bail!("usage endpoint returned {status}");
    }
    let v: Value = resp.json().await?;
    let mut r = parse(&v);
    r.plan = creds.plan;
    Ok(r)
}

pub(crate) fn parse(v: &Value) -> LimitReport {
    let mut r = LimitReport::new(Provider::Claude);
    for (key, label, field) in [
        ("5h", "5h", "five_hour"),
        ("week", "week", "seven_day"),
        ("week_opus", "week · Opus", "seven_day_opus"),
        ("week_sonnet", "week · Sonnet", "seven_day_sonnet"),
    ] {
        if let Some(w) = j::get(v, &[field]) {
            r.windows.push(LimitWindow {
                key: key.into(),
                label: label.into(),
                used_pct: j::f(w, &["utilization"]),
                resets_at: j::ts_s(w.get("resets_at")),
                detail: None,
            });
        }
    }
    if let Some(x) = j::get(v, &["extra_usage"]) {
        if x.get("is_enabled").and_then(Value::as_bool) == Some(true) {
            let used = j::f(x, &["used_credits"]);
            let limit = j::f(x, &["monthly_limit"]);
            r.windows.push(LimitWindow {
                key: "extra".into(),
                label: "extra usage".into(),
                used_pct: j::f(x, &["utilization"]),
                resets_at: None,
                // Credits are reported in cents.
                detail: match (used, limit) {
                    (Some(u), Some(l)) => Some(format!("${:.2} of ${:.0}", u / 100.0, l / 100.0)),
                    _ => None,
                },
            });
        }
    }
    r
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_windows() {
        let v = serde_json::json!({
            "five_hour": {"utilization": 72.0, "resets_at": "2026-11-06T22:00:00Z"},
            "seven_day": {"utilization": 31, "resets_at": "2026-11-10T08:00:00+00:00"},
            "seven_day_opus": null
        });
        let r = super::parse(&v);
        assert_eq!(r.windows.len(), 2);
        assert_eq!(r.windows[0].used_pct, Some(72.0));
        assert!(r.windows[0].resets_at.is_some());
    }
}
