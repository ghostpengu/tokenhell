//! Daily progress and serialized, persistent notification deduplication.
use crate::state::AppState;
use chrono::{DateTime, Local, NaiveTime, TimeZone};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use th_core::{store::Store, summary::DailyGoal};

const NOTIFIED: &str = "daily_goal_notified_day";

pub fn progress(state: &AppState) -> anyhow::Result<Option<DailyGoal>> {
    state
        .settings()
        .daily_token_goal
        .map(|target| progress_at(&state.store, target, Local::now()))
        .transpose()
}

fn progress_at<T: TimeZone>(
    store: &Store,
    target: u64,
    now: DateTime<T>,
) -> anyhow::Result<DailyGoal> {
    let day = now.date_naive();
    let start = now
        .timezone()
        .from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest()
        .ok_or_else(|| anyhow::anyhow!("Local midnight unavailable"))?;
    // The store's upper bound is exclusive. Include events through now, never future events.
    let used = store
        .events_in(start.timestamp_millis(), now.timestamp_millis() + 1)?
        .iter()
        .map(|e| e.total())
        .sum();
    Ok(DailyGoal {
        day: day.to_string(),
        target,
        used,
    })
}

fn claim(store: &Store, goal: &DailyGoal) -> anyhow::Result<bool> {
    if goal.used < goal.target || store.kv_get(NOTIFIED)?.as_deref() == Some(&goal.day) {
        return Ok(false);
    }
    // Claim before sending: a crash must not cause duplicate delivery on restart.
    store.kv_set(NOTIFIED, &goal.day)?;
    Ok(true)
}

pub fn check(app: &AppHandle) {
    if let Err(e) = check_inner(app) {
        tracing::warn!("daily goal: {e:#}");
    }
}

fn check_inner(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<std::sync::Arc<AppState>>();
    let _guard = state.goal_check.lock().unwrap_or_else(|e| e.into_inner());
    let Some(goal) = progress(&state)? else {
        return Ok(());
    };
    if goal.used < goal.target || !permission(app, false)? {
        return Ok(());
    }
    if claim(&state.store, &goal)? {
        app.notification()
            .builder()
            .title("Daily token goal reached")
            .body(format!(
                "You've burned {} tokens today. Goal: {}.",
                goal.used, goal.target
            ))
            .show()?;
    }
    Ok(())
}

/// Tauri's desktop permission API always returns Granted; query macOS itself instead.
#[cfg(target_os = "macos")]
pub fn permission(_app: &AppHandle, request: bool) -> anyhow::Result<bool> {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::NSError;
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNNotificationSettings,
        UNUserNotificationCenter,
    };
    if objc2_foundation::NSBundle::mainBundle()
        .bundleIdentifier()
        .is_none()
    {
        anyhow::bail!("Native notification permission requires the bundled app");
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (tx, rx) = std::sync::mpsc::channel();
    if request {
        let callback = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            let _ = tx.send(granted.as_bool());
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &callback,
        );
    } else {
        let callback = RcBlock::new(move |settings: std::ptr::NonNull<UNNotificationSettings>| {
            // The framework guarantees a valid settings object during the callback.
            let granted = matches!(
                unsafe { settings.as_ref() }.authorizationStatus(),
                UNAuthorizationStatus::Authorized | UNAuthorizationStatus::Provisional
            );
            let _ = tx.send(granted);
        });
        center.getNotificationSettingsWithCompletionHandler(&callback);
    }
    Ok(rx.recv_timeout(std::time::Duration::from_secs(60))?)
}

#[cfg(not(target_os = "macos"))]
pub fn permission(app: &AppHandle, request: bool) -> anyhow::Result<bool> {
    use tauri_plugin_notification::PermissionState;
    let state = if request {
        app.notification().request_permission()?
    } else {
        app.notification().permission_state()?
    };
    Ok(state == PermissionState::Granted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use th_core::{Provider, model::UsageEvent, store::FileRow};

    #[test]
    fn counts_all_buckets_and_tools_in_the_local_day_only() {
        let store = Store::open_in_memory().unwrap();
        let tz = chrono::FixedOffset::east_opt(3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 11, 9, 12, 0, 0).unwrap();
        let midnight = tz
            .with_ymd_and_hms(2026, 11, 9, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        for provider in [Provider::Claude, Provider::Codex] {
            let events: Vec<_> = [
                midnight - 1,
                midnight,
                now.timestamp_millis(),
                now.timestamp_millis() + 1,
            ]
            .into_iter()
            .enumerate()
            .map(|(i, ts)| UsageEvent {
                dedup_key: i.to_string(),
                ts,
                input: 1,
                output: 2,
                cache_read: 3,
                cache_write: 4,
                ..Default::default()
            })
            .collect();
            store
                .commit_file(
                    provider,
                    provider.as_str(),
                    false,
                    &events,
                    &FileRow::default(),
                )
                .unwrap();
        }
        let goal = progress_at(&store, 100, now).unwrap();
        assert_eq!(goal.day, "2026-11-09");
        assert_eq!(goal.used, 40);
        let tomorrow = tz.with_ymd_and_hms(2026, 11, 10, 0, 0, 0).unwrap();
        assert_eq!(progress_at(&store, 100, tomorrow).unwrap().used, 0);
    }

    #[test]
    fn notification_claim_survives_restart_and_goal_edits() {
        let dir = std::env::temp_dir().join(format!("tokenhell-goal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let mut goal = DailyGoal {
            day: "2026-11-09".into(),
            target: 100,
            used: 99,
        };
        {
            let store = Store::open(&path).unwrap();
            assert!(!claim(&store, &goal).unwrap());
            goal.used = 100;
            assert!(claim(&store, &goal).unwrap());
        }
        let store = Store::open(&path).unwrap();
        goal.target = 50;
        goal.used = 200;
        assert!(!claim(&store, &goal).unwrap());
        goal.day = "2026-11-10".into();
        assert!(claim(&store, &goal).unwrap());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_checks_claim_once() {
        let store = Store::open_in_memory().unwrap();
        let guard = std::sync::Mutex::new(());
        let goal = DailyGoal {
            day: "2026-11-09".into(),
            target: 100,
            used: 101,
        };
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        let _lock = guard.lock().unwrap();
                        claim(&store, &goal).unwrap()
                    })
                })
                .collect();
            assert_eq!(
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap())
                    .filter(|claimed| *claimed)
                    .count(),
                1
            );
        });
    }

    #[test]
    fn local_calendar_day_includes_both_dst_hours() {
        // Run with TZ=Europe/Bratislava to exercise the 25-hour fall-back day.
        let store = Store::open_in_memory().unwrap();
        let now = Local.with_ymd_and_hms(2026, 10, 25, 23, 59, 59).unwrap();
        let midnight = Local.with_ymd_and_hms(2026, 10, 25, 0, 0, 0).unwrap();
        let events: Vec<_> = (0..=(now.timestamp() - midnight.timestamp()) / 3600)
            .map(|h| UsageEvent {
                dedup_key: h.to_string(),
                ts: midnight.timestamp_millis() + h * 3_600_000,
                output: 1,
                ..Default::default()
            })
            .collect();
        store
            .commit_file(Provider::Codex, "dst", false, &events, &FileRow::default())
            .unwrap();
        assert_eq!(
            progress_at(&store, 100, now).unwrap().used,
            events.len() as u64
        );
        if std::env::var("TZ").as_deref() == Ok("Europe/Bratislava") {
            assert_eq!(events.len(), 25);
        }
    }
}
