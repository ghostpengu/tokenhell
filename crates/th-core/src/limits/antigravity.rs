//! Live plan limits from agy's supported, read-only `/usage` command report.
//! Since 1.1.11 this runs without an agent turn or token spend; older versions
//! must never receive the slash command as a prompt. Credentials stay with agy.

use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use tokio::io::AsyncReadExt;

use crate::model::{LimitReport, LimitWindow, Provider, j};

fn executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn agy_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ANTIGRAVITY_CLI_PATH") {
        let path = PathBuf::from(path);
        return executable(&path).then(|| path.canonicalize().ok()).flatten();
    }
    // Menu-bar apps don't inherit the user's shell PATH.
    let home = dirs::home_dir().unwrap_or_default();
    let mut candidates = vec![home.join(".local/bin/agy"), "/opt/homebrew/bin/agy".into(), "/usr/local/bin/agy".into()];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|p| p.join("agy")));
    }
    candidates.into_iter().find(|p| executable(p)).and_then(|p| p.canonicalize().ok())
}

pub fn available() -> bool {
    agy_bin().is_some()
}

pub async fn poll() -> LimitReport {
    let result = async {
        let bin = agy_bin()
            .ok_or_else(|| anyhow!("agy CLI not found; install Antigravity CLI or set ANTIGRAVITY_CLI_PATH"))?;
        fetch(&bin).await
    }
    .await;
    result.unwrap_or_else(|e| LimitReport::error(Provider::Antigravity, format!("{e:#}")))
}

async fn fetch(bin: &Path) -> Result<LimitReport> {
    let version = run(bin, &["--version"], Duration::from_secs(3), 8192).await?;
    if !supports_report(std::str::from_utf8(&version).unwrap_or_default()) {
        bail!("update Antigravity CLI to 1.1.11 or newer for live usage limits");
    }
    let output = run(bin, &["-p", "/usage", "--output-format", "json"], Duration::from_secs(24), 1024 * 1024).await?;
    parse(&serde_json::from_slice(&output).context("invalid JSON from agy usage report")?)
}

fn supports_report(version: &str) -> bool {
    version
        .split_whitespace()
        .find_map(|s| {
            let mut parts = s.trim_start_matches('v').split('.');
            let major = parts.next()?.parse::<u64>().ok()?;
            let minor = parts.next()?.parse::<u64>().ok()?;
            let patch = parts.next()?.split('-').next()?.parse::<u64>().ok()?;
            Some((major, minor, patch) >= (1, 1, 11))
        })
        .unwrap_or(false)
}

/// Kill only the process group created for this probe, including any child helpers.
/// The guard runs on success, error, timeout and cancellation by the app's poll loop.
struct ProbeGroup(u32);

impl Drop for ProbeGroup {
    fn drop(&mut self) {
        // SAFETY: process_group(0) below creates a new group named by this owned
        // child's PID. A negative PID targets that group, never the user's agy.
        unsafe {
            libc::kill(-(self.0 as libc::pid_t), libc::SIGKILL);
        }
    }
}

async fn run(bin: &Path, args: &[&str], timeout: Duration, max_bytes: u64) -> Result<Vec<u8>> {
    // Avoid loading workspace customizations or opening browser sign-in during a
    // background refresh. Existing agy login/Keychain handling remains its own.
    let dir = tempfile::Builder::new().prefix("tokenhell-agy-").tempdir()?;
    for name in ["open", "xdg-open"] {
        let launcher = dir.path().join(name);
        std::fs::write(&launcher, "#!/bin/sh\nexit 1\n")?;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut paths = vec![dir.path().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into())));
    let path = std::env::join_paths(paths)?;
    let mut child = tokio::process::Command::new(bin)
        .args(args)
        .current_dir(dir.path())
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "true")
        .env("BROWSER", dir.path().join("open"))
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .context("starting agy usage check")?;
    let _group = ProbeGroup(child.id().ok_or_else(|| anyhow!("agy usage check did not start"))?);
    tokio::time::timeout(timeout, async {
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("missing agy report output"))?
            .take(max_bytes + 1)
            .read_to_end(&mut output)
            .await?;
        if output.len() as u64 > max_bytes {
            bail!("agy report exceeded the output limit");
        }
        if !child.wait().await?.success() {
            // Never surface stderr: it may contain login URLs or account details.
            bail!("Antigravity usage unavailable; open agy to check your login and /usage");
        }
        Ok(output)
    })
    .await
    .map_err(|_| anyhow!("Antigravity usage check timed out; open agy to check your login"))?
}

