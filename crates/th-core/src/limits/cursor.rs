//! Cursor: plan usage (`/api/usage-summary`), weekly Grok-in-Cursor allowance (`get-sand-usage-status`)
//! and per-event token history (`get-filtered-usage-events`). All private dashboard endpoints.
//! Auth: desktop app JWT from `state.vscdb` → `WorkosCursorSessionToken=<userId>%3A%3A<jwt>` cookie.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde_json::{Value, json};

use crate::model::{LimitReport, LimitWindow, Provider, UsageEvent, j};
use crate::store::Store;

const BASE: &str = "https://cursor.com";

fn db_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
}

pub fn available() -> bool {
    db_path().is_file()
}

#[derive(Clone)]
struct Auth {
    cookie: String,
    exp: Option<i64>,
}

const SAFE_PREFIX: &str = "cursor-safe-storage:v1:";

fn read_raw_token() -> Result<String> {
    let conn = rusqlite::Connection::open_with_flags(db_path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("opening Cursor state.vscdb")?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    let raw: String = conn
        .query_row("SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'", [], |r| r.get(0))
        .map_err(|_| anyhow!("not signed in to Cursor"))?;
    Ok(raw.trim().trim_matches('"').to_string())
}

/// Chromium-style safe storage used by Cursor 3.27+ on macOS:
/// base64("v10" + AES-128-CBC(ciphertext)), key = PBKDF2-SHA1(keychain pw, "saltysalt", 1003, 16), IV = 16 spaces.
pub(crate) fn decrypt_safe_storage(b64: &str, password: &str) -> Result<String> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
    let data = base64::engine::general_purpose::STANDARD.decode(b64.trim())?;
    let Some(ct) = data.strip_prefix(b"v10") else { bail!("unknown Cursor token encryption version") };
    if ct.is_empty() || ct.len() % 16 != 0 {
        bail!("malformed encrypted Cursor token");
    }
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), b"saltysalt", 1003, &mut key);
    let plain = cbc::Decryptor::<aes::Aes128>::new(&key.into(), &[b' '; 16].into())
        .decrypt_padded_vec_mut::<Pkcs7>(ct)
        .map_err(|_| anyhow!("could not decrypt Cursor token"))?;
    Ok(String::from_utf8(plain)?)
}

async fn safe_storage_password() -> Result<String> {
    use super::keychain::{KeychainError, generic_password};
    for account in ["Cursor Key", "Cursor"] {
        match generic_password("Cursor Safe Storage", Some(account)).await {
            Ok(p) => return Ok(p),
            Err(e) if e.downcast_ref::<KeychainError>() == Some(&KeychainError::NotFound) => continue,
            Err(e) => return Err(e),
        }
    }
    bail!("Cursor Safe Storage key not found in Keychain")
}

fn cookie_from_jwt(jwt: &str) -> Result<Auth> {
    let payload = jwt.split('.').nth(1).ok_or_else(|| anyhow!("malformed Cursor token"))?;
    let claims: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('='))?,
    )?;
    let exp = claims.get("exp").and_then(Value::as_i64);
    if exp.is_some_and(|e| e < chrono::Utc::now().timestamp()) {
        bail!("Cursor login expired: open Cursor to refresh");
    }
    let sub = j::s(&claims, &["sub"]).ok_or_else(|| anyhow!("token has no subject"))?;
    let user = sub.rsplit('|').next().unwrap_or(sub);
    if !user.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("unexpected Cursor user id format");
    }
    Ok(Auth { cookie: format!("WorkosCursorSessionToken={user}%3A%3A{jwt}"), exp })
}

fn auth_cache() -> &'static std::sync::Mutex<Option<(String, Auth)>> {
    static C: std::sync::OnceLock<std::sync::Mutex<Option<(String, Auth)>>> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// Decrypted auth is cached per raw DB value until the JWT expires (no Keychain hit per poll).
async fn auth() -> Result<Auth> {
    let raw = read_raw_token()?;
    let now = chrono::Utc::now().timestamp();
    if let Some((r, a)) = auth_cache().lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if r == raw && a.exp.is_none_or(|e| e > now + 60) {
            return Ok(a);
        }
    }
    let jwt = match raw.strip_prefix(SAFE_PREFIX) {
        Some(b64) => decrypt_safe_storage(b64, &safe_storage_password().await?)?,
        None if raw.starts_with("cursor-safe-storage:") => bail!("unsupported Cursor token storage version"),
        None => raw.clone(),
    };
    let a = cookie_from_jwt(&jwt)?;
    *auth_cache().lock().unwrap_or_else(|e| e.into_inner()) = Some((raw, a.clone()));
    Ok(a)
}

async fn get(http: &reqwest::Client, a: &Auth, path: &str) -> Result<Value> {
    let resp = http
        .get(format!("{BASE}{path}"))
        .header("Cookie", &a.cookie)
        .header("Accept", "application/json")
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("{path} returned {}", resp.status());
    }
    Ok(resp.json().await?)
}

