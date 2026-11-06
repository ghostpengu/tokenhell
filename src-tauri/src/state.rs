use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use th_core::Provider;
use th_core::pricing::Pricing;
use th_core::store::Store;
use tokio::sync::Notify;

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
            limits: BTreeMap::new(),
            poll_minutes: 5,
            hot_threshold: 85.0,
            price_refresh: true,
        }
    }
}

impl Settings {
    pub fn limit_enabled(&self, p: Provider, detected: &[Provider]) -> bool {
        self.limits.get(&p).copied().unwrap_or_else(|| detected.contains(&p))
    }
}

pub struct AppState {
    pub store: Arc<Store>,
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
            pricing: RwLock::new(Pricing::load(&data_dir)),
            settings: RwLock::new(settings),
            http: th_core::http_client(),
            wake: Notify::new(),
            panel_open: AtomicBool::new(false),
            last_hide_ms: AtomicI64::new(0),
            polling: AtomicBool::new(false),
            hot: AtomicBool::new(false),
            detected: th_core::limits::detected(),
            store,
            data_dir,
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn save_settings(&self, s: Settings) -> anyhow::Result<()> {
        self.store.kv_set(SETTINGS_KEY, &serde_json::to_string(&s)?)?;
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = s;
        Ok(())
    }

    pub fn enabled_limits(&self) -> Vec<Provider> {
        let s = self.settings();
        Provider::ALL.into_iter().filter(|p| s.limit_enabled(*p, &self.detected)).collect()
    }
}
