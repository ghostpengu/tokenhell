# Cursor real-time usage (CLI + Grok)

Research date: 2026-11-06. Read-only. Account is the signed-in desktop user: unified billing, `membershipType` `premium`, `billingBrand` `SAND_BILLING_BRAND_GROK`, `grokPlanLabel` `Grok Bot Plan`, CLI model `grok-4.7` with max mode (`~/.cursor/cli-config.json`). CLI build `2026.11.05-86047df` at `~/.local/share/cursor-agent/versions/2026.11.05-86047df`. Desktop app `3.29.9` (`/Applications/Cursor.app/Contents/Resources/app/product.json`). No `cursor-agent -p` run (that would spend the allowance). No secrets are recorded here.

## Answers

1. **Per-request token counts for current Grok-in-Cursor / CLI usage exist only on the live agent stream.** They are not in any local database, and they are not in `get-filtered-usage-events` after 2026-10-13. The stream message is `TurnEndedUpdate` (turn totals) and `ModelCallCompletedUpdate` (one model call). Print mode writes them to stdout. Interactive mode keeps them in memory and passes them to hooks if a hook is configured. Nothing on disk records them.
2. **Stream counts arrive when that turn or model call finishes** (same RPC stream; debug logs show `interactionUpdate:turnEnded` as the stream ends). **`usagePercent` is a weekly meter that already includes today, but it did not tick during a few minutes of continued CLI use.** **Filtered usage events are not lagging by minutes:** the newest row is still `2026-10-13T09:49:35.279Z` while CLI activity is in `ai-code-tracking.db` through `2026-11-06T21:07:28Z`.
3. **`usagePercent` is used-percent of the weekly Grok allowance** (same number as `GetCurrentPeriodUsage.planUsage.totalPercentUsed`). This payload has no token totals and no included-cents cap, so it cannot be converted to tokens or dollars. Daily `GetGrokBotUsageBreakdown` ppm rows also do not equal that percent.
4. **Keep the weekly percent for the Grok ring. Get per-request tokens from the CLI stream (print stdout or an opt-in hook). Keep polling filtered events only for history through 2026-10-13.**

## 1. What each source actually contains

### Agent stream (the per-request source)

Verified in `1706.index.js` and the protobuf schemas embedded in `index.js` of the CLI build above.

`TurnEndedUpdate` fields: `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens`, `ended_at_ms`, optional `estimated_cost` (`raw_cost_cents`, `charged_cents`, `source`). `ModelCallCompletedUpdate` has the five token counts plus `model_call_id`. `EstimatedCostSource` is `UNSPECIFIED | CATALOG | USAGE`.

**`--print` stdout** (`async function R` in `1706.index.js`):

- `--output-format json` and `stream-json` put the turn totals on the final `{type:"result", usage}` object when `turnEnded` carried any of the four main counts.
- `stream-json` (and `--stream-partial-output`) also writes one line per model call: `{type:"usage", subtype:"model_call", model_call_id, usage}`. Plain `json` does not.
- `text` prints the answer only. It still computes usage internally and does not write it.
- Before those objects are serialized, `function $(e)` **replaces `inputTokens` with `max(inputTokens - cacheReadTokens - cacheWriteTokens, 0)`** and leaves the cache fields in place. `reasoningTokens` is copied when non-zero. `estimated_cost` is not copied. Summing `model_call` lines and `result.usage` double-counts; `turnEnded` replaces the result usage rather than adding another copy. A background-completion combiner in the same function does add token objects together.

**Interactive CLI** (separate handler in the same file): on `turnEnded` it stores the raw four counts, then builds a second object `{input_tokens: max(input - cacheRead - cacheWrite, 0), output_tokens, cache_creation_input_tokens, cache_read_input_tokens}` for the on-screen usage state. `reasoning_tokens` are not copied there. `afterAgentResponse` and `stop` hooks receive the **raw** counts as `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens` (`index.js` hook step, event names `afterAgentResponse` and `stop`). Hook paths, verified in `6139.index.js`: `~/.cursor/hooks.json`, `<workspace>/.cursor/hooks.json`, and `/Library/Application Support/Cursor/hooks.json`. None of those files exist here.

