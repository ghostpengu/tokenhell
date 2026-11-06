# TokenHell: plan

A small macOS menu-bar app (Rust + Tauri v2) that shows how many tokens you burned across every
AI coding tool, roughly what that would cost at API prices, and how close you are to each plan's limits.
It lives in the top bar next to the system icons and opens a Liquid Glass panel when clicked.

Research source: `research/FINDINGS.md` (Codex, 2026-11-06).

---

## 1. Goals / non-goals

**Goals**
- Lives only in the menu bar: no Dock icon, no main window. Starts at login.
- Under 15 MB bundle, under 40 MB RAM when idle, close to 0% CPU when idle (file events, no busy polling).
- Token totals for today, the last 7 days, and the last 30 days, broken down by tool and by model, with an estimated $ cost.
- Live limit windows (5h / weekly / monthly), each with its % used and reset countdown.
- Local-first. Message content is never stored and nothing is uploaded.

**Non-goals (v1)**
- Windows/Linux (keep the core crate portable, but only ship macOS).
- Exact invoices. $ figures are labelled "API-equivalent estimate".
- Team dashboards, leaderboards, or cloud sync.

---

## 2. Architecture

```
tokenhell/
├─ crates/
│  ├─ th-core/          # pure Rust: no Tauri dep, unit-testable
│  │  ├─ model.rs       # UsageEvent, LimitWindow, Provider ids
│  │  ├─ store.rs       # rusqlite: events, checkpoints, aggregates, limit snapshots
│  │  ├─ pricing.rs     # LiteLLM price map (bundled + weekly refresh), cost calc
│  │  ├─ ingest.rs      # notify watcher, debounce, incremental tail reader
│  │  └─ providers/
│  │     ├─ mod.rs      # trait HistorySource, trait LimitSource
│  │     ├─ claude.rs   # ~/.claude/projects/**/*.jsonl + OAuth /api/oauth/usage
│  │     ├─ codex.rs    # ~/.codex/sessions/** + `codex app-server` rateLimits
│  │     ├─ gemini.rs   # ~/.gemini/tmp/*/chats/*.json (+ quota, opt-in)
│  │     └─ ...         # pi, opencode, copilot, cursor, kimi, qwen, droid, amp
│  └─ th-app/           # Tauri v2 shell
│     ├─ src/main.rs    # tray, panel window, activation policy, glass
│     ├─ src/commands.rs# invoke handlers: get_summary, get_limits, settings
│     └─ tauri.conf.json
└─ ui/                  # Svelte 5 + Vite + TS (tiny runtime, no UI kit)
   ├─ Panel.svelte
   ├─ components/ (Gauge, ProviderRow, Sparkline, ResetTimer)
   └─ glass.css
```

**Flow:** watcher → adapter parses new bytes → normalised `UsageEvent`s → SQLite (one transaction
per batch, which also commits the file offset) → aggregates updated → `emit("usage-updated")` → UI re-queries.
Limit pollers run on their own timers and write `LimitSnapshot`s → `emit("limits-updated")` → tray title refresh.

### Core traits

```rust
trait HistorySource {
    fn id(&self) -> ProviderId;
    fn roots(&self) -> Vec<PathBuf>;              // honours CLAUDE_CONFIG_DIR, CODEX_HOME, ...
    fn parse(&self, file: &FileCtx, bytes: &[u8]) -> ParseResult; // complete lines only
}

#[async_trait]
trait LimitSource {
    fn id(&self) -> ProviderId;
    fn requires_consent(&self) -> bool;           // keychain/OAuth access
    async fn poll(&self) -> Result<Vec<LimitWindow>, LimitError>;
}
```

### Data model (SQLite, `~/Library/Application Support/TokenHell/th.db`)

| table | key columns |
|---|---|
| `events` | `provider, dedup_key (UNIQUE), ts, model, input, output, cache_read, cache_write, reasoning, source_file_id` |
| `files` | `id, path, inode, size, mtime, offset, parser_version` |
| `daily_agg` | `day, provider, model` → token sums, est_cost |
| `limit_snapshots` | `provider, window (5h/7d/7d_opus/monthly/credits), used_pct, resets_at, observed_at, error` |
| `prices` | `model, input/output/cache_read/cache_write per Mtok, source, fetched_at` |

