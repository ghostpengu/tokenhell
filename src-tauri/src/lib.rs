mod state;
mod tray;
mod workers;

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use th_core::summary::{Range, Summary, summarize};
use th_core::{LimitReport, Provider};

use state::{AppState, Settings};

type St<'a> = State<'a, Arc<AppState>>;

/// Async + blocking pool: sync commands run on the main thread, and a summary can wait on the
/// store lock while the ingest thread commits a large chunk.
#[tauri::command]
async fn get_summary(state: St<'_>, range: Range) -> Result<Summary, String> {
    let s = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let pricing = s.pricing.read().unwrap_or_else(|e| e.into_inner());
        summarize(&s.store, &pricing, range).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_limits(state: St<'_>) -> Vec<LimitReport> {
    let enabled = state.enabled_limits();
    state.store.limits().unwrap_or_default().into_iter().filter(|r| enabled.contains(&r.provider)).collect()
}

#[tauri::command]
async fn refresh(app: AppHandle) {
    // User-initiated: allow Keychain prompts again after an earlier denial.
    th_core::limits::keychain::reset_denials();
    workers::poll_all(&app).await;
}

#[derive(Serialize)]
struct ProviderInfo {
    id: Provider,
    label: &'static str,
    detected: bool,
    limits_supported: bool,
    limits_enabled: bool,
}

#[derive(Serialize)]
struct SettingsView {
    settings: Settings,
    providers: Vec<ProviderInfo>,
    launch_at_login: bool,
    version: &'static str,
}

#[tauri::command]
fn get_settings(app: AppHandle, state: St<'_>) -> SettingsView {
    let s = state.settings();
    let limits_supported =
        |p: Provider| matches!(p, Provider::Claude | Provider::Codex | Provider::Cursor | Provider::Grok | Provider::Copilot);
    SettingsView {
        providers: Provider::ALL
            .into_iter()
            .filter(|p| limits_supported(*p))
            .map(|p| ProviderInfo {
                id: p,
                label: p.label(),
                detected: state.detected.contains(&p),
                limits_supported: true,
                limits_enabled: s.limit_enabled(p, &state.detected),
            })
            .collect(),
        settings: s,
        launch_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[tauri::command]
fn set_settings(app: AppHandle, state: St<'_>, settings: Settings) -> Result<(), String> {
    let before = state.enabled_limits();
    state.save_settings(settings).map_err(|e| format!("{e:#}"))?;
    let after = state.enabled_limits();
    for p in before.iter().filter(|p| !after.contains(p)) {
        let _ = state.store.delete_limit(*p);
    }
    if after.iter().any(|p| !before.contains(p)) {
        state.wake.notify_one();
    }
    tray::update(&app);
    Ok(())
}

#[tauri::command]
fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<(), String> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    tray::hide_panel(&app);
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| tray::show_panel(app)))
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![
            get_summary,
            get_limits,
            refresh,
            get_settings,
            set_settings,
            set_launch_at_login,
            hide_panel,
            quit
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let dir = app.path().app_data_dir()?;
            app.manage(Arc::new(AppState::new(dir)?));
            tray::build(app.handle())?;
            workers::start(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "panel" {
                if let WindowEvent::Focused(false) = event {
                    tray::hide_panel(window.app_handle());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building TokenHell")
        .run(|_app, event| {
            // Keep running with no visible windows (menu-bar app).
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = event {
                api.prevent_exit();
            }
        });
}
