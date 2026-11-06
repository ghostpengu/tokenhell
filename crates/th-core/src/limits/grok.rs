//! SuperGrok / Grok Build subscription credits via `cli-chat-proxy.grok.com/v1/billing?format=credits`
//! using the Grok CLI's own OAuth token (`~/.grok/auth.json`). Never refreshed or written back.

use anyhow::{Result, anyhow, bail};
use serde_json::Value;

use crate::model::{LimitReport, LimitWindow, Provider, j};

/// Find an access token in auth.json without assuming the exact layout.
fn token() -> Option<String> {
    let text = std::fs::read_to_string(crate::providers::grok_home().join("auth.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    fn find(v: &Value, depth: u8) -> Option<String> {
        if depth > 4 {
            return None;
        }
        let obj = v.as_object()?;
        for k in ["access_token", "accessToken", "token", "id_token"] {
            if let Some(s) = obj.get(k).and_then(Value::as_str) {
                if s.len() > 20 {
                    return Some(s.to_string());
                }
            }
        }
        obj.values().find_map(|x| find(x, depth + 1))
    }
    find(&v, 0)
}

pub fn available() -> bool {
    crate::providers::grok_home().join("auth.json").is_file()
}

pub async fn poll(http: &reqwest::Client) -> LimitReport {
    match fetch(http).await {
        Ok(r) => r,
        Err(e) => LimitReport::error(Provider::Grok, format!("{e:#}")),
    }
}

async fn fetch(http: &reqwest::Client) -> Result<LimitReport> {
    let t = token().ok_or_else(|| anyhow!("no Grok CLI login (~/.grok/auth.json)"))?;
    let resp = http
        .get("https://cli-chat-proxy.grok.com/v1/billing?format=credits")
        .bearer_auth(&t)
        .header("x-xai-token-auth", "xai-grok-cli")
        .header("Accept", "application/json")
        .send()
        .await?;
    let st = resp.status();
    if st.as_u16() == 401 || st.as_u16() == 403 {
        bail!("not authorised: run `grok` once to refresh login");
    }
    if !st.is_success() {
        bail!("billing endpoint returned {st}");
    }
    Ok(parse(&resp.json().await?))
}

pub(crate) fn parse(v: &Value) -> LimitReport {
    let mut r = LimitReport::new(Provider::Grok);
    let c = v.get("config").unwrap_or(v);
    r.plan = j::s(c, &["subscriptionTier"]).or(j::s(v, &["subscriptionTier"])).map(str::to_string);
    let used = j::f(c, &["onDemandUsed", "val"]);
    let cap = j::f(c, &["onDemandCap", "val"]).filter(|c| *c > 0.0);
    let pct = j::f(c, &["creditUsagePercent"]).or(match (used, cap) {
        (Some(u), Some(cp)) => Some(u / cp * 100.0),
        _ => None,
    });
    let reset = j::ts_s(j::get(c, &["currentPeriod", "end"]).or(c.get("billingPeriodEnd")));
    if pct.is_some() || reset.is_some() {
        r.windows.push(LimitWindow {
            key: "credits".into(),
            label: "credits".into(),
            used_pct: pct,
            resets_at: reset,
            detail: None,
        });
    }
    if let Some(b) = j::f(c, &["prepaidBalance", "val"]) {
        r.notes.push(format!("${:.2} prepaid", b / 100.0));
    }
    r
}
