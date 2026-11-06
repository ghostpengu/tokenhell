//! macOS Keychain reads via `/usr/bin/security`.
//! The approval dialog is attributed to `security`, so "Always Allow" survives our own rebuilds.
//! After a denial we stay quiet for a while instead of re-prompting on every background poll.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

const DENIAL_COOLDOWN: Duration = Duration::from_secs(30 * 60);

fn denials() -> &'static Mutex<HashMap<String, Instant>> {
    static D: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    D.get_or_init(Default::default)
}

/// Forget denials (e.g. on a user-initiated refresh).
pub fn reset_denials() {
    denials().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[derive(Debug, PartialEq)]
pub enum KeychainError {
    NotFound,
    Denied,
    Waiting,
}

impl std::fmt::Display for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeychainError::NotFound => write!(f, "Keychain item not found"),
            KeychainError::Denied => write!(f, "Keychain access denied (refresh to ask again)"),
            KeychainError::Waiting => write!(f, "waiting for Keychain approval"),
        }
    }
}
impl std::error::Error for KeychainError {}

#[derive(Default)]
struct Slots {
    /// Reads waiting on a user prompt; their eventual result lands in `ready`.
    inflight: std::collections::HashSet<String>,
    ready: HashMap<String, Result<String, bool>>, // Err(true) = not found, Err(false) = denied
}

fn slots() -> &'static Mutex<Slots> {
    static S: OnceLock<Mutex<Slots>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Read a generic password. If macOS is showing an approval dialog, the read keeps going in the
/// background (no duplicate prompts) and this returns `Waiting` until the user answers.
pub async fn generic_password(service: &str, account: Option<&str>) -> Result<String> {
    let key = format!("{service}\u{0}{}", account.unwrap_or(""));
    if let Some(t) = denials().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        if t.elapsed() < DENIAL_COOLDOWN {
            bail!(KeychainError::Denied);
        }
    }
    {
        let mut sl = slots().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = sl.ready.remove(&key) {
            return match r {
                Ok(p) => Ok(p),
                Err(true) => bail!(KeychainError::NotFound),
                Err(false) => bail!(KeychainError::Denied),
            };
        }
        if sl.inflight.contains(&key) {
            bail!(KeychainError::Waiting);
        }
        sl.inflight.insert(key.clone());
    }
    let (svc, acct, k) = (service.to_string(), account.map(str::to_string), key.clone());
    let task = tokio::spawn(async move {
        let mut cmd = tokio::process::Command::new("/usr/bin/security");
        cmd.args(["find-generic-password", "-s", &svc]);
        if let Some(a) = &acct {
            cmd.args(["-a", a]);
        }
        cmd.arg("-w");
        let res = match cmd.output().await {
            Ok(out) if out.status.success() => {
                Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
            }
            // 44 = errSecItemNotFound; anything else (cancel, denied, locked) → cool down.
            Ok(out) if out.status.code() == Some(44) => Err(true),
            _ => Err(false),
        };
        if res == Err(false) {
            denials().lock().unwrap_or_else(|e| e.into_inner()).insert(k.clone(), Instant::now());
        }
        let mut sl = slots().lock().unwrap_or_else(|e| e.into_inner());
        sl.inflight.remove(&k);
        sl.ready.insert(k, res);
    });
    // Unprompted reads finish in milliseconds; a prompt keeps running in the background.
    let _ = tokio::time::timeout(Duration::from_secs(8), task).await;
    let mut sl = slots().lock().unwrap_or_else(|e| e.into_inner());
    match sl.ready.remove(&key) {
        Some(Ok(p)) => Ok(p),
        Some(Err(true)) => bail!(KeychainError::NotFound),
        Some(Err(false)) => bail!(KeychainError::Denied),
        None => bail!(KeychainError::Waiting),
    }
}
