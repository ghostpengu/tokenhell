# Brief: real-time Cursor token tracking (esp. Cursor CLI + Grok)

## Context
TokenHell is a macOS menu-bar app (Rust/Tauri, repo: this folder) that shows tokens burned per AI tool.
Claude Code is "live" because it appends one JSONL line per finished API response with `usage`; we tail it via FSEvents.
Cursor has no such local log. Today we pull `POST https://cursor.com/api/dashboard/get-filtered-usage-events`
every 15 min (see `crates/th-core/src/limits/cursor.rs`), auth = `WorkosCursorSessionToken=<userId>%3A%3A<jwt>` cookie.

## Observed problem (verified 2026-11-06)
- User runs the Cursor CLI (`cursor-agent` / `agent`, ~/.local/bin) with model `grok-4.7` (maxMode).
- Billing is "Grok Bot Plan" / `SAND_BILLING_BRAND_GROK`, unified billing, plan "Premium".
- `get-filtered-usage-events` returns NOTHING after 2026-10-13 (older rows: model `grok-bot-default`,
  kind `USAGE_EVENT_KIND_CUSTOM_SUBSCRIPTION`, with tokenUsage).
- Only live signal: `POST /api/dashboard/get-sand-usage-status` → `usagePercent` (weekly).
- CLI local chat stores `~/.cursor/chats/<hash>/<uuid>/store.db` (tables `blobs`, `meta`) contain no obvious
  structured token counts (scanned for inputTokens/outputTokens/cacheRead: none).

## Questions to answer
1. Is there ANY source of per-request token counts for Cursor CLI / Grok-in-Cursor usage? Candidates:
   - other cursor.com / api2.cursor.sh dashboard endpoints (sand/grok-specific usage, aggregated usage,
     unified-billing "credits" endpoints)
   - the CLI's own data: `~/.cursor/chats/**/store.db` blob format (protobuf? JSON?), `~/.cursor/cli-config.json`,
     statsig cache, logs, `~/.cursor/projects`, `~/.cursor/ai-tracking/ai-code-tracking.db`
   - `cursor-agent -p --output-format stream-json`: does it include usage/token fields per turn?
     (Interactive sessions matter most, but note this.)
   - the CLI JS bundle itself (find where it's installed, grep for usage/tokenUsage/inputTokens fields it gets from the server)
   - Cursor desktop app local DBs (`~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`,
     workspaceStorage) for composer/agent token counts
2. How fast does each source update after a request completes (seconds? minutes?), and is there server-side lag in
   `get-filtered-usage-events` for normal (non-Grok) models?
3. Does `usagePercent` in `get-sand-usage-status` move per request, and can it be converted to tokens/$?
4. Recommendation: the best way for TokenHell to show Cursor usage near-real-time, ranked by reliability.

## Rules
- Read-only. Do NOT modify repo source files or any Cursor files. Copy DBs to /tmp before opening if they may be locked.
- NEVER print, log or write tokens, cookies, API keys, or Keychain secrets. Key names and shapes only.
- Small authenticated read requests to cursor.com dashboard endpoints are OK; do not change any settings.
- Cite sources (URLs, file paths, line refs) for every claim; mark each as verified vs. inferred.
- Write findings to `research/FINDINGS-codex.md` (Codex) or `research/FINDINGS-cursor.md` (Cursor). Keep it concise.
- Do not start other agents.
