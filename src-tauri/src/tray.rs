use std::sync::Arc;
use std::sync::atomic::Ordering;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_positioner::{Position, WindowExt};
use th_core::summary::{Range, summarize};

use crate::state::{AppState, TitleMode};

const TRAY_ID: &str = "main";
const ICON: &[u8] = include_bytes!("../icons/tray.png");
const ICON_HOT: &[u8] = include_bytes!("../icons/tray-hot.png");

/// Decode a horizontal strip into individual template images once at startup.
#[cfg(target_os = "macos")]
fn animation_frames(bytes: &[u8]) -> Option<Vec<Image<'static>>> {
    const SIZE: usize = 44;
    const COUNT: usize = 16;
    let strip = Image::from_bytes(bytes).ok()?;
    if strip.width() != (SIZE * COUNT) as u32 || strip.height() != SIZE as u32 { return None }
    let pixels = strip.rgba();
    Some((0..COUNT).map(|frame| {
        let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
        for row in 0..SIZE {
            let start = (row * SIZE * COUNT + frame * SIZE) * 4;
            rgba.extend_from_slice(&pixels[start..start + SIZE * 4]);
        }
        Image::new_owned(rgba, SIZE as u32, SIZE as u32)
    }).collect())
}

#[cfg(target_os = "macos")]
fn tray_frames(normal: &[u8], hot: &[u8]) -> Option<(Vec<Image<'static>>, Vec<Image<'static>>)> {
    match (animation_frames(normal), animation_frames(hot)) {
        (Some(normal), Some(hot)) => Some((normal, hot)),
        _ => {
            tracing::warn!("could not decode tray animation; using static icons");
            match (Image::from_bytes(ICON), Image::from_bytes(ICON_HOT)) {
                (Ok(normal), Ok(hot)) => Some((vec![normal], vec![hot])),
                _ => {
                    tracing::error!("could not decode static tray icons");
                    None
                }
            }
        }
    }
}

/// Animate only after recent local token usage, preserving macOS template colouring.
#[cfg(target_os = "macos")]
fn animate(app: AppHandle) {
    std::thread::spawn(move || {
        let Some((normal, hot)) = tray_frames(
            include_bytes!("../icons/tray-frames.png"),
            include_bytes!("../icons/tray-hot-frames.png"),
        ) else { return };
        let animated = normal.len() > 1;
        let mut step = 0;
        let mut displayed = None;
        let mut reduce_motion = false;
        let mut next_motion_check = std::time::Instant::now();
        loop {
            let state = app.state::<Arc<AppState>>();
            let active = now_ms() < state.burning_until();
            let is_hot = state.hot.load(Ordering::Relaxed);
            if active && animated && std::time::Instant::now() >= next_motion_check {
                // Query AppKit on the main thread so changes to Reduce Motion apply promptly.
                let (tx, rx) = std::sync::mpsc::channel();
                if app.run_on_main_thread(move || {
                    use objc2::{class, msg_send};
                    use objc2::runtime::{AnyObject, Bool};
                    // SAFETY: NSWorkspace is an AppKit singleton, queried on the main thread.
                    let reduce = unsafe {
                        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
                        let value: Bool = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
                        value.as_bool()
                    };
                    let _ = tx.send(reduce);
                }).is_err() { break }
                let Ok(reduce) = rx.recv() else { break };
                reduce_motion = reduce;
                next_motion_check = std::time::Instant::now() + std::time::Duration::from_secs(1);
            } else if !active {
                next_motion_check = std::time::Instant::now();
            }
            let moving = active && animated && !reduce_motion;
            let frame = if moving { step % normal.len() } else { 0 };
            if displayed != Some((is_hot, frame)) {
                let Some(tray) = app.tray_by_id(TRAY_ID) else { break };
                let image = if is_hot { &hot[frame] } else { &normal[frame] };
                if tray.set_icon_with_as_template(Some(image.clone()), true).is_ok() {
                    displayed = Some((is_hot, frame));
                }
            }
            if moving { step += 1 } else { step = 0 }
            std::thread::sleep(std::time::Duration::from_millis(if moving { 80 } else { 250 }));
        }
    });
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", "Open TokenHell", true, None::<&str>)?,
            &MenuItem::with_id(app, "refresh", "Refresh limits", true, Some("CmdOrCtrl+R"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit TokenHell", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(true)
        .tooltip("TokenHell")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_panel(app),
            "refresh" => {
                th_core::limits::keychain::reset_denials();
                app.state::<Arc<AppState>>().wake.notify_one()
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    #[cfg(target_os = "macos")]
    animate(app.clone());
    Ok(())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn toggle_panel(app: &AppHandle) {
    let Some(w) = app.get_webview_window("panel") else { return };
    if w.is_visible().unwrap_or(false) {
        hide_panel(app);
        return;
    }
    // The click that blurred (and hid) the panel shouldn't immediately reopen it.
    let s = app.state::<Arc<AppState>>();
    if now_ms() - s.last_hide_ms.load(Ordering::Relaxed) < 300 {
        return;
    }
    show_panel(app);
}

pub fn show_panel(app: &AppHandle) {
    let Some(w) = app.get_webview_window("panel") else { return };
    let _ = w.move_window_constrained(Position::TrayCenter);
    let _ = w.show();
    let _ = w.set_focus();
    refit_shadow(&w);
    let s = app.state::<Arc<AppState>>();
    let was_open = s.panel_open.swap(true, Ordering::Relaxed);
    let _ = app.emit("panel-shown", ());
    // Refresh limits on open if the newest data is older than a minute.
    let newest = s.store.limits().unwrap_or_default().iter().map(|r| r.observed_at).max().unwrap_or(0);
    if !was_open && chrono::Utc::now().timestamp() - newest > 60 {
        s.wake.notify_one();
    }
}

/// Corner radius of the panel; must match `windowEffects.radius` in tauri.conf.json.
#[cfg(target_os = "macos")]
const PANEL_RADIUS: f64 = 22.0;

/// The glass view is rounded, but the window (and the webview above the glass) is a plain
/// rectangle, so macOS draws its shadow and rim with square corners. Clip the whole window
/// content to the glass shape, then have the shadow recomputed from it.
fn refit_shadow(w: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        fn apply(w: &tauri::WebviewWindow) {
            use objc2::runtime::{AnyObject, Bool};
            use objc2::msg_send;
            let w2 = w.clone();
            let _ = w.run_on_main_thread(move || {
                let Ok(ns) = w2.ns_window() else { return };
                // SAFETY: valid NSWindow owned by tao; we're on the main thread.
                unsafe {
                    let win = &*(ns as *mut AnyObject);
                    let content: *mut AnyObject = msg_send![win, contentView];
                    if let Some(content) = content.as_ref() {
                        let frame: *mut AnyObject = msg_send![content, superview];
                        for v in [Some(content), frame.as_ref()].into_iter().flatten() {
                            let _: () = msg_send![v, setWantsLayer: Bool::YES];
                            let layer: *mut AnyObject = msg_send![v, layer];
                            if let Some(layer) = layer.as_ref() {
                                let _: () = msg_send![layer, setCornerRadius: PANEL_RADIUS];
                                let _: () = msg_send![layer, setCornerCurve: &*objc2_foundation::ns_string!("continuous")];
                                let _: () = msg_send![layer, setMasksToBounds: Bool::YES];
                            }
                        }
                    }
                    let _: () = msg_send![win, invalidateShadow];
                }
            });
        }
        apply(w);
        // Again once the webview has painted (first open draws after show).
        let w = w.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            apply(&w);
        });
    }
}

pub fn hide_panel(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("panel") {
        let _ = w.hide();
    }
    let s = app.state::<Arc<AppState>>();
    s.panel_open.store(false, Ordering::Relaxed);
    s.last_hide_ms.store(now_ms(), Ordering::Relaxed);
}

pub fn fmt_tokens(n: u64) -> String {
    let f = n as f64;
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}K", f / 1e3),
        1_000_000..=999_999_999 => format!("{:.1}M", f / 1e6),
        _ => format!("{:.2}B", f / 1e9),
    }
    .replace(".0K", "K")
    .replace(".0M", "M")
}