The subtraction is the CLI's own reading of the stream field: it treats `input_tokens` as including cache read and cache write. That is the opposite of the billing-ledger meaning of `tokenUsage.inputTokens` (uncached) documented for usage events. These are different messages. No live numeric payload was captured, so which reading the server actually uses is inferred from the client, not measured.

The status-line object in the same file sets `context_window.total_input_tokens` to `round(used_percentage / 100 * context_window_size)`. That is context-window fill. `total_output_tokens` is an in-memory counter. `cli-config.json` has no status-line command, and this object is not written to a file.

### Dashboard ledgers

Auth used: desktop `cursorAuth/accessToken` in `state.vscdb` (`cursor-safe-storage:v1:`, 603 chars), decrypted with the `Cursor Safe Storage` / `Cursor Key` keychain item, sent as the `WorkosCursorSessionToken` cookie and as `Authorization: Bearer` to `api2.cursor.sh`. Calls were POSTs with empty or date-bounded bodies. Responses below are from 2026-11-06 ~21:15–21:17Z.

**`POST https://cursor.com/api/dashboard/get-filtered-usage-events`** and the same Connect method `aiserver.v1.DashboardService/GetFilteredUsageEvents` (identical ledger):

- `totalUsageEventsCount` **171**. Page 1 is newest-first. Newest row `2026-10-13T09:49:35.279Z`, model `grok-bot-default`, kind `USAGE_EVENT_KIND_CUSTOM_SUBSCRIPTION`, `usageType` `included`, `customSubscriptionName` `free`, `isTokenBasedCall` true, `isHeadless` false, `usageBasedCosts` `"-"`.
- `tokenUsage` counts are JSON numbers. On the newest row: input 4, output 5, cache write 993, no cache-read field, `totalCents` 0.2024…, `chargedCents` equal to that, `cursorTokenFee` 0. The next row has cache read 41146 as well.
- `startDate` `1791936000000` (2026-10-14T00:00:00Z) through now returned HTTP 200 and `{}`. No rows after 13 Oct.
- `clientType` `cli`, `ide`, `glass`, `agent`, and `desktop` each returned `{}`. Returned rows do not include `clientType`.
- Page 2 (71 rows) ends at `2026-09-15T08:44:31.537Z` and includes 69 `grok-bot-default` plus 2 `claude-3.5-sonnet`. Older history exists; it is not the current Grok Bot Plan traffic.
- TokenHell's `tokenhell.db` has 168 `cursor` / `grok-bot-default` events, max timestamp the same `1791884975279`. The server has three more rows than the local store.

**`GetAggregatedUsageEvents`** (api2, and the cursor.com route for the post-Oct-14 window): HTTP 200 body is only `{"startDate": ...}` for this week (starting `2026-11-03T09:55:11.127Z`), for November, for 2026-09-15..10-14 (a window that does contain the 171 events), and for `useMonthlyBillingCycleStart: true` (echoed start `2026-10-09T09:21:39.864Z`). No `aggregations` and no token totals. Combining `startDate: "0"` with the monthly flag returns HTTP 400: the window crosses both `2025-08-01` and `2026-05-14`, and the server asks for a split. Empty aggregates are not that error.

**`GetGrokBotUsageBreakdown`** works on `POST https://api2.cursor.sh/aiserver.v1.DashboardService/GetGrokBotUsageBreakdown` with the Cursor bearer (864-byte JSON). The guessed cursor.com path `POST /api/dashboard/get-grok-bot-usage-breakdown` returned the website HTML. Schema in `index.js`: `billed_cents`, `weekly_limit_ppm`, `billing_cycle_ppm`. This response had only `weeklyLimitPpm` for `2026-11-03T09:55:11.127Z` .. `2026-11-10T09:55:11.127Z`:

| UTC day | source | ppm |
| --- | --- | --- |
| 2026-11-04 | AUTOMATION | 2535 |
| 2026-11-04 | USER | 1628 |
| 2026-11-05 | AUTOMATION | 652 |
| 2026-11-05 | USER | 10478 |
| 2026-11-06 | USER | 4642 |

