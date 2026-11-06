<script lang="ts">
  import { onMount } from 'svelte'
  import { api, on, type LimitReport, type Range, type Summary } from './lib/api'
  import { PROVIDER_COLOR } from './lib/colors'
  import { ago, tokens, usd } from './lib/format'
  import Limits from './lib/Limits.svelte'
  import Mix from './lib/Mix.svelte'
  import Settings from './lib/Settings.svelte'
  import Sparkline from './lib/Sparkline.svelte'

  const RANGES: { id: Range; label: string }[] = [
    { id: 'today', label: 'Today' }, { id: '7d', label: '7d' }, { id: '30d', label: '30d' }, { id: 'all', label: 'All' },
  ]

  let range = $state<Range>('today')
  let summary = $state<Summary | null>(null)
  let limits = $state<LimitReport[]>([])
  let view = $state<'main' | 'settings'>('main')
  let expanded = $state<string | null>(null)
  let refreshing = $state(false)
  let now = $state(Date.now())
  let error = $state<string | null>(null)

  async function loadSummary() {
    try {
      summary = await api.summary(range)
      error = null
    } catch (e) {
      error = String(e)
    }
  }
  async function loadLimits() {
    limits = await api.limits()
  }
  async function refresh() {
    refreshing = true
    try {
      await api.refresh()
      await Promise.all([loadLimits(), loadSummary()])
    } finally {
      refreshing = false
    }
  }

  $effect(() => {
    range
    loadSummary()
  })

  onMount(() => {
    loadLimits()
    const tick = setInterval(() => (now = Date.now()), 15_000)
    const subs = [
      on('usage-updated', loadSummary),
      on('limits-updated', loadLimits),
      on('panel-shown', () => { now = Date.now(); view = 'main'; loadSummary(); loadLimits() }),
    ]
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') view === 'settings' ? (view = 'main') : api.hide()
      if (e.key === 'r' && e.metaKey) { e.preventDefault(); refresh() }
      if (e.key === ',' && e.metaKey) { e.preventDefault(); view = 'settings' }
      if (e.key === 'q' && e.metaKey) api.quit()
    }
    window.addEventListener('keydown', onKey)
    return () => {
      clearInterval(tick)
      window.removeEventListener('keydown', onKey)
      subs.forEach((p) => p.then((u) => u()))
    }
  })

  const newestLimit = $derived(limits.reduce((m, r) => Math.max(m, r.stale ? 0 : r.observed_at), 0))
  const rangeWord = $derived(range === 'today' ? 'today' : range === 'all' ? 'all time' : `last ${range.replace('d', ' days')}`)
</script>

