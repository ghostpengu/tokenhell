<script lang="ts">
  import type { DayApp, DayPoint, Provider } from './api'
  import { PROVIDER_COLOR } from './colors'
  import { day, tokens, usd } from './format'

  let { points }: { points: DayPoint[] } = $props()

  const W = 328
  const H = 64
  let hover = $state<number | null>(null)

  const max = $derived(Math.max(1, ...points.map((p) => p.tokens)))
  const colGap = $derived(points.length > 14 ? 2 : 4)
  const bw = $derived((W - colGap * (points.length - 1)) / Math.max(points.length, 1))
  const hovered = $derived(hover !== null ? points[hover] : undefined)

  // A range switch replaces the columns under the pointer. Drop the tip until
  // the pointer enters a bar of the new series.
  $effect(() => {
    points
    hover = null
  })

  const legend = $derived.by(() => {
    const totals = new Map<Provider, { label: string; tokens: number }>()
    for (const p of points) {
      for (const a of p.apps ?? []) {
        if (a.tokens <= 0) continue
        const cur = totals.get(a.provider)
        if (cur) cur.tokens += a.tokens
        else totals.set(a.provider, { label: a.label, tokens: a.tokens })
      }
    }
    return [...totals.entries()]
      .map(([provider, v]) => ({ provider, ...v }))
      .sort((a, b) => b.tokens - a.tokens || a.label.localeCompare(b.label))
  })

  function color(provider: Provider) {
    return PROVIDER_COLOR[provider] ?? 'var(--ink-3)'
  }

  function appsOf(p: DayPoint): DayApp[] {
    const rank = new Map(legend.map((a, i) => [a.provider, i]))
    return (p.apps ?? [])
      .filter((a) => a.tokens > 0)
      .sort((a, b) => (rank.get(a.provider) ?? 99) - (rank.get(b.provider) ?? 99) || a.label.localeCompare(b.label))
  }

  // Rounded top, square at the baseline. Segments are clipped to this outline.
  function bar(x: number, h: number, w: number) {
    if (h <= 0) return ''
    const r = Math.min(4, w / 2, h)
    const y = H - h
    return `M${x},${H}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${H}Z`
  }

  function stack(p: DayPoint, totalH: number) {
    const apps = appsOf(p)
    const sum = apps.reduce((s, a) => s + a.tokens, 0)
    if (totalH <= 0 || sum <= 0) return []
    const n = apps.length
    const slit = n > 1 && totalH >= n * 3 + (n - 1) ? 1 : 0
    const inner = totalH - slit * (n - 1)
    let y = H
    let used = 0
    return apps.map((a, i) => {
      const h = i === n - 1 ? inner - used : (a.tokens / sum) * inner
      used += h
      y -= h
      const seg = { ...a, y, h }
      y -= slit
      return seg
    })
  }
</script>

<div class="chart">
  <div class="spark" role="img" aria-label="Tokens per day, split by app">
    <svg viewBox="0 0 {W} {H}" width={W} height={H}>
      <line x1="0" x2={W} y1={H - 0.5} y2={H - 0.5} class="base" />
      {#each points as p, i (p.day)}
        {@const x = i * (bw + colGap)}
        {@const h = p.tokens > 0 ? Math.max(2, (p.tokens / max) * (H - 4)) : 0}
        {@const segs = stack(p, h)}
        {#if segs.length}
          <clipPath id="day-{i}">
            <path d={bar(x, h, bw)} />
          </clipPath>
          <g clip-path="url(#day-{i})" class:dim={hover !== null && hover !== i}>
            {#each segs as s (s.provider)}
              <rect x={x} y={s.y} width={bw} height={s.h} fill={color(s.provider)} />
            {/each}
          </g>
        {:else if h > 0}
          <path d={bar(x, h, bw)} class="solo" class:dim={hover !== null && hover !== i} />
        {/if}
        <rect
          x={x - colGap / 2} y="0" width={bw + colGap} height={H} fill="transparent"
          role="presentation"
          onmouseenter={() => (hover = i)} onmouseleave={() => (hover = null)}
        />
      {/each}
    </svg>
    {#if hovered && hover !== null}
      {@const left = Math.min(Math.max(hover * (bw + colGap) + bw / 2, 108), W - 108)}
      <div class="tip" style="left:{left}px">
        <div class="tip-day">{day(hovered.day)}</div>
        <div class="num head"><b>{tokens(hovered.tokens)}</b> tokens · {usd(hovered.cost_usd)}</div>
        {#each appsOf(hovered) as a (a.provider)}
          <div class="tip-app">
            <span class="sw" style="background:{color(a.provider)}"></span>
            <span class="name">{a.label}</span>
            <span class="num val">{tokens(a.tokens)}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
  {#if legend.length}
    <div class="legend">
      {#each legend as a (a.provider)}
        <span><i style="background:{color(a.provider)}"></i>{a.label}</span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .chart { position: relative; z-index: 2; }
  .spark { position: relative; height: 64px; }
  svg { display: block; overflow: visible; }
  g, path.solo { transition: opacity 120ms; }
  g.dim, path.dim { opacity: 0.45; }
  path.solo { fill: var(--seq-weak); }
  .base { stroke: var(--hair); stroke-width: 1; }
  .tip {
    position: absolute; top: calc(100% + 4px); transform: translateX(-50%);
    display: flex; flex-direction: column; align-items: stretch; white-space: nowrap; pointer-events: none;
    background: var(--tip-bg); border: 0.5px solid var(--hair); border-radius: 8px; padding: 5px 8px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.14); font-size: 11.5px; z-index: 5;
  }
  .tip-day { color: var(--ink-2); font-size: 10.5px; }
  .head { margin-top: 1px; }
  .tip-app { display: flex; align-items: center; gap: 6px; margin-top: 3px; }
  .sw { width: 7px; height: 7px; border-radius: 50%; flex: none; }
  .name { color: var(--ink-2); }
  .val { margin-left: auto; padding-left: 12px; }
  .legend { display: flex; flex-wrap: wrap; gap: 3px 12px; margin-top: 8px; font-size: 11px; color: var(--ink-2); }
  .legend span { display: inline-flex; align-items: center; gap: 5px; }
  .legend i { width: 7px; height: 7px; border-radius: 50%; display: block; }
</style>
