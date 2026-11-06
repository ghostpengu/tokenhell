//! TokenHell core: local usage ingestion, limit polling, pricing and aggregation.
//! No Tauri dependency so it can be tested (and reused by a CLI) on its own.

pub mod ingest;
pub mod limits;
pub mod model;
pub mod pricing;
pub mod providers;
pub mod store;
pub mod summary;

pub use model::{LimitReport, LimitWindow, Provider, ResetCredits, UsageEvent};

/// Shared HTTP client: short timeouts, no cookies stored, identifiable UA.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("TokenHell/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(8))
        .build()
        .expect("http client")
}

pub use reqwest;
