//! Background work: file ingestion, limit polling, Cursor event sync, price refresh.

use std::sync::Arc;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use th_core::Provider;
use th_core::ingest::Ingestor;
use th_core::pricing::Pricing;

use crate::state::{AppState, MAX_TURN_SILENCE_MS};
use crate::tray;

fn st(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

pub fn usage_changed(app: &AppHandle) {
    let _ = app.emit("usage-updated", ());
    tray::update(app);
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || crate::goal::check(&a));
}

/// Local sources report completed usage events. Only recent token-bearing events count as
/// activity, so rescans, old imports and pricing refreshes do not animate the flame.
fn local_usage_changed(app: &AppHandle, paths: &[std::path::PathBuf], usage_changed_flag: bool, rescanned: bool) {
    let s = st(app);
    let now = chrono::Utc::now().timestamp_millis();
    let before = s.burning_until();
    let codex_root = th_core::providers::codex_home();
    if rescanned && let Ok(active) = s.store.active_codex_files(now - MAX_TURN_SILENCE_MS) {
        let mut turns = s.active_codex.lock().unwrap_or_else(|e| e.into_inner());
        // Preserve known quiet turns, but do not resurrect old abandoned sessions on startup.
        *turns = reconcile_turns(&turns, active, now);
    }
    for path in paths.iter().filter(|p| p.starts_with(codex_root.join("sessions")) || p.starts_with(codex_root.join("archived_sessions"))) {
        let key = path.to_string_lossy().into_owned();
        if let Ok(Some(row)) = s.store.file(&key) {
            if let Some(active) = row.state.get("turn_active").and_then(serde_json::Value::as_bool) {
                let mut turns = s.active_codex.lock().unwrap_or_else(|e| e.into_inner());
                if active { turns.insert(key, row.mtime); } else { turns.remove(&key); }
            }
        }
    }
    if usage_changed_flag && let Ok(events) = s.store.events_in(now - 12_000, now + 1_000) {
        if let Some(until) = burning_until(&events, now) {
            s.burning_until_ms.fetch_max(until, Ordering::Relaxed);
        }
    }
    let after = s.burning_until();
    if after != before {
        let _ = app.emit("burning-until", after);
    }
    if usage_changed_flag { usage_changed(app); }
}

fn reconcile_turns(existing: &BTreeMap<String, i64>, active: Vec<(String, i64)>, now: i64) -> BTreeMap<String, i64> {
    active.into_iter()
        .filter(|(path, modified)| *modified >= now - 5 * 60_000 || existing.contains_key(path))
        .collect()
}

fn burning_until(events: &[th_core::store::EventRow], now: i64) -> Option<i64> {
    events.iter()
        .filter(|e| e.total() > 0 && e.ts <= now && e.ts > now - 12_000)
        .map(|e| e.ts + 12_000)
        .max()
}

#[cfg(test)]
mod activity_tests {
    use super::{burning_until, reconcile_turns};
    use th_core::store::EventRow;

    #[test]
    fn only_recent_token_events_keep_the_flame_active() {
        let event = |ts, output| EventRow { ts, output, ..Default::default() };
        let now = 100_000;
        assert_eq!(burning_until(&[event(88_000, 1), event(99_000, 0), event(100_001, 1)], now), None);
        assert_eq!(burning_until(&[event(95_000, 10), event(99_000, 5)], now), Some(111_000));
    }

    #[test]
    fn rescan_keeps_quiet_turns_and_removes_completed_ones() {
        let now = 1_000_000;
        let existing = [("quiet".into(), 100_000), ("completed".into(), 200_000)].into_iter().collect();
        let active = vec![("quiet".into(), 100_000), ("recent".into(), 999_000), ("abandoned".into(), 100_000)];
        let result = reconcile_turns(&existing, active, now);
        assert_eq!(result.keys().map(String::as_str).collect::<Vec<_>>(), vec!["quiet", "recent"]);
    }
}

