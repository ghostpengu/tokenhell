# Research brief 3: precise Cursor cost + token semantics

Context: read research/FINDINGS-2.md section B/C. TokenHell now syncs Cursor per-event history from
`POST https://cursor.com/api/dashboard/get-filtered-usage-events` and stores per event:
tokenUsage.{inputTokens,outputTokens,cacheReadTokens,cacheWriteTokens} and uses `tokenUsage.totalCents/100`
as the event's cost. RESEARCH ONLY (no code outside research/). Write `research/FINDINGS-3.md`.

Answer precisely, citing sources (Cursor docs/pricing pages, Cursor forum staff posts, CodexBar/tokscale/
openusage source, changelogs):
1. What exactly is `tokenUsage.totalCents` vs top-level `chargedCents`, `requestsCosts`, `usageBasedCosts`,
   `cursorTokenFee`, `isChargeable`, `kind` (e.g. USAGE_EVENT_KIND_INCLUDED_IN_PRO_PLUS / ON_DEMAND /
   ERRORED_NOT_CHARGED / ABORTED...)? Which one equals "API-equivalent cost of the tokens" and which is
   "what the user was actually charged"? Does totalCents include Cursor's token fee?
2. Are the four token categories disjoint (is inputTokens uncached-only, or does it include cache reads)?
3. Which event kinds should be excluded from token totals (errored/aborted/not charged)?
4. Is `totalCents` possibly fractional / a string, and are there events without tokenUsage (request-based
   billing) that still carry a cost?
5. Cursor model ids like `grok-bot-default`, `claude-4.5-sonnet-thinking`, `auto`, `composer-1`: does Cursor
   publish per-model token prices (cursor.com/docs/models or pricing) we could use when totalCents is missing?
6. Any better endpoint for exact per-event cost (e.g. `/api/dashboard/get-aggregated-usage-events`,
   `api2.cursor.sh DashboardService/GetFilteredUsageEvents`) and its response fields.
Also: you may run read-only `sqlite3 -readonly` on
`~/Library/Application Support/io.github.ghostpengu.tokenhell/tokenhell.db` to look at stored Cursor rows
(provider='cursor': model, token columns, cost_reported) to sanity-check, but print no credentials.
When done: `hc send t10 "codex research 3 done: research/FINDINGS-3.md"`.
