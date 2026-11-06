<script lang="ts">
  import { PROVIDER_LABEL, type LimitReport } from './api'
  import { PROVIDER_COLOR } from './colors'
  import { ago, band, until } from './format'
  import ResetCredits from './ResetCredits.svelte'

  let { reports, now }: { reports: LimitReport[]; now: number } = $props()
  const BAND_LABEL = { good: '', warning: '', serious: 'high', critical: 'near limit', unknown: '' }
</script>

{#each reports as r (r.provider)}
  <div class="card">
    <div class="head">
      <span class="dot" style="background:{PROVIDER_COLOR[r.provider]}"></span>
      <span class="name">{PROVIDER_LABEL[r.provider]}</span>
      {#if r.plan}<span class="plan">{r.plan}</span>{/if}
      {#if r.stale}<span class="stale" title="Showing last known values">· {ago(r.observed_at, now)}</span>{/if}
    </div>
    {#each r.windows as w (w.key)}
      {@const b = band(w.used_pct)}
      <div class="row">
        <span class="wl">{w.label}</span>
        <div class="track" role="meter" aria-label="{w.label} used" aria-valuenow={w.used_pct ?? undefined} aria-valuemin="0" aria-valuemax="100">
          {#if w.used_pct != null}
            <div class="fill {b}" style="width:{Math.min(100, Math.max(w.used_pct, 1.5))}%"></div>
          {/if}
        </div>
        <span class="pct num {b}">
          {#if b === 'critical'}<span aria-hidden="true">▲</span>{/if}{w.used_pct == null ? '–' : Math.round(w.used_pct) + '%'}
        </span>
        <span class="reset num" title={w.resets_at ? new Date(w.resets_at * 1000).toLocaleString() : ''}>
          {w.resets_at ? until(w.resets_at, now) : (w.detail ?? '')}
        </span>
      </div>
      {#if w.detail && w.resets_at}<div class="detail">{w.detail}</div>{/if}
      {#if BAND_LABEL[b]}<span class="sr">{BAND_LABEL[b]}</span>{/if}
    {/each}
    {#each r.notes as n}<div class="note">{n}</div>{/each}
    {#if r.reset_credits?.available}<ResetCredits report={r} {now} />{/if}
    {#if r.error}<div class="err" title={r.error}><span aria-hidden="true">⚠︎</span> {r.error}</div>{/if}
  </div>
{/each}

<style>
  .card { padding: 9px 0 8px; }
  .card + .card { border-top: 0.5px solid var(--hair); }
  .head { display: flex; align-items: center; gap: 7px; margin-bottom: 6px; }
  .name { font-weight: 600; font-size: 12.5px; }
  .plan {
    font-size: 10px; font-weight: 600; letter-spacing: 0.03em; text-transform: uppercase;
    padding: 1px 5px; border-radius: 5px; background: var(--fill-2); color: var(--ink-2);
  }
  .stale { color: var(--ink-3); font-size: 11px; }
  .row { display: grid; grid-template-columns: 74px 1fr 42px 52px; align-items: center; gap: 8px; height: 20px; }
  .wl { color: var(--ink-2); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .track { height: 5px; border-radius: 3px; background: var(--track); overflow: hidden; }
  .fill { height: 100%; border-radius: 3px; transition: width 400ms cubic-bezier(.2,.8,.2,1); }
  .fill.good { background: var(--good); }
  .fill.warning { background: var(--warning); }
  .fill.serious { background: var(--serious); }
  .fill.critical { background: var(--critical); }
  .pct { text-align: right; font-size: 12px; font-weight: 600; color: var(--ink-1); }
  .pct.critical { color: var(--critical); }
  .pct span { font-size: 8px; margin-right: 3px; vertical-align: 1px; }
  .reset { text-align: right; font-size: 11px; color: var(--ink-3); white-space: nowrap; }
  .detail, .note { font-size: 11px; color: var(--ink-3); padding-left: 82px; margin-top: -1px; }
  .note { padding-left: 0; margin-top: 3px; }
  .err {
    font-size: 11px; color: var(--ink-2); margin-top: 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .err span { color: var(--serious); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
</style>
