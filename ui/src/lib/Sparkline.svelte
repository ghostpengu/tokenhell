<script lang="ts">
  import type { DayPoint } from './api'
  import { day, tokens, usd } from './format'

  let { points }: { points: DayPoint[] } = $props()

  const W = 328
  const H = 46
  let hover = $state<number | null>(null)

  const max = $derived(Math.max(1, ...points.map((p) => p.tokens)))
  const gap = $derived(points.length > 14 ? 2 : 4)
  const bw = $derived((W - gap * (points.length - 1)) / Math.max(points.length, 1))

  // Bar with 4px (or narrower) rounded top, square at the baseline.
  function bar(x: number, h: number, w: number) {
    if (h <= 0) return ''
    const r = Math.min(4, w / 2, h)
    const y = H - h
    return `M${x},${H}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${H}Z`
  }
</script>

<div class="spark" role="img" aria-label="Tokens per day">
  <svg viewBox="0 0 {W} {H}" width={W} height={H}>
    <line x1="0" x2={W} y1={H - 0.5} y2={H - 0.5} class="base" />
    {#each points as p, i}
      {@const x = i * (bw + gap)}
      {@const h = p.tokens > 0 ? Math.max(2, (p.tokens / max) * (H - 4)) : 0}
      <path d={bar(x, h, bw)} class:last={i === points.length - 1} class:dim={hover !== null && hover !== i} />
      <rect
        x={x - gap / 2} y="0" width={bw + gap} height={H} fill="transparent"
        role="presentation"
        onmouseenter={() => (hover = i)} onmouseleave={() => (hover = null)}
      />
    {/each}
  </svg>
  {#if hover !== null}
    {@const p = points[hover]}
    {@const left = Math.min(Math.max(hover * (bw + gap) + bw / 2, 60), W - 60)}
    <div class="tip" style="left:{left}px">
      <div class="tip-day">{day(p.day)}</div>
      <div class="num"><b>{tokens(p.tokens)}</b> tokens · {usd(p.cost_usd)}</div>
    </div>
  {/if}
</div>

<style>
  .spark { position: relative; height: 46px; }
  svg { display: block; overflow: visible; }
  path { fill: var(--seq-weak); transition: opacity 120ms; }
  path.last { fill: var(--seq); }
  path.dim { opacity: 0.45; }
  .base { stroke: var(--hair); stroke-width: 1; }
  .tip {
    position: absolute; bottom: 52px; transform: translateX(-50%); white-space: nowrap; pointer-events: none;
    background: var(--tip-bg); border: 0.5px solid var(--hair); border-radius: 8px; padding: 5px 8px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.14); font-size: 11.5px; z-index: 5;
  }
  .tip-day { color: var(--ink-2); font-size: 10.5px; }
</style>