pub fn start(app: &AppHandle) {
    // 1) Local history: initial scan, then FSEvents watch (+ rescan every 10 min).
    let a = app.clone();
    std::thread::Builder::new()
        .name("ingest".into())
        .spawn(move || {
            let s = st(&a);
            let ing = Arc::new(Ingestor::new(s.store.clone(), th_core::providers::all_history_sources()));
            let t = std::time::Instant::now();
            let n = ing.scan_all();
            tracing::info!("initial scan: {n} events in {:?}", t.elapsed());
            if let Ok(active) = s.store.active_codex_files(chrono::Utc::now().timestamp_millis() - 5 * 60_000) {
                s.active_codex.lock().unwrap_or_else(|e| e.into_inner()).extend(active);
                let _ = a.emit("burning-until", s.burning_until());
            }
            usage_changed(&a);
            let a2 = a.clone();
            if let Err(e) = ing.watch(Duration::from_secs(600), move |paths, changed, rescanned| local_usage_changed(&a2, paths, changed, rescanned)) {
                tracing::error!("watcher stopped: {e:#}");
            }
        })
        .expect("spawn ingest");

    // 2) Limits + Cursor events.
    tauri::async_runtime::spawn(limits_loop(app.clone()));

    // 3) Prices.
    tauri::async_runtime::spawn(prices_loop(app.clone()));
}

async fn limits_loop(app: AppHandle) {
    let s = st(&app);
    let mut last_cursor_sync = 0i64;
    loop {
        poll_all(&app).await;
        let now = chrono::Utc::now().timestamp();
        if s.enabled_limits().contains(&Provider::Cursor) && now - last_cursor_sync > 15 * 60 {
            // Only advance the timer on success, so a failed sync (e.g. Keychain prompt still open)
            // is retried on the next poll instead of 15 minutes later.
            match th_core::limits::cursor::sync_events(&s.http, &s.store).await {
                Ok(n) => {
                    last_cursor_sync = now;
                    if n > 0 {
                        usage_changed(&app);
                    }
                }
                Err(e) => tracing::info!("cursor events: {e:#}"),
            }
        }
        let secs = if s.panel_open.load(Ordering::Relaxed) { 90 } else { s.settings().poll_minutes.max(1) * 60 };
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
            _ = s.wake.notified() => {}
        }
    }
}

/// Poll every enabled provider concurrently. Keeps the last good windows if a poll fails.
pub async fn poll_all(app: &AppHandle) {
    let s = st(app);
    if s.polling.swap(true, Ordering::SeqCst) {
        return;
    }
    let mut handles = vec![];
    for p in s.enabled_limits() {
        let http = s.http.clone();
        let a = app.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            let r = match tokio::time::timeout(Duration::from_secs(30), th_core::limits::poll(&http, p)).await {
                Ok(r) => r,
                Err(_) => Some(th_core::LimitReport::error(p, "timed out")),
            };
            if let Some(r) = r {
                save_report(&st(&a), r);
                let _ = a.emit("limits-updated", ());
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    s.polling.store(false, Ordering::SeqCst);
    let _ = app.emit("limits-updated", ());
    tray::update(app);
}

/// Poll a single provider now (e.g. right after an action changed its limits).
pub async fn poll_one(app: &AppHandle, p: Provider) {
    let s = st(app);
    let r = match tokio::time::timeout(Duration::from_secs(30), th_core::limits::poll(&s.http, p)).await {
        Ok(r) => r,
        Err(_) => Some(th_core::LimitReport::error(p, "timed out")),
    };
    if let Some(r) = r {
        save_report(&s, r);
    }
    let _ = app.emit("limits-updated", ());
    tray::update(app);
}

/// Save a report, keeping the last good windows (marked stale) when this poll failed.
fn save_report(s: &AppState, mut r: th_core::LimitReport) {
    if let Some(err) = r.error.clone() {
        let prev = s.store.limits().unwrap_or_default();
        if let Some(mut old) = prev.into_iter().find(|o| o.provider == r.provider && !o.windows.is_empty()) {
            old.error = Some(err);
            old.stale = true;
            r = old;
        }
    }
    let _ = s.store.save_limit(&r);
}

async fn prices_loop(app: AppHandle) {
    let s = st(&app);
    loop {
        let stale = s.pricing.read().map(|p| p.is_stale()).unwrap_or(true);
        if stale && s.settings().price_refresh {
            match Pricing::refresh(&s.http, &s.data_dir).await {
                Ok(p) => {
                    *s.pricing.write().unwrap_or_else(|e| e.into_inner()) = p;
                    usage_changed(&app);
                }
                Err(e) => tracing::warn!("price refresh: {e:#}"),
            }
        }
        tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
    }
}