`byBot` repeats those days with an optional bot id. `GrokBotUsageSource` is `USER | AUTOMATION | AGENT | GROUP | OTHER`. This is daily, not per request. No cents. Sum of ppm is 19935. `19935 / 1000 = 19.935`, which is not `usagePercent` `18.17984832069339`. No other simple scaling of that sum matches the percent. Do not convert ppm to the percent or to tokens.

**`GetSandUsageStatus`** (cursor.com REST and api2; same 1447-byte body): `usagePercent` **18.17984832069339**, `currentPeriodStart` `2026-11-03T09:55:11.127Z`, `nextResetTimestampUtc` `2026-11-10T09:55:11.127Z`, `hasAvailableUsage` true, `hasNonZeroIncludedLimit` true, `isUnified` true, `billingBrand` `SAND_BILLING_BRAND_GROK`, `grokPlanLabel` `Grok Bot Plan`. `includedLimitZero` is absent. `SandBillingBrand` in the bundle is `UNSPECIFIED | CURSOR | GROK`; the wire value is the prefixed form. No allowance size, no tokens, no cents.

**`GetCurrentPeriodUsage`** (REST `POST /api/dashboard/get-current-period-usage` and api2) returns that same weekly window and the same `autoPercentUsed` / `totalPercentUsed` `18.17984832069339`. `displayMessage` is `You've used 18% of your included usage`. Proto fields `total_spend`, `included_spend`, `limit` are omitted. `autoBucketModels` is `["default"]`.

**`GET /api/usage-summary`** is a different pool: cycle `2026-11-01` .. `2026-12-01`, plan `used` 0, `limit` **4615** cents, `totalPercentUsed` 0, on-demand disabled. Grok usage is not drawing this down. TokenHell's stored snapshot at `2026-11-06T21:12:37Z` already showed `$0.00 of $46` plus the Grok week at this same percent.

**`GetPlanInfo`**: `includedUsagePeriod` `INCLUDED_USAGE_PERIOD_WEEKLY`, `includedUsageResetsAt` `1794304511127` (the sand reset). Current plan `includedAmountCents` is absent. `nextUpgrade.includedAmountCents` is 16159 for tier `super` (the next plan, not this allowance). REST and api2 disagree on `planInfo.billingCycleEnd` (`1794563151249` vs `1794216099864`) across two polls; both agree on the weekly reset. `GetMonthlyBillingCycle` matches the api2 end (`2026-10-09` .. `2026-11-09`). Use the sand reset, not `billingCycleEnd`.

**Other reads:** `GetCreditGrantsBalance` and `GetClientVisibleCreditGrants` were `{}`. `GetUserPrepaidBilling` was prepaid, `spentThisMonthCents` `"0"`. `GetXaiUserUsageSnapshot` on api2 was HTTP 401 `Invalid xAI identity assertion` (Cursor bearer is not accepted). `GetSandAccessStatus` was `SAND_ACCESS_STATE_GRANTED` and the same Grok brand. Guessed REST paths for the xAI snapshot, usage-limit policy, and sand enterprise spend returned HTML.

### Local files

| Location | Result |
| --- | --- |
| `~/.cursor/chats/<hash>/<uuid>/store.db` | Tables `blobs(id, data)` and `meta(key, value)`. Meta JSON keys: `agentId`, `latestRootBlobId`, `name`, `mode`, `createdAt`, `lastUsedModel`, `blobEncryptionKey` (a 32-byte hex key; not copied here). Three DBs. JSON blobs are chat messages (`role`, `content`, `providerOptions`). ASCII `inputTokens` / `tokenUsage` hits were this research's own prompt and source text inside one chat, not a usage record. |
| `~/.cursor/projects/*/agent-transcripts/*/*.jsonl` | 9 files. Line types `user`, `assistant`, `turn_ended`. `turn_ended` keys are only `type`, `status`, `error`. 30 such lines, no token fields. |
| `~/.cursor/ai-tracking/ai-code-tracking.db` | `ai_code_hashes` has `model`, `source`, `timestamp`, file identity, `requestId`. 2547 rows, all model `grok-4.7`, source `cli` 600 and `composer` 1947, latest `2026-11-06T21:07:28Z`. No token columns. |
| `state.vscdb` `cursorDiskKV` | `composerData:*` and `bubbleId:*`. 1656 `tokenCount` objects, every `inputTokens` and `outputTokens` is 0. `usageData` is `{}` on 13 composers. Five composers have `contextTokensUsed` / `contextUsagePercent` / `promptTokenBreakdown` against `contextTokenLimit` 500000 (context window). Today's CLI chat ids are not in this DB. |
| `workspaceStorage/*/state.vscdb` | No usage ledger (`cursorDiskKV` empty). `conversation-search.db` is title/body search. |
| `$TMPDIR/cursor-agent-logs-501/*.log` | Message type `interactionUpdate:turnEnded` appears. No `"inputTokens": <number>` records. |
| `~/.cursor/cli-config.json`, `agent-cli-state.json`, `statsig-cache.json` | Model selection and UI flags. No usage counters. |

