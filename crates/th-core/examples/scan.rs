//! Smoke test: `cargo run -p th-core --example scan -- [db-path] [--limits]`
use std::sync::Arc;
use th_core::{ingest::Ingestor, pricing::Pricing, providers, store::Store, summary};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db = args.get(1).filter(|a| !a.starts_with("--")).cloned().unwrap_or("/tmp/th-scan.db".into());
    let dir = std::path::Path::new(&db).parent().unwrap().to_path_buf();
    let store = Arc::new(Store::open(db.as_ref())?);
    let t = std::time::Instant::now();
    let n = Ingestor::new(store.clone(), providers::all_history_sources()).scan_all();
    println!("scanned: {n} events in {:?}", t.elapsed());
    let http = th_core::http_client();
    let pricing = match Pricing::load(&dir) { p if p.is_stale() => Pricing::refresh(&http, &dir).await?, p => p };
    for r in [summary::Range::Today, summary::Range::Week, summary::Range::All] {
        let s = summary::summarize(&store, &pricing, r)?;
        println!("\n== {:?}: {} tokens, ${:.2}{}", r, s.tokens.total, s.cost_usd, if s.cost_partial { " (partial)" } else { "" });
        for p in &s.providers {
            println!("  {:<12} {:>12} ${:>8.2}", p.label, p.tokens.total, p.cost_usd);
            for m in &p.models { println!("      {:<28} {:>12} {}", m.model, m.tokens.total, m.cost_usd.map(|c| format!("${c:.2}")).unwrap_or("$?".into())); }
        }
    }
    if args.iter().any(|a| a == "--cursor-events") {
        match th_core::limits::cursor::sync_events(&http, &store).await {
            Ok(n) => println!("cursor events synced: {n}"),
            Err(e) => println!("cursor events error: {e:#}"),
        }
        let s = summary::summarize(&store, &pricing, summary::Range::All)?;
        for p in s.providers.iter().filter(|p| p.label == "Cursor") {
            println!("  Cursor all-time {} tokens ${:.2}", p.tokens.total, p.cost_usd);
            for m in &p.models { println!("      {:<28} {:>12} {}", m.model, m.tokens.total, m.cost_usd.map(|c| format!("${c:.2}")).unwrap_or("$?".into())); }
        }
    }
    if args.iter().any(|a| a == "--limits") {
        for p in [th_core::Provider::Codex, th_core::Provider::Cursor] {
            let r = th_core::limits::poll(&http, p).await.unwrap();
            println!("\n{:?} plan={:?} stale={} err={:?} notes={:?}", p, r.plan, r.stale, r.error, r.notes);
            for w in r.windows { println!("  {:<12} {:>6.1}% resets={:?} {:?}", w.label, w.used_pct.unwrap_or(-1.0), w.resets_at, w.detail); }
        }
    }
    Ok(())
}
