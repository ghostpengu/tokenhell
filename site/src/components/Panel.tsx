import { useEffect, useState } from 'react'
import { Flame } from './Flame'

// A live HTML recreation of the TokenHell panel (ui/src/App.svelte), fed with demo numbers.

type Range = 'Today' | '7d' | '30d' | 'All'

const APPS = [
  { key: 'claude', color: 'var(--color-claude)' },
  { key: 'codex', color: 'var(--color-codex)' },
  { key: 'cursor', color: 'var(--color-cursor)' },
  { key: 'grok', color: 'var(--color-grok)' },
] as const

const DATA: Record<Range, { tokens: number; cost: number; label: string; days: number }> = {
  Today: { tokens: 111_300_000, cost: 40.82, label: 'tokens burned today', days: 7 },
  '7d': { tokens: 684_200_000, cost: 251.37, label: 'tokens in 7 days', days: 7 },
  '30d': { tokens: 2_412_000_000, cost: 903.12, label: 'tokens in 30 days', days: 14 },
  All: { tokens: 5_874_000_000, cost: 2204.55, label: 'tokens all time', days: 14 },
}

const MIX = [
  { label: 'Input', value: '701.4K', pct: '<1%', color: 'var(--color-codex)', w: 1.5 },
  { label: 'Output', value: '586.6K', pct: '<1%', color: 'var(--color-claude)', w: 1.2 },
  { label: 'Cache read', value: '108.8M', pct: '98%', color: 'var(--color-cursor)', w: 95 },
  { label: 'Cache write', value: '1.2M', pct: '1%', color: 'var(--color-grok)', w: 2.3 },
]

function bars(days: number) {
  return Array.from({ length: days }, (_, k) => {
    const v = (0.4 + 0.6 * Math.abs(Math.sin(k * 1.7 + 0.3))) * (k % 6 === 5 ? 0.25 : 1)
    const w = APPS.map((_, i) => 0.35 + Math.abs(Math.sin(k * 1.1 + i * 1.4)))
    const s = w.reduce((a, b) => a + b, 0)
    return { v, parts: w.map((x) => x / s) }
  })
}

export function fmtTokens(n: number) {
  if (n >= 1e9) return (n / 1e9).toFixed(2) + 'B'
  if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M'
  if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K'
  return String(n)
}

function fmtLeft(s: number) {
  const d = Math.floor(s / 86400)
  const h = Math.floor((s % 86400) / 3600)
  const m = Math.floor((s % 3600) / 60)
  return d > 0 ? `${d}d ${h}h` : `${h}h ${String(m).padStart(2, '0')}m`
}

const gaugeColor = (p: number) => (p >= 85 ? 'var(--color-hot)' : p >= 50 ? 'var(--color-warn)' : 'var(--color-good)')

const LIMITS = [
  {
    name: 'Claude Code',
    plan: 'Max',
    color: 'var(--color-claude)',
    rows: [
      { label: '5h', pct: 59, left: 3 * 3600 + 43 * 60 },
      { label: 'week', pct: 40, left: 2 * 86400 + 16 * 3600 },
    ],
  },
  {
    name: 'Codex',
    plan: 'Team',
    color: 'var(--color-codex)',
    note: '2 full resets available',
    rows: [
      { label: '5h', pct: 22, left: 3 * 3600 + 43 * 60 },
      { label: 'week', pct: 54, left: 86400 + 14 * 3600 },
    ],
  },
  {
    name: 'Cursor',
    plan: 'Pro',
    color: 'var(--color-cursor)',
    note: '$7.60 of $20',
    rows: [{ label: 'month', pct: 38, left: 17 * 86400 + 4 * 3600 }],
  },
]

/** Today's count ticks up the way it does in the app while an agent is working. */
export function useLiveTokens(start = 111_300_000) {
  const [n, setN] = useState(start)
  useEffect(() => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    const id = setInterval(() => setN((x) => x + 18_000 + Math.round(Math.random() * 42_000)), 1400)
    return () => clearInterval(id)
  }, [])
  return n
}

