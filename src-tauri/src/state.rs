use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use th_core::Provider;
use th_core::pricing::Pricing;
use th_core::store::Store;
use tokio::sync::Notify;

/// Bound abandoned turns if a client exits without writing its completion event.
pub const MAX_TURN_SILENCE_MS: i64 = 24 * 60 * 60_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TitleMode {
    Icon,
    #[default]
    Tokens,
    Limit,
    Cost,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub title_mode: TitleMode,
    pub daily_token_goal: Option<u64>,
    /// Provider → limit polling enabled. Missing = enabled when detected.
    pub limits: BTreeMap<Provider, bool>,
    /// Background limit poll interval while the panel is closed.
    pub poll_minutes: u64,
    /// Show a filled tray icon when any limit reaches this %.
    pub hot_threshold: f64,
    /// Fetch the LiteLLM price list weekly.
    pub price_refresh: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            title_mode: TitleMode::Tokens,
            daily_token_goal: None,
            limits: BTreeMap::new(),
            poll_minutes: 5,
            hot_threshold: 85.0,
            price_refresh: true,
        }
    }
}

impl Settings {
    fn validate(&self) -> anyhow::Result<()> {
        if self.daily_token_goal.is_some_and(|n| n == 0 || n > 9_007_199_254_740_991) {
            anyhow::bail!("Daily goal must be a positive safe integer");
        }
        Ok(())
    }

    pub fn limit_enabled(&self, p: Provider, detected: &[Provider]) -> bool {
        self.limits.get(&p).copied().unwrap_or_else(|| detected.contains(&p))
    }
}

pub struct AppState {
    pub store: Arc<Store>,
    pub goal_check: Mutex<()>,
    pub pricing: RwLock<Pricing>,
    pub settings: RwLock<Settings>,
    pub http: th_core::reqwest::Client,
    pub data_dir: PathBuf,
    /// Pokes the limits loop to poll now.
    pub wake: Notify,
    pub panel_open: AtomicBool,
    pub last_hide_ms: AtomicI64,
    pub polling: AtomicBool,
    pub hot: AtomicBool,
    /// End of the visual activity window, in Unix milliseconds.
    pub burning_until_ms: AtomicI64,
    /// Active Codex logs and their last observed modification time.
    pub active_codex: Mutex<BTreeMap<String, i64>>,
    pub detected: Vec<Provider>,
}

const SETTINGS_KEY: &str = "settings";

impl AppState {
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        let store = Arc::new(Store::open(&data_dir.join("tokenhell.db"))?);
        let settings = store
            .kv_get(SETTINGS_KEY)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ok(AppState {
            goal_check: Mutex::new(()),
            pricing: RwLock::new(Pricing::load(&data_dir)),
            settings: RwLock::new(settings),
            http: th_core::http_client(),
            wake: Notify::new(),
            panel_open: AtomicBool::new(false),
            last_hide_ms: AtomicI64::new(0),
            polling: AtomicBool::new(false),
            hot: AtomicBool::new(false),
            burning_until_ms: AtomicI64::new(0),
            active_codex: Mutex::new(BTreeMap::new()),
            detected: th_core::limits::detected(),
            store,
            data_dir,
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn burning_until(&self) -> i64 {
        let now = chrono::Utc::now().timestamp_millis();
        let mut active = self.active_codex.lock().unwrap_or_else(|e| e.into_inner());
        active.retain(|_, modified| *modified + MAX_TURN_SILENCE_MS > now);
        active.values().map(|modified| modified + MAX_TURN_SILENCE_MS)
            .max().unwrap_or(0).max(self.burning_until_ms.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn save_settings(&self, s: Settings) -> anyhow::Result<()> {
        s.validate()?;
        self.store.kv_set(SETTINGS_KEY, &serde_json::to_string(&s)?)?;
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = s;
        Ok(())
    }

    pub fn enabled_limits(&self) -> Vec<Provider> {
        let s = self.settings();
        Provider::ALL.into_iter().filter(|p| s.limit_enabled(*p, &self.detected)).collect()
    }
}

#[cfg(test)]
mod goal_tests {
    use super::*;

    #[test]
    fn goal_validation_accepts_disabled_and_safe_positive_integers() {
        for target in [None, Some(1), Some(9_007_199_254_740_991)] {
            assert!(Settings { daily_token_goal: target, ..Default::default() }.validate().is_ok());
        }
        for target in [0, 9_007_199_254_740_992, u64::MAX] {
            assert!(Settings { daily_token_goal: Some(target), ..Default::default() }.validate().is_err());
        }
        for value in ["-1", "1.5"] {
            assert!(serde_json::from_str::<Settings>(&format!(r#"{{"daily_token_goal":{value}}}"#)).is_err());
        }
    }

    #[test]
    fn old_settings_disable_goal_and_roundtrip_preserves_target() {
        let mut settings: Settings = serde_json::from_str(r#"{"title_mode":"cost"}"#).unwrap();
        assert_eq!(settings.daily_token_goal, None);
        settings.daily_token_goal = Some(100_000_000);
        let restored: Settings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.daily_token_goal, Some(100_000_000));
        assert_eq!(restored.title_mode, TitleMode::Cost);
    }
}