`dedup_key` is built per provider. Inserts use `INSERT .. ON CONFLICT DO UPDATE`, so the latest snapshot replaces earlier ones and never gets added twice.

---

## 3. Providers: v1 vs later

| Provider | History (local) | Limits | v1? |
|---|---|---|---|
| **Claude Code** | `~/.claude/projects/**/*.jsonl`; `message.usage.*`; dedup on `(requestId, message.id)`, keeping the last record | `GET api.anthropic.com/api/oauth/usage` (`five_hour`, `seven_day`, `seven_day_opus`) using the token from Keychain item `Claude Code-credentials`. **Opt-in** | ✅ |
| **Codex CLI** | `~/.codex/sessions/YYYY/MM/DD/*.jsonl` `token_count` events. Use **deltas** of the cumulative `total_token_usage` per session; the model comes from `turn_context` | Live: spawn `codex app-server` and call JSON-RPC `account/rateLimits/read`. Fallback: last `rate_limits` snapshot in the session file (shown as stale) | ✅ |
| Gemini CLI | `~/.gemini/tmp/*/chats/*.json`; `tokens.{input,output,cached,thoughts}`; files are snapshots | `cloudcode-pa…:retrieveUserQuota` (undocumented, opt-in) | v1.1 |
| Pi | `~/.pi/agent/sessions/**/*.jsonl` | n/a | v1.1 |
| OpenCode | `~/.local/share/opencode/opencode.db` (read-only, WAL) | n/a | v1.2 |
| Copilot | `~/.copilot/otel/*.jsonl` / sqlite | `api.github.com/copilot_internal/user` | v1.2 |
| Cursor | no reliable local ledger | `cursor.com/api/usage-summary` | later |
| Droid / Kimi / Qwen / Amp | local JSONL (formats need checking) | various private APIs | later |
| Anthropic / OpenAI Admin, OpenRouter | n/a | official usage/cost/credits APIs (admin key, opt-in) | later |

**Rules carried over from the research:**
- Local token totals and account limits are **separate cards**. Don't derive a % from local tokens.
- `null`, stale, unauthorised, and `0` each look different in the UI.
- Never refresh or rewrite another app's OAuth credentials. Read the token only, and if it has expired show "open Claude Code to refresh".
- Keychain reads happen only after the user turns a provider on, never silently in the background.

---

## 4. UI

### Menu bar item
- Monochrome **template icon**: a small flame, so it recolours with the system like the moon and mouse icons in your screenshot.
- Optional title next to it (`TrayIcon::set_title`). The user picks one mode:
  - `icon only` · `today's tokens  1.2M` · `highest limit  72%` · `today's $  $4.10`
- Title updates are throttled to at most one every 5 s, and the text is padded to a fixed width so the menu bar doesn't jump around.
- At 80% or more on any limit the icon becomes a filled variant. At 95% or more it shows a red dot (the one place colour is allowed).

### Panel (click the tray icon)
A 340×480 borderless panel, centred under the icon (`tauri-plugin-positioner::TrayCenter`). It hides when it loses focus.

```
╭──────────────────────────────────────╮
│  TokenHell            Today ▾    ⚙   │
│                                      │
│   3.42M tokens        ≈ $11.80       │
│   ▁▂▂▅▃▇█▅  last 7 days              │
│ ──────────────────────────────────── │
│  LIMITS                              │
│  Claude   5h    ███████░░░  72%  1h12│
│           week  ███░░░░░░░  31%  4d  │
│  Codex    5h    ██░░░░░░░░  18%  3h40│
│           week  █████░░░░░  49%  2d  │
│ ──────────────────────────────────── │
│  BY TOOL                             │
│  ◉ Claude Code   2.10M   $8.20       │
│  ◉ Codex         1.20M   $3.40       │
│  ◉ Gemini        0.12M   $0.20       │
│                                      │
│  updated 12s ago              Quit   │
╰──────────────────────────────────────╯
```
- Clicking a tool row expands it to show per-model splits (input / output / cache) for that tool.
- Range switcher: Today · 7d · 30d · All.
- Settings is a second view in the same panel, not another window. It has provider toggles and consent, the title mode, launch at login, and the refresh interval.

