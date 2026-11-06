# Research brief 2: Grok + Cursor (implementation-level detail)

Context: read `research/FINDINGS.md` and `PLAN.md` first. We are now BUILDING TokenHell. I need
exact, implementation-ready details for two providers. RESEARCH ONLY, do not edit code outside
`research/`. Write results to `research/FINDINGS-2.md`.

## A. Grok (xAI)
The user wants Grok usage tracked. Find every way Grok usage/limits can be observed:
- Grok CLIs (e.g. superagent-ai/grok-cli, xAI official CLI if one exists, "grok build"/agent tools):
  local session/log paths, file format, token fields.
- xAI API: management/billing API endpoints for usage, spend, credits, rate limits
  (management-api.x.ai or api.x.ai). Auth type (management key vs API key), exact JSON shapes.
- SuperGrok / X Premium subscription limits: any endpoint (grok.com internal) exposing remaining
  queries/rate limits; how CodexBar or other tools do it, if any.
- Grok used inside Cursor (Cursor's "get-sand-usage-status" weekly Grok quota).

## B. Cursor (installed on this Mac)
Give exact implementation detail, verified against CodexBar and tokscale SOURCE CODE (not just docs):
- Which `state.vscdb` ItemTable key holds the access token (e.g. `cursorAuth/accessToken`) and
  any other keys needed (user id, email, membership type). You may run read-only sqlite3 queries
  on `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb` to list KEY NAMES ONLY
  matching `cursorAuth%` or similar. NEVER print values.
- How the token is sent (Bearer header? `WorkosCursorSessionToken` cookie built as `userId%3A%3Ajwt`?)
- Exact request + response JSON shapes for: `GET https://cursor.com/api/usage-summary`,
  `GET https://cursor.com/api/usage?user=`, `POST https://cursor.com/api/dashboard/get-filtered-usage-events`
  (or whatever tokscale uses for per-event tokens), `POST /api/dashboard/get-sand-usage-status`,
  and `GET /api/auth/me` if needed. Include field names for tokens, model, timestamp, cost,
  billing cycle start/end, plan limit, % used.
- Pagination + dedup key for usage events.

Cite the source file URLs (GitHub blob links with line refs where possible).
When done run: `hc send t10 "codex research 2 done: research/FINDINGS-2.md"` (ignore failure).