## 2. How fast it updates

- **Stream / hooks:** at completion of that model call or turn, on the open agent connection. Debug logs confirm the server delivers `turnEnded` as the stream ends. Not a separate poll. Latency was not stopwatched.
- **`usagePercent`:** TokenHell's limit row at `2026-11-06T21:12:37Z` and live polls around `21:15–21:16Z` were the same float `18.17984832069339`, including two polls one second apart. This CLI session was active across that window (tracking rows through 21:07, and the session continued). That does not isolate a single finished request, and it does show the figure is not a per-token live ticker. The Nov 6 daily ppm bucket exists, so the weekly meter is current at day resolution while filtered events are still frozen on 13 Oct.
- **Filtered and aggregated events:** no rows for any usage after `2026-10-13T09:49:35Z`, checked the same evening as CLI Grok use. That is a missing ledger, not a short server lag. For ordinary models, this account's page 2 still has two `claude-3.5-sonnet` rows, the newest of them on 2026-09-15. There is no fresh non-Grok traffic here to time.

## 3. `usagePercent` and money

Verified: it is used-percent, already in percent points (`18.18` displays as 18%). On this unified account it is the same field as `GetCurrentPeriodUsage` `totalPercentUsed` / `autoPercentUsed`, over the weekly window above, not over the monthly $46.15 plan pool (that pool is at 0).

The sand and period-usage payloads omit the absolute limit. Plan info omits current `includedAmountCents`. Prepaid spend this month is 0. Breakdown cents are absent. **There is no denominator in these responses to turn the percent into tokens or dollars.** The Super upgrade's 16159 cents is a different product.

## 4. What TokenHell should do

1. **Weekly Grok ring:** keep `get-sand-usage-status` (api2 `GetSandUsageStatus` is the same body). Show `usagePercent` and `nextResetTimestampUtc`. `GetCurrentPeriodUsage` is the same percent on this account; the monthly `usage-summary` plan window is a separate, currently unused, cent pool. Polling every few seconds will not yield per-request steps.
2. **Per-request tokens:** tail a file the CLI does not write today. The reliable producers are:
   - **Interactive:** a user hook on `afterAgentResponse` and `stop` that appends one JSONL line (`input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `conversation_id`, `generation_id`, `model`). Those counts are the raw stream fields. No cents and no reasoning.
   - **Scripts:** `cursor-agent -p --output-format stream-json`. Use `result.usage` as the turn total, or the `usage` / `model_call` lines, not both. Remember `inputTokens` in that JSON already had cache read and write subtracted.
3. **History:** keep `get-filtered-usage-events` for rows that exist (here, through 2026-10-13, model `grok-bot-default`, with real `tokenUsage`). Treat HTTP 200 `{}` as an empty page. Do not expect this route, or `GetAggregatedUsageEvents`, to grow with current Grok Bot Plan CLI usage.
4. **Do not parse** `store.db`, transcript `turn_ended` lines, `ai-code-tracking.db`, composer `tokenCount`, or the debug logs for billing tokens. Context-window fields (`contextTokensUsed`, status-line `used_percentage`) are prompt occupancy, capped at 500000 for these Grok 4.7 chats.
