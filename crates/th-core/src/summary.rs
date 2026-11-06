//! Aggregates for the UI.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{Duration, Local, NaiveTime, TimeZone};
use serde::Serialize;

use crate::model::Provider;
use crate::pricing::Pricing;
use crate::store::Store;

#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Range {
    Today,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
    All,
}

impl Range {
    /// (from_ms, days shown in the sparkline)
    fn bounds(self) -> (i64, i64) {
        let midnight = Local
            .from_local_datetime(&Local::now().date_naive().and_time(NaiveTime::MIN))
            .earliest()
            .unwrap_or_else(Local::now);
        let back = |d: i64| (midnight - Duration::days(d)).timestamp_millis();
        match self {
            Range::Today => (back(0), 7),
            Range::Week => (back(6), 7),
            Range::Month => (back(29), 30),
            Range::All => (0, 30),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Tokens {
    pub total: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Tokens {
    fn add(&mut self, i: u64, o: u64, cr: u64, cw: u64) {
        self.input += i;
        self.output += o;
        self.cache_read += cr;
        self.cache_write += cw;
        self.total += i + o + cr + cw;
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelSummary {
    pub model: String,
    pub tokens: Tokens,
    pub cost_usd: Option<f64>,
    pub events: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderSummary {
    pub provider: Provider,
    pub label: &'static str,
    pub tokens: Tokens,
    pub cost_usd: f64,
    pub cost_partial: bool,
    pub models: Vec<ModelSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DayApp {
    pub provider: Provider,
    pub label: &'static str,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DayPoint {
    pub day: String,
    pub tokens: u64,
    pub cost_usd: f64,
    /// Apps that spent tokens this day. Largest share of the window first,
    /// which is the bottom segment of the stacked bar.
    pub apps: Vec<DayApp>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub range: Range,
    pub tokens: Tokens,
    pub cost_usd: f64,
    pub cost_partial: bool,
    pub providers: Vec<ProviderSummary>,
    pub daily: Vec<DayPoint>,
    pub prices_updated: Option<i64>,
    pub generated_at: i64,
}

pub fn summarize(store: &Store, pricing: &Pricing, range: Range) -> Result<Summary> {
    let now = chrono::Utc::now().timestamp_millis() + 60_000;
    let (from, days) = range.bounds();

    // Sparkline window: fixed number of days ending today, zero-filled.
    let today = Local::now().date_naive();
    let spark_from = Local
        .from_local_datetime(&(today - Duration::days(days - 1)).and_time(NaiveTime::MIN))
        .earliest()
        .map(|d| d.timestamp_millis())
        .unwrap_or(0);
    struct DayAcc {
        tokens: u64,
        cost_usd: f64,
        apps: BTreeMap<String, (u64, f64)>,
    }
    let mut by_day: BTreeMap<String, DayAcc> = (0..days)
        .map(|i| {
            let d = (today - Duration::days(days - 1 - i)).format("%Y-%m-%d").to_string();
            (d, DayAcc { tokens: 0, cost_usd: 0.0, apps: BTreeMap::new() })
        })
        .collect();

    // provider → model → (tokens, cost, priced-all, events)
    struct Acc {
        tokens: Tokens,
        cost: f64,
        complete: bool,
        events: u64,
    }
    let mut acc: BTreeMap<String, BTreeMap<String, Acc>> = BTreeMap::new();
    let mut tokens = Tokens::default();

    for e in store.events_in(from.min(spark_from), now)? {
        let cost = pricing.cost(&e);
        if e.ts >= spark_from {
            let day = Local
                .timestamp_millis_opt(e.ts)
                .single()
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            if let Some(dp) = by_day.get_mut(&day) {
                let n = e.total();
                let c = cost.unwrap_or(0.0);
                dp.tokens += n;
                dp.cost_usd += c;
                let slot = dp.apps.entry(e.provider.clone()).or_insert((0, 0.0));
                slot.0 += n;
                slot.1 += c;
            }
        }
        if e.ts < from {
            continue;
        }
        tokens.add(e.input, e.output, e.cache_read, e.cache_write);
        let a = acc
            .entry(e.provider.clone())
            .or_default()
            .entry(e.model.clone().unwrap_or_else(|| "unknown".into()))
            .or_insert(Acc { tokens: Tokens::default(), cost: 0.0, complete: true, events: 0 });
        a.tokens.add(e.input, e.output, e.cache_read, e.cache_write);
        a.cost += cost.unwrap_or(0.0);
        a.complete &= cost.is_some();
        a.events += 1;
    }

    let mut providers: Vec<ProviderSummary> = vec![];
    let (mut cost_total, mut partial) = (0.0, false);
    for (prov, models) in acc {
        let Some(p) = Provider::parse(&prov) else { continue };
        let mut ps = ProviderSummary {
            provider: p,
            label: p.label(),
            tokens: Tokens::default(),
            cost_usd: 0.0,
            cost_partial: false,
            models: vec![],
        };
        for (model, a) in models {
            let t = &a.tokens;
            ps.tokens.add(t.input, t.output, t.cache_read, t.cache_write);
            ps.cost_usd += a.cost;
            ps.cost_partial |= !a.complete;
            ps.models.push(ModelSummary {
                model,
                cost_usd: (a.complete || a.cost > 0.0).then_some(a.cost),
                tokens: a.tokens,
                events: a.events,
            });
        }
        ps.models.sort_by(|a, b| b.tokens.total.cmp(&a.tokens.total));
        cost_total += ps.cost_usd;
        partial |= ps.cost_partial;
        providers.push(ps);
    }
    providers.sort_by(|a, b| b.tokens.total.cmp(&a.tokens.total));

    let mut app_totals: BTreeMap<String, u64> = BTreeMap::new();
    for day in by_day.values() {
        for (provider, (n, _)) in &day.apps {
            *app_totals.entry(provider.clone()).or_default() += *n;
        }
    }
    let daily = by_day
        .into_iter()
        .map(|(day, d)| {
            let mut apps: Vec<DayApp> = d
                .apps
                .into_iter()
                .filter_map(|(provider, (tokens, cost_usd))| {
                    if tokens == 0 {
                        return None;
                    }
                    let provider = Provider::parse(&provider)?;
                    Some(DayApp { label: provider.label(), provider, tokens, cost_usd })
                })
                .collect();
            apps.sort_by(|a, b| {
                let ta = app_totals.get(a.provider.as_str()).copied().unwrap_or(0);
                let tb = app_totals.get(b.provider.as_str()).copied().unwrap_or(0);
                tb.cmp(&ta).then(a.label.cmp(b.label))
            });
            DayPoint { day, tokens: d.tokens, cost_usd: d.cost_usd, apps }
        })
        .collect();

    Ok(Summary {
        range,
        tokens,
        cost_usd: cost_total,
        cost_partial: partial,
        providers,
        daily,
        prices_updated: pricing.updated,
        generated_at: chrono::Utc::now().timestamp(),
    })
}
