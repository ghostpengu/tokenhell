# Research brief: TokenHell

We are planning **TokenHell**: a lightweight macOS menu-bar app (Rust + Tauri v2) that tracks
token usage across every AI coding tool / provider the user has, shows how many tokens (and $)
were burned, and shows current plan limits / rate-limit windows and when they reset.
UI: lives in the macOS top bar (tray icon + popover panel), clean "Liquid Glass" look (macOS 26+).

Your job: RESEARCH ONLY. Do not scaffold or write app code. Use web search plus read-only inspection
of this machine. Write findings to `research/FINDINGS.md` in this folder.

## Questions to answer

### 1. Data sources per tool (most important)
For each tool below, find: where usage data lives (local file paths + format, or API endpoint),
which fields hold input/output/cache-read/cache-write/reasoning tokens, model name, timestamps,
how to dedupe, and whether current limits / rate-limit windows (5h / weekly / monthly, % used,
reset time) are obtainable and how (local files, undocumented endpoints, OAuth tokens in Keychain, etc.).

- Claude Code (`~/.claude/projects/**/*.jsonl`, plus the OAuth usage/limits endpoint used by `/usage`)
- OpenAI Codex CLI (`~/.codex/sessions/**`, token_count events, rate_limits fields)
- Gemini CLI
- Cursor (local sqlite + any usage API)
- GitHub Copilot
- OpenCode, Amp, Factory Droid, Kimi / Qwen CLI, Pi — whichever have local logs
- Raw API keys: Anthropic Admin usage/cost API, OpenAI usage/costs API, OpenRouter credits API

You MAY inspect `~/.claude`, `~/.codex`, `~/.gemini`, `~/.cursor`, `~/Library/Application Support/*`
on this machine to confirm real formats. Show field names and a redacted example line.
NEVER print, copy, or write tokens, API keys, OAuth credentials, or prompt/message content —
only structure and key names.

### 2. Prior art
Survey existing tools that do this (e.g. ccusage, CodexBar, tokscale, Claude usage menubar apps,
etc.). For each: what it tracks, how it gets limit data, license, and what we can learn/reuse.
Note pricing data sources (e.g. LiteLLM model price JSON) for $ cost estimation.

### 3. Tauri v2 menu-bar + Liquid Glass
- Best current way to do a tray-only app in Tauri v2 (no Dock icon, ActivationPolicy::Accessory,
  popover window anchored under tray icon, hide on blur). Relevant plugins (tauri-plugin-positioner etc).
- Dynamic tray title text (e.g. show "1.2M" or "42%" next to the icon) and template icons.
- Liquid Glass on macOS 26+: NSGlassEffectView from Tauri (any crate/plugin like
  tauri-plugin-liquid-glass, window-vibrancy status), fallback to NSVisualEffectView vibrancy on older macOS.
- File watching (notify crate) + incremental JSONL parsing performance tips; storing aggregates (SQLite via rusqlite?).
- Keychain access from Rust (security-framework crate) for reading OAuth creds of CLIs, and any caveats.

## Output format for research/FINDINGS.md
- One section per question area, one subsection per tool.
- For each data source: path/endpoint, format, key fields, limits availability (yes/no/how),
  reliability (official / undocumented / scraped), and links.
- End with a short "Recommended v1 scope" list ranked by value/effort.

Keep it factual, cite URLs. When done, also run:
`hc send t10 "codex research done: research/FINDINGS.md"` (if that command fails, just finish).