export default function Panel({ live }: { live: number }) {
  const [range, setRange] = useState<Range>('Today')
  const [elapsed, setElapsed] = useState(0)
  useEffect(() => {
    const id = setInterval(() => setElapsed((e) => e + 1), 1000)
    return () => clearInterval(id)
  }, [])

  const d = DATA[range]
  const tokens = range === 'Today' ? live : d.tokens + (live - 111_300_000)
  const cost = d.cost + (live - 111_300_000) * 0.00000036
  const series = bars(d.days)
  const max = Math.max(...series.map((b) => b.v))

  return (
    <div className="glass w-[340px] max-w-full select-none rounded-[22px] p-4 text-[13px] text-ink-1">
      {/* header */}
      <div className="flex items-center gap-2">
        <Flame className="h-4 w-4" />
        <span className="font-semibold tracking-tight">TokenHell</span>
        <div className="ml-auto flex rounded-[9px] bg-fill-2 p-[2px]" role="tablist" aria-label="Range">
          {(Object.keys(DATA) as Range[]).map((r) => (
            <button
              key={r}
              role="tab"
              aria-selected={range === r}
              onClick={() => setRange(r)}
              className={`h-[22px] rounded-[7px] px-[9px] text-[11.5px] transition-colors ${
                range === r ? 'bg-white/15 font-semibold text-ink-1 shadow-sm' : 'text-ink-2 hover:text-ink-1'
              }`}
            >
              {r}
            </button>
          ))}
        </div>
        <svg viewBox="0 0 24 24" className="h-[18px] w-[18px] text-ink-2" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden>
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </div>

      {/* totals */}
      <div className="mt-4 flex items-end justify-between">
        <div>
          <div className="tnum text-[40px] leading-none font-semibold tracking-[-0.03em]">{fmtTokens(tokens)}</div>
          <div className="mt-1.5 text-[11px] text-ink-3">{d.label}</div>
        </div>
        <div className="text-right">
          <div className="tnum text-[22px] leading-none font-semibold tracking-tight">
            ${cost.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
          </div>
          <div className="mt-1.5 text-[11px] text-ink-3">API-equivalent</div>
        </div>
      </div>

      {/* sparkline, split by app */}
      <div key={range} className="mt-4 flex h-[46px] items-end gap-[3px]">
        {series.map((b, i) => (
          <div
            key={i}
            className="bar-rise flex flex-1 flex-col-reverse overflow-hidden rounded-[3px]"
            style={{ height: `${(b.v / max) * 100}%`, animationDelay: `${i * 35}ms` }}
          >
            {b.parts.map((p, j) => (
              <span key={j} style={{ height: `${p * 100}%`, background: APPS[j].color }} className="block opacity-90" />
            ))}
          </div>
        ))}
      </div>

      {/* token mix */}
      <div className="mt-3 flex h-[5px] gap-[2px] overflow-hidden rounded-full">
        {MIX.map((m) => (
          <span key={m.label} className="bar-grow" style={{ flexGrow: m.w, background: m.color }} />
        ))}
      </div>
      <div className="mt-2.5 grid grid-cols-2 gap-x-3 gap-y-1.5 whitespace-nowrap text-[11.5px]">
        {MIX.map((m) => (
          <div key={m.label} className="flex items-center gap-1.5">
            <span className="h-[7px] w-[7px] rounded-full" style={{ background: m.color }} />
            <span className="text-ink-2">{m.label}</span>
            <span className="tnum ml-auto">{m.value}</span>
            <span className="tnum w-7 text-right text-ink-3">{m.pct}</span>
          </div>
        ))}
      </div>

      {/* limits */}
      <div className="mt-4 border-t-[0.5px] border-hair pt-3">
        <div className="flex items-center text-[11px] text-ink-3">
          <span>Limits · % used</span>
          <span className="tnum ml-auto">↻ {(elapsed % 60) + 4}s ago</span>
        </div>
        <div className="mt-1">
          {LIMITS.map((l, li) => (
            <div key={l.name} className={`py-2.5 ${li ? 'border-t-[0.5px] border-hair' : ''}`}>
              <div className="flex items-center gap-2">
                <span className="h-2 w-2 rounded-full" style={{ background: l.color }} />
                <span className="font-semibold">{l.name}</span>
                <span className="rounded-[5px] bg-fill-2 px-1.5 py-px text-[10px] font-semibold text-ink-2">{l.plan}</span>
              </div>
              {l.rows.map((r) => (
                <div key={r.label} className="mt-1.5 grid grid-cols-[48px_1fr_38px_52px] items-center gap-2 text-[12px]">
                  <span className="text-ink-2">{r.label}</span>
                  <span className="h-[5px] overflow-hidden rounded-full bg-fill-2">
                    <span className="bar-grow block h-full rounded-full" style={{ width: `${r.pct}%`, background: gaugeColor(r.pct) }} />
                  </span>
                  <span className="tnum text-right font-semibold">{r.pct}%</span>
                  <span className="tnum text-right text-ink-3">{fmtLeft(r.left - elapsed)}</span>
                </div>
              ))}
              {l.note && <div className="mt-1.5 text-[11px] text-ink-3">{l.note}</div>}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
