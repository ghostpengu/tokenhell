<script lang="ts">
  import type { Tokens } from './api'
  import { tokens as fmt } from './format'

  let { t }: { t: Tokens } = $props()
  let hover = $state<string | null>(null)

  const parts = $derived(
    [
      { key: 'input', label: 'Input', v: t.input, c: 'var(--s1)' },
      { key: 'output', label: 'Output', v: t.output, c: 'var(--s2)' },
      { key: 'cache_read', label: 'Cache read', v: t.cache_read, c: 'var(--s3)' },
      { key: 'cache_write', label: 'Cache write', v: t.cache_write, c: 'var(--s4)' },
    ].map((p) => ({ ...p, pct: t.total ? (p.v / t.total) * 100 : 0 })),
  )
</script>

<div class="mix">
  <div class="bar" role="img" aria-label="Token mix">
    {#each parts.filter((p) => p.v > 0) as p (p.key)}
      <div
        class="seg" style="flex-grow:{p.v}; background:{p.c}" class:dim={hover && hover !== p.key}
        role="presentation" onmouseenter={() => (hover = p.key)} onmouseleave={() => (hover = null)}
      ></div>
    {/each}
  </div>
  <div class="legend">
    {#each parts as p (p.key)}
      <div class="item" class:dim={hover && hover !== p.key}>
        <span class="dot" style="background:{p.c}"></span>
        <span class="lab">{p.label}</span>
        <span class="num val">{fmt(p.v)}</span>
        <span class="num pct">{p.pct < 1 && p.v > 0 ? '<1' : p.pct.toFixed(0)}%</span>
      </div>
    {/each}
  </div>
</div>

<style>
  .bar { display: flex; gap: 2px; height: 6px; border-radius: 3px; overflow: hidden; background: var(--track); }
  .seg { min-width: 3px; height: 100%; transition: opacity 120ms; }
  .seg:first-child { border-radius: 3px 0 0 3px; }
  .seg:last-child { border-radius: 0 3px 3px 0; }
  .legend { display: grid; grid-template-columns: 1fr 1fr; gap: 3px 14px; margin-top: 8px; font-size: 11.5px; }
  .item { display: flex; align-items: center; gap: 6px; transition: opacity 120ms; }
  .dim { opacity: 0.4; }
  .lab { color: var(--ink-2); }
  .val { margin-left: auto; color: var(--ink-1); }
  .pct { color: var(--ink-3); width: 30px; text-align: right; }
  .dot { width: 7px; height: 7px; }
</style>
