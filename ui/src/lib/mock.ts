// Browser-only preview data (vite dev without Tauri). Never used inside the app.
const now = () => Math.floor(Date.now() / 1000)
const t = (i: number, o: number, cr: number, cw: number) => ({ total: i + o + cr + cw, input: i, output: o, cache_read: cr, cache_write: cw })

function summary(range: string) {
  const days = range === '30d' || range === 'all' ? 30 : 7
  const daily = Array.from({ length: days }, (_, k) => {
    const d = new Date(Date.now() - (days - 1 - k) * 86400e3)
    const v = Math.round((40 + 60 * Math.abs(Math.sin(k * 1.7))) * 1e6) * (k % 6 === 5 ? 0.2 : 1)
    return { day: d.toISOString().slice(0, 10), tokens: v, cost_usd: v / 4e6 }
  })
  return {
    range, tokens: t(2_310_000, 3_604_000, 66_280_000, 4_116_000), cost_usd: 41.86, cost_partial: false,
    providers: [
      { provider: 'claude', label: 'Claude Code', tokens: t(900_000, 2_100_000, 52_000_000, 3_400_000), cost_usd: 28.37, cost_partial: false,
        models: [{ model: 'claude-opus-5-5', tokens: t(860_000, 2_030_000, 50_400_000, 3_310_000), cost_usd: 27.91, events: 812 },
                 { model: 'claude-haiku-5-5', tokens: t(40_000, 70_000, 1_600_000, 90_000), cost_usd: 0.46, events: 57 }] },
      { provider: 'codex', label: 'Codex', tokens: t(700_000, 1_100_000, 9_800_000, 0), cost_usd: 9.12, cost_partial: false,
        models: [{ model: 'gpt-6.1-terra', tokens: t(700_000, 1_100_000, 9_800_000, 0), cost_usd: 9.12, events: 164 }] },
      { provider: 'cursor', label: 'Cursor', tokens: t(560_000, 330_000, 3_900_000, 640_000), cost_usd: 3.41, cost_partial: false,
        models: [{ model: 'composer-2.5', tokens: t(560_000, 330_000, 3_900_000, 640_000), cost_usd: 3.41, events: 48 }] },
      { provider: 'grok', label: 'Grok', tokens: t(150_000, 74_000, 580_000, 76_000), cost_usd: 0.96, cost_partial: false,
        models: [{ model: 'grok-4.7', tokens: t(150_000, 74_000, 580_000, 76_000), cost_usd: 0.96, events: 21 }] },
    ],
    daily, prices_updated: now() - 3600, generated_at: now(),
  }
}

const limits = () => [
  { provider: 'claude', plan: 'max', notes: [], observed_at: now() - 40, stale: false, error: null,
    windows: [{ key: '5h', label: '5h', used_pct: 72, resets_at: now() + 4320, detail: null },
              { key: 'week', label: 'week', used_pct: 31, resets_at: now() + 4 * 86400, detail: null },
              { key: 'week_opus', label: 'week · Opus', used_pct: 93, resets_at: now() + 4 * 86400, detail: null }] },
  { provider: 'codex', plan: 'team', notes: ['2 full resets available'], observed_at: now() - 40, stale: false, error: null,
    windows: [{ key: '5h', label: '5h', used_pct: 9, resets_at: now() + 13200, detail: null },
              { key: 'week', label: 'week', used_pct: 52, resets_at: now() + 2 * 86400, detail: null }] },
  { provider: 'cursor', plan: 'pro', notes: [], observed_at: now() - 40, stale: false, error: null,
    windows: [{ key: 'plan', label: 'month', used_pct: 38, resets_at: now() + 17 * 86400, detail: '$7.60 / $20' }] },
]

let settings = {
  title_mode: 'tokens', limits: {}, poll_minutes: 5, hot_threshold: 85, price_refresh: true,
}

export async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const r: Record<string, () => unknown> = {
    get_summary: () => summary(String(args?.range ?? 'today')),
    get_limits: limits,
    refresh: () => undefined,
    get_settings: () => ({
      settings, launch_at_login: false, version: '0.1.0',
      providers: ['claude', 'codex', 'cursor', 'grok', 'copilot'].map((id, i) => ({
        id, label: { claude: 'Claude Code', codex: 'Codex', cursor: 'Cursor', grok: 'Grok', copilot: 'Copilot' }[id],
        detected: i < 3, limits_supported: true, limits_enabled: i < 3,
      })),
    }),
    set_settings: () => { settings = args!.settings as typeof settings },
  }
  return (r[cmd]?.() ?? undefined) as T
}
