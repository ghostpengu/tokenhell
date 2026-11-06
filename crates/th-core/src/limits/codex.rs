//! Codex limits: live via `codex app-server` JSON-RPC (`account/rateLimits/read`),
//! falling back to the newest `rate_limits` snapshot in local session files.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::model::{LimitReport, LimitWindow, Provider, ResetCredits, j};

pub async fn poll() -> LimitReport {
    match tokio::time::timeout(Duration::from_secs(20), live()).await {
        Ok(Ok(r)) => r,
        other => {
            let err = match other {
                Ok(Err(e)) => format!("{e:#}"),
                _ => "app-server timed out".into(),
            };
            match snapshot() {
                Some(mut r) => {
                    r.stale = true;
                    r
                }
                None => LimitReport::error(Provider::Codex, err),
            }
        }
    }
}

fn codex_bin() -> Option<PathBuf> {
    // GUI apps don't inherit the shell PATH: probe common install locations.
    let home = dirs::home_dir().unwrap_or_default();
    let mut c = vec![
        home.join(".local/bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
        home.join(".npm-global/bin/codex"),
        home.join(".bun/bin/codex"),
    ];
    if let Ok(path) = std::env::var("PATH") {
        c.extend(path.split(':').map(|d| PathBuf::from(d).join("codex")));
    }
    c.into_iter().find(|p| p.is_file())
}

async fn live() -> Result<LimitReport> {
    Ok(parse_live(&rpc("account/rateLimits/read", None).await?))
}

/// Outcome of redeeming a Codex "full reset" credit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResetOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}

/// Spend one rate-limit reset credit (the backend picks which). Irreversible.
/// `idempotency_key` identifies one logical attempt: reuse it when retrying that attempt so a
/// lost response can't spend a second credit.
pub async fn consume_reset_credit(idempotency_key: &str) -> Result<ResetOutcome> {
    if idempotency_key.trim().is_empty() {
        return Err(anyhow!("missing idempotency key"));
    }
    let fut = rpc("account/rateLimitResetCredit/consume", Some(json!({ "idempotencyKey": idempotency_key })));
    let res = tokio::time::timeout(Duration::from_secs(30), fut)
        .await
        .map_err(|_| anyhow!("app-server timed out"))??;
    parse_outcome(&res)
}

pub(crate) fn parse_outcome(res: &Value) -> Result<ResetOutcome> {
    Ok(match j::s(res, &["outcome"]) {
        Some("reset") => ResetOutcome::Reset,
        Some("nothingToReset") => ResetOutcome::NothingToReset,
        Some("noCredit") => ResetOutcome::NoCredit,
        Some("alreadyRedeemed") => ResetOutcome::AlreadyRedeemed,
        other => return Err(anyhow!("unexpected reset outcome: {}", other.unwrap_or("none"))),
    })
}

/// One request against a short-lived `codex app-server` (stdio JSON-RPC); returns `result`.
async fn rpc(method: &str, params: Option<Value>) -> Result<Value> {
    let bin = codex_bin().ok_or_else(|| anyhow!("codex CLI not found"))?;
    let mut child = tokio::process::Command::new(bin)
        .arg("app-server")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawning codex app-server")?;
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut req = json!({"id": 2, "method": method});
    if let Some(p) = params {
        req["params"] = p;
    }
    let msgs = [
        json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "tokenhell", "title": "TokenHell", "version": env!("CARGO_PKG_VERSION")}}}),
        json!({"method": "initialized"}),
        req,
    ];
    for m in msgs {
        stdin.write_all(format!("{m}\n").as_bytes()).await?;
    }
    stdin.flush().await?;
    while let Some(line) = lines.next_line().await? {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v.get("id") == Some(&json!(2)) {
            let _ = child.kill().await;
            if let Some(e) = v.get("error") {
                return Err(anyhow!("app-server: {}", j::s(e, &["message"]).unwrap_or("error")));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }
    Err(anyhow!("app-server closed without answering"))
}

fn window_label(mins: Option<f64>, fallback: &str) -> (String, String) {
    match mins.map(|m| m as i64) {
        Some(300) => ("5h".into(), "5h".into()),
        Some(10080) => ("week".into(), "week".into()),
        Some(m) if m > 0 && m % 1440 == 0 => (format!("{}d", m / 1440), format!("{}d", m / 1440)),
        Some(m) if m > 0 && m % 60 == 0 => (format!("{}h", m / 60), format!("{}h", m / 60)),
        _ => (fallback.into(), fallback.into()),
    }
}

pub(crate) fn parse_live(res: &Value) -> LimitReport {
    let mut r = LimitReport::new(Provider::Codex);
    let rl = res.get("rateLimits").unwrap_or(res);
    for (slot, fb) in [("primary", "primary"), ("secondary", "secondary")] {
        if let Some(w) = j::get(rl, &[slot]) {
            let (key, label) = window_label(j::f(w, &["windowDurationMins"]), fb);
            r.windows.push(LimitWindow {
                key,
                label,
                used_pct: j::f(w, &["usedPercent"]),
                resets_at: j::ts_s(w.get("resetsAt")),
                detail: None,
            });
        }
    }
    r.plan = j::s(rl, &["planType"]).map(str::to_string);
    if let Some(c) = rl.get("credits") {
        if c.get("hasCredits").and_then(Value::as_bool) == Some(true) {
            if let Some(b) = j::s(c, &["balance"]) {
                r.notes.push(format!("{b} credits"));
            }
        }
    }
    if let Some(rc) = j::get(res, &["rateLimitResetCredits"]) {
        let available = j::u(rc, &["availableCount"]);
        let next_expires_at = rc
            .get("credits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|c| j::s(c, &["status"]).is_none_or(|s| s == "available"))
            .filter_map(|c| j::ts_s(c.get("expiresAt")))
            .min();
        if available > 0 {
            r.reset_credits = Some(ResetCredits { available, next_expires_at });
        }
    }
    r
}

/// Newest `rate_limits` object in the most recently modified session files.
fn snapshot() -> Option<LimitReport> {
    let root = crate::providers::codex_home().join("sessions");
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.into_path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, f) in files.into_iter().take(5) {
        let text = std::fs::read_to_string(&f).ok()?;
        for line in text.lines().rev() {
            if !line.contains("\"rate_limits\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let Some(rl) = j::get(&v, &["payload", "rate_limits"]) else { continue };
            let mut r = LimitReport::new(Provider::Codex);
            for slot in ["primary", "secondary"] {
                if let Some(w) = j::get(rl, &[slot]) {
                    let (key, label) = window_label(j::f(w, &["window_minutes"]), slot);
                    r.windows.push(LimitWindow {
                        key,
                        label,
                        used_pct: j::f(w, &["used_percent"]),
                        resets_at: j::ts_s(w.get("resets_at")),
                        detail: None,
                    });
                }
            }
            r.plan = j::s(rl, &["plan_type"]).map(str::to_string);
            r.observed_at = j::ts_s(v.get("timestamp")).unwrap_or(r.observed_at);
            return Some(r);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_app_server_result() {
        let v = serde_json::json!({"rateLimits":{"primary":{"usedPercent":6,"windowDurationMins":300,"resetsAt":1794011361},
            "secondary":{"usedPercent":52,"windowDurationMins":10080,"resetsAt":1794135251},
            "credits":{"hasCredits":false},"planType":"team"},
            "rateLimitResetCredits":{"availableCount":2}});
        let r = super::parse_live(&v);
        assert_eq!(r.windows.len(), 2);
        assert_eq!(r.windows[0].key, "5h");
        assert_eq!(r.windows[1].key, "week");
        assert_eq!(r.windows[1].used_pct, Some(52.0));
        assert_eq!(r.plan.as_deref(), Some("team"));
        assert!(r.notes.is_empty());
        assert_eq!(r.reset_credits, Some(super::ResetCredits { available: 2, next_expires_at: None }));
    }

    #[test]
    fn reset_credit_expiry_uses_earliest_available() {
        let v = serde_json::json!({"rateLimits":{},"rateLimitResetCredits":{"availableCount":2,"credits":[
            {"id":"a","status":"available","expiresAt":1800000000},
            {"id":"b","status":"redeemed","expiresAt":1700000000},
            {"id":"c","status":"available","expiresAt":1790000000},
            {"id":"d","status":"available","expiresAt":null}]}});
        let rc = super::parse_live(&v).reset_credits.unwrap();
        assert_eq!(rc.next_expires_at, Some(1790000000));
        let none = serde_json::json!({"rateLimits":{},"rateLimitResetCredits":{"availableCount":0,"credits":[]}});
        assert_eq!(super::parse_live(&none).reset_credits, None);
    }

    /// Read-only smoke test against the real `codex app-server` (never consumes a credit).
    #[tokio::test]
    #[ignore]
    async fn live_rate_limits_read() {
        let r = super::live().await.expect("live read");
        println!("{r:#?}");
        assert!(!r.windows.is_empty());
    }

    #[test]
    fn parses_reset_outcomes() {
        use super::{ResetOutcome::*, parse_outcome};
        let o = |s: &str| parse_outcome(&serde_json::json!({"outcome": s})).ok();
        assert_eq!(o("reset"), Some(Reset));
        assert_eq!(o("nothingToReset"), Some(NothingToReset));
        assert_eq!(o("noCredit"), Some(NoCredit));
        assert_eq!(o("alreadyRedeemed"), Some(AlreadyRedeemed));
        assert_eq!(o("bogus"), None);
    }
}
