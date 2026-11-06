//! Codex limits: live via `codex app-server` JSON-RPC (`account/rateLimits/read`),
//! falling back to the newest `rate_limits` snapshot in local session files.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::model::{LimitReport, LimitWindow, Provider, j};

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
    let msgs = [
        json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "tokenhell", "title": "TokenHell", "version": env!("CARGO_PKG_VERSION")}}}),
        json!({"method": "initialized"}),
        json!({"id": 2, "method": "account/rateLimits/read"}),
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
            return Ok(parse_live(v.get("result").unwrap_or(&Value::Null)));
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
    let resets = j::u(res, &["rateLimitResetCredits", "availableCount"]);
    if resets > 0 {
        r.notes.push(format!("{resets} full reset{} available", if resets == 1 { "" } else { "s" }));
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
        assert_eq!(r.notes, vec!["2 full resets available"]);
    }
}