async fn post(http: &reqwest::Client, a: &Auth, path: &str, body: Value) -> Result<Value> {
    let resp = http
        .post(format!("{BASE}{path}"))
        .header("Cookie", &a.cookie)
        .header("Accept", "application/json")
        .header("Origin", BASE)
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("{path} returned {}", resp.status());
    }
    Ok(resp.json().await?)
}

pub async fn poll(http: &reqwest::Client) -> LimitReport {
    let a = match auth().await {
        Ok(a) => a,
        Err(e) => return LimitReport::error(Provider::Cursor, format!("{e:#}")),
    };
    let (summary, sand) = tokio::join!(
        get(http, &a, "/api/usage-summary"),
        post(http, &a, "/api/dashboard/get-sand-usage-status", json!({}))
    );
    let summary = match summary {
        Ok(s) => s,
        Err(e) => return LimitReport::error(Provider::Cursor, format!("{e:#}")),
    };
    parse(&summary, sand.ok().as_ref())
}

pub(crate) fn parse(s: &Value, sand: Option<&Value>) -> LimitReport {
    let mut r = LimitReport::new(Provider::Cursor);
    r.plan = j::s(s, &["membershipType"]).map(str::to_string);
    let reset = j::ts_s(s.get("billingCycleEnd"));
    let plan = j::get(s, &["individualUsage", "plan"]);
    if let Some(p) = plan.filter(|p| p.get("enabled").and_then(Value::as_bool) != Some(false)) {
        let used = j::f(p, &["used"]);
        let limit = j::f(p, &["limit"]).filter(|l| *l > 0.0);
        let pct = j::f(p, &["totalPercentUsed"]).or(match (used, limit) {
            (Some(u), Some(l)) => Some(u / l * 100.0),
            _ => None,
        });
        r.windows.push(LimitWindow {
            key: "monthly".into(),
            label: "plan".into(),
            used_pct: pct,
            resets_at: reset,
            detail: match (used, limit) {
                (Some(u), Some(l)) => Some(format!("${:.2} of ${:.0}", u / 100.0, l / 100.0)),
                _ => None,
            },
        });
    }
    if let Some(od) = j::get(s, &["individualUsage", "onDemand"]) {
        let used = j::f(od, &["used"]).unwrap_or(0.0);
        let limit = j::f(od, &["limit"]).filter(|l| *l > 0.0);
        if od.get("enabled").and_then(Value::as_bool) == Some(true) && (used > 0.0 || limit.is_some()) {
            r.windows.push(LimitWindow {
                key: "on_demand".into(),
                label: "on-demand".into(),
                used_pct: limit.map(|l| used / l * 100.0),
                resets_at: reset,
                detail: Some(match limit {
                    Some(l) => format!("${:.2} of ${:.0}", used / 100.0, l / 100.0),
                    None => format!("${:.2}", used / 100.0),
                }),
            });
        }
    }
    if let Some(sd) = sand {
        let has_limit = match sd.get("includedLimitZero").and_then(Value::as_bool) {
            Some(zero) => !zero,
            None => sd.get("hasNonZeroIncludedLimit").and_then(Value::as_bool).unwrap_or(false),
        };
        if has_limit {
            r.windows.push(LimitWindow {
                key: "grok_week".into(),
                label: "Grok · week".into(),
                used_pct: j::f(sd, &["usagePercent"]),
                resets_at: j::ts_s(sd.get("nextResetTimestampUtc")),
                detail: None,
            });
        }
    }
    r
}

/// Pull per-event token history into the store. Incremental from the newest stored event.
pub async fn sync_events(http: &reqwest::Client, store: &Store) -> Result<usize> {
    let a = auth().await?;
    let now = chrono::Utc::now().timestamp_millis();
    let watermark: i64 = store.kv_get("cursor.watermark")?.and_then(|s| s.parse().ok()).unwrap_or(0);
    // Re-fetch the last hour to catch late corrections.
    let start = if watermark > 0 { watermark - 3_600_000 } else { 0 };
    let mut events = vec![];
    let mut newest = watermark;
    let mut fetched = 0u64;
    for page in 1..=200 {
        let body = json!({"startDate": start.to_string(), "endDate": now.to_string(), "page": page, "pageSize": 500});
        let v = post(http, &a, "/api/dashboard/get-filtered-usage-events", body).await?;
        let rows = v.get("usageEventsDisplay").and_then(Value::as_array).cloned().unwrap_or_default();
        let total = v.get("totalUsageEventsCount").and_then(Value::as_u64);
        fetched += rows.len() as u64;
        for e in &rows {
            if let Some(ev) = event(e) {
                if ev.ts > 0 && ev.ts <= now + 300_000 {
                    newest = newest.max(ev.ts);
                }
                events.push(ev);
            }
        }
        let done = match total {
            Some(t) => fetched >= t,
            None => rows.len() < 500,
        };
        if rows.is_empty() || done {
            break;
        }
    }
    store.replace_virtual(Provider::Cursor, "cursor://usage-events", &events)?;
    store.kv_set("cursor.watermark", &newest.to_string())?;
    Ok(events.len())
}