pub(crate) fn parse(v: &Value) -> Result<LimitReport> {
    if j::s(v, &["status"]) != Some("SUCCESS") || j::s(v, &["command", "name"]) != Some("usage") {
        bail!("agy did not return a successful usage command report");
    }
    let groups = j::get(v, &["command", "data", "groups"])
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("agy usage report has no quota groups"))?;
    let mut report = LimitReport::new(Provider::Antigravity);
    let mut seen = HashSet::new();
    for group in groups {
        let name = j::s(group, &["name"]).unwrap_or("Quota");
        let family = match name.to_ascii_lowercase().as_str() {
            "gemini models" => "Gemini",
            "claude and gpt models" => "Claude/GPT",
            _ => name,
        };
        for bucket in group.get("buckets").and_then(Value::as_array).into_iter().flatten() {
            let Some(id) = j::s(bucket, &["id"]).filter(|s| !s.is_empty()) else { continue };
            let disabled = bucket.get("disabled").and_then(Value::as_bool) == Some(true);
            let remaining = j::f(bucket, &["remaining_fraction"]).filter(|f| f.is_finite() && (0.0..=1.0).contains(f));
            let used_pct = if disabled { None } else { remaining.map(|f| (1.0 - f) * 100.0) };
            let window = j::s(bucket, &["window"]).unwrap_or("");
            let cadence = match window {
                "weekly" => "week",
                "" if id.ends_with("weekly") => "week",
                "" if id.ends_with("5h") => "5h",
                other => other,
            };
            let label = if cadence.is_empty() {
                format!("{family} · {}", j::s(bucket, &["name"]).unwrap_or("quota"))
            } else {
                format!("{family} · {cadence}")
            };
            let w = LimitWindow {
                key: id.to_string(),
                label,
                used_pct,
                resets_at: j::ts_s(bucket.get("reset_time")),
                detail: if disabled {
                    Some("Disabled".into())
                } else if used_pct.is_none() {
                    Some("Usage unavailable".into())
                } else {
                    None
                },
            };
            if seen.insert(id.to_string()) {
                report.windows.push(w);
            } else if let Some(old) = report.windows.iter_mut().find(|w| w.key == id && w.used_pct.is_none()) {
                if w.used_pct.is_some() {
                    *old = w;
                }
            }
        }
    }
    if !report.windows.iter().any(|w| w.used_pct.is_some()) {
        bail!("Antigravity did not report measured limits; open agy and check /usage");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(groups: Value) -> Value {
        json!({"status":"SUCCESS", "num_turns":0, "command":{"name":"usage", "data":{"groups":groups}}})
    }

    #[test]
    fn parses_five_hour_weekly_and_independent_model_quotas() {
        let v = report(json!([
            {"name":"Gemini Models", "buckets":[
                {"id":"gemini-weekly","window":"weekly","remaining_fraction":0.75,"reset_time":"2026-11-21T23:10:22Z"},
                {"id":"gemini-5h","window":"5h","remaining_fraction":0.4,"reset_time":"2026-11-16T01:52:52Z"}]},
            {"name":"Claude and GPT models", "buckets":[
                {"id":"3p-weekly","window":"weekly","remaining_fraction":1},
                {"id":"3p-5h","window":"5h","remaining_fraction":0}]},
            {"name":"Gemini 4 Argon", "buckets":[
                {"id":"gemini-argon-wtu-weekly","window":"weekly","remaining_fraction":0.2}]}
        ]));
        let r = parse(&v).unwrap();
        assert_eq!(r.provider, Provider::Antigravity);
        assert_eq!(r.windows.len(), 5);
        assert_eq!(r.windows[0].label, "Gemini · week");
        assert_eq!(r.windows[0].used_pct, Some(25.0));
        assert_eq!(r.windows[1].used_pct, Some(60.0));
        assert_eq!(r.windows[1].resets_at, Some(1_794_793_972));
        assert_eq!(r.windows[2].label, "Claude/GPT · week");
        assert_eq!(r.windows[3].used_pct, Some(100.0));
        assert_eq!(r.windows[4].label, "Gemini 4 Argon · week");
        assert_eq!(r.windows[4].used_pct, Some(80.0));
        assert_eq!(r.max_used(), Some(100.0));
    }

    #[test]
    fn missing_disabled_and_invalid_usage_never_becomes_exhausted_quota() {
        let r = parse(&report(json!([{"name":"Gemini Models", "buckets":[
            {"id":"missing-weekly","reset_time":"2026-11-21T23:10:22Z"},
            {"id":"disabled","remaining_fraction":0,"disabled":true},
            {"id":"invalid","remaining_fraction":1.5},
            {"id":"nan","remaining_fraction":"NaN"},
            {"id":"measured","window":"weekly","remaining_fraction":"0.8"}
        ]}])))
        .unwrap();
        assert!(r.windows[..4].iter().all(|w| w.used_pct.is_none()));
        assert!(r.windows[0].resets_at.is_some());
        assert_eq!(r.windows[1].detail.as_deref(), Some("Disabled"));
        assert!((r.max_used().unwrap() - 20.0).abs() < 1e-9);
        assert!(parse(&report(json!([{"buckets":[{"id":"unknown"}]}]))).is_err());
        assert!(parse(&report(json!([]))).is_err());
        assert!(parse(&json!({"status":"SUCCESS", "response":"model-generated text"})).is_err());
        assert!(parse(&json!({"status":"ERROR", "command":{"name":"usage"}})).is_err());
    }

    #[test]
    fn shared_bucket_ids_are_deduplicated_and_known_usage_wins() {
        let r = parse(&report(json!([{"name":"Gemini Models", "buckets":[
            {"id":"gemini-weekly","reset_time":"2026-11-21T23:10:22Z"},
            {"id":"gemini-weekly","remaining_fraction":0.5,"reset_time":"2026-11-21T23:10:22Z"},
            {"id":"gemini-weekly","remaining_fraction":0.5}
        ]}])))
        .unwrap();
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.windows[0].used_pct, Some(50.0));
        assert!(r.windows[0].resets_at.is_some());
    }

    #[test]
    fn requires_a_version_that_handles_usage_without_an_agent_turn() {
        for v in ["1.0.8", "1.1.10", "unknown", "", "1.1"] {
            assert!(!supports_report(v), "{v}");
        }
        for v in ["1.1.11", "v1.2.3", "agy version 1.5.9", "2.0.0"] {
            assert!(supports_report(v), "{v}");
        }
    }

    fn fake_cli(dir: &Path, script: &str) -> PathBuf {
        let path = dir.join("agy");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[tokio::test]
    async fn runner_uses_read_only_report_with_background_login_and_update_guards() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_cli(
            dir.path(),
            r#"
            test "$AGY_CLI_DISABLE_AUTO_UPDATE" = true || exit 20
            "$BROWSER" && exit 21
            open && exit 22
            if test "$1" = --version; then
                printf '1.5.9\n'
            else
                test "$1" = -p && test "$2" = /usage && test "$3" = --output-format && test "$4" = json || exit 23
                printf '%s' '{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[{"name":"Gemini Models","buckets":[{"id":"gemini-weekly","remaining_fraction":0.75}]}]}}}'
            fi
        "#,
        );
        let r = fetch(&bin).await.unwrap();
        assert_eq!(r.windows[0].used_pct, Some(25.0));
    }

    #[tokio::test]
    async fn old_cli_is_never_sent_a_usage_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_cli(
            dir.path(),
            r#"
            if test "$1" = --version; then
                printf '1.1.10\n'
            else
                touch "$(dirname "$0")/usage-called"
                exit 1
            fi
        "#,
        );
        assert!(fetch(&bin).await.unwrap_err().to_string().contains("1.1.11"));
        assert!(!dir.path().join("usage-called").exists());
    }

    #[tokio::test]
    async fn runner_bounds_output_and_stops_hanging_probes() {
        let dir = tempfile::tempdir().unwrap();
        let noisy = fake_cli(dir.path(), "while :; do printf '0123456789'; done");
        assert!(run(&noisy, &[], Duration::from_secs(2), 64).await.unwrap_err().to_string().contains("output limit"));
        let hanging = fake_cli(dir.path(), "sleep 30");
        let start = std::time::Instant::now();
        assert!(
            run(&hanging, &[], Duration::from_millis(100), 64).await.unwrap_err().to_string().contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
