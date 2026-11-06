# Grok and Cursor: implementation research

Research date: 2026-11-06. This updates `FINDINGS.md`; it is a source review plus a **read-only, key-name-only** inspection of the local Cursor database. No credential values, session contents, or authenticated API responses were read. JSON below is illustrative, with optional fields combined for clarity, not a captured response. CodexBar and tokscale `main` are moving targets; the linked lines describe the source inspected, not a guaranteed API contract. All Cursor dashboard and Grok subscription endpoints described below are private and can change.

## Decisions and scope

* Grok Build, the unrelated superagent `grok-cli`, the xAI developer API, Grok consumer subscriptions, and Grok Bot in Cursor have different ledgers and credentials. Do not add their account-wide and local counts together. A Cursor `isUnified` subscription may share one weekly pool with Grok; display it once when account identity and pool equivalence are established.
* The earlier findings missed **official Grok Build completed-turn token records** and a working community-discovered **SuperGrok credit usage** path. `signals.json` is only a context-size proxy, not equivalent to billed token throughput. Use completed turns where available.
* For Cursor, import account-scoped server event history with the desktop session; SQLite is a credential source, not a verified complete usage ledger. Quotas are separate snapshots. Do not transmit tokens to any host other than the intended Cursor endpoints, or log raw response bodies and credentials.

## A. Grok / xAI

### 1. Official xAI Grok Build CLI: local records