/// One `usageEventsDisplay` row → event. Per FINDINGS-3:
/// - token buckets are disjoint (input is uncached), so they're summed as-is;
/// - cost = `tokenUsage.totalCents` (Cursor's model cost), else `chargedCents` for
///   request-based rows with no `tokenUsage`; `*_NOT_CHARGED` kinds cost $0 but keep tokens.
fn event(e: &Value) -> Option<UsageEvent> {
    let ts = j::ts_ms(e.get("timestamp"))?;
    let model = j::s(e, &["model"]).map(str::to_string);
    let empty = Value::Null;
    let t = e.get("tokenUsage").filter(|t| t.is_object()).unwrap_or(&empty);
    let (i, o, cr, cw) = (
        j::u(t, &["inputTokens"]),
        j::u(t, &["outputTokens"]),
        j::u(t, &["cacheReadTokens"]),
        j::u(t, &["cacheWriteTokens"]),
    );
    let cents = |v: &Value, k: &str| j::f(v, &[k]).filter(|c| c.is_finite() && *c >= 0.0);
    let not_charged = j::s(e, &["kind"]).is_some_and(|k| k.to_ascii_uppercase().replace([' ', ','], "_").contains("NOT_CHARGED"));
    let cost = if not_charged {
        Some(0.0)
    } else {
        cents(t, "totalCents").or_else(|| cents(e, "chargedCents")).map(|c| c / 100.0)
    };
    let key = match j::s(e, &["usageEventId"]) {
        Some(id) => format!("id:{id}"),
        None => format!(
            "t:{}:{ts}:{}:{i}:{o}:{cr}:{cw}",
            j::s(e, &["conversationId"]).unwrap_or(""),
            model.as_deref().unwrap_or("")
        ),
    };
    let ev = UsageEvent {
        dedup_key: key,
        session: j::s(e, &["conversationId"]).map(str::to_string),
        ts,
        model,
        input: i,
        output: o,
        cache_read: cr,
        cache_write: cw,
        cost_reported: cost,
        ..Default::default()
    };
    // Keep zero-token rows only when they carry a positive cost (request-based plans).
    (ev.total() > 0 || cost.is_some_and(|c| c > 0.0)).then_some(ev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};

    #[test]
    fn event_costs() {
        let tok = json!({"timestamp":"1790900000000","model":"m","usageEventId":"a","kind":"USAGE_EVENT_KIND_INCLUDED_IN_PRO",
            "chargedCents":9,"tokenUsage":{"inputTokens":"100","outputTokens":10,"cacheReadTokens":50,"totalCents":"4.457141876220703"}});
        let e = event(&tok).unwrap();
        assert_eq!((e.input, e.output, e.cache_read, e.total()), (100, 10, 50, 160));
        assert!((e.cost_reported.unwrap() - 0.04457141876220703).abs() < 1e-12);

        let req = json!({"timestamp":"1790900000000","model":"m","usageEventId":"b","isTokenBasedCall":false,"requestsCosts":1.4,"chargedCents":8});
        let e = event(&req).unwrap();
        assert_eq!((e.total(), e.cost_reported), (0, Some(0.08)));

        let err = json!({"timestamp":"1790900000000","model":"m","usageEventId":"c","kind":"USAGE_EVENT_KIND_ERRORED_NOT_CHARGED",
            "tokenUsage":{"inputTokens":5,"totalCents":3}});
        assert_eq!(event(&err).unwrap().cost_reported, Some(0.0));
        let aborted = json!({"timestamp":"1790900000000","usageEventId":"d","kind":"Aborted, Not Charged","tokenUsage":{"outputTokens":5,"totalCents":2}});
        assert_eq!(event(&aborted).unwrap().cost_reported, Some(0.0));

        let nothing = json!({"timestamp":"1790900000000","usageEventId":"e","tokenUsage":{"totalCents":-1}});
        assert!(event(&nothing).is_none());
    }

    #[test]
    fn safe_storage_roundtrip() {
        let pw = "hunter2";
        let mut key = [0u8; 16];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(pw.as_bytes(), b"saltysalt", 1003, &mut key);
        let ct = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &[b' '; 16].into())
            .encrypt_padded_vec_mut::<Pkcs7>(b"header.payload.sig");
        let mut blob = b"v10".to_vec();
        blob.extend(ct);
        let b64 = base64::engine::general_purpose::STANDARD.encode(blob);
        assert_eq!(decrypt_safe_storage(&b64, pw).unwrap(), "header.payload.sig");
        assert!(decrypt_safe_storage(&b64, "wrong").is_err() || decrypt_safe_storage(&b64, "wrong").unwrap() != "header.payload.sig");
    }
}