<main>
  {#if view === 'settings'}
    <Settings onback={() => { view = 'main'; loadLimits(); loadSummary() }} />
  {:else}
    <header>
      <div class="brand">
        <svg width="13" height="15" viewBox="0 0 100 100" aria-hidden="true">
          <path d="M50 96C29 96 15 82 17 63 19 46 35 39 38 24 40 14 39 7 37 3 54 10 64 25 61 41 67 37 69 31 69 25 81 38 86 52 83 66 80 84 67 96 50 96Z" fill="var(--accent)"/>
        </svg>
        <span>TokenHell</span>
      </div>
      <div class="ranges" role="tablist">
        {#each RANGES as r}
          <button role="tab" aria-selected={range === r.id} class:on={range === r.id} onclick={() => (range = r.id)}>{r.label}</button>
        {/each}
      </div>
      <button class="icon" onclick={() => (view = 'settings')} aria-label="Settings" title="Settings (⌘,)">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z"/></svg>
      </button>
    </header>

    <div class="scroll">
      {#if summary}
        <section class="hero">
          <div>
            <div class="big num">{tokens(summary.tokens.total)}</div>
            <div class="sub">tokens burned {rangeWord}</div>
          </div>
          <div class="right">
            <div class="cost num">{usd(summary.cost_usd)}</div>
            <div class="sub" title="Claude/Codex: priced per request at API list rates (LiteLLM). Cursor/Grok: the cost they report. Subscriptions bill differently.">API-equivalent</div>
          </div>
        </section>

        <section>
          <Sparkline points={summary.daily} />
        </section>

        {#if summary.tokens.total > 0}
          <section><Mix t={summary.tokens} /></section>
        {/if}
      {:else if error}
        <div class="empty">{error}</div>
      {/if}

      {#if limits.length}
        <section class="block">
          <div class="row-head">
            <span class="eyebrow">Limits <span class="unit">· % used</span></span>
            <button class="link" onclick={refresh} disabled={refreshing} title="Refresh (⌘R)">
              <svg class:spin={refreshing} width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6"/></svg>
              {newestLimit ? ago(newestLimit, now) : 'refresh'}
            </button>
          </div>
          <Limits reports={limits} {now} />
        </section>
      {/if}

      {#if summary && summary.providers.length}
        <section class="block">
          <div class="row-head"><span class="eyebrow">By tool</span></div>
          {#each summary.providers as p (p.provider)}
            {@const share = summary.tokens.total ? p.tokens.total / summary.tokens.total : 0}
            <button class="tool" onclick={() => (expanded = expanded === p.provider ? null : p.provider)} aria-expanded={expanded === p.provider}>
              <span class="dot" style="background:{PROVIDER_COLOR[p.provider]}"></span>
              <span class="tname">{p.label}</span>
              <span class="share"><span style="width:{share * 100}%; background:{PROVIDER_COLOR[p.provider]}"></span></span>
              <span class="num tval">{tokens(p.tokens.total)}</span>
              <span class="num tcost">{usd(p.cost_usd)}</span>
            </button>
            {#if expanded === p.provider}
              <div class="models">
                {#each p.models as m (m.model)}
                  <div class="model">
                    <span class="mname" title={m.model}>{m.model}</span>
                    <span class="num mval">{tokens(m.tokens.total)}</span>
                    <span class="num mcost">{usd(m.cost_usd)}</span>
                  </div>
                  <div class="msplit num">
                    in {tokens(m.tokens.input)} · out {tokens(m.tokens.output)} · cache {tokens(m.tokens.cache_read)}/{tokens(m.tokens.cache_write)} · {m.events} calls
                  </div>
                {/each}
              </div>
            {/if}
          {/each}
        </section>
      {:else if summary}
        <div class="empty">No usage {rangeWord} yet. Go burn some tokens.</div>
      {/if}
    </div>
  {/if}
</main>

<style>
  main { height: 100%; display: flex; flex-direction: column; padding: 14px 16px 12px; }
  header { display: flex; align-items: center; gap: 10px; height: 28px; flex: none; }
  .brand { display: flex; align-items: center; gap: 6px; font-weight: 700; font-size: 13.5px; letter-spacing: -0.01em; }
  .ranges { margin-left: auto; display: flex; padding: 2px; border-radius: 9px; background: var(--fill-1); }
  .ranges button { height: 22px; padding: 0 9px; border-radius: 7px; font-size: 11.5px; color: var(--ink-2); }
  .ranges button.on { background: var(--tip-bg); color: var(--ink-1); font-weight: 600; box-shadow: 0 1px 3px rgba(0,0,0,.12); }
  .icon { width: 26px; height: 26px; display: grid; place-items: center; border-radius: 7px; color: var(--ink-2); }
  .icon:hover { background: var(--fill-2); color: var(--ink-1); }

  .scroll { flex: 1; overflow-y: auto; margin: 0 -16px; padding: 0 16px; }
  section { margin-top: 14px; }
  .hero { display: flex; justify-content: space-between; align-items: flex-end; margin-top: 16px; }
  .big { font-size: 34px; font-weight: 700; letter-spacing: -0.03em; line-height: 1; }
  .cost { font-size: 20px; font-weight: 650; letter-spacing: -0.02em; line-height: 1; text-align: right; }
  .sub { font-size: 11px; color: var(--ink-3); margin-top: 5px; }
  .right .sub { text-align: right; }

  .block { margin-top: 18px; padding-top: 12px; border-top: 0.5px solid var(--hair); }
  .unit { text-transform: none; letter-spacing: 0; font-weight: 500; }
  .row-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 2px; }
  .link { display: flex; align-items: center; gap: 4px; font-size: 11px; color: var(--ink-3); padding: 2px 6px; border-radius: 6px; }
  .link:hover:not(:disabled) { background: var(--fill-2); color: var(--ink-1); }
  .spin { animation: spin 0.9s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .tool {
    width: 100%; display: grid; grid-template-columns: 8px 1fr 50px 56px 64px; align-items: center; gap: 9px;
    height: 30px; margin: 0 -6px; padding: 0 6px; border-radius: 8px; width: calc(100% + 12px); text-align: left;
  }
  .tool:hover { background: var(--fill-1); }
  .tname { font-weight: 550; }
  .share { height: 3px; border-radius: 2px; background: var(--track); overflow: hidden; }
  .share span { display: block; height: 100%; border-radius: 2px; opacity: 0.85; }
  .tval { text-align: right; font-weight: 600; }
  .tcost { text-align: right; color: var(--ink-2); }
  .models { padding: 2px 0 6px 17px; }
  .model { display: grid; grid-template-columns: 1fr 56px 64px; gap: 9px; font-size: 11.5px; margin-top: 4px; }
  .mname { color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .mval, .mcost { text-align: right; color: var(--ink-2); }
  .msplit { font-size: 10.5px; color: var(--ink-3); }
  .empty { margin-top: 24px; text-align: center; color: var(--ink-3); font-size: 12px; }
</style>
