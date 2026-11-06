# Task: live Cursor CLI token tracking via a Cursor `stop` hook

Read first: `research/FINDINGS-codex.md` and `research/FINDINGS-cursor.md` (both verified the hook payload shape
in CLI build `~/.local/share/cursor-agent/versions/2026.11.05-86047df`).

## Goal
Cursor CLI usage (user runs `grok-4.7` on the Grok Bot Plan) must show up in TokenHell within seconds of each
turn finishing, the way Claude Code does today. Cursor's server ledger has no rows for this usage, so the hook is
the only source.

## Build
1. **Hook script** (shipped in the repo, e.g. `hooks/cursor-usage-hook.sh` or a tiny Rust bin; pick the simplest
   robust option, no Python/Node dependency assumptions beyond macOS defaults):
   - Reads the hook JSON from stdin. On the `stop` event only (do not also record `afterAgentResponse`: double count).
   - Appends ONE JSONL line to `~/Library/Application Support/io.github.ghostpengu.tokenhell/cursor-hook/usage.jsonl`
     (create the dir): `ts` (ms, now), `conversation_id`, `generation_id`, `model`, `status`, raw `input_tokens`,
     `output_tokens`, `cache_read_tokens`, `cache_write_tokens`. Missing counts → omit/null, never invent.
   - NEVER write prompt or response text, file paths from the payload beyond IDs, or any secret.
   - Must exit 0 quickly, print `{}` to stdout, and never block or break the agent, even on malformed input.
2. **Installer**: `~/.cursor/hooks.json` does not exist yet. Write it per the CLI's hook format (verify the exact
   schema in the installed bundle `6139.index.js` / Cursor hooks docs https://cursor.com/docs/hooks). If a file exists
   at install time, merge and preserve existing hooks; back it up first. Install the script to a stable path outside
   the repo build dir (e.g. `~/Library/Application Support/io.github.ghostpengu.tokenhell/bin/`). Provide this as a
   script (`scripts/install-cursor-hook.sh`) and run it once for this user.
3. **Ingest in th-core**: new `HistorySource` (see `crates/th-core/src/providers/mod.rs`, `ingest.rs`,
   `claude.rs` for the pattern) for `Provider::Cursor` that tails `usage.jsonl` incrementally.
   - Buckets are disjoint in TokenHell: `input = max(input_tokens - cache_read_tokens - cache_write_tokens, 0)`.
   - `dedup_key = "hook:<conversation_id>:<generation_id>"`.
   - `cost_reported`: none (no cost in payload). Price via the normal pricing table if `grok-4.7` resolves; if it does
     not, leave cost unknown rather than fabricate. Check `pricing.rs` for how unknown models are shown.
   - Must not double-count with `cursor://usage-events` (server sync). Those have different keys and there are no
     server rows after 2026-10-13, so overlap is not expected now; add a short comment about it.
   - Unit tests: inclusive-input normalization, missing fields, malformed lines, dedup.
4. Register the source in `all_history_sources()`. The FSEvents watcher should then pick up appends live; confirm the
   watcher covers the new directory (see `ingest.rs` `watch`).

## Verify
- `cargo test -p th-core` and `cargo build` for the workspace pass.
- Pipe a sample stop payload into the installed hook and confirm one correct JSONL line.
- Do NOT run a live `cursor-agent` turn yourself; tell the user when it's ready so they run one short CLI turn, then
  confirm the line appears and that `cargo run -p th-core --example scan -- /tmp/th-hook.db` shows Cursor tokens today.

## Rules
- The working tree has unrelated uncommitted changes from another session (reset-credits feature: `workers.rs`,
  `lib.rs`, `tray.rs`, `codex.rs`, `model.rs`, `summary.rs`, UI files). Do not revert or reformat them; touch only
  what this task needs.
- No commits. No secrets in output or files. No subagents.
- When done (or stuck), run: `hc send t1d 'cursor hook: <done|stuck> - <one line>'`
