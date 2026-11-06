pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod grok;
pub mod keychain;

use crate::model::{LimitReport, Provider};

/// Providers that can report account limits, and whether they look set up on this machine.
pub fn detected() -> Vec<Provider> {
    let mut v = vec![];
    if crate::ingest::env_or_home("CLAUDE_CONFIG_DIR", &[".claude"]).iter().any(|p| p.exists()) {
        v.push(Provider::Claude);
    }
    if crate::providers::codex_home().exists() {
        v.push(Provider::Codex);
    }
    if cursor::available() {
        v.push(Provider::Cursor);
    }
    if grok::available() {
        v.push(Provider::Grok);
    }
    if copilot::available() {
        v.push(Provider::Copilot);
    }
    v
}

pub async fn poll(http: &reqwest::Client, p: Provider) -> Option<LimitReport> {
    Some(match p {
        Provider::Claude => claude::poll(http).await,
        Provider::Codex => codex::poll().await,
        Provider::Cursor => cursor::poll(http).await,
        Provider::Grok => grok::poll(http).await,
        Provider::Copilot => copilot::poll(http).await,
        _ => return None,
    })
}
