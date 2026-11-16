/** Parse whole tokens or case-insensitive K/M/B shorthand without rounding. */
export function parseTokenAmount(value: string): number | null {
  const match = /^(\d+)(?:\.(\d+))?\s*([kmb])?$/i.exec(value.trim())
  if (!match) return null
  const scale = { k: 3, m: 6, b: 9 }[match[3]?.toLowerCase() ?? ''] ?? 0
  const fraction = (match[2] ?? '').replace(/0+$/, '')
  if (fraction.length > scale) return null
  const amount = BigInt(match[1] + fraction.padEnd(scale, '0'))
  return amount > 0n && amount <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(amount) : null
}

export function tokens(n: number): string {
  if (n < 1_000) return String(n)
  if (n < 1_000_000) return trim((n / 1e3).toFixed(1)) + 'K'
  if (n < 1_000_000_000) return trim((n / 1e6).toFixed(1)) + 'M'
  return (n / 1e9).toFixed(2) + 'B'
}
const trim = (s: string) => s.replace(/\.0$/, '')

export function usd(n: number | null | undefined): string {
  if (n == null) return '$ –'
  return '$' + n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

/** "2h 14m", "3d 4h", "12m", "now" */
export function until(unixSec: number | null, nowMs = Date.now()): string {
  if (!unixSec) return ''
  let s = Math.round(unixSec - nowMs / 1000)
  if (s <= 0) return 'now'
  const d = Math.floor(s / 86400); s -= d * 86400
  const h = Math.floor(s / 3600); s -= h * 3600
  const m = Math.floor(s / 60)
  if (d > 0) return `${d}d ${h}h`
  if (h > 0) return `${h}h ${m}m`
  return `${Math.max(m, 1)}m`
}

export function ago(unixSec: number | null, nowMs = Date.now()): string {
  if (!unixSec) return 'never'
  const s = Math.max(0, Math.round(nowMs / 1000 - unixSec))
  if (s < 10) return 'just now'
  if (s < 60) return `${s}s ago`
  if (s < 3600) return `${Math.floor(s / 60)}m ago`
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`
  return `${Math.floor(s / 86400)}d ago`
}

export function day(iso: string): string {
  const d = new Date(iso + 'T12:00:00')
  return d.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' })
}

/** Status band for a used %, paired with text label everywhere it's shown. */
export function band(pct: number | null): 'good' | 'warning' | 'serious' | 'critical' | 'unknown' {
  if (pct == null) return 'unknown'
  if (pct >= 90) return 'critical'
  if (pct >= 75) return 'serious'
  if (pct >= 50) return 'warning'
  return 'good'
}
