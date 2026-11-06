<script lang="ts">
  import { api, type LimitReport, type ResetOutcome } from './api'
  import { until } from './format'

  let { report, now }: { report: LimitReport; now: number } = $props()

  const credits = $derived(report.reset_credits!)
  // The most-used window: what a reset would actually buy you back.
  const top = $derived(
    report.windows
      .filter((w) => w.used_pct != null)
      .sort((a, b) => (b.used_pct ?? 0) - (a.used_pct ?? 0))[0],
  )
  const lowUsage = $derived(top != null && (top.used_pct ?? 0) < 75)

  let open = $state(false)
  let busy = $state(false)
  let error = $state<string | null>(null)
  let result = $state<ResetOutcome | null>(null)
  // One key per logical attempt, kept across retries until the server gives a definite answer,
  // so a lost response can never spend a second credit.
  let key: string | null = null

  const RESULT: Record<ResetOutcome, { text: string; ok: boolean }> = {
    reset: { text: 'Limits reset', ok: true },
    nothingToReset: { text: 'Nothing to reset, no credit used', ok: false },
    noCredit: { text: 'No reset credits left', ok: false },
    alreadyRedeemed: { text: 'That reset was already used', ok: false },
  }

  function uuid(): string {
    if (crypto.randomUUID) return crypto.randomUUID()
    const b = crypto.getRandomValues(new Uint8Array(16))
    b[6] = (b[6] & 0x0f) | 0x40
    b[8] = (b[8] & 0x3f) | 0x80
    const h = [...b].map((x) => x.toString(16).padStart(2, '0')).join('')
    return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`
  }

  function ask() {
    result = null
    error = null
    open = true
  }

  async function confirm() {
    if (busy) return
    key ??= uuid()
    busy = true
    error = null
    try {
      result = await api.codexConsumeReset(key)
      key = null
      open = false
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }
</script>

<div class="bar">
  <span class="count">{credits.available} full reset{credits.available === 1 ? '' : 's'} available</span>
  {#if !open}
    <button class="btn" onclick={ask}>Reset…</button>
  {/if}
</div>

{#if result && !open}
  <div class="result" class:ok={RESULT[result].ok} role="status">
    <span aria-hidden="true">{RESULT[result].ok ? '✓' : 'ⓘ'}</span> {RESULT[result].text}
  </div>
{/if}

{#if open}
  <div class="warn" role="alertdialog" aria-labelledby="reset-title" aria-describedby="reset-body">
    <div class="title" id="reset-title"><span class="icon" aria-hidden="true">⚠︎</span> Use a full reset?</div>
    <div class="body" id="reset-body">
      <p>This spends <b>1 of your {credits.available}</b> Codex reset credits and clears your usage limits right away. <b>It can't be undone.</b></p>
      {#if lowUsage && top}
        <p class="caution">You've only used {Math.round(top.used_pct ?? 0)}% of your {top.label} limit. It's better to save the credit until you're close to the limit.</p>
      {/if}
      {#if top?.resets_at}
        <p>Your {top.label} limit resets by itself in <b>{until(top.resets_at, now)}</b>.</p>
      {/if}
      {#if credits.next_expires_at}
        <p class="dim">Next credit expires in {until(credits.next_expires_at, now)}.</p>
      {/if}
    </div>
    {#if error}<div class="err" title={error}><span aria-hidden="true">⚠︎</span> {error}</div>{/if}
    <div class="actions">
      <button class="btn" onclick={() => (open = false)} disabled={busy}>Cancel</button>
      <button class="btn danger" onclick={confirm} disabled={busy}>
        {busy ? 'Resetting…' : error ? 'Try again' : 'Reset now'}
      </button>
    </div>
  </div>
{/if}

<style>
  .bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-top: 3px; min-height: 20px; }
  .count { font-size: 11px; color: var(--ink-3); }
  .btn {
    font: inherit; font-size: 11px; font-weight: 500; line-height: 1; padding: 4px 9px; border-radius: 6px;
    border: 0.5px solid var(--hair); background: var(--fill-1); color: var(--ink-1); cursor: default;
  }
  .btn:hover:not(:disabled) { background: var(--fill-2); }
  .btn:disabled { opacity: 0.55; }
  .btn.danger { background: var(--critical); border-color: transparent; color: #fff; font-weight: 600; }
  .btn.danger:hover:not(:disabled) { filter: brightness(1.08); background: var(--critical); }
  .warn {
    margin-top: 6px; padding: 8px 10px; border-radius: 8px;
    background: color-mix(in srgb, var(--critical) 9%, transparent);
    border: 0.5px solid color-mix(in srgb, var(--critical) 35%, transparent);
  }
  .title { font-size: 12px; font-weight: 600; color: var(--ink-1); margin-bottom: 4px; }
  .icon { color: var(--critical); }
  .body p { margin: 0 0 4px; font-size: 11px; line-height: 1.4; color: var(--ink-2); }
  .body b { color: var(--ink-1); font-weight: 600; }
  .body .caution { color: var(--ink-1); }
  .body .caution::before { content: '▲ '; color: var(--serious); font-size: 8px; vertical-align: 1px; }
  .body .dim { color: var(--ink-3); }
  .err { font-size: 11px; color: var(--ink-2); margin: 2px 0 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .err span { color: var(--serious); }
  .actions { display: flex; justify-content: flex-end; gap: 6px; margin-top: 6px; }
  .result { font-size: 11px; color: var(--ink-2); margin-top: 3px; }
  .result.ok span { color: var(--good); }
</style>