[xAI's Grok Build repository](https://github.com/xai-org/grok-build) identifies the official coding agent; CodexBar uses `grok agent stdio` for its ACP integration. It is distinct from the community executable also named `grok`. Respect `GROK_HOME` (default `~/.grok`).

**Completed turns.** Read `${GROK_HOME}/sessions/<encoded-workspace>/<session-id>/updates.jsonl`, one JSON object per line. Newer versions persist `method: "session/update"` where `params.update.sessionUpdate == "turn_completed"`; an example of the relevant shape is:

```json
{"method":"session/update","params":{"sessionId":"<session>","update":{"sessionUpdate":"turn_completed","prompt_id":"<turn>","usage":{"inputTokens":1000,"outputTokens":100,"totalTokens":1100,"cachedReadTokens":400,"cacheCreationTokens":0,"reasoningTokens":20,"modelCalls":2,"costUsdTicks":1230000,"modelUsage":{"grok-4.5":{"inputTokens":1000,"outputTokens":100,"cachedReadTokens":400,"reasoningTokens":20}}}},"_meta":{"eventId":"<event>","agentTimestampMs":1790900000000}}}
```

`modelUsage` is a map keyed by actual model; use its complete per-model entries for a breakdown, and do not add the parent aggregate again. Some releases use `promptId`, alternative cache field names or no model breakdown. `inputTokens` includes cache reads (and cache-creation where present); `outputTokens` includes reasoning. For additive buckets, subtract the known subsets from their parents once after validating their relationships. Preserve unknown fields as unknown, and reject inconsistent or incomplete splits rather than clamp them into facts. `params._meta.agentTimestampMs` is milliseconds; use the envelope timestamp when absent. [Tokscale parser and normalization](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/grok.rs#L159-L239), [completed-turn fixtures](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/grok.rs#L2383-L2431), [ccusage Grok guide](https://ccusage.com/guide/grok/).

Treat `(session ID, prompt_id or promptId)` as a turn identity when present, keeping the best complete replacement rather than summing retransmissions; Tokscale's fixture uses `grok:<session>:prompt:<prompt>`. If prompt identity is absent use available event identity only where its scope is established, otherwise preserve an explicitly weaker file/record identity. Include child sessions but verify parent/child rollup before summing. `costUsdTicks / 10^10` is a provider-reported metric, not established as the subscription charge or even necessarily the user's actual bill. Respect `costIsPartial`/`usageIsIncomplete`; if per-model costs are incomplete do not claim a complete total. See [Tokscale cost handling and tests](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/grok.rs#L280-L315) and [ccstats coverage explanation](https://github.com/majiayu000/ccstats) (its README also discusses the retention caveat).

**Inference telemetry.** `${GROK_HOME}/logs/unified.jsonl` can contain `msg: "shell.turn.inference_done"`, `ts`, `sid`, optional `pid`, and `ctx` with `prompt_tokens`, `cached_prompt_tokens`, `completion_tokens`, `reasoning_tokens`, `loop_index`, and sometimes `request_id`. Prompt includes cached prompt, and completion includes reasoning; these are per-inference records, potentially more useful for tiered API-equivalent pricing. `summary.json` can carry `current_model_id` and session metadata, but model changes and parent/child scopes require care; do not guess. Prefer request ID scoped to session for identity when present, preserve distinct id-less records, and handle log truncation/replacement: this file can be trimmed in place, losing history before a scanner captures it. Inference rows and completed turns overlap: reconcile coverage per session/turn, never simply sum both. [Tokscale inference parser and fixtures](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/grok.rs#L725-L820), [ccstats retention and pricing notes](https://github.com/majiayu000/ccstats), [openusage report](https://github.com/robinebers/openusage/issues/1126).

**Fallback only.** `signals.json` contains `contextTokensUsed`, `totalTokensBeforeCompaction`, `contextWindowTokens`, `primaryModelId`, `modelsUsed`, `turnCount` and `sessionDurationSeconds`. CodexBar sums `totalTokensBeforeCompaction + contextTokensUsed` once per session and attributes the **entire mutable snapshot** to the file modification day. This measures context/compaction proxy, not all repeated model input tokens; a long session spanning days moves the total on update and has no input/output or per-model split. Keep it explicitly labeled and do not combine with completed turns. [CodexBar scanner](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Grok/GrokLocalSessionScanner.swift#L71-L140).

### 2. Community superagent-ai/grok-cli

This is an independent API-key client, not xAI's Grok Build. It uses `~/.grok/grok.db` SQLite. Open read-only, following WAL, and query only `usage_events` columns; do not read prompts in `messages`. Current schema:

```sql
SELECT id, session_id, message_seq, source, model, input_tokens,
       output_tokens, total_tokens, cost_micros, created_at
FROM usage_events ORDER BY id;
```

`id` is an autoincrement primary key, stable only within that database; namespace by database identity, and reconcile replacement/deletion. `created_at` is ISO timestamp. `recordUsageEvent` records `inputTokens`, `outputTokens`, and `totalTokens` or input+output; it inserts only when there is positive usage. `cost_micros` is the client's model-price estimate, not invoice cost; zero for an unknown model is **not** proof of free usage. No separate cache/reasoning columns are established here. Check actual version/schema and do not assume Grok Build writes this database. [DB path](https://github.com/superagent-ai/grok-cli/blob/main/src/storage/db.ts#L21-L25), [schema](https://github.com/superagent-ai/grok-cli/blob/main/src/storage/migrations.ts#L78-L89), [usage writer](https://github.com/superagent-ai/grok-cli/blob/main/src/storage/usage.ts).

### 3. Official xAI API: billing, credits, per-request usage and limits

**Authentication:** `https://management-api.x.ai` requires `Authorization: Bearer <management key>`, separately created in Console Settings → Management Keys with appropriate team ACLs. The normal inference key for `https://api.x.ai` is a different credential and does not substitute. Obtain the team ID from Console or management-key identity; namespace all data by team. [Official management guide](https://docs.x.ai/developers/management-api-guide), [auth reference](https://docs.x.ai/developers/rest-api-reference/management/auth).

**Team history:** `POST /v1/billing/teams/{team_id}/usage`, `Content-Type: application/json`. A valid daily request and representative response are:

```json
{"analyticsRequest":{"timeRange":{"startTime":"2026-10-26 00:00:00","endTime":"2026-11-03 00:00:00","timezone":"Etc/GMT"},"timeUnit":"TIME_UNIT_DAY","values":[{"name":"units","aggregation":"AGGREGATION_SUM"},{"name":"usd","aggregation":"AGGREGATION_SUM"}],"groupBy":["unit_type","description"],"filters":[]}}
```
```json
{"timeSeries":[{"group":["Prompt text tokens","Chat grok-4.5"],"groupLabels":["Prompt text tokens","Chat grok-4.5"],"dataPoints":[{"timestamp":"2026-10-26T00:00:00Z","values":[1234,0.002]}]}],"limitReached":false}
```

`group` follows `groupBy` order; `values` follows requested value order. `units`, `usd`, `items`, `transactions` use `AGGREGATION_SUM`. `Prompt text tokens` is **uncached** and `Cached prompt text tokens` is separate; distinguish `Completion text tokens` and `Reasoning text tokens` as provider billing dimensions and do not assume non-token units are tokens. `description` is a product/resource label, usually containing the model, not a guaranteed canonical model ID. Other group/filter fields include `api_key_id`, `product`, `request_ip`, `user_id`, `unit_price`. `startTime` and exclusive `endTime` are local to supplied IANA zone; upsert team + bucket + all dimensions and refresh for revisions. `limitReached: true` means incomplete; split the interval or reduce cardinality, don't silently accept it. Add `unit_price` if price tier matters; it is in 10^-10 USD per unit. [Official billing example and schema](https://docs.x.ai/developers/rest-api-reference/management/billing#daily-token-usage).

**Balances and spend controls:** with the same management Bearer, `GET /v1/billing/teams/{team_id}/prepaid/balance` responds with `{"changes":[{"teamId":"…","changeOrigin":"PURCHASE","topupStatus":"SUCCEEDED","amount":{"val":"-1000"},"createTime":"…"}],"total":{"val":"-1000"}}`; `val` is an integer string in USD cents. Preserve provider sign until independently validated: the official example itself uses a negative `total`, so don't unconditionally assert `total.val / 100` is a positive spendable balance. `GET /v1/billing/teams/{team_id}/postpaid/spending-limits` returns `{"spendingLimits":{"hardSlAuto":{"val":"22500"},"effectiveHardSl":{"val":"22500"},"softSl":{"val":"20000"},"effectiveSl":{"val":"20000"}}}` in cents. Postpaid limits do not cap prepaid consumption. `GET /v1/billing/teams/{team_id}/postpaid/invoice/preview` previews the current postpaid invoice; consult the billing reference for its current response schema. Do not call top-up or limit-setting endpoints. [Billing reference](https://docs.x.ai/developers/rest-api-reference/management/billing).

**Per request:** normal inference responses include usage: Chat Completions `usage.prompt_tokens`, `completion_tokens`, `total_tokens`, `prompt_tokens_details.cached_tokens`, `completion_tokens_details.reasoning_tokens`; Responses uses `input_tokens`, `output_tokens`, `input_tokens_details.cached_tokens`, `output_tokens_details.reasoning_tokens`, and may include `cost_in_usd_ticks`. Capture these only when the application is actually the client or logs expose them; there is no demonstrated passive local feed for arbitrary API clients. [xAI usage schemas](https://docs.x.ai/developers/advanced-api-usage/prompt-caching/usage-and-pricing), [Responses reference](https://docs.x.ai/developers/rest-api-reference/inference/responses).

**Rate limits:** current official docs describe per-team, per-model RPS and TPM; exceeding limits returns 429, and tier depends on spend. Console shows personalized limits. Management `GET /auth/teams/{teamId}/api-keys` yields configured per-key `qps`, `qpm`, `tpm`, not live remaining counters; follow response `paginationToken`. We did not verify a supported endpoint returning live remaining team/model quota or reset headers: expose configured limits separately, and handle 429 with backoff. [Rate limits](https://docs.x.ai/developers/rate-limits), [API-key schema](https://docs.x.ai/developers/rest-api-reference/management/auth).

### 4. SuperGrok / X subscription credits

CodexBar supports a real subscription usage path, distinct from xAI API management. First run `grok agent stdio`, ACP `initialize`, then JSON-RPC method `_x.ai/billing` without params; a `-32601` missing-method response can fall back for personal accounts. Auth is owned by Grok; do not refresh or rewrite it. CodexBar reports unsupported team usage separately. The CLI RPC uses the same credits parser as the proxy and can expose a wallet even if usage percentage is unavailable. [CodexBar integration](https://github.com/steipete/CodexBar/blob/main/docs/grok.md#L20-L52).

With an unexpired user OAuth token from `~/.grok/auth.json`, CodexBar also calls:

```http
GET https://cli-chat-proxy.grok.com/v1/billing?format=credits
Authorization: Bearer <grok-cli-oauth-token>
x-xai-token-auth: xai-grok-cli
Accept: application/json
```

Illustrative optional fields parsed by [the source](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Grok/GrokCreditsProxyFetcher.swift#L40-L105):

```json
{"subscriptionTier":"…","config":{"subscriptionTier":"…","creditUsagePercent":23.5,"currentPeriod":{"start":"2026-11-01T00:00:00Z","end":"2026-11-08T00:00:00Z"},"billingPeriodStart":"…","billingPeriodEnd":"…","onDemandUsed":{"val":235},"onDemandCap":{"val":1000},"prepaidBalance":{"val":"2500"},"productUsage":[{"product":"GrokBuild","usagePercent":23.5}]}}
```

Use `creditUsagePercent` as percent used; absent, fallback to `onDemandUsed.val / onDemandCap.val * 100` only with valid positive cap. `currentPeriod.end`, else `billingPeriodEnd`, is reset; match start to chosen period. `prepaidBalance.val` is cents; it is a wallet, not a percent. Product percentages are portions of the same pool, not separate limits. `GET https://cli-chat-proxy.grok.com/v1/settings` with the same headers may give `subscription_tier_display`. No value means unknown, not zero. These fields describe credits, not a universal remaining-query count.

Best-effort browser/bearer fallback: `POST https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig`, gRPC-web frame bytes `00 00 00 00 02 08 00`; it returns protobuf, not JSON, with percent/reset decoded by CodexBar. Authentication can require a browser-held Web Key Exchange keypair, so cookie alone can fail. Optional `POST https://grok.com/prod_mc_billing.ConsumerUiSvc/GetRemainingResets` retrieves reset-coupon inventory and is not the quota itself. CodexBar accepts a narrowly validated omitted proto3 scalar as zero for a current period, but the wire format cannot distinguish withheld from zero; TokenHell can retain unknown unless it adopts that explicit inference. [CodexBar fallback details](https://github.com/steipete/CodexBar/blob/main/docs/grok.md#L120-L156), [fetcher](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Grok/GrokWebBillingFetcher.swift).

## B. Cursor on this Mac

### 1. Authentication and local discovery

`~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`, SQLite `ItemTable`: the access token is `cursorAuth/accessToken`. The requested read-only key-name query found these keys only (no values were retrieved):

```text
cursorAuth/accessToken
cursorAuth/cachedEmail
cursorAuth/cachedScopedProfile
cursorAuth/cachedSignUpType
cursorAuth/hasInvalidatedMembershipForUnifiedSemanticsV2
cursorAuth/refreshToken
cursorAuth/storedServerBackendUrl
cursorAuth/stripeMembershipAuthId
cursorAuth/stripeMembershipType
cursorAuth/stripeSubscriptionStatus
```

Names do not establish whether a value is populated or authoritative. CodexBar only needs `accessToken`; it decodes the JWT payload `sub`, takes the component after the last `|` as user ID, validates its characters, checks numeric `exp`, and builds `Cookie: WorkosCursorSessionToken=<userId>%3A%3A<jwt>`. It uses the cookie on **cursor.com dashboard** requests, not a Bearer header. `GET /api/auth/me` supplies current `sub` and email if available; `membershipType` comes from usage-summary. `cachedEmail`/membership keys are optional stale hints, not needed for the request; never read the refresh token. JWT parsing is not signature verification. [CodexBar auth code](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorAppAuth.swift#L104-L149), [SQLite selection](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorAppAuth.swift#L275-L317).

Open SQLite read-only with ordinary WAL-aware settings and bounded busy timeout. The user should explicitly enable Cursor access; hold the JWT in memory and handle expired, missing, signed-out, inaccessible, and changed-account cases. This was schema discovery only, not validation that the current token works. Browser cookie import is a separate optional credential source; no need to read browser cookies for the desktop path.

### 2. Common dashboard protocol

Base `https://cursor.com`. Send `Accept: application/json` and the `Cookie` above. For POST, send `Content-Type: application/json`, `Origin: https://cursor.com`, and a JSON body; CodexBar uses `{}` for Sand. [CodexBar request construction](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift#L1245-L1285), [Tokscale headers](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-cli/src/cursor.rs#L297-L325). The separate `api2.cursor.sh` Connect RPC uses Bearer in other clients; do not confuse it with these dashboard calls.

**GET `/api/usage-summary`**: all shown fields are optional and plan-dependent, cents unless percent/date/flag. A shape synthesized from CodexBar's decoder:

```json
{"billingCycleStart":"2026-11-01T00:00:00Z","billingCycleEnd":"2026-12-01T00:00:00Z","membershipType":"pro","limitType":"…","isUnlimited":false,
 "individualUsage":{"plan":{"enabled":true,"used":500,"limit":2000,"remaining":1500,"breakdown":{"included":500,"bonus":0,"total":500},"autoPercentUsed":25.0,"apiPercentUsed":15.0,"totalPercentUsed":20.0},"onDemand":{"enabled":true,"used":0,"limit":1000,"remaining":1000},"overall":{"enabled":true,"used":500,"limit":2000,"remaining":1500}},
 "teamUsage":{"onDemand":{"used":0,"limit":null},"pooled":{"used":500,"limit":10000,"remaining":9500}}}
```

The cycle values are ISO strings. `plan.limit`, not `breakdown.total`, is the cap. `autoPercentUsed`, `apiPercentUsed`, and `totalPercentUsed` are **already percentages**, including sub-1 values; do not multiply by 100. CodexBar prefers verified member team budget, then `totalPercentUsed`, then its own average of auto/api, a single available component, `used/limit`, overall, pooled. The average is a client fallback, not a guarantee of Cursor's plan math; retain provenance and prefer unknown if semantics do not match. CodexBar defaults some absent values to zero for its UI; TokenHell's existing null/unknown rule should avoid that loss. Team accounts can require `/api/dashboard/teams` and paged `/api/dashboard/get-team-spend` to establish a verified personal cap; pooled and member scopes are different. [Models](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift#L187-L284), [projection](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe%2BUsageSummary.swift#L13-L81), [team behavior](https://github.com/steipete/CodexBar/blob/main/docs/cursor.md).

**GET `/api/auth/me`**: optional response shape `{"sub":"<subject>","email":"…","emailVerified":true,"name":"…","createdAt":"…","updatedAt":"…","picture":"…"}`. CodexBar fetches it alongside summary and passes its `sub` to the legacy query when available, with a token-derived fallback. It is optional; failure should not destroy a valid quota snapshot. [Decoder and request](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift#L286-L312).

**GET `/api/usage?user=<URL-encoded-user-id>`**: legacy request-based plan, not event history. Response shape `{"startOfMonth":"…","gpt-4":{"numRequests":42,"numRequestsTotal":42,"numTokens":1000,"maxRequestUsage":500,"maxTokenUsage":null}}`. CodexBar uses `numRequestsTotal ?? numRequests` and `maxRequestUsage` when positive; don't mistake request quota for token quota. Endpoint may fail or be absent. [Legacy decoder](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift#L286-L304), [mapping](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe%2BUsageSummary.swift#L82-L85).

**POST `/api/dashboard/get-sand-usage-status`**, body `{}`: optional Cursor Grok Bot allowance:

```json
{"currentPeriodStart":"2026-11-01T00:00:00Z","nextResetTimestampUtc":"2026-11-08T00:00:00Z","usagePercent":42.5,"hasAvailableUsage":true,"hasNonZeroIncludedLimit":true,"includedLimitZero":false,"sandTrialExpiresAt":null,"isUnified":false}
```

`usagePercent` is used percentage, not remaining. New `includedLimitZero`, if present, overrides legacy `hasNonZeroIncludedLimit`; an unexpired `sandTrialExpiresAt` also enables display when no paid allowance exists. Paid allowance has a seven-day duration if a valid reset is present; a trial expiry is **not** a recurring reset. An exhausted but unexpired trial can still be shown. A failed/missing/malformed optional response must not fail summary. For `isUnified`, CodexBar can use the weekly Sand pool as the main shared pool when no explicit legacy request quota exists, rather than present it as independent extra usage. [Sand decoder/mapping](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorSandUsage.swift#L1-L53), [unified mapping](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift#L1055-L1080).

### 3. Per-event history, pagination and identity

**POST `/api/dashboard/get-filtered-usage-events`** with the dashboard cookie and POST headers above. Tokscale starts page numbers at 1, asks for 500 rows, and supplies string epoch-millisecond bounds even on first sync:

```json
{"startDate":"0","endDate":"1793923200000","page":1,"pageSize":500}
```

Do not add `teamId: 0`: Tokscale documents it as a filter that hides personal events. Illustrative response:

```json
{"totalUsageEventsCount":1,"usageEventsDisplay":[{"usageEventId":"<optional-id>","conversationId":"<optional-session>","timestamp":"1792841120156","model":"grok-bot-automation","kind":"USAGE_EVENT_KIND_INCLUDED_IN_PRO_PLUS","tokenUsage":{"inputTokens":100,"outputTokens":40,"cacheReadTokens":300,"cacheWriteTokens":20,"totalCents":1.25},"chargedCents":0,"cursorTokenFee":0,"isChargeable":true}]}
```

The event can also include `requestsCosts`, `usageBasedCosts`, `isTokenBasedCall`, `owningUser`, `owningTeam`, `isHeadless`. `timestamp` is normally a millisecond string; tokscale accepts numeric or ISO timestamps. `totalCents` is provider-reported metered token cost; `chargedCents` is actual charged amount and can be different or zero for included usage. Preserve **both** separately; neither is automatically TokenHell's API-equivalent estimate. Each cents field can be a number or numeric string, may be absent, and may be fractional. Reasoning is not established by tokscale's event decoder. It treats four token categories as additive; before shipping, verify input/cache inclusion for actual plan/version because independent audits describe inclusive values for some sources. Preserve raw numeric categories and source/version rather than doubling cache. [Tokscale event parser](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/cursor.rs#L129-L235), [API request and pagination](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-cli/src/cursor.rs#L1903-L2159), [Cursor Admin API contract](https://prod.cursor.com/docs/account/teams/admin-api).

Pagination: when `totalUsageEventsCount` exists, continue until that many raw rows are fetched; a short page may be a server-clamped size, so do not stop just because it is shorter than requested. Empty page before advertised total is incomplete/error. Without count, use short/empty fallback. Enforce timeout, max body/pages, 429 backoff, and make sync atomic; do not advance the watermark on a partial result. On incremental sync, tokscale requests the hour before the newest **valid** cached timestamp through a fixed sync-end time; it can stop when a valid page crosses the lower bound. It accepts only timestamps >0 and no more than five minutes ahead of sync start as watermarks, retaining malformed rows without allowing them to skip history. Older billing corrections can still be missed, so schedule or allow wider reconciliation. [Request body](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-cli/src/cursor.rs#L468-L481), [pagination](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-cli/src/cursor.rs#L2100-L2138), [incremental behavior](https://github.com/junhoyeo/tokscale#cursor-ide-commands).

**Identity:** when `usageEventId` is present, namespace it by authenticated account and use it for repeated snapshots; same ID may appear with cumulative updated totals. Tokscale's report parser picks the largest snapshot for a repeated ID and its cache merge replaces old matching identities with fresh ones. Multiple IDs in a single conversation are distinct. For id-less rows, tokscale's overlap matching uses normalized `(conversationId or absent, timestamp, model, input, output, cache read, cache write)` while excluding mutable cost and unknown fields from the matching identity; raw rows are preserved. It keeps all incoming occurrences, even identical ones, and replaces matched cached occurrences as a group. This fallback is a reconciliation heuristic, **not a proven unique event key**: two legitimate identical events can collide and a token correction can change the tuple. Prefer replacing the complete fetched overlapping time interval atomically and preserving multiplicity; distinguish separate accounts, avoid inventing request IDs, and label any unavoidable id-less uncertainty. [Tokscale identity](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/cursor.rs#L237-L280), [merge](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-cli/src/cursor.rs#L371-L449), [snapshot tests](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-core/src/sessions/cursor.rs#L837-L893).

For eligible team admins Cursor also offers a separately authenticated, documented `api.cursor.com/teams/filtered-usage-events` Admin API. It has different permissions, aggregation and rate constraints; it is not a drop-in personal session endpoint. Server rows cannot be reliably matched one-to-one to local transcript turns without a common request identity.

## C. Cursor 3.x encrypted token

Follow-up research: 2026-11-06. **This section corrects B.1:** reading `cursorAuth/accessToken` as an immediately usable JWT is not sufficient for Cursor 3.27+ encrypted installations. The user reports Cursor 3.29.9 and an approximately 603-character value with two colons on this Mac. That is consistent with the public `cursor-safe-storage:v1:<base64>` envelope, but we did **not** read the value or its prefix and therefore did not independently confirm this particular row's format, decrypt it, inspect Keychain secrets, or send an authenticated request. The two colons in the known envelope separate literal labels and a base64 payload; they are not evidence of separate IV/tag fields. [CodexBar issue #4755](https://github.com/steipete/CodexBar/issues/4755), [merged support #4790](https://github.com/steipete/CodexBar/pull/4790).

### Exact known macOS envelope and crypto

This is the Cursor 3.27-era implementation corroborated by CodexBar's source and a separate Cursor implementation; it is an implementation detail, not a documented cross-platform format guarantee.

1. Open `state.vscdb` read-only and read only `cursorAuth/accessToken` in memory. Dispatch on prefix: a legacy ordinary JWT can use the old path; `cursor-safe-storage:v1:` means remove **that entire literal prefix** and base64-decode the remainder. A `cursor-safe-storage:` value with an unknown version, invalid base64, or failed decryption must be unavailable, never a raw Bearer or cookie token.
2. Decoded bytes are ASCII `v10` (three bytes) followed by ciphertext. Require more than three bytes and `(length - 3) % 16 == 0`. There is no per-message IV or GCM tag field in this observed macOS v1 format.
3. Read the generic-password item service `Cursor Safe Storage`. Account names are important: CodexBar's Nov 5 merge tries `Cursor Key` first and `Cursor` as a legacy fallback. The Keychain password is the PBKDF2 input, not directly the AES key. Query exact service/account with consistent preflight and secret lookup; if multiple candidate items exist, avoid silently selecting an unrelated identity.
4. Derive 16 bytes using `PBKDF2-HMAC-SHA1(password UTF-8 bytes, salt UTF-8 "saltysalt", iterations 1003, length 16)`. Decrypt the bytes after `v10` with **AES-128-CBC**, IV sixteen bytes `0x20` (ASCII spaces), using PKCS#7 padding; decode valid UTF-8. CBC is unauthenticated: valid padding alone does not prove authenticity. Validate resulting JWT structure, `sub` and `exp`, and verify the account with the server before associating history. Never log plaintext or key material; bad padding, malformed UTF-8, invalid identity, expiration, denial, or unknown version are distinct failures.
5. Build the Cursor dashboard cookie from the decrypted JWT as described in B; the encrypted envelope cannot be used as that JWT. The 3.27 feature was described as an account-dependent encryption-at-rest rollout; local existence of the key `cursor.storage.authTokenEncryptionAtRest` does not establish its value or prove encryption is enabled.

Source: pinned [CodexBar decryption entry point and account order](https://github.com/steipete/CodexBar/blob/42e2a14/Sources/CodexBarCore/Providers/Cursor/CursorAppAuth.swift#L225-L253), [PBKDF2/AES/PKCS#7 implementation](https://github.com/steipete/CodexBar/blob/42e2a14/Sources/CodexBarCore/ChromiumSafeStorage.swift#L41-L98), independent [cc-bar implementation notes](https://github.com/nanvon/cc-bar/blob/main/docs/%E6%8A%80%E6%9C%AF%E5%AE%9E%E7%8E%B0.md). These describe macOS; do not apply its Keychain/KDF assumptions to Windows or Linux.

**Keychain interaction.** Offer this as an explicit Cursor connection action; first check access without UI, permit an OS prompt only on a user-initiated refresh, and back off after denial. Read in memory only, don't alter Cursor's keychain item or auth database, and use a stable signed app identity where possible. CodexBar's code shares a bounded prompt allowance and denial cooldown with browser import; background/settings probes do not trigger a prompt. macOS policy and ACLs can still deny access even if the item exists. Do not equate the presence of a Safe Storage item with permission or a successful decrypt. [Safe Storage keychain gate](https://github.com/steipete/CodexBar/blob/42e2a14/Sources/CodexBarCore/ChromiumSafeStorage.swift#L9-L40), [account fix live-proof discussion](https://github.com/steipete/CodexBar/pull/4913).

### What the three clients currently do

* **CodexBar:** encrypted macOS support was merged October 26 in [#4790](https://github.com/steipete/CodexBar/pull/4790), initially looking only for account `Cursor`. A real 3.27.22 report found account `Cursor Key`, causing a not-found result before any prompt ([#4880](https://github.com/steipete/CodexBar/issues/4880)). [#4913](https://github.com/steipete/CodexBar/pull/4913) was merged November 5 at `42e2a14`, with a reported live manual-refresh success and both account names; the merged PR says it ships in the next release. Do not assume an already installed release includes that fix. Code also retains browser-session alternatives; its documented `cursor-agent` file fallback is specifically Linux, not proof of a macOS file fallback. [Source at merge](https://github.com/steipete/CodexBar/blob/42e2a14/Sources/CodexBarCore/Providers/Cursor/CursorAppAuth.swift#L232-L253), [Cursor provider notes](https://github.com/steipete/CodexBar/blob/main/docs/cursor.md).
* **Tokscale:** the inspected current `main` [Cursor importer](https://github.com/junhoyeo/tokscale/blob/main/crates/tokscale-cli/src/cursor.rs#L699-L757) reads the database string directly, then attempts to decode it as a JWT to construct `user_id%3A%3Atoken`; no `cursor-safe-storage` handling was found in that source file. Consequently this path cannot automatically import the known encrypted envelope; that is an inference from the code, not a local execution. Its explicit browser-cookie login remains a separate workaround. We found no verified upstream issue or merged PR for this exact encryption envelope in the search; this is not proof none exists.
* **OpenUsage:** its publicly retrievable [Cursor usage client](https://github.com/robinebers/openusage/blob/main/Sources/OpenUsage/Providers/Cursor/CursorUsageClient.swift) uses a supplied Bearer for `api2.cursor.sh` RPC and derives a cookie from JWT `sub` for cursor.com; that client does not decrypt the desktop envelope itself. A cached older `CursorAuthStore` source read raw SQLite tokens and `cursor-access-token`/`cursor-refresh-token` Keychain services, and selected between SQLite and Keychain; it did not implement the v1 Safe Storage decoder. However, the old `CursorAuthStore.swift` main-path now returns 404 to a fresh fetch, so the latest auth-store implementation and issue/PR status could not be verified reliably. Do **not** conclude current OpenUsage either supports or fails encrypted tokens from the older cached file. Its [auto-detection docs](https://openusage.sh/docs/concepts/auto-detection/) confirm a Cursor desktop source but do not settle 3.27+ encryption. No live test was performed.

### Alternatives to decrypting the desktop token

* **Independently signed-in Cursor Agent CLI:** on current macOS builds, reported CLI Keychain service is `cursor-access-token`, account `cursor-user`; `cursor-refresh-token` is separate and should not be read or refreshed by TokenHell. After explicit user consent, a valid access JWT can be used as `Authorization: Bearer <access-token>` with the private Connect JSON API:

  ```http
  POST https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage
  Authorization: Bearer <cli-access-token>
  Content-Type: application/json
  Connect-Protocol-Version: 1

  {}
  ```

  Related methods include `GetPlanInfo` and `GetSandUsageStatus`. The decoded `planUsage` contains `includedSpend`, `limit`, `totalPercentUsed`, `autoPercentUsed`, `apiPercentUsed`; billing cycle bounds can be millisecond strings. This is a different login and may be a different account or plan: compare server/JWT subject before merging with desktop data. This route avoids **desktop Safe Storage decryption**, not necessarily a Keychain consent prompt. It also does not by itself establish access to the cursor.com per-event endpoint; a validated CLI JWT may be used to derive a cookie as in B, but verify account and endpoint behavior first. [OpenUsage request source](https://github.com/robinebers/openusage/blob/main/Sources/OpenUsage/Providers/Cursor/CursorUsageClient.swift#L6-L16), [independent CLI credential research](https://seanthegeek.net/doubleagent/research/cursor.html), [CLI quota implementation notes](https://github.com/levi-qiao/herdr-agent-usage/blob/main/CHANGELOG.md).
* **CLI files:** depending on CLI build/platform and an explicit file-backed credential-store setting, `~/.cursor/auth.json` on macOS or `${XDG_CONFIG_HOME:-~/.config}/cursor/auth.json` on Linux can contain `accessToken`; some older clients also checked `~/.cursor/cli-config.json`. Current macOS reports instead describe `cli-config.json` `authInfo` as identity metadata and the secret in Keychain. Do not assume the metadata is a credential or force a storage backend/change config. On this Mac filename checks found `~/.cursor/cli-config.json` and `~/.cursor/agent-cli-state.json`; `~/.cursor/auth.json` and `~/.config/cursor/auth.json` were absent. `cursor-agent` is on PATH. We did not read file contents, check Keychain for the CLI item, or run login/status, so CLI authentication is unknown. [Observed CLI storage](https://seanthegeek.net/doubleagent/research/cursor.html), [quota-axi implementation notes](https://github.com/hphsu/quota-axi).
* **Browser session:** an explicitly supplied Cursor `WorkosCursorSessionToken` cookie, or user-authorized browser-cookie import, avoids decrypting Cursor.app's database value. Browser cookie encryption may itself require another Keychain access and the session can belong to a different account. `cursor.com` event sync remains available subject to account permissions, expiration and API changes. [Tokscale browser login](https://github.com/junhoyeo/tokscale#cursor-ide-commands).
* **Team Admin API:** an admin-provided API key for `api.cursor.com/teams/filtered-usage-events` avoids both desktop and CLI token decryption, but is a separate credential and a team-admin surface, not a universal personal quota endpoint. [Cursor Admin API](https://prod.cursor.com/docs/account/teams/admin-api).

**Other database keys:** the local key-name-only query found `cursorAuth/cachedScopedProfile`, `cursorAuth/cachedEmail`, `cursorAuth/stripeMembershipType`, and `cursor.storage.authTokenEncryptionAtRest`; these are metadata/settings, not a demonstrated replacement bearer. Worker `secret://` keys also exist but refer to other encrypted app secrets, not a verified account token. We did not inspect their values. No safe, working plaintext alternate desktop access-token key was established. An endpoint change alone cannot turn encrypted ciphertext into valid authentication.