/// Recompute tray title + hot icon from current data.
pub fn update(app: &AppHandle) {
    let s = app.state::<Arc<AppState>>().inner().clone();
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let settings = s.settings();
    let enabled = s.enabled_limits();
    let limits: Vec<_> =
        s.store.limits().unwrap_or_default().into_iter().filter(|r| enabled.contains(&r.provider)).collect();
    let max_pct = limits.iter().filter_map(|r| r.max_used()).fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v))));

    let title = match settings.title_mode {
        TitleMode::Icon => None,
        TitleMode::Limit => max_pct.map(|p| format!("{p:.0}%")),
        TitleMode::Tokens | TitleMode::Cost => {
            let pricing = s.pricing.read().unwrap_or_else(|e| e.into_inner());
            summarize(&s.store, &pricing, Range::Today).ok().map(|sum| {
                if settings.title_mode == TitleMode::Cost {
                    format!("${:.2}", sum.cost_usd)
                } else {
                    fmt_tokens(sum.tokens.total)
                }
            })
        }
    };
    let _ = tray.set_title(title.as_deref());

    let hot = max_pct.is_some_and(|p| p >= settings.hot_threshold);
    #[cfg(target_os = "macos")]
    s.hot.store(hot, Ordering::Relaxed);
    #[cfg(not(target_os = "macos"))]
    if s.hot.swap(hot, Ordering::Relaxed) != hot {
        if let Ok(img) = Image::from_bytes(if hot { ICON_HOT } else { ICON }) {
            let _ = tray.set_icon(Some(img));
            let _ = tray.set_icon_as_template(true);
        }
    }
    let tip = match max_pct {
        Some(p) => format!("TokenHell · highest limit {p:.0}%"),
        None => "TokenHell".into(),
    };
    let _ = tray.set_tooltip(Some(tip));
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn animation_strips_contain_distinct_visible_frames() {
        for bytes in [include_bytes!("../icons/tray-frames.png").as_slice(), include_bytes!("../icons/tray-hot-frames.png").as_slice()] {
            let frames = super::animation_frames(bytes).expect("valid animation strip");
            assert_eq!(frames.len(), 16);
            assert!(frames.iter().all(|f| f.rgba().chunks_exact(4).any(|pixel| pixel[3] != 0)));
            assert_ne!(frames[0].rgba(), frames[4].rgba());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn invalid_animation_uses_both_static_icons() {
        let (normal, hot) = super::tray_frames(b"invalid", include_bytes!("../icons/tray-hot-frames.png")).unwrap();
        assert_eq!((normal.len(), hot.len()), (1, 1));
        assert_ne!(normal[0].rgba(), hot[0].rgba());
    }

    #[test]
    fn tokens_format() {
        assert_eq!(super::fmt_tokens(950), "950");
        assert_eq!(super::fmt_tokens(12_000), "12K");
        assert_eq!(super::fmt_tokens(60_420_262), "60.4M");
        assert_eq!(super::fmt_tokens(1_234_000_000), "1.23B");
    }
}
