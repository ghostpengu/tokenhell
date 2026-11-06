mod claude;
mod codex;
mod grok;
mod others;

pub use claude::Claude;
pub use codex::{Codex, codex_home};
pub use grok::{GrokBuild, GrokCliDb, grok_home};
pub use others::{Gemini, Kimi, OpenCode, Pi, Qwen};

use crate::ingest::HistorySource;

pub fn all_history_sources() -> Vec<Box<dyn HistorySource>> {
    vec![
        Box::new(Claude),
        Box::new(Codex),
        Box::new(GrokBuild),
        Box::new(GrokCliDb),
        Box::new(Gemini),
        Box::new(Pi),
        Box::new(Qwen),
        Box::new(Kimi),
        Box::new(OpenCode),
    ]
}