### Liquid Glass look
- `window-vibrancy` 0.7+: `apply_liquid_glass()` on macOS 26 and later, falling back to `apply_vibrancy(NSVisualEffectMaterial::Popover)` on older versions.
- Window settings: `transparent: true`, `decorations: false`, `shadow: true`, `macOSPrivateApi: true` (we're not targeting the App Store, so that's acceptable).
- CSS: transparent `body`; SF Pro via `-apple-system`; 13 px base; numbers in `font-variant-numeric: tabular-nums`; hairline separators `rgba(255,255,255,.08)`; 16 px corner radius matching the system panel; no hard backgrounds.
- Follow `prefers-reduced-transparency` (solid fallback) and `prefers-color-scheme`.
- Gauges are thin 4 px capsules with a soft tint (green → amber → red). Everything else stays monochrome.
- ⚠ Known issue: window-vibrancy #229 reports packaged WebKit crashes on one macOS 27 setup. Test a **packaged** build on this Mac in week 1, and keep `tauri-plugin-liquid-glass` / a small objc2 `NSGlassEffectView` bridge as plan B.

---

## 5. Tauri shell specifics

- `ActivationPolicy::Accessory` + `LSUIElement=true` → no Dock icon.
- `TrayIconBuilder` with `icon_as_template(true)`. Left click toggles the panel; right click opens a small native menu (Refresh, Settings, Quit).
- The panel is created hidden at startup so the first click opens it instantly.
- `WindowEvent::Focused(false)` → hide (short grace period, ignore while a native dialog is up).
- Plugins: `positioner`, `autostart`, `single-instance`, `store` (settings), `updater` (later).
- Heavy work runs on a tokio task, never on the main thread.

---

## 6. Ingestion details

- **Startup:** full scan of the roots, resuming from each file's stored offset (cheap after the first run). The first run shows a progress line ("indexing 1,240 sessions…").
- **Live:** `notify` recursive FSEvents, debounced 500 ms. Read from the offset, parse **only complete newline-terminated lines**, keep the partial tail.
- **File changed underneath us** (inode changed, size < offset, parser version bumped): delete that file's events and re-parse it.
- **JSON snapshot / SQLite sources** (Gemini, OpenCode): re-read and upsert. Never assume they're append-only.
- **Pricing:** bundle a LiteLLM `model_prices_and_context_window.json` snapshot and refresh it weekly. Unknown model → `$ ?`, never `$0`.
- **Limit polling:** Claude every 60 s while the panel is open and every 5 min otherwise, with backoff on errors. Codex app-server stays running as a child process while that provider is enabled.

---

## 7. Privacy & security
- Store only numbers, model ids, timestamps, and hashed dedup keys. Never prompts, paths inside messages, or credentials.
- Credentials are read on demand, held only in memory, and never logged.
- No network access except: limit endpoints for providers the user enabled, and the price-map refresh (can be turned off).
- Signed and notarised build (Keychain ACL prompts are tied to the code signature, so signing must be stable).

---

## 8. Milestones

| # | Deliverable | Est. |
|---|---|---|
| M0 | Workspace scaffold, Tauri tray app, Accessory policy, glass panel spike **in a packaged build on macOS 27** | 1 d |
| M1 | `th-core`: store, ingest engine, Claude + Codex history adapters with fixture tests (dedup, deltas, truncation) | 2–3 d |
| M2 | Pricing + aggregates, `get_summary` command, panel UI (totals, sparkline, by-tool) | 2 d |
| M3 | Limits: Codex app-server + snapshot fallback, Claude OAuth (opt-in, Keychain consent) and gauges + countdowns | 2 d |
| M4 | Tray title modes, threshold states, settings view, autostart, single instance | 1 d |
| M5 | Gemini + Pi adapters, polish, signing/notarisation, DMG | 2 d |
| later | OpenCode, Copilot, Cursor, Droid/Kimi/Qwen/Amp, admin API keys, notifications at 90% | — |

---

## 9. Open questions for you
1. Frontend: **Svelte 5** (recommended: smallest runtime) or plain TS / React?
2. Which tools do you actually use daily besides Claude Code + Codex? That decides the v1.1 order.
3. Default tray title: icon only, tokens today, or highest limit %?
4. Distribution: just for you (ad-hoc signing is fine) or public (needs a Developer ID and notarisation)?
